//! Rules the page and the traceability tables state in prose and tables (doc_ tests): the kernel tab, the
//! disabled notice, the extension point table, the traceability rows F-050..F-054, the boot rows.

use crate::common::user_guide;
use crate::manual::{manual_page, page_text, table_after};

fn row_of(text: &str, id: &str) -> Vec<String> {
    let prefix = format!("| {id} |");
    let line = text
        .lines()
        .find(|line| line.starts_with(&prefix))
        .unwrap_or_else(|| panic!("row {id}"));
    line.trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

/// REQ: F-050
#[test]
fn doc_050_traceability_row_is_the_voice_session() {
    let row = row_of(&manual_page("reference-feature-traceability.md"), "F-050");
    assert_eq!(row[1], "Voice session");
    assert_eq!(row[3], "plugin-wasm");
    assert_eq!(row[4], "voice plugin");
    assert_eq!(row[5], "both");
    assert_eq!(row[6], "audio, inference");
}

/// REQ: F-051
#[test]
fn doc_051_traceability_row_is_the_client_confirmation_modal() {
    let row = row_of(&manual_page("reference-feature-traceability.md"), "F-051");
    assert_eq!(row[1], "Voice confirmations");
    assert_eq!(row[5], "client");
    assert_eq!(row[6], "ui.modal");
}

/// REQ: F-052
#[test]
fn doc_052_traceability_row_is_ilium_voice_say_on_both_sides() {
    let row = row_of(&manual_page("reference-feature-traceability.md"), "F-052");
    assert_eq!(row[1], "ilium voice say");
    assert_eq!(row[2], "`cli-reference`");
    assert_eq!(row[5], "both");
    assert_eq!(row[6], "cli.contribute, client requests");
    assert_eq!(row[10], "COR-040");
}

/// REQ: F-053
#[test]
fn doc_053_traceability_row_is_the_onboarding_demo_on_the_client() {
    let row = row_of(&manual_page("reference-feature-traceability.md"), "F-053");
    assert_eq!(row[1], "Voice onboarding demo");
    assert_eq!(row[5], "client");
    assert_eq!(row[6], "audio");
}

/// REQ: F-054
#[test]
fn doc_054_traceability_row_is_device_selection_on_the_client() {
    let row = row_of(&manual_page("reference-feature-traceability.md"), "F-054");
    assert_eq!(row[1], "Voice device selection");
    assert_eq!(row[5], "client");
    assert_eq!(row[6], "audio");
}

/// REQ: F-053
#[test]
fn doc_053_the_onboarding_page_gives_voice_only_the_demo_step() {
    let traceability = manual_page("reference-feature-traceability.md");
    assert!(traceability.contains("Voice owns only the demo step (F-053)."));
}

/// REQ: COR-030, COR-033
#[test]
fn doc_030_the_plugin_owns_exactly_the_voice_control_tab() {
    let rows = table_after(&page_text(), "| Tab id | Label | Config table | Readers |");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0],
        ["`voice-control`", "Voice control", "`[voice]`", "none"]
    );
}

/// REQ: COR-022
#[test]
fn doc_022_disabled_notice_keeps_the_values_read_only() {
    let page = page_text();
    assert!(page.contains(
        "the tab stays in the rail with its stored values shown read-only and the notice \"Voice is disabled. Values are kept.\""
    ));
}

/// REQ: COR-022
#[test]
fn doc_022_disabled_voice_has_no_voice_and_f8_is_unbound() {
    let boot = manual_page("reference-kernel-and-boot.md");
    assert!(boot.contains("| Voice | No voice. F8 is unbound | None |"));
}

/// REQ: COR-033, COR-030
#[test]
fn doc_034_the_tab_writes_every_voice_key_except_the_custom_prompt() {
    let config = manual_page("reference-config-and-data-migration.md");
    assert!(config.contains("| `voice-control` | Voice control | `[voice]` | Voice | none | Every `[voice]` key except `custom_prompt` |"));
    assert!(config.contains("`[voice]` | `enabled`, `api_key`, `model`, `voice`, `reasoning_effort`, `input_mode`, `vad_eagerness`, `input_device_name`, `output_device_name`, `output_volume_percent`, `confirm_terminal_submissions`, `pause_media_while_active`, `custom_prompt`"));
}

/// REQ: COR-050
#[test]
fn doc_050_activation_triggers_in_the_boot_table_equal_the_manifest() {
    let boot = manual_page("reference-kernel-and-boot.md");
    assert!(boot.contains(
        "| Voice | `command:voice.toggle`, `cli:voice` | `command:voice.toggle` | The key F8 is a keybinding that fires the command, which starts the plugin |"
    ));
}

/// REQ: F-052
#[test]
fn doc_052_the_cli_compat_row_lists_voice_say_as_jsonl_through_the_client_side() {
    let compat = manual_page("reference-cli-compatibility.md");
    assert!(compat.contains("| `ilium voice say` | voice | [voice](/pages/core-plugin-voice/) | yes | JSONL | Asks the plugin's client side. |"));
}

/// REQ: F-052
#[test]
fn doc_052_the_error_code_table_has_the_ten_documented_codes_in_order() {
    let rows = table_after(&user_guide(), "| Code | Meaning | Fix |");
    let codes: Vec<&str> = rows.iter().map(|row| row[0].trim_matches('`')).collect();
    assert_eq!(
        codes,
        [
            "invalid-request",
            "no-voice-client",
            "voice-off",
            "voice-unavailable",
            "client-unresponsive",
            "connection-failed",
            "request-send-failed",
            "server-closed-connection",
            "timeout",
            "stdin-unreadable"
        ]
    );
}

/// REQ: F-050
#[test]
fn doc_050_input_mode_table_describes_press_and_release_of_f8() {
    let rows = table_after(&user_guide(), "| Input mode | Press F8 | Release F8 |");
    assert_eq!(
        rows[0],
        [
            "Semantic VAD (default)",
            "Toggles voice on or off",
            "nothing"
        ]
    );
    assert_eq!(
        rows[1],
        [
            "Push to talk",
            "Turns voice on if it is off and starts recording",
            "Stops recording and commits what you said"
        ]
    );
}

/// REQ: F-051
#[test]
fn doc_051_five_actions_always_require_confirmation() {
    let guide = user_guide();
    for bullet in [
        "- Running a shell command pane (\"run this command\").",
        "- Replacing an editor's entire document.",
        "- Deleting a board card or column (the exact item is named in the question).",
        "- Killing the Ilium session and every pane, and restarting the detached server.",
    ] {
        assert!(guide.contains(bullet), "{bullet}");
    }
    assert!(guide.contains("Detach and restarting the client do not need confirmation."));
    assert!(guide.contains("A cancelled action reports \"cancelled the pending action\"."));
}

/// REQ: F-054
#[test]
fn doc_054_voice_settings_table_lists_both_devices_and_the_reconnect_button() {
    let rows = table_after(&user_guide(), "| Option | Values | Default | Notes |");
    let names: Vec<&str> = rows.iter().map(|row| row[0].as_str()).collect();
    assert_eq!(
        names,
        [
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
            "Reconnect now"
        ]
    );
}

/// REQ: SEC-293
#[test]
fn doc_293_the_page_points_to_the_corrected_security_table() {
    assert!(page_text().contains("Risk marks of this plugin's extension points follow the corrected table in [Extension security](/pages/reference-extension-security/) ([SEC-293])."));
}
