//! reference-plugin-cli.md session commands, checked against the real `ilium` binary
//! with no session running (DEV-180, DEV-192).

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
    tempfile::tempdir_in("/media/arthur/tmp")
        .unwrap_or_else(|_| tempfile::tempdir().expect("tempdir"))
}

/// REQ: DEV-180
#[test]
fn dev_180_dev_without_a_session_exits_six_with_one_error_and_starts_no_session() {
    let dir = scratch();
    let project = dir.path().join("proj");
    std::fs::create_dir_all(&project).expect("mkdir");
    let started = std::time::Instant::now();
    let output = ilium(
        dir.path(),
        &[
            "dev",
            project.to_str().expect("utf8"),
            "--cwd",
            dir.path().to_str().expect("utf8"),
        ],
    );
    assert_eq!(output.status.code(), Some(6));
    let records = lines(&output);
    assert_eq!(records.iter().filter(|r| r["type"] == "error").count(), 1);
    assert_eq!(records[0]["code"], "session.unreachable");
    assert_eq!(records[records.len() - 1]["type"], "result");
    assert_eq!(records[records.len() - 1]["exit_code"], 6);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "no retry loop"
    );
    let sockets = std::fs::read_dir(dir.path().join("run"))
        .map(|d| d.count())
        .unwrap_or(0);
    assert_eq!(sockets, 0, "dev must not start a session");
}

/// REQ: DEV-180
#[test]
fn dev_180_session_flag_names_a_missing_session_and_still_exits_six() {
    let dir = scratch();
    let output = ilium(
        dir.path(),
        &[
            "dev",
            "--session",
            "no-such-session",
            "--cwd",
            dir.path().to_str().expect("utf8"),
        ],
    );
    assert_eq!(output.status.code(), Some(6));
}

/// REQ: DEV-192
#[test]
fn dev_192_singular_log_is_not_a_command_and_exits_two() {
    let dir = scratch();
    let output = ilium(dir.path(), &["log", "acme.demo"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(lines(&output).iter().all(|r| r["type"] != "log"));
}

/// REQ: DEV-192
#[test]
fn dev_192_plural_logs_is_a_command_that_reaches_the_session_check() {
    let dir = scratch();
    let output = ilium(
        dir.path(),
        &[
            "logs",
            "acme.demo",
            "--cwd",
            dir.path().to_str().expect("utf8"),
        ],
    );
    assert_eq!(output.status.code(), Some(6));
}
