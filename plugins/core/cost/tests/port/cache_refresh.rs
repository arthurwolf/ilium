//! Twins of ilium-server/src/cache_refresh_request.rs and ilium-server/src/cache_refresh_transport.rs
//! (the kernel files were deleted when the logic moved into `cache_refresh`). The time arguments are
//! milliseconds; the kernel tests used `Instant`, so "now" is `0` and an expiry is a later millisecond.

use ilium_plugin_core_cost::cache_refresh::{
    classify_usage, claude_refresh_body, finish, prepare, Capture, ClaudeRefreshError,
    RefreshOutcome, MAX_RESPONSE_BYTES,
};
use serde_json::{json, Value};

fn capture(expires_at_ms: u64) -> Capture {
    Capture {
        endpoint: "https://example.invalid/v1/messages".to_owned(),
        headers: vec![
            ("x-api-key".to_owned(), "secret-value".to_owned()),
            ("Content-Length".to_owned(), "123".to_owned()),
            ("HOST".to_owned(), "example.invalid".to_owned()),
            ("transfer-encoding".to_owned(), "chunked".to_owned()),
            ("Connection".to_owned(), "keep-alive".to_owned()),
            ("anthropic-version".to_owned(), "2023-06-01".to_owned()),
        ],
        body: r#"{"model":"example","messages":[]}"#.to_owned(),
        cache_expires_at_ms: expires_at_ms,
        cache_lifetime_ms: 60_000,
        expected_cached_tokens: 100,
    }
}

/// Twin of cache_refresh_request.rs::preserves_request_content_and_only_changes_output_budget_and_streaming
#[test]
fn preserves_request_content_and_only_changes_output_budget_and_streaming() {
    let original = json!({"model":"claude-example","max_tokens":900,"stream":true,
        "thinking":{"type":"adaptive"},"system":[{"type":"text","text":"prefix","cache_control":{"type":"ephemeral"}}],
        "messages":[{"role":"user","content":"hello"}],"metadata":{"user_id":"example"}});
    let mut expected = original.clone();
    expected["max_tokens"] = json!(0);
    expected.as_object_mut().expect("object").remove("stream");
    let prepared = claude_refresh_body(&original.to_string()).expect("a refreshable request");
    assert_eq!(
        serde_json::from_str::<Value>(&prepared).expect("valid JSON"),
        expected
    );
    assert_eq!(original["max_tokens"], 900);
}

/// Twin of cache_refresh_request.rs::rejects_documented_incompatible_output_modes
#[test]
fn rejects_documented_incompatible_output_modes() {
    let base = json!({"model":"claude-example","messages":[]});
    for (field, value, expected) in [
        (
            "thinking",
            json!({"type":"enabled"}),
            ClaudeRefreshError::ExtendedThinking,
        ),
        (
            "output_config",
            json!({"format":{"type":"json_schema"}}),
            ClaudeRefreshError::StructuredOutput,
        ),
        (
            "tool_choice",
            json!({"type":"tool","name":"x"}),
            ClaudeRefreshError::ForcedTool,
        ),
        (
            "tool_choice",
            json!({"type":"any"}),
            ClaudeRefreshError::ForcedTool,
        ),
        ("compaction", json!({}), ClaudeRefreshError::Compaction),
    ] {
        let mut request = base.clone();
        request[field] = value;
        assert_eq!(claude_refresh_body(&request.to_string()), Err(expected));
    }
    assert_eq!(
        claude_refresh_body("[]"),
        Err(ClaudeRefreshError::NotObject)
    );
    assert_eq!(
        claude_refresh_body("{}"),
        Err(ClaudeRefreshError::MissingModelOrMessages)
    );
}

