//! Twin of the cost-related test in ilium-server/src/ipc/handlers.rs: visible-pane recovery admission.

use serde_json::json;

use crate::common::server;

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::visible_recovery_uses_reserved_storage_when_general_quota_is_full
#[test]
fn port_ipc_handlers_visible_recovery_uses_reserved_storage_when_general_quota_is_full() {
    let reply = server(
        "ipc.reserve-event-storage",
        json!({ "fill_general_quota": true, "class": "visible_pane_recovery", "pane": 1, "events": 1, "timeout_ms": 1000 }),
    );
    // The recovery admission must bypass unrelated general storage pressure and own at least two recovery events.
    assert_eq!(reply["admitted"], json!(true));
    assert_eq!(reply["general_worker_bytes_full"], json!(true));
    assert_eq!(reply["resident_at_least_two_events"], json!(true));
}
