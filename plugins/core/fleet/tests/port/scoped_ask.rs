//! Port twins of the Ask for update scope tests of ilium-client (`agent_message_dialog.rs`,
//! `app.rs`, `tree_ui.rs`). Manual: core-plugin-fleet.md "Ask for update" (the dialog, the
//! `ask-for-update` message, `send-input` with `delivery: submitted`) and api-tree-chrome.md
//! `row_actions` (`applies_to`, origin `row-action`, arguments `{"node": <id>}`).
//!
//! The manual does not say how a scope reaches the server side. These twins use the channel
//! `ask-for-update-scoped` with the JSON payload `{"node": <id>, "text": <string or absent>}`; an
//! absent text means the fixed status request (API_GAPS_scoped_ask.md).

use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, AgentActivity, AgentView, CommandContext, CommandInvokedEvent, CommandOrigin, Delivery,
    Event, Intent, NodeClass, NodeId, ViewTarget,
};
use serde_json::{Value, json};

use crate::common::*;

const STATUS_REQUEST: &str =
    "please remind me, in a very compact way, what you were doing, what I asked you to do, how it went, etc, remind me what's going on";
const CHANNEL: &str = "ask-for-update-scoped";

fn node(host: &mut FakeHost, name: &str, parent: Option<u64>, class: NodeClass, kind: Option<&str>, agent: bool) -> u64 {
    host.tree().add(TreeNode {
        id: 0,
        parent,
        name: name.to_string(),
        class,
        content_kind: kind.map(str::to_string),
        agent: agent.then(|| AgentView {
            signature: "claude".to_string(),
            activity: AgentActivity::Working,
            objective: None,
            goal: None,
        }),
    })
}

fn container(host: &mut FakeHost, name: &str, parent: Option<u64>) -> u64 {
    node(host, name, parent, NodeClass::Container, None, false)
}

fn agent_in(host: &mut FakeHost, name: &str, parent: u64, directory: &str) -> u64 {
    let id = node(host, name, Some(parent), NodeClass::Pane, Some("ilium.terminal"), true);
    host.tree().set_pane_directory(id, directory);
    id
}

fn scoped_ask(host: &mut FakeHost, scope: u64, text: Option<&str>) {
    let payload = match text {
        Some(text) => json!({"node": scope, "text": text}),
        None => json!({"node": scope}),
    };
    host.deliver_one(message(CHANNEL, payload.to_string().into_bytes()));
    host.settle();
}

fn recipients(host: &FakeHost) -> Vec<u64> {
    let mut ids: Vec<u64> = sent_panes(host).into_iter().map(|NodeId(id)| id).collect();
    ids.sort_unstable();
    ids
}

struct World {
    host: FakeHost,
    project: u64,
    group: u64,
    folder: u64,
    exact: u64,
    descendant: u64,
    prefix: u64,
}

fn world() -> World {
    let mut host = server_host();
    let project = container(&mut host, "app", None);
    let group = container(&mut host, "nested", Some(project));
    let deeper = container(&mut host, "deeper", Some(group));
    let folder = node(&mut host, "src", Some(group), NodeClass::Folder, None, false);
    host.tree().set_pane_directory(folder, "project:src");
    let other_project = container(&mut host, "other", None);
    let exact = agent_in(&mut host, "exact", project, "project:src");
    let descendant = agent_in(&mut host, "descendant", deeper, "project:src/deep");
    let prefix = agent_in(&mut host, "prefix", deeper, "project:src-extra");
    agent_in(&mut host, "outside", other_project, "project:src");
    add_shell_in(&mut host, "shell", deeper);
    node(&mut host, "editor", Some(deeper), NodeClass::Pane, Some("ilium.editor"), false);
    World { host, project, group, folder, exact, descendant, prefix }
}

fn add_shell_in(host: &mut FakeHost, name: &str, parent: u64) -> u64 {
    node(host, name, Some(parent), NodeClass::Pane, Some("ilium.terminal"), false)
}

/// REQ: PORT:ilium-client/src/agent_message_dialog.rs::scope_is_recursive_and_filesystem_membership_is_component_safe_and_project_local
#[test]
fn port_911_scope_is_recursive_and_folder_membership_is_component_safe_and_project_local() {
    let expected = |scope: fn(&World) -> u64, members: fn(&World) -> Vec<u64>| {
        let mut world = world();
        let target = scope(&world);
        scoped_ask(&mut world.host, target, Some("status"));
        let mut want = members(&world);
        want.sort_unstable();
        assert_eq!(recipients(&world.host), want);
    };
    expected(|w| w.project, |w| vec![w.descendant, w.prefix, w.exact]);
    expected(|w| w.group, |w| vec![w.descendant, w.prefix]);
    expected(|w| w.folder, |w| vec![w.descendant, w.exact]);
}

