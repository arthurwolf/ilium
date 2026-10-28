//! Port twins of the tests of `app.rs`.

use serde_json::json;

use crate::common::*;

/// REQ: PORT:ilium-client/src/app.rs::configured_none_and_user_titles_prevent_automatic_retitle_work
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

/// REQ: PORT:ilium-client/src/app.rs::completed_restructure_applies_a_plan_when_the_project_changed_in_flight, PORT:ilium-client/src/app.rs::pending_restructure_retains_original_title_observation_after_manual_rename
#[test]
fn port_043_completed_restructure_applies_a_plan() {
    let host = organised_host(&plan_reply());
    let specs = restructures(&host.submitted());
    assert_eq!(specs.len(), 1);
    assert!(!specs[0].label.is_empty());
    assert!(specs[0].operations.len() <= 256);
}

/// REQ: PORT:ilium-client/src/app.rs::automatic_restructure_failures_back_off_but_manual_and_changed_input_bypass_them, PORT:ilium-client/src/app.rs::automatic_restructure_retry_delay_is_bounded_exponential_backoff
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

/// REQ: PORT:ilium-client/src/app.rs::finished_trigger_routes_retitle_and_owning_project_restructure, PORT:ilium-client/src/app.rs::global_restructure_queues_one_worker_per_nonempty_project_and_skips_empty_projects, PORT:ilium-client/src/app.rs::successful_checkpoint_makes_the_next_restructure_skip_unchanged_project
#[test]
fn port_044_finish_trigger_starts_restructure() {
    let mut host = host_with_two_panes();
    host.deliver_one(finished(1));
    host.settle();
    assert!(!restructure_requests(&host).is_empty());
}

/// REQ: PORT:ilium-client/src/app.rs::removed_project_prunes_its_restructure_job_and_retry_state
#[test]
fn port_043_no_panes_no_restructure_request() {
    let mut host = activated_server();
    host.deliver_one(finished(1));
    host.settle();
    assert!(restructure_requests(&host).is_empty());
}

/// REQ: PORT:ilium-client/src/app.rs::configured_none_and_user_titles_prevent_automatic_retitle_work
#[test]
fn port_045_default_trigger_starts_inference_for_the_same_prompt() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    assert!(!host.inference_requests().is_empty());
}

/// REQ: PORT:ilium-client/src/app.rs::completed_restructure_applies_a_plan_when_the_project_changed_in_flight
#[test]
fn port_046_completed_plan_submits_operations_for_the_planned_panes() {
    let host = organised_host(&plan_reply());
    let specs = restructures(&host.submitted());
    assert_eq!(specs.len(), 1);
    assert!(!specs[0].operations.is_empty());
}

/// REQ: PORT:ilium-client/src/app.rs::pending_restructure_retains_original_title_observation_after_manual_rename
#[test]
fn port_047_manual_rename_while_pending_still_lets_the_plan_apply() {
    let mut host = host_with_two_panes();
    host.deliver_one(finished(1));
    host.settle();
    host.tree().rename(1, "manual name");
    host.settle();
    let (id, _) = restructure_requests(&host).remove(0);
    host.complete_inference(id, completion(&plan_reply()));
    host.settle();
    assert_eq!(restructures(&host.submitted()).len(), 1);
}

/// REQ: PORT:ilium-client/src/app.rs::automatic_restructure_failures_back_off_but_manual_and_changed_input_bypass_them
#[test]
fn port_048_added_pane_bypasses_the_failure_backoff() {
    let mut host = rejected_host("garbage");
    let attempts = restructure_requests(&host).len();
    host.tree().add(pane("shell-3", None));
    host.settle();
    host.deliver_one(finished(1));
    host.settle();
    assert!(restructure_requests(&host).len() > attempts);
}

/// REQ: PORT:ilium-client/src/app.rs::automatic_restructure_retry_delay_is_bounded_exponential_backoff
#[test]
fn port_049_retry_after_a_long_quiet_period_makes_exactly_one_request() {
    let mut host = rejected_host("garbage");
    let attempts = restructure_requests(&host).len();
    host.advance_clock(24 * 60 * 60 * 1_000);
    host.deliver_one(finished(1));
    host.settle();
    assert_eq!(restructure_requests(&host).len(), attempts + 1);
}

/// REQ: PORT:ilium-client/src/app.rs::finished_trigger_routes_retitle_and_owning_project_restructure
#[test]
fn port_050_started_work_does_not_route_to_restructure() {
    let mut host = host_with_two_panes();
    host.deliver_one(started_working(1));
    host.settle();
    assert!(restructure_requests(&host).is_empty());
}

/// REQ: PORT:ilium-client/src/app.rs::global_restructure_queues_one_worker_per_nonempty_project_and_skips_empty_projects
#[test]
fn port_051_two_finished_panes_queue_one_restructure_per_project() {
    let mut host = host_with_two_panes();
    host.deliver_one(finished(1));
    host.deliver_one(finished(2));
    host.settle();
    assert_eq!(restructure_requests(&host).len(), 1);
}

/// REQ: PORT:ilium-client/src/app.rs::removed_project_prunes_its_restructure_job_and_retry_state
#[test]
fn port_052_pending_plan_is_dropped_when_its_panes_are_removed() {
    let mut host = host_with_two_panes();
    host.deliver_one(finished(1));
    host.settle();
    let (id, _) = restructure_requests(&host).remove(0);
    host.tree().remove(1);
    host.tree().remove(2);
    host.settle();
    host.complete_inference(id, completion(&plan_reply()));
    host.settle();
    assert!(restructures(&host.submitted()).is_empty());
}

/// REQ: PORT:ilium-client/src/app.rs::successful_checkpoint_makes_the_next_restructure_skip_unchanged_project
#[test]
fn port_053_unchanged_project_after_a_completed_restructure_is_skipped() {
    let mut host = organised_host(&plan_reply());
    let before = restructure_requests(&host).len();
    host.deliver_one(finished(1));
    host.settle();
    assert_eq!(restructure_requests(&host).len(), before);
}
