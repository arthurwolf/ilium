//! Twins of the unit tests of `ilium-client/src/session_conversion.rs` (dialog model, app flow, worker).
//! The dialog model lives in the client side of the plugin, the app flow is split between both sides.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ErrorCode, Intent, Outcome, PluginError, PluginSide};
use serde_json::{json, Value};

use crate::common::*;

fn claude_dialog() -> FakeHost {
    let mut host = client_host();
    server_says(
        &mut host,
        "begin",
        json!({"pane": 7, "source": "claude", "target": "codex"}),
    );
    host
}

fn rows(host: &mut FakeHost, height: u16) -> Vec<String> {
    host.render(panel(), 48, height)
        .text()
        .split('\n')
        .map(str::to_string)
        .collect()
}

fn percent_shown(host: &mut FakeHost) -> u32 {
    let text = host.render(panel(), 48, 12).text();
    let at = text.find('%').expect("percent sign");
    let digits: String = text[..at]
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    digits.parse().expect("percent digits")
}

fn press(host: &mut FakeHost, key: &str) -> Vec<ilium_plugin_sdk::types::Submission> {
    let before = host.submitted().len();
    host.key(panel(), key);
    host.submitted()[before..].to_vec()
}

/// A Claude to Codex conversion whose app-server died after the agent stopped.
fn failed_claude_to_codex() -> (FakeHost, u64) {
    let (mut host, pane) = server_with_pane("claude", true);
    start_and_stop(&mut host, pane, "codex");
    host.settle();
    let child = host.process().children()[0].id;
    host.deliver_one(ilium_plugin_sdk::types::Event::ChildExited(
        ilium_plugin_sdk::types::ChildExitedEvent {
            child: ilium_plugin_sdk::types::ChildId(child),
            status: Some(1),
            signal: None,
        },
    ));
    (host, pane)
}

/// A Codex to Claude conversion whose job failed after the agent stopped.
fn failed_codex_to_claude() -> (FakeHost, u64) {
    let (mut host, pane) = server_with_pane("codex", true);
    start_and_stop(&mut host, pane, "claude");
    let request = host.jobs().list().first().expect("job started").request;
    let error = PluginError {
        code: ErrorCode::Io,
        message: "transcript unreadable".into(),
        retry_after_ms: None,
    };
    host.jobs().finish(request, Err(error));
    host.settle();
    (host, pane)
}

/// REQ: PORT:ilium-client/src/session_conversion.rs::conversion_worker_completes_through_the_bounded_io_receipt
#[test]
fn port_session_conversion_worker_with_a_missing_transcript_ends_as_a_refusal_with_a_toast() {
    // Original: the worker ends with a failure naming the missing Claude Code transcript.
    // Plugin form: the start is refused at once, nothing is frozen and the user gets one toast.
    let (mut host, pane) = server_with_pane("claude", false);
    let submissions = host.deliver_one(start_message(pane, "codex"));
    assert!(frozen(&submissions).is_empty());
    assert!(terminated(&submissions).is_empty());
    assert_eq!(toasts(&submissions), 1, "{submissions:?}");
}

/// REQ: PORT:ilium-client/src/session_conversion.rs::conversion_worker_completes_through_the_bounded_io_receipt
#[test]
fn port_session_conversion_worker_completion_is_reported_with_one_finished_or_failed_message() {
    let (_host, pane, submissions) = converted_codex_to_claude();
    let finished: Vec<Value> = messages(&submissions)
        .into_iter()
        .filter(|m| m.0 == PluginSide::Client && m.1 == "finished")
        .map(|m| serde_json::from_str(&m.2).unwrap())
        .collect();
    assert_eq!(finished, [json!({"pane": pane, "ok": true})]);
}

/// REQ: PORT:ilium-client/src/session_conversion.rs::step_events_populate_the_step_list_and_log
#[test]
fn port_session_conversion_step_events_populate_the_step_list_and_log() {
    let mut host = claude_dialog();
    server_says(&mut host, "step", json!({"index": 2}));
    server_says(
        &mut host,
        "progress",
        json!({"percent": 10, "message": "read 12 lines"}),
    );
    let rows = rows(&mut host, 12);
    assert!(rows[1].starts_with("│ ✓ "), "{}", rows[1]);
    assert!(rows[2].starts_with("│ ▶ "), "{}", rows[2]);
    assert!(rows[3].starts_with("│ ○ "), "{}", rows[3]);
    assert!(rows[9].contains("read 12 lines"), "{}", rows[9]);
}

/// REQ: PORT:ilium-client/src/session_conversion.rs::progress_never_moves_backwards_or_past_one
#[test]
fn port_session_conversion_progress_never_moves_backwards_or_past_one() {
    let mut host = claude_dialog();
    server_says(
        &mut host,
        "progress",
        json!({"percent": 60, "message": "a"}),
    );
    server_says(
        &mut host,
        "progress",
        json!({"percent": 20, "message": "b"}),
    );
    assert_eq!(percent_shown(&mut host), 60);
    server_says(
        &mut host,
        "progress",
        json!({"percent": 300, "message": "c"}),
    );
    assert_eq!(percent_shown(&mut host), 100);
}

