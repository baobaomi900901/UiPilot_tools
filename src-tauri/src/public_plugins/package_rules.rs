//! Shared v1 package path/quantity/resource rules, without filesystem I/O.
use super::manifest::PublicPackageError;
use std::collections::BTreeMap;
use unicode_normalization::UnicodeNormalization;
// These are fixed v1 resource names/MIME values, not platform file paths.
const ALARM_PATH: &str = "assets/sounds/timer-alarm.wav";
const ALARM_MIME: &str = "audio/wav";
const ICON_MIME: &str = "image/png";
pub(super) fn fold_path(value: &str) -> String {
    let first_nfc: String = value.nfc().collect();
    let folded = icu_casemap::CaseMapper::new().fold_string(&first_nfc);
    folded.nfc().collect()
}
pub(super) const MAX_DIRECTORIES: usize = 64;
pub(super) const MAX_FILES: usize = 256;
pub(super) const MAX_DEPTH: usize = 8;
pub(super) const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
pub(super) const MAX_TOTAL_BYTES: u64 = 16 * 1024 * 1024;
pub(super) const MAX_PATH_BYTES: usize = 240;
pub(super) const MAX_COMPONENT_BYTES: usize = 100;

pub(super) fn canonical_relative_path(value: &str) -> Result<String, PublicPackageError> {
    if value.is_empty()
        || value.starts_with('/')
        || value.contains('\\')
        || value.len() > MAX_PATH_BYTES
    {
        return Err(PublicPackageError::InvalidPackage);
    }
    let components = value.split('/').collect::<Vec<_>>();
    if components.is_empty() || components.len() > MAX_DEPTH {
        return Err(PublicPackageError::InvalidPackage);
    }
    if components.iter().any(|component| {
        component.is_empty()
            || *component == "."
            || *component == ".."
            || component.len() > MAX_COMPONENT_BYTES
            || component.ends_with(['.', ' '])
            || component
                .chars()
                .any(|character| character.is_control() || "<>:\"|?*".contains(character))
            || windows_reserved_component(component)
            || !component.nfc().eq(component.chars())
    }) {
        return Err(PublicPackageError::InvalidPackage);
    }
    Ok(components.join("/"))
}

fn windows_reserved_component(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or_default();
    let lower = stem.to_ascii_lowercase();
    matches!(lower.as_str(), "con" | "prn" | "aux" | "nul")
        || lower.get(3..).is_some_and(|suffix| {
            (lower.starts_with("com") || lower.starts_with("lpt"))
                && suffix.len() == 1
                && matches!(suffix.as_bytes()[0], b'1'..=b'9')
        })
}

pub(super) fn register_parent_directories(
    path: &str,
    directories: &mut BTreeMap<String, String>,
) -> Result<(), PublicPackageError> {
    let components = path.split('/').collect::<Vec<_>>();
    for end in 1..components.len() {
        register_directory(&components[..end].join("/"), directories)?;
    }
    Ok(())
}

pub(super) fn register_directory(
    path: &str,
    directories: &mut BTreeMap<String, String>,
) -> Result<(), PublicPackageError> {
    let folded = fold_path(path);
    if directories
        .get(&folded)
        .is_some_and(|existing| existing != path)
    {
        return Err(PublicPackageError::InvalidPackage);
    }
    directories.insert(folded, path.to_owned());
    if directories.len() > MAX_DIRECTORIES {
        return Err(PublicPackageError::InvalidPackage);
    }
    Ok(())
}
pub(super) fn validate_package_resource_path(
    path: &str,
) -> Result<&'static str, PublicPackageError> {
    if path == ALARM_PATH {
        return Ok(ALARM_MIME);
    }
    if path == "plugin.json" {
        return Ok("application/json");
    }
    let basename = path
        .rsplit('/')
        .next()
        .ok_or(PublicPackageError::InvalidPackage)?;
    let parts = basename.split('.').collect::<Vec<_>>();
    if parts.len() != 2 || parts[0].is_empty() {
        return Err(PublicPackageError::InvalidPackage);
    }
    match parts[1] {
        "html" => Ok("text/html"),
        "js" => Ok("text/javascript"),
        "css" => Ok("text/css"),
        "png" => Ok(ICON_MIME),
        _ => Err(PublicPackageError::InvalidPackage),
    }
}

pub(super) fn invalid_unix_mode(mode: Option<u32>, directory: bool) -> bool {
    let Some(mode) = mode else {
        return false;
    };
    let file_type = mode & 0o170000;
    file_type != 0 && file_type != if directory { 0o040000 } else { 0o100000 }
}

