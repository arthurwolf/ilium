//! Shared "render an XML-ish Handlebars prompt, call the selected inference provider
//! once, then parse+validate a bounded-word-count JSON reply" pipeline used
//! by both `project_naming` and `session_naming`. Each caller supplies only
//! what's genuinely distinct to it: the template text, the JSON field name
//! the model is asked to return, and its word-count bounds. Neither caller
//! retries a failed provider call -- a naming inference is best-effort, and
//! the caller decides how to react to (or surface) a failure.

use handlebars::Handlebars;
use ilium_inference::{
    provider_from_settings, InferenceError, InferenceRequest, InferenceSettings,
};
use serde::Serialize;

/// Sends one already-rendered prompt to the selected provider and returns its raw
/// text reply. Both `project_naming` and `session_naming` implement this
/// purely so tests can inject a fake generator without real HTTP; production
/// production code uses the settings-backed provider adapter below.
pub trait PromptCompletionClient {
    fn complete_prompt(&self, prompt: String) -> Result<String, InferenceError>;
}

/// Settings-backed adapter. The concrete provider is constructed only when a
/// worker runs, keeping persisted configuration independent from transport.
impl PromptCompletionClient for InferenceSettings {
    fn complete_prompt(&self, prompt: String) -> Result<String, InferenceError> {
        provider_from_settings(self)
            .complete(&InferenceRequest::json_only(prompt))
            .map(|response| response.text)
    }
}

/// Registers `template` under `template_name`, renders it with `context`,
/// and sends the result to `client` exactly once. `template_name` only needs
/// to be unique within this single render call, since the `Handlebars`
/// instance is not shared or cached across calls.
pub fn render_and_complete<C, T>(
    client: &C,
    template_name: &str,
    template: &str,
    context: &T,
) -> anyhow::Result<String>
where
    C: PromptCompletionClient,
    T: Serialize,
{
    let mut handlebars = Handlebars::new();
    // The rendered output is a plain-text LLM prompt, not HTML -- Handlebars'
    // default escape fn would otherwise mangle README/CLAUDE.md content and
    // user prompts (turning `=`, backticks, quotes, `<`/`>` into HTML
    // entities) before the model ever sees it.
    handlebars.register_escape_fn(handlebars::no_escape);
    handlebars.register_template_string(template_name, template)?;
    let prompt = handlebars.render(template_name, context)?;
    Ok(client.complete_prompt(prompt)?)
}

/// Parses `response` as a JSON object, reads `field` as a string, and
/// normalizes it to `min_words..=max_words` whitespace-collapsed words no
/// longer than 64 characters with no control characters. `context_label`
/// (e.g. `"project-name"`, `"session-title"`) only shapes the error message.
pub fn parse_bounded_word_json(
    response: &str,
    field: &str,
    min_words: usize,
    max_words: usize,
    context_label: &str,
) -> anyhow::Result<String> {
    let parsed = parse_json_object(response, context_label)?;
    extract_bounded_word_field(&parsed, field, min_words, max_words, context_label)
}

/// A short-form/long-form pair of titles parsed from one LLM JSON reply --
/// the shared shape `session_naming` and `terminal_naming` return so
/// `tree_ui` can pick between them by tree-panel width without caring which
/// pipeline produced them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DualTitle {
    /// One concise UTF-8 visual marker for the work the title describes.
    pub icon: String,
    /// 2-3 words, shown when the tree panel is too narrow for `long`.
    pub short: String,
    /// 5-7 words, shown when the tree panel is wide enough.
    pub long: String,
}

/// Word-count bounds and the JSON field name to read them from -- one
/// instance each for the short and long fields passed to
/// `parse_dual_bounded_word_json`.
pub struct BoundedField {
    pub field: &'static str,
    pub min_words: usize,
    pub max_words: usize,
}

