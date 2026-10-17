//! reference-test-harness.md "Replay files": DEV-540 to DEV-544.

use std::path::PathBuf;

use ilium_plugin_testkit::replay::ReplayHeader;
use ilium_plugin_testkit::FakeHost;

use crate::cli_support::{project, run_test};
use crate::common::{client_batch, spy_reset, temp_dir, with_clean_env, Spy};

/// Records one session of the spy plugin and returns the replay file path.
fn record_session(label: &str, advance_ms: u64) -> PathBuf {
    spy_reset();
    let path = temp_dir(label).join("session.replay.jsonl");
    let mut host = FakeHost::new();
    host.record_to(&path);
    host.activate::<Spy>();
    if advance_ms > 0 {
        host.advance_clock(advance_ms);
    }
    host.deliver(client_batch(2));
    host.settle();
    path
}

fn lines_of(path: &PathBuf) -> Vec<String> {
    std::fs::read_to_string(path)
        .expect("replay file")
        .lines()
        .map(str::to_string)
        .collect()
}

fn replay_on_fresh_host(path: &PathBuf) -> Result<(), ilium_plugin_testkit::ReplayMismatch> {
    spy_reset();
    let mut host = FakeHost::new();
    host.activate::<Spy>();
    host.replay(path)
}

// ---- DEV-540 ---------------------------------------------------------------------------------

/// REQ: DEV-540
#[test]
fn dev_540_record_to_writes_a_file() {
    with_clean_env(|| {
        let path = record_session("r540a", 0);
        assert!(path.is_file());
        assert!(!lines_of(&path).is_empty());
    });
}

/// REQ: DEV-540
#[test]
fn dev_540_the_header_is_the_first_line() {
    with_clean_env(|| {
        let path = record_session("r540b", 0);
        let first = &lines_of(&path)[0];
        let header: ReplayHeader = serde_json::from_str(first).expect("header line");
        assert_eq!(header.kind, "header");
        assert_eq!(header.version, 1);
        assert_eq!(header.api, "ilium:plugin@0.1.0");
        assert_eq!(header.side, "server");
        assert_eq!(header.seed, 1);
        assert_eq!(header.start, "2030-01-01 00:00:00");
    });
}

/// REQ: DEV-540
#[test]
fn dev_540_only_the_first_line_is_a_header() {
    with_clean_env(|| {
        let path = record_session("r540c", 0);
        let headers = lines_of(&path)
            .iter()
            .filter(|line| line.contains("\"type\":\"header\""))
            .count();
        assert_eq!(headers, 1);
    });
}

/// REQ: DEV-540
#[test]
fn dev_540_every_line_is_a_json_object_with_a_type() {
    with_clean_env(|| {
        let path = record_session("r540d", 0);
        for line in lines_of(&path) {
            let value: serde_json::Value = serde_json::from_str(&line).expect("json line");
            assert!(value["type"].is_string(), "line without type: {line}");
        }
    });
}

/// REQ: DEV-540
#[test]
fn dev_540_the_delivered_batch_is_recorded_as_an_event_batch_line() {
    with_clean_env(|| {
        let path = record_session("r540e", 0);
        let batches: Vec<serde_json::Value> = lines_of(&path)
            .iter()
            .filter_map(|line| serde_json::from_str(line).ok())
            .filter(|value: &serde_json::Value| value["type"] == "event-batch")
            .collect();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0]["events"].as_array().map(Vec::len), Some(2));
    });
}

// ---- DEV-541 ---------------------------------------------------------------------------------

/// REQ: DEV-541
#[test]
fn dev_541_replaying_an_unchanged_recording_matches() {
    with_clean_env(|| {
        let path = record_session("r541a", 0);
        assert_eq!(replay_on_fresh_host(&path), Ok(()));
    });
}

/// REQ: DEV-541, DEV-542
#[test]
fn dev_541_an_extra_recorded_host_call_diverges_at_its_line_number() {
    with_clean_env(|| {
        let path = record_session("r541b", 0);
        let mut lines = lines_of(&path);
        let expected_line = lines.len() as u32 + 1;
        lines.push(
            r#"{"type":"host-call","at":0,"import":"storage.get","key":"count","result":{"tag":"none"}}"#
                .to_string(),
        );
        std::fs::write(&path, lines.join("\n") + "\n").expect("rewrite");
        let mismatch = replay_on_fresh_host(&path).expect_err("the plugin makes no such call");
        assert_eq!(mismatch.line, expected_line);
        assert!(mismatch.expected.contains("storage.get"));
        assert_ne!(mismatch.expected, mismatch.actual);
    });
}

