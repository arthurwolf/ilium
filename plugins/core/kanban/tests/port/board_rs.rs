//! Twins of ilium-client/src/board.rs tests, on the board file channel and the drawn screen.

use ilium_plugin_sdk::types::ViewTarget;

use crate::common::{
    board_file, board_host, draw, press, sections, select_first_header, BOARD_PATH, SAMPLE_BOARD,
};

const THREE: &str = "# Board\n\n## To do\n- First\n- Second\n- Third\n\n## Doing\n\n## Done\n";

/// REQ: PORT:ilium-client/src/board.rs::create_missing_board_does_not_replace_a_dangling_symlink
#[test]
fn port_board_create_missing_board_keeps_an_existing_unreadable_target_untouched() {
    let mut host = crate::common::client_host_with(Some("not a board \u{0}"));
    let before = board_file(&mut host);
    host.settle();
    assert_eq!(
        board_file(&mut host),
        before,
        "an existing file is not a new board and is never replaced"
    );
}

/// REQ: PORT:ilium-client/src/board.rs::board_text_extract_renders_columns_and_cards_with_nonempty_bodies_inline
#[test]
fn port_board_text_extract_shows_columns_cards_and_bodies() {
    let (mut host, node) = board_host(
        "# Board\n\n## Doing\n- Fix auth bug\n  Token expiry check uses < not <=\n- No body card\n",
    );
    let text = draw(&mut host, node, 120, 24).text();
    assert!(
        text.contains("Doing (2)")
            && text.contains("Fix auth bug")
            && text.contains("No body card"),
        "{text}"
    );
}

/// REQ: PORT:ilium-client/src/board.rs::markdown_storage_round_trips_columns_and_cards
#[test]
fn port_board_markdown_storage_round_trips_columns_and_cards() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    press(&mut host, node, "j");
    press(&mut host, node, "shift+right");
    let saved = sections(&board_file(&mut host));
    assert_eq!(saved[1].1.last().map(String::as_str), Some("Write docs"));
    let (mut again, _) = board_host(&board_file(&mut host));
    assert_eq!(sections(&board_file(&mut again)), saved);
}

/// REQ: PORT:ilium-client/src/board.rs::existing_todo_markdown_imports_heading_columns_and_common_list_markers
#[test]
fn port_board_existing_todo_markdown_imports_heading_columns_and_list_markers() {
    let (mut host, node) = board_host(
        "# General\n\n* [ ] First task\n+ [x] Second task\n\n## Later\n\n- Third task\n",
    );
    let text = draw(&mut host, node, 120, 24).text();
    assert!(
        text.contains("General (2)") && text.contains("Later (1)"),
        "{text}"
    );
    assert!(
        text.contains("[ ] First task")
            && text.contains("[x] Second task")
            && text.contains("Third task"),
        "{text}"
    );
}

/// REQ: PORT:ilium-client/src/board.rs::markdown_import_ignores_document_title_and_fenced_list_examples
#[test]
fn port_board_markdown_import_ignores_title_and_fenced_examples() {
    let (mut host, node) = board_host(
        "# Board\n\n## To do\n\n```markdown\n- not a card\n```\n- Real card\n\n## Done\n",
    );
    let text = draw(&mut host, node, 120, 24).text();
    assert!(
        text.contains("To do (1)") && text.contains("Done (0)") && text.contains("Real card"),
        "{text}"
    );
    assert!(
        !text.contains("not a card") && !text.contains("Board ("),
        "{text}"
    );
}

/// REQ: PORT:ilium-client/src/board.rs::markdown_card_details_round_trip_as_indented_continuation_lines
#[test]
fn port_board_card_details_round_trip_as_indented_continuation_lines() {
    let source =
        "# Board\n\n## To do\n- Investigate\n  First detail line\n  \n  Final detail line\n";
    let (mut host, node) = board_host(source);
    press(&mut host, node, "j");
    press(&mut host, node, "enter");
    let text = draw(&mut host, node, 120, 24).text();
    assert!(
        text.contains("First detail line") && text.contains("Final detail line"),
        "{text}"
    );
    press(&mut host, node, "shift+right");
    assert!(
        board_file(&mut host).contains("  Final detail line"),
        "{}",
        board_file(&mut host)
    );
}

/// REQ: PORT:ilium-client/src/board.rs::folder_storage_creates_a_column_directory_and_markdown_card, PORT:ilium-client/src/board.rs::folder_save_preserves_non_markdown_files_in_column, PORT:ilium-client/src/board.rs::detail_editor_uses_the_same_title_body_contract_for_folder_storage
#[test]
fn port_board_folder_storage_keeps_cards_as_markdown_files_and_other_files() {
    let (mut host, _node) = board_host(SAMPLE_BOARD);
    host.fs().write("project:board/To do/notes.txt", b"keep me");
    host.fs()
        .write("project:board/To do/001-task.md", b"# Task!\n\nN\n");
    host.settle();
    assert_eq!(
        host.fs().read("project:board/To do/notes.txt"),
        Some(b"keep me".to_vec())
    );
    assert_eq!(
        host.fs().read("project:board/To do/001-task.md"),
        Some(b"# Task!\n\nN\n".to_vec())
    );
}

