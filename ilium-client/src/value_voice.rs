//! Onboarding reuses the native settings catalogs, volume bounds and runtime saves.
use crate::app::{App, Mode};
use crate::value_control::{ControlKind, ControlSpec, ValueControl};
use crate::value_dialog::DialogOutcome;
use crate::value_dialog_host::{ValueDialogHost, ValueTarget};
use crate::value_settings::SettingsNumber;
use crate::value_settings_choice::SettingsChoice;
use crate::voice_settings::VoiceRow;
use ratatui::layout::Rect;
use std::sync::Arc;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceValue {
    Choice(SettingsChoice),
    Volume,
}
impl VoiceValue {
    pub(crate) fn from_row(row: VoiceRow) -> Option<Self> {
        Some(match row {
            VoiceRow::Model => Self::Choice(SettingsChoice::VoiceModel),
            VoiceRow::Voice => Self::Choice(SettingsChoice::VoiceName),
            VoiceRow::InputMode => Self::Choice(SettingsChoice::VoiceInputMode),
            VoiceRow::InputDevice => Self::Choice(SettingsChoice::VoiceInputDevice),
            VoiceRow::OutputDevice => Self::Choice(SettingsChoice::VoiceOutputDevice),
            VoiceRow::ReasoningEffort => Self::Choice(SettingsChoice::VoiceReasoning),
            VoiceRow::VadEagerness => Self::Choice(SettingsChoice::VoiceVadEagerness),
            VoiceRow::OutputVolume => Self::Volume,
            _ => return None,
        })
    }
    fn available(self, app: &App) -> bool {
        app.onboarding.is_some()
            && app.onboarding_progress.wizard.step == crate::onboarding::state::Step::Voice
            && matches!(
                self,
                Self::Volume
                    | Self::Choice(
                        SettingsChoice::VoiceModel
                            | SettingsChoice::VoiceName
                            | SettingsChoice::VoiceInputMode
                            | SettingsChoice::VoiceInputDevice
                            | SettingsChoice::VoiceOutputDevice
                            | SettingsChoice::VoiceReasoning
                            | SettingsChoice::VoiceVadEagerness
                    )
            )
    }
    pub(crate) fn control(self, row: Rect, app: &App) -> ValueControl {
        let (kind, value, previous_enabled, next_enabled) = match self {
            Self::Choice(field) => {
                let (options, current) = field.options(app);
                let value = options
                    .iter()
                    .find(|option| option.id == current)
                    .map(|option| option.label.clone())
                    .unwrap_or(current);
                let enabled = options
                    .iter()
                    .filter(|option| option.disabled_reason.is_none())
                    .count()
                    > 1;
                (ControlKind::Choice, value, enabled, enabled)
            }
            Self::Volume => (
                ControlKind::Number,
                app.voice_settings.output_volume_percent.to_string(),
                app.voice_settings.output_volume_percent > 0,
                app.voice_settings.output_volume_percent < 100,
            ),
        };
        ValueControl::new(
            Rect::new(
                row.x.saturating_add(row.width.min(3)),
                row.y.saturating_add(1),
                row.width.saturating_sub(3),
                1,
            ),
            ControlSpec {
                kind,
                label: "",
                value: &value,
                label_width: 0,
                previous_enabled,
                next_enabled,
                open_enabled: true,
            },
        )
    }
}
impl App {
    pub(crate) fn step_onboarding_voice_value(&mut self, control: VoiceValue, previous: bool) {
        if !control.available(self) {
            return;
        }
        let direction = if previous { -1 } else { 1 };
        match control {
            VoiceValue::Choice(field) => self.step_settings_choice(field, direction),
            VoiceValue::Volume => self.step_settings_number(SettingsNumber::VoiceVolume, direction),
        }
    }
    pub(crate) fn begin_onboarding_voice_dialog(&mut self, control: VoiceValue) {
        let result = (|| {
            if !control.available(self)
                || !matches!(self.mode, Mode::Normal | Mode::Settings(_))
                || !self.modal_stack.is_empty()
            {
                return Err("Voice setup changed; reopen this control".to_string());
            }
            let directory = self
                .config_dir
                .clone()
                .filter(|path| path.is_absolute())
                .ok_or("The voice configuration directory is unavailable")?;
            let identity = self
                .onboarding
                .as_ref()
                .ok_or("Voice setup closed")?
                .identity
                .clone();
            let mut host = match control {
                VoiceValue::Choice(field) => {
                    ValueDialogHost::settings_choice(field, self, directory.clone())?
                }
                VoiceValue::Volume => ValueDialogHost::settings_number(
                    SettingsNumber::VoiceVolume,
                    self,
                    directory.clone(),
                )?,
            };
            host.target = ValueTarget::OnboardingVoice {
                control,
                identity,
                revision: self.onboarding_revision,
                original: self.voice_settings.clone(),
                directory,
            };
            Ok(host)
        })();
        match result {
            Ok(host) => self.push_modal(Mode::ValueDialog(Box::new(host))),
            Err(error) => self.status_message = Some(error),
        }
    }
    pub(crate) fn commit_onboarding_voice_dialog(
        &mut self,
        host: &mut ValueDialogHost,
        outcome: &DialogOutcome,
    ) -> Result<(), String> {
        let ValueTarget::OnboardingVoice {
            control,
            identity,
            revision,
            original,
            directory,
        } = &host.target
        else {
            return Err("This is not a voice setup control".into());
        };
        if host.is_saving()
            || !control.available(self)
            || *revision != self.onboarding_revision
            || original != &self.voice_settings
            || self.config_dir.as_ref() != Some(directory)
            || !matches!(
                self.modal_stack.last(),
                Some(Mode::Normal | Mode::Settings(_))
            )
            || !self
                .onboarding
                .as_ref()
                .is_some_and(|ui| Arc::ptr_eq(identity, &ui.identity))
        {
            return Err("Voice setup changed; reopen this control".into());
        }
        let token = Arc::new(());
        match (control, outcome) {
            (VoiceValue::Choice(field), DialogOutcome::Choose(id)) => {
                self.save_settings_choice(*field, id, directory.clone(), Some(token.clone()))?
            }
            (VoiceValue::Volume, DialogOutcome::CommitNumber(text)) => self.save_settings_number(
                SettingsNumber::VoiceVolume,
                text,
                directory.clone(),
                Some(token.clone()),
            )?,
            _ => return Err("This value does not match its voice control".into()),
        }
        if let ValueTarget::OnboardingVoice { original, .. } = &mut host.target {
            *original = self.voice_settings.clone();
        }
        host.begin_save(token);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onboarding::{screen::WizardUi, state::Step};
    use crate::value_dialog::ValueDialogState;
    fn synthetic_app(directory: &std::path::Path) -> App {
        let mut app = App::new("synthetic-voice-controls".into(), directory.into());
        app.config_dir = Some(directory.into());
        app.onboarding_progress.begin();
        app.onboarding_progress.wizard.step = Step::Voice;
        app.onboarding = Some(WizardUi::default());
        app
    }
    #[test]
    fn exactly_seven_native_choices_and_volume_reuse_existing_complete_catalogs() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = synthetic_app(directory.path());
        let mut choices = 0;
        let mut numbers = 0;
        for row in crate::onboarding::voice_ui::CONFIG_ROWS {
            match VoiceValue::from_row(row) {
                Some(VoiceValue::Choice(field)) => {
                    choices += 1;
                    let (options, selected) = field.options(&app);
                    app.begin_onboarding_voice_dialog(VoiceValue::Choice(field));
                    let Mode::ValueDialog(host) = std::mem::replace(&mut app.mode, Mode::Normal)
                    else {
                        panic!("catalog")
                    };
                    let ValueDialogState::Choice(dialog) = &host.dialog else {
                        panic!("choice")
                    };
                    assert_eq!(dialog.options(), options.as_slice());
                    if !selected.is_empty() {
                        assert!(options.iter().any(|option| option.id == selected));
                    }
                    app.finish_value_dialog(host, DialogOutcome::Cancel);
                }
                Some(VoiceValue::Volume) => numbers += 1,
                None => {}
            }
        }
        assert_eq!((choices, numbers), (7, 1));
        assert!(!VoiceValue::Choice(SettingsChoice::SoundSource).available(&app));
    }
    #[test]
    fn exact_volume_failure_retry_keeps_native_bounds_and_unrelated_voice_settings() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = synthetic_app(directory.path());
        let original = app.voice_settings.clone();
        std::fs::write(directory.path().join("config.toml"), "[voice\n").unwrap();
        app.begin_onboarding_voice_dialog(VoiceValue::Volume);
        let Mode::ValueDialog(host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("number")
        };
        app.finish_value_dialog(host, DialogOutcome::CommitNumber("101".into()));
        assert_eq!(app.voice_settings, original);
        let Mode::ValueDialog(host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("invalid retained")
        };
        app.finish_value_dialog(host, DialogOutcome::CommitNumber("73".into()));
        app.settle_filesystem_for_test();
        let Mode::ValueDialog(host) = &app.mode else {
            panic!("disk failure retains number")
        };
        assert!(!host.is_saving());
        let mut expected = original.clone();
        expected.output_volume_percent = 73;
        assert_eq!(app.voice_settings, expected);
        assert!(app.take_voice_runtime_request().is_none());
        std::fs::write(directory.path().join("config.toml"), "").unwrap();
        let Mode::ValueDialog(host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("retry")
        };
        app.finish_value_dialog(host, DialogOutcome::CommitNumber("73".into()));
        app.settle_filesystem_for_test();
        assert!(matches!(app.mode, Mode::Normal));
        assert_eq!(
            crate::config::load(directory.path()).unwrap().voice,
            expected
        );
        assert!(app.onboarding.is_some());
    }
    #[test]
    fn reopened_wizard_or_changed_voice_configuration_refuses_old_dialog() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = synthetic_app(directory.path());
        app.begin_onboarding_voice_dialog(VoiceValue::Volume);
        let Mode::ValueDialog(mut host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("number")
        };
        app.onboarding = Some(WizardUi::default());
        assert!(app
            .commit_onboarding_voice_dialog(&mut host, &DialogOutcome::CommitNumber("73".into()))
            .is_err());
        assert!(!directory.path().join("config.toml").exists());
    }
}
