//! F-027 and F-034: activity icons in the tree. On `agent-activity-changed` the plugin submits
//! `set-decoration` for the `activity` slot; the host steps the frames itself (page "Activity icons in the tree").

use ilium_plugin_sdk::types::{AgentActivity, AgentActivityEvent, AgentIdentifiedEvent, Event, NodeId};

use crate::common::{SPINNER_FRAMES, decorations, plain, server_host};

fn changed(node: u64, previous: AgentActivity, current: AgentActivity) -> Event {
    Event::AgentActivityChanged(AgentActivityEvent { node: NodeId(node), previous, current, revision: 2 })
}

/// REQ: F-027, F-034
#[test]
fn feature_034_a_working_agent_gets_the_animated_spinner_icon() {
    let mut host = server_host();
    let submissions = host.deliver_one(changed(7, AgentActivity::Idle, AgentActivity::Working));
    let specs = decorations(&submissions, "activity");
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].node, NodeId(7));
    assert_eq!(specs[0].icon.as_deref(), Some("⠋"));
}

/// REQ: F-027, F-034
#[test]
fn feature_034_the_spinner_has_the_ten_braille_frames_in_order() {
    let mut host = server_host();
    let submissions = host.deliver_one(changed(7, AgentActivity::Idle, AgentActivity::Working));
    let spec = &decorations(&submissions, "activity")[0];
    let frames: Vec<String> = spec.frames.iter().map(|frame| plain(frame)).collect();
    assert_eq!(frames, SPINNER_FRAMES);
}

/// REQ: F-027, F-034
#[test]
fn feature_034_the_spinner_steps_every_hundred_milliseconds() {
    let mut host = server_host();
    let submissions = host.deliver_one(changed(7, AgentActivity::Idle, AgentActivity::Working));
    let spec = &decorations(&submissions, "activity")[0];
    assert_eq!(spec.frame_ms, 100);
    assert!(spec.frame_ms >= 90, "the host refuses a frame time below 90 ms");
}

/// REQ: F-027, F-034
#[test]
fn feature_034_the_frame_count_is_inside_the_two_to_sixteen_limit() {
    let mut host = server_host();
    let submissions = host.deliver_one(changed(7, AgentActivity::Idle, AgentActivity::Working));
    let spec = &decorations(&submissions, "activity")[0];
    assert!((2..=16).contains(&spec.frames.len()));
}

/// REQ: F-027, F-034
#[test]
fn feature_034_an_agent_awaiting_approval_gets_a_still_question_mark() {
    let mut host = server_host();
    let submissions = host.deliver_one(changed(7, AgentActivity::Working, AgentActivity::AwaitingApproval));
    let spec = &decorations(&submissions, "activity")[0];
    assert_eq!(spec.icon.as_deref(), Some("?"));
    assert!(spec.frames.is_empty());
}

/// REQ: F-027, F-034
#[test]
fn feature_034_an_idle_agent_gets_a_still_dot() {
    let mut host = server_host();
    let submissions = host.deliver_one(changed(7, AgentActivity::Working, AgentActivity::Idle));
    let spec = &decorations(&submissions, "activity")[0];
    assert_eq!(spec.icon.as_deref(), Some("●"));
    assert!(spec.frames.is_empty());
}

/// REQ: F-027
#[test]
fn feature_027_a_done_agent_gets_an_activity_decoration() {
    let mut host = server_host();
    let submissions = host.deliver_one(changed(7, AgentActivity::Working, AgentActivity::Done));
    let specs = decorations(&submissions, "activity");
    assert_eq!(specs.len(), 1);
    assert!(specs[0].frames.is_empty(), "only a working agent animates");
}

/// REQ: F-027
#[test]
fn feature_027_the_activity_submission_is_keyed_per_node() {
    let mut host = server_host();
    let submissions = host.deliver_one(changed(7, AgentActivity::Idle, AgentActivity::Working));
    assert_eq!(submissions.len(), 1);
    assert_eq!(submissions[0].dedup_key.as_deref(), Some("activity-7"));
}

/// REQ: F-027
#[test]
fn feature_027_two_activity_changes_of_one_node_share_one_dedup_key() {
    let mut host = server_host();
    let submissions = host.deliver(vec![
        changed(7, AgentActivity::Idle, AgentActivity::Working),
        changed(7, AgentActivity::Working, AgentActivity::Idle),
    ]);
    assert_eq!(submissions.len(), 2);
    assert_eq!(submissions[0].dedup_key, submissions[1].dedup_key);
}

/// REQ: F-027
#[test]
fn feature_027_activity_changes_of_two_nodes_use_distinct_keys() {
    let mut host = server_host();
    let submissions = host.deliver(vec![
        changed(1, AgentActivity::Idle, AgentActivity::Working),
        changed(2, AgentActivity::Idle, AgentActivity::Working),
    ]);
    assert_ne!(submissions[0].dedup_key, submissions[1].dedup_key);
}

/// REQ: F-027
#[test]
fn feature_027_events_the_plugin_does_not_handle_submit_nothing() {
    let mut host = server_host();
    let submissions = host.deliver_one(Event::TreeChanged(vec![NodeId(1)]));
    assert!(submissions.is_empty());
}

/// REQ: F-027
#[test]
fn feature_027_an_empty_batch_submits_nothing() {
    let mut host = server_host();
    assert!(host.deliver(Vec::new()).is_empty());
}

/// REQ: F-027
#[test]
fn feature_027_a_batch_submits_in_event_order() {
    let mut host = server_host();
    let submissions = host.deliver(vec![
        changed(3, AgentActivity::Idle, AgentActivity::Working),
        Event::TreeChanged(vec![NodeId(9)]),
        changed(4, AgentActivity::Idle, AgentActivity::AwaitingApproval),
    ]);
    let nodes: Vec<NodeId> = decorations(&submissions, "activity").iter().map(|spec| spec.node).collect();
    assert_eq!(nodes, [NodeId(3), NodeId(4)]);
}

/// REQ: F-026
#[test]
fn feature_026_agent_identified_logs_the_node_and_signature_at_info_level() {
    let mut host = server_host();
    let submissions = host.deliver_one(Event::AgentIdentified(AgentIdentifiedEvent {
        node: NodeId(7),
        signature: "example-agent".to_string(),
    }));
    assert!(submissions.is_empty());
    let logs = host.logs();
    assert!(logs.iter().any(|line| line.level == "info" && line.message == "node 7 runs example-agent"), "{logs:?}");
}

/// REQ: F-026
#[test]
fn feature_026_each_identified_agent_logs_one_line() {
    let mut host = server_host();
    host.deliver(vec![
        Event::AgentIdentified(AgentIdentifiedEvent { node: NodeId(1), signature: "claude".to_string() }),
        Event::AgentIdentified(AgentIdentifiedEvent { node: NodeId(2), signature: "codex".to_string() }),
    ]);
    let lines: Vec<String> = host.logs().into_iter().map(|line| line.message).collect();
    assert!(lines.contains(&"node 1 runs claude".to_string()));
    assert!(lines.contains(&"node 2 runs codex".to_string()));
}

/// REQ: F-027
#[test]
fn feature_027_the_activity_slot_is_an_icon_slot_with_priority_ninety() {
    let text = std::fs::read_to_string(crate::common::manifest_path()).expect("manifest");
    let value: serde_json::Value = serde_json::from_str(&text).expect("json");
    let slot = &value["contributes"]["decoration_slots"][0];
    assert_eq!(slot["id"], "activity");
    assert_eq!(slot["placement"], "icon");
    assert_eq!(slot["priority"], 90);
}
