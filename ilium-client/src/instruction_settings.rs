//! Shared instruction inputs: each location edits the same persisted value.
use crate::app::{App, SettingsTab};
use ratatui::{layout::Rect, text::Line, widgets::Paragraph, Frame};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstructionField {
    Voice,
    EntryNaming,
    Organization,
    SharedContext,
    ProjectNaming,
    SmartCopy,
    AskForUpdate,
}
impl InstructionField {
    pub const ALL: [Self; 7] = [
        Self::Voice,
        Self::EntryNaming,
        Self::Organization,
        Self::SharedContext,
        Self::ProjectNaming,
        Self::SmartCopy,
        Self::AskForUpdate,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Voice => "Voice assistant instructions",
            Self::EntryNaming => "Entry naming instructions",
            Self::Organization => "Organization instructions",
            Self::SharedContext => "Shared naming and organization context",
            Self::ProjectNaming => "Project naming instructions",
            Self::SmartCopy => "Smart Copy instructions",
            Self::AskForUpdate => "Ask for update instructions",
        }
    }
    pub fn compact_label(self) -> &'static str {
        match self {
            Self::Voice => "Voice assistant",
            Self::EntryNaming => "Entry naming",
            Self::Organization => "Organization",
            Self::SharedContext => "Shared naming + grouping",
            Self::ProjectNaming => "Project naming",
            Self::SmartCopy => "Smart Copy",
            Self::AskForUpdate => "Ask for update",
        }
    }
    pub fn compact_description(self) -> &'static str {
        match self {
            Self::Voice => "Language and response style.",
            Self::EntryNaming => "Wording for entry titles.",
            Self::Organization => "Grouping and ordering rules.",
            Self::SharedContext => "Vocabulary for names + groups.",
            Self::ProjectNaming => "Project names and acronyms.",
            Self::SmartCopy => "Prioritize exact-source items.",
            Self::AskForUpdate => "What status updates emphasize.",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::Voice => "Language, vocabulary and how the voice assistant responds.",
            Self::EntryNaming => {
                "Preferred wording and terminology for entry titles and summaries."
            }
            Self::Organization => "How entries are grouped and ordered during restructure.",
            Self::SharedContext => {
                "Vocabulary and preferences applied to both naming and organization."
            }
            Self::ProjectNaming => "Preferred language and abbreviations for project names.",
            Self::SmartCopy => "Which exact-source copy targets to prioritize.",
            Self::AskForUpdate => "What agents should emphasize when asked for a status update.",
        }
    }
    pub fn value(self, app: &App) -> &str {
        let i = &app.inference_settings.instructions;
        match self {
            Self::Voice => &app.voice_settings.custom_prompt,
            Self::EntryNaming => &i.entry_naming,
            Self::Organization => &i.organization,
            Self::SharedContext => &i.naming_and_organization,
            Self::ProjectNaming => &i.project_naming,
            Self::SmartCopy => &i.smart_copy,
            Self::AskForUpdate => &i.ask_for_update,
        }
    }
}
pub fn fields(tab: SettingsTab) -> &'static [InstructionField] {
    use InstructionField::*;
    match tab {
        SettingsTab::LlmInstructions => &InstructionField::ALL,
        SettingsTab::Titles => &[EntryNaming],
        SettingsTab::Inference => &[Organization, SharedContext, ProjectNaming, SmartCopy],
        SettingsTab::AgentMonitoring => &[AskForUpdate],
        _ => &[],
    }
}
pub const SELECTION_BASE: usize = 10000;
pub fn panel_height(tab: SettingsTab, area: Rect) -> u16 {
    if fields(tab).is_empty() {
        0
    } else {
        (fields(tab).len() as u16 * 3 + 2).min(if tab == SettingsTab::LlmInstructions {
            area.height
        } else {
            area.height / 2
        })
    }
}
pub fn first_visible(tab: SettingsTab, area: Rect, selection: usize) -> usize {
    let count = fields(tab).len();
    let visible = (usize::from(area.height.saturating_sub(1)) / 3).max(1);
    selection
        .saturating_sub(SELECTION_BASE)
        .min(count.saturating_sub(1))
        .saturating_sub(visible.saturating_sub(1))
        .min(count.saturating_sub(visible))
}
pub fn render(frame: &mut Frame, area: Rect, app: &App, tab: SettingsTab, selection: usize) {
    let mut lines = vec![Line::from(
        "Additional instructions · i: focus · Enter: edit · Delete: clear",
    )];
    for (index, field) in fields(tab)
        .iter()
        .enumerate()
        .skip(first_visible(tab, area, selection))
    {
        let marker = if selection == SELECTION_BASE + index {
            "›"
        } else {
            " "
        };
        let value = if field.value(app).is_empty() {
            "Built-in default".to_owned()
        } else {
            field
                .value(app)
                .replace('\n', " ")
                .chars()
                .take(usize::from(area.width.saturating_sub(8)))
                .collect()
        };
        lines.push(Line::from(format!(
            "{marker} {}: {value}",
            field.compact_label()
        )));
        lines.push(Line::from(format!("  {}", field.compact_description())));
        lines.push(Line::default());
    }
    frame.render_widget(Paragraph::new(lines), area);
}
impl App {
    pub fn settings_open_instruction(&mut self, field: InstructionField) {
        let mut state = crate::voice_settings::VoicePromptEditorState::new(field.value(self));
        state.instruction_field = field;
        self.push_modal(crate::app::Mode::VoicePromptEditor(Box::new(state)));
    }
    pub fn settings_commit_instruction(&mut self, field: InstructionField, value: String) {
        if field == InstructionField::Voice {
            self.settings_commit_voice_prompt(value);
            return;
        }
        let mut settings = self.inference_settings.clone();
        let i = &mut settings.instructions;
        match field {
            InstructionField::EntryNaming => i.entry_naming = value,
            InstructionField::Organization => i.organization = value,
            InstructionField::SharedContext => i.naming_and_organization = value,
            InstructionField::ProjectNaming => i.project_naming = value,
            InstructionField::SmartCopy => i.smart_copy = value,
            InstructionField::AskForUpdate => i.ask_for_update = value,
            InstructionField::Voice => unreachable!(),
        };
        self.apply_and_persist_inference_settings(settings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_inputs_persist_through_the_same_values_and_clear() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new("instructions".into(), directory.path().to_path_buf());
        app.config_dir = Some(directory.path().to_path_buf());
        for field in InstructionField::ALL {
            let value = format!("{}\n{{{{literal}}}} & unicode 🦀\n", field.label());
            app.settings_commit_instruction(field, value.clone());
            assert_eq!(field.value(&app), value);
            app.settings_open_instruction(field);
            let crate::app::Mode::VoicePromptEditor(editor) = &app.mode else {
                panic!("missing editor")
            };
            assert_eq!(editor.instruction_field, field);
            assert_eq!(editor.text(), value);
            app.pop_modal();
        }
        let loaded = crate::config::load(directory.path()).unwrap();
        assert_eq!(
            loaded.inference.instructions,
            app.inference_settings.instructions
        );
        assert_eq!(loaded.voice.custom_prompt, app.voice_settings.custom_prompt);
        for field in InstructionField::ALL {
            app.settings_commit_instruction(field, String::new());
            assert!(field.value(&app).is_empty());
        }
    }
    #[test]
    fn every_input_has_a_specific_location_and_the_shared_location() {
        assert_eq!(fields(SettingsTab::LlmInstructions), InstructionField::ALL);
        let mut specific = Vec::new();
        for tab in [
            SettingsTab::Titles,
            SettingsTab::Inference,
            SettingsTab::AgentMonitoring,
        ] {
            specific.extend_from_slice(fields(tab));
        }
        specific.push(InstructionField::Voice); // existing VoiceControl custom-prompt row
        for field in InstructionField::ALL {
            assert_eq!(specific.iter().filter(|f| **f == field).count(), 1);
        }
    }
    #[test]
    fn short_shared_panel_keeps_last_input_visible() {
        let area = Rect::new(0, 0, 60, 12);
        assert_eq!(
            first_visible(SettingsTab::LlmInstructions, area, SELECTION_BASE + 6),
            4
        );
    }
}

pub fn editor_area(screen: Rect) -> Rect {
    let area = crate::modal::multiline_prompt_dialog_layout(screen).editor_area;
    let description_height = 4.min(area.height.saturating_sub(1));
    Rect {
        y: area.y + description_height,
        height: area.height - description_height,
        ..area
    }
}
pub fn render_editor(
    frame: &mut Frame,
    screen: Rect,
    state: &crate::voice_settings::VoicePromptEditorState,
) {
    crate::modal::render_multiline_prompt(
        frame,
        screen,
        state.instruction_field.compact_label(),
        &state.textarea,
        "Apply",
    );
    let original = crate::modal::multiline_prompt_dialog_layout(screen).editor_area;
    frame.render_widget(ratatui::widgets::Clear, original);
    let editor = editor_area(screen);
    let description = Rect {
        height: editor.y - original.y,
        ..original
    };
    frame.render_widget(
        Paragraph::new(format!(
            "{}\n{}",
            state.instruction_field.label(),
            state.instruction_field.description()
        ))
        .wrap(ratatui::widgets::Wrap { trim: false }),
        description,
    );
    frame.render_widget(&state.textarea, editor);
}

#[cfg(test)]
mod interaction_tests {
    use super::*;
    use crate::app::{Mode, SettingsState};
    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    fn key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
        crate::keys::handle_event(app, Event::Key(KeyEvent::new(code, modifiers)));
    }
    #[test]
    fn shared_keyboard_and_specific_mouse_edit_the_same_value_and_return_to_tab() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new("instructions".into(), directory.path().to_path_buf());
        app.config_dir = Some(directory.path().to_path_buf());
        app.layout.screen_area = Rect::new(0, 0, 80, 24);
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::LlmInstructions,
            selected_row: SELECTION_BASE + 1,
            ..Default::default()
        });
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        key(&mut app, KeyCode::Char('X'), KeyModifiers::NONE);
        key(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);
        assert_eq!(app.inference_settings.instructions.entry_naming, "X");
        assert!(matches!(&app.mode,Mode::Settings(s) if s.tab==SettingsTab::LlmInstructions));
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Titles,
            ..Default::default()
        });
        let area = crate::settings_ui::compute_layout(app.layout.screen_area).content_area;
        crate::mouse::handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: area.x + 1,
                row: area.y + 1,
                modifiers: KeyModifiers::NONE,
            },
        );
        let Mode::VoicePromptEditor(editor) = &mut app.mode else {
            panic!("specific editor missing")
        };
        assert_eq!(editor.text(), "X");
        editor.textarea.insert_str("Y");
        let actions = crate::modal::multiline_prompt_dialog_layout(app.layout.screen_area).actions;
        crate::mouse::handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: actions.confirm_button.x,
                row: actions.confirm_button.y,
                modifiers: KeyModifiers::NONE,
            },
        );
        assert!(app
            .inference_settings
            .instructions
            .entry_naming
            .contains('Y'));
        assert!(matches!(&app.mode,Mode::Settings(s) if s.tab==SettingsTab::Titles));
        key(&mut app, KeyCode::Up, KeyModifiers::NONE);
        key(&mut app, KeyCode::Delete, KeyModifiers::NONE);
        assert!(app.inference_settings.instructions.entry_naming.is_empty());
    }
    #[test]
    fn editor_shows_full_shared_explanation_at_sixty_columns() {
        let mut state = crate::voice_settings::VoicePromptEditorState::new("");
        state.instruction_field = InstructionField::SharedContext;
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 20)).unwrap();
        terminal
            .draw(|frame| render_editor(frame, Rect::new(0, 0, 60, 20), &state))
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("Shared naming and organization context"));
        assert!(text.contains("organization."));
    }
    #[test]
    fn wheel_reaches_the_last_shared_field_in_a_short_terminal() {
        let mut app = App::new("instructions".into(), std::env::temp_dir());
        app.layout.screen_area = Rect::new(0, 0, 60, 20);
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::LlmInstructions,
            ..Default::default()
        });
        let area = crate::settings_ui::compute_layout(app.layout.screen_area).content_area;
        for _ in 0..6 {
            crate::mouse::handle_mouse_event(
                &mut app,
                MouseEvent {
                    kind: MouseEventKind::ScrollDown,
                    column: area.x + 1,
                    row: area.y + 1,
                    modifiers: KeyModifiers::NONE,
                },
            );
        }
        assert!(matches!(&app.mode,Mode::Settings(s) if s.selected_row==SELECTION_BASE+6));
        let backend = ratatui::backend::TestBackend::new(60, 20);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    area,
                    &app,
                    SettingsTab::LlmInstructions,
                    SELECTION_BASE + 6,
                )
            })
            .unwrap();
        let screen = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(screen.contains("Ask for update"));
    }
}
