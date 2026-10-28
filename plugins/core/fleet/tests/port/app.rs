//! Twins of `ilium-client/src/app.rs` agent-message and create-agent tests, through the
//! `ask-for-update` popup and channel and through `POST /create_agent`.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, AgentActivity, Delivery, Event, Intent, NodeId,
};
use serde_json::json;

use crate::common::*;

const FIXED_PROMPT: &str = "please remind me, in a very compact way, what you were doing, what I asked you to do, how it went, etc, remind me what's going on";

fn open_dialog() -> FakeHost {
    let mut host = client_host();
    host.deliver_one(Event::CommandInvoked(
        ilium_plugin_sdk::types::CommandInvokedEvent {
            command: "ask-for-update".to_string(),
            arguments_json: "{}".to_string(),
            origin: ilium_plugin_sdk::types::CommandOrigin::Palette,
            context: ilium_plugin_sdk::types::CommandContext::default(),
        },
    ));
    host.settle();
    host
}

fn messages(host: &FakeHost) -> Vec<(String, Vec<u8>)> {
    host.submitted()
        .into_iter()
        .filter_map(|s| {
            if let Intent::Message(m) = s.intent {
                Some((m.channel, m.payload))
            } else {
                None
            }
        })
        .collect()
}

fn type_and_send(host: &mut FakeHost, text: &str) {
    host.deliver_one(dialog_action(
        "update-text",
        ActionKind::Changed,
        Some(ActionValue::Text(text.to_string())),
    ));
    host.deliver_one(dialog_action(
        "update-send",
        ActionKind::Activated,
        Some(ActionValue::None),
    ));
    host.settle();
}

fn agents() -> (FakeHost, u64, u64, u64) {
    let mut host = server_host();
    let idle = add_agent(&mut host, "idle agent", "claude", AgentActivity::Idle);
    let done = add_agent(&mut host, "done agent", "codex", AgentActivity::Done);
    let shell = add_shell(&mut host, "shell");
    (host, idle, done, shell)
}

/// REQ: PORT:ilium-client/src/app.rs::agent_message_dialog_rejects_blank_or_unselected_input_without_requests
#[test]
fn app_agent_message_dialog_rejects_blank_input_without_requests() {
    let mut host = open_dialog();
    type_and_send(&mut host, "  \n\t");
    assert!(messages(&host).is_empty());
}

/// REQ: PORT:ilium-client/src/app.rs::agent_message_dialog_sends_selected_snapshot_agents_and_preserves_message
#[test]
fn app_agent_message_dialog_sends_the_typed_message_unchanged() {
    let mut host = open_dialog();
    type_and_send(&mut host, "  keep my spacing\nand lines ");
    assert_eq!(
        messages(&host),
        [(
            "ask-for-update".to_string(),
            b"  keep my spacing\nand lines ".to_vec()
        )]
    );
}

/// REQ: PORT:ilium-client/src/app.rs::agent_message_dialog_leaves_multiline_paste_mode_to_the_server
#[test]
fn app_agent_message_dialog_leaves_multiline_paste_mode_to_the_server() {
    let mut host = open_dialog();
    type_and_send(&mut host, "one\ntwo");
    assert_eq!(
        messages(&host)[0].1,
        b"one\ntwo".to_vec(),
        "the dialog sends the text literally"
    );
    let (mut server, ..) = agents();
    server.deliver_one(message("ask-for-update", b"one\ntwo".to_vec()));
    server.settle();
    let inputs = sent_inputs(&server);
    assert!(!inputs.is_empty());
    assert!(inputs.iter().all(|spec| spec.delivery == Delivery::Pasted));
}

/// REQ: PORT:ilium-client/src/app.rs::ask_for_update_sends_the_fixed_prompt_to_every_idle_agent_pane_only
#[test]
fn app_ask_for_update_sends_the_fixed_prompt_to_idle_and_done_agents_and_never_a_shell() {
    let (mut host, idle, done, shell) = agents();
    host.deliver_one(message("ask-for-update", FIXED_PROMPT.as_bytes().to_vec()));
    host.settle();
    let sent = sent_panes(&host);
    assert!(sent.contains(&NodeId(idle)) && sent.contains(&NodeId(done)));
    assert!(!sent.contains(&NodeId(shell)));
    assert!(sent_inputs(&host)
        .iter()
        .all(|spec| spec.text == FIXED_PROMPT));
}

/// REQ: PORT:ilium-client/src/app.rs::create_agent_dialog_queues_selected_command_prompt_and_submission
#[test]
fn app_create_agent_queues_the_selected_command_and_prompt_as_one_creation() {
    let mut host = server_host();
    let body = json!({"agent_type": "codex", "project": "/home/me/dev/app", "prompt": "/goal custom task"});
    host.deliver_one(http_call(
        1,
        "POST",
        "/create_agent",
        body.to_string().as_bytes(),
    ));
    host.settle();
    let specs = created_panes(&host);
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].command, Some(vec!["codex".to_string()]));
    assert_eq!(specs[0].initial_input.as_deref(), Some("/goal custom task"));
}

/// REQ: PORT:ilium-client/src/app.rs::create_agent_line_empty_prompt_preserves_exact_draft_without_admission
#[test]
fn app_create_agent_with_a_blank_prompt_creates_nothing() {
    let mut host = server_host();
    let body = json!({"agent_type": "codex", "project": "/home/me/dev/app", "prompt": " \t"});
    host.deliver_one(http_call(
        1,
        "POST",
        "/create_agent",
        body.to_string().as_bytes(),
    ));
    host.settle();
    assert!(created_panes(&host).is_empty());
}

