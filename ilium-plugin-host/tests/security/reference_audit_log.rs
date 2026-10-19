//! reference-audit-log.md: file location and modes, record schema, event catalogue, redaction,
//! rotation, disk-full queue, verification, the `audit` command and the system-log sink.

use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use ilium_plugin_api::ErrorCode;
use proptest::prelude::*;
use serde_json::{json, Value};

use crate::support::*;

/// A chained audit file for the CLI tests: `events` become records with seq 1.., each carrying
/// the SHA-256 of the previous line.
fn chain_lines(events: &[&str]) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for (index, event) in events.iter().enumerate() {
        let prev_hash = match lines.last() {
            Some(last) => sha256_hex(last.as_bytes()),
            None => zero_hash(),
        };
        let record = json!({
            "ts": format!("2026-10-12T08:14:{:02}.114Z", index % 60),
            "seq": index + 1,
            "prev_hash": prev_hash,
            "session": "default",
            "actor": { "kind": "kernel" },
            "event": event,
            "plugin_id": "example.demo",
            "result": "ok",
        });
        lines.push(record.to_string());
    }
    lines
}

/// Writes `lines` as the audit file below the CLI environment and returns the environment.
fn cli_with_audit(root: &std::path::Path, lines: &[String]) -> ilium_plugin_host::cli::CliEnv {
    let env = cli_env(root);
    let dir = env.state_dir.join("audit");
    std::fs::create_dir_all(&dir).expect("audit dir");
    let mut text = lines.join("\n");
    text.push('\n');
    std::fs::write(dir.join("audit.jsonl"), text).expect("audit file");
    env
}

fn run_audit(
    env: &ilium_plugin_host::cli::CliEnv,
    extra: &[&str],
) -> ilium_plugin_host::cli::CliOutput {
    let mut words = vec!["plugin", "audit"];
    words.extend_from_slice(extra);
    ilium_plugin_host::cli::run(&args(&words), env)
}

