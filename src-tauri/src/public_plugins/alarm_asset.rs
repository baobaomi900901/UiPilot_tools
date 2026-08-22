use std::{fs, path::Path, sync::Arc};

use sha2::{Digest, Sha256};

use super::{PublicPackageError, PublicResource};

pub(crate) const ALARM_PATH: &str = "assets/sounds/timer-alarm.wav";
pub(crate) const ALARM_MIME: &str = "audio/wav";
const MAX_ALARM_BYTES: usize = 2 * 1024 * 1024;
const MAX_ALARM_SECONDS: u64 = 15;

#[derive(Clone, Debug)]
pub(crate) struct PreparedAlarmAsset {
    pub(crate) resource_sha256: String,
    pub(crate) bytes: Arc<[u8]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AlarmAssetIdentity {
    pub(crate) plugin_id: String,
    pub(crate) plugin_generation: u64,
    pub(crate) activation_id: u64,
    pub(crate) package_digest: String,
    pub(crate) resource_sha256: String,
    pub(crate) fixed_relative_path: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ValidatedAlarmAsset {
    pub(crate) identity: AlarmAssetIdentity,
    pub(crate) bytes: Arc<[u8]>,
}

impl ValidatedAlarmAsset {
    pub(super) fn reactivate(
        &self,
        plugin_generation: u64,
        activation_id: u64,
    ) -> ValidatedAlarmAsset {
        let mut identity = self.identity.clone();
        identity.plugin_generation = plugin_generation;
        identity.activation_id = activation_id;
        ValidatedAlarmAsset {
            identity,
            bytes: Arc::clone(&self.bytes),
        }
    }
}

pub(super) fn prepare(
    root: &Path,
    resource: &PublicResource,
) -> Result<PreparedAlarmAsset, PublicPackageError> {
    let path = root.join(ALARM_PATH);
    if !single_link_file(&path) {
        return Err(PublicPackageError::InvalidPackage);
    }
    let bytes = fs::read(path).map_err(|_| PublicPackageError::InvalidPackage)?;
    if bytes.len() as u64 != resource.length {
        return Err(PublicPackageError::InvalidPackage);
    }
    validate_wav(&bytes)?;
    if lower_hex(&Sha256::digest(&bytes)) != resource.sha256 {
        return Err(PublicPackageError::InvalidPackage);
    }
    Ok(PreparedAlarmAsset {
        resource_sha256: resource.sha256.clone(),
        bytes: Arc::from(bytes),
    })
}

impl PreparedAlarmAsset {
    pub(super) fn activate(
        &self,
        plugin_id: &str,
        plugin_generation: u64,
        activation_id: u64,
        package_digest: &str,
    ) -> ValidatedAlarmAsset {
        ValidatedAlarmAsset {
            identity: AlarmAssetIdentity {
                plugin_id: plugin_id.to_owned(),
                plugin_generation,
                activation_id,
                package_digest: package_digest.to_owned(),
                resource_sha256: self.resource_sha256.clone(),
                fixed_relative_path: ALARM_PATH,
            },
            bytes: Arc::clone(&self.bytes),
        }
    }

    pub(super) fn revalidate_at(&self, root: &Path) -> Result<(), PublicPackageError> {
        let path = root.join(ALARM_PATH);
        if !single_link_file(&path) {
            return Err(PublicPackageError::InvalidPackage);
        }
        let bytes = fs::read(path).map_err(|_| PublicPackageError::InvalidPackage)?;
        if bytes.as_slice() != self.bytes.as_ref()
            || lower_hex(&Sha256::digest(&bytes)) != self.resource_sha256
        {
            return Err(PublicPackageError::InvalidPackage);
        }
        validate_wav(&bytes)
    }
}

#[cfg(windows)]
fn single_link_file(path: &Path) -> bool {
    use std::{fs::File, os::windows::io::AsRawHandle};
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION},
    };

    let Ok(file) = File::open(path) else {
        return false;
    };
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    unsafe {
        GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut information).is_ok()
            && information.nNumberOfLinks == 1
    }
}

#[cfg(not(windows))]
fn single_link_file(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.nlink() == 1)
}

