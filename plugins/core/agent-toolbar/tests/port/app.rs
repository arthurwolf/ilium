//! Twins of app.rs: what toolbar actions do in the client application (bytes sent, resize, submenu, dead pane).
//! Request/reply shape: see API_GAPS_ports.md.
use serde_json::Value;
use crate::common::{ask, Place};

fn check(place: Place, op: &str, request: &str, expected: &str) {
    let request: Value = serde_json::from_str(request).expect("request literal parses");
    let expected: Value = serde_json::from_str(expected).expect("expected literal parses");
    let reply = ask(place, op, request);
    let fields = expected.as_object().expect("expected is an object");
    for (name, value) in fields {
        assert_eq!(&reply[name.as_str()], value, "field {name} of reply {reply}");
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
