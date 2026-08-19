use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use windows::{
    core::{IInspectable, HSTRING},
    Data::Xml::Dom::XmlDocument,
    Foundation::TypedEventHandler,
    UI::Notifications::{
        NotificationSetting, ToastDismissedEventArgs, ToastFailedEventArgs, ToastNotification,
        ToastNotificationManager, ToastNotifier,
    },
};

use super::{MessagePublished, MessageToast, NativeEffectError};

const UIPILOT_AUMID: &str = "com.uipilot.launcher";
const TOAST_TEMPLATE: &str =
    r#"<toast><visual><binding template="ToastGeneric"><text/><text/></binding></visual></toast>"#;
const TOAST_ROUTE_PREFIX: &str = "uipilot:messages:";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ToastError {
    Disabled,
    InvalidMessageId,
    Platform,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ToastEvent {
    Activated,
    Failed,
    Dismissed,
}

type ToastEventSink = Arc<dyn Fn(ToastEvent) + Send + Sync>;

trait ToastBackend: Send + Sync + 'static {
    type Handle: Clone + Send + Sync + 'static;

    fn enabled(&self) -> Result<bool, ToastError>;
    fn prepare(
        &self,
        document: XmlDocument,
        id: &str,
        sink: ToastEventSink,
    ) -> Result<Self::Handle, ToastError>;
    fn show(&self, handle: &Self::Handle) -> Result<(), ToastError>;
    fn hide(&self, handle: &Self::Handle) -> Result<(), ToastError>;
    fn cleanup(&self, handle: &Self::Handle);
}

struct ToastControllerInner<B: ToastBackend> {
    backend: B,
    active: Mutex<HashMap<String, B::Handle>>,
    route_messages: Arc<dyn Fn() + Send + Sync>,
}

impl<B: ToastBackend> ToastControllerInner<B> {
    fn finish(&self, id: &str, event: ToastEvent) {
        let handle = self.active.lock().expect("toast lock poisoned").remove(id);
        if let Some(handle) = handle {
            self.backend.cleanup(&handle);
            if event == ToastEvent::Activated {
                (self.route_messages)();
            } else if event == ToastEvent::Failed {
                eprintln!("[message-center] Windows notification failed asynchronously");
            }
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
    notifier: ToastNotifier,
}

impl WinRtToastBackend {
    fn new() -> Result<Self, ToastError> {
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(UIPILOT_AUMID))
            .map(|notifier| Self { notifier })
            .map_err(|_| ToastError::Platform)
    }
}

impl ToastBackend for WinRtToastBackend {
    type Handle = WinRtToastHandle;

    fn enabled(&self) -> Result<bool, ToastError> {
        self.notifier
            .Setting()
            .map(|setting| setting == NotificationSetting::Enabled)
            .map_err(|_| ToastError::Platform)
    }

    fn prepare(
        &self,
        document: XmlDocument,
        _id: &str,
        sink: ToastEventSink,
    ) -> Result<Self::Handle, ToastError> {
        let notification = ToastNotification::CreateToastNotification(&document)
            .map_err(|_| ToastError::Platform)?;

        let activated_sink = Arc::clone(&sink);
        let activated_token = notification
            .Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(
                move |_, _| {
                    activated_sink(ToastEvent::Activated);
                    Ok(())
                },
            ))
            .map_err(|_| ToastError::Platform)?;

