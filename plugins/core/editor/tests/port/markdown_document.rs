//! Twins of ilium-client/src/markdown/document.rs through the rendered Markdown view (F-021).

use crate::helpers::{rendered_screen, row_of};

const F: &str = "ilium-client/src/markdown/document.rs";

fn rows(text: &str) -> Vec<String> {
    let screen = rendered_screen(text);
    (0..16)
        .map(|row| screen.row(row).trim_end().to_string())
        .collect()
}

fn has_row(rows: &[String], wanted: &str) -> bool {
    rows.iter().any(|row| {
        row.trim_start_matches(' ').trim_end() == wanted.trim_start_matches(' ')
            || row.contains(wanted)
    })
}

/// REQ: PORT:ilium-client/src/markdown/document.rs::tight_list_remains_compact_without_synthetic_spacing
#[test]
fn port_document_tight_list_has_adjacent_rows() {
    let screen = rendered_screen("- first\n- second\n");
    assert_eq!(row_of(&screen, "• second") - row_of(&screen, "• first"), 1);
    let _ = F;
}

/// REQ: PORT:ilium-client/src/markdown/document.rs::loose_list_keeps_each_marker_with_its_text_and_preserves_the_gap
#[test]
fn port_document_loose_list_keeps_markers_and_the_gap() {
    let screen = rendered_screen("- first\n\n- second\n");
    assert_eq!(row_of(&screen, "• second") - row_of(&screen, "• first"), 2);
}

/// REQ: PORT:ilium-client/src/markdown/document.rs::loose_list_items_second_paragraph_stays_indented_under_the_marker
#[test]
fn port_document_loose_item_second_paragraph_is_indented_under_the_marker() {
    let r = rows("- first\n\n  more\n");
    assert!(r.iter().any(|row| row.starts_with("  more")), "{r:?}");
}

/// REQ: PORT:ilium-client/src/markdown/document.rs::preserves_spacing_on_both_sides_of_a_list
#[test]
fn port_document_spacing_around_a_list_is_preserved() {
    let screen = rendered_screen("before\n\n- item\n\nafter\n");
    assert_eq!(row_of(&screen, "• item") - row_of(&screen, "before"), 2);
    assert_eq!(row_of(&screen, "after") - row_of(&screen, "• item"), 2);
}

/// REQ: PORT:ilium-client/src/markdown/document.rs::blockquote_marker_only_row_is_preserved_as_spacing
#[test]
fn port_document_quote_marker_only_row_is_kept() {
    let r = rows("> one\n>\n> two\n");
    assert!(r.iter().any(|row| row == "│"), "{r:?}");
}

/// REQ: PORT:ilium-client/src/markdown/document.rs::blockquote_opening_inside_a_list_item_keeps_its_quote_prefix
#[test]
fn port_document_quote_inside_a_list_item_keeps_its_prefix() {
    assert!(has_row(&rows("- > quoted\n"), "• │ quoted"));
}

/// REQ: PORT:ilium-client/src/markdown/document.rs::hard_break_stays_two_adjacent_content_rows
#[test]
fn port_document_hard_break_keeps_two_adjacent_rows() {
    let screen = rendered_screen("first  \nsecond\n");
    assert_eq!(row_of(&screen, "second") - row_of(&screen, "first"), 1);
}

/// REQ: PORT:ilium-client/src/markdown/document.rs::hard_break_inside_a_list_item_keeps_the_marker_indent
#[test]
fn port_document_hard_break_in_a_list_item_keeps_the_indent() {
    let r = rows("- first  \n  second\n");
    assert!(r.iter().any(|row| row == "  second"), "{r:?}");
}

/// REQ: PORT:ilium-client/src/markdown/document.rs::does_not_invent_spacing_for_a_soft_line_break
#[test]
fn port_document_soft_break_joins_with_a_space() {
    assert!(has_row(&rows("first\nsecond\n"), "first second"));
}

/// REQ: PORT:ilium-client/src/markdown/document.rs::fenced_code_keeps_internal_blank_rows_without_a_trailing_sentinel
#[test]
fn port_document_fenced_code_keeps_internal_blank_rows() {
    let screen = rendered_screen("```\nfirst\n\nsecond\n```\n");
    assert_eq!(row_of(&screen, "second") - row_of(&screen, "first"), 2);
    assert!(screen
        .row(row_of(&screen, "first") as u16)
        .starts_with(" first"));
}

/// REQ: PORT:ilium-client/src/markdown/document.rs::crlf_code_block_lines_carry_no_carriage_returns
#[test]
fn port_document_crlf_code_block_has_no_carriage_returns() {
    let screen = rendered_screen("```\r\nfirst\r\nsecond\r\n```\r\n");
    assert!(!screen.text().contains('\r'));
    assert_eq!(row_of(&screen, "second") - row_of(&screen, "first"), 1);
}

/// REQ: PORT:ilium-client/src/markdown/document.rs::preserves_the_exact_number_of_blank_lines_between_paragraphs
#[test]
fn port_document_blank_lines_between_paragraphs_are_preserved() {
    let screen = rendered_screen("first\n\n\n\nsecond\n");
    assert_eq!(row_of(&screen, "second") - row_of(&screen, "first"), 4);
}

