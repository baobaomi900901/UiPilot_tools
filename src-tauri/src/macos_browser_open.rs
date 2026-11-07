//! Single explicit browser submission. No URL fetching, shell or default-app mutation.
use objc2::rc::{autoreleasepool, Retained};
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSString, NSURL};
use std::panic::AssertUnwindSafe;

pub(crate) const MAX_URL_BYTES: usize = 16 * 1024;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OpenError { Unavailable, Stale, Unconfirmed }

fn validate_url(raw: &str) -> Result<url::Url, OpenError> {
    if raw.len() > MAX_URL_BYTES || raw.bytes().any(|byte| byte.is_ascii_control()) { return Err(OpenError::Unavailable); }
    // Inspect raw authority before Url normalizes away an empty userinfo (`@`).
    // Only standard absolute web URLs are accepted; queries may contain `@`.
    let (scheme, rest) = raw.split_once("://").ok_or(OpenError::Unavailable)?;
    let authority = rest.split(['/', '?', '#']).next().ok_or(OpenError::Unavailable)?;
    if !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        || authority.is_empty() || authority.contains('@') || authority.contains('\\') {
        return Err(OpenError::Unavailable);
    }
    let url = url::Url::parse(raw).map_err(|_| OpenError::Unavailable)?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none()
        || !url.username().is_empty() || url.password().is_some() || url.as_str().len() > MAX_URL_BYTES {
        return Err(OpenError::Unavailable);
    }
    Ok(url)
}
trait BrowserDriver {
    type Prepared;
    fn prepare(&mut self, url: &url::Url) -> Result<Self::Prepared, OpenError>;
    fn submit(&mut self, url: Self::Prepared) -> Result<bool, OpenError>;
}
fn open_with<D: BrowserDriver>(raw: &str, admit: impl FnOnce() -> bool, factory: impl FnOnce() -> Result<D, OpenError>) -> Result<(), OpenError> {
    let url = validate_url(raw)?;
    let mut driver = factory()?;
    let prepared = driver.prepare(&url)?;
    if !admit() { return Err(OpenError::Stale); }
    // From admission onward the ticket is consumed even if openURL returns false
    // or throws. No retry and no re-creation of a previously consumed action.
    match driver.submit(prepared) { Ok(true) => Ok(()), _ => Err(OpenError::Unconfirmed) }
}

