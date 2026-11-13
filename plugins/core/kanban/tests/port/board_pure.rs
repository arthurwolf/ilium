//! The pure board rules of `ilium-client/src/board.rs`, tested through the plugin's `board` module.

use ilium_plugin_core_kanban::board::{
    board_text_extract, checkbox_occurrences, parse_folder_card, parse_markdown_board, safe_name,
    serialize_folder_card, serialize_markdown_board, validate_card_title, validate_column_title,
    BoardCard, BoardColumn,
};

fn card(title: &str, body: &str) -> BoardCard {
    BoardCard {
        title: title.into(),
        body: body.into(),
    }
}

fn column(title: &str, cards: Vec<BoardCard>) -> BoardColumn {
    BoardColumn {
        title: title.into(),
        cards,
    }
}

/// REQ: F-024
#[test]
fn markdown_storage_round_trips_columns_cards_and_bodies() {
    let columns = vec![
        column(
            "Todo",
            vec![card("one", ""), card("two", "line a\n  indented b")],
        ),
        column("Done", vec![]),
    ];
    let source = serialize_markdown_board(&columns);
    assert!(source.starts_with("# Board\n\n## Todo\n- one\n- two\n  line a\n    indented b\n"));
    assert_eq!(parse_markdown_board(&source), columns);
}

/// REQ: F-024
#[test]
fn existing_todo_markdown_imports_heading_columns_and_common_list_markers() {
    let columns = parse_markdown_board("## Now\n- a\n* b\n+ c\n\n### Later\n- d\n");
    assert_eq!(columns.len(), 2);
    assert_eq!(columns[0].title, "Now");
    let titles: Vec<&str> = columns[0].cards.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, ["a", "b", "c"]);
    assert_eq!(columns[1].title, "Later");
}

/// REQ: F-024
#[test]
fn markdown_import_ignores_document_title_and_fenced_list_examples() {
    let source = "# Project\n\n## Todo\n- real\n  ```\n  - fenced\n  ```\n";
    let columns = parse_markdown_board(source);
    assert_eq!(columns.len(), 1);
    assert_eq!(columns[0].cards.len(), 1);
    assert_eq!(columns[0].cards[0].title, "real");
    assert!(columns[0].cards[0].body.contains("- fenced"));
}

/// REQ: F-024
#[test]
fn a_list_without_a_heading_lands_in_an_items_column() {
    let columns = parse_markdown_board("- a\n- b\n");
    assert_eq!(
        columns,
        vec![column("Items", vec![card("a", ""), card("b", "")])]
    );
    assert!(parse_markdown_board("just prose\n").is_empty());
}

/// REQ: F-024
#[test]
fn a_hash_glued_to_a_column_title_is_part_of_the_title() {
    let columns = parse_markdown_board("## C#\n- x\n## Title ##\n- y\n");
    assert_eq!(columns[0].title, "C#");
    assert_eq!(columns[1].title, "Title");
}

/// REQ: F-024
#[test]
fn an_indented_hash_line_is_card_body_not_a_new_column() {
    let columns = parse_markdown_board("## A\n- card\n  # not a heading\n");
    assert_eq!(columns.len(), 1);
    assert_eq!(columns[0].cards[0].body, "# not a heading");
}

/// REQ: F-024
#[test]
fn folder_cards_round_trip_and_fall_back_to_the_file_name() {
    let with_body = card("Title", "body\ntext");
    let text = serialize_folder_card(&with_body);
    assert_eq!(text, "# Title\n\nbody\ntext\n");
    assert_eq!(
        parse_folder_card(&text, "fallback".into()),
        ("Title".into(), "body\ntext".into())
    );
    assert_eq!(serialize_folder_card(&card("T", "")), "# T\n");
    assert_eq!(
        parse_folder_card("no heading\nmore\n", "name".into()),
        ("name".into(), "no heading\nmore".into())
    );
    assert_eq!(parse_folder_card("# \nx", "name".into()).0, "name");
    assert_eq!(
        parse_folder_card("", "name".into()),
        ("name".into(), String::new())
    );
}

/// REQ: F-024
#[test]
fn titles_are_validated_for_both_storage_forms() {
    assert_eq!(validate_card_title("  hi  "), Ok("hi".to_string()));
    assert!(validate_card_title("  ").is_err());
    assert!(validate_card_title("a\nb").is_err());
    assert_eq!(validate_column_title(" Todo "), Ok("Todo".to_string()));
    for bad in ["", ".", "..", "a/b", "a\\b", "a\nb"] {
        assert!(validate_column_title(bad).is_err(), "{bad:?}");
    }
}

/// REQ: F-024
#[test]
fn task_checkboxes_are_found_with_their_state() {
    assert_eq!(
        checkbox_occurrences("a [ ] b [x] c [X]"),
        vec![(2, false), (8, true), (14, true)]
    );
    assert!(checkbox_occurrences("[y] []").is_empty());
}

/// REQ: F-024
#[test]
fn board_text_extract_renders_columns_and_cards_with_nonempty_bodies_inline() {
    let columns = vec![column("Todo", vec![card("a", "  note "), card("b", "")])];
    assert_eq!(board_text_extract(&columns), "## Todo\n- a: note\n- b\n");
}

/// REQ: F-024
#[test]
fn safe_names_are_lowercase_dashed_bounded_and_never_empty() {
    assert_eq!(safe_name("Hello, World!"), "hello--world");
    assert_eq!(safe_name("???"), "card");
    assert_eq!(safe_name(&"a".repeat(200)).len(), 80);
}
