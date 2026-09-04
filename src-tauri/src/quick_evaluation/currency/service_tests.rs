use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration as StdDuration, Instant},
};

use rust_decimal::Decimal;
use time::{Date, Duration, Month, OffsetDateTime};
use tokio::sync::Semaphore;

use super::{
    cache::{CacheCommitter, ExchangeRateCache},
    provider::{ExchangeRateProvider, ExchangeRateProviderFuture},
    service::{CurrencyResolveError, ExchangeRateService, UtcClock},
    CurrencyClassification, CurrencyCode, CurrencyFailure, CurrencyIntent, ExchangeRateError,
    ExchangeRateRequest, ExchangeRateResult,
};
use crate::atomic_file::{AtomicFileError, AtomicPaths};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "uipilot-exchange-rate-service-{label}-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        if self.0.exists() {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

#[derive(Default)]
struct NoopCommitter;

impl CacheCommitter for NoopCommitter {
    fn commit(
        &self,
        _paths: &AtomicPaths,
        _previous: Option<&[u8]>,
        _candidate: &[u8],
    ) -> Result<(), AtomicFileError> {
        Ok(())
    }
}

struct CaptureCommitter {
    commits: Mutex<Vec<(Option<Vec<u8>>, Vec<u8>)>>,
    finished: Semaphore,
}

impl Default for CaptureCommitter {
    fn default() -> Self {
        Self {
            commits: Mutex::new(Vec::new()),
            finished: Semaphore::new(0),
        }
    }
}

impl CaptureCommitter {
    fn commits(&self) -> Vec<(Option<Vec<u8>>, Vec<u8>)> {
        self.commits.lock().unwrap().clone()
    }

    async fn wait_for_commits(&self, count: u32) {
        self.finished.acquire_many(count).await.unwrap().forget();
    }
}

impl CacheCommitter for CaptureCommitter {
    fn commit(
        &self,
        _paths: &AtomicPaths,
        previous: Option<&[u8]>,
        candidate: &[u8],
    ) -> Result<(), AtomicFileError> {
        self.commits
            .lock()
            .unwrap()
            .push((previous.map(<[u8]>::to_vec), candidate.to_vec()));
        self.finished.add_permits(1);
        Ok(())
    }
}

struct ManualClock {
    now: Mutex<Result<OffsetDateTime, ()>>,
    reads: AtomicUsize,
}

impl ManualClock {
    fn new(now: OffsetDateTime) -> Self {
        Self {
            now: Mutex::new(Ok(now)),
            reads: AtomicUsize::new(0),
        }
    }

    fn set(&self, now: OffsetDateTime) {
        *self.now.lock().unwrap() = Ok(now);
    }

    fn fail(&self) {
        *self.now.lock().unwrap() = Err(());
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

impl UtcClock for ManualClock {
    fn now_utc(&self) -> Result<OffsetDateTime, ()> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        *self.now.lock().unwrap()
    }
}

struct BlockingClock {
    now: OffsetDateTime,
    block_call: usize,
    calls: AtomicUsize,
    entered: Semaphore,
    released: Mutex<bool>,
    release_signal: Condvar,
}

impl BlockingClock {
    fn new(now: OffsetDateTime, block_call: usize) -> Self {
        Self {
            now,
            block_call,
            calls: AtomicUsize::new(0),
            entered: Semaphore::new(0),
            released: Mutex::new(false),
            release_signal: Condvar::new(),
        }
    }

    async fn wait_until_entered(&self) {
        self.entered.acquire().await.unwrap().forget();
    }

    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.release_signal.notify_all();
    }
}

impl UtcClock for BlockingClock {
    fn now_utc(&self) -> Result<OffsetDateTime, ()> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if call == self.block_call {
            self.entered.add_permits(1);
            let mut released = self.released.lock().unwrap();
            while !*released {
                released = self.release_signal.wait(released).unwrap();
            }
        }
        Ok(self.now)
    }
}

struct ScriptedBlockingClock {
    values: Vec<OffsetDateTime>,
    block_call: usize,
    calls: AtomicUsize,
    entered: Semaphore,
    released: Mutex<bool>,
    release_signal: Condvar,
}

impl ScriptedBlockingClock {
    fn new(values: Vec<OffsetDateTime>, block_call: usize) -> Self {
        assert!(!values.is_empty());
        Self {
            values,
            block_call,
            calls: AtomicUsize::new(0),
            entered: Semaphore::new(0),
            released: Mutex::new(false),
            release_signal: Condvar::new(),
        }
    }

    async fn wait_until_entered(&self) {
        self.entered.acquire().await.unwrap().forget();
    }

    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.release_signal.notify_all();
    }
}

impl UtcClock for ScriptedBlockingClock {
    fn now_utc(&self) -> Result<OffsetDateTime, ()> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        let value = self.values[call.saturating_sub(1).min(self.values.len() - 1)];
        if call == self.block_call {
            self.entered.add_permits(1);
            let mut released = self.released.lock().unwrap();
            while !*released {
                released = self.release_signal.wait(released).unwrap();
            }
        }
        Ok(value)
    }
}

struct BlockingCommitter {
    entered: Semaphore,
    finished: Semaphore,
    released: Mutex<bool>,
    release_signal: Condvar,
    commits: Mutex<Vec<(Option<Vec<u8>>, Vec<u8>)>>,
}

