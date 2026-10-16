//! The `[remote_compaction]` table: defaults, ranges, step ladders and persistence keys, unchanged from the built-in tab (COR-011, COR-033, F-080).

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ActionKind, ActionValue, ViewTarget};
use proptest::prelude::*;
use serde_json::{json, Value};

use crate::common::*;

fn key(name: &str) -> String {
    format!("remote_compaction.{name}")
}

fn read(host: &mut FakeHost, name: &str) -> Option<Value> {
    host.settings().get(&key(name))
}

fn tab() -> ViewTarget {
    ViewTarget::SettingsPage("remote-compaction".to_string())
}

fn step(host: &mut FakeHost, name: &str, direction: f64) {
    host.send_action(tab(), name, ActionKind::Changed, ActionValue::Number(direction));
}

fn set_then_read(name: &str, value: Value) -> Option<Value> {
    let mut host = client_host();
    host.settings().set(&key(name), value).expect("setting accepted");
    read(&mut host, name)
}

/// REQ: COR-011, F-080
#[test]
fn settings_011_defaults_are_off_and_65_percent() {
    let mut host = client_host();
    assert_eq!(read(&mut host, "enabled"), Some(json!(false)));
    assert_eq!(read(&mut host, "automatic"), Some(json!(false)));
    assert_eq!(read(&mut host, "threshold_percent"), Some(json!(65)));
}

/// REQ: COR-011, F-080
#[test]
fn settings_011_default_techniques_are_claude_code_codex_and_claude_code_for_other() {
    let mut host = client_host();
    assert_eq!(read(&mut host, "claude_technique"), Some(json!("claude_code")));
    assert_eq!(read(&mut host, "codex_technique"), Some(json!("codex")));
    assert_eq!(read(&mut host, "other_technique"), Some(json!("claude_code")));
    assert_eq!(read(&mut host, "custom_prompt"), Some(json!("")));
}

/// REQ: COR-011, F-080
#[test]
fn settings_011_default_budgets_match_the_built_in_tab() {
    let mut host = client_host();
    assert_eq!(read(&mut host, "tail_tokens"), Some(json!(20_000)));
    assert_eq!(read(&mut host, "protected_recent_tool_tokens"), Some(json!(40_000)));
    assert_eq!(read(&mut host, "tool_result_chars"), Some(json!(2_000)));
    assert_eq!(read(&mut host, "summarizer_context_tokens"), Some(json!(120_000)));
}

/// REQ: COR-011, F-080
#[test]
fn settings_011_default_timing_redaction_backups_and_banner() {
    let mut host = client_host();
    assert_eq!(read(&mut host, "redact_secrets"), Some(json!(true)));
    assert_eq!(read(&mut host, "pause_timeout_seconds"), Some(json!(120)));
    assert_eq!(read(&mut host, "cooldown_minutes"), Some(json!(10)));
    assert_eq!(read(&mut host, "keep_backups"), Some(json!(3)));
    assert_eq!(read(&mut host, "privacy_banner_dismissed"), Some(json!(false)));
}

/// REQ: COR-011, COR-033
#[test]
fn settings_033_values_persist_under_the_remote_compaction_table_keys() {
    let mut host = client_host();
    host.settings().set(&key("enabled"), json!(true)).expect("setting accepted");
    assert_eq!(read(&mut host, "enabled"), Some(json!(true)));
    assert!(host.settings().get("enabled").is_none(), "no key outside the [remote_compaction] table");
}

/// REQ: COR-011
#[test]
fn settings_011_threshold_below_30_is_raised_to_30() {
    assert_eq!(set_then_read("threshold_percent", json!(29)), Some(json!(30)));
    assert_eq!(set_then_read("threshold_percent", json!(30)), Some(json!(30)));
    assert_eq!(set_then_read("threshold_percent", json!(31)), Some(json!(31)));
}

/// REQ: COR-011
#[test]
fn settings_011_threshold_above_88_is_lowered_to_88() {
    assert_eq!(set_then_read("threshold_percent", json!(87)), Some(json!(87)));
    assert_eq!(set_then_read("threshold_percent", json!(88)), Some(json!(88)));
    assert_eq!(set_then_read("threshold_percent", json!(89)), Some(json!(88)));
}

