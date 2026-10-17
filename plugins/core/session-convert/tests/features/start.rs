//! F-077: the server side starts a conversion. It checks the pane and the source transcript, freezes
//! the pane, writes the state to `storage`, adds a journal entry and arms the ten minute watchdog
//! (page sample "Freeze the pane and keep a journal").

use ilium_plugin_sdk::host::Bucket;
use ilium_plugin_sdk::primitives::SessionRef;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    DurableTimerEvent, Event, FsPath, Intent, PluginSide, Submission, TimerId,
};
use serde_json::{json, Value};

use crate::common::*;

/// A server host with a pane of `signature`, its verified session and (optionally) the transcript file.
pub fn server_with_pane(signature: &str, with_transcript: bool) -> (FakeHost, u64) {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "agent", signature);
    let scope = format!("agent-sessions:{signature}");
    let path = format!("projects/p/{SOURCE_SESSION}.jsonl");
    host.agents().set_session(
        pane,
        Some(SessionRef {
            scope: scope.clone(),
            path: path.clone(),
            session_id: SOURCE_SESSION.to_string(),
        }),
    );
    if with_transcript {
        host.fs()
            .write(FsPath { scope, path }, b"{\"type\":\"user\"}\n");
    }
    (host, pane)
}

/// The `start` message the client side sends.
pub fn start_message(pane: u64, target: &str) -> Event {
    message_from(
        PluginSide::Client,
        "start",
        json!({"pane": pane, "target": target})
            .to_string()
            .as_bytes(),
    )
}

/// Starts a conversion of a Claude pane to Codex and returns what the plugin submitted.
pub fn start_claude(host: &mut FakeHost, pane: u64) -> Vec<Submission> {
    host.deliver_one(start_message(pane, "codex"))
}

fn state(host: &FakeHost, pane: u64) -> Option<Value> {
    host.storage()
        .get(Bucket::Session, &format!("convert/{pane}"))
        .map(|bytes| serde_json::from_slice(&bytes).expect("JSON state"))
}

fn timer_fired(name: &str) -> Event {
    Event::DurableTimerFired(DurableTimerEvent {
        timer: TimerId(1),
        name: name.to_string(),
        late_by_millis: 0,
    })
}

fn toasts(submissions: &[Submission]) -> usize {
    submissions
        .iter()
        .filter(|submission| matches!(submission.intent, Intent::Toast(_)))
        .count()
}

/// REQ: F-077
#[test]
fn f_077_start_freezes_the_pane_under_the_dedup_key_freeze_pane() {
    let (mut host, pane) = server_with_pane("claude", true);
    let submissions = start_claude(&mut host, pane);
    assert_eq!(frozen(&submissions), [pane]);
    let freeze = submissions
        .iter()
        .find(|s| matches!(s.intent, Intent::FreezePane(_)))
        .unwrap();
    assert_eq!(freeze.dedup_key, Some(format!("freeze-{pane}")));
}

/// REQ: F-077
#[test]
fn f_077_start_stops_the_agent_process_after_the_freeze() {
    let (mut host, pane) = server_with_pane("claude", true);
    let submissions = start_claude(&mut host, pane);
    let order: Vec<&str> = submissions
        .iter()
        .filter_map(|s| match &s.intent {
            Intent::FreezePane(_) => Some("freeze"),
            Intent::TerminatePaneProcess(spec) if spec.pane.0 == pane => Some("terminate"),
            _ => None,
        })
        .collect();
    assert_eq!(order, ["freeze", "terminate"]);
}

/// REQ: F-077
#[test]
fn f_077_start_writes_the_state_to_the_session_bucket() {
    let (mut host, pane) = server_with_pane("claude", true);
    start_claude(&mut host, pane);
    assert_eq!(
        state(&host, pane),
        Some(json!({"pane": pane, "target": "codex", "phase": "frozen"}))
    );
}

/// REQ: F-077
#[test]
fn f_077_start_for_a_codex_pane_records_target_claude() {
    let (mut host, pane) = server_with_pane("codex", true);
    host.deliver_one(start_message(pane, "claude"));
    assert_eq!(
        state(&host, pane),
        Some(json!({"pane": pane, "target": "claude", "phase": "frozen"}))
    );
}

