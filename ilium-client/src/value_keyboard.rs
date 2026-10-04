//! Exact prefix catalogs shared by Settings and the onboarding keyboard editor.
use crate::app::{App, Mode, SettingsTab};
use crate::config::KeyboardSettings;
use crate::keymap::ShortcutBase;
use crate::value_control::{ControlKind, ControlSpec, ValueControl};
use crate::value_dialog::{ChoiceOption, DialogOutcome};
use crate::value_dialog_host::{ValueDialogHost, ValueTarget};
use ratatui::layout::Rect;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyboardPrefix {
    General,
    Navigation,
}
impl KeyboardPrefix {
    pub const ALL: [Self; 2] = [Self::General, Self::Navigation];
    pub fn row(self) -> usize {
        match self {
            Self::General => 0,
            Self::Navigation => 1,
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::General => "General prefix",
            Self::Navigation => "Tree prefix",
        }
    }
    pub fn current(self, settings: KeyboardSettings) -> ShortcutBase {
        match self {
            Self::General => settings.shortcut_base,
            Self::Navigation => settings.navigation_shortcut_base,
        }
    }
    pub fn desired(self, mut settings: KeyboardSettings, prefix: ShortcutBase) -> KeyboardSettings {
        match self {
            Self::General => settings.shortcut_base = prefix,
            Self::Navigation => settings.navigation_shortcut_base = prefix,
        };
        settings
    }
    pub fn options(self) -> Vec<ChoiceOption> {
        ('a'..='z')
            .map(|letter| {
                let value = ShortcutBase::parse(&letter.to_string());
                ChoiceOption {
                    id: letter.to_string(),
                    label: value.map_or_else(|| letter.to_string(), ShortcutBase::label),
                    disabled_reason: None,
                }
            })
            .collect()
    }
    pub fn control(self, row: Rect, settings: KeyboardSettings, label_width: u16) -> ValueControl {
        ValueControl::new(
            row,
            ControlSpec {
                kind: ControlKind::Choice,
                label: self.title(),
                value: &self.current(settings).label(),
                label_width: label_width.min(row.width.saturating_sub(13)),
                previous_enabled: true,
                next_enabled: true,
                open_enabled: true,
            },
        )
    }
    pub fn settings_control(
        self,
        area: Rect,
        scroll: u16,
        settings: KeyboardSettings,
    ) -> Option<ValueControl> {
        let line = (1 + 2 * self.row()) as u16;
        let offset = line.checked_sub(scroll)?;
        if offset >= area.height {
            return None;
        }
        let inset = area.width.min(2);
        Some(self.control(
            Rect::new(area.x + inset, area.y + offset, area.width - inset, 1),
            settings,
            39,
        ))
    }
}
pub enum KeyboardDestination {
    Settings,
    Onboarding { identity: Arc<()>, revision: u64 },
}
impl App {
    pub(crate) fn set_keyboard_prefix(&mut self, field: KeyboardPrefix, prefix: ShortcutBase) {
        let result = (|| {
            let directory = self
                .config_dir
                .clone()
                .filter(|path| path.is_absolute())
                .ok_or("The keyboard configuration directory is unavailable")?;
            let desired = field.desired(self.keyboard_settings, prefix);
            self.enqueue_configuration(
                directory,
                crate::filesystem::configuration::ConfigurationChange::Keyboard(desired),
                crate::filesystem::configurations::ConfigurationIntent::Plain {
                    label: "Keyboard prefix",
                    success: None,
                },
            )?;
            self.keyboard_settings = desired;
            Ok::<(), String>(())
        })();
        if let Err(error) = result {
            self.status_message = Some(error);
        }
    }
    pub(crate) fn step_keyboard_prefix(&mut self, field: KeyboardPrefix, direction: i32) {
        self.set_keyboard_prefix(
            field,
            field.current(self.keyboard_settings).stepped(direction),
        );
    }

