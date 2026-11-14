//! The "New group" dialog: the client command `new-group` opens a popup, and the answer reaches
//! the server side as a `create-group` message that becomes a `create-group` intent.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, CommandContext, CommandInvokedEvent, CommandOrigin, CreateGroupSpec,
    Event, Intent, MessageEvent, NodeClass, NodeId, Placement, PluginId, PluginSide, Submission,
    UiAction, ViewTarget, Widget,
};

use crate::common::{add_node, client_host, server_host};

const PLUGIN: &str = "ilium.core.tree";

fn popup() -> ViewTarget {
    ViewTarget::Popup("new-group".to_string())
}

fn invoke(host: &mut FakeHost, command: &str, arguments: &str) -> Vec<Submission> {
    host.deliver_one(Event::CommandInvoked(CommandInvokedEvent {
        command: command.to_string(),
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

/// A root with a project and a group under it, and a pane in the group. Returns the ids as
/// `(root, project, group, pane)`.
fn grown(host: &mut FakeHost) -> (u64, u64, u64, u64) {
    let root = add_node(host, None, "root", NodeClass::Container);
    let project = add_node(host, Some(root), "ilium", NodeClass::Container);
    let group = add_node(host, Some(project), "work", NodeClass::Container);
    let pane = add_node(host, Some(group), "shell", NodeClass::Pane);
    (root, project, group, pane)
}

fn opened_views(submissions: &[Submission]) -> Vec<ViewTarget> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::OpenView(spec) => Some(spec.view.clone()),
            _ => None,
        })
        .collect()
}

fn closed_views(submissions: &[Submission]) -> Vec<ViewTarget> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::CloseView(view) => Some(view.clone()),
            _ => None,
        })
        .collect()
}

fn answers(submissions: &[Submission]) -> Vec<(String, Vec<u8>)> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::Message(message) if message.to.side == PluginSide::Server => {
                Some((message.channel.clone(), message.payload.clone()))
            }
            _ => None,
        })
        .collect()
}

fn parent_of(answer: &[u8]) -> (u64, String) {
    let value: serde_json::Value = serde_json::from_slice(answer).expect("answer is JSON");
    (
        value["parent"].as_u64().expect("parent"),
        value["name"].as_str().expect("name").to_string(),
    )
}

/// REQ: F-002
#[test]
fn feature_002_new_group_command_opens_the_popup() {
    let mut host = client_host();
    let (_, _, group, _) = grown(&mut host);
    let submissions = invoke(
        &mut host,
        "ilium.core.tree/new-group",
        &format!("{{\"parent\": {group}}}"),
    );
    assert_eq!(opened_views(&submissions), vec![popup()]);
}

/// REQ: F-002
#[test]
fn feature_002_popup_lists_the_top_level_and_the_groups_and_preselects_the_parent() {
    let mut host = client_host();
    let (_, _, group, _) = grown(&mut host);
    invoke(
        &mut host,
        "ilium.core.tree/new-group",
        &format!("{{\"parent\": {group}}}"),
    );
    host.render(popup(), 60, 20);
    let tree = host.last_tree(popup());
    let choice = tree
        .nodes
        .iter()
        .find_map(|node| match &node.widget {
            Widget::Choice(choice) => Some(choice.clone()),
            _ => None,
        })
        .expect("the popup has a destination list");
    assert_eq!(choice.options, ["Top level", "ilium", "  work"]);
    assert_eq!(choice.selected, 2);
}

/// REQ: F-002
#[test]
fn feature_002_create_sends_the_typed_name_and_the_chosen_parent_then_closes() {
    let mut host = client_host();
    let (_, _, group, _) = grown(&mut host);
    invoke(&mut host, "new-group", &format!("{{\"parent\": {group}}}"));
    act(&mut host, "name", "", ActionKind::Changed, text("docs"));
    let submissions = act(&mut host, "", "create", ActionKind::Activated, None);
    let sent = answers(&submissions);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "create-group");
    assert_eq!(parent_of(&sent[0].1), (group, "docs".to_string()));
    assert_eq!(closed_views(&submissions), vec![popup()]);
}

/// REQ: F-002
#[test]
fn feature_002_enter_in_the_name_field_creates_the_group() {
    let mut host = client_host();
    let (_, project, _, _) = grown(&mut host);
    invoke(
        &mut host,
        "new-group",
        &format!("{{\"parent\": {project}}}"),
    );
    let submissions = act(&mut host, "name", "", ActionKind::Submitted, text("notes"));
    let sent = answers(&submissions);
    assert_eq!(sent.len(), 1);
    assert_eq!(parent_of(&sent[0].1), (project, "notes".to_string()));
    assert_eq!(closed_views(&submissions), vec![popup()]);
}

/// REQ: F-002
#[test]
fn feature_002_a_blank_name_is_called_group() {
    let mut host = client_host();
    let (_, project, _, _) = grown(&mut host);
    invoke(
        &mut host,
        "new-group",
        &format!("{{\"parent\": {project}}}"),
    );
    act(&mut host, "name", "", ActionKind::Changed, text("   "));
    let submissions = act(&mut host, "", "create", ActionKind::Activated, None);
    assert_eq!(
        parent_of(&answers(&submissions)[0].1),
        (project, "group".to_string())
    );
}

