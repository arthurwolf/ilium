//! F-048 Desktop notifications: the notice texts, the master switch and the four event switches
//! (src/docs/notifications.md "Desktop notifications"; ilium-server/src/notifications.rs).

use ilium_plugin_sdk::types::{AgentActivity, Urgency};
use serde_json::json;

use crate::common::*;

/// REQ: F-048
#[test]
fn desktop_048_a_finished_turn_raises_one_notice_titled_with_the_pane() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let found = notices(&finish_turn(&mut host, node, 1));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].title, "claude-api finished");
}

/// REQ: F-048
#[test]
fn desktop_048_the_finished_body_names_the_agent_and_says_it_waits_on_you() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let found = notices(&finish_turn(&mut host, node, 1));
    let body = found[0].body.clone().expect("body");
    assert!(body.starts_with("Session \""), "{body}");
    assert!(body.ends_with("the agent in \"claude-api\" is done and waiting on you."), "{body}");
}

/// REQ: F-048
#[test]
fn desktop_048_an_approval_prompt_raises_a_notice_by_default() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let found = notices(&transition(&mut host, node, AgentActivity::Working, AgentActivity::AwaitingApproval, 1));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].title, "claude-api needs approval");
}

/// REQ: F-048
#[test]
fn desktop_048_the_approval_body_says_the_agent_waits_for_approval() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let found = notices(&transition(&mut host, node, AgentActivity::Working, AgentActivity::AwaitingApproval, 1));
    let body = found[0].body.clone().expect("body");
    assert!(body.ends_with("the agent in \"claude-api\" is waiting for your approval."), "{body}");
}

/// REQ: F-048
#[test]
fn desktop_048_the_master_switch_off_silences_every_notice() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.enabled", json!(false));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    assert!(notices(&finish_turn(&mut host, node, 1)).is_empty());
    assert!(notices(&transition(&mut host, node, AgentActivity::Done, AgentActivity::AwaitingApproval, 2)).is_empty());
    let shell = add_pane(&mut host, "build-shell", None);
    assert!(notices(&task_outcome(&mut host, shell, "job-1", "error")).is_empty());
}

/// REQ: F-048
#[test]
fn desktop_048_the_master_switch_off_keeps_the_sounds() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.enabled", json!(false));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    assert_eq!(sounds(&finish_turn(&mut host, node, 1)), ["finished"]);
}

/// REQ: F-048
#[test]
fn desktop_048_the_master_switch_does_not_change_the_individual_switches() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.agent_finished", json!(false));
    set_setting(&mut host, "notifications.enabled", json!(false));
    set_setting(&mut host, "notifications.enabled", json!(true));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    assert!(notices(&finish_turn(&mut host, node, 1)).is_empty(), "agent_finished stayed off");
    let approval = notices(&transition(&mut host, node, AgentActivity::Done, AgentActivity::AwaitingApproval, 2));
    assert_eq!(approval.len(), 1, "approval_required stayed on");
}

/// REQ: F-048
#[test]
fn desktop_048_the_agent_finished_switch_only_silences_finished_notices() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.agent_finished", json!(false));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    assert!(notices(&finish_turn(&mut host, node, 1)).is_empty());
    let approval = transition(&mut host, node, AgentActivity::Done, AgentActivity::AwaitingApproval, 2);
    assert_eq!(notices(&approval).len(), 1);
}

/// REQ: F-048
#[test]
fn desktop_048_the_approval_switch_only_silences_approval_notices() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.approval_required", json!(false));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let approval = transition(&mut host, node, AgentActivity::Working, AgentActivity::AwaitingApproval, 1);
    assert!(notices(&approval).is_empty());
    assert_eq!(notices(&finish_turn(&mut host, node, 2)).len(), 1);
}

/// REQ: F-048
#[test]
fn desktop_048_the_first_classification_of_a_pane_never_notifies() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Done));
    assert!(notices(&transition(&mut host, node, AgentActivity::None, AgentActivity::Done, 1)).is_empty());
}

