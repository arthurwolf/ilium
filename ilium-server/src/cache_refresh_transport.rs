//! Transport for an already captured, verified Claude Messages request.
//!
//! Captures contain credentials and prompt contents: never persist, log,
//! format, or expose them. The owner must invalidate them when the invocation,
//! account, endpoint, cache prefix, or original transport changes.

use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::header::{HeaderMap, CONNECTION, CONTENT_LENGTH, HOST, TRANSFER_ENCODING};
use reqwest::{Client, Url};
use tokio::sync::Mutex;

use crate::cache_refresh_request::claude_refresh_body;

const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Only a trusted agent transport may construct this from its own request.
/// `gate` must also be held by the agent transport for ordinary requests, so
/// a refresh cannot race a new turn or replay a stale prefix.
pub(crate) struct CapturedClaudeRequest {
    pub endpoint: Url,
    pub headers: HeaderMap,
    pub body: String,
    pub cache_expires_at: Instant,
    /// Shortest verified lifetime among the cached prefix segments being kept.
    /// A capture owner must not infer this from a provider's default setting.
    pub cache_lifetime: Duration,
    /// Previously observed cached prefix size for this exact request. A
    /// shorter read may refresh only part of the context.
    pub expected_cached_tokens: u64,
    pub gate: Arc<Mutex<()>>,
}

impl CapturedClaudeRequest {
    fn record_refresh(&mut self, started: Instant, outcome: RefreshOutcome) {
        if matches!(outcome, RefreshOutcome::CacheRead { .. }) {
            if let Some(expires_at) = started.checked_add(self.cache_lifetime) {
                self.cache_expires_at = expires_at;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RefreshOutcome {
    CacheRead { tokens: u64 },
    Expired,
    Busy,
    Unsupported,
    CacheMiss,
    Rejected { status: u16 },
    Unknown,
}

/// The capture owner must determine the effective route for this particular
/// request, including any proxy bypass rules. A proxy URL may contain secrets;
/// never log or format it.
pub(crate) enum CapturedRoute<'a> {
    Direct,
    Proxy(&'a str),
}

/// Never follow redirects with authorization headers.
pub(crate) struct ClaudeRefreshClient(Client);

pub(crate) fn claude_refresh_client(route: CapturedRoute<'_>) -> Result<ClaudeRefreshClient, ()> {
    let builder = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30));
    let builder = match route {
        CapturedRoute::Direct => builder,
        CapturedRoute::Proxy(url) => builder.proxy(reqwest::Proxy::all(url).map_err(|_| ())?),
    };
    builder.build().map(ClaudeRefreshClient).map_err(|_| ())
}

/// Request is only eligible before its positively established expiration.
/// No response body or transport error is surfaced: both may contain secrets.
pub(crate) async fn refresh_claude(
    client: &ClaudeRefreshClient,
    captured: &mut CapturedClaudeRequest,
) -> RefreshOutcome {
    let gate = Arc::clone(&captured.gate);
    let Ok(_guard) = gate.try_lock() else {
        return RefreshOutcome::Busy;
    };
    if Instant::now() >= captured.cache_expires_at {
        return RefreshOutcome::Expired;
    }
    if captured.expected_cached_tokens == 0 || captured.cache_lifetime.is_zero() {
        return RefreshOutcome::Unsupported;
    }
    let Ok(body) = claude_refresh_body(&captured.body) else {
        return RefreshOutcome::Unsupported;
    };
    let mut headers = captured.headers.clone();
    // The body length changed; transport-specific framing is regenerated.
    headers.remove(CONTENT_LENGTH);
    headers.remove(HOST);
    headers.remove(TRANSFER_ENCODING);
    headers.remove(CONNECTION);
    // The lifetime starts at request start, not response completion. Check
    // again after preparing the body so a stale capture is never sent.
    let request_started = Instant::now();
    if request_started >= captured.cache_expires_at {
        return RefreshOutcome::Expired;
    }
    let Ok(mut response) = client
        .0
        .post(captured.endpoint.clone())
        .headers(headers)
        .body(body)
        .send()
        .await
    else {
        return RefreshOutcome::Unknown;
    };
    if !response.status().is_success() {
        return RefreshOutcome::Rejected {
            status: response.status().as_u16(),
        };
    }
    let mut bytes = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) if bytes.len().saturating_add(chunk.len()) <= MAX_RESPONSE_BYTES => {
                bytes.extend_from_slice(&chunk)
            }
            Ok(Some(_)) | Err(_) => return RefreshOutcome::Unknown,
            Ok(None) => break,
        }
    }
    let Ok(body) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return RefreshOutcome::Unknown;
    };
    let outcome = classify_usage(&body, captured.expected_cached_tokens);
    captured.record_refresh(request_started, outcome);
    outcome
}

fn classify_usage(body: &serde_json::Value, expected: u64) -> RefreshOutcome {
    match body
        .get("usage")
        .and_then(|v| v.get("cache_read_input_tokens"))
        .and_then(|v| v.as_u64())
    {
        Some(tokens) if expected > 0 && tokens >= expected => RefreshOutcome::CacheRead { tokens },
        Some(_) => RefreshOutcome::CacheMiss,
        None => RefreshOutcome::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture(expiry: Instant) -> CapturedClaudeRequest {
        CapturedClaudeRequest {
            endpoint: Url::parse("https://example.invalid/v1/messages").unwrap(),
            headers: HeaderMap::new(),
            body: r#"{"model":"example","messages":[]}"#.to_owned(),
            cache_expires_at: expiry,
            cache_lifetime: Duration::from_secs(60),
            expected_cached_tokens: 100,
            gate: Arc::new(Mutex::new(())),
        }
    }

    #[tokio::test]
    async fn expired_or_busy_capture_never_attempts_a_request() {
        let client = claude_refresh_client(CapturedRoute::Direct).unwrap();
        let mut expired = capture(Instant::now());
        assert_eq!(
            refresh_claude(&client, &mut expired).await,
            RefreshOutcome::Expired
        );
        let mut busy = capture(Instant::now() + Duration::from_secs(60));
        let gate = Arc::clone(&busy.gate);
        let _guard = gate.lock().await;
        assert_eq!(
            refresh_claude(&client, &mut busy).await,
            RefreshOutcome::Busy
        );
    }

    #[test]
    fn cache_result_requires_the_full_previously_observed_prefix() {
        use serde_json::json;
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

    #[test]
    fn only_a_confirmed_read_extends_the_lease_from_request_start() {
        let start = Instant::now();
        let initial = start + Duration::from_secs(10);
        let mut request = capture(initial);
        request.record_refresh(start, RefreshOutcome::CacheMiss);
        assert_eq!(request.cache_expires_at, initial);
        request.record_refresh(start, RefreshOutcome::Unknown);
        assert_eq!(request.cache_expires_at, initial);
        request.record_refresh(start, RefreshOutcome::CacheRead { tokens: 100 });
        assert_eq!(request.cache_expires_at, start + Duration::from_secs(60));
    }
}
