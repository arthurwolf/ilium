//! Small, provider-neutral boundary for Kilo Gateway chat completions.
//!
//! The rest of Ilium depends only on this adapter's completion and free-model
//! discovery contracts. It never needs to know about Kilo's HTTP endpoints,
//! retryable status codes, or OpenAI-compatible response envelopes.

use std::io::{BufRead, BufReader};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Kilo's documented OpenAI-compatible base URL.
pub const DEFAULT_BASE_URL: &str = "https://api.kilo.ai/api/gateway";
/// Kilo's stable native free router, selected on a clean installation.
pub const DEFAULT_FREE_MODEL: &str = "kilo-auto/free";
/// Used when the selected provider/model does not publish a reliable output
/// limit. Callers control response length through the prompt.
pub const UNKNOWN_MODEL_MAX_OUTPUT_TOKENS: u32 = 1_000_000;
/// Stable virtual routes retained when live model discovery is unavailable.
pub const FALLBACK_FREE_MODELS: [&str; 2] = ["kilo-auto/free", "openrouter/free"];
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAXIMUM_COMPLETION_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;
const MAXIMUM_STREAM_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAXIMUM_MODEL_CATALOG_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;

/// One paid egress proxy used to reach Kilo Gateway instead of calling it
/// directly. Shape mirrors the `paid_proxies` row used elsewhere (ip, port,
/// CONNECT protocol, optional credentials) so operators can copy entries
/// from an existing paid-proxy list verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PaidProxy {
    pub ip: String,
    pub port: u16,
    pub protocol: String,
    pub username: String,
    pub password: String,
}

impl Default for PaidProxy {
    fn default() -> Self {
        Self {
            ip: String::new(),
            port: 0,
            protocol: "http".to_string(),
            username: String::new(),
            password: String::new(),
        }
    }
}

impl PaidProxy {
    /// Full CONNECT URL `protocol://[user:pass@]ip:port` -- credentials only
    /// when both fields are non-empty, so an IP-authorized proxy stays the
    /// clean `ip:port` form. User-info bytes are percent-encoded so a proxy
    /// credential loaded from MongoDB cannot change the URL delimiters.
    pub fn connect_url(&self) -> String {
        let credentials = if !self.username.is_empty() && !self.password.is_empty() {
            format!(
                "{}:{}@",
                encode_proxy_user_info(&self.username),
                encode_proxy_user_info(&self.password)
            )
        } else {
            String::new()
        };
        format!(
            "{}://{}{}:{}",
            self.protocol, credentials, self.ip, self.port
        )
    }
}

fn encode_proxy_user_info(value: &str) -> String {
    const HEX_DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

/// Picks one proxy uniformly at random from `proxies`, or `None` when the
/// list is empty.
pub fn choose_random_paid_proxy(proxies: &[PaidProxy]) -> Option<&PaidProxy> {
    if proxies.is_empty() {
        return None;
    }
    proxies.get(rand::random_range(0..proxies.len()))
}

/// One text-only OpenAI-compatible chat message.
#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: &'static str,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system",
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user",
            content: content.into(),
        }
    }
}

/// Inputs that affect one completion request, independent from the HTTP client.
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: u32,
    pub temperature: f32,
}

/// Provider-neutral facts emitted while an OpenAI-compatible completion is
/// streaming. The callback controls cancellation: returning `false` closes
/// the response body immediately and ends the request successfully.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionStreamEvent {
    TextDelta(String),
    OutputTokens(u64),
}

impl CompletionRequest {
    /// Builds a deterministic, non-streaming request with the caller's exact
    /// output budget. Provider-neutral callers own that budget because a
    /// project restructure needs materially more output than a title.
    pub fn new(model: impl Into<String>, messages: Vec<ChatMessage>, max_tokens: u32) -> Self {
        Self {
            model: model.into(),
            messages,
            max_tokens,
            temperature: 0.0,
        }
    }

    pub fn with_default_free_model(messages: Vec<ChatMessage>) -> Self {
        Self::new(
            DEFAULT_FREE_MODEL,
            messages,
            UNKNOWN_MODEL_MAX_OUTPUT_TOKENS,
        )
    }
}

/// Retry policy for temporary gateway or upstream failures.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_attempts: u8,
    pub initial_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 4,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(8),
        }
    }
}

