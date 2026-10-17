//! Twins of the settings, state and argument tests of the voice tool executor
//! (`ilium-client/src/control/{mod,settings,parser_pool_settings_regressions}.rs`), written against
//! the realtime tool bridge. The settings tools are assumed to be `ilium_read_settings` and
//! `ilium_set_setting` with `{ "path", "value" }` (API_GAPS_ports.md).

use crate::host_ext::*;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ErrorCode, PluginError};
use serde_json::{json, Value};

use crate::common::{add_pane, configure, enabled_client};

fn set_setting(host: &mut FakeHost, path: &str, value: Value) -> Result<String, PluginError> {
    host.realtime().call_tool(
        "ilium_set_setting",
        &json!({ "path": path, "value": value }).to_string(),
    )
}

fn read_settings(host: &mut FakeHost) -> String {
    host.realtime()
        .call_tool("ilium_read_settings", "{}")
        .expect("settings are readable")
}

fn refused_code(result: Result<String, PluginError>) -> ErrorCode {
    result.expect_err("the change is refused").code
}

/// REQ: PORT:ilium-client/src/control/mod.rs::control_argument_overflow_is_truthfully_rejected_before_preparation
#[test]
fn port_009_an_argument_over_one_mebibyte_is_refused_before_anything_runs() {
    let mut host = enabled_client("semantic_vad");
    add_pane(&mut host, "docs");
    let text = "a".repeat(1024 * 1024 + 1);
    let result = host
        .realtime()
        .call_tool("ilium_type_text", &json!({ "text": text }).to_string());
    assert_eq!(refused_code(result), ErrorCode::InvalidArgument);
    assert_eq!(host.realtime().typed_text(), Vec::<String>::new());
}

/// REQ: PORT:ilium-client/src/control/mod.rs::git_settings_control_paths_persist_and_read_back
#[test]
fn port_010_git_settings_are_accepted_and_read_back() {
    let mut host = enabled_client("semantic_vad");
    set_setting(&mut host, "git.branch_prefix", json!("review/")).expect("prefix accepted");
    set_setting(&mut host, "git.branch_line", json!("off")).expect("line accepted");
    let settings: Value =
        serde_json::from_str(&read_settings(&mut host)).expect("settings are JSON");
    assert_eq!(settings["git"]["branch_prefix"], json!("review/"));
    assert_eq!(settings["git"]["branch_line"], json!("off"));
}

/// REQ: PORT:ilium-client/src/control/mod.rs::state_preparation_is_deferred_ordered_and_shares_its_retained_json
#[test]
fn port_012_get_state_with_a_repeated_call_id_returns_the_same_result_and_runs_once() {
    let mut host = enabled_client("semantic_vad");
    add_pane(&mut host, "docs");
    let first = host
        .realtime()
        .call_tool_with_id("state-1", "ilium_get_state", "{}")
        .expect("first state");
    let second = host
        .realtime()
        .call_tool_with_id("state-1", "ilium_get_state", "{}")
        .expect("second state");
    assert_eq!(first, second);
    assert_eq!(host.realtime().executions("state-1"), 1);
}

/// REQ: PORT:ilium-client/src/control/mod.rs::voice_replacement_cancels_old_reads_and_fences_cached_call_ids
#[test]
fn port_013_a_new_voice_instance_does_not_reuse_the_cached_result_of_a_call_id() {
    let mut host = enabled_client("semantic_vad");
    add_pane(&mut host, "docs");
    let before = host
        .realtime()
        .call_tool_with_id("state-1", "ilium_get_state", "{}")
        .expect("state before");
    configure(&mut host, "enabled", json!(false));
    configure(&mut host, "enabled", json!(true));
    add_pane(&mut host, "api");
    let after = host
        .realtime()
        .call_tool_with_id("state-1", "ilium_get_state", "{}")
        .expect("state after");
    assert_ne!(before, after);
    assert_eq!(host.realtime().executions("state-1"), 1);
}

/// REQ: PORT:ilium-client/src/control/parser_pool_settings_regressions.rs::parser_pool_execute_rejects_invalid_values_without_reporting_a_save
#[test]
fn port_014_engine_memory_budget_rejects_invalid_values_and_keeps_the_old_one() {
    let mut host = enabled_client("semantic_vad");
    set_setting(&mut host, "terminal.engine_memory_budget_mib", json!(512)).expect("seed");
    let invalid = [
        json!(-1),
        json!(1),
        json!(255),
        json!(16385),
        json!(4294967295u64),
        json!(4294967296u64),
        json!(1.5),
    ];
    for value in invalid {
        let code = refused_code(set_setting(
            &mut host,
            "terminal.engine_memory_budget_mib",
            value.clone(),
        ));
        assert_eq!(code, ErrorCode::InvalidArgument, "{value}");
        let settings: Value =
            serde_json::from_str(&read_settings(&mut host)).expect("settings are JSON");
        assert_eq!(
            settings["terminal"]["engine_memory_budget_mib"],
            json!(512),
            "{value}"
        );
    }
}

/// REQ: PORT:ilium-client/src/control/parser_pool_settings_regressions.rs::parser_pool_execute_reports_pending_then_reloads_every_valid_boundary_and_custom_value
#[test]
fn port_016_engine_memory_budget_accepts_zero_and_the_whole_valid_range() {
    let mut host = enabled_client("semantic_vad");
    for value in [0, 256, 257, 16384] {
        set_setting(&mut host, "terminal.engine_memory_budget_mib", json!(value))
            .expect("accepted");
        let settings: Value =
            serde_json::from_str(&read_settings(&mut host)).expect("settings are JSON");
        assert_eq!(
            settings["terminal"]["engine_memory_budget_mib"],
            json!(value)
        );
    }
}

