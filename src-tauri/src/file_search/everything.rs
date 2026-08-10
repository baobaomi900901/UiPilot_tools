use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use everything_ipc::client::{EverythingClient, EverythingClientError};
use everything_ipc::protocol::{
    EverythingQueryResult, EverythingQuerySpec, EverythingResultItem, EverythingSort,
};
use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::Time::FileTimeToSystemTime;

use super::windows::path_auth::{authenticate_path, AuthenticatedPathSnapshot};
use super::{
    EverythingPathAction, FileCategory, FileExecutionError, FilePathKind, FileResultKind,
    PublishedFileBatch, PublishedFileDraft,
};

pub(crate) struct EverythingSearchState {
    client: Mutex<Option<Arc<EverythingClient>>>,
    revision: AtomicU64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EverythingSearchError {
    Unavailable,
    RevisionExhausted,
}

impl EverythingSearchState {
    pub(crate) fn new() -> Self {
        Self {
            client: Mutex::new(None),
            revision: AtomicU64::new(0),
        }
    }

    pub(crate) fn search(
        &self,
        query: &str,
        category: FileCategory,
    ) -> Result<PublishedFileBatch, EverythingSearchError> {
        run_search_with_category(
            query,
            category,
            &self.revision,
            |spec| {
                query_cached_with(
                    &self.client,
                    spec,
                    || {
                        connect_ready_with(
                            || EverythingClient::connect("", Duration::from_millis(250)),
                            EverythingClient::query,
                        )
                    },
                    EverythingClient::query,
                )
                .map_err(|_| EverythingClientError::IpcUnavailable)
            },
            authenticate_everything_item,
        )
    }
}

fn everything_index_probe_spec() -> Result<EverythingQuerySpec, EverythingClientError> {
    Ok(EverythingQuerySpec {
        search: Vec::new(),
        offset: 0,
        max_results: 1,
        request_flags: 0x155,
        sort: EverythingSort::DateModifiedDescending,
        deadline: Instant::now()
            .checked_add(Duration::from_secs(1))
            .ok_or(EverythingClientError::IpcUnavailable)?,
    })
}

fn connect_ready_with<C, Connect, Probe>(
    connect: Connect,
    probe: Probe,
) -> Result<C, EverythingClientError>
where
    Connect: FnOnce() -> Result<C, EverythingClientError>,
    Probe: FnOnce(&C, EverythingQuerySpec) -> Result<EverythingQueryResult, EverythingClientError>,
{
    let client = connect()?;
    let result = probe(&client, everything_index_probe_spec()?)?;
    if result.total == 0 {
        Err(EverythingClientError::IpcUnavailable)
    } else {
        Ok(client)
    }
}

pub(crate) fn encode_literal_query(query: &str) -> Vec<u16> {
    let mut encoded = String::with_capacity(query.len().saturating_mul(6));
    for scalar in query.chars() {
        use std::fmt::Write as _;
        write!(&mut encoded, "#x{:X}:", u32::from(scalar)).expect("String writes are infallible");
    }
    encoded.encode_utf16().collect()
}

fn literal_search_query(query: &str) -> Vec<u16> {
    let mut search = "nowildcards:".encode_utf16().collect::<Vec<_>>();
    search.extend(encode_literal_query(query));
    search
}

#[cfg(test)]
fn run_search_with<Q, A>(
    query: &str,
    revision: &AtomicU64,
    query_client: Q,
    authenticate: A,
) -> Result<PublishedFileBatch, EverythingSearchError>
where
    Q: FnOnce(EverythingQuerySpec) -> Result<EverythingQueryResult, EverythingClientError>,
    A: FnMut(&EverythingResultItem) -> Result<AuthenticatedPathSnapshot, FileExecutionError>,
{
    run_search_with_category(
        query,
        FileCategory::All,
        revision,
        query_client,
        authenticate,
    )
}

fn run_search_with_category<Q, A>(
    query: &str,
    category: FileCategory,
    revision: &AtomicU64,
    query_client: Q,
    mut authenticate: A,
) -> Result<PublishedFileBatch, EverythingSearchError>
where
    Q: FnOnce(EverythingQuerySpec) -> Result<EverythingQueryResult, EverythingClientError>,
    A: FnMut(&EverythingResultItem) -> Result<AuthenticatedPathSnapshot, FileExecutionError>,
{
    if query.is_empty() {
        return Err(EverythingSearchError::Unavailable);
    }

    let result = query_client(EverythingQuerySpec {
        search: category_search_query(query, category),
        offset: 0,
        max_results: 200,
        request_flags: 0x155,
        sort: EverythingSort::DateModifiedDescending,
        deadline: Instant::now()
            .checked_add(Duration::from_secs(1))
            .ok_or(EverythingSearchError::Unavailable)?,
    })
    .map_err(|_| EverythingSearchError::Unavailable)?;

    let mut items = Vec::with_capacity(result.items.len().min(200));
    for item in result.items.into_iter().take(200) {
        let snapshot = match authenticate(&item) {
            Ok(snapshot) => snapshot,
            Err(_) => continue,
        };
        items.push(published_draft(item, snapshot)?);
    }

    let index_revision = revision
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
            value.checked_add(1)
        })
        .map_err(|_| EverythingSearchError::RevisionExhausted)?
        .checked_add(1)
        .ok_or(EverythingSearchError::RevisionExhausted)?;

    Ok(PublishedFileBatch {
        index_revision,
        items,
    })
}
fn published_draft(
    item: EverythingResultItem,
    snapshot: AuthenticatedPathSnapshot,
) -> Result<PublishedFileDraft, EverythingSearchError> {
    let kind = match snapshot.identity.kind {
        FilePathKind::File => FileResultKind::File,
        FilePathKind::Directory => FileResultKind::Folder,
    };
    let full_path = snapshot.identity.display_path.clone();
    let size_bytes = match kind {
        FileResultKind::File => item.size_bytes.or(snapshot.size_bytes),
        FileResultKind::Folder => None,
    };
    let modified_filetime = item.modified_filetime.unwrap_or(snapshot.modified_filetime);
    let modified_utc = filetime_to_rfc3339(modified_filetime)?;

    Ok(PublishedFileDraft {
        action: EverythingPathAction::new(snapshot.identity).into(),
        name: item.file_name,
        kind,
        size_bytes,
        modified_utc,
        full_path,
    })
}

