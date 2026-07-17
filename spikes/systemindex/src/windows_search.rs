use std::ffi::c_void;
use std::mem::{MaybeUninit, size_of};
use std::slice;

use windows::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize,
};
use windows::Win32::System::Search::{
    CSearchManager, ISearchCatalogManager, ISearchManager, ISearchScopeRule,
};
use windows::Win32::System::Services::{
    CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatusEx, SC_HANDLE,
    SC_MANAGER_CONNECT, SC_STATUS_PROCESS_INFO, SERVICE_QUERY_STATUS, SERVICE_RUNNING,
    SERVICE_STATUS_PROCESS,
};
use windows::core::{PCWSTR, PWSTR, w};

use crate::{
    CrawlRule, IndexedScope, OperationCounters, ScopeEvidence, SearchStatus, SpikeError,
    validated_file_scopes,
};

pub struct WindowsSearch {
    _apartment: ComApartment,
    manager: ISearchManager,
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
                take_com_string(value)?
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

unsafe fn take_com_string(value: PWSTR) -> Result<String, SpikeError> {
    if value.is_null() {
        return Err(SpikeError::verification_failed(
            "scope rule returned a null URL",
        ));
    }
    let result = unsafe { value.to_string() }.map_err(|error| {
        SpikeError::verification_failed(format!("decode scope rule URL: {error}"))
    });
    unsafe { CoTaskMemFree(Some(value.0.cast::<c_void>())) };
    result
}

fn windows_error(context: &str, error: windows::core::Error) -> SpikeError {
    SpikeError::not_runnable(format!("{context}: {error}"))
}
