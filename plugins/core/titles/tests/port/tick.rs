//! Port twins of the tests of `tick.rs`.

use ilium_plugin_sdk::types::*;

use crate::common::*;

/// REQ: PORT:ilium-client/src/tick.rs::fresh_session_title_completion_refires_a_queued_manual_retitle, PORT:ilium-client/src/tick.rs::stale_manual_session_title_is_discarded_after_session_change, PORT:ilium-client/src/tick.rs::successful_title_debug_event_keeps_exact_rendered_request_and_raw_response
#[test]
fn port_041_inferred_title_is_applied_once() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "  Login fix  ");
    let applied = renames(&host.submitted());
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].title, "Login fix");
    assert_eq!(applied[0].source, TitleSource::Inferred);
}