impl BlockingCommitter {
    fn new() -> Self {
        Self {
            entered: Semaphore::new(0),
            finished: Semaphore::new(0),
            released: Mutex::new(false),
            release_signal: Condvar::new(),
            commits: Mutex::new(Vec::new()),
        }
    }

    async fn wait_until_entered(&self) {
        self.entered.acquire().await.unwrap().forget();
    }

    async fn wait_until_finished(&self) {
        self.finished.acquire().await.unwrap().forget();
    }

    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.release_signal.notify_all();
    }

    fn commits(&self) -> Vec<(Option<Vec<u8>>, Vec<u8>)> {
        self.commits.lock().unwrap().clone()
    }
}

impl CacheCommitter for BlockingCommitter {
    fn commit(
        &self,
        _paths: &AtomicPaths,
        previous: Option<&[u8]>,
        candidate: &[u8],
    ) -> Result<(), AtomicFileError> {
        let call = {
            let mut commits = self.commits.lock().unwrap();
            commits.push((previous.map(<[u8]>::to_vec), candidate.to_vec()));
            commits.len()
        };
        if call == 1 {
            self.entered.add_permits(1);
            let mut released = self.released.lock().unwrap();
            while !*released {
                released = self.release_signal.wait(released).unwrap();
            }
        }
        self.finished.add_permits(1);
        Ok(())
    }
}

struct SleepingCommitter;

impl CacheCommitter for SleepingCommitter {
    fn commit(
        &self,
        _paths: &AtomicPaths,
        _previous: Option<&[u8]>,
        _candidate: &[u8],
    ) -> Result<(), AtomicFileError> {
        std::thread::sleep(StdDuration::from_millis(300));
        Ok(())
    }
}

struct FailingCommitter;

impl CacheCommitter for FailingCommitter {
    fn commit(
        &self,
        _paths: &AtomicPaths,
        _previous: Option<&[u8]>,
        _candidate: &[u8],
    ) -> Result<(), AtomicFileError> {
        Err(AtomicFileError::CandidateWrite)
    }
}

struct PanickingCommitter;

impl CacheCommitter for PanickingCommitter {
    fn commit(
        &self,
        _paths: &AtomicPaths,
        _previous: Option<&[u8]>,
        _candidate: &[u8],
    ) -> Result<(), AtomicFileError> {
        panic!("cache commit panic sentinel");
    }
}

struct QueueProvider {
    outcomes: Mutex<VecDeque<Result<ExchangeRateResult, ExchangeRateError>>>,
    calls: AtomicUsize,
}

#[derive(Clone, Copy)]
enum ControlledOutcome {
    Success,
    Unsupported,
    Unavailable,
    Panic,
}

struct ControlledProvider {
    outcomes: Mutex<VecDeque<ControlledOutcome>>,
    calls: AtomicUsize,
    entered: Semaphore,
    release: Semaphore,
}

impl ControlledProvider {
    fn new(outcomes: Vec<ControlledOutcome>) -> Self {
        Self {
            outcomes: Mutex::new(outcomes.into()),
            calls: AtomicUsize::new(0),
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    async fn wait_until_called(&self) {
        self.entered.acquire().await.unwrap().forget();
    }

    fn release(&self, permits: usize) {
        self.release.add_permits(permits);
    }
}

impl ExchangeRateProvider for ControlledProvider {
    fn fetch_rate<'a>(
        &'a self,
        request: &'a ExchangeRateRequest,
    ) -> ExchangeRateProviderFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let outcome = self
                .outcomes
                .lock()
                .unwrap()
                .pop_front()
                .expect("queued controlled provider outcome");
            self.entered.add_permits(1);
            self.release.acquire().await.unwrap().forget();
            match outcome {
                ControlledOutcome::Success => {
                    let rate = if request.base.as_str() == "USD" {
                        "8"
                    } else {
                        "0.125"
                    };
                    Ok(result(request.base.as_str(), request.quote.as_str(), rate))
                }
                ControlledOutcome::Unavailable => Err(ExchangeRateError::Unavailable),
                ControlledOutcome::Unsupported => Err(ExchangeRateError::UnsupportedCode),
                ControlledOutcome::Panic => panic!("provider panic sentinel"),
            }
        })
    }
}

impl QueueProvider {
    fn new(outcomes: Vec<Result<ExchangeRateResult, ExchangeRateError>>) -> Self {
        Self {
            outcomes: Mutex::new(outcomes.into()),
            calls: AtomicUsize::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl ExchangeRateProvider for QueueProvider {
    fn fetch_rate<'a>(
        &'a self,
        _request: &'a ExchangeRateRequest,
    ) -> ExchangeRateProviderFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.outcomes
                .lock()
                .unwrap()
                .pop_front()
                .expect("queued provider outcome")
        })
    }
}

fn instant(hour: u8) -> OffsetDateTime {
    Date::from_calendar_date(2026, Month::September, 3)
        .unwrap()
        .with_hms(hour, 0, 0)
        .unwrap()
        .assume_utc()
}

fn code(value: &str) -> CurrencyCode {
    CurrencyCode::new(value).unwrap()
}

fn result(base: &str, quote: &str, rate: &str) -> ExchangeRateResult {
    ExchangeRateResult {
        base: code(base),
        quote: code(quote),
        rate: Decimal::from_str_exact(rate).unwrap(),
        effective_date: Date::from_calendar_date(2026, Month::September, 2).unwrap(),
    }
}

fn intent(source: &str) -> CurrencyIntent {
    let CurrencyClassification::Ready(intent) = super::classify(source) else {
        panic!("expected ready currency intent for {source:?}");
    };
    intent
}

