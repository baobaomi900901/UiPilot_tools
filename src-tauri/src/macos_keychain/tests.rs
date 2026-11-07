use super::*;
use std::{sync::{atomic::AtomicUsize, mpsc}, thread, time::{Duration, Instant}};
const WAIT: Duration = Duration::from_secs(5);
const A: &str = "com.example.one";
const B: &str = "com.example.two";
#[derive(Default)]
struct Fake {
    records: Mutex<BTreeMap<Address, Vec<u8>>>, calls: AtomicUsize,
    failure: Mutex<Option<Error>>, before_write: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl Backend for Arc<Fake> {
    fn get(&self, address: &Address, _: usize) -> Result<Option<SecretBytes>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = *self.failure.lock().unwrap() { return Err(error); }
        Ok(self.records.lock().unwrap().get(address).cloned().map(SecretBytes::new))
    }
    fn put(&self, address: &Address, value: &SecretBytes) -> Result<()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(hook) = self.before_write.lock().unwrap().take() { hook(); }
        if let Some(error) = *self.failure.lock().unwrap() { return Err(error); }
        self.records.lock().unwrap().insert(address.clone(), value.as_bytes().to_vec()); Ok(())
    }
    fn delete(&self, address: &Address) -> Result<bool> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = *self.failure.lock().unwrap() { return Err(error); }
        Ok(self.records.lock().unwrap().remove(address).is_some())
    }
}
fn fixture() -> (Arc<Fake>, Arc<Vault<Arc<Fake>>>) {
    let fake = Arc::new(Fake::default()); let vault = Arc::new(Vault::new(Arc::clone(&fake))); (fake, vault)
}
fn secret(value: &str) -> SecretBytes { SecretBytes::new(value.as_bytes().to_vec()) }

