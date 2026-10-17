//! The behaviour contract table of the page and the user documentation it quotes (src/docs/automation.md).
//! These tests read the documents: the exact values are the spec the plugin must reproduce.

use crate::common::{behaviour_rows, behaviour_value, manual_page, page_text, user_doc};

fn automation() -> String {
    user_doc("automation.md")
}

/// REQ: scheduler-B01, F-055
#[test]
fn doc_b01_the_table_has_the_21_rows_in_order() {
    let ids: Vec<String> = behaviour_rows().into_iter().map(|row| row.0).collect();
    let expected: Vec<String> = (1..=21).map(|n| format!("scheduler-B{n:02}")).collect();
    assert_eq!(ids, expected);
}

/// REQ: scheduler-B01, F-055
#[test]
fn doc_b01_menu_entry_text_is_on_page_and_in_user_docs() {
    assert!(behaviour_value("scheduler-B01")
        .starts_with("\"Hit key(s) X time from now\", on terminal panes only (shell or agent)"));
    assert!(automation().contains("Choose **Hit key(s) X time from now**."));
    assert!(automation().contains("Editors and boards do not have this entry."));
}

/// REQ: scheduler-B02, F-055
#[test]
fn doc_b02_field_ranges_and_initial_values() {
    let value = behaviour_value("scheduler-B02");
    assert_eq!(
        value,
        "Hours: any whole number, blank is 0. Minutes: 0 to 59, blank is 0. Seconds: 0 to 59, initial 30. Payload: single line, any Unicode. Send Enter: on by default"
    );
    let docs = automation();
    assert!(docs.contains("**Hours**: any whole number; blank counts as 0."));
    assert!(docs.contains("**Minutes**: 0 to 59; blank counts as 0."));
    assert!(docs.contains("**Seconds**: 0 to 59; starts at `30`."));
}

/// REQ: scheduler-B03, F-055
#[test]
fn doc_b03_dialog_keys() {
    assert_eq!(
        behaviour_value("scheduler-B03"),
        "Tab and Shift+Tab move between fields, Space toggles Send Enter, Ctrl+Enter schedules, Esc cancels"
    );
    let docs = automation();
    assert!(docs.contains("| Tab / Shift+Tab | Next / previous field (Hours, Minutes, Seconds, Text, Send Enter, button) |"));
    assert!(docs.contains("| Space (on Send Enter) | Toggle Send Enter |"));
    assert!(docs.contains("| Ctrl+Enter | Schedule |"));
}

/// REQ: scheduler-B04, F-055
#[test]
fn doc_b04_zero_delay_message() {
    assert_eq!(
        behaviour_value("scheduler-B04"),
        "Refused: \"Choose a delay of at least one second\""
    );
    assert!(automation()
        .contains("| Total delay is 0 | Refused: \"Choose a delay of at least one second\". |"));
}

/// REQ: scheduler-B05, F-055
#[test]
fn doc_b05_empty_payload_message() {
    assert_eq!(
        behaviour_value("scheduler-B05"),
        "Refused: \"Enter text, enable Send Enter, or use both\""
    );
    assert!(automation().contains("| Empty text and Send Enter off | Refused: \"Enter text, enable Send Enter, or use both\". |"));
}

/// REQ: scheduler-B06, F-055
#[test]
fn doc_b06_out_of_range_and_non_digit() {
    assert_eq!(
        behaviour_value("scheduler-B06"),
        "Refused with a message naming the field. A non-digit typed in a number field is ignored"
    );
    assert!(automation().contains("| Non-digit typed in a number field | Ignored. |"));
}

/// REQ: scheduler-B07, F-055
#[test]
fn doc_b07_one_pending_schedule_per_pane() {
    assert_eq!(
        behaviour_value("scheduler-B07"),
        "One. A new schedule replaces the old one"
    );
    assert!(automation().contains("There is one pending schedule per pane."));
}

/// REQ: scheduler-B08, F-055
#[test]
fn doc_b08_text_then_enter_with_the_gate_held() {
    assert!(behaviour_value("scheduler-B08").starts_with(
        "The text is written, then Enter as a separate step, while the pane input gate is held"
    ));
    assert!(automation().contains("| Text only | The text is written to the pane without Enter. |"));
    assert!(automation().contains("| Enter only | A single Enter is sent. |"));
}

/// REQ: scheduler-B09, F-055
#[test]
fn doc_b09_deadline_does_not_check_idleness() {
    assert!(
        behaviour_value("scheduler-B09").ends_with("It does not check whether the agent is idle")
    );
    assert!(automation().contains("Scheduled input does not check whether the pane is idle."));
}

/// REQ: scheduler-B10, F-055
#[test]
fn doc_b10_restart_and_close() {
    assert_eq!(
        behaviour_value("scheduler-B10"),
        "The deadline survives a restart. The pane closing discards the schedule"
    );
    assert!(automation().contains("| The pane is closed | The schedule is discarded with it. |"));
}

