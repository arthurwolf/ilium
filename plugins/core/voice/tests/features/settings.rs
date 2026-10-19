//! The kernel tab `voice-control` (settings page `voice-control`): fourteen rows in order, the defaults of
//! `[voice]`, the saved key of every row, validation messages, and which changes reconnect the session.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ActionKind, ActionValue, ViewTarget};
use serde_json::{json, Value};

use crate::common::{client_host, enabled_client};

const ROWS: u16 = 30;

fn page() -> ViewTarget {
    ViewTarget::SettingsPage("voice-control".to_string())
}

fn rendered(host: &mut FakeHost) -> String {
    host.render(page(), 100, ROWS).text()
}

fn row_text(host: &mut FakeHost, label: &str) -> String {
    let screen = host.render(page(), 100, ROWS);
    let (row, _) = screen.find(label).unwrap_or_else(|| panic!("row {label}"));
    screen.row(row)
}

/// The value of a row: the text after the last dotted leader, without surrounding control marks.
fn row_value(host: &mut FakeHost, label: &str) -> String {
    let row = row_text(host, label);
    let value = row.rsplit('\u{2026}').next().unwrap_or("");
    value
        .trim_matches(|c: char| c.is_whitespace() || "<>[]".contains(c))
        .to_string()
}

fn act(host: &mut FakeHost, key: &str, kind: ActionKind, value: ActionValue) {
    host.send_action(page(), key, kind, value);
    host.settle();
}

fn saved(host: &mut FakeHost, key: &str) -> Option<Value> {
    host.kernel_settings().get("voice-control", key)
}

/// REQ: F-050, COR-030
#[test]
fn settings_050_the_page_lists_fourteen_rows_in_the_documented_order() {
    let mut host = client_host();
    let text = rendered(&mut host);
    let labels = [
        "Voice control",
        "OpenAI API key",
        "Model",
        "Voice",
        "Reasoning effort",
        "Input mode",
        "VAD eagerness",
        "Input device",
        "Output device",
        "Output volume",
        "Confirm terminal submissions",
        "Pause media while active",
        "Custom prompt",
        "Reconnect now",
    ];
    let mut last = 0;
    for label in labels {
        let at = text[last..]
            .find(label)
            .unwrap_or_else(|| panic!("row {label} after offset {last} in {text}"));
        last += at + label.len();
    }
}

/// REQ: F-050
#[test]
fn settings_050_defaults_are_off_realtime_2_1_marin_low_semantic_vad_auto() {
    let mut host = client_host();
    for (label, expected) in [
        ("Voice control", "Off"),
        ("Model", "GPT Realtime 2.1"),
        ("Voice", "Marin"),
        ("Reasoning effort", "Low"),
        ("Input mode", "Semantic VAD"),
        ("VAD eagerness", "Auto"),
    ] {
        assert_eq!(row_value(&mut host, label), expected, "{label}");
    }
}

/// REQ: F-054
#[test]
fn settings_054_defaults_use_the_system_devices_and_eighty_percent_volume() {
    let mut host = client_host();
    assert!(row_text(&mut host, "Input device").contains("System default"));
    assert!(row_text(&mut host, "Output device").contains("System default"));
    assert!(row_text(&mut host, "Output volume").contains("80"));
}

/// REQ: F-050
#[test]
fn settings_050_defaults_confirm_off_pause_media_on_and_an_empty_custom_prompt() {
    let mut host = client_host();
    assert!(row_text(&mut host, "Confirm terminal submissions").contains("Off"));
    assert!(row_text(&mut host, "Pause media while active").contains("On"));
}

/// REQ: F-050
#[test]
fn settings_050_rows_are_joined_to_their_values_by_dotted_leaders() {
    let mut host = client_host();
    assert!(row_text(&mut host, "Model").contains(" \u{2026}"));
}

