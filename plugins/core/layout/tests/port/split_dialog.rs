//! The "New split view" dialog, moved here from the kernel client. Each test names the kernel
//! test it replaces (`PORT:<file>::<test>`); the kernel copies are deleted.

use ilium_plugin_core_layout::new_split::{self, Answer, Dialog, Page, Step, TreeFact};
use ilium_plugin_core_layout::LayoutClient;
use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, AgentActivity, AgentView, CommandContext, CommandInvokedEvent,
    CommandOrigin, ContainerKind, Event, Intent, NodeClass, NodeId, NodeView, SplitDirection,
    Submission, TriState, UiAction, Widget,
};
use serde_json::json;

use crate::common::*;

const GROUP: u64 = 10;

fn fact(id: u64, parent: u64, kind: &str, title: &str) -> TreeFact {
    TreeFact {
        id,
        parent: Some(parent),
        kind: kind.to_owned(),
        content: (kind == "pane").then(|| "terminal".to_owned()),
        agent: None,
        title: title.to_owned(),
    }
}

/// The tree as facts: root 0 > project 1 > group 10 > panes, plus a split view 20 holding
/// pane 21 and a worktree 30 holding pane 31. Returns the facts and the panes in tree order.
fn facts(free_panes: &[u64]) -> (Vec<TreeFact>, Vec<u64>) {
    let mut nodes = vec![
        fact(1, 0, "project", "ilium"),
        fact(GROUP, 1, "group", "work"),
        fact(20, GROUP, "split", "Vertical split"),
        fact(21, 20, "pane", "held"),
        fact(30, 1, "worktree", "feature"),
        fact(31, 30, "pane", "tree"),
    ];
    let mut panes: Vec<u64> = free_panes.to_vec();
    for pane in free_panes {
        nodes.push(TreeFact {
            content: Some("editor".to_owned()),
            ..fact(*pane, GROUP, "pane", &format!("pane {pane}"))
        });
    }
    panes.extend([21, 31]);
    (nodes, panes)
}

/// The same tree in the fake host, which the plugin reads through the tree import.
fn grow_tree(host: &mut FakeHost, free_panes: &[u64]) {
    let container = |id: u64, parent: Option<u64>, name: &str| TreeNode {
        id,
        parent,
        name: name.to_owned(),
        class: NodeClass::Container,
        content_kind: None,
        agent: None,
    };
    let pane = |id: u64, parent: u64, name: &str, content: &str| TreeNode {
        id,
        parent: Some(parent),
        name: name.to_owned(),
        class: NodeClass::Pane,
        content_kind: Some(content.to_owned()),
        agent: None,
    };
    let tree = host.tree();
    tree.add_with_id(container(0, None, "root"));
    tree.add_with_id(container(1, Some(0), "ilium"));
    tree.set_container(1, ContainerKind::Project);
    tree.add_with_id(container(GROUP, Some(1), "work"));
    tree.add_with_id(container(20, Some(GROUP), "Vertical split"));
    tree.set_container(20, ContainerKind::SplitVertical);
    tree.add_with_id(pane(21, 20, "held", "ilium.terminal"));
    tree.add_with_id(container(30, Some(1), "feature"));
    tree.set_container(30, ContainerKind::Worktree);
    tree.add_with_id(pane(31, 30, "tree", "ilium.terminal"));
    for id in free_panes {
        tree.add_with_id(pane(*id, GROUP, &format!("pane {id}"), "ilium.editor"));
    }
}

fn parent_arguments() -> String {
    json!({ "parent": GROUP }).to_string()
}

fn node_view(class: NodeClass, container: Option<ContainerKind>) -> NodeView {
    NodeView {
        id: NodeId(7),
        parent: Some(NodeId(1)),
        class,
        content_kind: None,
        title: "node".to_owned(),
        cwd: None,
        agent: None,
        locked: false,
        revision: 0,
        container,
    }
}

fn dialog(free_panes: &[u64]) -> Dialog {
    let (nodes, panes) = facts(free_panes);
    Dialog::from_facts(GROUP, 0, &nodes, &panes)
}

