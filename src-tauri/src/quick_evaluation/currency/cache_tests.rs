use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use rust_decimal::Decimal;
use serde_json::{json, Value};
use time::{Date, Duration, Month, OffsetDateTime};

use super::{
    cache::{AtomicCacheReader, CacheCommitter, CacheLookup, CacheReader, ExchangeRateCache},
    CurrencyCode, ExchangeRateRequest, ExchangeRateResult,
};
use crate::atomic_file::{commit_with_backup, AtomicFileError, AtomicPaths};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "uipilot-exchange-rate-cache-{label}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn current(&self) -> PathBuf {
        self.0.join("exchange-rate-cache.json")
    }

    fn backup(&self) -> PathBuf {
        self.0.join("exchange-rate-cache.json.backup")
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        if self.0.exists() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

fn instant(hour: u8, minute: u8, second: u8) -> OffsetDateTime {
    Date::from_calendar_date(2026, Month::September, 3)
        .unwrap()
        .with_hms(hour, minute, second)
        .unwrap()
        .assume_utc()
}

fn code(value: &str) -> CurrencyCode {
    CurrencyCode::new(value).unwrap()
}

fn request(base: &str, quote: &str) -> ExchangeRateRequest {
    ExchangeRateRequest {
        base: code(base),
        quote: code(quote),
    }
}

fn result(base: &str, quote: &str, rate: &str) -> ExchangeRateResult {
    ExchangeRateResult {
        base: code(base),
        quote: code(quote),
        rate: Decimal::from_str_exact(rate).unwrap(),
        effective_date: Date::from_calendar_date(2026, Month::September, 2).unwrap(),
    }
}

fn lookup_rate(lookup: &CacheLookup) -> Decimal {
    match lookup {
        CacheLookup::Fresh(cached) | CacheLookup::Stale(cached) => cached.result.rate,
    }
}

fn entry(base: &str, quote: &str, rate: &str, fetched_at: &str) -> Value {
    json!({
        "base": base,
        "quote": quote,
        "rate": rate,
        "effectiveDate": "2026-09-02",
        "fetchedAt": fetched_at,
    })
}

fn document(schema: u32, entries: Vec<Value>) -> Vec<u8> {
    serde_json::to_vec(&json!({ "schema": schema, "entries": entries })).unwrap()
}

fn cached_rate(cache: &mut ExchangeRateCache, base: &str, quote: &str) -> Option<Decimal> {
    cache
        .lookup(&request(base, quote), instant(12, 0, 0))
        .map(|lookup| lookup_rate(&lookup))
}

struct FixtureReader {
    current: Result<Option<Vec<u8>>, AtomicFileError>,
    backup: Result<Option<Vec<u8>>, AtomicFileError>,
    calls: Mutex<Vec<String>>,
}

impl FixtureReader {
    fn new(
        current: Result<Option<Vec<u8>>, AtomicFileError>,
        backup: Result<Option<Vec<u8>>, AtomicFileError>,
    ) -> Self {
        Self {
            current,
            backup,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl CacheReader for FixtureReader {
    fn read(&self, path: &Path) -> Result<Option<Vec<u8>>, AtomicFileError> {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        self.calls.lock().unwrap().push(name.clone());
        if name.ends_with(".backup") {
            self.backup.clone()
        } else {
            self.current.clone()
        }
    }
}

#[derive(Default)]
struct CaptureCommitter {
    commits: Mutex<Vec<(Option<Vec<u8>>, Vec<u8>)>>,
}

impl CaptureCommitter {
    fn commits(&self) -> Vec<(Option<Vec<u8>>, Vec<u8>)> {
        self.commits.lock().unwrap().clone()
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
        Ok(())
    }
}

struct FailNextCommitter {
    fail_next: AtomicBool,
    calls: AtomicU64,
}

impl FailNextCommitter {
    fn new() -> Self {
        Self {
            fail_next: AtomicBool::new(false),
            calls: AtomicU64::new(0),
        }
    }

    fn fail_next(&self) {
        self.fail_next.store(true, Ordering::SeqCst);
    }

    fn calls(&self) -> u64 {
        self.calls.load(Ordering::SeqCst)
    }
}

impl CacheCommitter for FailNextCommitter {
    fn commit(
        &self,
        paths: &AtomicPaths,
        previous: Option<&[u8]>,
        candidate: &[u8],
    ) -> Result<(), AtomicFileError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_next.swap(false, Ordering::SeqCst) {
            return Err(AtomicFileError::CandidateWrite);
        }
        commit_with_backup(paths, previous, candidate)
    }
}

#[test]
fn direct_and_reverse_lookups_share_one_pair_and_preserve_requested_orientation() {
    let dir = TestDir::new("reverse");
    let fetched_at = instant(12, 0, 0);
    let mut cache = ExchangeRateCache::load(dir.path(), fetched_at);
    cache.store(result("USD", "CNY", "8"), fetched_at, fetched_at);

    let direct = cache
        .lookup(&request("USD", "CNY"), fetched_at)
        .expect("direct entry");
    let reverse = cache
        .lookup(&request("CNY", "USD"), fetched_at)
        .expect("reverse entry");

    assert_eq!(lookup_rate(&direct), Decimal::from(8));
    assert_eq!(
        lookup_rate(&reverse),
        Decimal::from_str_exact("0.125").unwrap()
    );
    let CacheLookup::Fresh(reverse) = reverse else {
        panic!("reverse entry should be fresh");
    };
    assert_eq!(reverse.result.base.as_str(), "CNY");
    assert_eq!(reverse.result.quote.as_str(), "USD");
    assert_eq!(reverse.result.effective_date.to_string(), "2026-09-02");
    assert_eq!(reverse.fetched_at, fetched_at);
}

#[test]
fn exact_one_hour_and_seven_day_lookup_boundaries_are_inclusive() {
    let dir = TestDir::new("boundaries");
    let fetched_at = instant(12, 0, 0);
    let mut cache = ExchangeRateCache::load(dir.path(), fetched_at);
    cache.store(result("USD", "CNY", "7.1"), fetched_at, fetched_at);

    assert!(matches!(
        cache.lookup(&request("USD", "CNY"), fetched_at + Duration::HOUR),
        Some(CacheLookup::Fresh(_))
    ));
    assert!(matches!(
        cache.lookup(
            &request("USD", "CNY"),
            fetched_at + Duration::HOUR + Duration::SECOND
        ),
        Some(CacheLookup::Stale(_))
    ));
    assert!(matches!(
        cache.lookup(&request("USD", "CNY"), fetched_at + Duration::days(7)),
        Some(CacheLookup::Stale(_))
    ));
    assert_eq!(
        cache.lookup(
            &request("USD", "CNY"),
            fetched_at + Duration::days(7) + Duration::SECOND
        ),
        None
    );
    assert_eq!(
        cache.lookup(&request("USD", "CNY"), fetched_at + Duration::HOUR),
        None,
        "expired lookup removes the entry"
    );
}

#[test]
fn a_future_fetch_timestamp_is_never_treated_as_fresh() {
    let dir = TestDir::new("future");
    let now = instant(12, 0, 0);
    let mut cache = ExchangeRateCache::load(dir.path(), now);

    cache.store(result("USD", "CNY", "7.1"), now + Duration::SECOND, now);

    assert_eq!(cache.lookup(&request("USD", "CNY"), now), None);
}

#[test]
fn load_reads_current_first_and_does_not_consult_backup_when_current_is_valid() {
    let dir = TestDir::new("current-first");
    let reader = Arc::new(FixtureReader::new(
        Ok(Some(document(
            1,
            vec![entry("USD", "CNY", "7.1", "2026-09-03T11:00:00Z")],
        ))),
        Ok(Some(document(
            1,
            vec![entry("USD", "CNY", "6.9", "2026-09-03T11:00:00Z")],
        ))),
    ));

    let mut cache =
        ExchangeRateCache::load_with_reader(dir.path(), instant(12, 0, 0), reader.clone());

    assert_eq!(
        cached_rate(&mut cache, "USD", "CNY"),
        Some(Decimal::from_str_exact("7.1").unwrap())
    );
    assert_eq!(reader.calls(), vec!["exchange-rate-cache.json"]);
}

#[test]
fn load_tries_backup_after_missing_or_corrupt_current() {
    for (label, current) in [
        ("missing", Ok(None)),
        ("corrupt", Ok(Some(b"not-json".to_vec()))),
    ] {
        let dir = TestDir::new(label);
        let reader = Arc::new(FixtureReader::new(
            current,
            Ok(Some(document(
                1,
                vec![entry("USD", "CNY", "6.9", "2026-09-03T11:00:00Z")],
            ))),
        ));
        let mut cache =
            ExchangeRateCache::load_with_reader(dir.path(), instant(12, 0, 0), reader.clone());

        assert_eq!(
            cached_rate(&mut cache, "USD", "CNY"),
            Some(Decimal::from_str_exact("6.9").unwrap()),
            "{label}"
        );
        assert_eq!(
            reader.calls(),
            vec![
                "exchange-rate-cache.json",
                "exchange-rate-cache.json.backup"
            ],
            "{label}"
        );
    }
}

#[test]
fn two_unusable_files_and_unknown_schema_degrade_to_an_empty_cache() {
    let dir = TestDir::new("both-corrupt");
    fs::write(dir.current(), b"bad-current").unwrap();
    fs::write(dir.backup(), b"bad-backup").unwrap();
    let mut cache = ExchangeRateCache::load(dir.path(), instant(12, 0, 0));
    assert_eq!(cached_rate(&mut cache, "USD", "CNY"), None);

    let dir = TestDir::new("unknown-schema");
    fs::write(
        dir.current(),
        document(2, vec![entry("USD", "CNY", "7.1", "2026-09-03T11:00:00Z")]),
    )
    .unwrap();
    let mut cache = ExchangeRateCache::load(dir.path(), instant(12, 0, 0));
    assert_eq!(cached_rate(&mut cache, "USD", "CNY"), None);
}

#[test]
fn malformed_or_impossible_entry_fields_invalidate_the_document() {
    let cases = [
        (
            "lowercase-code",
            entry("usd", "CNY", "7.1", "2026-09-03T11:00:00Z"),
        ),
        (
            "same-code",
            entry("USD", "USD", "7.1", "2026-09-03T11:00:00Z"),
        ),
        (
            "noncanonical-rate",
            entry("USD", "CNY", "7.10", "2026-09-03T11:00:00Z"),
        ),
        (
            "zero-rate",
            entry("USD", "CNY", "0", "2026-09-03T11:00:00Z"),
        ),
        (
            "impossible-date",
            json!({
                "base": "USD", "quote": "CNY", "rate": "7.1",
                "effectiveDate": "2026-02-30", "fetchedAt": "2026-09-03T11:00:00Z"
            }),
        ),
        (
            "signed-year-date",
            json!({
                "base": "USD", "quote": "CNY", "rate": "7.1",
                "effectiveDate": "+026-09-02", "fetchedAt": "2026-09-03T11:00:00Z"
            }),
        ),
        (
            "impossible-timestamp",
            entry("USD", "CNY", "7.1", "2026-13-03T11:00:00Z"),
        ),
        (
            "signed-hour-timestamp",
            entry("USD", "CNY", "7.1", "2026-09-03T+1:00:00Z"),
        ),
        (
            "offset-not-utc-z",
            entry("USD", "CNY", "7.1", "2026-09-03T11:00:00+00:00"),
        ),
        (
            "future-timestamp",
            entry("USD", "CNY", "7.1", "2026-09-03T12:00:01Z"),
        ),
        (
            "forbidden-extra-field",
            json!({
                "base": "USD", "quote": "CNY", "rate": "7.1",
                "effectiveDate": "2026-09-02", "fetchedAt": "2026-09-03T11:00:00Z",
                "amount": "100"
            }),
        ),
    ];

    for (label, invalid_entry) in cases {
        let dir = TestDir::new(label);
        fs::write(dir.current(), document(1, vec![invalid_entry])).unwrap();
        let mut cache = ExchangeRateCache::load(dir.path(), instant(12, 0, 0));
        assert_eq!(cached_rate(&mut cache, "USD", "CNY"), None, "{label}");
    }
}

#[test]
fn duplicate_unordered_pairs_or_more_than_256_entries_invalidate_the_document() {
    let dir = TestDir::new("duplicate-pair");
    fs::write(
        dir.current(),
        document(
            1,
            vec![
                entry("USD", "CNY", "7.1", "2026-09-03T11:00:00Z"),
                entry("CNY", "USD", "0.14", "2026-09-03T11:00:00Z"),
            ],
        ),
    )
    .unwrap();
    let mut cache = ExchangeRateCache::load(dir.path(), instant(12, 0, 0));
    assert_eq!(cached_rate(&mut cache, "USD", "CNY"), None);

    let dir = TestDir::new("too-many");
    let entries = (0..257)
        .map(|index| {
            let middle = char::from(b'A' + u8::try_from(index / 26).unwrap());
            let last = char::from(b'A' + u8::try_from(index % 26).unwrap());
            entry(
                "AAA",
                &format!("B{middle}{last}"),
                "1",
                "2026-09-03T11:00:00Z",
            )
        })
        .collect();
    fs::write(dir.current(), document(1, entries)).unwrap();
    let mut cache = ExchangeRateCache::load(dir.path(), instant(12, 0, 0));
    assert_eq!(cached_rate(&mut cache, "AAA", "BAA"), None);
}

#[test]
fn loading_prunes_expired_entries_but_keeps_the_valid_snapshot() {
    let dir = TestDir::new("load-prune");
    fs::write(
        dir.current(),
        document(
            1,
            vec![
                entry("USD", "CNY", "7.1", "2026-08-27T11:59:59Z"),
                entry("EUR", "JPY", "160", "2026-09-03T11:00:00Z"),
            ],
        ),
    )
    .unwrap();

    let mut cache = ExchangeRateCache::load(dir.path(), instant(12, 0, 0));

    assert_eq!(cached_rate(&mut cache, "USD", "CNY"), None);
    assert_eq!(
        cached_rate(&mut cache, "EUR", "JPY"),
        Some(Decimal::from(160))
    );
}

#[test]
fn persisted_json_is_versioned_canonical_and_contains_only_cache_metadata() {
    let dir = TestDir::new("serialized-contract");
    let fetched_at = instant(12, 0, 0);
    let mut cache = ExchangeRateCache::load(dir.path(), fetched_at);

    cache.store(result("USD", "CNY", "7.1000"), fetched_at, fetched_at);

    let raw = fs::read(dir.current()).unwrap();
    let value = serde_json::from_slice::<Value>(&raw).unwrap();
    let root = value.as_object().unwrap();
    assert_eq!(
        root.keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        ["entries", "schema"].into_iter().collect()
    );
    assert_eq!(root["schema"], 1);
    let stored = root["entries"].as_array().unwrap()[0].as_object().unwrap();
    assert_eq!(
        stored
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        ["base", "effectiveDate", "fetchedAt", "quote", "rate"]
            .into_iter()
            .collect()
    );
    assert_eq!(stored["base"], "USD");
    assert_eq!(stored["quote"], "CNY");
    assert_eq!(stored["rate"], "7.1");
    assert_eq!(stored["effectiveDate"], "2026-09-02");
    assert_eq!(stored["fetchedAt"], "2026-09-03T12:00:00Z");
    let text = String::from_utf8(raw).unwrap();
    for forbidden in ["amount", "rawQuery", "result", "invocation", "history"] {
        assert!(!text.contains(forbidden), "forbidden field {forbidden}");
    }

    let mut restarted = ExchangeRateCache::load(dir.path(), fetched_at);
    assert_eq!(
        cached_rate(&mut restarted, "USD", "CNY"),
        Some(Decimal::from_str_exact("7.1").unwrap())
    );
}

#[test]
fn commit_backs_up_only_bytes_from_a_validated_current_file() {
    let dir = TestDir::new("validated-current-backup");
    let valid = document(1, vec![entry("USD", "CNY", "7.1", "2026-09-03T11:00:00Z")]);
    let exact_current = [b"\n".as_slice(), valid.as_slice(), b"\n".as_slice()].concat();
    fs::write(dir.current(), &exact_current).unwrap();
    let mut cache = ExchangeRateCache::load(dir.path(), instant(12, 0, 0));

    cache.store(
        result("EUR", "JPY", "160"),
        instant(12, 0, 0),
        instant(12, 0, 0),
    );

    assert_eq!(fs::read(dir.backup()).unwrap(), exact_current);

    let dir = TestDir::new("invalid-current-not-backed-up");
    let backup = document(1, vec![entry("USD", "CNY", "7.1", "2026-09-03T11:00:00Z")]);
    fs::write(dir.current(), b"bad-current").unwrap();
    fs::write(dir.backup(), &backup).unwrap();
    let mut cache = ExchangeRateCache::load(dir.path(), instant(12, 0, 0));

    cache.store(
        result("EUR", "JPY", "160"),
        instant(12, 0, 0),
        instant(12, 0, 0),
    );

    assert_eq!(fs::read(dir.backup()).unwrap(), backup);
    assert!(serde_json::from_slice::<Value>(&fs::read(dir.current()).unwrap()).is_ok());
}

#[test]
fn write_prunes_expired_entries_and_evicts_the_oldest_fetch_beyond_256() {
    let dir = TestDir::new("write-prune");
    let first_fetch = instant(12, 0, 0);
    let mut cache = ExchangeRateCache::load(dir.path(), first_fetch);
    cache.store(result("USD", "CNY", "7.1"), first_fetch, first_fetch);
    let later = first_fetch + Duration::days(7) + Duration::SECOND;
    cache.store(result("EUR", "JPY", "160"), later, later);
    let persisted = serde_json::from_slice::<Value>(&fs::read(dir.current()).unwrap()).unwrap();
    assert_eq!(persisted["entries"].as_array().unwrap().len(), 1);
    assert_eq!(persisted["entries"][0]["base"], "EUR");

    cache.store(
        result("GBP", "CAD", "1.8"),
        later - Duration::days(7) - Duration::SECOND,
        later,
    );
    let persisted = serde_json::from_slice::<Value>(&fs::read(dir.current()).unwrap()).unwrap();
    assert_eq!(persisted["entries"].as_array().unwrap().len(), 1);
    assert_eq!(persisted["entries"][0]["base"], "EUR");

    let dir = TestDir::new("eviction");
    let committer = Arc::new(CaptureCommitter::default());
    let now = instant(12, 0, 0);
    let reader = Arc::new(FixtureReader::new(Ok(None), Ok(None)));
    let mut cache = ExchangeRateCache::load_with(dir.path(), now, reader, committer.clone());
    let start = instant(11, 0, 0);
    for index in 0..257 {
        let middle = char::from(b'A' + u8::try_from(index / 26).unwrap());
        let last = char::from(b'A' + u8::try_from(index % 26).unwrap());
        cache.store(
            result("AAA", &format!("B{middle}{last}"), "1"),
            start + Duration::seconds(i64::from(index)),
            now,
        );
    }

    assert_eq!(cached_rate(&mut cache, "AAA", "BAA"), None);
    assert_eq!(cached_rate(&mut cache, "AAA", "BAB"), Some(Decimal::ONE));
    let commits = committer.commits();
    let (_, last_candidate) = commits.last().unwrap();
    let persisted = serde_json::from_slice::<Value>(last_candidate).unwrap();
    assert_eq!(persisted["entries"].as_array().unwrap().len(), 256);
}

#[test]
fn read_failure_disables_persistence_but_validated_memory_data_remains_usable() {
    let dir = TestDir::new("read-failure");
    let reader = Arc::new(FixtureReader::new(
        Err(AtomicFileError::Read),
        Ok(Some(document(
            1,
            vec![entry("USD", "CNY", "7.1", "2026-09-03T11:00:00Z")],
        ))),
    ));
    let committer = Arc::new(CaptureCommitter::default());
    let now = instant(12, 0, 0);
    let mut cache =
        ExchangeRateCache::load_with(dir.path(), now, reader.clone(), committer.clone());
    assert_eq!(
        cached_rate(&mut cache, "USD", "CNY"),
        Some(Decimal::from_str_exact("7.1").unwrap())
    );

    cache.store(result("EUR", "JPY", "160"), now, now);

    assert_eq!(
        cached_rate(&mut cache, "EUR", "JPY"),
        Some(Decimal::from(160))
    );
    assert!(committer.commits().is_empty());
    assert_eq!(
        reader.calls(),
        vec![
            "exchange-rate-cache.json",
            "exchange-rate-cache.json.backup"
        ]
    );
}

#[test]
fn atomic_write_failure_preserves_disk_and_keeps_new_network_data_in_memory() {
    let dir = TestDir::new("atomic-failure");
    let committer = Arc::new(FailNextCommitter::new());
    let now = instant(12, 0, 0);
    let mut cache = ExchangeRateCache::load_with(
        dir.path(),
        now,
        Arc::new(AtomicCacheReader),
        committer.clone(),
    );
    cache.store(result("USD", "CNY", "7.1"), now, now);
    cache.store(result("EUR", "JPY", "160"), now, now);
    let current_before = fs::read(dir.current()).unwrap();
    let backup_before = fs::read(dir.backup()).unwrap();
    committer.fail_next();

    cache.store(result("GBP", "CAD", "1.8"), now, now);

    assert_eq!(fs::read(dir.current()).unwrap(), current_before);
    assert_eq!(fs::read(dir.backup()).unwrap(), backup_before);
    assert_eq!(
        cached_rate(&mut cache, "GBP", "CAD"),
        Some(Decimal::from_str_exact("1.8").unwrap())
    );
    cache.store(result("AUD", "NZD", "1.1"), now, now);
    assert_eq!(
        committer.calls(),
        3,
        "persistence stays unavailable after failure"
    );
    assert_eq!(
        cached_rate(&mut cache, "AUD", "NZD"),
        Some(Decimal::from_str_exact("1.1").unwrap())
    );

    let mut restarted = ExchangeRateCache::load(dir.path(), now);
    assert_eq!(cached_rate(&mut restarted, "GBP", "CAD"), None);
    assert_eq!(
        cached_rate(&mut restarted, "EUR", "JPY"),
        Some(Decimal::from(160))
    );
}

#[test]
fn checked_reverse_handles_the_smallest_positive_decimal_without_a_panic() {
    let dir = TestDir::new("inverse-smallest");
    let now = instant(12, 0, 0);
    let mut cache = ExchangeRateCache::load(dir.path(), now);
    cache.store(
        result("USD", "CNY", "0.0000000000000000000000000001"),
        now,
        now,
    );

    let reverse = cache
        .lookup(&request("CNY", "USD"), now)
        .expect("the reciprocal remains representable");
    assert_eq!(
        lookup_rate(&reverse),
        Decimal::from_str_exact("10000000000000000000000000000").unwrap()
    );
    assert!(cache.lookup(&request("USD", "CNY"), now).is_some());
}

#[test]
fn reverse_lookup_never_returns_a_zero_rate_after_decimal_underflow() {
    let dir = TestDir::new("inverse-underflow");
    let now = instant(12, 0, 0);
    let mut cache = ExchangeRateCache::load(dir.path(), now);
    cache.store(
        result("USD", "CNY", "79228162514264337593543950335"),
        now,
        now,
    );

    assert_eq!(cache.lookup(&request("CNY", "USD"), now), None);
    assert!(cache.lookup(&request("USD", "CNY"), now).is_some());
}
