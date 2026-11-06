//! Only opaque OS bookmark bytes carry authority between launches. The recorded identity is
//! additional negative evidence for replacement, never an alternate way to obtain access.

use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use core_foundation::{
    base::{CFAllocatorRef, CFType, CFTypeRef, TCFType},
    boolean::CFBoolean,
    error::CFErrorRef,
    string::{CFString, CFStringRef},
};
use objc2::runtime::Bool;
use objc2_foundation::{
    NSData, NSString, NSURLBookmarkCreationOptions, NSURLBookmarkResolutionOptions, NSURLVolumeUUIDStringKey,
};
use rustix::fs::Stat;
use serde::{Deserialize, Serialize};
use std::ptr;

const MAX_RECORD_BYTES: usize = 256 * 1024;
const MAX_BOOKMARK_BYTES: usize = 160 * 1024;
const RECORD_VERSION: u8 = 1;

#[link(name = "Security", kind = "framework")]
extern "C" {
    fn SecTaskCreateFromSelf(allocator: CFAllocatorRef) -> CFTypeRef;
    fn SecTaskCopyValueForEntitlement(task: CFTypeRef, entitlement: CFStringRef, error: *mut CFErrorRef) -> CFTypeRef;
}

fn bookmark_entitled() -> bool {
    // SAFETY: use the current process's task, the default allocator and no error output;
    // both Create/Copy references are owned and released by the corresponding CFType.
    unsafe {
        let task = SecTaskCreateFromSelf(ptr::null());
        if task.is_null() { return false; }
        let task = CFType::wrap_under_create_rule(task);
        let key = CFString::from_static_string("com.apple.security.files.bookmarks.app-scope");
        let value = SecTaskCopyValueForEntitlement(task.as_CFTypeRef(), key.as_concrete_TypeRef(), ptr::null_mut());
        if value.is_null() { return false; }
        CFType::wrap_under_create_rule(value).downcast_into::<CFBoolean>().is_some_and(bool::from)
    }
}

#[derive(Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeIdentity {
    device: i32,
    inode: u64,
    birth_seconds: i64,
    birth_nanos: i64,
    generation: u32,
}