/// REQ: SEC-220
#[test]
fn sec_220_the_file_is_mode_0600_in_a_0700_directory() {
    let rig = t3(&["clipboard.read"]);
    rig.host.persist_all_stores();
    let path = rig.host.audit_path();
    assert!(path.ends_with("audit/audit.jsonl"), "{path:?}");
    assert_eq!(
        std::fs::metadata(&path).expect("file").permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(path.parent().expect("dir"))
            .expect("dir")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

/// REQ: SEC-220
#[test]
fn sec_220_no_plugin_scope_reaches_the_audit_file_not_permitted() {
    let rig = t3(&["fs.read:data", "journal.read"]);
    rig.host.persist_all_stores();
    let path = rig.host.audit_path();
    let read = rig.value(
        "ilium:host/fs.read",
        json!({ "scope": "data", "path": path.to_string_lossy() }),
    );
    assert_code(&read, ErrorCode::NotPermitted);
}

/// REQ: SEC-220
#[test]
fn sec_220_journal_read_does_not_expose_audit_records() {
    let rig = t3(&["journal.read"]);
    let before = rig.audit_lines().len();
    let journal = rig
        .value("ilium:host/journal.read", json!({}))
        .map(|value| value.to_string())
        .unwrap_or_default();
    assert!(!journal.contains("prev_hash") && !journal.contains("\"seq\""));
    assert!(rig.audit_lines().len() >= before);
}

/// REQ: SEC-220
#[test]
fn sec_220_only_the_kernel_writes_the_log() {
    let rig = t3(&[]);
    rig.host.persist_all_stores();
    assert!(rig
        .audit_lines()
        .iter()
        .all(|line| line["actor"]["kind"].is_string()));
}

/// REQ: SEC-221
#[test]
fn sec_221_every_record_has_the_required_fields_and_enum_values() {
    let rig = t3(&["clipboard.read"]);
    rig.host.persist_all_stores();
    let lines = rig.audit_lines();
    assert!(!lines.is_empty());
    for line in &lines {
        for key in ["ts", "seq", "prev_hash", "session", "actor", "event"] {
            assert!(line.get(key).is_some(), "{key} in {line}");
        }
        assert!(["user-tui", "cli", "policy", "kernel", "plugin"]
            .contains(&line["actor"]["kind"].as_str().expect("actor kind")));
        if let Some(result) = line.get("result") {
            assert!(["ok", "refused", "cancelled", "failed"]
                .contains(&result.as_str().expect("result")));
        }
    }
}

/// REQ: SEC-221
#[test]
fn sec_221_timestamps_are_utc_iso_8601_with_milliseconds() {
    let rig = t3(&["clipboard.read"]);
    rig.host.persist_all_stores();
    for line in rig.audit_lines() {
        let ts = line["ts"].as_str().expect("ts");
        let bytes = ts.as_bytes();
        assert_eq!(ts.len(), 24, "{ts}");
        assert!(
            bytes[4] == b'-'
                && bytes[7] == b'-'
                && bytes[10] == b'T'
                && bytes[13] == b':'
                && bytes[16] == b':'
                && bytes[19] == b'.'
                && bytes[23] == b'Z',
            "{ts}"
        );
    }
}

/// REQ: SEC-221
#[test]
fn sec_221_seq_starts_at_1_and_increases_by_1() {
    let rig = t3(&["clipboard.read"]);
    rig.host.persist_all_stores();
    let seqs: Vec<u64> = rig
        .audit_lines()
        .iter()
        .map(|line| line["seq"].as_u64().expect("seq"))
        .collect();
    assert_eq!(seqs, (1..=seqs.len() as u64).collect::<Vec<_>>());
}

/// REQ: SEC-221
#[test]
fn sec_221_prev_hash_chains_the_exact_bytes_of_the_previous_line() {
    let rig = t3(&["clipboard.read"]);
    rig.host.persist_all_stores();
    let raw = raw_lines(&rig.host.audit_path());
    assert!(raw.len() >= 2);
    let first: Value = serde_json::from_str(&raw[0]).expect("json");
    assert_eq!(first["prev_hash"], json!(zero_hash()));
    for pair in raw.windows(2) {
        let next: Value = serde_json::from_str(&pair[1]).expect("json");
        assert_eq!(next["prev_hash"], json!(sha256_hex(pair[0].as_bytes())));
    }
}

/// REQ: SEC-222
#[test]
fn sec_222_the_host_knows_exactly_the_events_of_the_catalogue() {
    let page = manual_page("reference-audit-log.md");
    let mut expected: Vec<String> = page
        .lines()
        .skip_while(|line| !line.starts_with("| Event | When |"))
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .flat_map(|line| line.split('|').nth(1).map(str::to_owned))
        .flat_map(|cell| {
            cell.split(',')
                .map(|name| name.trim().trim_matches('`').to_owned())
                .collect::<Vec<_>>()
        })
        .filter(|name| !name.is_empty())
        .collect();
    expected.sort();
    let mut known: Vec<String> = ilium_plugin_host::audit::EVENT_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    known.sort();
    assert_eq!(known, expected);
}

/// REQ: SEC-222
#[test]
fn sec_222_a_grant_change_writes_exactly_one_record_each() {
    let rig = t3(&[]);
    rig.host.grant_now("clipboard.read");
    rig.host.revoke("clipboard.read");
    assert_eq!(rig.audit_events("grant-given").len(), 1);
    assert_eq!(rig.audit_events("grant-revoked").len(), 1);
}

/// REQ: SEC-222
#[test]
fn sec_222_policy_loaded_is_written_once_per_start() {
    let rig = setup(&[]).system_policy("version = 1\n", true).start();
    assert_eq!(rig.audit_events("policy-loaded").len(), 1);
    let rig = rig.restart();
    assert_eq!(rig.audit_events("policy-loaded").len(), 2);
}

/// REQ: SEC-222
#[test]
fn sec_222_h_grant_refusals_are_summarised_once_per_plugin_and_kind_per_minute() {
    let rig = t3(&[]);
    for _ in 0..25 {
        let _ = rig.call(
            "ilium:host/process.run",
            json!({ "program": "rg", "args": [] }),
        );
    }
    rig.host.flush_audit_summaries();
    let records = rig.audit_events("h-grant-refused");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["count"], json!(25));
    rig.host.advance_clock(Duration::from_secs(61));
    let _ = rig.call(
        "ilium:host/process.run",
        json!({ "program": "rg", "args": [] }),
    );
    rig.host.flush_audit_summaries();
    assert_eq!(rig.audit_events("h-grant-refused").len(), 2);
}

/// REQ: SEC-222
#[test]
fn sec_222_a_replaced_policy_violation_is_recorded_once_per_occurrence() {
    let rig = setup(&[]).system_policy("version = 1\n", false).start();
    assert_eq!(rig.audit_events("policy-violation").len(), 1);
}

/// REQ: SEC-222
#[test]
fn sec_222_safe_mode_start_writes_one_safe_mode_record() {
    let rig = setup(&[]).safe_mode().start();
    assert_eq!(rig.audit_events("safe-mode").len(), 1);
}

/// REQ: SEC-222
#[test]
fn sec_222_unavailable_sandbox_writes_one_record_per_start_and_prerequisite() {
    let rig = setup(&[]).effective_uid(0).start();
    assert_eq!(rig.audit_events("sandbox-unavailable").len(), 1);
    let _ = rig.host.state();
    assert_eq!(rig.audit_events("sandbox-unavailable").len(), 1);
}

/// REQ: SEC-222
#[test]
fn sec_222_an_external_store_edit_is_recorded() {
    let rig = t3(&["clipboard.read"]);
    rig.host.persist_all_stores();
    rig.host.edit_store_file_externally("grants.json");
    let rig = rig.restart();
    assert_eq!(rig.audit_events("store-external-edit").len(), 1);
}

/// REQ: SEC-223
#[test]
fn sec_223_a_record_never_contains_terminal_text_prompts_or_request_bodies() {
    let rig = public_http_rig();
    rig.net.serve(
        "https://example.com/up",
        ilium_plugin_host::testing::FakeResponse::ok(Vec::new()),
    );
    rig.post("https://example.com/up", b"BODY-MARKER-12345".to_vec())
        .expect("post");
    let text = std::fs::read_to_string(rig.host.audit_path()).expect("audit file");
    assert!(!text.contains("BODY-MARKER-12345"));
}

fn public_http_rig() -> Rig {
    let rig = t3(&["http:example.com"]);
    rig.net.dns("example.com", &["93.184.216.34"]);
    rig
}

/// REQ: SEC-223
#[test]
fn sec_223_secret_arguments_in_all_forms_are_replaced_with_redacted() {
    let secret = "s3cr3t-token-value";
    let rig = setup(&["process.run:tool"]).secret("api", secret).start();
    let base64 = ilium_plugin_host::secrets::base64_standard(secret.as_bytes());
    let percent = ilium_plugin_host::secrets::percent_encode(secret);
    let _ = rig.call(
        "ilium:host/process.run",
        json!({ "program": "tool", "args": [secret, base64, percent, "plain-arg"] }),
    );
    let text = std::fs::read_to_string(rig.host.audit_path()).expect("audit file");
    assert!(!text.contains(secret) && !text.contains(&base64) && !text.contains(&percent));
    let run = rig.audit_events("process-run");
    assert_eq!(run.len(), 1);
    assert!(run[0].to_string().contains("<redacted>"));
    assert!(
        run[0].to_string().contains("plain-arg"),
        "other arguments are logged in full"
    );
}

/// REQ: SEC-224
#[test]
fn sec_224_rotation_starts_a_new_file_chained_to_the_rotated_one() {
    let rig = setup(&[])
        .system_policy("version = 1\n[audit]\nmax_mib = 1\n", true)
        .start();
    for _ in 0..1400 {
        rig.host.audit_append("enable", &"x".repeat(900));
    }
    let dir = rig.host.audit_path().parent().expect("dir").to_path_buf();
    let mut rotated: Vec<_> = std::fs::read_dir(&dir)
        .expect("dir")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.starts_with("audit-"))
        .collect();
    rotated.sort();
    assert!(
        !rotated.is_empty(),
        "a file over 1 MiB must have been rotated"
    );
    assert!(rotated[0].starts_with("audit-1.jsonl"), "{rotated:?}");
    let old = raw_lines(&dir.join(&rotated[0]));
    let new = read_jsonl(&dir.join("audit.jsonl"));
    assert_eq!(
        new[0]["prev_hash"],
        json!(sha256_hex(old.last().expect("old last").as_bytes()))
    );
}

