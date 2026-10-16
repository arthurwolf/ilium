//! Ports of the notice-text tests of ilium-server/src/notifications.rs: the title and body of the
//! finished, approval and task-outcome notices, as carried by the plugin's `notify` submissions.
//!
//! The session name and the long agent description reach the original through its arguments; the
//! plugin API gives neither (API_GAPS.md), so the session part is asserted as a prefix and the long
//! description is taken from the agent's `objective`.

use ilium_plugin_sdk::types::AgentActivity;
use serde_json::json;

use crate::common::*;

fn finished_body(name: &str, objective: Option<&str>) -> (String, String) {
    let mut host = server_host();
    let node = add_pane_full(&mut host, name, "claude", Some(AgentActivity::Working), objective, None);
    let found = notices(&finish_turn(&mut host, node, 1));
    assert_eq!(found.len(), 1);
    (found[0].title.clone(), found[0].body.clone().expect("body"))
}

/// REQ: PORT:ilium-server/src/notifications.rs::notification_appends_distinct_long_agent_description_after_current_text
#[test]
fn texts_port_a_distinct_long_description_follows_the_current_text() {
    let (title, body) = finished_body("Auth Bug", Some("Fix Authentication Bug In Login Flow"));
    assert_eq!(title, "Auth Bug finished");
    assert!(body.starts_with("Session \""), "{body}");
    assert!(
        body.ends_with(
            "the agent in \"Auth Bug\" is done and waiting on you.\n\nAbout: Fix Authentication Bug In Login Flow"
        ),
        "{body}"
    );
}

/// REQ: PORT:ilium-server/src/notifications.rs::notification_without_distinct_long_description_preserves_current_text
#[test]
fn texts_port_no_long_description_keeps_the_current_text() {
    let (title, body) = finished_body("Manually Named Pane", None);
    assert_eq!(title, "Manually Named Pane finished");
    assert!(body.ends_with("the agent in \"Manually Named Pane\" is done and waiting on you."), "{body}");
    assert!(!body.contains("About:"), "{body}");
}

/// REQ: PORT:ilium-server/src/notifications.rs::equal_short_and_long_names_do_not_duplicate_the_description
#[test]
fn texts_port_equal_short_and_long_names_do_not_duplicate_the_description() {
    let (_, body) = finished_body("Agent Work", Some("Agent Work"));
    assert!(body.ends_with("the agent in \"Agent Work\" is done and waiting on you."), "{body}");
    assert!(!body.contains("About:"), "{body}");
}

/// REQ: PORT:ilium-server/src/notifications.rs::approval_notification_names_the_pane_and_the_prompt
#[test]
fn texts_port_the_approval_notice_names_the_pane_and_the_prompt() {
    let mut host = server_host();
    let node = add_pane(&mut host, "Auth Bug", Some(AgentActivity::Working));
    let found = notices(&transition(&mut host, node, AgentActivity::Working, AgentActivity::AwaitingApproval, 1));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].title, "Auth Bug needs approval");
    let body = found[0].body.clone().expect("body");
    assert!(body.starts_with("Session \""), "{body}");
    assert!(body.ends_with("the agent in \"Auth Bug\" is waiting for your approval."), "{body}");
}

fn task_notice(name: &str, activity: Option<AgentActivity>, status: &str) -> (String, String) {
    let mut host = server_host();
    set_setting(&mut host, "notifications.task_succeeded", json!(true));
    let node = add_pane(&mut host, name, activity);
    let found = notices(&task_outcome(&mut host, node, "build", status));
    assert_eq!(found.len(), 1, "one task notice for {status} on {name}");
    (found[0].title.clone(), found[0].body.clone().expect("body"))
}

/// REQ: PORT:ilium-server/src/notifications.rs::task_outcome_text_leads_with_the_pane_and_names_the_agent_state
#[test]
fn texts_port_a_task_notice_on_a_working_agent_says_the_agent_is_still_working() {
    let (title, body) = task_notice("Auth Bug", Some(AgentActivity::Working), "done");
    assert_eq!(title, "Auth Bug: background task finished (agent still working)");
    assert!(body.contains("The agent is still working"), "{body}");
}

/// REQ: PORT:ilium-server/src/notifications.rs::task_outcome_text_leads_with_the_pane_and_names_the_agent_state
#[test]
fn texts_port_a_failed_task_notice_on_a_working_agent_says_the_agent_may_be_continuing() {
    let (title, _) = task_notice("Auth Bug", Some(AgentActivity::Working), "error");
    assert_eq!(title, "Auth Bug: background task failed (agent may be continuing)");
}

/// REQ: PORT:ilium-server/src/notifications.rs::task_outcome_text_leads_with_the_pane_and_names_the_agent_state
#[test]
fn texts_port_a_task_notice_on_a_plain_shell_names_no_agent_state() {
    let (title, body) = task_notice("Shell", None, "done");
    assert_eq!(title, "Shell: background task finished");
    assert!(!body.contains("still working"), "{body}");
}
