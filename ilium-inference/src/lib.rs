//! Provider-neutral inference boundary for Ilium's title and organization
//! features. Every backend implements [`InferenceProvider`], insulating the
//! client from individual HTTP envelopes and authentication details.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use ilium_kilo_gateway::{ChatMessage, CompletionRequest, KiloGatewayClient};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DEFAULT_OLLAMA_URL: &str = "http://127.0.0.1:11434";
pub const DEFAULT_OPENAI_URL: &str = "https://api.openai.com/v1";
pub const DEFAULT_ANTHROPIC_URL: &str = "https://api.anthropic.com";
pub const DEFAULT_OPENROUTER_URL: &str = "https://openrouter.ai/api/v1";
pub const DEFAULT_OPENROUTER_MODEL: &str = "openrouter/free";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum InferenceProviderKind {
    #[default]
    KiloGateway,
    Ollama,
    OpenAi,
    Anthropic,
    OpenRouter,
}

impl InferenceProviderKind {
    pub const ALL: [Self; 5] = [
        Self::KiloGateway,
        Self::Ollama,
        Self::OpenAi,
        Self::Anthropic,
        Self::OpenRouter,
    ];
    pub const fn label(self) -> &'static str {
        match self {
            Self::KiloGateway => "Kilo Gateway",
            Self::Ollama => "Ollama (local)",
            Self::OpenAi => "OpenAI-compatible",
            Self::Anthropic => "Anthropic",
            Self::OpenRouter => "OpenRouter",
        }
    }
}

/// Complete durable settings. Switching providers preserves every other
/// provider's endpoint, model, and credentials for a later switch back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InferenceSettings {
    pub selected_provider: InferenceProviderKind,
    pub ollama: OllamaSettings,
    pub openai: ApiKeyProviderSettings,
    pub anthropic: ApiKeyProviderSettings,
    pub openrouter: OpenRouterSettings,
}

