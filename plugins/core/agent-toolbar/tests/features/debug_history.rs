//! Debug history popover: server reads the pane journal (`journal.read`), client shows a table in a popup.

use ilium_plugin_sdk::testing::assert_golden;
use ilium_plugin_sdk::types::{AgentActivity, CommandOrigin, PluginSide, ViewTarget};
use serde_json::Value;

use crate::common::*;

const GOLDEN_47: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/features/golden/debug_history_47.txt"
);
/// 2030-01-01 14:06:00 UTC: the "now" of the page figure; its entries are inside the last hour.
const NOW_MS: u64 = CLOCK_START_MS + (14 * 3600 + 6 * 60) * 1000;
const HOUR_MS: u64 = 3_600_000;
const JOURNAL_LIMIT: usize = 50;

fn at(hour: u64, minute: u64, second: u64) -> u64 {
    CLOCK_START_MS + ((hour * 60 + minute) * 60 + second) * 1000
}

fn history_host() -> (ilium_plugin_sdk::testing::FakeHost, u64) {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Working);
    host.set_clock(NOW_MS);
    (host, pane)
}

fn request_history(host: &mut ilium_plugin_sdk::testing::FakeHost, pane: u64) -> Vec<Value> {
    let submissions = host.deliver_one(message_from(
        PluginSide::Client,
        "history",
        pane.to_string().as_bytes(),
    ));
    let sent = messages(&submissions);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(
        (sent[0].0.as_str(), sent[0].1, sent[0].2.as_str()),
        (PLUGIN_ID, PluginSide::Client, "history")
    );
    serde_json::from_slice::<Value>(&sent[0].3)
        .expect("payload is JSON")
        .as_array()
        .expect("a JSON array")
        .clone()
}

fn figure_entries(host: &mut ilium_plugin_sdk::testing::FakeHost, pane: u64) {
    host.journal()
        .append(pane, at(14, 2, 11), "input-sent", "/compact");
    host.journal()
        .append(pane, at(14, 2, 12), "activity", "idle to working");
    host.journal()
        .append(pane, at(14, 5, 40), "activity", "working to approval");
}

/// REQ: F-035
#[test]
fn f_035_history_command_on_a_row_is_forwarded_with_the_pane_id() {
    let mut host = client_host();
    let submissions = host.deliver_one(command_on("history", CommandOrigin::RowAction, Some(12)));
    let sent = messages(&submissions);
    assert_eq!(
        sent,
        vec![(
            PLUGIN_ID.to_string(),
            PluginSide::Server,
            "history".to_string(),
            b"12".to_vec()
        )]
    );
}

/// REQ: F-035
#[test]
fn f_035_history_reply_lists_at_kind_and_detail_per_entry() {
    let (mut host, pane) = history_host();
    figure_entries(&mut host, pane);
    let rows = request_history(&mut host, pane);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["at"], at(14, 2, 11));
    assert_eq!(rows[0]["kind"], "input-sent");
    assert_eq!(rows[0]["detail"], "/compact");
    assert_eq!(rows[2]["detail"], "working to approval");
}

