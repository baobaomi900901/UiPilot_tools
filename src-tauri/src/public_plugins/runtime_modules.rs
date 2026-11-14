//! Immutable, filesystem-free ES module graph. This is not an execution grant.
use super::package_rules::*;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Only the diagnostic helper's first input frame uses this allowance.
/// A 5 MiB installed snapshot can expand when its UTF-8 source is JSON escaped.
pub const MAX_MODULE_INIT_BYTES: usize = 24 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct RuntimeModuleGraph(Arc<Graph>);
#[derive(Debug, Serialize)]
struct Graph {
    entry: String,
    modules: BTreeMap<String, String>,
}
impl Serialize for RuntimeModuleGraph {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}
impl RuntimeModuleGraph {
    /// Revalidate on both sides of a transport. Names are package-relative,
    /// never file URLs; the caller must reject duplicate JSON object keys.
    pub fn new<'a>(
        entry: &str,
        sources: impl IntoIterator<Item = (&'a str, &'a [u8])>,
    ) -> Result<Self, ()> {
        let mut modules = BTreeMap::new();
        let mut folded = BTreeSet::new();
        let mut directories = BTreeMap::new();
        let mut total = 0u64;
        for (name, bytes) in sources {
            canonical_relative_path(name).map_err(|_| ())?;
            if validate_package_resource_path(name).map_err(|_| ())? != "text/javascript"
                || name.contains(['%', '#', '?'])
                || modules.len() >= MAX_FILES
                || bytes.len() as u64 > MAX_FILE_BYTES
                || bytes.contains(&0)
                || !folded.insert(fold_path(name))
            {
                return Err(());
            }
            register_parent_directories(name, &mut directories).map_err(|_| ())?;
            total = total
                .checked_add(bytes.len() as u64)
                .filter(|n| *n <= MAX_TOTAL_BYTES)
                .ok_or(())?;
            modules.insert(
                name.to_owned(),
                std::str::from_utf8(bytes).map_err(|_| ())?.to_owned(),
            );
        }
        if !modules.contains_key(entry) || directories.keys().any(|name| folded.contains(name)) {
            return Err(());
        }
        Ok(Self(Arc::new(Graph {
            entry: entry.into(),
            modules,
        })))
    }
    pub fn entry(&self) -> &str {
        &self.0.entry
    }
    pub fn source(&self, name: &str) -> Option<&str> {
        self.0.modules.get(name).map(String::as_str)
    }
    pub fn resolve(&self, base: &str, name: &str) -> Option<String> {
        if self.source(base).is_none()
            || name.len() > MAX_PATH_BYTES
            || !(name.starts_with("./") || name.starts_with("../"))
            || name.contains(['%', '#', '?', '\\'])
        {
            return None;
        }
        let mut parts: Vec<_> = base.split('/').collect();
        parts.pop();
        for part in name.split('/') {
            match part {
                "." => (),
                ".." => {
                    parts.pop()?;
                }
                "" => return None,
                _ => parts.push(part),
            }
        }
        let resolved = parts.join("/");
        self.source(&resolved).map(|_| resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn graph(entries: &[(&str, &str)]) -> Result<RuntimeModuleGraph, ()> {
        RuntimeModuleGraph::new(
            entries.first().map_or("entry.js", |x| x.0),
            entries.iter().map(|(p, s)| (*p, s.as_bytes())),
        )
    }
    #[test]
    fn exact_unicode_paths_and_relative_imports() {
        let graph = graph(&[
            ("入口/run.js", ""),
            ("共享 模块/状态.js", "export let count=0;"),
        ])
        .unwrap();
        assert_eq!(
            graph.resolve("入口/run.js", "../共享 模块/状态.js"),
            Some("共享 模块/状态.js".into())
        );
        assert!(graph.resolve("unknown.js", "./共享 模块/状态.js").is_none());
        for name in [
            "../../outside.js",
            "../missing.js",
            "std",
            "os",
            "node:fs",
            "https://x/a.js",
            "/共享 模块/状态.js",
            "../共享%20模块/状态.js",
            "../共享 模块/状态.js#x",
            "../共享 模块/状态.js?q",
            "../共享 模块/状态",
            "../共享 模块/状态.qbc",
        ] {
            assert!(graph.resolve("入口/run.js", name).is_none(), "{name}");
        }
    }
    #[test]
    fn rejects_aliases_conflicts_invalid_sources_and_missing_entry() {
        for entries in [
            vec![("a.js", ""), ("a.js", "")],
            vec![("a.js", ""), ("A.js", "")],
            vec![("a.js", ""), ("a.js/b.js", "")],
            vec![("Dir/a.js", ""), ("dir/b.js", "")],
            vec![("e\u{301}.js", "")],
            vec![("a.js", "\0")],
            vec![("a%20b.js", "")],
            vec![("a.js", ""), ("../b.js", "")],
        ] {
            assert!(graph(&entries).is_err(), "{entries:?}");
        }
        assert!(RuntimeModuleGraph::new("absent.js", [("a.js", b"".as_slice())]).is_err());
        assert!(RuntimeModuleGraph::new("a.js", [("a.js", &[0xff][..])]).is_err());
    }
    #[test]
    fn package_limits_replace_experiment_limits() {
        let source = " ".repeat(4096);
        let names: Vec<_> = (0..MAX_FILES).map(|n| format!("m{n}.js")).collect();
        assert!(RuntimeModuleGraph::new(
            "m0.js",
            names.iter().map(|n| (n.as_str(), source.as_bytes()))
        )
        .is_ok());
        let mut too_many = names.clone();
        too_many.push("extra.js".into());
        assert!(RuntimeModuleGraph::new(
            "m0.js",
            too_many.iter().map(|n| (n.as_str(), b"".as_slice()))
        )
        .is_err());
        let max = " ".repeat(MAX_FILE_BYTES as usize);
        assert!(graph(&[("a.js", &max)]).is_ok());
        assert!(graph(&[("a.js", &(max.clone() + " "))]).is_err());
        assert!(RuntimeModuleGraph::new(
            "m0.js",
            names[..9].iter().map(|n| (n.as_str(), max.as_bytes()))
        )
        .is_err());
        let deep = format!("{}a.js", "d/".repeat(MAX_DEPTH));
        assert!(graph(&[(&deep, "")]).is_err());
    }
}