impl From<Stat> for NodeIdentity {
    fn from(stat: Stat) -> Self {
        Self { device: stat.st_dev, inode: stat.st_ino, birth_seconds: stat.st_birthtime,
            birth_nanos: stat.st_birthtime_nsec, generation: stat.st_gen }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u8,
    volume_uuid: String,
    chain: Vec<NodeIdentity>,
    bookmark: String,
}

impl Record {
    fn decode(bytes: &[u8]) -> Result<(Self, Vec<u8>), FindError> {
        if bytes.is_empty() || bytes.len() > MAX_RECORD_BYTES { return Err(FindError::InvalidInput); }
        // A future version's valid record must survive a run of an older binary.
        #[derive(Deserialize)]
        struct Header { version: u64 }
        let header: Header = serde_json::from_slice(bytes).map_err(|_| FindError::InvalidInput)?;
        if header.version != u64::from(RECORD_VERSION) { return Err(FindError::Unavailable); }
        let record: Self = serde_json::from_slice(bytes).map_err(|_| FindError::InvalidInput)?;
        // Documents grants always contain three nodes: the parent of the account's home,
        // the home, and Documents. No independent scope root is supplied by a stored record.
        if record.chain.len() != 3 || !valid_uuid(&record.volume_uuid) {
            return Err(FindError::InvalidInput);
        }
        if record.chain.iter().any(|node| node.inode == 0 || node.device != record.chain[0].device
            || !(0..1_000_000_000).contains(&node.birth_nanos)) {
            return Err(FindError::InvalidInput);
        }
        let data = STANDARD.decode(&record.bookmark).map_err(|_| FindError::InvalidInput)?;
        if data.is_empty() || data.len() > MAX_BOOKMARK_BYTES { return Err(FindError::InvalidInput); }
        Ok((record, data))
    }
}

fn valid_uuid(value: &str) -> bool {
    value.len() == 36 && value.bytes().enumerate().all(|(i, byte)| {
        if matches!(i, 8 | 13 | 18 | 23) { byte == b'-' } else { byte.is_ascii_hexdigit() }
    }) && value.bytes().any(|byte| byte != b'0' && byte != b'-')
}

fn volume_uuid(grant: &DocumentsGrant) -> Result<String, FindError> {
    // Get the UUID through Foundation on both passes. The physical APFS snapshot can have a
    // different UUID from the logical volume reported by the immutable file-reference URL.
    let uuid = resource(&grant.reference, unsafe { NSURLVolumeUUIDStringKey })?
        .downcast::<NSString>().map_err(|_| FindError::Denied)?.to_string();
    if !valid_uuid(&uuid) { return Err(FindError::Denied); }
    Ok(uuid)
}

fn current_chain(grant: &DocumentsGrant) -> Result<Vec<NodeIdentity>, FindError> {
    let mut fd = rustix::io::fcntl_dupfd_cloexec(&grant.anchor_fd, 0).map_err(|_| FindError::Denied)?;
    let mut chain = vec![NodeIdentity::from(fstat(&fd).map_err(|_| FindError::Denied)?)];
    let suffix = grant.root.path.strip_prefix(&grant.anchor_path).map_err(|_| FindError::Denied)?;
    for component in suffix.components() {
        let Component::Normal(name) = component else { return Err(FindError::Denied) };
        fd = openat(&fd, name, OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty()).map_err(|_| FindError::Denied)?;
        chain.push(fstat(&fd).map_err(|_| FindError::Denied)?.into());
    }
    if chain.len() != 3 || !chain.iter().enumerate().all(|(depth, node)|
        node.device == chain[0].device && Some(node.inode) == grant.identity.inode_at_depth(depth)) {
        return Err(FindError::Stale);
    }
    Ok(chain)
}

impl DocumentsGrant {
    /// Return private, bounded record bytes only after an actual no-UI bookmark round trip.
    /// Failure leaves the selected URL and its session grant usable.
    pub(crate) fn persistent_record(&self) -> Result<Vec<u8>, FindError> {
        if !self.selected_in_panel || !cfg!(feature = "macos-find-preview")
            || cfg!(feature = "macos-find-system-qa") || !bookmark_entitled() {
            return Err(FindError::Unavailable);
        }
        self.revalidate()?;
        let chain = current_chain(self)?;
        let selected = {
            let guard = self.lease.lock().unwrap_or_else(|e| e.into_inner());
            if self.is_revoked() { return Err(FindError::Stale); }
            guard.as_ref().ok_or(FindError::Stale)?.url.clone()
        };
        let options = NSURLBookmarkCreationOptions::WithSecurityScope
            | NSURLBookmarkCreationOptions::SecurityScopeAllowOnlyReadAccess;
        let bookmark = selected.bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
            options, None, None).map_err(|_| FindError::Unavailable)?;
        if bookmark.is_empty() || bookmark.len() > MAX_BOOKMARK_BYTES { return Err(FindError::Denied); }
        let bytes = serde_json::to_vec(&Record { version: RECORD_VERSION, volume_uuid: volume_uuid(self)?,
            chain, bookmark: STANDARD.encode(bookmark.to_vec()) }).map_err(|_| FindError::Denied)?;
        Self::restore_expected(&bytes, &self.root.path, self.require_system_location)?;
        self.revalidate()?;
        Ok(bytes)
    }

    pub(crate) fn from_persistent_record(bytes: &[u8]) -> Result<Arc<Self>, FindError> {
        if !cfg!(feature = "macos-find-preview") || cfg!(feature = "macos-find-system-qa") {
            return Err(FindError::Unavailable);
        }
        // Never look up the user's Documents location for an empty or malformed record.
        let (record, data) = Record::decode(bytes)?;
        if !bookmark_entitled() { return Err(FindError::Unavailable); }
        let expected = documents_directory_url()?.to_file_path().ok_or(FindError::Denied)?;
        Self::restore_decoded(record, &data, &expected, true)
    }

    fn restore_expected(bytes: &[u8], expected: &Path, system: bool) -> Result<Arc<Self>, FindError> {
        let (record, data) = Record::decode(bytes)?;
        if !bookmark_entitled() { return Err(FindError::Unavailable); }
        Self::restore_decoded(record, &data, expected, system)
    }

