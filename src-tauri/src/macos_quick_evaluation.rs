//! Pure quick-evaluation presentation shared with the existing launcher DTOs.
//! These are single-query values, not favoritable reusable launcher commands.
use crate::{
    model::{LauncherResultActivation, ResultIconKind, ResultItem, SearchResponse, search_response},
    quick_evaluation::{self, QuickClassification, QuickKind},
    result_registry::{QueryToken, ResultAction, ResultRegistry},
};

pub(crate) fn publish(registry: &ResultRegistry, token: QueryToken, classification: QuickClassification) -> Option<SearchResponse> {
    publish_with_clocks(registry, token, classification,
        || time::OffsetDateTime::now_local().map(|now| now.date()).map_err(|_| ()),
        || Ok(time::OffsetDateTime::now_utc()))
}
fn icon(kind: QuickKind) -> ResultIconKind {
    match kind {
        QuickKind::Currency => ResultIconKind::Currency, QuickKind::Unit => ResultIconKind::Unit,
        QuickKind::Arithmetic => ResultIconKind::Calculator, QuickKind::Date => ResultIconKind::Date,
        QuickKind::TimeZone => ResultIconKind::TimeZone,
    }
}
fn publish_with_clocks<L, U>(registry: &ResultRegistry, token: QueryToken, classification: QuickClassification, local_today: L, utc_now: U) -> Option<SearchResponse>
where L: FnOnce() -> Result<time::Date, ()>, U: FnOnce() -> Result<time::OffsetDateTime, ()> {
    let evaluated = match classification {
        QuickClassification::NoMatch => return None,
        QuickClassification::Invalid(failure) => Err(failure),
        QuickClassification::Ready(intent) => quick_evaluation::evaluate(intent, local_today, utc_now),
    };
    publish_evaluated(registry, token, evaluated)
}

// The same token is captured before awaiting and checked only at publication.
// A late rate can populate the service cache but cannot replace a newer query.
pub(crate) async fn publish_currency<R, F>(registry: &ResultRegistry, token: QueryToken, intent: quick_evaluation::currency::CurrencyIntent, resolver: R) -> Option<SearchResponse>
where R: FnOnce(quick_evaluation::currency::CurrencyIntent) -> F,
      F: std::future::Future<Output = Result<quick_evaluation::currency::CurrencyOutput, quick_evaluation::currency::service::CurrencyResolveError>> {
    let evaluated = quick_evaluation::evaluate_currency(intent, resolver).await;
    publish_evaluated(registry, token, evaluated)
}

