use super::*;
use serde_json::json;
fn context() -> PluginRequestContext {
    PluginRequestContext {
        plugin_id: "com.example.result".into(),
        plugin_generation: 1,
        request_id: "public-request-0123456789abcdef".into(),
    }
}
fn response(rows: Value) -> Value {
    json!({"requestId":context().request_id,"results":rows})
}
fn parse(rows: Value) -> Result<Vec<PublicMainResult>, PluginRuntimeError> {
    parse_main_result_response(&context(), response(rows))
}
#[test]
fn strict_shape_request_identity_and_row_identity_follow_legacy_contract() {
    assert!(parse(json!([])).unwrap().is_empty());
    for invalid in [
        json!({"requestId":context().request_id,"results":[],"extra":0}),
        json!({"requestId":"wrong","results":[]}),
        json!({"requestId":null,"results":[]}),
        json!({"requestId":context().request_id,"results":{}}),
    ] {
        assert_eq!(
            parse_main_result_response(&context(), invalid),
            Err(PluginRuntimeError::InvalidOperation)
        );
    }
    for invalid in [
        json!([{"id":"","title":"x"}]),
        json!([{"id":"x","title":"x","extra":0}]),
        json!([{"id":"x","title":"x"},{"id":"x","title":"other"}]),
        json!([{"id":1,"title":"x"}]),
        json!([{"id":"x","title":null}]),
    ] {
        assert_eq!(parse(invalid), Err(PluginRuntimeError::InvalidOperation));
    }
    // The parser matches the admitted request ID, it does not independently mint
    // or validate a request context; admission owns that separate contract.
    let mut old = context();
    old.request_id = "legacy-caller-id".into();
    assert!(
        parse_main_result_response(&old, json!({"requestId":"legacy-caller-id","results":[]}))
            .is_ok()
    );
    assert!(
        parse(json!([{"id":" ","title":""},{"id":"A","title":""},{"id":"a","title":""}])).is_ok()
    );
}
#[test]
fn optional_null_and_copy_action_keep_private_payload_separate() {
    let row = parse(
        json!([{"id":"x","title":"visible","subtitle":null,"detail":null,"defaultAction":null}]),
    )
    .unwrap()
    .remove(0);
    assert_eq!(row.subtitle, None);
    assert_eq!(row.detail, None);
    assert_eq!(row.copy_text, None);
    let row=parse(json!([{"id":"x","title":"visible","defaultAction":{"type":"copyText","text":"  private\u{0000}😀  "}}])).unwrap().remove(0);
    assert_eq!(row.title, "visible");
    assert_eq!(row.subtitle, None);
    assert_eq!(row.detail, None);
    assert_eq!(row.copy_text.as_deref(), Some("  private\0😀  "));
    for action in [
        json!({"type":"unknown","text":"x"}),
        json!({"type":"copyText","text":"x","extra":true}),
        json!({"type":"copyText","text":null}),
        json!({"text":"x"}),
        json!("copyText"),
    ] {
        assert_eq!(
            parse(json!([{"id":"x","title":"x","defaultAction":action}])),
            Err(PluginRuntimeError::InvalidOperation)
        );
    }
    assert_eq!(
        parse(json!([{"id":"x","title":"x","defaultAction":{"type":"copyText","text":""}}]))
            .unwrap()[0]
            .copy_text,
        Some(String::new())
    );
}
#[test]
fn row_count_accepts_twenty_and_rejects_twenty_one() {
    let rows: Vec<Value> = (0..20)
        .map(|n| json!({"id":n.to_string(),"title":"x"}))
        .collect();
    assert_eq!(parse(json!(rows)).unwrap().len(), 20);
    let mut rows = rows;
    rows.push(json!({"id":"20","title":"x"}));
    assert_eq!(
        parse(json!(rows)),
        Err(PluginRuntimeError::InvalidOperation)
    );
}
#[test]
fn title_subtitle_count_unicode_scalars_but_detail_counts_utf8_bytes() {
    assert!(parse(json!([{"id":"x","title":"😀".repeat(256),"subtitle":"中".repeat(512),"detail":"é".repeat(8192)}])).is_ok());
    for row in [
        json!({"id":"x","title":"😀".repeat(257)}),
        json!({"id":"x","title":"","subtitle":"中".repeat(513)}),
        json!({"id":"x","title":"","detail":"é".repeat(8192)+"x"}),
    ] {
        assert_eq!(
            parse(json!([row])),
            Err(PluginRuntimeError::InvalidOperation)
        );
    }
}
#[test]
fn aggregate_compact_utf8_budget_includes_private_copy_text_exactly() {
    let mut value =
        response(json!([{"id":"x","title":"","defaultAction":{"type":"copyText","text":""}}]));
    let base = serde_json::to_vec(&value).unwrap().len();
    value["results"][0]["defaultAction"]["text"] = json!("x".repeat(65536 - base));
    assert_eq!(serde_json::to_vec(&value).unwrap().len(), 65536);
    assert!(parse_main_result_response(&context(), value.clone()).is_ok());
    value["results"][0]["defaultAction"]["text"] = json!("x".repeat(65537 - base));
    assert_eq!(
        parse_main_result_response(&context(), value),
        Err(PluginRuntimeError::InvalidOperation)
    );
}
#[test]
fn runtime_error_names_remain_the_original_vocabulary() {
    use PluginRuntimeError::*;
    for (error, name) in [
        (InvalidContext, "InvalidContext"),
        (ExpiredRequest, "ExpiredRequestError"),
        (InvalidCaller, "InvalidCaller"),
        (InvalidOperation, "InvalidOperation"),
        (PermissionDenied, "PermissionDenied"),
        (InvalidNotification, "InvalidNotification"),
        (InvalidDelay, "InvalidDelay"),
        (ScheduleLimitExceeded, "ScheduleLimitExceeded"),
        (AlreadyPublished, "AlreadyPublished"),
        (MessageStoreUnavailable, "MessageStoreUnavailable"),
        (Storage, "StorageError"),
        (Unavailable, "RuntimeUnavailable"),
    ] {
        assert_eq!(error.to_string(), name);
    }
}
