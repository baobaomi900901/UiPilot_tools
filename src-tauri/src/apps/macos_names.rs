//! Search names are bundle metadata, kept separate from the verified launch target.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    os::fd::OwnedFd,
    path::{Component, Path},
};

use objc2_foundation::{
    NSArray, NSBundle, NSData, NSDictionary, NSPropertyListReadOptions,
    NSPropertyListSerialization, NSString,
};
use plist::stream::{Event, Reader};
use rustix::fs::{open, openat, Dir, Mode, OFlags};

const MAX_LOCALES: usize = 128;
const MAX_ENTRIES: usize = 2048;
const MAX_RESOURCE: usize = 256 * 1024;
const MAX_TOTAL: usize = 2 * 1024 * 1024;
// Prefer each macOS-specific value to its unqualified equivalent.
const NAME_KEYS: [&str; 4] = [
    "CFBundleDisplayName-macos", "CFBundleDisplayName", "CFBundleName-macos", "CFBundleName",
];
const DIRECTORY: OFlags = OFlags::RDONLY.union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW).union(OFlags::CLOEXEC);

type Names = Vec<(usize, String)>;
type Localizations = BTreeMap<String, Names>;

fn name(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.len() <= 1024 && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}

// Binary plists can reference the same collection or string many times. Limit the
// expanded representation before passing any allocation hints to the Value builder.
pub(super) fn bounded_dictionary(bytes: &[u8]) -> Option<plist::Dictionary> {
    const MAX_EVENTS: usize = 32_768;
    const MAX_EXPANDED_BYTES: usize = 4 * 1024 * 1024;
    const MAX_DEPTH: usize = 32;
    let mut events = Vec::new();
    let mut bytes_left = MAX_EXPANDED_BYTES;
    let mut depth = 0usize;
    for event in Reader::new(std::io::Cursor::new(bytes)) {
        let event = event.ok()?;
        if events.len() == MAX_EVENTS { return None; }
        match &event {
            Event::StartArray(hint) | Event::StartDictionary(hint) => {
                depth += 1;
                if depth > MAX_DEPTH || hint.is_some_and(|size| size > MAX_EVENTS as u64) {
                    return None;
                }
            }
            Event::EndCollection => depth = depth.checked_sub(1)?,
            Event::String(text) => bytes_left = bytes_left.checked_sub(text.len())?,
            Event::Data(data) => bytes_left = bytes_left.checked_sub(data.len())?,
            _ => {}
        }
        events.push(event);
    }
    plist::Value::from_events(events.into_iter().map(Ok)).ok()?.into_dictionary()
}

fn plist_names(dict: &plist::Dictionary) -> Names {
    NAME_KEYS.iter().enumerate().filter_map(|(priority, key)| {
        Some((priority, name(dict.get(*key)?.as_string()?)?))
    }).collect()
}

fn strings_names(data: &[u8]) -> Option<Names> {
    if data.starts_with(b"bplist") {
        return bounded_dictionary(data).as_ref().map(plist_names);
    }
    // Foundation handles text strings (including UTF-16).
    // SAFETY: no format output is requested; Foundation receives only bounded in-memory data.
    let value = unsafe {
        NSPropertyListSerialization::propertyListWithData_options_format_error(
            &NSData::with_bytes(data), NSPropertyListReadOptions::Immutable, std::ptr::null_mut()
        )
    }.ok()?.downcast::<NSDictionary>().ok()?;
    Some(NAME_KEYS.iter().enumerate().filter_map(|(priority, key)| {
        let value = value.objectForKey(&NSString::from_str(key))?.downcast::<NSString>().ok()?;
        Some((priority, name(&value.to_string())?))
    }).collect())
}

fn resources(path: &Path) -> Option<OwnedFd> {
    let mut directory = open("/", DIRECTORY, Mode::empty()).ok()?;
    for component in path.join("Contents/Resources").components() {
        match component {
            Component::RootDir => continue,
            Component::Normal(child) => directory = openat(&directory, child, DIRECTORY, Mode::empty()).ok()?,
            _ => return None,
        }
    }
    Some(directory)
}