fn cache(dir: &TestDir, now: OffsetDateTime) -> ExchangeRateCache {
    cache_with_committer(dir, now, Arc::new(NoopCommitter))
}

fn cache_with_committer(
    dir: &TestDir,
    now: OffsetDateTime,
    committer: Arc<dyn CacheCommitter>,
) -> ExchangeRateCache {
    ExchangeRateCache::load_with(
        dir.path(),
        now,
        Arc::new(super::cache::AtomicCacheReader),
        committer,
    )
}

#[tokio::test]
async fn same_code_and_fresh_cache_return_without_provider_work() {
    let dir = TestDir::new("fresh-order");
    let now = instant(12);
    let provider = Arc::new(QueueProvider::new(Vec::new()));
    let mut cache = cache(&dir, now);
    cache.store(result("USD", "CNY", "8"), now, now);
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider.clone(),
        cache,
        Arc::new(ManualClock::new(now)),
    ));

    let same = service.resolve(intent("12.5 USD to usd")).await.unwrap();
    let direct = service.resolve(intent("2 USD to CNY")).await.unwrap();
    let reverse = service.resolve(intent("16 CNY to USD")).await.unwrap();

    assert_eq!(same.title, "12.5 USD");
    assert_eq!(direct.title, "16 CNY");
    assert_eq!(reverse.title, "2 USD");
    assert_eq!(provider.calls(), 0);
}

#[tokio::test]
async fn stale_cache_refreshes_and_provider_success_wins() {
    let dir = TestDir::new("stale-refresh");
    let now = instant(12);
    let fetched_at = now - Duration::HOUR - Duration::SECOND;
    let provider = Arc::new(QueueProvider::new(vec![Ok(result("USD", "CNY", "8"))]));
    let mut cache = cache(&dir, now);
    cache.store(result("USD", "CNY", "7"), fetched_at, now);
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider.clone(),
        cache,
        Arc::new(ManualClock::new(now)),
    ));

    let output = service.resolve(intent("2 USD to CNY")).await.unwrap();

    assert_eq!(output.title, "16 CNY");
    assert!(!output.subtitle.contains("缓存"));
    assert_eq!(provider.calls(), 1);
}

#[tokio::test]
async fn unavailable_refresh_uses_reverse_stale_through_exact_seven_day_boundary() {
    let dir = TestDir::new("stale-fallback");
    let now = instant(12);
    let fetched_at = now - Duration::days(7);
    let provider = Arc::new(QueueProvider::new(vec![Err(
        ExchangeRateError::Unavailable,
    )]));
    let mut cache = cache(&dir, now);
    cache.store(result("USD", "CNY", "8"), fetched_at, now);
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider,
        cache,
        Arc::new(ManualClock::new(now)),
    ));

    let output = service.resolve(intent("16 CNY to USD")).await.unwrap();

    assert_eq!(output.title, "2 USD");
    assert!(output.subtitle.contains("缓存"));
    assert!(output.subtitle.contains("汇率日期 2026-09-02"));
    assert!(output.subtitle.contains("获取于 2026-08-27T12:00:00Z"));
}

#[tokio::test]
async fn unsupported_code_never_uses_stale_fallback() {
    let dir = TestDir::new("unsupported");
    let now = instant(12);
    let provider = Arc::new(QueueProvider::new(vec![Err(
        ExchangeRateError::UnsupportedCode,
    )]));
    let mut cache = cache(&dir, now);
    cache.store(
        result("USD", "XYZ", "2"),
        now - Duration::HOUR - Duration::SECOND,
        now,
    );
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider.clone(),
        cache,
        Arc::new(ManualClock::new(now)),
    ));

    let error = service.resolve(intent("2 USD to XYZ")).await.unwrap_err();

    assert_eq!(
        error,
        CurrencyResolveError::Invalid(CurrencyFailure::InvalidCode)
    );
    assert_eq!(provider.calls(), 1);
}

#[tokio::test]
async fn direct_inverse_and_amount_only_callers_share_one_provider_task() {
    let dir = TestDir::new("coalescing");
    let now = instant(12);
    let provider = Arc::new(ControlledProvider::new(vec![
        ControlledOutcome::Success,
        ControlledOutcome::Success,
        ControlledOutcome::Success,
    ]));
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider.clone(),
        cache(&dir, now),
        Arc::new(ManualClock::new(now)),
    ));

    let direct_service = service.clone();
    let direct = tokio::spawn(async move { direct_service.resolve(intent("2 USD to CNY")).await });
    provider.wait_until_called().await;
    let inverse_service = service.clone();
    let inverse =
        tokio::spawn(async move { inverse_service.resolve(intent("16 CNY to USD")).await });
    let amount_service = service.clone();
    let amount = tokio::spawn(async move { amount_service.resolve(intent("3 USD to CNY")).await });
    tokio::task::yield_now().await;
    provider.release(3);

    assert_eq!(direct.await.unwrap().unwrap().title, "16 CNY");
    assert_eq!(inverse.await.unwrap().unwrap().title, "2 USD");
    assert_eq!(amount.await.unwrap().unwrap().title, "24 CNY");
    assert_eq!(provider.calls(), 1);
}

