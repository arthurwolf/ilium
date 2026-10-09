//! Provider-neutral inference boundary for Ilium's title and organization
//! features. Every backend implements [`InferenceProvider`], insulating the
//! client from individual HTTP envelopes and authentication details.

use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

pub use ilium_kilo_gateway::PaidProxy;
use ilium_kilo_gateway::{
    choose_random_paid_proxy, ChatMessage, CompletionRequest, CompletionStreamEvent, GatewayError,
    KiloGatewayClient, DEFAULT_BASE_URL as DEFAULT_KILO_GATEWAY_URL,
    FALLBACK_FREE_MODELS as KILO_GATEWAY_FALLBACK_MODELS,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DEFAULT_OLLAMA_URL: &str = "http://127.0.0.1:11434";
pub const DEFAULT_OPENAI_URL: &str = "https://api.openai.com/v1";
pub const DEFAULT_ANTHROPIC_URL: &str = "https://api.anthropic.com";
pub const DEFAULT_OPENROUTER_URL: &str = "https://openrouter.ai/api/v1";
pub const DEFAULT_OPENROUTER_MODEL: &str = "openrouter/free";
/// The Kilo Gateway model a fresh install selects. A concrete free model is
/// used instead of the `kilo-auto/free` router because every request here asks
/// for `UNKNOWN_MODEL_MAX_OUTPUT_TOKENS` (1,000,000) output tokens, and the
/// router's 1,000,000-token context window rejects that with HTTP 400
/// (`context_length_exceeded`, observed 2026-09-26), which would make the
/// default-on AI titling and restructuring fail on first boot.
pub const DEFAULT_KILO_GATEWAY_SELECTED_MODEL: &str = "stepfun/step-3.7-flash:free";
pub const DEFAULT_PROXY_DATABASE_URI: &str = "mongodb://127.0.0.1:27017";
pub const DEFAULT_PROXY_DATABASE_NAME: &str = "money";
pub const DEFAULT_PROXY_COLLECTION_NAME: &str = "paid_proxies";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
const MAXIMUM_PROVIDER_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;
const MAXIMUM_STREAM_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
/// Fallback budget for backends without an authoritative output capability.
/// Official OpenAI does not transmit this fallback: it uses a documented model
/// maximum where known and otherwise omits the explicit output-token limit.
/// Never replace an unknown maximum with a small convenience budget.
pub const UNKNOWN_MODEL_MAX_OUTPUT_TOKENS: u32 = 1_000_000;

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
    /// True when the model catalog is scoped to one endpoint and API key, so
    /// discovery results must be invalidated when either changes.
    pub const fn has_keyed_model_catalog(self) -> bool {
        matches!(self, Self::OpenAi | Self::Anthropic)
    }
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

/// How AI-authored pane titles are chosen. This affects title inference only;
/// provider selection and user-written titles keep their own ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TitleStyle {
    #[default]
    Labeling,
    Summarization,
}

/// Extra user guidance added to application prompts when inference runs.
#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PromptInstructions {
    pub entry_naming: String,
    pub organization: String,
    pub naming_and_organization: String,
    pub project_naming: String,
    pub smart_copy: String,
    pub ask_for_update: String,
}

/// Default safety budget for rendered restructure input, in estimated tokens.
pub const DEFAULT_RESTRUCTURE_PROMPT_TOKEN_LIMIT: u32 = 200_000;

fn deserialize_positive_token_limit<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<u32, D::Error> {
    let value = u32::deserialize(deserializer)?;
    if value == 0 {
        return Err(serde::de::Error::custom(
            "restructure_prompt_token_limit must be greater than zero",
        ));
    }
    Ok(value)
}

/// Complete durable settings. Switching providers preserves every other
/// provider's endpoint, model, and credentials for a later switch back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InferenceSettings {
    pub selected_provider: InferenceProviderKind,
    pub title_style: TitleStyle,
    /// Rough input-token budget (four Unicode characters per token).
    #[serde(deserialize_with = "deserialize_positive_token_limit")]
    pub restructure_prompt_token_limit: u32,
    pub instructions: PromptInstructions,
    pub kilo_gateway: KiloGatewaySettings,
    pub ollama: OllamaSettings,
    pub openai: ApiKeyProviderSettings,
    pub anthropic: ApiKeyProviderSettings,
    pub openrouter: OpenRouterSettings,
}

impl Default for InferenceSettings {
    fn default() -> Self {
        Self {
            selected_provider: InferenceProviderKind::KiloGateway,
            title_style: TitleStyle::default(),
            restructure_prompt_token_limit: DEFAULT_RESTRUCTURE_PROMPT_TOKEN_LIMIT,
            instructions: PromptInstructions::default(),
            kilo_gateway: KiloGatewaySettings::default(),
            ollama: OllamaSettings::default(),
            openai: ApiKeyProviderSettings::new(DEFAULT_OPENAI_URL),
            anthropic: ApiKeyProviderSettings::new(DEFAULT_ANTHROPIC_URL),
            openrouter: OpenRouterSettings::default(),
        }
    }
}

