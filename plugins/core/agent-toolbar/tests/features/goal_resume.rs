//! F-035 Resume a paused goal: a Codex pane whose goal is paused gets a footer button that types `/goal resume`
//! through the server side, replacing the clickable footer text the kernel used to paint.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    AgentActivity, AgentGoalEvent, AgentIdentifiedEvent, CommandOrigin, Event, Intent, NodeId,
    PluginSide, SpanTarget,
};

use crate::common::*;

fn identified(pane: u64, signature: &str) -> Event {
    Event::AgentIdentified(AgentIdentifiedEvent {
        node: NodeId(pane),
        signature: signature.to_string(),
    })
}

fn goal(pane: u64, state: Option<&str>) -> Event {
    Event::AgentGoalChanged(AgentGoalEvent {
        node: NodeId(pane),
        goal: state.map(str::to_string),
    })
}

fn codex_host() -> (FakeHost, u64) {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "codex", "codex", AgentActivity::Idle);
    host.deliver_one(identified(pane, "codex"));
    (host, pane)
}

fn click(pane: u64) -> Event {
    let mut event = command_on("resume-goal", CommandOrigin::FrameSlot, None);
    if let Event::CommandInvoked(invoked) = &mut event {
        invoked.arguments_json = format!("{{\"node\":{pane}}}");
    }
    event
}

/// REQ: F-035
#[test]
fn f_035_a_paused_codex_goal_sets_a_clickable_resume_row() {
    let (mut host, pane) = codex_host();
    let submissions = host.deliver_one(goal(pane, Some("paused")));
    assert_eq!(
        set_slots(&submissions),
        vec![(
            pane,
            "goal".to_string(),
            " \u{25b6} Resume goal ".to_string()
        )]
    );
    let target = submissions
        .iter()
        .find_map(|submission| match &submission.intent {
            Intent::SetFrameSlot(spec) => spec.content.first().map(|span| span.target.clone()),
            _ => None,
        });
    assert_eq!(target, Some(SpanTarget::Command("resume-goal".to_string())));
}

/// REQ: F-035
#[test]
fn f_035_an_active_goal_or_another_agent_gets_no_resume_row() {
    let (mut host, pane) = codex_host();
    assert!(host.deliver_one(goal(pane, Some("active"))).is_empty());
    let other = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Idle);
    host.deliver_one(identified(other, "claude"));
    assert!(host.deliver_one(goal(other, Some("paused"))).is_empty());
}

/// REQ: F-035
#[test]
fn f_035_the_row_is_set_once_and_leaves_with_the_paused_state() {
    let (mut host, pane) = codex_host();
    host.deliver_one(goal(pane, Some("paused")));
    assert!(host.deliver_one(goal(pane, Some("paused"))).is_empty());
    let resumed = host.deliver_one(goal(pane, Some("active")));
    assert_eq!(cleared_slots(&resumed), vec![(pane, "goal".to_string())]);
    assert!(host.deliver_one(goal(pane, None)).is_empty());
}

/// REQ: F-035
#[test]
fn f_035_the_row_leaves_when_the_agent_leaves_the_pane() {
    let (mut host, pane) = codex_host();
    host.deliver_one(goal(pane, Some("paused")));
    let gone = host.deliver_one(activity(pane, AgentActivity::Idle, AgentActivity::None));
    assert!(cleared_slots(&gone).contains(&(pane, "goal".to_string())));
}

/// REQ: F-035
#[test]
fn f_035_a_click_asks_the_server_side_to_resume_the_clicked_pane() {
    let (mut host, pane) = codex_host();
    host.deliver_one(goal(pane, Some("paused")));
    let sent = messages(&host.deliver_one(click(pane)));
    assert_eq!(
        sent,
        vec![(
            PLUGIN_ID.to_string(),
            PluginSide::Server,
            "goal-resume".to_string(),
            pane.to_string().into_bytes()
        )]
    );
}

/// REQ: F-035
#[test]
fn f_035_a_stale_click_after_the_goal_left_the_paused_state_resumes_nothing() {
    let (mut host, pane) = codex_host();
    host.deliver_one(goal(pane, Some("paused")));
    host.deliver_one(goal(pane, Some("active")));
    assert!(host.deliver_one(click(pane)).is_empty());
}

/// REQ: F-035
#[test]
fn f_035_the_server_side_types_the_resume_command_through_an_agent_action() {
    let mut host = server_host();
    let submissions = host.deliver_one(message_from(PluginSide::Client, "goal-resume", b"7"));
    assert_eq!(
        agent_actions(&submissions),
        vec![(7, "goal-resume".to_string())]
    );
    assert!(host
        .deliver_one(message_from(PluginSide::Client, "goal-resume", b"x"))
        .is_empty());
}
