//! Infers a short-form (2-3 word) and long-form (5-7 word) pair of titles
//! for a plain terminal pane from its current on-screen text, reusing the
//! same `crate::naming` (Handlebars prompt + Kilo-Gateway free model +
//! bounded-word-JSON-reply) pipeline `session_naming` uses for agent panes.
//! `ilium-client`'s tree panel shows the short title when the panel is
//! narrow and the long title when it's wide (see `crate::tree_ui`).
//!
//! Unlike `session_naming`, there is no transcript file to read here -- a
//! plain shell has no concept of a "session." The context is instead
//! whatever is currently on screen (see `crate::terminal_view::TerminalView::with_screen`),
//! clipped to a bounded size before being sent to the model.

use serde::Serialize;

use crate::naming::{self, BoundedField, DualTitle, PromptCompletionClient};

const TERMINAL_TITLE_SHORT_MIN_WORDS: usize = 2;
const TERMINAL_TITLE_SHORT_MAX_WORDS: usize = 3;
const TERMINAL_TITLE_LONG_MIN_WORDS: usize = 5;
const TERMINAL_TITLE_LONG_MAX_WORDS: usize = 7;

/// Upper bound (in characters) on how much screen text is sent to the
/// model -- a full scrollback dump would blow the free model's context and
/// cost for no benefit, and the tail of the visible screen almost always
/// carries the most recent, most relevant command and output anyway.
const TERMINAL_SCREEN_CLIP_CHARS: usize = 4000;

// `{{{screen_text}}}` (triple-stash) deliberately skips Handlebars' default
// HTML-escaping: shell output is dense with `<`, `>`, `&`, and quotes
// (redirections, pipes, quoting) that would otherwise turn into `&lt;`/
// `&amp;` noise and degrade the model's read of what's actually on screen.
const TERMINAL_TITLE_TEMPLATE: &str = r#"<instructions>
Infer two titles describing what this terminal is currently being used for, based on the commands and output visible on its screen below: a short title of 2 to 3 words, and a long title of 5 to 7 words. Prefer the shortest accurate wording for each over a longer one. Do not return punctuation-only text or a generic phrase such as "terminal session".
</instructions>
<terminal-screen>
{{{screen_text}}}
</terminal-screen>
<output-example>{"terminal_title_short":"Rust Build","terminal_title_long":"Build Rust Project With Cargo"}</output-example>
<response-format>Return exactly one JSON object following the output example. Do not wrap it in Markdown.</response-format>"#;

/// Clips `screen_text` and asks the free model for a short/long title pair.
/// This is the entry point `naming_workers::spawn_terminal_title_worker`
/// spawns a worker thread around.
pub fn infer_terminal_title<G: PromptCompletionClient>(
    generator: &G,
    screen_text: &str,
) -> anyhow::Result<DualTitle> {
    let clipped = clip_screen_text(screen_text);
    if clipped.is_empty() {
        anyhow::bail!("no screen content available to infer a terminal title from");
    }

    let context = TerminalTitleContext {
        screen_text: clipped,
    };
    let response = naming::render_and_complete(
        generator,
        "terminal-title",
        TERMINAL_TITLE_TEMPLATE,
        &context,
    )?;
    parse_terminal_title_response(&response)
}

/// Trims `screen_text` and, if still over `TERMINAL_SCREEN_CLIP_CHARS`,
/// keeps only its tail -- `vt100::Screen::contents()` orders rows top
/// (oldest visible) to bottom (most recent), so the tail is the most
/// recent, most relevant content.
fn clip_screen_text(screen_text: &str) -> String {
    let trimmed = screen_text.trim();
    let char_count = trimmed.chars().count();
    if char_count <= TERMINAL_SCREEN_CLIP_CHARS {
        return trimmed.to_string();
    }
    let skip = char_count - TERMINAL_SCREEN_CLIP_CHARS;
    trimmed.chars().skip(skip).collect()
}

#[derive(Debug, Serialize)]
struct TerminalTitleContext {
    screen_text: String,
}