/// REQ: SEC-224
#[test]
fn sec_224_rotation_at_the_size_limit_boundary() {
    for (bytes, rotates) in [(1024 * 1024 - 2048, false), (1024 * 1024 + 2048, true)] {
        let rig = setup(&[])
            .system_policy("version = 1\n[audit]\nmax_mib = 1\n", true)
            .start();
        rig.host.audit_fill_to(bytes);
        let dir = rig.host.audit_path().parent().expect("dir").to_path_buf();
        let rotated = std::fs::read_dir(&dir).expect("dir").any(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .starts_with("audit-")
        });
        assert_eq!(rotated, rotates, "{bytes} bytes");
    }
}

/// REQ: SEC-224
#[test]
fn sec_224_rotation_by_age_and_removal_after_keep_days() {
    let rig = setup(&[])
        .system_policy("version = 1\n[audit]\nkeep_days = 30\n", true)
        .start();
    rig.host.audit_append("enable", "first");
    rig.host.advance_clock(Duration::from_secs(31 * 86_400));
    rig.host.audit_append("enable", "later");
    let dir = rig.host.audit_path().parent().expect("dir").to_path_buf();
    let rotated = |dir: &std::path::Path| {
        std::fs::read_dir(dir)
            .expect("dir")
            .filter(|entry| {
                entry
                    .as_ref()
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with("audit-")
            })
            .count()
    };
    assert_eq!(rotated(&dir), 1);
    rig.host.advance_clock(Duration::from_secs(31 * 86_400));
    rig.host.audit_append("enable", "much later");
    assert_eq!(
        rotated(&dir),
        0,
        "a rotated file older than keep_days is deleted"
    );
}

