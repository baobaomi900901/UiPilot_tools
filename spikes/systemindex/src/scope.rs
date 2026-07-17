use std::collections::HashSet;

use serde::Serialize;

use crate::{OperationCounters, SpikeError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchStatus {
    pub catalog: String,
    pub service_running: bool,
    pub catalog_available: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrawlRule {
    pub pattern_or_url: String,
    pub is_included: bool,
    pub is_default: bool,
}

impl CrawlRule {
    pub fn included(pattern_or_url: impl Into<String>) -> Self {
        Self {
            pattern_or_url: pattern_or_url.into(),
            is_included: true,
            is_default: false,
        }
    }

    pub fn excluded(pattern_or_url: impl Into<String>) -> Self {
        Self {
            pattern_or_url: pattern_or_url.into(),
            is_included: false,
            is_default: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IndexedScope {
    pub url: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeEvidence {
    pub catalog: String,
    pub service_running: bool,
    pub catalog_available: bool,
    pub included_file_roots: Vec<String>,
    pub exclusion_rules: Vec<String>,
    pub counters: OperationCounters,
}

pub fn validated_file_scopes(
    status: &SearchStatus,
    rules: Vec<CrawlRule>,
) -> Result<Vec<IndexedScope>, SpikeError> {
    if !status.service_running {
        return Err(SpikeError::not_runnable(
            "Windows Search service is stopped",
        ));
    }
    if !status.catalog_available || status.catalog != "SystemIndex" {
        return Err(SpikeError::not_runnable(
            "SystemIndex catalog is unavailable",
        ));
    }

    let mut seen = HashSet::new();
    let scopes = rules
        .into_iter()
        .filter(|rule| rule.is_included && is_local_file_directory_rule(&rule.pattern_or_url))
        .filter_map(|rule| {
            seen.insert(rule.pattern_or_url.clone())
                .then_some(IndexedScope {
                    url: rule.pattern_or_url,
                })
        })
        .collect::<Vec<_>>();

    if scopes.is_empty() {
        return Err(SpikeError::not_runnable(
            "no provable indexed local file scope is available",
        ));
    }
    Ok(scopes)
}

fn is_local_file_directory_rule(value: &str) -> bool {
    const PREFIX: &str = "file:///";
    let Some(prefix) = value.get(..PREFIX.len()) else {
        return false;
    };
    if !prefix.eq_ignore_ascii_case(PREFIX) {
        return false;
    }
    let path = &value[PREFIX.len()..];
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
        && matches!(bytes.last(), Some(b'/' | b'\\'))
        && !path.contains(['*', '?'])
}