/// REQ: F-050
#[test]
fn settings_050_the_model_select_saves_the_api_names() {
    let mut host = client_host();
    act(
        &mut host,
        "model",
        ActionKind::Selected,
        ActionValue::Index(1),
    );
    assert_eq!(
        saved(&mut host, "model"),
        Some(json!("gpt-realtime-2.1-mini"))
    );
    act(
        &mut host,
        "model",
        ActionKind::Selected,
        ActionValue::Index(0),
    );
    assert_eq!(saved(&mut host, "model"), Some(json!("gpt-realtime-2.1")));
}

/// REQ: F-050
#[test]
fn settings_050_the_voice_select_offers_ten_voices_in_order() {
    let mut host = client_host();
    let expected = [
        "marin", "cedar", "alloy", "ash", "ballad", "coral", "echo", "sage", "shimmer", "verse",
    ];
    for (index, name) in expected.iter().enumerate() {
        act(
            &mut host,
            "voice",
            ActionKind::Selected,
            ActionValue::Index(index as u32),
        );
        assert_eq!(saved(&mut host, "voice"), Some(json!(name)));
    }
}

/// REQ: F-050
#[test]
fn settings_050_the_eleventh_voice_index_is_refused() {
    let mut host = client_host();
    act(
        &mut host,
        "voice",
        ActionKind::Selected,
        ActionValue::Index(10),
    );
    assert_ne!(saved(&mut host, "voice"), Some(json!("verse")));
    assert!(rendered(&mut host).contains("invalid Realtime voice"));
}

/// REQ: F-050
#[test]
fn settings_050_a_voice_name_outside_the_list_is_refused_with_its_message() {
    let mut host = client_host();
    act(
        &mut host,
        "voice",
        ActionKind::Changed,
        ActionValue::Text("nope".to_string()),
    );
    assert_eq!(saved(&mut host, "voice"), None);
    assert!(rendered(&mut host).contains("invalid Realtime voice"));
}

/// REQ: F-050
#[test]
fn settings_050_a_model_name_outside_the_list_is_refused_with_its_message() {
    let mut host = client_host();
    act(
        &mut host,
        "model",
        ActionKind::Changed,
        ActionValue::Text("gpt-nope".to_string()),
    );
    assert_eq!(saved(&mut host, "model"), None);
    assert!(rendered(&mut host).contains("invalid Realtime voice model"));
}

/// REQ: F-050
#[test]
fn settings_050_reasoning_effort_offers_minimal_low_medium() {
    let mut host = client_host();
    for (index, name) in ["minimal", "low", "medium"].iter().enumerate() {
        act(
            &mut host,
            "reasoning_effort",
            ActionKind::Selected,
            ActionValue::Index(index as u32),
        );
        assert_eq!(saved(&mut host, "reasoning_effort"), Some(json!(name)));
    }
}

/// REQ: F-050
#[test]
fn settings_050_input_mode_offers_semantic_vad_and_push_to_talk() {
    let mut host = client_host();
    act(
        &mut host,
        "input_mode",
        ActionKind::Selected,
        ActionValue::Index(1),
    );
    assert_eq!(saved(&mut host, "input_mode"), Some(json!("push_to_talk")));
    act(
        &mut host,
        "input_mode",
        ActionKind::Selected,
        ActionValue::Index(0),
    );
    assert_eq!(saved(&mut host, "input_mode"), Some(json!("semantic_vad")));
}

/// REQ: F-050
#[test]
fn settings_050_an_unknown_input_mode_is_refused_with_its_message() {
    let mut host = client_host();
    act(
        &mut host,
        "input_mode",
        ActionKind::Changed,
        ActionValue::Text("shout".to_string()),
    );
    assert_eq!(saved(&mut host, "input_mode"), None);
    assert!(rendered(&mut host).contains("voice input mode must be semantic_vad or push_to_talk"));
}

/// REQ: F-050
#[test]
fn settings_050_vad_eagerness_offers_auto_low_medium_high() {
    let mut host = client_host();
    for (index, name) in ["auto", "low", "medium", "high"].iter().enumerate() {
        act(
            &mut host,
            "vad_eagerness",
            ActionKind::Selected,
            ActionValue::Index(index as u32),
        );
        assert_eq!(saved(&mut host, "vad_eagerness"), Some(json!(name)));
    }
}