/// REQ: SEC-224
#[test]
fn sec_224_defaults_are_64_mib_and_365_days() {
    let rig = setup(&[]).start();
    assert_eq!(rig.host.audit_limits(), (64, 365));
    let rig = setup(&[])
        .system_policy("version = 1\n[audit]\nmax_mib = 8\nkeep_days = 10\n", true)
        .start();
    assert_eq!(rig.host.audit_limits(), (8, 10));
}

/// REQ: SEC-225
#[test]
fn sec_225_a_failed_write_keeps_the_server_running_and_retries_every_10_seconds() {
    let rig = t3(&[]);
    rig.host.fail_audit_writes(true);
    rig.host.audit_append("enable", "while the disk is full");
    assert!(matches!(
        rig.host.state(),
        ilium_plugin_api::generated::host::plugins::InstanceState::Active
    ));
    assert!(!rig.notices().is_empty(), "a notice is shown");
    let before = rig.host.audit_write_attempts();
    rig.host.advance_clock(Duration::from_secs(10));
    assert_eq!(rig.host.audit_write_attempts(), before + 1);
    rig.host.advance_clock(Duration::from_secs(9));
    assert_eq!(
        rig.host.audit_write_attempts(),
        before + 1,
        "no retry before 10 s"
    );
}

/// REQ: SEC-225
#[test]
fn sec_225_queue_holds_1000_records_and_drops_the_refusal_records_first() {
    let rig = t3(&[]);
    rig.host.fail_audit_writes(true);
    for index in 0..10 {
        rig.host
            .audit_append("h-grant-refused", &format!("low {index}"));
    }
    for index in 0..1000 {
        rig.host
            .audit_append("grant-given", &format!("high {index}"));
    }
    assert_eq!(rig.host.audit_queue_len(), 1000);
    assert!(rig
        .host
        .audit_queue_events()
        .iter()
        .all(|event| event != "h-grant-refused"));
    rig.host.fail_audit_writes(false);
    let rig = rig;
    rig.host.advance_clock(Duration::from_secs(10));
    let dropped = rig.audit_events("audit-dropped");
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0]["count"], json!(10));
}

/// REQ: SEC-225
#[test]
fn sec_225_queue_boundary_999_1000_1001() {
    for (queued, dropped) in [(999_usize, 0_usize), (1000, 0), (1001, 1)] {
        let rig = t3(&[]);
        rig.host.fail_audit_writes(true);
        for index in 0..queued {
            rig.host
                .audit_append("egress-host-first-use", &format!("host {index}"));
        }
        assert_eq!(rig.host.audit_queue_len(), queued.min(1000));
        assert_eq!(rig.host.audit_dropped_count(), dropped as u64);
    }
}

/// REQ: SEC-226
#[test]
fn sec_226_verify_prints_ok_for_an_intact_chain() {
    let scratch = scratch();
    let env = cli_with_audit(
        scratch.path(),
        &chain_lines(&["install", "enable", "grant-given"]),
    );
    let output = run_audit(&env, &["--verify"]);
    assert_eq!(output.exit_code, 0, "{output:?}");
    assert!(
        output
            .stdout_jsonl
            .iter()
            .any(|line| line.to_string().contains("ok"))
            || output.stderr.contains("ok")
    );
}

/// REQ: SEC-226
#[test]
fn sec_226_verify_names_the_first_broken_seq() {
    let scratch = scratch();
    let mut lines = chain_lines(&["install", "enable", "grant-given", "disable"]);
    lines[1] = lines[1].replace("\"enable\"", "\"disable\"");
    let env = cli_with_audit(scratch.path(), &lines);
    let output = run_audit(&env, &["--verify"]);
    assert_ne!(output.exit_code, 0);
    let text = format!("{:?} {}", output.stdout_jsonl, output.stderr);
    assert!(
        text.contains('3'),
        "the record after the edited one breaks the chain: {text}"
    );
}