fn validate_wav(bytes: &[u8]) -> Result<(), PublicPackageError> {
    if bytes.is_empty() || bytes.len() > MAX_ALARM_BYTES || bytes.len() < 44 {
        return Err(PublicPackageError::InvalidPackage);
    }
    if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err(PublicPackageError::InvalidPackage);
    }
    let riff_size = usize::try_from(read_u32(bytes, 4)?).map_err(invalid)?;
    if riff_size.checked_add(8) != Some(bytes.len())
        || bytes.get(12..16) != Some(b"fmt ")
        || read_u32(bytes, 16)? != 16
        || bytes.get(36..40) != Some(b"data")
    {
        return Err(PublicPackageError::InvalidPackage);
    }

    let format = read_u16(bytes, 20)?;
    let channels = read_u16(bytes, 22)?;
    let sample_rate = read_u32(bytes, 24)?;
    let byte_rate = read_u32(bytes, 28)?;
    let block_align = read_u16(bytes, 32)?;
    let bits_per_sample = read_u16(bytes, 34)?;
    let data_length = usize::try_from(read_u32(bytes, 40)?).map_err(invalid)?;
    let padding = data_length % 2;
    let expected_length = 44_usize
        .checked_add(data_length)
        .and_then(|length| length.checked_add(padding))
        .ok_or(PublicPackageError::InvalidPackage)?;
    if expected_length != bytes.len()
        || (padding == 1 && bytes.last() != Some(&0))
        || format != 1
        || !matches!(channels, 1 | 2)
        || !matches!(sample_rate, 44_100 | 48_000)
        || !matches!(bits_per_sample, 16 | 24)
    {
        return Err(PublicPackageError::InvalidPackage);
    }

    let bytes_per_sample = bits_per_sample
        .checked_div(8)
        .ok_or(PublicPackageError::InvalidPackage)?;
    let expected_align = channels
        .checked_mul(bytes_per_sample)
        .ok_or(PublicPackageError::InvalidPackage)?;
    let expected_rate = sample_rate
        .checked_mul(u32::from(expected_align))
        .ok_or(PublicPackageError::InvalidPackage)?;
    if block_align != expected_align
        || byte_rate != expected_rate
        || data_length % usize::from(block_align) != 0
    {
        return Err(PublicPackageError::InvalidPackage);
    }
    let frames = data_length / usize::from(block_align);
    let max_frames = u64::from(sample_rate)
        .checked_mul(MAX_ALARM_SECONDS)
        .ok_or(PublicPackageError::InvalidPackage)?;
    if frames == 0 || u64::try_from(frames).map_err(invalid)? > max_frames {
        return Err(PublicPackageError::InvalidPackage);
    }
    Ok(())
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, PublicPackageError> {
    let end = offset
        .checked_add(2)
        .ok_or(PublicPackageError::InvalidPackage)?;
    bytes
        .get(offset..end)
        .and_then(|value| value.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or(PublicPackageError::InvalidPackage)
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, PublicPackageError> {
    let end = offset
        .checked_add(4)
        .ok_or(PublicPackageError::InvalidPackage)?;
    bytes
        .get(offset..end)
        .and_then(|value| value.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(PublicPackageError::InvalidPackage)
}

fn invalid(_: std::num::TryFromIntError) -> PublicPackageError {
    PublicPackageError::InvalidPackage
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(HEX[usize::from(byte >> 4)] as char);
        value.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    value
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{validate_wav, PreparedAlarmAsset, ALARM_PATH};

    fn wav(frames: u32, channels: u16, sample_rate: u32, bits: u16) -> Vec<u8> {
        let block_align = channels * (bits / 8);
        let byte_rate = sample_rate * u32::from(block_align);
        let data = vec![0_u8; usize::from(block_align) * usize::try_from(frames).unwrap()];
        let padding = data.len() % 2;
        let riff_size = 36_u32 + u32::try_from(data.len() + padding).unwrap();
        let mut bytes = Vec::with_capacity(44 + data.len() + padding);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&riff_size.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&byte_rate.to_le_bytes());
        bytes.extend_from_slice(&block_align.to_le_bytes());
        bytes.extend_from_slice(&bits.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(&data);
        if padding == 1 {
            bytes.push(0);
        }
        bytes
    }

    #[test]
    fn strict_parser_accepts_supported_pcm_boundaries() {
        for bytes in [
            wav(1, 1, 44_100, 16),
            wav(1, 1, 44_100, 24),
            wav(100, 2, 48_000, 24),
            wav(44_100 * 15, 1, 44_100, 24),
        ] {
            assert_eq!(validate_wav(&bytes), Ok(()));
        }
    }

    #[test]
    fn strict_parser_rejects_noncanonical_chunks_and_pcm_fields() {
        let valid = wav(100, 1, 44_100, 16);
        let mut cases = Vec::new();
        let mut bad = valid.clone();
        bad[0] = b'X';
        cases.push(bad);
        let mut bad = valid.clone();
        bad[16..20].copy_from_slice(&18_u32.to_le_bytes());
        cases.push(bad);
        let mut bad = valid.clone();
        bad[36..40].copy_from_slice(b"JUNK");
        cases.push(bad);
        let mut bad = valid.clone();
        bad[20..22].copy_from_slice(&3_u16.to_le_bytes());
        cases.push(bad);
        let mut bad = valid.clone();
        bad[28..32].copy_from_slice(&1_u32.to_le_bytes());
        cases.push(bad);
        let mut bad = valid.clone();
        bad.push(0);
        cases.push(bad);

        for bytes in cases {
            assert!(validate_wav(&bytes).is_err());
        }
    }

    #[test]
    fn activation_identity_freezes_the_exact_prepared_bytes() {
        let bytes: Arc<[u8]> = Arc::from(wav(100, 1, 44_100, 16));
        let prepared = PreparedAlarmAsset {
            resource_sha256: "a".repeat(64),
            bytes: Arc::clone(&bytes),
        };

        let active = prepared.activate("com.example.timer", 7, 19, &"b".repeat(64));

        assert_eq!(active.identity.plugin_id, "com.example.timer");
        assert_eq!(active.identity.plugin_generation, 7);
        assert_eq!(active.identity.activation_id, 19);
        assert_eq!(active.identity.package_digest, "b".repeat(64));
        assert_eq!(active.identity.resource_sha256, "a".repeat(64));
        assert_eq!(active.identity.fixed_relative_path, ALARM_PATH);
        assert!(Arc::ptr_eq(&active.bytes, &bytes));
    }
}
