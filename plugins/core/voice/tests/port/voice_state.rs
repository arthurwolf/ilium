//! Twins of the voice session tests of `ilium-client/src/{lib,normal_voice}.rs` and the sound studio,
//! written against the realtime tool bridge and the `voice-control` kernel tab.

use crate::host_ext::*;
use ilium_plugin_sdk::types::ErrorCode;
use serde_json::json;

use crate::common::{add_pane, configure, enabled_client};

/// REQ: PORT:ilium-client/src/lib.rs::realtime_context_update_is_emitted_once_per_agent_boundary_change
#[test]
fn port_034_the_agent_default_dictation_rule_is_stated_for_a_detected_agent_and_not_repeated() {
    let mut host = enabled_client("semantic_vad");
    add_pane(&mut host, "docs");
    host.realtime().set_active_agent("claude");
    let first = host
        .realtime()
        .target_context()
        .expect("a context for the detected agent");
    assert!(
        first.contains("agent-default dictation rule is active"),
        "{first}"
    );
    assert!(
        first.contains("user's complete utterance as `text`"),
        "{first}"
    );
    host.realtime().set_active_agent("claude");
    assert_eq!(host.realtime().target_context(), Some(first));
}

/// REQ: PORT:ilium-client/src/normal_voice.rs::blocked_actor_fifo_preserves_order_originals_and_dated_retry
#[test]
fn port_040_tool_calls_run_in_the_order_they_were_made() {
    let mut host = enabled_client("semantic_vad");
    add_pane(&mut host, "docs");
    for text in ["first", "second", "third"] {
        host.realtime()
            .call_tool("ilium_type_text", &json!({ "text": text }).to_string())
            .expect("typed");
    }
    assert_eq!(host.realtime().typed_text(), ["first", "second", "third"]);
}

/// REQ: PORT:ilium-client/src/normal_voice.rs::borrowed_startup_bound_covers_actual_context_layout_and_maximum_prompt
#[test]
fn port_041_the_maximum_custom_prompt_is_carried_into_the_system_prompt() {
    let mut host = enabled_client("semantic_vad");
    let prompt = "p".repeat(64 * 1024);
    configure(&mut host, "custom_prompt", json!(prompt));
    assert!(host.realtime().system_prompt().contains(&prompt));
}

/// REQ: PORT:ilium-client/src/normal_voice.rs::replacement_cannot_inherit_originals_and_metadata_outlives_cancellation_receipt
#[test]
fn port_042_a_replaced_voice_session_starts_without_the_old_typed_text() {
    let mut host = enabled_client("semantic_vad");
    add_pane(&mut host, "docs");
    host.realtime()
        .call_tool("ilium_type_text", &json!({ "text": "old" }).to_string())
        .expect("typed");
    configure(&mut host, "enabled", json!(false));
    configure(&mut host, "enabled", json!(true));
    assert_eq!(host.realtime().tool_calls(), Vec::<String>::new());
}

/// REQ: PORT:ilium-client/src/onboarding/studio.rs::every_control_can_reach_its_entire_safe_range
#[test]
fn port_046_the_output_volume_reaches_both_ends_of_its_range_and_no_further() {
    let mut host = enabled_client("semantic_vad");
    for value in [0, 100] {
        configure(&mut host, "output_volume_percent", json!(value));
        assert_eq!(
            host.kernel_settings()
                .get("voice-control", "output_volume_percent"),
            Some(json!(value))
        );
    }
    let above = host
        .kernel_settings()
        .set("voice-control", "output_volume_percent", json!(101))
        .expect_err("101 refused");
    assert_eq!(above.code, ErrorCode::InvalidArgument);
    assert_eq!(
        host.kernel_settings()
            .get("voice-control", "output_volume_percent"),
        Some(json!(100))
    );
}

/// REQ: PORT:ilium-client/src/lib.rs::realtime_context_update_is_emitted_once_per_agent_boundary_change
#[test]
fn port_035_switching_to_another_detected_agent_is_a_new_context() {
    let mut host = enabled_client("semantic_vad");
    add_pane(&mut host, "docs");
    host.realtime().set_active_agent("claude");
    let first = host
        .realtime()
        .target_context()
        .expect("a context for the first agent");
    host.realtime().set_active_agent("codex");
    let second = host
        .realtime()
        .target_context()
        .expect("a context for the second agent");
    assert_ne!(second, first);
}

/// REQ: PORT:ilium-client/src/normal_voice.rs::blocked_actor_fifo_preserves_order_originals_and_dated_retry
#[test]
fn port_043_each_tool_call_is_executed_exactly_once_in_order() {
    let mut host = enabled_client("semantic_vad");
    add_pane(&mut host, "docs");
    for text in ["one", "two"] {
        host.realtime()
            .call_tool("ilium_type_text", &json!({ "text": text }).to_string())
            .expect("typed");
    }
    assert_eq!(host.realtime().executions("ilium_type_text"), 2);
}

/// REQ: PORT:ilium-client/src/normal_voice.rs::borrowed_startup_bound_covers_actual_context_layout_and_maximum_prompt
#[test]
fn port_044_the_tools_stay_offered_with_the_maximum_custom_prompt() {
    let mut host = enabled_client("semantic_vad");
    let prompt = "x".repeat(64 * 1024);
    configure(&mut host, "custom_prompt", json!(prompt));
    assert!(!host.realtime().tools().is_empty());
}

/// REQ: PORT:ilium-client/src/normal_voice.rs::replacement_cannot_inherit_originals_and_metadata_outlives_cancellation_receipt
#[test]
fn port_045_a_restarted_session_does_not_re_execute_an_earlier_tool_call() {
    let mut host = enabled_client("semantic_vad");
    add_pane(&mut host, "docs");
    host.realtime()
        .call_tool("ilium_type_text", &json!({ "text": "once" }).to_string())
        .expect("typed");
    configure(&mut host, "enabled", json!(false));
    configure(&mut host, "enabled", json!(true));
    assert_eq!(host.realtime().executions("ilium_type_text"), 0);
}

/// REQ: PORT:ilium-client/src/onboarding/studio.rs::every_control_can_reach_its_entire_safe_range
#[test]
fn port_047_a_volume_below_zero_is_refused_and_the_stored_value_stays() {
    let mut host = enabled_client("semantic_vad");
    configure(&mut host, "output_volume_percent", json!(40));
    let below = host
        .kernel_settings()
        .set("voice-control", "output_volume_percent", json!(-1))
        .expect_err("-1 refused");
    assert_eq!(below.code, ErrorCode::InvalidArgument);
    assert_eq!(
        host.kernel_settings()
            .get("voice-control", "output_volume_percent"),
        Some(json!(40))
    );
}
