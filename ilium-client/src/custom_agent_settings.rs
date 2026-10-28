//! Keyboard interaction for the custom-agent settings editor.
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

use crate::app::{App, SettingsState};
use crate::custom_agent_editor::{CustomAgentEditor, EditorRow};
use crate::filesystem::configuration::ConfigurationChange;
use crate::filesystem::configurations::ConfigurationIntent;

/// Missing identities remain recoverable without copying their former secrets into snapshots.
pub(crate) fn missing_profiles(app: &App) -> Vec<ilium_core::custom_agent::CustomAgentIdentity> {
    let mut missing: Vec<_> = app
        .custom_agent_panes
        .values()
        .filter(|identity| {
            !app.custom_agents
                .profiles
                .iter()
                .any(|profile| profile.id == identity.profile_id)
        })
        .cloned()
        .collect();
    missing.sort_by(|a, b| {
        a.profile_id
            .cmp(&b.profile_id)
            .then(a.base_provider.cmp(&b.base_provider))
    });
    missing.dedup_by(|a, b| a.profile_id == b.profile_id && a.base_provider == b.base_provider);
    missing
}

fn text_field(editor: &mut CustomAgentEditor, row: EditorRow) -> Option<&mut String> {
    match row {
        EditorRow::Name => Some(&mut editor.draft.name),
        EditorRow::Icon => Some(&mut editor.draft.icon),
        EditorRow::EnvironmentKey(index) => editor
            .draft
            .environment
            .get_mut(index)
            .map(|entry| &mut entry.key),
        EditorRow::EnvironmentValue(index) => editor
            .draft
            .environment
            .get_mut(index)
            .map(|entry| &mut entry.value),
        _ => None,
    }
}

fn enqueue(app: &mut App, state: &mut SettingsState, desired: crate::config::CustomAgentSettings) {
    let Some(directory) = app.config_dir.clone() else {
        app.status_message = Some("Configuration directory unavailable; profile is unsaved".into());
        return;
    };
    let change = ConfigurationChange::CustomAgents {
        previous: app.custom_agents.clone(),
        desired,
    };
    if app
        .enqueue_configuration(directory, change, ConfigurationIntent::CustomAgents)
        .is_ok()
    {
        state.custom_agent_save_pending = true;
    }
}