/// Worker-only trusted host entry. The caller constructs the URL from a fixed
/// provider and must consume its current, typed registry ticket in `admit`.
pub(crate) fn open_url(raw: &str, admit: impl FnOnce() -> bool) -> Result<(), OpenError> {
    autoreleasepool(|_| open_with(raw, admit, || {
        objc2::exception::catch(|| NativeBrowser(NSWorkspace::sharedWorkspace())).map_err(|_| OpenError::Unavailable)
    }))
}
struct NativeBrowser(Retained<NSWorkspace>);
impl BrowserDriver for NativeBrowser {
    type Prepared = Retained<NSURL>;
    fn prepare(&mut self, url: &url::Url) -> Result<Self::Prepared, OpenError> {
        objc2::exception::catch(|| NSURL::URLWithString(&NSString::from_str(url.as_str())))
            .map_err(|_| OpenError::Unavailable)?.ok_or(OpenError::Unavailable)
    }
    fn submit(&mut self, url: Self::Prepared) -> Result<bool, OpenError> {
        // Apple documents openURL: as safe from any thread on macOS >= 10.6.
        // Neither this bool nor a returned application implies page download.
        objc2::exception::catch(AssertUnwindSafe(|| self.0.openURL(&url))).map_err(|_| OpenError::Unconfirmed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};
    struct Fake { submitted: Rc<Cell<usize>>, result: Result<bool, OpenError>, prepare_failed: bool }
    impl BrowserDriver for Fake {
        type Prepared = String;
        fn prepare(&mut self, url: &url::Url) -> Result<String, OpenError> {
            if self.prepare_failed { Err(OpenError::Unavailable) } else { Ok(url.to_string()) }
        }
        fn submit(&mut self, _: String) -> Result<bool, OpenError> { self.submitted.set(self.submitted.get() + 1); self.result }
    }
    #[test]
    fn rejects_unsupported_urls_before_driver_or_admission() {
        for raw in ["", "file:///tmp/test", "javascript:alert(1)", "data:text/plain,hello", "mailto:x@example.com", "https://user:password@example.com/", "https://user@example.com/", "https://@example.com/", "https://:@example.com/", "https://example.com/\0bad", "https://example.com/\nbad", "https://"] {
            assert_eq!(open_with::<Fake>(raw, || panic!("invalid URL cannot admit"), || panic!("invalid URL cannot construct native objects")), Err(OpenError::Unavailable), "{raw:?}");
        }
        let base = "https://example.com/"; let exact = format!("{base}{}", "a".repeat(MAX_URL_BYTES - base.len()));
        assert!(validate_url(&exact).is_ok()); assert!(validate_url(&(exact + "a")).is_err());
        assert!(validate_url("http://example.com/").is_ok());
        assert!(validate_url("https://example.com/search?q=user@example.org").is_ok());
    }
    #[test]
    fn prepare_failure_or_stale_admission_never_submits() {
        let calls = Rc::new(Cell::new(0));
        assert_eq!(open_with("https://example.com/", || panic!("failed prepare"), || Ok(Fake { submitted: calls.clone(), result: Ok(true), prepare_failed: true })), Err(OpenError::Unavailable));
        assert_eq!(open_with("https://example.com/", || false, || Ok(Fake { submitted: calls.clone(), result: Ok(true), prepare_failed: false })), Err(OpenError::Stale));
        assert_eq!(calls.get(), 0);
    }
    #[test]
    fn accepted_false_and_exception_each_submit_exactly_once_without_retry() {
        for result in [Ok(true), Ok(false), Err(OpenError::Unconfirmed)] {
            let calls = Rc::new(Cell::new(0)); let admitted = Cell::new(0);
            let outcome = open_with("https://example.com/", || { admitted.set(admitted.get() + 1); true }, || Ok(Fake { submitted: calls.clone(), result, prepare_failed: false }));
            assert_eq!(outcome, if result == Ok(true) { Ok(()) } else { Err(OpenError::Unconfirmed) });
            assert_eq!(calls.get(), 1); assert_eq!(admitted.get(), 1);
        }
    }
    #[test]
    fn prepared_browser_submission_uses_real_typed_registry_admission() {
        use crate::result_registry::{MainActionKind, QueryDomain, ResultAction, ResultRegistries, ResultRegistry};
        struct PreparedProbe { registry: ResultRegistry, stale: bool, prepared: Rc<Cell<bool>>, calls: Rc<Cell<usize>> }
        impl BrowserDriver for PreparedProbe {
            type Prepared = String;
            fn prepare(&mut self, url: &url::Url) -> Result<String, OpenError> {
                self.prepared.set(true);
                if self.stale { self.registry.begin_query(QueryDomain::Application, "1", 2).unwrap(); }
                Ok(url.to_string())
            }
            fn submit(&mut self, _: String) -> Result<bool, OpenError> { self.calls.set(self.calls.get() + 1); Ok(true) }
        }
        for stale in [false, true] {
            let registry = ResultRegistries::default().main().clone(); registry.on_show("1".into());
            let token = registry.begin_query(QueryDomain::Application, "1", 1).unwrap();
            let (request, id) = registry.publish_if_latest(token, vec![((), Some(ResultAction::OpenWebSearch { engine: crate::settings::WebSearchEngine::Bing, query: "fixture".into() }))], || true,
                |request, rows| (request, rows[0].0.clone())).unwrap();
            let (_, ticket) = registry.resolve_with_ticket(&request, &id).unwrap();
            let prepared = Rc::new(Cell::new(false)); let calls = Rc::new(Cell::new(0)); let mut owner = None;
            let result = open_with("https://www.bing.com/search?q=fixture", || {
                assert!(prepared.get(), "URL payload must exist before ticket is consumed");
                owner = registry.consume_main_action(&ticket, MainActionKind::WebSearch).ok(); owner.is_some()
            }, || Ok(PreparedProbe { registry: registry.clone(), stale, prepared: prepared.clone(), calls: calls.clone() }));
            assert_eq!(result, if stale { Err(OpenError::Stale) } else { Ok(()) });
            assert_eq!(calls.get(), usize::from(!stale));
            if let Some(owner) = owner { assert!(registry.finish_main_action(&owner)); }
        }
    }

}
