mod loader;
use loader::Graph;
use rquickjs::{Context, Ctx, FromJs, Function, Module, Promise, Runtime};
use serde_json::{json, Value};
use std::{
    cell::Cell,
    io::{Read, Write},
    rc::Rc,
    time::{Duration, Instant},
};

const HEAP: usize = 8 * 1024 * 1024;
const STACK: usize = 1024 * 1024;
const ENTRY: &str = "pkg/runtime.js";

fn graph(case: &str) -> Result<Graph, ()> {
    let source = match case {
        "demo" => include_str!(
            "../../../examples/public-plugins/com.uipilot.demo-return/package/dist/runtime.js"
        ),
        "modules" => include_str!("../fixtures/cycle/runtime.js"),
        "dynamicDenied" => include_str!("../fixtures/dynamic-denied.js"),
        "globals" => include_str!("../fixtures/globals.js"),
        "heapLow" | "heapHigh" => include_str!("../fixtures/heap.js"),
        "stackLow" | "stackHigh" => include_str!("../fixtures/stack.js"),
        "interrupt" | "parentInfinite" => include_str!("../fixtures/infinite.js"),
        "pending" => include_str!("../fixtures/pending.js"),
        "modulePending" => include_str!("../fixtures/module-pending.js"),
        "strict" => include_str!("../../macos-jsc-esm-probe/fixtures/strict.js"),
        "forged" => include_str!("../../macos-jsc-esm-probe/fixtures/private-channel.js"),
        "intrinsics" => include_str!("../../macos-jsc-esm-probe/fixtures/intrinsics.js"),
        "wrongRequest" => include_str!("../../macos-jsc-esm-probe/fixtures/wrong-request.js"),
        "oversized" => include_str!("../../macos-jsc-esm-probe/fixtures/oversized.js"),
        "throw" => include_str!("../../macos-jsc-esm-probe/fixtures/throw.js"),
        "staticStd" => "import 'std'; export function onCommand(){}",
        "staticOs" => "import 'os'; export function onCommand(){}",
        "staticEscape" => "import '../../outside.js'; export function onCommand(){}",
        "attributes" => "import './state.js' with {type:'json'}; export function onCommand(){}",
        _ => return Err(()),
    };
    let mut entries = vec![(ENTRY, source)];
    if case == "modules" {
        entries.extend([
            ("pkg/a.js", include_str!("../fixtures/cycle/a.js")),
            ("pkg/b.js", include_str!("../fixtures/cycle/b.js")),
            (
                "pkg/reexport.js",
                include_str!("../fixtures/cycle/reexport.js"),
            ),
        ]);
    } else if case == "dynamicDenied" || case == "attributes" {
        entries.push(("pkg/state.js", "export const value = 1;"));
    }
    Graph::new(&entries)
}

fn pump<'js, T: FromJs<'js>>(
    ctx: &Ctx<'js>,
    promise: &Promise<'js>,
    interrupted: &Cell<bool>,
) -> rquickjs::Result<T> {
    loop {
        if interrupted.get() {
            return Err(rquickjs::Error::Exception);
        }
        if let Some(result) = promise.result() {
            return result;
        }
        if !ctx.execute_pending_job() {
            std::thread::sleep(Duration::from_millis(2));
        }
        // A pending Promise without jobs cannot execute the engine interrupt.
        // The independent parent deadline remains responsible for kill + wait.
    }
}