/// Parses `response` as a JSON object and reads both a short and a long
/// bounded-word field from it in one pass, so a single malformed reply
/// (missing field, out-of-range word count) fails the whole inference
/// rather than leaving one half silently unset.
pub fn parse_dual_bounded_word_json(
    response: &str,
    short: BoundedField,
    long: BoundedField,
    context_label: &str,
) -> anyhow::Result<DualTitle> {
    let parsed = parse_json_object(response, context_label)?;
    Ok(DualTitle {
        icon: extract_icon_field(&parsed, "icon", context_label)?,
        short: extract_bounded_word_field(
            &parsed,
            short.field,
            short.min_words,
            short.max_words,
            context_label,
        )?,
        long: extract_bounded_word_field(
            &parsed,
            long.field,
            long.min_words,
            long.max_words,
            context_label,
        )?,
    })
}

/// Accepts one compact UTF-8 icon/emoticon while rejecting prose, spacing,
/// and terminal control characters. Emoji sequences may contain several
/// scalar values (variation selectors / ZWJ), so validation is deliberately
/// character-count based rather than assuming one Unicode scalar.
pub fn extract_icon_field(
    parsed: &serde_json::Value,
    field: &str,
    context_label: &str,
) -> anyhow::Result<String> {
    let raw_value = parsed
        .get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            anyhow::anyhow!("{context_label} response missing string field \"{field}\"")
        })?;
    normalize_icon(raw_value).ok_or_else(|| {
        anyhow::anyhow!(
            "{context_label} response field \"{field}\" must be one compact UTF-8 icon or emoticon"
        )
    })
}

/// Normalizes a model-supplied visual marker without turning it into prose.
pub fn normalize_icon(value: &str) -> Option<String> {
    let icon = value.trim();
    (!icon.is_empty()
        && icon.chars().count() <= 16
        && !icon
            .chars()
            .any(|character| character.is_control() || character.is_whitespace()))
    .then_some(icon.to_string())
}

fn parse_json_object(response: &str, context_label: &str) -> anyhow::Result<serde_json::Value> {
    serde_json::from_str(response)
        .map_err(|error| anyhow::anyhow!("{context_label} response was not valid JSON: {error}"))
}

fn extract_bounded_word_field(
    parsed: &serde_json::Value,
    field: &str,
    min_words: usize,
    max_words: usize,
    context_label: &str,
) -> anyhow::Result<String> {
    let raw_value = parsed
        .get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            anyhow::anyhow!("{context_label} response missing string field \"{field}\"")
        })?;
    normalize_word_bounded(raw_value, min_words, max_words).ok_or_else(|| {
        anyhow::anyhow!(
            "{context_label} response field \"{field}\" must contain {min_words} to {max_words} short, non-empty words"
        )
    })
}

