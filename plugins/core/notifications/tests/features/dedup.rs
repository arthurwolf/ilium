//! Dedup keys of the submissions (api-notifications.md "Dedup keys"): 1 to 128 characters from
//! `A-Za-z0-9._:/-`; the same prompt (node and revision) gets the same key, a new revision a new one.

use ilium_plugin_sdk::types::{AgentActivity, Intent};
use proptest::prelude::*;
use serde_json::json;

use crate::common::*;

fn approval_sound_key(revision: u64) -> Option<String> {
    let mut host = server_host();
    set_setting(&mut host, "sound.events.approval_required", json!(true));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    transition(
        &mut host,
        node,
        AgentActivity::Working,
        AgentActivity::AwaitingApproval,
        revision,
    )
    .iter()
    .find(|submission| matches!(submission.intent, Intent::PlaySound(_)))
    .and_then(|submission| submission.dedup_key.clone())
}

/// REQ: F-047
#[test]
fn dedup_047_the_same_prompt_gets_the_same_key_twice() {
    let first = approval_sound_key(7);
    let second = approval_sound_key(7);
    assert!(first.is_some());
    assert_eq!(first, second);
}

/// REQ: F-047
#[test]
fn dedup_047_a_new_revision_is_a_new_prompt_with_a_new_key() {
    assert_ne!(approval_sound_key(7), approval_sound_key(8));
}

/// REQ: F-047
#[test]
fn dedup_047_a_repeat_of_the_same_event_repeats_the_same_keys() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let first = finish_turn(&mut host, node, 5);
    let second = finish_turn(&mut host, node, 5);
    let keys = |submissions: &[ilium_plugin_sdk::types::Submission]| -> Vec<Option<String>> {
        submissions
            .iter()
            .map(|submission| submission.dedup_key.clone())
            .collect()
    };
    assert_eq!(keys(&first), keys(&second));
}

/// REQ: F-047
#[test]
fn dedup_047_a_later_turn_of_the_same_pane_gets_other_keys() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let first = finish_turn(&mut host, node, 5);
    let later = finish_turn(&mut host, node, 6);
    let first_keys: Vec<_> = first
        .iter()
        .filter_map(|submission| submission.dedup_key.clone())
        .collect();
    let later_keys: Vec<_> = later
        .iter()
        .filter_map(|submission| submission.dedup_key.clone())
        .collect();
    assert!(!first_keys.is_empty());
    assert!(first_keys.iter().all(|key| !later_keys.contains(key)));
}

proptest! {
    // REQ: F-047
    #[test]
    fn dedup_047_property_the_approval_sound_key_is_ask_node_revision(revision in 0u64..u64::MAX) {
        let mut host = server_host();
        set_setting(&mut host, "sound.events.approval_required", json!(true));
        let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
        let submissions = transition(&mut host, node, AgentActivity::Working, AgentActivity::AwaitingApproval, revision);
        let key = submissions
            .iter()
            .find(|submission| matches!(submission.intent, Intent::PlaySound(_)))
            .and_then(|submission| submission.dedup_key.clone());
        prop_assert_eq!(key.clone(), Some(format!("ask:{}:{}", node.0, revision)));
        prop_assert!(is_valid_dedup_key(&key.unwrap()));
    }

    // REQ: F-048
    #[test]
    fn dedup_048_property_every_key_is_valid_whatever_the_pane_name(name in "\\PC{1,200}", revision in 0u64..1_000_000) {
        let mut host = server_host();
        let node = add_pane(&mut host, &name, Some(AgentActivity::Working));
        let mut all = finish_turn(&mut host, node, revision);
        all.extend(transition(&mut host, node, AgentActivity::Done, AgentActivity::AwaitingApproval, revision + 1));
        for submission in &all {
            if let Some(key) = &submission.dedup_key {
                prop_assert!(is_valid_dedup_key(key), "{}", key);
            }
        }
    }

    // REQ: F-048
    #[test]
    fn dedup_048_property_the_keys_of_one_batch_are_distinct(revision in 0u64..1_000_000) {
        let mut host = server_host();
        set_setting(&mut host, "sound.events.approval_required", json!(true));
        let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
        let submissions = transition(&mut host, node, AgentActivity::Working, AgentActivity::AwaitingApproval, revision);
        let mut keys: Vec<String> = submissions.iter().filter_map(|submission| submission.dedup_key.clone()).collect();
        let count = keys.len();
        keys.sort();
        keys.dedup();
        prop_assert_eq!(keys.len(), count);
    }
}
