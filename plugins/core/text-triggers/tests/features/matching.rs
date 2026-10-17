//! Server side behaviour: each rule is one output filter on every agent pane, each `output-matched` event is
//! answered by one `send-input` intent, floods are logged from `events-dropped`, and matches are counted.

use ilium_plugin_sdk::host::Bucket;
use ilium_plugin_sdk::types::{Delivery, ErrorCode, Event, NodeId, Outcome};
use serde_json::json;

use crate::common::*;

const CONFIRM: &str = "\\(y/n\\)\\s*$";

fn host_with_confirm_rule() -> (ilium_plugin_sdk::testing::FakeHost, u64) {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &[CONFIRM], &["y"]);
    (host, pane)
}

/// REQ: F-056
#[test]
fn feature_056_a_rule_becomes_one_output_filter_on_an_agent_pane() {
    let (host, pane) = host_with_confirm_rule();
    let filters: Vec<_> = host
        .output_filters()
        .into_iter()
        .filter(|(node, _)| *node == pane)
        .collect();
    assert_eq!(filters.len(), 1, "{filters:?}");
    assert_eq!(filters[0].1.id, rule_id(0));
    assert_eq!(filters[0].1.regex, CONFIRM);
    assert_eq!(filters[0].1.rate_per_second, RATE_PER_SECOND);
}

/// REQ: F-056
#[test]
fn feature_056_two_rules_become_two_filters_with_distinct_ids() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &[CONFIRM, "Apply changes\\?"], &["y", "yes"]);
    let mut ids: Vec<String> = host
        .output_filters()
        .into_iter()
        .filter(|(node, _)| *node == pane)
        .map(|(_, filter)| filter.id)
        .collect();
    ids.sort();
    assert_eq!(ids, [rule_id(0), rule_id(1)]);
}

/// REQ: F-056
#[test]
fn feature_056_a_pane_created_after_the_rules_gets_the_filters_too() {
    let mut host = server_host();
    set_rules(&mut host, &[CONFIRM], &["y"]);
    let pane = add_agent_pane(&mut host, "codex");
    host.settle();
    assert_eq!(
        host.output_filters()
            .iter()
            .filter(|(node, _)| *node == pane)
            .count(),
        1
    );
}

/// REQ: F-056
#[test]
fn feature_056_a_terminal_pane_without_an_agent_gets_no_filter() {
    let mut host = server_host();
    let pane = host.tree().add(ilium_plugin_sdk::testing::TreeNode {
        id: 0,
        parent: None,
        name: "shell".to_string(),
        class: ilium_plugin_sdk::types::NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: None,
    });
    set_rules(&mut host, &[CONFIRM], &["y"]);
    assert_eq!(
        host.output_filters()
            .iter()
            .filter(|(node, _)| *node == pane)
            .count(),
        0
    );
}

/// REQ: F-056
#[test]
fn feature_056_removing_a_rule_removes_its_filter() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["a", "b"], &["1", "2"]);
    set_rules(&mut host, &["a"], &["1"]);
    let ids: Vec<String> = host
        .output_filters()
        .into_iter()
        .filter(|(node, _)| *node == pane)
        .map(|(_, filter)| filter.id)
        .collect();
    assert_eq!(ids, [rule_id(0)]);
}

/// REQ: F-056
#[test]
fn feature_056_clearing_all_rules_leaves_no_filter() {
    let (mut host, _) = host_with_confirm_rule();
    set_rules(&mut host, &[], &[]);
    assert!(host.output_filters().is_empty());
}

/// REQ: F-056
#[test]
fn feature_056_a_removed_pane_drops_its_filters() {
    let (mut host, pane) = host_with_confirm_rule();
    host.tree().remove(pane);
    host.settle();
    assert!(host.output_filters().iter().all(|(node, _)| *node != pane));
}

/// REQ: F-056
#[test]
fn feature_056_a_match_is_answered_with_a_submitted_send_input_of_the_reply() {
    let (mut host, pane) = host_with_confirm_rule();
    let submissions = host.deliver_one(matched(pane, &rule_id(0), "Continue? (y/n)", 5));
    assert_eq!(send_inputs(&submissions), [expected_spec(pane, "y")]);
    assert_eq!(send_inputs(&submissions)[0].delivery, Delivery::Submitted);
}

