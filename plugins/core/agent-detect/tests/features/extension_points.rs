//! The extension points, service and topic the page declares: `activity-rules` (collect) and `classify`
//! (hook, transform), the `state` service (get, list) and the `state-changed` topic.

use crate::host_ext::*;
use ilium_plugin_sdk::exports::{HookCall, HookReply};
use ilium_plugin_sdk::types::{AgentActivity, AgentActivityEvent, AgentIdentifiedEvent, Event, NodeId};
use serde_json::{Value, json};

use crate::common::{page_text, server_host};

/// Rows of the Extension points table: `(point, kind, what, payload, risk)`.
fn point_rows() -> Vec<[String; 5]> {
    let page = page_text();
    let start = page.find("| Point | Kind |").expect("extension point table");
    page[start..]
        .lines()
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|line| {
            let cells: Vec<String> = line.trim_matches('|').split('|').map(|cell| cell.trim().to_string()).collect();
            [cells[0].clone(), cells[1].clone(), cells[2].clone(), cells[3].clone(), cells[4].clone()]
        })
        .collect()
}

/// REQ: F-027
#[test]
fn feature_027_the_page_lists_exactly_two_points() {
    let names: Vec<String> = point_rows().into_iter().map(|row| row[0].trim_matches('`').to_string()).collect();
    assert_eq!(names, ["activity-rules", "classify"]);
}

/// REQ: F-027
#[test]
fn feature_027_activity_rules_is_a_collect_point_with_the_documented_entry() {
    let rows = point_rows();
    let row = rows.iter().find(|row| row[0] == "`activity-rules`").expect("row");
    assert_eq!(row[1], "collect");
    assert_eq!(
        row[3],
        "entry: id, signature, state, regex, region (last-lines, status-row, whole-screen)"
    );
    assert_eq!(row[4], "standard");
}

/// REQ: F-027
#[test]
fn feature_027_classify_is_a_transform_hook_replying_with_a_state() {
    let rows = point_rows();
    let row = rows.iter().find(|row| row[0] == "`classify`").expect("row");
    assert_eq!(row[1], "hook (transform)");
    assert_eq!(row[3], "pane, signature, state, reason; reply: state");
    assert_eq!(row[4], "standard");
}

/// REQ: F-027
#[test]
fn feature_027_the_page_names_the_provide_and_contribute_grants() {
    let page = page_text();
    assert!(page.contains("grant `extensions.provide:ilium.core.agent-detect/<point>`"));
    assert!(page.contains("`extensions.contribute:ilium.core.agent-detect/<point>`"));
}

/// REQ: F-026
#[test]
fn feature_026_the_state_service_offers_get_and_list() {
    let page = page_text();
    assert!(page.contains("- `ilium.core.agent-detect/state`: get, list: current identity and activity of panes"));
    assert!(page.contains("(grant `services.use:<name>`)"));
}

/// REQ: F-027
#[test]
fn feature_027_the_state_changed_topic_payload_is_pane_signature_previous_current() {
    let page = page_text();
    assert!(page.contains("- `ilium.core.agent-detect/state-changed`: { pane, signature, previous, current }"));
}

/// REQ: F-026
#[test]
fn feature_026_the_plugin_is_replaceable() {
    let page = page_text();
    assert!(page.contains("**Replaceable:** yes. A replacement lists `\"replaces\": [\"ilium.core.agent-detect\"]`"));
}

/// REQ: F-027
#[test]
fn feature_027_the_classify_hook_without_a_contributor_passes() {
    let mut host = server_host();
    let payload = json!({ "pane": 7, "signature": "claude", "state": "idle", "reason": "no marker" });
    let reply = host.run_hook(HookCall { call: 1, point: "classify".to_string(), payload_json: payload.to_string() });
    assert_eq!(reply, HookReply::Pass);
}

/// REQ: F-027
#[test]
fn feature_027_a_replaced_state_from_the_classify_hook_is_a_valid_activity_name() {
    let mut host = server_host();
    let payload = json!({ "pane": 7, "signature": "claude", "state": "idle", "reason": "rule" });
    let reply = host.run_hook(HookCall { call: 2, point: "classify".to_string(), payload_json: payload.to_string() });
    if let HookReply::Replace(text) = reply {
        let value: Value = serde_json::from_str(&text).expect("JSON reply");
        let state = value["state"].as_str().expect("state");
        assert!(AgentActivity::CASE_NAMES.contains(&state), "{state}");
    }
}