/// REQ: PORT:ilium-client/src/app.rs::agent_message_filesystem_folder_status_update_reaches_agents_within_folder_path
#[test]
fn port_912_a_folder_status_update_reaches_only_agents_within_the_folder_path() {
    let mut host = server_host();
    let project = container(&mut host, "ask-for-update-folder-scope", None);
    let group = container(&mut host, "agents", Some(project));
    let folder = node(&mut host, "src", Some(group), NodeClass::Folder, None, false);
    host.tree().set_pane_directory(folder, "project:src");
    let in_folder = agent_in(&mut host, "working-in-folder", group, "project:src/nested");
    agent_in(&mut host, "working-outside-folder", group, "project:tests");
    scoped_ask(&mut host, folder, None);
    let inputs = sent_inputs(&host);
    assert_eq!(inputs.len(), 1, "{inputs:?}");
    assert_eq!(inputs[0].pane, NodeId(in_folder));
    assert_eq!(inputs[0].text, STATUS_REQUEST);
    assert_eq!(inputs[0].delivery, Delivery::Submitted);
}

/// REQ: PORT:ilium-client/src/agent_message_dialog.rs::enter_toggles_focused_recipient_and_submit_option
#[test]
fn port_913_the_ask_presses_enter_for_every_recipient() {
    let mut world = world();
    let project = world.project;
    scoped_ask(&mut world.host, project, Some("status"));
    let inputs = sent_inputs(&world.host);
    assert_eq!(inputs.len(), 3, "{inputs:?}");
    assert_eq!(inputs.iter().filter(|spec| spec.delivery == Delivery::Submitted).count(), 3);
}

/// REQ: PORT:ilium-client/src/app.rs::agent_message_dialog_marks_moved_snapshot_recipient_unavailable
#[test]
fn port_914_an_agent_moved_out_of_the_scope_before_the_send_is_not_messaged() {
    let mut host = server_host();
    let project = container(&mut host, "message-moved-recipient", None);
    let group = container(&mut host, "selected agents", Some(project));
    let outside_group = container(&mut host, "other agents", Some(project));
    let retained = agent_in(&mut host, "retained", group, "project:.");
    let moved = agent_in(&mut host, "moved", group, "project:.");
    host.tree().move_node(moved, outside_group);
    host.settle();
    scoped_ask(&mut host, group, Some("deliver only to the retained agent"));
    let inputs = sent_inputs(&host);
    assert_eq!(inputs.len(), 1, "{inputs:?}");
    assert_eq!(inputs[0].pane, NodeId(retained));
    assert_eq!(inputs[0].text, "deliver only to the retained agent");
}

fn open_on_row(node_id: u64) -> FakeHost {
    let mut host = client_host();
    host.deliver_one(Event::CommandInvoked(CommandInvokedEvent {
        command: "ask-for-update".to_string(),
        arguments_json: json!({"node": node_id}).to_string(),
        origin: CommandOrigin::RowAction,
        context: CommandContext::default(),
    }));
    host.settle();
    host
}

fn scoped_messages(host: &FakeHost) -> Vec<Value> {
    host.submitted()
        .into_iter()
        .filter_map(|submission| match submission.intent {
            Intent::Message(message) if message.channel == CHANNEL => {
                Some(serde_json::from_slice(&message.payload).expect("JSON payload"))
            }
            _ => None,
        })
        .collect()
}

fn send_activation() -> Event {
    dialog_action("update-send", ActionKind::Activated, Some(ActionValue::None))
}

/// REQ: PORT:ilium-client/src/app.rs::folder_agent_message_menu_offers_status_and_send_to_all
#[test]
fn port_915_a_row_action_on_a_folder_opens_the_dialog_for_that_node() {
    let mut host = open_on_row(7);
    let popup = ViewTarget::Popup("ask-for-update".to_string());
    let text = host.render(popup, 80, 24).text();
    assert!(text.contains("Ask for update"), "{text}");
    assert!(text.contains("Message"), "{text}");
    host.deliver_one(dialog_action("update-text", ActionKind::Changed, Some(ActionValue::Text("hello".to_string()))));
    host.deliver_one(send_activation());
    host.settle();
    assert_eq!(scoped_messages(&host), vec![json!({"node": 7, "text": "hello"})]);
}

/// REQ: PORT:ilium-client/src/agent_message_dialog.rs::queued_and_unavailable_rows_cannot_be_reselected
#[test]
fn port_916_a_second_send_after_the_message_was_queued_sends_nothing_more() {
    let mut host = open_on_row(7);
    host.deliver_one(dialog_action("update-text", ActionKind::Changed, Some(ActionValue::Text("once".to_string()))));
    host.deliver_one(send_activation());
    host.settle();
    host.deliver_one(send_activation());
    host.settle();
    assert_eq!(scoped_messages(&host), vec![json!({"node": 7, "text": "once"})]);
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::ask_for_update_row_action_is_offered_only_for_project_rows
#[test]
fn port_917_the_ask_for_update_row_action_applies_to_project_rows_only() {
    let manifest: Value = serde_json::from_str(&std::fs::read_to_string(manifest_path()).expect("manifest")).expect("manifest JSON");
    let actions = manifest["contributes"]["row_actions"].as_array().cloned().unwrap_or_default();
    let ask: Vec<&Value> = actions.iter().filter(|action| action["command"] == "ask-for-update").collect();
    assert_eq!(ask.len(), 1, "{actions:?}");
    assert_eq!(ask[0]["applies_to"], json!(["project"]));
}
