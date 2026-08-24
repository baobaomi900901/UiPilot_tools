use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use windows::{
    core::{IInspectable, HSTRING},
    Data::Xml::Dom::XmlDocument,
    Foundation::TypedEventHandler,
    Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
    UI::Notifications::{
        NotificationSetting, ToastDismissedEventArgs, ToastFailedEventArgs, ToastNotification,
        ToastNotificationManager, ToastNotifier,
    },
};

use crate::message_center::{MessagePublished, MessageToast, NativeEffectError};

use super::{
    windows_identity::{self, BuildIdentity},
    NativeNotificationId, ToastCallbackKind, ToastCallbackSink,
};

const TOAST_TEMPLATE: &str =
    r#"<toast><visual><binding template="ToastGeneric"><text/><text/></binding></visual></toast>"#;
const ACTIVE_TOAST_CAPACITY: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ToastError {
    Disabled,
    InvalidMessageId,
    Capacity,
    Platform,
}

trait ToastBackend: Send {
    type Handle: Clone;

    fn enabled(&self) -> Result<bool, ToastError>;
    fn prepare(
        &self,
        document: XmlDocument,
        notification_id: NativeNotificationId,
        sink: ToastCallbackSink,
    ) -> Result<Self::Handle, ToastError>;
    fn show(&self, handle: &Self::Handle) -> Result<(), ToastError>;
    fn hide(&self, handle: &Self::Handle) -> Result<(), ToastError>;
    fn cleanup(&self, handle: &Self::Handle);
}

struct ToastController<B: ToastBackend> {
    backend: B,
    active: HashMap<NativeNotificationId, B::Handle>,
}

impl<B: ToastBackend> ToastController<B> {
    fn new(backend: B) -> Self {
        Self {
            backend,
            active: HashMap::new(),
        }
    }

    fn show(
        &mut self,
        message: &MessagePublished,
        sink: ToastCallbackSink,
    ) -> Result<(), ToastError> {
        if self.active.len() >= ACTIVE_TOAST_CAPACITY {
            return Err(ToastError::Capacity);
        }
        let notification_id = parse_notification_id(&message.id)?;
        match self.backend.enabled() {
            Ok(true) => {}
            Ok(false) => return Err(ToastError::Disabled),
            // A failed preflight cannot override the authoritative Show result.
            Err(ToastError::Platform) => {}
            Err(error) => return Err(error),
        }
        let document = build_toast_document(&message.plugin_name_snapshot, &message.content)?;
        let handle = self.backend.prepare(document, notification_id, sink)?;
        self.active.insert(notification_id, handle.clone());
        if let Err(error) = self.backend.show(&handle) {
            if let Some(handle) = self.active.remove(&notification_id) {
                self.backend.cleanup(&handle);
            }
            return Err(error);
        }
        Ok(())
    }

    fn finish(&mut self, notification_id: NativeNotificationId) {
        if let Some(handle) = self.active.remove(&notification_id) {
            self.backend.cleanup(&handle);
        }
    }

    fn shutdown(&mut self) {
        for (_, handle) in self.active.drain() {
            let _ = self.backend.hide(&handle);
            self.backend.cleanup(&handle);
        }
    }
}

#[derive(Clone)]
struct WinRtToastHandle {
    notification: ToastNotification,
    activated_token: i64,
    failed_token: i64,
    dismissed_token: i64,
}

struct WinRtToastBackend {
    notifier: Option<ToastNotifier>,
    initialized: bool,
}

impl WinRtToastBackend {
    fn new(identity: BuildIdentity) -> Result<Self, ToastError> {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(|_| ToastError::Platform)?;
        match ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(identity.aumid)) {
            Ok(notifier) => Ok(Self {
                notifier: Some(notifier),
                initialized: true,
            }),
            Err(_) => {
                unsafe { RoUninitialize() };
                Err(ToastError::Platform)
            }
        }
    }

    fn notifier(&self) -> Result<&ToastNotifier, ToastError> {
        self.notifier.as_ref().ok_or(ToastError::Platform)
    }
}

impl ToastBackend for WinRtToastBackend {
    type Handle = WinRtToastHandle;

    fn enabled(&self) -> Result<bool, ToastError> {
        self.notifier()?
            .Setting()
            .map(|setting| setting == NotificationSetting::Enabled)
            .map_err(|_| ToastError::Platform)
    }

