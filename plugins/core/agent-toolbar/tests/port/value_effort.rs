//! Twins of value_effort.rs: the effort value control of the Claude toolbar.
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

/// REQ: PORT:ilium-client/src/value_effort.rs::all_native_efforts_and_reverse_order_are_preserved
#[test]
fn port_value_effort_all_native_efforts_and_reverse_order_are_preserved() {
    check(
        Place::Client,
        "effort.options",
        r####"{}"####,
        r####"{"count": 7, "next_previous_round_trip": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/value_effort.rs::choice_routes_exact_command_and_ultracode_without_submitting_user_prompt
#[test]
fn port_value_effort_choice_routes_exact_command_and_ultracode_without_submitting_user_prompt() {
    check(
        Place::Client,
        "effort.choose",
        r####"{"choose": "Max", "then": ["previous", "next"]}"####,
        r####"{"max": {"submit_terminal_text": "/effort max", "source": "toolbar-action"}, "back_to_ultracode": {"key_input": "ultracode ", "submission": null}, "forward": "max"}"####,
    );
}

/// REQ: PORT:ilium-client/src/value_effort.rs::refused_command_keeps_original_effort_and_catalog_for_retry
#[test]
fn port_value_effort_refused_command_keeps_original_effort_and_catalog_for_retry() {
    check(
        Place::Client,
        "effort.choose",
        r####"{"choose": "Max", "command_refused": true}"####,
        r####"{"effort": "auto", "catalog_kept": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/value_effort.rs::stale_effort_or_changed_provider_is_refused_without_command
#[test]
fn port_value_effort_stale_effort_or_changed_provider_is_refused_without_command() {
    check(
        Place::Client,
        "effort.choose",
        r####"{"choose": "Max", "stale_or_provider": "codex"}"####,
        r####"{"refused": true, "command": null}"####,
    );
}

/// REQ: PORT:ilium-client/src/value_effort.rs::all_native_efforts_and_reverse_order_are_preserved
#[test]
fn port_value_effort_all_native_efforts_and_reverse_order_are_preserved_without_duplicates() {
    check(
        Place::Client,
        "effort.options",
        r####"{}"####,
        r####"{"count": 7, "duplicates": 0}"####,
    );
}

/// REQ: PORT:ilium-client/src/value_effort.rs::choice_routes_exact_command_and_ultracode_without_submitting_user_prompt
#[test]
fn port_value_effort_choice_routes_exact_command_and_ultracode_without_submitting_user_prompt_when_already_active(
) {
    check(
        Place::Client,
        "effort.choose",
        r####"{"choose": "Max", "already_active": true}"####,
        r####"{"command_sent": false, "submit_terminal_text": null}"####,
    );
}

/// REQ: PORT:ilium-client/src/value_effort.rs::refused_command_keeps_original_effort_and_catalog_for_retry
#[test]
fn port_value_effort_refused_command_keeps_original_effort_and_catalog_for_retry_offers_retry() {
    check(
        Place::Client,
        "effort.choose",
        r####"{"choose": "Max", "command_refused": true}"####,
        r####"{"retry_available": true, "effort": "auto"}"####,
    );
}

/// REQ: PORT:ilium-client/src/value_effort.rs::stale_effort_or_changed_provider_is_refused_without_command
#[test]
fn port_value_effort_stale_effort_or_changed_provider_is_refused_without_command_for_stale_effort()
{
    check(
        Place::Client,
        "effort.choose",
        r####"{"choose": "Max", "stale": true, "stale_or_provider": "claude"}"####,
        r####"{"refused": true, "command": null, "reason": "stale-effort"}"####,
    );
}
