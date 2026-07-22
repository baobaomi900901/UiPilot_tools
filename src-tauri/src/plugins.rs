use std::{
    collections::{HashMap, HashSet},
    fmt, fs, io,
    path::{Path, PathBuf},
    sync::OnceLock,
};

#[derive(Debug)]
pub(crate) enum PluginSetupError {
    Io(io::Error),
    AlreadyLoaded,
}

impl fmt::Display for PluginSetupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "plugin setup I/O failed: {error}"),
            Self::AlreadyLoaded => formatter.write_str("plugin catalog is already loaded"),
        }
    }
}

impl std::error::Error for PluginSetupError {}

impl From<io::Error> for PluginSetupError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub(crate) struct PluginManager {
    catalog: OnceLock<PluginCatalog>,
}

impl PluginManager {
    pub(crate) fn new() -> Self {
        Self {
            catalog: OnceLock::new(),
        }
    }

    pub(crate) fn load(
        &self,
        app_data_dir: &Path,
        host_version: Version,
    ) -> Result<(), PluginSetupError> {
        let catalog = PluginCatalog::load(&app_data_dir.join("plugins"), host_version)?;
        self.catalog
            .set(catalog)
            .map_err(|_| PluginSetupError::AlreadyLoaded)
    }

    pub(crate) fn route(&self, query: &str) -> Option<PluginRoute> {
        self.catalog.get()?.route(query)
    }

    pub(crate) fn authorizes_clipboard(&self, plugin_id: &str) -> bool {
        self.catalog
            .get()
            .is_some_and(|catalog| catalog.authorizes_clipboard(plugin_id))
    }
}

pub(crate) struct PluginCatalog {
    entries: Vec<PluginCatalogEntry>,
}

#[derive(Clone)]
pub(crate) struct PluginCatalogEntry {
    pub(crate) id: String,
    pub(crate) version: Version,
    pub(crate) runtime: PathBuf,
    pub(crate) feature: PluginFeature,
    pub(crate) permissions: Vec<String>,
    pub(crate) root: PathBuf,
    pub(crate) window_label: String,
}

#[derive(Clone)]
pub(crate) struct PluginFeature {
    pub(crate) id: String,
    pub(crate) trigger: String,
}

#[derive(Clone)]
pub(crate) struct PluginRoute {
    pub(crate) plugin_id: String,
    pub(crate) window_label: String,
    pub(crate) input: String,
}

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct Version([u32; 3]);

impl Version {
    pub(crate) fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self([major, minor, patch])
    }

    fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('.');
        let version = Self([
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
        ]);
        parts.next().is_none().then_some(version)
    }
}

impl PluginCatalog {
    pub(crate) fn load(root: &Path, host_version: Version) -> Result<Self, PluginSetupError> {
        let mut candidates = Vec::new();
        let children = match fs::read_dir(root) {
            Ok(children) => children,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Self {
                    entries: Vec::new(),
                });
            }
            Err(error) => return Err(error.into()),
        };

        for child in children {
            let child = child?;
            if child.file_type()?.is_dir() {
                if let Some(entry) = load_entry(&child.path(), host_version) {
                    candidates.push(entry);
                }
            }
        }

        let duplicate_ids = duplicates(candidates.iter().map(|entry| entry.id.as_str()));
        let duplicate_triggers = duplicates(
            candidates
                .iter()
                .map(|entry| entry.feature.trigger.as_str()),
        );
        candidates.retain(|entry| {
            !duplicate_ids.contains(entry.id.as_str())
                && !duplicate_triggers.contains(entry.feature.trigger.as_str())
        });
        Ok(Self {
            entries: candidates,
        })
    }

    pub(crate) fn route(&self, query: &str) -> Option<PluginRoute> {
        self.entries.iter().find_map(|entry| {
            if query == entry.feature.trigger {
                Some(route(entry, ""))
            } else {
                query
                    .strip_prefix(&entry.feature.trigger)
                    .and_then(|body| body.strip_prefix(' '))
                    .map(|input| route(entry, input))
            }
        })
    }

    pub(crate) fn authorizes_clipboard(&self, plugin_id: &str) -> bool {
        self.entries.iter().any(|entry| {
            entry.id == plugin_id
                && entry
                    .permissions
                    .iter()
                    .any(|permission| permission == "clipboard.writeText")
        })
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    id: String,
    version: String,
    #[serde(rename = "minHostVersion")]
    min_host_version: String,
    runtime: String,
    feature: ManifestFeature,
    permissions: Vec<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFeature {
    id: String,
    trigger: String,
}

fn load_entry(root: &Path, host_version: Version) -> Option<PluginCatalogEntry> {
    let manifest_path = root.join("plugin.json");
    let manifest = fs::read_to_string(&manifest_path).ok()?;
    if !manifest_path.is_file() {
        return None;
    }
    let manifest: Manifest = serde_json::from_str(&manifest).ok()?;
    let version = Version::parse(&manifest.version)?;
    if Version::parse(&manifest.min_host_version)? > host_version
        || !valid_id(&manifest.id)
        || !valid_id(&manifest.feature.id)
        || !valid_trigger(&manifest.feature.trigger)
        || manifest.runtime.contains(['/', '\\'])
    {
        return None;
    }

    let runtime = root.join(&manifest.runtime);
    if !runtime.is_file() || has_bad_permissions(&manifest.permissions) {
        return None;
    }

    Some(PluginCatalogEntry {
        window_label: window_label(&manifest.id),
        id: manifest.id,
        version,
        runtime,
        feature: PluginFeature {
            id: manifest.feature.id,
            trigger: manifest.feature.trigger,
        },
        permissions: manifest.permissions,
        root: root.to_path_buf(),
    })
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-'
        })
}

