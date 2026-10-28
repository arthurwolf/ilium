//! Twins of ilium-server text_trigger_config.rs: how the plugin reloads the [text_triggers] table of config.toml and keeps the last valid rules.

use ilium_plugin_sdk::types::ErrorCode;
use serde_json::json;

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium-server/src/text_trigger_config.rs::a_rule_stored_before_the_delay_setting_loads_with_sixty_seconds
#[test]
fn port_001_a_rule_stored_before_the_delay_setting_loads_with_sixty_seconds() {
    let mut host = host_with_table(json!({"patterns": ["a"], "replies": ["r"]}));
    host.settle();
    assert_eq!(host.settings().get("delays"), Some(json!([60])));
}

/// REQ: PORT:ilium-server/src/text_trigger_config.rs::accepting_identical_settings_does_not_publish_or_advance_revision
#[test]
fn port_002_accepting_identical_settings_does_not_publish_or_advance_revision() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["a"], &["r"]);
    let before = host.output_filters();
    host.kernel_settings().set_tab(
        "text_triggers",
        json!({"patterns": ["a"], "replies": ["r"]}),
    );
    host.settle();
    assert_eq!(host.output_filters(), before);
    assert_eq!(filter_ids(&host, pane), [rule_id(0)]);
}

/// REQ: PORT:ilium-server/src/text_trigger_config.rs::oversized_durable_read_retains_the_last_accepted_rules_and_revision
#[test]
fn port_003_oversized_durable_read_retains_the_last_accepted_rules_and_revision() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["a"], &["r"]);
    let huge = "x".repeat(600 * 1024);
    host.kernel_settings().set_tab(
        "text_triggers",
        json!({"patterns": [huge], "replies": ["r"]}),
    );
    host.settle();
    assert_eq!(filter_regexes(&host, pane), ["a"]);
    assert!(host
        .logs()
        .iter()
        .any(|line| line.level == "error" || line.level == "warn"));
}

/// REQ: PORT:ilium-server/src/text_trigger_config.rs::regex_validation_uses_a_real_cpu_worker_and_acceptance_keeps_its_storage
#[test]
fn port_004_regex_validation_uses_a_real_cpu_worker_and_acceptance_keeps_its_storage() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    assert_eq!(
        settings_error(&mut host, "patterns", json!(["(unclosed"])),
        ErrorCode::InvalidArgument
    );
    set_rules(&mut host, &["(a|b)+c"], &["r"]);
    assert_eq!(filter_regexes(&host, pane), ["(a|b)+c"]);
    assert_eq!(host.settings().get("patterns"), Some(json!(["(a|b)+c"])));
}

/// REQ: PORT:ilium-server/src/text_trigger_config.rs::text_trigger_cancelled_reload_cannot_install_and_releases_owner
#[test]
fn port_005_text_trigger_cancelled_reload_cannot_install_and_releases_owner() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["first"], &["r"]);
    host.kernel_settings().set_tab(
        "text_triggers",
        json!({"patterns": ["second"], "replies": ["r"]}),
    );
    host.kernel_settings().set_tab(
        "text_triggers",
        json!({"patterns": ["third"], "replies": ["r"]}),
    );
    host.settle();
    assert_eq!(filter_regexes(&host, pane), ["third"]);
}

/// REQ: PORT:ilium-server/src/text_trigger_config.rs::text_trigger_event_admission_refusal_keeps_the_accepted_rules
#[test]
fn port_006_text_trigger_event_admission_refusal_keeps_the_accepted_rules() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["keep"], &["r"]);
    host.revoke("terminal.output-filter");
    host.kernel_settings().set_tab(
        "text_triggers",
        json!({"patterns": ["other"], "replies": ["r"]}),
    );
    host.settle();
    assert_eq!(host.settings().get("patterns"), Some(json!(["keep"])));
    assert!(host.output_filters().iter().all(|(node, _)| *node != pane));
}

/// REQ: PORT:ilium-server/src/text_trigger_config.rs::text_trigger_explicit_empty_survives_fresh_state_and_noop_preserves_revision
#[test]
fn port_007_text_trigger_explicit_empty_survives_fresh_state_and_noop_preserves_revision() {
    let mut host = host_with_table(json!({"patterns": [], "replies": []}));
    let pane = add_agent_pane(&mut host, "claude");
    host.settle();
    assert!(filter_ids(&host, pane).is_empty());
    assert_eq!(
        host.kernel_settings().tab("text_triggers").expect("table")["patterns"],
        json!([])
    );
    host.kernel_settings()
        .set_tab("text_triggers", json!({"patterns": [], "replies": []}));
    host.settle();
    assert!(filter_ids(&host, pane).is_empty());
}

/// REQ: PORT:ilium-server/src/text_trigger_config.rs::text_trigger_invalid_partial_and_absent_sources_retain_last_valid_without_rewriting
#[test]
fn port_008_text_trigger_invalid_partial_and_absent_sources_retain_last_valid_without_rewriting() {
    let bad_sources = [
        json!("not a table"),
        json!({"replies": ["r"]}),
        json!({"patterns": 12, "replies": ["r"]}),
        json!({"patterns": ["["], "replies": ["r"]}),
        json!({"patterns": [""], "replies": ["r"]}),
        json!({"patterns": ["a"], "replies": ["one\ntwo"]}),
        json!({"patterns": ["a", "b"], "replies": ["r", "s"], "ids": ["x", "x"]}),
    ];
    for bad in bad_sources {
        let mut host = server_host();
        let pane = add_agent_pane(&mut host, "claude");
        set_rules(&mut host, &["good"], &["r"]);
        host.kernel_settings().set_tab("text_triggers", bad.clone());
        host.settle();
        assert_eq!(filter_regexes(&host, pane), ["good"], "{bad}");
        assert_eq!(
            host.kernel_settings().tab("text_triggers"),
            Some(bad),
            "an invalid source is not rewritten"
        );
    }
}

/// REQ: PORT:ilium-server/src/text_trigger_config.rs::text_trigger_revision_exhaustion_refuses_change_without_event
#[test]
fn port_009_text_trigger_revision_exhaustion_refuses_change_without_event() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["a"], &["r"]);
    let before = host.output_filters();
    for round in 0..64 {
        host.settings()
            .set("patterns", json!([format!("p{round}")]))
            .expect("accepted");
        host.settle();
    }
    assert_eq!(filter_ids(&host, pane), [rule_id(0)]);
    assert_eq!(host.output_filters().len(), before.len());
}

/// REQ: PORT:ilium-server/src/text_trigger_config.rs::regex_validation_uses_a_real_cpu_worker_and_acceptance_keeps_its_storage
#[test]
fn port_010_a_refused_pattern_leaves_the_installed_filters_untouched() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["go"], &["yes"]);
    assert_eq!(
        settings_error(&mut host, "patterns", json!(["(bad"])),
        ErrorCode::InvalidArgument
    );
    host.settle();
    assert_eq!(filter_regexes(&host, pane), ["go"]);
    assert_eq!(host.settings().get("patterns"), Some(json!(["go"])));
}
