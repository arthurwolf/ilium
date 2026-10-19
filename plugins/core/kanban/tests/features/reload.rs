//! Reload when the file changes, the guarded write and its retries (page samples "Reload when the
//! file changes" and "Read and write the Markdown file safely").

use ilium_plugin_sdk::types::{BufferId, EditorSavedEvent, ErrorCode, Event, FsPath, PluginError};

use crate::common::{
    board_file, board_host, client_host_with, draw, press, sections, select_first_header,
    BOARD_PATH, SAMPLE_BOARD,
};

const CHANGED: &str = "# Board\n\n## To do\n- Brand new card\n\n## Doing\n\n## Done\n";

fn conflict() -> PluginError {
    PluginError {
        code: ErrorCode::Conflict,
        message: "revision changed".to_string(),
        retry_after_ms: None,
    }
}

fn move_first_card_right(
    host: &mut ilium_plugin_sdk::testing::FakeHost,
    node: ilium_plugin_sdk::types::NodeId,
) {
    select_first_header(host, node);
    press(host, node, "j");
    press(host, node, "shift+right");
}

/// REQ: F-024
#[test]
fn reload_024_an_outside_change_to_the_file_redraws_the_board() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    host.fs().write(BOARD_PATH, CHANGED.as_bytes());
    host.settle();
    let text = draw(&mut host, node, 80, 20).text();
    assert!(text.contains("Brand new card"), "{text}");
    assert!(!text.contains("Write docs"), "{text}");
}

/// REQ: F-024
#[test]
fn reload_024_the_reload_does_not_rewrite_the_file() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    host.fs().write(BOARD_PATH, CHANGED.as_bytes());
    host.settle();
    let _ = draw(&mut host, node, 80, 20);
    assert_eq!(board_file(&mut host), CHANGED);
}

/// REQ: F-024
#[test]
fn reload_024_a_second_outside_change_is_picked_up_too() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    host.fs().write(BOARD_PATH, CHANGED.as_bytes());
    host.settle();
    host.fs().write(BOARD_PATH, SAMPLE_BOARD.as_bytes());
    host.settle();
    assert!(draw(&mut host, node, 80, 20).text().contains("Write docs"));
}

/// REQ: F-024
#[test]
fn reload_024_a_change_to_another_file_leaves_the_board_alone() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    let before = draw(&mut host, node, 80, 20).text();
    host.fs().write("project:docs/other.md", b"# Other\n");
    host.settle();
    assert_eq!(draw(&mut host, node, 80, 20).text(), before);
}

/// REQ: F-024
#[test]
fn reload_024_editor_saved_for_the_board_path_reloads_the_board() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    host.fs().write(BOARD_PATH, CHANGED.as_bytes());
    host.deliver_one(Event::EditorSaved(EditorSavedEvent {
        buffer: BufferId(7),
        path: FsPath {
            scope: "project".to_string(),
            path: "docs/plan.md".to_string(),
        },
    }));
    host.settle();
    assert!(draw(&mut host, node, 80, 20)
        .text()
        .contains("Brand new card"));
}

/// REQ: F-024
#[test]
fn reload_024_editor_saved_for_another_path_does_not_reload() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    let before = draw(&mut host, node, 80, 20).text();
    host.deliver_one(Event::EditorSaved(EditorSavedEvent {
        buffer: BufferId(8),
        path: FsPath {
            scope: "project".to_string(),
            path: "docs/other.md".to_string(),
        },
    }));
    host.settle();
    assert_eq!(draw(&mut host, node, 80, 20).text(), before);
}

/// REQ: F-024
#[test]
fn reload_024_activation_watches_the_board_file_so_no_event_is_lost() {
    let mut host = client_host_with(Some(SAMPLE_BOARD));
    host.fs().write(BOARD_PATH, CHANGED.as_bytes());
    host.settle();
    assert!(host.failures().is_empty());
}

/// REQ: F-024
#[test]
fn guard_024_one_conflict_is_retried_and_the_move_is_written() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    host.inject_failure("fs.write-atomic", conflict());
    move_first_card_right(&mut host, node);
    assert!(sections(&board_file(&mut host))[1]
        .1
        .contains(&"Write docs".to_string()));
}

/// REQ: F-024
#[test]
fn guard_024_two_conflicts_are_retried_and_the_third_attempt_writes() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    host.inject_failure("fs.write-atomic", conflict());
    host.inject_failure("fs.write-atomic", conflict());
    move_first_card_right(&mut host, node);
    assert!(sections(&board_file(&mut host))[1]
        .1
        .contains(&"Write docs".to_string()));
}

/// REQ: F-024
#[test]
fn guard_024_three_conflicts_end_in_conflict_and_the_file_stays_untouched() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    for _ in 0..3 {
        host.inject_failure("fs.write-atomic", conflict());
    }
    move_first_card_right(&mut host, node);
    assert_eq!(board_file(&mut host), SAMPLE_BOARD);
}

/// REQ: F-024
#[test]
fn guard_024_after_exhausted_retries_the_next_move_works_again() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    for _ in 0..3 {
        host.inject_failure("fs.write-atomic", conflict());
    }
    move_first_card_right(&mut host, node);
    press(&mut host, node, "shift+right");
    assert_eq!(
        sections(&board_file(&mut host))
            .iter()
            .map(|(_, cards)| cards.len())
            .sum::<usize>(),
        4
    );
}

/// REQ: F-024
#[test]
fn guard_024_reads_with_a_revision_before_every_attempt() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    host.inject_failure("fs.write-atomic", conflict());
    move_first_card_right(&mut host, node);
    assert!(host.failures().is_empty());
}

/// REQ: F-024
#[test]
fn guard_024_a_board_of_exactly_512_kib_loads() {
    let head = "## L\n- t\n  ";
    let text = format!("{head}{}", "x".repeat(512 * 1024 - head.len()));
    let (mut host, node) = board_host(&text);
    assert!(draw(&mut host, node, 80, 20).text().contains("L (1)"));
}

/// REQ: F-024
#[test]
fn guard_024_a_board_of_512_kib_plus_one_byte_is_refused() {
    let head = "## L\n- t\n  ";
    let text = format!("{head}{}", "x".repeat(512 * 1024 + 1 - head.len()));
    let (mut host, node) = board_host(&text);
    assert!(!draw(&mut host, node, 80, 20).text().contains("L (1)"));
}

/// REQ: F-024
#[test]
fn guard_024_a_board_of_512_kib_minus_one_byte_loads() {
    let head = "## L\n- t\n  ";
    let text = format!("{head}{}", "x".repeat(512 * 1024 - 1 - head.len()));
    let (mut host, node) = board_host(&text);
    assert!(draw(&mut host, node, 80, 20).text().contains("L (1)"));
}
