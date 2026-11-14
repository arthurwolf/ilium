//! The tree toolbar editor: the client command `tree-toolbar` opens a popup holding the buttons
//! in use and the buttons that can be added; every press is sent to the kernel at once as
//! `kernel.set-tree-toolbar`, and Cancel sends the toolbar back that was in use.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, CommandContext, CommandInvokedEvent, CommandOrigin, Event, Intent,
    Submission, UiAction, ViewTarget, Widget,
};

use crate::common::client_host;

fn popup() -> ViewTarget {
    ViewTarget::Popup("tree-toolbar".to_string())
}

fn arguments() -> String {
    let entry = |key: &str| serde_json::json!({ "key": key, "label": format!("{key} label"), "glyph": key });
    serde_json::json!({
        "entries": [entry("search"), entry("shell"), entry("settings")],
        "catalogue": [entry("search"), entry("shell"), entry("codex"), entry("settings")],
        "defaults": ["search", "shell", "codex", "settings"],
        "minutes": 15,
    })
    .to_string()
}

fn invoke(host: &mut FakeHost, arguments: &str) -> Vec<Submission> {
    host.deliver_one(Event::CommandInvoked(CommandInvokedEvent {
        command: "ilium.core.tree/tree-toolbar".to_string(),
        arguments_json: arguments.to_string(),
        origin: CommandOrigin::Menu,
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

fn press(host: &mut FakeHost, id: &str) -> Vec<Submission> {
    act(host, "", id, ActionKind::Activated, None)
}

/// The `kernel.set-tree-toolbar` calls among `submissions`, as parsed arguments.
fn sets(submissions: &[Submission]) -> Vec<serde_json::Value> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::InvokeCommand(spec) if spec.command == "kernel.set-tree-toolbar" => {
                Some(serde_json::from_str(&spec.arguments_json).expect("arguments are JSON"))
            }
            _ => None,
        })
        .collect()
}

fn closed(submissions: &[Submission]) -> usize {
    submissions
        .iter()
        .filter(
            |submission| matches!(&submission.intent, Intent::CloseView(view) if *view == popup()),
        )
        .count()
}

#[test]
fn the_command_opens_the_popup_with_both_lists() {
    let mut host = client_host();
    let submissions = invoke(&mut host, &arguments());
    assert!(submissions.iter().any(
        |submission| matches!(&submission.intent, Intent::OpenView(spec) if spec.view == popup())
    ));
    host.render(popup(), 80, 24);
    let tree = host.last_tree(popup());
    let lists: Vec<usize> = tree
        .nodes
        .iter()
        .filter_map(|node| match &node.widget {
            Widget::Choice(choice) => Some(choice.options.len()),
            _ => None,
        })
        .collect();
    // Three buttons in use; codex is the only one left to add.
    assert_eq!(lists, [3, 1]);
}

#[test]
fn malformed_arguments_open_nothing() {
    for bad in ["", "{}", r#"{"entries": []}"#] {
        let mut host = client_host();
        assert!(invoke(&mut host, bad).is_empty(), "{bad}");
    }
}

#[test]
fn removing_a_button_sets_the_toolbar_at_once() {
    let mut host = client_host();
    invoke(&mut host, &arguments());
    let submissions = press(&mut host, "remove");
    let sent = sets(&submissions);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["entries"], serde_json::json!(["shell", "settings"]));
    assert_eq!(sent[0]["minutes"], 15);
    assert_eq!(closed(&submissions), 0);
}

#[test]
fn adding_a_button_puts_it_last() {
    let mut host = client_host();
    invoke(&mut host, &arguments());
    let sent = sets(&press(&mut host, "add"));
    assert_eq!(
        sent[0]["entries"],
        serde_json::json!(["search", "shell", "settings", "codex"])
    );
}

#[test]
fn longer_and_shorter_change_the_minutes() {
    let mut host = client_host();
    invoke(&mut host, &arguments());
    assert_eq!(sets(&press(&mut host, "longer"))[0]["minutes"], 16);
    assert_eq!(sets(&press(&mut host, "shorter"))[0]["minutes"], 15);
}

#[test]
fn cancel_sends_back_the_toolbar_that_was_open_and_closes() {
    let mut host = client_host();
    invoke(&mut host, &arguments());
    press(&mut host, "remove");
    let submissions = press(&mut host, "cancel");
    let sent = sets(&submissions);
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0]["entries"],
        serde_json::json!(["search", "shell", "settings"])
    );
    assert_eq!(closed(&submissions), 1);
}

#[test]
fn done_keeps_the_toolbar_and_closes_and_a_late_action_does_nothing() {
    let mut host = client_host();
    invoke(&mut host, &arguments());
    press(&mut host, "remove");
    let submissions = press(&mut host, "done");
    assert!(sets(&submissions).is_empty());
    assert_eq!(closed(&submissions), 1);
    assert!(press(&mut host, "remove").is_empty());
}

#[test]
fn the_kernel_closing_the_popup_keeps_the_toolbar_and_sends_nothing() {
    let mut host = client_host();
    invoke(&mut host, &arguments());
    press(&mut host, "remove");
    let submissions = act(&mut host, "", "", ActionKind::Closed, None);
    assert!(submissions.is_empty());
}