#[tokio::test]
async fn cancelling_the_first_caller_does_not_cancel_the_shared_work() {
    let dir = TestDir::new("leader-cancel");
    let now = instant(12);
    let provider = Arc::new(ControlledProvider::new(vec![
        ControlledOutcome::Success,
        ControlledOutcome::Success,
    ]));
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider.clone(),
        cache(&dir, now),
        Arc::new(ManualClock::new(now)),
    ));

    let leader_service = service.clone();
    let leader = tokio::spawn(async move { leader_service.resolve(intent("2 USD to CNY")).await });
    provider.wait_until_called().await;
    leader.abort();
    let follower_service = service.clone();
    let follower =
        tokio::spawn(async move { follower_service.resolve(intent("16 CNY to USD")).await });
    tokio::task::yield_now().await;
    provider.release(2);

    let output = tokio::time::timeout(Duration::seconds(1).unsigned_abs(), follower)
        .await
        .expect("follower settled")
        .unwrap()
        .unwrap();
    assert_eq!(output.title, "2 USD");
    assert_eq!(provider.calls(), 1);
    assert_eq!(
        service.resolve(intent("1 USD to CNY")).await.unwrap().title,
        "8 CNY"
    );
    assert_eq!(provider.calls(), 1);
}

#[tokio::test]
async fn provider_panic_is_unavailable_and_does_not_strand_a_follower() {
    let dir = TestDir::new("provider-panic");
    let now = instant(12);
    let provider = Arc::new(ControlledProvider::new(vec![ControlledOutcome::Panic]));
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider.clone(),
        cache(&dir, now),
        Arc::new(ManualClock::new(now)),
    ));

    let first_service = service.clone();
    let first = tokio::spawn(async move { first_service.resolve(intent("2 USD to CNY")).await });
    provider.wait_until_called().await;
    let follower_service = service.clone();
    let follower =
        tokio::spawn(async move { follower_service.resolve(intent("16 CNY to USD")).await });
    tokio::task::yield_now().await;
    provider.release(1);

    for caller in [first, follower] {
        assert_eq!(
            tokio::time::timeout(Duration::seconds(1).unsigned_abs(), caller)
                .await
                .expect("caller settled")
                .unwrap(),
            Err(CurrencyResolveError::Unavailable)
        );
    }
    assert_eq!(provider.calls(), 1);
}

#[tokio::test]
async fn terminal_failure_removes_inflight_state_and_allows_retry() {
    let dir = TestDir::new("failure-retry");
    let now = instant(12);
    let provider = Arc::new(ControlledProvider::new(vec![
        ControlledOutcome::Unavailable,
        ControlledOutcome::Success,
    ]));
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider.clone(),
        cache(&dir, now),
        Arc::new(ManualClock::new(now)),
    ));

    let first_service = service.clone();
    let first = tokio::spawn(async move { first_service.resolve(intent("2 USD to CNY")).await });
    provider.wait_until_called().await;
    let follower_service = service.clone();
    let follower =
        tokio::spawn(async move { follower_service.resolve(intent("16 CNY to USD")).await });
    tokio::task::yield_now().await;
    provider.release(2);
    assert_eq!(first.await.unwrap(), Err(CurrencyResolveError::Unavailable));
    assert_eq!(
        follower.await.unwrap(),
        Err(CurrencyResolveError::Unavailable)
    );
    assert_eq!(provider.calls(), 1);

    let retry_service = service.clone();
    let retry = tokio::spawn(async move { retry_service.resolve(intent("2 USD to CNY")).await });
    provider.wait_until_called().await;
    provider.release(1);
    assert_eq!(retry.await.unwrap().unwrap().title, "16 CNY");
    assert_eq!(provider.calls(), 2);
}

#[tokio::test]
async fn expired_or_missing_cache_returns_typed_unavailable() {
    let dir = TestDir::new("unavailable");
    let now = instant(12);
    let provider = Arc::new(QueueProvider::new(vec![Err(
        ExchangeRateError::Unavailable,
    )]));
    let mut cache = cache(&dir, now);
    cache.store(
        result("USD", "CNY", "7"),
        now - Duration::days(7) - Duration::SECOND,
        now,
    );
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider,
        cache,
        Arc::new(ManualClock::new(now)),
    ));

    assert_eq!(
        service.resolve(intent("2 USD to CNY")).await,
        Err(CurrencyResolveError::Unavailable)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn provider_wait_does_not_hold_cache_or_inflight_locks() {
    let dir = TestDir::new("provider-locks");
    let now = instant(12);
    let provider = Arc::new(ControlledProvider::new(vec![ControlledOutcome::Success]));
    let mut seeded = cache(&dir, now);
    seeded.store(result("EUR", "JPY", "160"), now, now);
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider.clone(),
        seeded,
        Arc::new(ManualClock::new(now)),
    ));

    let cold_service = service.clone();
    let cold = tokio::spawn(async move { cold_service.resolve(intent("2 USD to CNY")).await });
    provider.wait_until_called().await;
    let fresh_service = service.clone();
    let mut fresh =
        tokio::spawn(async move { fresh_service.resolve(intent("2 EUR to JPY")).await });
    let prompt = tokio::time::timeout(StdDuration::from_millis(100), &mut fresh).await;
    let completed_promptly = prompt.is_ok();
    provider.release(1);
    let cold_output = cold.await.unwrap().unwrap();
    let fresh_output = match prompt {
        Ok(output) => output.unwrap().unwrap(),
        Err(_) => fresh.await.unwrap().unwrap(),
    };

    assert!(
        completed_promptly,
        "fresh cache lookup was blocked by provider I/O"
    );
    assert_eq!(fresh_output.title, "320 JPY");
    assert_eq!(cold_output.title, "16 CNY");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn provider_success_returns_before_blocked_persistence_and_populates_memory_first() {
    let dir = TestDir::new("disk-locks");
    let now = instant(12);
    let provider = Arc::new(QueueProvider::new(vec![Ok(result("USD", "CNY", "8"))]));
    let committer = Arc::new(BlockingCommitter::new());
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider.clone(),
        cache_with_committer(&dir, now, committer.clone()),
        Arc::new(ManualClock::new(now)),
    ));

    let first_service = service.clone();
    let mut first =
        tokio::spawn(async move { first_service.resolve(intent("2 USD to CNY")).await });
    committer.wait_until_entered().await;
    let prompt = tokio::time::timeout(StdDuration::from_millis(100), &mut first).await;
    let completed_promptly = prompt.is_ok();
    let first_output = prompt.ok().map(|output| output.unwrap().unwrap());
    let fresh_output = tokio::time::timeout(
        StdDuration::from_millis(100),
        service.resolve(intent("3 USD to CNY")),
    )
    .await
    .expect("same-pair memory lookup must not wait for persistence")
    .unwrap();
    assert_eq!(provider.calls(), 1);
    committer.release();
    committer.wait_until_finished().await;
    let first_output = match first_output {
        Some(output) => output,
        None => first.await.unwrap().unwrap(),
    };

    assert!(
        completed_promptly,
        "provider success waited for blocked cache persistence"
    );
    assert_eq!(first_output.title, "16 CNY");
    assert_eq!(fresh_output.title, "24 CNY");
}