pub(super) fn validate_css_text(
    path: &str,
    css: &str,
    exists: &impl Fn(&str) -> bool,
) -> Result<(), PublicPackageError> {
    let mut folded = css.to_owned();
    folded.make_ascii_lowercase();

    let mut offset = 0;
    while let Some(index) = folded[offset..].find("url(") {
        let start = offset + index + 4;
        let end = folded[start..]
            .find(')')
            .map(|end| start + end)
            .ok_or(PublicPackageError::InvalidPackage)?;
        let reference = css[start..end]
            .trim()
            .trim_matches(|character| character == '\'' || character == '"');
        validate_local_css_reference(path, reference, exists)?;
        offset = end + 1;
    }

    let mut offset = 0;
    while let Some(index) = folded[offset..].find("@import") {
        let start = offset + index + "@import".len();
        let end = folded[start..]
            .find(';')
            .map(|end| start + end)
            .ok_or(PublicPackageError::InvalidPackage)?;
        let import = css[start..end].trim();
        if !import.to_ascii_lowercase().starts_with("url(") {
            let (reference, remainder) = take_css_string(import)?;
            if !remainder.trim().is_empty() {
                return Err(PublicPackageError::InvalidPackage);
            }
            validate_local_css_reference(path, reference, exists)?;
        }
        offset = end + 1;
    }
    Ok(())
}

fn take_css_string(value: &str) -> Result<(&str, &str), PublicPackageError> {
    let quote = value
        .chars()
        .next()
        .filter(|quote| matches!(quote, '\'' | '"'))
        .ok_or(PublicPackageError::InvalidPackage)?;
    let body = &value[quote.len_utf8()..];
    let end = body.find(quote).ok_or(PublicPackageError::InvalidPackage)?;
    Ok((&body[..end], &body[end + quote.len_utf8()..]))
}
fn validate_local_css_reference(
    stylesheet: &str,
    reference: &str,
    exists: &impl Fn(&str) -> bool,
) -> Result<(), PublicPackageError> {
    if reference.is_empty()
        || reference.contains(['?', '#', '\\', ':', '%'])
        || reference.starts_with('/')
        || reference.chars().any(char::is_control)
    {
        return Err(PublicPackageError::InvalidPackage);
    }
    let mut components = stylesheet
        .rsplit_once('/')
        .map_or_else(Vec::new, |(parent, _)| {
            parent.split('/').collect::<Vec<_>>()
        });
    for component in reference.split('/') {
        match component {
            "" => return Err(PublicPackageError::InvalidPackage),
            "." => {}
            ".." => {
                components.pop().ok_or(PublicPackageError::InvalidPackage)?;
            }
            component => components.push(component),
        }
    }
    let canonical = canonical_relative_path(&components.join("/"))?;
    if !exists(&canonical) {
        return Err(PublicPackageError::InvalidPackage);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_path_normalization_and_unicode_fold_contract() {
        for path in ["plugin.json", "runtime.js", "nested/é.js", "icon.png"] {
            assert_eq!(canonical_relative_path(path).unwrap(), path);
        }
        for path in [
            "../x.js",
            "/x.js",
            "a\\b.js",
            "a//b.js",
            "CON.js",
            "a./b.js",
            "e\u{301}.js",
        ] {
            assert!(canonical_relative_path(path).is_err());
        }
        assert_eq!(fold_path("Straße.js"), fold_path("STRASSE.JS"));
        assert_eq!(fold_path("é.js"), fold_path("e\u{301}.js"));
        assert!(validate_package_resource_path("runtime.min.js").is_err());
        assert_eq!(
            validate_package_resource_path("assets/sounds/timer-alarm.wav").unwrap(),
            "audio/wav"
        );
    }
    #[test]
    fn original_css_reference_and_unix_mode_contract() {
        let exists = |path: &str| matches!(path, "theme.css" | "icon.png");
        assert!(validate_css_text(
            "nested/style.css",
            "@import '../theme.css'; a{background:URL('../icon.png')}",
            &exists
        )
        .is_ok());
        for css in [
            "a{background:url(../../escape.png)}",
            "@import 'https://example.invalid/a.css';",
            "a{background:url(data:image/png;base64,x)}",
        ] {
            assert!(validate_css_text("nested/style.css", css, &exists).is_err());
        }
        assert!(!invalid_unix_mode(None, false));
        assert!(!invalid_unix_mode(Some(0o100644), false));
        assert!(invalid_unix_mode(Some(0o120777), false));
        assert!(invalid_unix_mode(Some(0o040755), false));
    }
}
