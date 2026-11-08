use rquickjs::{Context, Function, Module, Promise, Runtime};
use serde_json::{json, Value};
use std::{
    cell::Cell,
    io::{Read, Write},
    time::{Duration, Instant},
};
fn read_frame() -> Result<Value, ()> {
    let mut header = [0u8; 4];
    std::io::stdin().read_exact(&mut header).map_err(|_| ())?;
    let len = u32::from_be_bytes(header) as usize;
    if len > 4096 {
        return Err(());
    }
    let mut bytes = vec![0; len];
    std::io::stdin().read_exact(&mut bytes).map_err(|_| ())?;
    serde_json::from_slice(&bytes).map_err(|_| ())
}
fn send(value: Value) -> Result<(), ()> {
    let bytes = serde_json::to_vec(&value).map_err(|_| ())?;
    if bytes.len() > 4096 {
        return Err(());
    }
    let mut out = std::io::stdout().lock();
    out.write_all(&(bytes.len() as u32).to_be_bytes())
        .map_err(|_| ())?;
    out.write_all(&bytes).map_err(|_| ())?;
    out.flush().map_err(|_| ())
}
fn run() -> Result<(), ()> {
    let init = read_frame()?;
    let context = init["context"].clone();
    let request = context["requestId"].as_str().ok_or(())?.to_owned();
    let mode = init["mode"].as_str().ok_or(())?.to_owned();
    if mode == "exit" {
        return Err(());
    }
    if mode == "forged" || mode == "duplicate" {
        let mut wrong = context.clone();
        if mode == "forged" {
            wrong["requestId"] = json!("public-request-ffffffffffffffff");
        }
        let frame = json!({"kind":"call","context":wrong,"callId":1,"operation":"testMemory.write","key":"sample","value":"fixed"});
        send(frame.clone())?;
        if mode == "duplicate" {
            let _ = read_frame()?;
            send(frame)?;
        }
        std::thread::sleep(Duration::from_secs(5));
        return Err(());
    }
    if matches!(
        mode.as_str(),
        "partialHeader" | "partialBody" | "oversizedFrame"
    ) {
        let mut out = std::io::stdout().lock();
        if mode == "partialHeader" {
            out.write_all(&[0, 0]).map_err(|_| ())?;
        } else if mode == "partialBody" {
            out.write_all(&100u32.to_be_bytes()).map_err(|_| ())?;
            out.write_all(b"{}").map_err(|_| ())?;
        } else {
            out.write_all(&4097u32.to_be_bytes()).map_err(|_| ())?;
        }
        out.flush().map_err(|_| ())?;
        std::thread::sleep(Duration::from_secs(5));
        return Err(());
    }
    if mode == "doubleComplete" || mode == "tailBytes" {
        let frame = json!({"kind":"complete","context":context,"response":{"requestId":request,"data":"must-reject"}});
        send(frame.clone())?;
        if mode == "doubleComplete" {
            send(frame)?;
        } else {
            std::io::stdout().write_all(b"x").map_err(|_| ())?;
        }
        return Ok(());
    }
    if mode == "stderrTail" {
        send(
            json!({"kind":"complete","context":context,"response":{"requestId":request,"data":"must-reject"}}),
        )?;
        // Fixed transport fault: stdout EOF deliberately precedes late stderr.
        unsafe extern "C" {
            fn close(fd: i32) -> i32;
        }
        unsafe {
            close(1);
        }
        std::thread::sleep(Duration::from_millis(30));
        std::io::stderr()
            .write_all(&vec![b'x'; 9000])
            .map_err(|_| ())?;
        return Ok(());
    }
    if mode == "late" {
        send(
            json!({"kind":"complete","context":context,"response":{"requestId":request,"data":"late"}}),
        )?;
        std::thread::sleep(Duration::from_secs(5));
        return Ok(());
    }
    let runtime = Runtime::new().map_err(|_| ())?;
    runtime.set_memory_limit(8 * 1024 * 1024);
    runtime.set_max_stack_size(1024 * 1024);
    let deadline = Instant::now() + Duration::from_secs(1);
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));
    let engine = Context::full(&runtime).map_err(|_| ())?;
    let response=engine.with(|ctx|->rquickjs::Result<String>{
        let next=Cell::new(1u64);let owner=context.clone();
        let bridge=Function::new(ctx.clone(),move |operation:String,key:String,value:String|->rquickjs::Result<String>{
            let call=next.get();next.set(call+1);
            send(json!({"kind":"call","context":owner,"callId":call,"operation":operation,"key":key,"value":value})).map_err(|_|rquickjs::Error::Exception)?;
            let reply=read_frame().map_err(|_|rquickjs::Error::Exception)?;
            if reply["context"]!=owner || reply["callId"]!=call || reply["kind"]!="reply" {return Err(rquickjs::Error::Exception);}
            Ok(reply["value"].to_string())
        })?;
        // The test port is deliberately NOT api.storage or any public SDK API.
        let factory:Function=ctx.eval(r#"(function(call){const parse=JSON.parse;return Object.freeze({
            write:async(k,v)=>parse(call('testMemory.write',k,v)),
            read:async(k)=>parse(call('testMemory.read',k,''))});})"#)?;
        let port:rquickjs::Value=factory.call((bridge,))?;
        let module=Module::declare(ctx.clone(),"fixed-fixture.js",r#"export async function onCommand(inv,testMemory){
            await testMemory.write('sample','fixed');const data=await testMemory.read('sample');
            return {requestId:inv.requestId,data};}"#)?;
        let(module,evaluated)=module.eval()?;evaluated.finish::<()>()?;
        let handler:Function=module.get("onCommand")?;
        let invocation=ctx.json_parse(json!({"requestId":request}).to_string())?;
        let promise:Promise=handler.call((invocation,port))?;
        let result:rquickjs::Value=promise.finish()?;
        Ok(ctx.json_stringify(result)?.ok_or(rquickjs::Error::Exception)?.to_string()?)
    }).map_err(|_|())?;
    let result: Value = serde_json::from_str(&response).map_err(|_| ())?;
    send(json!({"kind":"complete","context":context,"response":result}))
}
fn main() {
    if run().is_err() {
        std::process::exit(2);
    }
}
