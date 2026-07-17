use std::ffi::OsString;
use std::process::Command as ProcessCommand;

use systemindex_spike::{Command, parse_args};

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

#[test]
fn parses_only_the_three_supported_command_shapes() {
    assert_eq!(
        parse_args(args(&["systemindex-spike", "status", "--json"])).unwrap(),
        Command::Status
    );
    assert_eq!(
        parse_args(args(&["systemindex-spike", "scopes", "--json"])).unwrap(),
        Command::Scopes
    );
    assert_eq!(
        parse_args(args(&[
            "systemindex-spike",
            "query",
            "--literal",
            "报告*.txt",
            "--limit",
            "20",
            "--json",
        ]))
        .unwrap(),
        Command::Query {
            literal: "报告*.txt".to_owned(),
            limit: 20,
        }
    );
}

#[test]
fn enforces_literal_and_limit_boundaries() {
    for literal in ["", "has\u{0}control", "has\u{1f}control"] {
        assert!(
            parse_args(args(&[
                "systemindex-spike",
                "query",
                "--literal",
                literal,
                "--limit",
                "20",
                "--json",
            ]))
            .is_err()
        );
    }

    let accepted = "界".repeat(256);
    let rejected = "界".repeat(257);
    assert!(
        parse_args(args(&[
            "systemindex-spike",
            "query",
            "--literal",
            &accepted,
            "--limit",
            "1",
            "--json",
        ]))
        .is_ok()
    );
    assert!(
        parse_args(args(&[
            "systemindex-spike",
            "query",
            "--literal",
            &rejected,
            "--limit",
            "1",
            "--json",
        ]))
        .is_err()
    );

    for limit in ["0", "101", "not-a-number"] {
        assert!(
            parse_args(args(&[
                "systemindex-spike",
                "query",
                "--literal",
                "report",
                "--limit",
                limit,
                "--json",
            ]))
            .is_err()
        );
    }
}

#[test]
fn rejects_unknown_flags_and_caller_supplied_paths_or_scopes() {
    for extra in [
        ["--scope", "file:///C:/"],
        ["--path", r"C:\"],
        ["--unknown", "value"],
    ] {
        let mut values = vec![
            "systemindex-spike",
            "query",
            "--literal",
            "report",
            "--limit",
            "20",
            "--json",
        ];
        values.extend(extra);
        assert!(parse_args(args(&values)).is_err());
    }
}

#[test]
fn invalid_cli_input_exits_nonzero_with_json_on_stderr() {
    let output = ProcessCommand::new(env!("CARGO_BIN_EXE_systemindex-spike"))
        .args(["query", "--literal", "", "--limit", "20", "--json"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let evidence: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(evidence["kind"], "invalidInput");
}