/// Failure information stable enough for UI workflows to decide whether to retry later.
#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("Kilo Gateway returned HTTP {status}: {message}")]
    Http { status: u16, message: String },
    #[error("could not send request to Kilo Gateway: {0}")]
    Transport(String),
    #[error("Kilo Gateway returned an invalid response: {0}")]
    InvalidResponse(String),
    #[error("invalid paid proxy URL: {0}")]
    InvalidProxy(String),
}

impl GatewayError {
    fn is_retryable(&self) -> bool {
        matches!(self, Self::Transport(_) | Self::InvalidResponse(_))
            || matches!(
                self,
                Self::Http {
                    status: 429 | 500 | 502 | 503 | 504,
                    ..
                }
            )
    }
}

/// Client for Kilo Gateway's OpenAI-compatible `/chat/completions` endpoint.
pub struct KiloGatewayClient {
    base_url: String,
    retry_policy: RetryPolicy,
    /// Full CONNECT URL of a paid proxy this client should route every
    /// request through, when the caller opted into paid-proxy egress.
    proxy_url: Option<String>,
}

impl Default for KiloGatewayClient {
    fn default() -> Self {
        Self::new(DEFAULT_BASE_URL, RetryPolicy::default())
    }
}

impl KiloGatewayClient {
    pub fn new(base_url: impl Into<String>, retry_policy: RetryPolicy) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            retry_policy,
            proxy_url: None,
        }
    }

    /// Routes every request this client sends through the given proxy's
    /// CONNECT URL instead of calling Kilo Gateway directly.
    pub fn with_proxy_url(mut self, proxy_url: impl Into<String>) -> Self {
        self.proxy_url = Some(proxy_url.into());
        self
    }

    fn build_agent(&self) -> Result<ureq::Agent, GatewayError> {
        let mut config = ureq::Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .http_status_as_error(false);
        if let Some(proxy_url) = &self.proxy_url {
            let proxy = ureq::Proxy::new(proxy_url).map_err(|error| {
                GatewayError::InvalidProxy(format!(
                    "{}: {error}",
                    ilium_logging::redacted_url(proxy_url)
                ))
            })?;
            config = config.proxy(Some(proxy));
        }
        Ok(ureq::Agent::new_with_config(config.build()))
    }

    /// Sends a non-streaming completion and returns only its assistant text.
    ///
    /// This intentionally omits `Authorization`: Kilo identifies anonymous
    /// free-model calls by public IP.
    pub fn complete_text(&self, request: &CompletionRequest) -> Result<String, GatewayError> {
        self.complete_text_with_sender(request, |payload| self.send_once(payload))
    }

    /// Streams assistant text as Kilo's OpenAI-compatible SSE response
    /// arrives. A streaming request is deliberately not retried after the
    /// response starts: replaying an unknown prefix would duplicate semantic
    /// records at the caller.
    pub fn stream_text(
        &self,
        request: &CompletionRequest,
        on_event: &mut dyn FnMut(CompletionStreamEvent) -> bool,
    ) -> Result<(), GatewayError> {
        let payload = ChatCompletionPayload {
            model: &request.model,
            messages: &request.messages,
            max_tokens: request.max_tokens,
            temperature: request.temperature,
            stream: true,
            stream_options: Some(StreamOptions {
                include_usage: true,
            }),
        };
        let url = format!("{}/chat/completions", self.base_url);
        let diagnostic_url = ilium_logging::redacted_url(&url);
        let agent = self.build_agent()?;
        let mut response = agent
            .post(&url)
            .header("Content-Type", "application/json")
            .send_json(&payload)
            .map_err(|error| {
                GatewayError::Transport(error.to_string().replace(&url, &diagnostic_url))
            })?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            let message = response
                .body_mut()
                .with_config()
                .limit(MAXIMUM_COMPLETION_RESPONSE_BYTES)
                .read_to_string()
                .map_err(|error| GatewayError::Transport(error.to_string()))?;
            return Err(GatewayError::Http { status, message });
        }

        let mut reader = BufReader::new(response.body_mut().as_reader());
        let mut line = String::new();
        let mut received_bytes = 0usize;
        loop {
            line.clear();
            let read = reader
                .read_line(&mut line)
                .map_err(|error| GatewayError::Transport(error.to_string()))?;
            if read == 0 {
                return Ok(());
            }
            received_bytes = received_bytes.saturating_add(read);
            if received_bytes > MAXIMUM_STREAM_RESPONSE_BYTES {
                return Err(GatewayError::InvalidResponse(
                    "stream exceeded the 16 MiB response limit".to_string(),
                ));
            }
            let Some(data) = line.trim().strip_prefix("data:").map(str::trim) else {
                continue;
            };
            if data == "[DONE]" {
                return Ok(());
            }
            let value: serde_json::Value = serde_json::from_str(data)
                .map_err(|error| GatewayError::InvalidResponse(error.to_string()))?;
            if let Some(tokens) = value
                .get("usage")
                .and_then(|usage| usage.get("completion_tokens"))
                .and_then(serde_json::Value::as_u64)
            {
                if !on_event(CompletionStreamEvent::OutputTokens(tokens)) {
                    return Ok(());
                }
            }
            if let Some(text) = value
                .pointer("/choices/0/delta/content")
                .and_then(serde_json::Value::as_str)
            {
                if !text.is_empty() && !on_event(CompletionStreamEvent::TextDelta(text.to_string()))
                {
                    return Ok(());
                }
            }
        }
    }

    /// Fetches Kilo's unauthenticated live catalog and returns only free,
    /// text-generating models that accept `max_tokens`.
    pub fn list_free_models(&self) -> Result<Vec<String>, GatewayError> {
        let catalog: ModelCatalogResponse = self.get_json("models")?;
        Ok(filter_free_text_models(catalog.data))
    }

    fn complete_text_with_sender<F>(
        &self,
        request: &CompletionRequest,
        mut send: F,
    ) -> Result<String, GatewayError>
    where
        F: FnMut(&ChatCompletionPayload) -> Result<ChatCompletionResponse, GatewayError>,
    {
        let payload = ChatCompletionPayload {
            model: &request.model,
            messages: &request.messages,
            max_tokens: request.max_tokens,
            temperature: request.temperature,
            stream: false,
            stream_options: None,
        };
        let attempts = self.retry_policy.max_attempts.max(1);
        // Clamp up front, not just on the doubling step below: a caller-built
        // `RetryPolicy` with `initial_delay > max_delay` would otherwise let
        // the very first sleep exceed the policy's own stated ceiling.
        let mut delay = self
            .retry_policy
            .initial_delay
            .min(self.retry_policy.max_delay);

        for attempt in 1..=attempts {
            tracing::info!(
                provider = "kilo_gateway",
                attempt,
                attempts,
                model = payload.model,
                max_tokens = payload.max_tokens,
                message_count = payload.messages.len(),
                prompt_characters = payload
                    .messages
                    .iter()
                    .map(|message| message.content.chars().count())
                    .sum::<usize>(),
                "LLM provider attempt started"
            );
            tracing::debug!(
                provider = "kilo_gateway",
                attempt,
                request_body = %serde_json::to_string(&payload).unwrap_or_else(|error| format!("<serialization failed: {error}>")),
                "LLM provider attempt payload"
            );
            match send(&payload) {
                Ok(response) => {
                    let routed_model = response
                        .model
                        .clone()
                        .unwrap_or_else(|| "<not reported>".to_string());
                    let finish_reason = response
                        .choices
                        .first()
                        .and_then(|choice| choice.finish_reason.as_deref())
                        .unwrap_or("<not reported>")
                        .to_string();
                    match response.assistant_text() {
                        Ok(text) => {
                            tracing::info!(
                                provider = "kilo_gateway",
                                attempt,
                                routed_model = %routed_model,
                                finish_reason = %finish_reason,
                                response_characters = text.chars().count(),
                                "LLM provider attempt completed"
                            );
                            tracing::debug!(provider = "kilo_gateway", attempt, response_text = %text, "LLM provider attempt response");
                            return Ok(text);
                        }
                        Err(error) if attempt < attempts => {
                            tracing::warn!(
                                provider = "kilo_gateway",
                                attempt,
                                routed_model = %routed_model,
                                finish_reason = %finish_reason,
                                error_kind = gateway_error_kind(&error),
                                retry_delay_milliseconds = delay.as_millis(),
                                "LLM provider response validation failed and will retry"
                            );
                            tracing::debug!(provider = "kilo_gateway", attempt, error = %error, error_debug = ?error, "LLM provider response validation details");
                            thread::sleep(delay);
                            delay = delay.saturating_mul(2).min(self.retry_policy.max_delay);
                        }
                        Err(error) => {
                            tracing::error!(
                                provider = "kilo_gateway",
                                attempt,
                                routed_model = %routed_model,
                                finish_reason = %finish_reason,
                                error_kind = gateway_error_kind(&error),
                                "LLM provider response validation failed"
                            );
                            tracing::debug!(provider = "kilo_gateway", attempt, error = %error, error_debug = ?error, "LLM provider response validation details");
                            return Err(error);
                        }
                    }
                }
                Err(error) if error.is_retryable() && attempt < attempts => {
                    tracing::warn!(
                        provider = "kilo_gateway",
                        attempt,
                        error_kind = gateway_error_kind(&error),
                        http_status = ?gateway_http_status(&error),
                        retry_delay_milliseconds = delay.as_millis(),
                        "LLM provider attempt failed and will retry"
                    );
                    tracing::debug!(provider = "kilo_gateway", attempt, error = %error, error_debug = ?error, "LLM provider attempt failure details");
                    thread::sleep(delay);
                    delay = delay.saturating_mul(2).min(self.retry_policy.max_delay);
                }
                Err(error) => {
                    tracing::error!(provider = "kilo_gateway", attempt, error_kind = gateway_error_kind(&error), http_status = ?gateway_http_status(&error), "LLM provider attempt failed");
                    tracing::debug!(provider = "kilo_gateway", attempt, error = %error, error_debug = ?error, "LLM provider attempt failure details");
                    return Err(error);
                }
            }
        }
        unreachable!("the non-empty retry loop always returns")
    }

    fn send_once(
        &self,
        payload: &ChatCompletionPayload<'_>,
    ) -> Result<ChatCompletionResponse, GatewayError> {
        let url = format!("{}/chat/completions", self.base_url);
        let diagnostic_url = ilium_logging::redacted_url(&url);
        tracing::info!(
            method = "POST",
            url = %diagnostic_url,
            headers = ?[("Content-Type", "application/json")],
            request_characters = serde_json::to_string(payload).map(|body| body.chars().count()).unwrap_or_default(),
            "HTTP request started"
        );
        tracing::debug!(
            method = "POST",
            url = %diagnostic_url,
            request_body = %serde_json::to_string(payload).unwrap_or_else(|error| format!("<serialization failed: {error}>")),
            "HTTP request payload"
        );
        // A one-shot UI enrichment must never leave its tracked worker
        // waiting indefinitely on a broken network path.
        let agent = self.build_agent()?;
        let mut response = match agent
            .post(&url)
            .header("Content-Type", "application/json")
            .send_json(payload)
        {
            Ok(response) => response,
            Err(error) => {
                let error_message = error.to_string().replace(&url, &diagnostic_url);
                tracing::error!(method = "POST", url = %diagnostic_url, error = %error_message, "HTTP transport failed");
                return Err(GatewayError::Transport(error_message));
            }
        };
        let status = response.status().as_u16();
        let response_headers = redacted_response_headers(response.headers());
        let body = response
            .body_mut()
            .with_config()
            .limit(MAXIMUM_COMPLETION_RESPONSE_BYTES)
            .read_to_string()
            .map_err(|error| {
                tracing::error!(method = "POST", url = %diagnostic_url, status, ?response_headers, error = %error, "failed to read HTTP response body");
                GatewayError::Transport(error.to_string())
            })?;
        if !(200..300).contains(&status) {
            tracing::error!(method = "POST", url = %diagnostic_url, status, ?response_headers, response_characters = body.chars().count(), "HTTP request failed");
            tracing::debug!(method = "POST", url = %diagnostic_url, status, response_body = %body, "HTTP error response body");
            return Err(GatewayError::Http {
                status,
                message: body,
            });
        }
        tracing::info!(method = "POST", url = %diagnostic_url, status, ?response_headers, response_characters = body.chars().count(), "HTTP request completed");
        tracing::debug!(method = "POST", url = %diagnostic_url, status, response_body = %body, "HTTP response body");
        serde_json::from_str(&body).map_err(|error| {
            tracing::error!(
                method = "POST",
                url = %diagnostic_url,
                status,
                ?response_headers,
                response_characters = body.chars().count(),
                error = %error,
                "HTTP response was not valid JSON"
            );
            GatewayError::InvalidResponse(error.to_string())
        })
    }

    /// Sends one unauthenticated JSON GET against the adapter-owned base URL.
    fn get_json<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T, GatewayError> {
        let url = format!("{}/{}", self.base_url, path.trim_start_matches('/'));
        let diagnostic_url = ilium_logging::redacted_url(&url);
        tracing::info!(method = "GET", url = %diagnostic_url, "HTTP request started");
        let agent = self.build_agent()?;
        let mut response = agent.get(&url).call().map_err(|error| {
            let message = error.to_string().replace(&url, &diagnostic_url);
            tracing::error!(method = "GET", url = %diagnostic_url, error = %message, "HTTP transport failed");
            GatewayError::Transport(message)
        })?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(MAXIMUM_MODEL_CATALOG_RESPONSE_BYTES)
            .read_to_string()
            .map_err(|error| {
                tracing::error!(method = "GET", url = %diagnostic_url, status, error = %error, "failed to read HTTP response body");
                GatewayError::Transport(error.to_string())
            })?;
        if !(200..300).contains(&status) {
            tracing::error!(method = "GET", url = %diagnostic_url, status, response_characters = body.chars().count(), "HTTP request failed");
            tracing::debug!(method = "GET", url = %diagnostic_url, status, response_body = %body, "HTTP error response body");
            return Err(GatewayError::Http {
                status,
                message: body,
            });
        }
        tracing::info!(method = "GET", url = %diagnostic_url, status, response_characters = body.chars().count(), "HTTP request completed");
        tracing::debug!(method = "GET", url = %diagnostic_url, status, response_body = %body, "HTTP response body");
        serde_json::from_str(&body)
            .map_err(|error| GatewayError::InvalidResponse(error.to_string()))
    }
}

