//! The [`InferenceBackend`] the server and the client compose for `ilium:host/inference`: the
//! user's selected provider of `ilium-inference`, configured from the `inference` kernel
//! settings tab (the `[inference]` table of `config.toml`, credentials included).
//!
//! The settings are read on every call, so a provider, model or key the user changes in Settings
//! applies to the next request without restarting a plugin. A completion is streamed from the
//! provider; the cancel flag of the instance is checked between chunks, so an instance that ends
//! drops the provider's response body mid-answer.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use ilium_inference::{
    provider_from_settings, InferenceError, InferenceProvider, InferenceRequest, InferenceSettings,
    InferenceStreamEvent, PaidProxy, UNKNOWN_MODEL_MAX_OUTPUT_TOKENS,
};
use ilium_plugin_api::generated::host::inference::{Completion, CompletionRequest};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::kernel::KernelSettings;

use super::env::CancelFlag;
use super::inference::InferenceBackend;

/// The kernel settings tab that holds the provider choice and credentials.
pub const INFERENCE_TAB: &str = "inference";
/// The longest a plugin completion may run before the provider transport gives up.
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(300);

/// Turns a provider failure into the plugin error a request completes with.
fn plugin_error(failure: InferenceError) -> PluginError {
    let code = match &failure {
        InferenceError::Configuration(_) => ErrorCode::Unsupported,
        InferenceError::Http { status, .. } => match status {
            401 | 403 => ErrorCode::NotPermitted,
            404 => ErrorCode::NotFound,
            408 | 504 => ErrorCode::TimedOut,
            429 => ErrorCode::RateLimited,
            _ => ErrorCode::Network,
        },
        InferenceError::Transport(_) => ErrorCode::Network,
        InferenceError::InvalidResponse(_) => ErrorCode::Internal,
    };
    PluginError::new(code, failure.to_string())
}

/// The provider request of a plugin request: system messages become the system prompt, the
/// other turns the user prompt (one turn verbatim, several as `role: content` blocks), and a
/// JSON schema is added to the system prompt as the shape the answer must have.
pub fn provider_request(request: &CompletionRequest) -> InferenceRequest {
    let mut system = Vec::new();
    let mut turns = Vec::new();
    for message in &request.messages {
        if message.role.eq_ignore_ascii_case("system") {
            system.push(message.content.as_str());
        } else {
            turns.push(message);
        }
    }
    let mut system_prompt = system.join("\n\n");
    if let Some(schema) = &request.json_schema {
        if !system_prompt.is_empty() {
            system_prompt.push_str("\n\n");
        }
        system_prompt.push_str(
            "Answer with one JSON value only, no prose and no code fence. It must match this JSON schema:\n",
        );
        system_prompt.push_str(schema);
    }
    let user_prompt = match turns.as_slice() {
        [only] => only.content.clone(),
        many => many
            .iter()
            .map(|message| format!("{}: {}", message.role, message.content))
            .collect::<Vec<_>>()
            .join("\n\n"),
    };
    InferenceRequest {
        system_prompt,
        user_prompt,
        max_tokens: request
            .max_output_tokens
            .unwrap_or(UNKNOWN_MODEL_MAX_OUTPUT_TOKENS),
        timeout: Some(COMPLETION_TIMEOUT),
    }
}

/// An estimate of the input tokens of a request (four characters per token, the rule the
/// restructure budget uses); providers report output tokens only through the stream.
fn estimated_input_tokens(request: &InferenceRequest) -> u32 {
    let characters = request.system_prompt.chars().count() + request.user_prompt.chars().count();
    u32::try_from(characters.div_ceil(4)).unwrap_or(u32::MAX)
}

/// Builds the provider of a settings value; tests swap in a fake.
pub type ProviderFactory = dyn Fn(&InferenceSettings) -> Box<dyn InferenceProvider> + Send + Sync;

/// The selected provider of the user's inference settings.
pub struct SettingsInference {
    settings: Arc<dyn KernelSettings>,
    paid_proxies: Mutex<Vec<PaidProxy>>,
    factory: Box<ProviderFactory>,
}

impl std::fmt::Debug for SettingsInference {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SettingsInference")
            .finish_non_exhaustive()
    }
}

impl SettingsInference {
    /// Reads the provider choice and credentials from `settings` on every call.
    pub fn new(settings: Arc<dyn KernelSettings>) -> Self {
        Self::with_factory(settings, Box::new(provider_from_settings))
    }

