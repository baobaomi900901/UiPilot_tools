use std::env;
use std::fmt;
use std::process::ExitCode;
use std::time::Instant;

use everything_ipc::cli::{parse_args as parse_cli_args, render_result, CliError};
use everything_ipc::client::{EverythingClient, EverythingClientError};
use everything_ipc::protocol::{EverythingQuerySpec, EverythingSort};

const REQUEST_NAME: u32 = 0x0000_0001;
const REQUEST_FULL_PATH_AND_NAME: u32 = 0x0000_0004;
const REQUEST_SIZE: u32 = 0x0000_0010;
const REQUEST_DATE_MODIFIED: u32 = 0x0000_0040;
const REQUEST_ATTRIBUTES: u32 = 0x0000_0100;
const REQUEST_FLAGS: u32 = REQUEST_NAME
    | REQUEST_FULL_PATH_AND_NAME
    | REQUEST_SIZE
    | REQUEST_DATE_MODIFIED
    | REQUEST_ATTRIBUTES;

#[derive(Debug)]
enum ProbeError {
    Cli(CliError),
    DeadlineOverflow,
    Client(EverythingClientError),
}

impl ProbeError {
    fn code(&self) -> &'static str {
        match self {
            Self::Cli(CliError::InvalidArguments) => "E_ARGUMENTS",
            Self::Cli(CliError::InvalidLimit) => "E_LIMIT",
            Self::Cli(CliError::InvalidTimeout) => "E_TIMEOUT",
            Self::Cli(CliError::InvalidFormat) => "E_FORMAT",
            Self::Cli(CliError::RenderFailed) => "E_RENDER",
            Self::Client(EverythingClientError::ConnectionTimedOut)
            | Self::Client(EverythingClientError::IpcUnavailable) => "E_EVERYTHING_UNAVAILABLE",
            Self::Client(EverythingClientError::QueryTimedOut) => "E_QUERY_TIMEOUT",
            Self::Client(EverythingClientError::Protocol(_)) => "E_PROTOCOL",
            Self::DeadlineOverflow | Self::Client(_) => "E_IPC",
        }
    }

    fn exit_code(&self) -> u8 {
        match self {
            Self::Cli(CliError::RenderFailed) => 4,
            Self::Client(EverythingClientError::ConnectionTimedOut)
            | Self::Client(EverythingClientError::IpcUnavailable)
            | Self::Client(EverythingClientError::QueryTimedOut) => 3,
            Self::Cli(_) => 2,
            Self::DeadlineOverflow | Self::Client(_) => 4,
        }
    }
}

impl fmt::Display for ProbeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Cli(CliError::InvalidArguments) => "invalid arguments",
            Self::Cli(CliError::InvalidLimit) => "invalid result limit",
            Self::Cli(CliError::InvalidTimeout) => "invalid timeout",
            Self::Cli(CliError::InvalidFormat) => "invalid output format",
            Self::Cli(CliError::RenderFailed) => "failed to render query result",
            Self::DeadlineOverflow => "query deadline overflow",
            Self::Client(error) => return error.fmt(formatter),
        })
    }
}

impl From<CliError> for ProbeError {
    fn from(error: CliError) -> Self {
        Self::Cli(error)
    }
}

impl From<EverythingClientError> for ProbeError {
    fn from(error: EverythingClientError) -> Self {
        Self::Client(error)
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}: {error}", error.code());
            ExitCode::from(error.exit_code())
        }
    }
}

fn run() -> Result<(), ProbeError> {
    let args = parse_cli_args(env::args_os().skip(1))?;
    let client = EverythingClient::connect(&args.instance, args.timeout)?;
    let deadline = Instant::now()
        .checked_add(args.timeout)
        .ok_or(ProbeError::DeadlineOverflow)?;
    let result = client.query(EverythingQuerySpec {
        search: args.query.encode_utf16().collect(),
        offset: 0,
        max_results: args.limit,
        request_flags: REQUEST_FLAGS,
        sort: EverythingSort::DateModifiedDescending,
        deadline,
    })?;
    println!("{}", render_result(args.format, &result)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use everything_ipc::protocol::ProtocolError;

    #[test]
    fn maps_expected_failures_to_stable_codes() {
        assert_eq!(ProbeError::Cli(CliError::InvalidLimit).code(), "E_LIMIT");
        assert_eq!(ProbeError::Cli(CliError::InvalidLimit).exit_code(), 2);
        assert_eq!(
            ProbeError::Client(EverythingClientError::ConnectionTimedOut).code(),
            "E_EVERYTHING_UNAVAILABLE"
        );
        assert_eq!(
            ProbeError::Client(EverythingClientError::QueryTimedOut).code(),
            "E_QUERY_TIMEOUT"
        );
        assert_eq!(
            ProbeError::Client(EverythingClientError::QueryTimedOut).exit_code(),
            3
        );
        assert_eq!(ProbeError::Cli(CliError::RenderFailed).code(), "E_RENDER");
        assert_eq!(ProbeError::Cli(CliError::RenderFailed).exit_code(), 4);
        assert_eq!(
            ProbeError::Client(EverythingClientError::Protocol(
                ProtocolError::PayloadTooShort
            ))
            .code(),
            "E_PROTOCOL"
        );
    }
}