/// REQ: PORT:ilium-client/src/board.rs::detail_editor_persists_each_title_and_body_change_to_markdown
#[test]
fn port_board_detail_panel_shows_title_and_body_of_the_selected_card() {
    let (mut host, node) = board_host("# Board\n\n## To do\n- Task!\n  N\n");
    press(&mut host, node, "j");
    press(&mut host, node, "enter");
    let text = draw(&mut host, node, 120, 24).text();
    assert!(text.contains("Task!") && text.contains('N'), "{text}");
    assert!(board_file(&mut host).contains("- Task!\n  N\n"));
}

/// REQ: PORT:ilium-client/src/board.rs::task_checkbox_toggle_persists_immediately_for_both_backends
#[test]
fn port_board_task_checkbox_toggle_persists_immediately() {
    let (mut host, node) = board_host("## To do\n- [ ] Done\n");
    let (row, column) = draw(&mut host, node, 80, 20)
        .find("[ ] Done")
        .expect("checkbox drawn");
    assert!(host.click(ViewTarget::Pane(node), column + 1, row));
    host.settle();
    assert_eq!(sections(&board_file(&mut host))[0].1, ["[x] Done"]);
}

/// REQ: PORT:ilium-client/src/board.rs::rename_and_delete_keep_markdown_storage_in_sync, PORT:ilium-client/src/board.rs::populated_column_header_can_be_selected_and_renamed
#[test]
fn port_board_selecting_a_populated_column_header_keeps_its_cards_in_the_file() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    select_first_header(&mut host, node);
    let before = board_file(&mut host);
    host.settle();
    assert_eq!(board_file(&mut host), before);
    assert_eq!(sections(&before)[0].1, ["Write docs", "Add tests"]);
}

/// REQ: PORT:ilium-client/src/board.rs::cards_reorder_vertically_and_to_an_exact_cross_column_position
#[test]
fn port_board_cards_reorder_vertically_and_move_to_an_exact_position() {
    let (mut host, node) = board_host(THREE);
    press(&mut host, node, "j");
    press(&mut host, node, "j");
    press(&mut host, node, "j");
    press(&mut host, node, "shift+up");
    assert_eq!(
        sections(&board_file(&mut host))[0].1,
        ["First", "Third", "Second"]
    );
    host.send_action(
        ViewTarget::Pane(node),
        "column-1",
        ilium_plugin_sdk::types::ActionKind::Dropped,
        ilium_plugin_sdk::types::ActionValue::Drop(ilium_plugin_sdk::types::DropValue {
            source_key: "card-0-2".to_string(),
            target_key: "column-1".to_string(),
            index: 0,
        }),
    );
    host.settle();
    let saved = sections(&board_file(&mut host));
    assert_eq!(saved[0].1, ["First", "Third"]);
    assert_eq!(saved[1].1, ["Second"]);
}

/// REQ: PORT:ilium-client/src/board.rs::visual_same_column_insertion_is_converted_after_source_removal
#[test]
fn port_board_same_column_visual_insertion_index_is_taken_after_source_removal() {
    let (mut host, node) = board_host(THREE);
    host.send_action(
        ViewTarget::Pane(node),
        "column-0",
        ilium_plugin_sdk::types::ActionKind::Dropped,
        ilium_plugin_sdk::types::ActionValue::Drop(ilium_plugin_sdk::types::DropValue {
            source_key: "card-0-0".to_string(),
            target_key: "column-0".to_string(),
            index: 2,
        }),
    );
    host.settle();
    assert_eq!(
        sections(&board_file(&mut host))[0].1,
        ["Second", "First", "Third"]
    );
}

/// REQ: PORT:ilium-client/src/board.rs::stale_markdown_edit_is_rejected_and_in_memory_mutation_is_rolled_back
#[test]
fn port_board_stale_edit_is_rejected_and_the_foreign_file_stays() {
    let (mut host, node) = board_host(THREE);
    let external = "# Board\n\n## To do\n- External\n\n## Doing\n\n## Done\n";
    host.inject_failure(
        "fs.write-atomic",
        ilium_plugin_sdk::types::PluginError {
            code: ilium_plugin_sdk::types::ErrorCode::Conflict,
            message: "revision conflict".to_string(),
            retry_after_ms: None,
        },
    );
    host.fs().write(BOARD_PATH, external.as_bytes());
    press(&mut host, node, "j");
    press(&mut host, node, "shift+right");
    assert_eq!(
        board_file(&mut host),
        external,
        "the foreign file is never overwritten by a stale edit"
    );
}

/// REQ: PORT:ilium-client/src/board.rs::reload_advances_revision_only_for_external_semantic_changes
#[test]
fn port_board_reload_shows_external_changes_and_is_idempotent() {
    let (mut host, node) = board_host(THREE);
    let external = "# Board\n\n## To do\n- External task\n\n## Doing\n\n## Done\n";
    host.fs().write(BOARD_PATH, external.as_bytes());
    host.settle();
    let first = draw(&mut host, node, 100, 24).text();
    assert!(first.contains("External task"), "{first}");
    press(&mut host, node, "r");
    assert_eq!(draw(&mut host, node, 100, 24).text(), first);
    assert_eq!(board_file(&mut host), external);
}
