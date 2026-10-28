//! Twins of `ilium/src/broadcast.rs`, driven through `ilium broadcast`
//! (src/docs/cli-reference.md, "ilium broadcast").

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::AgentActivity;
use serde_json::Value;

use crate::common::*;

/// Own session `default` with a working agent, an idle agent and a shell.
fn fleet() -> (FakeHost, u64, u64) {
    let mut host = server_host();
    let working = add_agent(
        &mut host,
        "refactor parser",
        "claude",
        AgentActivity::Working,
    );
    let idle = add_agent(&mut host, "tests", "codex", AgentActivity::Idle);
    add_shell(&mut host, "shell");
    add_session(&mut host, "default", true, &[]);
    (host, working, idle)
}

fn outcomes(all: &[Value]) -> Vec<(u64, String)> {
    let mut found: Vec<(u64, String)> = of_type(all, "result")
        .iter()
        .map(|r| {
            (
                r["pane_id"].as_u64().unwrap(),
                r["outcome"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    found.sort();
    found
}

/// REQ: PORT:ilium/src/broadcast.rs::message_words_join_and_stdin_and_line_endings_normalise
#[test]
fn broadcast_message_words_join_and_stdin_and_line_endings_normalise() {
    let (mut host, _, _) = fleet();
    run_cli(
        &mut host,
        &["broadcast", "please", "re-read", "the", "rules"],
        b"",
    );
    assert!(sent_inputs(&host)
        .iter()
        .all(|s| s.text == "please re-read the rules"));
    let (mut host, _, _) = fleet();
    run_cli(&mut host, &["broadcast", "-"], b"line one\r\nline two");
    let inputs = sent_inputs(&host);
    assert!(!inputs.is_empty());
    assert!(inputs.iter().all(|s| s.text == "line one\nline two"));
    let (mut host, _, _) = fleet();
    let (stdout, _, exit) = run_cli(&mut host, &["broadcast", "-"], b" \t\n");
    assert_eq!(exit, Some(2));
    assert_eq!(
        of_type(&records(&stdout), "error")[0]["code"],
        "invalid-request"
    );
}

/// REQ: PORT:ilium/src/broadcast.rs::a_message_or_file_is_required_and_selection_flags_parse
#[test]
fn broadcast_a_message_or_file_is_required_and_selection_flags_parse() {
    let (mut host, _, _) = fleet();
    let (stdout, _, exit) = run_cli(&mut host, &["broadcast"], b"");
    assert_eq!(exit, Some(2));
    assert_eq!(
        of_type(&records(&stdout), "error")[0]["code"],
        "invalid-request"
    );
    let (_, _, exit) = run_cli(
        &mut host,
        &[
            "broadcast",
            "--project",
            "ilium,/w/lumen",
            "--regex",
            "review|audit",
            "--invert",
            "--agent",
            "codex",
            "--state",
            "idle,working",
            "--when-idle",
            "hello",
        ],
        b"",
    );
    assert_ne!(exit, Some(2), "the documented selection flags must parse");
}

/// REQ: PORT:ilium/src/broadcast.rs::when_idle_queues_only_for_busy_agents
#[test]
fn broadcast_when_idle_queues_only_for_busy_agents() {
    let (mut host, working, idle) = fleet();
    let (stdout, _, _) = run_cli(&mut host, &["broadcast", "--when-idle", "hello"], b"");
    let mut expected = vec![
        (working, "queued".to_string()),
        (idle, "delivered".to_string()),
    ];
    expected.sort();
    assert_eq!(outcomes(&records(&stdout)), expected);
}

/// REQ: PORT:ilium/src/broadcast.rs::tally_counts_skips_apart_from_recipients
#[test]
fn broadcast_tally_counts_skips_apart_from_recipients() {
    let (mut host, working, _) = fleet();
    let (stdout, _, _) =
        run_cli_in_pane(&mut host, working, &["broadcast", "--when-idle", "hello"]);
    let totals = summary(&records(&stdout));
    assert_eq!(totals["skipped"], 1);
    assert_eq!(totals["recipients"], 1);
    assert_eq!(totals["delivered"], 1);
    assert_eq!(totals["queued"], 0);
    assert_eq!(totals["failed"], 0);
}

/// REQ: PORT:ilium/src/broadcast.rs::message_words_join_and_stdin_and_line_endings_normalise
#[test]
fn broadcast_stdin_trailing_newlines_are_dropped_but_inner_blank_lines_kept() {
    let (mut host, _, _) = fleet();
    run_cli(&mut host, &["broadcast", "-"], b"first\n\nsecond\n\n\n");
    let inputs = sent_inputs(&host);
    assert!(!inputs.is_empty());
    assert!(inputs.iter().all(|s| s.text == "first\n\nsecond"));
}

/// REQ: PORT:ilium/src/broadcast.rs::a_message_or_file_is_required_and_selection_flags_parse
#[test]
fn broadcast_dry_run_with_selection_flags_sends_nothing() {
    let (mut host, _, _) = fleet();
    let (_, _, exit) = run_cli(
        &mut host,
        &[
            "broadcast",
            "--dry-run",
            "--agent",
            "codex",
            "--state",
            "idle,working",
            "hello",
        ],
        b"",
    );
    assert_ne!(exit, Some(2), "the selection flags must parse");
    assert!(sent_inputs(&host).is_empty());
}

/// REQ: PORT:ilium/src/broadcast.rs::when_idle_queues_only_for_busy_agents
#[test]
fn broadcast_when_idle_never_sends_immediately_to_the_busy_agent() {
    let (mut host, working, _) = fleet();
    run_cli(&mut host, &["broadcast", "--when-idle", "hello"], b"");
    assert!(sent_panes(&host).iter().all(|pane| pane.0 != working));
}

/// REQ: PORT:ilium/src/broadcast.rs::tally_counts_skips_apart_from_recipients
#[test]
fn broadcast_tally_recipients_exclude_only_skipped_results() {
    let (mut host, _, _) = fleet();
    let (stdout, _, _) = run_cli(&mut host, &["broadcast", "hello"], b"");
    let all = records(&stdout);
    let results = of_type(&all, "result");
    let non_skipped = results
        .iter()
        .filter(|r| r["outcome"].as_str() != Some("skipped"))
        .count();
    let totals = summary(&all);
    assert_eq!(totals["recipients"].as_u64(), Some(non_skipped as u64));
}
