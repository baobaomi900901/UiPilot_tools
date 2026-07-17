use std::ffi::c_void;
use std::mem::{MaybeUninit, size_of};
use std::slice;

use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CLSCTX_LOCAL_SERVER, COINIT_MULTITHREADED, CoCreateInstance,
    CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows::Win32::System::Search::Common::COP_VALUE_CONTAINS;
use windows::Win32::System::Search::{
    CSearchManager, ConditionFactory, ICondition, IConditionFactory, IRichChunk,
    ISearchCatalogManager, ISearchManager, ISearchScopeRule,
};
use windows::Win32::System::Services::{
    CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatusEx, SC_HANDLE,
    SC_MANAGER_CONNECT, SC_STATUS_PROCESS_INFO, SERVICE_QUERY_STATUS, SERVICE_RUNNING,
    SERVICE_STATUS_PROCESS,
};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    BHID_EnumItems, IEnumShellItems, ISearchFolderItemFactory, IShellItem, IShellItemArray,
    SHCreateShellItemArrayFromIDLists, SHGetIDListFromObject, SIGDN_DESKTOPABSOLUTEPARSING,
    SIGDN_NORMALDISPLAY, SearchFolderItemFactory,
};
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

use crate::{
    CrawlRule, IndexedScope, OperationCounters, QueryOperations, ScopeEvidence, SearchBackend,
    SearchHit, SearchStatus, SpikeError, run_query_operations, validated_file_scopes,
};

pub struct WindowsSearch {
    _apartment: ComApartment,
    manager: ISearchManager,
}

impl SearchBackend for WindowsSearch {
    fn status(&self) -> Result<SearchStatus, SpikeError> {
        WindowsSearch::status(self)
    }

    fn indexed_scopes(&self) -> Result<Vec<IndexedScope>, SpikeError> {
        WindowsSearch::indexed_scopes(self)
    }

    fn query_literal(
        &self,
        literal: &str,
        limit: u32,
        scopes: &[IndexedScope],
    ) -> Result<Vec<SearchHit>, SpikeError> {
        let mut operations = WindowsQueryOperations::default();
        run_query_operations(&mut operations, literal, limit, scopes)
    }
}

#[derive(Default)]
struct WindowsQueryOperations {
    condition: Option<ICondition>,
    factory: Option<ISearchFolderItemFactory>,
    search_item: Option<IShellItem>,
}

impl WindowsQueryOperations {
    fn factory(&self) -> Result<&ISearchFolderItemFactory, SpikeError> {
        self.factory
            .as_ref()
            .ok_or_else(|| SpikeError::verification_failed("Search Folder factory was not created"))
    }
}

impl QueryOperations for WindowsQueryOperations {
    fn create_condition_leaf(&mut self, literal: &str) -> Result<(), SpikeError> {
        let factory: IConditionFactory = unsafe {
            CoCreateInstance(&ConditionFactory, None, CLSCTX_INPROC_SERVER)
                .map_err(|error| windows_error("create IConditionFactory", error))?
        };
        let value = PROPVARIANT::from(literal);
        self.condition = Some(unsafe {
            factory
                .MakeLeaf(
                    w!("System.FileName"),
                    COP_VALUE_CONTAINS,
                    PCWSTR::null(),
                    &value,
                    None::<&IRichChunk>,
                    None::<&IRichChunk>,
                    None::<&IRichChunk>,
                    false,
                )
                .map_err(|error| windows_error("create System.FileName literal condition", error))?
        });
        Ok(())
    }

    fn create_search_folder_factory(&mut self) -> Result<(), SpikeError> {
        self.factory = Some(unsafe {
            CoCreateInstance(&SearchFolderItemFactory, None, CLSCTX_INPROC_SERVER)
                .map_err(|error| windows_error("create Search Folder factory", error))?
        });
        Ok(())
    }

    fn set_condition(&mut self) -> Result<(), SpikeError> {
        let condition = self
            .condition
            .as_ref()
            .ok_or_else(|| SpikeError::verification_failed("query condition was not created"))?;
        unsafe {
            self.factory()?
                .SetCondition(condition)
                .map_err(|error| windows_error("set Search Folder condition", error))
        }
    }

    fn set_display_name(&mut self) -> Result<(), SpikeError> {
        unsafe {
            self.factory()?
                .SetDisplayName(w!("UiPilot SystemIndex Spike"))
                .map_err(|error| windows_error("set Search Folder display name", error))
        }
    }