fn read_resource(dir: &OwnedFd, file: &str, remaining: &mut usize) -> Option<Vec<u8>> {
    let file = File::from(openat(dir, file, OFlags::RDONLY | OFlags::NOFOLLOW |
        OFlags::CLOEXEC | OFlags::NONBLOCK, Mode::empty()).ok()?);
    let meta = file.metadata().ok()?;
    let limit = MAX_RESOURCE.min(*remaining);
    if !meta.is_file() || meta.len() > limit as u64 { return None; }
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(limit as u64 + 1).read_to_end(&mut bytes).ok()?;
    *remaining = remaining.saturating_sub(bytes.len());
    (bytes.len() <= limit).then_some(bytes)
}

fn locale(value: &str) -> bool {
    !value.is_empty() && value.len() <= 64 &&
        value.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

fn localized(path: &Path) -> Localizations {
    let mut names = Localizations::new();
    let Some(resources) = resources(path) else { return names; };
    let mut remaining = MAX_TOTAL;
    if let Some(table) = read_resource(&resources, "InfoPlist.loctable", &mut remaining)
        .and_then(|bytes| bounded_dictionary(&bytes))
    {
        for (language, dict) in table.into_iter().filter(|(language, _)| locale(language)).take(MAX_LOCALES) {
            if let Some(dict) = dict.as_dictionary() {
                let values = plist_names(dict);
                if !values.is_empty() { names.insert(language, values); }
            }
        }
    }

    let Ok(entries) = Dir::read_from(&resources) else { return names; };
    let mut locales = Vec::new();
    for (index, entry) in entries.take(MAX_ENTRIES + 1).enumerate() {
        if index == MAX_ENTRIES { return names; }
        if let Ok(entry) = entry {
            if let Some(language) = entry.file_name().to_str().ok().and_then(|s| s.strip_suffix(".lproj")).filter(|s| locale(s)) {
                locales.push(language.to_owned());
            }
        }
    }
    locales.sort();
    for language in locales.into_iter().take(MAX_LOCALES) {
        if remaining == 0 { break; }
        let Ok(dir) = openat(&resources, format!("{language}.lproj"), DIRECTORY, Mode::empty()) else { continue; };
        if let Some(values) = read_resource(&dir, "InfoPlist.strings", &mut remaining)
            .and_then(|bytes| strings_names(&bytes)).filter(|names| !names.is_empty()) {
            names.entry(language).or_default().extend(values);
        }
    }
    names
}

pub(super) fn read(path: &Path, info: &plist::Dictionary) -> Option<(String, Vec<String>)> {
    let localized = localized(path);
    let base = plist_names(info);
    let stem = path.file_stem().and_then(|s| s.to_str()).and_then(name);
    let development = info.get("CFBundleDevelopmentRegion").and_then(|v| v.as_string())
        .filter(|s| locale(s)).unwrap_or("en");
    let locales: Vec<_> = std::iter::once(development)
        .chain(localized.keys().map(String::as_str).filter(|&language| language != development))
        .map(NSString::from_str).collect();
    let preferred = NSBundle::preferredLocalizationsFromArray(&NSArray::from_retained_slice(&locales));
    let selected = preferred.firstObject().and_then(|lang| localized.get(&lang.to_string()));
    // Resolve DisplayName before Name, consulting the preferred locale before base
    // for each field. A generic localized title can correctly override an English base.
    let display = selected.into_iter().flatten().map(|(priority, text)| (priority / 2, 0, priority, text))
        .chain(base.iter().map(|(priority, text)| (priority / 2, 1, priority, text)))
        .min_by_key(|(field, source, priority, _)| (*field, *source, *priority))
        .map(|(_, _, _, text)| text).or(stem.as_ref())?.clone();
    let display_lower = display.to_lowercase();
    let aliases: BTreeSet<_> = base.iter().chain(localized.values().flatten())
        .map(|(_, text)| text).chain(stem.iter())
        .map(|s| s.to_lowercase()).filter(|alias| alias != &display_lower).collect();
    Some((display, aliases.into_iter().collect()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::{macos_backend::tests::Fixture, macos_backend::application, rank};
    use std::{fs, os::unix::fs::symlink};

    fn table(name: &str) -> plist::Value {
        let mut fields = plist::Dictionary::new();
        fields.insert("CFBundleDisplayName".into(), name.into());
        let mut languages = plist::Dictionary::new();
        languages.insert("zh_CN".into(), fields.into());
        languages.into()
    }

    #[test]
    fn table_strings_and_filename_match_one_application_without_changing_its_target() {
        let f = Fixture::new();
        let path = f.bundle("Tool.app", "com.fixture.localized", "Workbench");
        let resources = path.join("Contents/Resources");
        fs::create_dir_all(resources.join("fr.lproj")).unwrap();
        let original = application(&f.0, &path).unwrap();
        table("测试台").to_file_binary(resources.join("InfoPlist.loctable")).unwrap();
        let utf16: Vec<u8> = std::iter::once(0xfeffu16)
            .chain(r#""CFBundleName" = "Établi";"#.encode_utf16())
            .flat_map(u16::to_le_bytes).collect();
        fs::write(resources.join("fr.lproj/InfoPlist.strings"), utf16).unwrap();
        let app = application(&f.0, &path).unwrap();
        assert_eq!(app.app_id, original.app_id);
        assert_eq!(app.target, original.target);
        let apps = vec![app];
        for query in ["测试台", "WORKBENCH", "établi", "Tool"] {
            let found = rank(&apps, query);
            assert_eq!(found.first(), apps.first(), "{query}");
            assert_eq!(found.len(), 1);
        }
    }

    // Each array refers to its child twice. The on-disk graph is linear in depth.
    fn shared_tree(depth: u8, leaf: &[u8]) -> Vec<u8> {
        let mut objects = vec![vec![0x52, b'e', b'n'], vec![0xd1, 0, 2]];
        for level in 0..depth {
            let child = 3 + level;
            objects.push(vec![0xa2, child, child]);
        }
        objects.push(leaf.to_vec());
        let mut bytes = b"bplist00".to_vec();
        let mut offsets = Vec::new();
        for object in &objects {
            offsets.push(bytes.len() as u16);
            bytes.extend(object);
        }
        let offset_table = bytes.len() as u64;
        for offset in offsets { bytes.extend(offset.to_be_bytes()); }
        bytes.extend([0, 0, 0, 0, 0, 0, 2, 1]); // 2-byte offsets, 1-byte object refs
        bytes.extend((objects.len() as u64).to_be_bytes());
        bytes.extend(1u64.to_be_bytes()); // root dictionary
        bytes.extend(offset_table.to_be_bytes());
        bytes
    }

    #[test]
    fn shared_plist_references_have_bounded_event_count_depth_and_payload() {
        let leaf = [0x51, b'x'];
        let valid = bounded_dictionary(&shared_tree(2, &leaf)).unwrap();
        assert!(valid["en"].as_array().is_some());
        assert!(plist_names(&valid).is_empty()); // Nested data is never an application name.
        assert!(bounded_dictionary(&shared_tree(17, &leaf)).is_none());
        assert!(bounded_dictionary(&shared_tree(33, &leaf)).is_none());
        for marker in [0x4f, 0x5f] { // one shared 2-KiB data or ASCII string object
            let mut payload = vec![marker, 0x11, 0x08, 0x00];
            payload.resize(payload.len() + 2048, b'x');
            assert!(bounded_dictionary(&shared_tree(12, &payload)).is_none());
        }
    }

    #[test]
    fn macos_qualified_names_win_and_generic_names_remain_searchable() {
        let f = Fixture::new();
        let path = f.bundle("Tool.app", "com.fixture.platform", "Base");
        let mut info = plist::Dictionary::new();
        for (key, value) in [
            ("CFBundleDisplayName-iphoneos", "Phone Display"),
            ("CFBundleName-iphoneos", "Phone Name"),
            ("CFBundleName", "Generic Name"),
            ("CFBundleName-macos", "Mac Name"),
            ("CFBundleDisplayName", "Generic Display"),
            ("CFBundleDisplayName-macos", "Mac Display"),
        ] { info.insert(key.into(), value.into()); }

        let (display, aliases) = read(&path, &info).unwrap();
        assert_eq!(display, "Mac Display");
        assert!(["generic display", "generic name", "mac name"].iter()
            .all(|expected| aliases.iter().any(|alias| alias == expected)));
        assert!(aliases.iter().all(|alias| !alias.contains("phone")));
        info.remove("CFBundleDisplayName-macos");
        assert_eq!(read(&path, &info).unwrap().0, "Generic Display");
        info.remove("CFBundleDisplayName");
        assert_eq!(read(&path, &info).unwrap().0, "Mac Name");

        let localized = strings_names(br#"
            "CFBundleDisplayName" = "Everyone";
            "CFBundleDisplayName-macos" = "Desktop";
            "CFBundleName" = "All";
            "CFBundleName-macos" = "Mac";
            "CFBundleDisplayName-iphoneos" = "Phone";
        "#).unwrap();
        assert_eq!(localized, [(0, "Desktop".into()), (1, "Everyone".into()),
            (2, "Mac".into()), (3, "All".into())]);
    }

    #[test]
    #[ignore = "read-only smoke against the macOS Image Playground bundle"]
    fn system_image_playground_matches_the_native_title_and_generic_name() {
        use objc2_foundation::NSURL;
        let root = Path::new("/System/Applications");
        let path = root.join("Image Playground.app");
        let app = application(root, &path).unwrap();
        let native = NSBundle::bundleWithURL(&NSURL::from_file_path(&path).unwrap()).unwrap()
            .objectForInfoDictionaryKey(&NSString::from_str("CFBundleDisplayName")).unwrap()
            .downcast::<NSString>().unwrap().to_string();
        assert_eq!(app.display_name, native);
        eprintln!("Image Playground title: {:?}; NSBundle: {:?}", app.display_name, native);
        let apps = vec![app];
        for query in ["Playground", "Image Playground", "图乐园"] {
            assert_eq!(rank(&apps, query).first(), apps.first(), "{query}");
        }
    }

    #[test]
    fn oversized_and_symlinked_localizations_are_ignored_without_losing_the_app() {
        let f = Fixture::new();
        let path = f.bundle("Tool.app", "com.fixture.boundary", "Workbench");
        let resources = path.join("Contents/Resources");
        fs::create_dir_all(resources.join("de.lproj")).unwrap();
        let outside_dir = f.0.join("outside");
        fs::create_dir(&outside_dir).unwrap();
        let outside = outside_dir.join("InfoPlist.strings");
        fs::write(&outside, br#""CFBundleName" = "Escaped";"#).unwrap();
        symlink(&outside_dir, resources.join("en.lproj")).unwrap();
        symlink(&outside, resources.join("de.lproj/InfoPlist.strings")).unwrap();
        let oversized = resources.join("InfoPlist.loctable");
        table("Oversized").to_file_xml(&oversized).unwrap();
        File::options().write(true).open(&oversized).unwrap().set_len(MAX_RESOURCE as u64 + 1).unwrap();
        let app = application(&f.0, &path).unwrap();
        assert_eq!(app.display_name, "Workbench");
        assert!(!app.search_aliases.iter().any(|alias| alias == "escaped" || alias == "oversized"));
        fs::rename(&resources, f.0.join("moved")).unwrap();
        symlink(&outside_dir, &resources).unwrap();
        table("Escaped").to_file_binary(outside_dir.join("InfoPlist.loctable")).unwrap();
        assert!(localized(&path).is_empty());
    }
}