/// REQ: F-048
#[test]
fn desktop_048_idle_to_done_and_done_to_idle_do_not_notify() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Idle));
    assert!(notices(&transition(&mut host, node, AgentActivity::Idle, AgentActivity::Done, 1)).is_empty());
    assert!(notices(&transition(&mut host, node, AgentActivity::Done, AgentActivity::Idle, 2)).is_empty());
}

/// REQ: F-048
#[test]
fn desktop_048_a_turn_starting_does_not_notify() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Idle));
    assert!(notices(&transition(&mut host, node, AgentActivity::Idle, AgentActivity::Working, 1)).is_empty());
    assert!(notices(&transition(&mut host, node, AgentActivity::Done, AgentActivity::Working, 2)).is_empty());
}

/// REQ: F-048
#[test]
fn desktop_048_a_notice_carries_no_sound_because_the_sound_is_a_separate_submission() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let found = notices(&finish_turn(&mut host, node, 1));
    assert_eq!(found[0].sound, None);
}

/// REQ: F-048
#[test]
fn desktop_048_the_notice_and_the_sound_use_different_dedup_keys() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let submissions = finish_turn(&mut host, node, 1);
    let keys: Vec<String> = submissions.iter().filter_map(|submission| submission.dedup_key.clone()).collect();
    let mut unique = keys.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(keys.len(), unique.len(), "{keys:?}");
    assert!(keys.iter().all(|key| is_valid_dedup_key(key)), "{keys:?}");
}

/// REQ: F-048
#[test]
fn desktop_048_a_notice_coalesce_window_stays_inside_the_sixty_second_limit() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    for spec in notices(&finish_turn(&mut host, node, 1)) {
        assert!(spec.coalesce_window_ms <= 60_000);
    }
}

/// REQ: F-048
#[test]
fn desktop_048_the_approval_urgency_is_never_lower_than_the_finished_urgency() {
    let rank = |urgency: &Urgency| match urgency {
        Urgency::Low => 0,
        Urgency::Normal => 1,
        Urgency::High => 2,
    };
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let finished = notices(&finish_turn(&mut host, node, 1));
    let approval = notices(&transition(&mut host, node, AgentActivity::Done, AgentActivity::AwaitingApproval, 2));
    assert!(rank(&approval[0].urgency) >= rank(&finished[0].urgency));
}

/// REQ: F-048
#[test]
fn desktop_048_a_failed_task_notice_is_on_by_default() {
    let mut host = server_host();
    let node = add_pane(&mut host, "build-shell", None);
    assert_eq!(notices(&task_outcome(&mut host, node, "job-7", "error")).len(), 1);
}

/// REQ: F-048
#[test]
fn desktop_048_a_succeeded_task_notice_is_off_by_default() {
    let mut host = server_host();
    let node = add_pane(&mut host, "build-shell", None);
    assert!(notices(&task_outcome(&mut host, node, "job-7", "done")).is_empty());
}

/// REQ: F-048
#[test]
fn desktop_048_a_succeeded_task_notice_appears_when_switched_on() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.task_succeeded", json!(true));
    let node = add_pane(&mut host, "build-shell", None);
    assert_eq!(notices(&task_outcome(&mut host, node, "job-7", "done")).len(), 1);
}

/// REQ: F-048
#[test]
fn desktop_048_the_task_failed_switch_covers_failed_and_lost() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.task_failed", json!(false));
    set_setting(&mut host, "notifications.task_coalesce_seconds", json!(0));
    let node = add_pane(&mut host, "build-shell", None);
    assert!(notices(&task_outcome(&mut host, node, "job-7", "error")).is_empty());
    assert!(notices(&task_outcome(&mut host, node, "job-8", "lost")).is_empty());
}

/// REQ: F-048
#[test]
fn desktop_048_a_failed_task_with_no_agent_reads_pane_colon_what_happened() {
    let mut host = server_host();
    let node = add_pane(&mut host, "build-shell", None);
    let found = notices(&task_outcome(&mut host, node, "job-7", "error"));
    assert_eq!(found[0].title, "build-shell: background task failed");
}

