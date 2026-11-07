//! Private Spotlight candidate gathering. The host's serial worker supplies the approved scope
//! and a fresh cancellation token per request. Only the Find controller may authorize or publish
//! these paths, after rechecking visibility, scope revision, query ownership and file identity.

use crate::macos_find_results::IndexedLocalScope;
use block2::RcBlock;
use objc2::{
    rc::{autoreleasepool, Retained},
    runtime::{AnyObject, NSObjectProtocol, ProtocolObject},
};
use objc2_foundation::{
    NSArray, NSDate, NSDefaultRunLoopMode, NSMetadataItemFSContentChangeDateKey,
    NSMetadataItem, NSMetadataItemFSNameKey, NSMetadataItemPathKey, NSMetadataQuery,
    NSMetadataQueryDidFinishGatheringNotification,
    NSNotification, NSNotificationCenter, NSPredicate, NSRunLoop, NSSortDescriptor, NSString,
    NSThread, NSURL,
};
use std::{
    path::{Path, PathBuf},
    ptr::NonNull,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const CANDIDATE_LIMIT: usize = 400;
const QUERY_TIMEOUT: Duration = Duration::from_secs(3);
// Stop gathering a little early so unfinished queries can yield a bounded partial snapshot.
const SNAPSHOT_RESERVE: Duration = Duration::from_millis(50);
const RUN_LOOP_STEP: Duration = Duration::from_millis(15);

#[derive(Clone, Copy)]
pub(crate) enum ApprovedScope<'a> {
    IndexedLocal(&'a IndexedLocalScope),
    Directory(&'a Path),
}

#[derive(Clone, Default)]
pub(crate) struct QueryCancellation(Arc<AtomicBool>);

impl QueryCancellation {
    pub(crate) fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub(crate) fn check(&self) -> Result<(), GatherError> {
        if self.0.load(Ordering::Acquire) {
            Err(GatherError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum GatherError {
    InvalidInput,
    ServiceUnavailable,
    Cancelled,
    TimedOut,
}

#[derive(Debug)]
pub(crate) struct Candidates {
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) partial: bool,
    // Even when candidates exist, timing out without *authorized* rows is a retryable error.
    pub(crate) timed_out: bool,
}

fn filename_predicate(text: &str) -> Result<Retained<NSPredicate>, GatherError> {
    if text.is_empty() || text.len() > 1024 || text.chars().count() > 255 || text.contains('\0') {
        return Err(GatherError::InvalidInput);
    }
    let value = NSString::from_str(text);
    // SAFETY: both arguments are NSString; only the fixed attribute is interpreted as a key.
    unsafe {
        let args = NSArray::<AnyObject>::from_slice(&[NSMetadataItemFSNameKey, &value]);
        Ok(NSPredicate::predicateWithFormat_argumentArray(
            &NSString::from_str("%K CONTAINS[cd] %@"),
            Some(&args),
        ))
    }
}

struct NativeQuery {
    query: Retained<NSMetadataQuery>,
    center: Retained<NSNotificationCenter>,
    observer: Retained<ProtocolObject<dyn NSObjectProtocol>>,
    finished: Arc<AtomicBool>,
    approved_paths: Vec<PathBuf>,
    system_query: bool,
}

impl NativeQuery {
    fn new(scope: ApprovedScope<'_>, text: &str) -> Result<Self, GatherError> {
        let predicate = filename_predicate(text)?;
        // Host-owned scopes are installed before startQuery. kMDItemPath is returned as
        // metadata but adding BEGINSWITH to the Spotlight predicate drops real font hits.
        #[cfg(feature = "macos-find-system-qa")]
        let system_query = matches!(&scope, ApprovedScope::IndexedLocal(scope) if scope.uses_system_query());
        #[cfg(not(feature = "macos-find-system-qa"))]
        let system_query = false;
        let approved_paths: Vec<PathBuf> = match scope {
            ApprovedScope::IndexedLocal(scope) => scope.query_paths().map_err(|_| GatherError::ServiceUnavailable)?,
            ApprovedScope::Directory(path) => vec![path.to_owned()],
        };
        if approved_paths.is_empty() || approved_paths.iter().any(|path| !path.is_absolute()) {
            return Err(GatherError::InvalidInput);
        }
        let scopes = if system_query {
            // SAFETY: this Apple scope constant is immutable. Only isolated QA scopes
            // that passed account-home and mount preflight may select it.
            NSArray::<AnyObject>::from_slice(&[unsafe { objc2_foundation::NSMetadataQueryIndexedLocalComputerScope }])
        } else {
            let urls = approved_paths.iter().map(|path| NSURL::from_directory_path(path)
                .ok_or(GatherError::InvalidInput)).collect::<Result<Vec<_>, _>>()?;
            let objects: Vec<&AnyObject> = urls.iter().map(|url| &***url as &AnyObject).collect();
            NSArray::<AnyObject>::from_slice(&objects)
        };

        let query = NSMetadataQuery::new();
        query.setPredicate(Some(&predicate));
        // SAFETY: all scopes are host-owned file URLs or the verified Foundation QA scope.
        unsafe { query.setSearchScopes(&scopes) };
        // SAFETY: Foundation exports an immortal NSString attribute key.
        let date_key = unsafe { NSMetadataItemFSContentChangeDateKey };
        let sort = NSSortDescriptor::sortDescriptorWithKey_ascending(Some(date_key), false);
        query.setSortDescriptors(&NSArray::<NSSortDescriptor>::from_slice(&[&sort]));

        let finished = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&finished);
        let block = RcBlock::new(move |_: NonNull<NSNotification>| flag.store(true, Ordering::Release));
        let center = NSNotificationCenter::defaultCenter();
        // SAFETY: nil queue uses the posting thread. This sendable callback owns only an atomic
        // specific to this query, even if notification delivery races observer removal.
        let observer = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(NSMetadataQueryDidFinishGatheringNotification),
                Some(&query),
                None,
                &block,
            )
        };
        Ok(Self { query, center, observer, finished, approved_paths, system_query })
    }

    fn gather(&self, cancel: &QueryCancellation, deadline: Instant) -> Result<Candidates, GatherError> {
        cancel.check()?;
        if Instant::now() >= deadline {
            return Err(GatherError::TimedOut);
        }
        if !self.query.startQuery() {
            cancel.check()?;
            return Err(GatherError::ServiceUnavailable);
        }
        let gathered = self.sample_until(cancel, deadline);
        cancel.check()?;
        sample_response(self.system_query, gathered)
    }

    fn sample_until(&self, cancel: &QueryCancellation, deadline: Instant) -> Result<Candidates, GatherError> {
        let run_loop = NSRunLoop::currentRunLoop();
        loop {
            cancel.check()?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(GatherError::TimedOut);
            }
            let finished = self.finished.load(Ordering::Acquire);
            if finished || remaining <= SNAPSHOT_RESERVE {
                self.query.disableUpdates();
                if Instant::now() >= deadline {
                    cancel.check()?;
                    return Err(GatherError::TimedOut);
                }
                return snapshot(
                    self.query.resultCount(),
                    finished,
                    cancel,
                    || Instant::now() >= deadline,
                    |index| {
                        let item = self.query.resultAtIndex(index).downcast::<NSMetadataItem>().ok()?;
                        // SAFETY: Foundation exports an immortal NSString attribute key.
                        let value = item.valueForAttribute(unsafe { NSMetadataItemPathKey })?;
                        let path = PathBuf::from(value.downcast::<NSString>().ok()?.to_string());
                        self.approved_paths.iter().any(|root| path.starts_with(root)).then_some(path)
                    },
                );
            }
            let step = (remaining - SNAPSHOT_RESERVE).min(RUN_LOOP_STEP);
            autoreleasepool(|_| {
                let until = NSDate::dateWithTimeIntervalSinceNow(step.as_secs_f64());
                // SAFETY: Foundation exports an immortal run-loop mode constant.
                if !run_loop.runMode_beforeDate(unsafe { NSDefaultRunLoopMode }, &until) {
                    std::thread::sleep(step);
                }
            });
        }
    }
}

impl Drop for NativeQuery {
    fn drop(&mut self) {
        if self.query.isStarted() && !self.query.isStopped() {
            self.query.stopQuery();
        }
        // SAFETY: this is the exact observer token registered with this center.
        unsafe { self.center.removeObserver((*self.observer).as_ref()) };
    }
}

// A successful system query samples at most CANDIDATE_LIMIT Spotlight rows.
// Global count, path-filter misses, truncation and the sampling deadline reveal
// nothing through the returned status. They can still affect which authorized
// hits fit in the sample and how long Spotlight takes; the sample is not exhaustive.
fn sample_response(system: bool, result: Result<Candidates, GatherError>) -> Result<Candidates, GatherError> {
    if !system { return result; }
    match result {
        Ok(sample) => Ok(Candidates { paths: sample.paths, partial: true, timed_out: false }),
        Err(GatherError::TimedOut) => Ok(Candidates { paths: Vec::new(), partial: true, timed_out: false }),
        Err(err) => Err(err),
    }
}

fn snapshot(
    count: usize,
    finished: bool,
    cancel: &QueryCancellation,
    expired: impl Fn() -> bool,
    mut path_at: impl FnMut(usize) -> Option<PathBuf>,
) -> Result<Candidates, GatherError> {
    let mut paths = Vec::with_capacity(count.min(CANDIDATE_LIMIT));
    let mut partial = count > CANDIDATE_LIMIT;
    for index in 0..count.min(CANDIDATE_LIMIT) {
        cancel.check()?;
        if expired() {
            break;
        }
        let path = path_at(index);
        // A native call already in flight cannot be interrupted. Exclude its value if
        // it finished after the deadline, including when it was the final candidate.
        if expired() {
            break;
        }
        match path {
            Some(path) => paths.push(path),
            None => partial = true,
        }
    }
    let timed_out = !finished || expired();
    partial |= timed_out;
    cancel.check()?;
    if timed_out && paths.is_empty() {
        return Err(GatherError::TimedOut);
    }
    Ok(Candidates { paths, partial, timed_out })
}

pub(crate) fn gather_on_worker(
    scope: ApprovedScope<'_>,
    text: &str,
    cancel: &QueryCancellation,
) -> Result<Candidates, GatherError> {
    cancel.check()?;
    if NSThread::isMainThread_class() {
        return Err(GatherError::ServiceUnavailable);
    }
    let deadline = Instant::now() + QUERY_TIMEOUT;
    autoreleasepool(|_| {
        let query = NativeQuery::new(scope, text)?;
        #[cfg(feature = "macos-find-system-qa")]
        if let ApprovedScope::IndexedLocal(scope) = scope {
            if scope.uses_system_query() {
                // Recheck immediately before starting the system-wide native request.
                scope.query_paths().map_err(|_| GatherError::ServiceUnavailable)?;
            }
        }
        query.gather(cancel, deadline)
    })
}

/// Inspect the exact builder used by gather_on_worker without starting an extra native query.
#[cfg(all(feature = "macos-find-system-qa", any(test, feature = "macos-find-qa-probe")))]
pub(crate) fn configured_for_indexed_local(scope: &IndexedLocalScope, text: &str) -> bool {
    let Ok(native) = NativeQuery::new(ApprovedScope::IndexedLocal(scope), text) else { return false; };
    let scopes = native.query.searchScopes();
    if scopes.count() != 1 { return false; }
    let Ok(setting) = scopes.objectAtIndex(0).downcast::<NSString>() else { return false; };
    // SAFETY: this Foundation scope identifier is an immortal NSString constant.
    &*setting == unsafe { objc2_foundation::NSMetadataQueryIndexedLocalComputerScope }
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_foundation::NSDictionary;

    #[test]
    fn foundation_matches_only_literal_filename_text() {
        autoreleasepool(|_| {
            // SAFETY: the dictionary below supplies the expected NSString key and value.
            let key = unsafe { NSMetadataItemFSNameKey };
            let name = NSString::from_str("会议 Café *'?.pdf");
            let file = NSDictionary::<NSString, NSString>::from_slices(&[key], &[&name]);
            for (text, matches) in [("cafe *'?", true), ("会议", true), ("会*", false), ("' OR 1=1", false)] {
                let predicate = filename_predicate(text).unwrap();
                assert_eq!(unsafe { predicate.evaluateWithObject(Some(&file)) }, matches);
            }
        });
    }

    #[test]
    fn invalid_text_or_scope_cannot_start_a_native_query() {
        for text in ["", "a\0b", &"a".repeat(256)] {
            assert_eq!(filename_predicate(text).err(), Some(GatherError::InvalidInput));
        }
        assert_eq!(NativeQuery::new(ApprovedScope::Directory(Path::new("relative")), "ab").err(), Some(GatherError::InvalidInput));
    }

    #[test]
    fn caps_candidate_copies_and_marks_missing_paths_partial() {
        let mut copies = 0;
        let result = snapshot(501, true, &QueryCancellation::default(), || false, |index| {
            copies += 1;
            (index != 10).then(|| PathBuf::from("/test"))
        }).unwrap();
        assert_eq!(copies, 400);
        assert_eq!(result.paths.len(), 399);
        assert!(result.partial);
    }

    #[test]
    fn distinguishes_empty_finish_timeout_and_partial_timeout() {
        let cancel = QueryCancellation::default();
        assert!(!snapshot(0, true, &cancel, || false, |_| None).unwrap().partial);
        assert_eq!(snapshot(1, false, &cancel, || false, |_| None).err(), Some(GatherError::TimedOut));
        let result = snapshot(1, false, &cancel, || false, |_| Some("/test".into())).unwrap();
        assert!(result.timed_out && result.partial);
    }

    #[test]
    fn system_samples_do_not_report_global_count_misses_or_truncation() {
        fn sample(count: usize, approved_index: Option<usize>, finished: bool, expired: bool) -> Candidates {
            let mut copies = 0;
            let raw = snapshot(count, finished, &QueryCancellation::default(), || expired, |i| {
                copies += 1;
                (Some(i) == approved_index).then(|| PathBuf::from("/approved/example.pdf"))
            });
            assert!(copies <= CANDIDATE_LIMIT);
            sample_response(true, raw).unwrap()
        }
        for (count, finished, expired) in [(0,true,false),(1,true,false),(600,true,false),
                                          (600,false,true),(0,false,false)] {
            let result = sample(count, None, finished, expired);
            assert!(result.paths.is_empty() && result.partial && !result.timed_out);
        }
        for count in [1, 2, CANDIDATE_LIMIT + 1] {
            let result = sample(count, Some(0), true, false);
            assert_eq!(result.paths, [PathBuf::from("/approved/example.pdf")]);
            assert!(result.partial && !result.timed_out);
        }
        assert_eq!(sample_response(true, Err(GatherError::Cancelled)).unwrap_err(), GatherError::Cancelled);
        assert_eq!(sample_response(true, Err(GatherError::ServiceUnavailable)).unwrap_err(), GatherError::ServiceUnavailable);
    }

    #[test]
    fn expired_snapshot_never_reads_a_candidate() {
        let result = snapshot(400, true, &QueryCancellation::default(), || true, |_| {
            panic!("read after deadline")
        });
        assert_eq!(result.err(), Some(GatherError::TimedOut));
    }

    #[test]
    fn only_copies_completed_before_deadline_survive() {
        use std::cell::Cell;

        // Simulate a stall on the first, middle and final native lookup.
        for stalls_at in 0..3 {
            let expired = Cell::new(false);
            let mut lookups = 0;
            let result = snapshot(3, true, &QueryCancellation::default(), || expired.get(), |index| {
                lookups += 1;
                expired.set(index == stalls_at);
                Some(format!("/row{index}").into())
            });
            assert_eq!(lookups, stalls_at + 1);
            if stalls_at == 0 {
                assert_eq!(result.err(), Some(GatherError::TimedOut));
            } else {
                let candidates = result.unwrap();
                let expected: Vec<PathBuf> = (0..stalls_at)
                    .map(|index| format!("/row{index}").into()).collect();
                assert_eq!(candidates.paths, expected);
                assert!(candidates.partial && candidates.timed_out);
            }
        }
    }

    #[test]
    fn cancellation_during_last_copy_discards_snapshot_without_cancelling_new_owner() {
        let old = QueryCancellation::default();
        let new = QueryCancellation::default();
        let result = snapshot(1, true, &old, || old.check().is_err(), |_| {
            old.cancel();
            Some("/old".into())
        });
        assert_eq!(result.err(), Some(GatherError::Cancelled));
        assert_eq!(new.check(), Ok(()));
    }

    #[test]
    #[ignore = "requires a running Spotlight service with the public macOS font sample indexed"]
    fn public_font_query_gathers_on_a_background_run_loop() {
        let root = Path::new("/System/Library/Fonts/Supplemental");
        let name = "Andale Mono.ttf";
        let result = gather_on_worker(
            ApprovedScope::Directory(root),
            name,
            &QueryCancellation::default(),
        ).unwrap();
        assert!(!result.timed_out);
        assert!(result.paths.contains(&root.join(name)), "candidate_count={} partial={}", result.paths.len(), result.partial);
    }

    #[test]
    fn late_native_finish_updates_only_its_own_query() {
        autoreleasepool(|_| {
            let scope = || ApprovedScope::Directory(Path::new("/System/Library/Fonts"));
            let first = NativeQuery::new(scope(), "a").unwrap();
            let second = NativeQuery::new(scope(), "b").unwrap();
            // SAFETY: this notification is scoped to the first query, using its actual object.
            unsafe {
                first.center.postNotificationName_object(
                    NSMetadataQueryDidFinishGatheringNotification, Some(&first.query)
                );
            }
            assert!(first.finished.load(Ordering::Acquire));
            assert!(!second.finished.load(Ordering::Acquire));
            let old = first.query.clone();
            let late = first.finished.clone();
            drop(first);
            late.store(false, Ordering::Release);
            unsafe {
                second.center.postNotificationName_object(
                    NSMetadataQueryDidFinishGatheringNotification, Some(&old)
                );
            }
            assert!(!late.load(Ordering::Acquire));
            assert!(!second.finished.load(Ordering::Acquire));
        });
    }
}
