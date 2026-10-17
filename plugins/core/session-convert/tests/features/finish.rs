//! F-077: how a conversion ends. On success the server creates a new pane that resumes the converted session
//! and closes the frozen one; after a failure that came after the agent stopped, Resume original replaces the pane
//! command and keeps the scrollback, Close unfreezes (page sample "Replace the frozen pane").

use ilium_plugin_sdk::host::Bucket;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ErrorCode, Event, Intent, PluginError, PluginSide, Submission};
use serde_json::{json, Value};

use crate::common::*;
use crate::start::{server_with_pane, start_claude};
use crate::write_verify::{finish_job, job_output, run_until_job};

fn converted_codex_to_claude() -> (FakeHost, u64, Vec<Submission>) {
    let (mut host, pane) = server_with_pane("codex", true);
    let request = run_until_job(&mut host, pane);
    let submissions = finish_job(&mut host, request, Ok(job_output(3, 3)));
    (host, pane, submissions)
}

fn failed_codex_to_claude() -> (FakeHost, u64) {
    let (mut host, pane) = server_with_pane("codex", true);
    let request = run_until_job(&mut host, pane);
    let error = PluginError {
        code: ErrorCode::Io,
        message: "rollout unreadable".into(),
        retry_after_ms: None,
    };
    finish_job(&mut host, request, Err(error));
    (host, pane)
}

fn client_message(channel: &str, pane: u64) -> Event {
    message_from(
        PluginSide::Client,
        channel,
        json!({"pane": pane}).to_string().as_bytes(),
    )
}

fn is_uuid(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    parts.len() == 5
        && parts.iter().map(|p| p.len()).collect::<Vec<_>>() == [8, 4, 4, 4, 12]
        && text.chars().all(|c| c == '-' || c.is_ascii_hexdigit())
}

fn state_present(host: &FakeHost, pane: u64) -> bool {
    host.storage()
        .get(Bucket::Session, &format!("convert/{pane}"))
        .is_some()
}

/// REQ: F-077
#[test]
fn f_077_success_creates_one_pane_titled_claude_converted_with_the_terminal_kind() {
    let (_host, _pane, submissions) = converted_codex_to_claude();
    let panes = created(&submissions);
    assert_eq!(panes.len(), 1, "{panes:?}");
    assert_eq!(panes[0].0, "claude (converted)");
    assert_eq!(panes[0].1, "ilium.terminal");
}

/// REQ: F-077
#[test]
fn f_077_success_resumes_the_converted_session_with_the_claude_resume_command() {
    let (_host, _pane, submissions) = converted_codex_to_claude();
    let command = created(&submissions).remove(0).2;
    assert_eq!(command.len(), 3, "{command:?}");
    assert_eq!(&command[..2], ["claude", "--resume"]);
    assert!(is_uuid(&command[2]), "{}", command[2]);
}

/// REQ: F-077
#[test]
fn f_077_success_closes_the_frozen_pane_after_creating_the_new_one() {
    let (_host, pane, submissions) = converted_codex_to_claude();
    assert_eq!(closed(&submissions), [pane]);
    let order: Vec<&str> = submissions
        .iter()
        .filter_map(|s| match &s.intent {
            Intent::CreatePane(_) => Some("create"),
            Intent::CloseNode(_) => Some("close"),
            _ => None,
        })
        .collect();
    assert_eq!(order, ["create", "close"]);
}

/// REQ: F-077
#[test]
fn f_077_success_removes_the_state_so_the_watchdog_has_nothing_to_undo() {
    let (mut host, pane, _) = converted_codex_to_claude();
    assert!(!state_present(&host, pane));
    host.advance_clock(WATCHDOG_MILLIS);
    assert!(unfrozen(&host.submitted()).is_empty());
}

/// REQ: F-077
#[test]
fn f_077_success_never_unfreezes_the_old_pane() {
    let (_host, _pane, submissions) = converted_codex_to_claude();
    assert!(unfrozen(&submissions).is_empty());
}

/// REQ: F-077
#[test]
fn f_077_success_tells_the_client_the_conversion_ended_well() {
    let (_host, pane, submissions) = converted_codex_to_claude();
    let finished = messages(&submissions)
        .into_iter()
        .filter(|m| m.1 == PluginSide::Client && m.2 == "finished")
        .map(|m| serde_json::from_str::<Value>(&m.3).unwrap())
        .last()
        .expect("finished message");
    assert_eq!(finished, json!({"pane": pane, "ok": true}));
}

/// REQ: F-077
#[test]
fn f_077_a_new_conversion_may_start_after_a_success() {
    let (mut host, pane, _) = converted_codex_to_claude();
    let second = add_agent_pane(&mut host, "second", "claude");
    host.agents().set_session(
        second,
        Some(ilium_plugin_sdk::primitives::SessionRef {
            scope: "agent-sessions:claude".into(),
            path: "projects/p/second.jsonl".into(),
            session_id: "second".into(),
        }),
    );
    host.fs().write(
        ilium_plugin_sdk::types::FsPath {
            scope: "agent-sessions:claude".into(),
            path: "projects/p/second.jsonl".into(),
        },
        b"{}\n",
    );
    assert_ne!(pane, second);
    assert_eq!(frozen(&start_claude(&mut host, second)), [second]);
}

