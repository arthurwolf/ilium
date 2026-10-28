//! Twins of app.rs: what toolbar actions do in the client application (bytes sent, resize, submenu, dead pane).
//! Request/reply shape: see API_GAPS_ports.md.
use crate::common::{ask, Place};
use serde_json::Value;

fn check(place: Place, op: &str, request: &str, expected: &str) {
    let request: Value = serde_json::from_str(request).expect("request literal parses");
    let expected: Value = serde_json::from_str(expected).expect("expected literal parses");
    let reply = ask(place, op, request);
    let fields = expected.as_object().expect("expected is an object");
    for (name, value) in fields {
        assert_eq!(
            &reply[name.as_str()],
            value,
            "field {name} of reply {reply}"
        );
    }
}

/// REQ: PORT:ilium-client/src/app.rs::agent_toolbar_actions_send_the_bytes_their_tooltip_promises
#[test]
fn port_app_agent_toolbar_actions_send_the_bytes_their_tooltip_promises() {
    check(
        Place::Client,
        "toolbar.click",
        r####"{"actions": ["compact", "stop", "model-0", "cycle-effort-x6"]}"####,
        r####"{"compact": {"submit_terminal_text": "/compact", "source": "toolbar-action"}, "stop": {"key_input": "\u001b"}, "model-0": {"submit_terminal_text": "/model haiku"}, "cycle-effort-x6": {"key_input": "ultracode "}}"####,
    );
}

/// REQ: PORT:ilium-client/src/app.rs::closing_the_agent_toolbar_hides_it_and_resizes_the_pty_back_up
#[test]
fn port_app_closing_the_agent_toolbar_hides_it_and_resizes_the_pty_back_up() {
    check(
        Place::Client,
        "toolbar.close",
        r####"{}"####,
        r####"{"agent_toolbar_enabled": false, "pty_rows_delta": 1, "resize_pane_sent": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/app.rs::open_agent_toolbar_model_submenu_anchors_below_the_clicked_tier_button
#[test]
fn port_app_open_agent_toolbar_model_submenu_anchors_below_the_clicked_tier_button() {
    check(
        Place::Client,
        "toolbar.submenu",
        r####"{"tier": 2}"####,
        r####"{"height": 7, "x": "anchor.x", "y": "anchor.bottom", "selected_index": 0}"####,
    );
}

/// REQ: PORT:ilium-client/src/app.rs::pixel_header_toolbar_action_rebuilds_rendered_markdown_as_plain_text
#[test]
fn port_app_pixel_header_toolbar_action_rebuilds_rendered_markdown_as_plain_text() {
    check(
        Place::Client,
        "toolbar.editor",
        r####"{"view": "rendered", "action": "toggle-heading-rendering"}"####,
        r####"{"text": "Portable title"}"####,
    );
}

/// REQ: PORT:ilium-client/src/app.rs::toolbar_agent_actions_explain_themselves_on_a_dead_agent_pane
#[test]
fn port_app_toolbar_agent_actions_explain_themselves_on_a_dead_agent_pane() {
    check(
        Place::Client,
        "toolbar.click",
        r####"{"pane": "dead-codex", "actions": ["stop", "copy-screen"]}"####,
        r####"{"stop": {"sent": null, "status": "Stop unavailable: the agent in this pane is no longer running"}, "copy-screen": {"status_not_prefixed": "Screen unavailable:"}}"####,
    );
}

/// REQ: PORT:ilium-client/src/app.rs::agent_toolbar_actions_send_the_bytes_their_tooltip_promises
#[test]
fn port_app_model_action_sends_the_sonnet_slash_command_as_toolbar_text() {
    check(
        Place::Client,
        "toolbar.action",
        r####"{"action": "model", "index": 1, "pane": "live_agent"}"####,
        r####"{"text": "/model sonnet", "source": "toolbar-action"}"####,
    );
}

/// REQ: PORT:ilium-client/src/app.rs::closing_the_agent_toolbar_hides_it_and_resizes_the_pty_back_up
#[test]
fn port_app_second_close_of_the_toolbar_sends_no_further_resize() {
    check(
        Place::Client,
        "toolbar.action",
        r####"{"action": "close", "repeat": true, "pane": "live_agent"}"####,
        r####"{"resize_sent_on_second_close": false}"####,
    );
}

/// REQ: PORT:ilium-client/src/app.rs::open_agent_toolbar_model_submenu_anchors_below_the_clicked_tier_button
#[test]
fn port_app_submenu_opened_near_the_screen_bottom_is_clamped_on_screen() {
    check(
        Place::Client,
        "toolbar.submenu",
        r####"{"tier_index": 0, "anchor": [100, 38], "screen": [0, 0, 120, 40]}"####,
        r####"{"area": [90, 32, 30, 8]}"####,
    );
}

/// REQ: PORT:ilium-client/src/app.rs::pixel_header_toolbar_action_rebuilds_rendered_markdown_as_plain_text
#[test]
fn port_app_view_rendered_action_builds_the_rendered_document() {
    check(
        Place::Client,
        "toolbar.action",
        r####"{"actions": ["ViewRendered"], "document": "# Portable title"}"####,
        r####"{"rendered_built": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/app.rs::toolbar_agent_actions_explain_themselves_on_a_dead_agent_pane
#[test]
fn port_app_compact_on_a_dead_agent_pane_sends_nothing() {
    check(
        Place::Client,
        "toolbar.action",
        r####"{"action": "compact", "pane": "dead_agent"}"####,
        r####"{"outbound": []}"####,
    );
}
