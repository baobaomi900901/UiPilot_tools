//! Internal Keychain boundary; no host IPC or plugin capability is enabled here.
#![allow(dead_code, unused_imports)]
use std::{collections::BTreeMap, fmt, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}, Weak}};
use sha2::{Digest, Sha256};

mod native;
pub(crate) use native::NativeKeychain;

const MAGIC: &[u8] = b"UIPILOT-KEYCHAIN-V1\0";
const ENVELOPE_OVERHEAD: usize = MAGIC.len() + 32;

pub(crate) struct SecretBytes(Vec<u8>);
impl SecretBytes {
    pub(crate) fn new(value: Vec<u8>) -> Self { Self(value) }
    pub(crate) fn as_bytes(&self) -> &[u8] { &self.0 }
}
impl fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("SecretBytes([REDACTED])") }
}
impl Drop for SecretBytes {
    fn drop(&mut self) {
        for byte in &mut self.0 { unsafe { std::ptr::write_volatile(byte, 0) }; }
        std::sync::atomic::compiler_fence(Ordering::SeqCst);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Error { PermissionDenied, Stopped, InvalidKey, TooLarge, Corrupt, InteractionRequired, Unavailable, MutationUnconfirmed, Conflict }
pub(crate) type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum HostService { YoudaoTranslation, BaiduTranslation, BaiduOcr, Deepseek }
impl HostService {
    fn name(self) -> &'static str { match self {
        Self::YoudaoTranslation => "youdaoTranslation", Self::BaiduTranslation => "baiduTranslation",
        Self::BaiduOcr => "baiduOcr", Self::Deepseek => "deepseek",
    } }
}
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum Owner { Host(HostService), Plugin(String) }
impl Owner {
    fn valid(&self) -> bool { match self { Self::Host(_) => true, Self::Plugin(id) => crate::manifest::valid_plugin_id(id) } }
    fn limit(&self) -> usize { match self { Self::Host(_) => 128 * 1024, Self::Plugin(_) => 64 * 1024 } }
    fn address(&self, key: &str) -> Result<Address> {
        let (service, owner) = match self {
            Self::Host(service) if key == "record" => ("com.uipilot.launcher.host-credentials.v1", service.name()),
            Self::Host(_) => return Err(Error::InvalidKey),
            Self::Plugin(id) if crate::manifest::valid_setting_key(key) => ("com.uipilot.launcher.public-plugin-secrets.v1", id.as_str()),
            Self::Plugin(_) => return Err(Error::InvalidKey),
        };
        Ok(Address { service, account: format!("{}:{owner}:{}:{key}", owner.len(), key.len()) })
    }
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct Address { service: &'static str, account: String }
impl Address {
    fn binding(&self) -> [u8; 32] {
        let mut digest = Sha256::new();
        for value in [self.service.as_bytes(), self.account.as_bytes()] {
            digest.update((value.len() as u64).to_be_bytes()); digest.update(value);
        }
        digest.finalize().into()
    }
}

/// Implementations use one exact address; never enumerate or search by prefix.
pub(crate) trait Backend: Send + Sync {
    fn get(&self, address: &Address, max_bytes: usize) -> Result<Option<SecretBytes>>;
    fn put(&self, address: &Address, value: &SecretBytes) -> Result<()>;
    fn delete(&self, address: &Address) -> Result<bool>;
}
struct Epoch { active: AtomicBool, gate: Mutex<()> }
struct State { stopped: bool, owners: BTreeMap<Owner, Arc<Epoch>> }
struct Authority { lifecycle: Mutex<()>, state: Mutex<State> }
#[derive(Clone)]
pub(crate) struct Token { issuer: Weak<Authority>, owner: Owner, epoch: Arc<Epoch> }
pub(crate) struct Vault<B: Backend> { authority: Arc<Authority>, backend: B }

impl<B: Backend> Vault<B> {
    pub(crate) fn new(backend: B) -> Self {
        Self { backend, authority: Arc::new(Authority { lifecycle: Mutex::new(()),
            state: Mutex::new(State { stopped: false, owners: BTreeMap::new() }) }) }
    }
    /// Trusted host authorization only; do not expose this operation over plugin RPC.
    pub(crate) fn authorize(&self, owner: Owner) -> Result<Token> {
        if !owner.valid() { return Err(Error::PermissionDenied); }
        let _lifecycle = self.authority.lifecycle.lock().map_err(|_| Error::Unavailable)?;
        let mut state = self.authority.state.lock().map_err(|_| Error::Unavailable)?;
        if state.stopped { return Err(Error::Stopped); }
        let epoch = state.owners.entry(owner.clone()).or_insert_with(|| Arc::new(Epoch {
            active: AtomicBool::new(true), gate: Mutex::new(()),
        }));
        Ok(Token { issuer: Arc::downgrade(&self.authority), owner, epoch: Arc::clone(epoch) })
    }
    fn admitted<T>(&self, token: &Token, key: &str, action: impl FnOnce(Address, usize) -> Result<T>) -> Result<T> {
        if !Weak::ptr_eq(&token.issuer, &Arc::downgrade(&self.authority)) || !token.epoch.active.load(Ordering::Acquire) {
            return Err(Error::PermissionDenied);
        }
        let _gate = token.epoch.gate.lock().map_err(|_| Error::Unavailable)?;
        {
            let state = self.authority.state.lock().map_err(|_| Error::Unavailable)?;
            if state.stopped { return Err(Error::Stopped); }
            if !token.epoch.active.load(Ordering::Acquire) || !state.owners.get(&token.owner).is_some_and(|epoch| Arc::ptr_eq(epoch, &token.epoch)) {
                return Err(Error::PermissionDenied);
            }
        }
        action(token.owner.address(key)?, token.owner.limit())
    }
    pub(crate) fn read(&self, token: &Token, key: &str) -> Result<Option<SecretBytes>> {
        self.admitted(token, key, |address, limit| {
            let Some(record) = self.backend.get(&address, limit + ENVELOPE_OVERHEAD)? else { return Ok(None); };
            if record.0.len() > limit + ENVELOPE_OVERHEAD { return Err(Error::TooLarge); }
            if record.0.len() < ENVELOPE_OVERHEAD || !record.0.starts_with(MAGIC)
                || record.0[MAGIC.len()..ENVELOPE_OVERHEAD] != address.binding() { return Err(Error::Corrupt); }
            Ok(Some(SecretBytes::new(record.0[ENVELOPE_OVERHEAD..].to_vec())))
        })
    }
    pub(crate) fn write(&self, token: &Token, key: &str, value: &SecretBytes) -> Result<()> {
        self.admitted(token, key, |address, limit| {
            if value.0.len() > limit { return Err(Error::TooLarge); }
            let mut record = SecretBytes::new(Vec::with_capacity(ENVELOPE_OVERHEAD + value.0.len()));
            record.0.extend_from_slice(MAGIC); record.0.extend_from_slice(&address.binding()); record.0.extend_from_slice(&value.0);
            self.backend.put(&address, &record)
        })
    }
    pub(crate) fn delete(&self, token: &Token, key: &str) -> Result<bool> {
        self.admitted(token, key, |address, _| self.backend.delete(&address))
    }
    pub(crate) fn revoke(&self, owner: &Owner) -> Result<()> {
        let _lifecycle = self.authority.lifecycle.lock().map_err(|_| Error::Unavailable)?;
        let retired = {
            let mut state = self.authority.state.lock().map_err(|_| Error::Unavailable)?;
            if state.stopped { return Err(Error::Stopped); }
            let retired = state.owners.remove(owner);
            if let Some(epoch) = &retired { epoch.active.store(false, Ordering::Release); }
            retired
        };
        if let Some(epoch) = retired { drop(epoch.gate.lock().unwrap_or_else(|e| e.into_inner())); }
        Ok(())
    }
    pub(crate) fn shutdown(&self) {
        let _lifecycle = self.authority.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        let retired = {
            let mut state = self.authority.state.lock().unwrap_or_else(|e| e.into_inner());
            state.stopped = true;
            let retired = std::mem::take(&mut state.owners);
            for epoch in retired.values() { epoch.active.store(false, Ordering::Release); }
            retired
        };
        for epoch in retired.values() { drop(epoch.gate.lock().unwrap_or_else(|e| e.into_inner())); }
    }
}
impl<B: Backend> Drop for Vault<B> { fn drop(&mut self) { self.shutdown(); } }

#[cfg(test)]
mod tests;