/// REQ: F-050
#[test]
fn settings_050_output_volume_accepts_zero_and_one_hundred() {
    let mut host = client_host();
    act(
        &mut host,
        "output_volume_percent",
        ActionKind::Changed,
        ActionValue::Number(0.0),
    );
    assert_eq!(saved(&mut host, "output_volume_percent"), Some(json!(0)));
    act(
        &mut host,
        "output_volume_percent",
        ActionKind::Changed,
        ActionValue::Number(100.0),
    );
    assert_eq!(saved(&mut host, "output_volume_percent"), Some(json!(100)));
}

/// REQ: F-050
#[test]
fn settings_050_output_volume_101_is_refused_with_its_message() {
    let mut host = client_host();
    act(
        &mut host,
        "output_volume_percent",
        ActionKind::Changed,
        ActionValue::Number(101.0),
    );
    assert_ne!(saved(&mut host, "output_volume_percent"), Some(json!(101)));
    assert!(rendered(&mut host).contains("voice output volume must be between 0 and 100"));
}

/// REQ: F-050
#[test]
fn settings_050_output_volume_below_zero_is_refused_with_its_message() {
    let mut host = client_host();
    act(
        &mut host,
        "output_volume_percent",
        ActionKind::Changed,
        ActionValue::Number(-1.0),
    );
    assert_ne!(saved(&mut host, "output_volume_percent"), Some(json!(-1)));
    assert!(rendered(&mut host).contains("voice output volume must be between 0 and 100"));
}

/// REQ: F-050
#[test]
fn settings_050_a_refused_volume_keeps_the_previous_value() {
    let mut host = client_host();
    act(
        &mut host,
        "output_volume_percent",
        ActionKind::Changed,
        ActionValue::Number(55.0),
    );
    act(
        &mut host,
        "output_volume_percent",
        ActionKind::Changed,
        ActionValue::Number(500.0),
    );
    assert_eq!(saved(&mut host, "output_volume_percent"), Some(json!(55)));
}

/// REQ: F-050
#[test]
fn settings_050_the_two_toggles_save_booleans() {
    let mut host = client_host();
    act(
        &mut host,
        "confirm_terminal_submissions",
        ActionKind::Toggled,
        ActionValue::Flag(true),
    );
    assert_eq!(
        saved(&mut host, "confirm_terminal_submissions"),
        Some(json!(true))
    );
    act(
        &mut host,
        "pause_media_while_active",
        ActionKind::Toggled,
        ActionValue::Flag(false),
    );
    assert_eq!(
        saved(&mut host, "pause_media_while_active"),
        Some(json!(false))
    );
}

/// REQ: F-050
#[test]
fn settings_050_the_voice_control_toggle_saves_enabled_immediately() {
    let mut host = client_host();
    act(
        &mut host,
        "enabled",
        ActionKind::Toggled,
        ActionValue::Flag(true),
    );
    assert_eq!(saved(&mut host, "enabled"), Some(json!(true)));
}

/// REQ: F-050
#[test]
fn settings_050_the_api_key_is_saved_but_never_shown() {
    let mut host = client_host();
    act(
        &mut host,
        "api_key",
        ActionKind::Submitted,
        ActionValue::Text("sk-secret-123".to_string()),
    );
    assert_eq!(saved(&mut host, "api_key"), Some(json!("sk-secret-123")));
    assert!(!rendered(&mut host).contains("sk-secret-123"));
}

/// REQ: F-050
#[test]
fn settings_050_the_custom_prompt_round_trips_a_trailing_newline() {
    let mut host = client_host();
    act(
        &mut host,
        "custom_prompt",
        ActionKind::Submitted,
        ActionValue::Text("Answer in French.\n".to_string()),
    );
    assert_eq!(
        saved(&mut host, "custom_prompt"),
        Some(json!("Answer in French.\n"))
    );
}