/// Twin of cache_refresh_request.rs::retains_raw_nested_values_and_rejects_ambiguous_duplicate_fields
#[test]
fn retains_raw_nested_values_and_rejects_ambiguous_duplicate_fields() {
    let request = r#"{ "model":"claude-example", "messages" : [{"role":"user", "content":"a\\u0062"}], "tools":[{"z":1,"a":2}], "max_tokens":200,"stream":true }"#;
    let prepared = claude_refresh_body(request).expect("a refreshable request");
    assert!(prepared.contains(r#"[{"role":"user", "content":"a\\u0062"}]"#));
    assert!(prepared.contains(r#"[{"z":1,"a":2}]"#));
    assert_eq!(
        claude_refresh_body(r#"{"model":"a","model":"b","messages":[]}"#),
        Err(ClaudeRefreshError::InvalidJson)
    );
}

/// Twin of cache_refresh_transport.rs::expired_or_busy_capture_never_attempts_a_request
#[test]
fn expired_or_busy_capture_never_attempts_a_request() {
    let expired = capture(1_000);
    assert_eq!(
        prepare(&expired, 1_000, false),
        Err(RefreshOutcome::Expired)
    );
    assert_eq!(
        prepare(&expired, 5_000, false),
        Err(RefreshOutcome::Expired)
    );
    let busy = capture(1_000 + 60_000);
    assert_eq!(prepare(&busy, 1_000, true), Err(RefreshOutcome::Busy));
    assert!(prepare(&busy, 1_000, false).is_ok());
}

/// Twin of cache_refresh_transport.rs::cache_result_requires_the_full_previously_observed_prefix
#[test]
fn cache_result_requires_the_full_previously_observed_prefix() {
    assert_eq!(
        classify_usage(&json!({"usage":{"cache_read_input_tokens":100}}), 100),
        RefreshOutcome::CacheRead { tokens: 100 }
    );
    assert_eq!(
        classify_usage(&json!({"usage":{"cache_read_input_tokens":99}}), 100),
        RefreshOutcome::CacheMiss
    );
    assert_eq!(
        classify_usage(&json!({"usage":{"cache_read_input_tokens":0}}), 100),
        RefreshOutcome::CacheMiss
    );
    assert_eq!(
        classify_usage(&json!({"usage":{}}), 100),
        RefreshOutcome::Unknown
    );
    assert_eq!(
        classify_usage(&json!({"usage":{"cache_read_input_tokens":100}}), 0),
        RefreshOutcome::CacheMiss
    );
}

/// Twin of cache_refresh_transport.rs::only_a_confirmed_read_extends_the_lease_from_request_start
#[test]
fn only_a_confirmed_read_extends_the_lease_from_request_start() {
    let start = 5_000;
    let initial = start + 10_000;
    let mut request = capture(initial);
    let miss = json!({"usage":{"cache_read_input_tokens":99}}).to_string();
    assert_eq!(
        finish(&mut request, start, 200, miss.as_bytes()),
        RefreshOutcome::CacheMiss
    );
    assert_eq!(request.cache_expires_at_ms, initial);
    let unknown = json!({"usage":{}}).to_string();
    assert_eq!(
        finish(&mut request, start, 200, unknown.as_bytes()),
        RefreshOutcome::Unknown
    );
    assert_eq!(request.cache_expires_at_ms, initial);
    let hit = json!({"usage":{"cache_read_input_tokens":100}}).to_string();
    assert_eq!(
        finish(&mut request, start, 200, hit.as_bytes()),
        RefreshOutcome::CacheRead { tokens: 100 }
    );
    assert_eq!(request.cache_expires_at_ms, start + 60_000);
}

#[test]
fn a_prepared_refresh_drops_the_framing_headers_and_keeps_the_rest_in_order() {
    let prepared = prepare(&capture(100_000), 1_000, false).expect("eligible");
    assert_eq!(prepared.endpoint, "https://example.invalid/v1/messages");
    assert_eq!(
        prepared.headers,
        vec![
            ("x-api-key".to_owned(), "secret-value".to_owned()),
            ("anthropic-version".to_owned(), "2023-06-01".to_owned()),
        ]
    );
    assert_eq!(prepared.started_ms, 1_000);
    let body: Value = serde_json::from_str(&prepared.body).expect("valid JSON");
    assert_eq!(body["max_tokens"], 0);
    assert!(body.get("stream").is_none());
}

#[test]
fn a_capture_without_an_observed_cache_or_lifetime_is_unsupported() {
    let mut without_tokens = capture(100_000);
    without_tokens.expected_cached_tokens = 0;
    assert_eq!(
        prepare(&without_tokens, 0, false),
        Err(RefreshOutcome::Unsupported)
    );
    let mut without_lifetime = capture(100_000);
    without_lifetime.cache_lifetime_ms = 0;
    assert_eq!(
        prepare(&without_lifetime, 0, false),
        Err(RefreshOutcome::Unsupported)
    );
}

#[test]
fn an_incompatible_request_body_is_unsupported_and_sends_nothing() {
    let mut forced = capture(100_000);
    forced.body = r#"{"model":"m","messages":[],"tool_choice":{"type":"any"}}"#.to_owned();
    assert_eq!(prepare(&forced, 0, false), Err(RefreshOutcome::Unsupported));
    let mut broken = capture(100_000);
    broken.body = "not json".to_owned();
    assert_eq!(prepare(&broken, 0, false), Err(RefreshOutcome::Unsupported));
}

#[test]
fn a_busy_gate_wins_over_an_expired_capture() {
    let expired = capture(10);
    assert_eq!(prepare(&expired, 100, true), Err(RefreshOutcome::Busy));
}

#[test]
fn a_failure_status_is_rejected_and_never_extends_the_lease() {
    let mut request = capture(15_000);
    let hit = json!({"usage":{"cache_read_input_tokens":100}}).to_string();
    for status in [199_u16, 300, 401, 429, 500] {
        assert_eq!(
            finish(&mut request, 5_000, status, hit.as_bytes()),
            RefreshOutcome::Rejected { status }
        );
    }
    assert_eq!(request.cache_expires_at_ms, 15_000);
}

#[test]
fn an_oversized_or_unreadable_response_is_unknown() {
    let mut request = capture(15_000);
    let oversized = vec![b' '; MAX_RESPONSE_BYTES + 1];
    assert_eq!(
        finish(&mut request, 5_000, 200, &oversized),
        RefreshOutcome::Unknown
    );
    assert_eq!(
        finish(&mut request, 5_000, 200, b"not json"),
        RefreshOutcome::Unknown
    );
    assert_eq!(
        finish(&mut request, 5_000, 200, b""),
        RefreshOutcome::Unknown
    );
    assert_eq!(request.cache_expires_at_ms, 15_000);
}

#[test]
fn the_lease_does_not_overflow_at_the_end_of_time() {
    let mut request = capture(u64::MAX);
    let hit = json!({"usage":{"cache_read_input_tokens":100}}).to_string();
    assert_eq!(
        finish(&mut request, u64::MAX, 200, hit.as_bytes()),
        RefreshOutcome::CacheRead { tokens: 100 }
    );
    assert_eq!(request.cache_expires_at_ms, u64::MAX);
}

#[test]
fn debug_output_names_no_credential_endpoint_or_prompt() {
    let request = capture(100_000);
    let prepared = prepare(&request, 0, false).expect("eligible");
    for text in [format!("{request:?}"), format!("{prepared:?}")] {
        assert!(!text.contains("secret-value"), "{text}");
        assert!(!text.contains("example.invalid"), "{text}");
        assert!(!text.contains("messages"), "{text}");
    }
}
