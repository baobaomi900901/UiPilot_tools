use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};

#[cfg(windows)]
use webview2_com::{
    AddScriptToExecuteOnDocumentCreatedCompletedHandler, CoTaskMemPWSTR,
    IsMutedChangedEventHandler,
    Microsoft::Web::WebView2::Win32::{ICoreWebView2, ICoreWebView2Controller, ICoreWebView2_8},
};
#[cfg(windows)]
use windows::core::{Interface, BOOL};

pub(super) const INERT_PATH: &str = "__uipilot_inert.html";
pub(super) const INERT_DOCUMENT: &str = "<!doctype html><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; media-src 'none'; base-uri 'none'; form-action 'none'\">";

pub(crate) fn inert_url() -> Result<tauri::Url, WebViewGuardError> {
    tauri::Url::parse(&format!("uipilot-public-plugin://localhost/{INERT_PATH}"))
        .map_err(|_| WebViewGuardError::Native)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WebViewGuardOwner {
    Runtime {
        label: String,
        plugin_id: String,
        generation: u64,
        activation_id: u64,
    },
    Content {
        label: String,
        plugin_id: String,
        session_generation: u64,
    },
}

impl WebViewGuardOwner {
    fn label(&self) -> &str {
        match self {
            Self::Runtime { label, .. } | Self::Content { label, .. } => label,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WebViewGuardHandle {
    token: u64,
    owner: WebViewGuardOwner,
    instance: Arc<WebViewGuardInstance>,
}

impl WebViewGuardHandle {
    pub(super) fn instance(&self) -> Arc<WebViewGuardInstance> {
        Arc::clone(&self.instance)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct WebViewGuardInstance {
    id: u64,
    label: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WebViewGuardError {
    Native,
    NotMuted,
    Stale,
    TokenExhausted,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GuardPhase {
    Preparing,
    Navigated,
}

#[derive(Clone, Debug)]
struct GuardRecord {
    token: u64,
    phase: GuardPhase,
    owner: WebViewGuardOwner,
    instance: Arc<WebViewGuardInstance>,
}

#[derive(Default)]
pub(crate) struct WebViewGuardAuthority {
    next_token: AtomicU64,
    current_by_label: Mutex<HashMap<String, GuardRecord>>,
}

impl WebViewGuardAuthority {
    pub(super) fn issue(
        &self,
        owner: WebViewGuardOwner,
    ) -> Result<WebViewGuardHandle, WebViewGuardError> {
        let token = allocate_checked(&self.next_token)?;
        let instance = Arc::new(WebViewGuardInstance {
            id: token,
            label: owner.label().into(),
        });
        self.current_by_label
            .lock()
            .map_err(|_| WebViewGuardError::Unavailable)?
            .insert(
                owner.label().into(),
                GuardRecord {
                    token,
                    phase: GuardPhase::Preparing,
                    owner: owner.clone(),
                    instance: Arc::clone(&instance),
                },
            );
        Ok(WebViewGuardHandle {
            token,
            owner,
            instance,
        })
    }

    pub(crate) fn rebind_current(
        &self,
        owner: WebViewGuardOwner,
    ) -> Result<WebViewGuardHandle, WebViewGuardError> {
        let mut current = self
            .current_by_label
            .lock()
            .map_err(|_| WebViewGuardError::Unavailable)?;
        let Some(record) = current.get_mut(owner.label()) else {
            return Err(WebViewGuardError::Stale);
        };
        if record.phase != GuardPhase::Navigated {
            return Err(WebViewGuardError::Stale);
        }
        let token = allocate_checked(&self.next_token)?;
        record.token = token;
        record.phase = GuardPhase::Navigated;
        record.owner = owner.clone();
        Ok(WebViewGuardHandle {
            token,
            owner,
            instance: Arc::clone(&record.instance),
        })
    }

    #[cfg(test)]
    pub(super) fn is_current(&self, guard: &WebViewGuardHandle) -> bool {
        self.current_by_label
            .lock()
            .ok()
            .and_then(|current| current.get(guard.owner.label()).cloned())
            .is_some_and(|record| record.token == guard.token)
    }

    pub(super) fn revoke(&self, guard: &WebViewGuardHandle) -> bool {
        let Ok(mut current) = self.current_by_label.lock() else {
            return false;
        };
        if current
            .get(guard.owner.label())
            .is_some_and(|record| record.token == guard.token)
        {
            current.remove(guard.owner.label());
            true
        } else {
            false
        }
    }

    #[cfg(test)]
    pub(super) fn observe_unmuted(&self, guard: &WebViewGuardHandle) -> bool {
        self.revoke(guard)
    }

    pub(super) fn observe_instance_unmuted(
        &self,
        instance: &Arc<WebViewGuardInstance>,
    ) -> Option<WebViewGuardOwner> {
        let mut current = self.current_by_label.lock().ok()?;
        let record = current.get(&instance.label)?;
        if !Arc::ptr_eq(&record.instance, instance) {
            return None;
        }
        current.remove(&instance.label).map(|record| record.owner)
    }

    fn authorize_navigation(&self, guard: &WebViewGuardHandle) -> bool {
        let Ok(mut current) = self.current_by_label.lock() else {
            return false;
        };
        let Some(record) = current.get_mut(guard.owner.label()) else {
            return false;
        };
        if record.token != guard.token || record.phase != GuardPhase::Preparing {
            return false;
        }
        record.phase = GuardPhase::Navigated;
        true
    }
}

fn allocate_checked(next_token: &AtomicU64) -> Result<u64, WebViewGuardError> {
    let mut current = next_token.load(Ordering::Acquire);
    loop {
        let next = current
            .checked_add(1)
            .ok_or(WebViewGuardError::TokenExhausted)?;
        match next_token.compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return Ok(next),
            Err(observed) => current = observed,
        }
    }
}

pub(super) trait NativeMutePreparation {
    fn cast_controller(&mut self) -> Result<(), WebViewGuardError>;
    fn set_muted(&mut self) -> Result<(), WebViewGuardError>;
    fn register_unmute_listener(&mut self) -> Result<(), WebViewGuardError>;
    fn is_muted(&mut self) -> Result<bool, WebViewGuardError>;
}

pub(super) trait NativeNavigation {
    fn navigate(&mut self) -> Result<(), WebViewGuardError>;
}

pub(super) fn prepare_native_mute(
    boundary: &mut impl NativeMutePreparation,
) -> Result<(), WebViewGuardError> {
    boundary.cast_controller()?;
    boundary.set_muted()?;
    boundary.register_unmute_listener()?;
    if !boundary.is_muted()? {
        return Err(WebViewGuardError::NotMuted);
    }
    Ok(())
}

pub(super) fn complete_bootstrap(
    authority: &WebViewGuardAuthority,
    guard: &WebViewGuardHandle,
    boundary: &mut impl NativeNavigation,
) -> Result<(), WebViewGuardError> {
    if !authority.authorize_navigation(guard) {
        return Err(WebViewGuardError::Stale);
    }
    if let Err(error) = boundary.navigate() {
        let _ = authority.revoke(guard);
        return Err(error);
    }
    Ok(())
}

fn settle_native_dispatch(
    authority: &WebViewGuardAuthority,
    guard: &WebViewGuardHandle,
    result: Result<(), WebViewGuardError>,
) -> Result<(), WebViewGuardError> {
    if result.is_err() {
        let _ = authority.revoke(guard);
    }
    result
}

pub(crate) type UnmutedCallback = Arc<dyn Fn(WebViewGuardOwner) + Send + Sync>;

#[cfg(windows)]
pub(crate) fn prepare_windows_webview(
    webview: &tauri::Webview,
    authority: Arc<WebViewGuardAuthority>,
    owner: WebViewGuardOwner,
    bootstrap: String,
    target_url: tauri::Url,
    on_unmuted: UnmutedCallback,
    timeout: Duration,
) -> Result<WebViewGuardHandle, WebViewGuardError> {
    let target_url = windows_navigation_target(target_url)?;
    let guard = authority.issue(owner)?;
    let (sender, receiver) = mpsc::sync_channel(1);
    let setup_authority = Arc::clone(&authority);
    let setup_guard = guard.clone();
    let dispatched = webview
        .with_webview(move |platform| {
            let result = schedule_windows_bootstrap(
                platform.controller(),
                setup_authority,
                setup_guard,
                bootstrap,
                target_url,
                on_unmuted,
                sender.clone(),
            );
            if let Err(error) = result {
                let _ = sender.send(Err(error));
            }
        })
        .map_err(|_| WebViewGuardError::Native);
    settle_native_dispatch(&authority, &guard, dispatched)?;

    match receiver.recv_timeout(timeout) {
        Ok(Ok(())) => Ok(guard),
        Ok(Err(error)) => {
            let _ = authority.revoke(&guard);
            Err(error)
        }
        Err(_) => {
            let _ = authority.revoke(&guard);
            Err(WebViewGuardError::Unavailable)
        }
    }
}

#[cfg(windows)]
pub(crate) fn verify_windows_webview_muted(
    webview: &tauri::Webview,
    timeout: Duration,
) -> Result<(), WebViewGuardError> {
    let (sender, receiver) = mpsc::sync_channel(1);
    webview
        .with_webview(move |platform| {
            let result = unsafe { platform.controller().CoreWebView2() }
                .map_err(|_| WebViewGuardError::Native)
                .and_then(|core| read_core_muted(&core).map_err(|_| WebViewGuardError::Native))
                .and_then(|muted| muted.then_some(()).ok_or(WebViewGuardError::NotMuted));
            let _ = sender.send(result);
        })
        .map_err(|_| WebViewGuardError::Native)?;
    receiver
        .recv_timeout(timeout)
        .map_err(|_| WebViewGuardError::Unavailable)?
}

#[cfg(not(windows))]
pub(crate) fn verify_windows_webview_muted(
    _webview: &tauri::Webview,
    _timeout: Duration,
) -> Result<(), WebViewGuardError> {
    Err(WebViewGuardError::Native)
}

#[cfg(not(windows))]
pub(crate) fn prepare_windows_webview(
    _webview: &tauri::Webview,
    _authority: Arc<WebViewGuardAuthority>,
    _owner: WebViewGuardOwner,
    _bootstrap: String,
    _target_url: tauri::Url,
    _on_unmuted: UnmutedCallback,
    _timeout: Duration,
) -> Result<WebViewGuardHandle, WebViewGuardError> {
    Err(WebViewGuardError::Native)
}

#[cfg(windows)]
fn schedule_windows_bootstrap(
    controller: ICoreWebView2Controller,
    authority: Arc<WebViewGuardAuthority>,
    guard: WebViewGuardHandle,
    bootstrap: String,
    target_url: tauri::Url,
    on_unmuted: UnmutedCallback,
    sender: mpsc::SyncSender<Result<(), WebViewGuardError>>,
) -> Result<(), WebViewGuardError> {
    let instance = guard.instance();
    let listener_authority = Arc::clone(&authority);
    let listener = IsMutedChangedEventHandler::create(Box::new(move |sender, _| {
        let Some(sender) = sender else {
            return Ok(());
        };
        let muted = read_core_muted(&sender)?;
        if !muted {
            if let Some(owner) = listener_authority.observe_instance_unmuted(&instance) {
                on_unmuted(owner);
            }
        }
        Ok(())
    }));
    let mut boundary = WindowsMutePreparation {
        controller,
        core: None,
        core8: None,
        listener,
    };
    prepare_native_mute(&mut boundary)?;
    let core = boundary.core.ok_or(WebViewGuardError::Native)?;
    let completion_core = core.clone();
    let completion_authority = Arc::clone(&authority);
    let completion_guard = guard.clone();
    let target = target_url.to_string();
    let completion =
        AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(move |error, _| {
            let result = if error.is_ok() {
                {
                    let mut navigation = WindowsNavigation {
                        core: completion_core,
                        target,
                    };
                    complete_bootstrap(&completion_authority, &completion_guard, &mut navigation)
                }
            } else {
                Err(WebViewGuardError::Native)
            };
            let _ = sender.send(result);
            Ok(())
        }));
    let script = CoTaskMemPWSTR::from(bootstrap.as_str());
    unsafe {
        core.AddScriptToExecuteOnDocumentCreated(*script.as_ref().as_pcwstr(), &completion)
            .map_err(|_| WebViewGuardError::Native)
    }
}

#[cfg(windows)]
fn windows_navigation_target(target: tauri::Url) -> Result<tauri::Url, WebViewGuardError> {
    if target.scheme() == "uipilot-public-plugin"
        && target.host_str() == Some("localhost")
        && target.port().is_none()
    {
        let mut mapped = format!("http://uipilot-public-plugin.localhost{}", target.path());
        if let Some(query) = target.query() {
            mapped.push('?');
            mapped.push_str(query);
        }
        if let Some(fragment) = target.fragment() {
            mapped.push('#');
            mapped.push_str(fragment);
        }
        return tauri::Url::parse(&mapped).map_err(|_| WebViewGuardError::Native);
    }
    Ok(target)
}

#[cfg(windows)]
struct WindowsMutePreparation {
    controller: ICoreWebView2Controller,
    core: Option<ICoreWebView2>,
    core8: Option<ICoreWebView2_8>,
    listener:
        webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2IsMutedChangedEventHandler,
}

#[cfg(windows)]
impl NativeMutePreparation for WindowsMutePreparation {
    fn cast_controller(&mut self) -> Result<(), WebViewGuardError> {
        let core =
            unsafe { self.controller.CoreWebView2() }.map_err(|_| WebViewGuardError::Native)?;
        let core8 = core
            .cast::<ICoreWebView2_8>()
            .map_err(|_| WebViewGuardError::Native)?;
        self.core = Some(core);
        self.core8 = Some(core8);
        Ok(())
    }

    fn set_muted(&mut self) -> Result<(), WebViewGuardError> {
        unsafe {
            self.core8
                .as_ref()
                .ok_or(WebViewGuardError::Native)?
                .SetIsMuted(true)
        }
        .map_err(|_| WebViewGuardError::Native)
    }

    fn register_unmute_listener(&mut self) -> Result<(), WebViewGuardError> {
        let mut token = 0;
        unsafe {
            self.core8
                .as_ref()
                .ok_or(WebViewGuardError::Native)?
                .add_IsMutedChanged(&self.listener, &mut token)
        }
        .map_err(|_| WebViewGuardError::Native)
    }

    fn is_muted(&mut self) -> Result<bool, WebViewGuardError> {
        let mut muted = BOOL::default();
        unsafe {
            self.core8
                .as_ref()
                .ok_or(WebViewGuardError::Native)?
                .IsMuted(&mut muted)
        }
        .map_err(|_| WebViewGuardError::Native)?;
        Ok(muted.as_bool())
    }
}

#[cfg(windows)]
struct WindowsNavigation {
    core: ICoreWebView2,
    target: String,
}

#[cfg(windows)]
impl NativeNavigation for WindowsNavigation {
    fn navigate(&mut self) -> Result<(), WebViewGuardError> {
        let target = CoTaskMemPWSTR::from(self.target.as_str());
        unsafe { self.core.Navigate(*target.as_ref().as_pcwstr()) }
            .map_err(|_| WebViewGuardError::Native)
    }
}

#[cfg(windows)]
fn read_core_muted(core: &ICoreWebView2) -> windows::core::Result<bool> {
    let core8 = core.cast::<ICoreWebView2_8>()?;
    let mut muted = BOOL::default();
    unsafe { core8.IsMuted(&mut muted)? };
    Ok(muted.as_bool())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    #[derive(Default)]
    struct FakeBoundary {
        calls: Vec<&'static str>,
        muted: bool,
        navigate_calls: usize,
    }

    impl NativeMutePreparation for FakeBoundary {
        fn cast_controller(&mut self) -> Result<(), WebViewGuardError> {
            self.calls.push("cast");
            Ok(())
        }

        fn set_muted(&mut self) -> Result<(), WebViewGuardError> {
            self.calls.push("set-muted");
            self.muted = true;
            Ok(())
        }

        fn register_unmute_listener(&mut self) -> Result<(), WebViewGuardError> {
            self.calls.push("listener");
            Ok(())
        }

        fn is_muted(&mut self) -> Result<bool, WebViewGuardError> {
            self.calls.push("readback");
            Ok(self.muted)
        }
    }

    impl NativeNavigation for FakeBoundary {
        fn navigate(&mut self) -> Result<(), WebViewGuardError> {
            self.calls.push("navigate");
            self.navigate_calls += 1;
            Ok(())
        }
    }

    fn runtime_owner(label: &str, activation_id: u64) -> WebViewGuardOwner {
        WebViewGuardOwner::Runtime {
            label: label.into(),
            plugin_id: "com.example.timer".into(),
            generation: 1,
            activation_id,
        }
    }

    #[test]
    fn mute_preparation_registers_listener_before_final_readback() {
        let mut boundary = FakeBoundary::default();

        prepare_native_mute(&mut boundary).unwrap();

        assert_eq!(
            boundary.calls,
            ["cast", "set-muted", "listener", "readback"]
        );
    }

    #[cfg(windows)]
    #[test]
    fn native_navigation_uses_the_windows_custom_protocol_origin() {
        let custom =
            tauri::Url::parse("uipilot-public-plugin://localhost/dist/runtime.js?generation=3")
                .unwrap();

        let mapped = windows_navigation_target(custom).unwrap();

        assert_eq!(
            mapped.as_str(),
            "http://uipilot-public-plugin.localhost/dist/runtime.js?generation=3"
        );
    }

    #[test]
    fn bootstrap_completion_navigates_only_for_the_current_guard() {
        let authority = WebViewGuardAuthority::default();
        let current = authority.issue(runtime_owner("runtime", 7)).unwrap();
        let mut boundary = FakeBoundary::default();
        prepare_native_mute(&mut boundary).unwrap();
        boundary.calls.push("bootstrap-complete");

        complete_bootstrap(&authority, &current, &mut boundary).unwrap();

        assert_eq!(
            boundary.calls,
            [
                "cast",
                "set-muted",
                "listener",
                "readback",
                "bootstrap-complete",
                "navigate"
            ]
        );
        assert_eq!(boundary.navigate_calls, 1);
    }

    #[test]
    fn timeout_then_late_bootstrap_completion_never_navigates() {
        let authority = WebViewGuardAuthority::default();
        let guard = authority.issue(runtime_owner("runtime", 7)).unwrap();
        assert!(authority.revoke(&guard));
        let mut boundary = FakeBoundary::default();

        assert_eq!(
            complete_bootstrap(&authority, &guard, &mut boundary),
            Err(WebViewGuardError::Stale)
        );
        assert_eq!(boundary.navigate_calls, 0);
    }

    #[test]
    fn native_dispatch_failure_revokes_the_issued_guard() {
        let authority = WebViewGuardAuthority::default();
        let guard = authority.issue(runtime_owner("runtime", 7)).unwrap();

        assert_eq!(
            settle_native_dispatch(&authority, &guard, Err(WebViewGuardError::Native)),
            Err(WebViewGuardError::Native)
        );
        assert!(!authority.is_current(&guard));
    }

    #[test]
    fn old_unmute_callback_cannot_revoke_a_same_label_replacement() {
        let authority = WebViewGuardAuthority::default();
        let old = authority.issue(runtime_owner("runtime", 7)).unwrap();
        let replacement = authority.issue(runtime_owner("runtime", 8)).unwrap();

        assert!(!authority.observe_unmuted(&old));
        assert!(authority.is_current(&replacement));
        assert!(authority.observe_unmuted(&replacement));
        assert!(!authority.is_current(&replacement));
    }

    #[test]
    fn reused_content_webview_rebinds_the_listener_to_the_latest_session() {
        let authority = WebViewGuardAuthority::default();
        let first_owner = WebViewGuardOwner::Content {
            label: "content".into(),
            plugin_id: "com.example.timer".into(),
            session_generation: 1,
        };
        let first = authority.issue(first_owner).unwrap();
        let instance = first.instance();
        let mut boundary = FakeBoundary::default();
        complete_bootstrap(&authority, &first, &mut boundary).unwrap();
        let latest_owner = WebViewGuardOwner::Content {
            label: "content".into(),
            plugin_id: "com.example.timer".into(),
            session_generation: 2,
        };
        let rebound = authority.rebind_current(latest_owner.clone()).unwrap();

        assert!(!authority.is_current(&first));
        assert!(authority.is_current(&rebound));
        assert_eq!(
            authority.observe_instance_unmuted(&instance),
            Some(latest_owner)
        );
        assert!(!authority.is_current(&rebound));
    }

    #[test]
    fn preparing_guard_cannot_be_rebound_as_if_navigation_completed() {
        let authority = WebViewGuardAuthority::default();
        let owner = WebViewGuardOwner::Content {
            label: "content".into(),
            plugin_id: "com.example.timer".into(),
            session_generation: 1,
        };
        authority.issue(owner).unwrap();

        assert_eq!(
            authority.rebind_current(WebViewGuardOwner::Content {
                label: "content".into(),
                plugin_id: "com.example.timer".into(),
                session_generation: 2,
            }),
            Err(WebViewGuardError::Stale)
        );
    }

    #[test]
    fn token_exhaustion_fails_closed_without_reuse() {
        let authority = WebViewGuardAuthority {
            next_token: AtomicU64::new(u64::MAX),
            ..WebViewGuardAuthority::default()
        };

        assert_eq!(
            authority.issue(runtime_owner("runtime", 7)),
            Err(WebViewGuardError::TokenExhausted)
        );
    }

    #[test]
    fn runtime_creation_starts_inert_and_defers_bootstrap_to_the_guard() {
        let source = include_str!("../public_plugins.rs").replace("\r\n", "\n");
        let production = source
            .split("pub(crate) fn create_runtime(")
            .nth(1)
            .and_then(|tail| tail.split("pub(crate) fn destroy_runtime(").next())
            .expect("runtime creation source markers are missing");

        for required in [
            "WebviewUrl::CustomProtocol(inert_url)",
            "prepare_windows_webview(",
            "public_runtime_bootstrap(candidate.network_https_declared)",
        ] {
            assert!(production.contains(required), "missing {required}");
        }
        assert!(!production.contains(".initialization_script(public_runtime_bootstrap"));
    }
}
