//! The Test provider button of the Inference settings page (manual `core-plugin-inference.md`,
//! sample 4). One short request goes to the provider the user selected; when its result arrives
//! the plugin writes the latency and the model into the `last_test` info row, or `failed`. The
//! prompt is fixed and synthetic: it never carries the user's terminals, files or transcripts.
//! This module is the pure half (the request and the row text); the events are handled in
//! `lib.rs`.

use ilium_plugin_sdk::generated::host::{ChatMessage, CompletionRequest};

/// The command id suffix of the button row (`ilium.core.inference/test-provider`).
pub const TEST_COMMAND_SUFFIX: &str = "/test-provider";
/// The id of the info row that shows the last result.
pub const LAST_TEST_ROW: &str = "last_test";
/// What the row reads before any run.
pub const NOT_RUN_YET: &str = "Not run yet";
/// What the row reads when the host returned no completion.
pub const FAILED: &str = "failed";
/// The `purpose` the admission gate and the privacy log record for the test.
pub const TEST_PURPOSE: &str = "provider-test";
/// The fixed prompt.
pub const TEST_PROMPT: &str = "Reply with the word ok.";
/// The reply is one word, so a small budget is enough.
pub const TEST_MAXIMUM_OUTPUT_TOKENS: u32 = 16;

/// The one request of a test.
pub fn request() -> CompletionRequest {
    CompletionRequest {
        messages: vec![ChatMessage {
            role: "user".to_owned(),
            content: TEST_PROMPT.to_owned(),
        }],
        max_output_tokens: Some(TEST_MAXIMUM_OUTPUT_TOKENS),
        json_schema: None,
        purpose: TEST_PURPOSE.to_owned(),
    }
}

/// The text of the `last_test` row: `812 ms, model` for a reply, `failed` for none.
pub fn result_text(elapsed_millis: u64, model: Option<&str>) -> String {
    match model {
        Some(model) => format!("{elapsed_millis} ms, {model}"),
        None => FAILED.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_request_is_the_fixed_one_word_prompt_with_the_test_purpose() {
        let request = request();
        assert_eq!(request.purpose, "provider-test");
        assert_eq!(request.max_output_tokens, Some(16));
        assert_eq!(request.json_schema, None);
        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.messages[0].role, "user");
        assert_eq!(request.messages[0].content, "Reply with the word ok.");
    }

    #[test]
    fn the_row_text_carries_latency_and_model_or_failed() {
        assert_eq!(
            result_text(812, Some("gpt-5.6-terra")),
            "812 ms, gpt-5.6-terra"
        );
        assert_eq!(result_text(0, Some("m")), "0 ms, m");
        assert_eq!(result_text(5, None), "failed");
    }
}
