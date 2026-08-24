use std::io::Cursor;

use png::{Decoder, Limits};

use super::PublicPackageError;

pub(super) const ICON_PATH: &str = "icon.png";
pub(super) const ICON_MIME: &str = "image/png";
pub(super) const MAX_ICON_BYTES: usize = 128 * 1024;
#[cfg(any(target_os = "windows", target_os = "android"))]
const ICON_URL_ORIGIN: &str = "http://uipilot-public-plugin.localhost";
#[cfg(not(any(target_os = "windows", target_os = "android")))]
const ICON_URL_ORIGIN: &str = "uipilot-public-plugin://localhost";
const ICON_REQUEST_PREFIX: &str = "/__uipilot_icon/";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum IconRequest {
    Installed { plugin_id: String, generation: u64 },
    Prepared { token: String },
}

pub(super) fn installed_url(plugin_id: &str, generation: u64) -> String {
    format!("{ICON_URL_ORIGIN}{ICON_REQUEST_PREFIX}installed/{plugin_id}/{generation}/{ICON_PATH}")
}

pub(super) fn prepared_url(token: &str) -> String {
    format!("{ICON_URL_ORIGIN}{ICON_REQUEST_PREFIX}prepared/{token}/{ICON_PATH}")
}

pub(super) fn parse_request_path(path: &str) -> Option<IconRequest> {
    if path.contains(['\\', '?', '#', '%']) {
        return None;
    }
    let parts = path
        .strip_prefix(ICON_REQUEST_PREFIX)?
        .split('/')
        .collect::<Vec<_>>();
    match parts.as_slice() {
        ["installed", plugin_id, raw_generation, ICON_PATH]
            if super::manifest::valid_plugin_id(plugin_id) =>
        {
            let generation = raw_generation.parse::<u64>().ok()?;
            if generation == 0 || generation.to_string() != *raw_generation {
                return None;
            }
            Some(IconRequest::Installed {
                plugin_id: (*plugin_id).to_owned(),
                generation,
            })
        }
        ["prepared", token, ICON_PATH] if valid_prepare_token(token) => {
            Some(IconRequest::Prepared {
                token: (*token).to_owned(),
            })
        }
        _ => None,
    }
}

pub(super) fn is_icon_request(path: &str) -> bool {
    path.starts_with(ICON_REQUEST_PREFIX)
}

fn valid_prepare_token(value: &str) -> bool {
    let Some(value) = value.strip_prefix("public-prepare-") else {
        return false;
    };
    let Some((process, sequence)) = value.split_once('-') else {
        return false;
    };
    [process, sequence].into_iter().all(|part| {
        part.len() == 16
            && part
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    })
}

pub(super) fn validate_png(bytes: &[u8]) -> Result<(), PublicPackageError> {
    if bytes.is_empty() || bytes.len() > MAX_ICON_BYTES {
        return Err(PublicPackageError::InvalidPackage);
    }
    let limits = Limits {
        bytes: MAX_ICON_BYTES * 2,
    };
    let mut reader = Decoder::new_with_limits(Cursor::new(bytes), limits)
        .read_info()
        .map_err(|_| PublicPackageError::InvalidPackage)?;
    let info = reader.info();
    if info.width != 128 || info.height != 128 || info.animation_control.is_some() {
        return Err(PublicPackageError::InvalidPackage);
    }
    let output_size = reader
        .output_buffer_size()
        .ok_or(PublicPackageError::InvalidPackage)?;
    let mut output = vec![0_u8; output_size];
    reader
        .next_frame(&mut output)
        .map_err(|_| PublicPackageError::InvalidPackage)?;
    reader
        .finish()
        .map_err(|_| PublicPackageError::InvalidPackage)
}

#[cfg(test)]
mod tests {
    use super::installed_url;

    #[test]
    fn emitted_icon_origin_matches_the_platform_custom_protocol_transport() {
        let url = installed_url("com.example.demo", 1);
        #[cfg(any(target_os = "windows", target_os = "android"))]
        assert!(url.starts_with("http://uipilot-public-plugin.localhost/"));
        #[cfg(not(any(target_os = "windows", target_os = "android")))]
        assert!(url.starts_with("uipilot-public-plugin://localhost/"));
    }
}
