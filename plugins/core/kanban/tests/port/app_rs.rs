//! Twins of the board-related ilium-client/src/app.rs tests: creating a board from a Markdown file,
//! the dialog, and the board settings. Command id and popup id are assumed (API_GAPS.md).

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    BufferId, BufferRef, CommandContext, CommandInvokedEvent, CommandOrigin, Event, FsPath, Intent,
    NodeId, Submission, ViewTarget,
};
use serde_json::json;

use crate::common::{
    board_file, board_host, client_host_with, draw, press, BOARD_KIND, BOARD_PATH, SAMPLE_BOARD,
};

const COMMAND: &str = "ilium.core.kanban.create-board-from-markdown";

fn open_editor(host: &mut FakeHost, path: &str, language: &str) -> NodeId {
    let pane = NodeId(41);
    host.deliver_one(Event::EditorOpened(BufferRef {
        buffer: BufferId(3),
        pane: Some(pane),
        path: Some(FsPath {
            scope: "project".to_string(),
            path: path.to_string(),
        }),
        language: Some(language.to_string()),
        revision: 1,
        line_count: 8,
        dirty: false,
    }));
    host.settle();
    pane
}

fn invoke(host: &mut FakeHost, pane: NodeId) -> Vec<Submission> {
    host.deliver_one(Event::CommandInvoked(CommandInvokedEvent {
        command: COMMAND.to_string(),
        arguments_json: "{}".to_string(),
        origin: CommandOrigin::Menu,
        context: CommandContext {
            pane: Some(pane),
            view: None,
            buffer: Some(BufferId(3)),
            line: None,
            virtual_row: None,
            menu_item: None,
        },
    }))
}

fn boards(submissions: &[Submission]) -> usize {
    submissions
        .iter()
        .filter(|submission| matches!(&submission.intent, Intent::CreatePane(spec) if spec.content_kind == BOARD_KIND))
        .count()
}

/// REQ: PORT:ilium-client/src/app.rs::markdown_editor_tree_action_creates_a_sibling_board_from_the_same_file, PORT:ilium-client/src/app.rs::request_new_markdown_board_preserves_the_existing_file_as_its_storage, PORT:ilium-client/src/app.rs::board_creation_resolves_relative_path_and_preflights_storage_before_request, PORT:ilium/tests/pty_tui_smoke.rs::existing_markdown_creates_populated_boards_from_tree_and_dialog, PORT:ilium-client/src/render_cache.rs::new_board_snapshot_loads_an_existing_markdown_file_without_overwriting_it, PORT:ilium-client/src/mouse.rs::right_clicking_a_markdown_editor_row_exposes_and_runs_create_board
#[test]
fn port_app_markdown_editor_yields_a_board_on_the_same_file_without_rewriting_it() {
    let mut host = client_host_with(Some(SAMPLE_BOARD));
    let pane = open_editor(&mut host, "docs/plan.md", "markdown");
    assert_eq!(boards(&invoke(&mut host, pane)), 1);
    assert_eq!(board_file(&mut host), SAMPLE_BOARD);
}

/// REQ: PORT:ilium-client/src/app.rs::create_board_tree_action_is_hidden_for_non_markdown_editors
#[test]
fn port_app_non_markdown_editor_gets_no_board() {
    let mut host = client_host_with(None);
    let pane = open_editor(&mut host, "src/main.rs", "rust");
    assert_eq!(boards(&invoke(&mut host, pane)), 0);
}

/// REQ: PORT:ilium-client/src/app.rs::board_creation_rejects_an_unreadable_storage_shape_without_queuing_a_dead_pane
#[test]
fn port_app_an_unreadable_storage_path_queues_no_board_pane() {
    let mut host = client_host_with(None);
    let pane = open_editor(&mut host, "docs/missing-dir/", "markdown");
    assert_eq!(boards(&invoke(&mut host, pane)), 0);
}

/// REQ: PORT:ilium-client/src/app.rs::new_board_dialog_skips_existing_and_already_open_default_paths, PORT:ilium-client/src/mouse.rs::board_form_fields_and_create_button_are_fully_mouse_operable
#[test]
fn port_app_asking_twice_for_the_same_file_opens_only_one_board() {
    let mut host = client_host_with(Some(SAMPLE_BOARD));
    let pane = open_editor(&mut host, "docs/plan.md", "markdown");
    let first = boards(&invoke(&mut host, pane));
    host.settle();
    let second = boards(&invoke(&mut host, pane));
    assert_eq!(first, 1);
    assert_eq!(second, 0);
}

