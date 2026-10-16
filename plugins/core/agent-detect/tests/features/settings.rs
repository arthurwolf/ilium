//! The settings of the Agent Monitoring kernel tab (`[ui]` and `[detection]` keys, same `config.toml` tables as
//! before): defaults, accepted values and the validation messages of the existing configuration loader.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::ErrorCode;
use serde_json::{Value, json};

use crate::common::client_host;

fn rejected(host: &mut FakeHost, key: &str, value: Value) -> String {
    let error = host.settings().set(key, value).expect_err("the value is refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    error.message
}

/// REQ: F-030
#[test]
fn feature_030_monitoring_mode_defaults_to_normal() {
    let mut host = client_host();
    assert_eq!(host.settings().get("ui.agent_monitoring_mode"), Some(json!("normal")));
}

/// REQ: F-030
#[test]
fn feature_030_monitoring_mode_accepts_normal_and_attention() {
    let mut host = client_host();
    host.settings().set("ui.agent_monitoring_mode", json!("attention")).expect("attention");
    assert_eq!(host.settings().get("ui.agent_monitoring_mode"), Some(json!("attention")));
    host.settings().set("ui.agent_monitoring_mode", json!("normal")).expect("normal");
    assert_eq!(host.settings().get("ui.agent_monitoring_mode"), Some(json!("normal")));
}

/// REQ: F-030
#[test]
fn feature_030_monitoring_mode_outside_the_closed_set_is_invalid_argument() {
    let mut host = client_host();
    let message = rejected(&mut host, "ui.agent_monitoring_mode", json!("loud"));
    assert!(message.contains("ui.agent_monitoring_mode = \"loud\" must be \"normal\" or \"attention\""), "{message}");
}

/// REQ: F-030
#[test]
fn feature_030_a_refused_monitoring_mode_keeps_the_previous_value() {
    let mut host = client_host();
    host.settings().set("ui.agent_monitoring_mode", json!("attention")).expect("attention");
    let _ = host.settings().set("ui.agent_monitoring_mode", json!("loud"));
    assert_eq!(host.settings().get("ui.agent_monitoring_mode"), Some(json!("attention")));
}

/// REQ: F-030
#[test]
fn feature_030_a_non_string_monitoring_mode_is_invalid_argument() {
    let mut host = client_host();
    rejected(&mut host, "ui.agent_monitoring_mode", json!(3));
}

/// REQ: F-034
#[test]
fn feature_034_identifier_mode_defaults_to_icon() {
    let mut host = client_host();
    assert_eq!(host.settings().get("ui.agent_identifier_mode"), Some(json!("icon")));
}

/// REQ: F-034
#[test]
fn feature_034_identifier_mode_accepts_the_four_modes() {
    let mut host = client_host();
    for mode in ["full_name", "letter", "icon", "hidden"] {
        host.settings().set("ui.agent_identifier_mode", json!(mode)).unwrap_or_else(|error| panic!("{mode}: {error:?}"));
        assert_eq!(host.settings().get("ui.agent_identifier_mode"), Some(json!(mode)));
    }
}

/// REQ: F-034
#[test]
fn feature_034_identifier_mode_is_matched_case_insensitively_after_trimming() {
    let mut host = client_host();
    host.settings().set("ui.agent_identifier_mode", json!(" Letter ")).expect("accepted");
}

/// REQ: F-034
#[test]
fn feature_034_identifier_mode_outside_the_closed_set_is_invalid_argument() {
    let mut host = client_host();
    let message = rejected(&mut host, "ui.agent_identifier_mode", json!("emoji_soup"));
    assert!(
        message.contains("ui.agent_identifier_mode = \"emoji_soup\" must be \"full_name\", \"letter\", \"icon\", or \"hidden\""),
        "{message}"
    );
}

/// REQ: F-034
#[test]
fn feature_034_claude_icon_defaults_to_crab() {
    let mut host = client_host();
    assert_eq!(host.settings().get("ui.claude_agent_icon"), Some(json!("crab")));
}

/// REQ: F-034
#[test]
fn feature_034_claude_icon_accepts_the_six_curated_names() {
    let mut host = client_host();
    for icon in ["brain", "magic_wand", "compass", "thread", "crab", "lobster"] {
        host.settings().set("ui.claude_agent_icon", json!(icon)).unwrap_or_else(|error| panic!("{icon}: {error:?}"));
    }
}

/// REQ: F-034
#[test]
fn feature_034_claude_icon_outside_the_curated_set_is_invalid_argument() {
    let mut host = client_host();
    let message = rejected(&mut host, "ui.claude_agent_icon", json!("robot"));
    assert!(message.contains("ui.claude_agent_icon = \"robot\" is not a supported Claude icon"), "{message}");
}

/// REQ: F-034
#[test]
fn feature_034_codex_icon_defaults_to_turtle() {
    let mut host = client_host();
    assert_eq!(host.settings().get("ui.codex_agent_icon"), Some(json!("turtle")));
}

/// REQ: F-034
#[test]
fn feature_034_codex_icon_accepts_the_six_curated_names() {
    let mut host = client_host();
    for icon in ["gear", "tools", "dna", "book", "cross_mark", "turtle"] {
        host.settings().set("ui.codex_agent_icon", json!(icon)).unwrap_or_else(|error| panic!("{icon}: {error:?}"));
    }
}

/// REQ: F-034
#[test]
fn feature_034_codex_icon_outside_the_curated_set_is_invalid_argument() {
    let mut host = client_host();
    let message = rejected(&mut host, "ui.codex_agent_icon", json!("crab"));
    assert!(message.contains("ui.codex_agent_icon = \"crab\" is not a supported Codex icon"), "{message}");
}

/// REQ: F-034
#[test]
fn feature_034_antigravity_icon_defaults_to_atom() {
    let mut host = client_host();
    assert_eq!(host.settings().get("ui.antigravity_agent_icon"), Some(json!("atom")));
}

/// REQ: F-034
#[test]
fn feature_034_antigravity_icon_accepts_the_four_curated_names() {
    let mut host = client_host();
    for icon in ["spark", "satellite", "orbit", "atom"] {
        host.settings().set("ui.antigravity_agent_icon", json!(icon)).unwrap_or_else(|error| panic!("{icon}: {error:?}"));
    }
}

/// REQ: F-034
#[test]
fn feature_034_antigravity_icon_outside_the_curated_set_is_invalid_argument() {
    let mut host = client_host();
    let message = rejected(&mut host, "ui.antigravity_agent_icon", json!("planet"));
    assert!(message.contains("ui.antigravity_agent_icon = \"planet\" is not a supported Antigravity icon"), "{message}");
}

/// REQ: F-030
#[test]
fn feature_030_attention_running_indicator_defaults_to_icon_and_accepts_six_values() {
    let mut host = client_host();
    assert_eq!(host.settings().get("ui.attention_running_indicator"), Some(json!("icon")));
    for value in ["off", "icon", "spinner", "pulsing_dot", "steady_dot", "title_accent"] {
        host.settings().set("ui.attention_running_indicator", json!(value)).unwrap_or_else(|error| panic!("{value}: {error:?}"));
    }
}

/// REQ: F-030
#[test]
fn feature_030_attention_running_indicator_outside_the_set_is_invalid_argument() {
    let mut host = client_host();
    let message = rejected(&mut host, "ui.attention_running_indicator", json!("blink"));
    assert!(
        message.contains("ui.attention_running_indicator = \"blink\" must be \"off\", \"icon\", \"spinner\", \"pulsing_dot\", \"steady_dot\", or \"title_accent\""),
        "{message}"
    );
}

/// REQ: F-030
#[test]
fn feature_030_poll_intervals_default_to_ten_and_forty_five_seconds() {
    let mut host = client_host();
    assert_eq!(host.settings().get("detection.working_poll_seconds"), Some(json!(10)));
    assert_eq!(host.settings().get("detection.idle_poll_seconds"), Some(json!(45)));
}

/// REQ: F-030
#[test]
fn feature_030_poll_intervals_accept_whole_seconds() {
    let mut host = client_host();
    host.settings().set("detection.working_poll_seconds", json!(5)).expect("5 s");
    assert_eq!(host.settings().get("detection.working_poll_seconds"), Some(json!(5)));
}

/// REQ: COR-011
#[test]
fn core_011_auto_answer_interstitial_prompts_defaults_to_true() {
    let mut host = client_host();
    assert_eq!(host.settings().get("detection.auto_answer_interstitial_prompts"), Some(json!(true)));
}

/// REQ: COR-011
#[test]
fn core_011_auto_answer_interstitial_prompts_accepts_booleans() {
    let mut host = client_host();
    host.settings().set("detection.auto_answer_interstitial_prompts", json!(false)).expect("false");
    assert_eq!(host.settings().get("detection.auto_answer_interstitial_prompts"), Some(json!(false)));
    host.settings().set("detection.auto_answer_interstitial_prompts", json!(true)).expect("true");
}

/// REQ: COR-011
#[test]
fn core_011_auto_answer_interstitial_prompts_refuses_a_non_boolean_as_invalid_argument() {
    let mut host = client_host();
    rejected(&mut host, "detection.auto_answer_interstitial_prompts", json!("yes"));
}

/// REQ: COR-030, COR-033
#[test]
fn core_030_every_key_of_the_tab_lives_in_the_ui_or_detection_table() {
    let mut host = client_host();
    for key in [
        "ui.agent_monitoring_mode",
        "ui.agent_identifier_mode",
        "ui.claude_agent_icon",
        "ui.codex_agent_icon",
        "ui.antigravity_agent_icon",
        "detection.working_poll_seconds",
        "detection.idle_poll_seconds",
        "detection.auto_answer_interstitial_prompts",
    ] {
        assert!(host.settings().get(key).is_some(), "{key}");
    }
}