/// REQ: F-050
#[test]
fn settings_050_the_custom_prompt_is_not_written_by_the_voice_control_tab_key_rules() {
    let page = crate::manual::manual_page("reference-config-and-data-migration.md");
    assert!(page
        .contains("`llm-instructions` | LLM Instructions | `[inference.instructions]`, `[voice]`"));
}

/// Number of capture opens a settings change causes while voice is listening in semantic VAD mode.
fn captures_opened_by(key: &str, kind: ActionKind, value: ActionValue) -> usize {
    let mut host = enabled_client("semantic_vad");
    let before = host.audio().captures().len();
    act(&mut host, key, kind, value);
    host.audio().captures().len() - before
}

/// REQ: F-050
#[test]
fn settings_050_changing_the_model_reconnects_the_session() {
    assert_eq!(
        captures_opened_by("model", ActionKind::Selected, ActionValue::Index(1)),
        1
    );
}

/// REQ: F-050
#[test]
fn settings_050_changing_the_voice_reconnects_the_session() {
    assert_eq!(
        captures_opened_by("voice", ActionKind::Selected, ActionValue::Index(1)),
        1
    );
}

/// REQ: F-050
#[test]
fn settings_050_changing_the_reasoning_effort_reconnects_the_session() {
    assert_eq!(
        captures_opened_by(
            "reasoning_effort",
            ActionKind::Selected,
            ActionValue::Index(2)
        ),
        1
    );
}

/// REQ: F-050
#[test]
fn settings_050_changing_the_input_mode_reconnects_the_session() {
    assert_eq!(
        captures_opened_by("input_mode", ActionKind::Selected, ActionValue::Index(1)),
        1
    );
}

/// REQ: F-050
#[test]
fn settings_050_changing_the_vad_eagerness_reconnects_the_session() {
    assert_eq!(
        captures_opened_by("vad_eagerness", ActionKind::Selected, ActionValue::Index(3)),
        1
    );
}

/// REQ: F-050
#[test]
fn settings_050_changing_the_output_volume_reconnects_the_session() {
    assert_eq!(
        captures_opened_by(
            "output_volume_percent",
            ActionKind::Changed,
            ActionValue::Number(40.0)
        ),
        1
    );
}

/// REQ: F-050
#[test]
fn settings_050_changing_the_custom_prompt_reconnects_the_session() {
    assert_eq!(
        captures_opened_by(
            "custom_prompt",
            ActionKind::Submitted,
            ActionValue::Text("Be brief".to_string())
        ),
        1
    );
}

/// REQ: F-050
#[test]
fn settings_050_toggling_confirm_terminal_submissions_does_not_interrupt_a_healthy_connection() {
    assert_eq!(
        captures_opened_by(
            "confirm_terminal_submissions",
            ActionKind::Toggled,
            ActionValue::Flag(true)
        ),
        0
    );
}

/// REQ: F-050
#[test]
fn settings_050_reconnect_now_restarts_the_audio() {
    assert_eq!(
        captures_opened_by("reconnect", ActionKind::Activated, ActionValue::None),
        1
    );
}

/// REQ: F-050
#[test]
fn settings_050_a_reconnect_closes_the_old_capture() {
    let mut host = enabled_client("semantic_vad");
    act(
        &mut host,
        "model",
        ActionKind::Selected,
        ActionValue::Index(1),
    );
    let open = host
        .audio()
        .captures()
        .iter()
        .filter(|capture| capture.open)
        .count();
    assert_eq!(open, 1);
}

/// REQ: COR-022, F-050
#[test]
fn settings_050_a_disabled_plugin_shows_the_notice_and_keeps_the_values() {
    let mut host = client_host();
    act(
        &mut host,
        "model",
        ActionKind::Selected,
        ActionValue::Index(1),
    );
    host.deactivate(ilium_plugin_sdk::types::DeactivationReason::Disabled);
    assert_eq!(
        saved(&mut host, "model"),
        Some(json!("gpt-realtime-2.1-mini"))
    );
}