/// REQ: F-027
#[test]
fn feature_027_an_activity_rule_marks_a_claude_pane_awaiting_approval() {
    let mut host = server_host();
    host.contribute_extension(
        "ilium.core.agent-detect/activity-rules",
        "build-waiting",
        50,
        json!({ "signature": "claude", "state": "awaiting-approval", "regex": "Do you want to proceed\\?", "region": "last-lines" }),
    );
    let screen = "building...\nDo you want to proceed?";
    assert_eq!(host.classify("claude", crate::common::screen_from_text(screen)).activity, AgentActivity::AwaitingApproval);
}

/// REQ: F-027
#[test]
fn feature_027_an_activity_rule_for_another_signature_does_not_apply() {
    let mut host = server_host();
    host.contribute_extension(
        "ilium.core.agent-detect/activity-rules",
        "build-waiting",
        50,
        json!({ "signature": "claude", "state": "awaiting-approval", "regex": "Do you want to proceed\\?", "region": "last-lines" }),
    );
    assert_eq!(host.classify("codex", crate::common::screen_from_text("Do you want to proceed?")).activity, AgentActivity::Idle);
}

/// REQ: F-027
#[test]
fn feature_027_a_last_lines_rule_ignores_text_far_above_the_end_of_the_screen() {
    let mut host = server_host();
    host.contribute_extension(
        "ilium.core.agent-detect/activity-rules",
        "build-waiting",
        50,
        json!({ "signature": "claude", "state": "awaiting-approval", "regex": "Do you want to proceed\\?", "region": "last-lines" }),
    );
    let mut screen = String::from("Do you want to proceed?\n");
    for line in 0..40 {
        screen.push_str(&format!("{line} filler\n"));
    }
    assert_eq!(host.classify("claude", crate::common::screen_from_text(&screen)).activity, AgentActivity::Idle);
}

/// REQ: F-026
#[test]
fn feature_026_the_state_service_lists_the_identity_and_activity_of_panes() {
    let mut host = server_host();
    host.deliver(vec![
        Event::AgentIdentified(AgentIdentifiedEvent { node: NodeId(7), signature: "claude".to_string() }),
        Event::AgentActivityChanged(AgentActivityEvent {
            node: NodeId(7),
            previous: AgentActivity::Idle,
            current: AgentActivity::Working,
            revision: 2,
        }),
    ]);
    let reply = host.call_own_service("ilium.core.agent-detect/state", &json!({ "method": "list" }).to_string()).expect("done");
    let value: Value = serde_json::from_str(&reply).expect("JSON");
    let entries = value.as_array().expect("a list");
    assert!(entries.iter().any(|entry| entry["pane"] == 7 && entry["signature"] == "claude" && entry["activity"] == "working"), "{value}");
}

/// REQ: F-026
#[test]
fn feature_026_the_state_service_get_of_an_unknown_pane_is_not_found() {
    let mut host = server_host();
    let error = host
        .call_own_service("ilium.core.agent-detect/state", &json!({ "method": "get", "pane": 999 }).to_string())
        .expect_err("unknown pane");
    assert_eq!(error.code, ilium_plugin_sdk::types::ErrorCode::NotFound);
}

/// REQ: F-027
#[test]
fn feature_027_an_activity_change_publishes_the_state_changed_topic() {
    let mut host = server_host();
    host.deliver(vec![
        Event::AgentIdentified(AgentIdentifiedEvent { node: NodeId(7), signature: "claude".to_string() }),
        Event::AgentActivityChanged(AgentActivityEvent {
            node: NodeId(7),
            previous: AgentActivity::Idle,
            current: AgentActivity::Working,
            revision: 2,
        }),
    ]);
    let published = host.published_topics();
    let (topic, payload) = published.last().expect("a publication");
    assert_eq!(topic, "ilium.core.agent-detect/state-changed");
    let value: Value = serde_json::from_str(payload).expect("JSON");
    assert_eq!(value, json!({ "pane": 7, "signature": "claude", "previous": "idle", "current": "working" }));
}

/// REQ: F-027
#[test]
fn feature_027_each_activity_change_publishes_exactly_one_topic_message() {
    let mut host = server_host();
    host.deliver_one(Event::AgentActivityChanged(AgentActivityEvent {
        node: NodeId(7),
        previous: AgentActivity::Idle,
        current: AgentActivity::Working,
        revision: 2,
    }));
    assert_eq!(host.published_topics().len(), 1);
}