#[tokio::test(flavor = "current_thread")]
async fn synchronous_committer_runs_off_the_async_runtime() {
    let dir = TestDir::new("spawn-blocking");
    let now = instant(12);
    let provider = Arc::new(QueueProvider::new(vec![Ok(result("USD", "CNY", "8"))]));
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider,
        cache_with_committer(&dir, now, Arc::new(SleepingCommitter)),
        Arc::new(ManualClock::new(now)),
    ));
    let started = Instant::now();
    let resolve_service = service.clone();
    let resolve =
        tokio::spawn(async move { resolve_service.resolve(intent("2 USD to CNY")).await });

    tokio::time::sleep(StdDuration::from_millis(25)).await;
    let heartbeat = started.elapsed();
    let output = resolve.await.unwrap().unwrap();

    assert!(
        heartbeat < StdDuration::from_millis(150),
        "blocking cache commit stalled the async runtime for {heartbeat:?}"
    );
    assert_eq!(output.title, "16 CNY");
}

#[tokio::test]
async fn persistence_failure_keeps_provider_success_and_fresh_memory_rate() {
    let dir = TestDir::new("persistence-failure");
    let now = instant(12);
    let provider = Arc::new(QueueProvider::new(vec![Ok(result("USD", "CNY", "8"))]));
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider.clone(),
        cache_with_committer(&dir, now, Arc::new(FailingCommitter)),
        Arc::new(ManualClock::new(now)),
    ));

    assert_eq!(
        service.resolve(intent("2 USD to CNY")).await.unwrap().title,
        "16 CNY"
    );
    assert_eq!(
        service.resolve(intent("3 USD to CNY")).await.unwrap().title,
        "24 CNY"
    );
    assert_eq!(provider.calls(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_completions_persist_serial_snapshots_without_lost_entries() {
    let dir = TestDir::new("serialized-persistence");
    let now = instant(12);
    let provider = Arc::new(QueueProvider::new(vec![
        Ok(result("USD", "CNY", "8")),
        Ok(result("EUR", "JPY", "160")),
    ]));
    let committer = Arc::new(BlockingCommitter::new());
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider,
        cache_with_committer(&dir, now, committer.clone()),
        Arc::new(ManualClock::new(now)),
    ));

    let first_service = service.clone();
    let first = tokio::spawn(async move { first_service.resolve(intent("2 USD to CNY")).await });
    committer.wait_until_entered().await;
    let second_service = service.clone();
    let second = tokio::spawn(async move { second_service.resolve(intent("2 EUR to JPY")).await });
    tokio::task::yield_now().await;
    committer.release();
    committer.wait_until_finished().await;
    committer.wait_until_finished().await;
    assert_eq!(first.await.unwrap().unwrap().title, "16 CNY");
    assert_eq!(second.await.unwrap().unwrap().title, "320 JPY");

    let commits = committer.commits();
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0].0, None);
    assert_eq!(commits[1].0.as_deref(), Some(commits[0].1.as_slice()));
    let first_document = serde_json::from_slice::<serde_json::Value>(&commits[0].1).unwrap();
    let second_document = serde_json::from_slice::<serde_json::Value>(&commits[1].1).unwrap();
    assert_eq!(first_document["entries"].as_array().unwrap().len(), 1);
    assert_eq!(second_document["entries"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn production_constructor_loads_the_owned_cache_without_network_work() {
    let dir = TestDir::new("production-load");
    let service = Arc::new(ExchangeRateService::load(dir.path()).unwrap());

    let output = service.resolve(intent("12.5 USD to USD")).await.unwrap();

    assert_eq!(output.title, "12.5 USD");
}

#[tokio::test]
async fn inverse_leader_broadcasts_the_reciprocal_to_a_direct_follower() {
    let dir = TestDir::new("inverse-leader");
    let now = instant(12);
    let provider = Arc::new(ControlledProvider::new(vec![ControlledOutcome::Success]));
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider.clone(),
        cache(&dir, now),
        Arc::new(ManualClock::new(now)),
    ));

    let inverse_service = service.clone();
    let inverse =
        tokio::spawn(async move { inverse_service.resolve(intent("16 CNY to USD")).await });
    provider.wait_until_called().await;
    let direct_service = service.clone();
    let direct = tokio::spawn(async move { direct_service.resolve(intent("2 USD to CNY")).await });
    tokio::task::yield_now().await;
    provider.release(1);

    assert_eq!(inverse.await.unwrap().unwrap().title, "2 USD");
    assert_eq!(direct.await.unwrap().unwrap().title, "16 CNY");
    assert_eq!(provider.calls(), 1);
}