fn authenticate_everything_item(
    item: &EverythingResultItem,
) -> Result<AuthenticatedPathSnapshot, FileExecutionError> {
    authenticate_path(
        &item.full_path,
        file_path_kind_for_attributes(item.attributes),
    )
}

fn file_path_kind_for_attributes(attributes: u32) -> FilePathKind {
    if attributes & 0x10 == 0 {
        FilePathKind::File
    } else {
        FilePathKind::Directory
    }
}

fn filetime_to_rfc3339(filetime: u64) -> Result<String, EverythingSearchError> {
    let filetime = FILETIME {
        dwLowDateTime: filetime as u32,
        dwHighDateTime: (filetime >> 32) as u32,
    };
    let mut system_time = SYSTEMTIME::default();
    unsafe { FileTimeToSystemTime(&filetime, &mut system_time) }
        .map_err(|_| EverythingSearchError::Unavailable)?;
    if system_time.wYear > 9_999 {
        return Err(EverythingSearchError::Unavailable);
    }

    Ok(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        system_time.wYear,
        system_time.wMonth,
        system_time.wDay,
        system_time.wHour,
        system_time.wMinute,
        system_time.wSecond,
        system_time.wMilliseconds
    ))
}

fn query_cached_with<C, Connect, Query>(
    slot: &Mutex<Option<Arc<C>>>,
    spec: EverythingQuerySpec,
    connect: Connect,
    query: Query,
) -> Result<EverythingQueryResult, EverythingSearchError>
where
    Connect: FnOnce() -> Result<C, EverythingClientError>,
    Query: FnOnce(&C, EverythingQuerySpec) -> Result<EverythingQueryResult, EverythingClientError>,
{
    let client = {
        let mut cached = slot
            .lock()
            .map_err(|_| EverythingSearchError::Unavailable)?;
        if let Some(client) = cached.as_ref() {
            Arc::clone(client)
        } else {
            let client = Arc::new(connect().map_err(|_| EverythingSearchError::Unavailable)?);
            *cached = Some(Arc::clone(&client));
            client
        }
    };

    match query(client.as_ref(), spec) {
        Ok(result) => Ok(result),
        Err(error) => {
            if evicts_cached_client(&error) {
                let mut cached = slot
                    .lock()
                    .map_err(|_| EverythingSearchError::Unavailable)?;
                if cached
                    .as_ref()
                    .is_some_and(|cached| Arc::ptr_eq(cached, &client))
                {
                    *cached = None;
                }
            }
            Err(EverythingSearchError::Unavailable)
        }
    }
}

