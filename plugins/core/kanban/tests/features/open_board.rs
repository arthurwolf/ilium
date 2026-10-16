//! F-025 and the picker: a board created from a Markdown file, and the "Open board from file" dialog.
//!
//! The command id `ilium.core.kanban.create-board-from-markdown` and the popup id `open-board` are
//! assumed (API_GAPS.md): the manifest declares no command or menu item.

use ilium_plugin_sdk::types::{
    BufferId, BufferRef, CommandContext, CommandInvokedEvent, CommandOrigin, Event, FsPath, Intent, NodeId, Outcome,
    Submission, ViewTarget,
};

use crate::common::{BOARD_KIND, BOARD_PATH, SAMPLE_BOARD, client_host_with, rows_of};

const COMMAND: &str = "ilium.core.kanban.create-board-from-markdown";

fn open_editor(host: &mut ilium_plugin_sdk::testing::FakeHost, path: &str, language: &str, dirty: bool) -> NodeId {
    let pane = NodeId(41);
    host.deliver_one(Event::EditorOpened(BufferRef {
        buffer: BufferId(3),
        pane: Some(pane),
        path: Some(FsPath { scope: "project".to_string(), path: path.to_string() }),
        language: Some(language.to_string()),
        revision: 1,
        line_count: 8,
        dirty,
    }));
    host.settle();
    pane
}

fn invoke(host: &mut ilium_plugin_sdk::testing::FakeHost, pane: NodeId) -> Vec<Submission> {
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

fn created_boards(submissions: &[Submission]) -> usize {
    submissions
        .iter()
        .filter(|submission| matches!(&submission.intent, Intent::CreatePane(spec) if spec.content_kind == BOARD_KIND))
        .count()
}

/// REQ: F-025
#[test]
fn create_025_a_markdown_editor_yields_one_board_pane_on_the_same_file() {
    let mut host = client_host_with(Some(SAMPLE_BOARD));
    let pane = open_editor(&mut host, "docs/plan.md", "markdown", false);
    assert_eq!(created_boards(&invoke(&mut host, pane)), 1);
}

/// REQ: F-025
#[test]
fn create_025_the_board_opens_in_the_group_of_the_editor() {
    let mut host = client_host_with(Some(SAMPLE_BOARD));
    let pane = open_editor(&mut host, "docs/plan.md", "markdown", false);
    let submissions = invoke(&mut host, pane);
    let spec = submissions
        .iter()
        .find_map(|submission| match &submission.intent {
            Intent::CreatePane(spec) if spec.content_kind == BOARD_KIND => Some(spec.clone()),
            _ => None,
        })
        .expect("board pane requested");
    assert!(spec.parent.is_some(), "the board shares the group of the editor");
}

/// REQ: F-025
#[test]
fn create_025_the_existing_file_is_not_rewritten_at_creation() {
    let mut host = client_host_with(Some("Intro prose.\n\n## To do\n* star card\n"));
    let pane = open_editor(&mut host, "docs/plan.md", "markdown", false);
    invoke(&mut host, pane);
    host.settle();
    assert_eq!(host.fs().read(BOARD_PATH), Some(b"Intro prose.\n\n## To do\n* star card\n".to_vec()));
}

/// REQ: F-025
#[test]
fn create_025_a_non_markdown_editor_gets_no_board() {
    let mut host = client_host_with(None);
    host.fs().write("project:notes.txt", b"- not a board\n");
    let pane = open_editor(&mut host, "notes.txt", "plaintext", false);
    assert_eq!(created_boards(&invoke(&mut host, pane)), 0);
}

/// REQ: F-025
#[test]
fn create_025_the_markdown_extension_is_what_counts_not_the_language_name() {
    let mut host = client_host_with(None);
    host.fs().write("project:docs/readme.markdown", b"## L\n- a\n");
    let pane = open_editor(&mut host, "docs/readme.markdown", "markdown", false);
    assert_eq!(created_boards(&invoke(&mut host, pane)), 1);
}

/// REQ: F-025
#[test]
fn create_025_asking_twice_for_the_same_file_opens_only_one_board() {
    let mut host = client_host_with(Some(SAMPLE_BOARD));
    let pane = open_editor(&mut host, "docs/plan.md", "markdown", false);
    let first = created_boards(&invoke(&mut host, pane));
    host.settle();
    let second = created_boards(&invoke(&mut host, pane));
    assert_eq!((first, second), (1, 0));
}

/// REQ: F-025
#[test]
fn create_025_a_picker_is_started_from_the_dialog_for_markdown_files() {
    let mut host = client_host_with(None);
    host.render(ViewTarget::Popup("open-board".to_string()), 48, 8);
    host.click(ViewTarget::Popup("open-board".to_string()), 40, 1);
    host.settle();
    assert_eq!(host.pending_requests().len(), 1, "the Browse button starts one fs.pick request");
}

/// REQ: F-025
#[test]
fn create_025_cancelling_the_picker_opens_no_board() {
    let mut host = client_host_with(None);
    host.render(ViewTarget::Popup("open-board".to_string()), 48, 8);
    host.click(ViewTarget::Popup("open-board".to_string()), 40, 1);
    host.settle();
    let request = host.pending_requests().into_iter().next().expect("pick request");
    host.complete(request, Outcome::Cancelled);
    host.settle();
    assert_eq!(created_boards(&host.submitted()), 0);
    assert!(host.failures().is_empty());
}

/// REQ: F-025
#[test]
fn create_025_the_dialog_shows_the_file_field_the_width_and_the_buttons() {
    let mut host = client_host_with(None);
    let screen = host.render(ViewTarget::Popup("open-board".to_string()), 48, 6);
    let rows = rows_of(&screen);
    assert!(rows.iter().any(|row| row.contains("Open board from file")), "{rows:?}");
    assert!(rows.iter().any(|row| row.contains("Markdown file") && row.contains("[ Browse ]")), "{rows:?}");
    assert!(rows.iter().any(|row| row.contains("Column width") && row.contains("24")), "{rows:?}");
    assert!(rows.iter().any(|row| row.contains("[ Open ]") && row.contains("[ Cancel ]")), "{rows:?}");
}

/// REQ: F-025
#[test]
fn create_025_the_dialog_width_row_follows_the_column_width_setting() {
    let mut host = client_host_with(None);
    host.settings().set("column_width", serde_json::json!(36)).expect("valid");
    let screen = host.render(ViewTarget::Popup("open-board".to_string()), 48, 6);
    assert!(rows_of(&screen).iter().any(|row| row.contains("Column width") && row.contains("36")));
}

/// REQ: F-025
#[test]
fn create_025_the_dialog_rows_are_joined_by_dotted_leaders() {
    let mut host = client_host_with(None);
    let screen = host.render(ViewTarget::Popup("open-board".to_string()), 48, 6);
    let rows = rows_of(&screen);
    let width_row = rows.iter().find(|row| row.contains("Column width")).expect("row");
    assert!(width_row.contains("…"), "leaders are made of U+2026: {width_row}");
    assert!(width_row.contains(" … ") || width_row.contains(" ……"), "one blank cell before the dots: {width_row}");
}
