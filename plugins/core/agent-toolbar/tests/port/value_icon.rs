//! Twins of value_icon.rs: the icon picker value control and assignment rows.
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

/// REQ: PORT:ilium-client/src/value_icon.rs::both_layouts_are_complete_and_reopened_picker_is_rejected
#[test]
fn port_value_icon_both_layouts_are_complete_and_reopened_picker_is_rejected() {
    check(
        Place::Client,
        "icon-picker.layout",
        r####"{"modes": ["multi", "single"], "query": "authored query", "selected_entry": 3}"####,
        r####"{"complete": true, "query": "authored query", "selected_entry": 3, "reopen_rejected": true, "mode_after_reopen": "multi"}"####,
    );
}

/// REQ: PORT:ilium-client/src/value_icon.rs::icon_assignment_rows_render_exact_chrome_with_authored_glyphs_and_scroll
#[test]
fn port_value_icon_icon_assignment_rows_render_exact_chrome_with_authored_glyphs_and_scroll() {
    check(
        Place::Client,
        "icon-picker.rows",
        r####"{"widths": [60, 100, 170], "scroll": [0, 3]}"####,
        r####"{"chrome": "← + →", "at_hit_geometry": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/value_icon.rs::matching_search_reply_reaches_retained_picker_and_stale_revision_is_ignored
#[test]
fn port_value_icon_matching_search_reply_reaches_retained_picker_and_stale_revision_is_ignored() {
    check(
        Place::Client,
        "icon-picker.search",
        r####"{"query": "rocket"}"####,
        r####"{"matching_applied": true, "stale_ignored": true, "query": "rocket", "status": ["searching", "browse"]}"####,
    );
}

/// REQ: PORT:ilium-client/src/value_icon.rs::real_settings_renderer_paints_layout_chrome_at_hit_geometry
#[test]
fn port_value_icon_real_settings_renderer_paints_layout_chrome_at_hit_geometry() {
    check(
        Place::Client,
        "icon-picker.rows",
        r####"{"widths": [40, 80, 140]}"####,
        r####"{"chrome": "← + →", "at_hit_geometry": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/value_icon.rs::responsive_chrome_has_exact_forward_reverse_and_catalog_hits
#[test]
fn port_value_icon_responsive_chrome_has_exact_forward_reverse_and_catalog_hits() {
    check(
        Place::Client,
        "icon-picker.hits",
        r####"{}"####,
        r####"{"previous": "previous-choice", "next": "next-choice", "open": "open-choices", "value_left": "next-choice", "value_right": "previous-choice", "label": null}"####,
    );
}