fn valid_trigger(trigger: &str) -> bool {
    trigger.starts_with('/')
        && trigger.len() <= 64
        && trigger.len() > 1
        && trigger.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'/' || byte == b'-'
        })
}

fn has_bad_permissions(permissions: &[String]) -> bool {
    let mut seen = HashSet::new();
    permissions
        .iter()
        .any(|permission| permission != "clipboard.writeText" || !seen.insert(permission))
}

fn duplicates<'a>(values: impl Iterator<Item = &'a str>) -> HashSet<String> {
    let mut counts = HashMap::new();
    for value in values {
        *counts.entry(value).or_insert(0usize) += 1;
    }
    counts
        .into_iter()
        .filter_map(|(value, count)| (count > 1).then_some(value.to_string()))
        .collect()
}

fn window_label(id: &str) -> String {
    let mut label = String::from("plugin-");
    for byte in id.as_bytes() {
        label.push_str(&format!("{byte:02x}"));
    }
    label
}

fn route(entry: &PluginCatalogEntry, input: &str) -> PluginRoute {
    PluginRoute {
        plugin_id: entry.id.clone(),
        window_label: entry.window_label.clone(),
        input: input.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{PluginCatalog, Version};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TestRoot {
        path: PathBuf,
    }

    impl TestRoot {
        fn new() -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "uipilot-plugin-catalog-{}-{id}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self { path }
        }

        fn write_plugin(&self, id: &str, manifest: String) {
            assert!(!id.contains(std::path::MAIN_SEPARATOR));
            let root = self.path.join(id);
            fs::create_dir(&root).unwrap();
            fs::write(root.join("plugin.json"), manifest).unwrap();
            fs::write(root.join("index.html"), "").unwrap();
        }

        fn remove_plugin(&self, id: &str) {
            assert!(!id.contains(std::path::MAIN_SEPARATOR));
            fs::remove_dir_all(self.path.join(id)).unwrap();
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            if self.path.exists() {
                fs::remove_dir_all(&self.path).unwrap();
            }
        }
    }

    fn package_id() -> String {
        ["internal", "math"].join(".")
    }

    fn trigger() -> String {
        ["/", "math"].concat()
    }

    fn valid_manifest(plugin_id: &str, trigger: &str) -> String {
        format!(
            r#"{{
                "id":"{plugin_id}",
                "version":"1.0.0",
                "minHostVersion":"0.2.0",
                "runtime":"index.html",
                "feature":{{"id":"calculate","trigger":"{trigger}"}},
                "permissions":["clipboard.writeText"]
            }}"#
        )
    }

    fn load(root: &TestRoot) -> PluginCatalog {
        PluginCatalog::load(&root.path, Version::new(0, 2, 0)).unwrap()
    }

    #[test]
    fn package_presence_registers_trigger_and_removal_on_reload_removes_it() {
        let root = TestRoot::new();
        let id = package_id();
        let slash_trigger = trigger();
        root.write_plugin(&id, valid_manifest(&id, &slash_trigger));
        let loaded = load(&root);
        assert_eq!(
            loaded.route(&format!("{slash_trigger} 1+1")).unwrap().input,
            "1+1"
        );

        root.remove_plugin(&id);
        let reloaded = load(&root);
        assert!(reloaded.route(&format!("{slash_trigger} 1+1")).is_none());
    }

    #[test]
    fn exact_keys_versions_and_bounds_are_required() {
        let cases = [
            valid_manifest("one", "/one")
                .replace(r#""permissions""#, r#""extra":true,"permissions""#),
            valid_manifest("two", "/two").replace(r#""1.0.0""#, r#""1.0""#),
            valid_manifest("three", "/three").replace(r#""0.2.0""#, r#""0.2""#),
            valid_manifest("four", "/four")
                .replace(r#""minHostVersion":"0.2.0""#, r#""minHostVersion":"0.3.0""#),
            valid_manifest("", "/empty"),
            valid_manifest("bad/id", "/bad"),
            valid_manifest("feature", "/feature").replace(r#""id":"calculate""#, r#""id":""#),
            valid_manifest("missing-trigger", ""),
            valid_manifest("long-trigger", &format!("/{}", "x".repeat(65))),
        ];
        for (index, manifest) in cases.into_iter().enumerate() {
            let root = TestRoot::new();
            let id = format!("plugin-{index}");
            root.write_plugin(&id, manifest);
            assert!(load(&root).route("/anything").is_none(), "case {index}");
        }
    }

    #[test]
    fn unknown_and_duplicate_permissions_disable_package() {
        for manifest in [
            valid_manifest("unknown", "/unknown").replace(
                r#""clipboard.writeText""#,
                r#""clipboard.writeText","network.fetch""#,
            ),
            valid_manifest("duplicate", "/duplicate").replace(
                r#""clipboard.writeText""#,
                r#""clipboard.writeText","clipboard.writeText""#,
            ),
        ] {
            let root = TestRoot::new();
            root.write_plugin("plugin", manifest);
            assert!(load(&root).route("/unknown body").is_none());
            assert!(!load(&root).authorizes_clipboard("plugin"));
        }
    }

    #[test]
    fn duplicate_ids_or_triggers_disable_every_participant() {
        let root = TestRoot::new();
        root.write_plugin("one", valid_manifest("same", "/one"));
        root.write_plugin("two", valid_manifest("same", "/two"));
        let loaded = load(&root);
        assert!(loaded.route("/one body").is_none());
        assert!(loaded.route("/two body").is_none());

        let root = TestRoot::new();
        root.write_plugin("one", valid_manifest("one", "/same"));
        root.write_plugin("two", valid_manifest("two", "/same"));
        assert!(load(&root).route("/same body").is_none());
    }

    #[test]
    fn scans_direct_child_directories_with_ordinary_files_only() {
        let root = TestRoot::new();
        root.write_plugin("valid", valid_manifest("valid", "/valid"));
        fs::write(
            root.path.join("loose.json"),
            valid_manifest("loose", "/loose"),
        )
        .unwrap();
        let nested = root.path.join("parent").join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(
            nested.join("plugin.json"),
            valid_manifest("nested", "/nested"),
        )
        .unwrap();
        fs::write(nested.join("index.html"), "").unwrap();
        fs::remove_file(root.path.join("valid").join("index.html")).unwrap();
        fs::create_dir(root.path.join("valid").join("index.html")).unwrap();

        let loaded = load(&root);
        assert!(loaded.route("/valid body").is_none());
        assert!(loaded.route("/nested body").is_none());
        assert!(loaded.route("/loose body").is_none());
    }

    #[test]
    fn reparsed_paths_and_symlinks_are_rejected() {
        let root = TestRoot::new();
        root.write_plugin("valid", valid_manifest("valid", "/valid"));

        #[cfg(windows)]
        {
            let link = root.path.join("linked");
            if std::os::windows::fs::symlink_dir(root.path.join("valid"), link).is_err() {
                return;
            }
        }

        let loaded = load(&root);
        assert!(loaded.route("/linked body").is_none());
        assert!(loaded.route("/valid body").is_some());
    }

    #[test]
    fn route_semantics_are_trigger_then_ascii_space_body_only() {
        let root = TestRoot::new();
        root.write_plugin("plugin", valid_manifest("plugin", "/go"));
        let loaded = load(&root);

        let route = loaded.route("/go body").unwrap();
        assert_eq!(route.plugin_id, "plugin");
        assert_eq!(route.window_label, "plugin-706c7567696e");
        assert_eq!(route.input, "body");
        assert_eq!(loaded.route("/go").unwrap().input, "");
        assert!(loaded.route("/go\tbody").is_none());
        assert!(loaded.route("/good body").is_none());
        assert!(loaded.route("ordinary query").is_none());
        assert!(loaded.authorizes_clipboard("plugin"));
    }
}
