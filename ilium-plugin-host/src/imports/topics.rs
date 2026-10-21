//! Host implementation of `ilium:host/topics` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostTopics` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::topics::HostTopics;
use ilium_plugin_api::generated::types::errors::PluginError;

/// Host side of the `topics` interface.
#[derive(Debug)]
pub struct TopicsImport;

impl HostTopics for TopicsImport {
    fn publish(&mut self, _topic: String, _payload_json: String) -> Result<(), PluginError> {
        unimplemented!("host::imports::topics::publish")
    }
}

#[cfg(test)]
mod tests {
    use super::TopicsImport;
    use ilium_plugin_api::generated::host::topics::HostTopics;
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};

    /// A topic the example plugin declares under `contributes.topics.publishes`.
    const DECLARED_TOPIC: &str = "example.notes/saved";

    /// Largest topic payload, in bytes of valid JSON (api-messages-and-topics, "Limits").
    const PAYLOAD_LIMIT_BYTES: usize = 64 * 1024;

    /// Longest topic name, in characters (api-messages-and-topics, "Limits").
    const TOPIC_NAME_LIMIT_CHARS: usize = 128;

    /// Successful publishes allowed per plugin per second (API-071, topics.publish 100 per second).
    const PUBLISH_RATE_PER_SECOND: usize = 100;

    /// Calls `topics::publish` on a fresh import, as the plugin side does.
    fn publish(topic: &str, payload_json: &str) -> Result<(), PluginError> {
        let mut topics = TopicsImport;
        topics.publish(topic.to_string(), payload_json.to_string())
    }

    /// Asserts that the call failed with the expected code and returns the error.
    fn expect_error(result: Result<(), PluginError>, expected: ErrorCode) -> PluginError {
        match result {
            Err(error) => {
                assert_eq!(error.code, expected, "message: {}", error.message);
                error
            }
            Ok(()) => panic!("expected {expected:?}, got Ok"),
        }
    }

    /// REQ: SVC-021
    #[test]
    fn publish_declared_topic_with_json_object_returns_ok() {
        assert_eq!(publish(DECLARED_TOPIC, r#"{"path":"notes/plan.md"}"#), Ok(()));
    }

    /// REQ: SVC-022
    #[test]
    fn publish_empty_payload_is_invalid_argument() {
        expect_error(publish(DECLARED_TOPIC, ""), ErrorCode::InvalidArgument);
    }

    /// REQ: SVC-020
    #[test]
    fn publish_empty_topic_name_is_not_permitted() {
        expect_error(publish("", "{}"), ErrorCode::NotPermitted);
    }

    /// REQ: SVC-020
    #[test]
    fn publish_topic_under_another_plugin_id_is_not_permitted() {
        expect_error(publish("other.x/y", "{}"), ErrorCode::NotPermitted);
    }

    /// REQ: SVC-020
    #[test]
    fn publish_under_kernel_namespace_is_not_permitted() {
        expect_error(publish("ilium.progress/finished", "{}"), ErrorCode::NotPermitted);
    }

    /// REQ: SVC-021
    #[test]
    fn publish_topic_not_declared_in_manifest_is_invalid_argument() {
        expect_error(publish("example.notes/undeclared", "{}"), ErrorCode::InvalidArgument);
    }

    /// REQ: SVC-022
    #[test]
    fn publish_topic_name_over_128_characters_is_invalid_argument() {
        let topic = format!("example.notes/{}", "a".repeat(TOPIC_NAME_LIMIT_CHARS + 1 - "example.notes/".len()));
        assert_eq!(topic.chars().count(), TOPIC_NAME_LIMIT_CHARS + 1);
        expect_error(publish(&topic, "{}"), ErrorCode::InvalidArgument);
    }

    /// REQ: SVC-022
    #[test]
    fn publish_payload_that_is_not_json_is_invalid_argument() {
        expect_error(publish(DECLARED_TOPIC, r#"{"path":"#), ErrorCode::InvalidArgument);
    }

    /// REQ: SVC-022
    #[test]
    fn publish_payload_at_exactly_64_kib_is_accepted() {
        let payload = format!("\"{}\"", "a".repeat(PAYLOAD_LIMIT_BYTES - 2));
        assert_eq!(payload.len(), PAYLOAD_LIMIT_BYTES);
        assert_eq!(publish(DECLARED_TOPIC, &payload), Ok(()));
    }

    /// REQ: SVC-022
    #[test]
    fn publish_payload_one_byte_over_64_kib_is_invalid_argument() {
        let payload = format!("\"{}\"", "a".repeat(PAYLOAD_LIMIT_BYTES - 1));
        assert_eq!(payload.len(), PAYLOAD_LIMIT_BYTES + 1);
        expect_error(publish(DECLARED_TOPIC, &payload), ErrorCode::InvalidArgument);
    }

    /// REQ: SVC-023
    #[test]
    fn publish_beyond_100_per_second_is_rate_limited() {
        for call in 0..PUBLISH_RATE_PER_SECOND {
            assert_eq!(publish(DECLARED_TOPIC, "{}"), Ok(()), "call {call} inside the rate must succeed");
        }
        expect_error(publish(DECLARED_TOPIC, "{}"), ErrorCode::RateLimited);
    }

    /// REQ: API-071
    #[test]
    fn rate_limited_publish_carries_retry_after_ms() {
        for _ in 0..PUBLISH_RATE_PER_SECOND {
            assert_eq!(publish(DECLARED_TOPIC, "{}"), Ok(()));
        }
        let error = expect_error(publish(DECLARED_TOPIC, "{}"), ErrorCode::RateLimited);
        let retry_after_ms = error.retry_after_ms.expect("rate-limited must set retry-after-ms");
        assert!(retry_after_ms > 0, "retry-after-ms must be positive");
    }
}