/// REQ: F-056
#[test]
fn feature_056_the_send_input_goes_to_the_pane_that_printed_the_line() {
    let mut host = server_host();
    let first = add_agent_pane(&mut host, "claude");
    let second = add_agent_pane(&mut host, "codex");
    set_rules(&mut host, &[CONFIRM], &["y"]);
    let submissions = host.deliver_one(matched(second, &rule_id(0), "Continue? (y/n)", 1));
    assert_eq!(send_inputs(&submissions)[0].pane, NodeId(second));
    assert_ne!(send_inputs(&submissions)[0].pane, NodeId(first));
}

/// REQ: F-056
#[test]
fn feature_056_the_send_input_carries_the_dedup_key_of_filter_node_and_row() {
    let (mut host, pane) = host_with_confirm_rule();
    let submissions = host.deliver_one(matched(pane, &rule_id(0), "Continue? (y/n)", 5));
    assert_eq!(submissions.len(), 1);
    assert_eq!(
        submissions[0].dedup_key,
        Some(dedup_key(&rule_id(0), pane, 5))
    );
}

/// REQ: F-056
#[test]
fn feature_056_a_repeated_line_produces_the_same_dedup_key() {
    let (mut host, pane) = host_with_confirm_rule();
    let again = host.deliver(vec![
        matched(pane, &rule_id(0), "Continue? (y/n)", 5),
        matched(pane, &rule_id(0), "Continue? (y/n)", 5),
    ]);
    assert_eq!(again.len(), 2);
    assert_eq!(again[0].dedup_key, again[1].dedup_key);
}

/// REQ: F-056
#[test]
fn feature_056_a_repeated_line_is_answered_once_by_the_host_dedup() {
    let (mut host, pane) = host_with_confirm_rule();
    host.deliver(vec![
        matched(pane, &rule_id(0), "Continue? (y/n)", 5),
        matched(pane, &rule_id(0), "Continue? (y/n)", 5),
    ]);
    host.settle();
    let ok = host
        .outcomes()
        .into_iter()
        .filter(|outcome| !matches!(outcome, Outcome::Duplicate(_)))
        .count();
    let duplicate = host
        .outcomes()
        .into_iter()
        .filter(|outcome| matches!(outcome, Outcome::Duplicate(_)))
        .count();
    assert_eq!((ok, duplicate), (1, 1), "{:?}", host.outcomes());
}

/// REQ: F-056
#[test]
fn feature_056_a_new_row_of_the_same_text_is_answered_again() {
    let (mut host, pane) = host_with_confirm_rule();
    let submissions = host.deliver(vec![
        matched(pane, &rule_id(0), "Continue? (y/n)", 5),
        matched(pane, &rule_id(0), "Continue? (y/n)", 9),
    ]);
    assert_ne!(submissions[0].dedup_key, submissions[1].dedup_key);
    assert_eq!(send_inputs(&submissions).len(), 2);
}

/// REQ: F-056
#[test]
fn feature_056_the_same_row_on_another_pane_is_a_different_key() {
    let mut host = server_host();
    let first = add_agent_pane(&mut host, "claude");
    let second = add_agent_pane(&mut host, "codex");
    set_rules(&mut host, &[CONFIRM], &["y"]);
    let submissions = host.deliver(vec![
        matched(first, &rule_id(0), "Continue? (y/n)", 5),
        matched(second, &rule_id(0), "Continue? (y/n)", 5),
    ]);
    assert_ne!(submissions[0].dedup_key, submissions[1].dedup_key);
}

/// REQ: F-056
#[test]
fn feature_056_each_rule_answers_with_its_own_reply() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &[CONFIRM, "Apply changes\\?"], &["y", "yes"]);
    let submissions = host.deliver(vec![
        matched(pane, &rule_id(1), "Apply changes?", 2),
        matched(pane, &rule_id(0), "Continue? (y/n)", 3),
    ]);
    let texts: Vec<String> = send_inputs(&submissions)
        .into_iter()
        .map(|spec| spec.text)
        .collect();
    assert_eq!(texts, ["yes", "y"]);
}

/// REQ: F-056
#[test]
fn feature_056_submissions_follow_the_order_of_the_events_of_the_batch() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["a", "b", "c"], &["1", "2", "3"]);
    let submissions = host.deliver(vec![
        matched(pane, &rule_id(2), "c", 1),
        matched(pane, &rule_id(0), "a", 2),
        matched(pane, &rule_id(1), "b", 3),
    ]);
    let texts: Vec<String> = send_inputs(&submissions)
        .into_iter()
        .map(|spec| spec.text)
        .collect();
    assert_eq!(texts, ["3", "1", "2"]);
}