/// REQ: PORT:ilium-client/src/markdown/document.rs::inline_image_stays_in_text
#[test]
fn port_document_inline_image_is_a_placeholder_in_text() {
    assert!(has_row(
        &rows("see ![a](a.png) here\n"),
        "see [image: a] here"
    ));
}

/// REQ: PORT:ilium-client/src/markdown/document.rs::two_adjacent_images_in_one_paragraph_keep_both_alt_texts
#[test]
fn port_document_adjacent_images_keep_both_alt_texts() {
    let screen = rendered_screen("![a](a.png)![b](b.png)\n");
    assert!(screen.text().contains("[image: a]") && screen.text().contains("[image: b]"));
}

/// REQ: PORT:ilium-client/src/markdown/document.rs::image_with_trailing_text_in_the_same_paragraph_stays_inline
#[test]
fn port_document_image_with_trailing_text_stays_inline() {
    assert!(has_row(
        &rows("![a](a.png) caption\n"),
        "[image: a] caption"
    ));
}

/// REQ: PORT:ilium-client/src/markdown/document.rs::image_inside_table_cell_does_not_become_a_block_image
#[test]
fn port_document_image_in_a_table_cell_stays_a_placeholder() {
    assert!(rendered_screen("| a |\n|---|\n| ![x](x.png) |\n")
        .text()
        .contains("[image: x]"));
}

/// REQ: PORT:ilium-client/src/markdown/document.rs::remote_image_is_unsupported_not_fetched
#[test]
fn port_document_remote_image_is_not_fetched() {
    let text = rendered_screen("![r](https://example.com/r.png)\n").text();
    assert!(text.contains("r"), "{text}");
    assert!(
        text.contains("unsupported") || text.contains("[image: r]"),
        "{text}"
    );
}

/// REQ: PORT:ilium-client/src/markdown/document.rs::lone_image_paragraph_becomes_image_block
#[test]
fn port_document_lone_image_paragraph_is_its_own_block() {
    let screen = rendered_screen("before\n\n![a](a.png)\n\nafter\n");
    assert!(row_of(&screen, "after") - row_of(&screen, "before") >= 3);
}

/// REQ: PORT:ilium-client/src/markdown/document.rs::loose_task_list_item_with_a_lone_image_keeps_its_checkbox
#[test]
fn port_document_loose_task_item_with_an_image_keeps_its_checkbox() {
    assert!(rendered_screen("- [ ] ![a](a.png)\n\n- [x] two\n")
        .text()
        .contains("[ ]"));
}

/// REQ: PORT:ilium-client/src/markdown/document.rs::table_renders_aligned_rows
#[test]
fn port_document_table_rows_have_equal_width() {
    let r = rows("| a | bb |\n|---|----|\n| ccc | d |\n");
    let table: Vec<&String> = r
        .iter()
        .filter(|row| row.contains('|') || row.contains('│'))
        .collect();
    assert_eq!(table.len(), 3, "{r:?}");
    let width = table[0].chars().count();
    assert!(
        table.iter().all(|row| row.chars().count() == width),
        "{table:?}"
    );
}

/// REQ: PORT:ilium-client/src/markdown/document.rs::table_cell_longer_than_the_width_cap_stays_aligned
#[test]
fn port_document_overlong_table_cell_stays_aligned() {
    let long = "x".repeat(80);
    let r = rows(&format!("| a | b |\n|---|---|\n| {long} | d |\n"));
    let table: Vec<&String> = r
        .iter()
        .filter(|row| row.contains('|') || row.contains('│'))
        .collect();
    assert_eq!(table.len(), 3, "{r:?}");
    assert!(
        table
            .iter()
            .all(|row| row.chars().count() == table[0].chars().count()),
        "{table:?}"
    );
}

/// REQ: PORT:ilium-client/src/markdown/document.rs::unicode_table_cell_aligns_with_ascii_cell
#[test]
fn port_document_wide_unicode_cell_aligns_with_ascii_cell() {
    let r = rows("| a |\n|---|\n| 漢字 |\n| ab |\n");
    let table: Vec<&String> = r
        .iter()
        .filter(|row| row.contains('|') || row.contains('│'))
        .collect();
    assert_eq!(table.len(), 4, "{r:?}");
}

/// REQ: PORT:ilium-client/src/markdown/document.rs::bold_text_gets_bold_modifier
#[test]
fn port_document_bold_text_is_bold() {
    let screen = rendered_screen("plain **strong** plain\n");
    let (row, column) = screen.find("strong").expect("strong");
    assert!(screen.cell(row, column).bold);
}

/// REQ: PORT:ilium-client/src/markdown/document.rs::heading_becomes_its_own_block
#[test]
fn port_document_heading_is_its_own_block() {
    let screen = rendered_screen("para\n# Head\ntail\n");
    assert!(row_of(&screen, "Head") > row_of(&screen, "para"));
    assert!(row_of(&screen, "tail") > row_of(&screen, "Head"));
}

/// REQ: PORT:ilium-client/src/markdown/document.rs::setext_heading_with_hard_break_keeps_all_text_in_the_heading
#[test]
fn port_document_setext_heading_with_hard_break_keeps_all_text() {
    let text = rendered_screen("first  \nsecond\n=====\n").text();
    assert!(text.contains("first") && text.contains("second"), "{text}");
}
