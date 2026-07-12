//! Shared "render an XML-ish Handlebars prompt, call the free-model gateway
//! once, then parse+validate a bounded-word-count JSON reply" pipeline used
//! by both `project_naming` and `session_naming`. Each caller supplies only
//! what's genuinely distinct to it: the template text, the JSON field name
//! the model is asked to return, and its word-count bounds. Neither caller
//! retries a failed gateway call -- a naming inference is best-effort, and
//! the caller decides how to react to (or surface) a failure.

use handlebars::Handlebars;
use illium_kilo_gateway::{ChatMessage, CompletionRequest, GatewayError, KiloGatewayClient};
use serde::Serialize;

/// Sends one already-rendered prompt to the free model and returns its raw
/// text reply. Both `project_naming` and `session_naming` implement this
/// purely so tests can inject a fake generator without real HTTP; production
/// code always uses the blanket `KiloGatewayClient` impl below.
pub trait PromptCompletionClient {
    fn complete_prompt(&self, prompt: String) -> Result<String, GatewayError>;
}

impl PromptCompletionClient for KiloGatewayClient {
    fn complete_prompt(&self, prompt: String) -> Result<String, GatewayError> {
        self.complete_text(&CompletionRequest::with_default_free_model(vec![
            ChatMessage::system("You return concise, valid JSON only."),
            ChatMessage::user(prompt),
        ]))
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
    let parsed: serde_json::Value = serde_json::from_str(response)
        .map_err(|error| anyhow::anyhow!("{context_label} response was not valid JSON: {error}"))?;
    let raw_value = parsed
        .get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            anyhow::anyhow!("{context_label} response missing string field \"{field}\"")
        })?;
    normalize_word_bounded(raw_value, min_words, max_words).ok_or_else(|| {
        anyhow::anyhow!(
            "{context_label} response must contain {min_words} to {max_words} short, non-empty words"
        )
    })
}

/// Collapses internal whitespace and accepts `value` only if it has
/// `min_words..=max_words` words, is at most 64 characters, and contains no
/// control characters -- the shared validity bar for an inferred name/title.
pub fn normalize_word_bounded(value: &str, min_words: usize, max_words: usize) -> Option<String> {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let word_count = normalized.split_whitespace().count();
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
    }

    impl PromptCompletionClient for FakeClient {
        fn complete_prompt(&self, _prompt: String) -> Result<String, GatewayError> {
            self.calls.set(self.calls.get() + 1);
            Ok(self.response.clone())
        }
    }

    #[test]
    fn render_and_complete_renders_the_template_and_calls_the_client_once() {
        let client = FakeClient {
            calls: Cell::new(0),
            response: "reply".to_string(),
        };
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
    fn normalize_word_bounded_rejects_out_of_range_word_counts_and_control_characters() {
        assert_eq!(
            normalize_word_bounded("Illium", 1, 2),
            Some("Illium".to_string())
        );
        assert_eq!(normalize_word_bounded("One Two Three", 1, 2), None);
        assert_eq!(normalize_word_bounded("bad\u{0007}name", 1, 2), None);
    }
}
