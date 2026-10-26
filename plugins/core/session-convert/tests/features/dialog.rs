//! F-077: the client side draws the progress dialog from the messages of the server side: one row per step,
//! a progress bar and a log line; Esc cancels, and after a failure Enter resumes the original session and
//! Esc closes the dialog (page figures "The progress dialog" and "Conversion failed").

use ilium_plugin_sdk::testing::{assert_golden, FakeHost};
use ilium_plugin_sdk::types::{Intent, PluginSide, ViewTarget};
use proptest::prelude::*;
use serde_json::{json, Value};

use crate::common::*;

const CLAUDE_STEPS: [&str; 6] = [
    "Locate the Claude Code transcript",
    "Inspect the transcript",
    "Start a private codex app-server",
    "Import the session",
    "Verify the Codex rollout",
    "Stop the app-server",
];
const CODEX_STEPS: [&str; 6] = [
    "Locate the Codex rollout",
    "Parse the rollout",
    "Convert to Claude Code lines",
    "Write the Claude Code transcript",
    "Verify the transcript",
    "Register with Claude Code",
];

fn panel() -> ViewTarget {
    ViewTarget::Panel("progress".to_string())
}

fn server_says(host: &mut FakeHost, channel: &str, payload: Value) {
    host.deliver_one(message_from(
        PluginSide::Server,
        channel,
        payload.to_string().as_bytes(),
    ));
}

/// A client with a running Claude to Codex dialog at step 4 and 51 percent (the first page figure).
fn claude_dialog() -> FakeHost {
    let mut host = client_host();
    server_says(
        &mut host,
        "begin",
        json!({"pane": 42, "source": "claude", "target": "codex"}),
    );
    server_says(&mut host, "step", json!({"index": 4}));
    server_says(
        &mut host,
        "progress",
        json!({"percent": 51, "message": "imported 186 of 364 items"}),
    );
    host
}

/// A client with a Codex to Claude dialog that failed at the verification (the second page figure).
fn failed_dialog(can_resume: bool) -> FakeHost {
    let mut host = client_host();
    server_says(
        &mut host,
        "begin",
        json!({"pane": 42, "source": "codex", "target": "claude"}),
    );
    server_says(&mut host, "step", json!({"index": 5}));
    server_says(
        &mut host,
        "failed",
        json!({"message": "The Claude Code transcript did not verify.", "carried": 212, "left_out": 57, "can_resume": can_resume}),
    );
    host
}

fn rows(host: &mut FakeHost, columns: u16, height: u16) -> Vec<String> {
    host.render(panel(), columns, height)
        .text()
        .split('\n')
        .map(str::to_string)
        .collect()
}

fn press(host: &mut FakeHost, key: &str) -> Vec<ilium_plugin_sdk::types::Submission> {
    let before = host.submitted().len();
    host.key(panel(), key);
    host.submitted()[before..].to_vec()
}

fn percent_shown(host: &mut FakeHost) -> u32 {
    let text = host.render(panel(), 48, 12).text();
    let at = text.find('%').expect("percent sign");
    let digits: String = text[..at]
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    digits.parse().expect("percent digits")
}

/// REQ: F-077
#[test]
fn f_077_the_progress_dialog_matches_the_page_figure_at_48_columns() {
    let mut host = claude_dialog();
    assert_golden(
        &golden("dialog_progress_48.txt"),
        &host.render(panel(), 48, 12).text(),
    );
}

/// REQ: F-077
#[test]
fn f_077_the_failed_dialog_matches_the_page_figure_at_48_columns() {
    let mut host = failed_dialog(true);
    assert_golden(
        &golden("dialog_failed_48.txt"),
        &host.render(panel(), 48, 10).text(),
    );
}

/// REQ: F-077
#[test]
fn f_077_the_title_names_the_source_and_the_target_for_claude_to_codex() {
    let mut host = claude_dialog();
    assert!(rows(&mut host, 48, 12)[0].contains("Convert Claude Code to Codex"));
}