/// REQ: F-077
#[test]
fn f_077_start_adds_the_journal_entry_convert_started_with_the_target() {
    let (mut host, pane) = server_with_pane("claude", true);
    start_claude(&mut host, pane);
    let entries = host.journal().entries();
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].0, Some(pane));
    assert_eq!(entries[0].1, "convert-started");
    assert_eq!(
        serde_json::from_str::<Value>(&entries[0].2).unwrap(),
        json!({"target": "codex"})
    );
}

/// REQ: F-077
#[test]
fn f_077_start_arms_a_durable_watchdog_ten_minutes_ahead() {
    let (mut host, pane) = server_with_pane("claude", true);
    start_claude(&mut host, pane);
    assert_eq!(
        host.durable_timers(),
        [(
            format!("convert-watchdog/{pane}"),
            CLOCK_START_MS + WATCHDOG_MILLIS
        )]
    );
}

/// REQ: F-077
#[test]
fn f_077_the_watchdog_is_exactly_600000_milliseconds() {
    assert_eq!(WATCHDOG_MILLIS, 600_000);
    assert!(page_text().contains("const WATCHDOG_MILLIS: u64 = 10 * 60 * 1000;"));
}

/// REQ: F-077
#[test]
fn f_077_a_missing_source_transcript_refuses_the_start_and_leaves_the_pane_running() {
    let (mut host, pane) = server_with_pane("claude", false);
    let submissions = start_claude(&mut host, pane);
    assert!(frozen(&submissions).is_empty());
    assert!(submissions
        .iter()
        .all(|s| !matches!(s.intent, Intent::TerminatePaneProcess(_))));
    assert_eq!(state(&host, pane), None);
    assert!(host.durable_timers().is_empty());
    assert!(host.journal().entries().is_empty());
}

/// REQ: F-077
#[test]
fn f_077_a_refused_start_tells_the_user_with_one_toast() {
    let (mut host, pane) = server_with_pane("claude", false);
    let submissions = start_claude(&mut host, pane);
    assert_eq!(toasts(&submissions), 1, "{submissions:?}");
}

/// REQ: F-077
#[test]
fn f_077_a_pane_without_a_session_is_refused() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "agent", "claude");
    let submissions = start_claude(&mut host, pane);
    assert!(frozen(&submissions).is_empty());
    assert_eq!(state(&host, pane), None);
}

/// REQ: F-077
#[test]
fn f_077_a_worktree_pane_is_refused() {
    let (mut host, pane) = server_with_pane("claude", true);
    host.tree().set_worktree(pane, true);
    let submissions = start_claude(&mut host, pane);
    assert!(frozen(&submissions).is_empty());
    assert_eq!(state(&host, pane), None);
}

/// REQ: F-077
#[test]
fn f_077_an_antigravity_session_cannot_be_converted() {
    let (mut host, pane) = server_with_pane("antigravity", true);
    for target in ["codex", "claude"] {
        let submissions = host.deliver_one(start_message(pane, target));
        assert!(frozen(&submissions).is_empty(), "target {target}");
    }
    assert_eq!(state(&host, pane), None);
}

/// REQ: F-077
#[test]
fn f_077_a_claude_pane_cannot_be_converted_to_claude() {
    let (mut host, pane) = server_with_pane("claude", true);
    let submissions = host.deliver_one(start_message(pane, "claude"));
    assert!(frozen(&submissions).is_empty());
    assert_eq!(state(&host, pane), None);
}

/// REQ: F-077
#[test]
fn f_077_a_codex_pane_cannot_be_converted_to_codex() {
    let (mut host, pane) = server_with_pane("codex", true);
    let submissions = host.deliver_one(start_message(pane, "codex"));
    assert!(frozen(&submissions).is_empty());
}

/// REQ: F-077
#[test]
fn f_077_an_unknown_target_is_refused() {
    let (mut host, pane) = server_with_pane("claude", true);
    let submissions = host.deliver_one(start_message(pane, "gemini"));
    assert!(frozen(&submissions).is_empty());
    assert_eq!(state(&host, pane), None);
}

