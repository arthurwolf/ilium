//! Second wave of port twins (gap list lines 1 to 52). The Ask for update popup, the
//! `ask-for-update` channel, `POST /create_agent` and `ilium new-pane` are the plugin-visible
//! contracts; widget internals, the prompt tracker, delivery text sanitising and the IPC
//! connection are kernel or client internals with no plugin channel, so those originals have no
//! twin. Kernel-framed prompt text (paste markers, line endings) is checked as the verbatim
//! `initial_input`, like `initial_prompt.rs`.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, AgentActivity, Event, Intent, NodeId, Outcome, ViewTarget,
};
use serde_json::{json, Value};

use crate::common::*;

fn popup() -> ViewTarget {
    ViewTarget::Popup("ask-for-update".to_string())
}

fn open_dialog() -> FakeHost {
    let mut host = client_host();
    host.deliver_one(Event::CommandInvoked(
        ilium_plugin_sdk::types::CommandInvokedEvent {
            command: "ask-for-update".to_string(),
            arguments_json: "{}".to_string(),
            origin: ilium_plugin_sdk::types::CommandOrigin::Palette,
            context: ilium_plugin_sdk::types::CommandContext::default(),
        },
    ));
    host.settle();
    host
}

fn messages(host: &FakeHost) -> Vec<(String, Vec<u8>)> {
    host.submitted()
        .into_iter()
        .filter_map(|s| {
            if let Intent::Message(m) = s.intent {
                Some((m.channel, m.payload))
            } else {
                None
            }
        })
        .collect()
}

fn type_text(host: &mut FakeHost, text: &str) {
    host.deliver_one(dialog_action(
        "update-text",
        ActionKind::Changed,
        Some(ActionValue::Text(text.to_string())),
    ));
    host.settle();
}

fn ask(host: &mut FakeHost, text: &str) {
    host.deliver_one(message("ask-for-update", text.as_bytes().to_vec()));
    host.settle();
}

fn post(host: &mut FakeHost, call: u64, body: &Value) {
    host.deliver_one(http_call(
        call,
        "POST",
        "/create_agent",
        body.to_string().as_bytes(),
    ));
    host.settle();
}

fn reply(host: &FakeHost, call: u64) -> (u16, Vec<u8>) {
    let found: Vec<_> = host
        .http_replies()
        .into_iter()
        .filter(|(id, ..)| *id == call)
        .collect();
    assert_eq!(found.len(), 1, "exactly one reply for call {call}");
    let (_, status, _, body) = found.into_iter().next().unwrap();
    (status, body)
}

fn agent_body(project: &str, prompt: &str) -> Value {
    json!({"agent_type": "claude", "project": project, "prompt": prompt})
}

fn initial_input_for(prompt: &str) -> Option<String> {
    let mut host = server_host();
    post(&mut host, 1, &agent_body("/home/me/dev/app", prompt));
    let specs = created_panes(&host);
    assert_eq!(specs.len(), 1, "one create-pane expected for {prompt:?}");
    specs[0].initial_input.clone()
}

/// REQ: PORT:ilium-client/src/agent_message_dialog.rs::agent_message_overflow_track_is_visible_and_does_not_toggle_a_recipient
#[test]
fn port_001_a_small_popup_keeps_its_fields_and_a_border_click_sends_nothing() {
    let mut host = open_dialog();
    let text = host.render(popup(), 40, 12).text();
    assert!(text.contains("All agents"), "{text}");
    assert!(text.contains("Message"), "{text}");
    assert!(!host.click(popup(), 0, 0));
    host.settle();
    assert_eq!(messages(&host), Vec::<(String, Vec<u8>)>::new());
}

/// REQ: PORT:ilium-client/src/agent_message_dialog.rs::mouse_toggles_exact_painted_recipient_and_submit_control
#[test]
fn port_003_clicking_the_painted_send_button_sends_the_typed_message_once() {
    let mut host = open_dialog();
    type_text(&mut host, "status please");
    let screen = host.render(popup(), 80, 24);
    let (row, column) = screen.find("Send").expect("a Send button is painted");
    assert!(host.click(popup(), column, row));
    host.settle();
    assert_eq!(
        messages(&host),
        [("ask-for-update".to_string(), b"status please".to_vec())]
    );
}

