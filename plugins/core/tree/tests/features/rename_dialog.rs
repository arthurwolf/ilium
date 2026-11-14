//! The "Rename" dialog: the client command `rename` opens a popup holding the node's name, and
//! the answer reaches the server side as a `rename-node` message that becomes a `rename-node`
//! intent. A worktree row is refused with a toast, a row that is not a node does nothing.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, CommandContext, CommandInvokedEvent, CommandOrigin, ContainerKind,
    Event, Intent, MessageEvent, NodeClass, NodeId, PluginId, PluginSide, RenameNodeSpec,
    Submission, TitleSource, UiAction, ViewTarget, Widget,
};

use crate::common::{add_node, client_host, server_host};

const PLUGIN: &str = "ilium.core.tree";

fn popup() -> ViewTarget {
    ViewTarget::Popup("rename".to_string())
}

fn invoke(host: &mut FakeHost, arguments: &str) -> Vec<Submission> {
    host.deliver_one(Event::CommandInvoked(CommandInvokedEvent {
        command: "ilium.core.tree/rename".to_string(),
        arguments_json: arguments.to_string(),
        origin: CommandOrigin::Keybinding,
        context: CommandContext::default(),
    }))
}

fn act(
    host: &mut FakeHost,
    key: &str,
    action_id: &str,
    kind: ActionKind,
    value: Option<ActionValue>,
) -> Vec<Submission> {
    host.deliver_one(Event::UiAction(UiAction {
        view: popup(),
        key: key.to_string(),
        action_id: action_id.to_string(),
        kind,
        value,
    }))
}

fn text(value: &str) -> Option<ActionValue> {
    Some(ActionValue::Text(value.to_string()))
}

fn opened(submissions: &[Submission]) -> usize {
    submissions
        .iter()
        .filter(|submission| matches!(&submission.intent, Intent::OpenView(spec) if spec.view == popup()))
        .count()
}

fn closed(submissions: &[Submission]) -> usize {
    submissions
        .iter()
        .filter(
            |submission| matches!(&submission.intent, Intent::CloseView(view) if *view == popup()),
        )
        .count()
}

fn toasts(submissions: &[Submission]) -> Vec<String> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::Toast(spec) => Some(spec.title.clone()),
            _ => None,
        })
        .collect()
}

fn answers(submissions: &[Submission]) -> Vec<(String, serde_json::Value)> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::Message(message) if message.to.side == PluginSide::Server => Some((
                message.channel.clone(),
                serde_json::from_slice(&message.payload).expect("answer is JSON"),
            )),
            _ => None,
        })
        .collect()
}

/// A group holding one pane; returns `(group, pane)`.
fn grown(host: &mut FakeHost) -> (u64, u64) {
    let group = add_node(host, None, "work", NodeClass::Container);
    let pane = add_node(host, Some(group), "shell", NodeClass::Pane);
    (group, pane)
}

/// REQ: F-004
#[test]
fn feature_004_rename_command_opens_the_popup_for_the_node() {
    let mut host = client_host();
    let (_, pane) = grown(&mut host);
    let submissions = invoke(&mut host, &format!("{{\"node\": {pane}}}"));
    assert_eq!(opened(&submissions), 1);
    assert!(toasts(&submissions).is_empty());
}

/// REQ: F-004
#[test]
fn feature_004_popup_holds_the_current_name() {
    let mut host = client_host();
    let (_, pane) = grown(&mut host);
    invoke(&mut host, &format!("{{\"node\": {pane}}}"));
    host.render(popup(), 60, 20);
    let tree = host.last_tree(popup());
    let initial = tree
        .nodes
        .iter()
        .find_map(|node| match &node.widget {
            Widget::TextInput(input) => Some(input.initial.clone()),
            _ => None,
        })
        .expect("the popup has a name input");
    assert_eq!(initial, "shell");
}

/// REQ: F-004
#[test]
fn feature_004_rename_sends_the_typed_name_and_closes() {
    let mut host = client_host();
    let (_, pane) = grown(&mut host);
    invoke(&mut host, &format!("{{\"node\": {pane}}}"));
    act(&mut host, "name", "", ActionKind::Changed, text("build"));
    let submissions = act(&mut host, "", "rename", ActionKind::Activated, None);
    let sent = answers(&submissions);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "rename-node");
    assert_eq!(sent[0].1["node"].as_u64(), Some(pane));
    assert_eq!(sent[0].1["title"].as_str(), Some("build"));
    assert_eq!(closed(&submissions), 1);
}