fn evicts_cached_client(error: &EverythingClientError) -> bool {
    matches!(
        error,
        EverythingClientError::InvalidInstance
            | EverythingClientError::IpcUnavailable
            | EverythingClientError::IpcSendFailed
            | EverythingClientError::ClientClosed
            | EverythingClientError::Protocol(_)
            | EverythingClientError::RequestIdExhausted
    )
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use everything_ipc::client::EverythingClientError;
    use everything_ipc::protocol::{
        EverythingQueryResult, EverythingQuerySpec, EverythingResultItem, EverythingSort,
        ProtocolError,
    };
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows::Win32::System::Time::SystemTimeToFileTime;

    use super::super::windows::path_auth::{AuthenticatedPathIdentity, AuthenticatedPathSnapshot};
    use super::super::{FileExecutionAction, FileExecutionError, FilePathKind, FileResultKind};
    use super::{
        connect_ready_with, encode_literal_query, file_path_kind_for_attributes,
        filetime_to_rfc3339, query_cached_with, run_search_with, EverythingSearchError,
        EverythingSearchState,
    };

    const UNIX_EPOCH_FILETIME: u64 = 116_444_736_000_000_000;

    fn literal_text(query: &str) -> String {
        String::from_utf16(&encode_literal_query(query)).unwrap()
    }

    fn filetime_for_test(system_time: SYSTEMTIME) -> u64 {
        let mut filetime = FILETIME::default();
        unsafe { SystemTimeToFileTime(&system_time, &mut filetime) }.unwrap();
        (u64::from(filetime.dwHighDateTime) << 32) | u64::from(filetime.dwLowDateTime)
    }

    fn query_item_for_test(index: usize) -> EverythingResultItem {
        EverythingResultItem {
            full_path: format!(r"C:\items\item-{index}.txt"),
            file_name: format!("item-{index}.txt"),
            attributes: 0,
            size_bytes: Some(index as u64),
            modified_filetime: Some(UNIX_EPOCH_FILETIME + index as u64 * 10_000),
        }
    }

    fn query_result_for_test(count: usize) -> EverythingQueryResult {
        EverythingQueryResult {
            total: u32::MAX,
            request_flags: 0x155,
            sort_type: 14,
            items: (0..count).map(query_item_for_test).collect(),
        }
    }

    fn authenticated_snapshot_for_test(
        display_path: impl Into<String>,
        kind: FilePathKind,
        size_bytes: Option<u64>,
        modified_filetime: u64,
    ) -> AuthenticatedPathSnapshot {
        AuthenticatedPathSnapshot {
            identity: AuthenticatedPathIdentity {
                display_path: display_path.into(),
                volume_guid_path: r"\\?\Volume{EVERYTHING-TEST}\".into(),
                relative_path: r"items\authenticated.txt".into(),
                volume_serial: 41,
                file_id: [9; 16],
                kind,
            },
            size_bytes,
            modified_filetime,
        }
    }

    fn authenticate_item_for_test(
        item: &EverythingResultItem,
    ) -> Result<AuthenticatedPathSnapshot, FileExecutionError> {
        Ok(authenticated_snapshot_for_test(
            &item.full_path,
            file_path_kind_for_attributes(item.attributes),
            Some(10_000),
            UNIX_EPOCH_FILETIME,
        ))
    }

    fn query_spec_for_test() -> EverythingQuerySpec {
        EverythingQuerySpec {
            search: Vec::new(),
            offset: 0,
            max_results: 200,
            request_flags: 0x155,
            sort: EverythingSort::DateModifiedDescending,
            deadline: Instant::now() + Duration::from_secs(1),
        }
    }

    fn run_successful_search_for_test(
        revision: &AtomicU64,
    ) -> Result<super::super::PublishedFileBatch, EverythingSearchError> {
        run_search_with(
            "x",
            revision,
            |_| Ok(query_result_for_test(1)),
            authenticate_item_for_test,
        )
    }

    #[test]
    fn new_state_is_lazy_and_starts_at_revision_zero() {
        let state = EverythingSearchState::new();

        assert!(state.client.lock().unwrap().is_none());
        assert_eq!(state.revision.load(Ordering::Acquire), 0);
    }

    #[test]
    fn literal_query_encodes_every_unicode_scalar_and_no_operator_survives() {
        assert_eq!(
            literal_text("a b|!*?<>\"文件😀"),
            "#x61:#x20:#x62:#x7C:#x21:#x2A:#x3F:#x3C:#x3E:#x22:#x6587:#x4EF6:#x1F600:"
        );
        assert_eq!(literal_text("e\u{301}"), "#x65:#x301:");
    }

    #[test]
    fn query_contract_is_fixed_and_authentication_preserves_order() {
        let captured = RefCell::new(None);
        let result = run_search_with(
            "report",
            &AtomicU64::new(0),
            |spec| {
                *captured.borrow_mut() = Some(spec.clone());
                Ok(query_result_for_test(205))
            },
            authenticate_item_for_test,
        )
        .unwrap();
        let spec = captured.borrow();
        let spec = spec.as_ref().unwrap();
        assert_eq!(
            String::from_utf16(&spec.search).unwrap(),
            format!("nowildcards:{}", literal_text("report"))
        );
        assert_eq!(spec.offset, 0);
        assert_eq!(spec.max_results, 200);
        assert_eq!(spec.request_flags, 0x155);
        assert_eq!(spec.sort, EverythingSort::DateModifiedDescending);
        assert_eq!(result.items.len(), 200);
        assert_eq!(result.items[0].name, "item-0.txt");
        assert_eq!(result.items[199].name, "item-199.txt");
        assert_eq!(result.index_revision, 1);
    }

    #[test]
    fn empty_query_is_rejected_before_transport_or_revision_allocation() {
        let queried = Cell::new(false);
        let revision = AtomicU64::new(3);

        let result = run_search_with(
            "",
            &revision,
            |_| {
                queried.set(true);
                Ok(query_result_for_test(1))
            },
            authenticate_item_for_test,
        );

        assert_eq!(result, Err(EverythingSearchError::Unavailable));
        assert!(!queried.get());
        assert_eq!(revision.load(Ordering::Acquire), 3);
    }

    #[test]
    fn authentication_errors_omit_items_without_reordering_survivors() {
        let authenticated = RefCell::new(Vec::new());

        let result = run_search_with(
            "x",
            &AtomicU64::new(0),
            |_| Ok(query_result_for_test(4)),
            |item| {
                authenticated.borrow_mut().push(item.file_name.clone());
                if item.file_name == "item-1.txt" {
                    Err(FileExecutionError::Stale)
                } else {
                    authenticate_item_for_test(item)
                }
            },
        )
        .unwrap();

        assert_eq!(
            authenticated.into_inner(),
            ["item-0.txt", "item-1.txt", "item-2.txt", "item-3.txt"]
        );
        assert_eq!(
            result
                .items
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            ["item-0.txt", "item-2.txt", "item-3.txt"]
        );
    }

    #[test]
    fn directory_attribute_drives_authenticated_and_published_kind() {
        let mut item = query_item_for_test(0);
        item.attributes = 0x10;

        assert_eq!(
            file_path_kind_for_attributes(item.attributes),
            FilePathKind::Directory
        );
        let result = run_search_with(
            "x",
            &AtomicU64::new(0),
            |_| {
                Ok(EverythingQueryResult {
                    items: vec![item],
                    ..query_result_for_test(0)
                })
            },
            authenticate_item_for_test,
        )
        .unwrap();
        assert_eq!(result.items[0].kind, FileResultKind::Folder);
        assert_eq!(result.items[0].size_bytes, None);
        match &result.items[0].action {
            FileExecutionAction::Everything(action) => {
                assert_eq!(action.identity().kind, FilePathKind::Directory);
            }
            FileExecutionAction::Indexed(_) => panic!("unexpected indexed action"),
        }
    }

    #[test]
    fn missing_query2_metadata_falls_back_to_authenticated_snapshot() {
        let mut missing_size = query_item_for_test(0);
        missing_size.full_path = r"C:\reported\size.txt".into();
        missing_size.size_bytes = None;
        missing_size.modified_filetime = Some(UNIX_EPOCH_FILETIME + 1_230_000);
        let mut missing_modified = query_item_for_test(1);
        missing_modified.full_path = r"C:\reported\modified.txt".into();
        missing_modified.size_bytes = Some(123);
        missing_modified.modified_filetime = None;

        let result = run_search_with(
            "x",
            &AtomicU64::new(0),
            |_| {
                Ok(EverythingQueryResult {
                    items: vec![missing_size, missing_modified],
                    ..query_result_for_test(0)
                })
            },
            |item| {
                let (display_path, size_bytes, modified_filetime) =
                    if item.file_name == "item-0.txt" {
                        (r"C:\authenticated\size.txt", Some(456), UNIX_EPOCH_FILETIME)
                    } else {
                        (
                            r"C:\authenticated\modified.txt",
                            Some(789),
                            UNIX_EPOCH_FILETIME + 7_890_000,
                        )
                    };
                Ok(authenticated_snapshot_for_test(
                    display_path,
                    FilePathKind::File,
                    size_bytes,
                    modified_filetime,
                ))
            },
        )
        .unwrap();

        assert_eq!(result.items[0].size_bytes, Some(456));
        assert_eq!(result.items[0].modified_utc, "1970-01-01T00:00:00.123Z");
        assert_eq!(result.items[0].full_path, r"C:\authenticated\size.txt");
        assert_eq!(result.items[1].size_bytes, Some(123));
        assert_eq!(result.items[1].modified_utc, "1970-01-01T00:00:00.789Z");
        assert_eq!(result.items[1].full_path, r"C:\authenticated\modified.txt");
    }

    #[test]
    fn rfc3339_accepts_latest_four_digit_year() {
        let filetime = filetime_for_test(SYSTEMTIME {
            wYear: 9_999,
            wMonth: 12,
            wDay: 31,
            wHour: 23,
            wMinute: 59,
            wSecond: 59,
            wMilliseconds: 999,
            ..SYSTEMTIME::default()
        });

        assert_eq!(
            filetime_to_rfc3339(filetime),
            Ok("9999-12-31T23:59:59.999Z".into())
        );
    }

    #[test]
    fn rfc3339_rejects_year_beyond_four_digits() {
        let filetime = filetime_for_test(SYSTEMTIME {
            wYear: 10_000,
            wMonth: 1,
            wDay: 1,
            ..SYSTEMTIME::default()
        });

        assert_eq!(
            filetime_to_rfc3339(filetime),
            Err(EverythingSearchError::Unavailable)
        );
    }

    #[test]
    fn out_of_range_modified_year_fails_batch_without_allocating_revision() {
        let revision = AtomicU64::new(23);
        let filetime = filetime_for_test(SYSTEMTIME {
            wYear: 10_000,
            wMonth: 1,
            wDay: 1,
            ..SYSTEMTIME::default()
        });

        let result = run_search_with(
            "x",
            &revision,
            |_| {
                let mut query_result = query_result_for_test(1);
                query_result.items[0].modified_filetime = Some(filetime);
                Ok(query_result)
            },
            authenticate_item_for_test,
        );

        assert_eq!(result, Err(EverythingSearchError::Unavailable));
        assert_eq!(revision.load(Ordering::Acquire), 23);
    }

    #[test]
    fn failed_query_or_authentication_batch_does_not_allocate_revision() {
        let revision = AtomicU64::new(9);
        assert_eq!(
            run_search_with(
                "x",
                &revision,
                |_| Err(EverythingClientError::QueryTimedOut),
                |_| unreachable!()
            ),
            Err(EverythingSearchError::Unavailable)
        );
        assert_eq!(revision.load(Ordering::Acquire), 9);
    }

    #[test]
    fn revision_allocation_is_checked_and_monotonic() {
        let revision = AtomicU64::new(0);

        assert_eq!(
            run_successful_search_for_test(&revision)
                .unwrap()
                .index_revision,
            1
        );
        assert_eq!(
            run_successful_search_for_test(&revision)
                .unwrap()
                .index_revision,
            2
        );
        assert_eq!(revision.load(Ordering::Acquire), 2);
    }

    #[test]
    fn revision_exhaustion_stays_failed_closed() {
        let revision = AtomicU64::new(u64::MAX);
        for _ in 0..2 {
            assert_eq!(
                run_successful_search_for_test(&revision),
                Err(EverythingSearchError::RevisionExhausted)
            );
            assert_eq!(revision.load(Ordering::Acquire), u64::MAX);
        }
    }

    #[derive(Debug)]
    struct TestClient {
        id: usize,
    }

    #[test]
    fn connection_requires_a_nonempty_loaded_index() {
        let captured = RefCell::new(None);
        let empty = connect_ready_with(
            || Ok(TestClient { id: 1 }),
            |_, spec| {
                *captured.borrow_mut() = Some(spec);
                Ok(EverythingQueryResult {
                    total: 0,
                    request_flags: 0x155,
                    sort_type: 14,
                    items: Vec::new(),
                })
            },
        );
        assert!(matches!(empty, Err(EverythingClientError::IpcUnavailable)));
        let spec = captured.borrow();
        let spec = spec.as_ref().unwrap();
        assert!(spec.search.is_empty());
        assert_eq!(spec.max_results, 1);

        assert_eq!(
            connect_ready_with(
                || Ok(TestClient { id: 2 }),
                |_, _| Ok(EverythingQueryResult {
                    total: 1,
                    request_flags: 0x155,
                    sort_type: 14,
                    items: Vec::new(),
                }),
            )
            .unwrap()
            .id,
            2
        );
    }

    #[test]
    fn failed_readiness_probe_is_not_cached() {
        let slot = Mutex::new(None);
        let result = query_cached_with(
            &slot,
            query_spec_for_test(),
            || {
                connect_ready_with(
                    || Ok(TestClient { id: 1 }),
                    |_, _| {
                        Ok(EverythingQueryResult {
                            total: 0,
                            request_flags: 0x155,
                            sort_type: 14,
                            items: Vec::new(),
                        })
                    },
                )
            },
            |_, _| unreachable!(),
        );

        assert_eq!(result, Err(EverythingSearchError::Unavailable));
        assert!(slot.lock().unwrap().is_none());
    }

    #[test]
    fn cached_client_connects_lazily_reuses_and_is_not_locked_during_query() {
        let slot = Mutex::new(None);
        let connects = Cell::new(0);

        for _ in 0..2 {
            let result = query_cached_with(
                &slot,
                query_spec_for_test(),
                || {
                    connects.set(connects.get() + 1);
                    Ok(TestClient { id: connects.get() })
                },
                |client, _| {
                    assert!(slot.try_lock().is_ok());
                    assert_eq!(client.id, 1);
                    Ok(query_result_for_test(0))
                },
            );
            assert!(result.is_ok());
        }

        assert_eq!(connects.get(), 1);
    }

    #[test]
    fn query_error_cache_matrix_evicts_only_reconnectable_failures() {
        let cases = [
            (
                "invalid instance",
                EverythingClientError::InvalidInstance,
                true,
            ),
            ("unavailable", EverythingClientError::IpcUnavailable, true),
            ("send failure", EverythingClientError::IpcSendFailed, true),
            ("client closed", EverythingClientError::ClientClosed, true),
            (
                "protocol mismatch",
                EverythingClientError::Protocol(ProtocolError::ReplyContractMismatch),
                true,
            ),
            (
                "request id exhaustion",
                EverythingClientError::RequestIdExhausted,
                true,
            ),
            ("timeout", EverythingClientError::QueryTimedOut, false),
            ("overload", EverythingClientError::Overloaded, false),
        ];

        for (label, error, evicts) in cases {
            let slot = Mutex::new(Some(Arc::new(TestClient { id: 1 })));
            let connects = Cell::new(0);
            let first = query_cached_with(
                &slot,
                query_spec_for_test(),
                || {
                    connects.set(connects.get() + 1);
                    Ok(TestClient { id: 2 })
                },
                |_, _| Err(error),
            );
            assert_eq!(first, Err(EverythingSearchError::Unavailable), "{label}");
            assert_eq!(connects.get(), 0, "{label}");

            let queried_client = Cell::new(0);
            let second = query_cached_with(
                &slot,
                query_spec_for_test(),
                || {
                    connects.set(connects.get() + 1);
                    Ok(TestClient { id: 2 })
                },
                |client, _| {
                    queried_client.set(client.id);
                    Ok(query_result_for_test(0))
                },
            );
            assert!(second.is_ok(), "{label}");
            assert_eq!(connects.get(), usize::from(evicts), "{label}");
            assert_eq!(queried_client.get(), if evicts { 2 } else { 1 }, "{label}");
        }
    }

    #[test]
    fn connection_failure_is_not_cached_and_next_call_reconnects() {
        let slot = Mutex::new(None);
        let connects = Cell::new(0);

        assert_eq!(
            query_cached_with(
                &slot,
                query_spec_for_test(),
                || {
                    connects.set(connects.get() + 1);
                    Err(EverythingClientError::ConnectionTimedOut)
                },
                |_, _| unreachable!()
            ),
            Err(EverythingSearchError::Unavailable)
        );
        assert!(slot.lock().unwrap().is_none());

        let result = query_cached_with(
            &slot,
            query_spec_for_test(),
            || {
                connects.set(connects.get() + 1);
                Ok(TestClient { id: 2 })
            },
            |client, _| {
                assert_eq!(client.id, 2);
                Ok(query_result_for_test(0))
            },
        );
        assert!(result.is_ok());
        assert_eq!(connects.get(), 2);
    }

    #[test]
    fn failed_client_does_not_evict_a_concurrently_installed_replacement() {
        let original = Arc::new(TestClient { id: 1 });
        let replacement = Arc::new(TestClient { id: 2 });
        let slot = Mutex::new(Some(Arc::clone(&original)));

        let result = query_cached_with(
            &slot,
            query_spec_for_test(),
            || unreachable!(),
            |client, _| {
                assert_eq!(client.id, 1);
                *slot.lock().unwrap() = Some(Arc::clone(&replacement));
                Err(EverythingClientError::IpcSendFailed)
            },
        );

        assert_eq!(result, Err(EverythingSearchError::Unavailable));
        let cached = slot.lock().unwrap();
        assert!(Arc::ptr_eq(cached.as_ref().unwrap(), &replacement));
    }
}
#[cfg(test)]
mod category_tests {
    use super::{category_predicate, category_search_query, FileCategory};
    #[test]
    fn category_predicates_use_the_fixed_everything_filters() {
        for (category, expected) in [
            (FileCategory::All, ""),
            (FileCategory::Folder, "folder:"),
            (FileCategory::Excel, "file: ext:xls;xlsx;xlsm;xlsb;csv"),
            (FileCategory::Word, "file: ext:doc;docx;docm;rtf"),
            (FileCategory::Ppt, "file: ext:ppt;pptx;pptm"),
            (FileCategory::Pdf, "file: ext:pdf"),
            (
                FileCategory::Image,
                "file: ext:bmp;gif;heic;jpeg;jpg;png;svg;tif;tiff;webp",
            ),
            (
                FileCategory::Video,
                "file: ext:avi;m4v;mkv;mov;mp4;webm;wmv",
            ),
            (
                FileCategory::Audio,
                "file: ext:aac;flac;m4a;mp3;ogg;wav;wma",
            ),
            (FileCategory::Archive, "file: ext:7z;bz2;gz;rar;tar;tgz;zip"),
        ] {
            assert_eq!(category_predicate(category), expected);
        }
    }

