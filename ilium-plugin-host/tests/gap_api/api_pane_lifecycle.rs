//! Pane lifecycle page (API-216): an import aimed at a closed pane is refused, never redirected to a reused pane.

use ilium_plugin_host::testing::harness::HostHarness;
use serde_json::json;

/// REQ: API-216
#[test]
fn api_216_import_targeting_a_pane_closed_before_apply_is_refused_not_found() {
    let handle = HostHarness::new()
        .grants(&["terminal.write"])
        .build()
        .expect("build harness");
    let pane_id = handle.open_pane_for_test("notes-pane");
    handle.close_pane_for_test(pane_id);
    let outcome =
        handle.submit_pane_import_for(pane_id, "pane.send-keys", json!({"text": "hello"}));
    assert_eq!(
        outcome.refusal_code().as_deref(),
        Some("not-found"),
        "an import targeting a closed pane must complete as refused(not-found)"
    );
}

/// REQ: API-216
#[test]
fn api_216_reused_screen_position_pane_is_not_affected_by_stale_import() {
    let handle = HostHarness::new()
        .grants(&["terminal.write"])
        .build()
        .expect("build harness");
    let closed_pane = handle.open_pane_for_test("old-pane");
    let screen_slot = handle.screen_slot_of(closed_pane);
    handle.close_pane_for_test(closed_pane);
    let reused_pane = handle.open_pane_in_slot_for_test("new-pane", screen_slot);
    let outcome =
        handle.submit_pane_import_for(closed_pane, "pane.send-keys", json!({"text": "stale"}));
    assert!(
        outcome.refusal_code().is_some(),
        "the stale import must be refused"
    );
    assert_eq!(
        handle.pane_input_log(reused_pane).len(),
        0,
        "the pane that reuses the screen position must receive nothing from the stale import"
    );
}