#[test]
fn host_and_plugins_use_distinct_namespaces_and_exact_addresses() {
    let (_, vault) = fixture();
    let host = vault.authorize(Owner::Host(HostService::Deepseek)).unwrap();
    let a = vault.authorize(Owner::Plugin(A.into())).unwrap(); let b = vault.authorize(Owner::Plugin(B.into())).unwrap();
    for (token, key, value) in [(&host, "record", "host"), (&a, "record", "a"), (&b, "record", "b")] {
        vault.write(token, key, &secret(value)).unwrap();
        assert_eq!(vault.read(token, key).unwrap().unwrap().as_bytes(), value.as_bytes());
    }
    assert!(vault.delete(&a, "record").unwrap()); assert!(!vault.delete(&a, "record").unwrap());
    assert_eq!(vault.read(&host, "record").unwrap().unwrap().as_bytes(), b"host");
    assert_eq!(vault.read(&b, "record").unwrap().unwrap().as_bytes(), b"b");
}
#[test]
fn invalid_foreign_and_retired_authority_never_calls_backend() {
    let (fake, vault) = fixture(); let (_, other) = fixture();
    assert!(matches!(vault.authorize(Owner::Plugin("../invalid".into())), Err(Error::PermissionDenied)));
    let a = vault.authorize(Owner::Plugin(A.into())).unwrap();
    assert!(matches!(other.read(&a, "token"), Err(Error::PermissionDenied)));
    assert!(matches!(vault.read(&a, "../key"), Err(Error::InvalidKey)));
    vault.revoke(&Owner::Plugin(A.into())).unwrap();
    let fresh = vault.authorize(Owner::Plugin(A.into())).unwrap();
    assert!(matches!(vault.read(&a, "token"), Err(Error::PermissionDenied)));
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    assert!(vault.read(&fresh, "token").unwrap().is_none());
}
#[test]
fn bound_record_cannot_be_replayed_under_another_owner_or_key() {
    let (fake, vault) = fixture(); let a = vault.authorize(Owner::Plugin(A.into())).unwrap(); let b = vault.authorize(Owner::Plugin(B.into())).unwrap();
    vault.write(&a, "token", &secret("private fixture")).unwrap();
    let bytes = fake.records.lock().unwrap().get(&a.owner.address("token").unwrap()).unwrap().clone();
    for (token, key) in [(&b, "token"), (&a, "other")] {
        fake.records.lock().unwrap().insert(token.owner.address(key).unwrap(), bytes.clone());
        assert!(matches!(vault.read(token, key), Err(Error::Corrupt)));
    }
}
#[test]
fn exact_limits_empty_unicode_and_redacted_debug() {
    let (fake, vault) = fixture(); let a = vault.authorize(Owner::Plugin(A.into())).unwrap();
    for value in [Vec::new(), "合成🐱\0".as_bytes().to_vec(), vec![7; 64 * 1024]] {
        vault.write(&a, "token", &SecretBytes::new(value.clone())).unwrap();
        assert_eq!(vault.read(&a, "token").unwrap().unwrap().as_bytes(), value);
    }
    let before = fake.calls.load(Ordering::SeqCst);
    assert_eq!(vault.write(&a, "token", &SecretBytes::new(vec![1; 64 * 1024 + 1])), Err(Error::TooLarge));
    assert_eq!(fake.calls.load(Ordering::SeqCst), before);
    assert_eq!(format!("{:?}", secret("do not expose")), "SecretBytes([REDACTED])");
    fake.records.lock().unwrap().insert(a.owner.address("token").unwrap(), vec![0; 64 * 1024 + ENVELOPE_OVERHEAD + 1]);
    assert!(matches!(vault.read(&a, "token"), Err(Error::TooLarge)));
}
#[test]
fn failure_never_deletes_old_value_or_retries_and_reopen_has_no_authority() {
    let (fake, vault) = fixture(); let token = vault.authorize(Owner::Plugin(A.into())).unwrap();
    vault.write(&token, "token", &secret("old")).unwrap();
    *fake.failure.lock().unwrap() = Some(Error::InteractionRequired);
    let before = fake.calls.load(Ordering::SeqCst);
    assert_eq!(vault.write(&token, "token", &secret("new")), Err(Error::InteractionRequired));
    assert_eq!(fake.calls.load(Ordering::SeqCst), before + 1);
    *fake.failure.lock().unwrap() = None;
    assert_eq!(vault.read(&token, "token").unwrap().unwrap().as_bytes(), b"old");
    vault.shutdown(); let reopened = Vault::new(Arc::clone(&fake));
    assert!(matches!(reopened.read(&token, "token"), Err(Error::PermissionDenied)));
    let fresh = reopened.authorize(Owner::Plugin(A.into())).unwrap();
    assert_eq!(reopened.read(&fresh, "token").unwrap().unwrap().as_bytes(), b"old");
}
#[test]
fn revoke_and_shutdown_close_admission_before_draining_native_operation() {
    for shutdown in [false, true] {
        let (fake, vault) = fixture(); let token = vault.authorize(Owner::Plugin(A.into())).unwrap();
        let (entered_tx, entered_rx) = mpsc::channel(); let (release_tx, release_rx) = mpsc::channel();
        *fake.before_write.lock().unwrap() = Some(Box::new(move || { entered_tx.send(()).unwrap(); release_rx.recv_timeout(WAIT).unwrap(); }));
        let writer_vault = Arc::clone(&vault); let writer_token = token.clone();
        let writer = thread::spawn(move || writer_vault.write(&writer_token, "token", &secret("in flight")));
        entered_rx.recv_timeout(WAIT).unwrap();
        let retiring_vault = Arc::clone(&vault); let (done_tx, done_rx) = mpsc::channel();
        let retiring = thread::spawn(move || {
            if shutdown { retiring_vault.shutdown(); } else { retiring_vault.revoke(&Owner::Plugin(A.into())).unwrap(); }
            done_tx.send(()).unwrap();
        });
        let deadline = Instant::now() + WAIT;
        while token.epoch.active.load(Ordering::Acquire) { assert!(Instant::now() < deadline); thread::yield_now(); }
        assert!(done_rx.try_recv().is_err());
        assert!(matches!(vault.read(&token, "token"), Err(Error::PermissionDenied)));
        release_tx.send(()).unwrap(); writer.join().unwrap().unwrap(); done_rx.recv_timeout(WAIT).unwrap(); retiring.join().unwrap();
        assert!(matches!(vault.read(&token, "token"), Err(Error::PermissionDenied)));
        if shutdown { assert!(matches!(vault.authorize(Owner::Plugin(A.into())), Err(Error::Stopped))); }
        else { let fresh = vault.authorize(Owner::Plugin(A.into())).unwrap(); assert_eq!(vault.read(&fresh, "token").unwrap().unwrap().as_bytes(), b"in flight"); }
    }
}
