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
    assert_eq!(
        run("grants", &args(&["example.test"]), &env(None)).exit_code,
        6
    );
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

fn offline_environment(root: &std::path::Path) -> CliEnv {
    let mut env = env(None);
    env.data_dir = root.join("data");
    env.config_dir = root.join("config");
    env.state_dir = root.join("state");
    env.cwd = root.to_owned();
    env
}
fn installed_row(id: &str) -> serde_json::Value {
    serde_json::json!({"id":id,"active_version":"1.0.0","previous_version":null,
        "state":"installed-enabled","source":"file","source_url":null,"channel":"file",
        "publisher_id":null,"key_id":null,"package_sha256":"a".repeat(64),"manifest_sha256":"b".repeat(64),
        "pinned":false,"skip":[],"declined":[],"failure_stopped":[],
        "installed_at":"2030-01-01T12:00:00Z","update":null})
}
#[test]
fn offline_list_and_audit_missing_state_are_empty_without_creating_files() {
    let root = tempfile::tempdir().unwrap();
    let env = offline_environment(root.path());
    for command in ["list", "audit"] {
        let output = run(command, &args(&["--json"]), &env);
        assert_eq!(output.exit_code, 0, "{}", output.stdout_raw);
        assert_eq!(output.stdout_jsonl.len(), 2);
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
#[test]
fn offline_list_honors_each_durable_flag_without_inventing_runtime_metadata() {
    use ilium_plugin_host::registry::{EnableFlags, InstalledRegistry};
    let root = tempfile::tempdir().unwrap();
    let env = offline_environment(root.path());
    std::fs::create_dir_all(env.data_dir.join("plugins")).unwrap();
    std::fs::create_dir_all(&env.config_dir).unwrap();
    let mut registry =
        InstalledRegistry::open(&env.data_dir.join("plugins/installed.json")).unwrap();
    registry
        .put_complete_record(installed_row("example.first"))
        .unwrap();
    registry
        .put_complete_record(installed_row("example.second"))
        .unwrap();
    let mut flags = EnableFlags::open(&env.config_dir.join("config.toml")).unwrap();
    flags
        .set_enabled(
            &ilium_plugin_api::ids::PluginId("example.first".into()),
            false,
        )
        .unwrap();
    let output = run("list", &args(&["--json"]), &env);
    assert_eq!(output.exit_code, 0, "{}", output.stdout_raw);
    let first = output
        .stdout_jsonl
        .iter()
        .find(|record| record["id"] == "example.first")
        .unwrap();
    let second = output
        .stdout_jsonl
        .iter()
        .find(|record| record["id"] == "example.second")
        .unwrap();
    assert_eq!(first["details"]["disabled"], true);
    assert_eq!(second["details"]["enabled"], true);
    assert_eq!(first["details"]["runtime_observed"], false);
    assert!(first.get("tier").is_none());
    assert!(first.get("state").is_none());
    assert!(first.get("grants_requested").is_none());
}
#[test]
fn malformed_enable_flags_refuse_without_rewriting_user_configuration() {
    let root = tempfile::tempdir().unwrap();
    let env = offline_environment(root.path());
    std::fs::create_dir_all(&env.config_dir).unwrap();
    let bytes = b"[plugins.\"example.first\"]\nenabled = 'false'\n";
    std::fs::write(env.config_dir.join("config.toml"), bytes).unwrap();
    let output = run("list", &args(&["--json"]), &env);
    assert_eq!(output.exit_code, 1);
    assert_eq!(output.stdout_jsonl[0]["code"], "config.invalid");
    assert_eq!(
        std::fs::read(env.config_dir.join("config.toml")).unwrap(),
        bytes
    );
    assert!(!env.data_dir.exists());
}
#[test]
fn audit_reads_verified_actual_integrity_event_and_refuses_tampered_chain() {
    use ilium_plugin_host::audit::{
        AuditActor, AuditContext, AuditLog, AuditRecord, AuditRetention,
    };
    let root = tempfile::tempdir().unwrap();
    let env = offline_environment(root.path());
    let path = env.state_dir.join("audit/audit.jsonl");
    let mut writer = AuditLog::open_with_context(
        path.clone(),
        Some(AuditContext {
            session: "read-only-audit-test".into(),
            actor: AuditActor {
                kind: "cli".into(),
                pane: None,
                plugin: None,
            },
        }),
        AuditRetention::default(),
    )
    .unwrap();
    writer
        .append(&AuditRecord {
            at_unix_millis: 120_000,
            plugin: "example.first".into(),
            action: "integrity-failure".into(),
            detail_json: "{\"result\":\"failed\",\"reason\":\"manifest.file-hash\"}".into(),
        })
        .unwrap();
    // The next genuine record anchors the preceding record hash; the current
    // format cannot authenticate an unanchored final record against replacement.
    writer
        .append(&AuditRecord {
            at_unix_millis: 121_000,
            plugin: "example.second".into(),
            action: "consent-given".into(),
            detail_json: "{\"result\":\"ok\",\"version\":\"0.1.0\",\"tier\":\"t3\"}".into(),
        })
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    let output = run(
        "audit",
        &args(&[
            "--json",
            "--plugin",
            "example.first",
            "--event",
            "integrity-failure",
        ]),
        &env,
    );
    assert_eq!(output.exit_code, 0, "{}", output.stdout_raw);
    assert_eq!(output.stdout_jsonl[0]["event"], "integrity-failure");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let altered = String::from_utf8(before)
        .unwrap()
        .replace("manifest.file-hash", "manifest.file-xxxx");
    std::fs::write(&path, altered.as_bytes()).unwrap();
    let refused = run("audit", &args(&["--json"]), &env);
    assert_ne!(refused.exit_code, 0);
    assert!(!refused
        .stdout_jsonl
        .iter()
        .any(|record| record["type"] == "audit"));
    assert_eq!(std::fs::read(&path).unwrap(), altered.as_bytes());
}
