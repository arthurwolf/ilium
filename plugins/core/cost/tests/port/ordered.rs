//! Twin of the cost-related test in ilium-client/src/filesystem/ordered.rs: the ordered filesystem writer's
//! admission of configuration writes.

use serde_json::{json, Value};

use crate::common::client;

const MIB: u64 = 1024 * 1024;

/// REQ: PORT:ilium-client/src/filesystem/ordered.rs::original_configuration_cost_refuses_seventh_write_locally_before_capture
#[test]
fn port_ordered_original_configuration_cost_refuses_seventh_write_locally_before_capture() {
    let reply = client(
        "filesystem.ordered-config-writes",
        json!({ "enqueue": 7, "retained_limit_bytes": 64 * MIB }),
    );
    assert_eq!(reply["cost"]["input_bytes"], json!(8 * MIB));
    assert_eq!(reply["cost"]["result_bytes"], json!(2 * MIB));
    assert_eq!(reply["pending_after_six"], json!(6));
    assert_eq!(reply["retained_after_six"], json!(60 * MIB));
    assert_eq!(
        reply["seventh_failure"],
        json!({ "retained_bytes": { "requested": 10 * MIB, "used": 60 * MIB, "limit": 64 * MIB } })
    );
    // The refused value is handed back untouched and the quota is unchanged.
    assert_eq!(reply["refused_value_returned_unchanged"], json!(true));
    assert_eq!(reply["quota_input_bytes_unchanged"], json!(true));
    assert_eq!(reply["quota_jobs_unchanged"], json!(true));
    // The refusal precedes the authored capture: a lazily built job is never built.
    assert_eq!(reply["lazy_builder_called"], json!(false));
    assert_eq!(reply["pending_after_refusals"], json!(6));
    let _: &Value = &reply;
}
