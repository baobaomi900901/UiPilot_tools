use rquickjs::{
    loader::{ImportAttributes, Loader, Resolver},
    Ctx, Error, Module, Result,
};
use std::{collections::BTreeMap, rc::Rc};

#[derive(Clone)]
pub struct Graph(Rc<BTreeMap<String, String>>);

impl Graph {
    pub fn new(entries: &[(&str, &str)]) -> std::result::Result<Self, ()> {
        if entries.is_empty() || entries.len() > 32 {
            return Err(());
        }
        let mut total = 0;
        let mut files = BTreeMap::new();
        for &(name, source) in entries {
            if !canonical(name)
                || source.len() > 128 * 1024
                || source.contains('\0')
                || files.insert(name.to_owned(), source.to_owned()).is_some()
            {
                return Err(());
            }
            total += source.len();
        }
        if total > 1024 * 1024 {
            return Err(());
        }
        Ok(Self(Rc::new(files)))
    }
    pub fn source(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }
}

fn canonical(name: &str) -> bool {
    name.starts_with("pkg/")
        && name.ends_with(".js")
        && name.len() <= 512
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-/".contains(&c))
        && name.split('/').count() <= 8
        && name
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn resolve_name(base: &str, name: &str) -> Option<String> {
    if !canonical(base)
        || name.len() > 512
        || !(name.starts_with("./") || name.starts_with("../"))
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-/".contains(&c))
    {
        return None;
    }
    let mut pieces: Vec<_> = base.split('/').collect();
    pieces.pop();
    for part in name.split('/') {
        match part {
            "." => (),
            ".." if pieces.len() > 1 => {
                pieces.pop();
            }
            ".." | "" => return None,
            _ => pieces.push(part),
        }
    }
    let result = pieces.join("/");
    canonical(&result).then_some(result)
}

impl Resolver for Graph {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        attributes: Option<ImportAttributes<'js>>,
    ) -> Result<String> {
        if attributes.is_none() && self.source(base).is_some() {
            if let Some(result) = resolve_name(base, name) {
                if self.source(&result).is_some() {
                    return Ok(result);
                }
            }
        }
        // Fixed diagnostic; do not include a requested path in an exception.
        Err(Error::new_resolving("module", "denied"))
    }
}

impl Loader for Graph {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        attributes: Option<ImportAttributes<'js>>,
    ) -> Result<Module<'js>> {
        if attributes.is_none() && canonical(name) {
            if let Some(source) = self.source(name) {
                return Module::declare(ctx.clone(), name, source);
            }
        }
        Err(Error::new_loading("denied"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn relative_normalization_stays_in_package() {
        assert_eq!(
            resolve_name("pkg/runtime.js", "./state.js"),
            Some("pkg/state.js".into())
        );
        assert_eq!(
            resolve_name("pkg/nested/entry.js", "../state.js"),
            Some("pkg/state.js".into())
        );
    }
    #[test]
    fn unsafe_specifiers_are_rejected() {
        for name in [
            "std",
            "os",
            "node:fs",
            "other.js",
            "/etc/passwd",
            "../../outside.js",
            "https://example.invalid/a.js",
            "./state.js?q",
            "./state.js#x",
            "./%2e%2e/other.js",
            "./x.qbc",
            "./x.so",
            "./x\\y.js",
        ] {
            assert_eq!(resolve_name("pkg/runtime.js", name), None, "{name}");
        }
    }
    #[test]
    fn graph_rejects_noncanonical_names_and_duplicate_entries() {
        for name in ["std", "pkg/../a.js", "pkg/x.qbc", "pkg//a.js", "/pkg/a.js"] {
            assert!(Graph::new(&[(name, "export const a=1")]).is_err());
        }
        assert!(Graph::new(&[("pkg/a.js", ""), ("pkg/a.js", "")]).is_err());
    }
    #[test]
    fn resolver_and_loader_each_enforce_graph_membership() {
        let runtime = rquickjs::Runtime::new().unwrap();
        runtime.set_memory_limit(2 * 1024 * 1024);
        runtime.set_max_stack_size(256 * 1024);
        let context = rquickjs::Context::full(&runtime).unwrap();
        context.with(|ctx| {
            let mut graph = Graph::new(&[
                ("pkg/runtime.js", ""),
                ("pkg/state.js", "export const value=1"),
            ])
            .unwrap();
            assert_eq!(
                graph
                    .resolve(&ctx, "pkg/runtime.js", "./state.js", None)
                    .unwrap(),
                "pkg/state.js"
            );
            assert!(graph
                .resolve(&ctx, "pkg/unknown.js", "./state.js", None)
                .is_err());
            assert!(graph
                .resolve(&ctx, "pkg/runtime.js", "./missing.js", None)
                .is_err());
            assert!(graph.load(&ctx, "pkg/missing.js", None).is_err());
            assert!(graph.load(&ctx, "pkg/state.js", None).is_ok());
        });
    }
}
