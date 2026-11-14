//! The crash-recovery question ("Restore previous session?"), moved here from the kernel client
//! (`Mode::ConfirmSessionRecovery` in `ilium-client/src/{app,keys,mouse,ui,render_cache}.rs`).
//! The kernel asks through the command `session-recovery` and carries out the answer
//! `kernel.resolve-session-recovery`; the wording, the buttons and the rules live here.

use ilium_plugin_core_sessions::session_recovery as dialog;
use ilium_plugin_sdk::types::{
    ActionKind, ButtonRole, CommandContext, CommandInvokedEvent, CommandOrigin, Event, Intent,
    Submission, UiAction, Widget,
};
use serde_json::{json, Value};

use crate::common::activated_client;

fn command(arguments: Value) -> Event {
    Event::CommandInvoked(CommandInvokedEvent {
        command: dialog::COMMAND.to_owned(),
        arguments_json: arguments.to_string(),
        origin: CommandOrigin::Cli,
        context: CommandContext::default(),
    })
}

fn action(action_id: &str, kind: ActionKind) -> Event {
    Event::UiAction(UiAction {
        view: dialog::view(),
        key: "dialog".to_owned(),
        action_id: action_id.to_owned(),
        kind,
        value: None,
    })
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
    submissions.iter().any(
        |submission| matches!(&submission.intent, Intent::CloseView(view) if *view == dialog::view()),
    )
}

/// Replaces the kernel test `session_recovery_buttons_do_not_leak_clicks_to_the_obscured_tui`:
/// the Restore button answers `restore: true` and the Discard button `restore: false`, each
/// once, and each closes the popup.
#[test]
fn port_session_recovery_each_button_answers_the_server_once_and_closes_the_popup() {
    for (button, restore) in [("restore", true), ("discard", false)] {
        let mut host = activated_client();
        let opened = host.deliver_one(command(json!({ "pane_count": 3 })));
        assert!(opened.iter().any(
            |submission| matches!(&submission.intent, Intent::OpenView(spec) if spec.view == dialog::view())
        ));
        let ended = host.deliver_one(action(button, ActionKind::Activated));
        assert_eq!(
            kernel_commands(&ended),
            [(
                "kernel.resolve-session-recovery".to_owned(),
                json!({ "restore": restore })
            )]
        );
        assert!(closes_the_dialog(&ended));
        // The answer is final: a second press of a button of the closed dialog sends nothing.
        let again = host.deliver_one(action(button, ActionKind::Activated));
        assert!(kernel_commands(&again).is_empty());
    }
}

/// Replaces the kernel `ConfirmSessionRecovery` rendering: the title, the message with the pane
/// count, and the Discard (danger) and Restore (primary, the default) buttons.
#[test]
fn port_session_recovery_popup_shows_the_stored_pane_count_and_both_buttons() {
    let mut host = activated_client();
    host.deliver_one(command(json!({ "pane_count": 12 })));
    host.render(dialog::view(), 80, 24);
    let tree = host.last_tree(dialog::view());
    let Widget::Dialog(spec) = &tree.nodes[tree.root as usize].widget else {
        panic!("the popup root is a dialog");
    };
    assert_eq!(spec.title[0].text, "Restore previous session?");
    let message = &spec.message.as_ref().expect("a message")[0][0].text;
    assert_eq!(
        message,
        "A stored snapshot contains 12 pane(s). Restore it, or discard it and start fresh?"
    );
    let buttons: Vec<_> = spec
        .buttons
        .iter()
        .map(|button| (button.label.as_str(), button.role))
        .collect();
    assert_eq!(
        buttons,
        [
            ("Discard", ButtonRole::Danger),
            ("Restore", ButtonRole::Primary)
        ]
    );
}

/// Closing the popup without a button (Escape, a click outside it, or a kernel that could not
/// show it) answers nothing, so no snapshot is thrown away by accident.
#[test]
fn port_session_recovery_closing_the_popup_does_not_answer() {
    let mut host = activated_client();
    host.deliver_one(command(json!({ "pane_count": 2 })));
    let closed = host.deliver_one(action("", ActionKind::Closed));
    assert!(kernel_commands(&closed).is_empty());
    // The kernel asks again; the dialog opens and can still be answered.
    host.deliver_one(command(json!({ "pane_count": 2 })));
    let ended = host.deliver_one(action("restore", ActionKind::Activated));
    assert_eq!(kernel_commands(&ended).len(), 1);
}

/// A command without a usable pane count opens nothing.
#[test]
fn port_session_recovery_malformed_arguments_open_no_dialog() {
    let mut host = activated_client();
    for bad in [json!({}), json!({ "pane_count": "3" }), json!([])] {
        let submissions = host.deliver_one(command(bad));
        assert!(submissions.is_empty());
    }
}