/// REQ: F-077
#[test]
fn f_077_resume_original_replaces_the_command_of_a_codex_pane_and_keeps_the_scrollback() {
    let (mut host, pane) = failed_codex_to_claude();
    let submissions = host.deliver_one(client_message("resume", pane));
    assert_eq!(
        replaced(&submissions),
        [(
            pane,
            vec![
                "codex".to_string(),
                "resume".to_string(),
                SOURCE_SESSION.to_string()
            ],
            true
        )]
    );
}

/// REQ: F-077
#[test]
fn f_077_resume_original_uses_the_claude_grammar_for_a_claude_pane() {
    let (mut host, pane) = crate::app_server::failed_claude_to_codex();
    let submissions = host.deliver_one(client_message("resume", pane));
    assert_eq!(
        replaced(&submissions),
        [(
            pane,
            vec![
                "claude".to_string(),
                "--resume".to_string(),
                SOURCE_SESSION.to_string()
            ],
            true
        )]
    );
}

/// REQ: F-077
#[test]
fn f_077_resume_original_removes_the_state() {
    let (mut host, pane) = failed_codex_to_claude();
    host.deliver_one(client_message("resume", pane));
    assert!(!state_present(&host, pane));
}

/// REQ: F-077
#[test]
fn f_077_resume_original_twice_replaces_the_command_once() {
    let (mut host, pane) = failed_codex_to_claude();
    host.deliver_one(client_message("resume", pane));
    let second = host.deliver_one(client_message("resume", pane));
    assert!(replaced(&second).is_empty());
}

/// REQ: F-077
#[test]
fn f_077_resume_without_a_conversion_does_nothing() {
    let (mut host, pane) = server_with_pane("codex", true);
    assert!(host.deliver_one(client_message("resume", pane)).is_empty());
}

/// REQ: F-077
#[test]
fn f_077_resume_is_refused_while_the_agent_still_runs() {
    let (mut host, pane) = server_with_pane("codex", true);
    host.deliver_one(crate::start::start_message(pane, "claude"));
    let submissions = host.deliver_one(client_message("resume", pane));
    assert!(replaced(&submissions).is_empty());
}

/// REQ: F-077
#[test]
fn f_077_close_after_a_failure_unfreezes_the_pane_and_removes_the_state() {
    let (mut host, pane) = failed_codex_to_claude();
    let submissions = host.deliver_one(client_message("close", pane));
    assert_eq!(unfrozen(&submissions), [pane]);
    assert!(!state_present(&host, pane));
}

/// REQ: F-077
#[test]
fn f_077_close_is_the_user_answer_for_a_failure_before_the_agent_stopped() {
    let (mut host, pane) = server_with_pane("codex", true);
    host.deliver_one(crate::start::start_message(pane, "claude"));
    let submissions = host.deliver_one(client_message("close", pane));
    assert_eq!(unfrozen(&submissions), [pane]);
}

/// REQ: F-077
#[test]
fn f_077_close_never_replaces_the_command() {
    let (mut host, pane) = failed_codex_to_claude();
    let submissions = host.deliver_one(client_message("close", pane));
    assert!(replaced(&submissions).is_empty());
}

/// REQ: F-077
#[test]
fn f_077_cancel_asks_the_running_job_to_stop() {
    let (mut host, pane) = server_with_pane("codex", true);
    let request = run_until_job(&mut host, pane);
    host.deliver_one(client_message("cancel", pane));
    assert_eq!(host.jobs().cancelled(), [request]);
}

/// REQ: F-077
#[test]
fn f_077_cancel_without_a_running_job_does_nothing() {
    let (mut host, pane) = server_with_pane("codex", true);
    host.deliver_one(client_message("cancel", pane));
    assert!(host.jobs().cancelled().is_empty());
}

/// REQ: F-077
#[test]
fn f_077_a_cancelled_conversion_ends_as_a_failure_that_offers_the_original_session() {
    let (mut host, pane) = server_with_pane("codex", true);
    let request = run_until_job(&mut host, pane);
    host.deliver_one(client_message("cancel", pane));
    let error = PluginError {
        code: ErrorCode::Cancelled,
        message: "conversion cancelled".into(),
        retry_after_ms: None,
    };
    let submissions = finish_job(&mut host, request, Err(error));
    let failed = messages(&submissions)
        .into_iter()
        .rev()
        .find(|m| m.2 == "failed")
        .expect("failed message");
    let payload: Value = serde_json::from_str(&failed.3).unwrap();
    assert_eq!(payload["can_resume"], true);
    assert!(created(&submissions).is_empty());
}
