use std::{collections::BTreeMap, fs, path::Path, sync::Arc};

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::{
    format_description::well_known::Rfc3339, Date, Duration, Month, OffsetDateTime, Time, UtcOffset,
};

use super::{parse_decimal_exact, CurrencyCode, ExchangeRateRequest, ExchangeRateResult};
use crate::atomic_file::{commit_with_backup, read_optional, AtomicFileError, AtomicPaths};

const CACHE_FILE_NAME: &str = "exchange-rate-cache.json";
const CACHE_SCHEMA: u32 = 1;
const MAX_ENTRIES: usize = 256;
const FRESH_FOR: Duration = Duration::HOUR;
const RETAIN_FOR: Duration = Duration::days(7);

pub(super) trait CacheReader: Send + Sync {
    fn read(&self, path: &Path) -> Result<Option<Vec<u8>>, AtomicFileError>;
}

pub(super) struct AtomicCacheReader;

impl CacheReader for AtomicCacheReader {
    fn read(&self, path: &Path) -> Result<Option<Vec<u8>>, AtomicFileError> {
        read_optional(path)
    }
}

pub(super) trait CacheCommitter: Send + Sync {
    fn commit(
        &self,
        paths: &AtomicPaths,
        previous: Option<&[u8]>,
        candidate: &[u8],
    ) -> Result<(), AtomicFileError>;
}

pub(super) struct AtomicCacheCommitter;

