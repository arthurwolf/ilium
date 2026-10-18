//! Port twins of the tests of `terminal_title_inference.rs`.

use serde_json::json;

use crate::common::*;

/// REQ: PORT:ilium-client/src/terminal_title_inference.rs::a_pane_already_loading_is_not_eligible_again, PORT:ilium-client/src/terminal_title_inference.rs::a_user_renamed_pane_is_never_eligible, PORT:ilium-client/src/terminal_title_inference.rs::an_agent_pane_is_never_eligible, PORT:ilium-client/src/terminal_title_inference.rs::an_editor_pane_is_never_eligible, PORT:ilium-client/src/terminal_title_inference.rs::an_unknown_node_id_is_not_eligible
#[test]
fn port_044_cleared_trigger_starts_no_inference() {
    let mut host = activated_server();
    host.settings()
        .set(PROMPT_EVENT, json!([]))
        .expect("setting accepted");
    host.settle();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    assert!(host.inference_requests().is_empty());
    assert!(renames(&host.submitted()).is_empty());
}

/// REQ: PORT:ilium-client/src/terminal_title_inference.rs::a_plain_shell_pane_is_eligible
#[test]
fn port_044_plain_shell_is_eligible() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    assert_eq!(host.inference_requests().len(), 1);
}
