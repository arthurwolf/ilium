//! Twins of `ilium-server/tests/session_conversion.rs`: stopping the process of a pane keeps the pane,
//! replacing swaps the frozen pane for a new one, and refusals leave the pane alone.

use ilium_plugin_sdk::types::{ErrorCode, Outcome, PluginError};

use crate::common::*;

/// REQ: PORT:ilium-server/tests/session_conversion.rs::terminate_keeps_the_pane_and_replace_swaps_it_in_place
#[test]
fn port_session_conversion_server_terminate_keeps_the_pane_and_replace_swaps_it() {
    let (mut host, pane) = server_with_pane("codex", true);
    let start = host.deliver_one(start_message(pane, "claude"));
    assert_eq!(terminated(&start), [pane]);
    assert!(
        closed(&start).is_empty(),
        "stopping the process keeps the pane"
    );
    answer_terminate(&mut host, &start, Outcome::Ok);
    let request = host.jobs().list().first().expect("job").request;
    host.jobs().finish(request, Ok(job_output(3, 3)));
    host.settle();
    let submissions = host.submitted();
    let panes = created(&submissions);
    assert_eq!(panes.len(), 1, "{panes:?}");
    assert_eq!(panes[0].1, "ilium.terminal");
    assert_eq!(closed(&submissions), [pane]);
}

/// REQ: PORT:ilium-server/tests/session_conversion.rs::freeze_rejects_generic_command_without_stopping_the_pane
#[test]
fn port_session_conversion_server_a_pane_without_session_identity_is_not_frozen_or_stopped() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "agent", "claude");
    let submissions = host.deliver_one(start_message(pane, "codex"));
    assert!(frozen(&submissions).is_empty());
    assert!(terminated(&submissions).is_empty());
}

/// REQ: PORT:ilium-server/tests/session_conversion.rs::freeze_rejects_generic_command_without_stopping_the_pane
#[test]
fn port_session_conversion_server_unfreezing_a_pane_that_is_not_frozen_submits_nothing() {
    let (mut host, pane) = server_with_pane("claude", true);
    let submissions = host.deliver_one(client_message("close", pane));
    assert!(unfrozen(&submissions).is_empty());
}

/// REQ: PORT:ilium-server/tests/session_conversion.rs::terminating_an_unknown_pane_reports_the_failure
#[test]
fn port_session_conversion_server_an_unknown_pane_is_refused_without_a_stop() {
    let (mut host, _pane) = server_with_pane("claude", true);
    let submissions = host.deliver_one(start_message(9999, "codex"));
    assert!(frozen(&submissions).is_empty());
    assert!(terminated(&submissions).is_empty());
}

/// REQ: PORT:ilium-server/tests/session_conversion.rs::terminating_an_unknown_pane_reports_the_failure
#[test]
fn port_session_conversion_server_a_failed_stop_is_reported_to_the_client() {
    let (mut host, pane) = server_with_pane("codex", true);
    let start = host.deliver_one(start_message(pane, "claude"));
    let error = PluginError {
        code: ErrorCode::NotFound,
        message: "no such pane".into(),
        retry_after_ms: None,
    };
    let reply = answer_terminate(&mut host, &start, Outcome::Failed(error));
    assert!(failed_payload(&reply).is_some(), "{reply:?}");
}
