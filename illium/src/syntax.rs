//! Token-level syntax highlighting for Source-mode editor panes, built on
//! `syntect` (the Sublime-Text-grammar-compatible highlighting engine) with
//! `two-face`'s bundled syntax/theme sets standing in for a hand-rolled
//! lexer per language -- this is what gives Rust/JS/TS/Python/Markdown/etc.
//! highlighting "for free" instead of writing one parser per language (see
//! `two_face::syntax`'s own doc listing for the full set). Foreground color
//! and bold/italic/underline are taken from the theme; background is
//! deliberately dropped (see `to_style`) so highlighted code never paints a
//! colored rectangle behind the text, which would clash with illium's own
//! dark chrome (`theme.rs`). `editor_highlight` is the module that turns
//! this output into actual spans on screen.

use std::ops::Range;
use std::path::Path;
use std::sync::LazyLock;

use ratatui::style::{Color, Modifier, Style};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme};
use syntect::parsing::{SyntaxReference, SyntaxSet};

/// Loaded once for the process: `two_face::syntax::extra_no_newlines()` is
/// the same Sublime syntax set `bat` ships (Rust/JS/TS/Python/Markdown/
/// TOML/Dockerfile/... -- see that crate's own doc listing), built for
/// being fed one newline-free line at a time, which is exactly how
/// `EditorPane`'s buffer is stored (`TextArea::lines()`).
static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_no_newlines);

/// One dark theme applied everywhere, matching illium's dark-only chrome
/// (see `theme.rs`) -- there is no light-mode toggle to key a second theme
/// off of. "One Half Dark" (Atom's "One Dark") is a widely recognized,
/// unsurprising default rather than a hand-tuned custom palette.
static THEME: LazyLock<Theme> = LazyLock::new(|| {
    two_face::theme::extra()
        .get(two_face::theme::EmbeddedThemeName::OneHalfDark)
        .clone()
});

/// Looks up a language from the file name: first as a whole (so
/// extension-less names like `Makefile`/`Dockerfile` match, the same
/// lookup `syntect::SyntaxSet::find_syntax_for_file` does before it falls
/// back to reading the file's first line off disk -- which this
/// deliberately skips, since an unsaved buffer's in-memory content is the
/// source of truth, not whatever is or isn't on disk at `path`), then by
/// extension.
fn syntax_for_path(path: &Path) -> Option<&'static SyntaxReference> {
    let file_name = path.file_name()?.to_str()?;
    if let Some(syntax) = SYNTAX_SET.find_syntax_by_extension(file_name) {
        return Some(syntax);
    }
    let extension = path.extension()?.to_str()?;
    SYNTAX_SET.find_syntax_by_extension(extension)
}

/// One line's tokens as byte ranges into that same line's `String`, each
/// with the ratatui `Style` its scope resolved to -- callers slice the
/// buffer's own line text with these ranges rather than this module
/// storing a second copy of it.
pub type LineTokens = Vec<(Range<usize>, Style)>;

/// Highlights every line of `lines` for the language `path`'s name/
/// extension resolves to. Returns `None` when nothing matches -- callers
/// fall back to plain (unstyled) rendering rather than guessing a
/// language, the same allowlist-not-guess approach `EditorPane::is_markdown`
/// already takes for Rendered mode.
pub fn highlight(path: &Path, lines: &[String]) -> Option<Vec<LineTokens>> {
    let syntax = syntax_for_path(path)?;
    let mut highlighter = HighlightLines::new(syntax, &THEME);
    Some(
        lines
            .iter()
            .map(|line| {
                highlighter
                    .highlight_line(line, &SYNTAX_SET)
                    .unwrap_or_default()
                    .into_iter()
                    .scan(0usize, |next_byte, (style, text)| {
                        let start = *next_byte;
                        *next_byte += text.len();
                        Some((start..*next_byte, to_style(style)))
                    })
                    .collect()
            })
            .collect(),
    )
}

/// Converts a `syntect` scope style to a ratatui one, foreground and font
/// weight only -- see the module doc for why background is dropped.
fn to_style(style: syntect::highlighting::Style) -> Style {
    let mut result = Style::new().fg(Color::Rgb(
        style.foreground.r,
        style.foreground.g,
        style.foreground.b,
    ));
    if style.font_style.contains(FontStyle::BOLD) {
        result = result.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        result = result.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        result = result.add_modifier(Modifier::UNDERLINED);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_rust_js_ts_python_and_markdown_by_extension() {
        for name in [
            "main.rs",
            "app.js",
            "app.ts",
            "app.tsx",
            "script.py",
            "README.md",
        ] {
            let path = Path::new(name);
            assert!(
                syntax_for_path(path).is_some(),
                "expected a syntax for {name}"
            );
        }
    }

    #[test]
    fn recognizes_extensionless_well_known_file_names() {
        for name in ["Makefile", "Dockerfile"] {
            let path = Path::new(name);
            assert!(
                syntax_for_path(path).is_some(),
                "expected a syntax for {name}"
            );
        }
    }

    #[test]
    fn returns_none_for_unrecognized_extension() {
        let path = Path::new("data.unknownext");
        assert!(syntax_for_path(path).is_none());
    }

    #[test]
    fn highlight_covers_every_byte_of_every_line_contiguously() {
        let lines = vec![
            "fn main() {".to_string(),
            "    println!(\"hi\");".to_string(),
            "}".to_string(),
        ];
        let tokens = highlight(Path::new("main.rs"), &lines).expect("rust is recognized");
        assert_eq!(tokens.len(), lines.len());
        for (line, line_tokens) in lines.iter().zip(&tokens) {
            let mut expected_start = 0usize;
            for (range, _style) in line_tokens {
                assert_eq!(range.start, expected_start);
                expected_start = range.end;
            }
            assert_eq!(expected_start, line.len());
        }
    }

    /// Proves each requested language actually gets multiple distinct
    /// colors out of `two_face`'s bundled grammars -- not just a
    /// recognized-language boolean, but real per-token variety, across
    /// every language the project goal calls out by name plus Makefile
    /// (a common extensionless case).
    #[test]
    fn highlight_produces_multiple_colors_for_every_requested_language() {
        let fixtures: &[(&str, &str)] = &[
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
        for (name, source) in fixtures {
            let lines: Vec<String> = source.lines().map(str::to_string).collect();
            let tokens = highlight(Path::new(name), &lines)
                .unwrap_or_else(|| panic!("expected {name} to be recognized"));
            let distinct_colors: std::collections::HashSet<_> =
                tokens.iter().flatten().map(|(_, style)| style.fg).collect();
            assert!(
                distinct_colors.len() > 1,
                "expected {name} to highlight with more than one color, got {distinct_colors:?}"
            );
        }
    }

    #[test]
    fn unrecognized_extension_yields_no_highlighting() {
        let lines = vec!["just plain text".to_string()];
        assert!(highlight(Path::new("notes.unknownext"), &lines).is_none());
    }
}