/// REQ: F-077
#[test]
fn f_077_the_title_names_the_source_and_the_target_for_codex_to_claude() {
    let mut host = failed_dialog(true);
    assert!(rows(&mut host, 48, 10)[0].contains("Convert Codex to Claude Code"));
}

/// REQ: F-077
#[test]
fn f_077_claude_to_codex_lists_the_six_steps_of_the_page() {
    let mut host = claude_dialog();
    let screen = host.render(panel(), 48, 12).text();
    for title in CLAUDE_STEPS {
        assert!(screen.contains(title), "{title}");
    }
}

/// REQ: F-077
#[test]
fn f_077_codex_to_claude_lists_the_six_steps_of_the_manual() {
    let mut host = client_host();
    server_says(
        &mut host,
        "begin",
        json!({"pane": 42, "source": "codex", "target": "claude"}),
    );
    let screen = host.render(panel(), 48, 12).text();
    for title in CODEX_STEPS {
        assert!(screen.contains(title), "{title}");
    }
}

/// REQ: F-077
#[test]
fn f_077_finished_steps_show_a_check_mark_the_current_step_an_arrow_and_pending_steps_a_circle() {
    let mut host = claude_dialog();
    let rows = rows(&mut host, 48, 12);
    for (index, expected) in ["✓", "✓", "✓", "▶", "○", "○"].iter().enumerate() {
        let row = &rows[1 + index];
        assert!(
            row.starts_with(&format!("│ {expected} ")),
            "row {index}: {row}"
        );
    }
}

/// REQ: F-077
#[test]
fn f_077_before_the_first_step_message_every_step_is_pending() {
    let mut host = client_host();
    server_says(
        &mut host,
        "begin",
        json!({"pane": 42, "source": "claude", "target": "codex"}),
    );
    let rows = rows(&mut host, 48, 12);
    assert!(
        rows[1..7]
            .iter()
            .all(|row| row.starts_with("│ ○ ") || row.starts_with("│ ▶ ")),
        "{rows:?}"
    );
}

/// REQ: F-077
#[test]
fn f_077_the_log_row_shows_the_last_progress_message() {
    let mut host = claude_dialog();
    assert!(rows(&mut host, 48, 12)[9].contains("imported 186 of 364 items"));
}

/// REQ: F-077
#[test]
fn f_077_the_hint_row_says_esc_cancels_while_converting() {
    let mut host = claude_dialog();
    assert!(rows(&mut host, 48, 12)[10].contains("Esc cancels"));
}

/// REQ: F-077
#[test]
fn f_077_the_bar_fills_half_of_its_24_cells_at_51_percent() {
    let mut host = claude_dialog();
    let bar = &rows(&mut host, 48, 12)[8];
    assert_eq!(bar.chars().filter(|c| *c == '█').count(), 12, "{bar}");
    assert_eq!(bar.chars().filter(|c| *c == '░').count(), 12, "{bar}");
}

/// REQ: F-077
#[test]
fn f_077_progress_never_moves_backwards() {
    let mut host = claude_dialog();
    server_says(
        &mut host,
        "progress",
        json!({"percent": 40, "message": "late"}),
    );
    assert_eq!(percent_shown(&mut host), 51);
}

/// REQ: F-077
#[test]
fn f_077_progress_never_passes_one_hundred_percent() {
    let mut host = claude_dialog();
    server_says(
        &mut host,
        "progress",
        json!({"percent": 250, "message": "over"}),
    );
    assert_eq!(percent_shown(&mut host), 100);
}

