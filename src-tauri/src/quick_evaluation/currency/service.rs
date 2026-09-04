use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
};

use rust_decimal::Decimal;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::oneshot;

use super::{
    cache::{CacheLookup, CachePersistenceOutcome, CachedExchangeRate, ExchangeRateCache},
    evaluate_provider_result, evaluate_same_code,
    provider::{ExchangeRateProvider, FrankfurterProvider},
    CurrencyFailure, CurrencyIntent, CurrencyOutput, ExchangeRateError, ExchangeRateRequest,
    ExchangeRateResult,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CurrencyResolveError {
    Invalid(CurrencyFailure),
    Unavailable,
}

pub(super) trait UtcClock: Send + Sync {
    fn now_utc(&self) -> Result<OffsetDateTime, ()>;
}

struct SystemUtcClock;

impl UtcClock for SystemUtcClock {
    fn now_utc(&self) -> Result<OffsetDateTime, ()> {
        Ok(OffsetDateTime::now_utc())
    }
}

pub(crate) struct ExchangeRateService {
    provider: Arc<dyn ExchangeRateProvider>,
    clock: Arc<dyn UtcClock>,
    cache: Mutex<ExchangeRateCache>,
    inflight: Mutex<BTreeMap<InFlightKey, Vec<oneshot::Sender<ProviderTerminal>>>>,
    persistence: tokio::sync::Mutex<()>,
}

#[derive(Clone)]
struct ProviderTerminal {
    result: Result<ExchangeRateResult, ExchangeRateError>,
    completed_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct InFlightKey {
    first: String,
    second: String,
}

impl InFlightKey {
    fn new(request: &ExchangeRateRequest) -> Self {
        let (first, second) = if request.base.as_str() < request.quote.as_str() {
            (request.base.as_str(), request.quote.as_str())
        } else {
            (request.quote.as_str(), request.base.as_str())
        };
        Self {
            first: first.into(),
            second: second.into(),
        }
    }
}

impl ExchangeRateService {
    pub(crate) fn load(root: &Path) -> Result<Self, ExchangeRateError> {
        let clock = Arc::new(SystemUtcClock);
        let now = clock
            .now_utc()
            .map_err(|()| ExchangeRateError::Unavailable)?;
        let provider = Arc::new(FrankfurterProvider::new()?);
        Ok(Self::with_dependencies(
            provider,
            ExchangeRateCache::load(root, now),
            clock,
        ))
    }

    pub(super) fn with_dependencies(
        provider: Arc<dyn ExchangeRateProvider>,
        cache: ExchangeRateCache,
        clock: Arc<dyn UtcClock>,
    ) -> Self {
        Self {
            provider,
            clock,
            cache: Mutex::new(cache),
            inflight: Mutex::new(BTreeMap::new()),
            persistence: tokio::sync::Mutex::new(()),
        }
    }

    pub(crate) async fn resolve(
        self: &Arc<Self>,
        intent: CurrencyIntent,
    ) -> Result<CurrencyOutput, CurrencyResolveError> {
        if let Some(output) = evaluate_same_code(&intent) {
            return output.map_err(CurrencyResolveError::Invalid);
        }
        let now = self
            .clock
            .now_utc()
            .map_err(|()| CurrencyResolveError::Unavailable)?;
        let request = intent
            .exchange_request()
            .expect("different currency codes have an exchange request");
        let cached = self
            .cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .lookup(&request, now);
        match cached {
            Some(CacheLookup::Fresh(cached)) => {
                return evaluate_provider_result(&intent, cached.result)
                    .map_err(CurrencyResolveError::Invalid);
            }
            Some(CacheLookup::Stale(_)) | None => {}
        }

        let terminal = self.join_or_start(request.clone(), now).await;
        match terminal.result {
            Ok(result) => {
                let result =
                    orient_result(&result, &request).ok_or(CurrencyResolveError::Unavailable)?;
                evaluate_provider_result(&intent, result).map_err(CurrencyResolveError::Invalid)
            }
            Err(ExchangeRateError::UnsupportedCode) => {
                Err(CurrencyResolveError::Invalid(CurrencyFailure::InvalidCode))
            }
            Err(ExchangeRateError::Unavailable) => {
                let completed_at = terminal
                    .completed_at
                    .ok_or(CurrencyResolveError::Unavailable)?;
                match self
                    .cache
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .lookup(&request, completed_at)
                {
                    Some(CacheLookup::Fresh(cached)) => {
                        evaluate_provider_result(&intent, cached.result)
                            .map_err(CurrencyResolveError::Invalid)
                    }
                    Some(CacheLookup::Stale(cached)) => {
                        evaluate_stale(&intent, cached).map_err(CurrencyResolveError::Invalid)
                    }
                    None => Err(CurrencyResolveError::Unavailable),
                }
            }
        }
    }

    async fn join_or_start(
        self: &Arc<Self>,
        request: ExchangeRateRequest,
        now: OffsetDateTime,
    ) -> ProviderTerminal {
        let key = InFlightKey::new(&request);
        let (sender, receiver) = oneshot::channel();
        let should_start = {
            let mut inflight = self
                .inflight
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            match inflight.get_mut(&key) {
                Some(waiters) => {
                    waiters.push(sender);
                    false
                }
                None => {
                    if let Some(CacheLookup::Fresh(cached)) = self
                        .cache
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .lookup(&request, now)
                    {
                        return ProviderTerminal {
                            result: Ok(cached.result),
                            completed_at: Some(now),
                        };
                    }
                    inflight.insert(key.clone(), vec![sender]);
                    true
                }
            }
        };
        if should_start {
            let service = self.clone();
            tokio::spawn(async move {
                service.coordinate(key, request).await;
            });
        }
        receiver.await.unwrap_or(ProviderTerminal {
            result: Err(ExchangeRateError::Unavailable),
            completed_at: None,
        })
    }

    async fn coordinate(self: Arc<Self>, key: InFlightKey, request: ExchangeRateRequest) {
        let provider = self.provider.clone();
        let provider_request = request.clone();
        let provider_task =
            tokio::spawn(async move { provider.fetch_rate(&provider_request).await });
        let provider_result = match provider_task.await {
            Ok(Ok(result)) if valid_provider_result(&request, &result) => Ok(result),
            Ok(Ok(_)) | Err(_) => Err(ExchangeRateError::Unavailable),
            Ok(Err(error)) => Err(error),
        };
        let completed_at = self.clock.now_utc().ok();
        let result = if completed_at.is_some() {
            provider_result
        } else {
            Err(ExchangeRateError::Unavailable)
        };
        if let (Ok(result), Some(completed_at)) = (&result, completed_at) {
            self.store_provider_result(result.clone(), completed_at)
                .await;
        }
        let terminal = ProviderTerminal {
            result,
            completed_at,
        };
        let waiters = self
            .inflight
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&key)
            .unwrap_or_default();
        for waiter in waiters {
            let _ = waiter.send(terminal.clone());
        }
    }

    async fn store_provider_result(&self, result: ExchangeRateResult, fetched_at: OffsetDateTime) {
        let _persistence = self.persistence.lock().await;
        let persistence = self
            .cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .prepare_store(result, fetched_at, fetched_at);
        let Some(persistence) = persistence else {
            return;
        };
        let outcome = tokio::task::spawn_blocking(move || persistence.commit())
            .await
            .unwrap_or(CachePersistenceOutcome::Failed);
        self.cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .finish_persistence(outcome);
    }
}

fn valid_provider_result(request: &ExchangeRateRequest, result: &ExchangeRateResult) -> bool {
    result.base == request.base
        && result.quote == request.quote
        && result.base != result.quote
        && result.rate > Decimal::ZERO
}

fn orient_result(
    result: &ExchangeRateResult,
    request: &ExchangeRateRequest,
) -> Option<ExchangeRateResult> {
    if result.base == request.base && result.quote == request.quote {
        return Some(result.clone());
    }
    if result.base != request.quote || result.quote != request.base {
        return None;
    }
    let rate = Decimal::ONE.checked_div(result.rate)?;
    (rate > Decimal::ZERO).then(|| ExchangeRateResult {
        base: request.base.clone(),
        quote: request.quote.clone(),
        rate,
        effective_date: result.effective_date,
    })
}

fn evaluate_stale(
    intent: &CurrencyIntent,
    cached: CachedExchangeRate,
) -> Result<CurrencyOutput, CurrencyFailure> {
    let fetched_at = cached
        .fetched_at
        .format(&Rfc3339)
        .unwrap_or_else(|_| cached.fetched_at.to_string());
    let mut output = evaluate_provider_result(intent, cached.result)?;
    output
        .subtitle
        .push_str(&format!(" · 缓存 · 获取于 {fetched_at}"));
    Ok(output)
}
