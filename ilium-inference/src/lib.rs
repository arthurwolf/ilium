//! Provider-neutral inference boundary for Ilium's title and organization
//! features. Every backend implements [`InferenceProvider`], insulating the
//! client from individual HTTP envelopes and authentication details.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

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
    match settings.selected_provider {
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
                .build(),
        )
    })
}
fn get_json(url: &str, headers: &[(&str, String)]) -> Result<serde_json::Value, InferenceError> {
    let mut request = agent().get(url);
    for (name, value) in headers {
        request = request.header(*name, value);
    }
    send(request.call())
}
fn post_json(
    url: &str,
    headers: &[(&str, String)],
    body: serde_json::Value,
) -> Result<serde_json::Value, InferenceError> {
    let mut request = agent().post(url).header("Content-Type", "application/json");
    for (name, value) in headers {
        request = request.header(*name, value);
    }
    send(request.send_json(body))
}
fn send(
    result: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<serde_json::Value, InferenceError> {
    let mut response = match result {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(status)) => {
            return Err(InferenceError::Http {
                status,
                message: "provider rejected the request".to_string(),
            })
        }
        Err(error) => return Err(InferenceError::Transport(error.to_string())),
    };
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|error| InferenceError::Transport(error.to_string()))?;
    serde_json::from_str(&body).map_err(|error| InferenceError::InvalidResponse(error.to_string()))
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
}