/// REQ: COR-011
#[test]
fn settings_011_tail_tokens_are_kept_between_2000_and_100000() {
    assert_eq!(set_then_read("tail_tokens", json!(1_999)), Some(json!(2_000)));
    assert_eq!(set_then_read("tail_tokens", json!(2_000)), Some(json!(2_000)));
    assert_eq!(set_then_read("tail_tokens", json!(100_000)), Some(json!(100_000)));
    assert_eq!(set_then_read("tail_tokens", json!(100_001)), Some(json!(100_000)));
}

/// REQ: COR-011
#[test]
fn settings_011_protected_tool_tokens_are_kept_between_0_and_200000() {
    assert_eq!(set_then_read("protected_recent_tool_tokens", json!(0)), Some(json!(0)));
    assert_eq!(set_then_read("protected_recent_tool_tokens", json!(200_000)), Some(json!(200_000)));
    assert_eq!(set_then_read("protected_recent_tool_tokens", json!(200_001)), Some(json!(200_000)));
}

/// REQ: COR-011
#[test]
fn settings_011_tool_result_chars_are_kept_between_200_and_20000() {
    assert_eq!(set_then_read("tool_result_chars", json!(199)), Some(json!(200)));
    assert_eq!(set_then_read("tool_result_chars", json!(200)), Some(json!(200)));
    assert_eq!(set_then_read("tool_result_chars", json!(20_000)), Some(json!(20_000)));
    assert_eq!(set_then_read("tool_result_chars", json!(20_001)), Some(json!(20_000)));
}

/// REQ: COR-011
#[test]
fn settings_011_summarizer_context_is_kept_between_8000_and_900000() {
    assert_eq!(set_then_read("summarizer_context_tokens", json!(7_999)), Some(json!(8_000)));
    assert_eq!(set_then_read("summarizer_context_tokens", json!(900_000)), Some(json!(900_000)));
    assert_eq!(set_then_read("summarizer_context_tokens", json!(900_001)), Some(json!(900_000)));
}

/// REQ: COR-011
#[test]
fn settings_011_pause_timeout_is_kept_between_10_and_1800_seconds() {
    assert_eq!(set_then_read("pause_timeout_seconds", json!(9)), Some(json!(10)));
    assert_eq!(set_then_read("pause_timeout_seconds", json!(10)), Some(json!(10)));
    assert_eq!(set_then_read("pause_timeout_seconds", json!(1_800)), Some(json!(1_800)));
    assert_eq!(set_then_read("pause_timeout_seconds", json!(1_801)), Some(json!(1_800)));
}

/// REQ: COR-011
#[test]
fn settings_011_cooldown_is_kept_between_1_and_240_minutes() {
    assert_eq!(set_then_read("cooldown_minutes", json!(0)), Some(json!(1)));
    assert_eq!(set_then_read("cooldown_minutes", json!(1)), Some(json!(1)));
    assert_eq!(set_then_read("cooldown_minutes", json!(240)), Some(json!(240)));
    assert_eq!(set_then_read("cooldown_minutes", json!(241)), Some(json!(240)));
}

/// REQ: COR-011
#[test]
fn settings_011_keep_backups_is_kept_between_1_and_20() {
    assert_eq!(set_then_read("keep_backups", json!(0)), Some(json!(1)));
    assert_eq!(set_then_read("keep_backups", json!(20)), Some(json!(20)));
    assert_eq!(set_then_read("keep_backups", json!(21)), Some(json!(20)));
}

/// REQ: COR-011
#[test]
fn settings_011_custom_prompt_is_cut_at_20000_characters() {
    let at_limit = "x".repeat(20_000);
    assert_eq!(set_then_read("custom_prompt", json!(at_limit.clone())), Some(json!(at_limit)));
    let over = "x".repeat(20_001);
    assert_eq!(set_then_read("custom_prompt", json!(over)), Some(json!("x".repeat(20_000))));
}

/// REQ: COR-011
#[test]
fn settings_011_custom_prompt_is_also_capped_by_its_escaped_json_size_of_56000_bytes() {
    // 30 000 newlines escape to two bytes each (60 000 bytes), so only 28 000 fit.
    let prompt = "\n".repeat(30_000);
    let stored = set_then_read("custom_prompt", json!(prompt)).expect("stored");
    assert_eq!(stored.as_str().expect("text").len(), 28_000);
}