impl InferenceSettings {
    /// Returns the exact persisted model used by the selected provider.
    pub fn selected_model(&self) -> &str {
        match self.selected_provider {
            InferenceProviderKind::KiloGateway => &self.kilo_gateway.model,
            InferenceProviderKind::Ollama => &self.ollama.model,
            InferenceProviderKind::OpenAi => &self.openai.model,
            InferenceProviderKind::Anthropic => &self.anthropic.model,
            InferenceProviderKind::OpenRouter => &self.openrouter.model,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct KiloGatewaySettings {
    pub model: String,
    /// Power-user escape hatch, deliberately absent from the settings UI:
    /// enable only by hand-editing `config.toml`'s `[inference.kilo_gateway]`
    /// table. When true, every Kilo Gateway call is routed through one proxy
    /// loaded from `proxy_database` at client boot instead of calling Kilo
    /// directly.
    #[serde(default)]
    pub paid_proxies_enabled: bool,
    /// MongoDB source for the paid proxies. The database source is durable
    /// configuration; the records themselves are loaded into `paid_proxies`
    /// once during boot and are never serialized into `config.toml`.
    pub proxy_database: ProxyDatabaseSettings,
    /// Runtime-only proxy records loaded from MongoDB during client boot.
    #[serde(skip, default)]
    pub paid_proxies: Vec<PaidProxy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProxyDatabaseSettings {
    pub uri: String,
    pub database: String,
    pub collection: String,
    pub structure: ProxyDatabaseStructure,
}

impl Default for ProxyDatabaseSettings {
    fn default() -> Self {
        Self {
            uri: DEFAULT_PROXY_DATABASE_URI.to_string(),
            database: DEFAULT_PROXY_DATABASE_NAME.to_string(),
            collection: DEFAULT_PROXY_COLLECTION_NAME.to_string(),
            structure: ProxyDatabaseStructure::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProxyDatabaseStructure {
    pub ip: String,
    pub port: String,
    pub protocol: String,
    pub username: String,
    pub password: String,
    pub enabled: String,
}

impl Default for ProxyDatabaseStructure {
    fn default() -> Self {
        Self {
            ip: "ip".to_string(),
            port: "port".to_string(),
            protocol: "protocol".to_string(),
            username: "username".to_string(),
            password: "password".to_string(),
            enabled: "enabled".to_string(),
        }
    }
}

impl Default for KiloGatewaySettings {
    fn default() -> Self {
        Self {
            model: DEFAULT_KILO_GATEWAY_SELECTED_MODEL.to_string(),
            paid_proxies_enabled: false,
            proxy_database: ProxyDatabaseSettings::default(),
            paid_proxies: Vec::new(),
        }
    }
}

/// Builds a Kilo Gateway client honoring the hidden paid-proxies flag: a
/// fresh random proxy is sampled per call so repeated calls spread across
/// the configured list rather than pinning to one egress IP.
fn kilo_gateway_client(
    settings: &KiloGatewaySettings,
    timeout: Option<Duration>,
) -> Result<KiloGatewayClient, InferenceError> {
    let mut client = KiloGatewayClient::default();
    if let Some(timeout) = timeout {
        client = client.with_request_timeout(timeout);
    }
    if !settings.paid_proxies_enabled {
        return Ok(client);
    }
    match choose_random_paid_proxy(&settings.paid_proxies) {
        Some(proxy) => Ok(client.with_proxy_url(proxy.connect_url())),
        None => Err(InferenceError::Configuration(
            "paid proxy egress is enabled but no proxies were loaded from MongoDB".to_string(),
        )),
    }
}

/// Returns the stable router IDs shown before the live Kilo catalog has been
/// loaded, and retained when discovery fails.
pub fn kilo_gateway_fallback_models() -> Vec<String> {
    KILO_GATEWAY_FALLBACK_MODELS
        .iter()
        .map(|model| (*model).to_string())
        .collect()
}

pub fn kilo_gateway_model_catalog_url() -> String {
    format!("{DEFAULT_KILO_GATEWAY_URL}/models")
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
    // `ApiKeyProviderSettings` backs both the `openai` and `anthropic` fields
    // of `InferenceSettings`, which have different correct endpoints -- this
    // shared type cannot know which one it is for, so it must not bake in
    // either provider's URL. A blank `base_url` is resolved to the right
    // default inside each provider's own `complete`, not here. Baking
    // `DEFAULT_OPENAI_URL` in here previously meant a hand-edited
    // `[inference.anthropic]` table missing only `base_url` silently
    // defaulted to OpenAI's endpoint (`#[serde(default)]` fills any field
    // missing from a *present* table from this impl).
    fn default() -> Self {
        Self::new("")
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
    /// Whole-request transport timeout. `None` keeps the short default meant
    /// for titles and JSON helpers; long summarization calls set it higher.
    pub timeout: Option<Duration>,
}
impl InferenceRequest {
    pub fn json_only(user_prompt: impl Into<String>) -> Self {
        Self {
            system_prompt: ilium_prompts::naming::JSON_ONLY.to_string(),
            user_prompt: user_prompt.into(),
            max_tokens: UNKNOWN_MODEL_MAX_OUTPUT_TOKENS,
            timeout: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceResponse {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferenceStreamEvent {
    TextDelta(String),
    /// Exact provider-reported output-token usage. Providers commonly emit
    /// this only in the terminal event; callers must not infer exact token
    /// counts from transport chunks.
    OutputTokens(u64),
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
/// optional because only providers with a reliable catalog implement it.
pub trait InferenceProvider: Send + Sync {
    fn kind(&self) -> InferenceProviderKind;
    fn selected_model(&self) -> Option<&str> {
        None
    }
    fn complete(&self, request: &InferenceRequest) -> Result<InferenceResponse, InferenceError>;
    /// Streams one completion. Returning `false` from `on_event` cancels the
    /// request by dropping its response body. Implementations must not replay
    /// an already-observed prefix through an automatic retry.
    fn stream(
        &self,
        request: &InferenceRequest,
        on_event: &mut dyn FnMut(InferenceStreamEvent) -> bool,
    ) -> Result<(), InferenceError> {
        let response = self.complete(request)?;
        let _ = on_event(InferenceStreamEvent::TextDelta(response.text));
        Ok(())
    }
    fn list_models(&self) -> Result<Vec<String>, InferenceError> {
        Ok(Vec::new())
    }
}

pub fn provider_from_settings(settings: &InferenceSettings) -> Box<dyn InferenceProvider> {
    let inner: Box<dyn InferenceProvider> = match settings.selected_provider {
        InferenceProviderKind::KiloGateway => {
            Box::new(KiloGatewayProvider(Arc::new(settings.kilo_gateway.clone())))
        }
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

    fn selected_model(&self) -> Option<&str> {
        self.inner.selected_model()
    }

    fn complete(&self, request: &InferenceRequest) -> Result<InferenceResponse, InferenceError> {
        let operation_id = next_operation_id();
        let provider = self.inner.kind();
        let selected_model = self.inner.selected_model().unwrap_or("<not configured>");
        let started_at = Instant::now();
        let operation_span = tracing::info_span!("llm_inference", operation_id, ?provider);
        let _operation_guard = operation_span.enter();
        tracing::info!(
            operation_id,
            ?provider,
            selected_model,
            max_tokens = request.max_tokens,
            system_prompt_characters = request.system_prompt.chars().count(),
            user_prompt_characters = request.user_prompt.chars().count(),
            "LLM inference started"
        );
        tracing::debug!(
            operation_id,
            ?provider,
            system_prompt = %request.system_prompt,
            user_prompt = %request.user_prompt,
            "LLM inference prompt"
        );
        let result = self.inner.complete(request);
        match &result {
            Ok(response) => tracing::info!(
                operation_id,
                ?provider,
                elapsed_milliseconds = started_at.elapsed().as_millis(),
                response_characters = response.text.chars().count(),
                "LLM inference completed"
            ),
            Err(error) => tracing::error!(
                operation_id,
                ?provider,
                elapsed_milliseconds = started_at.elapsed().as_millis(),
                error_kind = inference_error_kind(error),
                http_status = ?inference_http_status(error),
                "LLM inference failed"
            ),
        }
        if let Ok(response) = &result {
            tracing::debug!(operation_id, ?provider, response_text = %response.text, "LLM inference response");
        } else if let Err(error) = &result {
            tracing::debug!(operation_id, ?provider, error = %error, error_debug = ?error, "LLM inference failure details");
        }
        result
    }

    fn stream(
        &self,
        request: &InferenceRequest,
        on_event: &mut dyn FnMut(InferenceStreamEvent) -> bool,
    ) -> Result<(), InferenceError> {
        let operation_id = next_operation_id();
        let provider = self.inner.kind();
        let selected_model = self.inner.selected_model().unwrap_or("<not configured>");
        let started_at = Instant::now();
        let mut response_characters = 0usize;
        tracing::info!(
            operation_id,
            ?provider,
            selected_model,
            max_tokens = request.max_tokens,
            "LLM inference stream started"
        );
        let result = self.inner.stream(request, &mut |event| {
            if let InferenceStreamEvent::TextDelta(text) = &event {
                response_characters = response_characters.saturating_add(text.chars().count());
            }
            on_event(event)
        });
        match &result {
            Ok(()) => tracing::info!(
                operation_id,
                ?provider,
                elapsed_milliseconds = started_at.elapsed().as_millis(),
                response_characters,
                "LLM inference stream completed"
            ),
            Err(error) => tracing::error!(
                operation_id,
                ?provider,
                elapsed_milliseconds = started_at.elapsed().as_millis(),
                response_characters,
                error_kind = inference_error_kind(error),
                "LLM inference stream failed"
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
                model_count = models.len(),
                "LLM model discovery completed"
            ),
            Err(error) => tracing::error!(
                operation_id,
                ?provider,
                elapsed_milliseconds = started_at.elapsed().as_millis(),
                error_kind = inference_error_kind(error),
                http_status = ?inference_http_status(error),
                "LLM model discovery failed"
            ),
        }
        if let Ok(models) = &result {
            tracing::debug!(
                operation_id,
                ?provider,
                ?models,
                "LLM model discovery result"
            );
        } else if let Err(error) = &result {
            tracing::debug!(operation_id, ?provider, error = %error, error_debug = ?error, "LLM model discovery failure details");
        }
        result
    }
}

pub struct KiloGatewayProvider(Arc<KiloGatewaySettings>);
impl InferenceProvider for KiloGatewayProvider {
    fn kind(&self) -> InferenceProviderKind {
        InferenceProviderKind::KiloGateway
    }
    fn selected_model(&self) -> Option<&str> {
        Some(&self.0.model)
    }
    fn complete(&self, request: &InferenceRequest) -> Result<InferenceResponse, InferenceError> {
        require(
            &self.0.model,
            "Select a Kilo Gateway model before testing or using inference",
        )?;
        let timeout = request.timeout;
        let request = CompletionRequest::new(
            &self.0.model,
            vec![
                ChatMessage::system(request.system_prompt.as_str()),
                ChatMessage::user(request.user_prompt.as_str()),
            ],
            request.max_tokens,
        );
        kilo_gateway_client(&self.0, timeout)?
            .complete_text(&request)
            .map(|text| InferenceResponse { text })
            .map_err(map_gateway_error)
    }

    fn stream(
        &self,
        request: &InferenceRequest,
        on_event: &mut dyn FnMut(InferenceStreamEvent) -> bool,
    ) -> Result<(), InferenceError> {
        require(
            &self.0.model,
            "Select a Kilo Gateway model before using inference",
        )?;
        let timeout = request.timeout;
        let request = CompletionRequest::new(
            &self.0.model,
            vec![
                ChatMessage::system(request.system_prompt.as_str()),
                ChatMessage::user(request.user_prompt.as_str()),
            ],
            request.max_tokens,
        );
        kilo_gateway_client(&self.0, timeout)?
            .stream_text(&request, &mut |event| match event {
                CompletionStreamEvent::TextDelta(text) => {
                    on_event(InferenceStreamEvent::TextDelta(text))
                }
                CompletionStreamEvent::OutputTokens(tokens) => {
                    on_event(InferenceStreamEvent::OutputTokens(tokens))
                }
            })
            .map_err(map_gateway_error)
    }

    fn list_models(&self) -> Result<Vec<String>, InferenceError> {
        let models = kilo_gateway_client(&self.0, None)?
            .list_free_models()
            .map_err(map_gateway_error)?;
        if models.is_empty() {
            return Err(InferenceError::InvalidResponse(
                "Kilo Gateway catalog contained no compatible free text models".to_string(),
            ));
        }
        Ok(models)
    }
}

fn map_gateway_error(error: GatewayError) -> InferenceError {
    match error {
        GatewayError::Http { status, message } => InferenceError::Http { status, message },
        GatewayError::Transport(message) => InferenceError::Transport(message),
        GatewayError::InvalidResponse(message) => InferenceError::InvalidResponse(message),
        GatewayError::InvalidProxy(message) => {
            InferenceError::Configuration(format!("paid proxy configuration invalid: {message}"))
        }
    }
}

fn inference_error_kind(error: &InferenceError) -> &'static str {
    match error {
        InferenceError::Configuration(_) => "configuration",
        InferenceError::Http { .. } => "http",
        InferenceError::Transport(_) => "transport",
        InferenceError::InvalidResponse(_) => "invalid_response",
    }
}

fn inference_http_status(error: &InferenceError) -> Option<u16> {
    match error {
        InferenceError::Http { status, .. } => Some(*status),
        _ => None,
    }
}

struct OllamaProvider(Arc<OllamaSettings>);
impl InferenceProvider for OllamaProvider {
    fn kind(&self) -> InferenceProviderKind {
        InferenceProviderKind::Ollama
    }
    fn selected_model(&self) -> Option<&str> {
        Some(&self.0.model)
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
            request.timeout,
        )?;
        response_text(&response, &["message", "content"])
    }
    fn stream(
        &self,
        request: &InferenceRequest,
        on_event: &mut dyn FnMut(InferenceStreamEvent) -> bool,
    ) -> Result<(), InferenceError> {
        require(
            &self.0.model,
            "Select an Ollama model before using inference",
        )?;
        post_stream(
            &format_url(&self.0.base_url, "api/chat"),
            &[],
            serde_json::json!({"model":self.0.model,"stream":true,"messages":[{"role":"system","content":request.system_prompt},{"role":"user","content":request.user_prompt}],"options":{"temperature":0.0,"num_predict":request.max_tokens}}),
            request.timeout,
            StreamProtocol::OllamaJsonLines,
            on_event,
        )
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

/// Official OpenAI policy applies to the parsed HTTPS host, not to a provider
/// name, model-name prefix, URL prefix, path, or user-info component.
fn is_official_openai_base(base_url: &str) -> bool {
    url::Url::parse(resolve_base_url(base_url, DEFAULT_OPENAI_URL))
        .ok()
        .is_some_and(|url| url.scheme() == "https" && url.host_str() == Some("api.openai.com"))
}

/// Exact identifiers with output maxima verified against OpenAI's model pages
/// on 2026-10-02. This is not a capability catalog and does not infer anything
/// about fine-tunes, arbitrary dated identifiers, or similarly named models.
///
/// Sources:
/// https://developers.openai.com/api/docs/models/gpt-6-luna
/// https://developers.openai.com/api/docs/models/gpt-6-astra
/// https://developers.openai.com/api/docs/models/gpt-5
/// https://developers.openai.com/api/docs/models/gpt-5-mini
/// https://developers.openai.com/api/docs/models/gpt-5-nano
/// https://developers.openai.com/api/docs/models/gpt-4.1
/// https://developers.openai.com/api/docs/models/gpt-4o
/// https://developers.openai.com/api/docs/models/o3
fn official_openai_max_output_tokens(model: &str) -> Option<u32> {
    match model {
        "gpt-6-luna"
        | "gpt-6-astra"
        | "gpt-5"
        | "gpt-5-2025-08-07"
        | "gpt-5-mini"
        | "gpt-5-mini-2025-08-07"
        | "gpt-5-nano"
        | "gpt-5-nano-2025-08-07" => Some(128_000),
        "gpt-4.1" | "gpt-4.1-2025-04-14" => Some(32_768),
        "gpt-4o" => Some(16_384),
        "o3" | "o3-2025-04-16" => Some(100_000),
        _ => None,
    }
}

/// The only payload builder used by both OpenAI-compatible completion paths.
///
/// Official OpenAI:
/// - never receives max_tokens;
/// - never receives an explicit sampling temperature;
/// - receives the documented model maximum when this registry knows it;
/// - otherwise receives no explicit output-token budget.
///
/// The caller's max_tokens remains unchanged for custom compatible endpoints
/// and OpenRouter. For official OpenAI, the user's maximum-output policy takes
/// precedence over that provider-neutral fallback or any smaller caller hint.
fn openai_chat_payload(
    base_url: &str,
    model: &str,
    request: &InferenceRequest,
    stream: bool,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": request.system_prompt},
            {"role": "user", "content": request.user_prompt}
        ],
        "stream": stream
    });

    if is_official_openai_base(base_url) {
        if let Some(maximum) = official_openai_max_output_tokens(model) {
            body["max_completion_tokens"] = serde_json::Value::from(maximum);
        }
    } else {
        body["temperature"] = serde_json::json!(0.0);
        body["max_tokens"] = serde_json::Value::from(request.max_tokens);
    }

    if stream {
        body["stream_options"] = serde_json::json!({"include_usage": true});
    }

    body
}

/// Builds the new catalog request URL from the effective OpenAI-compatible
/// base. Preserve a custom path prefix and query; fragments are not sent.
///
/// Completion URL construction is deliberately not changed here, preserving
/// existing custom-compatible and OpenRouter completion behavior.
fn openai_model_catalog_url(base_url: &str) -> Result<url::Url, InferenceError> {
    api_model_catalog_url(base_url, DEFAULT_OPENAI_URL, "/models", "OpenAI-compatible")
}

/// Anthropic's base URL is the host only; the catalog lives under `/v1`.
/// `limit=1000` is the documented maximum page size, so one request covers
/// the catalog without following `after_id` pagination.
fn anthropic_model_catalog_url(base_url: &str) -> Result<url::Url, InferenceError> {
    let mut url =
        api_model_catalog_url(base_url, DEFAULT_ANTHROPIC_URL, "/v1/models", "Anthropic")?;
    url.set_query(Some("limit=1000"));
    Ok(url)
}

fn api_model_catalog_url(
    base_url: &str,
    default_base_url: &str,
    catalog_path_suffix: &str,
    provider_label: &str,
) -> Result<url::Url, InferenceError> {
    let invalid_url = || {
        InferenceError::Configuration(format!(
            "{provider_label} API URL must be an absolute HTTP or HTTPS URL"
        ))
    };

    let mut url =
        url::Url::parse(resolve_base_url(base_url, default_base_url)).map_err(|_| invalid_url())?;

    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(invalid_url());
    }

    let path = format!("{}{catalog_path_suffix}", url.path().trim_end_matches('/'));
    url.set_path(&path);
    url.set_fragment(None);
    Ok(url)
}

/// Credential-redacted catalog endpoint for progress metadata and rendering.
///
/// This is display metadata, NEVER a transport URL or cache identity. In
/// particular, it cannot distinguish API keys and cannot detect an ABA edit.
/// App must use its own monotonic discovery revision for that purpose.
///
/// Unsupported providers return None. An invalid OpenAI-compatible URL still
/// identifies a supported discovery operation; list_models reports its safe
/// configuration error without echoing the supplied URL.
pub fn model_catalog_endpoint(settings: &InferenceSettings) -> Option<String> {
    match settings.selected_provider {
        InferenceProviderKind::KiloGateway => Some(kilo_gateway_model_catalog_url()),
        InferenceProviderKind::Ollama => Some(ilium_logging::redacted_url(&format_url(
            &settings.ollama.base_url,
            "api/tags",
        ))),
        InferenceProviderKind::OpenAi => Some(redacted_catalog_display(
            openai_model_catalog_url(&settings.openai.base_url),
            &settings.openai.api_key,
            "<invalid OpenAI-compatible API URL>",
        )),
        InferenceProviderKind::Anthropic => Some(redacted_catalog_display(
            anthropic_model_catalog_url(&settings.anthropic.base_url),
            &settings.anthropic.api_key,
            "<invalid Anthropic API URL>",
        )),
        InferenceProviderKind::OpenRouter => None,
    }
}

fn redacted_catalog_display(
    url: Result<url::Url, InferenceError>,
    api_key: &str,
    invalid_display: &str,
) -> String {
    let Ok(mut url) = url else {
        return invalid_display.to_string();
    };

    let had_query = url.query().is_some();
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);

    let mut display = url.to_string();
    if had_query {
        display.push_str("?<redacted>");
    }

    // Also suppress a literal configured key accidentally placed in a custom
    // URL path. Never put the key itself into presentation state.
    if !api_key.is_empty() {
        display = display.replace(api_key, "<redacted>");
    }
    display
}

/// Extract exact IDs, not guessed chat capabilities or output limits.
///
/// Additional metadata is allowed. Every record must have a usable ID; one
/// malformed record fails the catalog rather than publishing a partial list.
/// No response values are interpolated into validation errors.
fn parse_openai_model_catalog(response: &serde_json::Value) -> Result<Vec<String>, InferenceError> {
    if response.get("error").is_some_and(|error| !error.is_null()) {
        return Err(InferenceError::InvalidResponse(
            "Model catalog returned an error envelope".to_string(),
        ));
    }

    if response
        .get("object")
        .is_some_and(|object| object.as_str() != Some("list"))
    {
        return Err(InferenceError::InvalidResponse(
            "Model catalog object must be a list".to_string(),
        ));
    }

    let records = response
        .get("data")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            InferenceError::InvalidResponse("Model catalog is missing its data array".to_string())
        })?;

    let mut models = Vec::with_capacity(records.len());
    for record in records {
        let id = record
            .get("id")
            .and_then(serde_json::Value::as_str)
            .filter(|id| {
                !id.is_empty()
                    && !id
                        .chars()
                        .any(|character| character.is_control() || character.is_whitespace())
            })
            .ok_or_else(|| {
                InferenceError::InvalidResponse(
                    "Model catalog contains a missing or invalid model ID".to_string(),
                )
            })?;
        models.push(id.to_string());
    }

    models.sort_unstable();
    models.dedup();
    if models.is_empty() {
        return Err(InferenceError::InvalidResponse(
            "Model catalog contained no model IDs".to_string(),
        ));
    }
    Ok(models)
}

/// Authenticated discovery has no selected-model prerequisite.
///
/// Do not route this through get_json/send: those existing general-purpose
/// helpers log raw response bodies, including authentication-error bodies.
/// This narrowly scoped path returns safe errors before DiagnosticProvider or
/// the client worker can log them, without changing other providers' transport.
fn list_openai_models(settings: &ApiKeyProviderSettings) -> Result<Vec<String>, InferenceError> {
    require(
        &settings.api_key,
        "Enter an API key before loading OpenAI-compatible models",
    )?;

    let url = openai_model_catalog_url(&settings.base_url)?;
    list_authenticated_models(
        "OpenAI-compatible",
        &url,
        &[("Authorization", format!("Bearer {}", settings.api_key))],
        &settings.api_key,
    )
}

fn list_anthropic_models(settings: &ApiKeyProviderSettings) -> Result<Vec<String>, InferenceError> {
    require(
        &settings.api_key,
        "Enter an API key before loading Anthropic models",
    )?;

    let url = anthropic_model_catalog_url(&settings.base_url)?;
    list_authenticated_models(
        "Anthropic",
        &url,
        &[
            ("x-api-key", settings.api_key.clone()),
            ("anthropic-version", "2023-06-01".to_string()),
        ],
        &settings.api_key,
    )
}

/// Performs one authenticated GET and validates the returned catalog. Both
/// OpenAI-compatible and Anthropic catalogs are `{"data":[{"id":...}]}`.
fn list_authenticated_models(
    provider_label: &str,
    url: &url::Url,
    auth_headers: &[(&str, String)],
    api_key: &str,
) -> Result<Vec<String>, InferenceError> {
    let mut request = agent().get(url.as_str());
    for (name, value) in auth_headers {
        request = request.header(*name, value.as_str());
    }
    let mut response = request.call().map_err(|_| {
        InferenceError::Transport(format!(
            "{provider_label} model discovery request failed; check the endpoint and connection"
        ))
    })?;

    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        // Do not read, render, or log an authentication-error body: an endpoint
        // can reflect the supplied credential in either text or JSON.
        let message = match status {
            401 => format!("Authentication failed; check the {provider_label} API key"),
            403 => {
                "Model discovery access denied; check the key and project permissions".to_string()
            }
            404 => "Model catalog endpoint not found; check the API base URL".to_string(),
            429 => "Model discovery was rate limited; retry later".to_string(),
            _ => "Model discovery request failed; response body withheld".to_string(),
        };
        return Err(InferenceError::Http { status, message });
    }

    let body = response
        .body_mut()
        .with_config()
        .limit(MAXIMUM_PROVIDER_RESPONSE_BYTES)
        .read_to_string()
        .map_err(|_| {
            InferenceError::Transport(
                "Could not read the model catalog within the response-size limit".to_string(),
            )
        })?;

    let response: serde_json::Value = serde_json::from_str(&body).map_err(|_| {
        InferenceError::InvalidResponse("Model catalog was not valid JSON".to_string())
    })?;
    let models = parse_openai_model_catalog(&response)?;

    // Successful bodies are untrusted too. DiagnosticProvider logs returned
    // model IDs, so reject credential reflection before returning the list.
    if models.iter().any(|model| model.contains(api_key)) {
        return Err(InferenceError::InvalidResponse(
            "Model catalog reflected credential material; response withheld".to_string(),
        ));
    }

    Ok(models)
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
        openai_chat_payload(base_url, model, request, false),
        request.timeout,
    )?;
    openai_compatible_response_text(&response)
}

fn stream_openai_compatible(
    base_url: &str,
    api_key: &str,
    model: &str,
    request: &InferenceRequest,
    on_event: &mut dyn FnMut(InferenceStreamEvent) -> bool,
) -> Result<(), InferenceError> {
    require(api_key, "Enter an API key before using inference")?;
    require(model, "Enter a model before using inference")?;
    post_stream(
        &format_url(base_url, "chat/completions"),
        &[("Authorization", format!("Bearer {api_key}"))],
        openai_chat_payload(base_url, model, request, true),
        request.timeout,
        StreamProtocol::OpenAiSse,
        on_event,
    )
}

struct OpenAiProvider(Arc<ApiKeyProviderSettings>);
impl InferenceProvider for OpenAiProvider {
    fn kind(&self) -> InferenceProviderKind {
        InferenceProviderKind::OpenAi
    }

    fn selected_model(&self) -> Option<&str> {
        Some(&self.0.model)
    }

    fn complete(&self, request: &InferenceRequest) -> Result<InferenceResponse, InferenceError> {
        complete_openai_compatible(
            resolve_base_url(&self.0.base_url, DEFAULT_OPENAI_URL),
            &self.0.api_key,
            &self.0.model,
            request,
        )
    }

    fn stream(
        &self,
        request: &InferenceRequest,
        on_event: &mut dyn FnMut(InferenceStreamEvent) -> bool,
    ) -> Result<(), InferenceError> {
        stream_openai_compatible(
            resolve_base_url(&self.0.base_url, DEFAULT_OPENAI_URL),
            &self.0.api_key,
            &self.0.model,
            request,
            on_event,
        )
    }

    fn list_models(&self) -> Result<Vec<String>, InferenceError> {
        list_openai_models(&self.0)
    }
}
struct OpenRouterProvider(Arc<OpenRouterSettings>);
impl InferenceProvider for OpenRouterProvider {
    fn kind(&self) -> InferenceProviderKind {
        InferenceProviderKind::OpenRouter
    }
    fn selected_model(&self) -> Option<&str> {
        Some(&self.0.model)
    }
    fn complete(&self, request: &InferenceRequest) -> Result<InferenceResponse, InferenceError> {
        complete_openai_compatible(
            DEFAULT_OPENROUTER_URL,
            &self.0.api_key,
            &self.0.model,
            request,
        )
    }
    fn stream(
        &self,
        request: &InferenceRequest,
        on_event: &mut dyn FnMut(InferenceStreamEvent) -> bool,
    ) -> Result<(), InferenceError> {
        stream_openai_compatible(
            DEFAULT_OPENROUTER_URL,
            &self.0.api_key,
            &self.0.model,
            request,
            on_event,
        )
    }
}
struct AnthropicProvider(Arc<ApiKeyProviderSettings>);
impl InferenceProvider for AnthropicProvider {
    fn kind(&self) -> InferenceProviderKind {
        InferenceProviderKind::Anthropic
    }
    fn selected_model(&self) -> Option<&str> {
        Some(&self.0.model)
    }
    fn list_models(&self) -> Result<Vec<String>, InferenceError> {
        list_anthropic_models(&self.0)
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
            &format_url(
                resolve_base_url(&self.0.base_url, DEFAULT_ANTHROPIC_URL),
                "v1/messages",
            ),
            &[
                ("x-api-key", self.0.api_key.as_str().to_string()),
                ("anthropic-version", "2023-06-01".to_string()),
            ],
            anthropic_messages_payload(&self.0.model, request, false),
            request.timeout,
        )?;
        anthropic_response_text(&response)
    }
    fn stream(
        &self,
        request: &InferenceRequest,
        on_event: &mut dyn FnMut(InferenceStreamEvent) -> bool,
    ) -> Result<(), InferenceError> {
        require(&self.0.api_key, "Enter an API key before using inference")?;
        require(&self.0.model, "Enter a model before using inference")?;
        post_stream(
            &format_url(
                resolve_base_url(&self.0.base_url, DEFAULT_ANTHROPIC_URL),
                "v1/messages",
            ),
            &[
                ("x-api-key", self.0.api_key.clone()),
                ("anthropic-version", "2023-06-01".to_string()),
            ],
            anthropic_messages_payload(&self.0.model, request, true),
            request.timeout,
            StreamProtocol::AnthropicSse,
            on_event,
        )
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
/// Falls back to `default_base_url` when settings carry a blank `base_url`
/// (the safe zero value for `ApiKeyProviderSettings`, since that type is
/// shared between providers with different correct endpoints and cannot
/// bake either one into its own `Default`).
fn resolve_base_url<'settings>(
    base_url: &'settings str,
    default_base_url: &'settings str,
) -> &'settings str {
    if base_url.trim().is_empty() {
        default_base_url
    } else {
        base_url
    }
}
fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ilium_http::agent(
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
    timeout: Option<Duration>,
) -> Result<serde_json::Value, InferenceError> {
    let diagnostic_url = ilium_logging::redacted_url(url);
    tracing::info!(
        method = "POST",
        url = %diagnostic_url,
        headers = ?redacted_headers(headers),
        request_characters = body.to_string().chars().count(),
        "HTTP request started"
    );
    tracing::debug!(method = "POST", url = %diagnostic_url, request_body = %body, "HTTP request payload");
    let mut request = agent().post(url).header("Content-Type", "application/json");
    if let Some(timeout) = timeout {
        request = request.config().timeout_global(Some(timeout)).build();
    }
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
                error = %error_message,
                "HTTP transport failed"
            );
            tracing::debug!(method, url = %diagnostic_url, request_body = ?request_body, "failed HTTP request payload");
            return Err(InferenceError::Transport(error_message));
        }
    };
    let status = response.status().as_u16();
    let response_headers = redacted_response_headers(response.headers());
    let body = response
        .body_mut()
        .with_config()
        .limit(MAXIMUM_PROVIDER_RESPONSE_BYTES)
        .read_to_string()
        .map_err(|error| {
            tracing::error!(method, url = %diagnostic_url, status, ?response_headers, error = %error, "failed to read HTTP response body");
            InferenceError::Transport(error.to_string())
        })?;
    if !(200..300).contains(&status) {
        tracing::error!(method, url = %diagnostic_url, status, ?response_headers, response_characters = body.chars().count(), "HTTP request failed");
        tracing::debug!(method, url = %diagnostic_url, status, response_body = %body, "HTTP error response body");
        return Err(InferenceError::Http {
            status,
            message: body,
        });
    }
    tracing::info!(method, url = %diagnostic_url, status, ?response_headers, response_characters = body.chars().count(), "HTTP request completed");
    tracing::debug!(method, url = %diagnostic_url, status, response_body = %body, "HTTP response body");
    serde_json::from_str(&body).map_err(|error| {
        tracing::error!(
            method,
            url = %diagnostic_url,
            status,
            ?response_headers,
            response_characters = body.chars().count(),
            error = %error,
            "HTTP response was not valid JSON"
        );
        InferenceError::InvalidResponse(error.to_string())
    })
}

#[derive(Debug, Clone, Copy)]
enum StreamProtocol {
    OpenAiSse,
    AnthropicSse,
    OllamaJsonLines,
}

fn post_stream(
    url: &str,
    headers: &[(&str, String)],
    body: serde_json::Value,
    timeout: Option<Duration>,
    protocol: StreamProtocol,
    on_event: &mut dyn FnMut(InferenceStreamEvent) -> bool,
) -> Result<(), InferenceError> {
    let diagnostic_url = ilium_logging::redacted_url(url);
    tracing::info!(method = "POST", url = %diagnostic_url, headers = ?redacted_headers(headers), request_characters = body.to_string().chars().count(), "HTTP stream request started");
    tracing::debug!(method = "POST", url = %diagnostic_url, request_body = %body, "HTTP stream request payload");
    let mut request = agent().post(url).header("Content-Type", "application/json");
    if let Some(timeout) = timeout {
        request = request.config().timeout_global(Some(timeout)).build();
    }
    for (name, value) in headers {
        request = request.header(*name, value);
    }
    let mut response = request.send_json(&body).map_err(|error| {
        InferenceError::Transport(error.to_string().replace(url, &diagnostic_url))
    })?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        let message = response
            .body_mut()
            .with_config()
            .limit(MAXIMUM_PROVIDER_RESPONSE_BYTES)
            .read_to_string()
            .map_err(|error| InferenceError::Transport(error.to_string()))?;
        return Err(InferenceError::Http { status, message });
    }

