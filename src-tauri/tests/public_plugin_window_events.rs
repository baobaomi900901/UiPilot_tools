#[cfg(windows)]
mod windows_probe {
    use std::{
        collections::BTreeSet,
        sync::{mpsc, Arc, Mutex},
        thread,
        time::Duration,
    };

    use tauri::{
        webview::{WebviewBuilder, WebviewWindowBuilder},
        AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl,
    };
    use webview2_com::{
        take_pwstr, GetProcessExtendedInfosCompletedHandler,
        Microsoft::Web::WebView2::Win32::{
            ICoreWebView2, ICoreWebView2Environment13, ICoreWebView2_2, COREWEBVIEW2_PROCESS_KIND,
            COREWEBVIEW2_PROCESS_KIND_RENDERER,
        },
    };
    use windows::{
        core::{Interface, BOOL, PWSTR},
        Win32::{
            Foundation::CloseHandle,
            System::Threading::{
                OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
            },
        },
    };

    const RUNTIME_A: &str =
        "plugin-runtime-636f6d2e756970696c6f742e70726f62652d61-g0000000000000001";
    const RUNTIME_B: &str =
        "plugin-runtime-636f6d2e756970696c6f742e70726f62652d62-g0000000000000001";
    const SHELL: &str = "plugin-shell-636f6d2e756970696c6f742e70726f6265";
    const CONTENT: &str = "plugin-content-636f6d2e756970696c6f742e70726f6265";

    #[derive(Debug)]
    struct RendererProcess {
        pid: u32,
        frame_urls: Vec<String>,
    }

    fn extended_processes(core: ICoreWebView2) -> Result<Vec<RendererProcess>, String> {
        let core: ICoreWebView2_2 = core.cast().map_err(|error| error.to_string())?;
        let environment: ICoreWebView2Environment13 = unsafe { core.Environment() }
            .map_err(|error| error.to_string())?
            .cast()
            .map_err(|error| error.to_string())?;
        let (sender, receiver) = mpsc::channel();
        let handler =
            GetProcessExtendedInfosCompletedHandler::create(Box::new(move |status, collection| {
                let _ = sender.send((status, collection));
                Ok(())
            }));
        unsafe { environment.GetProcessExtendedInfos(&handler) }
            .map_err(|error| error.to_string())?;
        let (status, collection) =
            webview2_com::wait_with_pump(receiver).map_err(|error| error.to_string())?;
        status.map_err(|error| error.to_string())?;
        let collection = collection.ok_or("WebView2 returned no process collection")?;
        let mut count = 0;
        unsafe { collection.Count(&mut count) }.map_err(|error| error.to_string())?;
        let mut output = Vec::new();
        for index in 0..count {
            let extended =
                unsafe { collection.GetValueAtIndex(index) }.map_err(|error| error.to_string())?;
            let info = unsafe { extended.ProcessInfo() }.map_err(|error| error.to_string())?;
            let mut kind = COREWEBVIEW2_PROCESS_KIND(0);
            unsafe { info.Kind(&mut kind) }.map_err(|error| error.to_string())?;
            if kind != COREWEBVIEW2_PROCESS_KIND_RENDERER {
                continue;
            }
            let mut pid = 0;
            unsafe { info.ProcessId(&mut pid) }.map_err(|error| error.to_string())?;
            let frames =
                unsafe { extended.AssociatedFrameInfos() }.map_err(|error| error.to_string())?;
            let iterator = unsafe { frames.GetIterator() }.map_err(|error| error.to_string())?;
            let mut has_current = BOOL::default();
            unsafe { iterator.HasCurrent(&mut has_current) }.map_err(|error| error.to_string())?;
            let mut frame_urls = Vec::new();
            while has_current.as_bool() {
                let frame = unsafe { iterator.GetCurrent() }.map_err(|error| error.to_string())?;
                let mut raw = PWSTR::null();
                unsafe { frame.Source(&mut raw) }.map_err(|error| error.to_string())?;
                frame_urls.push(take_pwstr(raw));
                unsafe { iterator.MoveNext(&mut has_current) }
                    .map_err(|error| error.to_string())?;
            }
            output.push(RendererProcess {
                pid: u32::try_from(pid).map_err(|_| "invalid renderer PID")?,
                frame_urls,
            });
        }
        Ok(output)
    }