/// REQ: F-048
#[test]
fn desktop_048_a_succeeded_task_with_no_agent_reads_pane_colon_what_happened() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.task_succeeded", json!(true));
    let node = add_pane(&mut host, "build-shell", None);
    let found = notices(&task_outcome(&mut host, node, "job-7", "done"));
    assert_eq!(found[0].title, "build-shell: background task finished");
}

/// REQ: F-048
#[test]
fn desktop_048_a_lost_task_with_no_agent_reads_pane_colon_what_happened() {
    let mut host = server_host();
    let node = add_pane(&mut host, "build-shell", None);
    let found = notices(&task_outcome(&mut host, node, "job-7", "lost"));
    assert_eq!(found[0].title, "build-shell: background task lost");
}

/// REQ: F-048
#[test]
fn desktop_048_a_failed_task_body_names_the_job_and_the_pane() {
    let mut host = server_host();
    let node = add_pane(&mut host, "build-shell", None);
    let body = notices(&task_outcome(&mut host, node, "job-7", "error"))[0].body.clone().expect("body");
    assert!(body.ends_with("background task job-7 in \"build-shell\" reported an error."), "{body}");
}

/// REQ: F-048
#[test]
fn desktop_048_a_succeeded_task_body_says_completed_successfully() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.task_succeeded", json!(true));
    let node = add_pane(&mut host, "build-shell", None);
    let body = notices(&task_outcome(&mut host, node, "job-7", "done"))[0].body.clone().expect("body");
    assert!(body.ends_with("background task job-7 in \"build-shell\" completed successfully."), "{body}");
}

/// REQ: F-048
#[test]
fn desktop_048_a_lost_task_body_says_the_outcome_is_unknown() {
    let mut host = server_host();
    let node = add_pane(&mut host, "build-shell", None);
    let body = notices(&task_outcome(&mut host, node, "job-7", "lost"))[0].body.clone().expect("body");
    assert!(
        body.ends_with(
            "background task job-7 in \"build-shell\" can no longer be observed by Ilium; its outcome is unknown."
        ),
        "{body}"
    );
}

/// REQ: F-048
#[test]
fn desktop_048_a_failed_task_of_a_working_agent_adds_the_agent_may_be_continuing_qualifier() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let found = notices(&task_outcome(&mut host, node, "job-7", "error"));
    assert_eq!(found[0].title, "claude-api: background task failed (agent may be continuing)");
}

/// REQ: F-048
#[test]
fn desktop_048_a_lost_task_of_a_working_agent_adds_the_agent_may_be_continuing_qualifier() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let found = notices(&task_outcome(&mut host, node, "job-7", "lost"));
    assert_eq!(found[0].title, "claude-api: background task lost (agent may be continuing)");
}

/// REQ: F-048
#[test]
fn desktop_048_a_succeeded_task_of_a_working_agent_adds_the_agent_still_working_qualifier() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.task_succeeded", json!(true));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let found = notices(&task_outcome(&mut host, node, "job-7", "done"));
    assert_eq!(found[0].title, "claude-api: background task finished (agent still working)");
}

/// REQ: F-048
#[test]
fn desktop_048_a_task_body_of_a_working_agent_says_its_own_alert_follows() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.task_succeeded", json!(true));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let body = notices(&task_outcome(&mut host, node, "job-7", "done"))[0].body.clone().expect("body");
    assert!(
        body.ends_with(
            "completed successfully. The agent is still working; its own finished alert follows when it is done."
        ),
        "{body}"
    );
}

/// REQ: F-048
#[test]
fn desktop_048_a_task_title_leads_with_the_pane_name() {
    let mut host = server_host();
    let node = add_pane(&mut host, "release-notes", None);
    let found = notices(&task_outcome(&mut host, node, "job-7", "error"));
    assert!(found[0].title.starts_with("release-notes: "));
}

/// REQ: F-048
#[test]
fn desktop_048_a_running_task_report_raises_no_notice() {
    let mut host = server_host();
    let node = add_pane(&mut host, "build-shell", None);
    assert!(notices(&task_outcome(&mut host, node, "job-7", "running")).is_empty());
}