    let mut reader = BufReader::new(response.body_mut().as_reader());
    let mut line = String::new();
    let mut received_bytes = 0usize;
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|error| InferenceError::Transport(error.to_string()))?;
        if read == 0 {
            return Ok(());
        }
        received_bytes = received_bytes.saturating_add(read);
        if received_bytes > MAXIMUM_STREAM_RESPONSE_BYTES {
            return Err(InferenceError::InvalidResponse(
                "stream exceeded the 16 MiB response limit".to_string(),
            ));
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let data = match protocol {
            StreamProtocol::OllamaJsonLines => trimmed,
            StreamProtocol::OpenAiSse | StreamProtocol::AnthropicSse => {
                let Some(data) = trimmed.strip_prefix("data:").map(str::trim) else {
                    continue;
                };
                if data == "[DONE]" {
                    return Ok(());
                }
                data
            }
        };
        let value: serde_json::Value = serde_json::from_str(data)
            .map_err(|error| InferenceError::InvalidResponse(error.to_string()))?;
        if let Some(error) = value.get("error").filter(|error| !error.is_null()) {
            return Err(InferenceError::InvalidResponse(format!(
                "provider returned an error event: {error}"
            )));
        }
        match protocol {
            StreamProtocol::OpenAiSse => {
                if let Some(tokens) = value
                    .get("usage")
                    .and_then(|usage| usage.get("completion_tokens"))
                    .and_then(serde_json::Value::as_u64)
                {
                    if !on_event(InferenceStreamEvent::OutputTokens(tokens)) {
                        return Ok(());
                    }
                }
                if let Some(text) = value
                    .pointer("/choices/0/delta/content")
                    .and_then(serde_json::Value::as_str)
                {
                    if !text.is_empty()
                        && !on_event(InferenceStreamEvent::TextDelta(text.to_string()))
                    {
                        return Ok(());
                    }
                }
            }
            StreamProtocol::AnthropicSse => {
                if let Some(tokens) = value
                    .pointer("/usage/output_tokens")
                    .and_then(serde_json::Value::as_u64)
                {
                    if !on_event(InferenceStreamEvent::OutputTokens(tokens)) {
                        return Ok(());
                    }
                }
                if let Some(text) = value
                    .pointer("/delta/text")
                    .and_then(serde_json::Value::as_str)
                {
                    if !text.is_empty()
                        && !on_event(InferenceStreamEvent::TextDelta(text.to_string()))
                    {
                        return Ok(());
                    }
                }
            }
            StreamProtocol::OllamaJsonLines => {
                if let Some(tokens) = value.get("eval_count").and_then(serde_json::Value::as_u64) {
                    if !on_event(InferenceStreamEvent::OutputTokens(tokens)) {
                        return Ok(());
                    }
                }
                if let Some(text) = value
                    .pointer("/message/content")
                    .and_then(serde_json::Value::as_str)
                {
                    if !text.is_empty()
                        && !on_event(InferenceStreamEvent::TextDelta(text.to_string()))
                    {
                        return Ok(());
                    }
                }
            }
        }
    }
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

