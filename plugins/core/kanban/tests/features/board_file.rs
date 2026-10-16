//! The single-file board format (src/docs/editors-and-boards.md "How boards are stored") as the
//! plugin reads it, observed on the drawn screen, and the canonical form it writes back.

use proptest::prelude::*;

use crate::common::{SAMPLE_BOARD, board_file, board_host, draw, press, sections, select_first_header};

fn text_of(source: &str) -> String {
    let (mut host, node) = board_host(source);
    draw(&mut host, node, 120, 24).text()
}

/// REQ: F-024
#[test]
fn file_024_every_heading_that_owns_content_becomes_a_column() {
    let text = text_of("# Board\n\n## Alpha\n- one\n\n## Beta\n- two\n");
    assert!(text.contains("Alpha (1)") && text.contains("Beta (1)"), "{text}");
}

/// REQ: F-024
#[test]
fn file_024_deeper_atx_headings_are_columns_too() {
    let text = text_of("### Alpha\n- one\n\n#### Beta\n- two\n");
    assert!(text.contains("Alpha (1)") && text.contains("Beta (1)"), "{text}");
}

/// REQ: F-024
#[test]
fn file_024_a_lone_level_one_title_with_no_cards_is_ignored() {
    let text = text_of("# Roadmap\n\n## To do\n- one\n");
    assert!(!text.contains("Roadmap ("), "{text}");
    assert!(text.contains("To do (1)"), "{text}");
}

/// REQ: F-024
#[test]
fn file_024_dash_star_and_plus_items_are_cards() {
    let text = text_of("## L\n- dash card\n* star card\n+ plus card\n");
    assert!(text.contains("L (3)"), "{text}");
    for title in ["dash card", "star card", "plus card"] {
        assert!(text.contains(title), "{title}");
    }
}

/// REQ: F-024
#[test]
fn file_024_nested_task_lists_are_cards() {
    let text = text_of("## L\n- parent\n  - [ ] child one\n  - [x] child two\n");
    assert!(text.contains("child one") && text.contains("child two"), "{text}");
}

/// REQ: F-024
#[test]
fn file_024_fenced_examples_are_skipped() {
    let text = text_of("## L\n- real\n\n```\n- fenced item\n```\n");
    assert!(text.contains("L (1)"), "{text}");
    assert!(!text.contains("fenced item"), "{text}");
}

/// REQ: F-024
#[test]
fn file_024_top_level_prose_between_lists_is_not_part_of_the_board() {
    let text = text_of("## L\n- a\n\nSome prose paragraph.\n\n- b\n");
    assert!(!text.contains("Some prose paragraph."), "{text}");
}

/// REQ: F-024
#[test]
fn file_024_indented_lines_under_a_card_are_its_notes() {
    let text = text_of("## L\n- title\n  note line one\n  note line two\n");
    assert!(text.contains("title") && text.contains("note line one"), "{text}");
    assert!(text.contains("L (1)"), "notes do not become cards:\n{text}");
}

/// REQ: F-024
#[test]
fn file_024_task_markers_stay_in_the_card_title() {
    let text = text_of("## L\n- [ ] open task\n- [x] done task\n");
    assert!(text.contains("[ ] open task") && text.contains("[x] done task"), "{text}");
}

/// REQ: F-024
#[test]
fn file_024_the_sample_of_the_docs_has_the_documented_columns() {
    assert_eq!(
        sections(SAMPLE_BOARD).iter().map(|(title, cards)| (title.as_str(), cards.len())).collect::<Vec<_>>(),
        [("To do", 2), ("Doing", 1), ("Done", 1)]
    );
}

/// REQ: F-024
#[test]
fn file_024_saving_rewrites_the_file_in_canonical_form() {
    let hand_written = "Intro prose.\n\n## To do\n* star card\n\n\n## Doing\n";
    let (mut host, node) = board_host(hand_written);
    select_first_header(&mut host, node);
    press(&mut host, node, "j");
    press(&mut host, node, "shift+right");
    let saved = board_file(&mut host);
    assert!(saved.starts_with("# Board\n"), "{saved}");
    assert!(!saved.contains("Intro prose."), "extra prose is normalised away:\n{saved}");
    assert!(!saved.contains("* star card"), "{saved}");
    assert_eq!(sections(&saved), [("To do".to_string(), vec![]), ("Doing".to_string(), vec!["star card".to_string()])]);
}

/// REQ: F-024
#[test]
fn file_024_canonical_form_is_title_headings_and_dash_items_only() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    select_first_header(&mut host, node);
    press(&mut host, node, "j");
    press(&mut host, node, "shift+down");
    for line in board_file(&mut host).lines().filter(|line| !line.is_empty()) {
        assert!(line == "# Board" || line.starts_with("## ") || line.starts_with("- "), "line {line:?}");
    }
}

/// REQ: F-024
#[test]
fn file_024_a_save_ends_with_one_newline() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    select_first_header(&mut host, node);
    press(&mut host, node, "j");
    press(&mut host, node, "shift+down");
    let saved = board_file(&mut host);
    assert!(saved.ends_with('\n') && !saved.ends_with("\n\n"), "{saved:?}");
}

/// REQ: F-024
#[test]
fn file_024_an_untouched_board_is_not_rewritten() {
    let hand_written = "Intro prose.\n\n## To do\n* star card\n";
    let (mut host, node) = board_host(hand_written);
    let _ = draw(&mut host, node, 80, 20);
    assert_eq!(board_file(&mut host), hand_written);
}

proptest! {
    // REQ: F-024
    #[test]
    fn file_024_card_counts_in_headers_match_the_file(counts in proptest::collection::vec(0usize..5, 1..4)) {
        let mut source = String::from("# Board\n");
        for (index, count) in counts.iter().enumerate() {
            source.push_str(&format!("\n## Column{index}\n"));
            for card in 0..*count {
                source.push_str(&format!("- card{index}x{card}\n"));
            }
        }
        let (mut host, node) = board_host(&source);
        let text = draw(&mut host, node, 120, 24).text();
        for (index, count) in counts.iter().enumerate() {
            let header = format!("Column{index} ({count})");
            prop_assert!(text.contains(&header), "{header} missing in\n{text}");
        }
    }
}
