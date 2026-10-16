//! F-051: destructive voice commands wait for a confirmation dialog (popup `confirm-voice`, kind
//! danger-confirm); the command is submitted only after the `ok` button, never after cancel or Esc.

use ilium_plugin_sdk::exports::HookReply;
use ilium_plugin_sdk::types::ViewTarget;
use serde_json::json;

use crate::common::{
    CONFIRM_POPUP, add_pane, contribute_phrase, enabled_client, hear, invoked_commands, popup_button,
};

fn host_with_docs_pane() -> ilium_plugin_sdk::testing::FakeHost {
    let mut host = enabled_client("push_to_talk");
    add_pane(&mut host, "docs");
    host
}

/// REQ: F-051
#[test]
fn confirm_051_a_destructive_phrase_opens_the_dialog_and_submits_nothing() {
    let mut host = host_with_docs_pane();
    hear(&mut host, "close the pane called docs");
    assert_eq!(host.popups(), [CONFIRM_POPUP]);
    assert!(invoked_commands(&host.submitted()).is_empty());
}

/// REQ: F-051
#[test]
fn confirm_051_ok_runs_the_close_pane_command_once() {
    let mut host = host_with_docs_pane();
    hear(&mut host, "close the pane called docs");
    let submissions = host.deliver_one(popup_button(CONFIRM_POPUP, "ok"));
    assert_eq!(invoked_commands(&submissions), ["kernel.close_pane"]);
}

/// REQ: F-051
#[test]
fn confirm_051_the_close_command_names_the_resolved_pane() {
    let mut host = host_with_docs_pane();
    hear(&mut host, "close the pane called docs");
    let submissions = host.deliver_one(popup_button(CONFIRM_POPUP, "ok"));
    let text = format!("{:?}", submissions[0].intent);
    assert!(text.contains("docs") || text.contains('1'), "{text}");
}

/// REQ: F-051
#[test]
fn confirm_051_cancel_submits_nothing() {
    let mut host = host_with_docs_pane();
    hear(&mut host, "close the pane called docs");
    let submissions = host.deliver_one(popup_button(CONFIRM_POPUP, "cancel"));
    assert!(invoked_commands(&submissions).is_empty());
}

/// REQ: F-051
#[test]
fn confirm_051_ok_after_cancel_does_not_resurrect_the_command() {
    let mut host = host_with_docs_pane();
    hear(&mut host, "close the pane called docs");
    host.deliver_one(popup_button(CONFIRM_POPUP, "cancel"));
    let submissions = host.deliver_one(popup_button(CONFIRM_POPUP, "ok"));
    assert!(invoked_commands(&submissions).is_empty());
}

/// REQ: F-051
#[test]
fn confirm_051_a_second_ok_does_not_run_the_command_again() {
    let mut host = host_with_docs_pane();
    hear(&mut host, "close the pane called docs");
    host.deliver_one(popup_button(CONFIRM_POPUP, "ok"));
    let again = host.deliver_one(popup_button(CONFIRM_POPUP, "ok"));
    assert!(invoked_commands(&again).is_empty());
}

/// REQ: F-051
#[test]
fn confirm_051_an_ok_with_nothing_waiting_submits_nothing() {
    let mut host = host_with_docs_pane();
    let submissions = host.deliver_one(popup_button(CONFIRM_POPUP, "ok"));
    assert!(invoked_commands(&submissions).is_empty());
}

/// REQ: F-051
#[test]
fn confirm_051_a_button_of_another_popup_does_not_confirm() {
    let mut host = host_with_docs_pane();
    hear(&mut host, "close the pane called docs");
    let submissions = host.deliver_one(popup_button("some-other-popup", "ok"));
    assert!(invoked_commands(&submissions).is_empty());
}