    fn renderer_pids(
        app: &AppHandle,
        label: &str,
        frame_fragment: &str,
    ) -> Result<BTreeSet<u32>, String> {
        let webview = app
            .get_webview(label)
            .ok_or_else(|| format!("missing WebView: {label}"))?;
        let (sender, receiver) = mpsc::channel();
        webview
            .with_webview(move |platform| {
                let result = unsafe { platform.controller().CoreWebView2() }
                    .map_err(|error| error.to_string())
                    .and_then(extended_processes);
                let _ = sender.send(result);
            })
            .map_err(|error| error.to_string())?;
        let processes = receiver
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| format!("timed out reading renderer processes for {label}"))??;
        let pids = processes
            .iter()
            .filter(|process| {
                process
                    .frame_urls
                    .iter()
                    .any(|url| url.contains(frame_fragment))
            })
            .map(|process| process.pid)
            .collect::<BTreeSet<_>>();
        if pids.len() != 1 {
            return Err(format!(
                "expected one renderer for {label} ({frame_fragment}), got {pids:?}; observed renderers: {processes:?}"
            ));
        }
        Ok(pids)
    }

    fn webview_host_fragment(app: &AppHandle, label: &str) -> Result<String, String> {
        let webview = app
            .get_webview(label)
            .ok_or_else(|| format!("missing WebView: {label}"))?;
        let url = webview.url().map_err(|error| error.to_string())?;
        let host = url
            .host_str()
            .ok_or_else(|| format!("WebView {label} URL has no host: {url}"))?;
        Ok(match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        })
    }

    fn assert_disjoint(
        left_name: &str,
        left: &BTreeSet<u32>,
        right_name: &str,
        right: &BTreeSet<u32>,
    ) -> Result<(), String> {
        if left.is_disjoint(right) {
            Ok(())
        } else {
            Err(format!(
                "renderer isolation failed: {left_name} {left:?} overlaps {right_name} {right:?}"
            ))
        }
    }

    fn terminate_renderer(pid: u32) -> Result<(), String> {
        let process = unsafe { OpenProcess(PROCESS_TERMINATE, false, pid) }
            .map_err(|error| error.to_string())?;
        let result =
            unsafe { TerminateProcess(process, 0x5549_504c) }.map_err(|error| error.to_string());
        let _ = unsafe { CloseHandle(process) };
        result
    }

    fn process_is_alive(pid: u32) -> bool {
        let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
        else {
            return false;
        };
        let _ = unsafe { CloseHandle(process) };
        true
    }

    fn single(set: &BTreeSet<u32>) -> u32 {
        *set.iter().next().expect("renderer set is empty")
    }

    fn probe(app: &AppHandle) -> Result<(), String> {
        let trusted_fragment = webview_host_fragment(app, "main")?;
        let main = renderer_pids(app, "main", &trusted_fragment)?;
        let find = renderer_pids(app, "find", &trusted_fragment)?;
        let shell = renderer_pids(app, SHELL, &trusted_fragment)?;
        let runtime_a = renderer_pids(app, RUNTIME_A, "probe=a")?;
        let runtime_b = renderer_pids(app, RUNTIME_B, "probe=b")?;
        let content = renderer_pids(app, CONTENT, "dist/window.html")?;

        for (trusted_name, trusted) in [("main", &main), ("find", &find), ("shell", &shell)] {
            assert_disjoint("runtime-a", &runtime_a, trusted_name, trusted)?;
            assert_disjoint("runtime-b", &runtime_b, trusted_name, trusted)?;
            assert_disjoint("content", &content, trusted_name, trusted)?;
        }
        assert_disjoint("runtime-a", &runtime_a, "runtime-b", &runtime_b)?;
        assert_disjoint("runtime-a", &runtime_a, "content", &content)?;
        assert_disjoint("runtime-b", &runtime_b, "content", &content)?;

        terminate_renderer(single(&runtime_a))?;
        thread::sleep(Duration::from_millis(750));
        for (name, pid) in [
            ("main", single(&main)),
            ("find", single(&find)),
            ("shell", single(&shell)),
            ("runtime-b", single(&runtime_b)),
            ("content", single(&content)),
        ] {
            if !process_is_alive(pid) {
                return Err(format!("{name} renderer exited with failed runtime-a"));
            }
        }

        terminate_renderer(single(&content))?;
        thread::sleep(Duration::from_millis(750));
        for (name, pid) in [
            ("main", single(&main)),
            ("find", single(&find)),
            ("shell", single(&shell)),
            ("runtime-b", single(&runtime_b)),
        ] {
            if !process_is_alive(pid) {
                return Err(format!("{name} renderer exited with failed content"));
            }
        }

        let main_window = app
            .get_webview_window("main")
            .ok_or("main window missing")?;
        main_window.show().map_err(|error| error.to_string())?;
        main_window.set_focus().map_err(|error| error.to_string())?;
        thread::sleep(Duration::from_millis(250));
        if !main_window
            .is_focused()
            .map_err(|error| error.to_string())?
        {
            return Err("main window did not receive native focus".into());
        }
        main_window.hide().map_err(|error| error.to_string())?;
        Ok(())
    }

    pub(super) fn run() -> Result<(), String> {
        let result = Arc::new(Mutex::new(None));
        let setup_result = Arc::clone(&result);
        let app = tauri::Builder::default()
            .any_thread()
            .register_uri_scheme_protocol("uipilot-public-plugin", |_context, _request| {
                tauri::http::Response::builder()
                    .status(200)
                    .header("content-type", "text/html")
                    .body(b"<!doctype html><title>probe</title>".to_vec())
                    .unwrap()
            })
            .setup(move |app| {
                for (label, marker) in [(RUNTIME_A, "a"), (RUNTIME_B, "b")] {
                    WebviewWindowBuilder::new(
                        app,
                        label,
                        WebviewUrl::CustomProtocol(
                            tauri::Url::parse(
                                &format!("uipilot-public-plugin://localhost/__uipilot_runtime.html?probe={marker}"),
                            )
                            .unwrap(),
                        ),
                    )
                    .visible(false)
                    .focusable(false)
                    .skip_taskbar(true)
                    .incognito(true)
                    .build()?;
                }
                let shell =
                    WebviewWindowBuilder::new(app, SHELL, WebviewUrl::App("index.html".into()))
                        .visible(false)
                        .decorations(false)
                        .build()?;
                let content = WebviewBuilder::new(
                    CONTENT,
                    WebviewUrl::CustomProtocol(
                        tauri::Url::parse("uipilot-public-plugin://localhost/dist/window.html")
                            .unwrap(),
                    ),
                );
                app.get_window(SHELL)
                    .ok_or("probe shell native window missing")?
                    .add_child(
                        content,
                        LogicalPosition::new(0.0, 44.0),
                        LogicalSize::new(640.0, 436.0),
                    )?;
                let handle = app.handle().clone();
                let worker_result = Arc::clone(&setup_result);
                thread::spawn(move || {
                    thread::sleep(Duration::from_secs(3));
                    let outcome = probe(&handle);
                    *worker_result.lock().expect("probe result lock poisoned") = Some(outcome);
                    handle.exit(0);
                });
                let _ = shell;
                Ok(())
            })
            .build(tauri::generate_context!())
            .map_err(|error| error.to_string())?;
        let exit_code = app.run_return(|_, _| {});
        if exit_code != 0 {
            return Err(format!("probe event loop exited with code {exit_code}"));
        }
        let outcome = result
            .lock()
            .map_err(|_| "probe result lock poisoned")?
            .take()
            .ok_or("probe exited without a result")?;
        outcome
    }
}

#[test]
fn public_plugin_windows_are_process_isolated_and_failure_reclaimable() {
    if std::env::var("UIPILOT_RUN_REAL_WINDOW_TESTS").as_deref() != Ok("1") {
        return;
    }
    #[cfg(windows)]
    windows_probe::run().unwrap();
    #[cfg(not(windows))]
    panic!("real public plugin window probe is only implemented on Windows");
}
