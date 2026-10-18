//! `ilium broadcast` (F-110, F-036, COR-040): message intake, selection, delivery to the agents of
//! this session through `send-input` (`delivery: submitted`), delivery to other sessions through
//! `sessions.send`, output records and exit codes (src/docs/cli-reference.md "ilium broadcast").

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{AgentActivity, Delivery, ErrorCode, NodeId, Outcome, PluginError};
use proptest::prelude::*;
use serde_json::Value;

use crate::common::*;
use crate::feature_only::*;

/// A host whose own session `default` holds two agents (working, idle) and one shell.
/// Returns (host, working agent, idle agent, shell).
fn fleet() -> (FakeHost, u64, u64, u64) {
    let mut host = server_host();
    let working = add_agent(
        &mut host,
        "refactor parser",
        "claude",
        AgentActivity::Working,
    );
    let idle = add_agent(&mut host, "tests", "codex", AgentActivity::Idle);
    let shell = add_shell(&mut host, "shell");
    add_session(&mut host, "default", true, &[]);
    (host, working, idle, shell)
}

fn results(all: &[Value]) -> Vec<(u64, String)> {
    of_type(all, "result")
        .iter()
        .map(|record| {
            (
                record["pane_id"].as_u64().unwrap(),
                record["outcome"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

/// REQ: F-110
#[test]
fn broadcast_110_sends_the_message_to_every_agent_of_this_session_as_submitted_input() {
    let (mut host, working, idle, _) = fleet();
    let (_, _, exit) = run_cli(&mut host, &["broadcast", "hello"], b"");
    assert_eq!(exit, Some(0));
    let mut inputs = sent_inputs(&host);
    inputs.sort_by_key(|spec| spec.pane.0);
    assert_eq!(inputs.len(), 2);
    assert_eq!(inputs[0].pane, NodeId(working));
    assert_eq!(inputs[1].pane, NodeId(idle));
    for spec in &inputs {
        assert_eq!(spec.text, "hello");
        assert_eq!(spec.delivery, Delivery::Submitted);
        assert!(spec.journal);
        assert!(!spec.lock_user_input);
        assert!(spec.stages.is_empty() && spec.raw.is_none() && spec.mouse.is_none());
        assert_eq!(spec.prompt_epoch, None);
        assert_eq!(spec.if_current, None);
    }
}

/// REQ: F-110
#[test]
fn broadcast_110_never_types_into_a_shell_pane() {
    let (mut host, _, _, shell) = fleet();
    run_cli(&mut host, &["broadcast", "hello"], b"");
    assert!(!sent_panes(&host).contains(&NodeId(shell)));
}

/// REQ: F-110
#[test]
fn broadcast_110_the_dedup_key_is_broadcast_pane_and_message_length() {
    let (mut host, working, _, _) = fleet();
    run_cli(&mut host, &["broadcast", "hello"], b"");
    let keys: Vec<Option<String>> = host
        .submitted()
        .into_iter()
        .map(|submission| submission.dedup_key)
        .collect();
    assert!(
        keys.contains(&Some(format!("broadcast-{working}-5"))),
        "{keys:?}"
    );
}

/// REQ: F-110
#[test]
fn broadcast_110_message_words_are_joined_with_single_spaces() {
    let (mut host, _, _, _) = fleet();
    run_cli(
        &mut host,
        &["broadcast", "re-read", "CLAUDE.md", "now"],
        b"",
    );
    let texts: Vec<String> = sent_inputs(&host)
        .into_iter()
        .map(|spec| spec.text)
        .collect();
    assert_eq!(texts, vec![String::from("re-read CLAUDE.md now"); 2]);
}

/// REQ: F-110
#[test]
fn broadcast_110_a_lone_dash_reads_the_message_from_standard_input() {
    let (mut host, _, _, _) = fleet();
    run_cli(&mut host, &["broadcast", "-"], b"from stdin");
    assert!(!sent_inputs(&host).is_empty());
    let texts: Vec<String> = sent_inputs(&host)
        .into_iter()
        .map(|spec| spec.text)
        .collect();
    assert_eq!(texts, vec![String::from("from stdin"); 2]);
}

/// REQ: F-110
#[test]
fn broadcast_110_a_trailing_newline_is_removed() {
    let (mut host, _, _, _) = fleet();
    run_cli(&mut host, &["broadcast", "-"], b"notice\n");
    let texts: Vec<String> = sent_inputs(&host)
        .into_iter()
        .map(|spec| spec.text)
        .collect();
    assert_eq!(texts, vec![String::from("notice"); 2]);
}

/// REQ: F-110
#[test]
fn broadcast_110_only_one_trailing_newline_is_removed() {
    let (mut host, _, _, _) = fleet();
    run_cli(&mut host, &["broadcast", "-"], b"notice\n\n");
    let texts: Vec<String> = sent_inputs(&host)
        .into_iter()
        .map(|spec| spec.text)
        .collect();
    assert_eq!(texts, vec![String::from("notice\n"); 2]);
}

/// REQ: F-110
#[test]
fn broadcast_110_crlf_line_endings_are_normalised_to_lf() {
    let (mut host, _, _, _) = fleet();
    run_cli(&mut host, &["broadcast", "-"], b"one\r\ntwo\r\n");
    let texts: Vec<String> = sent_inputs(&host)
        .into_iter()
        .map(|spec| spec.text)
        .collect();
    assert_eq!(texts, vec![String::from("one\ntwo"); 2]);
}

/// REQ: F-110
#[test]
fn broadcast_110_a_multi_line_message_is_sent_as_one_bracketed_paste() {
    let (mut host, _, _, _) = fleet();
    run_cli(&mut host, &["broadcast", "-"], b"one\ntwo");
    let inputs = sent_inputs(&host);
    assert!(!inputs.is_empty());
    assert!(inputs
        .iter()
        .all(|spec| spec.delivery == Delivery::Pasted && spec.text == "one\ntwo"));
}

/// REQ: F-110
#[test]
fn broadcast_110_file_reads_the_message_from_a_file() {
    let (mut host, _, _, _) = fleet();
    host.fs().write("/work/notice.md", b"from a file\n");
    run_cli(&mut host, &["broadcast", "--file", "/work/notice.md"], b"");
    let texts: Vec<String> = sent_inputs(&host)
        .into_iter()
        .map(|spec| spec.text)
        .collect();
    assert_eq!(texts, vec![String::from("from a file"); 2]);
}

/// REQ: F-110
#[test]
fn broadcast_110_a_missing_message_is_an_invalid_request_with_exit_two() {
    let (mut host, _, _, _) = fleet();
    let (stdout, _, exit) = run_cli(&mut host, &["broadcast"], b"");
    assert_eq!(
        of_type(&records(&stdout), "error")[0]["code"],
        "invalid-request"
    );
    assert_eq!(exit, Some(2));
    assert!(sent_inputs(&host).is_empty());
}

/// REQ: F-110
#[test]
fn broadcast_110_an_empty_standard_input_message_is_an_invalid_request_with_exit_two() {
    let (mut host, _, _, _) = fleet();
    let (stdout, _, exit) = run_cli(&mut host, &["broadcast", "-"], b"\n");
    assert_eq!(
        of_type(&records(&stdout), "error")[0]["code"],
        "invalid-request"
    );
    assert_eq!(exit, Some(2));
}

/// REQ: F-110
#[test]
fn broadcast_110_a_message_starting_with_a_hyphen_follows_a_double_dash() {
    let (mut host, _, _, _) = fleet();
    run_cli(
        &mut host,
        &["broadcast", "--", "-v", "is", "not", "a", "flag"],
        b"",
    );
    let texts: Vec<String> = sent_inputs(&host)
        .into_iter()
        .map(|spec| spec.text)
        .collect();
    assert_eq!(texts, vec![String::from("-v is not a flag"); 2]);
}

/// REQ: F-110
#[test]
fn broadcast_110_output_has_progress_then_results_then_summary() {
    let (mut host, _, _, _) = fleet();
    let (stdout, _, _) = run_cli(&mut host, &["broadcast", "hello"], b"");
    let all = records(&stdout);
    assert_eq!(all[0]["type"], "progress");
    assert_eq!(all[0]["command"], "broadcast");
    assert_eq!(all[0]["stage"], "sending");
    assert_eq!(all[0]["message_bytes"], 5);
    assert_eq!(all.last().unwrap()["type"], "summary");
    assert_eq!(of_type(&all, "result").len(), 2);
}

/// REQ: F-110
#[test]
fn broadcast_110_the_progress_record_counts_message_bytes_not_characters() {
    let (mut host, _, _, _) = fleet();
    let (stdout, _, _) = run_cli(&mut host, &["broadcast", "h\u{e9}llo"], b"");
    assert_eq!(records(&stdout)[0]["message_bytes"], 6);
}

/// REQ: F-110
#[test]
fn broadcast_110_a_result_record_carries_the_documented_fields() {
    let (mut host, working, _, _) = fleet();
    let (stdout, _, _) = run_cli(&mut host, &["broadcast", "hello"], b"");
    let all = records(&stdout);
    let record = of_type(&all, "result")
        .into_iter()
        .find(|r| r["pane_id"] == working)
        .expect("result")
        .clone();
    assert_eq!(record["command"], "broadcast");
    assert_eq!(record["outcome"], "delivered");
    assert_eq!(record["session"], "default");
    assert_eq!(record["name"], "refactor parser");
    assert_eq!(record["kind"], "agent");
    assert_eq!(record["state"], "working");
    for field in ["agent", "project", "cwd", "is_self"] {
        assert!(record.get(field).is_some(), "{field} missing in {record}");
    }
}

/// REQ: F-110
#[test]
fn broadcast_110_the_summary_counts_recipients_and_deliveries() {
    let (mut host, _, _, _) = fleet();
    let (stdout, _, _) = run_cli(&mut host, &["broadcast", "hello"], b"");
    let summary = summary(&records(&stdout));
    assert_eq!(summary["command"], "broadcast");
    assert_eq!(summary["ok"], true);
    assert_eq!(summary["dry_run"], false);
    assert_eq!(summary["sessions"], 1);
    assert_eq!(summary["unreachable_sessions"], 0);
    assert_eq!(summary["recipients"], 2);
    assert_eq!(summary["delivered"], 2);
    assert_eq!(summary["queued"], 0);
    assert_eq!(summary["planned"], 0);
    assert_eq!(summary["skipped"], 0);
    assert_eq!(summary["failed"], 0);
}

/// REQ: F-110
#[test]
fn broadcast_110_exit_zero_when_at_least_one_agent_was_reached_and_nothing_failed() {
    let (mut host, _, _, _) = fleet();
    assert_eq!(run_cli(&mut host, &["broadcast", "hello"], b"").2, Some(0));
}

/// REQ: F-110
#[test]
fn broadcast_110_exit_one_when_nothing_was_selected() {
    let (mut host, _, _, _) = fleet();
    let (stdout, _, exit) = run_cli(
        &mut host,
        &["broadcast", "--match", "no-such-pane-anywhere", "hello"],
        b"",
    );
    assert_eq!(exit, Some(1));
    assert_eq!(summary(&records(&stdout))["recipients"], 0);
    assert!(sent_inputs(&host).is_empty());
}

/// REQ: F-110
#[test]
fn broadcast_110_exit_one_when_a_delivery_failed() {
    let (mut host, _, _, _) = fleet();
    auto_outcome(
        &mut host,
        Outcome::Failed(PluginError {
            code: ErrorCode::Io,
            message: "pane gone".to_string(),
            retry_after_ms: None,
        }),
    );
    let (stdout, _, exit) = run_cli(&mut host, &["broadcast", "hello"], b"");
    let all = records(&stdout);
    assert_eq!(exit, Some(1));
    assert_eq!(summary(&all)["failed"], 2);
    assert!(of_type(&all, "result")
        .iter()
        .all(|r| r["outcome"] == "failed" && r["reason"].is_string()));
}

/// REQ: F-110
#[test]
fn broadcast_110_exit_one_when_a_running_session_could_not_be_read() {
    let (mut host, _, _, _) = fleet();
    add_unreachable_session(&mut host, "stuck");
    let (stdout, _, exit) = run_cli(&mut host, &["broadcast", "hello"], b"");
    let all = records(&stdout);
    assert_eq!(exit, Some(1));
    assert_eq!(of_type(&all, "warning").len(), 1);
    assert_eq!(summary(&all)["unreachable_sessions"], 1);
    assert_eq!(summary(&all)["ok"], false);
}

/// REQ: F-110
#[test]
fn broadcast_110_the_pane_running_the_command_is_skipped_by_default() {
    let (mut host, working, idle, _) = fleet();
    let (stdout, _, _) = run_cli_in_pane(&mut host, working, &["broadcast", "hello"]);
    let all = records(&stdout);
    assert_eq!(sent_panes(&host), [NodeId(idle)]);
    let skipped: Vec<&Value> = of_type(&all, "result")
        .into_iter()
        .filter(|r| r["outcome"] == "skipped")
        .collect();
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0]["pane_id"], working);
    assert_eq!(skipped[0]["is_self"], true);
    assert!(skipped[0]["reason"].is_string());
    assert_eq!(summary(&all)["skipped"], 1);
}

/// REQ: F-110
#[test]
fn broadcast_110_include_self_also_sends_to_the_calling_pane() {
    let (mut host, working, _, _) = fleet();
    let (stdout, _, _) = run_cli_in_pane(
        &mut host,
        working,
        &["broadcast", "--include-self", "hello"],
    );
    assert!(sent_panes(&host).contains(&NodeId(working)));
    assert_eq!(summary(&records(&stdout))["skipped"], 0);
}

/// REQ: F-110
#[test]
fn broadcast_110_when_idle_sends_now_only_to_idle_agents() {
    let (mut host, working, idle, _) = fleet();
    let (stdout, _, _) = run_cli(&mut host, &["broadcast", "--when-idle", "hello"], b"");
    let all = records(&stdout);
    let mut outcomes = results(&all);
    outcomes.sort();
    let mut expected = vec![
        (working, "queued".to_string()),
        (idle, "delivered".to_string()),
    ];
    expected.sort();
    assert_eq!(outcomes, expected);
    assert_eq!(summary(&all)["queued"], 1);
    assert_eq!(summary(&all)["delivered"], 1);
    assert!(!sent_inputs(&host)
        .iter()
        .any(|spec| spec.pane == NodeId(working) && spec.delivery == Delivery::Submitted));
}

/// REQ: F-110
#[test]
fn broadcast_110_dry_run_plans_each_recipient_and_sends_nothing() {
    let (mut host, working, idle, _) = fleet();
    let (stdout, _, exit) = run_cli(&mut host, &["broadcast", "--dry-run", "hello"], b"");
    let all = records(&stdout);
    assert!(sent_inputs(&host).is_empty());
    assert_eq!(exit, Some(0));
    let mut outcomes = results(&all);
    outcomes.sort();
    let mut expected = vec![
        (working, "would-send".to_string()),
        (idle, "would-send".to_string()),
    ];
    expected.sort();
    assert_eq!(outcomes, expected);
    let summary = summary(&all);
    assert_eq!(summary["dry_run"], true);
    assert_eq!(summary["planned"], 2);
    assert_eq!(summary["delivered"], 0);
}

/// REQ: F-110
#[test]
fn broadcast_110_dry_run_with_when_idle_reports_would_queue_for_busy_agents() {
    let (mut host, working, idle, _) = fleet();
    let (stdout, _, _) = run_cli(
        &mut host,
        &["broadcast", "--dry-run", "--when-idle", "hello"],
        b"",
    );
    let by_pane: std::collections::BTreeMap<u64, String> =
        results(&records(&stdout)).into_iter().collect();
    assert_eq!(by_pane[&working], "would-queue");
    assert_eq!(by_pane[&idle], "would-send");
}

/// REQ: F-110
#[test]
fn broadcast_110_selection_by_agent_narrows_the_recipients() {
    let (mut host, _, idle, _) = fleet();
    run_cli(&mut host, &["broadcast", "--agent", "codex", "hello"], b"");
    assert_eq!(sent_panes(&host), [NodeId(idle)]);
}

/// REQ: F-110
#[test]
fn broadcast_110_selection_by_state_narrows_the_recipients() {
    let (mut host, working, _, _) = fleet();
    run_cli(
        &mut host,
        &["broadcast", "--state", "working", "hello"],
        b"",
    );
    assert_eq!(sent_panes(&host), [NodeId(working)]);
}

/// REQ: F-110
#[test]
fn broadcast_110_a_shell_is_never_a_recipient_whatever_the_selection() {
    let (mut host, _, _, shell) = fleet();
    run_cli(&mut host, &["broadcast", "--kind", "shell", "hello"], b"");
    assert!(!sent_panes(&host).contains(&NodeId(shell)));
}

/// REQ: F-110
#[test]
fn broadcast_110_an_invalid_selection_is_an_invalid_request_with_exit_two() {
    let (mut host, _, _, _) = fleet();
    let (stdout, _, exit) = run_cli(&mut host, &["broadcast", "--regex", "(", "hello"], b"");
    assert_eq!(
        of_type(&records(&stdout), "error")[0]["code"],
        "invalid-request"
    );
    assert_eq!(exit, Some(2));
    assert!(sent_inputs(&host).is_empty());
}

/// REQ: F-110
#[test]
fn broadcast_110_invert_without_a_pattern_is_an_invalid_request() {
    let (mut host, _, _, _) = fleet();
    let (stdout, _, exit) = run_cli(&mut host, &["broadcast", "--invert", "hello"], b"");
    assert_eq!(
        of_type(&records(&stdout), "error")[0]["code"],
        "invalid-request"
    );
    assert_eq!(exit, Some(2));
}

/// REQ: F-110
#[test]
fn broadcast_110_other_sessions_get_a_broadcast_request_with_the_text() {
    let (mut host, _, _, _) = fleet();
    add_session(&mut host, "review", false, &[]);
    run_cli(&mut host, &["broadcast", "hello"], b"");
    let requests = host.sessions().requests();
    let sent: Vec<&(String, String, String)> = requests
        .iter()
        .filter(|(session, _, _)| session == "review")
        .collect();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].1, "broadcast");
    let arguments: Value = serde_json::from_str(&sent[0].2).expect("arguments are JSON");
    assert_eq!(arguments["text"], "hello");
}

/// REQ: F-110
#[test]
fn broadcast_110_the_own_session_is_not_asked_through_sessions_send() {
    let (mut host, _, _, _) = fleet();
    add_session(&mut host, "review", false, &[]);
    run_cli(&mut host, &["broadcast", "hello"], b"");
    assert!(host
        .sessions()
        .requests()
        .iter()
        .all(|(session, _, _)| session != "default"));
}

/// REQ: F-110
#[test]
fn broadcast_110_the_service_name_is_provided_for_other_plugins() {
    // service:broadcast activates the plugin on demand (activation trigger); the service name is on the page.
    assert!(page_text().contains("`ilium.core.fleet/broadcast`: send"));
}

/// REQ: F-110
#[test]
fn broadcast_110_a_before_broadcast_replace_changes_the_text_that_is_sent() {
    let (mut host, _, _, _) = fleet();
    host.set_hook_reply(
        "ilium.core.fleet/before-broadcast",
        ilium_plugin_sdk::exports::HookReply::Replace(
            serde_json::json!({"message": "rewritten", "targets": [], "source": "cli"}).to_string(),
        ),
    );
    run_cli(&mut host, &["broadcast", "original"], b"");
    assert!(!sent_inputs(&host).is_empty());
    let texts: Vec<String> = sent_inputs(&host)
        .into_iter()
        .map(|spec| spec.text)
        .collect();
    assert_eq!(texts, vec![String::from("rewritten"); 2]);
}

/// REQ: F-110
#[test]
fn broadcast_110_the_before_broadcast_payload_has_message_targets_and_source() {
    let (mut host, _, _, _) = fleet();
    host.set_hook_reply(
        "ilium.core.fleet/before-broadcast",
        ilium_plugin_sdk::exports::HookReply::Pass,
    );
    run_cli(&mut host, &["broadcast", "original"], b"");
    let calls = host.hook_payloads("ilium.core.fleet/before-broadcast");
    assert_eq!(calls.len(), 1);
    let payload: Value = serde_json::from_str(&calls[0]).expect("payload is JSON");
    assert_eq!(payload["message"], "original");
    // manual does not pin the targets encoding or the source value
    assert_eq!(payload["targets"].as_array().map(Vec::len), Some(2));
    assert!(!payload["source"]
        .as_str()
        .expect("source is a string")
        .is_empty());
}

/// REQ: F-110
#[test]
fn broadcast_110_a_pass_leaves_the_text_unchanged() {
    let (mut host, _, _, _) = fleet();
    host.set_hook_reply(
        "ilium.core.fleet/before-broadcast",
        ilium_plugin_sdk::exports::HookReply::Pass,
    );
    run_cli(&mut host, &["broadcast", "original"], b"");
    let texts: Vec<String> = sent_inputs(&host)
        .into_iter()
        .map(|spec| spec.text)
        .collect();
    assert_eq!(texts, vec![String::from("original"); 2]);
}

/// REQ: F-110
#[test]
fn broadcast_110_a_before_broadcast_replace_with_an_empty_message_ends_the_broadcast() {
    let (mut host, _, _, _) = fleet();
    host.set_hook_reply(
        "ilium.core.fleet/before-broadcast",
        ilium_plugin_sdk::exports::HookReply::Replace(
            serde_json::json!({"message": "", "targets": [], "source": "cli"}).to_string(),
        ),
    );
    run_cli(&mut host, &["broadcast", "original"], b"");
    assert!(sent_inputs(&host).is_empty());
}

/// REQ: F-110
#[test]
fn broadcast_110_the_broadcast_sent_topic_carries_targets_and_length() {
    let (mut host, _, _, _) = fleet();
    run_cli(&mut host, &["broadcast", "hello"], b"");
    let topics = host.published_topics();
    let sent: Vec<&(String, String)> = topics
        .iter()
        .filter(|(topic, _)| topic == "ilium.core.fleet/broadcast-sent")
        .collect();
    assert_eq!(sent.len(), 1);
    let payload: Value = serde_json::from_str(&sent[0].1).expect("payload is JSON");
    assert_eq!(payload["length"], 5);
    // manual does not pin the targets encoding (count or list); it must cover both agents
    let covered = match &payload["targets"] {
        Value::Number(count) => count.as_u64(),
        Value::Array(list) => Some(list.len() as u64),
        _ => None,
    };
    assert_eq!(covered, Some(2), "{payload}");
}

/// REQ: F-110
#[test]
fn broadcast_110_no_topic_is_published_when_nothing_was_sent() {
    let (mut host, _, _, _) = fleet();
    run_cli(&mut host, &["broadcast", "--dry-run", "hello"], b"");
    assert!(host
        .published_topics()
        .iter()
        .all(|(topic, _)| topic != "ilium.core.fleet/broadcast-sent"));
}

proptest! {
    // REQ: F-110
    #[test]
    fn broadcast_110_any_single_line_message_is_delivered_verbatim(message in "[a-zA-Z0-9 ,.!?_-]{1,40}") {
        prop_assume!(!message.trim().is_empty() && !message.starts_with('-') && !message.ends_with(' '));
        let (mut host, _, _, _) = fleet();
        run_cli(&mut host, &["broadcast", "--", &message], b"");
        let inputs = sent_inputs(&host);
        prop_assert!(!inputs.is_empty());
        // cli-reference: several words are joined with single spaces; one argument is verbatim
        prop_assert_eq!(inputs.len(), 2);
        prop_assert!(inputs.iter().all(|spec| spec.text == message));
    }
}

/// REQ: F-110
#[test]
fn broadcast_110_the_dedup_key_length_counts_bytes() {
    let (mut host, working, _, _) = fleet();
    run_cli(&mut host, &["broadcast", "h\u{e9}llo"], b"");
    let keys: Vec<Option<String>> = host.submitted().into_iter().map(|s| s.dedup_key).collect();
    assert!(
        keys.contains(&Some(format!("broadcast-{working}-6"))),
        "{keys:?}"
    );
}
