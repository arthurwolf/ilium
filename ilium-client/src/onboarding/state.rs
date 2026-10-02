//! Pure wizard navigation. Provider requests, persistence, audio and demo
//! actions remain owned by their adapters rather than by navigation.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    #[default]
    AiChoice,
    AiConfiguration,
    SoundChoice,
    SoundConfiguration,
    KeyboardChoice,
    KeyboardPractice,
    Voice,
}

impl Step {
    pub const ALL: [Self; 7] = [
        Self::AiChoice,
        Self::AiConfiguration,
        Self::SoundChoice,
        Self::SoundConfiguration,
        Self::KeyboardChoice,
        Self::KeyboardPractice,
        Self::Voice,
    ];

    pub const fn number(self) -> usize {
        match self {
            Self::AiChoice => 1,
            Self::AiConfiguration => 2,
            Self::SoundChoice => 3,
            Self::SoundConfiguration => 4,
            Self::KeyboardChoice => 5,
            Self::KeyboardPractice => 6,
            Self::Voice => 7,
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::AiChoice => "AI assistance",
            Self::AiConfiguration => "Connect AI",
            Self::SoundChoice => "Your sound",
            Self::SoundConfiguration => "Sound studio",
            Self::KeyboardChoice => "Your controls",
            Self::KeyboardPractice => "Playground",
            Self::Voice => "Voice control",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiChoice {
    Kilo,
    Paid,
    Local,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundChoice {
    Bundled,
    System,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyboardChoice {
    Tmux,
    Screen,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoiceRequired {
    Ai,
    Sound,
    Keyboard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Navigation {
    Moved,
    Finish,
}

/// Existing installations keep their settings until onboarding is explicitly
/// requested. A started wizard resumes even after a provider edit creates the
/// config file; completing it is the only automatic-entry dismissal.
pub const fn should_open(
    explicitly_requested: bool,
    config_exists: bool,
    previously_started: bool,
    completed: bool,
) -> bool {
    explicitly_requested || (!completed && (!config_exists || previously_started))
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WizardState {
    pub step: Step,
    pub ai: Option<AiChoice>,
    pub sound: Option<SoundChoice>,
    pub keyboard: Option<KeyboardChoice>,
}

impl WizardState {
    pub fn choose_ai(&mut self, choice: AiChoice) {
        self.ai = Some(choice);
        self.step = if choice == AiChoice::Disabled {
            Step::SoundChoice
        } else {
            Step::AiConfiguration
        };
    }

    pub fn skip_ai(&mut self) {
        self.choose_ai(AiChoice::Disabled);
    }

    pub fn choose_sound(&mut self, choice: SoundChoice) {
        self.sound = Some(choice);
        self.step = Step::SoundConfiguration;
    }

    pub fn choose_keyboard(&mut self, choice: KeyboardChoice) {
        self.keyboard = Some(choice);
        self.step = Step::KeyboardPractice;
    }

    /// Configuration tests report separately: a failed connection must not
    /// trap users in onboarding or silently change their selected provider.
    pub fn advance(&mut self) -> Result<Navigation, ChoiceRequired> {
        self.step = match self.step {
            Step::AiChoice => match self.ai {
                None => return Err(ChoiceRequired::Ai),
                Some(AiChoice::Disabled) => Step::SoundChoice,
                Some(_) => Step::AiConfiguration,
            },
            Step::AiConfiguration => Step::SoundChoice,
            Step::SoundChoice => {
                self.sound.ok_or(ChoiceRequired::Sound)?;
                Step::SoundConfiguration
            }
            Step::SoundConfiguration => Step::KeyboardChoice,
            Step::KeyboardChoice => {
                self.keyboard.ok_or(ChoiceRequired::Keyboard)?;
                Step::KeyboardPractice
            }
            Step::KeyboardPractice => Step::Voice,
            Step::Voice => return Ok(Navigation::Finish),
        };
        Ok(Navigation::Moved)
    }

    pub fn back(&mut self) {
        self.step = match self.step {
            Step::AiChoice | Step::AiConfiguration => Step::AiChoice,
            Step::SoundChoice if self.ai == Some(AiChoice::Disabled) => Step::AiChoice,
            Step::SoundChoice => Step::AiConfiguration,
            Step::SoundConfiguration => Step::SoundChoice,
            Step::KeyboardChoice => Step::SoundConfiguration,
            Step::KeyboardPractice => Step::KeyboardChoice,
            Step::Voice => Step::KeyboardPractice,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_policy_distinguishes_fresh_existing_partial_and_completed_setup() {
        assert!(should_open(false, false, false, false));
        assert!(!should_open(false, true, false, false));
        assert!(should_open(false, true, true, false));
        assert!(!should_open(false, true, true, true));
        for config_exists in [false, true] {
            for started in [false, true] {
                for completed in [false, true] {
                    assert!(should_open(true, config_exists, started, completed));
                }
            }
        }
    }

    #[test]
    fn choices_are_required_before_entering_configuration() {
        let mut wizard = WizardState::default();
        assert_eq!(wizard.advance(), Err(ChoiceRequired::Ai));
        wizard.choose_ai(AiChoice::Local);
        assert_eq!(wizard.step, Step::AiConfiguration);
        assert_eq!(wizard.ai, Some(AiChoice::Local));
    }

    #[test]
    fn skipping_ai_bypasses_provider_configuration_and_survives_back() {
        let mut wizard = WizardState::default();
        wizard.skip_ai();
        assert_eq!(wizard.step, Step::SoundChoice);
        assert_eq!(wizard.ai, Some(AiChoice::Disabled));
        wizard.back();
        assert_eq!(wizard.step, Step::AiChoice);
        wizard.advance().unwrap();
        assert_eq!(wizard.step, Step::SoundChoice);
    }

    #[test]
    fn seven_steps_keep_selections_when_navigating_back() {
        let mut wizard = WizardState::default();
        wizard.choose_ai(AiChoice::Paid);
        wizard.advance().unwrap();
        assert_eq!(wizard.advance(), Err(ChoiceRequired::Sound));
        wizard.choose_sound(SoundChoice::Custom);
        wizard.advance().unwrap();
        assert_eq!(wizard.advance(), Err(ChoiceRequired::Keyboard));
        wizard.choose_keyboard(KeyboardChoice::Custom);
        assert_eq!(wizard.step, Step::KeyboardPractice);
        wizard.back();
        assert_eq!(wizard.keyboard, Some(KeyboardChoice::Custom));
        wizard.advance().unwrap();
        wizard.advance().unwrap();
        assert_eq!(wizard.step, Step::Voice);
        assert_eq!(wizard.advance(), Ok(Navigation::Finish));
        assert_eq!(wizard.step, Step::Voice);
    }

    #[test]
    fn first_step_back_is_bounded_and_ai_can_be_reenabled() {
        let mut wizard = WizardState::default();
        wizard.back();
        assert_eq!(wizard.step, Step::AiChoice);
        wizard.skip_ai();
        wizard.back();
        wizard.choose_ai(AiChoice::Kilo);
        assert_eq!(wizard.step, Step::AiConfiguration);
        assert_eq!(wizard.ai, Some(AiChoice::Kilo));
    }

    #[test]
    fn every_step_has_a_stable_number_and_visible_title() {
        for (index, step) in Step::ALL.iter().enumerate() {
            assert_eq!(step.number(), index + 1);
            assert!(!step.title().is_empty());
        }
    }
}