/// REQ: F-002
#[test]
fn feature_002_choosing_another_destination_changes_the_parent() {
    let mut host = client_host();
    let (root, project, group, _) = grown(&mut host);
    invoke(&mut host, "new-group", &format!("{{\"parent\": {group}}}"));
    // The list is the top level, the project, then the group: index 0 is the root.
    act(
        &mut host,
        "destination",
        "",
        ActionKind::Changed,
        Some(ActionValue::Index(0)),
    );
    let top = act(&mut host, "", "create", ActionKind::Activated, None);
    assert_eq!(parent_of(&answers(&top)[0].1).0, root);

    invoke(&mut host, "new-group", &format!("{{\"parent\": {group}}}"));
    act(
        &mut host,
        "destination",
        "",
        ActionKind::Changed,
        Some(ActionValue::Index(1)),
    );
    let inside_project = act(&mut host, "", "create", ActionKind::Activated, None);
    assert_eq!(parent_of(&answers(&inside_project)[0].1).0, project);
}

/// REQ: F-002
#[test]
fn feature_002_cancel_closes_the_popup_and_sends_nothing() {
    let mut host = client_host();
    let (_, _, group, _) = grown(&mut host);
    invoke(&mut host, "new-group", &format!("{{\"parent\": {group}}}"));
    let submissions = act(&mut host, "", "cancel", ActionKind::Activated, None);
    assert!(answers(&submissions).is_empty());
    assert_eq!(closed_views(&submissions), vec![popup()]);
}

/// REQ: F-002
#[test]
fn feature_002_a_popup_the_kernel_closed_is_not_closed_again() {
    let mut host = client_host();
    let (_, _, group, _) = grown(&mut host);
    invoke(&mut host, "new-group", &format!("{{\"parent\": {group}}}"));
    let submissions = act(&mut host, "", "", ActionKind::Closed, None);
    assert!(submissions.is_empty());
}

/// REQ: F-002
#[test]
fn feature_002_an_action_with_no_dialog_open_does_nothing() {
    let mut host = client_host();
    grown(&mut host);
    assert!(act(&mut host, "", "create", ActionKind::Activated, None).is_empty());
}

/// REQ: F-002
#[test]
fn feature_002_after_the_dialog_ends_a_late_create_does_nothing() {
    let mut host = client_host();
    let (_, _, group, _) = grown(&mut host);
    invoke(&mut host, "new-group", &format!("{{\"parent\": {group}}}"));
    act(&mut host, "", "cancel", ActionKind::Activated, None);
    assert!(act(&mut host, "", "create", ActionKind::Activated, None).is_empty());
}

/// REQ: F-002
#[test]
fn feature_002_without_a_parent_argument_the_top_level_is_preselected() {
    let mut host = client_host();
    let (root, _, _, _) = grown(&mut host);
    for arguments in ["{}", "not json", "{\"parent\": \"7\"}"] {
        let opened = invoke(&mut host, "new-group", arguments);
        assert_eq!(opened_views(&opened), vec![popup()], "{arguments}");
        let submissions = act(&mut host, "", "create", ActionKind::Activated, None);
        assert_eq!(
            parent_of(&answers(&submissions)[0].1).0,
            root,
            "{arguments}"
        );
    }
}

/// REQ: F-002
#[test]
fn feature_002_a_stale_parent_falls_back_to_the_top_level() {
    let mut host = client_host();
    let (root, _, _, _) = grown(&mut host);
    invoke(&mut host, "new-group", "{\"parent\": 99999}");
    let submissions = act(&mut host, "", "create", ActionKind::Activated, None);
    assert_eq!(parent_of(&answers(&submissions)[0].1).0, root);
}

/// REQ: F-002
#[test]
fn feature_002_an_empty_tree_offers_nothing_and_creates_nothing() {
    let mut host = client_host();
    invoke(&mut host, "new-group", "{\"parent\": 1}");
    assert!(act(&mut host, "", "create", ActionKind::Activated, None).is_empty());
}

/// REQ: F-002
#[test]
fn feature_002_other_commands_open_nothing() {
    let mut host = client_host();
    grown(&mut host);
    assert!(invoke(&mut host, "ilium.core.tree/new-folder", "{}").is_empty());
}

fn answer_message(payload: &[u8]) -> Event {
    Event::Message(MessageEvent {
        from: PluginId(PLUGIN.to_string()),
        from_side: PluginSide::Client,
        from_client: None,
        channel: "create-group".to_string(),
        payload: payload.to_vec(),
    })
}

/// REQ: F-002
#[test]
fn feature_002_the_server_side_creates_the_named_group_last_under_the_parent() {
    let mut host = server_host();
    let submissions = host.deliver_one(answer_message(br#"{"parent": 7, "name": "docs"}"#));
    assert_eq!(submissions.len(), 1);
    assert_eq!(
        submissions[0].intent,
        Intent::CreateGroup(CreateGroupSpec {
            parent: Some(NodeId(7)),
            title: "docs".to_string(),
            placement: Placement::InsideLast,
            children: vec![],
        })
    );
}

/// REQ: F-002
#[test]
fn feature_002_the_server_side_ignores_a_malformed_answer() {
    let mut host = server_host();
    for payload in [
        &b""[..],
        b"nope",
        br#"{"parent": "7", "name": "x"}"#,
        br#"{"name": "x"}"#,
        br#"{"parent": 7}"#,
    ] {
        assert!(
            host.deliver_one(answer_message(payload)).is_empty(),
            "{}",
            String::from_utf8_lossy(payload)
        );
    }
}

/// REQ: F-002
#[test]
fn feature_002_a_blank_name_reaching_the_server_side_is_called_group() {
    let mut host = server_host();
    let submissions = host.deliver_one(answer_message(br#"{"parent": 7, "name": "  "}"#));
    let Intent::CreateGroup(spec) = &submissions[0].intent else {
        panic!("a group is created");
    };
    assert_eq!(spec.title, "group");
}
