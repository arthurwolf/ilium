//! Twins of the icon stepping tests of ilium-client/src/{app,mouse}.rs: arrows and pointer buttons step the
//! full icon catalogue and open its picker. Request/reply shape: see API_GAPS_ports.md (ops `icon-picker.cycle`,
//! `icon-picker.pointer`, `icon-picker.status-controls`).
use crate::common::{ask, Place};
use serde_json::Value;

fn check(op: &str, request: &str, expected: &str) {
    let request: Value = serde_json::from_str(request).expect("request literal parses");
    let expected: Value = serde_json::from_str(expected).expect("expected literal parses");
    let reply = ask(Place::Client, op, request);
    let fields = expected.as_object().expect("expected is an object");
    assert!(
        !fields.is_empty(),
        "the expectation names at least one field"
    );
    for (name, value) in fields {
        assert_eq!(
            &reply[name.as_str()],
            value,
            "field {name} of reply {reply}"
        );
    }
}

/// REQ: PORT:ilium-client/src/app.rs::icon_arrows_cycle_the_full_catalog_in_both_directions
#[test]
fn port_icon_cycle_arrows_step_the_full_catalogue_forward_back_and_wrap_at_both_ends() {
    check(
        "icon-picker.cycle",
        r####"{"from": "first", "step": 1}"####,
        r####"{"lands_on": "second"}"####,
    );
    check(
        "icon-picker.cycle",
        r####"{"from": "second", "step": -1}"####,
        r####"{"lands_on": "first"}"####,
    );
    check(
        "icon-picker.cycle",
        r####"{"from": "last", "step": 1}"####,
        r####"{"lands_on": "first"}"####,
    );
    check(
        "icon-picker.cycle",
        r####"{"from": "first", "step": -1}"####,
        r####"{"lands_on": "last"}"####,
    );
}

/// REQ: PORT:ilium-client/src/mouse.rs::assignment_pointer_preserves_label_and_reverses_value_then_opens_full_catalog
#[test]
fn port_icon_cycle_pointer_on_the_label_keeps_the_glyph_the_value_steps_both_ways_and_open_shows_the_catalogue(
) {
    check(
        "icon-picker.pointer",
        r####"{"part": "label", "button": "left", "start_index": 0}"####,
        r####"{"index": 0, "picker_open": false}"####,
    );
    check(
        "icon-picker.pointer",
        r####"{"part": "value", "button": "left", "start_index": 0}"####,
        r####"{"index": 1, "picker_open": false}"####,
    );
    check(
        "icon-picker.pointer",
        r####"{"part": "value", "button": "right", "start_index": 1}"####,
        r####"{"index": 0, "picker_open": false}"####,
    );
    check(
        "icon-picker.pointer",
        r####"{"part": "open", "button": "left", "start_index": 0}"####,
        r####"{"index": 0, "picker_open": true, "catalogue": "full"}"####,
    );
}

/// REQ: PORT:ilium-client/src/mouse.rs::status_icon_controls_step_in_both_directions_and_open_the_catalog
#[test]
fn port_icon_cycle_status_icon_controls_step_both_ways_wrap_through_the_suggestions_and_open_the_picker(
) {
    check(
        "icon-picker.status-controls",
        r####"{"control": "value", "button": "left", "start_index": 0}"####,
        r####"{"index": 1}"####,
    );
    check(
        "icon-picker.status-controls",
        r####"{"control": "value", "button": "right", "start_index": 1}"####,
        r####"{"index": 0}"####,
    );
    check(
        "icon-picker.status-controls",
        r####"{"control": "previous", "button": "left", "start_index": 0}"####,
        r####"{"index": "last_suggestion"}"####,
    );
    check(
        "icon-picker.status-controls",
        r####"{"control": "next", "button": "left", "start_index": "last_suggestion"}"####,
        r####"{"index": 0}"####,
    );
    check(
        "icon-picker.status-controls",
        r####"{"control": "open", "button": "left", "start_index": 0}"####,
        r####"{"index": 0, "picker_open": true}"####,
    );
}