/// REQ: F-077
#[test]
fn f_077_a_malformed_start_payload_is_refused_without_a_failure_record() {
    let (mut host, _pane) = server_with_pane("claude", true);
    let submissions = host.deliver_one(message_from(PluginSide::Client, "start", b"not json"));
    assert!(frozen(&submissions).is_empty());
    assert!(host.failures().is_empty());
}

/// REQ: F-077
#[test]
fn f_077_an_unknown_pane_is_refused() {
    let (mut host, _pane) = server_with_pane("claude", true);
    let submissions = host.deliver_one(start_message(9999, "codex"));
    assert!(frozen(&submissions).is_empty());
}

/// REQ: F-077
#[test]
fn f_077_only_one_conversion_runs_at_a_time() {
    let (mut host, pane) = server_with_pane("claude", true);
    let other = add_agent_pane(&mut host, "other", "codex");
    host.agents().set_session(
        other,
        Some(SessionRef {
            scope: "agent-sessions:codex".into(),
            path: "rollout.jsonl".into(),
            session_id: "abc".into(),
        }),
    );
    host.fs().write(
        FsPath {
            scope: "agent-sessions:codex".into(),
            path: "rollout.jsonl".into(),
        },
        b"{}\n",
    );
    start_claude(&mut host, pane);
    let second = host.deliver_one(start_message(other, "claude"));
    assert!(frozen(&second).is_empty());
    assert_eq!(state(&host, other), None);
    assert_eq!(host.durable_timers().len(), 1);
}

/// REQ: F-077
#[test]
fn f_077_the_state_is_in_the_snapshot_a_restart_would_see() {
    let (mut host, pane) = server_with_pane("claude", true);
    start_claude(&mut host, pane);
    let snapshot = host.snapshot();
    let session = snapshot.storage.get("session").expect("session bucket");
    assert!(session.contains_key(&format!("convert/{pane}")));
}

/// REQ: F-077
#[test]
fn f_077_the_watchdog_unfreezes_the_pane_and_removes_the_state() {
    let (mut host, pane) = server_with_pane("claude", true);
    start_claude(&mut host, pane);
    let submissions = host.deliver_one(timer_fired(&format!("convert-watchdog/{pane}")));
    assert_eq!(unfrozen(&submissions), [pane]);
    assert_eq!(state(&host, pane), None);
}

/// REQ: F-077
#[test]
fn f_077_the_watchdog_fires_when_the_clock_passes_ten_minutes() {
    let (mut host, pane) = server_with_pane("claude", true);
    start_claude(&mut host, pane);
    host.advance_clock(WATCHDOG_MILLIS);
    assert_eq!(unfrozen(&host.submitted()), [pane]);
    assert_eq!(state(&host, pane), None);
}

/// REQ: F-077
#[test]
fn f_077_the_watchdog_does_not_fire_one_millisecond_early() {
    let (mut host, pane) = server_with_pane("claude", true);
    start_claude(&mut host, pane);
    host.advance_clock(WATCHDOG_MILLIS - 1);
    assert!(unfrozen(&host.submitted()).is_empty());
    assert!(state(&host, pane).is_some());
}

/// REQ: F-077
#[test]
fn f_077_the_watchdog_does_nothing_when_the_state_is_already_gone() {
    let (mut host, _pane) = server_with_pane("claude", true);
    let submissions = host.deliver_one(timer_fired("convert-watchdog/777"));
    assert!(unfrozen(&submissions).is_empty());
}

/// REQ: F-077
#[test]
fn f_077_a_timer_with_another_name_is_ignored() {
    let (mut host, pane) = server_with_pane("claude", true);
    start_claude(&mut host, pane);
    let submissions = host.deliver_one(timer_fired(&format!("other-timer/{pane}")));
    assert!(unfrozen(&submissions).is_empty());
    assert!(state(&host, pane).is_some());
}

/// REQ: F-077
#[test]
fn f_077_the_watchdog_tells_the_client_the_conversion_ended() {
    let (mut host, pane) = server_with_pane("claude", true);
    start_claude(&mut host, pane);
    let submissions = host.deliver_one(timer_fired(&format!("convert-watchdog/{pane}")));
    let sent = messages(&submissions);
    assert!(
        sent.iter()
            .any(|m| m.1 == PluginSide::Client && m.2 == "finished"),
        "{sent:?}"
    );
}
