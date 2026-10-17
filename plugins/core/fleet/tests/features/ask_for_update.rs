//! "Ask for update" (F-036, F-113, F-023): the client dialog and the server fan-out of one fixed
//! or typed message to the agents. Dialog shape from the page figure; payload channel
//! "ask-for-update" is an assumption listed in API_GAPS.md.

use ilium_plugin_sdk::testing::{assert_golden, FakeHost};
use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, AgentActivity, Delivery, Event, Intent, ViewTarget,
};

use crate::common::*;

const FIXED_PROMPT: &str = "please remind me, in a very compact way, what you were doing, what I asked you to do, how it went, etc, remind me what's going on";

fn popup() -> ViewTarget {
    ViewTarget::Popup("ask-for-update".to_string())
}

fn command_event() -> Event {
    Event::CommandInvoked(ilium_plugin_sdk::types::CommandInvokedEvent {
        command: "ask-for-update".to_string(),
        arguments_json: "{}".to_string(),
        origin: ilium_plugin_sdk::types::CommandOrigin::Palette,
        context: ilium_plugin_sdk::types::CommandContext::default(),
    })
}

fn open_dialog() -> FakeHost {
    let mut host = client_host();
    host.deliver_one(command_event());
    host.settle();
    host
}

fn three_agents() -> FakeHost {
    let mut host = server_host();
    add_agent(
        &mut host,
        "refactor parser",
        "claude",
        AgentActivity::Working,
    );
    add_agent(&mut host, "tests", "codex", AgentActivity::Idle);
    add_agent(&mut host, "docs", "claude", AgentActivity::Idle);
    host
}

/// REQ: F-036
#[test]
fn ask_036_the_command_opens_the_ask_for_update_dialog() {
    let mut host = open_dialog();
    let screen = host.render(popup(), 80, 24);
    assert!(screen.text().contains("Ask for update"));
}

/// REQ: F-036
#[test]
fn ask_036_the_dialog_has_a_message_field_and_send_and_cancel_buttons() {
    let mut host = open_dialog();
    let text = host.render(popup(), 80, 24).text();
    for label in ["Message", "Send", "Cancel"] {
        assert!(text.contains(label), "{label} missing in\n{text}");
    }
}

/// REQ: F-036
#[test]
fn ask_036_the_dialog_matches_the_page_figure_at_48_columns() {
    let mut host = open_dialog();
    let text = host.render(popup(), 48, 6).text();
    assert_golden(&golden_path("ask_for_update_48.txt"), &text);
}

/// REQ: F-036
#[test]
fn ask_036_the_dialog_fits_every_documented_width() {
    for columns in [40u16, 80, 120] {
        let mut host = open_dialog();
        let text = host.render(popup(), columns, 24).text();
        for line in text.lines() {
            assert!(
                line.chars().count() <= usize::from(columns),
                "{columns}: {line:?}"
            );
        }
        assert!(
            text.contains("Send") && text.contains("Cancel"),
            "{columns}"
        );
    }
}

/// REQ: F-036
#[test]
fn ask_036_send_does_nothing_while_the_text_is_empty() {
    let mut host = open_dialog();
    host.deliver_one(dialog_action(
        "update-send",
        ActionKind::Activated,
        Some(ActionValue::None),
    ));
    host.settle();
    assert!(host
        .submitted()
        .iter()
        .all(|s| !matches!(s.intent, Intent::Message(_))));
}

/// REQ: F-036
#[test]
fn ask_036_send_does_nothing_while_the_text_is_only_whitespace() {
    let mut host = open_dialog();
    host.deliver_one(dialog_action(
        "update-text",
        ActionKind::Changed,
        Some(ActionValue::Text("  \n ".to_string())),
    ));
    host.deliver_one(dialog_action(
        "update-send",
        ActionKind::Activated,
        Some(ActionValue::None),
    ));
    host.settle();
    assert!(host
        .submitted()
        .iter()
        .all(|s| !matches!(s.intent, Intent::Message(_))));
}