fn activate(dialog: &mut Dialog, button: &str) -> Step {
    dialog.act("dialog", button, ActionKind::Activated, None)
}

fn toggle(dialog: &mut Dialog, pane: u64) -> Step {
    dialog.act(
        new_split::MEMBERS_KEY,
        "",
        ActionKind::Toggled,
        Some(ActionValue::ItemState((pane.to_string(), TriState::On))),
    )
}

fn activated_client() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client");
    host.activate::<LayoutClient>();
    // The first batch carries the padding table to the kernel; deliver it before the dialog tests.
    host.deliver_one(Event::NodeRemoved(NodeId(u64::MAX)));
    host
}

fn new_split_command(arguments_json: String) -> Event {
    Event::CommandInvoked(CommandInvokedEvent {
        command: "new-split".to_owned(),
        arguments_json,
        origin: CommandOrigin::Keybinding,
        context: CommandContext::default(),
    })
}

fn popup_action(key: &str, action_id: &str, kind: ActionKind, value: Option<ActionValue>) -> Event {
    Event::UiAction(UiAction {
        view: new_split::view(),
        key: key.to_owned(),
        action_id: action_id.to_owned(),
        kind,
        value,
    })
}

fn messages_to_server(submissions: &[Submission]) -> Vec<Answer> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::Message(message) if message.channel == "new-split" => {
                Answer::from_payload(&message.payload)
            }
            _ => None,
        })
        .collect()
}

/// REQ: PORT:ilium-client/src/app.rs::split_creation_choices_exclude_panes_already_in_a_split
#[test]
fn port_002_choices_exclude_panes_already_in_a_split_and_panes_of_a_worktree() {
    let dialog = dialog(&[11, 12]);
    let offered: Vec<u64> = dialog.choices.iter().map(|choice| choice.pane).collect();
    assert_eq!(offered, [11, 12]);
    assert_eq!(dialog.choices[0].label, "[editor] ilium / work / pane 11");
}

/// REQ: PORT:ilium-client/src/app.rs::split_creation_choices_exclude_panes_already_in_a_split
#[test]
fn port_002_an_agent_pane_is_labelled_as_an_agent() {
    let nodes = [
        fact(1, 0, "project", "ilium"),
        TreeFact {
            agent: Some("agent".to_owned()),
            ..fact(2, 1, "pane", "claude")
        },
    ];
    let dialog = Dialog::from_facts(1, 0, &nodes, &[2]);
    assert_eq!(dialog.choices[0].label, "[agent] ilium / claude");
}

/// REQ: F-003
#[test]
fn a_node_view_becomes_the_fact_the_rules_use() {
    let kind = |class, container| TreeFact::from_view(&node_view(class, container)).kind;
    assert_eq!(kind(NodeClass::Pane, None), "pane");
    assert_eq!(kind(NodeClass::Folder, None), "folder");
    assert_eq!(
        kind(NodeClass::Container, Some(ContainerKind::Project)),
        "project"
    );
    assert_eq!(
        kind(NodeClass::Container, Some(ContainerKind::Worktree)),
        "worktree"
    );
    assert_eq!(
        kind(NodeClass::Container, Some(ContainerKind::SplitVertical)),
        "split"
    );
    assert_eq!(
        kind(NodeClass::Container, Some(ContainerKind::Group)),
        "group"
    );

    let agent = |activity| {
        let mut view = node_view(NodeClass::Pane, None);
        view.content_kind = Some("ilium.terminal".to_owned());
        view.agent = Some(AgentView {
            signature: "claude".to_owned(),
            activity,
            objective: None,
            goal: None,
        });
        TreeFact::from_view(&view)
    };
    assert_eq!(agent(AgentActivity::Idle).agent.as_deref(), Some("agent"));
    assert_eq!(
        agent(AgentActivity::None).agent.as_deref(),
        Some("agent unavailable")
    );
    assert_eq!(
        agent(AgentActivity::Idle).content.as_deref(),
        Some("terminal")
    );
}

