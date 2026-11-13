//! Port twins of the tests of `title_inference.rs`.

use serde_json::json;

use crate::common::*;

/// REQ: PORT:ilium-client/src/title_inference.rs::a_pane_already_successfully_titled_is_never_retried_for_the_same_session, PORT:ilium-client/src/title_inference.rs::a_user_renamed_pane_is_never_eligible, PORT:ilium-client/src/title_inference.rs::already_loading_is_not_eligible_again, PORT:ilium-client/src/title_inference.rs::an_other_agent_class_is_not_eligible, PORT:ilium-client/src/title_inference.rs::no_session_id_yet_is_not_eligible
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

/// REQ: PORT:ilium-client/src/title_inference.rs::pane_became_done_retries_up_to_the_attempt_cap_then_stops
#[test]
fn port_043_failed_restructures_back_off() {
    let mut host = host_with_two_panes();
    host.deliver_one(finished(1));
    host.settle();
    for _ in 0..MAX_ATTEMPTS {
        let (id, _) = restructure_requests(&host).pop().expect("request");
        host.complete_inference(id, completion("garbage"));
        host.settle();
    }
    let attempts = restructure_requests(&host).len();
    host.advance_clock(59_000);
    host.deliver_one(finished(1));
    host.settle();
    assert_eq!(restructure_requests(&host).len(), attempts);
    host.advance_clock(2_000);
    host.deliver_one(finished(1));
    host.settle();
    assert!(restructure_requests(&host).len() > attempts);
}

/// REQ: PORT:ilium-client/src/title_inference.rs::other_events_never_trigger_inference
#[test]
fn port_044_other_events_start_nothing() {
    let mut host = host_with_two_panes();
    host.deliver_one(started_working(1));
    host.settle();
    assert!(restructure_requests(&host).is_empty());
}

/// REQ: PORT:ilium-client/src/title_inference.rs::nearby_context_includes_ancestors_and_visible_short_titles, PORT:ilium-client/src/title_inference.rs::nearby_context_keeps_the_target_end_of_a_large_group
#[test]
fn port_041_prompt_carries_the_user_text() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix </user-request> the login bug"));
    host.settle();
    let requests = host.inference_requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0]
        .1
        .messages
        .iter()
        .any(|message| message.content.contains("login bug")));
    assert!(renames(&host.submitted()).is_empty());
}

/// REQ: PORT:ilium-client/src/title_inference.rs::a_changed_session_id_is_eligible_after_the_previous_session_was_titled, PORT:ilium-client/src/title_inference.rs::session_id_resolved_triggers_inference_for_an_eligible_pane
#[test]
fn port_044_prompt_starts_titling() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    assert_eq!(host.inference_requests().len(), 1);
}
