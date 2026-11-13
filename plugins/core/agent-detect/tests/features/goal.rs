//! F-029 goal state tracking: the classification carries the goal, `agent-goal-changed` sets or clears the
//! `goal` badge slot (page "Status signals and goals").

use ilium_plugin_sdk::types::{AgentGoalEvent, Event, Intent, NodeId};

use crate::common::{cleared, decorations, goal_of, plain, server_host};

/// REQ: F-029
#[test]
fn feature_029_claude_active_goal_is_reported() {
    assert!(goal_of("claude", "claude_goal_active_working.txt").is_some());
}

/// REQ: F-029
#[test]
fn feature_029_claude_goal_on_a_shared_indicator_row_is_reported() {
    assert!(goal_of("claude", "claude_goal_active_shared_indicator_row.txt").is_some());
}

/// REQ: F-029
#[test]
fn feature_029_claude_goal_stays_after_an_interrupt() {
    assert!(goal_of("claude", "claude_goal_interrupted_still_active.txt").is_some());
}

/// REQ: F-029
#[test]
fn feature_029_claude_paused_goal_is_reported() {
    assert!(goal_of("claude", "claude_goal_paused_checks_capped.txt").is_some());
}

/// REQ: F-029
#[test]
fn feature_029_claude_cleared_goal_reports_no_goal() {
    assert_eq!(goal_of("claude", "claude_goal_cleared.txt"), None);
}

/// REQ: F-029
#[test]
fn feature_029_claude_screen_without_a_goal_reports_no_goal() {
    assert_eq!(goal_of("claude", "claude_no_goal_idle.txt"), None);
}

/// REQ: F-029
#[test]
fn feature_029_claude_goal_wording_outside_the_indicator_row_is_ignored() {
    let screen = "I discussed the /goal command in prose.\nGoal active: not an indicator\n";
    assert_eq!(crate::common::classify_text("claude", screen).goal, None);
}

/// REQ: F-029
#[test]
fn feature_029_codex_active_goal_footers_are_reported() {
    for name in [
        "codex_goal_pursuing_warning_footer.txt",
        "codex_goal_active_wide_footer.txt",
        "codex_goal_two_line_active.txt",
        "codex_goal_two_line_resumed.txt",
    ] {
        assert!(goal_of("codex", name).is_some(), "{name}");
    }
}

/// REQ: F-029
#[test]
fn feature_029_codex_paused_goal_footers_are_reported() {
    for name in [
        "codex_goal_paused_while_turn_finishes.txt",
        "codex_goal_paused_after_interrupt.txt",
        "codex_goal_two_line_paused.txt",
    ] {
        assert!(goal_of("codex", name).is_some(), "{name}");
    }
}

/// REQ: F-029
#[test]
fn feature_029_codex_stalled_goal_is_reported_as_a_blocked_goal() {
    assert!(goal_of("codex", "codex_goal_stalled_narrow.txt").is_some());
}

/// REQ: F-029
#[test]
fn feature_029_codex_cleared_goal_reports_no_goal() {
    assert_eq!(goal_of("codex", "codex_goal_cleared.txt"), None);
}

/// REQ: F-029
#[test]
fn feature_029_codex_footer_without_goal_metadata_reports_no_goal() {
    assert_eq!(goal_of("codex", "codex_no_goal_metadata_footer.txt"), None);
}

/// REQ: F-029
#[test]
fn feature_029_codex_goal_wording_outside_the_footer_is_ignored() {
    let screen = "Pursuing goal (5m) said the assistant in prose\n\u{203a} ";
    assert_eq!(crate::common::classify_text("codex", screen).goal, None);
}

/// REQ: F-029
#[test]
fn feature_029_a_goal_change_sets_the_goal_badge_with_the_goal_text() {
    let mut host = server_host();
    let submissions = host.deliver_one(Event::AgentGoalChanged(AgentGoalEvent {
        node: NodeId(7),
        goal: Some("ship the parser port".to_string()),
    }));
    let specs = decorations(&submissions, "goal");
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].node, NodeId(7));
    assert_eq!(plain(&specs[0].content), "goal ship the parser port");
}

