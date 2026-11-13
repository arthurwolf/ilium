//! Kilo Gateway (OpenAI-compatible) request and response shapes. The HTTP exchange itself (retry,
//! backoff, proxy tunnelling, streaming) is host work behind the `http` import; this module holds the pure
//! parts: paid-proxy URLs, request payloads, response and catalogue parsing and error classification.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DEFAULT_BASE_URL: &str = "https://api.kilo.ai/api/gateway";
/// Kilo's stable native free router, selected on a clean installation.
pub const DEFAULT_FREE_MODEL: &str = "kilo-auto/free";
/// Used when the selected provider/model does not publish a reliable output
/// limit. Callers control response length through the prompt.
pub const UNKNOWN_MODEL_MAX_OUTPUT_TOKENS: u32 = 1_000_000;
/// Stable virtual routes retained when live model discovery is unavailable.
pub const FALLBACK_FREE_MODELS: [&str; 2] = ["kilo-auto/free", "openrouter/free"];
/// Per-request timeout of the gateway, in seconds.
pub const REQUEST_TIMEOUT_SECONDS: u64 = 30;
pub const MAXIMUM_COMPLETION_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;
pub const MAXIMUM_STREAM_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
pub const MAXIMUM_MODEL_CATALOG_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;

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

/// Picks one proxy from `proxies` by `seed` (the host supplies fresh randomness, because the plugin has
/// no entropy source), or `None` when the list is empty.
pub fn choose_paid_proxy(proxies: &[PaidProxy], seed: u64) -> Option<&PaidProxy> {
    if proxies.is_empty() {
        return None;
    }
    let length = proxies.len() as u64;
    proxies.get((seed % length) as usize)
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

/// Retry policy for temporary gateway or upstream failures, in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u8,
    pub initial_delay_millis: u64,
    pub max_delay_millis: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 4,
            initial_delay_millis: 500,
            max_delay_millis: 8_000,
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
    pub fn is_retryable(&self) -> bool {
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

#[derive(Serialize)]
struct ChatCompletionPayload<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
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
pub struct KiloModel {
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
pub fn filter_free_text_models(models: Vec<KiloModel>) -> Vec<String> {
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

pub fn gateway_error_kind(error: &GatewayError) -> &'static str {
    match error {
        GatewayError::Http { .. } => "http",
        GatewayError::Transport(_) => "transport",
        GatewayError::InvalidResponse(_) => "invalid_response",
        GatewayError::InvalidProxy(_) => "invalid_proxy",
    }
}

pub fn gateway_http_status(error: &GatewayError) -> Option<u16> {
    match error {
        GatewayError::Http { status, .. } => Some(*status),
        _ => None,
    }
}

/// The JSON body of one chat completion, optionally streaming with usage reporting.
pub fn completion_payload(request: &CompletionRequest, stream: bool) -> serde_json::Value {
    let payload = ChatCompletionPayload {
        model: &request.model,
        messages: &request.messages,
        max_tokens: Some(request.max_tokens),
        temperature: request.temperature,
        stream,
        stream_options: stream.then_some(StreamOptions {
            include_usage: true,
        }),
    };
    serde_json::to_value(payload).unwrap_or(serde_json::Value::Null)
}

/// Extracts the trimmed assistant text of a non-streaming completion body.
pub fn parse_completion_text(body: &str) -> Result<String, GatewayError> {
    let response: ChatCompletionResponse = serde_json::from_str(body)
        .map_err(|error| GatewayError::InvalidResponse(error.to_string()))?;
    response.assistant_text()
}

/// Extracts the free, text-generating, budget-capable model ids of the unauthenticated catalogue body.
pub fn parse_free_models(body: &str) -> Result<Vec<String>, GatewayError> {
    let catalog: ModelCatalogResponse = serde_json::from_str(body)
        .map_err(|error| GatewayError::InvalidResponse(error.to_string()))?;
    Ok(filter_free_text_models(catalog.data))
}

/// One decoded stream line: the text delta and the exact output-token usage, when present.
pub fn parse_stream_data(data: &str) -> Result<Vec<CompletionStreamEvent>, GatewayError> {
    let value: serde_json::Value = serde_json::from_str(data)
        .map_err(|error| GatewayError::InvalidResponse(error.to_string()))?;
    let mut events = Vec::new();
    if let Some(tokens) = value
        .get("usage")
        .and_then(|usage| usage.get("completion_tokens"))
        .and_then(serde_json::Value::as_u64)
    {
        events.push(CompletionStreamEvent::OutputTokens(tokens));
    }
    if let Some(text) = value
        .pointer("/choices/0/delta/content")
        .and_then(serde_json::Value::as_str)
    {
        if !text.is_empty() {
            events.push(CompletionStreamEvent::TextDelta(text.to_string()));
        }
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> CompletionRequest {
        CompletionRequest::with_default_free_model(vec![ChatMessage::user("name this project")])
    }

    /// REQ: PORT:ilium-kilo-gateway/src/lib.rs::uses_kilo_auto_free_by_default
    #[test]
    fn uses_kilo_auto_free_by_default() {
        assert_eq!(request().model, DEFAULT_FREE_MODEL);
        assert_eq!(request().max_tokens, UNKNOWN_MODEL_MAX_OUTPUT_TOKENS);
    }

    /// REQ: PORT:ilium-kilo-gateway/src/lib.rs::explicit_request_preserves_selected_model_and_token_budget
    #[test]
    fn explicit_request_preserves_selected_model_and_token_budget() {
        let request = CompletionRequest::new(
            "dots-studio/dots-3-note-preview:free",
            vec![ChatMessage::user("restructure this project")],
            4096,
        );

        assert_eq!(request.model, "dots-studio/dots-3-note-preview:free");
        assert_eq!(request.max_tokens, 4096);
        let payload = completion_payload(&request, false);
        assert_eq!(payload["model"], "dots-studio/dots-3-note-preview:free");
        assert_eq!(payload["max_tokens"], 4096);
        assert!(payload.get("stream_options").is_none());
    }

    /// REQ: PORT:ilium-kilo-gateway/src/lib.rs::discovers_only_compatible_free_generation_models
    #[test]
    fn selected_model_and_restructure_budget_reach_the_http_payload() {
        let request = CompletionRequest::new(
            "dots-studio/dots-3-note-preview:free",
            vec![
                ChatMessage::system("rules"),
                ChatMessage::user("restructure"),
            ],
            65_536,
        );
        let payload = completion_payload(&request, true);
        assert_eq!(payload["model"], "dots-studio/dots-3-note-preview:free");
        assert_eq!(payload["max_tokens"], 65_536);
        assert_eq!(payload["stream"], true);
        assert_eq!(payload["stream_options"]["include_usage"], true);
        assert_eq!(payload["messages"][0]["role"], "system");
        assert_eq!(payload["messages"][1]["content"], "restructure");
    }

    /// REQ: PORT:ilium-kilo-gateway/src/lib.rs::discovers_only_compatible_free_generation_models
    #[test]
    fn discovers_only_compatible_free_generation_models() {
        let response_body = r#"{"data":[
            {"id":"kilo-auto/free","isFree":true,"architecture":{"output_modalities":["text"]},"supported_parameters":["max_tokens"],"pricing":{"prompt":"0","completion":"0"}},
            {"id":"provider/concrete:free","isFree":true,"architecture":{"output_modalities":["text"]},"supported_parameters":["max_tokens"],"pricing":{"prompt":"0","completion":"0"}},
            {"id":"nvidia/content-safety:free","isFree":true,"architecture":{"output_modalities":["text"]},"supported_parameters":["max_tokens"],"pricing":{"prompt":"0","completion":"0"}},
            {"id":"provider/image:free","isFree":true,"architecture":{"output_modalities":["image"]},"supported_parameters":["max_tokens"],"pricing":{"prompt":"0","completion":"0"}},
            {"id":"provider/paid","isFree":false,"architecture":{"output_modalities":["text"]},"supported_parameters":["max_tokens"],"pricing":{"prompt":"1","completion":"1"}}
        ]}"#;
        assert_eq!(
            parse_free_models(response_body).expect("discover free models"),
            vec![
                "kilo-auto/free".to_string(),
                "provider/concrete:free".to_string()
            ]
        );
    }

    /// REQ: PORT:ilium-kilo-gateway/src/lib.rs::paid_proxy_connect_url_includes_credentials_only_when_both_present
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

    /// REQ: PORT:ilium-kilo-gateway/src/lib.rs::choose_random_paid_proxy_returns_none_for_an_empty_list
    #[test]
    fn choose_random_paid_proxy_returns_none_for_an_empty_list() {
        assert!(choose_paid_proxy(&[], 7).is_none());
    }

    /// REQ: PORT:ilium-kilo-gateway/src/lib.rs::choose_random_paid_proxy_always_picks_from_the_list
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
        for seed in 0..20 {
            let chosen = choose_paid_proxy(&proxies, seed).expect("non-empty list yields Some");
            assert!(proxies.contains(chosen));
        }
    }

    /// REQ: PORT:ilium-kilo-gateway/src/lib.rs::streaming_completion_emits_each_delta_and_usage
    #[test]
    fn streaming_completion_emits_each_delta_and_usage() {
        let events =
            parse_stream_data(r#"{"choices":[{"delta":{"content":"one"}}]}"#).expect("delta line");
        assert_eq!(
            events,
            vec![CompletionStreamEvent::TextDelta("one".to_string())]
        );
        let usage = parse_stream_data(r#"{"choices":[],"usage":{"completion_tokens":42}}"#)
            .expect("usage line");
        assert_eq!(usage, vec![CompletionStreamEvent::OutputTokens(42)]);
    }

    #[test]
    fn completion_text_is_trimmed_and_empty_content_is_invalid() {
        let ok = parse_completion_text(
            r#"{"model":"m","choices":[{"message":{"content":"  hi  "},"finish_reason":"stop"}]}"#,
        )
        .expect("text");
        assert_eq!(ok, "hi");
        let empty = parse_completion_text(
            r#"{"model":"m","choices":[{"message":{"content":null},"finish_reason":"length"}]}"#,
        );
        assert!(matches!(empty, Err(GatewayError::InvalidResponse(_))));
        assert!(GatewayError::Transport("x".into()).is_retryable());
        assert!(!GatewayError::Http {
            status: 401,
            message: String::new()
        }
        .is_retryable());
    }
}
