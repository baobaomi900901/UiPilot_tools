//! Ephemeral secret bytes shared by platform backends; no persistence or authority.
use std::fmt;

pub(crate) struct SensitiveBytes(Vec<u8>);

impl SensitiveBytes {
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.0
    }
    // DPAPI's successful output path transfers ownership only after freeing
    // its native allocation; error paths still wipe this container on Drop.
    pub(super) fn into_vec(mut self) -> Vec<u8> {
        std::mem::take(&mut self.0)
    }
}

impl fmt::Debug for SensitiveBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SensitiveBytes([REDACTED])")
    }
}

impl Drop for SensitiveBytes {
    fn drop(&mut self) {
        for byte in &mut self.0 {
            // Volatile writes prevent the compiler from eliding the wipe.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::SensitiveBytes;

    #[test]
    fn preserves_bytes_and_redacts_debug() {
        let bytes = SensitiveBytes::new(b"synthetic-secret\0\xff".to_vec());
        assert_eq!(bytes.as_bytes(), b"synthetic-secret\0\xff");
        assert_eq!(format!("{bytes:?}"), "SensitiveBytes([REDACTED])");
        assert_eq!(bytes.into_vec(), b"synthetic-secret\0\xff");
    }

    #[test]
    fn accepts_empty_bytes_without_special_case() {
        let bytes = SensitiveBytes::new(Vec::new());
        assert!(bytes.as_bytes().is_empty());
        assert_eq!(format!("{bytes:?}"), "SensitiveBytes([REDACTED])");
    }
}