/// Collapses internal whitespace and accepts `value` only if it has
/// `min_words..=max_words` words, is at most 64 characters, and contains no
/// control characters -- the shared validity bar for an inferred name/title.
pub fn normalize_word_bounded(value: &str, min_words: usize, max_words: usize) -> Option<String> {
    let words: Vec<_> = value.split_whitespace().collect();
    let word_count = words.len();
    let normalized = words.join(" ");
    ((min_words..=max_words).contains(&word_count)
        && normalized.chars().count() <= 64
        && normalized.chars().all(|character| !character.is_control()))
    .then_some(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct FakeClient {
        calls: Cell<u8>,
        response: String,
        // Captures the exact rendered prompt text so tests can assert on it
        // (e.g. that it was not silently HTML-escaped by Handlebars).
        last_prompt: std::cell::RefCell<String>,
    }

    impl FakeClient {
        fn new(response: &str) -> Self {
            FakeClient {
                calls: Cell::new(0),
                response: response.to_string(),
                last_prompt: std::cell::RefCell::new(String::new()),
            }
        }
    }

    impl PromptCompletionClient for FakeClient {
        fn complete_prompt(&self, prompt: String) -> Result<String, InferenceError> {
            self.calls.set(self.calls.get() + 1);
            *self.last_prompt.borrow_mut() = prompt;
            Ok(self.response.clone())
        }
    }

    #[test]
    fn render_and_complete_renders_the_template_and_calls_the_client_once() {
        let client = FakeClient::new("reply");
        #[derive(Serialize)]
        struct Context {
            name: String,
        }

        let response = render_and_complete(
            &client,
            "greeting",
            "hello {{name}}",
            &Context {
                name: "world".to_string(),
            },
        )
        .unwrap();

        assert_eq!(response, "reply");
        assert_eq!(client.calls.get(), 1);
        assert_eq!(*client.last_prompt.borrow(), "hello world");
    }

    #[test]
    fn render_and_complete_does_not_html_escape_context_values() {
        let client = FakeClient::new("reply");
        #[derive(Serialize)]
        struct Context {
            snippet: String,
        }
        let snippet = "Vec<T> = a && b `code` \"quoted\" 'it's'";

        render_and_complete(
            &client,
            "escaping",
            "{{snippet}}",
            &Context {
                snippet: snippet.to_string(),
            },
        )
        .unwrap();

        // The prompt is plain text for an LLM, not HTML -- special
        // characters must reach the model unescaped rather than turned into
        // `&lt;`, `&amp;`, `&#x3D;`, `&#x27;`, `&#x60;` entities.
        assert_eq!(*client.last_prompt.borrow(), snippet);
    }

    #[test]
    fn parse_bounded_word_json_normalizes_whitespace_and_enforces_bounds() {
        let result =
            parse_bounded_word_json(r#"{"title":"  Fix   Auth Bug  "}"#, "title", 2, 4, "title");
        assert_eq!(result.unwrap(), "Fix Auth Bug");

        assert!(parse_bounded_word_json(r#"{"title":"Fix"}"#, "title", 2, 4, "title").is_err());
        assert!(parse_bounded_word_json("not json", "title", 2, 4, "title").is_err());
        assert!(
            parse_bounded_word_json(r#"{"other":"Fix Auth"}"#, "title", 2, 4, "title").is_err()
        );
    }

    #[test]
    fn parse_dual_bounded_word_json_reads_both_fields_in_one_pass() {
        let result = parse_dual_bounded_word_json(
            r#"{"icon":"🔐","short":"Auth Bug","long":"Fix the login authentication bug today"}"#,
            BoundedField {
                field: "short",
                min_words: 2,
                max_words: 3,
            },
            BoundedField {
                field: "long",
                min_words: 5,
                max_words: 7,
            },
            "title",
        )
        .unwrap();
        assert_eq!(result.short, "Auth Bug");
        assert_eq!(result.long, "Fix the login authentication bug today");
    }

    #[test]
    fn parse_dual_bounded_word_json_fails_when_either_field_is_out_of_bounds() {
        let bounds = || {
            (
                BoundedField {
                    field: "short",
                    min_words: 2,
                    max_words: 3,
                },
                BoundedField {
                    field: "long",
                    min_words: 5,
                    max_words: 7,
                },
            )
        };

        let (short, long) = bounds();
        assert!(parse_dual_bounded_word_json(
            r#"{"short":"Auth","long":"Fix the login authentication bug today"}"#,
            short,
            long,
            "title",
        )
        .is_err());

        let (short, long) = bounds();
        assert!(
            parse_dual_bounded_word_json(r#"{"short":"Auth Bug"}"#, short, long, "title").is_err()
        );
    }

    #[test]
    fn normalize_word_bounded_rejects_out_of_range_word_counts_and_control_characters() {
        assert_eq!(
            normalize_word_bounded("Ilium", 1, 2),
            Some("Ilium".to_string())
        );
        assert_eq!(normalize_word_bounded("One Two Three", 1, 2), None);
        assert_eq!(normalize_word_bounded("bad\u{0007}name", 1, 2), None);
    }
}
