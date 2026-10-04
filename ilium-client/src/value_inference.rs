//! Full native onboarding provider/model choices preserve authored model names.
use crate::app::{App, Mode};
use crate::value_control::{ControlKind, ControlSpec, ValueControl};
use crate::value_dialog::{ChoiceDialogState, ChoiceOption, DialogOutcome};
use crate::value_dialog_host::{ValueDialogHost, ValueTarget};
use ilium_inference::InferenceProviderKind as Provider;
use ratatui::layout::Rect;
use std::sync::Arc;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnboardingInference {
    PaidProvider,
    KiloModel,
    OpenAiModel,
}
impl OnboardingInference {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::PaidProvider => "Provider",
            Self::KiloModel => "Kilo model",
            Self::OpenAiModel => "OpenAI model",
        }
    }
    pub(crate) fn available(self, app: &App) -> bool {
        app.onboarding.is_some()
            && app.onboarding_progress.wizard.step
                == crate::onboarding::state::Step::AiConfiguration
            && match self {
                Self::PaidProvider => {
                    app.onboarding_progress.wizard.ai
                        == Some(crate::onboarding::state::AiChoice::Paid)
                }
                Self::KiloModel => {
                    app.inference_settings.selected_provider == Provider::KiloGateway
                }
                Self::OpenAiModel => app.inference_settings.selected_provider == Provider::OpenAi,
            }
    }
    pub(crate) fn current(self, app: &App) -> String {
        match self {
            Self::PaidProvider => app.inference_settings.selected_provider.label().into(),
            Self::KiloModel => app.inference_settings.kilo_gateway.model.clone(),
            Self::OpenAiModel => app.inference_settings.openai.model.clone(),
        }
    }
    pub(crate) fn options(self, app: &App) -> Vec<ChoiceOption> {
        if self == Self::PaidProvider {
            let mut options: Vec<_> = [Provider::OpenAi, Provider::Anthropic, Provider::OpenRouter]
                .into_iter()
                .map(|provider| ChoiceOption {
                    id: format!("{provider:?}"),
                    label: provider.label().into(),
                    disabled_reason: None,
                })
                .collect();
            let current = app.inference_settings.selected_provider;
            if ![Provider::OpenAi, Provider::Anthropic, Provider::OpenRouter].contains(&current) {
                options.insert(0,ChoiceOption {id:format!("{current:?}"),label:format!("{} (current)",current.label()),disabled_reason:Some("This provider belongs to another setup choice; go Back or choose a paid provider".into())});
            }
            return options;
        }
        let mut models = match self {
            Self::KiloModel if app.kilo_gateway_models.is_empty() => {
                ilium_inference::kilo_gateway_fallback_models()
            }
            Self::KiloModel => app.kilo_gateway_models.clone(),
            Self::OpenAiModel => app.openai_models.clone(),
            Self::PaidProvider => Vec::new(),
        };
        let current = self.current(app);
        if !current.is_empty() && !models.contains(&current) {
            models.insert(0, current);
        }
        models.retain(|model| !model.trim().is_empty());
        let mut unique = std::collections::HashSet::new();
        models.retain(|model| unique.insert(model.clone()));
        if models.is_empty() {
            return vec![ChoiceOption {
                id: "unavailable".into(),
                label: "No model configured".into(),
                disabled_reason: Some("Edit the model name or refresh available models".into()),
            }];
        }
        models
            .into_iter()
            .map(|model| ChoiceOption {
                id: model.clone(),
                label: model,
                disabled_reason: None,
            })
            .collect()
    }
    fn selected_id(self, app: &App) -> String {
        if self == Self::PaidProvider {
            format!("{:?}", app.inference_settings.selected_provider)
        } else {
            self.current(app)
        }
    }
    fn desired(self, app: &App, id: &str) -> Result<ilium_inference::InferenceSettings, String> {
        let options = self.options(app);
        if !options
            .iter()
            .any(|option| option.id == id && option.disabled_reason.is_none())
        {
            return Err("This provider or model is unavailable; reopen its choices".into());
        }
        let mut desired = app.inference_settings.clone();
        match self {
            Self::PaidProvider => {
                desired.selected_provider =
                    [Provider::OpenAi, Provider::Anthropic, Provider::OpenRouter]
                        .into_iter()
                        .find(|provider| format!("{provider:?}") == id)
                        .ok_or("This paid provider is unavailable")?
            }
            Self::KiloModel => desired.kilo_gateway.model = id.into(),
            Self::OpenAiModel => desired.openai.model = id.into(),
        };
        Ok(desired)
    }
    pub(crate) fn control(
        self,
        area: Rect,
        scroll: u16,
        index: usize,
        app: &App,
    ) -> Option<ValueControl> {
        let offset = (u16::try_from(index).ok()?.checked_mul(2)?).checked_sub(scroll)?;
        if offset >= area.height {
            return None;
        }
        let current = self.current(app);
        // Painting needs only the current value and whether a different native choice exists.
        // Materialize the complete catalog only for an explicit catalog/step action.
        let can_step = match self {
            Self::PaidProvider => true,
            Self::KiloModel => {
                app.kilo_gateway_models.is_empty()
                    || app
                        .kilo_gateway_models
                        .iter()
                        .any(|model| !model.trim().is_empty() && model != &current)
            }
            Self::OpenAiModel => app
                .openai_models
                .iter()
                .any(|model| !model.trim().is_empty() && model != &current),
        };
        Some(ValueControl::new(
            Rect::new(
                area.x.saturating_add(area.width.min(2)),
                area.y + offset,
                area.width.saturating_sub(2),
                1,
            ),
            ControlSpec {
                kind: ControlKind::Choice,
                label: self.label(),
                value: &current,
                label_width: 22.min(area.width.saturating_sub(16)),
                previous_enabled: can_step,
                next_enabled: can_step,
                open_enabled: true,
            },
        ))
    }
}
impl App {
    /// Admit one persisted snapshot before changing local inference state. The
    /// same receipt settles both durability and an optional exact dialog token.
    pub(crate) fn enqueue_inference_value(
        &mut self,
        directory: std::path::PathBuf,
        desired: &ilium_inference::InferenceSettings,
        token: Option<Arc<()>>,
    ) -> Result<(), String> {
        use crate::filesystem::configurations::{ConfigurationIntent, InferenceSaveState};
        let change = crate::filesystem::configuration::inference_snapshot(desired)?;
        let operation = Arc::new(());
        let intent = match token {
            Some(token) => ConfigurationIntent::InferenceValueDialog {
                operation: operation.clone(),
                token,
            },
            None => ConfigurationIntent::Inference {
                operation: operation.clone(),
            },
        };
        self.enqueue_configuration(directory, change, intent)?;
        self.inference_save_state = InferenceSaveState::Pending {
            operation,
            budget_dialog: None,
        };
        self.inference_settings_save_error = None;
        Ok(())
    }
    pub(crate) fn step_onboarding_inference(&mut self, field: OnboardingInference, previous: bool) {
        let result = (|| {
            if !field.available(self) {
                return Err("This AI setup control changed".to_string());
            }
            let options: Vec<_> = field
                .options(self)
                .into_iter()
                .filter(|option| option.disabled_reason.is_none())
                .collect();
            if options.len() < 2 {
                return Ok(());
            }
            let index = options
                .iter()
                .position(|option| option.id == field.selected_id(self))
                .unwrap_or(0);
            let next = if previous {
                (index + options.len() - 1) % options.len()
            } else {
                (index + 1) % options.len()
            };
            let desired = field.desired(self, &options[next].id)?;
            let directory = self
                .config_dir
                .clone()
                .filter(|path| path.is_absolute())
                .ok_or("The AI configuration directory is unavailable")?;
            self.enqueue_inference_value(directory, &desired, None)?;
            self.apply_inference_settings_locally(desired);
            Ok(())
        })();
        if let Err(error) = result {
            self.status_message = Some(error);
        }
    }
    pub(crate) fn begin_onboarding_inference_dialog(&mut self, field: OnboardingInference) {
        let result = (|| {
            if !field.available(self)
                || !matches!(self.mode, Mode::Normal | Mode::Settings(_))
                || !self.modal_stack.is_empty()
            {
                return Err("This AI setup control changed".to_string());
            }
            let directory = self
                .config_dir
                .clone()
                .filter(|path| path.is_absolute())
                .ok_or("The AI configuration directory is unavailable")?;
            let identity = self
                .onboarding
                .as_ref()
                .ok_or("AI setup closed")?
                .identity
                .clone();
            let dialog = ChoiceDialogState::new(
                field.label(),
                field.options(self),
                Some(field.selected_id(self)),
            )?;
            Ok(ValueDialogHost::choice_host(
                ValueTarget::OnboardingInference {
                    field,
                    identity,
                    revision: self.onboarding_revision,
                    directory,
                },
                dialog,
            ))
        })();
        match result {
            Ok(host) => self.push_modal(Mode::ValueDialog(Box::new(host))),
            Err(error) => self.status_message = Some(error),
        }
    }
    pub(crate) fn commit_onboarding_inference_dialog(
        &mut self,
        host: &mut ValueDialogHost,
        outcome: &DialogOutcome,
    ) -> Result<(), String> {
        let ValueTarget::OnboardingInference {
            field,
            identity,
            revision,
            directory,
        } = &host.target
        else {
            return Err("This is not an AI setup choice".into());
        };
        if host.is_saving()
            || self.config_dir.as_ref() != Some(directory)
            || *revision != self.onboarding_revision
            || !field.available(self)
            || !matches!(
                self.modal_stack.last(),
                Some(Mode::Normal | Mode::Settings(_))
            )
            || !self
                .onboarding
                .as_ref()
                .is_some_and(|ui| Arc::ptr_eq(identity, &ui.identity))
        {
            return Err("AI setup changed; reopen its choices".into());
        }
        let DialogOutcome::Choose(id) = outcome else {
            return Err("Choose a provider or model".into());
        };
        let desired = field.desired(self, id)?;
        let token = Arc::new(());
        self.enqueue_inference_value(directory.clone(), &desired, Some(token.clone()))?;
        self.apply_inference_settings_locally(desired);
        if let ValueTarget::OnboardingInference { revision, .. } = &mut host.target {
            *revision = self.onboarding_revision;
        }
        host.begin_save(token);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onboarding::{
        screen::WizardUi,
        state::{AiChoice, Step},
    };
    use crate::value_dialog::ValueDialogState;
    fn synthetic_app(directory: &std::path::Path) -> App {
        let mut app = App::new("synthetic-ai-choice".into(), directory.into());
        app.config_dir = Some(directory.into());
        app.onboarding_progress.begin();
        app.onboarding_progress.wizard.step = Step::AiConfiguration;
        app.onboarding_progress.wizard.ai = Some(AiChoice::Paid);
        app.onboarding = Some(WizardUi::default());
        app.inference_settings.selected_provider = Provider::OpenAi;
        app.inference_settings.openai.model = "authored/custom model".into();
        app.openai_models = (0..80)
            .map(|index| format!("synthetic-model-{index}"))
            .collect();
        app
    }
    #[test]
    fn full_model_catalog_keeps_authored_value_and_all_native_paid_providers() {
        let directory = tempfile::tempdir().unwrap();
        let app = synthetic_app(directory.path());
        let options = OnboardingInference::OpenAiModel.options(&app);
        assert_eq!(options.len(), 81);
        assert_eq!(options[0].id, "authored/custom model");
        for model in &app.openai_models {
            assert!(options.iter().any(|option| &option.id == model));
        }
        assert_eq!(OnboardingInference::PaidProvider.options(&app).len(), 3);
        let mut empty = synthetic_app(directory.path());
        empty.openai_models.clear();
        empty.inference_settings.openai.model.clear();
        let options = OnboardingInference::OpenAiModel.options(&empty);
        assert_eq!(options.len(), 1);
        assert!(options[0].disabled_reason.is_some());
    }
    #[test]
    fn failed_model_write_keeps_catalog_and_retries_matching_revision_without_touching_credentials()
    {
        let directory = tempfile::tempdir().unwrap();
        let mut app = synthetic_app(directory.path());
        let credential = app.inference_settings.openai.api_key.clone();
        std::fs::write(directory.path().join("config.toml"), "[inference\n").unwrap();
        app.begin_onboarding_inference_dialog(OnboardingInference::OpenAiModel);
        let Mode::ValueDialog(mut host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("catalog")
        };
        let ValueDialogState::Choice(choice) = &mut host.dialog else {
            panic!("choice")
        };
        choice.selected_id = Some("synthetic-model-79".into());
        app.finish_value_dialog(host, DialogOutcome::Choose("synthetic-model-79".into()));
        app.settle_filesystem_for_test();
        let Mode::ValueDialog(host) = &app.mode else {
            panic!("failed save retains catalog")
        };
        assert!(!host.is_saving());
        let ValueDialogState::Choice(choice) = &host.dialog else {
            panic!("choice")
        };
        assert!(choice.notice.is_some());
        assert!(matches!(
            app.inference_save_state,
            crate::filesystem::configurations::InferenceSaveState::Unsaved
        ));
        assert_eq!(choice.selected_id.as_deref(), Some("synthetic-model-79"));
        assert_eq!(app.inference_settings.openai.api_key, credential);
        std::fs::write(directory.path().join("config.toml"), "").unwrap();
        let Mode::ValueDialog(host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("retry")
        };
        app.finish_value_dialog(host, DialogOutcome::Choose("synthetic-model-79".into()));
        app.settle_filesystem_for_test();
        assert!(matches!(app.mode, Mode::Normal));
        assert!(matches!(
            app.inference_save_state,
            crate::filesystem::configurations::InferenceSaveState::Durable
        ));
        assert!(app.onboarding.is_some());
        assert_eq!(
            crate::config::load(directory.path())
                .unwrap()
                .inference
                .openai
                .model,
            "synthetic-model-79"
        );
    }
    #[test]
    fn ordinary_inference_catalog_and_exact_budget_preserve_credentials_and_save_durably() {
        use crate::value_settings::SettingsNumber;
        use crate::value_settings_choice::SettingsChoice;
        let directory = tempfile::tempdir().unwrap();
        let mut app = synthetic_app(directory.path());
        app.inference_settings.openai.api_key = "synthetic-kept-credential".into();
        app.inference_settings.selected_provider = Provider::KiloGateway;
        app.inference_settings.kilo_gateway.model = "authored/custom-kilo".into();
        app.kilo_gateway_models = vec!["synthetic-kilo-a".into(), "synthetic-kilo-b".into()];
        let (options, selected) = SettingsChoice::KiloModel.options(&app);
        assert_eq!(options.len(), 3);
        assert_eq!(selected, "authored/custom-kilo");
        assert!(options.iter().any(|option| option.id == selected));
        let (providers, _) = SettingsChoice::InferenceProvider.options(&app);
        assert_eq!(providers.len(), Provider::ALL.len());
        app.save_settings_choice(
            SettingsChoice::KiloModel,
            "synthetic-kilo-b",
            directory.path().into(),
            None,
        )
        .unwrap();
        app.settle_filesystem_for_test();
        assert!(matches!(
            app.inference_save_state,
            crate::filesystem::configurations::InferenceSaveState::Durable
        ));
        let attempts = app.configuration_admission.attempts;
        assert!(app
            .save_settings_number(
                SettingsNumber::InferenceTokenBudget,
                "0",
                directory.path().into(),
                None
            )
            .is_err());
        assert!(app
            .save_settings_number(
                SettingsNumber::InferenceTokenBudget,
                "4294967296",
                directory.path().into(),
                None
            )
            .is_err());
        assert_eq!(app.configuration_admission.attempts, attempts);
        app.save_settings_number(
            SettingsNumber::InferenceTokenBudget,
            "123456",
            directory.path().into(),
            None,
        )
        .unwrap();
        app.settle_filesystem_for_test();
        let saved = crate::config::load(directory.path()).unwrap().inference;
        assert_eq!(saved.restructure_prompt_token_limit, 123456);
        assert_eq!(saved.kilo_gateway.model, "synthetic-kilo-b");
        assert_eq!(saved.openai.api_key, "synthetic-kept-credential");
        assert_eq!(
            SettingsNumber::InferenceTokenBudget
                .stepped(&app, -1)
                .unwrap(),
            "123455"
        );
        assert!(matches!(
            app.inference_save_state,
            crate::filesystem::configurations::InferenceSaveState::Durable
        ));
    }

    #[test]
    fn ordinary_inference_catalog_rejects_revision_changed_after_open() {
        use crate::value_settings_choice::SettingsChoice;
        let directory = tempfile::tempdir().unwrap();
        let mut app = synthetic_app(directory.path());
        app.onboarding = None;
        app.mode = Mode::Settings(crate::app::SettingsState {
            tab: crate::app::SettingsTab::Inference,
            selected_row: 0,
            ..crate::app::SettingsState::default()
        });
        app.begin_settings_choice_dialog(SettingsChoice::InferenceProvider);
        let Mode::ValueDialog(mut host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("catalog");
        };
        app.onboarding_revision = app.onboarding_revision.wrapping_add(1);
        let attempts = app.configuration_admission.attempts;
        assert!(app
            .commit_settings_choice_dialog(&mut host, &DialogOutcome::Choose("Ollama".into()))
            .is_err());
        assert_eq!(app.configuration_admission.attempts, attempts);
        assert_eq!(app.inference_settings.selected_provider, Provider::OpenAi);
    }

    #[test]
    fn rejected_inference_admission_preserves_previous_durability_and_settings() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = synthetic_app(directory.path());
        let original = app.inference_settings.clone();
        app.inference_save_state = crate::filesystem::configurations::InferenceSaveState::Durable;
        app.configuration_files = None;
        let mut desired = original.clone();
        desired.openai.model = "synthetic-rejected-model".into();
        assert!(app
            .enqueue_inference_value(directory.path().into(), &desired, None)
            .is_err());
        assert_eq!(app.inference_settings, original);
        assert!(matches!(
            app.inference_save_state,
            crate::filesystem::configurations::InferenceSaveState::Durable
        ));
        assert!(!directory.path().join("config.toml").exists());
    }

