//! Tests of the pure Text Trigger rules, driven through the plugin's own modules. Each one carries
//! over a kernel test of `ilium-ipc/src/text_trigger.rs`, `ilium-server/src/text_trigger_config.rs` or
//! `ilium-server/src/text_triggers.rs`.

use ilium_plugin_core_text_triggers::config::{
    parse_table, validate, validate_identities, Rule, MAX_DELAY_SECONDS,
};
use ilium_plugin_core_text_triggers::matching::{PaneKind, Tracker, SETTLE_WINDOW_MS};
use serde_json::json;

fn rule(id: &str, regexp: &str) -> Rule {
    Rule {
        id: id.to_string(),
        regexp: regexp.to_string(),
        message: "continue".to_string(),
        ..Rule::default()
    }
}

fn lines(text: &[&str]) -> Vec<String> {
    text.iter().map(|line| line.to_string()).collect()
}

/// REQ: F-056
#[test]
fn a_rule_stored_before_the_delay_setting_loads_with_sixty_seconds() {
    let document = json!({"text_triggers": {"triggers": [
        {"id": "old", "enabled": true, "regexp": "ready$", "message": "go", "target": "both", "sample_text": ""},
        {"id": "new", "regexp": "x", "message": "y", "delay_seconds": 5}
    ]}});
    let rules = parse_table(&document).expect("parses").expect("present");
    assert_eq!(rules[0].delay_seconds, 60);
    assert_eq!(rules[1].delay_seconds, 5);
}

/// REQ: F-056
#[test]
fn invalid_partial_and_absent_sources_are_rejected_or_unspecified() {
    // Partial or malformed tables are errors (the last valid list stays active).
    assert!(parse_table(&json!({"text_triggers": {}})).is_err());
    assert!(parse_table(&json!({"text_triggers": {"triggers": 12}})).is_err());
    // An unrelated document leaves the list unspecified.
    assert_eq!(
        parse_table(&json!({"sound": {"source": "system_beep"}})),
        Ok(None)
    );
    // Each invalid candidate is refused by validation.
    let mut bad_regex = rule("a", "[");
    bad_regex.message = "x".into();
    let bad_id = rule("", "ready$");
    let mut multiline = rule("a", "ready$");
    multiline.message = "one\ntwo".into();
    for candidate in [vec![bad_regex], vec![bad_id], vec![multiline]] {
        assert!(validate(&candidate).is_some());
    }
    assert!(validate(&[rule("a", "ready$"), rule("a", "ready$")]).is_some());
}

/// REQ: F-056
#[test]
fn an_explicit_empty_list_clears_and_is_valid() {
    assert_eq!(
        parse_table(&json!({"text_triggers": {"triggers": []}})),
        Ok(Some(Vec::new()))
    );
    assert_eq!(validate(&[]), None);
}

/// REQ: F-056
#[test]
fn identity_validation_rejects_missing_blank_control_and_duplicate_ids() {
    for id in ["", "   ", "rule\nname", "\t"] {
        assert!(validate_identities(&[rule(id, "x")]).is_err());
    }
    assert_eq!(validate_identities(&[rule("authored-rule", "x")]), Ok(()));
    assert!(validate_identities(&[rule("stable", "x"), rule("stable", "x")]).is_err());
}

/// REQ: F-056
#[test]
fn validation_rejects_delays_beyond_the_maximum() {
    let mut late = rule("r", "x");
    late.delay_seconds = MAX_DELAY_SECONDS + 1;
    assert!(validate(&[late]).is_some());
    let mut edge = rule("r", "x");
    edge.delay_seconds = MAX_DELAY_SECONDS;
    assert_eq!(validate(&[edge]), None);
}

/// REQ: F-056
#[test]
fn a_clear_and_full_redraw_within_the_settle_window_does_not_refire() {
    let rules = [rule("r", "ready$")];
    let mut tracker = Tracker::default();
    let scan = |tracker: &mut Tracker, text: &[&str], now: u64| {
        tracker.evaluate(&rules, &lines(text), false, PaneKind::PlainShell, now)
    };
    scan(&mut tracker, &[], 0);
    assert_eq!(scan(&mut tracker, &["ready"], 10), ["r"]);
    assert!(scan(&mut tracker, &[], 20).is_empty());
    assert!(scan(&mut tracker, &["ready"], 20 + SETTLE_WINDOW_MS - 1).is_empty());
}

/// REQ: F-056
#[test]
fn a_match_missing_for_the_settle_window_fires_again_when_it_returns() {
    let rules = [rule("r", "ready$")];
    let mut tracker = Tracker::default();
    let scan = |tracker: &mut Tracker, text: &[&str], now: u64| {
        tracker.evaluate(&rules, &lines(text), false, PaneKind::PlainShell, now)
    };
    scan(&mut tracker, &[], 0);
    assert_eq!(scan(&mut tracker, &["ready"], 10), ["r"]);
    scan(&mut tracker, &[], 20);
    assert_eq!(
        scan(&mut tracker, &["ready"], 20 + SETTLE_WINDOW_MS + 50),
        ["r"]
    );
}