#[tokio::test]
async fn cancelled_only_caller_still_allows_success_to_populate_cache() {
    let dir = TestDir::new("cancelled-only-caller");
    let now = instant(12);
    let provider = Arc::new(ControlledProvider::new(vec![
        ControlledOutcome::Success,
        ControlledOutcome::Success,
    ]));
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider.clone(),
        cache(&dir, now),
        Arc::new(ManualClock::new(now)),
    ));

    let abandoned_service = service.clone();
    let abandoned =
        tokio::spawn(async move { abandoned_service.resolve(intent("2 USD to CNY")).await });
    provider.wait_until_called().await;
    abandoned.abort();
    provider.release(2);

    let output = tokio::time::timeout(
        StdDuration::from_secs(1),
        service.resolve(intent("3 USD to CNY")),
    )
    .await
    .expect("later cache lookup settled")
    .unwrap();
    assert_eq!(output.title, "24 CNY");
    assert_eq!(provider.calls(), 1);
}

#[tokio::test]
async fn unsupported_and_panicked_provider_terminals_each_allow_a_retry() {
    for (label, first_outcome, first_error) in [
        (
            "unsupported-retry",
            ControlledOutcome::Unsupported,
            CurrencyResolveError::Invalid(CurrencyFailure::InvalidCode),
        ),
        (
            "panic-retry",
            ControlledOutcome::Panic,
            CurrencyResolveError::Unavailable,
        ),
    ] {
        let dir = TestDir::new(label);
        let now = instant(12);
        let provider = Arc::new(ControlledProvider::new(vec![
            first_outcome,
            ControlledOutcome::Success,
        ]));
        let service = Arc::new(ExchangeRateService::with_dependencies(
            provider.clone(),
            cache(&dir, now),
            Arc::new(ManualClock::new(now)),
        ));

        let first_service = service.clone();
        let first =
            tokio::spawn(async move { first_service.resolve(intent("2 USD to CNY")).await });
        provider.wait_until_called().await;
        provider.release(1);
        assert_eq!(first.await.unwrap(), Err(first_error));

        let retry_service = service.clone();
        let retry =
            tokio::spawn(async move { retry_service.resolve(intent("2 USD to CNY")).await });
        provider.wait_until_called().await;
        provider.release(1);
        assert_eq!(retry.await.unwrap().unwrap().title, "16 CNY");
        assert_eq!(provider.calls(), 2);
    }
}

#[tokio::test]
async fn persistence_worker_panic_does_not_change_provider_success() {
    let dir = TestDir::new("persistence-panic");
    let now = instant(12);
    let provider = Arc::new(QueueProvider::new(vec![Ok(result("USD", "CNY", "8"))]));
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider.clone(),
        cache_with_committer(&dir, now, Arc::new(PanickingCommitter)),
        Arc::new(ManualClock::new(now)),
    ));

    assert_eq!(
        service.resolve(intent("2 USD to CNY")).await.unwrap().title,
        "16 CNY"
    );
    assert_eq!(
        service.resolve(intent("3 USD to CNY")).await.unwrap().title,
        "24 CNY"
    );
    assert_eq!(provider.calls(), 1);
}

#[tokio::test]
async fn service_rejects_noncanonical_successes_from_an_injected_provider() {
    for (label, bad_result) in [
        ("mismatched-success", result("EUR", "JPY", "160")),
        ("zero-success", result("USD", "CNY", "0")),
    ] {
        let dir = TestDir::new(label);
        let now = instant(12);
        let provider = Arc::new(QueueProvider::new(vec![Ok(bad_result)]));
        let service = Arc::new(ExchangeRateService::with_dependencies(
            provider.clone(),
            cache(&dir, now),
            Arc::new(ManualClock::new(now)),
        ));

        assert_eq!(
            service.resolve(intent("2 USD to CNY")).await,
            Err(CurrencyResolveError::Unavailable)
        );
        assert_eq!(provider.calls(), 1);
    }
}

#[tokio::test]
async fn successful_fetch_persists_the_completion_time_not_request_start() {
    let dir = TestDir::new("completion-fetch-time");
    let started_at = instant(12);
    let completed_at = started_at + Duration::minutes(30);
    let provider = Arc::new(ControlledProvider::new(vec![ControlledOutcome::Success]));
    let committer = Arc::new(CaptureCommitter::default());
    let clock = Arc::new(ManualClock::new(started_at));
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider.clone(),
        cache_with_committer(&dir, started_at, committer.clone()),
        clock.clone(),
    ));

    let resolve_service = service.clone();
    let resolve =
        tokio::spawn(async move { resolve_service.resolve(intent("2 USD to CNY")).await });
    provider.wait_until_called().await;
    clock.set(completed_at);
    provider.release(1);
    assert_eq!(resolve.await.unwrap().unwrap().title, "16 CNY");
    committer.wait_for_commits(1).await;

    let commits = committer.commits();
    let persisted = serde_json::from_slice::<serde_json::Value>(&commits[0].1).unwrap();
    assert_eq!(persisted["entries"][0]["fetchedAt"], "2026-09-03T12:30:00Z");
}