fn parse_terminal_title_response(response: &str) -> anyhow::Result<DualTitle> {
    naming::parse_dual_bounded_word_json(
        response,
        BoundedField {
            field: "terminal_title_short",
            min_words: TERMINAL_TITLE_SHORT_MIN_WORDS,
            max_words: TERMINAL_TITLE_SHORT_MAX_WORDS,
        },
        BoundedField {
            field: "terminal_title_long",
            min_words: TERMINAL_TITLE_LONG_MIN_WORDS,
            max_words: TERMINAL_TITLE_LONG_MAX_WORDS,
        },
        "terminal-title",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_kilo_gateway::GatewayError;
    use std::cell::{Cell, RefCell};

    struct FakeGenerator {
        calls: Cell<u8>,
        last_prompt: RefCell<Option<String>>,
        response: String,
    }

    impl FakeGenerator {
        fn new(response: impl Into<String>) -> Self {
            Self {
                calls: Cell::new(0),
                last_prompt: RefCell::new(None),
                response: response.into(),
            }
        }
    }

    impl PromptCompletionClient for FakeGenerator {
        fn complete_prompt(&self, prompt: String) -> Result<String, GatewayError> {
            self.calls.set(self.calls.get() + 1);
            *self.last_prompt.borrow_mut() = Some(prompt);
            Ok(self.response.clone())
        }
    }

    #[test]
    fn empty_screen_never_calls_the_gateway() {
        let generator = FakeGenerator::new(
            r#"{"terminal_title_short":"Rust Build","terminal_title_long":"Build Rust Project With Cargo"}"#,
        );
        let result = infer_terminal_title(&generator, "   \n  \n");
        assert!(result.is_err());
        assert_eq!(generator.calls.get(), 0);
    }

    #[test]
    fn successful_response_returns_the_normalized_title_pair() {
        let generator = FakeGenerator::new(
            r#"{"terminal_title_short":"  Rust   Build  ","terminal_title_long":"Build Rust Project With Cargo"}"#,
        );
        let result = infer_terminal_title(&generator, "$ cargo build\n   Compiling ilium").unwrap();
        assert_eq!(result.short, "Rust Build");
        assert_eq!(result.long, "Build Rust Project With Cargo");
        assert_eq!(generator.calls.get(), 1);
    }

    #[test]
    fn prompt_includes_the_screen_text_unescaped_and_the_json_output_example() {
        let generator = FakeGenerator::new(
            r#"{"terminal_title_short":"Rust Build","terminal_title_long":"Build Rust Project With Cargo"}"#,
        );
        infer_terminal_title(&generator, "$ echo <hello> && echo done").unwrap();

        let prompt = generator.last_prompt.borrow().clone().unwrap();
        assert!(prompt.contains("<terminal-screen>"));
        assert!(prompt.contains("$ echo <hello> && echo done"));
        assert!(!prompt.contains("&lt;"));
        assert!(prompt.contains(
            "<output-example>{\"terminal_title_short\":\"Rust Build\",\"terminal_title_long\":\"Build Rust Project With Cargo\"}</output-example>"
        ));
    }

    #[test]
    fn rejects_non_json_and_out_of_range_word_counts() {
        assert!(parse_terminal_title_response("Build Rust Project").is_err());
        assert!(parse_terminal_title_response(
            r#"{"terminal_title_short":"Build","terminal_title_long":"Build Rust Project With Cargo"}"#
        )
        .is_err());
        assert!(parse_terminal_title_response(
            r#"{"terminal_title_short":"Rust Build","terminal_title_long":"Build The Whole Rust Project With Cargo Today"}"#
        )
        .is_err());
    }

    #[test]
    fn clip_screen_text_keeps_only_the_tail_when_over_the_limit() {
        // Head and tail markers must differ, or a same-character fill (e.g.
        // all "a") would let a head-keeping (or middle-keeping) bug pass
        // this test undetected -- length alone doesn't prove which end
        // survived.
        let head = "h".repeat(500);
        let tail = "t".repeat(TERMINAL_SCREEN_CLIP_CHARS);
        let long_text = format!("{head}{tail}");
        let clipped = clip_screen_text(&long_text);
        assert_eq!(clipped.chars().count(), TERMINAL_SCREEN_CLIP_CHARS);
        assert_eq!(clipped, tail);
        assert!(!clipped.contains('h'));
    }

    #[test]
    fn clip_screen_text_leaves_short_text_untouched() {
        assert_eq!(clip_screen_text("  hello  "), "hello");
    }
}