/// REQ: PORT:ilium-client/src/session_conversion.rs::failure_is_terminal_and_logged
#[test]
fn port_session_conversion_failure_is_terminal_and_shown_with_its_message() {
    let mut host = claude_dialog();
    server_says(
        &mut host,
        "failed",
        json!({"message": "codex is not installed", "carried": 0, "left_out": 0, "can_resume": false}),
    );
    let screen = host.render(panel(), 48, 10).text();
    assert!(screen.contains("✕ Conversion failed"), "{screen}");
    assert!(screen.contains("codex is not installed"), "{screen}");
    server_says(&mut host, "step", json!({"index": 3}));
    assert!(host
        .render(panel(), 48, 10)
        .text()
        .contains("✕ Conversion failed"));
}

/// REQ: PORT:ilium-client/src/session_conversion.rs::log_is_bounded
#[test]
fn port_session_conversion_log_stays_bounded_and_shows_the_last_line() {
    let mut host = claude_dialog();
    for index in 0..550 {
        server_says(
            &mut host,
            "progress",
            json!({"percent": 1, "message": format!("line {index}")}),
        );
    }
    let rows = rows(&mut host, 12);
    assert!(rows.len() <= 12, "{}", rows.len());
    assert!(rows[9].contains("line 549"), "{}", rows[9]);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/session_conversion.rs::resume_command_uses_the_provider_grammar
#[test]
fn port_session_conversion_resume_command_of_a_codex_pane_is_codex_resume() {
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

/// REQ: PORT:ilium-client/src/session_conversion.rs::resume_command_uses_the_provider_grammar
#[test]
fn port_session_conversion_resume_command_of_a_claude_pane_is_claude_dash_dash_resume() {
    let (mut host, pane) = failed_claude_to_codex();
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

/// REQ: PORT:ilium-client/src/session_conversion.rs::only_the_opposite_builtin_provider_is_offered
#[test]
fn port_session_conversion_only_the_opposite_builtin_provider_is_offered() {
    let mut host = client_host();
    let claude = pane_with_session(&mut host, "claude");
    let codex = pane_with_session(&mut host, "codex");
    let antigravity = pane_with_session(&mut host, "antigravity");
    assert_eq!(
        menu(&mut host, Some(claude))
            .iter()
            .map(|e| e.id.clone())
            .collect::<Vec<_>>(),
        ["to-codex"]
    );
    assert_eq!(
        menu(&mut host, Some(codex))
            .iter()
            .map(|e| e.id.clone())
            .collect::<Vec<_>>(),
        ["to-claude"]
    );
    assert!(menu(&mut host, Some(antigravity)).is_empty());
}

/// REQ: PORT:ilium-client/src/session_conversion.rs::a_pane_without_a_resolved_session_is_not_convertible
#[test]
fn port_session_conversion_a_pane_without_a_resolved_session_is_not_convertible() {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "claude", "claude");
    let entries = menu(&mut host, Some(pane));
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].disabled_reason.as_deref(),
        Some("no verified session")
    );
    assert!(host.deliver_one(convert_click("to-codex", pane)).is_empty());
}

/// REQ: PORT:ilium-client/src/session_conversion.rs::no_second_conversion_is_offered_while_one_is_active
#[test]
fn port_session_conversion_no_second_conversion_is_offered_while_one_is_active() {
    let mut host = client_host();
    let pane = pane_with_session(&mut host, "claude");
    host.deliver_one(convert_click("to-codex", pane));
    let entries = menu(&mut host, Some(pane));
    assert_eq!(
        entries[0].disabled_reason.as_deref(),
        Some("another conversion runs")
    );
    assert!(host.deliver_one(convert_click("to-codex", pane)).is_empty());
}

/// REQ: PORT:ilium-client/src/session_conversion.rs::a_stopped_agent_starts_the_worker_and_a_failed_stop_does_not
#[test]
fn port_session_conversion_a_stopped_agent_starts_the_worker() {
    let (mut host, pane) = server_with_pane("codex", true);
    let submissions = host.deliver_one(start_message(pane, "claude"));
    assert!(
        host.jobs().list().is_empty(),
        "no worker before the agent stopped"
    );
    answer_terminate(&mut host, &submissions, Outcome::Ok);
    let jobs = host.jobs().list();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].kind, "codex-to-claude");
}

/// REQ: PORT:ilium-client/src/session_conversion.rs::a_stopped_agent_starts_the_worker_and_a_failed_stop_does_not
#[test]
fn port_session_conversion_a_failed_stop_starts_no_worker_and_offers_no_resume() {
    let (mut host, pane) = server_with_pane("codex", true);
    let submissions = host.deliver_one(start_message(pane, "claude"));
    let error = PluginError {
        code: ErrorCode::Busy,
        message: "still running".into(),
        retry_after_ms: None,
    };
    let reply = answer_terminate(&mut host, &submissions, Outcome::Failed(error));
    assert!(host.jobs().list().is_empty());
    assert_eq!(
        failed_payload(&reply).expect("failed message")["can_resume"],
        false
    );
}

