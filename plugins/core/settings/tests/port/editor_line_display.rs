//! The "Editor line display" dialog, moved here from the kernel client (`value_editor.rs`). Each
//! test names the kernel test it replaces; the kernel copy is deleted.

use ilium_plugin_core_settings::editor_line_display as dialog;
use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, CommandContext, CommandInvokedEvent, CommandOrigin, Event, Intent,
    Submission, UiAction, Widget,
};
use serde_json::{json, Value};

use crate::common::client_host;

fn command(arguments: Value) -> Event {
    Event::CommandInvoked(CommandInvokedEvent {
        command: dialog::COMMAND.to_owned(),
        arguments_json: arguments.to_string(),
        origin: CommandOrigin::Toolbar,
        context: CommandContext::default(),
    })
}

fn action(key: &str, action_id: &str, kind: ActionKind, value: Option<ActionValue>) -> Event {
    Event::UiAction(UiAction {
        view: dialog::view(),
        key: key.to_owned(),
        action_id: action_id.to_owned(),
        kind,
        value,
    })
}

fn open_with(current: &str) -> (ilium_plugin_sdk::testing::FakeHost, Vec<Submission>) {
    let mut host = client_host();
    let opened = host.deliver_one(command(json!({ "pane": 7, "line_display": current })));
    (host, opened)
}

fn kernel_commands(submissions: &[Submission]) -> Vec<(String, Value)> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::InvokeCommand(spec) => Some((
                spec.command.clone(),
                serde_json::from_str(&spec.arguments_json).expect("arguments are JSON"),
            )),
            _ => None,
        })
        .collect()
}

fn closes_the_dialog(submissions: &[Submission]) -> bool {
    submissions
        .iter()
        .any(|submission| matches!(&submission.intent, Intent::CloseView(view) if *view == dialog::view()))
}

/// REQ: PORT:ilium-client/src/value_editor.rs::full_catalog_changes_only_native_line_display
#[test]
fn port_editor_line_display_the_command_opens_a_popup_listing_clip_and_wrap() {
    let (mut host, opened) = open_with("clip");
    assert!(opened.iter().any(
        |submission| matches!(&submission.intent, Intent::OpenView(spec) if spec.view == dialog::view())
    ));
    host.render(dialog::view(), 60, 20);
    let tree = host.last_tree(dialog::view());
    let choice = tree
        .nodes
        .iter()
        .find_map(|node| match &node.widget {
            Widget::Choice(choice) => Some(choice.clone()),
            _ => None,
        })
        .expect("the popup holds the choice");
    assert_eq!(choice.options, ["Clip", "Wrap"]);
    assert_eq!(choice.selected, 0);
}

/// REQ: PORT:ilium-client/src/value_editor.rs::full_catalog_changes_only_native_line_display
#[test]
fn port_editor_line_display_choosing_wrap_sets_only_the_line_display_of_that_pane() {
    let (mut host, _) = open_with("clip");
    host.deliver_one(action(
        dialog::MODE_KEY,
        "",
        ActionKind::Selected,
        Some(ActionValue::Index(1)),
    ));
    let ended = host.deliver_one(action("dialog", "apply", ActionKind::Activated, None));
    assert_eq!(
        kernel_commands(&ended),
        [(
            "kernel.set-editor-option".to_owned(),
            json!({ "pane": 7, "option": "line-display", "value": "wrap" })
        )]
    );
    assert!(closes_the_dialog(&ended));
}

/// REQ: PORT:ilium-client/src/value_editor.rs::full_catalog_changes_only_native_line_display
#[test]
fn port_editor_line_display_choosing_the_current_mode_sends_no_command() {
    let (mut host, _) = open_with("wrap");
    let ended = host.deliver_one(action("dialog", "apply", ActionKind::Activated, None));
    assert!(kernel_commands(&ended).is_empty());
    assert!(closes_the_dialog(&ended));
}

/// REQ: PORT:ilium-client/src/value_editor.rs::closed_retargeted_or_invalid_editor_choices_are_rejected
#[test]
fn port_editor_line_display_an_invented_choice_or_button_changes_nothing() {
    let (mut host, _) = open_with("clip");
    for event in [
        action(
            dialog::MODE_KEY,
            "",
            ActionKind::Selected,
            Some(ActionValue::Index(9)),
        ),
        action("dialog", "invented", ActionKind::Activated, None),
    ] {
        assert!(host.deliver_one(event).is_empty());
    }
    let ended = host.deliver_one(action("dialog", "apply", ActionKind::Activated, None));
    assert!(kernel_commands(&ended).is_empty(), "the mode is still clip");
}

/// REQ: PORT:ilium-client/src/value_editor.rs::closed_retargeted_or_invalid_editor_choices_are_rejected
#[test]
fn port_editor_line_display_a_closed_dialog_ignores_a_late_apply() {
    let (mut host, _) = open_with("clip");
    host.deliver_one(action("dialog", "", ActionKind::Closed, None));
    assert!(host
        .deliver_one(action("dialog", "apply", ActionKind::Activated, None))
        .is_empty());
}

/// REQ: PORT:ilium-client/src/value_editor.rs::closed_retargeted_or_invalid_editor_choices_are_rejected
#[test]
fn port_editor_line_display_arguments_without_a_pane_or_mode_open_nothing() {
    let mut host = client_host();
    for arguments in [
        json!({}),
        json!({ "pane": 7 }),
        json!({ "pane": 7, "line_display": "fold" }),
    ] {
        assert!(host.deliver_one(command(arguments)).is_empty());
    }
}
