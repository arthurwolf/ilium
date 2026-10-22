//! Real provider adapter for the attention calls (Turbo and Shared refresh).
//!
//! Wraps the user's configured [`InferenceSettings`]. When the attention
//! settings name a model override, that model is tried first and the
//! configured model is the fallback, so a retired or misspelled override
//! degrades to the normal model instead of silencing the feature.

use ilium_inference::{
    provider_from_settings, InferenceError, InferenceProviderKind, InferenceRequest,
    InferenceSettings,
};

use super::worker::{AttentionCallError, AttentionCompletionClient};

pub struct ProviderAttentionClient {
    candidates: Vec<InferenceSettings>,
}

impl ProviderAttentionClient {
    /// `model_override` is the "Model for attention" setting; empty or absent
    /// means the configured model only.
    pub fn new(settings: &InferenceSettings, model_override: Option<&str>) -> Self {
        let mut candidates = Vec::with_capacity(2);
        if let Some(model) = model_override.map(str::trim).filter(|m| !m.is_empty()) {
            if model != settings.selected_model() {
                candidates.push(with_model(settings, model));
            }
        }
        candidates.push(settings.clone());
        Self { candidates }
    }
}

fn with_model(settings: &InferenceSettings, model: &str) -> InferenceSettings {
    let mut changed = settings.clone();
    let slot = match changed.selected_provider {
        InferenceProviderKind::KiloGateway => &mut changed.kilo_gateway.model,
        InferenceProviderKind::Ollama => &mut changed.ollama.model,
        InferenceProviderKind::OpenAi => &mut changed.openai.model,
        InferenceProviderKind::Anthropic => &mut changed.anthropic.model,
        InferenceProviderKind::OpenRouter => &mut changed.openrouter.model,
    };
    *slot = model.to_owned();
    changed
}

fn classify(error: &InferenceError) -> AttentionCallError {
    match error {
        InferenceError::Http {
            status: 429 | 503, ..
        } => AttentionCallError::Throttled(error.to_string()),
        other => AttentionCallError::classify(&other.to_string()),
    }
}

impl AttentionCompletionClient for ProviderAttentionClient {
    fn complete_attention_prompt(&self, prompt: &str) -> Result<String, AttentionCallError> {
        let mut last = AttentionCallError::Failed("no model configured".to_owned());
        for settings in &self.candidates {
            let request = InferenceRequest::json_only(prompt.to_owned());
            match provider_from_settings(settings).complete(&request) {
                Ok(response) => return Ok(response.text),
                // Throttling is about the gateway, not the model: let the
                // limiter back off instead of burning the fallback.
                Err(error) => match classify(&error) {
                    throttled @ AttentionCallError::Throttled(_) => return Err(throttled),
                    failed => last = failed,
                },
            }
        }
        Err(last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_is_tried_first_and_configured_model_is_the_fallback() {
        let settings = InferenceSettings::default();
        let client = ProviderAttentionClient::new(&settings, Some(" other/model:free "));
        assert_eq!(client.candidates.len(), 2);
        assert_eq!(client.candidates[0].selected_model(), "other/model:free");
        assert_eq!(
            client.candidates[1].selected_model(),
            settings.selected_model()
        );
    }

    #[test]
    fn blank_or_identical_override_adds_no_candidate() {
        let settings = InferenceSettings::default();
        assert_eq!(
            ProviderAttentionClient::new(&settings, Some("  "))
                .candidates
                .len(),
            1
        );
        let same = settings.selected_model().to_owned();
        assert_eq!(
            ProviderAttentionClient::new(&settings, Some(&same))
                .candidates
                .len(),
            1
        );
    }

    #[test]
    fn http_429_and_503_are_throttles_other_errors_are_failures() {
        let throttled = InferenceError::Http {
            status: 429,
            message: "x".into(),
        };
        assert!(matches!(
            classify(&throttled),
            AttentionCallError::Throttled(_)
        ));
        let failed = InferenceError::Http {
            status: 400,
            message: "bad".into(),
        };
        assert!(matches!(classify(&failed), AttentionCallError::Failed(_)));
    }
}
