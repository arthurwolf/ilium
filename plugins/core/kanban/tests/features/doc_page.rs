//! The page, the samples it prints and the user documentation the plugin must keep true (F-024, F-025).

use crate::common::{page_text, user_doc};

/// REQ: F-024
#[test]
fn doc_024_the_page_declares_the_plugin_id_and_the_client_side() {
    let page = page_text();
    assert!(page
        .contains("The Kanban plugin has the id `ilium.core.kanban` and runs on the client side."));
    assert!(page.contains(
        "It owns the board pane: a basic pane that draws cards in To do, Doing and Done columns."
    ));
}

/// REQ: F-024
#[test]
fn doc_024_the_page_lists_the_three_covered_behaviours() {
    let page = page_text();
    assert!(page.contains("- Kanban boards as a pane: columns, cards, a selected card"));
    assert!(page.contains(
        "- A board that is backed by a Markdown file, with automatic reload when the file changes"
    ));
    assert!(page.contains("- The Kanban Board settings tab: card preview and column width"));
}

/// REQ: F-024
#[test]
fn doc_024_the_pane_stays_kernel_native_and_the_plugin_never_touches_the_pty() {
    let page = page_text();
    assert!(
        page.contains("The pane itself, its place in the tree and its focus are kernel-native.")
    );
    assert!(page.contains("It never touches the PTY or the layout solver."));
}

/// REQ: F-025
#[test]
fn doc_025_the_page_says_a_board_can_be_backed_by_a_markdown_file_and_reloads() {
    let page = page_text();
    assert!(page.contains("A board can be backed by a Markdown file. The plugin reloads the board when that file changes"));
}

/// REQ: F-024
#[test]
fn doc_024_the_samples_name_the_reload_events_and_the_retry_limit() {
    let page = page_text();
    assert!(page.contains(
        "On `fs-changed`, or on `editor-saved` for the same path, it parses the file again"
    ));
    assert!(page.contains("const MAX_ATTEMPTS: u32 = 3;"));
    assert!(page.contains("the board changed three times; try again"));
}

/// REQ: F-024
#[test]
fn doc_024_the_samples_use_the_documented_columns_and_the_selected_marker() {
    let page = page_text();
    assert!(page.contains("const COLUMNS: [&str; 3] = [\"To do\", \"Doing\", \"Done\"];"));
    assert!(
        page.contains("let marker = if self.selected == (index, row) { \"> \" } else { \"  \" };")
    );
}

/// REQ: F-024
#[test]
fn doc_024_the_user_docs_describe_what_a_board_pane_does() {
    let doc = user_doc("editors-and-boards.md");
    assert!(doc.contains("A board pane shows columns of cards. Cards can be dragged between columns, edited in a side panel, and carry clickable task checkboxes."));
}

/// REQ: F-024
#[test]
fn doc_024_the_user_docs_list_the_board_keys() {
    let doc = user_doc("editors-and-boards.md");
    for row in [
        "| Left / Right, `h` / `l` | Previous / next column |",
        "| Up / Down, `k` / `j` | Move between the column header and its cards |",
        "| Enter | Open the selected card in the details panel |",
        "| `n` | New card in the selected column |",
        "| `c` | New column |",
        "| `e` | Rename the selected card or column |",
        "| `d` | Delete the selected card or column (asks for confirmation) |",
        "| `r` | Reload from storage (needed after the file was changed outside Ilium; see below) |",
    ] {
        assert!(doc.contains(row), "missing key row {row}");
    }
}

/// REQ: F-024
#[test]
fn doc_024_the_user_docs_state_the_card_and_column_title_rules() {
    let doc = user_doc("editors-and-boards.md");
    assert!(doc.contains("Card titles are one line; column titles are one line and cannot be empty or contain path separators."));
}

/// REQ: F-024
#[test]
fn doc_024_the_user_docs_state_the_save_refusal_message() {
    let doc = user_doc("editors-and-boards.md");
    assert!(doc.contains("\"changed outside this board; press r to reload before editing\""));
    assert!(doc.contains("Saving is guarded by a lock and by a check that the file still holds exactly what Ilium last read."));
}

/// REQ: F-024
#[test]
fn doc_024_the_user_docs_state_the_new_board_defaults() {
    let doc = user_doc("editors-and-boards.md");
    assert!(doc.contains("A storage path that does not exist yet is created with three empty columns: **To do**, **Doing**, **Done**."));
    assert!(doc.contains("The default is `.ilium/boards/board.md` under the project root"));
    assert!(doc.contains("A board's storage can be open in only one pane per session"));
    assert!(doc.contains("\"That board storage is already open\""));
}

/// REQ: F-024
#[test]
fn doc_024_the_user_docs_state_the_folder_board_naming_and_limits() {
    let doc = user_doc("editors-and-boards.md");
    assert!(doc.contains("Ilium names cards `NNN-slug.md` in column order (three-digit position, then a lower-case slug of the title, at most 80 characters)"));
    assert!(doc
        .contains("at most 8192 directory entries scanned, and 1 MiB of retained board content."));
    assert!(doc.contains("a single-file board is limited to 512 KiB"));
}

/// REQ: F-025
#[test]
fn doc_025_the_user_docs_describe_creating_a_board_from_markdown() {
    let doc = user_doc("editors-and-boards.md");
    assert!(doc.contains("Right-click an **open Markdown editor** in the tree and choose **Create board from Markdown**."));
    assert!(doc.contains("Unsaved edits are saved first so the board reads exactly what you see."));
    assert!(doc.contains("Or, in the file picker, right-click a `.md` file."));
    assert!(doc.contains(
        "The board opens as a **One Markdown file** board on that same file, in the same group."
    ));
}

/// REQ: F-025
#[test]
fn doc_025_the_user_docs_explain_why_the_create_item_may_be_missing() {
    let doc = user_doc("editors-and-boards.md");
    assert!(doc.contains("| \"Create board from Markdown\" is missing | The editor is not backed by a `.md`/`.markdown` file. |"));
}

/// REQ: F-024
#[test]
fn doc_024_the_page_figure_shows_the_board_title_and_three_headers() {
    let page = page_text();
    assert!(page.contains("╭ Board: plan.md ──"));
    assert!(page.contains("│ To do (2)      Doing (1)      Done (1)       │"));
}

/// REQ: F-024
#[test]
fn doc_024_the_traceability_rows_name_this_plugin_as_the_owner() {
    let path = crate::common::workspace_root()
        .join("docs/plugins-manual/pages/reference-feature-traceability.md");
    let table = std::fs::read_to_string(path).expect("traceability page");
    let f024 = table
        .lines()
        .find(|line| line.starts_with("| F-024 |"))
        .expect("F-024 row");
    assert!(f024.contains("kanban plugin in phase 7"));
    assert!(f024.contains("pane kinds") && f024.contains("[kanban_board]"));
    let f025 = table
        .lines()
        .find(|line| line.starts_with("| F-025 |"))
        .expect("F-025 row");
    assert!(f025.contains("plugin-wasm") && f025.contains("fs.read"));
}
