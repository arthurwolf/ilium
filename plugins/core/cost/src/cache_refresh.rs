//! Prompt-cache refresh requests (manual page core-plugin-cost.md, "Cache-wait observation and
//! refresh requests", PRM-517).
//!
//! A provider cache expires when an agent waits too long between two requests. A refresh repeats the
//! cached prefix of the original request with no output budget, so the provider reads the cache and
//! extends its lifetime. This module holds the decisions and the bytes of such a request as plain
//! data: it neither captures credentials nor sends anything. The caller (a trusted agent transport)
//! owns the capture, sends [`PreparedRefresh`] with its own `http` import, and passes the response
//! back to [`finish`].
//!
//! A [`Capture`] holds an endpoint, headers (which may carry credentials) and the prompt body. It is
//! never put on a service, a topic or a message, never logged, and its `Debug` output names no
//! field values. The owner drops it when the invocation, the account, the endpoint, the cache
//! prefix or the transport changes.
//!
//! Time is a plain millisecond count on any monotonic clock the caller chooses, used consistently.

use std::collections::HashSet;
use std::fmt;

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{value::RawValue, Value};

/// Why a Claude Messages request cannot be turned into a zero-output refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeRefreshError {
    NotObject,
    MissingModelOrMessages,
    ExtendedThinking,
    StructuredOutput,
    ForcedTool,
    Compaction,
    InvalidJson,
}

/// Preserve every original top-level value, including nested object ordering,
/// string escapes and whitespace, rather than reserializing prompt content.
struct RawFields(Vec<(String, Box<RawValue>)>);

impl<'de> Deserialize<'de> for RawFields {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct FieldsVisitor;
        impl<'de> Visitor<'de> for FieldsVisitor {
            type Value = RawFields;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an object without duplicate keys")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<RawFields, A::Error> {
                let mut keys = HashSet::new();
                let mut fields = Vec::new();
                while let Some((key, value)) = map.next_entry::<String, Box<RawValue>>()? {
                    if !keys.insert(key.clone()) {
                        return Err(serde::de::Error::custom("duplicate request field"));
                    }
                    fields.push((key, value));
                }
                Ok(RawFields(fields))
            }
        }
        deserializer.deserialize_map(FieldsVisitor)
    }
}

/// Prepare a Claude Messages request for a zero-output refresh.
///
/// Anthropic documents these exclusions and requires the original headers:
/// <https://platform.claude.com/docs/en/about-claude/models/optimizing-for-cost-and-intelligence>
/// <https://platform.claude.com/docs/en/build-with-claude/prompt-caching>
///
/// Returned bytes retain the raw original values in their original order;
/// the caller must still preserve the account, endpoint, and headers.
pub fn claude_refresh_body(original: &str) -> Result<String, ClaudeRefreshError> {
    let parsed: Value =
        serde_json::from_str(original).map_err(|_| ClaudeRefreshError::InvalidJson)?;
    let source = parsed.as_object().ok_or(ClaudeRefreshError::NotObject)?;
    let fields: RawFields =
        serde_json::from_str(original).map_err(|_| ClaudeRefreshError::InvalidJson)?;
    if source
        .get("model")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .is_none()
        || !source.get("messages").is_some_and(Value::is_array)
    {
        return Err(ClaudeRefreshError::MissingModelOrMessages);
    }
    if source
        .get("thinking")
        .and_then(|value| value.get("type"))
        .and_then(Value::as_str)
        == Some("enabled")
    {
        return Err(ClaudeRefreshError::ExtendedThinking);
    }
    if source
        .get("output_config")
        .and_then(|value| value.get("format"))
        .is_some_and(|value| !value.is_null())
    {
        return Err(ClaudeRefreshError::StructuredOutput);
    }
    if matches!(
        source
            .get("tool_choice")
            .and_then(|value| value.get("type"))
            .and_then(Value::as_str),
        Some("tool" | "any")
    ) {
        return Err(ClaudeRefreshError::ForcedTool);
    }
    if source
        .get("compaction")
        .is_some_and(|value| !value.is_null())
    {
        return Err(ClaudeRefreshError::Compaction);
    }
    let mut body = String::from("{");
    for (key, value) in fields.0 {
        if key == "stream" || key == "max_tokens" {
            continue;
        }
        if body.len() > 1 {
            body.push(',');
        }
        body.push_str(&serde_json::to_string(&key).map_err(|_| ClaudeRefreshError::InvalidJson)?);
        body.push(':');
        body.push_str(value.get());
    }
    if body.len() > 1 {
        body.push(',');
    }
    body.push_str("\"max_tokens\":0}");
    Ok(body)
}

/// The largest response body a refresh reads; a longer one counts as `Unknown`.
pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Headers that describe the framing of the original body. The refresh body has another length,
/// so the transport regenerates them.
const FRAMING_HEADERS: [&str; 4] = ["content-length", "host", "transfer-encoding", "connection"];