/// REQ: F-051
#[test]
fn confirm_051_enter_confirms_and_escape_cancels() {
    let mut host = host_with_docs_pane();
    hear(&mut host, "close the pane called docs");
    host.key(ViewTarget::Popup(CONFIRM_POPUP.to_string()), "Esc");
    host.settle();
    assert!(invoked_commands(&host.submitted()).is_empty());
    hear(&mut host, "close the pane called docs");
    host.key(ViewTarget::Popup(CONFIRM_POPUP.to_string()), "Enter");
    host.settle();
    assert_eq!(invoked_commands(&host.submitted()), ["kernel.close_pane"]);
}

/// REQ: F-051
#[test]
fn confirm_051_a_contributed_phrase_without_confirm_runs_at_once_with_empty_arguments() {
    let mut host = enabled_client("push_to_talk");
    contribute_phrase(&mut host, "open-board", "open the board", "open-board", false);
    hear(&mut host, "open the board");
    let submitted = host.submitted();
    assert_eq!(invoked_commands(&submitted), ["open-board"]);
    assert!(host.popups().is_empty());
    assert!(format!("{:?}", submitted[0].intent).contains("{}"));
}

/// REQ: F-051
#[test]
fn confirm_051_a_contributed_phrase_with_confirm_waits_for_the_dialog() {
    let mut host = enabled_client("push_to_talk");
    contribute_phrase(&mut host, "wipe", "wipe the board", "board.clear", true);
    hear(&mut host, "wipe the board");
    assert!(invoked_commands(&host.submitted()).is_empty());
    assert_eq!(host.popups(), [CONFIRM_POPUP]);
    let submissions = host.deliver_one(popup_button(CONFIRM_POPUP, "ok"));
    assert_eq!(invoked_commands(&submissions), ["board.clear"]);
}

/// REQ: F-051
#[test]
fn confirm_051_the_phrase_match_ignores_case_and_surrounding_space() {
    let mut host = enabled_client("push_to_talk");
    contribute_phrase(&mut host, "open-board", "open the board", "open-board", false);
    hear(&mut host, "  Open The Board \n");
    assert_eq!(invoked_commands(&host.submitted()), ["open-board"]);
}

/// REQ: F-051
#[test]
fn confirm_051_the_higher_priority_entry_wins_for_the_same_phrase() {
    let mut host = enabled_client("push_to_talk");
    host.contribute_extension(
        "ilium.core.voice/voice-intents",
        "low",
        10,
        &json!({ "phrases": ["go"], "command": "low-command", "confirm": false }).to_string(),
    );
    host.contribute_extension(
        "ilium.core.voice/voice-intents",
        "high",
        90,
        &json!({ "phrases": ["go"], "command": "high-command", "confirm": false }).to_string(),
    );
    hear(&mut host, "go");
    assert_eq!(invoked_commands(&host.submitted()), ["high-command"]);
}

/// REQ: F-051
#[test]
fn confirm_051_the_transcript_hook_corrects_the_text_before_phrases_are_matched() {
    let mut host = enabled_client("push_to_talk");
    contribute_phrase(&mut host, "open-board", "open the board", "open-board", false);
    host.set_hook_reply(
        "ilium.core.voice/transcript",
        HookReply::Replace(json!({ "text": "open the board" }).to_string()),
    );
    hear(&mut host, "open the boredom");
    assert_eq!(invoked_commands(&host.submitted()), ["open-board"]);
}

/// REQ: F-051
#[test]
fn confirm_051_without_a_hook_contributor_the_transcript_is_matched_unchanged() {
    let mut host = enabled_client("push_to_talk");
    contribute_phrase(&mut host, "open-board", "open the board", "open-board", false);
    hear(&mut host, "open the boredom");
    assert!(invoked_commands(&host.submitted()).is_empty());
}

/// REQ: F-051
#[test]
fn confirm_051_a_new_destructive_phrase_replaces_the_one_waiting() {
    let mut host = host_with_docs_pane();
    add_pane(&mut host, "logs");
    hear(&mut host, "close the pane called docs");
    hear(&mut host, "close the pane called logs");
    let submissions = host.deliver_one(popup_button(CONFIRM_POPUP, "ok"));
    assert_eq!(invoked_commands(&submissions), ["kernel.close_pane"]);
    assert!(format!("{:?}", submissions[0].intent).contains("logs"));
}
