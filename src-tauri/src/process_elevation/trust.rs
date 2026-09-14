use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PeerIdentity {
    pub(crate) pid: u32,
    pub(crate) creation_time: u64,
    pub(crate) user_sid: String,
    pub(crate) session_id: u32,
    pub(crate) image_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrustPackage {
    root: PathBuf,
    host_digest: String,
    helper_digest: String,
}

impl TrustPackage {
    pub(crate) fn new(
        root: PathBuf,
        host_digest: String,
        helper_digest: String,
    ) -> Result<Self, &'static str> {
        if !root.is_absolute()
            || root
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err("UntrustedInstallation");
        }
        if !valid_digest(&host_digest) || !valid_digest(&helper_digest) {
            return Err("UntrustedInstallation");
        }
        Ok(Self {
            root,
            host_digest,
            helper_digest,
        })
    }
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
    pub(crate) fn host_digest(&self) -> &str {
        &self.host_digest
    }
    pub(crate) fn helper_digest(&self) -> &str {
        &self.helper_digest
    }
    pub(crate) fn helper_path(&self) -> std::path::PathBuf {
        self.root.join("uipilot-process-elevation-helper.exe")
    }
}

pub(crate) fn verify_peer(
    expected: &PeerIdentity,
    actual: &PeerIdentity,
) -> Result<(), &'static str> {
    if expected.pid == 0
        || expected.creation_time == 0
        || actual.pid == 0
        || actual.creation_time == 0
    {
        return Err("PeerRejected");
    }
    if expected != actual {
        return Err("PeerRejected");
    }
    if !valid_digest(&actual.image_digest) || !valid_sid(&actual.user_sid) || actual.session_id == 0
    {
        return Err("PeerRejected");
    }
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_sid(value: &str) -> bool {
    let mut parts = value.split('-');
    matches!(parts.next(), Some("S"))
        && matches!(parts.next(), Some("1"))
        && parts.count() >= 2
        && value
            .split('-')
            .skip(1)
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn peer() -> PeerIdentity {
        PeerIdentity {
            pid: 7,
            creation_time: 11,
            user_sid: "S-1-5-21-1".into(),
            session_id: 1,
            image_digest: "a".repeat(64),
        }
    }
    #[test]
    fn exact_peer_identity_is_required() {
        let value = peer();
        assert_eq!(verify_peer(&value, &value), Ok(()));
    }
    #[test]
    fn pid_creation_sid_session_and_digest_mismatch_reject() {
        for mutate in 0..4 {
            let mut actual = peer();
            match mutate {
                0 => actual.pid = 8,
                1 => actual.creation_time = 12,
                2 => actual.user_sid = "malformed".into(),
                _ => actual.image_digest = "A".repeat(64),
            };
            assert_eq!(verify_peer(&peer(), &actual), Err("PeerRejected"));
        }
    }
    #[test]
    fn package_root_parent_escape_and_digest_shape_reject() {
        assert_eq!(
            TrustPackage::new(
                PathBuf::from(r"C:\Program Files\..\user"),
                "a".repeat(64),
                "b".repeat(64)
            ),
            Err("UntrustedInstallation")
        );
        assert_eq!(
            TrustPackage::new(
                PathBuf::from(r"C:\Program Files\Probe"),
                "a".into(),
                "b".into()
            ),
            Err("UntrustedInstallation")
        );
    }
}