    fn prepare(
        &self,
        document: XmlDocument,
        notification_id: NativeNotificationId,
        sink: ToastCallbackSink,
    ) -> Result<Self::Handle, ToastError> {
        let notification = ToastNotification::CreateToastNotification(&document)
            .map_err(|_| ToastError::Platform)?;
        let activated_sink = sink.clone();
        let activated_token = notification
            .Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(
                move |_, _| {
                    activated_sink(notification_id, ToastCallbackKind::Activated);
                    Ok(())
                },
            ))
            .map_err(|_| ToastError::Platform)?;
        let failed_sink = sink.clone();
        let failed_token = match notification.Failed(&TypedEventHandler::<
            ToastNotification,
            ToastFailedEventArgs,
        >::new(move |_, _| {
            failed_sink(notification_id, ToastCallbackKind::Failed);
            Ok(())
        })) {
            Ok(token) => token,
            Err(_) => {
                let _ = notification.RemoveActivated(activated_token);
                return Err(ToastError::Platform);
            }
        };
        let dismissed_token = match notification.Dismissed(&TypedEventHandler::<
            ToastNotification,
            ToastDismissedEventArgs,
        >::new(move |_, _| {
            sink(notification_id, ToastCallbackKind::Dismissed);
            Ok(())
        })) {
            Ok(token) => token,
            Err(_) => {
                let _ = notification.RemoveFailed(failed_token);
                let _ = notification.RemoveActivated(activated_token);
                return Err(ToastError::Platform);
            }
        };
        Ok(WinRtToastHandle {
            notification,
            activated_token,
            failed_token,
            dismissed_token,
        })
    }

    fn show(&self, handle: &Self::Handle) -> Result<(), ToastError> {
        self.notifier()?
            .Show(&handle.notification)
            .map_err(|_| ToastError::Platform)
    }

    fn hide(&self, handle: &Self::Handle) -> Result<(), ToastError> {
        self.notifier()?
            .Hide(&handle.notification)
            .map_err(|_| ToastError::Platform)
    }

    fn cleanup(&self, handle: &Self::Handle) {
        let _ = handle.notification.RemoveDismissed(handle.dismissed_token);
        let _ = handle.notification.RemoveFailed(handle.failed_token);
        let _ = handle.notification.RemoveActivated(handle.activated_token);
    }
}

impl Drop for WinRtToastBackend {
    fn drop(&mut self) {
        drop(self.notifier.take());
        if self.initialized {
            unsafe { RoUninitialize() };
            self.initialized = false;
        }
    }
}

enum ToastPortState {
    Uninitialized(BuildIdentity),
    Ready(ToastController<WinRtToastBackend>),
    Disabled,
}

pub(crate) struct WindowsToastPort {
    state: Mutex<ToastPortState>,
    callback_sink: OnceLock<ToastCallbackSink>,
}

impl WindowsToastPort {
    pub(crate) fn new() -> Self {
        let state = match windows_identity::prepare_shortcut() {
            Ok(identity) => ToastPortState::Uninitialized(identity),
            Err(error) => {
                eprintln!("[native-attention] Toast identity unavailable: {error:?}");
                ToastPortState::Disabled
            }
        };
        Self {
            state: Mutex::new(state),
            callback_sink: OnceLock::new(),
        }
    }

    fn with_controller<T>(
        &self,
        action: impl FnOnce(&mut ToastController<WinRtToastBackend>) -> Result<T, ToastError>,
    ) -> Result<T, ToastError> {
        let mut state = self.state.lock().map_err(|_| ToastError::Platform)?;
        let identity = match &*state {
            ToastPortState::Uninitialized(identity) => Some(*identity),
            ToastPortState::Ready(_) | ToastPortState::Disabled => None,
        };
        if let Some(identity) = identity {
            match WinRtToastBackend::new(identity) {
                Ok(backend) => *state = ToastPortState::Ready(ToastController::new(backend)),
                Err(error) => {
                    *state = ToastPortState::Disabled;
                    eprintln!("[native-attention] Toast worker initialization failed");
                    return Err(error);
                }
            }
        }
        match &mut *state {
            ToastPortState::Ready(controller) => action(controller),
            ToastPortState::Uninitialized(_) | ToastPortState::Disabled => {
                Err(ToastError::Disabled)
            }
        }
    }
}

impl MessageToast for WindowsToastPort {
    fn initialize_worker(&self) -> Result<(), NativeEffectError> {
        self.with_controller(|_| Ok(()))
            .map_err(|_| NativeEffectError)
    }

    fn show_message(&self, message: &MessagePublished) -> Result<(), NativeEffectError> {
        let sink = self.callback_sink.get().cloned().ok_or(NativeEffectError)?;
        self.with_controller(|controller| controller.show(message, sink))
            .map_err(|_| NativeEffectError)
    }