#[tokio::test]
async fn stale_entry_crossing_seven_days_during_refresh_is_not_returned() {
    let dir = TestDir::new("expires-during-refresh");
    let started_at = instant(12);
    let provider = Arc::new(ControlledProvider::new(vec![
        ControlledOutcome::Unavailable,
    ]));
    let clock = Arc::new(ManualClock::new(started_at));
    let mut seeded = cache(&dir, started_at);
    seeded.store(
        result("USD", "CNY", "8"),
        started_at - Duration::days(7),
        started_at,
    );
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider.clone(),
        seeded,
        clock.clone(),
    ));

    let resolve_service = service.clone();
    let resolve =
        tokio::spawn(async move { resolve_service.resolve(intent("16 CNY to USD")).await });
    provider.wait_until_called().await;
    clock.set(started_at + Duration::SECOND);
    provider.release(1);

    assert_eq!(
        resolve.await.unwrap(),
        Err(CurrencyResolveError::Unavailable)
    );
}

#[tokio::test]
async fn stale_entry_at_exactly_seven_days_on_completion_remains_eligible() {
    let dir = TestDir::new("exact-seven-days-at-completion");
    let started_at = instant(12);
    let fetched_at = started_at - Duration::days(7) + Duration::SECOND;
    let completed_at = started_at + Duration::SECOND;
    let provider = Arc::new(ControlledProvider::new(vec![
        ControlledOutcome::Unavailable,
    ]));
    let clock = Arc::new(ManualClock::new(started_at));
    let mut seeded = cache(&dir, started_at);
    seeded.store(result("USD", "CNY", "8"), fetched_at, started_at);
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider.clone(),
        seeded,
        clock.clone(),
    ));

    let resolve_service = service.clone();
    let resolve =
        tokio::spawn(async move { resolve_service.resolve(intent("16 CNY to USD")).await });
    provider.wait_until_called().await;
    clock.set(completed_at);
    provider.release(1);

    let output = resolve.await.unwrap().unwrap();
    assert_eq!(output.title, "2 USD");
    assert!(output.subtitle.contains("获取于 2026-08-27T12:00:01Z"));
}

#[tokio::test]
async fn coalesced_followers_share_one_completion_timestamp() {
    let dir = TestDir::new("shared-completion-time");
    let started_at = instant(12);
    let completed_at = started_at + Duration::minutes(30);
    let provider = Arc::new(ControlledProvider::new(vec![ControlledOutcome::Success]));
    let committer = Arc::new(CaptureCommitter::default());
    let clock = Arc::new(ManualClock::new(started_at));
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider.clone(),
        cache_with_committer(&dir, started_at, committer.clone()),
        clock.clone(),
    ));

    let leader_service = service.clone();
    let leader = tokio::spawn(async move { leader_service.resolve(intent("2 USD to CNY")).await });
    provider.wait_until_called().await;
    let follower_service = service.clone();
    let follower =
        tokio::spawn(async move { follower_service.resolve(intent("16 CNY to USD")).await });
    tokio::task::yield_now().await;
    clock.set(completed_at);
    provider.release(1);

    assert_eq!(leader.await.unwrap().unwrap().title, "16 CNY");
    assert_eq!(follower.await.unwrap().unwrap().title, "2 USD");
    assert_eq!(provider.calls(), 1);
    assert_eq!(
        clock.reads(),
        3,
        "two request reads and one shared completion read"
    );
    committer.wait_for_commits(1).await;
    let commits = committer.commits();
    let persisted = serde_json::from_slice::<serde_json::Value>(&commits[0].1).unwrap();
    assert_eq!(persisted["entries"][0]["fetchedAt"], "2026-09-03T12:30:00Z");
}

