//! Tree and status decorations (F-038, F-039, F-005 secondary).

use ilium_plugin_sdk::types::Intent;
use serde_json::json;

use crate::common::*;

fn fed(model: &str, tokens: u32) -> (ilium_plugin_sdk::testing::FakeHost, u64, usize) {
    let mut host = activated_server();
    let pane = add_agent(&mut host, "agent", "claude");
    let before = host.submitted().len();
    feed_pane(&mut host, pane, model, tokens);
    (host, pane, before)
}

/// REQ: F-038, F-039
#[test]
fn decor_038_dollar_unit_uses_slot_cost_trailing() {
    let (host, pane, before) = fed(KNOWN_MODEL, 50_000);
    let spec = latest_cost_decoration(&host, before, pane).expect("decoration");
    assert_eq!(spec.slot, SLOT);
    assert!(spec.trailing);
}

/// REQ: F-038
#[test]
fn decor_038_dollar_text_has_two_decimals() {
    let (host, pane, before) = fed(KNOWN_MODEL, 50_000);
    let text = decoration_text(&latest_cost_decoration(&host, before, pane).expect("decoration"));
    let digits = text.trim_start_matches("~$");
    let digits = digits.trim_start_matches('$');
    assert_eq!(digits.split('.').nth(1).map(str::len), Some(2), "{text}");
}

/// REQ: F-039, F-005
#[test]
fn decor_039_sort_key_is_negative_cents_so_highest_cost_sorts_first() {
    let (host, pane, before) = fed(KNOWN_MODEL, 5_000_000);
    let spec = latest_cost_decoration(&host, before, pane).expect("decoration");
    assert!(spec.sort_key.expect("sort key") < 0);
}

/// REQ: F-039, F-005
#[test]
fn decor_039_more_tokens_give_a_smaller_sort_key() {
    let (host_low, pane_low, before_low) = fed(KNOWN_MODEL, 100_000);
    let (host_high, pane_high, before_high) = fed(KNOWN_MODEL, 10_000_000);
    let low = latest_cost_decoration(&host_low, before_low, pane_low).expect("low").sort_key.expect("key");
    let high = latest_cost_decoration(&host_high, before_high, pane_high).expect("high").sort_key.expect("key");
    assert!(high < low);
}

/// REQ: F-038
#[test]
fn decor_038_quota_unit_shows_percent_for_non_claude_panes() {
    let mut host = activated_server();
    host.settings().set("unit", json!("quota")).expect("valid");
    let pane = add_agent(&mut host, "codex", "codex");
    let before = host.submitted().len();
    feed_pane(&mut host, pane, "gpt-5", 100_000);
    let text = decoration_text(&latest_cost_decoration(&host, before, pane).expect("decoration"));
    assert!(text.ends_with('%'), "{text}");
}

/// REQ: F-038
#[test]
fn decor_038_quota_unit_gives_claude_panes_no_indicator() {
    let mut host = activated_server();
    host.settings().set("unit", json!("quota")).expect("valid");
    let pane = add_agent(&mut host, "claude", "claude");
    let before = host.submitted().len();
    feed_pane(&mut host, pane, KNOWN_MODEL, 100_000);
    let shown = latest_cost_decoration(&host, before, pane);
    assert!(shown.is_none() || decoration_text(&shown.expect("some")).is_empty());
}

/// REQ: F-038
#[test]
fn decor_038_display_without_tree_emits_no_tree_decoration() {
    let mut host = activated_server();
    host.settings().set("display", json!(["status"])).expect("valid");
    let pane = add_agent(&mut host, "claude", "claude");
    let before = host.submitted().len();
    feed_pane(&mut host, pane, KNOWN_MODEL, 100_000);
    let shown = latest_cost_decoration(&host, before, pane);
    assert!(shown.is_none());
}

/// REQ: F-038
#[test]
fn decor_038_changing_the_unit_re_emits_the_decoration() {
    let (mut host, pane, _) = fed(KNOWN_MODEL, 100_000);
    let seen = host.submitted().len();
    host.settings().set("unit", json!("quota")).expect("valid");
    host.settle();
    let after = host.submitted();
    assert!(after.len() > seen);
    assert!(after[seen..].iter().any(|s| matches!(&s.intent, Intent::SetDecoration(spec) if spec.node.0 == pane)
        || matches!(&s.intent, Intent::ClearDecoration(key) if key.node.0 == pane)));
}

/// REQ: F-038
#[test]
fn decor_038_decoration_targets_the_fed_pane_only() {
    let (mut host, pane, before) = fed(KNOWN_MODEL, 100_000);
    let other = add_agent(&mut host, "other", "claude");
    let all = cost_decorations(&host.submitted()[before..]);
    assert!(all.iter().all(|spec| spec.node.0 == pane && spec.node.0 != other));
}
