use serde::de::{self, MapAccess, Visitor};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::{collections::HashSet, fmt};

const MAX_MANIFEST_BYTES: usize = 64 * 1024;

struct Manifest {
    version: String,
    host_digest: String,
    helper_digest: String,
}

impl<'de> Deserialize<'de> for Manifest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ManifestVisitor;
        impl<'de> Visitor<'de> for ManifestVisitor {
            type Value = Manifest;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a unique strict elevation manifest")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut seen = HashSet::new();
                let mut version = None;
                let mut host_digest = None;
                let mut helper_digest = None;
                while let Some(key) = map.next_key::<String>()? {
                    if !seen.insert(key.clone()) {
                        return Err(de::Error::custom("duplicate manifest field"));
                    }
                    match key.as_str() {
                        "version" => version = Some(map.next_value()?),
                        "host_digest" => host_digest = Some(map.next_value()?),
                        "helper_digest" => helper_digest = Some(map.next_value()?),
                        _ => {
                            return Err(de::Error::unknown_field(
                                &key,
                                &["version", "host_digest", "helper_digest"],
                            ))
                        }
                    }
                }
                Ok(Manifest {
                    version: version.ok_or_else(|| de::Error::missing_field("version"))?,
                    host_digest: host_digest
                        .ok_or_else(|| de::Error::missing_field("host_digest"))?,
                    helper_digest: helper_digest
                        .ok_or_else(|| de::Error::missing_field("helper_digest"))?,
                })
            }
        }
        deserializer.deserialize_map(ManifestVisitor)
    }
}

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
    version: String,
    host_digest: String,
    helper_digest: String,
}

impl TrustPackage {
    pub(crate) fn from_manifest(root: PathBuf, bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.is_empty() || bytes.len() > MAX_MANIFEST_BYTES {
            return Err("UntrustedInstallation");
        }
        let manifest: Manifest =
            serde_json::from_slice(bytes).map_err(|_| "UntrustedInstallation")?;
        if !valid_version(&manifest.version) {
            return Err("UntrustedInstallation");
        }
        Self::new_with_version(
            root,
            manifest.version,
            manifest.host_digest,
            manifest.helper_digest,
        )
    }

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
        Self::new_with_version(root, "legacy".into(), host_digest, helper_digest)
    }
    fn new_with_version(
        root: PathBuf,
        version: String,
        host_digest: String,
        helper_digest: String,
    ) -> Result<Self, &'static str> {
        if !valid_version(&version) || !valid_digest(&host_digest) || !valid_digest(&helper_digest)
        {
            return Err("UntrustedInstallation");
        }
        Ok(Self {
            root,
            version,
            host_digest,
            helper_digest,
        })
    }
    pub(crate) fn version(&self) -> &str {
        &self.version
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
    #[cfg(windows)]
    pub(crate) fn lease_helper_image(&self) -> Result<super::image::ImageLease, &'static str> {
        let image = super::image::ImageLease::acquire(&self.helper_path(), &self.helper_digest)?;
        image.verify_installation_permissions()?;
        Ok(image)
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

fn valid_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
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

    #[test]
    fn manifest_requires_bounded_strict_version_and_digests() {
        let root = PathBuf::from(r"C:\Program Files\UiPilot");
        let bytes = br#"{"version":"0.3.4+build.1","host_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","helper_digest":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}"#;
        let package = TrustPackage::from_manifest(root.clone(), bytes).unwrap();
        assert_eq!(package.version(), "0.3.4+build.1");
        for invalid in [
            br#"{"version":"","host_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","helper_digest":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}"# as &[u8],
            br#"{"version":"0.3.4","host_digest":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","helper_digest":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}"# as &[u8],
            br#"{"version":"0.3.4","host_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","helper_digest":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","extra":1}"# as &[u8],
            br#"{"version":"0.3.4","host_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","host_digest":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","helper_digest":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}"# as &[u8],
        ] {
            assert_eq!(
                TrustPackage::from_manifest(root.clone(), invalid),
                Err("UntrustedInstallation")
            );
        }
        assert_eq!(
            TrustPackage::from_manifest(root, &vec![b'x'; MAX_MANIFEST_BYTES + 1]),
            Err("UntrustedInstallation")
        );
    }
}
