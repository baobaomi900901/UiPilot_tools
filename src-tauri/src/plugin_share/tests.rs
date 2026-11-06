use super::{catalog::*, coordinator::ShareCoordinator, payload::*, types::*, ShareError};
use serde_json::json;
use std::time::{Duration, Instant};

#[derive(Default)]
struct Ids(u64);
impl IdSource for Ids {
    fn next(&mut self) -> Result<Ticket, ShareError> {
        self.0 += 1;
        Ok(Ticket(format!("test-{}", self.0)))
    }
}
fn owner() -> Owner {
    Owner {
        main_webview: "main".into(),
        document_nonce: "main-doc".into(),
        navigation_epoch: 1,
    }
}
fn doc(id: &str) -> Document {
    Document {
        plugin: PluginIdentity {
            plugin_id: id.into(),
            generation: 1,
            activation_id: 1,
            admission_epoch: 1,
            inventory_revision: 1,
        },
        webview_label: format!("panel-{id}"),
        nonce: format!("nonce-{id}"),
        epoch: if id == "source" { 1 } else { 2 },
    }
}
fn init() -> Initialization {
    Initialization {
        request_id: "request-1".into(),
        update_sequence: 1,
    }
}
fn receiver(id: &str) -> Receiver {
    Receiver {
        plugin: doc(id).plugin,
        plugin_name: "same name".into(),
        receiver_id: "receive".into(),
        title: "Text".into(),
        command_id: "default".into(),
        max_bytes: 8192,
    }
}
fn payload(text: &str) -> Payload {
    serde_json::from_value(json!({"type":"text/plain","text":text})).unwrap()
}
fn effects(c: &mut ShareCoordinator<Ids>) -> Vec<Effect> {
    c.take_effects().into_iter().map(|i| i.effect).collect()
}
fn text(p: &Payload) -> String {
    serde_json::to_value(p).unwrap()["text"]
        .as_str()
        .unwrap()
        .into()
}
struct Flow {
    c: ShareCoordinator<Ids>,
    now: Instant,
    id: Ticket,
    selector: Ticket,
    source: Document,
    target: Document,
    receiver: Receiver,
}
impl Flow {
    fn choosing() -> Self {
        let now = Instant::now();
        let source = doc("source");
        let target = doc("target");
        let mut c = ShareCoordinator::new(Ids::default());
        c.publish_source(source.clone(), "1", now).unwrap();
        let menu = c.menu_ticket(owner(), &source, now).unwrap();
        c.request_snapshot(&owner(), &menu, now).unwrap();
        let (id, selector) = c
            .snapshot(&source, &menu, "1", payload(" edited\n中文 "), now)
            .unwrap();
        Self {
            c,
            now,
            id,
            selector,
            source,
            target,
            receiver: receiver("target"),
        }
    }
    fn preparing() -> Self {
        let mut f = Self::choosing();
        f.c.select(
            &owner(),
            &f.id,
            &f.selector,
            &f.receiver,
            Some(&f.receiver),
            f.target.clone(),
            f.now,
        )
        .unwrap();
        f.c.bind_initialization(&f.id, &f.target, init(), f.now)
            .unwrap();
        f
    }
    fn ready() -> Self {
        let mut f = Self::preparing();
        f.c.prepared(&f.id, &f.target, &init(), f.now).unwrap();
        f.c.begin_step(&owner(), &f.id, PrepareStep::SourceConfirmation, f.now)
            .unwrap();
        f.c.confirm_source(&f.id, &f.source, "1", payload(" edited\n中文 "), f.now)
            .unwrap();
        f.c.begin_step(&owner(), &f.id, PrepareStep::NativeSwitch, f.now)
            .unwrap();
        f.c.native_prepared(&owner(), &f.id, &f.target, &init(), f.now)
            .unwrap();
        f
    }
    fn commit(&mut self) -> Result<CommitTransition, ShareError> {
        self.c.commit(
            &self.id,
            CommitView {
                owner: &owner(),
                active_source: &self.source,
                pending_target: &self.target,
                receiver: &self.receiver,
                main_visible: true,
                initialization: &init(),
            },
            self.now,
        )
    }
    fn committed() -> Self {
        let mut f = Self::ready();
        f.commit().unwrap();
        f
    }
    fn state(&self) -> State {
        self.c.transaction.as_ref().unwrap().state
    }
    fn has_body(&self) -> bool {
        self.c.transaction.as_ref().unwrap().body.is_some()
    }
    fn later(&self, secs: u64) -> Instant {
        self.now + Duration::from_secs(secs)
    }
}

mod catalog;
mod delivery;
mod selection;