    fn set_explicit_scopes(&mut self, scopes: &[IndexedScope]) -> Result<(), SpikeError> {
        let shell_items = scopes
            .iter()
            .map(|scope| shell_item_from_scope(&scope.url))
            .collect::<Result<Vec<_>, _>>()?;
        let pidls = PidlList::from_shell_items(&shell_items)?;
        let pointers = pidls
            .0
            .iter()
            .map(|pidl| *pidl as *const ITEMIDLIST)
            .collect::<Vec<_>>();
        let scope_array: IShellItemArray = unsafe {
            SHCreateShellItemArrayFromIDLists(&pointers)
                .map_err(|error| windows_error("create indexed scope array", error))?
        };
        unsafe {
            self.factory()?
                .SetScope(&scope_array)
                .map_err(|error| windows_error("set explicit indexed scopes", error))
        }
    }

    fn get_shell_item(&mut self) -> Result<(), SpikeError> {
        self.search_item = Some(unsafe {
            self.factory()?
                .GetShellItem()
                .map_err(|error| windows_error("get Search Folder shell item", error))?
        });
        Ok(())
    }

    fn enumerate(&mut self, limit: u32) -> Result<Vec<SearchHit>, SpikeError> {
        let search_item = self.search_item.as_ref().ok_or_else(|| {
            SpikeError::verification_failed("Search Folder shell item was not created")
        })?;
        let enumerator: IEnumShellItems = unsafe {
            search_item
                .BindToHandler(None, &BHID_EnumItems)
                .map_err(|error| windows_error("bind Search Folder result enumerator", error))?
        };

        let mut results = Vec::new();
        while results.len() < limit as usize {
            let mut slot: [Option<IShellItem>; 1] = [None];
            let mut fetched = 0u32;
            unsafe {
                enumerator
                    .Next(&mut slot, Some(&mut fetched))
                    .map_err(|error| windows_error("enumerate Search Folder result", error))?;
            }
            if fetched == 0 {
                break;
            }
            let item = slot[0].take().ok_or_else(|| {
                SpikeError::verification_failed("result enumerator returned null")
            })?;
            results.push(SearchHit {
                display_name: shell_item_name(
                    &item,
                    SIGDN_NORMALDISPLAY,
                    "read result display name",
                )?,
                parsing_path: shell_item_name(
                    &item,
                    SIGDN_DESKTOPABSOLUTEPARSING,
                    "read result canonical parsing path",
                )?,
            });
        }
        Ok(results)
    }
}

fn shell_item_from_scope(scope: &str) -> Result<IShellItem, SpikeError> {
    let scope = HSTRING::from(scope);
    unsafe {
        windows::Win32::UI::Shell::SHCreateItemFromParsingName(&scope, None)
            .map_err(|error| windows_error("create shell item for indexed scope", error))
    }
}

struct PidlList(Vec<*mut ITEMIDLIST>);

impl PidlList {
    fn from_shell_items(items: &[IShellItem]) -> Result<Self, SpikeError> {
        let mut pidls = Self(Vec::with_capacity(items.len()));
        for item in items {
            pidls.0.push(unsafe {
                SHGetIDListFromObject(item)
                    .map_err(|error| windows_error("get indexed scope ID list", error))
            }?);
        }
        Ok(pidls)
    }
}

impl Drop for PidlList {
    fn drop(&mut self) {
        for pidl in &self.0 {
            unsafe { CoTaskMemFree(Some((*pidl).cast::<c_void>())) };
        }
    }
}

fn shell_item_name(
    item: &IShellItem,
    format: windows::Win32::UI::Shell::SIGDN,
    context: &str,
) -> Result<String, SpikeError> {
    let value = unsafe {
        item.GetDisplayName(format)
            .map_err(|error| windows_error(context, error))?
    };
    unsafe { take_com_string(value, context) }
}

impl WindowsSearch {
    pub fn connect() -> Result<Self, SpikeError> {
        let apartment = ComApartment::initialize()?;
        let manager = unsafe {
            CoCreateInstance(&CSearchManager, None, CLSCTX_LOCAL_SERVER)
                .map_err(|error| windows_error("create CSearchManager", error))?
        };
        Ok(Self {
            _apartment: apartment,
            manager,
        })
    }

    pub fn status(&self) -> Result<SearchStatus, SpikeError> {
        let service_running = windows_search_service_running()?;
        let catalog_available = if service_running {
            unsafe { self.manager.GetCatalog(w!("SystemIndex")) }.is_ok()
        } else {
            false
        };
        Ok(SearchStatus {
            catalog: "SystemIndex".to_owned(),
            service_running,
            catalog_available,
        })
    }

