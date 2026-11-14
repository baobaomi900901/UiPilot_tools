use rquickjs::{
    loader::{ImportAttributes, Loader, Resolver},
    Ctx, Error, Module, Result,
};
use uipilot_runtime_bridge_probe::runtime_modules::RuntimeModuleGraph;

#[derive(Clone)]
pub struct Graph(pub RuntimeModuleGraph);
impl Graph {
    pub fn source(&self, name: &str) -> Option<&str> {
        self.0.source(name)
    }
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
            if let Some(result) = self.0.resolve(base, name) {
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
        if attributes.is_none() {
            if let Some(source) = self.source(name) {
                return Module::declare(ctx.clone(), name, source);
            }
        }
        Err(Error::new_loading("denied"))
    }
}