impl CacheCommitter for AtomicCacheCommitter {
    fn commit(
        &self,
        paths: &AtomicPaths,
        previous: Option<&[u8]>,
        candidate: &[u8],
    ) -> Result<(), AtomicFileError> {
        commit_with_backup(paths, previous, candidate)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CachedExchangeRate {
    pub(crate) result: ExchangeRateResult,
    pub(crate) fetched_at: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CacheLookup {
    Fresh(CachedExchangeRate),
    Stale(CachedExchangeRate),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct PairKey {
    first: String,
    second: String,
}

impl PairKey {
    fn new(base: &CurrencyCode, quote: &CurrencyCode) -> Self {
        let (first, second) = if base.as_str() < quote.as_str() {
            (base.as_str(), quote.as_str())
        } else {
            (quote.as_str(), base.as_str())
        };
        Self {
            first: first.into(),
            second: second.into(),
        }
    }
}

#[derive(Clone)]
struct CacheEntry {
    base: CurrencyCode,
    quote: CurrencyCode,
    rate: Decimal,
    effective_date: Date,
    fetched_at: OffsetDateTime,
}

pub(crate) struct ExchangeRateCache {
    paths: AtomicPaths,
    entries: BTreeMap<PairKey, CacheEntry>,
    validated_current_raw: Option<Vec<u8>>,
    persistence_available: bool,
    committer: Arc<dyn CacheCommitter>,
}

impl ExchangeRateCache {
    pub(crate) fn load(root: &Path, now: OffsetDateTime) -> Self {
        Self::load_with(
            root,
            now,
            Arc::new(AtomicCacheReader),
            Arc::new(AtomicCacheCommitter),
        )
    }

    pub(super) fn load_with_reader(
        root: &Path,
        now: OffsetDateTime,
        reader: Arc<dyn CacheReader>,
    ) -> Self {
        Self::load_with(root, now, reader, Arc::new(AtomicCacheCommitter))
    }

    pub(super) fn load_with(
        root: &Path,
        now: OffsetDateTime,
        reader: Arc<dyn CacheReader>,
        committer: Arc<dyn CacheCommitter>,
    ) -> Self {
        let paths = AtomicPaths::new(root, CACHE_FILE_NAME);
        if fs::create_dir_all(root).is_err() {
            return Self::empty(paths, committer, false);
        }

        let current = read_candidate(reader.as_ref(), paths.current(), now);
        if let Candidate::Valid { entries, raw } = current {
            return Self {
                paths,
                entries,
                validated_current_raw: Some(raw),
                persistence_available: true,
                committer,
            };
        }
        let current_read_failed = matches!(current, Candidate::ReadFailed);
        let backup = read_candidate(reader.as_ref(), paths.backup(), now);
        let backup_read_failed = matches!(backup, Candidate::ReadFailed);
        if let Candidate::Valid { entries, .. } = backup {
            return Self {
                paths,
                entries,
                validated_current_raw: None,
                persistence_available: !current_read_failed && !backup_read_failed,
                committer,
            };
        }
        Self::empty(
            paths,
            committer,
            !current_read_failed && !backup_read_failed,
        )
    }

    pub(crate) fn lookup(
        &mut self,
        request: &ExchangeRateRequest,
        now: OffsetDateTime,
    ) -> Option<CacheLookup> {
        let key = PairKey::new(&request.base, &request.quote);
        let entry = self.entries.get(&key)?.clone();
        if now < entry.fetched_at || now - entry.fetched_at > RETAIN_FOR {
            self.entries.remove(&key);
            return None;
        }

        let rate = if entry.base == request.base && entry.quote == request.quote {
            entry.rate
        } else {
            Decimal::ONE.checked_div(entry.rate)?
        };
        if rate <= Decimal::ZERO {
            return None;
        }
        let cached = CachedExchangeRate {
            result: ExchangeRateResult {
                base: request.base.clone(),
                quote: request.quote.clone(),
                rate,
                effective_date: entry.effective_date,
            },
            fetched_at: entry.fetched_at,
        };
        if now - entry.fetched_at <= FRESH_FOR {
            Some(CacheLookup::Fresh(cached))
        } else {
            Some(CacheLookup::Stale(cached))
        }
    }

    pub(crate) fn store(
        &mut self,
        result: ExchangeRateResult,
        fetched_at: OffsetDateTime,
        now: OffsetDateTime,
    ) {
        self.prune(now);
        if fetched_at > now
            || now - fetched_at > RETAIN_FOR
            || result.base == result.quote
            || result.rate <= Decimal::ZERO
        {
            return;
        }
        let key = PairKey::new(&result.base, &result.quote);
        self.entries.insert(
            key,
            CacheEntry {
                base: result.base,
                quote: result.quote,
                rate: result.rate,
                effective_date: result.effective_date,
                fetched_at,
            },
        );
        self.evict_oldest();
        self.persist();
    }

    fn prune(&mut self, now: OffsetDateTime) {
        self.entries
            .retain(|_, entry| entry.fetched_at <= now && now - entry.fetched_at <= RETAIN_FOR);
    }

    fn evict_oldest(&mut self) {
        while self.entries.len() > MAX_ENTRIES {
            let oldest = self
                .entries
                .iter()
                .min_by(|(left_key, left), (right_key, right)| {
                    left.fetched_at
                        .cmp(&right.fetched_at)
                        .then_with(|| left_key.cmp(right_key))
                })
                .map(|(key, _)| key.clone())
                .expect("an oversized cache has an oldest entry");
            self.entries.remove(&oldest);
        }
    }

    fn persist(&mut self) {
        if !self.persistence_available {
            return;
        }
        let Some(candidate) = serialize_document(&self.entries) else {
            self.persistence_available = false;
            return;
        };
        if self
            .committer
            .commit(
                &self.paths,
                self.validated_current_raw.as_deref(),
                &candidate,
            )
            .is_err()
        {
            self.persistence_available = false;
            return;
        }
        self.validated_current_raw = Some(candidate);
    }

    fn empty(
        paths: AtomicPaths,
        committer: Arc<dyn CacheCommitter>,
        persistence_available: bool,
    ) -> Self {
        Self {
            paths,
            entries: BTreeMap::new(),
            validated_current_raw: None,
            persistence_available,
            committer,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CacheDocument {
    schema: u32,
    entries: Vec<SerializedEntry>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SerializedEntry {
    base: String,
    quote: String,
    rate: String,
    effective_date: String,
    fetched_at: String,
}

enum Candidate {
    Missing,
    Valid {
        entries: BTreeMap<PairKey, CacheEntry>,
        raw: Vec<u8>,
    },
    Invalid,
    ReadFailed,
}

fn read_candidate(reader: &dyn CacheReader, path: &Path, now: OffsetDateTime) -> Candidate {
    match reader.read(path) {
        Ok(None) => Candidate::Missing,
        Ok(Some(raw)) => match parse_document(&raw, now) {
            Some(entries) => Candidate::Valid { entries, raw },
            None => Candidate::Invalid,
        },
        Err(_) => Candidate::ReadFailed,
    }
}

fn parse_document(bytes: &[u8], now: OffsetDateTime) -> Option<BTreeMap<PairKey, CacheEntry>> {
    let document = serde_json::from_slice::<CacheDocument>(bytes).ok()?;
    if document.schema != CACHE_SCHEMA || document.entries.len() > MAX_ENTRIES {
        return None;
    }

    let mut entries = BTreeMap::new();
    for serialized in document.entries {
        let base = CurrencyCode::new(&serialized.base)?;
        let quote = CurrencyCode::new(&serialized.quote)?;
        if base.as_str() != serialized.base || quote.as_str() != serialized.quote || base == quote {
            return None;
        }
        let rate = parse_decimal_exact(&serialized.rate)?;
        if rate <= Decimal::ZERO || rate.normalize().to_string() != serialized.rate {
            return None;
        }
        let effective_date = parse_iso_date(&serialized.effective_date)?;
        let fetched_at = parse_utc_timestamp(&serialized.fetched_at)?;
        if fetched_at > now {
            return None;
        }
        let key = PairKey::new(&base, &quote);
        if entries.contains_key(&key) {
            return None;
        }
        if now - fetched_at <= RETAIN_FOR {
            entries.insert(
                key,
                CacheEntry {
                    base,
                    quote,
                    rate,
                    effective_date,
                    fetched_at,
                },
            );
        }
    }
    Some(entries)
}

fn serialize_document(entries: &BTreeMap<PairKey, CacheEntry>) -> Option<Vec<u8>> {
    let entries = entries
        .values()
        .map(|entry| {
            Some(SerializedEntry {
                base: entry.base.as_str().into(),
                quote: entry.quote.as_str().into(),
                rate: entry.rate.normalize().to_string(),
                effective_date: entry.effective_date.to_string(),
                fetched_at: entry
                    .fetched_at
                    .to_offset(UtcOffset::UTC)
                    .format(&Rfc3339)
                    .ok()?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    serde_json::to_vec(&CacheDocument {
        schema: CACHE_SCHEMA,
        entries,
    })
    .ok()
}

fn parse_iso_date(source: &str) -> Option<Date> {
    if source.len() != 10
        || source.as_bytes().get(4) != Some(&b'-')
        || source.as_bytes().get(7) != Some(&b'-')
        || source
            .bytes()
            .enumerate()
            .any(|(index, byte)| index != 4 && index != 7 && !byte.is_ascii_digit())
    {
        return None;
    }
    let year = source.get(..4)?.parse::<i32>().ok()?;
    let month = Month::try_from(source.get(5..7)?.parse::<u8>().ok()?).ok()?;
    let day = source.get(8..)?.parse::<u8>().ok()?;
    (year >= 1)
        .then_some(())
        .and_then(|()| Date::from_calendar_date(year, month, day).ok())
}

fn parse_utc_timestamp(source: &str) -> Option<OffsetDateTime> {
    let source = source.strip_suffix('Z')?;
    let (date, time) = source.split_once('T')?;
    let date = parse_iso_date(date)?;
    let (whole_time, fraction) = time
        .split_once('.')
        .map_or((time, None), |(whole, fraction)| (whole, Some(fraction)));
    if whole_time.len() != 8
        || whole_time.as_bytes().get(2) != Some(&b':')
        || whole_time.as_bytes().get(5) != Some(&b':')
        || whole_time
            .bytes()
            .enumerate()
            .any(|(index, byte)| index != 2 && index != 5 && !byte.is_ascii_digit())
    {
        return None;
    }
    let hour = whole_time.get(..2)?.parse::<u8>().ok()?;
    let minute = whole_time.get(3..5)?.parse::<u8>().ok()?;
    let second = whole_time.get(6..)?.parse::<u8>().ok()?;
    let nanosecond = match fraction {
        None => 0,
        Some(value)
            if !value.is_empty()
                && value.len() <= 9
                && value.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            value.parse::<u32>().ok()? * 10_u32.pow(u32::try_from(9 - value.len()).ok()?)
        }
        Some(_) => return None,
    };
    let time = Time::from_hms_nano(hour, minute, second, nanosecond).ok()?;
    Some(date.with_time(time).assume_utc())
}