/// REQ: F-004
#[test]
fn feature_004_enter_in_the_name_field_renames() {
    let mut host = client_host();
    let (group, _) = grown(&mut host);
    invoke(&mut host, &format!("{{\"node\": {group}}}"));
    let submissions = act(&mut host, "name", "", ActionKind::Submitted, text("docs"));
    let sent = answers(&submissions);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].1["node"].as_u64(), Some(group));
    assert_eq!(sent[0].1["title"].as_str(), Some("docs"));
    assert_eq!(closed(&submissions), 1);
}

/// REQ: F-004
#[test]
fn feature_004_a_blank_name_is_sent_as_typed() {
    let mut host = client_host();
    let (_, pane) = grown(&mut host);
    invoke(&mut host, &format!("{{\"node\": {pane}}}"));
    act(&mut host, "name", "", ActionKind::Changed, text(""));
    let submissions = act(&mut host, "", "rename", ActionKind::Activated, None);
    assert_eq!(answers(&submissions)[0].1["title"].as_str(), Some(""));
}

/// REQ: F-004
#[test]
fn feature_004_cancel_closes_the_popup_and_sends_nothing() {
    let mut host = client_host();
    let (_, pane) = grown(&mut host);
    invoke(&mut host, &format!("{{\"node\": {pane}}}"));
    let submissions = act(&mut host, "", "cancel", ActionKind::Activated, None);
    assert!(answers(&submissions).is_empty());
    assert_eq!(closed(&submissions), 1);
}

/// REQ: F-004
#[test]
fn feature_004_a_popup_the_kernel_closed_is_not_closed_again() {
    let mut host = client_host();
    let (_, pane) = grown(&mut host);
    invoke(&mut host, &format!("{{\"node\": {pane}}}"));
    assert!(act(&mut host, "", "", ActionKind::Closed, None).is_empty());
}

/// REQ: F-004
#[test]
fn feature_004_after_the_dialog_ends_a_late_rename_does_nothing() {
    let mut host = client_host();
    let (_, pane) = grown(&mut host);
    invoke(&mut host, &format!("{{\"node\": {pane}}}"));
    act(&mut host, "", "cancel", ActionKind::Activated, None);
    assert!(act(&mut host, "", "rename", ActionKind::Activated, None).is_empty());
}

/// REQ: F-004
#[test]
fn feature_004_a_worktree_row_is_refused_with_a_toast() {
    let mut host = client_host();
    let (group, _) = grown(&mut host);
    host.tree().set_container(group, ContainerKind::Worktree);
    let submissions = invoke(&mut host, &format!("{{\"node\": {group}}}"));
    assert_eq!(opened(&submissions), 0);
    assert_eq!(
        toasts(&submissions),
        vec!["The worktree row uses its branch name".to_string()]
    );
    // The refused rename leaves no dialog behind.
    assert!(act(&mut host, "", "rename", ActionKind::Activated, None).is_empty());
}

/// REQ: F-004
#[test]
fn feature_004_a_row_that_is_not_a_node_does_nothing() {
    let mut host = client_host();
    grown(&mut host);
    for arguments in ["{}", "not json", "{\"node\": \"7\"}", "{\"node\": 99999}"] {
        assert!(invoke(&mut host, arguments).is_empty(), "{arguments}");
    }
}

fn answer_message(payload: &[u8]) -> Event {
    Event::Message(MessageEvent {
        from: PluginId(PLUGIN.to_string()),
        from_side: PluginSide::Client,
        from_client: None,
        channel: "rename-node".to_string(),
        payload: payload.to_vec(),
    })
}

/// REQ: F-004
#[test]
fn feature_004_the_server_side_renames_the_node_as_the_plugin() {
    let mut host = server_host();
    let submissions = host.deliver_one(answer_message(br#"{"node": 7, "title": "docs"}"#));
    assert_eq!(submissions.len(), 1);
    assert_eq!(
        submissions[0].intent,
        Intent::RenameNode(RenameNodeSpec {
            node: NodeId(7),
            title: "docs".to_string(),
            source: TitleSource::Plugin,
            expected_revision: None,
        })
    );
}

/// REQ: F-004
#[test]
fn feature_004_the_server_side_ignores_a_malformed_answer() {
    let mut host = server_host();
    for payload in [
        &b""[..],
        b"nope",
        br#"{"node": "7", "title": "x"}"#,
        br#"{"title": "x"}"#,
        br#"{"node": 7}"#,
        br#"{"node": 7, "title": 5}"#,
    ] {
        assert!(
            host.deliver_one(answer_message(payload)).is_empty(),
            "{}",
            String::from_utf8_lossy(payload)
        );
    }
}
