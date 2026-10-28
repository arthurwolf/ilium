//! Host implementation of `ilium:host/intents-host` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostIntentsHost` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::intents_host::HostIntentsHost;
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::RequestId;
use ilium_plugin_api::generated::types::intents::Submission;

use std::sync::atomic::{AtomicU64, Ordering};

use ilium_plugin_api::generated::types::errors::ErrorCode;

/// Longest dedup key, in characters.
const DEDUP_KEY_MAX_CHARS: usize = 128;

/// Process-wide request id counter; ids start at 1 so zero never names a request.
static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// True for the characters a dedup key may hold: A-Z a-z 0-9 . _ : / -.
fn is_dedup_key_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | ':' | '/' | '-')
}

/// Host side of the `intents-host` interface.
#[derive(Debug)]
pub struct IntentsHostImport;

impl HostIntentsHost for IntentsHostImport {
    fn submit(&mut self, submission: Submission) -> Result<RequestId, PluginError> {
        if let Some(key) = &submission.dedup_key {
            let length = key.chars().count();
            if length == 0 || length > DEDUP_KEY_MAX_CHARS || !key.chars().all(is_dedup_key_char) {
                return Err(PluginError {
                    code: ErrorCode::InvalidArgument,
                    message: "dedup key must be 1 to 128 characters of A-Z a-z 0-9 . _ : / -"
                        .to_owned(),
                    retry_after_ms: None,
                });
            }
        }
        Ok(RequestId(NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed)))
    }
}

#[cfg(test)]
mod tests {
    use super::IntentsHostImport;
    use ilium_plugin_api::generated::host::intents_host::HostIntentsHost;
    use ilium_plugin_api::generated::types::errors::ErrorCode;
    use ilium_plugin_api::generated::types::ids::NodeId;
    use ilium_plugin_api::generated::types::intents::{Intent, Submission};

    /// Largest correlation a plugin may choose: numbers from 2^63 up belong to the SDK.
    const LARGEST_PLUGIN_CORRELATION: u64 = (1u64 << 63) - 1;

    /// Longest `plugin-error.message` in characters (api-host "The plugin error record").
    const ERROR_MESSAGE_MAX_CHARS: usize = 256;

    /// Dedup key length bounds: 1 to 128 characters.
    const DEDUP_KEY_MAX_CHARS: usize = 128;

    /// Every character the dedup key format allows: A-Z a-z 0-9 . _ : / -.
    const DEDUP_KEY_ALLOWED_CHARS: &str =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._:/-";

    /// A well-formed submission of a sound intent with the given dedup key and correlation.
    fn play_sound_submission(dedup_key: Option<String>, correlation: u64) -> Submission {
        Submission {
            intent: Intent::PlaySound("chime".to_string()),
            correlation,
            dedup_key,
        }
    }

    /// REQ: RQO-020
    #[test]
    fn submit_accepts_submission_without_dedup_key() {
        let mut import = IntentsHostImport;
        let result = import.submit(play_sound_submission(None, 1));
        assert!(result.is_ok(), "a submission without a dedup key is valid");
    }

    /// REQ: RQO-020
    #[test]
    fn submit_accepts_dedup_key_of_one_character() {
        let mut import = IntentsHostImport;
        let result = import.submit(play_sound_submission(Some("a".to_string()), 1));
        assert!(
            result.is_ok(),
            "a one-character dedup key is the minimum valid length"
        );
    }

    /// REQ: RQO-020
    #[test]
    fn submit_accepts_dedup_key_of_128_characters() {
        let mut import = IntentsHostImport;
        let key = "k".repeat(DEDUP_KEY_MAX_CHARS);
        let result = import.submit(play_sound_submission(Some(key), 1));
        assert!(
            result.is_ok(),
            "a 128-character dedup key is the maximum valid length"
        );
    }

