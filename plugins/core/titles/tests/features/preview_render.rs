//! The client preview panel: the diff of the proposed tree (page figure 1) at widths 40, 80, 120 and the 48 column golden.

use ilium_plugin_core_titles::TitlesClient;
use ilium_plugin_sdk::testing::{assert_golden, FakeHost};
use ilium_plugin_sdk::types::{ClientId, Event, MessageEvent, PluginId, PluginSide, ViewTarget};
use serde_json::json;

use crate::common::*;

const GOLDEN_PREVIEW_48: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/features/golden/titles_preview_48.txt"
);
const GOLDEN_SETTINGS_48: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/features/golden/titles_settings_48.txt"
);
const PANEL: &str = "titles.preview";
const PREVIEW_ROWS: u16 = 12;

fn panel() -> ViewTarget {
    ViewTarget::Panel(PANEL.to_string())
}

/// The client with the proposal of the page figure delivered by the server side.
fn host_with_proposal() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client");
    host.activate::<TitlesClient>();
    let payload = json!({
        "old_outline": ["shell-1", "shell-2"],
        "new_outline": ["Billing API", "  claude", "  shell-1", "Web client", "  codex", "  shell-2"],
        "moves": 4,
        "groups": 2
    });
    host.deliver_one(Event::Message(MessageEvent {
        from: PluginId(PLUGIN_ID.to_string()),
        from_side: PluginSide::Server,
        from_client: Some(ClientId(1)),
        channel: "titles.preview".to_string(),
        payload: payload.to_string().into_bytes(),
    }));
    host.settle();
    host
}

fn figure(index: usize) -> String {
    term_blocks(&page_text()).remove(index)
}

/// REQ: F-043
#[test]
fn doc_f_043_preview_golden_file_equals_the_first_figure_of_the_page() {
    let golden = std::fs::read_to_string(GOLDEN_PREVIEW_48).expect("golden file exists");
    assert_eq!(golden.trim_end_matches('\n'), figure(0));
}

/// REQ: F-041
#[test]
fn doc_f_041_settings_golden_file_equals_the_second_figure_of_the_page() {
    let golden = std::fs::read_to_string(GOLDEN_SETTINGS_48).expect("golden file exists");
    assert_eq!(golden.trim_end_matches('\n'), figure(1));
}

/// REQ: F-043
#[test]
fn doc_f_043_preview_figure_is_48_columns_wide_on_every_row() {
    for line in figure(0).lines() {
        assert_eq!(line.chars().count(), 48, "{line}");
    }
}

/// REQ: F-043
#[test]
fn doc_f_043_preview_figure_ends_with_the_move_and_group_counts() {
    assert!(figure(0).contains("Moves: 4, new groups: 2"));
}

/// REQ: F-043
#[test]
fn doc_f_043_preview_figure_marks_removed_rows_with_minus_and_added_rows_with_plus() {
    let figure = figure(0);
    assert!(figure.contains("│- shell-1"));
    assert!(figure.contains("│- shell-2"));
    assert!(figure.contains("│+ Billing API"));
    assert!(figure.contains("│+ Web client"));
}

/// REQ: F-043
#[test]
fn doc_f_043_preview_figure_uses_danger_for_removed_and_success_for_added_rows() {
    let page = page_text();
    assert!(
        page.contains("removed rows use <code>danger</code>, added rows use <code>success</code>")
    );
    assert!(page.contains("<span class=\"t-err\">- shell-1</span>"));
    assert!(page.contains("<span class=\"t-ok\">+ Billing API</span>"));
}

/// REQ: F-043
#[test]
fn f_043_preview_at_48_columns_equals_the_page_figure() {
    let mut host = host_with_proposal();
    let screen = host.render(panel(), 48, PREVIEW_ROWS);
    assert_golden(GOLDEN_PREVIEW_48, &screen.text());
}

/// REQ: F-043
#[test]
fn f_043_preview_at_80_columns_shows_the_hunk_header_and_the_summary() {
    let mut host = host_with_proposal();
    let screen = host.render(panel(), 80, PREVIEW_ROWS);
    let text = screen.text();
    assert!(text.contains("@@ tree, 4 panes @@"));
    assert!(text.contains("Moves: 4, new groups: 2"));
}

/// REQ: F-043
#[test]
fn f_043_preview_at_120_columns_shows_removed_then_added_rows_in_order() {
    let mut host = host_with_proposal();
    let screen = host.render(panel(), 120, PREVIEW_ROWS);
    let text = screen.text();
    let removed = text.find("- shell-1").expect("removed row");
    let added = text.find("+ Billing API").expect("added row");
    assert!(removed < added);
}

/// REQ: F-043
#[test]
fn f_043_preview_at_40_columns_never_draws_a_row_wider_than_the_panel() {
    let mut host = host_with_proposal();
    let screen = host.render(panel(), 40, PREVIEW_ROWS);
    assert_eq!(screen.columns, 40);
    for line in screen.text().lines() {
        assert!(line.chars().count() <= 40, "{line}");
    }
}

/// REQ: F-043
#[test]
fn f_043_preview_has_a_rounded_border_with_the_title_at_every_width() {
    for columns in [40u16, 80, 120] {
        let mut host = host_with_proposal();
        let screen = host.render(panel(), columns, PREVIEW_ROWS);
        let first = screen.row(0);
        assert!(
            first.starts_with("╭─ Preview of the change"),
            "{columns}: {first}"
        );
        assert!(first.ends_with('╮'), "{columns}: {first}");
    }
}

/// REQ: F-043
#[test]
fn f_043_preview_summary_counts_follow_the_proposal() {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client");
    host.activate::<TitlesClient>();
    host.deliver_one(Event::Message(MessageEvent {
        from: PluginId(PLUGIN_ID.to_string()),
        from_side: PluginSide::Server,
        from_client: Some(ClientId(1)),
        channel: "titles.preview".to_string(),
        payload:
            json!({"old_outline": ["a"], "new_outline": ["G", "  a"], "moves": 1, "groups": 1})
                .to_string()
                .into_bytes(),
    }));
    host.settle();
    let screen = host.render(panel(), 80, PREVIEW_ROWS);
    assert!(screen.text().contains("Moves: 1, new groups: 1"));
}

/// REQ: F-043
#[test]
fn f_043_preview_rendering_the_same_proposal_twice_is_identical() {
    let mut host = host_with_proposal();
    let first = host.render(panel(), 80, PREVIEW_ROWS).text();
    let second = host.render(panel(), 80, PREVIEW_ROWS).text();
    assert_eq!(first, second);
}

/// REQ: F-043
#[test]
fn f_043_the_client_panel_needs_only_the_ui_panels_grant_for_its_view() {
    let manifest = manifest_value();
    let grants = manifest["capabilities"]["client"]
        .as_array()
        .expect("grants");
    assert!(grants.iter().any(|grant| grant == "ui.panels"));
}