#[tokio::test]
async fn clock_failure_is_unavailable_while_same_code_stays_fully_local() {
    let dir = TestDir::new("clock-failure");
    let now = instant(12);
    let provider = Arc::new(QueueProvider::new(Vec::new()));
    let clock = Arc::new(ManualClock::new(now));
    clock.fail();
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider.clone(),
        cache(&dir, now),
        clock.clone(),
    ));

    assert_eq!(
        service.resolve(intent("2 USD to USD")).await.unwrap().title,
        "2 USD"
    );
    assert_eq!(clock.reads(), 0);
    assert_eq!(
        service.resolve(intent("2 USD to CNY")).await,
        Err(CurrencyResolveError::Unavailable)
    );
    assert_eq!(provider.calls(), 0);

    let provider = Arc::new(ControlledProvider::new(vec![ControlledOutcome::Success]));
    let clock = Arc::new(ManualClock::new(now));
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider.clone(),
        cache(&dir, now),
        clock.clone(),
    ));
    let resolve_service = service.clone();
    let resolve =
        tokio::spawn(async move { resolve_service.resolve(intent("2 USD to CNY")).await });
    provider.wait_until_called().await;
    clock.fail();
    provider.release(1);
    assert_eq!(
        resolve.await.unwrap(),
        Err(CurrencyResolveError::Unavailable)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn blocking_completion_clock_holds_no_cache_or_inflight_lock() {
    let dir = TestDir::new("blocking-completion-clock");
    let now = instant(12);
    let provider = Arc::new(ControlledProvider::new(vec![ControlledOutcome::Success]));
    let clock = Arc::new(BlockingClock::new(now, 2));
    let mut seeded = cache(&dir, now);
    seeded.store(result("EUR", "JPY", "160"), now, now);
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider.clone(),
        seeded,
        clock.clone(),
    ));

    let cold_service = service.clone();
    let cold = tokio::spawn(async move { cold_service.resolve(intent("2 USD to CNY")).await });
    provider.wait_until_called().await;
    provider.release(1);
    clock.wait_until_entered().await;
    let fresh_service = service.clone();
    let mut fresh =
        tokio::spawn(async move { fresh_service.resolve(intent("2 EUR to JPY")).await });
    let prompt = tokio::time::timeout(StdDuration::from_millis(100), &mut fresh).await;
    let completed_promptly = prompt.is_ok();
    clock.release();
    let fresh_output = match prompt {
        Ok(output) => output.unwrap().unwrap(),
        Err(_) => fresh.await.unwrap().unwrap(),
    };

    assert!(
        completed_promptly,
        "fresh lookup was blocked by completion clock"
    );
    assert_eq!(fresh_output.title, "320 JPY");
    assert_eq!(cold.await.unwrap().unwrap().title, "16 CNY");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn historical_request_time_does_not_delete_a_newer_completed_rate() {
    let dir = TestDir::new("historical-request-time");
    let started_at = instant(12);
    let completed_at = instant(13);
    let provider = Arc::new(QueueProvider::new(vec![
        Ok(result("USD", "CNY", "8")),
        Ok(result("USD", "CNY", "8")),
    ]));
    let clock = Arc::new(ScriptedBlockingClock::new(
        vec![started_at, completed_at, completed_at, completed_at],
        1,
    ));
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider.clone(),
        cache(&dir, started_at),
        clock.clone(),
    ));

    let historical_service = service.clone();
    let historical =
        tokio::spawn(async move { historical_service.resolve(intent("2 USD to CNY")).await });
    clock.wait_until_entered().await;

    let current = service.resolve(intent("3 USD to CNY")).await.unwrap();
    clock.release();
    let historical = historical.await.unwrap().unwrap();

    assert_eq!(current.title, "24 CNY");
    assert_eq!(historical.title, "16 CNY");
    assert_eq!(
        provider.calls(),
        1,
        "the historical lookup must reuse the newer in-process rate"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn delayed_older_completion_persists_without_dropping_a_newer_pair() {
    let dir = TestDir::new("out-of-order-completions");
    let started_at = instant(12);
    let older_completion = instant(13);
    let newer_completion = instant(14);
    let provider = Arc::new(QueueProvider::new(vec![
        Ok(result("USD", "CNY", "8")),
        Ok(result("EUR", "JPY", "160")),
    ]));
    let committer = Arc::new(CaptureCommitter::default());
    let clock = Arc::new(ScriptedBlockingClock::new(
        vec![
            started_at,
            older_completion,
            newer_completion,
            newer_completion,
        ],
        2,
    ));
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider.clone(),
        cache_with_committer(&dir, started_at, committer.clone()),
        clock.clone(),
    ));

    let older_service = service.clone();
    let older = tokio::spawn(async move { older_service.resolve(intent("2 USD to CNY")).await });
    clock.wait_until_entered().await;

    let newer = service.resolve(intent("2 EUR to JPY")).await.unwrap();
    clock.release();
    let older = older.await.unwrap().unwrap();
    committer.wait_for_commits(2).await;

    assert_eq!(newer.title, "320 JPY");
    assert_eq!(older.title, "16 CNY");
    assert_eq!(provider.calls(), 2);
    let commits = committer.commits();
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0].0, None);
    assert_eq!(commits[1].0.as_deref(), Some(commits[0].1.as_slice()));
    let second = serde_json::from_slice::<serde_json::Value>(&commits[1].1).unwrap();
    assert_eq!(second["entries"].as_array().unwrap().len(), 2);
    assert!(second["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| { entry["base"] == "EUR" && entry["quote"] == "JPY" }));
}

#[tokio::test]
async fn historical_failure_fallback_reuses_a_later_success() {
    let dir = TestDir::new("historical-failure-fallback");
    let started_at = instant(12);
    let completed_at = instant(13);
    let provider = Arc::new(ControlledProvider::new(vec![
        ControlledOutcome::Unavailable,
        ControlledOutcome::Success,
    ]));
    let clock = Arc::new(ManualClock::new(started_at));
    let service = Arc::new(ExchangeRateService::with_dependencies(
        provider.clone(),
        cache(&dir, started_at),
        clock.clone(),
    ));

    let historical = service.resolve(intent("2 USD to CNY"));
    tokio::pin!(historical);
    tokio::select! {
        result = &mut historical => panic!("historical request completed before provider release: {result:?}"),
        () = provider.wait_until_called() => {}
    }
    provider.release(1);
    while clock.reads() < 2 {
        tokio::task::yield_now().await;
    }

    clock.set(completed_at);
    let current_service = service.clone();
    let current =
        tokio::spawn(async move { current_service.resolve(intent("3 USD to CNY")).await });
    provider.wait_until_called().await;
    provider.release(1);
    assert_eq!(current.await.unwrap().unwrap().title, "24 CNY");

    assert_eq!(historical.await.unwrap().title, "16 CNY");
    assert_eq!(provider.calls(), 2);
}