/// REQ: F-035
#[test]
fn f_035_history_keeps_the_journal_order() {
    let (mut host, pane) = history_host();
    figure_entries(&mut host, pane);
    let details: Vec<String> = request_history(&mut host, pane)
        .iter()
        .map(|row| row["detail"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        details,
        ["/compact", "idle to working", "working to approval"]
    );
}

/// REQ: F-035
#[test]
fn f_035_history_of_an_empty_journal_is_an_empty_array() {
    let (mut host, pane) = history_host();
    assert!(request_history(&mut host, pane).is_empty());
}

/// REQ: F-035
#[test]
fn f_035_history_reads_only_the_last_hour() {
    let (mut host, pane) = history_host();
    host.journal()
        .append(pane, NOW_MS - HOUR_MS - 1, "activity", "too old");
    host.journal()
        .append(pane, NOW_MS - HOUR_MS + 1, "activity", "just inside");
    let details: Vec<String> = request_history(&mut host, pane)
        .iter()
        .map(|row| row["detail"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(details, ["just inside"]);
}

/// REQ: F-035
#[test]
fn f_035_history_reads_at_most_50_entries() {
    let (mut host, pane) = history_host();
    for index in 0..51u64 {
        host.journal().append(
            pane,
            NOW_MS - 1_000 + index,
            "activity",
            &format!("entry {index}"),
        );
    }
    assert!(request_history(&mut host, pane).len() <= JOURNAL_LIMIT);
}

/// REQ: F-035
#[test]
fn f_035_history_of_exactly_50_entries_returns_all_of_them() {
    let (mut host, pane) = history_host();
    for index in 0..50u64 {
        host.journal().append(
            pane,
            NOW_MS - 1_000 + index,
            "activity",
            &format!("entry {index}"),
        );
    }
    assert_eq!(request_history(&mut host, pane).len(), 50);
}

/// REQ: F-035
#[test]
fn f_035_history_of_49_entries_returns_49() {
    let (mut host, pane) = history_host();
    for index in 0..49u64 {
        host.journal().append(
            pane,
            NOW_MS - 1_000 + index,
            "activity",
            &format!("entry {index}"),
        );
    }
    assert_eq!(request_history(&mut host, pane).len(), 49);
}

/// REQ: F-035
#[test]
fn f_035_history_of_one_pane_excludes_the_entries_of_another() {
    let (mut host, pane) = history_host();
    let other = add_agent_pane(&mut host, "codex", "codex", AgentActivity::Working);
    host.journal().append(pane, NOW_MS - 10, "activity", "mine");
    host.journal()
        .append(other, NOW_MS - 10, "activity", "theirs");
    let rows = request_history(&mut host, pane);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["detail"], "mine");
}

/// REQ: F-035
#[test]
fn f_035_history_with_a_bad_pane_payload_sends_nothing() {
    let (mut host, _pane) = history_host();
    let submissions = host.deliver_one(message_from(PluginSide::Client, "history", b"not-a-pane"));
    assert!(messages(&submissions).is_empty());
}

fn client_with_history(host_pane_name: &str) -> (ilium_plugin_sdk::testing::FakeHost, u64) {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, host_pane_name, "claude", AgentActivity::Working);
    host.set_clock(NOW_MS);
    let payload = serde_json::json!([
        {"at": at(14, 2, 11), "kind": "input-sent", "detail": "/compact"},
        {"at": at(14, 2, 12), "kind": "activity", "detail": "idle to working"},
        {"at": at(14, 5, 40), "kind": "activity", "detail": "working to approval"},
    ]);
    host.deliver_one(message_from(
        PluginSide::Server,
        "history",
        payload.to_string().as_bytes(),
    ));
    (host, pane)
}

fn popup() -> ViewTarget {
    ViewTarget::Popup("history".to_string())
}

/// REQ: F-035
#[test]
fn f_035_popup_at_47_columns_matches_the_page_figure() {
    let (mut host, _pane) = client_with_history("claude");
    let screen = host.render(popup(), 47, 6);
    assert_golden(GOLDEN_47, &screen.text());
}

/// REQ: F-035
#[test]
fn doc_f_035_golden_file_equals_the_popover_figure_of_the_page() {
    let figures = term_blocks(&page_text());
    let golden = std::fs::read_to_string(GOLDEN_47).expect("golden file exists");
    assert_eq!(golden, figures[1]);
}

/// REQ: F-035
#[test]
fn f_035_popup_titles_itself_debug_history_and_has_the_three_columns() {
    let (mut host, _pane) = client_with_history("claude");
    let screen = host.render(popup(), 60, 8);
    assert!(screen.find("Debug history").is_some());
    let header = screen.row(1);
    let (time, kind, detail) = (
        header.find("Time"),
        header.find("Kind"),
        header.find("Detail"),
    );
    assert!(time.is_some() && time < kind && kind < detail, "{header:?}");
}

/// REQ: F-035
#[test]
fn f_035_popup_shows_the_time_as_hours_minutes_seconds_in_utc() {
    let (mut host, _pane) = client_with_history("claude");
    let text = host.render(popup(), 60, 8).text();
    for time in ["14:02:11", "14:02:12", "14:05:40"] {
        assert!(text.contains(time), "{time} missing in\n{text}");
    }
}

/// REQ: F-035
#[test]
fn f_035_popup_rows_follow_the_entry_order() {
    let (mut host, _pane) = client_with_history("claude");
    let screen = host.render(popup(), 60, 8);
    let rows: Vec<u16> = ["/compact", "idle to working", "working to approval"]
        .iter()
        .map(|text| screen.find(text).expect("row").0)
        .collect();
    assert!(rows[0] < rows[1] && rows[1] < rows[2], "{rows:?}");
}

/// REQ: F-035
#[test]
fn f_035_popup_renders_at_widths_40_80_and_120_without_losing_the_header() {
    for width in [40u16, 80, 120] {
        let (mut host, _pane) = client_with_history("claude");
        let screen = host.render(popup(), width, 8);
        for label in ["Time", "Kind", "Detail"] {
            assert!(
                screen.find(label).is_some(),
                "{label} missing at {width} columns"
            );
        }
        for line in screen.text().lines() {
            assert!(
                line.chars().count() <= usize::from(width),
                "row wider than {width}"
            );
        }
    }
}

/// REQ: F-035
#[test]
fn f_035_popup_render_is_pure() {
    let (mut host, _pane) = client_with_history("claude");
    let first = host.render(popup(), 60, 8).text();
    let second = host.render(popup(), 60, 8).text();
    assert_eq!(first, second);
    for (needle, time, kind) in [
        ("/compact", "14:02:11", "input-sent"),
        ("idle to working", "14:02:12", "activity"),
        ("working to approval", "14:05:40", "activity"),
    ] {
        let row = first
            .lines()
            .find(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("{needle} row missing"));
        assert!(row.contains(time) && row.contains(kind), "{row:?}");
        assert!(
            row.find(time) < row.find(kind) && row.find(kind) < row.find(needle),
            "{row:?}"
        );
    }
}

/// REQ: F-035
#[test]
fn f_035_empty_history_renders_the_header_and_no_rows() {
    let mut host = client_host();
    host.deliver_one(message_from(PluginSide::Server, "history", b"[]"));
    let text = host.render(popup(), 60, 8).text();
    assert!(text.contains("Time"));
    assert!(!text.contains("input-sent"));
}

/// REQ: F-035
#[test]
fn f_035_malformed_history_payload_opens_no_rows_and_does_not_crash() {
    let mut host = client_host();
    host.deliver_one(message_from(PluginSide::Server, "history", b"{not json"));
    assert!(host.failures().is_empty());
}
