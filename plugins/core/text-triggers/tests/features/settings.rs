//! The rule-list settings of the Text Triggers kernel tab (`[text_triggers]`): the `patterns` and `replies` list
//! rows of the page sample, their defaults, limits, the `regex` item check and persistence (COR-033, COR-011).

use ilium_plugin_sdk::types::ErrorCode;
use serde_json::{json, Value};

use crate::common::*;

fn rejected(key: &str, value: Value) -> String {
    let mut host = client_host();
    let error = host
        .settings()
        .set(key, value)
        .expect_err("the value is refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    error.message
}

fn many(count: usize) -> Value {
    json!((0..count)
        .map(|index| format!("p{index}"))
        .collect::<Vec<_>>())
}

/// REQ: F-056, COR-011
#[test]
fn feature_056_patterns_default_to_an_empty_list() {
    let mut host = client_host();
    assert_eq!(host.settings().get("patterns"), Some(json!([])));
}

/// REQ: F-056, COR-011
#[test]
fn feature_056_replies_default_to_an_empty_list() {
    let mut host = client_host();
    assert_eq!(host.settings().get("replies"), Some(json!([])));
}

/// REQ: F-056
#[test]
fn feature_056_a_valid_pattern_list_is_accepted_and_read_back_in_order() {
    let mut host = client_host();
    host.settings()
        .set("patterns", json!(["\\(y/n\\)\\s*$", "Apply changes\\?"]))
        .expect("valid");
    assert_eq!(
        host.settings().get("patterns"),
        Some(json!(["\\(y/n\\)\\s*$", "Apply changes\\?"]))
    );
}

/// REQ: F-056
#[test]
fn feature_056_replies_are_stored_in_the_same_order_as_given() {
    let mut host = client_host();
    host.settings()
        .set("replies", json!(["y", "yes"]))
        .expect("valid");
    assert_eq!(host.settings().get("replies"), Some(json!(["y", "yes"])));
}

/// REQ: F-056
#[test]
fn feature_056_an_unclosed_group_pattern_is_invalid_argument() {
    let message = rejected("patterns", json!(["(y/n"]));
    assert!(message.contains("(y/n"), "{message}");
}

/// REQ: F-056
#[test]
fn feature_056_a_look_around_pattern_is_invalid_argument_because_the_dialect_is_rust_regex() {
    rejected("patterns", json!(["foo(?=bar)"]));
}

/// REQ: F-056
#[test]
fn feature_056_a_back_reference_pattern_is_invalid_argument() {
    rejected("patterns", json!(["(a)\\1"]));
}

/// REQ: F-056
#[test]
fn feature_056_a_refused_pattern_keeps_the_previous_list() {
    let mut host = client_host();
    host.settings()
        .set("patterns", json!(["a"]))
        .expect("valid");
    let _ = host.settings().set("patterns", json!(["a", "(b"]));
    assert_eq!(host.settings().get("patterns"), Some(json!(["a"])));
}

/// REQ: F-056
#[test]
fn feature_056_one_invalid_item_refuses_the_whole_list() {
    rejected("patterns", json!(["ok", "(bad", "also ok"]));
}

/// REQ: F-056
#[test]
fn feature_056_a_non_string_pattern_item_is_invalid_argument() {
    rejected("patterns", json!([3]));
}

/// REQ: F-056
#[test]
fn feature_056_a_non_list_patterns_value_is_invalid_argument() {
    rejected("patterns", json!("a"));
}

/// REQ: F-056
#[test]
fn feature_056_thirty_one_patterns_are_accepted() {
    let mut host = client_host();
    host.settings()
        .set("patterns", many(MAX_RULES - 1))
        .expect("n-1");
}

/// REQ: F-056
#[test]
fn feature_056_thirty_two_patterns_are_accepted() {
    let mut host = client_host();
    host.settings().set("patterns", many(MAX_RULES)).expect("n");
}

/// REQ: F-056
#[test]
fn feature_056_thirty_three_patterns_are_invalid_argument() {
    rejected("patterns", many(MAX_RULES + 1));
}

/// REQ: F-056
#[test]
fn feature_056_thirty_three_replies_are_invalid_argument() {
    rejected("replies", many(MAX_RULES + 1));
}

/// REQ: F-056
#[test]
fn feature_056_thirty_two_replies_are_accepted() {
    let mut host = client_host();
    host.settings().set("replies", many(MAX_RULES)).expect("n");
}

/// REQ: F-056
#[test]
fn feature_056_an_empty_pattern_list_clears_the_rules() {
    let mut host = client_host();
    host.settings()
        .set("patterns", json!(["a"]))
        .expect("valid");
    host.settings().set("patterns", json!([])).expect("clear");
    assert_eq!(host.settings().get("patterns"), Some(json!([])));
}

/// REQ: COR-033, COR-011
#[test]
fn settings_033_the_rules_persist_in_the_text_triggers_table_under_the_same_keys() {
    let mut host = client_host();
    host.settings()
        .set("patterns", json!(["a"]))
        .expect("valid");
    host.settings().set("replies", json!(["b"])).expect("valid");
    let table = host
        .kernel_settings()
        .tab("text_triggers")
        .expect("the [text_triggers] table exists");
    assert_eq!(table["patterns"], json!(["a"]));
    assert_eq!(table["replies"], json!(["b"]));
}

/// REQ: COR-033
#[test]
fn settings_033_an_unknown_key_of_the_table_survives_a_rule_change() {
    let mut host = client_host();
    host.kernel_settings().set_tab(
        "text_triggers",
        json!({ "future_key": 7, "patterns": [], "replies": [] }),
    );
    host.settings()
        .set("patterns", json!(["a"]))
        .expect("valid");
    let table = host.kernel_settings().tab("text_triggers").expect("table");
    assert_eq!(table["future_key"], json!(7));
    assert_eq!(table["patterns"], json!(["a"]));
}

/// REQ: COR-033
#[test]
fn settings_033_changing_replies_leaves_the_patterns_untouched() {
    let mut host = client_host();
    host.settings()
        .set("patterns", json!(["a"]))
        .expect("valid");
    host.settings().set("replies", json!(["x"])).expect("valid");
    host.settings().set("replies", json!(["y"])).expect("valid");
    assert_eq!(host.settings().get("patterns"), Some(json!(["a"])));
}

/// REQ: F-056
#[test]
fn feature_056_changing_the_rules_on_the_client_reaches_the_server_side_filters() {
    let mut server = server_host();
    let pane = add_agent_pane(&mut server, "claude");
    set_rules(&mut server, &["first"], &["1"]);
    set_rules(&mut server, &["second"], &["2"]);
    let filters = server.output_filters();
    let mine: Vec<_> = filters.iter().filter(|(node, _)| *node == pane).collect();
    assert_eq!(mine.len(), 1, "{filters:?}");
    assert_eq!(mine[0].1.regex, "second");
}