/// Returns true when this screen consumed the event.
pub(crate) fn handle(app: &mut App, state: &mut SettingsState, event: &Event) -> bool {
    if state.custom_agent_save_pending {
        return true;
    }
    if state.custom_agent_editing {
        let Some(editor) = state.custom_agent_editor.as_mut() else {
            return false;
        };
        let Some(row) = editor.rows().get(state.selected_row).copied() else {
            return false;
        };
        let Some(field) = text_field(editor, row) else {
            return false;
        };
        match event {
            Event::Paste(text) => field.push_str(text),
            Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                match key.code {
                    KeyCode::Enter => {
                        state.custom_agent_editing = false;
                        state.custom_agent_edit_original = None;
                    }
                    KeyCode::Esc => {
                        if let Some(original) = state.custom_agent_edit_original.take() {
                            *field = original;
                        }
                        state.custom_agent_editing = false;
                    }
                    KeyCode::Backspace => {
                        field.pop();
                    }
                    KeyCode::Char(character)
                        if !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                    {
                        field.push(character)
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        return true;
    }
    let Event::Key(key) = event else {
        return false;
    };
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return false;
    }
    if state.custom_agent_delete_confirm {
        match key.code {
            KeyCode::Char('y' | 'Y') => {
                let mut desired = app.custom_agents.clone();
                if state.selected_row < desired.profiles.len() {
                    desired.profiles.remove(state.selected_row);
                    enqueue(app, state, desired);
                }
                state.custom_agent_delete_confirm = false;
            }
            KeyCode::Char('n' | 'N') | KeyCode::Esc => state.custom_agent_delete_confirm = false,
            _ => {}
        }
        return true;
    }
    if let Some(editor) = &mut state.custom_agent_editor {
        let rows = editor.rows();
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                state.selected_row = state.selected_row.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                state.selected_row = (state.selected_row + 1).min(rows.len().saturating_sub(1))
            }
            KeyCode::Esc => {
                state.custom_agent_editor = None;
                state.selected_row = 0;
                state.scroll = 0;
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                let Some(row) = rows.get(state.selected_row).copied() else {
                    return true;
                };
                match row {
                    EditorRow::Icon => {
                        state.icon_picker = Some(crate::app::IconPickerState::new(
                            crate::icon_settings::IconTarget::Terminal,
                        ));
                    }
                    EditorRow::Name
                    | EditorRow::EnvironmentKey(_)
                    | EditorRow::EnvironmentValue(_) => {
                        if let Some(field) = text_field(editor, row) {
                            state.custom_agent_edit_original = Some(field.clone());
                            state.custom_agent_editing = true;
                        }
                    }
                    EditorRow::Provider => {
                        editor.draft.base_provider = match editor.draft.base_provider {
                            ilium_core::BuiltinAgentProvider::Claude => {
                                ilium_core::BuiltinAgentProvider::Codex
                            }
                            _ => ilium_core::BuiltinAgentProvider::Claude,
                        }
                    }
                    EditorRow::RemoveEnvironment(index) => {
                        editor.remove_environment_row(index);
                        state.selected_row = state.selected_row.min(editor.rows().len() - 1);
                    }
                    EditorRow::AddEnvironment => {
                        editor.add_environment_row();
                        state.selected_row = 3 + (editor.draft.environment.len() - 1) * 3;
                    }
                    EditorRow::RevealValues => editor.reveal_values = !editor.reveal_values,
                    EditorRow::Cancel => {
                        state.custom_agent_editor = None;
                        state.selected_row = 0;
                        state.scroll = 0;
                    }
                    EditorRow::Save => {
                        if let Err(error) = editor.validate(&app.custom_agents.profiles) {
                            app.status_message = Some(error.to_string());
                            return true;
                        }
                        let mut desired = app.custom_agents.clone();
                        if let Some(existing) = desired
                            .profiles
                            .iter_mut()
                            .find(|profile| profile.id == editor.draft.id)
                        {
                            *existing = editor.draft.clone();
                        } else {
                            desired.profiles.push(editor.draft.clone());
                        }
                        enqueue(app, state, desired);
                    }
                }
            }
            _ => return false,
        }
        return true;
    }
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            state.selected_row = state.selected_row.saturating_sub(1)
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.selected_row = (state.selected_row + 1)
                .min(app.custom_agents.profiles.len() + missing_profiles(app).len())
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            let missing = missing_profiles(app);
            state.custom_agent_editor = Some(
                if let Some(profile) = app.custom_agents.profiles.get(state.selected_row) {
                    CustomAgentEditor::edit(profile)
                } else if let Some(identity) = state
                    .selected_row
                    .checked_sub(app.custom_agents.profiles.len())
                    .and_then(|index| missing.get(index))
                {
                    let mut editor = CustomAgentEditor::new();
                    editor.draft.id = identity.profile_id.clone();
                    editor.draft.base_provider = identity.base_provider;
                    editor
                } else {
                    CustomAgentEditor::new()
                },
            );
            state.selected_row = 0;
            state.scroll = 0;
        }
        KeyCode::Delete | KeyCode::Backspace
            if state.selected_row < app.custom_agents.profiles.len() =>
        {
            state.custom_agent_delete_confirm = true
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn keyboard_draft_keeps_literal_values_and_cancels_edit_without_destroying_it() {
        let mut app = App::new("test".into(), std::env::temp_dir());
        let mut state = SettingsState::new();
        assert!(handle(&mut app, &mut state, &key(KeyCode::Enter)));
        assert!(handle(&mut app, &mut state, &key(KeyCode::Enter)));
        assert!(handle(&mut app, &mut state, &Event::Paste("Work ☃".into())));
        assert!(handle(&mut app, &mut state, &key(KeyCode::Enter)));
        assert_eq!(
            state.custom_agent_editor.as_ref().unwrap().draft.name,
            "Work ☃"
        );
        assert!(handle(&mut app, &mut state, &key(KeyCode::Enter)));
        assert!(handle(
            &mut app,
            &mut state,
            &Event::Paste(" discarded".into())
        ));
        assert!(handle(&mut app, &mut state, &key(KeyCode::Esc)));
        assert_eq!(
            state.custom_agent_editor.as_ref().unwrap().draft.name,
            "Work ☃"
        );
    }

    #[test]
    fn missing_profile_can_be_recreated_with_its_original_identity() {
        let mut app = App::new("test".into(), std::env::temp_dir());
        let identity = ilium_core::custom_agent::CustomAgentIdentity {
            profile_id: "missing-profile".into(),
            base_provider: ilium_core::BuiltinAgentProvider::Codex,
        };
        app.custom_agent_panes
            .insert(ilium_core::NodeId(1), identity.clone());
        let mut state = SettingsState::new();
        assert!(handle(&mut app, &mut state, &key(KeyCode::Enter)));
        let draft = &state.custom_agent_editor.as_ref().unwrap().draft;
        assert_eq!(draft.id, identity.profile_id);
        assert_eq!(draft.base_provider, identity.base_provider);
        assert!(draft.environment.is_empty());
    }

    #[test]
    fn icon_control_opens_the_shared_icon_catalog() {
        let mut app = App::new("test".into(), std::env::temp_dir());
        let mut state = SettingsState::new();
        state.custom_agent_editor = Some(CustomAgentEditor::new());
        state.selected_row = 1;
        assert!(handle(&mut app, &mut state, &key(KeyCode::Enter)));
        assert!(state.icon_picker.is_some());
        assert!(!state.custom_agent_editing);
    }
}