/// REQ: F-003
#[test]
fn the_fake_host_renders_the_open_dialog_and_keeps_its_tree() {
    let mut host = activated_client();
    grow_tree(&mut host, &[11]);
    host.settle();
    assert!(host.last_tree(new_split::view()).nodes.is_empty());
    host.deliver_one(new_split_command(parent_arguments()));
    host.render(new_split::view(), 60, 20);
    let tree = host.last_tree(new_split::view());
    assert!(tree
        .nodes
        .iter()
        .any(|node| matches!(node.widget, Widget::Dialog(_))));
    // The first row is the dialog title, which takes no click.
    assert!(!host.click(new_split::view(), 0, 0));
}

/// REQ: F-003
#[test]
fn the_command_builds_the_choices_from_the_tree_the_plugin_reads() {
    let mut host = activated_client();
    grow_tree(&mut host, &[11, 12]);
    host.settle();
    host.deliver_one(new_split_command(parent_arguments()));
    host.deliver_one(popup_action(
        "dialog",
        "choose",
        ActionKind::Activated,
        None,
    ));
    host.deliver_one(popup_action(
        new_split::MEMBERS_KEY,
        "",
        ActionKind::Toggled,
        Some(ActionValue::ItemState(("12".to_owned(), TriState::On))),
    ));
    // Pane 21 sits in a split view, so the plugin never offered it and ignores the toggle.
    host.deliver_one(popup_action(
        new_split::MEMBERS_KEY,
        "",
        ActionKind::Toggled,
        Some(ActionValue::ItemState(("21".to_owned(), TriState::On))),
    ));
    let ended = host.deliver_one(popup_action(
        "dialog",
        "create",
        ActionKind::Activated,
        None,
    ));
    assert_eq!(
        messages_to_server(&ended),
        [Answer {
            parent: GROUP,
            direction: SplitDirection::Vertical,
            panes: vec![12],
        }]
    );
}

/// REQ: PORT:ilium-client/src/keys.rs::split_orientation_dialog_can_skip_the_optional_member_picker
#[test]
fn port_001_the_orientation_step_can_skip_the_optional_member_picker() {
    let mut dialog = dialog(&[11]);
    dialog.act(
        new_split::ORIENTATION_KEY,
        "",
        ActionKind::Changed,
        Some(ActionValue::Index(1)),
    );
    assert_eq!(
        activate(&mut dialog, "empty"),
        Step::Create(Answer {
            parent: GROUP,
            direction: SplitDirection::Horizontal,
            panes: Vec::new(),
        })
    );
}

/// REQ: PORT:ilium-client/src/app.rs::skipping_the_member_picker_queues_an_empty_split_directly
#[test]
fn port_003_skipping_the_member_picker_sends_one_empty_split_to_the_server_side() {
    let mut host = activated_client();
    grow_tree(&mut host, &[11]);
    host.settle();
    let opened = host.deliver_one(new_split_command(parent_arguments()));
    assert!(opened
        .iter()
        .any(|submission| matches!(&submission.intent, Intent::OpenView(spec) if spec.view == new_split::view())));
    let ended = host.deliver_one(popup_action("dialog", "empty", ActionKind::Activated, None));
    assert_eq!(
        messages_to_server(&ended),
        [Answer {
            parent: GROUP,
            direction: SplitDirection::Vertical,
            panes: Vec::new(),
        }]
    );
    assert!(ended
        .iter()
        .any(|submission| matches!(&submission.intent, Intent::CloseView(view) if *view == new_split::view())));
    // The dialog is gone: a late action does nothing.
    let late = host.deliver_one(popup_action("dialog", "empty", ActionKind::Activated, None));
    assert!(late.is_empty());
}

/// REQ: PORT:ilium-client/src/app.rs::committing_an_empty_split_queues_one_atomic_request
#[test]
fn port_004_creating_with_no_pane_picked_asks_for_one_empty_split() {
    let mut dialog = dialog(&[11, 12]);
    assert_eq!(activate(&mut dialog, "choose"), Step::Stay);
    assert_eq!(dialog.page, Page::Members);
    assert_eq!(
        activate(&mut dialog, "create"),
        Step::Create(Answer {
            parent: GROUP,
            direction: SplitDirection::Vertical,
            panes: Vec::new(),
        })
    );
}

