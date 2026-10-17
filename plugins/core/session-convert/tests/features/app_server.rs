//! F-077: Claude Code to Codex uses Codex's own importer through a private `codex app-server`, a long-lived
//! child talked to by lines. The child ends when the conversion ends, whatever the result
//! (page sample "Claude Code to Codex with a private app-server").

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    ChildExitedEvent, ChildId, ChildOutputEvent, Event, FsPath, Intent, Outcome, OutputStream,
    RequestRef, RequestResult, Submission,
};
use serde_json::{json, Value};

use crate::common::*;
use crate::start::{server_with_pane, start_claude};

const THREAD: &str = "33333333-3333-4333-8333-333333333333";

/// A Claude pane whose agent has stopped: the app-server child is running.
pub fn stopped_claude() -> (FakeHost, u64) {
    let (mut host, pane) = server_with_pane("claude", true);
    let submissions = start_claude(&mut host, pane);
    let terminate = submissions
        .iter()
        .find(|s| matches!(s.intent, Intent::TerminatePaneProcess(_)))
        .expect("terminate submitted");
    host.deliver_one(Event::RequestCompleted(RequestResult {
        request: RequestRef::Submission(terminate.correlation),
        outcome: Outcome::Ok,
    }));
    host.settle();
    (host, pane)
}

/// A Claude to Codex conversion that failed because the app-server died.
pub fn failed_claude_to_codex() -> (FakeHost, u64) {
    let (mut host, pane) = stopped_claude();
    let child = host.process().children()[0].id;
    host.deliver_one(Event::ChildExited(ChildExitedEvent {
        child: ChildId(child),
        status: Some(1),
        signal: None,
    }));
    (host, pane)
}

fn say(host: &mut FakeHost, child: u64, message: Value) -> Vec<Submission> {
    let mut line = message.to_string();
    line.push('\n');
    host.deliver_one(Event::ChildOutput(ChildOutputEvent {
        child: ChildId(child),
        stream: OutputStream::Stdout,
        bytes: line.into_bytes(),
    }))
}