/// REQ: F-029
#[test]
fn feature_029_the_goal_badge_submission_is_keyed_per_node() {
    let mut host = server_host();
    let submissions = host.deliver_one(Event::AgentGoalChanged(AgentGoalEvent {
        node: NodeId(7),
        goal: Some("ship".to_string()),
    }));
    assert_eq!(submissions.len(), 1);
    assert_eq!(submissions[0].dedup_key.as_deref(), Some("goal-7"));
}

/// REQ: F-029
#[test]
fn feature_029_an_ended_goal_clears_the_goal_slot() {
    let mut host = server_host();
    let submissions = host.deliver_one(Event::AgentGoalChanged(AgentGoalEvent {
        node: NodeId(7),
        goal: None,
    }));
    assert_eq!(cleared(&submissions, "goal"), [NodeId(7)]);
    assert!(decorations(&submissions, "goal").is_empty());
}

/// REQ: F-029
#[test]
fn feature_029_the_clear_submission_is_keyed_per_node() {
    let mut host = server_host();
    let submissions = host.deliver_one(Event::AgentGoalChanged(AgentGoalEvent {
        node: NodeId(7),
        goal: None,
    }));
    assert_eq!(submissions[0].dedup_key.as_deref(), Some("goal-clear-7"));
}

/// REQ: F-029
#[test]
fn feature_029_clear_decoration_targets_the_goal_slot_of_the_node() {
    let mut host = server_host();
    let submissions = host.deliver_one(Event::AgentGoalChanged(AgentGoalEvent {
        node: NodeId(3),
        goal: None,
    }));
    match &submissions[0].intent {
        Intent::ClearDecoration(key) => {
            assert_eq!(key.node, NodeId(3));
            assert_eq!(key.slot, "goal");
        }
        other => panic!("expected clear-decoration, got {other:?}"),
    }
}

/// REQ: F-029
#[test]
fn feature_029_a_goal_then_its_end_submits_set_then_clear_in_order() {
    let mut host = server_host();
    let batch = vec![
        Event::AgentGoalChanged(AgentGoalEvent {
            node: NodeId(5),
            goal: Some("a".to_string()),
        }),
        Event::AgentGoalChanged(AgentGoalEvent {
            node: NodeId(5),
            goal: None,
        }),
    ];
    let submissions = host.deliver(batch);
    assert_eq!(submissions.len(), 2);
    assert!(matches!(submissions[0].intent, Intent::SetDecoration(_)));
    assert!(matches!(submissions[1].intent, Intent::ClearDecoration(_)));
}

/// REQ: F-029
#[test]
fn feature_029_goals_of_two_nodes_use_distinct_keys() {
    let mut host = server_host();
    let submissions = host.deliver(vec![
        Event::AgentGoalChanged(AgentGoalEvent {
            node: NodeId(1),
            goal: Some("one".to_string()),
        }),
        Event::AgentGoalChanged(AgentGoalEvent {
            node: NodeId(2),
            goal: Some("two".to_string()),
        }),
    ]);
    let keys: Vec<_> = submissions
        .iter()
        .map(|submission| submission.dedup_key.clone())
        .collect();
    assert_eq!(
        keys,
        [Some("goal-1".to_string()), Some("goal-2".to_string())]
    );
}

/// REQ: F-029
#[test]
fn feature_029_the_goal_slot_is_a_badge_slot_with_priority_eighty() {
    // The manifest declares the slot; the decoration lands in the same slot id.
    let text = std::fs::read_to_string(crate::common::manifest_path()).expect("manifest");
    let value: serde_json::Value = serde_json::from_str(&text).expect("json");
    let slot = &value["contributes"]["decoration_slots"][1];
    assert_eq!(slot["id"], "goal");
    assert_eq!(slot["placement"], "badge");
    assert_eq!(slot["priority"], 80);
}
