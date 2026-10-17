//! The search panel renders: the two page figures at 48 columns and the structure at 40, 80 and 120.

use ilium_plugin_sdk::testing::assert_golden;
use serde_json::{json, Value};

use crate::common::*;

fn grouped_hits() -> Value {
    json!([
        { "id": "7", "kind": "agent", "title": "claude: deploy script fix" },
        { "id": "8", "kind": "terminal", "title": "build shell" },
        { "id": "docs/deploy.md", "kind": "file", "title": "docs/deploy.md" },
        { "id": "ops/deploy.sh", "kind": "file", "title": "ops/deploy.sh" },
    ])
}

fn meaning_hits() -> Value {
    json!([
        { "id": "8", "kind": "meaning", "title": "build shell", "score": 0.82, "pane": "build shell" },
        { "id": "9", "kind": "meaning", "title": "claude: fix linker error", "score": 0.64, "pane": "claude" },
    ])
}

fn read_golden(name: &str) -> String {
    std::fs::read_to_string(golden_path(name)).expect("golden readable")
}

// ---- the page figures ---------------------------------------------------------------------

/// REQ: F-009
#[test]
fn doc_f_009_results_golden_equals_the_first_page_figure() {
    assert_eq!(
        read_golden("search_results_48.txt"),
        term_blocks(&page_text())[0]
    );
}

/// REQ: F-009
#[test]
fn doc_f_009_meaning_golden_equals_the_second_page_figure() {
    assert_eq!(
        read_golden("search_meaning_48.txt"),
        term_blocks(&page_text())[1]
    );
}

/// REQ: F-009
#[test]
fn doc_f_009_both_figures_are_titled_frames_48_columns_wide() {
    for figure in term_blocks(&page_text()) {
        assert!(figure
            .lines()
            .next()
            .expect("line")
            .starts_with("╭─ Workspace search"));
        assert!(figure.lines().last().expect("line").starts_with('╰'));
        assert!(
            figure.lines().all(|line| line.chars().count() == 48),
            "{figure}"
        );
    }
}

/// REQ: F-009
#[test]
fn search_009_grouped_results_render_equals_the_page_figure_at_48_columns() {
    let mut host = client_showing("deploy", grouped_hits());
    let screen = host.render(panel(), 48, 12);
    assert_golden(&golden_path("search_results_48.txt"), &screen.text());
}

/// REQ: F-009
#[test]
fn search_009_meaning_results_render_equals_the_page_figure_at_48_columns() {
    let mut host = client_showing("why did the build fail", meaning_hits());
    let screen = host.render(panel(), 48, 10);
    assert_golden(&golden_path("search_meaning_48.txt"), &screen.text());
}

// ---- structure at three widths ------------------------------------------------------------

fn render_at(columns: u16, rows: u16) -> String {
    let mut host = client_showing("deploy", grouped_hits());
    host.render(panel(), columns, rows).text()
}

fn assert_grouped_structure(text: &str, columns: u16) {
    assert!(
        text.lines()
            .next()
            .expect("first row")
            .starts_with("╭─ Workspace search"),
        "{text}"
    );
    assert!(text.contains("Search deploy"), "{text}");
    for heading in ["Agents", "Terminals", "Files"] {
        assert!(text.contains(heading), "{heading} missing in {text}");
    }
    let agents = text.find("Agents").expect("agents");
    let terminals = text.find("Terminals").expect("terminals");
    let files = text.find("Files").expect("files");
    assert!(
        agents < terminals && terminals < files,
        "groups appear as Agents, Terminals, Files"
    );
    assert!(text
        .lines()
        .all(|line| line.chars().count() <= usize::from(columns)));
}

/// REQ: F-009
#[test]
fn search_009_panel_renders_the_grouped_tree_at_width_40() {
    assert_grouped_structure(&render_at(40, 14), 40);
}

/// REQ: F-009
#[test]
fn search_009_panel_renders_the_grouped_tree_at_width_80() {
    assert_grouped_structure(&render_at(80, 14), 80);
}

/// REQ: F-009
#[test]
fn search_009_panel_renders_the_grouped_tree_at_width_120() {
    assert_grouped_structure(&render_at(120, 14), 120);
}

/// REQ: F-009
#[test]
fn search_009_the_first_result_row_is_marked_with_the_selection_arrow() {
    let text = render_at(80, 14);
    let line = text
        .lines()
        .find(|line| line.contains("claude: deploy script fix"))
        .expect("row");
    assert!(line.contains("▸"), "{line}");
    let other = text
        .lines()
        .find(|line| line.contains("build shell"))
        .expect("row");
    assert!(!other.contains("▸"), "{other}");
}

/// REQ: F-009
#[test]
fn search_009_the_panel_without_a_query_shows_the_empty_field_and_no_groups() {
    let mut host = activated_client();
    host.deliver(vec![open_command_event()]);
    let text = host.render(panel(), 80, 14).text();
    assert!(text.contains("Search"), "{text}");
    for heading in ["Agents", "Terminals", "Files", "Meaning"] {
        assert!(
            !text.contains(heading),
            "{heading} shown without a query: {text}"
        );
    }
}

