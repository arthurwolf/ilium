//! The extension points, service and topic the page declares: `rule-packs` (collect) and `before-fire` (hook,
//! veto), the `test-rule` service and the `fired` topic.

use ilium_plugin_sdk::exports::HookReply;
use serde_json::json;

use crate::common::*;
use crate::manual::{page_text, table_after};

fn point_rows() -> Vec<Vec<String>> {
    table_after(&page_text(), "| Point | Kind |")
}

/// REQ: F-056
#[test]
fn feature_056_the_page_lists_exactly_two_points_in_order() {
    let names: Vec<String> = point_rows()
        .into_iter()
        .map(|row| row[0].trim_matches('`').to_string())
        .collect();
    assert_eq!(names, ["rule-packs", "before-fire"]);
}

/// REQ: F-056
#[test]
fn feature_056_rule_packs_is_a_collect_point_with_the_documented_entry() {
    let rows = point_rows();
    let row = rows
        .iter()
        .find(|row| row[0] == "`rule-packs`")
        .expect("row");
    assert_eq!(row[1], "collect");
    assert_eq!(
        row[2],
        "Ship a group of rules the user can enable as a set."
    );
    assert_eq!(
        row[3],
        "entry: id, title, rules (list of pattern, reply, enabled)"
    );
}

/// REQ: F-056
#[test]
fn feature_056_before_fire_is_a_veto_hook_with_rule_pane_match_and_reply_length() {
    let rows = point_rows();
    let row = rows
        .iter()
        .find(|row| row[0] == "`before-fire`")
        .expect("row");
    assert_eq!(row[1], "hook (veto)");
    assert_eq!(row[2], "Refuse or end a rule that is about to send text.");
    assert_eq!(row[3], "rule, pane, match, reply_length");
    assert_eq!(row[4], "high");
}

/// REQ: F-056
#[test]
fn feature_056_the_page_names_the_provide_and_contribute_grants() {
    let page = page_text();
    assert!(page.contains("grant `extensions.provide:ilium.core.text-triggers/<point>`"));
    assert!(page.contains("`extensions.contribute:ilium.core.text-triggers/<point>`"));
}

/// REQ: F-056
#[test]
fn feature_056_the_test_rule_service_runs_a_pattern_against_sample_text() {
    let page = page_text();
    assert!(page.contains(
        "- `ilium.core.text-triggers/test-rule`: test: run a pattern against sample text"
    ));
    assert!(page.contains("(grant `services.use:<name>`)"));
}

/// REQ: F-056
#[test]
fn feature_056_the_fired_topic_payload_is_rule_and_pane() {
    assert!(page_text().contains("- `ilium.core.text-triggers/fired`: { rule, pane }"));
}

/// REQ: F-056
#[test]
fn feature_056_the_plugin_is_replaceable() {
    assert!(page_text().contains(
        "**Replaceable:** yes. A replacement lists `\"replaces\": [\"ilium.core.text-triggers\"]`"
    ));
}

/// REQ: F-056
#[test]
fn feature_056_without_a_contributor_before_fire_passes_and_the_reply_is_sent() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["\\(y/n\\)\\s*$"], &["y"]);
    let submissions = host.deliver_one(matched(pane, &rule_id(0), "Continue? (y/n)", 1));
    assert_eq!(send_inputs(&submissions), [expected_spec(pane, "y")]);
}

/// REQ: F-056
#[test]
fn feature_056_a_refusing_before_fire_contributor_stops_the_send_input() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["\\(y/n\\)\\s*$"], &["y"]);
    host.set_hook_reply(
        "before-fire",
        HookReply::Refuse("outside working hours".to_string()),
    );
    let submissions = host.deliver_one(matched(pane, &rule_id(0), "Continue? (y/n)", 1));
    host.settle();
    assert!(send_inputs(&submissions).is_empty(), "{submissions:?}");
}

/// REQ: F-056
#[test]
fn feature_056_a_passing_before_fire_contributor_keeps_the_send_input() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["\\(y/n\\)\\s*$"], &["y"]);
    host.set_hook_reply("before-fire", HookReply::Pass);
    let submissions = host.deliver_one(matched(pane, &rule_id(0), "Continue? (y/n)", 1));
    assert_eq!(send_inputs(&submissions), [expected_spec(pane, "y")]);
}