/// REQ: COR-011
#[test]
fn settings_011_threshold_steps_by_one_and_stops_at_the_range_ends() {
    let mut host = client_host();
    step(&mut host, "threshold_percent", 1.0);
    assert_eq!(read(&mut host, "threshold_percent"), Some(json!(66)));
    step(&mut host, "threshold_percent", -1.0);
    step(&mut host, "threshold_percent", -1.0);
    assert_eq!(read(&mut host, "threshold_percent"), Some(json!(64)));
    host.settings().set(&key("threshold_percent"), json!(30)).expect("setting accepted");
    step(&mut host, "threshold_percent", -1.0);
    assert_eq!(read(&mut host, "threshold_percent"), Some(json!(30)));
    host.settings().set(&key("threshold_percent"), json!(88)).expect("setting accepted");
    step(&mut host, "threshold_percent", 1.0);
    assert_eq!(read(&mut host, "threshold_percent"), Some(json!(88)));
}

/// REQ: COR-011
#[test]
fn settings_011_pause_timeout_walks_its_twelve_step_ladder() {
    let ladder = [10, 20, 30, 60, 90, 120, 180, 300, 600, 900, 1_200, 1_800];
    let mut host = client_host();
    host.settings().set(&key("pause_timeout_seconds"), json!(10)).expect("setting accepted");
    for expected in &ladder[1..] {
        step(&mut host, "pause_timeout_seconds", 1.0);
        assert_eq!(read(&mut host, "pause_timeout_seconds"), Some(json!(expected)));
    }
    step(&mut host, "pause_timeout_seconds", 1.0);
    assert_eq!(read(&mut host, "pause_timeout_seconds"), Some(json!(1_800)));
}

/// REQ: COR-011
#[test]
fn settings_011_cooldown_walks_its_thirteen_step_ladder_downwards() {
    let ladder = [1, 2, 5, 10, 15, 20, 30, 45, 60, 90, 120, 180, 240];
    let mut host = client_host();
    host.settings().set(&key("cooldown_minutes"), json!(240)).expect("setting accepted");
    for expected in ladder.iter().rev().skip(1) {
        step(&mut host, "cooldown_minutes", -1.0);
        assert_eq!(read(&mut host, "cooldown_minutes"), Some(json!(expected)));
    }
    step(&mut host, "cooldown_minutes", -1.0);
    assert_eq!(read(&mut host, "cooldown_minutes"), Some(json!(1)));
}

/// REQ: COR-011
#[test]
fn settings_011_a_value_off_the_ladder_snaps_to_the_next_one_in_the_direction() {
    let mut host = client_host();
    host.settings().set(&key("cooldown_minutes"), json!(12)).expect("setting accepted");
    step(&mut host, "cooldown_minutes", 1.0);
    assert_eq!(read(&mut host, "cooldown_minutes"), Some(json!(15)));
    host.settings().set(&key("cooldown_minutes"), json!(12)).expect("setting accepted");
    step(&mut host, "cooldown_minutes", -1.0);
    assert_eq!(read(&mut host, "cooldown_minutes"), Some(json!(10)));
}

/// REQ: COR-011
#[test]
fn settings_011_tail_tokens_ladder_has_ten_steps() {
    let ladder = [2_000, 5_000, 10_000, 15_000, 20_000, 30_000, 40_000, 60_000, 80_000, 100_000];
    let mut host = client_host();
    host.settings().set(&key("tail_tokens"), json!(2_000)).expect("setting accepted");
    for expected in &ladder[1..] {
        step(&mut host, "tail_tokens", 1.0);
        assert_eq!(read(&mut host, "tail_tokens"), Some(json!(expected)));
    }
}

/// REQ: COR-011
#[test]
fn settings_011_protected_tool_tokens_ladder_has_ten_steps() {
    let ladder = [0, 10_000, 20_000, 30_000, 40_000, 60_000, 80_000, 100_000, 150_000, 200_000];
    let mut host = client_host();
    host.settings().set(&key("protected_recent_tool_tokens"), json!(0)).expect("setting accepted");
    for expected in &ladder[1..] {
        step(&mut host, "protected_recent_tool_tokens", 1.0);
        assert_eq!(read(&mut host, "protected_recent_tool_tokens"), Some(json!(expected)));
    }
}