/// REQ: F-009
#[test]
fn search_009_meaning_hits_show_their_score_with_two_decimals_and_their_pane() {
    let mut host = client_showing("why did the build fail", meaning_hits());
    let text = host.render(panel(), 80, 14).text();
    assert!(text.contains("score 0.82"), "{text}");
    assert!(text.contains("pane: build shell"), "{text}");
    assert!(text.contains("score 0.64"), "{text}");
    assert!(text.contains("pane: claude"), "{text}");
}

/// REQ: F-009
#[test]
fn search_009_meaning_group_is_headed_meaning() {
    let mut host = client_showing("why did the build fail", meaning_hits());
    let text = host.render(panel(), 80, 14).text();
    assert!(text.contains("Meaning"), "{text}");
    assert!(!text.contains("Agents"));
}

/// REQ: F-009, PORT:ilium-client/src/search_ui.rs::full_screen_render_shows_live_query_and_places_the_native_cursor
#[test]
fn port_search_ui_render_shows_the_live_query_in_the_field_at_48_columns() {
    let mut host = client_showing("needle", json!([]));
    let screen = host.render(panel(), 48, 18);
    assert!(screen.text().contains("needle"));
    let (row, _) = screen.find("needle").expect("query on screen");
    assert_eq!(
        row, 1,
        "the query field sits on the first row inside the frame"
    );
}

/// REQ: F-009, PORT:ilium-client/src/search_ui.rs::full_screen_render_shows_object_metadata_and_highlighted_match
#[test]
fn port_search_ui_render_shows_title_last_command_and_the_match_once() {
    let hits = json!([{
        "id": "3", "kind": "agent", "title": "Investigate checkout",
        "last_command": "git status", "before": "before ", "matched": "needle", "after": " after"
    }]);
    let mut host = client_showing("needle", hits);
    let text = host.render(panel(), 100, 28).text();
    assert_eq!(text.matches("Workspace search").count(), 1);
    assert!(text.contains("Investigate checkout"), "{text}");
    assert!(text.contains("last command: git status"), "{text}");
    assert!(text.contains("before needle after"), "{text}");
}

/// REQ: F-009, PORT:ilium-client/src/search_ui.rs::search_frames_and_page_sizes_use_the_same_inner_geometry
#[test]
fn port_search_ui_frames_stay_inside_the_area_at_every_size() {
    for (columns, rows) in [(120u16, 40u16), (80, 24), (40, 12), (12, 6), (10, 3)] {
        let mut host = client_showing("deploy", grouped_hits());
        let screen = host.render(panel(), columns, rows);
        let text = screen.text();
        assert!(
            text.lines().count() <= usize::from(rows),
            "{columns}x{rows}: {text}"
        );
        assert!(
            text.lines()
                .all(|line| line.chars().count() <= usize::from(columns)),
            "{columns}x{rows}"
        );
    }
}

/// REQ: F-009, PORT:ilium-client/src/search_ui.rs::overflowing_results_reserve_a_separate_scrollbar_column
#[test]
fn port_search_ui_overflowing_results_reserve_a_separate_scrollbar_column() {
    let many: Vec<Value> = (0..30)
        .map(|index| json!({ "id": format!("f{index}"), "kind": "file", "title": format!("file-{index:02}.md") }))
        .collect();
    let mut host = client_showing("file", Value::Array(many));
    let screen = host.render(panel(), 40, 12);
    // Column 38 is the cell inside the right border (40 columns, border at 39): it belongs to
    // the scrollbar, so no title character may reach it.
    for row in 3..10u16 {
        let line = screen.row(row);
        let cells: Vec<char> = line.chars().collect();
        assert!(cells.len() >= 40, "{line:?}");
        assert!(
            !cells[38].is_alphanumeric(),
            "text runs into the scrollbar column: {line:?}"
        );
    }
    assert!(screen.text().contains("file-00.md"));
}

/// REQ: F-009, PORT:ilium-client/src/search_ui.rs::overflowing_results_reserve_a_separate_scrollbar_column
#[test]
fn port_search_ui_few_results_draw_no_scrollbar() {
    let mut host = client_showing(
        "file",
        json!([{ "id": "f1", "kind": "file", "title": "one.md" }]),
    );
    let with_one = host.render(panel(), 40, 12).text();
    let many: Vec<Value> = (0..30)
        .map(|index| json!({ "id": format!("f{index}"), "kind": "file", "title": format!("file-{index:02}.md") }))
        .collect();
    let mut crowded = client_showing("file", Value::Array(many));
    let with_many = crowded.render(panel(), 40, 12).text();
    let bar = ['█', '▐', '▌', '┃', '│', '║'];
    let right_column = |text: &str| -> Vec<char> {
        text.lines()
            .skip(2)
            .take(8)
            .filter_map(|line| line.chars().nth(38))
            .collect()
    };
    assert!(
        right_column(&with_one).iter().all(|symbol| *symbol == ' '),
        "{with_one}"
    );
    assert!(
        right_column(&with_many)
            .iter()
            .any(|symbol| bar.contains(symbol)),
        "{with_many}"
    );
}