    fn install_callback_sink(&self, sink: ToastCallbackSink) -> Result<(), NativeEffectError> {
        self.callback_sink.set(sink).map_err(|_| NativeEffectError)
    }

    fn finish_notification(&self, notification_id: u64) {
        let _ = self.with_controller(|controller| {
            controller.finish(notification_id);
            Ok(())
        });
    }

    fn shutdown(&self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let previous = std::mem::replace(&mut *state, ToastPortState::Disabled);
        drop(state);
        if let ToastPortState::Ready(mut controller) = previous {
            controller.shutdown();
        }
    }
}

fn parse_notification_id(value: &str) -> Result<NativeNotificationId, ToastError> {
    if value.is_empty() || (value.len() > 1 && value.starts_with('0')) {
        return Err(ToastError::InvalidMessageId);
    }
    value.parse().map_err(|_| ToastError::InvalidMessageId)
}

fn build_toast_document(plugin_name: &str, content: &str) -> Result<XmlDocument, ToastError> {
    let document = XmlDocument::new().map_err(|_| ToastError::Platform)?;
    document
        .LoadXml(&HSTRING::from(TOAST_TEMPLATE))
        .map_err(|_| ToastError::Platform)?;
    let text_nodes = document
        .GetElementsByTagName(&HSTRING::from("text"))
        .map_err(|_| ToastError::Platform)?;
    for (index, value) in [plugin_name, content].into_iter().enumerate() {
        let target = text_nodes
            .Item(index as u32)
            .map_err(|_| ToastError::Platform)?;
        let text = document
            .CreateTextNode(&HSTRING::from(value))
            .map_err(|_| ToastError::Platform)?;
        target
            .AppendChild(&text)
            .map_err(|_| ToastError::Platform)?;
    }
    Ok(document)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            Arc, Mutex,
        },
    };

    use windows::{core::HSTRING, Data::Xml::Dom::XmlDocument};

    use super::{
        build_toast_document, ToastBackend, ToastCallbackKind, ToastCallbackSink, ToastController,
        ToastError,
    };
    use crate::message_center::MessagePublished;

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct FakeHandle(u64);

    #[derive(Default)]
    struct FakeBackend {
        enabled: AtomicBool,
        fail_setting: AtomicBool,
        fail_show: AtomicBool,
        fail_hide: AtomicBool,
        next_handle: AtomicU64,
        sinks: Mutex<HashMap<u64, ToastCallbackSink>>,
        calls: Mutex<Vec<String>>,
    }

    impl FakeBackend {
        fn enabled() -> Self {
            Self {
                enabled: AtomicBool::new(true),
                ..Self::default()
            }
        }

        fn trigger(&self, id: u64, kind: ToastCallbackKind) {
            self.sinks.lock().unwrap().get(&id).unwrap()(id, kind);
        }
    }

    impl ToastBackend for Arc<FakeBackend> {
        type Handle = FakeHandle;

        fn enabled(&self) -> Result<bool, ToastError> {
            self.calls.lock().unwrap().push("setting".into());
            if self.fail_setting.load(Ordering::SeqCst) {
                return Err(ToastError::Platform);
            }
            Ok(self.enabled.load(Ordering::SeqCst))
        }

        fn prepare(
            &self,
            _document: XmlDocument,
            notification_id: u64,
            sink: ToastCallbackSink,
        ) -> Result<Self::Handle, ToastError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("prepare:{notification_id}"));
            self.sinks.lock().unwrap().insert(notification_id, sink);
            Ok(FakeHandle(self.next_handle.fetch_add(1, Ordering::SeqCst)))
        }

        fn show(&self, _handle: &Self::Handle) -> Result<(), ToastError> {
            self.calls.lock().unwrap().push("show".into());
            (!self.fail_show.load(Ordering::SeqCst))
                .then_some(())
                .ok_or(ToastError::Platform)
        }

        fn hide(&self, _handle: &Self::Handle) -> Result<(), ToastError> {
            self.calls.lock().unwrap().push("hide".into());
            (!self.fail_hide.load(Ordering::SeqCst))
                .then_some(())
                .ok_or(ToastError::Platform)
        }

        fn cleanup(&self, _handle: &Self::Handle) {
            self.calls.lock().unwrap().push("cleanup".into());
        }
    }

    fn message(id: &str) -> MessagePublished {
        MessagePublished {
            id: id.into(),
            plugin_id: "com.example.toast".into(),
            plugin_name_snapshot: "Toast".into(),
            created_at: "2026-08-21T00:00:00Z".into(),
            content: "message".into(),
            revision: id.into(),
            unread_count: 1,
        }
    }

    #[test]
    fn fixed_dom_keeps_untrusted_values_as_text_without_launch_or_actions() {
        let plugin = "Plugin <>&\"' </text><actions>";
        let content = "Message <>&\"' </text><actions><action launch='fake'/>";
        let document = build_toast_document(plugin, content).unwrap();
        let nodes = document
            .GetElementsByTagName(&HSTRING::from("text"))
            .unwrap();

        assert_eq!(
            nodes.Item(0).unwrap().InnerText().unwrap().to_string(),
            plugin
        );
        assert_eq!(
            nodes.Item(1).unwrap().InnerText().unwrap().to_string(),
            content
        );
        assert_eq!(
            document
                .GetElementsByTagName(&HSTRING::from("actions"))
                .unwrap()
                .Length()
                .unwrap(),
            0
        );
        assert!(document
            .DocumentElement()
            .unwrap()
            .GetAttribute(&HSTRING::from("launch"))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn disabled_notifications_stop_before_prepare() {
        let backend = Arc::new(FakeBackend::default());
        let mut controller = ToastController::new(Arc::clone(&backend));

        assert_eq!(
            controller.show(&message("1"), Arc::new(|_, _| {})),
            Err(ToastError::Disabled)
        );
        assert_eq!(*backend.calls.lock().unwrap(), ["setting"]);
    }

    #[test]
    fn setting_query_failure_still_attempts_to_show() {
        let backend = Arc::new(FakeBackend::enabled());
        backend.fail_setting.store(true, Ordering::SeqCst);
        let mut controller = ToastController::new(Arc::clone(&backend));

        controller.show(&message("4"), Arc::new(|_, _| {})).unwrap();

        assert_eq!(
            *backend.calls.lock().unwrap(),
            ["setting", "prepare:4", "show"]
        );
    }

    #[test]
    fn show_failure_cleans_prepared_handle_and_callback_only_enqueues() {
        let backend = Arc::new(FakeBackend::enabled());
        backend.fail_show.store(true, Ordering::SeqCst);
        let mut controller = ToastController::new(Arc::clone(&backend));

        assert_eq!(
            controller.show(&message("2"), Arc::new(|_, _| {})),
            Err(ToastError::Platform)
        );
        assert_eq!(
            *backend.calls.lock().unwrap(),
            ["setting", "prepare:2", "show", "cleanup"]
        );

        backend.fail_show.store(false, Ordering::SeqCst);
        let callbacks = Arc::new(Mutex::new(Vec::new()));
        let callback_log = Arc::clone(&callbacks);
        controller
            .show(
                &message("3"),
                Arc::new(move |id, kind| callback_log.lock().unwrap().push((id, kind))),
            )
            .unwrap();
        backend.trigger(3, ToastCallbackKind::Activated);
        assert_eq!(
            *callbacks.lock().unwrap(),
            [(3, ToastCallbackKind::Activated)]
        );
        assert_eq!(
            backend
                .calls
                .lock()
                .unwrap()
                .iter()
                .filter(|call| call.as_str() == "cleanup")
                .count(),
            1
        );
        controller.finish(3);
        assert_eq!(
            backend
                .calls
                .lock()
                .unwrap()
                .iter()
                .filter(|call| call.as_str() == "cleanup")
                .count(),
            2
        );
    }

    #[test]
    fn shutdown_hides_and_cleans_every_active_handle_after_hide_failure() {
        let backend = Arc::new(FakeBackend::enabled());
        backend.fail_hide.store(true, Ordering::SeqCst);
        let mut controller = ToastController::new(Arc::clone(&backend));
        controller.show(&message("4"), Arc::new(|_, _| {})).unwrap();
        controller.show(&message("5"), Arc::new(|_, _| {})).unwrap();

        controller.shutdown();

        let calls = backend.calls.lock().unwrap();
        assert_eq!(
            calls.iter().filter(|call| call.as_str() == "hide").count(),
            2
        );
        assert_eq!(
            calls
                .iter()
                .filter(|call| call.as_str() == "cleanup")
                .count(),
            2
        );
    }

    #[test]
    fn toast_backend_balances_mta_after_dropping_the_notifier() {
        let source = include_str!("windows_toast.rs");
        let initialize = source.find("RoInitialize(RO_INIT_MULTITHREADED)").unwrap();
        let create = source.find("CreateToastNotifierWithId").unwrap();
        let drop_notifier = source.find("drop(self.notifier.take())").unwrap();
        let uninitialize = source.rfind("RoUninitialize()").unwrap();

        assert!(initialize < create && create < drop_notifier && drop_notifier < uninitialize);
    }
}