/// What a refresh attempt established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshOutcome {
    /// The provider read at least the previously observed cached prefix.
    CacheRead { tokens: u64 },
    /// The cache had already expired when the attempt started; nothing was sent.
    Expired,
    /// Another request on the same capture was in flight; nothing was sent.
    Busy,
    /// The capture cannot be refreshed (no observed cache, no lifetime, or an incompatible request).
    Unsupported,
    /// The provider read less than the observed prefix: the cache was missed in part or in full.
    CacheMiss,
    /// The provider answered with a failure status.
    Rejected { status: u16 },
    /// The transport failed, the response was oversized or unreadable, or it carried no usage.
    Unknown,
}

/// An already captured, verified Claude Messages request. Only a trusted agent transport builds
/// one from its own request. The `Debug` output shows no endpoint, header or body.
#[derive(Clone, PartialEq, Eq)]
pub struct Capture {
    pub endpoint: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    /// When the cache is positively known to expire, in milliseconds on the caller's clock.
    pub cache_expires_at_ms: u64,
    /// Shortest verified lifetime among the cached prefix segments being kept. A capture owner
    /// must not infer this from a provider's default setting.
    pub cache_lifetime_ms: u64,
    /// Previously observed cached prefix size for this exact request. A shorter read may refresh
    /// only part of the context.
    pub expected_cached_tokens: u64,
}

impl fmt::Debug for Capture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Capture")
            .field("headers", &self.headers.len())
            .field("body_bytes", &self.body.len())
            .field("cache_expires_at_ms", &self.cache_expires_at_ms)
            .field("expected_cached_tokens", &self.expected_cached_tokens)
            .finish_non_exhaustive()
    }
}

impl Capture {
    /// Only a confirmed read extends the lease, and it extends it from the start of the request,
    /// not from the end of the response.
    fn record_refresh(&mut self, started_ms: u64, outcome: RefreshOutcome) {
        if matches!(outcome, RefreshOutcome::CacheRead { .. }) {
            if let Some(expires_at) = started_ms.checked_add(self.cache_lifetime_ms) {
                self.cache_expires_at_ms = expires_at;
            }
        }
    }
}

/// The request to send for one refresh. The `Debug` output shows no endpoint, header or body.
#[derive(Clone, PartialEq, Eq)]
pub struct PreparedRefresh {
    pub endpoint: String,
    /// The original headers without the framing headers.
    pub headers: Vec<(String, String)>,
    pub body: String,
    /// The time the attempt started; pass it back to [`finish`].
    pub started_ms: u64,
}

impl fmt::Debug for PreparedRefresh {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedRefresh")
            .field("headers", &self.headers.len())
            .field("body_bytes", &self.body.len())
            .field("started_ms", &self.started_ms)
            .finish_non_exhaustive()
    }
}

/// Decides whether `capture` may be refreshed at `now_ms` and builds the request. `Err` carries the
/// outcome that ends the attempt without sending anything.
///
/// `gate_busy` is true while the agent transport has an ordinary request in flight on the same
/// capture, so a refresh never races a new turn or replays a stale prefix.
pub fn prepare(
    capture: &Capture,
    now_ms: u64,
    gate_busy: bool,
) -> Result<PreparedRefresh, RefreshOutcome> {
    if gate_busy {
        return Err(RefreshOutcome::Busy);
    }
    if now_ms >= capture.cache_expires_at_ms {
        return Err(RefreshOutcome::Expired);
    }
    if capture.expected_cached_tokens == 0 || capture.cache_lifetime_ms == 0 {
        return Err(RefreshOutcome::Unsupported);
    }
    let body = claude_refresh_body(&capture.body).map_err(|_| RefreshOutcome::Unsupported)?;
    let headers = capture
        .headers
        .iter()
        .filter(|(name, _)| {
            !FRAMING_HEADERS
                .iter()
                .any(|framing| name.eq_ignore_ascii_case(framing))
        })
        .cloned()
        .collect();
    Ok(PreparedRefresh {
        endpoint: capture.endpoint.clone(),
        headers,
        body,
        started_ms: now_ms,
    })
}

/// Reads the answer to a prepared refresh. `status` is the HTTP status and `response` the body.
/// A transport failure that produced no answer is [`RefreshOutcome::Unknown`] and needs no call.
/// No response body or transport error is surfaced: both may contain secrets.
pub fn finish(
    capture: &mut Capture,
    started_ms: u64,
    status: u16,
    response: &[u8],
) -> RefreshOutcome {
    if !(200..300).contains(&status) {
        return RefreshOutcome::Rejected { status };
    }
    if response.len() > MAX_RESPONSE_BYTES {
        return RefreshOutcome::Unknown;
    }
    let Ok(body) = serde_json::from_slice::<Value>(response) else {
        return RefreshOutcome::Unknown;
    };
    let outcome = classify_usage(&body, capture.expected_cached_tokens);
    capture.record_refresh(started_ms, outcome);
    outcome
}

/// Compares the cache tokens the provider read with the prefix size observed before.
pub fn classify_usage(body: &Value, expected: u64) -> RefreshOutcome {
    match body
        .get("usage")
        .and_then(|value| value.get("cache_read_input_tokens"))
        .and_then(Value::as_u64)
    {
        Some(tokens) if expected > 0 && tokens >= expected => RefreshOutcome::CacheRead { tokens },
        Some(_) => RefreshOutcome::CacheMiss,
        None => RefreshOutcome::Unknown,
    }
}
