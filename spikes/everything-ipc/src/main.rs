use std::env;
use std::fmt;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use everything_ipc::client::{EverythingClient, EverythingClientError};
use everything_ipc::protocol::{EverythingQuerySpec, EverythingSort};

const DEFAULT_TIMEOUT_MS: u64 = 1_000;
const MAX_TIMEOUT_MS: u64 = 60_000;
const MAX_RESULTS: u32 = 200;
const REQUEST_NAME: u32 = 0x0000_0001;
const REQUEST_FULL_PATH_AND_NAME: u32 = 0x0000_0004;
const REQUEST_DATE_MODIFIED: u32 = 0x0000_0040;
const REQUEST_ATTRIBUTES: u32 = 0x0000_0100;
const REQUEST_FLAGS: u32 =
    REQUEST_NAME | REQUEST_FULL_PATH_AND_NAME | REQUEST_DATE_MODIFIED | REQUEST_ATTRIBUTES;

struct ProbeArgs {
    instance: String,
    query: String,
    timeout: Duration,
}

#[derive(Debug)]
enum ProbeError {
    InvalidArguments,
    InvalidTimeout,
    DeadlineOverflow,
    Client(EverythingClientError),
}

impl fmt::Display for ProbeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidArguments => "invalid probe arguments",
            Self::InvalidTimeout => "invalid probe timeout",
            Self::DeadlineOverflow => "probe deadline overflow",
            Self::Client(error) => return error.fmt(formatter),
        })
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
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), ProbeError> {
    let args = parse_args(env::args_os().skip(1))?;
    let client = EverythingClient::connect(&args.instance, args.timeout)?;
    let deadline = Instant::now()
        .checked_add(args.timeout)
        .ok_or(ProbeError::DeadlineOverflow)?;
    let result = client.query(EverythingQuerySpec {
        search: args.query.encode_utf16().collect(),
        offset: 0,
        max_results: MAX_RESULTS,
        request_flags: REQUEST_FLAGS,
        sort: EverythingSort::DateModifiedDescending,
        deadline,
    })?;
    println!(
        "total={} returned={} request_flags=0x{:08x} sort_type={}",
        result.total,
        result.items.len(),
        result.request_flags,
        result.sort_type
    );
    Ok(())
}

fn parse_args(args: impl IntoIterator<Item = std::ffi::OsString>) -> Result<ProbeArgs, ProbeError> {
    let mut instance = None;
    let mut query = None;
    let mut timeout_ms = None;
    let mut args = args.into_iter();

    while let Some(argument) = args.next() {
        let argument = argument
            .into_string()
            .map_err(|_| ProbeError::InvalidArguments)?;
        let value = args.next().ok_or(ProbeError::InvalidArguments)?;
        let value = value
            .into_string()
            .map_err(|_| ProbeError::InvalidArguments)?;
        match argument.as_str() {
            "--instance" if instance.is_none() => instance = Some(value),
            "--query" if query.is_none() => query = Some(value),
            "--timeout-ms" if timeout_ms.is_none() => {
                timeout_ms = Some(parse_timeout(&value)?);
            }
            _ => return Err(ProbeError::InvalidArguments),
        }
    }

    Ok(ProbeArgs {
        instance: instance.ok_or(ProbeError::InvalidArguments)?,
        query: query.ok_or(ProbeError::InvalidArguments)?,
        timeout: Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS)),
    })
}

fn parse_timeout(value: &str) -> Result<u64, ProbeError> {
    let timeout_ms = value
        .parse::<u64>()
        .map_err(|_| ProbeError::InvalidTimeout)?;
    if timeout_ms == 0 || timeout_ms > MAX_TIMEOUT_MS {
        return Err(ProbeError::InvalidTimeout);
    }
    Ok(timeout_ms)
}
