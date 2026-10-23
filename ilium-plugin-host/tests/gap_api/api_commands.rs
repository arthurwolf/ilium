//! Commands page (API-214): duplicate command ids conflict; unregistering during invocation lets it finish.

use ilium_plugin_host::testing::harness::HostHarness;
use serde_json::json;

/// REQ: API-214
#[test]
fn api_214_same_command_id_from_second_plugin_returns_conflict_and_keeps_both() {
    let handle = HostHarness::new()
        .two_plugins()
        .build()
        .expect("build harness");
    let first = handle.call_import(
        "commands.register",
        json!({"id": "notes.open", "plugin": "com.example.notes"}),
    );
    assert!(
        first.error_code().is_none(),
        "the first registration must succeed"
    );
    let second = handle.call_import(
        "commands.register",
        json!({"id": "notes.open", "plugin": "com.example.other"}),
    );
    assert_eq!(
        second.error_code().as_deref(),
        Some("conflict"),
        "registering an id another plugin owns must return conflict"
    );
    assert_eq!(
        handle.command_owner("notes.open").as_deref(),
        Some("com.example.notes"),
        "both registrations must stay unchanged: the first owner keeps the id"
    );
    assert_eq!(
        handle.command_registration_count("notes.open"),
        1,
        "the conflicting registration must not be added"
    );
}

/// REQ: API-214
#[test]
fn api_214_unregister_during_own_invocation_lets_handler_finish_and_deliver_result() {
    let handle = HostHarness::new()
        .grants(&["commands.invoke"])
        .build()
        .expect("build harness");
    let outcome = handle.invoke_command_that_unregisters_itself("notes.self-remove");
    assert!(
        outcome.handler_completed(),
        "the handler must run to completion after unregistering itself"
    );
    assert!(
        outcome.result_delivered(),
        "the invocation result must still be delivered to the caller"
    );
    assert!(
        !handle.command_is_registered("notes.self-remove"),
        "the command must be gone once the invocation finishes"
    );
}