/// REQ: F-056
#[test]
fn feature_056_a_match_of_an_unknown_filter_id_sends_nothing() {
    let (mut host, pane) = host_with_confirm_rule();
    let submissions = host.deliver_one(matched(pane, "rule-9", "Continue? (y/n)", 1));
    assert!(send_inputs(&submissions).is_empty());
}

/// REQ: F-056
#[test]
fn feature_056_a_match_with_a_foreign_filter_id_sends_nothing() {
    let (mut host, pane) = host_with_confirm_rule();
    let submissions = host.deliver_one(matched(pane, "rate-limit-seen", "Continue? (y/n)", 1));
    assert!(send_inputs(&submissions).is_empty());
}

/// REQ: F-056
#[test]
fn feature_056_events_other_than_matches_send_nothing() {
    let (mut host, _) = host_with_confirm_rule();
    let submissions = host.deliver(vec![
        Event::PaneFocused(None),
        Event::TreeChanged(vec![NodeId(1)]),
    ]);
    assert!(send_inputs(&submissions).is_empty());
}

/// REQ: F-056
#[test]
fn feature_056_an_empty_batch_returns_no_submission() {
    let (mut host, _) = host_with_confirm_rule();
    assert!(host.deliver(Vec::new()).is_empty());
}

/// REQ: F-056
#[test]
fn feature_056_the_reply_is_sent_literally_without_template_expansion() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["go"], &["{pane} $1 \\n"]);
    let submissions = host.deliver_one(matched(pane, &rule_id(0), "go", 1));
    assert_eq!(send_inputs(&submissions)[0].text, "{pane} $1 \\n");
}

/// REQ: F-056
#[test]
fn feature_056_a_disabled_rule_sends_nothing() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["a", "b"], &["1", "2"]);
    host.settings()
        .set("enabled", json!([true, false]))
        .expect("enabled flags");
    host.settle();
    let submissions = host.deliver(vec![
        matched(pane, &rule_id(0), "a", 1),
        matched(pane, &rule_id(1), "b", 2),
    ]);
    let texts: Vec<String> = send_inputs(&submissions)
        .into_iter()
        .map(|spec| spec.text)
        .collect();
    assert_eq!(texts, ["1"]);
}

/// REQ: F-056
#[test]
fn feature_056_a_disabled_rule_has_no_filter() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["a", "b"], &["1", "2"]);
    host.settings()
        .set("enabled", json!([false, true]))
        .expect("enabled flags");
    host.settle();
    let ids: Vec<String> = host
        .output_filters()
        .into_iter()
        .filter(|(node, _)| *node == pane)
        .map(|(_, filter)| filter.id)
        .collect();
    assert_eq!(ids, [rule_id(1)]);
}

/// REQ: F-056
#[test]
fn feature_056_enabling_a_rule_again_restores_its_filter() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["a"], &["1"]);
    host.settings().set("enabled", json!([false])).expect("off");
    host.settings().set("enabled", json!([true])).expect("on");
    host.settle();
    assert_eq!(
        host.output_filters()
            .iter()
            .filter(|(node, _)| *node == pane)
            .count(),
        1
    );
}

/// REQ: F-056
#[test]
fn feature_056_a_match_writes_one_info_log_line_with_rule_and_line() {
    let (mut host, pane) = host_with_confirm_rule();
    host.deliver_one(matched(pane, &rule_id(0), "Continue? (y/n)", 1));
    let lines: Vec<_> = host
        .logs()
        .into_iter()
        .filter(|line| line.level == "info")
        .collect();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].message, "rule rule-0 matched: Continue? (y/n)");
}

/// REQ: F-056
#[test]
fn feature_056_each_match_keeps_a_session_count_per_rule() {
    let (mut host, pane) = host_with_confirm_rule();
    for row in 1..=3 {
        host.deliver_one(matched(pane, &rule_id(0), "Continue? (y/n)", row));
    }
    let bytes = host
        .storage()
        .get(Bucket::Session, "count/rule-0")
        .expect("count stored");
    assert_eq!(u64::from_le_bytes(bytes.try_into().expect("8 bytes")), 3);
}

/// REQ: F-056
#[test]
fn feature_056_the_session_count_is_kept_per_rule() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["a", "b"], &["1", "2"]);
    host.deliver(vec![
        matched(pane, &rule_id(0), "a", 1),
        matched(pane, &rule_id(0), "a", 2),
        matched(pane, &rule_id(1), "b", 3),
    ]);
    let count = |key: &str| {
        u64::from_le_bytes(
            host.storage()
                .get(Bucket::Session, key)
                .expect("count")
                .try_into()
                .expect("8"),
        )
    };
    assert_eq!((count("count/rule-0"), count("count/rule-1")), (2, 1));
}

