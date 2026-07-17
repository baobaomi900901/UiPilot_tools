mod error;
mod query;
mod scope;
mod windows_search;

use std::ffi::OsString;

use serde::Serialize;

pub use error::{ErrorEvidence, ErrorKind, OperationCounters, SpikeError};
pub use query::{
    QueryEvidence, QueryOperations, SearchBackend, SearchHit, execute_indexed_literal_query,
    run_query_operations,
};
pub use scope::{CrawlRule, IndexedScope, ScopeEvidence, SearchStatus, validated_file_scopes};
pub use windows_search::WindowsSearch;

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "command", rename_all = "camelCase")]
pub enum Command {
    Status,
    Scopes,
    Query { literal: String, limit: u32 },
}

pub fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Command, SpikeError> {
    let values = args
        .into_iter()
        .skip(1)
        .map(|value| {
            value
                .into_string()
                .map_err(|_| SpikeError::invalid_input("arguments must be valid Unicode"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    match values.as_slice() {
        [command, json] if command == "status" && json == "--json" => Ok(Command::Status),
        [command, json] if command == "scopes" && json == "--json" => Ok(Command::Scopes),
        [command, literal_flag, literal, limit_flag, limit, json]
            if command == "query"
                && literal_flag == "--literal"
                && limit_flag == "--limit"
                && json == "--json" =>
        {
            validate_literal(literal)?;
            let limit = limit
                .parse::<u32>()
                .map_err(|_| SpikeError::invalid_input("limit must be an integer from 1 to 100"))?;
            if !(1..=100).contains(&limit) {
                return Err(SpikeError::invalid_input(
                    "limit must be an integer from 1 to 100",
                ));
            }
            Ok(Command::Query {
                literal: literal.clone(),
                limit,
            })
        }
        _ => Err(SpikeError::invalid_input("unsupported command shape")),
    }
}

pub(crate) fn validate_literal(literal: &str) -> Result<(), SpikeError> {
    let mut count = 0usize;
    for value in literal.chars() {
        count += 1;
        if value <= '\u{1f}' {
            return Err(SpikeError::invalid_input(
                "literal must not contain U+0000 through U+001F",
            ));
        }
    }
    if !(1..=256).contains(&count) {
        return Err(SpikeError::invalid_input(
            "literal must contain 1 to 256 Unicode scalar values",
        ));
    }
    Ok(())
}
