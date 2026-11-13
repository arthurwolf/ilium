//! The "New split view" dialog, moved here from the kernel client. Each test names the kernel
//! test it replaces (`PORT:<file>::<test>`); the kernel copies are deleted.

use ilium_plugin_core_layout::new_split::{self, Answer, Dialog, Page, Step};
use ilium_plugin_core_layout::LayoutClient;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, CommandContext, CommandInvokedEvent, CommandOrigin, Event, Intent,
    SplitDirection, Submission, TriState, UiAction, Widget,
};
use serde_json::json;

use crate::common::*;

const GROUP: u64 = 10;

/// The kernel's command arguments for: root 0 > project 1 > group 10 > panes, plus a split
/// view 20 holding pane 21 and a worktree 30 holding pane 31.
fn arguments(free_panes: &[u64]) -> String {
    let mut nodes = vec![
        json!({"id": 1, "parent": 0, "kind": "project", "title": "ilium"}),
        json!({"id": GROUP, "parent": 1, "kind": "group", "title": "work"}),
        json!({"id": 20, "parent": GROUP, "kind": "split", "title": "Vertical split"}),
        json!({"id": 21, "parent": 20, "kind": "pane", "content": "terminal", "title": "held"}),
        json!({"id": 30, "parent": 1, "kind": "worktree", "title": "feature"}),
        json!({"id": 31, "parent": 30, "kind": "pane", "content": "terminal", "title": "tree"}),
    ];
    let mut panes: Vec<u64> = free_panes.to_vec();
    for pane in free_panes {
        nodes.push(json!({
            "id": pane, "parent": GROUP, "kind": "pane", "content": "editor",
            "title": format!("pane {pane}"),
        }));
    }
    panes.extend([21, 31]);
    json!({"root": 0, "parent": GROUP, "nodes": nodes, "panes": panes}).to_string()
}

fn dialog(free_panes: &[u64]) -> Dialog {
    Dialog::open(&arguments(free_panes)).expect("the arguments parse")
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
    let arguments = json!({
        "root": 0, "parent": 1, "panes": [2],
        "nodes": [
            {"id": 1, "parent": 0, "kind": "project", "title": "ilium"},
            {"id": 2, "parent": 1, "kind": "pane", "content": "terminal", "agent": "agent",
             "title": "claude"},
        ],
    });
    let dialog = Dialog::open(&arguments.to_string()).expect("the arguments parse");
    assert_eq!(dialog.choices[0].label, "[agent] ilium / claude");
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
    let opened = host.deliver_one(new_split_command(arguments(&[11])));
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
