//! The plugin acts only inside its grants (F-077): it writes only into agent-config:claude, never touches
//! the session scopes it reads, and the client side submits no file or process intent.

use ilium_plugin_sdk::types::Intent;

use crate::common::*;
use crate::start::server_with_pane;
use crate::write_verify::{finish_job, job_output, run_until_job};

/// REQ: F-077
#[test]
fn grants_077_a_full_codex_to_claude_conversion_writes_nothing_into_the_codex_scope() {
    let (mut host, pane) = server_with_pane("codex", true);
    let before = host.fs().paths("agent-sessions:codex");
    let request = run_until_job(&mut host, pane);
    finish_job(&mut host, request, Ok(job_output(3, 3)));
    assert_eq!(host.fs().paths("agent-sessions:codex"), before);
}

/// REQ: F-077
#[test]
fn grants_077_a_full_codex_to_claude_conversion_writes_nothing_into_the_claude_session_scope() {
    let (mut host, pane) = server_with_pane("codex", true);
    let before = host.fs().paths("agent-sessions:claude");
    let request = run_until_job(&mut host, pane);
    finish_job(&mut host, request, Ok(job_output(3, 3)));
    assert_eq!(host.fs().paths("agent-sessions:claude"), before);
}

/// REQ: F-077
#[test]
fn grants_077_the_client_side_submits_no_pane_or_tree_intent_when_a_dialog_opens() {
    let mut host = client_host();
    host.deliver_one(message_from(
        ilium_plugin_sdk::types::PluginSide::Server,
        "begin",
        br#"{"pane":42,"source":"claude","target":"codex"}"#,
    ));
    assert!(intents(&host.submitted()).iter().all(|intent| !matches!(
        intent,
        Intent::CreatePane(_) | Intent::CloseNode(_) | Intent::TerminatePaneProcess(_)
    )));
}

/// REQ: F-077
#[test]
fn grants_077_the_server_side_asks_for_no_toast_on_a_refused_start() {
    let (mut host, pane) = server_with_pane("codex", false);
    let submissions = host.deliver_one(crate::start::start_message(pane, "claude"));
    assert!(frozen(&submissions).is_empty());
}
