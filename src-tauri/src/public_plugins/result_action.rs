//! Main-result business snapshots and private execution outcomes.
use super::{PluginRequestContext, PluginRuntimeError, PublicPluginRoute};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

#[derive(Clone, Debug)]
pub(crate) struct ResultActionLease(Arc<AtomicBool>);
impl Default for ResultActionLease {
    fn default() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }
}
impl PartialEq for ResultActionLease {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for ResultActionLease {}
impl ResultActionLease {
    pub(crate) fn live(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    pub(crate) fn revoke(&self) {
        self.0.store(false, Ordering::Release);
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum PublicResultAction {
    CopyText {
        id: String,
        title: String,
        text: String,
    },
    PasteText {
        id: String,
        title: String,
        text: String,
    },
    OpenUrl {
        id: String,
        title: String,
        url: String,
    },
    Runtime {
        id: String,
        title: String,
        payload: Value,
    },
}
impl PublicResultAction {
    pub(crate) fn id(&self) -> &str {
        match self {
            Self::CopyText { id, .. }
            | Self::PasteText { id, .. }
            | Self::OpenUrl { id, .. }
            | Self::Runtime { id, .. } => id,
        }
    }
    pub(crate) fn title(&self) -> &str {
        match self {
            Self::CopyText { title, .. }
            | Self::PasteText { title, .. }
            | Self::OpenUrl { title, .. }
            | Self::Runtime { title, .. } => title,
        }
    }
    pub(crate) fn is_copy(&self) -> bool {
        matches!(self, Self::CopyText { .. })
    }
}
/// Complete capability requirements of one immutable source snapshot.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ResultActionRequirements {
    pub(crate) copy: bool,
    pub(crate) url: bool,
    pub(crate) paste: bool,
}
#[cfg(test)]
impl From<bool> for ResultActionRequirements {
    fn from(copy: bool) -> Self {
        Self {
            copy,
            url: false,
            paste: false,
        }
    }
}
impl ResultActionRequirements {
    pub(crate) fn actions<'a>(actions: impl Iterator<Item = &'a PublicResultAction>) -> Self {
        let mut requirements = Self::default();
        for action in actions {
            requirements.copy |=
                action.is_copy() || matches!(action, PublicResultAction::PasteText { .. });
            requirements.paste |= matches!(action, PublicResultAction::PasteText { .. });
            requirements.url |= matches!(action, PublicResultAction::OpenUrl { .. });
        }
        requirements
    }
    pub(crate) fn rows(rows: &[PublicActionResult]) -> Self {
        Self::actions(rows.iter().flat_map(|r| r.actions.iter()))
    }
}
/// Strict initial browser destination validation. Do not trim or repair authority.
pub(crate) fn validate_open_url(raw: &str) -> Result<url::Url, ()> {
    if raw.len() > 4096
        || raw
            .chars()
            .any(|c| c.is_whitespace() || matches!(c as u32, 0..=31 | 127..=159) || c == '\\')
    {
        return Err(());
    }
    let prefix = if raw
        .get(..7)
        .is_some_and(|p| p.eq_ignore_ascii_case("http://"))
    {
        7
    } else if raw
        .get(..8)
        .is_some_and(|p| p.eq_ignore_ascii_case("https://"))
    {
        8
    } else {
        return Err(());
    };
    let authority = raw[prefix..].split(['/', '?', '#']).next().ok_or(())?;
    if authority.is_empty() || authority.contains('@') {
        return Err(());
    }
    let url = url::Url::parse(raw).map_err(|_| ())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none_or(str::is_empty)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.as_str().len() > 4096
    {
        return Err(());
    }
    Ok(url)
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublicActionResult {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) subtitle: Option<String>,
    pub(crate) detail: Option<String>,
    pub(crate) actions: Vec<PublicResultAction>,
    pub(crate) default_action_id: Option<String>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResultActionOrigin {
    pub(crate) ticket: crate::result_registry::ExecutionTicket,
    pub(crate) route: PublicPluginRoute,
    pub(crate) query_sequence: u64,
    pub(crate) lease: ResultActionLease,
    pub(crate) business_result_id: String,
    pub(crate) requirements: ResultActionRequirements,
    pub(crate) action: PublicResultAction,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SnapshotClaim {
    Available,
    InFlight,
    Consumed,
}
pub(crate) struct ResultActionSnapshot {
    pub(crate) route: PublicPluginRoute,
    pub(crate) lease: ResultActionLease,
    pub(crate) state: SnapshotClaim,
    pub(crate) rows: HashMap<String, (String, HashMap<String, PublicResultAction>)>,
    pub(crate) context: Option<PluginRequestContext>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ExecuteResultActionRequest {
    pub(crate) request_id: String,
    pub(crate) result_id: String,
    pub(crate) action_id: String,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RetireResultActionsRequest {
    pub(crate) request_id: String,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ResultActionErrorCode {
    ActionBusy,
    ActionInProgress,
    ActionAlreadyExecuted,
    ActionFailed,
    ActionInvalidResponse,
    ActionTimedOut,
    ActionUnavailable,
    ActionCancelled,
    StaleRequest,
    UnknownResult,
    PluginPermissionDenied,
    ClipboardWriteFailed,
    UrlOpenFailed,
    PasteTargetUnavailable,
    PasteInputBusy,
    PasteFailed,
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum ResultActionOutcome {
    Copied,
    UrlOpened,
    PasteSent,
    ResultsUpdated {
        response: crate::model::SearchResponse,
    },
    Failed {
        code: ResultActionErrorCode,
        #[serde(rename = "sourceConsumed")]
        source_consumed: bool,
    },
}
impl ResultActionOutcome {
    pub(crate) fn failed(code: ResultActionErrorCode, source_consumed: bool) -> Self {
        Self::Failed {
            code,
            source_consumed,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResultActionMetadata {
    pub(crate) actions: Vec<ResultActionDisplay>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) default_action_id: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResultActionDisplay {
    pub(crate) action_id: String,
    pub(crate) title: String,
}

// The Runtime service supplies the timeout operation; this is its single observer.
pub(crate) fn prepare_deadline_observer(
    expire: impl FnOnce(std::time::Instant) + Send + 'static,
) -> Result<std::sync::mpsc::Sender<std::time::Instant>, ResultActionErrorCode> {
    let (arm, watch) = std::sync::mpsc::channel::<std::time::Instant>();
    std::thread::Builder::new()
        .name("result-action-deadline".into())
        .spawn(move || {
            if let Ok(deadline) = watch.recv() {
                std::thread::sleep(deadline.saturating_duration_since(std::time::Instant::now()));
                expire(deadline);
            }
        })
        .map_err(|_| ResultActionErrorCode::ActionUnavailable)?;
    Ok(arm)
}
fn control(value: &str) -> bool {
    value.chars().any(|c| matches!(c as u32,0..=31|127..=159))
}
fn exact(value: &serde_json::Map<String, Value>, required: &[&str], optional: &[&str]) -> bool {
    required.iter().all(|k| value.contains_key(*k))
        && value
            .keys()
            .all(|k| required.contains(&k.as_str()) || optional.contains(&k.as_str()))
}
pub(crate) fn parse(
    context: &PluginRequestContext,
    value: Value,
) -> Result<Vec<PublicActionResult>, PluginRuntimeError> {
    let invalid = || PluginRuntimeError::InvalidOperation;
    if !super::content_action::valid_json(&value) {
        return Err(invalid());
    }
    let envelope = value.as_object().ok_or_else(invalid)?;
    if !exact(envelope, &["requestId", "results"], &[])
        || envelope.get("requestId").and_then(Value::as_str) != Some(context.request_id.as_str())
    {
        return Err(invalid());
    }
    let rows = envelope["results"].as_array().ok_or_else(invalid)?;
    if rows.len() > 20 {
        return Err(invalid());
    }
    let mut ids = HashSet::new();
    let mut output = Vec::with_capacity(rows.len());
    for row in rows {
        let row = row.as_object().ok_or_else(invalid)?;
        if !exact(
            row,
            &["id", "title"],
            &[
                "subtitle",
                "detail",
                "defaultAction",
                "actions",
                "defaultActionId",
            ],
        ) {
            return Err(invalid());
        }
        let id = row["id"].as_str().ok_or_else(invalid)?;
        let title = row["title"].as_str().ok_or_else(invalid)?;
        if id.is_empty()
            || id.len() > 128
            || control(id)
            || !ids.insert(id)
            || title.chars().count() > 256
        {
            return Err(invalid());
        }
        let optional =
            |key: &str, limit: usize, bytes: bool| -> Result<Option<String>, PluginRuntimeError> {
                match row.get(key) {
                    None | Some(Value::Null) => Ok(None),
                    Some(Value::String(s))
                        if (if bytes { s.len() } else { s.chars().count() }) <= limit =>
                    {
                        Ok(Some(s.clone()))
                    }
                    _ => Err(invalid()),
                }
            };
        let subtitle = optional("subtitle", 512, false)?;
        let detail = optional("detail", 16 * 1024, true)?;
        let mut actions = Vec::new();
        let default_action_id;
        if let Some(value) = row.get("actions") {
            if row.contains_key("defaultAction") {
                return Err(invalid());
            }
            let values = value.as_array().ok_or_else(invalid)?;
            if values.len() > 8 {
                return Err(invalid());
            }
            let mut action_ids = HashSet::new();
            for action in values {
                let obj = action.as_object().ok_or_else(invalid)?;
                // serde Option is deliberately avoided: payload is required even when null.
                let required = match obj.get("type").and_then(Value::as_str) {
                    Some("copyText" | "pasteText") => &["id", "title", "type", "text"][..],
                    Some("runtime") => &["id", "title", "type", "payload"][..],
                    Some("openUrl") => &["id", "title", "type", "url"][..],
                    _ => return Err(invalid()),
                };
                if !exact(obj, required, &[]) {
                    return Err(invalid());
                }
                let mut action: PublicResultAction =
                    serde_json::from_value(action.clone()).map_err(|_| invalid())?;
                let aid = action.id();
                let atitle = action.title();
                if !(aid.len() <= 32
                    && aid.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
                    && aid
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
                    || !action_ids.insert(aid.to_owned())
                    || atitle.chars().count() > 80
                    || atitle.trim().is_empty()
                    || control(atitle)
                {
                    return Err(invalid());
                }
                if let PublicResultAction::CopyText { text, .. } = &action {
                    if text.len() > 16 * 1024 || text.contains('\0') {
                        return Err(invalid());
                    }
                }
                if let PublicResultAction::PasteText { text, .. } = &action {
                    if text.is_empty() || text.len() > 8192 || text.contains('\0') {
                        return Err(invalid());
                    }
                }
                if let PublicResultAction::OpenUrl { url, .. } = &mut action {
                    *url = validate_open_url(url).map_err(|_| invalid())?.to_string();
                }
                actions.push(action);
            }
            default_action_id = match row.get("defaultActionId") {
                None => None,
                Some(Value::String(s)) if action_ids.contains(s) => Some(s.clone()),
                _ => return Err(invalid()),
            };
        } else {
            if row.contains_key("defaultActionId") {
                return Err(invalid());
            }
            default_action_id = match row.get("defaultAction") {
                None | Some(Value::Null) => None,
                Some(value) => {
                    let obj = value.as_object().ok_or_else(invalid)?;
                    if !exact(obj, &["type", "text"], &[]) || obj["type"] != "copyText" {
                        return Err(invalid());
                    }
                    let text = obj["text"].as_str().ok_or_else(invalid)?;
                    if text.len() > 16 * 1024 || text.contains('\0') {
                        return Err(invalid());
                    }
                    actions.push(PublicResultAction::CopyText {
                        id: "copy".into(),
                        title: "复制".into(),
                        text: text.into(),
                    });
                    Some("copy".into())
                }
            };
        }
        output.push(PublicActionResult {
            id: id.into(),
            title: title.into(),
            subtitle,
            detail,
            actions,
            default_action_id,
        });
    }
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn context() -> PluginRequestContext {
        PluginRequestContext {
            plugin_id: "com.example.test".into(),
            plugin_generation: 1,
            request_id: "public-request-0000000000000001".into(),
        }
    }
    fn response(row: Value) -> Value {
        json!({"requestId":context().request_id,"results":[row]})
    }
    fn row() -> Value {
        json!({"id":"constructor","title":"Text","actions":[{"id":"copy","title":"Copy","type":"copyText","text":""}],"defaultActionId":"copy"})
    }
    #[test]
    fn result_action_paste_text_strict_bounds() {
        for text in [" ".to_owned(), "汉\n".to_owned(), "a".repeat(8192)] {
            let rows = parse(&context(), response(json!({"id":"x","title":"x","actions":[{"id":"paste","title":"Paste","type":"pasteText","text":text}],"defaultActionId":"paste"}))).unwrap();
            assert_eq!(rows.len(), 1);
        }
        for text in [
            "".to_owned(),
            "a".repeat(8193),
            "汉".repeat(2731),
            "a\0b".to_owned(),
        ] {
            assert!(parse(&context(), response(json!({"id":"x","title":"x","actions":[{"id":"paste","title":"Paste","type":"pasteText","text":text}]}))).is_err());
        }
    }
    #[test]
    fn result_action_open_url_validates_and_canonicalizes_private_destinations() {
        for (raw, canonical) in [
            ("HTTPS://EXAMPLE.COM", "https://example.com/"),
            (
                "https://例子.测试/路径?q=%20#x",
                "https://xn--fsqu00a.xn--0zwm56d/%E8%B7%AF%E5%BE%84?q=%20#x",
            ),
            (
                "http://localhost:80/%00%0A%7F%80",
                "http://localhost/%00%0A%7F%80",
            ),
        ] {
            let rows = parse(&context(), response(json!({"id":"x","title":"x","actions":[{"id":"open","title":"Open","type":"openUrl","url":raw}],"defaultActionId":"open"}))).unwrap();
            assert!(
                matches!(&rows[0].actions[0], PublicResultAction::OpenUrl { url, .. } if url == canonical)
            );
        }
        for raw in [
            "https://@example.com",
            "https:///example.com",
            "https:example.com",
            "//example.com",
            "file:///tmp/x",
            "javascript:alert(1)",
            "https://user:pass@example.com",
            "https://example.com:bad",
            "https://example.com/ a",
            "https://example.com/\\x",
            "https://example.com/\u{80}",
            "https://example.com/\n",
            "https://",
            "https://?x",
            "https://#x",
            " https://example.com",
        ] {
            assert!(parse(&context(), response(json!({"id":"x","title":"x","actions":[{"id":"open","title":"Open","type":"openUrl","url":raw}]}))).is_err(), "{raw:?}");
        }
        let exact_limit = format!(
            "https://example.com/{}",
            "x".repeat(4096 - "https://example.com/".len())
        );
        assert!(parse(&context(), response(json!({"id":"x","title":"x","actions":[{"id":"open","title":"Open","type":"openUrl","url":exact_limit}]}))).is_ok());
        for raw in [
            format!("https://example.com/{}", "x".repeat(4096)),
            format!("https://example.com/{}", "汉".repeat(500)),
        ] {
            assert!(parse(&context(), response(json!({"id":"x","title":"x","actions":[{"id":"open","title":"Open","type":"openUrl","url":raw}]}))).is_err());
        }
        assert!(parse(&context(), response(json!({"id":"x","title":"x","actions":[{"id":"open","title":"Open","type":"openUrl","url":"https://example.com","payload":null}]}))).is_err());
    }
    #[test]
    fn result_action_accepts_bounded_actions_and_constructor_business_id() {
        assert!(parse(&context(), response(row())).is_ok());
    }
    #[test]
    fn result_action_rejects_entire_invalid_response() {
        for bad in [
            json!(null),
            json!({"id":"x","title":"x","actions":[],"defaultActionId":null}),
            json!({"id":"x","title":"x","actions":[],"defaultAction":null}),
            json!({"id":"x","title":"x","actions":[{"id":"run","title":"Run","type":"runtime"}]}),
            json!({"id":"x","title":"x","actions":[{"id":"run","title":"Run","type":"runtime","payload":{"constructor":null}}]}),
            json!({"id":"x","title":"x","actions":[{"id":"run","title":"\u{80}","type":"runtime","payload":null}]}),
            json!({"id":"x","title":"x","actions":[{"id":"run","title":"Run","type":"runtime","payload":null,"text":"bad"}]}),
        ] {
            assert!(parse(&context(), response(bad)).is_err());
        }
        let mut value = response(row());
        value["results"].as_array_mut().unwrap().push(row());
        assert!(parse(&context(), value).is_err());
    }
    #[test]
    fn result_action_requires_explicit_default_and_preserves_null_payload() {
        let rows=parse(&context(),response(json!({"id":"x","title":"x","actions":[{"id":"constructor","title":"Run","type":"runtime","payload":null}]}))).unwrap();
        assert!(rows[0].default_action_id.is_none());
        assert!(matches!(
            &rows[0].actions[0],
            PublicResultAction::Runtime {
                payload: Value::Null,
                ..
            }
        ));
    }
    #[test]
    fn result_action_enforces_complete_response_and_utf8_field_budgets() {
        for row in [
            serde_json::json!({"id":"x".repeat(129),"title":"x","actions":[]}),
            serde_json::json!({"id":"x","title":"x","actions":[{"id":"copy","title":"Copy","type":"copyText","text":"x".repeat(16385)}]}),
            serde_json::json!({"id":"x","title":"x","actions":[{"id":"copy","title":" ","type":"copyText","text":""}]}),
            serde_json::json!({"id":"x","title":"x","actions":[{"id":"bad.id","title":"Run","type":"runtime","payload":null}]}),
        ] {
            assert!(parse(&context(), response(row)).is_err());
        }
        let mut row = row();
        row["actions"]=Value::Array((0..9).map(|n|serde_json::json!({"id":format!("a{n}"),"title":"Run","type":"runtime","payload":null})).collect());
        assert!(parse(&context(), response(row)).is_err());
        let mut value = response(serde_json::json!({"id":"x","title":"x","actions":[]}));
        value["results"] = Value::Array(
            (0..21)
                .map(|n| serde_json::json!({"id":format!("id{n}"),"title":"x","actions":[]}))
                .collect(),
        );
        assert!(parse(&context(), value).is_err());
    }
    #[test]
    fn result_action_private_dtos_reject_retargeting() {
        for value in [
            json!({"requestId":"r","resultId":"i"}),
            json!({"requestId":"r","resultId":"i","actionId":"a","payload":null}),
            json!({"requestId":"r","resultId":1,"actionId":"a"}),
        ] {
            assert!(serde_json::from_value::<ExecuteResultActionRequest>(value).is_err());
        }
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use crate::{
        public_plugins::{
            PluginContextStatus, PluginRequestCandidate, PluginRequestOwner,
            PluginRequestScheduler, PluginScheduleOutcome, PluginSubmissionOwner,
            PublicActivationMode, PublicOutputMode,
        },
        result_registry::{QueryDomain, ResultRegistry},
    };
    use std::time::{Duration, Instant};
    pub(super) fn route() -> PublicPluginRoute {
        PublicPluginRoute {
            result_publication: None,
            command_id: "default".into(),
            plugin_id: "com.example.test".into(),
            generation: 1,
            activation_id: 11,
            admission_epoch: 23,
            runtime_recovery_needed: false,
            runtime_label: "plugin-runtime-test".into(),
            activation_mode: PublicActivationMode::Live,
            output_mode: PublicOutputMode::MainResult,
            command_label: "test".into(),
            input: "saved original input".into(),
            input_required: false,
            input_placeholder: None,
            window_entry: None,
            panel_entry: None,
            host_keys: vec![],
            host_key_focus: super::super::PanelHostKeyFocus::Content,
            icon_url: None,
        }
    }
    fn rows() -> Vec<PublicActionResult> {
        vec![PublicActionResult {
            id: "same-id".into(),
            title: "Row".into(),
            subtitle: None,
            detail: None,
            actions: vec![
                PublicResultAction::Runtime {
                    id: "save".into(),
                    title: "Save".into(),
                    payload: serde_json::json!({"saved":"private-payload"}),
                },
                PublicResultAction::CopyText {
                    id: "copy".into(),
                    title: "Copy".into(),
                    text: "private-copy".into(),
                },
            ],
            default_action_id: Some("copy".into()),
        }]
    }
    fn source() -> (
        ResultRegistry,
        ResultActionOrigin,
        ExecuteResultActionRequest,
    ) {
        let registry = ResultRegistry::default();
        registry.on_show("invocation".into());
        let token = registry
            .begin_query(QueryDomain::Plugin, "invocation", 1)
            .unwrap();
        let response = registry
            .publish_prepared_result_actions(
                token,
                registry.prepare_result_actions(route(), rows()).unwrap(),
            )
            .unwrap();
        let row = &response.items[0];
        let request = ExecuteResultActionRequest {
            request_id: response.request_id.clone(),
            result_id: row.result_id.clone(),
            action_id: row.result_actions.as_ref().unwrap().actions[0]
                .action_id
                .clone(),
        };
        let serialized = serde_json::to_string(&response).unwrap();
        for private in ["same-id", "private-payload", "private-copy"] {
            assert!(!serialized.contains(private));
        }
        let origin = registry.resolve_result_action(&request).unwrap();
        (registry, origin, request)
    }
    fn candidate(origin: &ResultActionOrigin, action: bool) -> PluginRequestCandidate {
        PluginRequestCandidate {
            command_id: origin.route.command_id.clone(),
            plugin_id: origin.route.plugin_id.clone(),
            plugin_generation: 1,
            activation_id: 11,
            admission_epoch: 23,
            activation_mode: PublicActivationMode::Submit,
            input: origin.route.input.clone(),
            owner: if action {
                PluginRequestOwner::ResultAction(origin.clone())
            } else {
                PluginRequestOwner::Command(PluginSubmissionOwner {
                    ui_intent_epoch: 1,
                    control_value: "test".into(),
                    submission_token: "test".into(),
                })
            },
        }
    }
    fn idle(scheduler: &PluginRequestScheduler, origin: &ResultActionOrigin, now: Instant) {
        let PluginScheduleOutcome::Dispatched(command) =
            scheduler.enqueue(candidate(origin, false), now).unwrap()
        else {
            panic!()
        };
        scheduler.complete(&command.context, now).unwrap();
    }
    #[test]
    fn result_action_snapshot_claim_replay_and_legacy_bypass() {
        let (registry, origin, request) = source();
        assert!(registry
            .resolve(&request.request_id, &request.result_id)
            .is_err());
        registry
            .claim_result_action(
                &origin,
                Some(PluginRequestContext {
                    plugin_id: route().plugin_id,
                    plugin_generation: 1,
                    request_id: "public-request-0000000000000001".into(),
                }),
            )
            .unwrap();
        assert_eq!(
            registry.claim_result_action(&origin, None),
            Err(ResultActionErrorCode::ActionInProgress)
        );
        registry.consume_result_action(&origin);
        assert_eq!(
            registry.claim_result_action(&origin, None),
            Err(ResultActionErrorCode::ActionAlreadyExecuted)
        );
    }
    #[test]
    fn result_action_every_registry_retirement_revokes_immediately() {
        for case in 0..8 {
            let (registry, origin, request) = source();
            match case {
                0 => registry.on_show("new".into()),
                1 => {
                    registry
                        .begin_query(QueryDomain::Application, "invocation", 2)
                        .unwrap();
                }
                2 => registry.hide_and_clear(),
                3 => {
                    assert!(registry.retire_result_set_if_current(&origin.ticket));
                }
                4 => {
                    registry.invalidate_domain(QueryDomain::Plugin).unwrap();
                }
                5 => {
                    let r = registry.reserve_plugin_epoch().unwrap();
                    registry.cancel_reserved_plugin_epoch(r).unwrap();
                }
                6 => {
                    let r = registry.reserve_plugin_epoch().unwrap();
                    registry.commit_reserved_plugin_epoch(r).unwrap();
                }
                _ => {
                    assert!(registry.retire_result_actions(&request.request_id));
                }
            }
            assert!(!origin.lease.live(), "case {case}");
            assert!(registry.claim_result_action(&origin, None).is_err());
        }
    }
    #[test]
    fn result_action_stale_retire_and_failed_query_cannot_touch_replacement() {
        let (registry, old, request) = source();
        assert!(registry
            .begin_query(QueryDomain::Plugin, "other", 3)
            .is_none());
        assert!(old.lease.live());
        let token = registry
            .begin_query(QueryDomain::Plugin, "invocation", 2)
            .unwrap();
        let response = registry
            .publish_prepared_result_actions(
                token,
                registry.prepare_result_actions(route(), rows()).unwrap(),
            )
            .unwrap();
        let next = ExecuteResultActionRequest {
            request_id: response.request_id,
            result_id: response.items[0].result_id.clone(),
            action_id: response.items[0].result_actions.as_ref().unwrap().actions[0]
                .action_id
                .clone(),
        };
        let origin = registry.resolve_result_action(&next).unwrap();
        assert!(!registry.retire_result_actions(&request.request_id));
        assert!(origin.lease.live());
        assert_ne!(request.action_id, next.action_id);
        assert_ne!(request.result_id, next.result_id);
    }
    #[test]
    fn result_action_busy_and_failed_precommit_leave_source_available() {
        let (registry, origin, _) = source();
        let scheduler = PluginRequestScheduler::default();
        let now = Instant::now();
        let PluginScheduleOutcome::Dispatched(command) =
            scheduler.enqueue(candidate(&origin, false), now).unwrap()
        else {
            panic!()
        };
        let reserved = scheduler.reserve(candidate(&origin, true)).unwrap();
        assert_eq!(
            scheduler.admit_result_action(reserved, now, || registry
                .claim_result_action(&origin, None)),
            Err(ResultActionErrorCode::ActionBusy)
        );
        scheduler.complete(&command.context, now).unwrap();
        let reserved = scheduler.reserve(candidate(&origin, true)).unwrap();
        assert_eq!(
            scheduler.admit_result_action(reserved, now, || Err(
                ResultActionErrorCode::ActionUnavailable
            )),
            Err(ResultActionErrorCode::ActionUnavailable)
        );
        registry.claim_result_action(&origin, None).unwrap();
    }
    #[test]
    fn result_action_lease_expires_apis_without_freeing_physical_slot() {
        let (registry, origin, request) = source();
        let scheduler = PluginRequestScheduler::default();
        let now = Instant::now();
        idle(&scheduler, &origin, now);
        let reserved = scheduler.reserve(candidate(&origin, true)).unwrap();
        let context = reserved.context.clone();
        let dispatch = scheduler
            .admit_result_action(reserved, now, || {
                registry.claim_result_action(&origin, Some(context.clone()))
            })
            .unwrap();
        assert_eq!(
            dispatch.deadline.duration_since(now),
            Duration::from_secs(30)
        );
        registry.retire_result_actions(&request.request_id);
        assert_eq!(
            scheduler.context_status(&context),
            PluginContextStatus::Expired
        );
        assert!(scheduler
            .with_current(&context, |_| panic!("retired API admitted"))
            .is_err());
        let queued = scheduler.enqueue(candidate(&origin, false), now).unwrap();
        assert!(matches!(queued, PluginScheduleOutcome::Waiting { .. }));
        assert!(scheduler.complete(&context, now).unwrap().next.is_some());
    }
    #[test]
    fn result_action_duplicate_ticket_and_completion_after_retirement_fail_closed() {
        for retire in [false, true] {
            let (registry, origin, request) = source();
            let scheduler = PluginRequestScheduler::default();
            let now = Instant::now();
            idle(&scheduler, &origin, now);
            let reserved = scheduler.reserve(candidate(&origin, true)).unwrap();
            let context = reserved.context.clone();
            scheduler
                .admit_result_action(reserved, now, || {
                    registry.claim_result_action(&origin, Some(context.clone()))
                })
                .unwrap();
            let duplicate = scheduler.reserve(candidate(&origin, true)).unwrap();
            assert_eq!(
                scheduler.admit_result_action(duplicate, now, || panic!()),
                Err(ResultActionErrorCode::ActionInProgress)
            );
            if retire {
                registry.retire_result_actions(&request.request_id);
            }
            assert_eq!(scheduler.complete(&context, now).unwrap().accepted, !retire);
            let prepared = registry.prepare_result_actions(route(), rows()).unwrap();
            let result = scheduler.finish_result_action(&context, &origin.lease, || {
                registry
                    .replace_result_actions(&origin, prepared)
                    .ok_or(ResultActionErrorCode::StaleRequest)
            });
            assert_eq!(result.is_ok(), !retire);
            assert!(scheduler
                .finish_result_action(&context, &origin.lease, || Ok(()))
                .is_err());
        }
    }
    #[test]
    fn result_action_shared_admission_blocks_legacy_invalidation_until_publication_releases() {
        use std::{
            sync::{mpsc, Arc},
            thread,
        };
        let legacy = Arc::new(crate::plugins::PluginManager::new());
        let (registry, origin, _) = source();
        let (entered, observed) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let worker = {
            let legacy = legacy.clone();
            thread::spawn(move || {
                legacy.with_public_result_admission(|| {
                    entered.send(()).unwrap();
                    wait.recv().unwrap();
                })
            })
        };
        observed.recv().unwrap();
        let (finished, done) = mpsc::channel();
        let invalidator = {
            let legacy = legacy.clone();
            let registry = registry.clone();
            thread::spawn(move || {
                legacy.invalidate_plugin_results(&registry);
                finished.send(()).unwrap();
            })
        };
        assert!(done.recv_timeout(Duration::from_millis(30)).is_err());
        assert!(origin.lease.live());
        release.send(()).unwrap();
        worker.join().unwrap();
        done.recv_timeout(Duration::from_secs(2)).unwrap();
        invalidator.join().unwrap();
        assert!(!origin.lease.live());
    }
}