/// REQ: COR-011
#[test]
fn settings_011_tool_result_chars_ladder_has_eight_steps() {
    let ladder = [200, 500, 1_000, 2_000, 3_000, 5_000, 10_000, 20_000];
    let mut host = client_host();
    host.settings().set(&key("tool_result_chars"), json!(200)).expect("setting accepted");
    for expected in &ladder[1..] {
        step(&mut host, "tool_result_chars", 1.0);
        assert_eq!(read(&mut host, "tool_result_chars"), Some(json!(expected)));
    }
}

/// REQ: COR-011
#[test]
fn settings_011_summarizer_context_ladder_has_ten_steps() {
    let ladder = [8_000, 16_000, 32_000, 64_000, 100_000, 120_000, 200_000, 400_000, 700_000, 900_000];
    let mut host = client_host();
    host.settings().set(&key("summarizer_context_tokens"), json!(8_000)).expect("setting accepted");
    for expected in &ladder[1..] {
        step(&mut host, "summarizer_context_tokens", 1.0);
        assert_eq!(read(&mut host, "summarizer_context_tokens"), Some(json!(expected)));
    }
}

/// REQ: COR-011
#[test]
fn settings_011_keep_backups_steps_by_one_and_stops_at_1_and_20() {
    let mut host = client_host();
    step(&mut host, "keep_backups", 1.0);
    assert_eq!(read(&mut host, "keep_backups"), Some(json!(4)));
    host.settings().set(&key("keep_backups"), json!(1)).expect("setting accepted");
    step(&mut host, "keep_backups", -1.0);
    assert_eq!(read(&mut host, "keep_backups"), Some(json!(1)));
    host.settings().set(&key("keep_backups"), json!(20)).expect("setting accepted");
    step(&mut host, "keep_backups", 1.0);
    assert_eq!(read(&mut host, "keep_backups"), Some(json!(20)));
}

/// REQ: COR-011, F-080
#[test]
fn settings_011_technique_rows_cycle_forward_through_all_six_and_wrap() {
    let order = ["claude_code", "codex", "opencode", "gemini_cli", "best_of_all_worlds", "custom"];
    let mut host = client_host();
    host.settings().set(&key("other_technique"), json!("claude_code")).expect("setting accepted");
    for expected in order.iter().skip(1).chain(std::iter::once(&order[0])) {
        step(&mut host, "other_technique", 1.0);
        assert_eq!(read(&mut host, "other_technique"), Some(json!(expected)));
    }
}

/// REQ: COR-011, F-080
#[test]
fn settings_011_technique_rows_cycle_backward_and_wrap_to_custom() {
    let mut host = client_host();
    step(&mut host, "claude_technique", -1.0);
    assert_eq!(read(&mut host, "claude_technique"), Some(json!("custom")));
}

/// REQ: COR-011
#[test]
fn settings_011_toggles_flip_on_activation() {
    let mut host = client_host();
    for name in ["enabled", "automatic", "redact_secrets"] {
        let before = read(&mut host, name).and_then(|value| value.as_bool()).expect("boolean");
        host.send_action(tab(), name, ActionKind::Toggled, ActionValue::Flag(!before));
        assert_eq!(read(&mut host, name), Some(json!(!before)), "{name}");
    }
}

/// REQ: COR-011
#[test]
fn settings_011_privacy_banner_closes_for_good_and_never_returns() {
    let mut host = client_host();
    host.send_action(tab(), "privacy_banner_dismissed", ActionKind::Closed, ActionValue::None);
    assert_eq!(read(&mut host, "privacy_banner_dismissed"), Some(json!(true)));
    // Closing again changes nothing, and no action reopens it.
    host.send_action(tab(), "privacy_banner_dismissed", ActionKind::Closed, ActionValue::None);
    assert_eq!(read(&mut host, "privacy_banner_dismissed"), Some(json!(true)));
}

proptest! {
    // REQ: COR-011
    #[test]
    fn settings_011_threshold_is_always_clamped_into_30_to_88(value in 0u32..1000) {
        let stored = set_then_read("threshold_percent", json!(value)).and_then(|v| v.as_u64()).expect("number");
        prop_assert_eq!(stored, u64::from(value.clamp(30, 88)));
    }

    // REQ: COR-011
    #[test]
    fn settings_011_keep_backups_is_always_clamped_into_1_to_20(value in 0u32..100) {
        let stored = set_then_read("keep_backups", json!(value)).and_then(|v| v.as_u64()).expect("number");
        prop_assert_eq!(stored, u64::from(value.clamp(1, 20)));
    }
}