    #[test]
    fn category_query_keeps_special_user_text_literal() {
        let query =
            String::from_utf16(&category_search_query("a b|!*?<>\"", FileCategory::Pdf)).unwrap();

        assert_eq!(
            query,
            "nowildcards:#x61:#x20:#x62:#x7C:#x21:#x2A:#x3F:#x3C:#x3E:#x22: file: ext:pdf"
        );
        assert!(!query.contains("a b|!*?<>\""));
    }
}

pub(crate) fn category_predicate(category: FileCategory) -> &'static str {
    match category {
        FileCategory::All => "",
        FileCategory::Folder => "folder:",
        FileCategory::Excel => "file: ext:xls;xlsx;xlsm;xlsb;csv",
        FileCategory::Word => "file: ext:doc;docx;docm;rtf",
        FileCategory::Ppt => "file: ext:ppt;pptx;pptm",
        FileCategory::Pdf => "file: ext:pdf",
        FileCategory::Image => "file: ext:bmp;gif;heic;jpeg;jpg;png;svg;tif;tiff;webp",
        FileCategory::Video => "file: ext:avi;m4v;mkv;mov;mp4;webm;wmv",
        FileCategory::Audio => "file: ext:aac;flac;m4a;mp3;ogg;wav;wma",
        FileCategory::Archive => "file: ext:7z;bz2;gz;rar;tar;tgz;zip",
    }
}

pub(crate) fn category_search_query(query: &str, category: FileCategory) -> Vec<u16> {
    let mut search = literal_search_query(query);
    let predicate = category_predicate(category);
    if !predicate.is_empty() {
        search.push(b' ' as u16);
        search.extend(predicate.encode_utf16());
    }

    search
}
