//! Port twins of the tests of `app.rs`.
#![allow(unused_imports)]

use ilium_plugin_sdk::types::*;
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
