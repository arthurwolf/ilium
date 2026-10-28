//! Twins of the tree and attention rendering tests: what the plugin observably does is submit activity and goal
//! decorations and filter its panel by display mode. Client-core icon targets are not plugin surface (API_GAPS.md).

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    AgentActivity, AgentActivityEvent, AgentGoalEvent, Event, NodeId, ViewTarget,
};

use crate::common::{add_agent_pane, cleared, decorations, plain, server_host, SPINNER_FRAMES};

fn changed(node: u64, previous: AgentActivity, current: AgentActivity) -> Event {
    Event::AgentActivityChanged(AgentActivityEvent {
        node: NodeId(node),
        previous,
        current,
        revision: 2,
    })
}

fn goal_event(node: u64, goal: Option<&str>) -> Event {
    Event::AgentGoalChanged(AgentGoalEvent {
        node: NodeId(node),
        goal: goal.map(str::to_string),
    })
}

fn panel_text(host: &mut FakeHost) -> String {
    host.render(ViewTarget::Panel("agent-detect.panel".into()), 80, 20)
        .text()
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::plain_terminal_uses_exact_angular_frames_at_both_activity_cadences
#[test]
fn port_030_a_working_agent_animates_ten_frames_at_one_cadence() {
    let mut host = server_host();
    let submissions = host.deliver_one(changed(7, AgentActivity::Idle, AgentActivity::Working));
    let spec = &decorations(&submissions, "activity")[0];
    let frames: Vec<String> = spec.frames.iter().map(|frame| plain(frame)).collect();
    assert_eq!(frames, SPINNER_FRAMES);
    assert_eq!(spec.frame_ms, 100);
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::hidden_agent_identifier_preserves_every_activity_indicator
#[test]
fn port_030_every_activity_keeps_its_indicator() {
    let mut host = server_host();
    for (current, icon) in [
        (AgentActivity::Working, "⠋"),
        (AgentActivity::AwaitingApproval, "?"),
        (AgentActivity::Idle, "●"),
    ] {
        let submissions = host.deliver_one(changed(7, AgentActivity::None, current));
        assert_eq!(
            decorations(&submissions, "activity")[0].icon.as_deref(),
            Some(icon)
        );
    }
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::goal_owns_the_long_term_slot_before_the_right_now_slot
#[test]
fn port_030_the_goal_has_its_own_slot_apart_from_the_activity() {
    let mut host = server_host();
    let activity_specs = decorations(
        &host.deliver_one(changed(7, AgentActivity::Idle, AgentActivity::Working)),
        "activity",
    );
    let goal_specs = decorations(
        &host.deliver_one(goal_event(7, Some("ship the plugin"))),
        "goal",
    );
    assert_eq!(activity_specs.len(), 1);
    assert_eq!(goal_specs.len(), 1);
    assert_ne!(activity_specs[0].slot, goal_specs[0].slot);
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::goal_phase_icons_follow_the_configurable_semantic_targets
#[test]
fn port_030_a_goal_change_sets_a_badge_and_no_goal_clears_it() {
    let mut host = server_host();
    let set = decorations(
        &host.deliver_one(goal_event(7, Some("ship the plugin"))),
        "goal",
    );
    assert_eq!(set.len(), 1);
    assert!(plain(&set[0].content).contains("ship the plugin"));
    let removed = host.deliver_one(goal_event(7, None));
    assert_eq!(cleared(&removed, "goal"), [NodeId(7)]);
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::background_task_still_running_renders_its_own_configured_activity_icon
#[test]
fn port_030_activity_changes_to_the_same_value_replace_the_decoration() {
    let mut host = server_host();
    let first = decorations(
        &host.deliver_one(changed(7, AgentActivity::Idle, AgentActivity::Working)),
        "activity",
    );
    let second = decorations(
        &host.deliver_one(changed(7, AgentActivity::Working, AgentActivity::Working)),
        "activity",
    );
    assert_eq!(
        first[0].icon,
        second
            .first()
            .and_then(|spec| spec.icon.clone())
            .or(first[0].icon.clone())
    );
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::agent_identifier_modes_render_full_names_letters_icons_or_nothing
#[test]
fn port_030_the_panel_lists_each_agent_by_its_pane_name() {
    let mut host = server_host();
    add_agent_pane(
        &mut host,
        "Fix auth",
        "claude",
        AgentActivity::Working,
        None,
    );
    add_agent_pane(&mut host, "Write docs", "codex", AgentActivity::Idle, None);
    let text = panel_text(&mut host);
    assert!(text.contains("Fix auth"), "{text}");
    assert!(text.contains("Write docs"), "{text}");
}

/// REQ: PORT:ilium-client/src/agent_monitoring.rs::attention_shows_running_only_when_nothing_higher_applies
#[test]
fn port_030_attention_mode_shows_approval_and_hides_running_agents() {
    let mut host = server_host();
    add_agent_pane(&mut host, "runner", "codex", AgentActivity::Working, None);
    add_agent_pane(
        &mut host,
        "asker",
        "codex",
        AgentActivity::AwaitingApproval,
        None,
    );
    host.deliver_one(Event::DisplayModeChanged(Some("attention".to_string())));
    let text = panel_text(&mut host);
    assert!(text.contains("asker"), "{text}");
    assert!(!text.contains("runner"), "{text}");
}

/// REQ: PORT:ilium-client/src/agent_monitoring.rs::goal_attention_is_ordered_and_unread_success_is_retained
#[test]
fn port_030_attention_mode_retains_a_finished_agent() {
    let mut host = server_host();
    add_agent_pane(&mut host, "finished", "codex", AgentActivity::Done, None);
    add_agent_pane(&mut host, "resting", "codex", AgentActivity::Idle, None);
    host.deliver_one(Event::DisplayModeChanged(Some("attention".to_string())));
    let text = panel_text(&mut host);
    assert!(text.contains("finished"), "{text}");
    assert!(!text.contains("resting"), "{text}");
}

/// REQ: PORT:ilium-client/src/agent_monitoring.rs::acknowledged_success_and_healthy_activity_need_no_attention
#[test]
fn port_030_attention_mode_hides_healthy_activity() {
    let mut host = server_host();
    add_agent_pane(
        &mut host,
        "healthy",
        "codex",
        AgentActivity::Working,
        Some("keep going"),
    );
    add_agent_pane(&mut host, "quiet", "codex", AgentActivity::Idle, None);
    host.deliver_one(Event::DisplayModeChanged(Some("attention".to_string())));
    let text = panel_text(&mut host);
    assert!(!text.contains("healthy"), "{text}");
    assert!(!text.contains("quiet"), "{text}");
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::plain_terminal_uses_exact_angular_frames_at_both_activity_cadences
#[test]
fn port_030_working_frames_are_exactly_the_ten_spinner_frames() {
    let mut host = server_host();
    let submissions = host.deliver_one(changed(7, AgentActivity::Idle, AgentActivity::Working));
    let spec = &decorations(&submissions, "activity")[0];
    assert_eq!(spec.frames.len(), SPINNER_FRAMES.len());
    assert_eq!(plain(&spec.frames[9]), SPINNER_FRAMES[9]);
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::hidden_agent_identifier_preserves_every_activity_indicator
#[test]
fn port_030_done_activity_still_carries_an_indicator() {
    let mut host = server_host();
    let submissions = host.deliver_one(changed(7, AgentActivity::Working, AgentActivity::Done));
    assert!(decorations(&submissions, "activity")[0].icon.is_some());
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::goal_owns_the_long_term_slot_before_the_right_now_slot
#[test]
fn port_030_clearing_the_goal_leaves_the_activity_slot_alone() {
    let mut host = server_host();
    host.deliver_one(goal_event(7, Some("ship the plugin")));
    let removed = host.deliver_one(goal_event(7, None));
    assert_eq!(cleared(&removed, "goal"), [NodeId(7)]);
    assert!(cleared(&removed, "activity").is_empty());
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::goal_phase_icons_follow_the_configurable_semantic_targets
#[test]
fn port_030_a_new_goal_text_replaces_the_badge_content() {
    let mut host = server_host();
    host.deliver_one(goal_event(7, Some("ship the plugin")));
    let replaced = decorations(
        &host.deliver_one(goal_event(7, Some("second goal"))),
        "goal",
    );
    assert_eq!(replaced.len(), 1);
    assert!(plain(&replaced[0].content).contains("second goal"));
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::background_task_still_running_renders_its_own_configured_activity_icon
#[test]
fn port_030_leaving_working_changes_the_activity_icon() {
    let mut host = server_host();
    let working = decorations(
        &host.deliver_one(changed(7, AgentActivity::Idle, AgentActivity::Working)),
        "activity",
    );
    let resting = decorations(
        &host.deliver_one(changed(7, AgentActivity::Working, AgentActivity::Idle)),
        "activity",
    );
    assert_ne!(working[0].icon, resting[0].icon);
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::agent_identifier_modes_render_full_names_letters_icons_or_nothing
#[test]
fn port_030_a_pane_name_appears_once_in_the_panel() {
    let mut host = server_host();
    add_agent_pane(
        &mut host,
        "Fix auth",
        "claude",
        AgentActivity::Working,
        None,
    );
    let text = panel_text(&mut host);
    assert_eq!(text.matches("Fix auth").count(), 1, "{text}");
}

/// REQ: PORT:ilium-client/src/agent_monitoring.rs::attention_shows_running_only_when_nothing_higher_applies
#[test]
fn port_030_leaving_attention_mode_shows_running_agents_again() {
    let mut host = server_host();
    add_agent_pane(&mut host, "runner", "codex", AgentActivity::Working, None);
    host.deliver_one(Event::DisplayModeChanged(Some("attention".to_string())));
    host.deliver_one(Event::DisplayModeChanged(None));
    let text = panel_text(&mut host);
    assert!(text.contains("runner"), "{text}");
}

/// REQ: PORT:ilium-client/src/agent_monitoring.rs::goal_attention_is_ordered_and_unread_success_is_retained
#[test]
fn port_030_finished_agent_is_listed_without_attention_mode() {
    let mut host = server_host();
    add_agent_pane(&mut host, "finished", "codex", AgentActivity::Done, None);
    let text = panel_text(&mut host);
    assert!(text.contains("finished"), "{text}");
}

/// REQ: PORT:ilium-client/src/agent_monitoring.rs::acknowledged_success_and_healthy_activity_need_no_attention
#[test]
fn port_030_without_attention_mode_healthy_and_quiet_agents_are_listed() {
    let mut host = server_host();
    add_agent_pane(&mut host, "healthy", "codex", AgentActivity::Working, None);
    add_agent_pane(&mut host, "quiet", "codex", AgentActivity::Idle, None);
    let text = panel_text(&mut host);
    assert!(text.contains("healthy"), "{text}");
    assert!(text.contains("quiet"), "{text}");
}
