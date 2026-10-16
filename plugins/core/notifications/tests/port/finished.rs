//! Ports of the finished-transition tests of ilium-server/src/notifications.rs: which activity
//! transitions of an agent raise the "agent finished" alert (one desktop notice plus the `finished`
//! sound) and which never do. Observed on the submissions of the plugin, not on a private predicate.
//!
//! Mapping (API_GAPS.md): the plugin API has no waiting-background or background-task activity; the
//! original NowSignal projection counts both as busy (Working), so they are driven as `Working`.

use ilium_plugin_sdk::types::{AgentActivity, Intent, Submission};

use crate::common::*;

/// True when the transition raised the finished alert; checks both channels agree.
fn finished_alert(previous: AgentActivity, current: AgentActivity, goal_previous: Option<&str>, goal_current: Option<&str>) -> bool {
    let mut host = server_host();
    let node = add_pane_with_goal(&mut host, "claude-api", Some(previous), goal_previous);
    let submissions = transition_with_goal(&mut host, node, previous, current, 1, goal_current);
    assert_alert_channels_agree(&submissions)
}

fn assert_alert_channels_agree(submissions: &[Submission]) -> bool {
    let notified = notices(submissions).iter().any(|spec| spec.title.ends_with(" finished"));
    let played = sounds(submissions).iter().any(|id| id == "finished");
    assert_eq!(notified, played, "desktop notice and sound must agree: {submissions:?}");
    assert!(
        !submissions.iter().any(|submission| matches!(submission.intent, Intent::Notify(_)) && !notified),
        "a non-finished transition raised an unexpected notice: {submissions:?}"
    );
    notified
}

/// REQ: PORT:ilium-server/src/notifications.rs::working_to_idle_is_a_parked_agent_and_does_not_notify
#[test]
fn finished_port_working_to_idle_is_a_parked_agent_and_does_not_notify() {
    assert_eq!(finished_alert(AgentActivity::Working, AgentActivity::Idle, None, None), false);
}

/// REQ: PORT:ilium-server/src/notifications.rs::working_to_done_notifies
#[test]
fn finished_port_working_to_done_notifies() {
    assert_eq!(finished_alert(AgentActivity::Working, AgentActivity::Done, None, None), true);
}

/// REQ: PORT:ilium-server/src/notifications.rs::idle_to_working_does_not_notify
#[test]
fn finished_port_idle_to_working_does_not_notify() {
    assert_eq!(finished_alert(AgentActivity::Idle, AgentActivity::Working, None, None), false);
}

/// REQ: PORT:ilium-server/src/notifications.rs::done_to_working_does_not_notify
#[test]
fn finished_port_done_to_working_does_not_notify() {
    assert_eq!(finished_alert(AgentActivity::Done, AgentActivity::Working, None, None), false);
}

/// REQ: PORT:ilium-server/src/notifications.rs::working_to_waiting_approval_does_not_notify
#[test]
fn finished_port_working_to_waiting_approval_does_not_notify() {
    assert_eq!(finished_alert(AgentActivity::Working, AgentActivity::AwaitingApproval, None, None), false);
}

/// REQ: PORT:ilium-server/src/notifications.rs::waiting_approval_to_idle_does_not_notify
#[test]
fn finished_port_waiting_approval_to_idle_does_not_notify() {
    assert_eq!(finished_alert(AgentActivity::AwaitingApproval, AgentActivity::Idle, None, None), false);
}

/// REQ: PORT:ilium-server/src/notifications.rs::working_to_waiting_background_does_not_notify
#[test]
fn finished_port_working_to_waiting_background_does_not_notify() {
    assert_eq!(finished_alert(AgentActivity::Working, AgentActivity::Working, None, None), false);
}

/// REQ: PORT:ilium-server/src/notifications.rs::waiting_background_to_idle_does_not_notify
#[test]
fn finished_port_waiting_background_to_idle_does_not_notify() {
    assert_eq!(finished_alert(AgentActivity::Working, AgentActivity::Idle, None, None), false);
}

/// REQ: PORT:ilium-server/src/notifications.rs::working_to_background_task_still_running_does_not_notify
#[test]
fn finished_port_working_to_background_task_still_running_does_not_notify() {
    assert_eq!(finished_alert(AgentActivity::Working, AgentActivity::Working, None, None), false);
}

/// REQ: PORT:ilium-server/src/notifications.rs::background_task_still_running_to_done_notifies
#[test]
fn finished_port_background_task_still_running_to_done_notifies() {
    assert_eq!(finished_alert(AgentActivity::Working, AgentActivity::Done, None, None), true);
}

/// REQ: PORT:ilium-server/src/notifications.rs::waiting_background_to_done_notifies
#[test]
fn finished_port_waiting_background_to_done_notifies() {
    assert_eq!(finished_alert(AgentActivity::Working, AgentActivity::Done, None, None), true);
}

/// REQ: PORT:ilium-server/src/notifications.rs::waiting_background_to_waiting_background_does_not_notify
#[test]
fn finished_port_waiting_background_to_waiting_background_does_not_notify() {
    assert_eq!(finished_alert(AgentActivity::Working, AgentActivity::Working, None, None), false);
}