/// REQ: F-056
#[test]
fn feature_056_a_rule_without_matches_has_no_count_entry() {
    let (host, _) = host_with_confirm_rule();
    assert_eq!(host.storage().get(Bucket::Session, "count/rule-0"), None);
}

/// REQ: F-056
#[test]
fn feature_056_events_dropped_for_output_matched_logs_a_warning_with_count_and_rule() {
    let (mut host, _) = host_with_confirm_rule();
    host.deliver_one(dropped(&rule_id(0), 5));
    let warnings: Vec<_> = host
        .logs()
        .into_iter()
        .filter(|line| line.level == "warn")
        .collect();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(
        warnings[0].message,
        "5 matches dropped for rule Some(\"rule-0\")"
    );
}

/// REQ: F-056
#[test]
fn feature_056_events_dropped_for_another_event_kind_logs_nothing() {
    let (mut host, _) = host_with_confirm_rule();
    host.deliver_one(Event::EventsDropped(vec![
        ilium_plugin_sdk::types::DroppedCount {
            event: "pane-resized".to_string(),
            source: None,
            count: 4,
        },
    ]));
    assert!(host.logs().into_iter().all(|line| line.level != "warn"));
}

/// REQ: F-056
#[test]
fn feature_056_events_dropped_sends_no_input() {
    let (mut host, _) = host_with_confirm_rule();
    let submissions = host.deliver_one(dropped(&rule_id(0), 5));
    assert!(send_inputs(&submissions).is_empty());
}

/// REQ: F-056
#[test]
fn feature_056_one_warning_per_output_matched_record_of_the_event() {
    let (mut host, _) = host_with_confirm_rule();
    host.deliver_one(Event::EventsDropped(vec![
        ilium_plugin_sdk::types::DroppedCount {
            event: "output-matched".to_string(),
            source: Some("rule-0".to_string()),
            count: 2,
        },
        ilium_plugin_sdk::types::DroppedCount {
            event: "output-matched".to_string(),
            source: Some("rule-1".to_string()),
            count: 7,
        },
    ]));
    let messages: Vec<String> = host
        .logs()
        .into_iter()
        .filter(|line| line.level == "warn")
        .map(|line| line.message)
        .collect();
    assert_eq!(
        messages,
        [
            "2 matches dropped for rule Some(\"rule-0\")",
            "7 matches dropped for rule Some(\"rule-1\")"
        ]
    );
}

/// REQ: F-056
#[test]
fn feature_056_a_dropped_count_of_one_is_logged_too() {
    let (mut host, _) = host_with_confirm_rule();
    host.deliver_one(dropped(&rule_id(0), 1));
    assert_eq!(
        host.logs()
            .into_iter()
            .filter(|line| line.level == "warn")
            .count(),
        1
    );
}

/// REQ: F-056
#[test]
fn feature_056_without_terminal_write_the_send_input_is_refused_not_permitted() {
    let (mut host, pane) = host_with_confirm_rule();
    host.revoke("terminal.write");
    host.deliver_one(matched(pane, &rule_id(0), "Continue? (y/n)", 1));
    host.settle();
    let outcomes = host.outcomes();
    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    assert!(
        matches!(&outcomes[0], Outcome::Refused(error) if error.code == ErrorCode::NotPermitted),
        "{outcomes:?}"
    );
}

/// REQ: F-056
#[test]
fn feature_056_without_output_filter_the_filters_are_not_installed() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    host.revoke("terminal.output-filter");
    set_rules(&mut host, &[CONFIRM], &["y"]);
    assert!(host.output_filters().iter().all(|(node, _)| *node != pane));
}

/// REQ: F-056
#[test]
fn feature_056_the_filter_regex_is_the_pattern_text_unchanged() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(
        &mut host,
        &["^Do you want to continue\\? \\(y/n\\)$"],
        &["y"],
    );
    let filter = host
        .output_filters()
        .into_iter()
        .find(|(node, _)| *node == pane)
        .expect("filter")
        .1;
    assert_eq!(filter.regex, "^Do you want to continue\\? \\(y/n\\)$");
}

/// REQ: F-056
#[test]
fn feature_056_a_pattern_with_no_reply_installs_no_filter() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["a", "b"], &["1"]);
    let ids: Vec<String> = host
        .output_filters()
        .into_iter()
        .filter(|(node, _)| *node == pane)
        .map(|(_, filter)| filter.id)
        .collect();
    assert_eq!(ids, [rule_id(0)]);
}