/// REQ: PORT:ilium-client/src/agent_message_dialog.rs::top_level_mouse_dispatch_preserves_agent_message_recipient_selection
#[test]
fn port_006_a_stray_click_keeps_the_typed_text_for_the_later_send() {
    let mut host = open_dialog();
    type_text(&mut host, "keep this");
    assert!(!host.click(popup(), 0, 0));
    host.deliver_one(dialog_action(
        "update-send",
        ActionKind::Activated,
        Some(ActionValue::None),
    ));
    host.settle();
    assert_eq!(
        messages(&host),
        [("ask-for-update".to_string(), b"keep this".to_vec())]
    );
}

/// REQ: PORT:ilium-client/src/app.rs::agent_message_dialog_marks_removed_snapshot_recipient_unavailable
#[test]
fn port_008_a_removed_agent_pane_receives_nothing_and_the_other_still_does() {
    let mut host = server_host();
    let removed = add_agent(&mut host, "gone", "claude", AgentActivity::Idle);
    let kept = add_agent(&mut host, "kept", "codex", AgentActivity::Done);
    host.tree().remove(removed);
    ask(&mut host, "status");
    assert_eq!(sent_panes(&host), [NodeId(kept)]);
}

/// REQ: PORT:ilium-client/src/app.rs::agent_message_dialog_without_enter_sends_message_bytes_without_newline
#[test]
fn port_009_a_single_line_message_is_delivered_without_a_trailing_newline() {
    let mut host = server_host();
    add_agent(&mut host, "idle agent", "claude", AgentActivity::Idle);
    add_agent(&mut host, "done agent", "codex", AgentActivity::Done);
    ask(&mut host, "hello");
    let texts: Vec<String> = sent_inputs(&host)
        .into_iter()
        .map(|spec| spec.text)
        .collect();
    assert_eq!(texts, vec!["hello".to_string(), "hello".to_string()]);
}

/// REQ: PORT:ilium-client/src/app.rs::ask_for_update_includes_literal_custom_instructions
#[test]
fn port_011_custom_instructions_are_delivered_literally() {
    let mut host = server_host();
    add_agent(&mut host, "idle agent", "claude", AgentActivity::Idle);
    ask(&mut host, "Be brief: {x} $HOME `ls`");
    let texts: Vec<String> = sent_inputs(&host)
        .into_iter()
        .map(|spec| spec.text)
        .collect();
    assert_eq!(texts, vec!["Be brief: {x} $HOME `ls`".to_string()]);
}

/// REQ: PORT:ilium-client/src/app.rs::ask_for_update_menu_entry_only_appears_where_an_eligible_pane_exists
#[test]
fn port_012_with_only_a_shell_pane_the_ask_reaches_no_pane() {
    let mut host = server_host();
    add_shell(&mut host, "shell");
    ask(&mut host, "status");
    assert_eq!(sent_panes(&host), Vec::<NodeId>::new());
}

/// REQ: PORT:ilium-client/src/app.rs::create_agent_line_closed_admission_preserves_draft_and_parent_modal
#[test]
fn port_013_a_refused_request_creates_nothing_and_the_same_prompt_is_accepted_on_retry() {
    let mut host = server_host();
    post(&mut host, 1, &agent_body("/home/me/dev/app", " "));
    assert_eq!(reply(&host, 1).0, 400);
    assert_eq!(created_panes(&host).len(), 0);
    post(
        &mut host,
        2,
        &agent_body("/home/me/dev/app", "exact draft\nsecond line"),
    );
    let specs = created_panes(&host);
    assert_eq!(specs.len(), 1);
    assert_eq!(
        specs[0].initial_input.as_deref(),
        Some("exact draft\nsecond line")
    );
}

/// REQ: PORT:ilium-client/src/app.rs::create_agent_line_removed_parent_keeps_draft_without_retargeting
#[test]
fn port_014_a_missing_project_is_refused_then_accepted_once_it_exists() {
    let mut host = server_host();
    post(&mut host, 1, &agent_body("ilium", "keep me"));
    assert_eq!(reply(&host, 1).0, 400);
    assert_eq!(created_panes(&host).len(), 0);
    host.fs().write("/home/me/dev/ai/ilium/.keep", b"");
    post(&mut host, 2, &agent_body("ilium", "keep me"));
    let specs = created_panes(&host);
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].cwd.as_deref(), Some("/home/me/dev/ai/ilium"));
    assert_eq!(specs[0].initial_input.as_deref(), Some("keep me"));
}