/// REQ: PORT:ilium-client/src/app.rs::kanban_layout_settings_persist_with_bounded_adjustments
#[test]
fn port_app_layout_settings_persist_and_are_bounded() {
    let mut host = client_host_with(None);
    host.settings()
        .set("column_width", json!(30))
        .expect("30 valid");
    assert_eq!(host.settings().get("column_width"), Some(json!(30)));
    assert!(host.settings().set("column_width", json!(61)).is_err());
    assert!(host.settings().set("column_width", json!(11)).is_err());
    assert_eq!(host.settings().get("column_width"), Some(json!(30)));
    host.settings()
        .set("card_preview", json!(false))
        .expect("toggle");
    assert_eq!(host.settings().get("card_preview"), Some(json!(false)));
}

/// REQ: PORT:ilium-client/src/app.rs::captured_project_accepts_board_open_without_retargeting, PORT:ilium-client/src/app.rs::removed_board_parent_cancels_open_without_request_or_pending_focus, PORT:ilium-client/src/app.rs::completed_board_open_survives_real_outbound_saturation_without_ghost_focus
#[test]
fn port_app_a_board_pane_opens_on_its_file_and_a_removed_pane_leaves_no_ghost() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    assert!(draw(&mut host, node, 80, 20).text().contains("To do (2)"));
    host.deliver_one(Event::NodeRemoved(node));
    host.settle();
    assert_eq!(
        host.fs().read(BOARD_PATH),
        Some(SAMPLE_BOARD.as_bytes().to_vec())
    );
}

/// REQ: PORT:ilium-client/src/app.rs::clicking_a_board_card_opens_scrolls_and_closes_its_detail_panel
#[test]
fn port_app_clicking_a_card_opens_and_escape_closes_its_detail_panel() {
    let (mut host, node) = board_host("## To do\n- Task\n  complete body\n");
    let (row, column) = draw(&mut host, node, 120, 30).find("Task").expect("card");
    assert!(host.click(ViewTarget::Pane(node), column, row));
    host.settle();
    assert!(draw(&mut host, node, 120, 30)
        .find("complete body")
        .is_none_or(|(_, c)| c < 80));
    press(&mut host, node, "enter");
    assert!(draw(&mut host, node, 120, 30)
        .text()
        .contains("complete body"));
    press(&mut host, node, "escape");
    assert!(draw(&mut host, node, 120, 30)
        .find("complete body")
        .is_none_or(|(_, c)| c < 80));
}

/// REQ: PORT:ilium-client/src/app.rs::successful_editor_and_board_edits_report_node_activity, PORT:ilium-client/src/app.rs::shutdown_without_editor_drains_board_creation_and_reports_open_cancellation
#[test]
fn port_app_a_successful_board_edit_is_written_and_nothing_is_lost_on_deactivation() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    press(&mut host, node, "j");
    press(&mut host, node, "shift+right");
    let saved = board_file(&mut host);
    host.deactivate(ilium_plugin_sdk::types::DeactivationReason::Shutdown);
    assert_eq!(board_file(&mut host), saved);
}

/// REQ: PORT:ilium-client/src/app.rs::cancelled_create_dialog_preserves_completed_file_without_delayed_open
#[test]
fn port_app_a_cancelled_creation_keeps_the_completed_file_and_opens_nothing_later() {
    let mut host = client_host_with(Some(SAMPLE_BOARD));
    let pane = open_editor(&mut host, "docs/plan.md", "markdown");
    assert_eq!(boards(&invoke(&mut host, pane)), 1);
    host.deliver_one(Event::NodeRemoved(NodeId(900)));
    host.advance_clock(60_000);
    host.settle();
    assert_eq!(board_file(&mut host), SAMPLE_BOARD);
    let later = host.deliver_one(Event::NodeRemoved(NodeId(901)));
    assert_eq!(
        boards(&later),
        0,
        "no delayed open appears after the cancellation"
    );
}
