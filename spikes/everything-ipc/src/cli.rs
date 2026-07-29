use std::ffi::OsString;
use std::time::Duration;

use serde::Serialize;

use crate::protocol::{EverythingQueryResult, EverythingResultItem};

const DEFAULT_LIMIT: u32 = 20;
const MAX_LIMIT: u32 = 200;
const DEFAULT_TIMEOUT_MS: u64 = 1_000;
const MAX_TIMEOUT_MS: u64 = 60_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliArgs {
    pub instance: String,
    pub query: String,
    pub limit: u32,
    pub timeout: Duration,
    pub format: OutputFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliError {
    InvalidArguments,
    InvalidLimit,
    InvalidTimeout,
    InvalidFormat,
    RenderFailed,
}

pub fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<CliArgs, CliError> {
    let mut args = args.into_iter();
    let mut instance = None;
    let mut query = None;
    let mut limit = None;
    let mut timeout_ms = None;
    let mut format = None;

    while let Some(option) = args.next() {
        let option = option
            .into_string()
            .map_err(|_| CliError::InvalidArguments)?;
        let value = args
            .next()
            .ok_or(CliError::InvalidArguments)?
            .into_string()
            .map_err(|_| CliError::InvalidArguments)?;

        match option.as_str() {
            "--instance" if instance.is_none() => instance = Some(value),
            "--query" if query.is_none() => query = Some(value),
            "--limit" if limit.is_none() => {
                let parsed = value.parse::<u32>().map_err(|_| CliError::InvalidLimit)?;
                if !(1..=MAX_LIMIT).contains(&parsed) {
                    return Err(CliError::InvalidLimit);
                }
                limit = Some(parsed);
            }
            "--timeout-ms" if timeout_ms.is_none() => {
                let parsed = value.parse::<u64>().map_err(|_| CliError::InvalidTimeout)?;
                if !(1..=MAX_TIMEOUT_MS).contains(&parsed) {
                    return Err(CliError::InvalidTimeout);
                }
                timeout_ms = Some(parsed);
            }
            "--format" if format.is_none() => match value.as_str() {
                "text" => format = Some(OutputFormat::Text),
                "json" => format = Some(OutputFormat::Json),
                _ => return Err(CliError::InvalidFormat),
            },
            _ => return Err(CliError::InvalidArguments),
        }
    }

    Ok(CliArgs {
        instance: instance.unwrap_or_default(),
        query: query.ok_or(CliError::InvalidArguments)?,
        limit: limit.unwrap_or(DEFAULT_LIMIT),
        timeout: Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS)),
        format: format.unwrap_or(OutputFormat::Text),
    })
}

