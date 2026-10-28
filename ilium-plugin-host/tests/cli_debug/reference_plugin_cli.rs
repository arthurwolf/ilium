//! reference-plugin-cli.md: inspect, doctor, trace and profile (DEV-730, 731, 800..803, 810, 811,
//! 820..823, 830).

use std::sync::Arc;

use ilium_plugin_host::cli::ProfileCall;
use ilium_plugin_host::package::write_package;
use serde_json::Value;

use crate::common::{
    env, env_with_session, error_code, record_types, records_of, run, scratch, FakeBackend,
    FakeProbe,
};

const ID: &str = "example.notes";

fn package_file(root: &std::path::Path, recorded_hash: &str) -> String {
    let manifest = format!(
        r#"{{"id":"{ID}","version":"0.1.0","files":[{{"path":"dist/server.wasm","sha256":"{recorded_hash}"}}]}}"#
    );
    let bytes = write_package(&[
        ("manifest.json".to_owned(), manifest.into_bytes()),
        ("dist/server.wasm".to_owned(), b"\0asm".to_vec()),
    ])
    .expect("package");
    let path = root.join("pkg.ilp");
    std::fs::write(&path, bytes).expect("write package");
    path.to_str().expect("utf8").to_owned()
}

fn stopped() -> FakeBackend {
    let mut backend = FakeBackend::new(ID);
    backend.running = false;
    backend
}

/// REQ: DEV-730
#[test]
fn dev_730_view_of_a_disabled_plugin_is_inspect_no_render() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), stopped());
    let output = run(&env, &["inspect", ID, "--view", "main", "--json"]);
    assert_eq!(error_code(&output).as_deref(), Some("inspect.no-render"));
    assert_eq!(output.exit_code, 1);
}

/// REQ: DEV-731
#[test]
fn dev_731_profile_of_a_disabled_plugin_is_plugin_not_running() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), stopped());
    let output = run(&env, &["profile", ID, "--json"]);
    assert_eq!(error_code(&output).as_deref(), Some("plugin.not-running"));
    assert_eq!(output.exit_code, 1);
}

/// REQ: DEV-800
#[test]
fn dev_800_inspect_of_a_file_works_without_a_session() {
    let scratch = scratch();
    let file = package_file(scratch.path(), "00");
    let output = run(&env(scratch.path()), &["inspect", &file, "--json"]);
    assert_ne!(error_code(&output).as_deref(), Some("session.unreachable"));
}

/// REQ: DEV-800
#[test]
fn dev_800_inspect_of_an_id_without_a_session_is_session_unreachable() {
    let scratch = scratch();
    let output = run(&env(scratch.path()), &["inspect", ID, "--json"]);
    assert_eq!(error_code(&output).as_deref(), Some("session.unreachable"));
    assert_eq!(output.exit_code, 6);
}

/// REQ: DEV-801
#[test]
fn dev_801_hash_mismatch_prints_a_diff_record_and_exits_1() {
    let scratch = scratch();
    let file = package_file(scratch.path(), "00");
    let output = run(
        &env(scratch.path()),
        &["inspect", &file, "--verify", "--json"],
    );
    assert!(!records_of(&output, "diff").is_empty());
    assert_eq!(
        error_code(&output).as_deref(),
        Some("inspect.hash-mismatch")
    );
    assert_eq!(output.exit_code, 1);
}

/// REQ: DEV-802
#[test]
fn dev_802_view_and_memory_need_a_session() {
    let scratch = scratch();
    let env = env(scratch.path());
    for flag in [vec!["--view", "main"], vec!["--memory"]] {
        let mut args = vec!["inspect", ID];
        args.extend(flag);
        args.push("--json");
        let output = run(&env, &args);
        assert_eq!(error_code(&output).as_deref(), Some("session.unreachable"));
    }
}

/// REQ: DEV-803
#[test]
fn dev_803_unreadable_file_is_inspect_file_unreadable() {
    let scratch = scratch();
    let missing = scratch.path().join("nope.ilp");
    let output = run(
        &env(scratch.path()),
        &["inspect", missing.to_str().expect("utf8"), "--json"],
    );
    assert_eq!(
        error_code(&output).as_deref(),
        Some("inspect.file-unreadable")
    );
}

/// REQ: DEV-803
#[test]
fn dev_803_other_file_is_inspect_not_a_package() {
    let scratch = scratch();
    let text = scratch.path().join("notes.txt");
    std::fs::write(&text, "hello").expect("write");
    let output = run(
        &env(scratch.path()),
        &["inspect", text.to_str().expect("utf8"), "--json"],
    );
    assert_eq!(
        error_code(&output).as_deref(),
        Some("inspect.not-a-package")
    );
}