fn redacted_response_headers(headers: &ureq::http::HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .filter(|(name, _)| ilium_logging::is_diagnostic_response_header(name.as_str()))
        .map(|(name, value)| {
            let value = value.to_str().unwrap_or("<non-text header>");
            (
                name.as_str().to_owned(),
                ilium_logging::redacted_header_value(name.as_str(), value),
            )
        })
        .collect()
}

#[derive(Serialize)]
struct ChatCompletionPayload<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    max_tokens: u32,
    temperature: f32,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptions>,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    #[serde(default)]
    model: Option<String>,
    choices: Vec<ChatChoice>,
}

impl ChatCompletionResponse {
    fn assistant_text(self) -> Result<String, GatewayError> {
        let model = self.model.unwrap_or_else(|| "<not reported>".to_string());
        let choice = self.choices.into_iter().next().ok_or_else(|| {
            GatewayError::InvalidResponse(format!("model {model} returned no choices"))
        })?;
        let finish_reason = choice
            .finish_reason
            .unwrap_or_else(|| "<not reported>".to_string());
        choice
            .message
            .content
            // Trim before the emptiness check, not after: callers receive
            // exactly the string that was validated as non-empty, so a
            // whitespace-only reply can't slip through as e.g. a single
            // trailing newline.
            .map(|content| content.trim().to_string())
            .filter(|content| !content.is_empty())
            .ok_or_else(|| {
                GatewayError::InvalidResponse(format!(
                    "model {model} returned no assistant content (finish_reason: {finish_reason})"
                ))
            })
    }
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatChoiceMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelCatalogResponse {
    data: Vec<KiloModel>,
}

#[derive(Debug, Deserialize)]
struct KiloModel {
    id: String,
    #[serde(rename = "isFree", default)]
    is_free: Option<bool>,
    #[serde(default)]
    architecture: ModelArchitecture,
    #[serde(default)]
    supported_parameters: Vec<String>,
    #[serde(default)]
    pricing: ModelPricing,
}

#[derive(Debug, Default, Deserialize)]
struct ModelArchitecture {
    #[serde(default)]
    output_modalities: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ModelPricing {
    #[serde(default)]
    prompt: String,
    #[serde(default)]
    completion: String,
}

/// Applies Kilo's authoritative free flag and excludes task-specific or
/// incompatible entries before model IDs reach the settings UI.
fn filter_free_text_models(models: Vec<KiloModel>) -> Vec<String> {
    let mut model_ids: Vec<String> = models
        .into_iter()
        .filter(|model| {
            let has_zero_pricing =
                parses_as_zero(&model.pricing.prompt) && parses_as_zero(&model.pricing.completion);
            let is_free = model.is_free.unwrap_or_else(|| {
                has_zero_pricing
                    || model.id.ends_with(":free")
                    || FALLBACK_FREE_MODELS.contains(&model.id.as_str())
            });
            let generates_text = model
                .architecture
                .output_modalities
                .iter()
                .any(|modality| modality == "text");
            let supports_budget = model
                .supported_parameters
                .iter()
                .any(|parameter| parameter == "max_tokens");
            is_free
                && generates_text
                && supports_budget
                && !model.id.to_ascii_lowercase().contains("content-safety")
        })
        .map(|model| model.id)
        .collect();
    model_ids.sort_by_key(|model_id| {
        (
            !FALLBACK_FREE_MODELS.contains(&model_id.as_str()),
            model_id.to_ascii_lowercase(),
        )
    });
    model_ids.dedup();
    model_ids
}

fn parses_as_zero(value: &str) -> bool {
    value.parse::<f64>().is_ok_and(|number| number == 0.0)
}

fn gateway_error_kind(error: &GatewayError) -> &'static str {
    match error {
        GatewayError::Http { .. } => "http",
        GatewayError::Transport(_) => "transport",
        GatewayError::InvalidResponse(_) => "invalid_response",
        GatewayError::InvalidProxy(_) => "invalid_proxy",
    }
}

fn gateway_http_status(error: &GatewayError) -> Option<u16> {
    match error {
        GatewayError::Http { status, .. } => Some(*status),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    fn spawn_http_response(status: &str, body: &str) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local HTTP fixture");
        let address = listener.local_addr().expect("fixture address");
        let status = status.to_owned();
        let body = body.to_owned();
        let (request_sender, request_receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept local HTTP request");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("request timeout");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4_096];
            loop {
                let read = stream.read(&mut buffer).expect("read HTTP request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n")
                else {
                    continue;
                };
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let content_length = headers.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                });
                if content_length
                    .is_some_and(|content_length| request.len() >= header_end + 4 + content_length)
                {
                    break;
                }
                let is_chunked = headers.lines().any(|line| {
                    line.split_once(':').is_some_and(|(name, value)| {
                        name.eq_ignore_ascii_case("transfer-encoding")
                            && value.trim().eq_ignore_ascii_case("chunked")
                    })
                });
                if is_chunked && request[header_end + 4..].ends_with(b"0\r\n\r\n") {
                    break;
                }
                if content_length.is_none() && !is_chunked {
                    break;
                }
            }
            request_sender
                .send(String::from_utf8_lossy(&request).into_owned())
                .expect("capture HTTP request");
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .expect("write HTTP response");
        });
        (format!("http://{address}"), request_receiver)
    }

    fn request() -> CompletionRequest {
        CompletionRequest::with_default_free_model(vec![ChatMessage::user("name this project")])
    }

    #[test]
    fn uses_kilo_auto_free_by_default() {
        assert_eq!(request().model, DEFAULT_FREE_MODEL);
        assert_eq!(request().max_tokens, UNKNOWN_MODEL_MAX_OUTPUT_TOKENS);
    }

    #[test]
    fn streaming_completion_emits_each_delta_and_usage() {
        let response_body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"one\\n\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"two\"}}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"completion_tokens\":11}}\n\n",
            "data: [DONE]\n\n"
        );
        let (base_url, request_receiver) = spawn_http_response("200 OK", response_body);
        let client = KiloGatewayClient::new(base_url, RetryPolicy::default());
        let mut events = Vec::new();

        client
            .stream_text(&request(), &mut |event| {
                events.push(event);
                true
            })
            .expect("stream fixture request");

        assert_eq!(
            events,
            vec![
                CompletionStreamEvent::TextDelta("one\n".to_string()),
                CompletionStreamEvent::TextDelta("two".to_string()),
                CompletionStreamEvent::OutputTokens(11),
            ]
        );
        let captured_request = request_receiver.recv().expect("captured HTTP request");
        let request_body = captured_request
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .expect("captured HTTP body");
        let request_json: serde_json::Value =
            serde_json::from_str(request_body).expect("valid request JSON");
        assert_eq!(request_json["stream"], true);
        assert_eq!(request_json["stream_options"]["include_usage"], true);
        assert_eq!(request_json["max_tokens"], UNKNOWN_MODEL_MAX_OUTPUT_TOKENS);
    }

    #[test]
    fn explicit_request_preserves_selected_model_and_token_budget() {
        let request = CompletionRequest::new(
            "stepfun/step-3.7-flash:free",
            vec![ChatMessage::user("restructure this project")],
            4096,
        );

        assert_eq!(request.model, "stepfun/step-3.7-flash:free");
        assert_eq!(request.max_tokens, 4096);
    }

    #[test]
    fn selected_model_and_restructure_budget_reach_the_http_payload() {
        let response_body = r#"{"model":"stepfun/step-3.7-flash:free","choices":[{"message":{"content":"{}"},"finish_reason":"stop"}]}"#;
        let (base_url, request_receiver) = spawn_http_response("200 OK", response_body);
        let client = KiloGatewayClient::new(
            base_url,
            RetryPolicy {
                max_attempts: 1,
                initial_delay: Duration::ZERO,
                max_delay: Duration::ZERO,
            },
        );
        let request = CompletionRequest::new(
            "stepfun/step-3.7-flash:free",
            vec![ChatMessage::user("restructure")],
            4096,
        );

        client
            .complete_text(&request)
            .expect("complete fixture request");

        let captured_request = request_receiver.recv().expect("captured HTTP request");
        let request_body = captured_request
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .expect("captured HTTP body");
        let request_json: serde_json::Value =
            serde_json::from_str(request_body).expect("valid request JSON");
        assert_eq!(request_json["model"], "stepfun/step-3.7-flash:free");
        assert_eq!(request_json["max_tokens"], 4096);
    }

    #[test]
    fn retries_temporary_failures_then_returns_assistant_text() {
        let client = KiloGatewayClient::new(
            "https://example.invalid",
            RetryPolicy {
                max_attempts: 3,
                initial_delay: Duration::ZERO,
                max_delay: Duration::ZERO,
            },
        );
        let calls = Cell::new(0);

        let result = client.complete_text_with_sender(&request(), |_| {
            calls.set(calls.get() + 1);
            if calls.get() < 3 {
                return Err(GatewayError::Http {
                    status: 429,
                    message: "slow down".to_string(),
                });
            }
            Ok(ChatCompletionResponse {
                model: Some("fixture/routed-model".to_string()),
                choices: vec![ChatChoice {
                    message: ChatChoiceMessage {
                        content: Some("Ilium".to_string()),
                    },
                    finish_reason: Some("stop".to_string()),
                }],
            })
        });

        assert_eq!(result.unwrap(), "Ilium");
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn retries_invalid_responses() {
        let client = KiloGatewayClient::new(
            "https://example.invalid",
            RetryPolicy {
                max_attempts: 3,
                initial_delay: Duration::ZERO,
                max_delay: Duration::ZERO,
            },
        );
        let calls = Cell::new(0);
        let result = client.complete_text_with_sender(&request(), |_| {
            calls.set(calls.get() + 1);
            Err(GatewayError::InvalidResponse("not JSON".to_string()))
        });

        assert!(matches!(result, Err(GatewayError::InvalidResponse(_))));
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn retries_content_null_length_response_then_returns_visible_text() {
        let client = KiloGatewayClient::new(
            "https://example.invalid",
            RetryPolicy {
                max_attempts: 2,
                initial_delay: Duration::ZERO,
                max_delay: Duration::ZERO,
            },
        );
        let calls = Cell::new(0);

        let result = client.complete_text_with_sender(&request(), |_| {
            calls.set(calls.get() + 1);
            Ok(ChatCompletionResponse {
                model: Some("provider/routed:free".to_string()),
                choices: vec![ChatChoice {
                    message: ChatChoiceMessage {
                        content: (calls.get() == 2).then(|| "Ilium".to_string()),
                    },
                    finish_reason: Some(if calls.get() == 1 {
                        "length".to_string()
                    } else {
                        "stop".to_string()
                    }),
                }],
            })
        });

        assert_eq!(result.unwrap(), "Ilium");
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn http_errors_preserve_the_complete_gateway_body_and_request_prompt() {
        let response_body =
            r#"{"error":{"message":"upstream rejected request","request_id":"req-42"}}"#;
        let (base_url, request_receiver) = spawn_http_response("502 Bad Gateway", response_body);
        let client = KiloGatewayClient::new(
            base_url,
            RetryPolicy {
                max_attempts: 1,
                initial_delay: Duration::ZERO,
                max_delay: Duration::ZERO,
            },
        );

        let result = client.complete_text(&request());

        assert!(matches!(
            result,
            Err(GatewayError::Http { status: 502, message }) if message == response_body
        ));
        let captured_request = request_receiver.recv().expect("captured HTTP request");
        assert!(captured_request.contains("name this project"));
        assert!(captured_request.contains("kilo-auto/free"));
    }

    #[test]
    fn discovers_only_compatible_free_generation_models() {
        let response_body = r#"{"data":[
            {"id":"kilo-auto/free","isFree":true,"architecture":{"output_modalities":["text"]},"supported_parameters":["max_tokens"],"pricing":{"prompt":"0","completion":"0"}},
            {"id":"provider/concrete:free","isFree":true,"architecture":{"output_modalities":["text"]},"supported_parameters":["max_tokens"],"pricing":{"prompt":"0","completion":"0"}},
            {"id":"nvidia/content-safety:free","isFree":true,"architecture":{"output_modalities":["text"]},"supported_parameters":["max_tokens"],"pricing":{"prompt":"0","completion":"0"}},
            {"id":"provider/image:free","isFree":true,"architecture":{"output_modalities":["image"]},"supported_parameters":["max_tokens"],"pricing":{"prompt":"0","completion":"0"}},
            {"id":"provider/paid","isFree":false,"architecture":{"output_modalities":["text"]},"supported_parameters":["max_tokens"],"pricing":{"prompt":"1","completion":"1"}}
        ]}"#;
        let (base_url, request_receiver) = spawn_http_response("200 OK", response_body);
        let client = KiloGatewayClient::new(base_url, RetryPolicy::default());

        let models = client.list_free_models().expect("discover free models");

        assert_eq!(
            models,
            vec![
                "kilo-auto/free".to_string(),
                "provider/concrete:free".to_string()
            ]
        );
        assert!(request_receiver
            .recv()
            .expect("captured model request")
            .starts_with("GET /models "));
    }

    #[test]
    fn paid_proxy_connect_url_includes_credentials_only_when_both_present() {
        let proxy = PaidProxy {
            ip: "198.51.100.7".to_string(),
            port: 8080,
            protocol: "http".to_string(),
            username: "user".to_string(),
            password: "pass".to_string(),
        };
        assert_eq!(proxy.connect_url(), "http://user:pass@198.51.100.7:8080");

        let encoded = PaidProxy {
            username: "user@example".to_string(),
            password: "p@ss:word".to_string(),
            ..proxy.clone()
        };
        assert_eq!(
            encoded.connect_url(),
            "http://user%40example:p%40ss%3Aword@198.51.100.7:8080"
        );

        let ip_authorized = PaidProxy {
            username: String::new(),
            password: String::new(),
            ..proxy.clone()
        };
        assert_eq!(ip_authorized.connect_url(), "http://198.51.100.7:8080");

        let one_sided = PaidProxy {
            username: "user".to_string(),
            password: String::new(),
            ..proxy
        };
        assert_eq!(one_sided.connect_url(), "http://198.51.100.7:8080");
    }

    #[test]
    fn choose_random_paid_proxy_returns_none_for_an_empty_list() {
        assert!(choose_random_paid_proxy(&[]).is_none());
    }

    #[test]
    fn choose_random_paid_proxy_always_picks_from_the_list() {
        let proxies = vec![
            PaidProxy {
                ip: "198.51.100.1".to_string(),
                port: 8080,
                ..Default::default()
            },
            PaidProxy {
                ip: "198.51.100.2".to_string(),
                port: 8081,
                ..Default::default()
            },
        ];
        for _ in 0..20 {
            let chosen = choose_random_paid_proxy(&proxies).expect("non-empty list yields Some");
            assert!(proxies.contains(chosen));
        }
    }

    #[test]
    fn client_with_proxy_url_routes_the_request_through_it() {
        let response_body = r#"{"choices":[{"message":{"content":"ok"},"finish_reason":"stop"}]}"#;
        let (target_url, target_receiver) = spawn_http_response("200 OK", response_body);
        let base_url = target_url.trim_end_matches("/chat/completions").to_string();
        // No proxy actually listens here; the point of this test is that the
        // client fails trying to reach a proxy instead of silently calling
        // the base URL directly.
        let client = KiloGatewayClient::new(
            base_url,
            RetryPolicy {
                max_attempts: 1,
                ..RetryPolicy::default()
            },
        )
        .with_proxy_url("http://127.0.0.1:1/");
        let request = CompletionRequest::new("kilo-auto/free", vec![ChatMessage::user("hi")], 16);

        let result = client.complete_text(&request);

        assert!(result.is_err());
        assert!(target_receiver.try_recv().is_err());
    }

    #[test]
    fn client_with_an_invalid_proxy_url_fails_with_invalid_proxy() {
        let client = KiloGatewayClient::new(
            DEFAULT_BASE_URL,
            RetryPolicy {
                max_attempts: 1,
                ..RetryPolicy::default()
            },
        )
        .with_proxy_url("not a url");
        let request = CompletionRequest::new("kilo-auto/free", vec![ChatMessage::user("hi")], 16);

        let error = client
            .complete_text(&request)
            .expect_err("malformed proxy URL must fail");

        assert!(matches!(error, GatewayError::InvalidProxy(_)));
    }
}
