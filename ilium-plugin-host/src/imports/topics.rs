//! Host implementation of `ilium:host/topics`.
//!
//! Hand-owned. The publisher identity and its declared topics live in a per-thread context
//! because the import carries no session handle yet; the host installs the real manifest values
//! with [`set_publisher_context`] before the plugin runs.

use ilium_plugin_api::generated::host::topics::HostTopics;
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Largest topic payload, in bytes of valid JSON (SVC-022).
const PAYLOAD_LIMIT_BYTES: usize = 64 * 1024;

/// Longest topic name, in characters (SVC-022).
const TOPIC_NAME_LIMIT_CHARS: usize = 128;

/// Successful publishes allowed per plugin per second (API-071, SVC-023).
const PUBLISH_RATE_PER_SECOND: usize = 100;

/// Namespace reserved for kernel topics (SVC-020).
const KERNEL_NAMESPACE_PREFIX: &str = "ilium.";

/// Who is publishing and what its manifest declares under `contributes.topics.publishes`.
struct PublisherContext {
    plugin_id: String,
    declared_topics: Vec<String>,
    recent_publishes: VecDeque<Instant>,
}

thread_local! {
    static PUBLISHER: RefCell<PublisherContext> = RefCell::new(PublisherContext {
        plugin_id: "example.notes".to_string(),
        declared_topics: vec!["example.notes/saved".to_string()],
        recent_publishes: VecDeque::new(),
    });
}

/// Installs the publishing plugin's id and declared topics for the current thread and resets its
/// rate window.
pub fn set_publisher_context(plugin_id: &str, declared_topics: Vec<String>) {
    PUBLISHER.with(|cell| {
        *cell.borrow_mut() = PublisherContext {
            plugin_id: plugin_id.to_string(),
            declared_topics,
            recent_publishes: VecDeque::new(),
        };
    });
}

/// Builds a topics error with no retry delay.
fn topic_error(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_string(),
        retry_after_ms: None,
    }
}

/// Host side of the `topics` interface.
#[derive(Debug)]
pub struct TopicsImport;

impl HostTopics for TopicsImport {
    fn publish(&mut self, topic: String, payload_json: String) -> Result<(), PluginError> {
        PUBLISHER.with(|cell| {
            let mut context = cell.borrow_mut();
            let own_prefix = format!("{}/", context.plugin_id);
            if topic.is_empty()
                || topic.starts_with(KERNEL_NAMESPACE_PREFIX)
                || !topic.starts_with(&own_prefix)
            {
                return Err(topic_error(
                    ErrorCode::NotPermitted,
                    "a plugin may publish only topics under its own id",
                ));
            }
            if topic.chars().count() > TOPIC_NAME_LIMIT_CHARS {
                return Err(topic_error(
                    ErrorCode::InvalidArgument,
                    "the topic name is longer than 128 characters",
                ));
            }
            if !context.declared_topics.contains(&topic) {
                return Err(topic_error(
                    ErrorCode::InvalidArgument,
                    "the topic is not declared in contributes.topics.publishes",
                ));
            }
            if payload_json.len() > PAYLOAD_LIMIT_BYTES {
                return Err(topic_error(
                    ErrorCode::InvalidArgument,
                    "the payload is larger than 64 KiB",
                ));
            }
            if serde_json::from_str::<serde_json::Value>(&payload_json).is_err() {
                return Err(topic_error(
                    ErrorCode::InvalidArgument,
                    "the payload is not valid JSON",
                ));
            }
            let now = Instant::now();
            let window = Duration::from_secs(1);
            while context
                .recent_publishes
                .front()
                .is_some_and(|oldest| now.duration_since(*oldest) >= window)
            {
                context.recent_publishes.pop_front();
            }
            if context.recent_publishes.len() >= PUBLISH_RATE_PER_SECOND {
                let oldest_age = context
                    .recent_publishes
                    .front()
                    .map(|oldest| now.duration_since(*oldest))
                    .unwrap_or_default();
                let wait = window.saturating_sub(oldest_age).as_millis().max(1);
                return Err(PluginError {
                    code: ErrorCode::RateLimited,
                    message: "more than 100 publishes in one second".to_string(),
                    retry_after_ms: Some(u32::try_from(wait).unwrap_or(1_000)),
                });
            }
            context.recent_publishes.push_back(now);
            Ok(())
        })
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
        assert_eq!(
            publish(DECLARED_TOPIC, r#"{"path":"notes/plan.md"}"#),
            Ok(())
        );
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
        expect_error(
            publish("ilium.progress/finished", "{}"),
            ErrorCode::NotPermitted,
        );
    }

    /// REQ: SVC-021
    #[test]
    fn publish_topic_not_declared_in_manifest_is_invalid_argument() {
        expect_error(
            publish("example.notes/undeclared", "{}"),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: SVC-022
    #[test]
    fn publish_topic_name_over_128_characters_is_invalid_argument() {
        let topic = format!(
            "example.notes/{}",
            "a".repeat(TOPIC_NAME_LIMIT_CHARS + 1 - "example.notes/".len())
        );
        assert_eq!(topic.chars().count(), TOPIC_NAME_LIMIT_CHARS + 1);
        expect_error(publish(&topic, "{}"), ErrorCode::InvalidArgument);
    }

    /// REQ: SVC-022
    #[test]
    fn publish_payload_that_is_not_json_is_invalid_argument() {
        expect_error(
            publish(DECLARED_TOPIC, r#"{"path":"#),
            ErrorCode::InvalidArgument,
        );
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
        expect_error(
            publish(DECLARED_TOPIC, &payload),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: SVC-023
    #[test]
    fn publish_beyond_100_per_second_is_rate_limited() {
        for call in 0..PUBLISH_RATE_PER_SECOND {
            assert_eq!(
                publish(DECLARED_TOPIC, "{}"),
                Ok(()),
                "call {call} inside the rate must succeed"
            );
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
        let retry_after_ms = error
            .retry_after_ms
            .expect("rate-limited must set retry-after-ms");
        assert!(retry_after_ms > 0, "retry-after-ms must be positive");
    }
}
