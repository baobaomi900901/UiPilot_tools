#[path = "../src/loader.rs"]
mod loader;
use rquickjs::{
    loader::{Loader, Resolver},
    Context, Runtime,
};
use uipilot_runtime_bridge_probe::runtime_modules::RuntimeModuleGraph;

#[test]
fn resolver_and_loader_independently_reject_unknown_members() {
    let runtime = Runtime::new().unwrap();
    runtime.set_memory_limit(2 * 1024 * 1024);
    let context = Context::full(&runtime).unwrap();
    context.with(|ctx| {
        let mut graph = loader::Graph(
            RuntimeModuleGraph::new(
                "入口.js",
                [
                    ("入口.js", b"".as_slice()),
                    ("共享/状态.js", b"export const value=1;".as_slice()),
                ],
            )
            .unwrap(),
        );
        assert_eq!(
            graph
                .resolve(&ctx, "入口.js", "./共享/状态.js", None)
                .unwrap(),
            "共享/状态.js"
        );
        assert!(graph
            .resolve(&ctx, "unknown.js", "./共享/状态.js", None)
            .is_err());
        assert!(graph.load(&ctx, "unknown.js", None).is_err());
        assert!(graph.load(&ctx, "共享/状态.js", None).is_ok());
    });
}
