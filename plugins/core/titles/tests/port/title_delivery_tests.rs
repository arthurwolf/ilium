//! Port twins of the tests of `title_delivery_tests.rs`.
#![allow(unused_imports)]

use ilium_plugin_sdk::types::*;
use serde_json::json;

use crate::common::*;

/// REQ: PORT:ilium-server/src/ipc/title_delivery_tests.rs::acknowledged_body_without_enter_cannot_establish_an_authored_title_receipt, PORT:ilium-server/src/ipc/title_delivery_tests.rs::automatic_and_explicit_session_titles_require_eligibility_and_preserve_manual_names, PORT:ilium-server/src/ipc/title_delivery_tests.rs::launch_intent_before_detection_cannot_use_the_plain_shell_title_bypass, PORT:ilium-server/src/ipc/title_delivery_tests.rs::synthetic_submission_sources_and_controls_never_create_task_receipts, PORT:ilium-server/src/ipc/title_delivery_tests.rs::unavailable_agent_title_grants_require_verified_own_project_history
#[test]
fn port_041_inferred_title_is_applied_once() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "  Login fix  ");
    let applied = renames(&host.submitted());
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].title, "Login fix");
    assert_eq!(applied[0].source, TitleSource::Inferred);
}