    /// Reads the `[inference]` table of the user's `config.toml` on every call.
    pub fn of_user() -> Self {
        Self::new(Arc::new(
            crate::config_settings::ConfigFileSettings::of_user(),
        ))
    }

    /// The same, building providers with `factory`.
    pub fn with_factory(settings: Arc<dyn KernelSettings>, factory: Box<ProviderFactory>) -> Self {
        Self {
            settings,
            paid_proxies: Mutex::new(Vec::new()),
            factory,
        }
    }

    /// The paid proxies Kilo Gateway calls go through when `paid_proxies_enabled` is set. They
    /// are runtime-only records (never in `config.toml`), so the process hands them over after it
    /// loaded them.
    pub fn set_paid_proxies(&self, proxies: Vec<PaidProxy>) {
        *self
            .paid_proxies
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = proxies;
    }

    /// The current settings; a table that does not parse is a configuration error, not the
    /// defaults, so a typo never sends a prompt to a provider the user did not choose.
    pub fn current(&self) -> Result<InferenceSettings, PluginError> {
        let text = self
            .settings
            .get(INFERENCE_TAB)
            .unwrap_or_else(|| "{}".to_owned());
        let mut settings: InferenceSettings = serde_json::from_str(&text).map_err(|failure| {
            PluginError::new(
                ErrorCode::Unsupported,
                format!("the inference settings do not parse: {failure}"),
            )
        })?;
        settings.kilo_gateway.paid_proxies = self
            .paid_proxies
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        Ok(settings)
    }
}

impl InferenceBackend for SettingsInference {
    fn complete(
        &self,
        request: &CompletionRequest,
        cancel: &CancelFlag,
    ) -> Result<Completion, PluginError> {
        let settings = self.current()?;
        let provider = (self.factory)(&settings);
        let provider_request = provider_request(request);
        let mut text = String::new();
        let mut output_tokens: Option<u64> = None;
        let streamed = provider.stream(&provider_request, &mut |event| {
            match event {
                InferenceStreamEvent::TextDelta(delta) => text.push_str(&delta),
                InferenceStreamEvent::OutputTokens(count) => output_tokens = Some(count),
            }
            !cancel.is_set()
        });
        if cancel.is_set() {
            return Err(PluginError::new(
                ErrorCode::Cancelled,
                "the instance ended during the completion",
            ));
        }
        streamed.map_err(plugin_error)?;
        let model = provider
            .selected_model()
            .map(str::to_owned)
            .unwrap_or_else(|| settings.selected_model().to_owned());
        let output_tokens = output_tokens
            .unwrap_or_else(|| text.chars().count().div_ceil(4) as u64)
            .min(u64::from(u32::MAX)) as u32;
        Ok(Completion {
            input_tokens: estimated_input_tokens(&provider_request),
            output_tokens,
            text,
            model,
        })
    }

    fn models(&self, cancel: &CancelFlag) -> Result<Vec<String>, PluginError> {
        let settings = self.current()?;
        let models = (self.factory)(&settings)
            .list_models()
            .map_err(plugin_error)?;
        if cancel.is_set() {
            return Err(PluginError::new(ErrorCode::Cancelled, "the instance ended"));
        }
        Ok(models)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_inference::{InferenceProviderKind, InferenceResponse};
    use ilium_plugin_api::generated::host::inference::ChatMessage;
    use ilium_plugin_api::kernel::FacadeError;

    /// Kernel settings holding one `inference` table.
    struct Tables(String);

    impl KernelSettings for Tables {
        fn get(&self, key: &str) -> Option<String> {
            (key == INFERENCE_TAB).then(|| self.0.clone())
        }
        fn set(&self, _key: &str, _value_json: &str) -> Result<(), FacadeError> {
            Ok(())
        }
        fn revision(&self) -> u64 {
            0
        }
    }

    /// Streams its reply in three pieces and records the request and settings it saw.
    struct Scripted {
        kind: InferenceProviderKind,
        model: String,
        seen: Arc<Mutex<Vec<InferenceRequest>>>,
    }

    impl InferenceProvider for Scripted {
        fn kind(&self) -> InferenceProviderKind {
            self.kind
        }
        fn selected_model(&self) -> Option<&str> {
            Some(&self.model)
        }
        fn complete(
            &self,
            _request: &InferenceRequest,
        ) -> Result<InferenceResponse, InferenceError> {
            Err(InferenceError::Transport("complete is not used".to_owned()))
        }
        fn stream(
            &self,
            request: &InferenceRequest,
            on_event: &mut dyn FnMut(InferenceStreamEvent) -> bool,
        ) -> Result<(), InferenceError> {
            self.seen.lock().expect("seen").push(request.clone());
            for piece in ["one ", "two ", "three"] {
                if !on_event(InferenceStreamEvent::TextDelta(piece.to_owned())) {
                    return Ok(());
                }
            }
            on_event(InferenceStreamEvent::OutputTokens(7));
            Ok(())
        }
        fn list_models(&self) -> Result<Vec<String>, InferenceError> {
            Ok(vec![self.model.clone()])
        }
    }

    fn backend(table: &str) -> (SettingsInference, Arc<Mutex<Vec<InferenceRequest>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&seen);
        let backend = SettingsInference::with_factory(
            Arc::new(Tables(table.to_owned())),
            Box::new(move |settings: &InferenceSettings| {
                Box::new(Scripted {
                    kind: settings.selected_provider,
                    model: settings.selected_model().to_owned(),
                    seen: Arc::clone(&recorder),
                }) as Box<dyn InferenceProvider>
            }),
        );
        (backend, seen)
    }