fn openai_compatible_response_text(
    value: &serde_json::Value,
) -> Result<InferenceResponse, InferenceError> {
    // Some free OpenAI-compatible routers emit a literal `"error": null` on an
    // otherwise successful envelope; only a non-null value is an actual error.
    if let Some(error) = value.get("error").filter(|error| !error.is_null()) {
        return Err(InferenceError::InvalidResponse(format!(
            "provider returned an error envelope with HTTP 200: {error}"
        )));
    }
    response_text(value, &["choices", "0", "message", "content"])
}

/// Builds a Messages API request. `temperature` is deliberately absent:
/// claude-haiku-5-5 rejects it with a 400 error, so the provider default applies.
fn anthropic_messages_payload(
    model: &str,
    request: &InferenceRequest,
    stream: bool,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": model,
        "system": request.system_prompt,
        "max_tokens": request.max_tokens,
        "messages": [{"role": "user", "content": request.user_prompt}]
    });
    if stream {
        body["stream"] = serde_json::Value::Bool(true);
    }
    body
}

fn anthropic_response_text(value: &serde_json::Value) -> Result<InferenceResponse, InferenceError> {
    if let Some(error) = value.get("error").filter(|error| !error.is_null()) {
        return Err(InferenceError::InvalidResponse(format!(
            "Anthropic returned an error envelope with HTTP 200: {error}"
        )));
    }
    let text = value
        .get("content")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|block| block.get("type").and_then(serde_json::Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(serde_json::Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        let stop_reason = value
            .get("stop_reason")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("not reported");
        return Err(InferenceError::InvalidResponse(format!(
            "Anthropic returned no non-empty text blocks (stop_reason: {stop_reason})"
        )));
    }
    Ok(InferenceResponse { text })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restructure_budget_rejects_invalid_configuration() {
        for value in ["0", "-1", "1.5", "4294967296", "\"200000\""] {
            let source = format!(r#"{{"restructure_prompt_token_limit":{value}}}"#);
            assert!(
                serde_json::from_str::<InferenceSettings>(&source).is_err(),
                "{source}"
            );
        }
    }

    #[test]
    fn restructure_budget_defaults_and_preserves_custom_tokens() {
        let defaults: InferenceSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(
            serde_json::to_value(&defaults).unwrap()["restructure_prompt_token_limit"],
            200_000
        );
        let custom: InferenceSettings =
            serde_json::from_str(r#"{"restructure_prompt_token_limit":123456}"#).unwrap();
        assert_eq!(
            serde_json::to_value(&custom).unwrap()["restructure_prompt_token_limit"],
            123456
        );
    }

    use ilium_kilo_gateway::DEFAULT_FREE_MODEL as DEFAULT_KILO_GATEWAY_MODEL;

    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    const OPENAI_CATALOG_FIXTURE_KEY: &str = "sk-fixture-openai-catalog-secret";

    fn openai_catalog_fixture_settings(response_url: &str) -> InferenceSettings {
        let origin = response_url
            .strip_suffix("/chat/completions")
            .expect("existing HTTP fixture URL suffix");
        InferenceSettings {
            selected_provider: InferenceProviderKind::OpenAi,
            openai: ApiKeyProviderSettings {
                base_url: format!("{origin}/gateway/v1///"),
                api_key: OPENAI_CATALOG_FIXTURE_KEY.to_string(),
                model: String::new(),
            },
            ..InferenceSettings::default()
        }
    }

    #[test]
    fn openai_official_policy_requires_the_parsed_exact_https_host() {
        for base in [
            "",
            "   ",
            DEFAULT_OPENAI_URL,
            "https://api.openai.com/v1/",
            "HTTPS://API.OPENAI.COM/v1",
            "https://api.openai.com:443/v1",
            "https://api.openai.com:8443/custom-prefix",
        ] {
            assert!(is_official_openai_base(base), "official base: {base}");
        }

        for base in [
            "http://api.openai.com/v1",
            "https://api.openai.com.example.invalid/v1",
            "https://api.openai.com@proxy.example/v1",
            "https://proxy.example/api.openai.com/v1",
            "https://proxy.example/v1?upstream=https://api.openai.com",
            "https://api.openai.com./v1",
            "http://127.0.0.1:8080/v1",
            DEFAULT_OPENROUTER_URL,
            "not a URL",
        ] {
            assert!(!is_official_openai_base(base), "non-official base: {base}");
        }
    }

    #[test]
    fn openai_official_complete_and_stream_payloads_omit_forbidden_parameters() {
        let request = InferenceRequest {
            system_prompt: "Return JSON only.".to_string(),
            user_prompt: "A quoted \"value\" and a newline:\nnext".to_string(),
            max_tokens: UNKNOWN_MODEL_MAX_OUTPUT_TOKENS,
            timeout: None,
        };

        for stream in [false, true] {
            for model in ["gpt-6-astra", "gpt-5", "o3", "future-unregistered-model"] {
                let body = openai_chat_payload(DEFAULT_OPENAI_URL, model, &request, stream);
                assert!(body.get("max_tokens").is_none(), "{model}, stream={stream}");
                assert!(
                    body.get("temperature").is_none(),
                    "{model}, stream={stream}"
                );
                assert!(body.get("top_p").is_none(), "{model}, stream={stream}");
                assert_eq!(body["model"], model);
                assert_eq!(body["stream"], stream);
                assert_eq!(body["messages"][0]["role"], "system");
                assert_eq!(body["messages"][0]["content"], request.system_prompt);
                assert_eq!(body["messages"][1]["role"], "user");
                assert_eq!(body["messages"][1]["content"], request.user_prompt);

                if stream {
                    assert_eq!(
                        body["stream_options"],
                        serde_json::json!({"include_usage": true})
                    );
                } else {
                    assert!(body.get("stream_options").is_none());
                }
            }
        }
    }

    #[test]
    fn openai_known_models_use_the_full_documented_maximum_not_a_caller_cap() {
        let mut request = InferenceRequest::json_only("fixture");
        request.max_tokens = 2;

        let cases = [
            ("gpt-6-luna", 128_000_u32),
            ("gpt-6-astra", 128_000_u32),
            ("gpt-5", 128_000),
            ("gpt-5-2025-08-07", 128_000),
            ("gpt-5-mini", 128_000),
            ("gpt-5-mini-2025-08-07", 128_000),
            ("gpt-5-nano", 128_000),
            ("gpt-5-nano-2025-08-07", 128_000),
            ("gpt-4.1", 32_768),
            ("gpt-4.1-2025-04-14", 32_768),
            ("gpt-4o", 16_384),
            ("o3", 100_000),
            ("o3-2025-04-16", 100_000),
        ];

        for (model, maximum) in cases {
            assert_eq!(official_openai_max_output_tokens(model), Some(maximum));
            for stream in [false, true] {
                let body = openai_chat_payload(DEFAULT_OPENAI_URL, model, &request, stream);
                assert_eq!(
                    body["max_completion_tokens"],
                    serde_json::Value::from(maximum),
                    "{model}, stream={stream}"
                );
            }
        }
    }

    #[test]
    fn openai_unknown_limits_are_omitted_without_family_or_fine_tune_guessing() {
        let request = InferenceRequest::json_only("fixture");
        for model in [
            "future-unregistered-model",
            "gpt-5-private",
            "gpt-5-2099-01-01",
            "gpt-6-astra-private",
            "ft:gpt-4.1:organization:custom:identifier",
            "gpt-4o-2024-05-13",
            "GPT-5",
        ] {
            assert_eq!(official_openai_max_output_tokens(model), None);
            for stream in [false, true] {
                let body = openai_chat_payload(DEFAULT_OPENAI_URL, model, &request, stream);
                assert!(body.get("max_completion_tokens").is_none());
                assert!(body.get("max_tokens").is_none());
            }
        }
    }

    #[test]
    fn openai_custom_and_openrouter_payloads_preserve_the_original_json_contract() {
        let request = InferenceRequest {
            system_prompt: "system fixture".to_string(),
            user_prompt: "user fixture".to_string(),
            max_tokens: 731,
            timeout: None,
        };

        for base in [
            "http://127.0.0.1:8080/v1",
            "https://custom.example/v1",
            "https://api.openai.com.example.invalid/v1",
            DEFAULT_OPENROUTER_URL,
        ] {
            for stream in [false, true] {
                let mut expected = serde_json::json!({
                    "model": "gpt-5",
                    "messages": [
                        {"role": "system", "content": "system fixture"},
                        {"role": "user", "content": "user fixture"}
                    ],
                    "temperature": 0.0,
                    "max_tokens": 731,
                    "stream": stream
                });
                if stream {
                    expected["stream_options"] = serde_json::json!({"include_usage": true});
                }
                assert_eq!(
                    openai_chat_payload(base, "gpt-5", &request, stream),
                    expected,
                    "{base}, stream={stream}"
                );
            }
        }
    }

    #[test]
    fn openai_catalog_parsing_sorts_deduplicates_and_does_not_guess_capabilities() {
        let response = serde_json::json!({
            "object": "list",
            "error": null,
            "data": [
                {"id": "z-model", "created": 1, "owned_by": "fixture"},
                {"id": "gpt-5", "shutdown_date": null},
                {"id": "text-embedding-3-small"},
                {"id": "gpt-5"},
                {"id": "ft:gpt-4.1:org:custom:abc"}
            ]
        });

        assert_eq!(
            parse_openai_model_catalog(&response).unwrap(),
            [
                "ft:gpt-4.1:org:custom:abc",
                "gpt-5",
                "text-embedding-3-small",
                "z-model",
            ]
        );

        assert_eq!(
            parse_openai_model_catalog(&serde_json::json!({
                "data": [{"id": "custom-compatible-model"}]
            }))
            .unwrap(),
            ["custom-compatible-model"]
        );
    }

    #[test]
    fn openai_catalog_parsing_rejects_empty_malformed_and_partial_catalogs() {
        let invalid = [
            serde_json::json!(null),
            serde_json::json!([]),
            serde_json::json!({}),
            serde_json::json!({"data": null}),
            serde_json::json!({"data": {}}),
            serde_json::json!({"data": []}),
            serde_json::json!({"object": "model", "data": [{"id": "ok"}]}),
            serde_json::json!({"data": [{}]}),
            serde_json::json!({"data": [null]}),
            serde_json::json!({"data": [{"id": 123}]}),
            serde_json::json!({"data": [{"id": ""}]}),
            serde_json::json!({"data": [{"id": " gpt-5"}]}),
            serde_json::json!({"data": [{"id": "gpt-5\n"}]}),
            serde_json::json!({"data": [{"id": "gpt-\u{1b}[31m5"}]}),
            serde_json::json!({"data": [{"id": "good"}, {"id": null}]}),
            serde_json::json!({
                "data": [{"id": "good"}],
                "error": {"message": "upstream failed"}
            }),
        ];

        for response in invalid {
            assert!(matches!(
                parse_openai_model_catalog(&response),
                Err(InferenceError::InvalidResponse(_))
            ));
        }
    }

    #[test]
    fn openai_catalog_url_uses_the_effective_base_and_preserves_custom_prefixes() {
        for base in [
            "",
            "   ",
            DEFAULT_OPENAI_URL,
            "https://api.openai.com/v1///",
        ] {
            assert_eq!(
                openai_model_catalog_url(base).unwrap().as_str(),
                "https://api.openai.com/v1/models"
            );
        }
        assert_eq!(
            openai_model_catalog_url(
                "https://custom.example/gateway/v1/?api-version=fixture#not-sent"
            )
            .unwrap()
            .as_str(),
            "https://custom.example/gateway/v1/models?api-version=fixture"
        );
        for base in [
            "not a URL",
            "file:///tmp/catalog",
            "mailto:fixture@example.com",
        ] {
            assert!(matches!(
                openai_model_catalog_url(base),
                Err(InferenceError::Configuration(_))
            ));
        }
    }

    #[test]
    fn openai_catalog_endpoint_metadata_is_redacted_and_model_independent() {
        let mut settings = InferenceSettings {
            selected_provider: InferenceProviderKind::OpenAi,
            ..InferenceSettings::default()
        };
        assert_eq!(
            model_catalog_endpoint(&settings).as_deref(),
            Some("https://api.openai.com/v1/models")
        );

        settings.openai.base_url = format!(
            "https://alice:password@custom.example/gateway/v1?api_key={}#ignored",
            OPENAI_CATALOG_FIXTURE_KEY
        );
        settings.openai.api_key = OPENAI_CATALOG_FIXTURE_KEY.to_string();
        settings.openai.model = "saved-choice".to_string();

        let endpoint = model_catalog_endpoint(&settings).unwrap();
        assert_eq!(
            endpoint,
            "https://custom.example/gateway/v1/models?<redacted>"
        );
        assert!(!endpoint.contains(OPENAI_CATALOG_FIXTURE_KEY));
        assert!(!endpoint.contains("alice"));
        assert!(!endpoint.contains("password"));
        assert!(!endpoint.contains("saved-choice"));

        settings.openai.base_url = "invalid supplied endpoint".to_string();
        assert_eq!(
            model_catalog_endpoint(&settings).as_deref(),
            Some("<invalid OpenAI-compatible API URL>")
        );

        settings.selected_provider = InferenceProviderKind::OpenRouter;
        assert_eq!(model_catalog_endpoint(&settings), None);
    }

    #[test]
    fn anthropic_payloads_omit_temperature_for_both_paths() {
        let request = InferenceRequest {
            system_prompt: "Return JSON only.".to_string(),
            user_prompt: "user fixture".to_string(),
            max_tokens: 512,
            timeout: None,
        };

        for stream in [false, true] {
            let body = anthropic_messages_payload("claude-haiku-5-5", &request, stream);
            assert!(body.get("temperature").is_none(), "stream={stream}");
            assert_eq!(body["model"], "claude-haiku-5-5");
            assert_eq!(body["system"], "Return JSON only.");
            assert_eq!(body["max_tokens"], 512);
            assert_eq!(body["messages"][0]["content"], "user fixture");
            assert_eq!(body.get("stream").is_some(), stream);
        }
    }

    #[test]
    fn anthropic_catalog_endpoint_is_v1_models_and_redacted() {
        let mut settings = InferenceSettings {
            selected_provider: InferenceProviderKind::Anthropic,
            ..InferenceSettings::default()
        };
        assert_eq!(
            model_catalog_endpoint(&settings).as_deref(),
            Some("https://api.anthropic.com/v1/models?<redacted>")
        );

        settings.anthropic.base_url = "https://alice:password@proxy.example/anthropic".to_string();
        settings.anthropic.api_key = OPENAI_CATALOG_FIXTURE_KEY.to_string();
        let endpoint = model_catalog_endpoint(&settings).unwrap();
        assert_eq!(
            endpoint,
            "https://proxy.example/anthropic/v1/models?<redacted>"
        );
        assert!(!endpoint.contains("alice") && !endpoint.contains("password"));
    }

    #[test]
    fn anthropic_catalog_parses_as_shared_model_list() {
        let response = serde_json::json!({
            "data": [
                {"type": "model", "id": "claude-haiku-5-5", "display_name": "Haiku"},
                {"type": "model", "id": "claude-sonnet-5-5"}
            ],
            "has_more": false,
            "first_id": "claude-haiku-5-5",
            "last_id": "claude-sonnet-5-5"
        });
        assert_eq!(
            parse_openai_model_catalog(&response).unwrap(),
            ["claude-haiku-5-5", "claude-sonnet-5-5"]
        );
    }

    #[test]
    fn openai_discovery_sends_authenticated_get_without_a_selected_model() {
        let (url, request_receiver) = spawn_http_response(
            "200 OK",
            r#"{"object":"list","data":[{"id":"z"},{"id":"a"},{"id":"z"}]}"#,
        );
        let settings = openai_catalog_fixture_settings(&url);
        assert!(settings.openai.model.is_empty());

        let models = provider_from_settings(&settings).list_models().unwrap();
        assert_eq!(models, ["a", "z"]);
        assert!(settings.openai.model.is_empty());

        let captured = request_receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("catalog request was captured");
        assert!(captured.starts_with("GET /gateway/v1/models HTTP/1.1\r\n"));
        assert!(captured.to_ascii_lowercase().contains(&format!(
            "authorization: bearer {}",
            OPENAI_CATALOG_FIXTURE_KEY
        )));
        assert_eq!(
            model_catalog_endpoint(&settings),
            Some(format!(
                "{}/gateway/v1/models",
                url.strip_suffix("/chat/completions").unwrap()
            ))
        );
    }

    #[test]
    fn openai_discovery_requires_a_key_but_not_a_model() {
        let settings = InferenceSettings {
            selected_provider: InferenceProviderKind::OpenAi,
            openai: ApiKeyProviderSettings {
                base_url: "not a valid URL".to_string(),
                api_key: "   ".to_string(),
                model: String::new(),
            },
            ..InferenceSettings::default()
        };
        let error = provider_from_settings(&settings).list_models().unwrap_err();
        assert!(matches!(
            error,
            InferenceError::Configuration(message) if message.contains("API key")
        ));
    }

    #[test]
    fn openai_discovery_reports_401_without_returning_a_reflected_key() {
        let body = format!(
            r#"{{"error":{{"message":"raw-body-marker {}"}}}}"#,
            OPENAI_CATALOG_FIXTURE_KEY
        );
        let (url, request_receiver) = spawn_http_response("401 Unauthorized", &body);
        let settings = openai_catalog_fixture_settings(&url);

        let error = provider_from_settings(&settings).list_models().unwrap_err();
        assert!(matches!(&error, InferenceError::Http { status: 401, .. }));
        assert!(error.to_string().contains("Authentication failed"));
        assert!(!format!("{error:?}").contains(OPENAI_CATALOG_FIXTURE_KEY));
        assert!(!error.to_string().contains("raw-body-marker"));
        request_receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("authentication fixture received the request");
    }

    #[test]
    fn openai_discovery_rejects_invalid_json_and_empty_or_malformed_envelopes() {
        for body in [
            "not JSON",
            r#"{"data":[]}"#,
            r#"{"data":[{"id":"good"},{"id":null}]}"#,
            r#"{"error":{"message":"failed"},"data":[{"id":"good"}]}"#,
        ] {
            let (url, request_receiver) = spawn_http_response("200 OK", body);
            let settings = openai_catalog_fixture_settings(&url);
            assert!(matches!(
                provider_from_settings(&settings).list_models(),
                Err(InferenceError::InvalidResponse(_))
            ));
            request_receiver
                .recv_timeout(Duration::from_secs(3))
                .expect("catalog validation fixture received the request");
        }
    }

    #[test]
    fn openai_discovery_rejects_credentials_reflected_in_successful_model_ids() {
        let body = format!(r#"{{"data":[{{"id":"{}"}}]}}"#, OPENAI_CATALOG_FIXTURE_KEY);
        let (url, request_receiver) = spawn_http_response("200 OK", &body);
        let settings = openai_catalog_fixture_settings(&url);

        let error = provider_from_settings(&settings).list_models().unwrap_err();
        assert!(matches!(&error, InferenceError::InvalidResponse(_)));
        assert!(!format!("{error:?}").contains(OPENAI_CATALOG_FIXTURE_KEY));
        request_receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("credential-reflection fixture received the request");
    }

    #[test]
    fn openai_custom_provider_complete_and_stream_keep_legacy_parameters_on_the_wire() {
        for stream in [false, true] {
            let response = if stream {
                concat!(
                    "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n",
                    "data: [DONE]\n\n"
                )
            } else {
                r#"{"choices":[{"message":{"content":"ok"}}]}"#
            };
            let (url, request_receiver) = spawn_http_response("200 OK", response);
            let mut settings = openai_catalog_fixture_settings(&url);
            settings.openai.model = "gpt-5".to_string();

            let mut request = InferenceRequest::json_only("fixture");
            request.max_tokens = 731;
            let provider = provider_from_settings(&settings);
            if stream {
                let mut text = String::new();
                provider
                    .stream(&request, &mut |event| {
                        if let InferenceStreamEvent::TextDelta(delta) = event {
                            text.push_str(&delta);
                        }
                        true
                    })
                    .unwrap();
                assert_eq!(text, "ok");
            } else {
                assert_eq!(provider.complete(&request).unwrap().text, "ok");
            }

            let captured = request_receiver
                .recv_timeout(Duration::from_secs(3))
                .expect("custom completion request was captured");
            assert!(captured.starts_with("POST /gateway/v1/chat/completions HTTP/1.1\r\n"));
            let (_, body) = captured
                .split_once("\r\n\r\n")
                .expect("captured request has HTTP headers");
            let body: serde_json::Value = serde_json::from_str(body).unwrap();
            assert_eq!(body["max_tokens"], 731);
            assert_eq!(body["temperature"], serde_json::json!(0.0));
            assert_eq!(body["stream"], stream);
            assert!(body.get("max_completion_tokens").is_none());
        }
    }

    #[test]
    fn prompt_instructions_default_and_persist_all_six_fields() {
        let old: InferenceSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(old.instructions, PromptInstructions::default());
        let mut settings = old;
        settings.instructions = PromptInstructions {
            entry_naming: "entry".into(),
            organization: "organization".into(),
            naming_and_organization: "shared".into(),
            project_naming: "project".into(),
            smart_copy: "copy".into(),
            ask_for_update: "update".into(),
        };
        let restored: InferenceSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(restored.instructions, settings.instructions);
        let partial: PromptInstructions =
            serde_json::from_str(r#"{"entry_naming":"one"}"#).unwrap();
        assert_eq!(partial.organization, "");
        assert_eq!(partial.entry_naming, "one");
    }

    #[test]
    fn title_style_defaults_to_labeling_and_round_trips_summarization() {
        let bare_config: InferenceSettings =
            serde_json::from_str(r#"{"selected_provider":"ollama"}"#).unwrap();
        assert_eq!(bare_config.title_style, TitleStyle::Labeling);
        assert_eq!(
            InferenceSettings::default().title_style,
            TitleStyle::Labeling
        );

        let mut summarized = bare_config;
        summarized.title_style = TitleStyle::Summarization;
        let encoded = serde_json::to_string(&summarized).unwrap();
        assert!(encoded.contains("\"title_style\":\"summarization\""));
        let restored: InferenceSettings = serde_json::from_str(&encoded).unwrap();
        assert_eq!(restored.title_style, TitleStyle::Summarization);
    }

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
        assert_eq!(
            settings.kilo_gateway.model,
            DEFAULT_KILO_GATEWAY_SELECTED_MODEL
        );
        assert_eq!(settings.openrouter.model, DEFAULT_OPENROUTER_MODEL);
        assert_eq!(
            InferenceRequest::json_only("{}").max_tokens,
            UNKNOWN_MODEL_MAX_OUTPUT_TOKENS
        );
        // Hidden power-user flag: safe/off by default, never exposed by the
        // settings UI, only reachable by hand-editing config.toml.
        assert!(!settings.kilo_gateway.paid_proxies_enabled);
        assert!(settings.kilo_gateway.paid_proxies.is_empty());
    }

    #[test]
    fn openai_stream_emits_text_and_exact_usage() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"first\\n\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"second\"}}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"completion_tokens\":17}}\n\n",
            "data: [DONE]\n\n"
        );
        let (url, request_receiver) = spawn_http_response("200 OK", body);
        let mut events = Vec::new();

        post_stream(
            &url,
            &[],
            serde_json::json!({"stream": true}),
            None,
            StreamProtocol::OpenAiSse,
            &mut |event| {
                events.push(event);
                true
            },
        )
        .expect("parse OpenAI stream");
        request_receiver.recv().expect("captured request");

        assert_eq!(
            events,
            vec![
                InferenceStreamEvent::TextDelta("first\n".to_string()),
                InferenceStreamEvent::TextDelta("second".to_string()),
                InferenceStreamEvent::OutputTokens(17),
            ]
        );
    }

    #[test]
    fn anthropic_stream_emits_incremental_text_and_usage() {
        let body = concat!(
            "event: content_block_delta\n",
            "data: {\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n",
            "event: message_delta\n",
            "data: {\"usage\":{\"output_tokens\":9}}\n\n"
        );
        let (url, request_receiver) = spawn_http_response("200 OK", body);
        let mut events = Vec::new();

        post_stream(
            &url,
            &[],
            serde_json::json!({"stream": true}),
            None,
            StreamProtocol::AnthropicSse,
            &mut |event| {
                events.push(event);
                true
            },
        )
        .expect("parse Anthropic stream");
        request_receiver.recv().expect("captured request");

        assert_eq!(
            events,
            vec![
                InferenceStreamEvent::TextDelta("hello".to_string()),
                InferenceStreamEvent::OutputTokens(9),
            ]
        );
    }

    #[test]
    fn ollama_stream_emits_text_and_final_usage() {
        let body = concat!(
            "{\"message\":{\"content\":\"alpha\"},\"done\":false}\n",
            "{\"message\":{\"content\":\" beta\"},\"done\":true,\"eval_count\":6}\n"
        );
        let (url, request_receiver) = spawn_http_response("200 OK", body);
        let mut events = Vec::new();

        post_stream(
            &url,
            &[],
            serde_json::json!({"stream": true}),
            None,
            StreamProtocol::OllamaJsonLines,
            &mut |event| {
                events.push(event);
                true
            },
        )
        .expect("parse Ollama stream");
        request_receiver.recv().expect("captured request");

        assert_eq!(
            events,
            vec![
                InferenceStreamEvent::TextDelta("alpha".to_string()),
                InferenceStreamEvent::OutputTokens(6),
                InferenceStreamEvent::TextDelta(" beta".to_string()),
            ]
        );
    }

    #[test]
    fn kilo_gateway_client_ignores_paid_proxies_when_disabled() {
        let settings = KiloGatewaySettings {
            paid_proxies_enabled: false,
            paid_proxies: vec![ilium_kilo_gateway::PaidProxy {
                ip: "127.0.0.1".to_string(),
                port: 1,
                protocol: "http".to_string(),
                username: String::new(),
                password: String::new(),
            }],
            ..KiloGatewaySettings::default()
        };
        // No assertion beyond "does not panic building the client" -- the
        // real behavioral guarantee (proxy actually used) is covered by
        // `kilo_gateway_provider_routes_through_a_configured_paid_proxy`
        // below, since `proxy_url` is private to `KiloGatewayClient`.
        let _client = kilo_gateway_client(&settings, None);
    }

    #[test]
    fn kilo_gateway_proxy_egress_fails_closed_without_boot_loaded_rows() {
        let settings = KiloGatewaySettings {
            paid_proxies_enabled: true,
            ..KiloGatewaySettings::default()
        };

        assert!(matches!(
            kilo_gateway_client(&settings, None),
            Err(InferenceError::Configuration(message))
                if message.contains("no proxies were loaded from MongoDB")
        ));
    }

    #[test]
    fn kilo_gateway_provider_routes_through_a_configured_paid_proxy() {
        let settings = KiloGatewaySettings {
            model: DEFAULT_KILO_GATEWAY_MODEL.to_string(),
            paid_proxies_enabled: true,
            paid_proxies: vec![ilium_kilo_gateway::PaidProxy {
                // Nothing listens here; a completion attempt must fail
                // trying to reach this proxy rather than silently calling
                // Kilo Gateway directly over the real network.
                ip: "127.0.0.1".to_string(),
                port: 1,
                protocol: "http".to_string(),
                username: String::new(),
                password: String::new(),
            }],
            ..KiloGatewaySettings::default()
        };
        let provider = KiloGatewayProvider(Arc::new(settings));

        let result = provider.complete(&InferenceRequest::json_only("hello"));

        assert!(result.is_err());
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
    fn gateway_errors_keep_their_provider_neutral_category() {
        assert!(matches!(
            map_gateway_error(GatewayError::Http {
                status: 429,
                message: "rate limited".to_string(),
            }),
            InferenceError::Http { status: 429, .. }
        ));
        assert!(matches!(
            map_gateway_error(GatewayError::InvalidResponse("content null".to_string())),
            InferenceError::InvalidResponse(message) if message == "content null"
        ));
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
            None,
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

    #[test]
    fn anthropic_response_concatenates_every_text_block() {
        let response = serde_json::json!({
            "content": [
                {"type": "text", "text": "first"},
                {"type": "tool_use", "id": "tool-1"},
                {"type": "text", "text": "second"}
            ],
            "stop_reason": "end_turn"
        });

        assert_eq!(
            anthropic_response_text(&response).unwrap().text,
            "first\nsecond"
        );
    }

    #[test]
    fn openai_compatible_response_rejects_success_status_error_envelopes() {
        let response = serde_json::json!({"error": {"message": "upstream failed"}});

        assert!(matches!(
            openai_compatible_response_text(&response),
            Err(InferenceError::InvalidResponse(message)) if message.contains("error envelope")
        ));
    }

    #[test]
    fn openai_compatible_response_tolerates_a_null_error_field() {
        let response = serde_json::json!({
            "error": null,
            "choices": [{"message": {"content": "real answer"}}]
        });

        assert_eq!(
            openai_compatible_response_text(&response).unwrap().text,
            "real answer"
        );
    }

    #[test]
    fn anthropic_response_tolerates_a_null_error_field() {
        let response = serde_json::json!({
            "error": null,
            "content": [{"type": "text", "text": "real answer"}]
        });

        assert_eq!(
            anthropic_response_text(&response).unwrap().text,
            "real answer"
        );
    }

    #[test]
    fn api_key_provider_settings_default_has_no_provider_specific_base_url() {
        // The type is shared between the `openai` and `anthropic` fields of
        // `InferenceSettings`, which need different URLs; its `Default` must
        // stay neutral so a partially hand-edited config table doesn't fall
        // back to the wrong provider's endpoint (see `resolve_base_url`).
        assert_eq!(ApiKeyProviderSettings::default().base_url, "");
    }

    #[test]
    fn resolve_base_url_falls_back_only_when_blank() {
        assert_eq!(
            resolve_base_url("", DEFAULT_ANTHROPIC_URL),
            DEFAULT_ANTHROPIC_URL
        );
        assert_eq!(
            resolve_base_url("   ", DEFAULT_ANTHROPIC_URL),
            DEFAULT_ANTHROPIC_URL
        );
        assert_eq!(
            resolve_base_url("https://custom.example/v1", DEFAULT_ANTHROPIC_URL),
            "https://custom.example/v1"
        );
    }
}