/// REQ: PORT:ilium-client/src/app.rs::committing_an_empty_split_queues_one_atomic_request
#[test]
fn port_004_picked_panes_are_sent_in_tree_order_and_a_fifth_is_refused() {
    let mut dialog = dialog(&[11, 12, 13, 14, 15]);
    activate(&mut dialog, "choose");
    for pane in [14, 11, 13, 12] {
        toggle(&mut dialog, pane);
    }
    assert_eq!(dialog.notice, None);
    toggle(&mut dialog, 15);
    assert_eq!(dialog.notice.as_deref(), Some(new_split::FULL_NOTICE));
    assert_eq!(dialog.selected(), [11, 12, 13, 14]);
    let Step::Create(answer) = activate(&mut dialog, "create") else {
        panic!("create ends the dialog");
    };
    assert_eq!(answer.panes, [11, 12, 13, 14]);
}

/// REQ: PORT:ilium-client/src/mouse.rs::clicking_inside_the_popup_on_the_vertical_row_advances_to_the_member_picker
#[test]
fn port_005_choosing_panes_shows_the_member_picker_with_one_row_per_eligible_pane() {
    let mut dialog = dialog(&[11, 12]);
    let first = dialog.render();
    assert!(matches!(first.nodes[1].widget, Widget::Choice(_)));
    activate(&mut dialog, "choose");
    let second = dialog.render();
    let Widget::CheckList(list) = &second.nodes[1].widget else {
        panic!("the member page is a check list");
    };
    assert_eq!(list.items.len(), 2);
    let Widget::Dialog(spec) = &second.nodes[0].widget else {
        panic!("the root is a dialog");
    };
    assert_eq!(spec.title[0].text, "Add panes to split");
}

/// REQ: PORT:ilium-client/src/ui.rs::split_orientation_dialog_shows_complete_cancel_instruction_at_demo_size
#[test]
fn port_006_both_pages_offer_a_cancel_button_and_cancel_creates_nothing() {
    let mut dialog = dialog(&[11]);
    for _ in 0..2 {
        let tree = dialog.render();
        let Widget::Dialog(spec) = &tree.nodes[0].widget else {
            panic!("the root is a dialog");
        };
        assert!(spec.buttons.iter().any(|button| button.is_cancel));
        activate(&mut dialog, "choose");
    }
    assert_eq!(activate(&mut dialog, "cancel"), Step::Cancel);
    assert_eq!(
        dialog.act("", "", ActionKind::Closed, None),
        Step::Cancel,
        "the kernel closing the popup cancels too"
    );
}

/// REQ: PORT:ilium-client/src/app.rs::committing_an_empty_split_queues_one_atomic_request
#[test]
fn port_004_the_server_side_turns_the_answer_into_one_create_split_under_the_group() {
    let mut host = activated_server();
    let answer = Answer {
        parent: GROUP,
        direction: SplitDirection::Horizontal,
        panes: vec![11, 12],
    };
    host.deliver_one(message_from_client("new-split", answer.to_payload()));
    host.settle();
    host.settle();
    let splits = create_splits(&host.submitted());
    assert_eq!(splits.len(), 1);
    assert_eq!(splits[0].direction, SplitDirection::Horizontal);
    assert_eq!(splits[0].parent.map(|parent| parent.0), Some(GROUP));
    let panes: Vec<u64> = splits[0].panes.iter().map(|pane| pane.0).collect();
    assert_eq!(panes, [11, 12]);
}

/// REQ: F-003
#[test]
fn a_malformed_or_oversized_answer_creates_nothing() {
    let mut host = activated_server();
    host.deliver_one(message_from_client("new-split", b"not json".to_vec()));
    let five = json!({"parent": GROUP, "direction": "vertical", "panes": [1, 2, 3, 4, 5]});
    host.deliver_one(message_from_client(
        "new-split",
        five.to_string().into_bytes(),
    ));
    host.settle();
    assert!(create_splits(&host.submitted()).is_empty());
}

/// REQ: F-003
#[test]
fn arguments_without_a_parent_open_no_dialog() {
    let mut host = activated_client();
    assert!(host
        .deliver_one(new_split_command("{}".to_owned()))
        .is_empty());
}
