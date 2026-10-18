//! F-047 Sound alerts: one sound per event kind, the six sound switches and their defaults
//! (src/docs/notifications.md "Two channels, many events" and "Sounds").

use ilium_plugin_sdk::types::AgentActivity;
use serde_json::json;

use crate::common::*;
use crate::feature_only::{is_valid_dedup_key, notices, sounds, task_outcome};

/// REQ: F-047
#[test]
fn sounds_047_a_finished_turn_plays_the_finished_sound_by_default() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let submissions = finish_turn(&mut host, node, 1);
    assert_eq!(sounds(&submissions), ["finished"]);
}

/// REQ: F-047
#[test]
fn sounds_047_a_finished_turn_plays_exactly_one_sound() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    assert_eq!(sounds(&finish_turn(&mut host, node, 1)).len(), 1);
}

/// REQ: F-047
#[test]
fn sounds_047_the_approval_sound_is_off_by_default() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let submissions = transition(
        &mut host,
        node,
        AgentActivity::Working,
        AgentActivity::AwaitingApproval,
        1,
    );
    assert!(sounds(&submissions).is_empty());
}

/// REQ: F-047
#[test]
fn sounds_047_the_approval_sound_plays_ask_when_switched_on() {
    let mut host = server_host();
    set_setting(&mut host, "sound.events.approval_required", json!(true));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let submissions = transition(
        &mut host,
        node,
        AgentActivity::Working,
        AgentActivity::AwaitingApproval,
        4,
    );
    assert_eq!(sounds(&submissions), ["ask"]);
}

/// REQ: F-047
#[test]
fn sounds_047_the_approval_sound_carries_the_ask_node_revision_dedup_key() {
    let mut host = server_host();
    set_setting(&mut host, "sound.events.approval_required", json!(true));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let submissions = transition(
        &mut host,
        node,
        AgentActivity::Working,
        AgentActivity::AwaitingApproval,
        4,
    );
    let keys: Vec<Option<String>> = submissions
        .iter()
        .filter(|submission| {
            matches!(
                submission.intent,
                ilium_plugin_sdk::types::Intent::PlaySound(_)
            )
        })
        .map(|submission| submission.dedup_key.clone())
        .collect();
    assert_eq!(keys, [Some(format!("ask:{}:4", node.0))]);
}

/// REQ: F-047
#[test]
fn sounds_047_the_approval_sound_stops_when_switched_off_again() {
    let mut host = server_host();
    set_setting(&mut host, "sound.events.approval_required", json!(true));
    set_setting(&mut host, "sound.events.approval_required", json!(false));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let submissions = transition(
        &mut host,
        node,
        AgentActivity::Working,
        AgentActivity::AwaitingApproval,
        1,
    );
    assert!(sounds(&submissions).is_empty());
}

/// REQ: F-047
#[test]
fn sounds_047_agent_started_is_silent_by_default() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Idle));
    let submissions = transition(
        &mut host,
        node,
        AgentActivity::Idle,
        AgentActivity::Working,
        1,
    );
    assert!(sounds(&submissions).is_empty());
}

/// REQ: F-047
#[test]
fn sounds_047_agent_started_plays_when_an_idle_agent_begins_a_turn() {
    let mut host = server_host();
    set_setting(&mut host, "sound.events.agent_started", json!(true));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Idle));
    let submissions = transition(
        &mut host,
        node,
        AgentActivity::Idle,
        AgentActivity::Working,
        1,
    );
    assert_eq!(sounds(&submissions).len(), 1);
}

/// REQ: F-047
#[test]
fn sounds_047_agent_started_plays_when_a_finished_agent_begins_a_turn() {
    let mut host = server_host();
    set_setting(&mut host, "sound.events.agent_started", json!(true));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Done));
    let submissions = transition(
        &mut host,
        node,
        AgentActivity::Done,
        AgentActivity::Working,
        1,
    );
    assert_eq!(sounds(&submissions).len(), 1);
}

/// REQ: F-047
#[test]
fn sounds_047_agent_started_has_no_desktop_notice() {
    let mut host = server_host();
    set_setting(&mut host, "sound.events.agent_started", json!(true));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Idle));
    let submissions = transition(
        &mut host,
        node,
        AgentActivity::Idle,
        AgentActivity::Working,
        1,
    );
    assert!(notices(&submissions).is_empty());
}

/// REQ: F-047
#[test]
fn sounds_047_the_finished_sound_stops_when_its_switch_is_off() {
    let mut host = server_host();
    set_setting(&mut host, "sound.events.agent_finished", json!(false));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    assert!(sounds(&finish_turn(&mut host, node, 1)).is_empty());
}

/// REQ: F-047
#[test]
fn sounds_047_the_muted_source_plays_nothing() {
    let mut host = server_host();
    set_setting(&mut host, "sound.source", json!("muted"));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    assert!(sounds(&finish_turn(&mut host, node, 1)).is_empty());
}