impl Default for InferenceSettings {
    fn default() -> Self {
        Self {
            selected_provider: InferenceProviderKind::KiloGateway,
            ollama: OllamaSettings::default(),
            openai: ApiKeyProviderSettings::new(DEFAULT_OPENAI_URL),
            anthropic: ApiKeyProviderSettings::new(DEFAULT_ANTHROPIC_URL),
            openrouter: OpenRouterSettings::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OllamaSettings {
    pub base_url: String,
    pub model: String,
}
impl Default for OllamaSettings {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_OLLAMA_URL.to_string(),
            model: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ApiKeyProviderSettings {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}
impl ApiKeyProviderSettings {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            api_key: String::new(),
            model: String::new(),
        }
    }
}
impl Default for ApiKeyProviderSettings {
    fn default() -> Self {
        Self::new(DEFAULT_OPENAI_URL)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OpenRouterSettings {
    pub api_key: String,
    pub model: String,
}
impl Default for OpenRouterSettings {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            model: DEFAULT_OPENROUTER_MODEL.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct InferenceRequest {
    pub system_prompt: String,
    pub user_prompt: String,
    pub max_tokens: u32,
}
impl InferenceRequest {
    pub fn json_only(user_prompt: impl Into<String>) -> Self {
        Self {
            system_prompt: "Return concise, valid JSON only.".to_string(),
            user_prompt: user_prompt.into(),
            max_tokens: 1536,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceResponse {
    pub text: String,
}

#[derive(Debug, Error)]
pub enum InferenceError {
    #[error("{0}")]
    Configuration(String),
    #[error("HTTP {status}: {message}")]
    Http { status: u16, message: String },
    #[error("transport error: {0}")]
    Transport(String),
    #[error("invalid provider response: {0}")]
    InvalidResponse(String),
}

/// Base polymorphic contract for all inference backends. Model discovery is
/// optional because it is a specific capability of the locally running Ollama API.
pub trait InferenceProvider: Send + Sync {
    fn kind(&self) -> InferenceProviderKind;
    fn complete(&self, request: &InferenceRequest) -> Result<InferenceResponse, InferenceError>;
    fn list_models(&self) -> Result<Vec<String>, InferenceError> {
        Ok(Vec::new())
    }
}

pub fn provider_from_settings(settings: &InferenceSettings) -> Box<dyn InferenceProvider> {
    let inner: Box<dyn InferenceProvider> = match settings.selected_provider {
        InferenceProviderKind::KiloGateway => Box::new(KiloGatewayProvider),
        InferenceProviderKind::Ollama => {
            Box::new(OllamaProvider(Arc::new(settings.ollama.clone())))
        }
        InferenceProviderKind::OpenAi => {
            Box::new(OpenAiProvider(Arc::new(settings.openai.clone())))
        }
        InferenceProviderKind::Anthropic => {
            Box::new(AnthropicProvider(Arc::new(settings.anthropic.clone())))
        }
        InferenceProviderKind::OpenRouter => {
            Box::new(OpenRouterProvider(Arc::new(settings.openrouter.clone())))
        }
    };
    Box::new(DiagnosticProvider { inner })
}

/// Logs the provider-neutral exchange once around every concrete adapter,
/// including configuration and response-validation failures that occur before
/// or after the HTTP transport seam.
struct DiagnosticProvider {
    inner: Box<dyn InferenceProvider>,
}

static NEXT_OPERATION_ID: AtomicU64 = AtomicU64::new(1);

fn next_operation_id() -> u64 {
    NEXT_OPERATION_ID.fetch_add(1, Ordering::Relaxed)
}

impl InferenceProvider for DiagnosticProvider {
    fn kind(&self) -> InferenceProviderKind {
        self.inner.kind()
    }

    fn complete(&self, request: &InferenceRequest) -> Result<InferenceResponse, InferenceError> {
        let operation_id = next_operation_id();
        let provider = self.inner.kind();
        let started_at = Instant::now();
        let operation_span = tracing::info_span!("llm_inference", operation_id, ?provider);
        let _operation_guard = operation_span.enter();
        tracing::info!(
            operation_id,
            ?provider,
            max_tokens = request.max_tokens,
            system_prompt = %request.system_prompt,
            user_prompt = %request.user_prompt,
            "LLM inference started"
        );
        let result = self.inner.complete(request);
        match &result {
            Ok(response) => tracing::info!(
                operation_id,
                ?provider,
                elapsed_milliseconds = started_at.elapsed().as_millis(),
                response_text = %response.text,
                "LLM inference completed"
            ),
            Err(error) => tracing::error!(
                operation_id,
                ?provider,
                elapsed_milliseconds = started_at.elapsed().as_millis(),
                error = %error,
                error_debug = ?error,
                "LLM inference failed"
            ),
        }
        result
    }

    fn list_models(&self) -> Result<Vec<String>, InferenceError> {
        let operation_id = next_operation_id();
        let provider = self.inner.kind();
        let started_at = Instant::now();
        let operation_span = tracing::info_span!("llm_model_discovery", operation_id, ?provider);
        let _operation_guard = operation_span.enter();
        tracing::info!(operation_id, ?provider, "LLM model discovery started");
        let result = self.inner.list_models();
        match &result {
            Ok(models) => tracing::info!(
                operation_id,
                ?provider,
                elapsed_milliseconds = started_at.elapsed().as_millis(),
                ?models,
                "LLM model discovery completed"
            ),
            Err(error) => tracing::error!(
                operation_id,
                ?provider,
                elapsed_milliseconds = started_at.elapsed().as_millis(),
                error = %error,
                error_debug = ?error,
                "LLM model discovery failed"
            ),
        }
        result
    }
}

pub struct KiloGatewayProvider;
impl InferenceProvider for KiloGatewayProvider {
    fn kind(&self) -> InferenceProviderKind {
        InferenceProviderKind::KiloGateway
    }
    fn complete(&self, request: &InferenceRequest) -> Result<InferenceResponse, InferenceError> {
        let request = CompletionRequest::with_default_free_model(vec![
            ChatMessage::system(request.system_prompt.as_str()),
            ChatMessage::user(request.user_prompt.as_str()),
        ]);
        KiloGatewayClient::default()
            .complete_text(&request)
            .map(|text| InferenceResponse { text })
            .map_err(|error| InferenceError::Transport(error.to_string()))
    }
}

struct OllamaProvider(Arc<OllamaSettings>);
impl InferenceProvider for OllamaProvider {
    fn kind(&self) -> InferenceProviderKind {
        InferenceProviderKind::Ollama
    }
    fn complete(&self, request: &InferenceRequest) -> Result<InferenceResponse, InferenceError> {
        require(
            &self.0.model,
            "Select an Ollama model before testing or using inference",
        )?;
        let response = post_json(
            &format_url(&self.0.base_url, "api/chat"),
            &[],
            serde_json::json!({"model":self.0.model,"stream":false,"messages":[{"role":"system","content":request.system_prompt},{"role":"user","content":request.user_prompt}],"options":{"temperature":0.0,"num_predict":request.max_tokens}}),
        )?;
        response_text(&response, &["message", "content"])
    }
    fn list_models(&self) -> Result<Vec<String>, InferenceError> {
        let response = get_json(&format_url(&self.0.base_url, "api/tags"), &[])?;
        Ok(response
            .get("models")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|model| {
                model
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            })
            .collect())
    }
}

/// Shared OpenAI-chat adapter logic working with borrowed settings.
/// Used by OpenAI and OpenRouter to avoid cloning settings.
fn complete_openai_compatible(
    base_url: &str,
    api_key: &str,
    model: &str,
    request: &InferenceRequest,
) -> Result<InferenceResponse, InferenceError> {
    require(
        api_key,
        "Enter an API key before testing or using inference",
    )?;
    require(model, "Enter a model before testing or using inference")?;
    let response = post_json(
        &format_url(base_url, "chat/completions"),
        &[("Authorization", format!("Bearer {}", api_key))],
        serde_json::json!({"model":model,"messages":[{"role":"system","content":request.system_prompt},{"role":"user","content":request.user_prompt}],"temperature":0.0,"max_tokens":request.max_tokens,"stream":false}),
    )?;
    response_text(&response, &["choices", "0", "message", "content"])
}

struct OpenAiProvider(Arc<ApiKeyProviderSettings>);
impl InferenceProvider for OpenAiProvider {
    fn kind(&self) -> InferenceProviderKind {
        InferenceProviderKind::OpenAi
    }
    fn complete(&self, request: &InferenceRequest) -> Result<InferenceResponse, InferenceError> {
        complete_openai_compatible(&self.0.base_url, &self.0.api_key, &self.0.model, request)
    }
}
struct OpenRouterProvider(Arc<OpenRouterSettings>);
impl InferenceProvider for OpenRouterProvider {
    fn kind(&self) -> InferenceProviderKind {
        InferenceProviderKind::OpenRouter
    }
    fn complete(&self, request: &InferenceRequest) -> Result<InferenceResponse, InferenceError> {
        complete_openai_compatible(
            DEFAULT_OPENROUTER_URL,
            &self.0.api_key,
            &self.0.model,
            request,
        )
    }
}
struct AnthropicProvider(Arc<ApiKeyProviderSettings>);
impl InferenceProvider for AnthropicProvider {
    fn kind(&self) -> InferenceProviderKind {
        InferenceProviderKind::Anthropic
    }
    fn complete(&self, request: &InferenceRequest) -> Result<InferenceResponse, InferenceError> {
        require(
            &self.0.api_key,
            "Enter an API key before testing or using inference",
        )?;
        require(
            &self.0.model,
            "Enter a model before testing or using inference",
        )?;
        let response = post_json(
            &format_url(&self.0.base_url, "v1/messages"),
            &[
                ("x-api-key", self.0.api_key.as_str().to_string()),
                ("anthropic-version", "2023-06-01".to_string()),
            ],
            serde_json::json!({"model":self.0.model,"system":request.system_prompt,"max_tokens":request.max_tokens,"messages":[{"role":"user","content":request.user_prompt}],"temperature":0.0}),
        )?;
        response_text(&response, &["content", "0", "text"])
    }
}

fn require(value: &str, message: &str) -> Result<(), InferenceError> {
    if value.trim().is_empty() {
        Err(InferenceError::Configuration(message.to_string()))
    } else {
        Ok(())
    }
}
fn format_url(base_url: &str, path: &str) -> String {
    format!("{}/{}", base_url.trim_end_matches('/'), path)
}
fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_global(Some(REQUEST_TIMEOUT))
                .http_status_as_error(false)
                .build(),
        )
    })
}
fn get_json(url: &str, headers: &[(&str, String)]) -> Result<serde_json::Value, InferenceError> {
    let diagnostic_url = ilium_logging::redacted_url(url);
    tracing::info!(
        method = "GET",
        url = %diagnostic_url,
        headers = ?redacted_headers(headers),
        "HTTP request started"
    );
    let mut request = agent().get(url);
    for (name, value) in headers {
        request = request.header(*name, value);
    }
    send("GET", url, None, request.call())
}
fn post_json(
    url: &str,
    headers: &[(&str, String)],
    body: serde_json::Value,
) -> Result<serde_json::Value, InferenceError> {
    let diagnostic_url = ilium_logging::redacted_url(url);
    tracing::info!(
        method = "POST",
        url = %diagnostic_url,
        headers = ?redacted_headers(headers),
        request_body = %body,
        "HTTP request started"
    );
    let mut request = agent().post(url).header("Content-Type", "application/json");
    for (name, value) in headers {
        request = request.header(*name, value);
    }
    send("POST", url, Some(&body), request.send_json(&body))
}
fn send(
    method: &'static str,
    url: &str,
    request_body: Option<&serde_json::Value>,
    result: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<serde_json::Value, InferenceError> {
    let diagnostic_url = ilium_logging::redacted_url(url);
    let mut response = match result {
        Ok(response) => response,
        Err(error) => {
            let error_message = error.to_string().replace(url, &diagnostic_url);
            tracing::error!(
                method,
                url = %diagnostic_url,
                request_body = ?request_body,
                error = %error_message,
                "HTTP transport failed"
            );
            return Err(InferenceError::Transport(error_message));
        }
    };
    let status = response.status().as_u16();
    let response_headers = redacted_response_headers(response.headers());
    let body = response.body_mut().read_to_string().map_err(|error| {
            tracing::error!(method, url = %diagnostic_url, status, ?response_headers, error = %error, "failed to read HTTP response body");
        InferenceError::Transport(error.to_string())
    })?;
    if !(200..300).contains(&status) {
        tracing::error!(method, url = %diagnostic_url, status, ?response_headers, response_body = %body, "HTTP request failed");
        return Err(InferenceError::Http {
            status,
            message: body,
        });
    }
    tracing::info!(method, url = %diagnostic_url, status, ?response_headers, response_body = %body, "HTTP request completed");
    serde_json::from_str(&body).map_err(|error| {
        tracing::error!(
            method,
            url = %diagnostic_url,
            status,
            ?response_headers,
            response_body = %body,
            error = %error,
            "HTTP response was not valid JSON"
        );
        InferenceError::InvalidResponse(error.to_string())
    })
}

/// Retains useful non-secret header values while making credential leakage
/// impossible even when complete request bodies are enabled.
fn redacted_headers(headers: &[(&str, String)]) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|(name, value)| {
            (
                (*name).to_owned(),
                ilium_logging::redacted_header_value(name, value),
            )
        })
        .collect()
}

fn redacted_response_headers(headers: &ureq::http::HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|(name, value)| {
            let value = value.to_str().unwrap_or("<non-text header>");
            (
                name.as_str().to_owned(),
                ilium_logging::redacted_header_value(name.as_str(), value),
            )
        })
        .collect()
}
fn response_text(
    value: &serde_json::Value,
    path: &[&str],
) -> Result<InferenceResponse, InferenceError> {
    let mut current = value;
    for segment in path {
        current = if let Ok(index) = segment.parse::<usize>() {
            current.get(index)
        } else {
            current.get(*segment)
        }
        .ok_or_else(|| {
            InferenceError::InvalidResponse(format!("missing response field {}", path.join(".")))
        })?;
    }
    let text = current
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| {
            InferenceError::InvalidResponse("missing non-empty assistant text".to_string())
        })?;
    Ok(InferenceResponse {
        text: text.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                if request.len() >= header_end + 4 + content_length {
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
        (
            format!("http://{address}/chat/completions"),
            request_receiver,
        )
    }

    #[test]
    fn defaults_are_safe() {
        let settings = InferenceSettings::default();
        assert_eq!(
            settings.selected_provider,
            InferenceProviderKind::KiloGateway
        );
        assert_eq!(settings.openrouter.model, DEFAULT_OPENROUTER_MODEL);
    }
    #[test]
    fn factory_selects_provider() {
        let settings = InferenceSettings {
            selected_provider: InferenceProviderKind::Anthropic,
            ..InferenceSettings::default()
        };
        assert_eq!(
            provider_from_settings(&settings).kind(),
            InferenceProviderKind::Anthropic
        );
    }

    #[test]
    fn http_errors_preserve_the_complete_response_and_requests_send_the_complete_body() {
        let response_body = r#"{"error":{"message":"model unavailable","code":"overloaded"}}"#;
        let (url, request_receiver) = spawn_http_response("503 Service Unavailable", response_body);
        let request_body = serde_json::json!({
            "model": "fixture-model",
            "messages": [{"role": "user", "content": "complete sensitive prompt"}],
        });

        let result = post_json(
            &url,
            &[("Authorization", "Bearer test-secret".to_owned())],
            request_body,
        );

        assert!(matches!(
            result,
            Err(InferenceError::Http { status: 503, message }) if message == response_body
        ));
        let captured_request = request_receiver.recv().expect("captured HTTP request");
        assert!(captured_request.contains("complete sensitive prompt"));
        assert!(captured_request
            .to_ascii_lowercase()
            .contains("authorization: bearer test-secret"));
    }

    #[test]
    fn diagnostic_headers_redact_every_supported_api_key_header() {
        let headers = redacted_headers(&[
            ("Authorization", "Bearer secret-one".to_owned()),
            ("x-api-key", "secret-two".to_owned()),
            ("anthropic-version", "2023-06-01".to_owned()),
        ]);

        assert_eq!(headers[0].1, "<redacted>");
        assert_eq!(headers[1].1, "<redacted>");
        assert_eq!(headers[2].1, "2023-06-01");
        assert!(!format!("{headers:?}").contains("secret"));
    }
}