/// REQ: PORT:ilium-client/src/control/settings.rs::kilo_model_setting_accepts_only_discovered_free_choices
#[test]
fn port_019_a_paid_kilo_model_is_refused() {
    let mut host = enabled_client("semantic_vad");
    let code = refused_code(set_setting(
        &mut host,
        "inference.kilo_gateway.model",
        json!("provider/paid"),
    ));
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: PORT:ilium-client/src/control/settings.rs::left_panel_widths_reject_invalid_values_without_mutating_state
#[test]
fn port_020_left_panel_widths_refuse_invalid_values_and_accept_a_valid_one() {
    let mut host = enabled_client("semantic_vad");
    assert_eq!(
        refused_code(set_setting(
            &mut host,
            "ui.left_panel_fixed_width",
            json!(5)
        )),
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        refused_code(set_setting(
            &mut host,
            "ui.left_panel_focused_width",
            json!(2147483648u64)
        )),
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        refused_code(set_setting(
            &mut host,
            "ui.left_panel_focused_width",
            json!(20)
        )),
        ErrorCode::InvalidArgument
    );
    set_setting(&mut host, "ui.left_panel_fixed_width", json!(40)).expect("40 is valid");
    let settings: Value =
        serde_json::from_str(&read_settings(&mut host)).expect("settings are JSON");
    assert_eq!(settings["ui"]["left_panel_fixed_width"], json!(40));
}

/// REQ: PORT:ilium-client/src/control/settings.rs::preview_control_reports_actual_refusal_instead_of_queued
#[test]
fn port_021_previewing_a_muted_sound_reports_that_it_is_muted() {
    let mut host = enabled_client("semantic_vad");
    host.kernel_settings()
        .set("sound", "source", json!("muted"))
        .expect("muted accepted");
    let error = host
        .realtime()
        .call_tool("ilium_preview_sound", "{}")
        .expect_err("preview refused");
    assert_eq!(error.message, "Sound is muted");
}

/// REQ: PORT:ilium-client/src/control/settings.rs::reset_planning_control_paths_toggle_independently
#[test]
fn port_022_the_two_reset_planning_monitors_toggle_independently() {
    let mut host = enabled_client("semantic_vad");
    set_setting(&mut host, "reset_planning.monitor_claude", json!(false)).expect("claude off");
    set_setting(&mut host, "reset_planning.time_style", json!("human")).expect("human accepted");
    assert_eq!(
        refused_code(set_setting(
            &mut host,
            "reset_planning.time_style",
            json!("soon")
        )),
        ErrorCode::InvalidArgument
    );
    let settings: Value =
        serde_json::from_str(&read_settings(&mut host)).expect("settings are JSON");
    assert_eq!(settings["reset_planning"]["monitor_claude"], json!(false));
    assert_eq!(settings["reset_planning"]["monitor_codex"], json!(true));
    assert_eq!(settings["reset_planning"]["time_style"], json!("human"));
}

/// REQ: PORT:ilium-client/src/control/settings.rs::restructure_token_setting_validates_and_updates_snapshot
#[test]
fn port_023_the_restructure_token_limit_takes_only_positive_whole_numbers() {
    let mut host = enabled_client("semantic_vad");
    set_setting(
        &mut host,
        "inference.restructure_prompt_token_limit",
        json!(345678),
    )
    .expect("accepted");
    for value in [
        json!(0),
        json!(-1),
        json!(1.5),
        json!("123"),
        json!(4294967296u64),
    ] {
        let code = refused_code(set_setting(
            &mut host,
            "inference.restructure_prompt_token_limit",
            value.clone(),
        ));
        assert_eq!(code, ErrorCode::InvalidArgument, "{value}");
    }
    let settings: Value =
        serde_json::from_str(&read_settings(&mut host)).expect("settings are JSON");
    assert_eq!(
        settings["inference"]["restructure_prompt_token_limit"],
        json!(345678)
    );
}

/// REQ: PORT:ilium-client/src/control/settings.rs::scrollback_budget_rejects_rather_than_hangs_when_misaligned
#[test]
fn port_024_the_scrollback_budget_takes_aligned_values_and_refuses_a_misaligned_one() {
    let mut host = enabled_client("semantic_vad");
    set_setting(&mut host, "terminal.scrollback_budget_mib", json!(32)).expect("32 is aligned");
    set_setting(&mut host, "terminal.scrollback_budget_mib", json!(64)).expect("64 is aligned");
    assert_eq!(
        refused_code(set_setting(
            &mut host,
            "terminal.scrollback_budget_mib",
            json!(5)
        )),
        ErrorCode::InvalidArgument
    );
    let settings: Value =
        serde_json::from_str(&read_settings(&mut host)).expect("settings are JSON");
    assert_eq!(settings["terminal"]["scrollback_budget_mib"], json!(64));
}

/// REQ: PORT:ilium-client/src/control/snapshot.rs::settings_snapshot_exposes_title_style_as_a_writable_choice
#[test]
fn port_026_the_title_style_can_be_set_and_is_read_back() {
    let mut host = enabled_client("semantic_vad");
    set_setting(&mut host, "inference.title_style", json!("labeling")).expect("labeling accepted");
    let settings: Value =
        serde_json::from_str(&read_settings(&mut host)).expect("settings are JSON");
    assert_eq!(settings["inference"]["title_style"], json!("labeling"));
}