    /// REQ: RQO-020
    #[test]
    fn submit_refuses_dedup_key_of_129_characters_with_invalid_argument() {
        let mut import = IntentsHostImport;
        let key = "k".repeat(DEDUP_KEY_MAX_CHARS + 1);
        let result = import.submit(play_sound_submission(Some(key), 1));
        let error = result.expect_err("a 129-character dedup key is malformed");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    /// REQ: RQO-020
    #[test]
    fn submit_refuses_empty_dedup_key_with_invalid_argument() {
        let mut import = IntentsHostImport;
        let result = import.submit(play_sound_submission(Some(String::new()), 1));
        let error = result.expect_err("an empty dedup key is malformed (1 to 128 characters)");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    /// REQ: RQO-020
    #[test]
    fn submit_accepts_dedup_key_with_every_allowed_character() {
        let mut import = IntentsHostImport;
        let result = import.submit(play_sound_submission(
            Some(DEDUP_KEY_ALLOWED_CHARS.to_string()),
            1,
        ));
        assert!(
            result.is_ok(),
            "every documented dedup key character must be accepted"
        );
    }

    /// REQ: RQO-020
    #[test]
    fn submit_refuses_dedup_key_containing_space_with_invalid_argument() {
        let mut import = IntentsHostImport;
        let result = import.submit(play_sound_submission(Some("finish node".to_string()), 1));
        let error = result.expect_err("a space is outside the dedup key character set");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    /// REQ: RQO-020
    #[test]
    fn submit_refuses_dedup_key_containing_non_ascii_letter_with_invalid_argument() {
        let mut import = IntentsHostImport;
        let result = import.submit(play_sound_submission(Some("café".to_string()), 1));
        let error = result.expect_err("non-ASCII letters are outside the dedup key character set");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    /// REQ: ABI-011
    #[test]
    fn submit_accepts_correlation_zero() {
        let mut import = IntentsHostImport;
        let result = import.submit(play_sound_submission(None, 0));
        assert!(
            result.is_ok(),
            "correlation 0 is a plugin-chosen number and must be accepted"
        );
    }

    /// REQ: ABI-011
    #[test]
    fn submit_accepts_largest_plugin_correlation_below_2_pow_63() {
        let mut import = IntentsHostImport;
        let result = import.submit(play_sound_submission(None, LARGEST_PLUGIN_CORRELATION));
        assert!(
            result.is_ok(),
            "correlation 2^63 - 1 still belongs to the plugin"
        );
    }

    /// REQ: ABI-040
    #[test]
    fn submit_returns_distinct_request_ids_for_successive_calls() {
        let mut import = IntentsHostImport;
        let first = import
            .submit(play_sound_submission(None, 1))
            .expect("first submission is valid");
        let second = import
            .submit(play_sound_submission(None, 2))
            .expect("second submission is valid");
        assert_ne!(first, second, "each call returns a fresh request id");
    }

    /// REQ: API-005
    #[test]
    fn submit_accepts_well_formed_node_intent_with_valid_key() {
        let mut import = IntentsHostImport;
        let submission = Submission {
            intent: Intent::CloseNode(NodeId(7)),
            correlation: 3,
            dedup_key: Some("close:node-7".to_string()),
        };
        let result = import.submit(submission);
        assert!(
            result.is_ok(),
            "a well-formed intent with a valid dedup key must be accepted"
        );
    }

    /// REQ: API-003
    #[test]
    fn submit_invalid_argument_error_has_no_retry_after() {
        let mut import = IntentsHostImport;
        let result = import.submit(play_sound_submission(Some(String::new()), 1));
        let error = result.expect_err("an empty dedup key is malformed");
        assert_eq!(
            error.retry_after_ms, None,
            "retry-after-ms is only set for rate-limited and CPU-budget quota errors"
        );
    }

    /// REQ: API-001
    #[test]
    fn submit_error_carries_message_within_256_characters() {
        let mut import = IntentsHostImport;
        let result = import.submit(play_sound_submission(Some("k".repeat(129)), 1));
        let error = result.expect_err("a 129-character dedup key is malformed");
        assert!(
            !error.message.is_empty(),
            "the plugin error record must carry a message"
        );
        assert!(
            error.message.chars().count() <= ERROR_MESSAGE_MAX_CHARS,
            "the message is capped at 256 characters"
        );
    }
}