fn doctor(probe: FakeProbe) -> ilium_plugin_host::cli::CliOutput {
    let scratch = scratch();
    let env = env(scratch.path()).with_probe(Arc::new(probe));
    run(&env, &["doctor", "--json"])
}

fn status_of(output: &ilium_plugin_host::cli::CliOutput, item: &str) -> String {
    records_of(output, "doctor-check")
        .iter()
        .find(|r| r["item"] == item)
        .and_then(|r| r["status"].as_str())
        .unwrap_or("missing")
        .to_owned()
}

/// REQ: DEV-810
#[test]
fn dev_810_one_record_per_item_in_table_order() {
    let output = doctor(FakeProbe::default());
    let items: Vec<&str> = records_of(&output, "doctor-check")
        .iter()
        .filter_map(|r| r["item"].as_str())
        .collect();
    assert_eq!(
        items,
        [
            "rustc",
            "rust-target",
            "bun",
            "sdk-pin",
            "wit-pins",
            "host-version",
            "platform-tiers",
            "trust-store",
            "session",
            "disk"
        ]
    );
}

/// REQ: DEV-810
#[test]
fn dev_810_records_carry_item_status_message_and_fix() {
    let output = doctor(FakeProbe::default());
    for record in records_of(&output, "doctor-check") {
        for key in ["item", "status", "message", "fix"] {
            assert!(record.get(key).is_some(), "{key} missing in {record}");
        }
    }
}

/// REQ: DEV-811
#[test]
fn dev_811_a_failed_item_exits_4() {
    let probe = FakeProbe {
        rust_target: false,
        ..FakeProbe::default()
    };
    let output = doctor(probe);
    assert_eq!(status_of(&output, "rust-target"), "fail");
    assert_eq!(output.exit_code, 4);
}

/// REQ: DEV-811
#[test]
fn dev_811_warnings_alone_exit_0() {
    let output = doctor(FakeProbe::default());
    assert_eq!(status_of(&output, "session"), "warn");
    assert_eq!(output.exit_code, 0);
}

/// REQ: DEV-811
#[test]
fn dev_811_disk_boundaries() {
    let mib = 1024 * 1024;
    for (free, expected) in [
        (127 * mib, "fail"),
        (128 * mib, "warn"),
        (511 * mib, "warn"),
        (512 * mib, "ok"),
    ] {
        let output = doctor(FakeProbe {
            free_bytes: free,
            ..FakeProbe::default()
        });
        assert_eq!(status_of(&output, "disk"), expected, "free {free}");
    }
}

fn trace_backend() -> FakeBackend {
    FakeBackend::new(ID)
}

/// REQ: DEV-820
#[test]
fn dev_820_trace_of_an_unknown_id_is_plugin_not_found() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), trace_backend());
    assert_eq!(
        error_code(&run(&env, &["trace", "example.unknown", "--json"])).as_deref(),
        Some("plugin.not-found")
    );
}

/// REQ: DEV-820
#[test]
fn dev_820_trace_needs_a_session() {
    let scratch = scratch();
    assert_eq!(
        error_code(&run(&env(scratch.path()), &["trace", ID, "--json"])).as_deref(),
        Some("session.unreachable")
    );
}

/// REQ: DEV-821
#[test]
fn dev_821_last_defaults_to_50_records() {
    let scratch = scratch();
    let mut backend = trace_backend();
    let mut ring = ilium_plugin_host::telemetry::TraceRing::new();
    ring.begin_instance();
    for n in 0..80 {
        ring.push(
            n,
            ilium_plugin_host::telemetry::TraceKind::Event,
            serde_json::json!({}),
        );
    }
    backend.trace = ring.records();
    let env = env_with_session(scratch.path(), backend);
    let output = run(&env, &["trace", ID, "--json"]);
    assert_eq!(records_of(&output, "trace").len(), 50);
}

/// REQ: DEV-821
#[test]
fn dev_821_kinds_accepts_a_comma_list() {
    let scratch = scratch();
    let mut backend = trace_backend();
    let mut ring = ilium_plugin_host::telemetry::TraceRing::new();
    ring.begin_instance();
    use ilium_plugin_host::telemetry::TraceKind::*;
    for (n, kind) in [Event, Trap, Render, Lifecycle].into_iter().enumerate() {
        ring.push(n as u64, kind, serde_json::json!({}));
    }
    backend.trace = ring.records();
    let env = env_with_session(scratch.path(), backend);
    let output = run(&env, &["trace", ID, "--kinds", "trap,render", "--json"]);
    let kinds: Vec<&str> = records_of(&output, "trace")
        .iter()
        .filter_map(|r| r["kind"].as_str())
        .collect();
    assert_eq!(kinds, ["trap", "render"]);
}

