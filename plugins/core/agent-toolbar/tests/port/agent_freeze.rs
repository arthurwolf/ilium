//! Twins of ilium-client/src/agent_freeze.rs: the auto-freeze policy for finished and waiting agents.

use serde_json::json;

use crate::common::{ask, Place};

fn should_freeze(
    settings: serde_json::Value,
    activity: &str,
    inactive_secs: u64,
) -> serde_json::Value {
    let reply = ask(
        Place::Server,
        "freeze.should-freeze",
        json!({ "settings": settings, "activity": activity, "inactive_secs": inactive_secs }),
    );
    reply["freeze"].clone()
}

/// REQ: PORT:ilium-client/src/agent_freeze.rs::defaults_only_freeze_finished_agents_after_six_hours
#[test]
fn port_agent_freeze_defaults_only_freeze_finished_agents_after_six_hours() {
    let settings = json!({ "enabled": true });
    assert_eq!(
        should_freeze(settings.clone(), "done", 6 * 60 * 60),
        json!(true)
    );
    assert_eq!(
        should_freeze(settings.clone(), "idle", 99 * 60 * 60),
        json!(false)
    );
    assert_eq!(
        should_freeze(settings, "working", 99 * 60 * 60),
        json!(false)
    );
}

/// REQ: PORT:ilium-client/src/agent_freeze.rs::unfreeze_resets_even_if_the_agent_stays_done
#[test]
fn port_agent_freeze_unfreeze_resets_even_if_the_agent_stays_done() {
    let reply = ask(
        Place::Server,
        "freeze.clock",
        json!({ "observations": [{ "elapsed_secs": 20, "activity": "done" }], "reset_on_unfreeze": true }),
    );
    assert_eq!(reply["inactive_secs"], json!(0));
}

/// REQ: PORT:ilium-client/src/agent_freeze.rs::waiting_state_options_are_independent
#[test]
fn port_agent_freeze_waiting_state_options_are_independent() {
    let settings = json!({
        "enabled": true,
        "after_secs": 10,
        "freeze_done": false,
        "freeze_waiting_for_input": true,
        "freeze_waiting_for_approval": false,
        "freeze_waiting_for_background": true
    });
    assert_eq!(should_freeze(settings.clone(), "idle", 10), json!(true));
    assert_eq!(
        should_freeze(settings.clone(), "waiting-approval", 10),
        json!(false)
    );
    assert_eq!(
        should_freeze(settings, "waiting-background", 10),
        json!(true)
    );
}

/// REQ: PORT:ilium-client/src/agent_freeze.rs::working_activity_clears_elapsed_time
#[test]
fn port_agent_freeze_working_activity_clears_elapsed_time() {
    let reply = ask(
        Place::Server,
        "freeze.clock",
        json!({
            "observations": [
                { "elapsed_secs": 20, "activity": "done" },
                { "elapsed_secs": 1, "activity": "working" }
            ],
            "reset_on_unfreeze": false
        }),
    );
    assert_eq!(reply["inactive_secs"], json!(0));
}

/// REQ: PORT:ilium-client/src/agent_freeze.rs::defaults_only_freeze_finished_agents_after_six_hours
#[test]
fn port_agent_freeze_master_switch_off_never_freezes_even_a_finished_agent() {
    assert_eq!(
        should_freeze(json!({ "enabled": false }), "done", 99 * 60 * 60),
        json!(false)
    );
}

/// REQ: PORT:ilium-client/src/agent_freeze.rs::unfreeze_resets_even_if_the_agent_stays_done
#[test]
fn port_agent_freeze_without_unfreeze_reset_the_done_time_keeps_counting() {
    let reply = ask(
        Place::Server,
        "freeze.clock",
        json!({ "observations": [{ "elapsed_secs": 20, "activity": "done" }], "reset_on_unfreeze": false }),
    );
    assert_eq!(reply["inactive_secs"], json!(20));
}

/// REQ: PORT:ilium-client/src/agent_freeze.rs::waiting_state_options_are_independent
#[test]
fn port_agent_freeze_approval_wait_freezes_only_after_the_threshold() {
    let settings = json!({
        "enabled": true,
        "after_secs": 10,
        "freeze_done": false,
        "freeze_waiting_for_input": false,
        "freeze_waiting_for_approval": true,
        "freeze_waiting_for_background": false
    });
    assert_eq!(
        should_freeze(settings.clone(), "waiting-approval", 9),
        json!(false)
    );
    assert_eq!(
        should_freeze(settings, "waiting-approval", 10),
        json!(true)
    );
}

/// REQ: PORT:ilium-client/src/agent_freeze.rs::working_activity_clears_elapsed_time
#[test]
fn port_agent_freeze_non_working_observations_accumulate() {
    let reply = ask(
        Place::Server,
        "freeze.clock",
        json!({
            "observations": [
                { "elapsed_secs": 5, "activity": "done" },
                { "elapsed_secs": 7, "activity": "idle" }
            ],
            "reset_on_unfreeze": false
        }),
    );
    assert_eq!(reply["inactive_secs"], json!(12));
}
