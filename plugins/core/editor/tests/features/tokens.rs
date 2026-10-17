//! Syntax highlighting through the token provider (F-020, api-token-provider.md).
//! The provider gets no language argument: the language comes from the `editor-opened` buffer.

use crate::support::{direct, direct_open};
use ilium_plugin_core_editor::EditorClient;
use ilium_plugin_sdk::generated::exports::token_provider::{Token, TokenKind};
use ilium_plugin_sdk::generated::types::{BufferId, Event};
use ilium_plugin_sdk::{Handler, TokenProvider};
use proptest::prelude::*;

fn tok(line: u32, start: u32, length: u32, kind: TokenKind) -> Token {
    Token {
        line,
        start,
        length,
        kind,
    }
}

fn rust_editor() -> EditorClient {
    let mut editor = direct("{}");
    direct_open(&mut editor, 1, Some("rust"), 1);
    editor
}

fn lines_of(texts: &[&str]) -> Vec<String> {
    texts.iter().map(|text| text.to_string()).collect()
}

/// REQ: F-020, WIT:ilium:exports/token-provider.tokens
#[test]
fn f_020_rust_keywords_and_string_literal_get_tokens() {
    let mut editor = rust_editor();
    let (tokens, _state) = TokenProvider::tokens(
        &mut editor,
        BufferId(1),
        0,
        lines_of(&["fn main() { let s = \"hi\"; }"]),
        None,
    );
    assert!(
        tokens.contains(&tok(0, 0, 2, TokenKind::Keyword)),
        "fn: {tokens:?}"
    );
    assert!(
        tokens.contains(&tok(0, 12, 3, TokenKind::Keyword)),
        "let: {tokens:?}"
    );
    assert!(
        tokens.contains(&tok(0, 20, 4, TokenKind::String)),
        "string: {tokens:?}"
    );
}

/// REQ: F-020
#[test]
fn f_020_keyword_and_string_kinds_differ() {
    let mut editor = rust_editor();
    let (tokens, _) = TokenProvider::tokens(
        &mut editor,
        BufferId(1),
        0,
        lines_of(&["fn main() { let s = \"hi\"; }"]),
        None,
    );
    let keyword = tokens
        .iter()
        .find(|token| token.start == 0)
        .map(|token| token.kind);
    let string = tokens
        .iter()
        .find(|token| token.start == 20)
        .map(|token| token.kind);
    assert_eq!(keyword, Some(TokenKind::Keyword));
    assert_eq!(string, Some(TokenKind::String));
}

/// REQ: F-020
#[test]
fn f_020_page_sample_lines_get_comment_and_number_tokens() {
    let mut editor = rust_editor();
    let lines = lines_of(&[
        "use std::fs;",
        "",
        "fn main() {",
        "    let n = 42;",
        "    // run it",
        "}",
    ]);
    let (tokens, _) = TokenProvider::tokens(&mut editor, BufferId(1), 0, lines, None);
    assert!(
        tokens.contains(&tok(0, 0, 3, TokenKind::Keyword)),
        "use: {tokens:?}"
    );
    assert!(
        tokens.contains(&tok(3, 12, 2, TokenKind::Number)),
        "42: {tokens:?}"
    );
    assert!(
        tokens.contains(&tok(4, 4, 9, TokenKind::Comment)),
        "comment: {tokens:?}"
    );
    assert!(
        tokens.iter().all(|token| token.line != 1),
        "the blank line has no token"
    );
}

/// REQ: F-020
#[test]
fn f_020_token_lines_are_absolute_buffer_lines() {
    let mut editor = rust_editor();
    let (tokens, _) = TokenProvider::tokens(
        &mut editor,
        BufferId(1),
        40,
        lines_of(&["fn a() {}", "fn b() {}"]),
        None,
    );
    assert!(tokens.contains(&tok(40, 0, 2, TokenKind::Keyword)));
    assert!(tokens.contains(&tok(41, 0, 2, TokenKind::Keyword)));
    assert!(tokens
        .iter()
        .all(|token| token.line == 40 || token.line == 41));
}

/// REQ: F-020
#[test]
fn f_020_columns_count_characters_not_bytes() {
    let mut editor = rust_editor();
    let (tokens, _) = TokenProvider::tokens(
        &mut editor,
        BufferId(1),
        0,
        lines_of(&["let s = \"\u{e9}\"; // \u{fc}"]),
        None,
    );
    assert!(
        tokens.contains(&tok(0, 8, 3, TokenKind::String)),
        "string by characters: {tokens:?}"
    );
    assert!(
        tokens.contains(&tok(0, 13, 4, TokenKind::Comment)),
        "comment by characters: {tokens:?}"
    );
}

/// REQ: F-020
#[test]
fn f_020_unrecognized_language_leaves_the_lines_plain() {
    let mut editor = direct("{}");
    direct_open(&mut editor, 1, None, 1);
    let (tokens, _) = TokenProvider::tokens(
        &mut editor,
        BufferId(1),
        0,
        lines_of(&["fn main() { let s = \"hi\"; }"]),
        None,
    );
    assert!(tokens.is_empty(), "no language, no tokens: {tokens:?}");
}

