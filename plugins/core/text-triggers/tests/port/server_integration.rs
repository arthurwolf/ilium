//! Twins of ilium-server/tests/text_triggers.rs: end to end behaviour of detection, delay and answering on a live pane.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ErrorCode, Event, Outcome};
use serde_json::{json, Value};

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium-server/tests/text_triggers.rs::a_fresh_server_attaches_with_durable_rules_without_a_client_replacement
#[test]
fn port_001_a_fresh_server_attaches_with_durable_rules_without_a_client_replacement() {
    let mut host = host_with_table(json!({"patterns": ["go"], "replies": ["yes"]}));
    let pane = add_agent_pane(&mut host, "claude");
    host.settle();
    assert_eq!(filter_regexes(&host, pane), ["go"]);
}

/// REQ: PORT:ilium-server/tests/text_triggers.rs::a_match_reappearing_after_the_row_clears_submits_again
#[test]
fn port_002_a_match_reappearing_after_the_row_clears_submits_again() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 4));
    host.advance_clock(DEDUP_WINDOW_MS + 1);
    host.deliver_one(hit(pane, 4));
    assert_eq!(applied(&mut host), 2);
}

/// REQ: PORT:ilium-server/tests/text_triggers.rs::a_running_server_reconciles_durable_trigger_edits_without_client_replacement
#[test]
fn port_003_a_running_server_reconciles_durable_trigger_edits_without_client_replacement() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["old"], &["r"]);
    host.seed_config_table("text_triggers", json!({"patterns": ["new"], "replies": ["r"]}));
    host.settle();
    assert_eq!(filter_regexes(&host, pane), ["new"]);
}

/// REQ: PORT:ilium-server/tests/text_triggers.rs::a_second_attach_receives_accepted_text_triggers_without_replacing_them
#[test]
fn port_004_a_second_attach_receives_accepted_text_triggers_without_replacing_them() {
    let mut host = server_host();
    set_rules(&mut host, &["go"], &["yes"]);
    let mut second = client_host();
    second.seed_config_table("text_triggers", host.config_table("text_triggers").expect("table"));
    second.settle();
    assert_eq!(second.settings().get("patterns"), Some(json!(["go"])));
    assert_eq!(host.settings().get("patterns"), Some(json!(["go"])));
}

/// REQ: PORT:ilium-server/tests/text_triggers.rs::a_stale_client_default_payload_cannot_replace_durable_rules
#[test]
fn port_005_a_stale_client_default_payload_cannot_replace_durable_rules() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["go"], &["yes"]);
    let mut stale = client_host();
    stale.settle();
    assert_eq!(stale.settings().get("patterns"), Some(json!([])));
    assert_eq!(filter_regexes(&host, pane), ["go"]);
    assert_eq!(host.settings().get("patterns"), Some(json!(["go"])));
}

/// REQ: PORT:ilium-server/tests/text_triggers.rs::client_semantic_submission_reaches_the_pty_after_a_separate_enter
#[test]
fn port_006_client_semantic_submission_reaches_the_pty_after_a_separate_enter() {
    let (mut host, pane) = armed("go", "text");
    let submissions = host.deliver_one(hit(pane, 1));
    let specs = send_inputs(&submissions);
    assert_eq!(specs, [expected_spec(pane, "text")]);
    assert_eq!(specs[0].delivery, ilium_plugin_sdk::types::Delivery::Submitted);
}

/// REQ: PORT:ilium-server/tests/text_triggers.rs::matched_terminal_output_submits_the_literal_reply_with_enter
#[test]
fn port_007_matched_terminal_output_submits_the_literal_reply_with_enter() {
    let (mut host, pane) = armed("Continue\\? \\(y/n\\)", "y");
    let submissions = host.deliver_one(matched(pane, &rule_id(0), "Continue? (y/n)", 0));
    assert_eq!(send_inputs(&submissions), [expected_spec(pane, "y")]);
}

/// REQ: PORT:ilium-server/tests/text_triggers.rs::repainting_one_matching_screen_line_submits_only_once
#[test]
fn port_008_repainting_one_matching_screen_line_submits_only_once() {
    let (mut host, pane) = armed("go", "yes");
    assert_eq!(deliver_rows(&mut host, pane, &[2, 2, 2, 2]), (1, 3));
}

/// REQ: PORT:ilium-server/tests/text_triggers.rs::the_reply_waits_for_the_rules_delay_after_detection
#[test]
fn port_009_the_reply_waits_for_the_rules_delay_after_detection() {
    let (mut host, pane) = armed("go", "yes");
    host.settings().set("delays", json!([3])).expect("delay accepted");
    host.settle();
    host.deliver_one(hit(pane, 1));
    host.settle();
    assert!(host.submitted().is_empty());
    host.advance_clock(2_900);
    host.settle();
    assert!(host.submitted().is_empty());
    host.advance_clock(200);
    host.settle();
    assert_eq!(texts(&host.submitted()), ["yes"]);
}

