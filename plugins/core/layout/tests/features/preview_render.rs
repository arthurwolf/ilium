//! "Layout preview panel": golden at 48 columns (the page figure) and width invariants at 40, 80, 120.

use ilium_plugin_sdk::testing::{assert_golden, FakeHost};
use ilium_plugin_sdk::types::{Event, NodeId, PaneResizedEvent, ViewTarget};

use crate::common::*;

const GOLDEN_48: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/features/golden/layout_preview_48.txt"
);
const SIZE_ROW: &str = "80x24 | 61x24 | 80x12 | 61x12";
const PREVIEW_ROWS: u16 = 7;

fn panel() -> ViewTarget {
    ViewTarget::Panel(PANEL.to_string())
}

/// The four panes of the page figure: api, main.rs, Board, web; api is focused.
fn host_with_four_panes() -> FakeHost {
    let mut host = activated_client();
    let group = host.tree().add(ilium_plugin_sdk::testing::TreeNode {
        id: 0,
        parent: None,
        name: "group".to_string(),
        class: ilium_plugin_sdk::types::NodeClass::Container,
        content_kind: None,
        agent: None,
    });
    let api = host
        .tree()
        .add(pane_node("api", "ilium.terminal", Some(group)));
    let editor = host
        .tree()
        .add(pane_node("main.rs", "ilium.editor", Some(group)));
    let board = host
        .tree()
        .add(pane_node("Board", "ilium.board", Some(group)));
    let web = host
        .tree()
        .add(pane_node("web", "ilium.terminal", Some(group)));
    let sizes = [
        (api, 80, 24),
        (editor, 61, 24),
        (board, 80, 12),
        (web, 61, 12),
    ];
    let events: Vec<Event> = sizes
        .iter()
        .map(|(node, columns, rows)| {
            Event::PaneResized(PaneResizedEvent {
                node: NodeId(*node),
                columns: *columns,
                rows: *rows,
            })
        })
        .collect();
    host.deliver(events);
    host.deliver_one(Event::PaneFocused(Some(NodeId(api))));
    host.settle();
    host
}

/// REQ: F-003
#[test]
fn doc_f_003_golden_file_equals_the_preview_figure_of_the_page() {
    let figures = term_blocks(&page_text());
    let golden = std::fs::read_to_string(GOLDEN_48).expect("golden file exists");
    assert_eq!(golden, figures[1]);
}

/// REQ: F-003
#[test]
fn f_003_preview_panel_at_48_columns_matches_the_page_figure() {
    let mut host = host_with_four_panes();
    let screen = host.render(panel(), 48, PREVIEW_ROWS);
    assert_golden(GOLDEN_48, &screen.text());
}

/// REQ: F-003
#[test]
fn f_003_preview_panel_is_a_titled_frame_at_every_width() {
    for width in [40u16, 80, 120] {
        let mut host = host_with_four_panes();
        let screen = host.render(panel(), width, PREVIEW_ROWS);
        let text = screen.text();
        let lines: Vec<&str> = text.split('\n').collect();
        assert_eq!(lines.len(), PREVIEW_ROWS as usize, "width {width}");
        assert!(lines[0].starts_with("╭─ Layout "), "width {width}");
        assert!(lines[0].ends_with('╮'), "width {width}");
        assert!(lines[lines.len() - 1].starts_with('╰'), "width {width}");
        assert!(lines[lines.len() - 1].ends_with('╯'), "width {width}");
        for line in &lines {
            assert_eq!(
                line.chars().count(),
                width as usize,
                "width {width}: {line}"
            );
        }
    }
}

/// REQ: F-003
#[test]
fn f_003_preview_panel_lists_each_pane_size_in_the_last_inner_row_at_every_width() {
    for width in [40u16, 80, 120] {
        let mut host = host_with_four_panes();
        let screen = host.render(panel(), width, PREVIEW_ROWS);
        assert!(
            screen.row(PREVIEW_ROWS - 2).contains(SIZE_ROW),
            "width {width}"
        );
    }
}

/// REQ: F-003
#[test]
fn f_003_preview_panel_shows_the_pane_names_at_every_width() {
    for width in [40u16, 80, 120] {
        let mut host = host_with_four_panes();
        let text = host.render(panel(), width, PREVIEW_ROWS).text();
        for name in ["api", "main.rs", "Board", "web"] {
            assert!(text.contains(name), "width {width}: {name}");
        }
    }
}

/// REQ: F-003
#[test]
fn f_003_preview_panel_draws_a_separator_rule_between_the_rows() {
    let mut host = host_with_four_panes();
    let screen = host.render(panel(), 48, PREVIEW_ROWS);
    assert!(screen.text().contains("────────"));
    assert!(screen.text().contains('╋'));
}

/// REQ: F-003
#[test]
fn f_003_preview_panel_marks_the_focused_pane() {
    let mut host = host_with_four_panes();
    let text = host.render(panel(), 48, PREVIEW_ROWS).text();
    assert!(text.contains("▌api"));
}

/// REQ: F-003
#[test]
fn f_003_preview_render_is_pure() {
    let mut host = host_with_four_panes();
    let first = host.render(panel(), 80, PREVIEW_ROWS);
    let second = host.render(panel(), 80, PREVIEW_ROWS);
    assert_eq!(first, second);
}

/// REQ: F-003
#[test]
fn f_003_preview_with_no_panes_still_draws_the_frame() {
    let mut host = activated_client();
    let text = host.render(panel(), 48, PREVIEW_ROWS).text();
    assert!(text.starts_with("╭─ Layout "));
}
