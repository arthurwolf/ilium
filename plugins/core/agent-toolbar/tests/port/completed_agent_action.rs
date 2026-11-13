//! Twins of completed_agent_action.rs: where the close action of a completed agent pane sits.
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

/// REQ: PORT:ilium-client/src/completed_agent_action.rs::omits_the_action_when_the_pane_has_no_inner_cells
#[test]
fn port_completed_agent_action_omits_the_action_when_the_pane_has_no_inner_cells() {
    check(
        Place::Client,
        "layout.completed-action",
        r####"{"outer": [10, 4, 40, 12], "content": [11, 5, 0, 0]}"####,
        r####"{"layout": null}"####,
    );
}

/// REQ: PORT:ilium-client/src/completed_agent_action.rs::reserves_the_full_bottom_inner_row_for_the_close_action
#[test]
fn port_completed_agent_action_reserves_the_full_bottom_inner_row_for_the_close_action() {
    check(
        Place::Client,
        "layout.completed-action",
        r####"{"outer": [10, 4, 40, 12], "content": [11, 5, 38, 10]}"####,
        r####"{"terminal_area": [11, 5, 38, 9], "button_area": [11, 14, 38, 1]}"####,
    );
}