fn stdin_methods(host: &mut FakeHost) -> Vec<String> {
    host.process().children()[0]
        .stdin_lines
        .iter()
        .map(|line| {
            serde_json::from_str::<Value>(line).expect("JSON line")["method"]
                .as_str()
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

/// Drives the handshake and the import to the completion notification; returns the submissions of the last step.
fn complete_import(host: &mut FakeHost) -> Vec<Submission> {
    let child = host.process().children()[0].id;
    say(
        host,
        child,
        json!({"id": 1, "result": {"userAgent": "codex 1.0", "codexHome": "/home/u/.codex"}}),
    );
    say(host, child, json!({"id": 2, "result": {"importId": "i1"}}));
    host.fs().write(
        FsPath {
            scope: "agent-sessions:codex".into(),
            path: format!("2030/01/01/rollout-{THREAD}.jsonl"),
        },
        b"{\"type\":\"session_meta\"}\n",
    );
    say(
        host,
        child,
        json!({"method": "externalAgentConfig/import/completed", "params": {"itemTypeResults": [
            {"itemType": "SESSIONS", "successes": [{"source": format!("projects/p/{SOURCE_SESSION}.jsonl"), "target": THREAD}], "failures": []}
        ]}}),
    );
    host.settle();
    host.submitted()
}

/// REQ: F-077
#[test]
fn f_077_no_app_server_starts_before_the_agent_stopped() {
    let (mut host, pane) = server_with_pane("claude", true);
    start_claude(&mut host, pane);
    assert!(host.process().children().is_empty());
}

/// REQ: F-077
#[test]
fn f_077_the_app_server_is_a_codex_app_server_child_with_open_stdin_in_line_mode() {
    let (mut host, _pane) = stopped_claude();
    let children = host.process().children();
    assert_eq!(children.len(), 1, "{children:?}");
    assert_eq!(children[0].program, "codex");
    assert_eq!(children[0].arguments, ["app-server"]);
    assert!(children[0].stdin_open);
    assert!(children[0].line_mode);
}

/// REQ: F-077
#[test]
fn f_077_the_first_line_sent_to_the_app_server_is_the_initialize_request() {
    let (mut host, _pane) = stopped_claude();
    let first: Value =
        serde_json::from_str(&host.process().children()[0].stdin_lines[0]).expect("JSON line");
    assert_eq!(first["method"], "initialize");
    assert_eq!(first["params"]["capabilities"]["experimentalApi"], true);
}

/// REQ: F-077
#[test]
fn f_077_after_the_initialize_answer_the_plugin_sends_initialized_then_the_import() {
    let (mut host, _pane) = stopped_claude();
    let child = host.process().children()[0].id;
    say(
        &mut host,
        child,
        json!({"id": 1, "result": {"userAgent": "codex 1.0", "codexHome": "/home/u/.codex"}}),
    );
    assert_eq!(
        stdin_methods(&mut host),
        ["initialize", "initialized", "externalAgentConfig/import"]
    );
}

/// REQ: F-077
#[test]
fn f_077_the_import_request_names_the_claude_transcript_as_one_sessions_item() {
    let (mut host, _pane) = stopped_claude();
    let child = host.process().children()[0].id;
    say(
        &mut host,
        child,
        json!({"id": 1, "result": {"userAgent": "codex 1.0", "codexHome": "/home/u/.codex"}}),
    );
    let lines = host.process().children()[0].stdin_lines.clone();
    let import: Value = serde_json::from_str(lines.last().unwrap()).expect("JSON line");
    let item = &import["params"]["migrationItems"][0];
    assert_eq!(item["itemType"], "SESSIONS");
    assert_eq!(item["details"]["sessions"].as_array().unwrap().len(), 1);
    assert!(item["details"]["sessions"][0]["path"]
        .as_str()
        .unwrap()
        .ends_with(&format!("{SOURCE_SESSION}.jsonl")));
}

/// REQ: F-077
#[test]
fn f_077_an_import_progress_notification_moves_the_progress_forward() {
    let (mut host, _pane) = stopped_claude();
    let child = host.process().children()[0].id;
    say(
        &mut host,
        child,
        json!({"id": 1, "result": {"userAgent": "codex 1.0", "codexHome": "/home/u/.codex"}}),
    );
    say(
        &mut host,
        child,
        json!({"id": 2, "result": {"importId": "i1"}}),
    );
    let submissions = say(
        &mut host,
        child,
        json!({"method": "externalAgentConfig/import/progress", "params": {"itemTypeResults": [
            {"itemType": "SESSIONS", "successes": [{"source": "x", "target": "y"}], "failures": []}
        ]}}),
    );
    let progress = messages(&submissions)
        .into_iter()
        .filter(|m| m.2 == "progress")
        .count();
    assert!(progress >= 1, "{submissions:?}");
}

/// REQ: F-077
#[test]
fn f_077_a_completed_import_creates_a_codex_pane_that_resumes_the_new_thread() {
    let (mut host, _pane) = stopped_claude();
    let submissions = complete_import(&mut host);
    let panes = created(&submissions);
    assert_eq!(panes.len(), 1, "{panes:?}");
    assert_eq!(panes[0].0, "codex (converted)");
    assert_eq!(panes[0].1, "ilium.terminal");
    assert_eq!(panes[0].2, ["codex", "resume", THREAD]);
}

/// REQ: F-077
#[test]
fn f_077_a_completed_import_closes_the_frozen_claude_pane() {
    let (mut host, pane) = stopped_claude();
    let submissions = complete_import(&mut host);
    assert_eq!(closed(&submissions), [pane]);
}

/// REQ: F-077
#[test]
fn f_077_the_app_server_is_stopped_with_terminate_when_the_conversion_succeeds() {
    let (mut host, _pane) = stopped_claude();
    complete_import(&mut host);
    assert_eq!(host.process().children()[0].signals, ["terminate"]);
}

/// REQ: F-077
#[test]
fn f_077_the_app_server_is_stopped_when_the_conversion_fails() {
    let (mut host, _pane) = stopped_claude();
    let child = host.process().children()[0].id;
    say(
        &mut host,
        child,
        json!({"id": 1, "error": {"code": -1, "message": "no"}}),
    );
    assert_eq!(host.process().children()[0].signals, ["terminate"]);
}

/// REQ: F-077
#[test]
fn f_077_the_app_server_is_stopped_when_the_user_cancels() {
    let (mut host, pane) = stopped_claude();
    host.deliver_one(message_from(
        ilium_plugin_sdk::types::PluginSide::Client,
        "cancel",
        json!({"pane": pane}).to_string().as_bytes(),
    ));
    assert_eq!(host.process().children()[0].signals, ["terminate"]);
}

/// REQ: F-077
#[test]
fn f_077_an_import_with_a_failure_fails_the_conversion_and_offers_the_original() {
    let (mut host, _pane) = stopped_claude();
    let child = host.process().children()[0].id;
    say(
        &mut host,
        child,
        json!({"id": 1, "result": {"userAgent": "codex 1.0", "codexHome": "/home/u/.codex"}}),
    );
    say(
        &mut host,
        child,
        json!({"id": 2, "result": {"importId": "i1"}}),
    );
    let submissions = say(
        &mut host,
        child,
        json!({"method": "externalAgentConfig/import/completed", "params": {"itemTypeResults": [
            {"itemType": "SESSIONS", "successes": [], "failures": [{"failureStage": "parse", "message": "bad line"}]}
        ]}}),
    );
    assert!(created(&submissions).is_empty());
    let failed = messages(&submissions)
        .into_iter()
        .find(|m| m.2 == "failed")
        .expect("failed message");
    let payload: Value = serde_json::from_str(&failed.3).unwrap();
    assert_eq!(payload["can_resume"], true);
    assert!(payload["message"].as_str().unwrap().contains("bad line"));
}

/// REQ: F-077
#[test]
fn f_077_an_app_server_that_exits_early_fails_the_conversion() {
    let (mut host, pane) = stopped_claude();
    let child = host.process().children()[0].id;
    let submissions = host.deliver_one(Event::ChildExited(ChildExitedEvent {
        child: ChildId(child),
        status: Some(1),
        signal: None,
    }));
    assert!(created(&submissions).is_empty());
    assert!(messages(&submissions).iter().any(|m| m.2 == "failed"));
    assert!(closed(&submissions).iter().all(|node| *node != pane));
}

/// REQ: F-077
#[test]
fn f_077_the_frozen_claude_pane_stays_open_after_a_failure() {
    let (mut host, pane) = failed_claude_to_codex();
    assert!(closed(&host.submitted()).iter().all(|node| *node != pane));
}

/// REQ: F-077
#[test]
fn f_077_a_noise_line_from_the_app_server_is_ignored() {
    let (mut host, _pane) = stopped_claude();
    let child = host.process().children()[0].id;
    let submissions = host.deliver_one(Event::ChildOutput(ChildOutputEvent {
        child: ChildId(child),
        stream: OutputStream::Stdout,
        bytes: b"not json at all\n".to_vec(),
    }));
    assert!(created(&submissions).is_empty());
    assert!(host.failures().is_empty());
}

/// REQ: F-077
#[test]
fn f_077_the_original_claude_transcript_is_left_untouched() {
    let (mut host, _pane) = stopped_claude();
    complete_import(&mut host);
    let bytes = host
        .fs()
        .read(FsPath {
            scope: "agent-sessions:claude".into(),
            path: format!("projects/p/{SOURCE_SESSION}.jsonl"),
        })
        .expect("transcript");
    assert_eq!(bytes, b"{\"type\":\"user\"}\n");
}
