//! Data Protection Keychain only. Never fall back to the classic implementation.
use super::*;
use core_foundation::{base::{CFType, TCFType}, boolean::CFBoolean, data::CFData,
    dictionary::CFDictionary, string::{CFString, CFStringRef}};
use security_framework_sys::{item::*, access_control::kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
    keychain_item::{SecItemAdd, SecItemCopyMatching, SecItemDelete, SecItemUpdate}};

extern "C" {
    static kSecUseAuthenticationUIFail: CFStringRef;
    static kSecAttrAccessible: CFStringRef;
}
type Dictionary = CFDictionary<CFString, CFType>;
pub(crate) trait NativeCalls: Send + Sync {
    fn copy(&self, query: &Dictionary) -> (i32, Option<CFType>);
    fn update(&self, query: &Dictionary, values: &Dictionary) -> i32;
    fn add(&self, query: &Dictionary) -> i32;
    fn delete(&self, query: &Dictionary) -> i32;
}
#[derive(Default)]
pub(crate) struct SecurityCalls;
impl NativeCalls for SecurityCalls {
    fn copy(&self, query: &Dictionary) -> (i32, Option<CFType>) {
        let mut raw = std::ptr::null();
        let status = unsafe { SecItemCopyMatching(query.as_concrete_TypeRef(), &mut raw) };
        let result = (!raw.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(raw) });
        (status, result)
    }
    fn update(&self, query: &Dictionary, values: &Dictionary) -> i32 {
        unsafe { SecItemUpdate(query.as_concrete_TypeRef(), values.as_concrete_TypeRef()) }
    }
    fn add(&self, query: &Dictionary) -> i32 {
        unsafe { SecItemAdd(query.as_concrete_TypeRef(), std::ptr::null_mut()) }
    }
    fn delete(&self, query: &Dictionary) -> i32 {
        unsafe { SecItemDelete(query.as_concrete_TypeRef()) }
    }
}
#[derive(Default)]
pub(crate) struct NativeKeychain<C: NativeCalls = SecurityCalls> { calls: C }
fn constant(value: CFStringRef) -> CFString { unsafe { CFString::wrap_under_get_rule(value) } }
fn query(address: &Address) -> Vec<(CFString, CFType)> {
    unsafe { vec![
        (constant(kSecClass), constant(kSecClassGenericPassword).into_CFType()),
        (constant(kSecAttrService), CFString::new(address.service).into_CFType()),
        (constant(kSecAttrAccount), CFString::new(&address.account).into_CFType()),
        (constant(kSecUseDataProtectionKeychain), CFBoolean::true_value().into_CFType()),
        (constant(kSecAttrSynchronizable), CFBoolean::false_value().into_CFType()),
        (constant(kSecAttrAccessible), constant(kSecAttrAccessibleWhenUnlockedThisDeviceOnly).into_CFType()),
        (constant(kSecUseAuthenticationUI), constant(kSecUseAuthenticationUIFail).into_CFType()),
    ] }
}
fn failure(status: i32, mutation: bool) -> Error {
    match status {
        -25308 | -25293 | -128 => Error::InteractionRequired,
        -25299 => Error::Conflict,
        // Missing signing entitlement: never retry against the classic Keychain.
        -34018 => Error::Unavailable,
        _ if mutation => Error::MutationUnconfirmed,
        _ => Error::Unavailable,
    }
}
impl<C: NativeCalls> Backend for NativeKeychain<C> {
    fn get(&self, address: &Address, max_bytes: usize) -> Result<Option<SecretBytes>> {
        let mut values = query(address);
        values.push((unsafe { constant(kSecReturnData) }, CFBoolean::true_value().into_CFType()));
        let dictionary = CFDictionary::from_CFType_pairs(&values);
        let (status, owned) = self.calls.copy(&dictionary);
        if status == -25300 { return Ok(None); }
        if status != 0 { return Err(failure(status, false)); }
        let result = owned.ok_or(Error::Corrupt)?;
        let data = result.downcast::<CFData>().ok_or(Error::Corrupt)?;
        // Security.framework may already have allocated CFData; limit the Rust copy.
        if data.len() < 0 || data.len() as usize > max_bytes { return Err(Error::TooLarge); }
        Ok(Some(SecretBytes::new(data.bytes().to_vec())))
    }
    fn put(&self, address: &Address, value: &SecretBytes) -> Result<()> {
        let data = CFData::from_buffer(value.as_bytes());
        let value_pair = (unsafe { constant(kSecValueData) }, data.into_CFType());
        let search = CFDictionary::from_CFType_pairs(&query(address));
        let update = CFDictionary::from_CFType_pairs(&[value_pair.clone()]);
        let status = self.calls.update(&search, &update);
        if status == 0 { return Ok(()); }
        if status != -25300 { return Err(failure(status, true)); }
        let mut values = query(address); values.push(value_pair);
        let add = CFDictionary::from_CFType_pairs(&values);
        let status = self.calls.add(&add);
        // A competing creation is Conflict. Never delete or retry over that item.
        if status == 0 { Ok(()) } else { Err(failure(status, true)) }
    }
    fn delete(&self, address: &Address) -> Result<bool> {
        let search = CFDictionary::from_CFType_pairs(&query(address));
        match self.calls.delete(&search) {
            0 => Ok(true), -25300 => Ok(false), status => Err(failure(status, true)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    struct Calls { statuses: Mutex<VecDeque<i32>>, seen: Mutex<Vec<&'static str>>, value: Vec<u8> }
    fn verify_query(query: &Dictionary, value: bool, read: bool) {
        unsafe {
            assert_eq!(query.get(constant(kSecUseDataProtectionKeychain)).downcast::<CFBoolean>().unwrap(), CFBoolean::true_value());
            assert_eq!(query.get(constant(kSecAttrSynchronizable)).downcast::<CFBoolean>().unwrap(), CFBoolean::false_value());
            assert_eq!(query.get(constant(kSecUseAuthenticationUI)).downcast::<CFString>().unwrap(), constant(kSecUseAuthenticationUIFail));
            assert_eq!(query.get(constant(kSecAttrAccessible)).downcast::<CFString>().unwrap(), constant(kSecAttrAccessibleWhenUnlockedThisDeviceOnly));
            assert_eq!(query.get(constant(kSecClass)).downcast::<CFString>().unwrap(), constant(kSecClassGenericPassword));
            assert_eq!(query.get(constant(kSecAttrService)).downcast::<CFString>().unwrap().to_string(), "com.uipilot.launcher.public-plugin-secrets.v1");
            assert_eq!(query.get(constant(kSecAttrAccount)).downcast::<CFString>().unwrap().to_string(), "15:com.example.one:5:token");
            assert!(query.find(constant(kSecMatchSearchList)).is_none());
            assert!(query.find(constant(kSecUseKeychain)).is_none());
            assert!(query.find(constant(kSecAttrAccessGroup)).is_none());
            assert_eq!(query.find(constant(kSecValueData)).is_some(), value);
            assert_eq!(query.find(constant(kSecReturnData)).is_some(), read);
            assert_eq!(query.len(), 7 + usize::from(value || read));
        }
    }
    impl Calls {
        fn status(&self, name: &'static str) -> i32 { self.seen.lock().unwrap().push(name); self.statuses.lock().unwrap().pop_front().unwrap() }
    }
    impl NativeCalls for Calls {
        fn copy(&self, query: &Dictionary) -> (i32, Option<CFType>) {
            verify_query(query, false, true);
            (self.status("get"), Some(CFData::from_buffer(&self.value).into_CFType()))
        }
        fn update(&self, query: &Dictionary, values: &Dictionary) -> i32 {
            verify_query(query, false, false); assert_eq!(values.len(), 1);
            assert_eq!(values.get(unsafe { constant(kSecValueData) }).downcast::<CFData>().unwrap().bytes(), b"synthetic");
            self.status("update")
        }
        fn add(&self, query: &Dictionary) -> i32 { verify_query(query, true, false); self.status("add") }
        fn delete(&self, query: &Dictionary) -> i32 { verify_query(query, false, false); self.status("delete") }
    }
    fn backend(statuses: &[i32]) -> NativeKeychain<Calls> {
        NativeKeychain { calls: Calls { statuses: Mutex::new(statuses.iter().copied().collect()), seen: Mutex::new(vec![]), value: b"synthetic".to_vec() } }
    }
    fn address() -> Address { Owner::Plugin("com.example.one".into()).address("token").unwrap() }
    #[test]
    fn all_native_query_shapes_are_exact_noninteractive_device_only_and_modern() {
        let backend = backend(&[0, 0, -25300, 0, 0, -25300]);
        assert_eq!(backend.get(&address(), 64).unwrap().unwrap().as_bytes(), b"synthetic");
        backend.put(&address(), &SecretBytes::new(b"synthetic".to_vec())).unwrap();
        backend.put(&address(), &SecretBytes::new(b"synthetic".to_vec())).unwrap();
        assert!(backend.delete(&address()).unwrap()); assert!(!backend.delete(&address()).unwrap());
        assert_eq!(*backend.calls.seen.lock().unwrap(), ["get", "update", "update", "add", "delete", "delete"]);
    }
    #[test]
    fn update_failures_never_add_delete_retry_or_fall_back() {
        for status in [-25308, -25293, -34018, -36] {
            let backend = backend(&[status]);
            assert_eq!(backend.put(&address(), &SecretBytes::new(b"synthetic".to_vec())), Err(failure(status, true)));
            assert_eq!(*backend.calls.seen.lock().unwrap(), ["update"]);
        }
        let backend = backend(&[-25300, -25299]);
        assert_eq!(backend.put(&address(), &SecretBytes::new(b"synthetic".to_vec())), Err(Error::Conflict));
        assert_eq!(*backend.calls.seen.lock().unwrap(), ["update", "add"]);
    }
    #[test]
    fn native_read_and_delete_failures_are_bounded_and_fixed() {
        let backend = backend(&[0, -25300, -25308, -36]);
        assert!(matches!(backend.get(&address(), 2), Err(Error::TooLarge)));
        assert!(backend.get(&address(), 64).unwrap().is_none());
        assert!(matches!(backend.get(&address(), 64), Err(Error::InteractionRequired)));
        assert_eq!(backend.delete(&address()), Err(Error::MutationUnconfirmed));
    }
}