    pub fn scope_evidence(&self) -> Result<ScopeEvidence, SpikeError> {
        let status = self.status()?;
        if !status.service_running || !status.catalog_available {
            return Err(SpikeError::not_runnable(
                "Windows Search service or SystemIndex is unavailable",
            ));
        }

        let rules = self.scope_rules()?;
        let exclusions = rules
            .iter()
            .filter(|rule| !rule.is_included)
            .map(|rule| rule.pattern_or_url.clone())
            .collect();
        let included_file_roots = validated_file_scopes(&status, rules)?
            .into_iter()
            .map(|scope| scope.url)
            .collect();

        Ok(ScopeEvidence {
            catalog: status.catalog,
            service_running: status.service_running,
            catalog_available: status.catalog_available,
            included_file_roots,
            exclusion_rules: exclusions,
            counters: OperationCounters::default(),
        })
    }

    pub fn indexed_scopes(&self) -> Result<Vec<IndexedScope>, SpikeError> {
        let status = self.status()?;
        let rules = self.scope_rules()?;
        validated_file_scopes(&status, rules)
    }

    fn scope_rules(&self) -> Result<Vec<CrawlRule>, SpikeError> {
        let catalog: ISearchCatalogManager = unsafe {
            self.manager
                .GetCatalog(w!("SystemIndex"))
                .map_err(|error| windows_error("open SystemIndex", error))?
        };
        let crawl = unsafe {
            catalog
                .GetCrawlScopeManager()
                .map_err(|error| windows_error("get Crawl Scope Manager", error))?
        };
        let enumerator = unsafe {
            crawl
                .EnumerateScopeRules()
                .map_err(|error| windows_error("enumerate crawl scope rules", error))?
        };

        let mut rules = Vec::new();
        loop {
            let mut slot: [Option<ISearchScopeRule>; 1] = [None];
            let mut fetched = 0u32;
            unsafe {
                enumerator
                    .Next(&mut slot, &mut fetched)
                    .map_err(|error| windows_error("read crawl scope rule", error))?;
            }
            if fetched == 0 {
                break;
            }
            let rule = slot[0]
                .take()
                .ok_or_else(|| SpikeError::verification_failed("scope enumerator returned null"))?;
            let pattern = unsafe {
                let value = rule
                    .PatternOrURL()
                    .map_err(|error| windows_error("read scope rule URL", error))?;
                take_com_string(value, "decode scope rule URL")?
            };
            let is_included = unsafe {
                rule.IsIncluded()
                    .map_err(|error| windows_error("read scope inclusion flag", error))?
                    .as_bool()
            };
            let is_default = unsafe {
                rule.IsDefault()
                    .map_err(|error| windows_error("read scope default flag", error))?
                    .as_bool()
            };
            rules.push(CrawlRule {
                pattern_or_url: pattern,
                is_included,
                is_default,
            });
        }
        Ok(rules)
    }
}

struct ComApartment;

impl ComApartment {
    fn initialize() -> Result<Self, SpikeError> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .map_err(|error| windows_error("initialize COM", error))?;
        }
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

struct ServiceHandle(SC_HANDLE);

impl Drop for ServiceHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseServiceHandle(self.0);
        }
    }
}

fn windows_search_service_running() -> Result<bool, SpikeError> {
    let manager = unsafe {
        OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT)
            .map(ServiceHandle)
            .map_err(|error| windows_error("open Service Control Manager", error))?
    };
    let service = unsafe {
        OpenServiceW(manager.0, w!("WSearch"), SERVICE_QUERY_STATUS)
            .map(ServiceHandle)
            .map_err(|error| windows_error("open Windows Search service", error))?
    };
    let mut status = MaybeUninit::<SERVICE_STATUS_PROCESS>::zeroed();
    let buffer = unsafe {
        slice::from_raw_parts_mut(
            status.as_mut_ptr().cast::<u8>(),
            size_of::<SERVICE_STATUS_PROCESS>(),
        )
    };
    let mut needed = 0u32;
    unsafe {
        QueryServiceStatusEx(service.0, SC_STATUS_PROCESS_INFO, Some(buffer), &mut needed)
            .map_err(|error| windows_error("query Windows Search service", error))?;
        Ok(status.assume_init().dwCurrentState == SERVICE_RUNNING)
    }
}

unsafe fn take_com_string(value: PWSTR, context: &str) -> Result<String, SpikeError> {
    if value.is_null() {
        return Err(SpikeError::verification_failed(format!(
            "{context}: returned a null string"
        )));
    }
    let result = unsafe { value.to_string() }
        .map_err(|error| SpikeError::verification_failed(format!("{context}: {error}")));
    unsafe { CoTaskMemFree(Some(value.0.cast::<c_void>())) };
    result
}

fn windows_error(context: &str, error: windows::core::Error) -> SpikeError {
    SpikeError::not_runnable(format!("{context}: {error}"))
}