/// REQ: F-077
#[test]
fn f_077_a_negative_progress_value_is_treated_as_zero() {
    let mut host = client_host();
    server_says(
        &mut host,
        "begin",
        json!({"pane": 42, "source": "claude", "target": "codex"}),
    );
    server_says(
        &mut host,
        "progress",
        json!({"percent": -5, "message": "x"}),
    );
    assert_eq!(percent_shown(&mut host), 0);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    // REQ: F-077
    #[test]
    fn f_077_prop_the_shown_percent_is_the_running_maximum_clamped_to_0_100(values in proptest::collection::vec(-50i32..200, 1..8)) {
        let mut host = client_host();
        server_says(&mut host, "begin", json!({"pane": 42, "source": "claude", "target": "codex"}));
        let mut expected = 0i32;
        for value in &values {
            server_says(&mut host, "progress", json!({"percent": value, "message": "m"}));
            expected = expected.max((*value).clamp(0, 100));
        }
        prop_assert_eq!(percent_shown(&mut host) as i32, expected);
    }
}

/// REQ: F-077
#[test]
fn f_077_esc_while_converting_asks_the_server_to_cancel_and_keeps_the_dialog() {
    let mut host = claude_dialog();
    let submissions = press(&mut host, "Esc");
    let sent = messages(&submissions);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(
        (sent[0].1, sent[0].2.as_str()),
        (PluginSide::Server, "cancel")
    );
    assert_eq!(
        serde_json::from_str::<Value>(&sent[0].3).unwrap(),
        json!({"pane": 42})
    );
    assert!(!intents(&submissions)
        .iter()
        .any(|intent| matches!(intent, Intent::CloseView(_))));
}

/// REQ: F-077
#[test]
fn f_077_enter_while_converting_does_nothing() {
    let mut host = claude_dialog();
    assert!(press(&mut host, "Enter").is_empty());
}

/// REQ: F-077
#[test]
fn f_077_the_failed_dialog_shows_the_failure_and_the_untouched_original() {
    let mut host = failed_dialog(true);
    let screen = host.render(panel(), 48, 10).text();
    assert!(screen.contains("✕ Conversion failed"));
    assert!(screen.contains("The original session is untouched."));
}

/// REQ: F-077
#[test]
fn f_077_the_failed_dialog_counts_carried_over_and_left_out_items_with_leaders() {
    let mut host = failed_dialog(true);
    let rows = rows(&mut host, 48, 10);
    assert!(
        rows[5].contains("Carried over …") && rows[5].ends_with("212 items │"),
        "{}",
        rows[5]
    );
    assert!(
        rows[6].contains("Left out …") && rows[6].ends_with("57 items │"),
        "{}",
        rows[6]
    );
}

/// REQ: F-077
#[test]
fn f_077_enter_after_a_failure_that_follows_the_stop_resumes_the_original_session() {
    let mut host = failed_dialog(true);
    let sent = messages(&press(&mut host, "Enter"));
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(
        (sent[0].1, sent[0].2.as_str()),
        (PluginSide::Server, "resume")
    );
    assert_eq!(
        serde_json::from_str::<Value>(&sent[0].3).unwrap(),
        json!({"pane": 42})
    );
}

/// REQ: F-077
#[test]
fn f_077_enter_after_a_failure_before_the_stop_does_not_resume_a_live_agent() {
    let mut host = failed_dialog(false);
    assert!(messages(&press(&mut host, "Enter"))
        .iter()
        .all(|m| m.2 != "resume"));
}

/// REQ: F-077
#[test]
fn f_077_the_failed_dialog_without_a_resume_choice_offers_only_close() {
    let mut host = failed_dialog(false);
    let screen = host.render(panel(), 48, 10).text();
    assert!(!screen.contains("Resume original"));
    assert!(screen.contains("Close"));
}

/// REQ: F-077
#[test]
fn f_077_esc_after_a_failure_closes_the_dialog_and_tells_the_server() {
    let mut host = failed_dialog(true);
    let submissions = press(&mut host, "Esc");
    let sent = messages(&submissions);
    assert_eq!(
        sent.iter().filter(|m| m.2 == "close").count(),
        1,
        "{sent:?}"
    );
    assert!(intents(&submissions).contains(&Intent::CloseView(panel())));
}