/// REQ: PORT:ilium-client/src/app.rs::create_agent_line_refusal_retains_exact_draft_then_retry_accepts_once
#[test]
fn app_create_agent_a_refused_creation_can_be_retried_and_is_accepted_once() {
    use ilium_plugin_sdk::types::{ErrorCode, Outcome, PluginError};
    let mut host = server_host();
    let body =
        json!({"agent_type": "codex", "project": "/home/me/dev/app", "prompt": "exact task"})
            .to_string();
    host.deliver_one(http_call(1, "POST", "/create_agent", body.as_bytes()));
    host.settle();
    for request in host.pending_requests() {
        host.complete(
            request,
            Outcome::Failed(PluginError {
                code: ErrorCode::Io,
                message: "refused".to_string(),
                retry_after_ms: None,
            }),
        );
    }
    host.settle();
    host.deliver_one(http_call(2, "POST", "/create_agent", body.as_bytes()));
    host.settle();
    let specs = created_panes(&host);
    assert_eq!(
        specs.last().and_then(|s| s.initial_input.as_deref()),
        Some("exact task")
    );
}

/// REQ: PORT:ilium-client/src/app.rs::agent_message_dialog_rejects_blank_or_unselected_input_without_requests
#[test]
fn app_agent_message_dialog_blank_attempt_does_not_block_a_later_send() {
    let mut host = open_dialog();
    type_and_send(&mut host, "   ");
    type_and_send(&mut host, "hello");
    assert_eq!(
        messages(&host),
        [("ask-for-update".to_string(), b"hello".to_vec())]
    );
}

/// REQ: PORT:ilium-client/src/app.rs::agent_message_dialog_sends_selected_snapshot_agents_and_preserves_message
#[test]
fn app_agent_message_dialog_preserves_non_ascii_bytes() {
    let mut host = open_dialog();
    let text = "h\u{e9}llo \u{2713}";
    type_and_send(&mut host, text);
    assert_eq!(messages(&host)[0].1, text.as_bytes().to_vec());
}

/// REQ: PORT:ilium-client/src/app.rs::agent_message_dialog_leaves_multiline_paste_mode_to_the_server
#[test]
fn app_agent_message_dialog_multiline_text_reaches_panes_unchanged() {
    let (mut server, ..) = agents();
    server.deliver_one(message("ask-for-update", b"one\ntwo".to_vec()));
    server.settle();
    let inputs = sent_inputs(&server);
    assert!(!inputs.is_empty());
    assert!(inputs.iter().all(|spec| spec.text == "one\ntwo"));
}

/// REQ: PORT:ilium-client/src/app.rs::ask_for_update_sends_the_fixed_prompt_to_every_idle_agent_pane_only
#[test]
fn app_ask_for_update_skips_agents_that_are_working() {
    let mut host = server_host();
    let working = add_agent(&mut host, "busy agent", "claude", AgentActivity::Working);
    host.deliver_one(message("ask-for-update", FIXED_PROMPT.as_bytes().to_vec()));
    host.settle();
    assert!(!sent_panes(&host).contains(&NodeId(working)));
}

/// REQ: PORT:ilium-client/src/app.rs::create_agent_dialog_queues_selected_command_prompt_and_submission
#[test]
fn app_create_agent_uses_the_selected_agent_type_as_the_command() {
    let mut host = server_host();
    let body =
        json!({"agent_type": "claude", "project": "/home/me/dev/app", "prompt": "summarise"});
    host.deliver_one(http_call(
        1,
        "POST",
        "/create_agent",
        body.to_string().as_bytes(),
    ));
    host.settle();
    let specs = created_panes(&host);
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].command, Some(vec!["claude".to_string()]));
}

/// REQ: PORT:ilium-client/src/app.rs::create_agent_line_empty_prompt_preserves_exact_draft_without_admission
#[test]
fn app_create_agent_blank_prompt_then_real_prompt_creates_exactly_one_agent() {
    let mut host = server_host();
    let blank = json!({"agent_type": "codex", "project": "/home/me/dev/app", "prompt": " \t"});
    host.deliver_one(http_call(
        1,
        "POST",
        "/create_agent",
        blank.to_string().as_bytes(),
    ));
    host.settle();
    let real = json!({"agent_type": "codex", "project": "/home/me/dev/app", "prompt": "go"});
    host.deliver_one(http_call(
        2,
        "POST",
        "/create_agent",
        real.to_string().as_bytes(),
    ));
    host.settle();
    assert_eq!(created_panes(&host).len(), 1);
}

/// REQ: PORT:ilium-client/src/app.rs::create_agent_line_refusal_retains_exact_draft_then_retry_accepts_once
#[test]
fn app_create_agent_retry_after_refusal_creates_one_agent_not_two() {
    use ilium_plugin_sdk::types::{ErrorCode, Outcome, PluginError};
    let mut host = server_host();
    let body =
        json!({"agent_type": "codex", "project": "/home/me/dev/app", "prompt": "exact task"})
            .to_string();
    host.deliver_one(http_call(1, "POST", "/create_agent", body.as_bytes()));
    host.settle();
    for request in host.pending_requests() {
        host.complete(
            request,
            Outcome::Failed(PluginError {
                code: ErrorCode::Io,
                message: "refused".to_string(),
                retry_after_ms: None,
            }),
        );
    }
    host.settle();
    assert!(created_panes(&host).is_empty());
    host.deliver_one(http_call(2, "POST", "/create_agent", body.as_bytes()));
    host.settle();
    assert_eq!(created_panes(&host).len(), 1);
}
