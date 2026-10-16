//! Twins of tree_ui.rs: the tree toolbar.
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

/// REQ: PORT:ilium-client/src/tree_ui.rs::toolbar_and_row_controls_use_their_configured_icons
#[test]
fn port_tree_ui_toolbar_and_row_controls_use_their_configured_icons() {
    check(
        Place::Client,
        "tree-toolbar.icons",
        r####"{}"####,
        r####"{"follows_configured_icons": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::toolbar_is_visible_for_tree_focus_or_footer_hover
#[test]
fn port_tree_ui_toolbar_is_visible_for_tree_focus_or_footer_hover() {
    check(
        Place::Client,
        "tree-toolbar.visible",
        r####"{"cases": [[true, false], [false, true], [true, true], [false, false]]}"####,
        r####"{"visible": [true, true, true, false]}"####,
    );
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::toolbar_keeps_settings_right_aligned_without_overlapping_left_actions
#[test]
fn port_tree_ui_toolbar_keeps_settings_right_aligned_without_overlapping_left_actions() {
    check(
        Place::Client,
        "tree-toolbar.layout",
        r####"{"width": 16}"####,
        r####"{"toolbar_width": 14, "project_x": 4, "restructure_x": 8, "settings_x": [12, 13], "overlap": false}"####,
    );
}
