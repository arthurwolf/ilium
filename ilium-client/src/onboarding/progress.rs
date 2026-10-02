//! Durable first-run and AI consent state. No credentials or live test results
//! are stored in wizard progress; provider configuration has its own owner.

use serde::{Deserialize, Serialize};

use super::state::{self, AiChoice, WizardState};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OnboardingProgress {
    pub started: bool,
    pub completed: bool,
    /// Defaults to enabled for pre-onboarding installations. Startup also
    /// checks `automatic_ai_allowed`, so a new user is never sent to a
    /// provider before making a choice.
    pub ai_enabled: bool,
    pub wizard: WizardState,
}

impl Default for OnboardingProgress {
    fn default() -> Self {
        Self {
            started: false,
            completed: false,
            ai_enabled: true,
            wizard: WizardState::default(),
        }
    }
}

impl OnboardingProgress {
    pub fn should_open(&self, explicitly_requested: bool, config_exists: bool) -> bool {
        state::should_open(
            explicitly_requested,
            config_exists,
            self.started,
            self.completed,
        )
    }

    pub fn begin(&mut self) {
        self.started = true;
    }

    pub fn choose_ai(&mut self, choice: AiChoice) {
        self.ai_enabled = choice != AiChoice::Disabled;
        self.wizard.choose_ai(choice);
    }

    pub fn finish(&mut self) {
        self.started = true;
        self.completed = true;
    }

    pub fn automatic_ai_allowed(&self) -> bool {
        self.ai_enabled && (!self.started || self.completed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starting_setup_blocks_automatic_requests_until_completion() {
        let mut progress = OnboardingProgress::default();
        assert!(progress.automatic_ai_allowed());
        progress.begin();
        assert!(!progress.automatic_ai_allowed());
        progress.choose_ai(AiChoice::Local);
        assert!(!progress.automatic_ai_allowed());
        progress.finish();
        assert!(progress.automatic_ai_allowed());
        progress.choose_ai(AiChoice::Disabled);
        progress.finish();
        assert!(!progress.automatic_ai_allowed());
    }

    #[test]
    fn partial_setup_and_skip_survive_toml_reload() {
        let mut progress = OnboardingProgress::default();
        progress.begin();
        progress.choose_ai(AiChoice::Disabled);
        let text = toml::to_string(&progress).unwrap();
        let loaded: OnboardingProgress = toml::from_str(&text).unwrap();
        assert_eq!(loaded, progress);
        assert!(loaded.should_open(false, true));
        assert!(!loaded.automatic_ai_allowed());
        progress.finish();
        let loaded: OnboardingProgress =
            toml::from_str(&toml::to_string(&progress).unwrap()).unwrap();
        assert!(!loaded.should_open(false, true));
        assert!(loaded.should_open(true, true));
    }

    #[test]
    fn legacy_defaults_preserve_preexisting_ai_settings() {
        let progress: OnboardingProgress = toml::from_str("").unwrap();
        assert!(!progress.should_open(false, true));
        assert!(progress.automatic_ai_allowed());
        assert!(progress.should_open(false, false));
    }

    #[test]
    fn rerunning_a_completed_wizard_keeps_its_completion_marker() {
        let mut progress = OnboardingProgress::default();
        progress.finish();
        progress.begin();
        assert!(progress.completed);
        assert!(!progress.should_open(false, true));
    }
}