/// REQ: DEV-822
#[test]
fn dev_822_record_writes_a_replay_file_and_prints_an_artifact_record() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), trace_backend());
    let file = scratch.path().join("run.replay");
    let output = run(
        &env,
        &[
            "trace",
            ID,
            "--record",
            file.to_str().expect("utf8"),
            "--json",
        ],
    );
    assert!(file.exists());
    let artifacts = records_of(&output, "artifact");
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0]["kind"], Value::from("replay"));
}

/// REQ: DEV-823
#[test]
fn dev_823_unredacted_is_refused_for_a_non_dev_plugin() {
    let scratch = scratch();
    let mut backend = trace_backend();
    backend.dev = false;
    let env = env_with_session(scratch.path(), backend);
    let output = run(&env, &["trace", ID, "--unredacted", "--json"]);
    assert_eq!(
        error_code(&output).as_deref(),
        Some("trace.unredacted-denied")
    );
}

fn profile_backend() -> FakeBackend {
    let mut backend = FakeBackend::new(ID);
    backend.calls = vec![ProfileCall {
        export: "render".to_owned(),
        duration_ms: 2.0,
    }];
    backend
}

/// REQ: DEV-830
#[test]
fn dev_830_duration_defaults_to_10() {
    let scratch = scratch();
    let backend = Arc::new(profile_backend());
    let env = env(scratch.path()).with_session(backend.clone());
    run(&env, &["profile", ID, "--json"]);
    assert_eq!(*backend.requested_duration.lock().expect("lock"), Some(10));
}

/// REQ: DEV-830
#[test]
fn dev_830_range_is_1_to_600() {
    for (value, ok) in [("0", false), ("1", true), ("600", true), ("601", false)] {
        let scratch = scratch();
        let env = env_with_session(scratch.path(), profile_backend());
        let output = run(&env, &["profile", ID, "--duration", value, "--json"]);
        assert_eq!(output.exit_code == 0, ok, "duration {value}");
        if !ok {
            assert_eq!(output.exit_code, 2);
        }
    }
}

/// REQ: DEV-830
#[test]
fn dev_830_one_timing_per_export_that_ran_then_a_summary() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), profile_backend());
    let output = run(&env, &["profile", ID, "--json"]);
    assert_eq!(record_types(&output)[..], ["timing", "summary", "result"]);
}

/// REQ: DEV-730
#[test]
fn dev_730_refused_view_prints_no_render_artifact() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), stopped());
    let output = run(&env, &["inspect", ID, "--view", "main", "--json"]);
    assert!(records_of(&output, "artifact").is_empty());
    assert_eq!(error_code(&output).as_deref(), Some("inspect.no-render"));
}

/// REQ: DEV-731
#[test]
fn dev_731_profile_of_a_running_plugin_is_not_refused_as_not_running() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), FakeBackend::new(ID));
    let output = run(&env, &["profile", ID, "--json"]);
    assert_ne!(error_code(&output).as_deref(), Some("plugin.not-running"));
}

/// REQ: DEV-801
#[test]
fn dev_801_hash_mismatch_diff_record_names_the_differing_file() {
    let scratch = scratch();
    let file = package_file(scratch.path(), "00");
    let output = run(
        &env(scratch.path()),
        &["inspect", &file, "--verify", "--json"],
    );
    let diffs = records_of(&output, "diff");
    assert!(!diffs.is_empty());
    assert!(diffs
        .iter()
        .any(|record| record.to_string().contains("dist/server.wasm")));
}

/// REQ: DEV-802
#[test]
fn dev_802_session_unreachable_exits_6_for_view_and_memory() {
    let scratch = scratch();
    let env = env(scratch.path());
    for flag in [vec!["--view", "main"], vec!["--memory"]] {
        let mut args = vec!["inspect", ID];
        args.extend(flag);
        args.push("--json");
        let output = run(&env, &args);
        assert_eq!(output.exit_code, 6);
    }
}

/// REQ: DEV-822
#[test]
fn dev_822_replay_artifact_record_names_the_written_file() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), trace_backend());
    let file = scratch.path().join("second.replay");
    let output = run(
        &env,
        &[
            "trace",
            ID,
            "--record",
            file.to_str().expect("utf8"),
            "--json",
        ],
    );
    let artifacts = records_of(&output, "artifact");
    assert_eq!(artifacts.len(), 1);
    assert!(artifacts[0].to_string().contains("second.replay"));
    assert!(std::fs::metadata(&file).expect("replay file").len() > 0);
}