pub fn render_result(
    format: OutputFormat,
    result: &EverythingQueryResult,
) -> Result<String, CliError> {
    match format {
        OutputFormat::Text => Ok(render_text(result)),
        OutputFormat::Json => {
            serde_json::to_string(&JsonOutput::from(result)).map_err(|_| CliError::RenderFailed)
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonOutput<'a> {
    total: u32,
    returned: usize,
    request_flags: u32,
    sort_type: u32,
    items: Vec<JsonItem<'a>>,
}

impl<'a> From<&'a EverythingQueryResult> for JsonOutput<'a> {
    fn from(result: &'a EverythingQueryResult) -> Self {
        Self {
            total: result.total,
            returned: result.items.len(),
            request_flags: result.request_flags,
            sort_type: result.sort_type,
            items: result.items.iter().map(JsonItem::from).collect(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonItem<'a> {
    full_path: &'a str,
    file_name: &'a str,
    kind: &'static str,
    size_bytes: Option<u64>,
    modified_filetime: Option<u64>,
    attributes: u32,
}

impl<'a> From<&'a EverythingResultItem> for JsonItem<'a> {
    fn from(item: &'a EverythingResultItem) -> Self {
        Self {
            full_path: &item.full_path,
            file_name: &item.file_name,
            kind: item_kind(item.attributes),
            size_bytes: item.size_bytes,
            modified_filetime: item.modified_filetime,
            attributes: item.attributes,
        }
    }
}

fn render_text(result: &EverythingQueryResult) -> String {
    let mut output = format!(
        "total={} returned={} request_flags=0x{:08x} sort_type={}",
        result.total,
        result.items.len(),
        result.request_flags,
        result.sort_type
    );

    for item in &result.items {
        output.push('\n');
        output.push_str(item_kind(item.attributes));
        output.push('\t');
        push_optional_number(&mut output, item.modified_filetime);
        output.push('\t');
        push_optional_number(&mut output, item.size_bytes);
        output.push('\t');
        output.push_str(&sanitize_text_field(&item.full_path));
    }

    output
}

fn item_kind(attributes: u32) -> &'static str {
    if attributes & 0x10 != 0 {
        "directory"
    } else {
        "file"
    }
}

fn push_optional_number(output: &mut String, value: Option<u64>) {
    match value {
        Some(value) => output.push_str(&value.to_string()),
        None => output.push('-'),
    }
}

fn sanitize_text_field(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\r' | '\n' | '\t' => ' ',
            _ => character,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{EverythingQueryResult, EverythingResultItem};

    #[test]
    fn parses_defaults() {
        let args = parse_args(["--query", "*.rs"].map(OsString::from)).unwrap();
        assert_eq!(args.instance, "");
        assert_eq!(args.query, "*.rs");
        assert_eq!(args.limit, 20);
        assert_eq!(args.timeout, Duration::from_millis(1_000));
        assert_eq!(args.format, OutputFormat::Text);
    }

    #[test]
    fn parses_explicit_values_in_any_order() {
        let args = parse_args(
            [
                "--format",
                "json",
                "--limit",
                "200",
                "--query",
                "report",
                "--timeout-ms",
                "60000",
                "--instance",
                "Work",
            ]
            .map(OsString::from),
        )
        .unwrap();
        assert_eq!(args.instance, "Work");
        assert_eq!(args.limit, 200);
        assert_eq!(args.timeout, Duration::from_millis(60_000));
        assert_eq!(args.format, OutputFormat::Json);
    }

    #[test]
    fn rejects_invalid_contract_values() {
        assert_eq!(
            parse_args(["--limit", "0", "--query", "x"].map(OsString::from)),
            Err(CliError::InvalidLimit)
        );
        assert_eq!(
            parse_args(["--limit", "201", "--query", "x"].map(OsString::from)),
            Err(CliError::InvalidLimit)
        );
        assert_eq!(
            parse_args(["--timeout-ms", "0", "--query", "x"].map(OsString::from)),
            Err(CliError::InvalidTimeout)
        );
        assert_eq!(
            parse_args(["--timeout-ms", "60001", "--query", "x"].map(OsString::from)),
            Err(CliError::InvalidTimeout)
        );
        assert_eq!(
            parse_args(["--format", "xml", "--query", "x"].map(OsString::from)),
            Err(CliError::InvalidFormat)
        );
        assert_eq!(
            parse_args(["--query", "x", "--query", "y"].map(OsString::from)),
            Err(CliError::InvalidArguments)
        );
        assert_eq!(
            parse_args(["--unknown", "x", "--query", "y"].map(OsString::from)),
            Err(CliError::InvalidArguments)
        );
    }

    #[test]
    fn accepts_empty_queries_but_rejects_malformed_arguments() {
        assert_eq!(
            parse_args(["--query", ""].map(OsString::from))
                .unwrap()
                .query,
            ""
        );
        assert_eq!(
            parse_args(["--query"].map(OsString::from)),
            Err(CliError::InvalidArguments)
        );
        assert_eq!(
            parse_args(["positional", "--query", "x"].map(OsString::from)),
            Err(CliError::InvalidArguments)
        );
    }

    #[cfg(windows)]
    #[test]
    fn rejects_non_unicode_arguments() {
        use std::os::windows::ffi::OsStringExt;

        assert_eq!(
            parse_args([OsString::from("--query"), OsString::from_wide(&[0xd800])]),
            Err(CliError::InvalidArguments)
        );
    }

    #[test]
    fn renders_text_and_json_result_items() {
        let result = fixture();

        let text = render_result(OutputFormat::Text, &result).unwrap();
        assert!(text.contains("C:\\\\code\\\\report.rs"));
        assert!(text.contains("C:\\\\code\\\\archive"));

        let json = render_result(OutputFormat::Json, &result).unwrap();
        let output = serde_json::from_str::<serde_json::Value>(&json).unwrap();
        assert_eq!(output["returned"], 2);
        assert_eq!(output["items"][0]["kind"], "file");
        assert_eq!(
            output["items"][0]["modifiedFiletime"],
            serde_json::Value::Null
        );
        assert_eq!(output["items"][1]["kind"], "directory");
        assert_eq!(output["items"][1]["sizeBytes"], serde_json::Value::Null);
    }

    #[test]
    fn renders_text_items_on_one_line_after_sanitizing_delimiters() {
        let raw_path = "C:\\bad\rpath\nwith\ttabs";
        let result = EverythingQueryResult {
            total: 1,
            request_flags: 0x145,
            sort_type: 14,
            items: vec![EverythingResultItem {
                full_path: raw_path.to_owned(),
                file_name: "report\r\n\t.rs".to_owned(),
                attributes: 0,
                size_bytes: Some(123),
                modified_filetime: Some(456),
            }],
        };

        let text = render_result(OutputFormat::Text, &result).unwrap();

        assert_eq!(text.lines().count(), 2);
        assert!(text.contains("file\t456\t123\tC:\\bad path with tabs"));
        assert!(!text.contains(raw_path));
    }

    fn fixture() -> EverythingQueryResult {
        EverythingQueryResult {
            total: 7,
            request_flags: 0x145,
            sort_type: 14,
            items: vec![
                EverythingResultItem {
                    full_path: "C:\\\\code\\\\report.rs".to_owned(),
                    file_name: "report.rs".to_owned(),
                    attributes: 0,
                    size_bytes: Some(123),
                    modified_filetime: None,
                },
                EverythingResultItem {
                    full_path: "C:\\\\code\\\\archive".to_owned(),
                    file_name: "archive".to_owned(),
                    attributes: 0x10,
                    size_bytes: None,
                    modified_filetime: Some(456),
                },
            ],
        }
    }
}