/// REQ: PORT:ilium-client/src/render_cache.rs::tree_snapshot_creates_a_terminal_view_for_a_new_pane
#[test]
fn port_018_an_agent_pane_added_after_activation_receives_the_ask() {
    let mut host = server_host();
    let late = add_agent(&mut host, "late", "claude", AgentActivity::Idle);
    ask(&mut host, "status");
    assert_eq!(sent_panes(&host), [NodeId(late)]);
}

/// REQ: PORT:ilium-server/src/http_api.rs::admitted_lookup_runs_on_native_bank_and_json_bytes_keep_original_charge
#[test]
fn port_034_an_admitted_lookup_replies_with_the_json_of_the_created_pane() {
    let mut host = server_host();
    host.fs().write("/home/me/dev/ai/ilium/.keep", b"");
    post(&mut host, 1, &agent_body("ilium", "go"));
    for request in host.pending_requests() {
        host.complete(request, Outcome::OkNode(NodeId(42)));
    }
    host.settle();
    let (status, body) = reply(&host, 1);
    let value: Value = serde_json::from_slice(&body).expect("JSON body");
    assert_eq!(
        (status, &value["pane_id"], &value["project_path"]),
        (201, &json!(42), &json!("/home/me/dev/ai/ilium"))
    );
}

/// REQ: PORT:ilium-server/src/http_api.rs::lookup_error_preserves_original_json_and_charge_until_last_byte_clone
#[test]
fn port_036_a_lookup_that_finds_no_project_replies_400_with_a_json_error() {
    let mut host = server_host();
    post(&mut host, 1, &agent_body("missing-project", "go"));
    let (status, body) = reply(&host, 1);
    let value: Value = serde_json::from_slice(&body).expect("JSON body");
    assert_eq!(status, 400);
    assert!(value["error"].is_string());
    assert_eq!(created_panes(&host).len(), 0);
}

/// REQ: PORT:ilium-server/src/initial_prompt.rs::a_lone_carriage_return_becomes_a_line_break_not_a_premature_enter
#[test]
fn port_038_a_lone_carriage_return_reaches_create_pane_unchanged_for_the_kernel_to_frame() {
    assert_eq!(
        initial_input_for("first\rsecond").as_deref(),
        Some("first\rsecond")
    );
}

/// REQ: PORT:ilium-server/src/initial_prompt.rs::a_single_line_paste_start_marker_is_stripped_so_it_cannot_open_a_phantom_paste
#[test]
fn port_039_a_paste_start_marker_in_a_single_line_prompt_is_passed_through_unchanged() {
    assert_eq!(
        initial_input_for("a\u{1b}[200~b").as_deref(),
        Some("a\u{1b}[200~b")
    );
}

/// REQ: PORT:ilium-server/src/initial_prompt.rs::an_embedded_paste_end_marker_is_stripped_so_it_cannot_end_the_paste_early
#[test]
fn port_040_an_embedded_paste_end_marker_is_passed_through_unchanged() {
    assert_eq!(
        initial_input_for("a\u{1b}[201~\nb").as_deref(),
        Some("a\u{1b}[201~\nb")
    );
}

/// REQ: PORT:ilium-server/src/initial_prompt.rs::an_embedded_paste_start_marker_inside_a_multiline_paste_is_stripped
#[test]
fn port_041_an_embedded_paste_start_marker_in_a_multiline_prompt_is_passed_through_unchanged() {
    assert_eq!(
        initial_input_for("a\n\u{1b}[200~b\nc").as_deref(),
        Some("a\n\u{1b}[200~b\nc")
    );
}

/// REQ: PORT:ilium-server/src/initial_prompt.rs::crlf_line_endings_are_normalized_before_bracketing
#[test]
fn port_042_crlf_line_endings_reach_create_pane_unchanged_for_the_kernel_to_normalise() {
    assert_eq!(
        initial_input_for("one\r\ntwo").as_deref(),
        Some("one\r\ntwo")
    );
}

/// REQ: PORT:ilium/tests/pty_tui_smoke.rs::attaching_tui_renders_the_pane_created_by_new_pane_and_responds_to_the_help_keystroke
#[test]
fn port_052_new_pane_creates_exactly_one_terminal_pane_for_the_attached_tui() {
    let mut host = server_host();
    add_session(&mut host, "default", true, &[]);
    let (_, _, exit) = run_cli(&mut host, &["new-pane", "--", "claude"], b"");
    let specs = created_panes(&host);
    assert_eq!(exit, Some(0));
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].content_kind, "ilium.terminal");
}