fn publish_evaluated(registry: &ResultRegistry, token: QueryToken, evaluated: Result<quick_evaluation::QuickOutput, quick_evaluation::QuickFailure>) -> Option<SearchResponse> {
    let (kind, title, subtitle, action, refresh_at_utc) = match evaluated {
        Ok(output) => {
            let refresh = if output.kind == QuickKind::TimeZone {
                output.refresh_at_utc.and_then(|instant| instant.format(&time::format_description::well_known::Rfc3339).ok())
            } else { None };
            (output.kind, output.title, output.subtitle, Some(ResultAction::CopyBuiltInText { text: output.copy_text }), refresh)
        }
        Err(failure) => (failure.kind, failure.title, Some(failure.explanation), None, None),
    };
    let actionable = action.is_some();
    let item = ResultItem {
        result_actions: None, result_id: String::new(),
        activation: if actionable { LauncherResultActivation::ExecuteResult } else { LauncherResultActivation::Informational },
        title, subtitle, icon: None, plugin_icon_url: None, icon_kind: Some(icon(kind)),
        detail: None, favorite: None, has_default_action: actionable,
    };
    registry.publish_if_latest(token, vec![(item, action)], || true, |id, rows| {
        let mut response = search_response(id, rows);
        response.replace_local_results = true;
        response.refresh_at_utc = refresh_at_utc;
        response
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use crate::result_registry::{QueryDomain, ResultRegistries, RegistryError};
    use quick_evaluation::currency::{CurrencyOutput, CurrencyFailure, service::CurrencyResolveError};

    fn currency_intent() -> quick_evaluation::currency::CurrencyIntent {
        let QuickClassification::Ready(quick_evaluation::QuickIntent::Currency(intent)) = quick_evaluation::classify("100 USD to CNY") else { panic!() };
        intent
    }
    fn currency_value() -> CurrencyOutput {
        CurrencyOutput { title: "713.245 CNY".into(), copy_text: "713.245 CNY".into(), subtitle: "Frankfurter · 汇率日期 2026-11-13".into() }
    }
    #[tokio::test]
    async fn currency_success_has_private_copy_action_and_failures_are_inert() {
        let registry = ResultRegistries::default().main().clone(); registry.on_show("1".into());
        let token = registry.begin_query(QueryDomain::Application, "1", 1).unwrap();
        let response = publish_currency(&registry, token, currency_intent(), |_| async { Ok(currency_value()) }).await.unwrap();
        let row = &response.items[0];
        assert_eq!(row.title, "713.245 CNY"); assert_eq!(row.icon_kind, Some(ResultIconKind::Currency));
        assert!(row.favorite.is_none()); assert!(response.replace_local_results); assert!(response.refresh_at_utc.is_none());
        assert_eq!(registry.resolve(&response.request_id, &row.result_id), Ok(ResultAction::CopyBuiltInText { text: "713.245 CNY".into() }));
        for (i, error, title, subtitle) in [
            (2, CurrencyResolveError::Unavailable, "汇率暂不可用", "请稍后重试"),
            (3, CurrencyResolveError::Invalid(CurrencyFailure::InvalidCode), "不支持的货币代码", "请检查三字母 ISO 4217 代码"),
        ] {
            let token = registry.begin_query(QueryDomain::Application, "1", i).unwrap();
            let response = publish_currency(&registry, token, currency_intent(), |_| async { Err(error) }).await.unwrap();
            let row = &response.items[0]; assert_eq!(row.title, title); assert_eq!(row.subtitle.as_deref(), Some(subtitle));
            assert!(!row.has_default_action); assert!(matches!(row.activation, LauncherResultActivation::Informational));
            assert_eq!(registry.resolve(&response.request_id, &row.result_id), Err(RegistryError::UnknownResult));
        }
    }
    #[tokio::test]
    async fn currency_completion_cannot_replace_new_query_hidden_or_new_invocation() {
        for transition in 0..3 {
            for fails in [false, true] {
                let registry = ResultRegistries::default().main().clone(); registry.on_show("1".into());
                let token = registry.begin_query(QueryDomain::Application, "1", 1).unwrap();
                let (send, receive) = tokio::sync::oneshot::channel();
                let pending = publish_currency(&registry, token, currency_intent(), |_| async { receive.await.unwrap() });
                tokio::pin!(pending);
                // Start the resolver and suspend it before changing query ownership.
                assert!(std::future::poll_fn(|cx| std::task::Poll::Ready(pending.as_mut().poll(cx).is_pending())).await);
                match transition {
                    0 => { query(&registry, "2+2", 2).unwrap(); },
                    1 => registry.hide_and_clear(),
                    _ => registry.on_show("2".into()),
                }
                send.send(if fails { Err(CurrencyResolveError::Unavailable) } else { Ok(currency_value()) }).unwrap();
                assert!(pending.await.is_none());
            }
        }
    }
    fn query(registry: &ResultRegistry, text: &str, sequence: u64) -> Option<SearchResponse> {
        let token = registry.begin_query(QueryDomain::Application, "1", sequence).unwrap();
        publish_with_clocks(registry, token, quick_evaluation::classify(text),
            || Ok(time::Date::from_calendar_date(2026, time::Month::November, 7).unwrap()),
            || Ok(time::OffsetDateTime::from_unix_timestamp(0).unwrap()))
    }
    #[test]
    fn pure_values_use_private_copy_actions_and_are_not_favoritable() {
        let registry = ResultRegistries::default().main().clone(); registry.on_show("1".into());
        for (index, (source, expected, kind)) in [
            ("2+2", "4", ResultIconKind::Calculator),
            ("1000 mm to in", "39.3701 in", ResultIconKind::Unit),
            ("today + 1 day", "2026-11-08", ResultIconKind::Date),
            ("Mumbai time", "05:30", ResultIconKind::TimeZone),
        ].into_iter().enumerate() {
            let response = query(&registry, source, index as u64 + 1).unwrap();
            assert!(response.replace_local_results); assert_eq!(response.items.len(), 1);
            let row = &response.items[0]; assert_eq!(row.title, expected); assert_eq!(row.icon_kind, Some(kind));
            assert!(row.favorite.is_none()); assert!(row.has_default_action);
            assert!(matches!(row.activation, LauncherResultActivation::ExecuteResult));
            assert_eq!(registry.resolve(&response.request_id, &row.result_id), Ok(ResultAction::CopyBuiltInText { text: expected.into() }));
            if kind == ResultIconKind::TimeZone { assert_eq!(response.refresh_at_utc.as_deref(), Some("1970-01-01T00:01:00Z")); }
            else { assert!(response.refresh_at_utc.is_none()); }
            let wire = serde_json::to_string(&response).unwrap(); assert!(!wire.contains("copy_text")); assert!(!wire.contains("copyText"));
        }
    }
    #[test]
    fn clock_failures_are_inert_and_old_query_cannot_publish() {
        let registry = ResultRegistries::default().main().clone(); registry.on_show("1".into());
        let old = registry.begin_query(QueryDomain::Application, "1", 1).unwrap();
        let latest = registry.begin_query(QueryDomain::Application, "1", 2).unwrap();
        let response = publish_with_clocks(&registry, latest, quick_evaluation::classify("today + 1 day"), || Err(()), || panic!("date does not read UTC")).unwrap();
        assert_eq!(response.items[0].subtitle.as_deref(), Some("请检查系统日期和时区设置"));
        assert!(!response.items[0].has_default_action);
        assert!(publish_with_clocks(&registry, old, quick_evaluation::classify("2+2"), || panic!(), || panic!()).is_none());
        assert!(matches!(quick_evaluation::classify("Calculator"), QuickClassification::NoMatch));
    }
}
