//! Twins of the inline tests in ilium-client/src/remote_compaction_native.rs.
//! Gap: `FakeHost::native_trigger_evaluate(target, limit, window, threshold_percent) -> Value`
//! and `FakeHost::native_trigger_detect(target) -> Value` (see ../API_GAPS_port.md).

use serde_json::{json, Value};

use crate::common::*;

const CLAUDE: &str = "claude-auto-compact-window";
const CODEX: &str = "codex-auto-compact-token-limit";

fn evaluate(target: &str, limit: Value, window: u64, threshold: u64) -> Value {
    server_host().native_trigger_evaluate(target, limit, window, threshold)
}

/// REQ: PORT:ilium-client/src/remote_compaction_native.rs::claude_default_fires_33k_below_the_window
#[test]
fn native_claude_default_fires_33k_below_the_window() {
    let result = evaluate(CLAUDE, json!("Default"), 200_000, 65);
    assert_eq!(result["default_trigger_tokens"], 167_000);
    assert_eq!(result["effective_trigger_tokens"], 167_000);
    assert_eq!(result["is_remote_shadowed"], false);
}

/// REQ: PORT:ilium-client/src/remote_compaction_native.rs::codex_default_is_ninety_percent
#[test]
fn native_codex_default_is_ninety_percent() {
    let result = evaluate(CODEX, json!("Default"), 272_000, 65);
    assert_eq!(result["default_trigger_tokens"], 244_800);
    assert_eq!(result["is_remote_shadowed"], false);
}

/// REQ: PORT:ilium-client/src/remote_compaction_native.rs::a_low_setting_shadows_the_remote_threshold
#[test]
fn native_a_low_setting_shadows_the_remote_threshold() {
    let result = evaluate(CLAUDE, json!({"Setting": 130_000}), 200_000, 65);
    assert_eq!(result["effective_trigger_tokens"], 97_000);
    assert_eq!(result["is_remote_shadowed"], true);
    assert_eq!(result["highest_working_percent"], 47);
}

/// REQ: PORT:ilium-client/src/remote_compaction_native.rs::codex_limit_above_the_clamp_is_reduced_to_ninety_percent
#[test]
fn native_codex_limit_above_the_clamp_is_reduced_to_ninety_percent() {
    let result = evaluate(CODEX, json!({"Setting": 999_999}), 272_000, 65);
    assert_eq!(result["effective_trigger_tokens"], 244_800);
}

/// REQ: PORT:ilium-client/src/remote_compaction_native.rs::percent_override_and_disable_resolve
#[test]
fn native_percent_override_and_disable_resolve() {
    assert_eq!(
        evaluate(CLAUDE, json!({"PercentOfWindow": 50}), 200_000, 65)["is_remote_shadowed"],
        true
    );
    let off = evaluate(CLAUDE, json!("Disabled"), 200_000, 88);
    assert!(off["effective_trigger_tokens"].is_null());
    assert_eq!(off["is_remote_shadowed"], false);
    assert!(off["highest_working_percent"].is_null());
}

/// REQ: PORT:ilium-client/src/remote_compaction_native.rs::equal_triggers_count_as_shadowed
#[test]
fn native_equal_triggers_count_as_shadowed() {
    assert_eq!(
        evaluate(CODEX, json!({"Setting": 130_000}), 200_000, 65)["is_remote_shadowed"],
        true
    );
}

/// REQ: PORT:ilium-client/src/remote_compaction_native.rs::detect_prefers_environment_then_user_file
#[test]
fn native_detect_prefers_environment_then_user_file() {
    let mut host = server_host();
    write_text(&mut host, CLAUDE_FILE, "{\"autoCompactWindow\": 250000}");
    write_text(
        &mut host,
        CODEX_FILE,
        "model_auto_compact_token_limit = 200000\n",
    );
    let from_file = host.native_trigger_detect(CLAUDE);
    assert_eq!(from_file["limit"], json!({"Setting": 250_000}));
    assert!(from_file["source"].get("UserFile").is_some());
    assert_eq!(
        host.native_trigger_detect(CODEX)["limit"],
        json!({"Setting": 200_000})
    );
    host.set_env("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE", "40");
    assert_eq!(
        host.native_trigger_detect(CLAUDE)["limit"],
        json!({"PercentOfWindow": 40})
    );
    host.set_env("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE", "");
    host.set_env("DISABLE_AUTO_COMPACT", "1");
    assert_eq!(
        host.native_trigger_detect(CLAUDE)["limit"],
        json!("Disabled")
    );
}