    #[test]
    fn obsolete_inference_receipt_cannot_mark_newer_save_durable() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = synthetic_app(directory.path());
        let obsolete = Arc::new(());
        let current = Arc::new(());
        app.inference_save_state = crate::filesystem::configurations::InferenceSaveState::Pending {
            operation: current.clone(),
            budget_dialog: None,
        };
        app.finish_inference_save(&obsolete, Ok(()));
        assert!(matches!(&app.inference_save_state,
            crate::filesystem::configurations::InferenceSaveState::Pending { operation, .. }
            if Arc::ptr_eq(operation, &current)));
        app.finish_inference_save(&current, Ok(()));
        assert!(matches!(
            app.inference_save_state,
            crate::filesystem::configurations::InferenceSaveState::Durable
        ));
    }

    #[test]
    fn reopened_wizard_or_changed_provider_refuses_old_catalog_without_mutation() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = synthetic_app(directory.path());
        app.begin_onboarding_inference_dialog(OnboardingInference::OpenAiModel);
        let Mode::ValueDialog(mut host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("catalog")
        };
        app.onboarding = Some(WizardUi::default());
        assert!(app
            .commit_onboarding_inference_dialog(
                &mut host,
                &DialogOutcome::Choose("synthetic-model-79".into())
            )
            .is_err());
        assert_eq!(app.inference_settings.openai.model, "authored/custom model");
        app.inference_settings.selected_provider = Provider::Anthropic;
        assert!(app
            .commit_onboarding_inference_dialog(
                &mut host,
                &DialogOutcome::Choose("synthetic-model-79".into())
            )
            .is_err());
        assert!(!directory.path().join("config.toml").exists());
    }
}