/// REQ: DEV-541
#[test]
fn dev_541_the_message_carries_the_code_and_the_line() {
    with_clean_env(|| {
        let path = record_session("r541c", 0);
        let mut lines = lines_of(&path);
        let line = lines.len() + 1;
        lines.push(r#"{"type":"host-call","at":0,"import":"storage.get","key":"k","result":{"tag":"none"}}"#.to_string());
        std::fs::write(&path, lines.join("\n") + "\n").expect("rewrite");
        let text = replay_on_fresh_host(&path)
            .expect_err("diverges")
            .to_string();
        assert!(text.starts_with("replay.diverged"), "message was {text}");
        assert!(text.contains(&format!("line {line}")), "message was {text}");
    });
}

/// REQ: DEV-541
#[test]
fn dev_541_the_first_difference_is_reported_not_a_later_one() {
    with_clean_env(|| {
        let path = record_session("r541d", 0);
        let mut lines = lines_of(&path);
        let first_bad = lines.len() as u32 + 1;
        for key in ["a", "b"] {
            lines.push(format!(
                r#"{{"type":"host-call","at":0,"import":"storage.get","key":"{key}","result":{{"tag":"none"}}}}"#
            ));
        }
        std::fs::write(&path, lines.join("\n") + "\n").expect("rewrite");
        assert_eq!(
            replay_on_fresh_host(&path).expect_err("diverges").line,
            first_bad
        );
    });
}

// ---- DEV-542 ---------------------------------------------------------------------------------

/// REQ: DEV-542
#[test]
fn dev_542_version_2_is_refused() {
    with_clean_env(|| {
        let path = record_session("r542a", 0);
        let text = std::fs::read_to_string(&path).expect("read").replacen(
            "\"version\":1",
            "\"version\":2",
            1,
        );
        std::fs::write(&path, text).expect("rewrite");
        let error = replay_on_fresh_host(&path).expect_err("version 2");
        let shown = format!("{error} {error:?}");
        assert!(
            shown.contains("replay.version-unsupported"),
            "error was {shown}"
        );
    });
}

/// REQ: DEV-542
#[test]
fn dev_542_version_0_is_refused() {
    with_clean_env(|| {
        let path = record_session("r542b", 0);
        let text = std::fs::read_to_string(&path).expect("read").replacen(
            "\"version\":1",
            "\"version\":0",
            1,
        );
        std::fs::write(&path, text).expect("rewrite");
        let error = replay_on_fresh_host(&path).expect_err("version 0");
        assert!(format!("{error} {error:?}").contains("replay.version-unsupported"));
    });
}

/// REQ: DEV-542
#[test]
fn dev_542_version_1_is_accepted() {
    with_clean_env(|| {
        let path = record_session("r542c", 0);
        let first_line = lines_of(&path).into_iter().next().expect("header line");
        assert!(
            first_line.contains("\"version\":1"),
            "header was {first_line}"
        );
        replay_on_fresh_host(&path).expect("version 1 replays with no mismatch");
    });
}

// ---- DEV-543 ---------------------------------------------------------------------------------

/// REQ: DEV-543
#[test]
fn dev_543_a_batch_delivered_at_start_has_time_zero() {
    with_clean_env(|| {
        let path = record_session("r543a", 0);
        let batch: serde_json::Value = lines_of(&path)
            .iter()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .find(|value| value["type"] == "event-batch")
            .expect("batch line");
        assert_eq!(batch["at"], 0);
    });
}

/// REQ: DEV-543
#[test]
fn dev_543_a_batch_after_1500_fake_ms_has_time_1500() {
    with_clean_env(|| {
        let path = record_session("r543b", 1_500);
        let batch: serde_json::Value = lines_of(&path)
            .iter()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .find(|value| value["type"] == "event-batch")
            .expect("batch line");
        assert_eq!(batch["at"], 1_500);
    });
}

/// REQ: DEV-543
#[test]
fn dev_543_the_header_start_is_the_zero_point() {
    with_clean_env(|| {
        let path = record_session("r543c", 250);
        let header: ReplayHeader = serde_json::from_str(&lines_of(&path)[0]).expect("header");
        assert_eq!(
            header.start, "2030-01-01 00:00:00",
            "the start does not move with the clock"
        );
    });
}

// ---- DEV-544 ---------------------------------------------------------------------------------

fn recorded_by_cli(label: &str) -> (PathBuf, PathBuf) {
    let dir = project(label, &[("a.json", r#"{ "schema": 1 }"#)]);
    let out = dir.join("run.replay.jsonl");
    let run = run_test(
        &dir,
        &[
            "--fixture",
            "tests/fixtures/a.json",
            "--record",
            out.to_str().expect("utf8"),
        ],
    );
    assert_eq!(run.code, 0, "stdout: {}", run.stdout);
    (dir, out)
}

/// REQ: DEV-544
#[test]
fn dev_544_replay_of_a_matching_file_exits_0() {
    let (dir, out) = recorded_by_cli("r544a");
    let run = run_test(&dir, &["--replay", out.to_str().expect("utf8")]);
    assert_eq!(run.code, 0, "stdout: {}", run.stdout);
}

/// REQ: DEV-544
#[test]
fn dev_544_replay_of_a_diverging_file_exits_3() {
    let (dir, out) = recorded_by_cli("r544b");
    let mut lines = lines_of(&out);
    lines.push(
        r#"{"type":"host-call","at":0,"import":"storage.get","key":"k","result":{"tag":"none"}}"#
            .to_string(),
    );
    std::fs::write(&out, lines.join("\n") + "\n").expect("rewrite");
    let run = run_test(&dir, &["--replay", out.to_str().expect("utf8")]);
    assert_eq!(run.code, 3, "stdout: {}", run.stdout);
    assert!(run.stdout.contains("replay.diverged") || run.stderr.contains("replay.diverged"));
}

/// REQ: DEV-544
#[test]
fn dev_544_replay_conflicts_with_fixture() {
    let (dir, out) = recorded_by_cli("r544c");
    let run = run_test(
        &dir,
        &[
            "--replay",
            out.to_str().expect("utf8"),
            "--fixture",
            "tests/fixtures/a.json",
        ],
    );
    assert_eq!(run.code, 2, "usage error, stdout: {}", run.stdout);
    // manual does not pin which stream carries the code
    let combined = format!("{}\n{}", run.stdout, run.stderr);
    assert!(combined.contains("usage.conflict"), "{combined}");
}