/// REQ: F-056
#[test]
fn feature_056_a_refusal_with_a_reply_the_veto_mode_does_not_allow_counts_as_pass() {
    // Page: a handler that returns a reply the mode does not allow counts as `pass`. A veto allows pass/refuse.
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["go"], &["y"]);
    host.set_hook_reply("before-fire", HookReply::Replace("n".to_string()));
    let submissions = host.deliver_one(matched(pane, &rule_id(0), "go", 1));
    assert_eq!(send_inputs(&submissions), [expected_spec(pane, "y")]);
}

/// REQ: F-056
#[test]
fn feature_056_the_before_fire_payload_carries_rule_pane_match_and_reply_length() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["go"], &["yes please"]);
    host.set_hook_reply("before-fire", HookReply::Pass);
    host.deliver_one(matched(pane, &rule_id(0), "go", 1));
    let calls = host.hook_calls("before-fire");
    assert_eq!(calls.len(), 1, "{calls:?}");
    let payload: serde_json::Value =
        serde_json::from_str(&calls[0].payload_json).expect("json payload");
    assert_eq!(payload["rule"], json!(rule_id(0)));
    assert_eq!(payload["pane"], json!(pane));
    assert_eq!(payload["match"], json!("go"));
    assert_eq!(payload["reply_length"], json!(10));
}

/// REQ: F-056
#[test]
fn feature_056_a_refused_rule_still_leaves_other_rules_free_to_fire() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["one", "two"], &["1", "2"]);
    host.set_hook_reply("before-fire", HookReply::Refuse("no".to_string()));
    let refused = host.deliver_one(matched(pane, &rule_id(0), "one", 1));
    assert!(send_inputs(&refused).is_empty());
    host.set_hook_reply("before-fire", HookReply::Pass);
    let allowed = host.deliver_one(matched(pane, &rule_id(1), "two", 2));
    assert_eq!(send_inputs(&allowed), [expected_spec(pane, "2")]);
}

/// REQ: F-056
#[test]
fn feature_056_without_a_rule_pack_no_output_matched_event_sends_input() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    let submissions = host.deliver_one(matched(pane, "confirmations-0", "Proceed?", 2));
    assert!(send_inputs(&submissions).is_empty());
}

/// REQ: F-056
#[test]
fn feature_056_a_rule_pack_contributes_rules_the_user_can_enable_as_a_set() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    host.contribute_collect(
        "rule-packs",
        &json!({ "id": "confirmations", "title": "Confirmations",
                 "rules": [{ "pattern": "Proceed\\?", "reply": "yes", "enabled": true }] })
        .to_string(),
    );
    host.settle();
    // The id of a pack rule filter is `<pack id>-<index>` (assumption recorded in API_GAPS.md).
    let submissions = host.deliver_one(matched(pane, "confirmations-0", "Proceed?", 2));
    assert_eq!(send_inputs(&submissions), [expected_spec(pane, "yes")]);
}

/// REQ: F-056
#[test]
fn feature_056_a_disabled_rule_in_a_pack_does_not_fire() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    host.contribute_collect(
        "rule-packs",
        &json!({ "id": "confirmations", "title": "Confirmations",
                 "rules": [{ "pattern": "Proceed\\?", "reply": "yes", "enabled": false }] })
        .to_string(),
    );
    host.settle();
    let submissions = host.deliver_one(matched(pane, "confirmations-0", "Proceed?", 2));
    assert!(send_inputs(&submissions).is_empty());
}

/// REQ: F-056
#[test]
fn feature_056_a_fired_rule_publishes_the_fired_topic_with_rule_and_pane() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["go"], &["y"]);
    host.deliver_one(matched(pane, &rule_id(0), "go", 1));
    host.settle();
    let published = host.published("ilium.core.text-triggers/fired");
    assert_eq!(
        published,
        [json!({ "rule": rule_id(0), "pane": pane }).to_string()]
    );
}

/// REQ: F-056
#[test]
fn feature_056_a_refused_rule_does_not_publish_the_fired_topic() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["go"], &["y"]);
    host.set_hook_reply("before-fire", HookReply::Refuse("no".to_string()));
    host.deliver_one(matched(pane, &rule_id(0), "go", 1));
    host.settle();
    assert!(host.published("ilium.core.text-triggers/fired").is_empty());
}
