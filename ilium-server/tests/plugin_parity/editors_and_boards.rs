//! Baseline scenarios of src/docs/editors-and-boards.md and panes-and-layout.md: F-019 editor
//! panes and F-024 Kanban boards stay in the kernel until the phase-7 plugins (COR-050).

use ilium_plugin_api::generated::types::{
    EditorOpenSpec, EditorPlacement, FsPath, Intent, NodeClass, Outcome,
};
use ilium_plugin_api::kernel::KernelFacade;

use crate::common::*;

fn open_editor(
    kernel: &ilium_server::plugin_host::ServerKernel,
    path: &str,
    line: Option<u32>,
) -> Outcome {
    apply(
        kernel,
        Intent::EditorOpen(EditorOpenSpec {
            path: FsPath {
                scope: "project".to_owned(),
                path: path.to_owned(),
            },
            line,
            placement: EditorPlacement::NewPane,
        }),
    )
}

/// REQ: F-019, COR-001
#[test]
fn f_019_opening_a_file_creates_an_editor_pane_titled_with_the_file_name() {
    in_both_modes(|kernel| {
        kernel.write_project_file("src/main.rs", "fn main() {}\n");
        let pane = created(open_editor(kernel, "src/main.rs", None));
        let view = node(kernel, pane);
        assert_eq!(view.class, NodeClass::Pane);
        assert_eq!(view.content_kind.as_deref(), Some("ilium.editor"));
        assert_eq!(view.title, "main.rs");
    });
}

/// REQ: F-019, COR-001
#[test]
fn f_019_typed_text_is_saved_to_the_file_on_save() {
    in_both_modes(|kernel| {
        kernel.write_project_file("notes.txt", "one\n");
        let pane = created(open_editor(kernel, "notes.txt", None));
        kernel.editor_type(pane, "zero ");
        kernel.editor_save(pane);
        assert_eq!(
            kernel.read_project_file("notes.txt").as_deref(),
            Some("zero one\n")
        );
    });
}

/// REQ: F-019, COR-001
#[test]
fn f_019_unsaved_edits_do_not_touch_the_file() {
    in_both_modes(|kernel| {
        kernel.write_project_file("notes.txt", "one\n");
        let pane = created(open_editor(kernel, "notes.txt", None));
        kernel.editor_type(pane, "zero ");
        assert_eq!(
            kernel.read_project_file("notes.txt").as_deref(),
            Some("one\n")
        );
    });
}

/// REQ: F-019, COR-001
#[test]
fn f_019_open_at_a_line_places_the_cursor_on_that_line() {
    in_both_modes(|kernel| {
        kernel.write_project_file("long.txt", "a\nb\nc\nd\n");
        let pane = created(open_editor(kernel, "long.txt", Some(3)));
        assert_eq!(kernel.editor_cursor_line(pane), Some(3));
    });
}

/// REQ: F-019, COR-001
#[test]
fn f_019_two_editor_panes_on_one_tree_are_independent_nodes() {
    in_both_modes(|kernel| {
        kernel.write_project_file("a.txt", "a");
        kernel.write_project_file("b.txt", "b");
        let first = created(open_editor(kernel, "a.txt", None));
        let second = created(open_editor(kernel, "b.txt", None));
        assert_ne!(first, second);
        assert!(kernel.tree().node(first).is_some() && kernel.tree().node(second).is_some());
    });
}

/// REQ: F-024, COR-001
#[test]
fn f_024_a_markdown_board_is_a_pane_and_its_file_exists() {
    in_both_modes(|kernel| {
        let pane = kernel.create_board("markdown", "board.md");
        assert_eq!(
            node(kernel, pane).content_kind.as_deref(),
            Some("ilium.board")
        );
        assert!(kernel.read_project_file("board.md").is_some());
    });
}

/// REQ: F-024, COR-001
#[test]
fn f_024_a_folder_board_creates_a_directory_of_card_files() {
    in_both_modes(|kernel| {
        let pane = kernel.create_board("folder", "cards");
        assert_eq!(
            node(kernel, pane).content_kind.as_deref(),
            Some("ilium.board")
        );
        assert!(kernel.project_dir_exists("cards"));
    });
}

/// REQ: F-024, COR-001
#[test]
fn f_024_cards_survive_closing_the_board_pane() {
    in_both_modes(|kernel| {
        let pane = kernel.create_board("markdown", "board.md");
        kernel.board_add_card(pane, "To do", "write parity tests");
        close(kernel, pane);
        let file = kernel
            .read_project_file("board.md")
            .expect("board file stays");
        assert!(file.contains("write parity tests"), "{file:?}");
    });
}

/// REQ: F-024, COR-001
#[test]
fn f_024_a_card_is_added_to_the_named_column_only() {
    in_both_modes(|kernel| {
        let pane = kernel.create_board("markdown", "board.md");
        kernel.board_add_card(pane, "To do", "first");
        assert_eq!(kernel.board_column(pane, "To do"), vec!["first".to_owned()]);
        assert!(kernel.board_column(pane, "Done").is_empty());
    });
}
