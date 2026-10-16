//! F-033 Last-prompt banner: `user-line-submitted` becomes `set-frame-slot` on the `prompt` slot.

use ilium_plugin_sdk::types::{AgentActivity, Event, Intent, NodeId};
use proptest::prelude::*;

use crate::common::*;

fn banner_for(prompt: &str) -> Vec<(u64, String, String)> {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Working);
    set_slots(&host.deliver_one(user_line(pane, prompt)))
}

/// REQ: F-033
#[test]
fn f_033_submitted_line_sets_the_prompt_slot_of_its_pane() {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Working);
    let slots = set_slots(&host.deliver_one(user_line(pane, "refactor the parser to use a lexer")));
    assert_eq!(slots, vec![(pane, "prompt".to_string(), "> refactor the parser to use a lexer".to_string())]);
}

/// REQ: F-033
#[test]
fn f_033_banner_text_is_prefixed_with_greater_than_and_a_space() {
    let slots = banner_for("hello");
    assert_eq!(slots[0].2, "> hello");
}

/// REQ: F-033
#[test]
fn f_033_surrounding_whitespace_is_trimmed() {
    let slots = banner_for("  \t hello world \n ");
    assert_eq!(slots[0].2, "> hello world");
}

/// REQ: F-033
#[test]
fn f_033_empty_line_sets_nothing() {
    assert!(banner_for("").is_empty());
}

/// REQ: F-033
#[test]
fn f_033_whitespace_only_line_sets_nothing() {
    assert!(banner_for("  \t\n  ").is_empty());
}

/// REQ: F-033
#[test]
fn f_033_prompt_of_119_characters_is_kept_whole() {
    let prompt = "a".repeat(119);
    assert_eq!(banner_for(&prompt)[0].2, format!("> {prompt}"));
}

/// REQ: F-033
#[test]
fn f_033_prompt_of_120_characters_is_kept_whole() {
    let prompt = "b".repeat(120);
    assert_eq!(banner_for(&prompt)[0].2, format!("> {prompt}"));
}

/// REQ: F-033
#[test]
fn f_033_prompt_of_121_characters_is_cut_to_120() {
    let prompt = "c".repeat(121);
    assert_eq!(banner_for(&prompt)[0].2, format!("> {}", "c".repeat(120)));
}

/// REQ: F-033
#[test]
fn f_033_the_cap_counts_characters_not_bytes() {
    let prompt = "é".repeat(150);
    let text = banner_for(&prompt)[0].2.clone();
    assert_eq!(text, format!("> {}", "é".repeat(120)));
}

/// REQ: F-033
#[test]
fn f_033_the_cap_applies_after_trimming() {
    let prompt = format!("   {}   ", "d".repeat(120));
    assert_eq!(banner_for(&prompt)[0].2, format!("> {}", "d".repeat(120)));
}

/// REQ: F-033
#[test]
fn f_033_multiline_prompt_keeps_its_line_breaks_in_the_content() {
    let slots = banner_for("one\ntwo\nthree");
    assert_eq!(slots[0].2, "> one\ntwo\nthree");
}

/// REQ: F-033
#[test]
fn f_033_each_pane_gets_its_own_banner_in_one_batch() {
    let mut host = client_host();
    let first = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Working);
    let second = add_agent_pane(&mut host, "codex", "codex", AgentActivity::Working);
    let slots = set_slots(&host.deliver(vec![user_line(first, "alpha"), user_line(second, "beta")]));
    assert_eq!(
        slots,
        vec![(first, "prompt".to_string(), "> alpha".to_string()), (second, "prompt".to_string(), "> beta".to_string())]
    );
}

/// REQ: F-033
#[test]
fn f_033_an_empty_line_in_a_batch_does_not_hide_the_others() {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Working);
    let slots = set_slots(&host.deliver(vec![user_line(pane, "first"), user_line(pane, "  "), user_line(pane, "third")]));
    let texts: Vec<&str> = slots.iter().map(|slot| slot.2.as_str()).collect();
    assert_eq!(texts, ["> first", "> third"]);
}

/// REQ: F-033
#[test]
fn f_033_a_later_prompt_replaces_the_earlier_one_in_the_same_slot() {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Working);
    host.deliver_one(user_line(pane, "first prompt"));
    let slots = set_slots(&host.deliver_one(user_line(pane, "second prompt")));
    assert_eq!(slots, vec![(pane, "prompt".to_string(), "> second prompt".to_string())]);
}

/// REQ: F-033
#[test]
fn f_033_the_banner_submission_is_accepted_by_the_host() {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Working);
    host.deliver_one(user_line(pane, "do it"));
    host.settle();
    assert!(host.outcomes().iter().all(|outcome| outcome.is_ok()), "{:?}", host.outcomes());
}

/// REQ: F-033
#[test]
fn f_033_only_the_prompt_slot_is_written_by_a_submitted_line() {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Working);
    let submissions = host.deliver_one(user_line(pane, "do it"));
    assert_eq!(submissions.len(), 1);
    assert!(matches!(&submissions[0].intent, Intent::SetFrameSlot(spec) if spec.slot == "prompt" && spec.icon.is_none() && spec.tooltip.is_none()));
}

/// REQ: F-033
#[test]
fn f_033_unrelated_events_produce_no_banner_intents() {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Working);
    let submissions = host.deliver(vec![Event::PaneFocused(Some(NodeId(pane))), Event::ThemeChanged]);
    assert!(set_slots(&submissions).is_empty());
}

/// REQ: F-033
#[test]
fn f_033_goal_changes_are_remembered_without_submitting() {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "codex", "codex", AgentActivity::Working);
    let goal = Event::AgentGoalChanged(ilium_plugin_sdk::types::AgentGoalEvent {
        node: NodeId(pane),
        goal: Some("ship the lexer".to_string()),
    });
    assert!(host.deliver_one(goal).is_empty());
}

/// REQ: F-033
#[test]
fn f_033_a_cleared_goal_submits_nothing_either() {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "codex", "codex", AgentActivity::Working);
    let set = Event::AgentGoalChanged(ilium_plugin_sdk::types::AgentGoalEvent { node: NodeId(pane), goal: Some("g".into()) });
    let cleared = Event::AgentGoalChanged(ilium_plugin_sdk::types::AgentGoalEvent { node: NodeId(pane), goal: None });
    host.deliver_one(set);
    assert!(host.deliver_one(cleared).is_empty());
}

proptest! {
    // REQ: F-033
    #[test]
    fn f_033_property_banner_is_prefix_plus_trimmed_capped_prompt(prompt in "[ a-zA-Z0-9\\n\\téß]{0,200}") {
        let mut host = client_host();
        let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Working);
        let slots = set_slots(&host.deliver_one(user_line(pane, &prompt)));
        if prompt.trim().is_empty() {
            prop_assert!(slots.is_empty());
        } else {
            prop_assert_eq!(slots.len(), 1);
            prop_assert_eq!(&slots[0].2, &expected_banner(&prompt));
            prop_assert!(slots[0].2.chars().count() <= BANNER_CHARS + 2);
        }
    }
}