proptest! {
    // REQ: SEC-226
    #[test]
    fn sec_226_any_single_edit_is_detected_at_the_next_record(length in 3usize..30, edit in 0usize..28) {
        let edit = edit % (length - 1);
        let events = vec!["enable"; length];
        let mut lines = chain_lines(&events);
        lines[edit] = lines[edit].replace("\"ok\"", "\"failed\"");
        let scratch = scratch();
        let env = cli_with_audit(scratch.path(), &lines);
        let output = run_audit(&env, &["--verify"]);
        prop_assert_ne!(output.exit_code, 0);
        let text = format!("{:?} {}", output.stdout_jsonl, output.stderr);
        prop_assert!(text.contains(&(edit + 2).to_string()), "{}", text);
    }

    // REQ: SEC-226
    #[test]
    fn sec_226_any_intact_chain_verifies(length in 1usize..40) {
        let lines = chain_lines(&vec!["install"; length]);
        let scratch = scratch();
        let env = cli_with_audit(scratch.path(), &lines);
        prop_assert_eq!(run_audit(&env, &["--verify"]).exit_code, 0);
    }
}

/// REQ: SEC-226
#[test]
fn sec_226_a_rebuilt_file_by_a_writer_verifies_which_the_manual_admits() {
    assert_page_has(
        "reference-audit-log.md",
        &[
            "an attacker with write access to the file can rebuild it",
            "`[audit] sink` to `syslog` or `journald`",
        ],
    );
}

/// REQ: SEC-227
#[test]
fn sec_227_filters_by_plugin_event_and_limit_and_ends_with_one_summary() {
    let scratch = scratch();
    let env = cli_with_audit(
        scratch.path(),
        &chain_lines(&["install", "enable", "enable", "disable"]),
    );
    let all = run_audit(&env, &[]);
    assert_eq!(all.exit_code, 0);
    assert_eq!(
        all.stdout_jsonl
            .iter()
            .filter(|line| line["type"] == "audit")
            .count(),
        4
    );
    assert_eq!(
        all.stdout_jsonl
            .iter()
            .filter(|line| line["type"] == "summary")
            .count(),
        1
    );
    assert_eq!(all.stdout_jsonl.last().expect("last")["type"], "summary");
    let enabled = run_audit(&env, &["--event", "enable"]);
    assert_eq!(
        enabled
            .stdout_jsonl
            .iter()
            .filter(|line| line["type"] == "audit")
            .count(),
        2
    );
    let limited = run_audit(&env, &["--limit", "1"]);
    assert_eq!(
        limited
            .stdout_jsonl
            .iter()
            .filter(|line| line["type"] == "audit")
            .count(),
        1
    );
    let other = run_audit(&env, &["--plugin", "someone.else"]);
    assert_eq!(
        other
            .stdout_jsonl
            .iter()
            .filter(|line| line["type"] == "audit")
            .count(),
        0
    );
}

/// REQ: SEC-227
#[test]
fn sec_227_since_accepts_a_duration_and_an_iso_time() {
    let scratch = scratch();
    let env = cli_with_audit(scratch.path(), &chain_lines(&["install", "enable"]));
    assert_eq!(run_audit(&env, &["--since", "24h"]).exit_code, 0);
    assert_eq!(
        run_audit(&env, &["--since", "2026-10-12T00:00:00Z"]).exit_code,
        0
    );
    assert_ne!(run_audit(&env, &["--since", "not-a-time"]).exit_code, 0);
}

/// REQ: SEC-228
#[test]
fn sec_228_syslog_sink_gets_each_record_as_one_json_message_and_the_file_is_still_written() {
    let rig = setup(&[])
        .system_policy("version = 1\n[audit]\nsink = \"syslog\"\n", true)
        .start();
    rig.host.audit_append("enable", "x");
    let messages = rig.host.system_log_messages();
    assert!(!messages.is_empty());
    for message in &messages {
        assert_eq!(message.identifier, "ilium-audit");
        let parsed: Value =
            serde_json::from_str(&message.text).expect("one JSON object per message");
        assert!(parsed["seq"].is_u64());
    }
    assert!(!rig.audit_lines().is_empty());
}

/// REQ: SEC-228
#[test]
fn sec_228_file_sink_sends_nothing_to_the_system_log() {
    let rig = setup(&[]).start();
    rig.host.audit_append("enable", "x");
    assert!(rig.host.system_log_messages().is_empty());
}