        let failed_sink = Arc::clone(&sink);
        let failed_token = match notification.Failed(&TypedEventHandler::<
            ToastNotification,
            ToastFailedEventArgs,
        >::new(move |_, _| {
            failed_sink(ToastEvent::Failed);
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
            sink(ToastEvent::Dismissed);
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
        self.notifier
            .Show(&handle.notification)
            .map_err(|_| ToastError::Platform)
    }

    fn hide(&self, handle: &Self::Handle) -> Result<(), ToastError> {
        self.notifier
            .Hide(&handle.notification)
            .map_err(|_| ToastError::Platform)
    }

    fn cleanup(&self, handle: &Self::Handle) {
        let _ = handle.notification.RemoveDismissed(handle.dismissed_token);
        let _ = handle.notification.RemoveFailed(handle.failed_token);
        let _ = handle.notification.RemoveActivated(handle.activated_token);
    }
}

pub(crate) struct WindowsNotificationAdapter {
    controller: Option<ToastController<WinRtToastBackend>>,
}

impl WindowsNotificationAdapter {
    pub(crate) fn new(route_messages: Arc<dyn Fn() + Send + Sync>) -> Self {
        let controller = WinRtToastBackend::new()
            .ok()
            .map(|backend| ToastController::new(backend, route_messages));
        Self { controller }
    }
}

impl MessageToast for WindowsNotificationAdapter {
    fn show_message(&self, message: &MessagePublished) -> Result<(), NativeEffectError> {
        self.controller
            .as_ref()
            .ok_or(NativeEffectError)?
            .show(&message.plugin_name_snapshot, &message.content, &message.id)
            .map_err(|_| NativeEffectError)
    }

    fn shutdown(&self) {
        if let Some(controller) = self.controller.as_ref() {
            controller.shutdown();
        }
    }
}

struct ToastController<B: ToastBackend> {
    inner: Arc<ToastControllerInner<B>>,
}

impl<B: ToastBackend> ToastController<B> {
    fn new(backend: B, route_messages: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self {
            inner: Arc::new(ToastControllerInner {
                backend,
                active: Mutex::new(HashMap::new()),
                route_messages,
            }),
        }
    }

    fn show(&self, plugin_name: &str, content: &str, id: &str) -> Result<(), ToastError> {
        if !self.inner.backend.enabled()? {
            return Err(ToastError::Disabled);
        }
        let document = build_toast_document(plugin_name, content, id)?;
        let weak = Arc::downgrade(&self.inner);
        let event_id = id.to_owned();
        let sink: ToastEventSink = Arc::new(move |event| {
            if let Some(inner) = weak.upgrade() {
                inner.finish(&event_id, event);
            }
        });
        let handle = self.inner.backend.prepare(document, id, sink)?;
        self.inner
            .active
            .lock()
            .expect("toast lock poisoned")
            .insert(id.to_owned(), handle.clone());
        if let Err(error) = self.inner.backend.show(&handle) {
            let handle = self
                .inner
                .active
                .lock()
                .expect("toast lock poisoned")
                .remove(id);
            if let Some(handle) = handle {
                self.inner.backend.cleanup(&handle);
            }
            return Err(error);
        }
        Ok(())
    }

    fn shutdown(&self) {
        let handles = self
            .inner
            .active
            .lock()
            .expect("toast lock poisoned")
            .drain()
            .map(|(_, handle)| handle)
            .collect::<Vec<_>>();
        for handle in handles {
            if self.inner.backend.hide(&handle).is_err() {
                eprintln!("[message-center] Windows notification cancellation failed");
            }
            self.inner.backend.cleanup(&handle);
        }
    }
}

fn canonical_message_id(value: &str) -> bool {
    if value.is_empty() || (value.len() > 1 && value.starts_with('0')) {
        return false;
    }
    value.bytes().all(|byte| byte.is_ascii_digit()) && value.parse::<u64>().is_ok()
}

pub(crate) fn build_toast_document(
    plugin_name: &str,
    content: &str,
    message_id: &str,
) -> Result<XmlDocument, ToastError> {
    if !canonical_message_id(message_id) {
        return Err(ToastError::InvalidMessageId);
    }

    let document = XmlDocument::new().map_err(|_| ToastError::Platform)?;
    document
        .LoadXml(&HSTRING::from(TOAST_TEMPLATE))
        .map_err(|_| ToastError::Platform)?;
    document
        .DocumentElement()
        .map_err(|_| ToastError::Platform)?
        .SetAttribute(
            &HSTRING::from("launch"),
            &HSTRING::from(format!("{TOAST_ROUTE_PREFIX}{message_id}")),
        )
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

    use windows::core::HSTRING;

    use super::{
        build_toast_document, ToastBackend, ToastController, ToastError, ToastEvent, ToastEventSink,
    };

    #[derive(Clone, Debug, Eq, Hash, PartialEq)]
    struct FakeHandle(u64);

    #[derive(Default)]
    struct FakeBackend {
        enabled: AtomicBool,
        fail_show: AtomicBool,
        fail_hide: AtomicBool,
        next_handle: AtomicU64,
        sinks: Mutex<HashMap<String, ToastEventSink>>,
        calls: Mutex<Vec<String>>,
    }

    impl FakeBackend {
        fn enabled() -> Arc<Self> {
            Arc::new(Self {
                enabled: AtomicBool::new(true),
                ..Self::default()
            })
        }

        fn trigger(&self, id: &str, event: ToastEvent) {
            let sink = self.sinks.lock().unwrap().get(id).cloned().unwrap();
            sink(event);
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl ToastBackend for Arc<FakeBackend> {
        type Handle = FakeHandle;

        fn enabled(&self) -> Result<bool, ToastError> {
            self.calls.lock().unwrap().push("setting".into());
            Ok(self.enabled.load(Ordering::SeqCst))
        }

        fn prepare(
            &self,
            _document: windows::Data::Xml::Dom::XmlDocument,
            id: &str,
            sink: ToastEventSink,
        ) -> Result<Self::Handle, ToastError> {
            self.calls.lock().unwrap().push(format!("prepare:{id}"));
            self.sinks.lock().unwrap().insert(id.into(), sink);
            Ok(FakeHandle(self.next_handle.fetch_add(1, Ordering::SeqCst)))
        }

        fn show(&self, _handle: &Self::Handle) -> Result<(), ToastError> {
            self.calls.lock().unwrap().push("show".into());
            if self.fail_show.load(Ordering::SeqCst) {
                Err(ToastError::Platform)
            } else {
                Ok(())
            }
        }

        fn hide(&self, _handle: &Self::Handle) -> Result<(), ToastError> {
            self.calls.lock().unwrap().push("hide".into());
            if self.fail_hide.load(Ordering::SeqCst) {
                Err(ToastError::Platform)
            } else {
                Ok(())
            }
        }

        fn cleanup(&self, _handle: &Self::Handle) {
            self.calls.lock().unwrap().push("cleanup".into());
        }
    }

    fn node_text(document: &windows::Data::Xml::Dom::XmlDocument, index: u32) -> String {
        document
            .GetElementsByTagName(&HSTRING::from("text"))
            .unwrap()
            .Item(index)
            .unwrap()
            .InnerText()
            .unwrap()
            .to_string()
    }

    #[test]
    fn toast_dom_keeps_untrusted_text_out_of_markup_and_route() {
        let plugin_name = "Plugin <>&\"' </text><actions>";
        let content = "Message <>&\"' </text><actions><action launch='fake'/>";
        let document = build_toast_document(plugin_name, content, "18446744073709551615")
            .expect("fixed toast document should build");

        assert_eq!(node_text(&document, 0), plugin_name);
        assert_eq!(node_text(&document, 1), content);
        assert_eq!(
            document
                .GetElementsByTagName(&HSTRING::from("actions"))
                .unwrap()
                .Length()
                .unwrap(),
            0
        );
        assert_eq!(
            document
                .DocumentElement()
                .unwrap()
                .GetAttribute(&HSTRING::from("launch"))
                .unwrap()
                .to_string(),
            "uipilot:messages:18446744073709551615"
        );
    }

    #[test]
    fn disabled_notifications_stop_before_prepare() {
        let backend = Arc::new(FakeBackend::default());
        let controller = ToastController::new(Arc::clone(&backend), Arc::new(|| {}));

        assert_eq!(
            controller.show("Plugin", "message", "1"),
            Err(ToastError::Disabled)
        );
        assert_eq!(backend.calls(), ["setting"]);
    }

    #[test]
    fn synchronous_show_failure_cleans_the_prepared_notification() {
        let backend = FakeBackend::enabled();
        backend.fail_show.store(true, Ordering::SeqCst);
        let controller = ToastController::new(Arc::clone(&backend), Arc::new(|| {}));

        assert_eq!(
            controller.show("Plugin", "message", "2"),
            Err(ToastError::Platform)
        );
        assert_eq!(backend.calls(), ["setting", "prepare:2", "show", "cleanup"]);
    }

    #[test]
    fn activation_routes_once_and_terminal_events_cleanup() {
        let backend = FakeBackend::enabled();
        let routed = Arc::new(AtomicU64::new(0));
        let route = {
            let routed = Arc::clone(&routed);
            Arc::new(move || {
                routed.fetch_add(1, Ordering::SeqCst);
            })
        };
        let controller = ToastController::new(Arc::clone(&backend), route);

        controller.show("Plugin", "first", "3").unwrap();
        backend.trigger("3", ToastEvent::Activated);
        controller.show("Plugin", "second", "4").unwrap();
        backend.trigger("4", ToastEvent::Failed);
        controller.show("Plugin", "third", "5").unwrap();
        backend.trigger("5", ToastEvent::Dismissed);

        assert_eq!(routed.load(Ordering::SeqCst), 1);
        assert_eq!(
            backend
                .calls()
                .into_iter()
                .filter(|call| call == "cleanup")
                .count(),
            3
        );
    }

    #[test]
    fn shutdown_hides_and_cleans_all_active_notifications_even_after_hide_failure() {
        let backend = FakeBackend::enabled();
        backend.fail_hide.store(true, Ordering::SeqCst);
        let controller = ToastController::new(Arc::clone(&backend), Arc::new(|| {}));
        controller.show("Plugin", "first", "6").unwrap();
        controller.show("Plugin", "second", "7").unwrap();

        controller.shutdown();

        let calls = backend.calls();
        assert_eq!(calls.iter().filter(|call| *call == "hide").count(), 2);
        assert_eq!(calls.iter().filter(|call| *call == "cleanup").count(), 2);
    }
}