/// REQ: F-077
#[test]
fn f_077_a_successful_conversion_closes_the_dialog() {
    let mut host = claude_dialog();
    let before = host.submitted().len();
    server_says(&mut host, "finished", json!({"pane": 42, "ok": true}));
    let submissions = host.submitted()[before..].to_vec();
    assert!(intents(&submissions).contains(&Intent::CloseView(panel())));
}

/// REQ: F-077
#[test]
fn f_077_the_menu_is_enabled_again_after_the_dialog_closed() {
    let mut host = failed_dialog(true);
    press(&mut host, "Esc");
    let pane = add_agent_pane(&mut host, "agent", "claude");
    host.agents().set_session(
        pane,
        Some(ilium_plugin_sdk::primitives::SessionRef {
            scope: "agent-sessions:claude".into(),
            path: "p.jsonl".into(),
            session_id: "s".into(),
        }),
    );
    let context = ilium_plugin_sdk::exports::MenuContext {
        menu: "pane".into(),
        node: Some(ilium_plugin_sdk::types::NodeId(pane)),
        pane: Some(ilium_plugin_sdk::types::NodeId(pane)),
        view: None,
        key: None,
        buffer: None,
        line: None,
        selection: None,
    };
    assert_eq!(host.menu_items(context)[0].disabled_reason, None);
}

/// REQ: F-077
#[test]
fn f_077_a_step_message_after_a_failure_does_not_revive_the_dialog() {
    let mut host = failed_dialog(true);
    server_says(&mut host, "step", json!({"index": 6}));
    assert!(host
        .render(panel(), 48, 10)
        .text()
        .contains("✕ Conversion failed"));
}

/// REQ: F-077
#[test]
fn f_077_a_malformed_server_message_is_ignored_without_a_failure_record() {
    let mut host = claude_dialog();
    host.deliver_one(message_from(PluginSide::Server, "progress", b"not json"));
    assert!(host.failures().is_empty());
    assert_eq!(percent_shown(&mut host), 51);
}

/// REQ: F-077
#[test]
fn f_077_the_dialog_is_48_columns_wide_at_80_columns() {
    let mut host = claude_dialog();
    let rows = rows(&mut host, 80, 24);
    let top = rows
        .iter()
        .find(|row| row.contains('╭'))
        .expect("top border");
    let left = top.chars().position(|c| c == '╭').unwrap();
    let right = top.chars().position(|c| c == '╮').unwrap();
    assert_eq!(right - left + 1, 48);
}

/// REQ: F-077
#[test]
fn f_077_the_dialog_is_48_columns_wide_at_120_columns() {
    let mut host = claude_dialog();
    let rows = rows(&mut host, 120, 30);
    let top = rows
        .iter()
        .find(|row| row.contains('╭'))
        .expect("top border");
    let left = top.chars().position(|c| c == '╭').unwrap();
    let right = top.chars().position(|c| c == '╮').unwrap();
    assert_eq!(right - left + 1, 48);
}

/// REQ: F-077
#[test]
fn f_077_the_dialog_is_centered_at_120_columns() {
    let mut host = claude_dialog();
    let rows = rows(&mut host, 120, 30);
    let top = rows
        .iter()
        .find(|row| row.contains('╭'))
        .expect("top border");
    assert_eq!(top.chars().position(|c| c == '╭').unwrap(), 36);
}

/// REQ: F-077
#[test]
fn f_077_no_row_is_wider_than_the_area_at_40_columns() {
    let mut host = claude_dialog();
    for row in rows(&mut host, 40, 20) {
        assert!(row.chars().count() <= 40, "{row}");
    }
}

/// REQ: F-077
#[test]
fn f_077_the_dialog_still_names_its_title_at_40_columns() {
    let mut host = claude_dialog();
    assert!(host.render(panel(), 40, 20).text().contains("Convert"));
}