    fn restore_decoded(record: Record, data: &[u8], expected: &Path, system: bool) -> Result<Arc<Self>, FindError> {
        let options = NSURLBookmarkResolutionOptions::WithSecurityScope
            | NSURLBookmarkResolutionOptions::WithoutUI | NSURLBookmarkResolutionOptions::WithoutMounting;
        let mut stale = Bool::YES;
        // SAFETY: stale is a writable BOOL valid until Foundation finishes this synchronous call.
        let resolved = unsafe { NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
            &NSData::with_bytes(data), options, None, &mut stale,
        ) }.map_err(|_| FindError::Denied)?;
        if stale.as_bool() { return Err(FindError::Stale); }
        // bind_url checks the exact system directory *before* starting security-scoped access
        // and requires a successful start, even if POSIX permissions alone would allow reading.
        if !resolved.isFileURL() { return Err(FindError::Denied); }
        let resolved_path = resolved.to_file_path().ok_or(FindError::Denied)?;
        if resolved_path.as_os_str() != expected.as_os_str() { return Err(FindError::Stale); }
        // Distinguish a positively observed changed identity from an inability to verify
        // it now. A denied descriptor, unavailable volume or failed scoped lease must
        // not turn a recoverable record into a durable invalidation marker.
        let grant = Self::bind_url(expected, &resolved, system, true).map_err(|_| FindError::Denied)?;
        if current_chain(&grant).map_err(|_| FindError::Denied)? != record.chain
            || !volume_uuid(&grant)?.eq_ignore_ascii_case(&record.volume_uuid) {
            return Err(FindError::Stale);
        }
        grant.revalidate().map_err(|_| FindError::Denied)?;
        Ok(grant)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn rejects_paths_preferences_and_invalid_record_structure_without_resolution() {
        for bytes in [&b"/arbitrary/Documents"[..], b"true", b"{}", b"{\"version\":1,\"path\":\"/tmp\"}"] {
            assert_eq!(Record::decode(bytes).err(), Some(FindError::InvalidInput));
        }
        assert_eq!(Record::decode(&vec![0; MAX_RECORD_BYTES + 1]).err(), Some(FindError::InvalidInput));
        assert_eq!(Record::decode(br#"{"version":2,"differentFormat":"supported by a newer app"}"#).err(),
            Some(FindError::Unavailable));
    }

    #[test]
    fn chain_tracks_ancestors_and_retains_identity_across_normal_edits() {
        let root = std::env::temp_dir().canonicalize().unwrap()
            .join(format!("uipilot-bookmark-chain-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let docs = root.join("home/Documents");
        fs::create_dir_all(&docs).unwrap();
        let bind = || DocumentsGrant::from_fixture(&docs, &NSURL::from_directory_path(&docs).unwrap()).unwrap();
        let old = current_chain(&bind()).unwrap();
        fs::write(docs.join("note.txt"), b"one").unwrap();
        assert!(current_chain(&bind()).unwrap() == old);
        // Keep the leaf inode unchanged but exchange its parent.
        fs::rename(root.join("home"), root.join("old-home")).unwrap();
        fs::create_dir(root.join("home")).unwrap();
        fs::rename(root.join("old-home/Documents"), &docs).unwrap();
        let changed = current_chain(&bind()).unwrap();
        assert!(changed != old);
        assert_eq!(changed[2].inode, old[2].inode);
        fs::remove_dir_all(root).unwrap();
    }

}

#[cfg(test)]
impl DocumentsGrant {
    pub(crate) fn create_fixture_bookmark(docs: &Path) -> Vec<u8> {
        let selected = NSURL::from_directory_path(docs).unwrap();
        let grant = Self::from_fixture(docs, &selected).unwrap();
        assert!(bookmark_entitled(), "executable lacks the app-scoped bookmark entitlement");
        let record = grant.persistent_record().expect("read-only app-scoped bookmark and round trip");
        for options in [NSURLBookmarkCreationOptions::empty(),
            NSURLBookmarkCreationOptions::WithoutImplicitSecurityScope] {
            let plain = selected.bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
                options, None, None).unwrap();
            let (evidence, _) = Record::decode(&record).unwrap();
            assert!(Self::restore_decoded(evidence, &plain.to_vec(), docs, false).is_err(),
                "ordinary bookmarks must not provide app-scoped authority");
        }
        grant.revoke();
        record
    }

    pub(crate) fn verify_fixture_bookmark(bytes: &[u8], docs: &Path) -> Result<(), FindError> {
        let grant = Self::restore_expected(bytes, docs, false)?;
        assert!(grant.is_active());
        let scope = IndexedLocalScope::bind_with_documents(&[], Some(grant.clone()))?;
        let row = authorize_row(&scope, docs.join("图片 🐋/画稿📷.png"), FileCategory::Image)?
            .ok_or(FindError::Denied)?;
        assert_eq!(row.name, "画稿📷.png");
        assert!(grant.persistent_record().is_err());
        grant.revoke();
        Ok(())
    }
}
