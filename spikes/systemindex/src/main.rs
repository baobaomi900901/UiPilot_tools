use std::process::ExitCode;

use systemindex_spike::{
    Command, OperationCounters, SpikeError, WindowsSearch, execute_indexed_literal_query,
    parse_args,
};

fn main() -> ExitCode {
    match parse_args(std::env::args_os()) {
        Ok(command) => execute(command),
        Err(error) => {
            eprintln!("{}", serde_json::to_string(&error.evidence()).unwrap());
            ExitCode::from(error.exit_code() as u8)
        }
    }
}

fn execute(command: Command) -> ExitCode {
    let result = WindowsSearch::connect().and_then(|search| match command {
        Command::Status => search.status().map(|status| {
            serde_json::json!({
                "catalog": status.catalog,
                "serviceRunning": status.service_running,
                "catalogAvailable": status.catalog_available,
                "counters": OperationCounters::default(),
            })
        }),
        Command::Scopes => search.scope_evidence().and_then(|evidence| {
            serde_json::to_value(evidence)
                .map_err(|error| SpikeError::verification_failed(error.to_string()))
        }),
        Command::Query { literal, limit } => {
            execute_indexed_literal_query(&search, &literal, limit).and_then(|evidence| {
                serde_json::to_value(evidence)
                    .map_err(|error| SpikeError::verification_failed(error.to_string()))
            })
        }
    });

    match result {
        Ok(evidence) => {
            println!("{evidence}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}", serde_json::to_string(&error.evidence()).unwrap());
            ExitCode::from(error.exit_code() as u8)
        }
    }
}
