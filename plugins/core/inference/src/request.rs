//! Provider request and response shapes: payload builders, model-catalogue endpoints and parsers, response
//! text extraction and the error type. The HTTP exchange is host work behind the `http` import; everything
//! here is a pure function of its arguments, moved from `ilium-inference`.

use thiserror::Error;

use crate::kilo::GatewayError;

use crate::redact::{is_diagnostic_response_header, redacted_header_value, redacted_url};
use crate::settings::{
    kilo_gateway_model_catalog_url, ApiKeyProviderSettings, InferenceProviderKind,
    InferenceSettings, DEFAULT_ANTHROPIC_URL, DEFAULT_OLLAMA_URL, DEFAULT_OPENAI_URL,
    DEFAULT_OPENROUTER_URL, UNKNOWN_MODEL_MAX_OUTPUT_TOKENS,
};

/// The system prompt of every JSON-only helper request (template `json-only.hbs`).
pub const JSON_ONLY_SYSTEM_PROMPT: &str = include_str!("../templates/json-only.hbs");

#[derive(Debug, Clone)]
pub struct InferenceRequest {
    pub system_prompt: String,
    pub user_prompt: String,
    pub max_tokens: u32,
    /// Whole-request transport timeout. `None` keeps the short default meant
    /// for titles and JSON helpers; long summarization calls set it higher.
    pub timeout_millis: Option<u64>,
}
impl InferenceRequest {
    pub fn json_only(user_prompt: impl Into<String>) -> Self {
        Self {
            system_prompt: JSON_ONLY_SYSTEM_PROMPT.to_string(),
            user_prompt: user_prompt.into(),
            max_tokens: UNKNOWN_MODEL_MAX_OUTPUT_TOKENS,
            timeout_millis: None,
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

pub fn is_official_openai_base(base_url: &str) -> bool {
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
pub fn official_openai_max_output_tokens(model: &str) -> Option<u32> {
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
pub fn openai_chat_payload(
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
pub fn openai_model_catalog_url(base_url: &str) -> Result<url::Url, InferenceError> {
    api_model_catalog_url(base_url, DEFAULT_OPENAI_URL, "/models", "OpenAI-compatible")
}

/// Anthropic's base URL is the host only; the catalog lives under `/v1`.
/// `limit=1000` is the documented maximum page size, so one request covers
/// the catalog without following `after_id` pagination.
pub fn anthropic_model_catalog_url(base_url: &str) -> Result<url::Url, InferenceError> {
    let mut url =
        api_model_catalog_url(base_url, DEFAULT_ANTHROPIC_URL, "/v1/models", "Anthropic")?;
    url.set_query(Some("limit=1000"));
    Ok(url)
}

pub fn api_model_catalog_url(
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
        InferenceProviderKind::Ollama => Some(redacted_url(&format_url(
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

pub fn redacted_catalog_display(
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
pub fn parse_openai_model_catalog(
    response: &serde_json::Value,
) -> Result<Vec<String>, InferenceError> {
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
pub fn require(value: &str, message: &str) -> Result<(), InferenceError> {
    if value.trim().is_empty() {
        Err(InferenceError::Configuration(message.to_string()))
    } else {
        Ok(())
    }
}
pub fn format_url(base_url: &str, path: &str) -> String {
    format!("{}/{}", base_url.trim_end_matches('/'), path)
}
/// Falls back to `default_base_url` when settings carry a blank `base_url`
/// (the safe zero value for `ApiKeyProviderSettings`, since that type is
/// shared between providers with different correct endpoints and cannot
/// bake either one into its own `Default`).
pub fn resolve_base_url<'settings>(
    base_url: &'settings str,
    default_base_url: &'settings str,
) -> &'settings str {
    if base_url.trim().is_empty() {
        default_base_url
    } else {
        base_url
    }
}

pub fn redacted_headers(headers: &[(&str, String)]) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|(name, value)| ((*name).to_owned(), redacted_header_value(name, value)))
        .collect()
}

pub fn response_text(
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

pub fn openai_compatible_response_text(
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

/// Documented output-token maximum per Anthropic model. The Messages API
/// rejects any `max_tokens` above the model's cap with a 400, so the
/// fallback budget is never sent to Anthropic.
/// https://docs.anthropic.com/en/docs/about-claude/models/overview
pub fn anthropic_max_output_tokens(model: &str) -> u32 {
    match model {
        "claude-haiku-4-5" | "claude-haiku-4-5-20251001" => 64_000,
        _ => 128_000,
    }
}

/// Builds a Messages API request. `temperature` is deliberately absent:
/// claude-haiku-5-5 rejects it with a 400 error, so the provider default applies.
/// `max_tokens` is the model's documented maximum, mirroring the official OpenAI
/// path: the caller's fallback budget is not transmitted.
pub fn anthropic_messages_payload(
    model: &str,
    request: &InferenceRequest,
    stream: bool,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": model,
        "system": request.system_prompt,
        "max_tokens": anthropic_max_output_tokens(model),
        "messages": [{"role": "user", "content": request.user_prompt}]
    });
    if stream {
        body["stream"] = serde_json::Value::Bool(true);
    }
    body
}

pub fn anthropic_response_text(
    value: &serde_json::Value,
) -> Result<InferenceResponse, InferenceError> {
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

/// Maps a Kilo Gateway failure to the provider-neutral error category.
pub fn map_gateway_error(error: GatewayError) -> InferenceError {
    match error {
        GatewayError::Http { status, message } => InferenceError::Http { status, message },
        GatewayError::Transport(message) => InferenceError::Transport(message),
        GatewayError::InvalidResponse(message) => InferenceError::InvalidResponse(message),
        GatewayError::InvalidProxy(message) => {
            InferenceError::Configuration(format!("paid proxy configuration invalid: {message}"))
        }
    }
}

/// Stable category name of an error, for diagnostics.
pub fn inference_error_kind(error: &InferenceError) -> &'static str {
    match error {
        InferenceError::Configuration(_) => "configuration",
        InferenceError::Http { .. } => "http",
        InferenceError::Transport(_) => "transport",
        InferenceError::InvalidResponse(_) => "invalid_response",
    }
}

/// The HTTP status of an error, when it has one.
pub fn inference_http_status(error: &InferenceError) -> Option<u16> {
    match error {
        InferenceError::Http { status, .. } => Some(*status),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    const OPENAI_CATALOG_FIXTURE_KEY: &str = "sk-fixture-openai-catalog-secret";
    use super::*;

    /// REQ: PORT:ilium-inference/src/lib.rs::openai_official_policy_requires_the_parsed_exact_https_host
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

    /// REQ: PORT:ilium-inference/src/lib.rs::openai_official_complete_and_stream_payloads_omit_forbidden_parameters
    #[test]
    fn openai_official_complete_and_stream_payloads_omit_forbidden_parameters() {
        let request = InferenceRequest {
            system_prompt: "Return JSON only.".to_string(),
            user_prompt: "A quoted \"value\" and a newline:\nnext".to_string(),
            max_tokens: UNKNOWN_MODEL_MAX_OUTPUT_TOKENS,
            timeout_millis: None,
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

    /// REQ: PORT:ilium-inference/src/lib.rs::openai_known_models_use_the_full_documented_maximum_not_a_caller_cap
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

    /// REQ: PORT:ilium-inference/src/lib.rs::openai_unknown_limits_are_omitted_without_family_or_fine_tune_guessing
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

    /// REQ: PORT:ilium-inference/src/lib.rs::openai_custom_and_openrouter_payloads_preserve_the_original_json_contract
    #[test]
    fn openai_custom_and_openrouter_payloads_preserve_the_original_json_contract() {
        let request = InferenceRequest {
            system_prompt: "system fixture".to_string(),
            user_prompt: "user fixture".to_string(),
            max_tokens: 731,
            timeout_millis: None,
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

    /// REQ: PORT:ilium-inference/src/lib.rs::openai_catalog_parsing_sorts_deduplicates_and_does_not_guess_capabilities
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

    /// REQ: PORT:ilium-inference/src/lib.rs::openai_catalog_parsing_rejects_empty_malformed_and_partial_catalogs
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

    /// REQ: PORT:ilium-inference/src/lib.rs::openai_catalog_url_uses_the_effective_base_and_preserves_custom_prefixes
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

    /// REQ: PORT:ilium-inference/src/lib.rs::openai_catalog_endpoint_metadata_is_redacted_and_model_independent
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

    /// REQ: PORT:ilium-inference/src/lib.rs::anthropic_payloads_omit_temperature_for_both_paths
    #[test]
    fn anthropic_payloads_omit_temperature_for_both_paths() {
        let request = InferenceRequest {
            system_prompt: "Return JSON only.".to_string(),
            user_prompt: "user fixture".to_string(),
            max_tokens: 512,
            timeout_millis: None,
        };

        for stream in [false, true] {
            let body = anthropic_messages_payload("claude-haiku-5-5", &request, stream);
            assert!(body.get("temperature").is_none(), "stream={stream}");
            assert_eq!(body["model"], "claude-haiku-5-5");
            assert_eq!(body["system"], "Return JSON only.");
            assert_eq!(body["max_tokens"], 128_000);
            assert_eq!(body["messages"][0]["content"], "user fixture");
            assert_eq!(body.get("stream").is_some(), stream);
        }
    }

    /// REQ: PORT:ilium-inference/src/lib.rs::anthropic_max_tokens_never_exceeds_model_output_cap
    #[test]
    fn anthropic_max_tokens_never_exceeds_model_output_cap() {
        let request = InferenceRequest::json_only("user fixture");
        for model in ["claude-haiku-5-5", "claude-opus-5-5", "claude-sonnet-5-5"] {
            let body = anthropic_messages_payload(model, &request, false);
            assert_eq!(body["max_tokens"], 128_000, "{model}");
        }
        let body = anthropic_messages_payload("claude-haiku-4-5", &request, false);
        assert_eq!(body["max_tokens"], 64_000);
        assert!(body["max_tokens"].as_u64().unwrap() <= 128_000);
    }

    /// REQ: PORT:ilium-inference/src/lib.rs::anthropic_catalog_endpoint_is_v1_models_and_redacted
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

    /// REQ: PORT:ilium-inference/src/lib.rs::anthropic_catalog_parses_as_shared_model_list
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

    /// REQ: PORT:ilium-inference/src/lib.rs::gateway_errors_keep_their_provider_neutral_category
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

    /// REQ: PORT:ilium-inference/src/lib.rs::diagnostic_headers_redact_every_supported_api_key_header
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

    /// REQ: PORT:ilium-inference/src/lib.rs::anthropic_response_concatenates_every_text_block
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

    /// REQ: PORT:ilium-inference/src/lib.rs::openai_compatible_response_rejects_success_status_error_envelopes
    #[test]
    fn openai_compatible_response_rejects_success_status_error_envelopes() {
        let response = serde_json::json!({"error": {"message": "upstream failed"}});

        assert!(matches!(
            openai_compatible_response_text(&response),
            Err(InferenceError::InvalidResponse(message)) if message.contains("error envelope")
        ));
    }

    /// REQ: PORT:ilium-inference/src/lib.rs::openai_compatible_response_tolerates_a_null_error_field
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

    /// REQ: PORT:ilium-inference/src/lib.rs::anthropic_response_tolerates_a_null_error_field
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

    /// REQ: PORT:ilium-inference/src/lib.rs::resolve_base_url_falls_back_only_when_blank
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
