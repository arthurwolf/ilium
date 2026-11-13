//! Twins of `ilium-ipc/tests/session_conversion_wire.rs`. The bincode wire is gone in the plugin world: what
//! remains observable is that the plugin submits the stop, replace and unfreeze intents of the conversion
//! flow, each exactly once, and never touches the Antigravity status line.

use ilium_plugin_sdk::types::{Intent, Outcome};

use crate::common::*;

/// REQ: PORT:ilium-ipc/tests/session_conversion_wire.rs::terminate_and_replace_requests_round_trip_with_stable_names
#[test]
fn port_session_conversion_wire_terminate_and_replace_intents_are_submitted_for_the_pane() {
    let (mut host, pane) = server_with_pane("codex", true);
    let start = host.deliver_one(start_message(pane, "claude"));
    assert_eq!(terminated(&start), [pane]);
    answer_terminate(&mut host, &start, Outcome::Ok);
    let request = host.jobs().list().first().expect("job").request;
    host.jobs().finish(
        request,
        Err(ilium_plugin_sdk::types::PluginError {
            code: ilium_plugin_sdk::types::ErrorCode::Io,
            message: "x".into(),
            retry_after_ms: None,
        }),
    );
    host.settle();
    let resumed = host.deliver_one(client_message("resume", pane));
    assert_eq!(replaced(&resumed).len(), 1);
}

/// REQ: PORT:ilium-ipc/tests/session_conversion_wire.rs::unfreeze_request_round_trips_at_the_append_only_request_tail
#[test]
fn port_session_conversion_wire_unfreeze_intent_is_submitted_for_the_pane() {
    let (mut host, pane) = server_with_pane("claude", true);
    host.deliver_one(start_message(pane, "codex"));
    let closed_by_user = host.deliver_one(client_message("close", pane));
    assert_eq!(unfrozen(&closed_by_user), [pane]);
}

/// REQ: PORT:ilium-ipc/tests/session_conversion_wire.rs::new_variants_do_not_shift_the_previous_last_variant
#[test]
fn port_session_conversion_wire_the_intent_set_of_one_conversion_is_stable() {
    // The append-only wire order has no plugin equivalent; the equivalent contract is that one start
    // submits freeze then terminate and nothing else of the pane lifecycle.
    let (mut host, pane) = server_with_pane("claude", true);
    let submissions = host.deliver_one(start_message(pane, "codex"));
    let lifecycle: Vec<&str> = submissions
        .iter()
        .filter_map(|s| match &s.intent {
            Intent::FreezePane(_) => Some("freeze"),
            Intent::TerminatePaneProcess(_) => Some("terminate"),
            Intent::UnfreezePane(_) => Some("unfreeze"),
            Intent::ReplacePaneCommand(_) => Some("replace"),
            Intent::CloseNode(_) => Some("close"),
            _ => None,
        })
        .collect();
    assert_eq!(lifecycle, ["freeze", "terminate"]);
}

/// REQ: PORT:ilium-ipc/tests/session_conversion_wire.rs::pane_process_terminated_round_trips_success_and_failure
#[test]
fn port_session_conversion_wire_a_terminate_answer_of_success_and_of_failure_is_handled() {
    let (mut ok_host, ok_pane) = server_with_pane("codex", true);
    let start = ok_host.deliver_one(start_message(ok_pane, "claude"));
    answer_terminate(&mut ok_host, &start, Outcome::Ok);
    assert_eq!(ok_host.jobs().list().len(), 1);

    let (mut bad_host, bad_pane) = server_with_pane("codex", true);
    let start = bad_host.deliver_one(start_message(bad_pane, "claude"));
    let reply = answer_terminate(
        &mut bad_host,
        &start,
        Outcome::Failed(ilium_plugin_sdk::types::PluginError {
            code: ilium_plugin_sdk::types::ErrorCode::Busy,
            message: "process tree still alive".into(),
            retry_after_ms: None,
        }),
    );
    assert!(bad_host.jobs().list().is_empty());
    assert!(failed_payload(&reply).is_some());
}

/// REQ: PORT:ilium-ipc/tests/session_conversion_wire.rs::antigravity_statusline_actions_round_trip_after_the_existing_request_tail
#[test]
fn port_session_conversion_wire_an_antigravity_pane_is_never_converted_or_stopped() {
    let (mut host, pane) = server_with_pane("antigravity", true);
    for target in ["codex", "claude"] {
        let submissions = host.deliver_one(start_message(pane, target));
        assert!(
            submissions.is_empty()
                || (frozen(&submissions).is_empty() && terminated(&submissions).is_empty()),
            "{target}"
        );
    }
}

/// REQ: PORT:ilium-ipc/tests/session_conversion_wire.rs::antigravity_statusline_completion_round_trips_at_the_append_only_event_tail
#[test]
fn port_session_conversion_wire_an_antigravity_pane_gets_no_menu_entry() {
    let mut host = client_host();
    let pane = pane_with_session(&mut host, "antigravity");
    assert!(menu(&mut host, Some(pane)).is_empty());
}