/// REQ: F-020
#[test]
fn f_020_language_without_a_lexer_leaves_the_lines_plain() {
    let mut editor = direct("{}");
    direct_open(&mut editor, 1, Some("no-such-language"), 1);
    let (tokens, _) = TokenProvider::tokens(
        &mut editor,
        BufferId(1),
        0,
        lines_of(&["fn main() {}"]),
        None,
    );
    assert!(tokens.is_empty());
}

/// REQ: F-020
#[test]
fn f_020_buffer_never_opened_has_no_tokens() {
    let mut editor = direct("{}");
    let (tokens, state) = TokenProvider::tokens(
        &mut editor,
        BufferId(99),
        0,
        lines_of(&["fn main() {}"]),
        None,
    );
    assert!(tokens.is_empty());
    assert!(state.is_none());
}

/// REQ: F-020
#[test]
fn f_020_closed_buffer_drops_its_language() {
    let mut editor = rust_editor();
    Handler::on_events(
        &mut editor,
        vec![Event::EditorClosed(crate::support::buffer_ref(
            1,
            None,
            Some("rust"),
            1,
        ))],
    );
    let (tokens, _) = TokenProvider::tokens(
        &mut editor,
        BufferId(1),
        0,
        lines_of(&["fn main() {}"]),
        None,
    );
    assert!(tokens.is_empty());
}

/// REQ: F-020
#[test]
fn f_020_empty_block_returns_no_tokens() {
    let mut editor = rust_editor();
    let (tokens, _) = TokenProvider::tokens(&mut editor, BufferId(1), 5, Vec::new(), None);
    assert!(tokens.is_empty());
}

/// REQ: F-020
#[test]
fn f_020_block_comment_state_carries_into_the_next_block() {
    let mut editor = rust_editor();
    let (_, state) =
        TokenProvider::tokens(&mut editor, BufferId(1), 0, lines_of(&["/* opening"]), None);
    assert!(
        state.is_some(),
        "an open block comment leaves a lexer state"
    );
    let (tokens, _) = TokenProvider::tokens(
        &mut editor,
        BufferId(1),
        1,
        lines_of(&["inside block comment", "*/ fn main() {}"]),
        state,
    );
    assert!(
        tokens.contains(&tok(1, 0, 20, TokenKind::Comment)),
        "line inside the comment: {tokens:?}"
    );
    assert!(
        tokens.contains(&tok(2, 3, 2, TokenKind::Keyword)),
        "fn after the comment: {tokens:?}"
    );
}

/// REQ: F-020
#[test]
fn f_020_without_the_carried_state_the_second_block_is_not_a_comment() {
    let mut editor = rust_editor();
    let (tokens, _) = TokenProvider::tokens(
        &mut editor,
        BufferId(1),
        1,
        lines_of(&["inside block comment"]),
        None,
    );
    assert!(
        tokens.iter().all(|token| token.kind != TokenKind::Comment),
        "{tokens:?}"
    );
}

/// REQ: F-020
#[test]
fn f_020_a_closed_comment_leaves_no_open_state_for_the_next_block() {
    let mut editor = rust_editor();
    let (_, state) = TokenProvider::tokens(
        &mut editor,
        BufferId(1),
        0,
        lines_of(&["/* done */ fn a() {}"]),
        None,
    );
    let (tokens, _) =
        TokenProvider::tokens(&mut editor, BufferId(1), 1, lines_of(&["fn b() {}"]), state);
    assert!(
        tokens.contains(&tok(1, 0, 2, TokenKind::Keyword)),
        "{tokens:?}"
    );
}

/// REQ: F-020
#[test]
fn f_020_two_buffers_keep_their_own_languages() {
    let mut editor = rust_editor();
    direct_open(&mut editor, 2, None, 1);
    let (plain, _) = TokenProvider::tokens(
        &mut editor,
        BufferId(2),
        0,
        lines_of(&["fn main() {}"]),
        None,
    );
    let (rust, _) = TokenProvider::tokens(
        &mut editor,
        BufferId(1),
        0,
        lines_of(&["fn main() {}"]),
        None,
    );
    assert!(plain.is_empty());
    assert!(!rust.is_empty());
}

proptest! {
    // REQ: F-020
    #[test]
    fn f_020_tokens_stay_inside_the_requested_block(
        lines in proptest::collection::vec("[ -~]{0,40}", 0..6),
        first_line in 0u32..1000,
    ) {
        let mut editor = rust_editor();
        let (tokens, _) = TokenProvider::tokens(&mut editor, BufferId(1), first_line, lines.clone(), None);
        for token in &tokens {
            prop_assert!(token.line >= first_line);
            prop_assert!((token.line - first_line) < lines.len() as u32);
            let length = lines[(token.line - first_line) as usize].chars().count() as u32;
            prop_assert!(token.length > 0);
            prop_assert!(token.start + token.length <= length);
        }
    }

    // REQ: F-020
    #[test]
    fn f_020_tokenizing_the_same_block_twice_gives_the_same_answer(
        lines in proptest::collection::vec("[ -~]{0,40}", 0..6),
    ) {
        let mut editor = rust_editor();
        let first = TokenProvider::tokens(&mut editor, BufferId(1), 0, lines.clone(), None);
        let second = TokenProvider::tokens(&mut editor, BufferId(1), 0, lines, None);
        prop_assert_eq!(first, second);
    }
}
