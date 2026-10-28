//! Twins of mouse.rs: toolbar clicks routed by the mouse handler.
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

/// REQ: PORT:ilium-client/src/mouse.rs::antigravity_toolbar_action_launches_its_registered_command
#[test]
fn port_mouse_antigravity_toolbar_action_launches_its_registered_command_alternate() {
    check(
        Place::Client,
        "toolbar.click",
        r####"{"provider": "antigravity", "refused": true}"####,
        r####"{"new_pane_command": null}"####,
    );
}

/// REQ: PORT:ilium-client/src/mouse.rs::pending_toolbar_buttons_have_distinct_bits_and_finish_once
#[test]
fn port_mouse_pending_toolbar_buttons_have_distinct_bits_and_finish_once_alternate() {
    check(
        Place::Client,
        "toolbar.pending",
        r####"{"begin": ["claude"], "finish": ["claude", "claude"]}"####,
        r####"{"pending_mask": 0, "second_finish_is_noop": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/mouse.rs::refused_toolbar_agent_click_keeps_the_refusal_and_shows_no_spinner
#[test]
fn port_mouse_refused_toolbar_agent_click_keeps_the_refusal_and_shows_no_spinner_alternate() {
    check(
        Place::Client,
        "toolbar.click",
        r####"{"refused": false, "provider": "claude"}"####,
        r####"{"pending_mask_nonzero": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/mouse.rs::search_toolbar_action_opens_the_full_screen_workspace_finder
#[test]
fn port_mouse_search_toolbar_action_opens_the_full_screen_workspace_finder_alternate() {
    check(
        Place::Client,
        "toolbar.click",
        r####"{"action": "search", "after": "esc"}"####,
        r####"{"mode": "tree"}"####,
    );
}

/// REQ: PORT:ilium-client/src/mouse.rs::settings_toolbar_press_and_release_keep_the_settings_view_open
#[test]
fn port_mouse_settings_toolbar_press_and_release_keep_the_settings_view_open_alternate() {
    check(
        Place::Client,
        "toolbar.click",
        r####"{"action": "settings", "release_without_press": true}"####,
        r####"{"mode": "tree"}"####,
    );
}

/// REQ: PORT:ilium-client/src/mouse.rs::antigravity_toolbar_action_launches_its_registered_command
#[test]
fn port_mouse_antigravity_toolbar_action_launches_its_registered_command() {
    check(
        Place::Client,
        "toolbar.click",
        r####"{"provider": "antigravity"}"####,
        r####"{"new_pane_command": "agy"}"####,
    );
}

/// REQ: PORT:ilium-client/src/mouse.rs::pending_toolbar_buttons_have_distinct_bits_and_finish_once
#[test]
fn port_mouse_pending_toolbar_buttons_have_distinct_bits_and_finish_once() {
    check(
        Place::Client,
        "toolbar.pending",
        r####"{}"####,
        r####"{"distinct_bits": true, "finish_clears_only_own_bit": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/mouse.rs::refused_toolbar_agent_click_keeps_the_refusal_and_shows_no_spinner
#[test]
fn port_mouse_refused_toolbar_agent_click_keeps_the_refusal_and_shows_no_spinner() {
    check(
        Place::Client,
        "toolbar.click",
        r####"{"refused": true}"####,
        r####"{"status_prefix": "Request rejected", "pending_mask": 0}"####,
    );
}

/// REQ: PORT:ilium-client/src/mouse.rs::search_toolbar_action_opens_the_full_screen_workspace_finder
#[test]
fn port_mouse_search_toolbar_action_opens_the_full_screen_workspace_finder() {
    check(
        Place::Client,
        "toolbar.click",
        r####"{"action": "search"}"####,
        r####"{"mode": "search"}"####,
    );
}

/// REQ: PORT:ilium-client/src/mouse.rs::settings_toolbar_press_and_release_keep_the_settings_view_open
#[test]
fn port_mouse_settings_toolbar_press_and_release_keep_the_settings_view_open() {
    check(
        Place::Client,
        "toolbar.click",
        r####"{"action": "settings", "press_release": true}"####,
        r####"{"mode": "settings"}"####,
    );
}