/// REQ: PORT:ilium-server/src/notifications.rs::idle_to_done_does_not_notify
#[test]
fn finished_port_idle_to_done_does_not_notify() {
    assert_eq!(finished_alert(AgentActivity::Idle, AgentActivity::Done, None, None), false);
}

/// REQ: PORT:ilium-server/src/notifications.rs::done_to_idle_does_not_notify
#[test]
fn finished_port_done_to_idle_does_not_notify() {
    assert_eq!(finished_alert(AgentActivity::Done, AgentActivity::Idle, None, None), false);
}

/// REQ: PORT:ilium-server/src/notifications.rs::working_to_working_does_not_notify
#[test]
fn finished_port_working_to_working_does_not_notify() {
    assert_eq!(finished_alert(AgentActivity::Working, AgentActivity::Working, None, None), false);
}

/// REQ: PORT:ilium-server/src/notifications.rs::idle_to_idle_does_not_notify
#[test]
fn finished_port_idle_to_idle_does_not_notify() {
    assert_eq!(finished_alert(AgentActivity::Idle, AgentActivity::Idle, None, None), false);
}

/// REQ: PORT:ilium-server/src/notifications.rs::first_ever_classification_never_notifies_even_if_it_looks_finished
#[test]
fn finished_port_first_ever_classification_never_notifies_even_if_it_looks_finished() {
    assert!(!finished_alert(AgentActivity::None, AgentActivity::Idle, None, None), "None -> Idle");
    assert!(!finished_alert(AgentActivity::None, AgentActivity::Done, None, None), "None -> Done");
    assert!(!finished_alert(AgentActivity::None, AgentActivity::Working, None, None), "None -> Working");
}

/// REQ: PORT:ilium-server/src/notifications.rs::working_to_done_notifies_regardless_of_which_agent_class
#[test]
fn finished_port_working_to_done_notifies_regardless_of_which_agent_class() {
    for signature in ["claude", "codex"] {
        let mut host = server_host();
        let node = add_pane_signature(&mut host, "agent-pane", signature, AgentActivity::Working);
        let submissions = transition(&mut host, node, AgentActivity::Working, AgentActivity::Done, 1);
        assert!(assert_alert_channels_agree(&submissions), "{signature}");
    }
}

/// REQ: PORT:ilium-server/src/notifications.rs::agent_with_goal_working_to_agent_with_goal_done_notifies
#[test]
fn finished_port_agent_with_goal_working_to_agent_with_goal_done_notifies() {
    assert_eq!(finished_alert(AgentActivity::Working, AgentActivity::Done, Some("ship it"), Some("ship it")), true);
}

/// REQ: PORT:ilium-server/src/notifications.rs::agent_with_goal_working_to_plain_agent_done_notifies_when_goal_clears_on_completion
#[test]
fn finished_port_agent_with_goal_working_to_plain_agent_done_notifies_when_goal_clears_on_completion() {
    assert_eq!(finished_alert(AgentActivity::Working, AgentActivity::Done, Some("ship it"), None), true);
}

/// REQ: PORT:ilium-server/src/notifications.rs::agent_with_goal_waiting_background_to_agent_with_goal_done_notifies
#[test]
fn finished_port_agent_with_goal_waiting_background_to_agent_with_goal_done_notifies() {
    assert_eq!(finished_alert(AgentActivity::Working, AgentActivity::Done, Some("ship it"), Some("ship it")), true);
}

/// REQ: PORT:ilium-server/src/notifications.rs::agent_with_goal_working_to_agent_with_goal_waiting_approval_does_not_notify
#[test]
fn finished_port_agent_with_goal_working_to_agent_with_goal_waiting_approval_does_not_notify() {
    assert_eq!(finished_alert(AgentActivity::Working, AgentActivity::AwaitingApproval, Some("ship it"), Some("ship it")), false);
}

/// REQ: PORT:ilium-server/src/notifications.rs::working_to_plain_shell_does_not_notify
#[test]
fn finished_port_working_to_plain_shell_does_not_notify() {
    let mut host = server_host();
    let node = add_pane(&mut host, "build-shell", Some(AgentActivity::Working));
    let submissions = transition(&mut host, node, AgentActivity::Working, AgentActivity::None, 1);
    assert!(!assert_alert_channels_agree(&submissions));
    assert!(notices(&submissions).is_empty() && sounds(&submissions).is_empty());
}

/// REQ: PORT:ilium-server/src/notifications.rs::plain_shell_to_working_does_not_notify
#[test]
fn finished_port_plain_shell_to_working_does_not_notify() {
    let mut host = server_host();
    let node = add_pane(&mut host, "build-shell", None);
    let submissions = transition(&mut host, node, AgentActivity::None, AgentActivity::Working, 1);
    assert!(!assert_alert_channels_agree(&submissions));
    assert!(notices(&submissions).is_empty() && sounds(&submissions).is_empty());
}
