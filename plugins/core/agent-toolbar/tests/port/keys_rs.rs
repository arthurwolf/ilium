//! Twins of the icon picker key tests of ilium-client/src/keys.rs. Request/reply shape: see API_GAPS_ports.md
//! (ops `icon-picker.status-controls` and `icon-picker.semantic-search`).
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

/// REQ: F-081, PORT:ilium-client/src/keys.rs::plus_opens_the_full_catalog_for_the_selected_status_icon
#[test]
fn port_keys_plus_opens_the_full_catalog_for_the_selected_status_icon() {
    check(
        Place::Client,
        "icon-picker.status-controls",
        r####"{"control": "open", "key": "+", "row": "status-icon", "target_index": 0}"####,
        r####"{"picker_open": true, "catalogue": "full", "target_index": 0}"####,
    );
}

/// REQ: F-081, PORT:ilium-client/src/keys.rs::icon_picker_queues_cpu_semantic_search_and_selects_its_result
#[test]
fn port_keys_icon_picker_queues_cpu_semantic_search_and_selects_its_result() {
    check(
        Place::Client,
        "icon-picker.semantic-search",
        r####"{"query": "rocket", "reply": [{"glyph": "🚀", "label": "rocket", "category": "Travel · Air"}], "confirm_keys": ["enter", "enter"]}"####,
        r####"{"query": "rocket", "status_while_waiting": "searching", "requests_queued": 1, "selected": "🚀", "stored_group_icon": "🚀"}"####,
    );
}
