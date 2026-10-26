use ilium_plugin_host::{
    cli::{
        session_commands::{run, run_with_log_reader, SessionLog, SessionLogReader},
        CliEnv, MemorySession,
    },
    registry_client::{FetchError, Fetcher},
};
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};
struct NoNetwork;
impl Fetcher for NoNetwork {
    fn get(&self, _: &str) -> Result<Vec<u8>, FetchError> {
        panic!("session reads cannot fetch or spawn")
    }
}
fn env(session: Option<Arc<MemorySession>>) -> CliEnv {
    let mut env = CliEnv::new(
        "/isolated/data".into(),
        "/isolated/state".into(),
        "/isolated/config".into(),
        "/isolated".into(),
        SystemTime::UNIX_EPOCH + Duration::from_secs(120),
        true,
        Arc::new(NoNetwork),
    );
    if let Some(session) = session {
        env = env.with_session(session);
    }
    env
}
fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).into()).collect()
}
#[test]
fn absent_and_unreachable_sessions_refuse_without_starting_one() {
    assert_eq!(run("list", &[], &env(None)).exit_code, 6);
    let session = MemorySession::unreachable();
    assert_eq!(run("list", &[], &env(Some(session.clone()))).exit_code, 6);
    assert_eq!(session.sessions_started(), 0);
}
#[test]
fn invalid_arguments_refuse_before_connect() {
    let session = MemorySession::new();
    let env = env(Some(session.clone()));
    for (command, arguments) in [
        ("list", vec!["unexpected"]),
        ("grants", vec!["example.test", "--revoke", "tree.read"]),
        ("logs", vec!["example.test", "--level", "loud"]),
        ("logs", vec!["example.test", "--since", "😃"]),
        ("logs", vec!["example.test", "--tail", "-1"]),
    ] {
        assert_eq!(run(command, &args(&arguments), &env).exit_code, 2);
    }
    assert_eq!(session.connect_attempts(), 0);
    assert_eq!(session.sessions_started(), 0);
}
#[test]
fn list_and_grants_report_actual_observations_without_mutation() {
    let session = MemorySession::new();
    session.add_plugin(
        "example.test",
        "2.3.4",
        "T2",
        "disabled-by-policy",
        true,
        &[
            ("tree.read", "normal", "granted"),
            ("pty.write:focused", "high", "revoked"),
        ],
    );
    let env = env(Some(session.clone()));
    let listed = run("list", &args(&["--json"]), &env);
    assert_eq!(listed.exit_code, 0);
    assert_eq!(listed.stdout_jsonl[0]["state"], "disabled-by-policy");
    assert_eq!(listed.stdout_jsonl[0]["grants_granted"], 1);
    assert!(run("list", &[], &env)
        .stdout_text
        .contains("T2 [dev] disabled grants 1 of 2"));
    let grants = run("grants", &args(&["example.test", "--json"]), &env);
    assert_eq!(grants.stdout_jsonl[1]["state"], "revoked");
    assert_eq!(session.started_plugin_count(), 0);
    assert!(session.journal().is_empty());
}
struct Logs {
    session: Arc<MemorySession>,
}
impl SessionLogReader for Logs {
    fn read(&self, id: &str) -> Result<Vec<SessionLog>, String> {
        Ok(self
            .session
            .log_lines(id)
            .into_iter()
            .enumerate()
            .map(|(index, (side, level, message))| SessionLog {
                at: SystemTime::UNIX_EPOCH + Duration::from_secs(60 + index as u64),
                local_time: format!("1970-01-01 00:01:{index:02}"),
                side,
                level,
                message,
            })
            .collect())
    }
    fn secrets(&self, _: &str) -> Result<Vec<String>, String> {
        Ok(vec!["secret-value".into()])
    }
}
#[test]
fn timestamped_reader_preserves_order_and_filters_then_tails_and_redacts() {
    let session = MemorySession::new();
    session.add_plugin("example.test", "1.0.0", "T3", "running", false, &[]);
    session.add_log_line("example.test", "server", "info", "first");
    session.add_log_line("example.test", "host", "warn", "second secret-value");
    session.add_log_line("example.test", "host", "error", "third secret-value\nnext");
    let env = env(Some(session.clone()));
    let logs = Logs { session };
    let output = run_with_log_reader(
        "logs",
        &args(&[
            "example.test",
            "--side",
            "host",
            "--level",
            "warn",
            "--tail",
            "1",
            "--since",
            "2m",
            "--json",
        ]),
        &env,
        Some(&logs),
    );
    assert_eq!(output.exit_code, 0);
    assert_eq!(output.stdout_jsonl[0]["at"], "1970-01-01 00:01:02");
    assert_eq!(output.stdout_jsonl[0]["message"], "third <redacted>\\nnext");
    assert_eq!(output.stdout_jsonl.len(), 3);
    assert!(!output.stdout_raw.contains("secret-value"));
}
#[test]
fn historical_timestamps_and_follow_are_never_fabricated() {
    let session = MemorySession::new();
    let env = env(Some(session.clone()));
    let missing = run("logs", &args(&["example.test"]), &env);
    assert_eq!(missing.exit_code, 1);
    assert_eq!(missing.stdout_jsonl[0]["code"], "logs.context-required");
    let connects = session.connect_attempts();
    let follow = run("logs", &args(&["example.test", "--follow"]), &env);
    assert_eq!(follow.exit_code, 1);
    assert_eq!(session.connect_attempts(), connects);
}
