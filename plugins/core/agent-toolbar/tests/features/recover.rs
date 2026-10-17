//! F-033 Clear the banner and recover work: `agent-activity-changed` to none or done.

use ilium_plugin_sdk::types::AgentActivity::{self, AwaitingApproval, Done, Idle, Menu, Working};

use crate::common::*;

fn host_with_prompt(prompt: &str) -> (ilium_plugin_sdk::testing::FakeHost, u64) {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Working);
    host.deliver_one(user_line(pane, prompt));
    (host, pane)
}

/// REQ: F-033
#[test]
fn f_033_done_clears_the_prompt_slot_and_sets_the_recover_hint() {
    let (mut host, pane) = host_with_prompt("fix the build");
    let submissions = host.deliver_one(activity(pane, Working, Done));
    assert_eq!(
        cleared_slots(&submissions),
        vec![(pane, "prompt".to_string())]
    );
    assert_eq!(
        set_slots(&submissions),
        vec![(
            pane,
            "recover".to_string(),
            "Stopped. Last prompt: fix the build".to_string()
        )]
    );
}

/// REQ: F-033
#[test]
fn f_033_the_prompt_slot_is_cleared_before_the_recover_hint_is_set() {
    let (mut host, pane) = host_with_prompt("fix the build");
    let submissions = host.deliver_one(activity(pane, Working, Done));
    assert_eq!(submissions.len(), 2);
    assert!(matches!(
        submissions[0].intent,
        ilium_plugin_sdk::types::Intent::ClearFrameSlot(_)
    ));
    assert!(matches!(
        submissions[1].intent,
        ilium_plugin_sdk::types::Intent::SetFrameSlot(_)
    ));
}

/// REQ: F-033
#[test]
fn f_033_activity_none_also_clears_and_recovers() {
    let (mut host, pane) = host_with_prompt("fix the build");
    let submissions = host.deliver_one(activity(pane, Working, AgentActivity::None));
    assert_eq!(
        cleared_slots(&submissions),
        vec![(pane, "prompt".to_string())]
    );
    assert_eq!(set_slots(&submissions).len(), 1);
}

/// REQ: F-033
#[test]
fn f_033_without_a_known_prompt_only_the_banner_is_cleared() {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Working);
    let submissions = host.deliver_one(activity(pane, Working, Done));
    assert_eq!(
        cleared_slots(&submissions),
        vec![(pane, "prompt".to_string())]
    );
    assert!(set_slots(&submissions).is_empty());
}

/// REQ: F-033
#[test]
fn f_033_working_does_not_clear_the_banner() {
    let (mut host, pane) = host_with_prompt("go");
    assert!(host.deliver_one(activity(pane, Idle, Working)).is_empty());
}

/// REQ: F-033
#[test]
fn f_033_idle_does_not_clear_the_banner() {
    let (mut host, pane) = host_with_prompt("go");
    assert!(host.deliver_one(activity(pane, Working, Idle)).is_empty());
}

/// REQ: F-033
#[test]
fn f_033_awaiting_approval_does_not_clear_the_banner() {
    let (mut host, pane) = host_with_prompt("go");
    assert!(host
        .deliver_one(activity(pane, Working, AwaitingApproval))
        .is_empty());
}

/// REQ: F-033
#[test]
fn f_033_menu_does_not_clear_the_banner() {
    let (mut host, pane) = host_with_prompt("go");
    assert!(host.deliver_one(activity(pane, Working, Menu)).is_empty());
}

/// REQ: F-033
#[test]
fn f_033_recover_hint_uses_the_last_prompt_of_that_pane_only() {
    let mut host = client_host();
    let first = add_agent_pane(&mut host, "one", "claude", AgentActivity::Working);
    let second = add_agent_pane(&mut host, "two", "codex", AgentActivity::Working);
    host.deliver(vec![user_line(first, "alpha"), user_line(second, "beta")]);
    let submissions = host.deliver_one(activity(second, Working, Done));
    assert_eq!(
        set_slots(&submissions),
        vec![(
            second,
            "recover".to_string(),
            "Stopped. Last prompt: beta".to_string()
        )]
    );
    assert_eq!(
        cleared_slots(&submissions),
        vec![(second, "prompt".to_string())]
    );
}

/// REQ: F-033
#[test]
fn f_033_recover_hint_carries_the_latest_of_several_prompts() {
    let (mut host, pane) = host_with_prompt("first");
    host.deliver_one(user_line(pane, "second"));
    let submissions = host.deliver_one(activity(pane, Working, Done));
    assert_eq!(set_slots(&submissions)[0].2, "Stopped. Last prompt: second");
}

/// REQ: F-033
#[test]
fn f_033_recover_hint_carries_the_trimmed_prompt() {
    let (mut host, pane) = host_with_prompt("   padded   ");
    let submissions = host.deliver_one(activity(pane, Working, Done));
    assert_eq!(set_slots(&submissions)[0].2, "Stopped. Last prompt: padded");
}

/// REQ: F-033
#[test]
fn f_033_a_new_prompt_after_done_sets_the_banner_again() {
    let (mut host, pane) = host_with_prompt("first");
    host.deliver_one(activity(pane, Working, Done));
    let slots = set_slots(&host.deliver_one(user_line(pane, "again")));
    assert_eq!(
        slots,
        vec![(pane, "prompt".to_string(), "> again".to_string())]
    );
}

/// REQ: F-033
#[test]
fn f_033_clear_and_recover_submissions_are_accepted_by_the_host() {
    let (mut host, pane) = host_with_prompt("first");
    host.deliver_one(activity(pane, Working, Done));
    host.settle();
    assert!(
        host.outcomes().iter().all(|outcome| outcome.is_ok()),
        "{:?}",
        host.outcomes()
    );
}
