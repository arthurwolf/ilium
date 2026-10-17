//! Twins of ilium-client/src/syntax.rs: language recognition and token coverage (F-020).
//! The language comes from the path (API_GAPS: path-derived language) or from the buffer.

use crate::helpers::plain_host;
use crate::support::{direct, direct_open, render_pane};
use ilium_plugin_sdk::generated::exports::token_provider::Token;
use ilium_plugin_sdk::generated::types::BufferId;
use ilium_plugin_sdk::TokenProvider;
use serde_json::json;

fn colours_in_first_text_row(file: &str, text: &str, rows: u16) -> usize {
    let mut host = plain_host(file, None, text, &[("show_line_numbers", json!(false))]);
    let screen = render_pane(&mut host, 60, rows);
    let mut seen: Vec<_> = Vec::new();
    for row in 0..rows {
        for column in 0..60 {
            let cell = screen.cell(row, column);
            if cell.symbol.trim().is_empty() {
                continue;
            }
            if !seen.contains(&cell.foreground) {
                seen.push(cell.foreground);
            }
        }
    }
    seen.len()
}

/// REQ: PORT:ilium-client/src/syntax.rs::recognizes_rust_js_ts_python_and_markdown_by_extension
#[test]
fn port_syntax_known_extensions_are_highlighted() {
    let samples = [
        ("main.rs", "fn main() { let x = \"s\"; }"),
        ("app.js", "function main() { const x = \"s\"; }"),
        ("app.ts", "function main(): void { const x = \"s\"; }"),
        ("app.tsx", "function main(): void { const x = \"s\"; }"),
        ("script.py", "def main():\n    x = \"s\""),
        ("README.md", "# Title\n\nSome **bold** and `code` text."),
    ];
    for (name, text) in samples {
        assert!(
            colours_in_first_text_row(name, text, 4) > 1,
            "expected a syntax for {name}"
        );
    }
}

/// REQ: PORT:ilium-client/src/syntax.rs::recognizes_extensionless_well_known_file_names
#[test]
fn port_syntax_extensionless_well_known_names_are_highlighted() {
    assert!(colours_in_first_text_row("Makefile", "build:\n\tcargo build --release", 4) > 1);
    assert!(colours_in_first_text_row("Dockerfile", "FROM rust:1\nRUN cargo build # step", 4) > 1);
}

/// REQ: PORT:ilium-client/src/syntax.rs::returns_none_for_unrecognized_extension, PORT:ilium-client/src/syntax.rs::unrecognized_extension_yields_no_highlighting
#[test]
fn port_syntax_unrecognized_extension_draws_one_colour() {
    assert_eq!(
        colours_in_first_text_row("data.unknownext", "fn main() { let s = \"hi\"; }", 3),
        1
    );
}

/// REQ: PORT:ilium-client/src/editor_highlight.rs::unrecognized_language_leaves_highlighted_lines_empty
#[test]
fn port_syntax_unrecognized_language_yields_no_tokens() {
    let mut editor = direct("{}");
    direct_open(&mut editor, 1, None, 1);
    let (tokens, _) = TokenProvider::tokens(
        &mut editor,
        BufferId(1),
        0,
        vec!["just plain text".to_string()],
        None,
    );
    assert!(tokens.is_empty(), "{tokens:?}");
}

/// REQ: PORT:ilium-client/src/syntax.rs::highlight_covers_every_byte_of_every_line_contiguously
#[test]
fn port_syntax_tokens_are_ordered_non_overlapping_and_inside_their_line() {
    let lines = vec![
        "fn main() {".to_string(),
        "    println!(\"hi\");".to_string(),
        "}".to_string(),
    ];
    let mut editor = direct("{}");
    direct_open(&mut editor, 1, Some("rust"), 3);
    let (tokens, _) = TokenProvider::tokens(&mut editor, BufferId(1), 0, lines.clone(), None);
    assert!(!tokens.is_empty());
    for (index, line) in lines.iter().enumerate() {
        let on_line: Vec<&Token> = tokens
            .iter()
            .filter(|token| token.line == index as u32)
            .collect();
        let mut next_free = 0u32;
        for token in on_line {
            assert!(
                token.start >= next_free,
                "tokens overlap on line {index}: {tokens:?}"
            );
            assert!(token.length > 0);
            next_free = token.start + token.length;
        }
        assert!(
            next_free as usize <= line.chars().count(),
            "token past the end of line {index}"
        );
    }
}

/// REQ: PORT:ilium-client/src/syntax.rs::highlight_produces_multiple_colors_for_every_requested_language
#[test]
fn port_syntax_every_requested_language_gets_more_than_one_colour() {
    let fixtures = [
        (
            "main.rs",
            "fn main() {\n    let x = 1;\n    println!(\"{x}\");\n}",
        ),
        (
            "app.js",
            "function main() {\n    const x = 1;\n    console.log(x);\n}",
        ),
        (
            "app.ts",
            "function main(): void {\n    const x: number = 1;\n    console.log(x);\n}",
        ),
        ("script.py", "def main():\n    x = 1\n    print(x)"),
        ("README.md", "# Title\n\nSome **bold** and `code` text."),
        ("Makefile", "build:\n\tcargo build --release"),
    ];
    for (name, text) in fixtures {
        assert!(colours_in_first_text_row(name, text, 6) > 1, "{name}");
    }
}