    pub(crate) fn begin_keyboard_prefix_dialog(&mut self, field: KeyboardPrefix) {
        let result = (|| {
            let destination = if matches!(&self.mode, Mode::Settings(state) if state.tab == SettingsTab::Keyboard && state.keyboard_picker.is_none())
            {
                KeyboardDestination::Settings
            } else if self.onboarding_progress.wizard.step
                == crate::onboarding::state::Step::KeyboardPractice
            {
                let ui = self
                    .onboarding
                    .as_ref()
                    .filter(|ui| ui.keyboard_editing && ui.keyboard_ui.pending_rebind.is_none())
                    .ok_or("The keyboard editor is no longer open")?;
                KeyboardDestination::Onboarding {
                    identity: ui.keyboard_ui.identity.clone(),
                    revision: self.onboarding_revision,
                }
            } else {
                return Err("The keyboard prefix control is no longer open".into());
            };
            let directory = self
                .config_dir
                .clone()
                .filter(|path| path.is_absolute())
                .ok_or("The keyboard configuration directory is unavailable")?;
            ValueDialogHost::keyboard_prefix(field, destination, directory, self.keyboard_settings)
        })();
        match result {
            Ok(host) => self.push_modal(Mode::ValueDialog(Box::new(host))),
            Err(error) => self.status_message = Some(error),
        }
    }
    pub(crate) fn commit_keyboard_prefix_dialog(
        &mut self,
        host: &mut ValueDialogHost,
        outcome: &DialogOutcome,
    ) -> Result<(), String> {
        let ValueTarget::KeyboardPrefix {
            field,
            destination,
            directory,
        } = &host.target
        else {
            return Err("This dialog belongs to another control".into());
        };
        if host.is_saving() || self.config_dir.as_ref() != Some(directory) {
            return Err("The keyboard destination changed or its save is pending".into());
        }
        let valid = match destination {
            KeyboardDestination::Settings => {
                matches!(self.modal_stack.last(),Some(Mode::Settings(state)) if state.tab == SettingsTab::Keyboard && state.keyboard_picker.is_none())
            }
            KeyboardDestination::Onboarding { identity, revision } => {
                *revision == self.onboarding_revision
                    && self.onboarding_progress.wizard.step
                        == crate::onboarding::state::Step::KeyboardPractice
                    && matches!(
                        self.modal_stack.last(),
                        Some(Mode::Normal | Mode::Settings(_))
                    )
                    && self.onboarding.as_ref().is_some_and(|ui| {
                        ui.keyboard_editing
                            && ui.keyboard_ui.pending_rebind.is_none()
                            && Arc::ptr_eq(identity, &ui.keyboard_ui.identity)
                    })
            }
        };
        if !valid {
            return Err("The keyboard editor changed; reopen its prefix catalog".into());
        }
        let DialogOutcome::Choose(id) = outcome else {
            return Err("Choose a keyboard prefix".into());
        };
        let prefix = ShortcutBase::parse(id).ok_or("This keyboard prefix is unavailable")?;
        let desired = field.desired(self.keyboard_settings, prefix);
        let token = Arc::new(());
        self.enqueue_configuration(
            directory.clone(),
            crate::filesystem::configuration::ConfigurationChange::Keyboard(desired),
            crate::filesystem::configurations::ConfigurationIntent::ValueDialog {
                token: token.clone(),
            },
        )?;
        self.keyboard_settings = desired;
        host.begin_save(token);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value_control::{ControlAction, PointerButton};
    use crate::value_dialog::ValueDialogState;
    use ratatui::{backend::TestBackend, layout::Position, Terminal};

    #[test]
    fn both_prefix_catalogs_contain_every_letter_and_paint_precise_controls() {
        let directory = tempfile::tempdir().unwrap();
        let app = App::new("prefix-render".into(), directory.path().into());
        for field in KeyboardPrefix::ALL {
            let host = ValueDialogHost::keyboard_prefix(
                field,
                KeyboardDestination::Settings,
                directory.path().into(),
                app.keyboard_settings,
            )
            .unwrap();
            let ValueDialogState::Choice(choice) = host.dialog else {
                panic!("catalog");
            };
            assert_eq!(choice.options().len(), 26);
            for (option, letter) in choice.options().iter().zip('a'..='z') {
                assert_eq!(option.id, letter.to_string());
                assert_eq!(
                    option.label,
                    format!("Ctrl+{}", letter.to_ascii_uppercase())
                );
            }
            for width in [24, 40, 80, 140] {
                let area = Rect::new(0, 0, width, 8);
                let control = field
                    .settings_control(area, 0, app.keyboard_settings)
                    .unwrap();
                let geometry = control.geometry();
                assert_eq!(
                    geometry.value.width, 6,
                    "Complete Ctrl+letter remains visible at {width} columns"
                );
                let mut terminal = Terminal::new(TestBackend::new(width, 8)).unwrap();
                terminal
                    .draw(|frame| control.render(frame, Default::default()))
                    .unwrap();
                for (rect, glyph, action) in [
                    (geometry.previous, "←", ControlAction::PreviousChoice),
                    (geometry.open, "+", ControlAction::OpenChoices),
                    (geometry.next, "→", ControlAction::NextChoice),
                ] {
                    assert_eq!(
                        terminal.backend().buffer()[(rect.x, rect.y)].symbol(),
                        glyph
                    );
                    assert_eq!(
                        control.hit(Position::new(rect.x, rect.y), PointerButton::Left),
                        Some(action)
                    );
                }
                assert_eq!(
                    control.hit(
                        Position::new(geometry.label.x, geometry.label.y),
                        PointerButton::Left
                    ),
                    None
                );
            }
        }
    }

    #[test]
    fn settings_renderer_paints_both_prefixes_at_the_pointer_geometry() {
        let directory = tempfile::tempdir().unwrap();
        let app = App::new("prefix-settings-render".into(), directory.path().into());
        let state = crate::app::SettingsState {
            tab: SettingsTab::Keyboard,
            ..Default::default()
        };
        for width in [80, 140] {
            let screen = Rect::new(0, 0, width, 60);
            let mut area =
                crate::settings_ui::compute_layout_for_mode(screen, &app, &state).content_area;
            let instructions = crate::instruction_settings::panel_height(state.tab, area);
            area.y += instructions;
            area.height = area.height.saturating_sub(instructions);
            let mut terminal = Terminal::new(TestBackend::new(width, 60)).unwrap();
            terminal
                .draw(|frame| crate::settings_ui::render(frame, screen, &app, &state))
                .unwrap();
            for field in KeyboardPrefix::ALL {
                let geometry = field
                    .settings_control(area, state.scroll, app.keyboard_settings)
                    .unwrap()
                    .geometry();
                for (rect, glyph) in [
                    (geometry.previous, "←"),
                    (geometry.open, "+"),
                    (geometry.next, "→"),
                ] {
                    assert_eq!(
                        terminal.backend().buffer()[(rect.x, rect.y)].symbol(),
                        glyph
                    );
                }
                let value: String = (geometry.value.x..geometry.value.right())
                    .map(|x| terminal.backend().buffer()[(x, geometry.value.y)].symbol())
                    .collect();
                assert_eq!(value, field.current(app.keyboard_settings).label());
            }
        }
    }

    #[test]
    fn prefix_catalog_retains_selection_after_save_failure_then_retries_without_remapping() {
        for field in KeyboardPrefix::ALL {
            let directory = tempfile::tempdir().unwrap();
            let mut app = App::new("prefix-save".into(), directory.path().into());
            app.config_dir = Some(directory.path().into());
            app.mode = Mode::Settings(crate::app::SettingsState {
                tab: SettingsTab::Keyboard,
                selected_row: field.row(),
                ..Default::default()
            });
            let original = app.keyboard_settings;
            let bindings = app.keybindings.clone();
            std::fs::write(directory.path().join("config.toml"), "[keyboard\n").unwrap();
            app.begin_keyboard_prefix_dialog(field);
            let Mode::ValueDialog(mut host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
                panic!("catalog");
            };
            let ValueDialogState::Choice(choice) = &mut host.dialog else {
                panic!("choice");
            };
            choice.selected_id = Some("z".into());
            app.finish_value_dialog(host, DialogOutcome::Choose("z".into()));
            app.settle_filesystem_for_test();
            let Mode::ValueDialog(host) = &app.mode else {
                panic!("failed write retains catalog");
            };
            assert!(!host.is_saving());
            let ValueDialogState::Choice(choice) = &host.dialog else {
                panic!("choice");
            };
            assert_eq!(choice.selected_id.as_deref(), Some("z"));
            assert!(choice.notice.is_some());
            assert_eq!(app.keybindings, bindings);
            let other = if field == KeyboardPrefix::General {
                KeyboardPrefix::Navigation
            } else {
                KeyboardPrefix::General
            };
            assert_eq!(
                other.current(app.keyboard_settings),
                other.current(original)
            );
            std::fs::write(directory.path().join("config.toml"), "").unwrap();
            let Mode::ValueDialog(host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
                panic!("retry");
            };
            app.finish_value_dialog(host, DialogOutcome::Choose("z".into()));
            app.settle_filesystem_for_test();
            assert!(matches!(app.mode, Mode::Settings(_)));
            assert_eq!(
                field
                    .current(crate::config::load(directory.path()).unwrap().keyboard)
                    .letter(),
                'z'
            );
            assert_eq!(app.keybindings, bindings);
        }
    }

    #[test]
    fn onboarding_prefix_rejects_replacement_editor_and_unadmitted_steps_preserve_values() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new("prefix-parent".into(), directory.path().into());
        let original = app.keyboard_settings;
        app.config_dir = None;
        app.step_keyboard_prefix(KeyboardPrefix::General, 1);
        assert_eq!(app.keyboard_settings, original);
        app.config_dir = Some(directory.path().into());
        app.onboarding_progress.wizard.step = crate::onboarding::state::Step::KeyboardPractice;
        let mut ui = crate::onboarding::screen::WizardUi::default();
        ui.keyboard_editing = true;
        app.onboarding = Some(ui);
        app.begin_keyboard_prefix_dialog(KeyboardPrefix::General);
        assert!(matches!(app.mode, Mode::ValueDialog(_)));
        app.onboarding.as_mut().unwrap().keyboard_ui = Default::default();
        let attempts = app.configuration_admission.attempts;
        let Mode::ValueDialog(host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("catalog");
        };
        app.finish_value_dialog(host, DialogOutcome::Choose("z".into()));
        assert!(matches!(app.mode, Mode::ValueDialog(_)));
        assert_eq!(app.keyboard_settings, original);
        assert_eq!(app.configuration_admission.attempts, attempts);
    }
}
