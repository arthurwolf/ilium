//! Host implementation of `ilium:host/inference` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostInference` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::inference::{Completion, CompletionRequest, HostInference};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::RequestId;

/// Host side of the `inference` interface.
#[derive(Debug)]
pub struct InferenceImport;

impl HostInference for InferenceImport {
    fn complete(&mut self, _request: CompletionRequest) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::inference::complete")
    }

    fn take_completion(&mut self, _request: RequestId) -> Option<Completion> {
        unimplemented!("host::imports::inference::take_completion")
    }

    fn models(&mut self) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::inference::models")
    }

    fn take_models(&mut self, _request: RequestId) -> Option<Vec<String>> {
        unimplemented!("host::imports::inference::take_models")
    }

    fn provider_reply(
        &mut self,
        _call: u64,
        _outcome: Result<Completion, PluginError>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::inference::provider_reply")
    }
}

#[cfg(test)]
mod tests {
    use super::InferenceImport;
    use ilium_plugin_api::generated::host::inference::{
        ChatMessage, Completion, CompletionRequest, HostInference,
    };
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::ids::RequestId;

    /// Limit on all messages together, in content bytes (API page "Limits": 4 MiB).
    const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
    /// Limit on the `json_schema` text (API page "Limits": 64 KiB).
    const MAX_JSON_SCHEMA_BYTES: usize = 64 * 1024;

    fn user_message(content: String) -> ChatMessage {
        ChatMessage {
            role: "user".to_string(),
            content,
        }
    }

    fn request_with(messages: Vec<ChatMessage>, json_schema: Option<String>) -> CompletionRequest {
        CompletionRequest {
            messages,
            max_output_tokens: None,
            json_schema,
            purpose: "unit-test".to_string(),
        }
    }

    /// A valid one-message request whose schema, if any, is provided by the caller.
    fn simple_request(json_schema: Option<String>) -> CompletionRequest {
        request_with(vec![user_message("hello".to_string())], json_schema)
    }

    /// Builds a JSON object text of exactly `total_bytes` bytes, padded inside a string value.
    fn json_schema_of_exact_length(total_bytes: usize) -> String {
        let prefix = r#"{"description":""#;
        let suffix = r#""}"#;
        let padding = total_bytes - prefix.len() - suffix.len();
        format!("{prefix}{}{suffix}", "a".repeat(padding))
    }

    fn expect_error(result: Result<RequestId, PluginError>, expected: ErrorCode) {
        match result {
            Ok(request_id) => panic!("expected {expected:?}, got request {request_id:?}"),
            Err(error) => assert_eq!(error.code, expected),
        }
    }

    fn expect_not_invalid_argument(result: Result<RequestId, PluginError>) {
        if let Err(error) = result {
            assert_ne!(
                error.code,
                ErrorCode::InvalidArgument,
                "valid input refused as invalid-argument: {}",
                error.message
            );
        }
    }

    fn reply_with_text(text: String) -> Completion {
        Completion {
            text,
            model: "unit-test-model".to_string(),
            input_tokens: 1,
            output_tokens: 1,
        }
    }

    /// REQ: API-005
    #[test]
    fn complete_returns_request_id_for_valid_request() {
        let mut import = InferenceImport;
        let result = import.complete(simple_request(None));
        assert!(
            result.is_ok(),
            "valid request must return a request id, got {result:?}"
        );
    }