/// REQ: scheduler-B11, F-057
#[test]
fn doc_b11_queue_entry_and_dialog_title() {
    assert_eq!(
        behaviour_value("scheduler-B11"),
        "Menu entry \"Queue prompt...\"; dialog \"Queue prompt after agent finishes\". Enter inserts a new line; Ctrl+Enter enqueues"
    );
    assert!(automation().contains("**Enter inserts a new line** here; it does not submit."));
}

/// REQ: scheduler-B12, F-057
#[test]
fn doc_b12_delivery_modes() {
    assert_eq!(
        behaviour_value("scheduler-B12"),
        "Once; Run X times (count field default 2, a positive whole number); Enqueue forever, marked DANGER"
    );
    assert!(automation().contains("The count field (default `2`) must be a positive whole number."));
}

/// REQ: scheduler-B13, F-057
#[test]
fn doc_b13_forever_warning_text() {
    let text =
        "DANGER: this will re-send forever whenever the agent finishes. Do not run it unmonitored.";
    assert_eq!(behaviour_value("scheduler-B13"), format!("\"{text}\""));
    assert!(automation().contains(text));
}

/// REQ: scheduler-B14, F-057
#[test]
fn doc_b14_empty_prompt_message() {
    assert_eq!(
        behaviour_value("scheduler-B14"),
        "Refused: \"Write a prompt before enqueueing it\""
    );
    assert!(automation().contains(
        "An empty or whitespace-only prompt is refused (\"Write a prompt before enqueueing it\")."
    ));
}

/// REQ: scheduler-B15, F-057
#[test]
fn doc_b15_clear_entry_only_while_not_empty() {
    assert_eq!(
        behaviour_value("scheduler-B15"),
        "Menu entry \"Clear prompt queue\", shown only while the queue is not empty"
    );
    assert!(automation().contains("That entry appears only while the queue is not empty."));
}

/// REQ: scheduler-B16, F-057
#[test]
fn doc_b16_delivery_trigger_is_a_finished_turn() {
    assert_eq!(
        behaviour_value("scheduler-B16"),
        "One prompt per finished turn (Working to Idle or Done), not a timer"
    );
}

/// REQ: scheduler-B17, F-057
#[test]
fn doc_b17_fifo_and_rotation() {
    assert_eq!(
        behaviour_value("scheduler-B17"),
        "First in, first out. A repeating entry goes back to the tail with its count decreased"
    );
    assert!(automation().contains("**First in, first out.**"));
}

/// REQ: scheduler-B18, F-057
#[test]
fn doc_b18_capacity_is_100() {
    assert_eq!(
        behaviour_value("scheduler-B18"),
        "At most 100 entries per pane"
    );
    assert!(automation().contains("The queue holds at most **100** entries per pane."));
}

/// REQ: scheduler-B19, F-057
#[test]
fn doc_b19_fenced_delivery() {
    assert!(behaviour_value("scheduler-B19")
        .ends_with("A prompt that may have reached the PTY is not replayed after a crash"));
}

/// REQ: scheduler-B20, F-057
#[test]
fn doc_b20_delivery_is_recorded_as_queued_prompt() {
    assert_eq!(
        behaviour_value("scheduler-B20"),
        "Each delivery is recorded in the agent debug log as a queued prompt"
    );
    assert!(automation().contains("A queued prompt is recorded as such in the agent debug log."));
}

/// REQ: scheduler-B21, F-055, F-057
#[test]
fn doc_b21_storage_section_and_fields() {
    let value = behaviour_value("scheduler-B21");
    assert!(value.starts_with("Snapshot section `ilium.core.scheduler`, holding `scheduled_input` and `prompt_queue` per pane"));
    let migration = manual_page("reference-config-and-data-migration.md");
    assert!(migration.contains("ilium.core.scheduler"));
}

/// REQ: F-055, F-057
#[test]
fn doc_f055_f057_traceability_rows() {
    let trace = manual_page("reference-feature-traceability.md");
    assert!(trace.contains("| F-055 | Scheduled input | `automation` | plugin-wasm | scheduler plugin | server | timers, terminal.write | none | snapshot section ilium.core.scheduler | 4b | COR-011 |"));
    assert!(trace.contains("| F-057 | Prompt queue | `automation` | primitive-extended | scheduler plugin | server | agent.read, terminal.write | none | snapshot section ilium.core.scheduler | 4b | COR-011 |"));
}

/// REQ: F-055
#[test]
fn doc_f055_the_page_covers_scheduled_input_with_the_menu_entry() {
    let page = page_text();
    assert!(page.contains("The menu entry is \"Hit key(s) X time from now\". A countdown of hours, minutes and seconds ends in a send of the text, then Enter."));
    assert!(
        page.contains("A pane has at most one pending schedule; a new one replaces the old one.")
    );
}

/// REQ: F-057
#[test]
fn doc_f057_the_page_covers_the_prompt_queue_with_the_menu_entry() {
    let page = page_text();
    assert!(page.contains("The menu entry is \"Queue prompt...\". One queued prompt is sent each time the agent finishes a turn, which is a Working to Idle or Done transition, not whenever it is idle"));
}

/// REQ: F-055, F-057
#[test]
fn doc_baseline_has_no_calendar_mode_and_no_queue_panel() {
    assert!(page_text()
        .contains("A calendar mode or a queue panel with edit and move keys is not parity"));
}
