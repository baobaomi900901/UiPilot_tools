mod store;

use std::path::Path;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use store::{MessageStore, MessageStoreError, PublishInput};

pub(crate) const MESSAGE_STATE_CHANGED_EVENT: &str = "message-center://state-changed";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MessagePublishRequest {
    pub(crate) plugin_id: String,
    pub(crate) plugin_name_snapshot: String,
    pub(crate) content: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MessagePublished {
    pub(crate) id: String,
    pub(crate) plugin_id: String,
    pub(crate) plugin_name_snapshot: String,
    pub(crate) created_at: String,
    pub(crate) content: String,
    pub(crate) revision: String,
    pub(crate) unread_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum MessagePublishOutcome {
    Published(MessagePublished),
    OperationFailed,
    Unavailable,
    BecameUnavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum MessagePostGuardEffect {
    Published(MessagePublished),
    Ready(MessageSummary),
    BecameUnavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MessageSummary {
    pub(crate) revision: String,
    pub(crate) unread_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MessageRecordSnapshot {
    pub(crate) id: String,
    pub(crate) plugin_id: String,
    pub(crate) plugin_name_snapshot: String,
    pub(crate) created_at: String,
    pub(crate) content: String,
    pub(crate) read_at: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MessageCenterSnapshot {
    pub(crate) revision: String,
    pub(crate) unread_count: usize,
    pub(crate) messages: Vec<MessageRecordSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MessageCenterExecution<T> {
    pub(crate) result: Result<T, MessageCenterError>,
    pub(crate) post_guard_effect: Option<MessagePostGuardEffect>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MessageCenterError {
    OperationFailed,
    Unavailable,
}

pub(crate) trait MessagePublisher: Send + Sync {
    fn commit_publish(&self, request: MessagePublishRequest) -> MessagePublishOutcome;
}

pub(crate) struct MessageCenterService {
    store: MessageStore,
}

impl MessageCenterService {
    pub(crate) fn load(app_data_dir: &Path) -> Self {
        Self {
            store: MessageStore::load(&app_data_dir.join("message-center")),
        }
    }

    pub(crate) fn summary(&self) -> Result<MessageSummary, MessageCenterError> {
        self.store
            .summary()
            .map(|summary| MessageSummary {
                revision: summary.revision,
                unread_count: summary.unread_count,
            })
            .map_err(map_store_error)
    }

    pub(crate) fn open_and_mark_read(&self) -> MessageCenterExecution<MessageCenterSnapshot> {
        map_mutation(self.store.open_and_mark_read())
    }

    pub(crate) fn read_snapshot(&self) -> Result<MessageCenterSnapshot, MessageCenterError> {
        self.store
            .read_snapshot()
            .map(map_snapshot)
            .map_err(map_store_error)
    }

    pub(crate) fn clear(&self) -> MessageCenterExecution<MessageCenterSnapshot> {
        map_mutation(self.store.clear())
    }

    pub(crate) fn dispatch_post_guard(
        &self,
        app: &AppHandle,
        effect: Option<MessagePostGuardEffect>,
    ) {
        let event = match effect {
            Some(MessagePostGuardEffect::Published(message)) => {
                MessageHostStateChangedEvent::Ready {
                    revision: message.revision,
                    unread_count: message.unread_count,
                }
            }
            Some(MessagePostGuardEffect::Ready(summary)) => MessageHostStateChangedEvent::Ready {
                revision: summary.revision,
                unread_count: summary.unread_count,
            },
            Some(MessagePostGuardEffect::BecameUnavailable) => {
                MessageHostStateChangedEvent::Unavailable {
                    error: "MessageStoreUnavailable",
                }
            }
            None => return,
        };
        let _ = app.emit_to("main", MESSAGE_STATE_CHANGED_EVENT, event);
    }
}

impl MessagePublisher for MessageCenterService {
    fn commit_publish(&self, request: MessagePublishRequest) -> MessagePublishOutcome {
        match self.store.publish(PublishInput {
            plugin_id: request.plugin_id,
            plugin_name_snapshot: request.plugin_name_snapshot,
            content: request.content,
        }) {
            Ok(commit) => MessagePublishOutcome::Published(MessagePublished {
                id: commit.record.id,
                plugin_id: commit.record.plugin_id,
                plugin_name_snapshot: commit.record.plugin_name_snapshot,
                created_at: commit.record.created_at,
                content: commit.record.content,
                revision: commit.summary.revision,
                unread_count: commit.summary.unread_count,
            }),
            Err(MessageStoreError::OperationFailed) => MessagePublishOutcome::OperationFailed,
            Err(MessageStoreError::Unavailable) => MessagePublishOutcome::Unavailable,
            Err(MessageStoreError::BecameUnavailable) => MessagePublishOutcome::BecameUnavailable,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
enum MessageHostStateChangedEvent {
    Ready {
        revision: String,
        unread_count: usize,
    },
    Unavailable {
        error: &'static str,
    },
}

fn map_store_error(error: MessageStoreError) -> MessageCenterError {
    match error {
        MessageStoreError::OperationFailed => MessageCenterError::OperationFailed,
        MessageStoreError::BecameUnavailable | MessageStoreError::Unavailable => {
            MessageCenterError::Unavailable
        }
    }
}

fn map_mutation(
    result: Result<store::MessageSnapshot, MessageStoreError>,
) -> MessageCenterExecution<MessageCenterSnapshot> {
    match result {
        Ok(snapshot) => {
            let changed = snapshot.changed;
            let snapshot = map_snapshot(snapshot);
            let effect = changed.then(|| {
                MessagePostGuardEffect::Ready(MessageSummary {
                    revision: snapshot.revision.clone(),
                    unread_count: snapshot.unread_count,
                })
            });
            MessageCenterExecution {
                result: Ok(snapshot),
                post_guard_effect: effect,
            }
        }
        Err(MessageStoreError::BecameUnavailable) => MessageCenterExecution {
            result: Err(MessageCenterError::Unavailable),
            post_guard_effect: Some(MessagePostGuardEffect::BecameUnavailable),
        },
        Err(error) => MessageCenterExecution {
            result: Err(map_store_error(error)),
            post_guard_effect: None,
        },
    }
}

fn map_snapshot(snapshot: store::MessageSnapshot) -> MessageCenterSnapshot {
    MessageCenterSnapshot {
        revision: snapshot.revision,
        unread_count: snapshot.unread_count,
        messages: snapshot
            .messages
            .into_iter()
            .map(|record| MessageRecordSnapshot {
                id: record.id,
                plugin_id: record.plugin_id,
                plugin_name_snapshot: record.plugin_name_snapshot,
                created_at: record.created_at,
                content: record.content,
                read_at: record.read_at,
            })
            .collect(),
    }
}

#[cfg(test)]
mod store_tests;

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "uipilot-message-service-{label}-{}-{id}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            if self.0.exists() {
                fs::remove_dir_all(&self.0).unwrap();
            }
        }
    }

    fn publish(service: &MessageCenterService) -> MessagePublishOutcome {
        service.commit_publish(MessagePublishRequest {
            plugin_id: "com.example.messages".into(),
            plugin_name_snapshot: "Messages".into(),
            content: "hello".into(),
        })
    }

    #[test]
    fn message_center_management_effects_exist_only_for_committed_changes() {
        let dir = TestDir::new("management-effects");
        let service = MessageCenterService::load(dir.path());
        assert!(matches!(
            publish(&service),
            MessagePublishOutcome::Published(_)
        ));

        let opened = service.open_and_mark_read();
        assert_eq!(opened.result.as_ref().unwrap().revision, "2");
        assert_eq!(opened.result.as_ref().unwrap().unread_count, 0);
        assert!(matches!(
            opened.post_guard_effect,
            Some(MessagePostGuardEffect::Ready(MessageSummary {
                revision,
                unread_count: 0,
            })) if revision == "2"
        ));

        let reopened = service.open_and_mark_read();
        assert_eq!(reopened.result.as_ref().unwrap().revision, "2");
        assert_eq!(reopened.post_guard_effect, None);
        assert_eq!(service.read_snapshot().unwrap().messages.len(), 1);

        let cleared = service.clear();
        assert_eq!(cleared.result.as_ref().unwrap().revision, "3");
        assert!(cleared.result.as_ref().unwrap().messages.is_empty());
        assert!(matches!(
            cleared.post_guard_effect,
            Some(MessagePostGuardEffect::Ready(MessageSummary {
                revision,
                unread_count: 0,
            })) if revision == "3"
        ));

        let cleared_again = service.clear();
        assert_eq!(cleared_again.result.as_ref().unwrap().revision, "3");
        assert_eq!(cleared_again.post_guard_effect, None);
    }

    #[test]
    fn message_center_corruption_is_unavailable_without_a_false_ready_effect() {
        let dir = TestDir::new("corrupt");
        let root = dir.path().join("message-center");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("messages.json"), b"bad-current").unwrap();
        fs::write(root.join("messages.json.backup"), b"bad-backup").unwrap();
        let service = MessageCenterService::load(dir.path());

        assert_eq!(service.summary(), Err(MessageCenterError::Unavailable));
        let opened = service.open_and_mark_read();
        assert_eq!(opened.result, Err(MessageCenterError::Unavailable));
        assert_eq!(opened.post_guard_effect, None);
    }
}