    /// REQ: API-061
    #[test]
    fn complete_rejects_empty_messages_with_invalid_argument() {
        let mut import = InferenceImport;
        let result = import.complete(request_with(Vec::new(), None));
        expect_error(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-062
    #[test]
    fn complete_rejects_messages_over_4_mib_with_invalid_argument() {
        let mut import = InferenceImport;
        let messages = vec![
            user_message("a".repeat(MAX_MESSAGE_BYTES)),
            user_message("b".to_string()),
        ];
        let result = import.complete(request_with(messages, None));
        expect_error(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-062
    #[test]
    fn complete_accepts_messages_at_exactly_4_mib() {
        let mut import = InferenceImport;
        let messages = vec![user_message("a".repeat(MAX_MESSAGE_BYTES))];
        let result = import.complete(request_with(messages, None));
        expect_not_invalid_argument(result);
    }

    /// REQ: API-062
    #[test]
    fn complete_rejects_json_schema_over_64_kib_with_invalid_argument() {
        let mut import = InferenceImport;
        let schema = json_schema_of_exact_length(MAX_JSON_SCHEMA_BYTES + 1);
        let result = import.complete(simple_request(Some(schema)));
        expect_error(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-062
    #[test]
    fn complete_accepts_json_schema_at_exactly_64_kib() {
        let mut import = InferenceImport;
        let schema = json_schema_of_exact_length(MAX_JSON_SCHEMA_BYTES);
        let result = import.complete(simple_request(Some(schema)));
        expect_not_invalid_argument(result);
    }

    /// REQ: API-061
    #[test]
    fn complete_rejects_json_schema_that_is_not_json_with_invalid_argument() {
        let mut import = InferenceImport;
        let result = import.complete(simple_request(Some("{not json".to_string())));
        expect_error(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-061
    #[test]
    fn complete_accepts_valid_json_schema() {
        let mut import = InferenceImport;
        let schema = r#"{"type":"object","properties":{"tags":{"type":"array"}}}"#.to_string();
        let result = import.complete(simple_request(Some(schema)));
        expect_not_invalid_argument(result);
    }

    /// REQ: API-073
    #[test]
    fn complete_returns_busy_for_65th_pending_request() {
        let mut import = InferenceImport;
        for accepted_index in 0..64 {
            let result = import.complete(simple_request(None));
            assert!(
                result.is_ok(),
                "request {accepted_index} of 64 must be accepted, got {result:?}"
            );
        }
        expect_error(import.complete(simple_request(None)), ErrorCode::Busy);
    }

    /// REQ: API-005
    #[test]
    fn take_completion_returns_none_for_unknown_request() {
        let mut import = InferenceImport;
        assert_eq!(import.take_completion(RequestId(0)), None);
    }

    /// REQ: API-005
    #[test]
    fn take_completion_returns_none_for_largest_request_id() {
        let mut import = InferenceImport;
        assert_eq!(import.take_completion(RequestId(u64::MAX)), None);
    }

    /// REQ: API-005
    #[test]
    fn models_returns_request_id_without_invalid_argument() {
        let mut import = InferenceImport;
        expect_not_invalid_argument(import.models());
    }

    /// REQ: API-005
    #[test]
    fn take_models_returns_none_for_unknown_request() {
        let mut import = InferenceImport;
        assert_eq!(import.take_models(RequestId(0)), None);
    }

    /// REQ: API-002
    #[test]
    fn provider_reply_returns_not_found_for_unknown_call() {
        let mut import = InferenceImport;
        let outcome = Err(PluginError {
            code: ErrorCode::Network,
            message: "provider unreachable".to_string(),
            retry_after_ms: None,
        });
        let result = import.provider_reply(999, outcome);
        match result {
            Ok(()) => panic!("unknown call must not be accepted"),
            Err(error) => assert_eq!(error.code, ErrorCode::NotFound),
        }
    }

    /// REQ: API-002
    #[test]
    fn provider_reply_returns_not_found_for_call_zero_with_completion() {
        let mut import = InferenceImport;
        let result = import.provider_reply(0, Ok(reply_with_text("ok".to_string())));
        match result {
            Ok(()) => panic!("unknown call must not be accepted"),
            Err(error) => assert_eq!(error.code, ErrorCode::NotFound),
        }
    }

    /// REQ: API-061
    #[test]
    fn provider_reply_rejects_reply_text_over_4_mib_with_invalid_argument() {
        let mut import = InferenceImport;
        let oversized_text = "a".repeat(MAX_MESSAGE_BYTES + 1);
        let result = import.provider_reply(1, Ok(reply_with_text(oversized_text)));
        match result {
            Ok(()) => panic!("reply text over 4 MiB must be refused"),
            Err(error) => assert_eq!(error.code, ErrorCode::InvalidArgument),
        }
    }

    /// REQ: API-062
    #[test]
    fn provider_reply_accepts_reply_text_at_exactly_4_mib() {
        let mut import = InferenceImport;
        let text = "a".repeat(MAX_MESSAGE_BYTES);
        let result = import.provider_reply(1, Ok(reply_with_text(text)));
        if let Err(error) = result {
            assert_ne!(
                error.code,
                ErrorCode::InvalidArgument,
                "reply text at exactly 4 MiB refused as invalid-argument"
            );
        }
    }
}