    fn request(messages: &[(&str, &str)], schema: Option<&str>) -> CompletionRequest {
        CompletionRequest {
            messages: messages
                .iter()
                .map(|(role, content)| ChatMessage {
                    role: (*role).to_owned(),
                    content: (*content).to_owned(),
                })
                .collect(),
            max_output_tokens: Some(64),
            json_schema: schema.map(str::to_owned),
            purpose: "test".to_owned(),
        }
    }

    #[test]
    fn the_selected_provider_and_model_come_from_the_kernel_settings_tab() {
        let (backend, seen) = backend(
            r#"{"selected_provider":"ollama","ollama":{"base_url":"http://127.0.0.1:1","model":"llama-test"}}"#,
        );
        let completion = backend
            .complete(&request(&[("user", "hi")], None), &CancelFlag::default())
            .expect("completion");
        assert_eq!(completion.text, "one two three");
        assert_eq!(completion.model, "llama-test");
        assert_eq!(completion.output_tokens, 7);
        assert_eq!(seen.lock().expect("seen")[0].max_tokens, 64);
        assert_eq!(
            backend.current().expect("settings").selected_provider,
            InferenceProviderKind::Ollama
        );
    }

    #[test]
    fn a_cancelled_instance_stops_the_stream() {
        let (backend, _) = backend("{}");
        let cancel = CancelFlag::default();
        cancel.set();
        let failure = backend
            .complete(&request(&[("user", "hi")], None), &cancel)
            .expect_err("cancelled");
        assert_eq!(failure.code, ErrorCode::Cancelled);
    }

    #[test]
    fn unparsable_settings_refuse_instead_of_falling_back_to_defaults() {
        let (backend, seen) = backend(r#"{"selected_provider":"no-such-provider"}"#);
        let failure = backend
            .complete(&request(&[("user", "hi")], None), &CancelFlag::default())
            .expect_err("refused");
        assert_eq!(failure.code, ErrorCode::Unsupported);
        assert!(seen.lock().expect("seen").is_empty());
    }

    #[test]
    fn system_turns_and_the_schema_form_the_system_prompt() {
        let built = provider_request(&request(
            &[("system", "be brief"), ("user", "a"), ("assistant", "b")],
            Some(r#"{"type":"object"}"#),
        ));
        assert!(built.system_prompt.starts_with("be brief"));
        assert!(built.system_prompt.ends_with(r#"{"type":"object"}"#));
        assert_eq!(built.user_prompt, "user: a\n\nassistant: b");
        let single = provider_request(&request(&[("user", "only")], None));
        assert_eq!(single.user_prompt, "only");
        assert!(single.system_prompt.is_empty());
    }

    #[test]
    fn models_are_listed_by_the_selected_provider() {
        let (backend, _) =
            backend(r#"{"selected_provider":"open_ai","openai":{"model":"gpt-test"}}"#);
        assert_eq!(
            backend.models(&CancelFlag::default()).expect("models"),
            vec!["gpt-test".to_owned()]
        );
    }

    #[test]
    fn provider_failures_map_to_plugin_error_codes() {
        assert_eq!(
            plugin_error(InferenceError::Http {
                status: 429,
                message: String::new()
            })
            .code,
            ErrorCode::RateLimited
        );
        assert_eq!(
            plugin_error(InferenceError::Configuration("no key".to_owned())).code,
            ErrorCode::Unsupported
        );
    }
}
