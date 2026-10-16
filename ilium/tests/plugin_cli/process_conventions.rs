//! reference-plugin-cli.md conventions, checked against the real `ilium` binary (DEV-003..016).

use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::Value;

fn ilium(runtime: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ilium"))
        .arg("plugin")
        .args(args)
        .env("XDG_RUNTIME_DIR", runtime.join("run"))
        .env("XDG_STATE_HOME", runtime.join("state"))
        .env("XDG_CONFIG_HOME", runtime.join("config"))
        .env("XDG_DATA_HOME", runtime.join("data"))
        .stdin(Stdio::null())
        .output()
        .expect("spawn ilium")
}

fn lines(output: &Output) -> Vec<Value> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("not JSON: {line}: {e}")))
        .collect()
}

fn scratch() -> tempfile::TempDir {
    tempfile::tempdir_in("/media/arthur/tmp").unwrap_or_else(|_| tempfile::tempdir().expect("tempdir"))
}

/// REQ: DEV-003
#[test]
fn dev_003_help_exits_zero() {
    let dir = scratch();
    assert_eq!(ilium(dir.path(), &["--help"]).status.code(), Some(0));
}

/// REQ: DEV-007
#[test]
fn dev_007_process_exit_code_equals_the_result_record() {
    let dir = scratch();
    let output = ilium(dir.path(), &["trust", "list", "--json"]);
    let records = lines(&output);
    let result = records.last().expect("result");
    assert_eq!(result["type"], "result");
    assert_eq!(Some(result["exit_code"].as_i64().expect("exit_code") as i32), output.status.code());
}

/// REQ: DEV-004
/// REQ: DEV-006
#[test]
fn dev_006_session_command_without_a_session_exits_six_without_retry() {
    let dir = scratch();
    let started = std::time::Instant::now();
    let output = ilium(dir.path(), &["list", "--json", "--cwd", dir.path().to_str().expect("utf8")]);
    assert_eq!(output.status.code(), Some(6));
    let records = lines(&output);
    assert_eq!(records.iter().filter(|r| r["type"] == "error").count(), 1);
    assert_eq!(records[0]["code"], "session.unreachable");
    assert!(started.elapsed() < std::time::Duration::from_secs(10), "no retry loop");
}

/// REQ: DEV-010
#[test]
fn dev_010_usage_error_exits_two_and_writes_no_files() {
    let dir = scratch();
    let target = dir.path().join("proj");
    let output = ilium(dir.path(), &["new", target.to_str().expect("utf8"), "--template", "no-such-template"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(!target.exists());
}

/// REQ: DEV-016
#[test]
fn dev_016_stdout_is_jsonl_and_text_goes_to_stderr() {
    let dir = scratch();
    let output = ilium(dir.path(), &["trust", "list", "--json"]);
    let records = lines(&output);
    assert!(records.iter().all(|r| r.as_object().and_then(|o| o.keys().next()).map(String::as_str) == Some("type")));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("\u{1b}["), "no ANSI on stdout");
}

/// REQ: DEV-017
#[test]
fn dev_017_summary_then_result_are_the_last_two_lines() {
    let dir = scratch();
    let records = lines(&ilium(dir.path(), &["trust", "list", "--json"]));
    let n = records.len();
    assert!(n >= 2);
    assert_eq!(records[n - 2]["type"], "summary");
    assert_eq!(records[n - 1]["type"], "result");
}