/// REQ: PORT:ilium-client/src/session_conversion.rs::success_replaces_the_pane_with_the_converted_session_and_closes_the_dialog
#[test]
fn port_session_conversion_success_replaces_the_pane_with_the_converted_session() {
    let (_host, pane, submissions) = converted_codex_to_claude();
    let panes = created(&submissions);
    assert_eq!(panes.len(), 1, "{panes:?}");
    assert_eq!(&panes[0].2[..2], ["claude", "--resume"]);
    assert_eq!(closed(&submissions), [pane]);
}

/// REQ: PORT:ilium-client/src/session_conversion.rs::success_replaces_the_pane_with_the_converted_session_and_closes_the_dialog
#[test]
fn port_session_conversion_success_closes_the_dialog() {
    let mut host = claude_dialog();
    let before = host.submitted().len();
    server_says(&mut host, "finished", json!({"pane": 7, "ok": true}));
    let submissions = host.submitted()[before..].to_vec();
    assert!(intents(&submissions).contains(&Intent::CloseView(panel())));
}

/// REQ: PORT:ilium-client/src/session_conversion.rs::failure_after_the_stop_offers_resuming_the_original_session
#[test]
fn port_session_conversion_failure_after_the_stop_offers_resuming_the_original_session() {
    let (mut host, pane) = failed_codex_to_claude();
    let submissions = host.submitted();
    assert_eq!(
        failed_payload(&submissions).expect("failed message")["can_resume"],
        true
    );
    let _ = pane;
    let mut client = claude_dialog();
    server_says(
        &mut client,
        "failed",
        json!({"message": "transcript unreadable", "carried": 0, "left_out": 0, "can_resume": true}),
    );
    let sent = messages(&press(&mut client, "Enter"));
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(
        (sent[0].0, sent[0].1.as_str()),
        (PluginSide::Server, "resume")
    );
    assert_eq!(
        serde_json::from_str::<Value>(&sent[0].2).unwrap(),
        json!({"pane": 7})
    );
}

/// REQ: PORT:ilium-client/src/session_conversion.rs::escape_closes_a_failed_dialog_and_unfreezes_the_pane
#[test]
fn port_session_conversion_escape_closes_a_failed_dialog_and_unfreezes_the_pane() {
    let mut client = claude_dialog();
    server_says(
        &mut client,
        "failed",
        json!({"message": "nope", "carried": 0, "left_out": 0, "can_resume": false}),
    );
    assert!(
        messages(&press(&mut client, "Enter"))
            .iter()
            .all(|m| m.1 != "resume"),
        "Enter must not resume a live agent"
    );
    let submissions = press(&mut client, "Esc");
    assert_eq!(
        messages(&submissions)
            .iter()
            .filter(|m| m.1 == "close")
            .count(),
        1
    );
    assert!(intents(&submissions).contains(&Intent::CloseView(panel())));

    let (mut server, pane) = server_with_pane("claude", true);
    server.deliver_one(start_message(pane, "codex"));
    let reply = server.deliver_one(client_message("close", pane));
    assert_eq!(unfrozen(&reply), [pane]);
}

/// REQ: PORT:ilium-client/src/session_conversion.rs::escape_while_converting_requests_cancellation
#[test]
fn port_session_conversion_escape_while_converting_requests_cancellation() {
    let mut client = claude_dialog();
    let submissions = press(&mut client, "Esc");
    let sent = messages(&submissions);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(
        (sent[0].0, sent[0].1.as_str()),
        (PluginSide::Server, "cancel")
    );
    assert!(!intents(&submissions)
        .iter()
        .any(|intent| matches!(intent, Intent::CloseView(_))));

    let (mut server, pane) = server_with_pane("codex", true);
    start_and_stop(&mut server, pane, "claude");
    let request = server.jobs().list().first().expect("job").request;
    server.deliver_one(client_message("cancel", pane));
    assert_eq!(server.jobs().cancelled(), [request]);
}

/// REQ: PORT:ilium-client/src/session_conversion.rs::the_replacement_takes_focus_when_the_old_pane_had_it
#[test]
fn port_session_conversion_the_replacement_is_created_before_the_old_pane_is_closed() {
    // The focus hand-off is the host's: it needs the replacement to exist when the old pane goes away.
    let (_host, pane, submissions) = converted_codex_to_claude();
    let order: Vec<&str> = submissions
        .iter()
        .filter_map(|s| match &s.intent {
            Intent::CreatePane(_) => Some("create"),
            Intent::CloseNode(node) if node.0 == pane => Some("close"),
            _ => None,
        })
        .collect();
    assert_eq!(order, ["create", "close"]);
}
