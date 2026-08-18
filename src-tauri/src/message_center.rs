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
    BecameUnavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MessageSummary {
    pub(crate) revision: String,
    pub(crate) unread_count: usize,
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

#[cfg(test)]
mod store_tests;