/// REQ: F-047
#[test]
fn sounds_047_the_muted_source_still_raises_the_desktop_notice() {
    let mut host = server_host();
    set_setting(&mut host, "sound.source", json!("muted"));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    assert_eq!(notices(&finish_turn(&mut host, node, 1)).len(), 1);
}

/// REQ: F-047
#[test]
fn sounds_047_the_first_classification_of_a_pane_never_plays() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Done));
    let submissions = transition(&mut host, node, AgentActivity::None, AgentActivity::Done, 1);
    assert!(sounds(&submissions).is_empty());
}

/// REQ: F-047
#[test]
fn sounds_047_idle_to_done_does_not_play() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Idle));
    assert!(sounds(&transition(
        &mut host,
        node,
        AgentActivity::Idle,
        AgentActivity::Done,
        1
    ))
    .is_empty());
}

/// REQ: F-047
#[test]
fn sounds_047_done_to_idle_does_not_play() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Done));
    assert!(sounds(&transition(
        &mut host,
        node,
        AgentActivity::Done,
        AgentActivity::Idle,
        1
    ))
    .is_empty());
}

/// REQ: F-047
#[test]
fn sounds_047_idle_to_idle_does_not_play() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Idle));
    assert!(sounds(&transition(
        &mut host,
        node,
        AgentActivity::Idle,
        AgentActivity::Idle,
        1
    ))
    .is_empty());
}

/// REQ: F-047
#[test]
fn sounds_047_a_turn_blocked_on_approval_does_not_play_the_finished_sound() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let submissions = transition(
        &mut host,
        node,
        AgentActivity::Working,
        AgentActivity::AwaitingApproval,
        1,
    );
    assert!(!sounds(&submissions).contains(&"finished".to_string()));
}

/// REQ: F-047
#[test]
fn sounds_047_a_failed_task_plays_a_sound_by_default() {
    let mut host = server_host();
    let node = add_pane(&mut host, "build-shell", None);
    assert_eq!(
        sounds(&task_outcome(&mut host, node, "job-1", "error")).len(),
        1
    );
}

/// REQ: F-047
#[test]
fn sounds_047_a_lost_monitor_plays_the_task_failed_sound() {
    let mut host = server_host();
    let node = add_pane(&mut host, "build-shell", None);
    assert_eq!(
        sounds(&task_outcome(&mut host, node, "job-1", "lost")).len(),
        1
    );
}

/// REQ: F-047
#[test]
fn sounds_047_a_succeeded_task_is_silent_by_default() {
    let mut host = server_host();
    let node = add_pane(&mut host, "build-shell", None);
    assert!(sounds(&task_outcome(&mut host, node, "job-1", "done")).is_empty());
}

/// REQ: F-047
#[test]
fn sounds_047_a_succeeded_task_plays_when_its_sound_switch_is_on() {
    let mut host = server_host();
    set_setting(&mut host, "sound.events.task_succeeded", json!(true));
    let node = add_pane(&mut host, "build-shell", None);
    assert_eq!(
        sounds(&task_outcome(&mut host, node, "job-1", "done")).len(),
        1
    );
}

/// REQ: F-047
#[test]
fn sounds_047_a_failed_task_is_silent_when_its_sound_switch_is_off() {
    let mut host = server_host();
    set_setting(&mut host, "sound.events.task_failed", json!(false));
    let node = add_pane(&mut host, "build-shell", None);
    assert!(sounds(&task_outcome(&mut host, node, "job-1", "error")).is_empty());
}

/// REQ: F-047
#[test]
fn sounds_047_a_running_task_report_plays_nothing() {
    let mut host = server_host();
    let node = add_pane(&mut host, "build-shell", None);
    assert!(sounds(&task_outcome(&mut host, node, "job-1", "running")).is_empty());
}

/// REQ: F-047
#[test]
fn sounds_047_the_sound_switches_are_independent_of_the_desktop_switches() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.agent_finished", json!(false));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let submissions = finish_turn(&mut host, node, 1);
    assert_eq!(sounds(&submissions), ["finished"]);
    assert!(notices(&submissions).is_empty());
}

/// REQ: F-047
#[test]
fn sounds_047_every_sound_submission_has_a_valid_dedup_key() {
    let mut host = server_host();
    set_setting(&mut host, "sound.events.approval_required", json!(true));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let mut all = transition(
        &mut host,
        node,
        AgentActivity::Working,
        AgentActivity::AwaitingApproval,
        1,
    );
    all.extend(transition(
        &mut host,
        node,
        AgentActivity::AwaitingApproval,
        AgentActivity::Working,
        2,
    ));
    all.extend(finish_turn(&mut host, node, 3));
    let keys: Vec<String> = all
        .iter()
        .filter_map(|submission| submission.dedup_key.clone())
        .collect();
    assert!(!keys.is_empty());
    assert!(keys.iter().all(|key| is_valid_dedup_key(key)), "{keys:?}");
}
