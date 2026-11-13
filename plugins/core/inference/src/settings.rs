//! Inference settings: provider kinds, per-provider defaults, prompt instructions and the restructure budget.
//! Pure data with serde, moved from `ilium-inference` (the `[inference]` tables of `config.toml`).

use serde::{Deserialize, Serialize};

pub use crate::kilo::PaidProxy;
use crate::kilo::{
    DEFAULT_BASE_URL as DEFAULT_KILO_GATEWAY_URL,
    FALLBACK_FREE_MODELS as KILO_GATEWAY_FALLBACK_MODELS,
};

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
pub const DEFAULT_KILO_GATEWAY_SELECTED_MODEL: &str = "dots-studio/dots-3-note-preview:free";
pub const DEFAULT_PROXY_DATABASE_URI: &str = "mongodb://127.0.0.1:27017";
pub const DEFAULT_PROXY_DATABASE_NAME: &str = "money";
pub const DEFAULT_PROXY_COLLECTION_NAME: &str = "paid_proxies";
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
    /// When set, Codex panes take the official thread name Codex stores on disk
    /// instead of asking the model. User-written titles still win.
    pub codex_official_thread_names: bool,
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
            codex_official_thread_names: false,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::InferenceRequest;

    /// REQ: PORT:ilium-inference/src/lib.rs::restructure_budget_rejects_invalid_configuration
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

    /// REQ: PORT:ilium-inference/src/lib.rs::restructure_budget_defaults_and_preserves_custom_tokens
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

    /// REQ: PORT:ilium-inference/src/lib.rs::prompt_instructions_default_and_persist_all_six_fields
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

    /// REQ: PORT:ilium-inference/src/lib.rs::title_style_defaults_to_labeling_and_round_trips_summarization
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

    /// REQ: PORT:ilium-inference/src/lib.rs::defaults_are_safe
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

    /// REQ: PORT:ilium-inference/src/lib.rs::api_key_provider_settings_default_has_no_provider_specific_base_url
    #[test]
    fn api_key_provider_settings_default_has_no_provider_specific_base_url() {
        // The type is shared between the `openai` and `anthropic` fields of
        // `InferenceSettings`, which need different URLs; its `Default` must
        // stay neutral so a partially hand-edited config table doesn't fall
        // back to the wrong provider's endpoint (see `resolve_base_url`).
        assert_eq!(ApiKeyProviderSettings::default().base_url, "");
    }
}