fn execute(case: &str, request: &str, graph: Graph) -> Result<Value, ()> {
    let heap = if case == "heapLow" {
        2 * 1024 * 1024
    } else {
        HEAP
    };
    let stack = if case == "stackLow" { 64 * 1024 } else { STACK };
    let runtime = Runtime::new().map_err(|_| ())?;
    runtime.set_memory_limit(heap);
    runtime.set_max_stack_size(stack);
    let interrupted = Rc::new(Cell::new(false));
    if case != "parentInfinite" {
        let flag = interrupted.clone();
        let deadline = Instant::now() + Duration::from_millis(150);
        runtime.set_interrupt_handler(Some(Box::new(move || {
            let expired = Instant::now() >= deadline;
            if expired {
                flag.set(true);
            }
            expired
        })));
    }
    runtime.set_loader(graph.clone(), graph.clone());
    let context = Context::full(&runtime).map_err(|_| ())?;
    let phase = Cell::new("setup");
    let result: Result<Value, ()> = context.with(|ctx| {
        let run = || -> rquickjs::Result<Value> {
            let factory: Function = ctx.eval(include_str!("../fixtures/runner.js"))?;
            let input = json!({"apiVersion":1,"requestId":request,"input":"中文 😀  保留空格",
                "context":{"platform":"macos","theme":"dark","invokedAt":"2026-11-08T12:34:56+08:00"}}).to_string();
            let runner: Function = factory.call((input,))?;
            phase.set("moduleDeclaration");
            let module = Module::declare(ctx.clone(), ENTRY, graph.source(ENTRY).unwrap_or(""))?;
            phase.set("moduleEvaluation");
            let (module, evaluated) = module.eval()?;
            pump::<()>(&ctx, &evaluated, &interrupted)?;
            // Evaluated Rust type alone is insufficient: TLA promise must finish.
            phase.set("command");
            let handler: Function = module.get("onCommand")?;
            let promise: Promise = runner.call((handler,))?;
            let text: String = pump(&ctx, &promise, &interrupted)?;
            if text.len() > 4096 { return Err(rquickjs::Error::Exception); }
            let response: Value = serde_json::from_str(&text).map_err(|_| rquickjs::Error::Exception)?;
            if response.get("requestId").and_then(Value::as_str) != Some(request) {
                return Err(rquickjs::Error::Exception);
            }
            Ok(response)
        };
        run().map_err(|_| { let _ = ctx.catch(); })
    });
    let resource_case = matches!(
        case,
        "heapLow" | "heapHigh" | "stackLow" | "stackHigh" | "interrupt"
    );
    if result.is_err() && !resource_case {
        return Err(());
    }
    let outcome = if interrupted.get() {
        "interrupted"
    } else if result.is_ok() {
        "completed"
    } else {
        "failed"
    };
    // Snapshot AFTER execution, with Context alive. Not a peak measurement/RSS.
    let memory = runtime.memory_usage();
    Ok(
        json!({"phase":phase.get(),"outcome":outcome,"response":result.ok(),"interruptFired":interrupted.get(),
        "limits":{"heap":heap,"stack":stack},
        "memory":{"mallocLimit":memory.malloc_limit,"mallocSize":memory.malloc_size,"memoryUsedSize":memory.memory_used_size}}),
    )
}

fn input() -> Result<(String, String, String, Graph), ()> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() > 4096 {
        return Err(());
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| ())?;
    if value.as_object().map(|object| object.len()) != Some(3) {
        return Err(());
    }
    let generation = value.get("generation").and_then(Value::as_str).ok_or(())?;
    let request = value.get("requestId").and_then(Value::as_str).ok_or(())?;
    let case = value.get("case").and_then(Value::as_str).ok_or(())?;
    if generation.starts_with('0')
        || generation.is_empty()
        || !generation.bytes().all(|x| x.is_ascii_digit())
        || generation.parse::<u64>().is_err()
        || request.is_empty()
        || request.chars().count() > 128
        || request.contains('\0')
    {
        return Err(());
    }
    Ok((generation.into(), request.into(), case.into(), graph(case)?))
}

fn main() {
    let Ok((generation, request, case, graph)) = input() else {
        std::process::exit(2);
    };
    let (status, payload) = match execute(&case, &request, graph) {
        Ok(value) => ("ok", value),
        Err(()) => ("scriptFailed", Value::Null),
    };
    let record =
        json!({"generation":generation,"requestId":request,"status":status,"value":payload});
    let Ok(wire) = serde_json::to_vec(&record) else {
        std::process::exit(2);
    };
    if wire.len() > 8192 || std::io::stdout().write_all(&wire).is_err() {
        std::process::exit(2);
    }
}