/// REQ: F-036
#[test]
fn ask_036_send_submits_one_message_to_the_server_side_of_the_plugin() {
    let mut host = open_dialog();
    host.deliver_one(dialog_action(
        "update-text",
        ActionKind::Changed,
        Some(ActionValue::Text("status?".to_string())),
    ));
    host.deliver_one(dialog_action(
        "update-send",
        ActionKind::Activated,
        Some(ActionValue::None),
    ));
    host.settle();
    let messages: Vec<_> = host
        .submitted()
        .into_iter()
        .filter_map(|s| {
            if let Intent::Message(m) = s.intent {
                Some(m)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].channel, "ask-for-update");
    assert_eq!(messages[0].payload, b"status?".to_vec());
}

/// REQ: F-036
#[test]
fn ask_036_cancel_sends_nothing() {
    let mut host = open_dialog();
    host.deliver_one(dialog_action(
        "update-text",
        ActionKind::Changed,
        Some(ActionValue::Text("status?".to_string())),
    ));
    host.deliver_one(dialog_action(
        "update-cancel",
        ActionKind::Activated,
        Some(ActionValue::None),
    ));
    host.settle();
    assert!(host
        .submitted()
        .iter()
        .all(|s| !matches!(s.intent, Intent::Message(_))));
}

/// REQ: F-036
#[test]
fn ask_036_the_server_sends_the_text_to_the_idle_agents() {
    let mut host = three_agents();
    host.deliver_one(message("ask-for-update", b"status?".to_vec()));
    host.settle();
    let inputs = sent_inputs(&host);
    assert!(!inputs.is_empty());
    assert!(inputs
        .iter()
        .all(|spec| spec.text == "status?" && spec.delivery == Delivery::Submitted));
}

/// REQ: F-036
#[test]
fn ask_036_a_shell_never_receives_the_text() {
    let mut host = three_agents();
    let shell = add_shell(&mut host, "shell");
    host.deliver_one(message("ask-for-update", b"status?".to_vec()));
    host.settle();
    assert!(!sent_panes(&host).contains(&ilium_plugin_sdk::types::NodeId(shell)));
}

/// REQ: F-036
#[test]
fn ask_036_an_empty_payload_sends_nothing() {
    let mut host = three_agents();
    host.deliver_one(message("ask-for-update", Vec::new()));
    host.settle();
    assert!(sent_inputs(&host).is_empty());
}

/// REQ: F-113
#[test]
fn ask_113_the_fixed_prompt_text_is_on_the_page() {
    assert!(page_text().contains(FIXED_PROMPT));
}

/// REQ: F-113
#[test]
fn ask_113_the_fixed_prompt_asks_every_idle_agent() {
    let mut host = three_agents();
    host.deliver_one(message("ask-for-update", FIXED_PROMPT.as_bytes().to_vec()));
    host.settle();
    assert!(sent_inputs(&host)
        .iter()
        .all(|spec| spec.text == FIXED_PROMPT));
}

/// REQ: F-113
#[test]
fn ask_113_a_multi_line_text_is_delivered_pasted() {
    let mut host = three_agents();
    host.deliver_one(message("ask-for-update", b"one\ntwo".to_vec()));
    host.settle();
    let inputs = sent_inputs(&host);
    assert!(!inputs.is_empty());
    assert!(inputs.iter().all(|spec| spec.delivery == Delivery::Pasted));
}

/// REQ: F-023
#[test]
fn ask_023_a_message_from_another_plugin_is_accepted_on_the_same_channel() {
    let mut host = three_agents();
    host.deliver_one(ilium_plugin_sdk::types::Event::Message(
        ilium_plugin_sdk::types::MessageEvent {
            from: ilium_plugin_sdk::types::PluginId("ilium.core.editor".to_string()),
            from_side: ilium_plugin_sdk::types::PluginSide::Server,
            from_client: None,
            channel: "ask-for-update".to_string(),
            payload: b"status?".to_vec(),
        },
    ));
    host.settle();
    assert!(!sent_inputs(&host).is_empty());
}

/// REQ: F-023
#[test]
fn ask_023_the_notifications_hand_off_goes_to_the_notifications_plugin() {
    let mut host = three_agents();
    host.deliver_one(message("agent-question", b"tests: Which branch?".to_vec()));
    host.settle();
    let sent: Vec<_> = host
        .submitted()
        .into_iter()
        .filter_map(|s| {
            if let Intent::Message(m) = s.intent {
                Some(m)
            } else {
                None
            }
        })
        .collect();
    assert!(sent.iter().all(|m| m.channel == "agent-question"));
}
