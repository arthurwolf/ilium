//! Keyboard handling for the custom status rules editor.
//!
//! `keys.rs` hands every key press over while the editor is open; this
//! module owns the bindings so they sit next to the hint lines that list
//! them (`HINTS_*`). Letter keys accept both cases. An edit the editor
//! refuses leaves the rules unchanged and shows the reason on the message
//! line until the next key.
//!
//! Bindings (all columns): Tab / Shift+Tab change column, Esc goes back
//! (closing an open list, value box or column first).
//!
//! Rules column: Up/Down select (Down past the last rule selects the
//! "no rule matches" fallback row), Alt+Up/Down, Ctrl+Up/Down or [ / ]
//! move the rule, Enter edits it, A new rule from a template, D duplicate,
//! N rename, Space enable or disable, X or Delete delete, U or Ctrl+Z undo,
//! R reset to the defaults.
//!
//! Condition column: Up/Down select a condition, Left/Right step through
//! conditions, Enter or / choose one from the grouped list, A add,
//! X or Delete remove, N NOT, O AND/OR, E edit its value, I ignore case,
//! L screen region, then for the icon V type, G icon, C colour, B bold,
//! D dim, - / + speed, P ping-pong.
//!
//! Preview column: T or Left/Right switch Live and Demo, Up/Down scroll.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::picker::{Picker, PickerChoice};
use super::{DraftTarget, EditorColumn, EditorState, PreviewTab};
use crate::app::App;

/// Hint lines under the condition column, one set per focused column.
pub const HINTS_RULES: [&str; 3] = [
    "↑↓ select · Enter edit · A new · D duplicate · N rename",
    "Alt+↑↓ or [ ] move · Space on/off · X delete · U undo",
    "R reset · Tab column · Esc back",
];
pub const HINTS_CONDITION: [&str; 4] = [
    "↑↓ select · ←→ change · Enter list · A add · X remove",
    "N NOT · O AND/OR · E edit value · I case · L region",
    "V type · G icon · C colour · B bold · D dim",
    "−/+ speed · P ping-pong · Tab column · Esc back",
];
pub const HINTS_PREVIEW: [&str; 1] = ["T or ←→ Live/Demo · ↑↓ scroll · Esc back"];

/// Handle one key press with the editor taken out of `app`; stores the
/// editor back unless the key leaves it.
pub(crate) fn handle_key(
    app: &mut App,
    mut editor: EditorState,
    key: &KeyEvent,
    rules_height: u16,
) {
    if editor.confirm_reset {
        confirm_reset(app, &mut editor, key);
    } else if let Some(draft) = editor.parameter_draft.take() {
        edit_draft(app, &mut editor, draft, key);
    } else if editor.picker.is_some() {
        picker_key(app, &mut editor, key, rules_height);
    } else if key.code == KeyCode::Esc
        || (key.code == KeyCode::Backspace && editor.column == EditorColumn::Rules)
    {
        // Esc steps out one level: a focused column returns to the rules
        // list, and the list (or Backspace there, like ‹ Back) closes the
        // editor.
        if key.code == KeyCode::Backspace || editor.column == EditorColumn::Rules {
            return;
        }
        editor.column = EditorColumn::Rules;
    } else {
        command_key(app, &mut editor, key, rules_height);
    }
    app.custom_status_rules_editor = Some(editor);
}

fn confirm_reset(app: &mut App, editor: &mut EditorState, key: &KeyEvent) {
    match key.code {
        KeyCode::Char('y' | 'Y') | KeyCode::Enter => {
            let before = app.custom_status_rules.clone();
            let defaults = ilium_core::status_rule_expression::default_rules();
            match app.settings_save_custom_status_rules(defaults) {
                Ok(()) => {
                    let history = std::mem::take(&mut editor.history);
                    *editor = EditorState::new(&app.custom_status_rules);
                    editor.history = history;
                    editor.record_history(before);
                    editor.save_error = app.custom_status_rules_write_error.clone();
                }
                Err(error) => {
                    editor.confirm_reset = false;
                    editor.save_error = Some(error);
                }
            }
        }
        KeyCode::Esc | KeyCode::Char('n' | 'N') => editor.confirm_reset = false,
        _ => {}
    }
}

/// Typing into the value box (a pattern, a number, an icon, a colour or a
/// rule name). Characters are kept as typed: patterns are case-sensitive.
fn edit_draft(app: &mut App, editor: &mut EditorState, mut draft: String, key: &KeyEvent) {
    match key.code {
        KeyCode::Esc => editor.save_error = app.custom_status_rules_write_error.clone(),
        KeyCode::Enter => {
            let mut rules = app.custom_status_rules.clone();
            let edit = match editor.draft_target {
                DraftTarget::Condition => editor.set_parameter(&mut rules, &draft),
                DraftTarget::Visual => editor.set_visual_value(&mut rules, &draft),
                DraftTarget::Colour => editor.set_colour(&mut rules, &draft),
                DraftTarget::RuleName => editor.set_rule_name(&mut rules, &draft),
            };
            let before = app.custom_status_rules.clone();
            match edit.and_then(|()| app.settings_save_custom_status_rules(rules)) {
                Ok(()) => {
                    editor.save_error = app.custom_status_rules_write_error.clone();
                    editor.record_history(before);
                }
                Err(error) => {
                    editor.save_error = Some(error);
                    editor.parameter_draft = Some(draft);
                }
            }
        }
        KeyCode::Backspace => {
            draft.pop();
            editor.parameter_draft = Some(draft);
        }
        KeyCode::Char(character) if !character.is_control() && draft.chars().count() < 256 => {
            draft.push(character);
            editor.parameter_draft = Some(draft);
        }
        _ => editor.parameter_draft = Some(draft),
    }
}

/// Keys while the condition or template list is open.
fn picker_key(app: &mut App, editor: &mut EditorState, key: &KeyEvent, rules_height: u16) {
    let Some(picker) = editor.picker.as_mut() else {
        return;
    };
    match key.code {
        KeyCode::Esc => editor.picker = None,
        KeyCode::Up => picker.move_by(-1),
        KeyCode::Down => picker.move_by(1),
        KeyCode::PageUp => picker.move_by(-8),
        KeyCode::PageDown => picker.move_by(8),
        KeyCode::Backspace => picker.pop_char(),
        KeyCode::Enter => choose(app, editor, rules_height),
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            picker.push_char(character)
        }
        _ => {}
    }
}

/// Apply the open list's highlighted entry (also used by a click on it).
pub(crate) fn choose(app: &mut App, editor: &mut EditorState, rules_height: u16) {
    let adds_rule = matches!(
        editor.picker.as_ref().and_then(Picker::chosen),
        Some(PickerChoice::Template(_))
    );
    let mut previous = editor.clone();
    previous.picker = None;
    let mut rules = app.custom_status_rules.clone();
    match editor.choose_from_picker(&mut rules) {
        Ok(true) => {
            editor.save_rules(app, previous, rules);
            editor.keep_selected_visible(rules_height);
            if adds_rule && editor.save_error.is_none() {
                editor.column = EditorColumn::ConditionAndVisual;
            }
        }
        Ok(false) => {}
        Err(message) => editor.save_error = Some(message),
    }
}

/// Letter keys in either case; characters typed with Ctrl or Alt keep
/// their case so Ctrl+Z stays distinct.
fn normalized(key: &KeyEvent) -> KeyCode {
    match key.code {
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            KeyCode::Char(character.to_ascii_lowercase())
        }
        other => other,
    }
}

fn command_key(app: &mut App, editor: &mut EditorState, key: &KeyEvent, rules_height: u16) {
    let previous = editor.clone();
    editor.notice = None;
    let mut rules = app.custom_status_rules.clone();
    let outcome = match key.code {
        KeyCode::Tab => {
            editor.column = match editor.column {
                EditorColumn::Rules => EditorColumn::ConditionAndVisual,
                EditorColumn::ConditionAndVisual => EditorColumn::Preview,
                EditorColumn::Preview => EditorColumn::Rules,
            };
            Ok(false)
        }
        KeyCode::BackTab => {
            editor.column = match editor.column {
                EditorColumn::Rules => EditorColumn::Preview,
                EditorColumn::ConditionAndVisual => EditorColumn::Rules,
                EditorColumn::Preview => EditorColumn::ConditionAndVisual,
            };
            Ok(false)
        }
        _ => match editor.column {
            EditorColumn::Rules => rules_key(editor, &mut rules, key, rules_height),
            EditorColumn::ConditionAndVisual => condition_key(editor, &mut rules, key),
            EditorColumn::Preview => {
                preview_key(editor, key);
                Ok(false)
            }
        },
    };
    match outcome {
        Ok(true) => {
            editor.save_rules(app, previous, rules);
            editor.keep_selected_visible(rules_height);
        }
        // A key that changed nothing clears the previous refusal; a lasting
        // write problem stays visible.
        Ok(false) => {
            if editor.parameter_draft.is_none() {
                editor.save_error = app.custom_status_rules_write_error.clone();
            }
        }
        Err(message) => editor.save_error = Some(message),
    }
}

fn refuse_on_fallback(editor: &EditorState, what: &str) -> Result<(), String> {
    if editor.fallback_selected {
        Err(format!("The \"no rule matches\" row {what}"))
    } else {
        Ok(())
    }
}

fn rules_key(
    editor: &mut EditorState,
    rules: &mut ilium_core::status_rule_expression::StatusRules,
    key: &KeyEvent,
    rules_height: u16,
) -> Result<bool, String> {
    let count = rules.rules.len();
    let reorder = key
        .modifiers
        .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL);
    let direction = match normalized(key) {
        KeyCode::Up if reorder => Some(-1),
        KeyCode::Down if reorder => Some(1),
        KeyCode::Char('[') => Some(-1),
        KeyCode::Char(']') => Some(1),
        _ => None,
    };
    if let Some(direction) = direction {
        refuse_on_fallback(editor, "always stays last")?;
        return Ok(editor.move_selected(rules, direction));
    }
    match normalized(key) {
        KeyCode::Up => {
            if editor.fallback_selected {
                if count > 0 {
                    editor.select_rule(count - 1, count, rules_height);
                }
            } else if let Some(index) = editor.selected_rule {
                editor.select_rule(index.saturating_sub(1), count, rules_height);
            }
            Ok(false)
        }
        KeyCode::Down => {
            match editor.selected_rule {
                _ if editor.fallback_selected => {}
                Some(index) if index + 1 < count => {
                    editor.select_rule(index + 1, count, rules_height)
                }
                _ => editor.select_fallback(),
            }
            Ok(false)
        }
        KeyCode::Enter => {
            editor.column = EditorColumn::ConditionAndVisual;
            Ok(false)
        }
        KeyCode::Char('z') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Ok(editor.undo(rules))
        }
        KeyCode::Char('u') => Ok(editor.undo(rules)),
        KeyCode::Char('a') => {
            editor.picker = Some(Picker::templates());
            Ok(false)
        }
        KeyCode::Char('d') => {
            refuse_on_fallback(editor, "cannot be duplicated")?;
            if editor.duplicate_selected(rules) {
                Ok(true)
            } else {
                Err(rule_limit_message(count))
            }
        }
        KeyCode::Char('n') => {
            refuse_on_fallback(editor, "has no name")?;
            editor.draft_target = DraftTarget::RuleName;
            editor.parameter_draft = editor
                .selected_rule
                .and_then(|index| rules.rules.get(index))
                .map(|rule| rule.name.clone());
            Ok(false)
        }
        KeyCode::Char('r') => {
            editor.confirm_reset = true;
            Ok(false)
        }
        KeyCode::Char(' ') => {
            refuse_on_fallback(editor, "is always on")?;
            Ok(editor.toggle_selected(rules))
        }
        KeyCode::Delete | KeyCode::Char('x') => {
            refuse_on_fallback(editor, "cannot be deleted; set its icon to Hidden instead")?;
            Ok(editor.delete_selected(rules))
        }
        _ => Ok(false),
    }
}

fn rule_limit_message(count: usize) -> String {
    if count >= ilium_core::status_rule_expression::MAXIMUM_RULES {
        format!(
            "At most {} rules",
            ilium_core::status_rule_expression::MAXIMUM_RULES
        )
    } else {
        "Select a rule first".to_owned()
    }
}

fn condition_key(
    editor: &mut EditorState,
    rules: &mut ilium_core::status_rule_expression::StatusRules,
    key: &KeyEvent,
) -> Result<bool, String> {
    let changed = |result: Result<(), &'static str>| result.map(|()| true).map_err(str::to_owned);
    match normalized(key) {
        KeyCode::Up => {
            editor.selected_condition = editor.selected_condition.saturating_sub(1);
            Ok(false)
        }
        KeyCode::Down => {
            let count = editor.expression(rules).map_or(0, |(terms, _)| terms.len());
            editor.selected_condition =
                (editor.selected_condition + 1).min(count.saturating_sub(1));
            Ok(false)
        }
        KeyCode::Left => changed(editor.cycle_condition(rules, -1)),
        KeyCode::Right => changed(editor.cycle_condition(rules, 1)),
        KeyCode::Enter | KeyCode::Char('/') => {
            editor.open_condition_picker(rules)?;
            Ok(false)
        }
        KeyCode::Char('a') => changed(editor.add_condition(rules)),
        KeyCode::Delete | KeyCode::Char('x') => changed(editor.remove_condition(rules)),
        KeyCode::Char('n') => changed(editor.negate_condition(rules)),
        KeyCode::Char('o') => changed(editor.toggle_connector(rules)),
        KeyCode::Char('e') => {
            editor.expression(rules)?;
            match editor.parameter(rules) {
                Some(value) => {
                    editor.draft_target = DraftTarget::Condition;
                    editor.parameter_draft = Some(value);
                }
                // Nothing to type for this condition: offer the list.
                None => editor.open_condition_picker(rules)?,
            }
            Ok(false)
        }
        KeyCode::Char('i') => changed(editor.toggle_screen_case(rules)),
        KeyCode::Char('l') => changed(editor.cycle_screen_region(rules)),
        KeyCode::Char('v') => visual_edit(editor.cycle_visual(rules)),
        KeyCode::Char('g') => {
            ensure_visual_target(editor, rules)?;
            let value = editor
                .visual_value(rules)
                .ok_or("Hidden shows no icon: press V to choose an icon type first")?;
            editor.draft_target = DraftTarget::Visual;
            editor.parameter_draft = Some(value);
            Ok(false)
        }
        KeyCode::Char('c') => {
            ensure_visual_target(editor, rules)?;
            editor.draft_target = DraftTarget::Colour;
            editor.parameter_draft = Some(
                editor
                    .visual(rules)
                    .and_then(|visual| visual.style.color.clone())
                    .unwrap_or_default(),
            );
            Ok(false)
        }
        KeyCode::Char('b') => visual_edit(editor.toggle_weight(rules, true)),
        KeyCode::Char('d') => visual_edit(editor.toggle_weight(rules, false)),
        KeyCode::Char('+') => Ok(editor.adjust_animation_speed(rules, 1)),
        KeyCode::Char('-') => Ok(editor.adjust_animation_speed(rules, -1)),
        KeyCode::Char('p') => Ok(editor.toggle_ping_pong(rules)),
        _ => Ok(false),
    }
}

fn ensure_visual_target(
    editor: &EditorState,
    rules: &ilium_core::status_rule_expression::StatusRules,
) -> Result<(), String> {
    if editor.fallback_selected || editor.rule_index(rules).is_some() {
        Ok(())
    } else {
        Err("Select a rule first".to_owned())
    }
}

fn visual_edit(changed: bool) -> Result<bool, String> {
    if changed {
        Ok(true)
    } else {
        Err("Select a rule first".to_owned())
    }
}

fn preview_key(editor: &mut EditorState, key: &KeyEvent) {
    match normalized(key) {
        KeyCode::Char('t') | KeyCode::Left | KeyCode::Right => {
            editor.preview_tab = match editor.preview_tab {
                PreviewTab::Live => PreviewTab::Demo,
                PreviewTab::Demo => PreviewTab::Live,
            };
        }
        KeyCode::Up if editor.preview_tab == PreviewTab::Demo => editor.scroll_demo(-1),
        KeyCode::Down if editor.preview_tab == PreviewTab::Demo => editor.scroll_demo(1),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::handle_key;
    use crate::app::App;
    use crate::status_rules_editor::{EditorColumn, EditorState};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ilium_core::status_rule_expression::{default_rules, Condition, VisualKind};

    fn editor_app(config: Option<&std::path::Path>) -> App {
        let mut app = App::new("editor-keys".into(), std::env::temp_dir());
        app.config_dir = config.map(std::path::Path::to_path_buf);
        app.custom_status_rules = default_rules();
        app.custom_status_rules_editor = Some(EditorState::new(&app.custom_status_rules));
        app
    }

    fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
        let editor = app.custom_status_rules_editor.take().expect("editor open");
        handle_key(app, editor, &KeyEvent::new(code, modifiers), 40);
    }

    fn editor(app: &App) -> &EditorState {
        app.custom_status_rules_editor
            .as_ref()
            .expect("editor open")
    }

    #[test]
    fn down_past_the_last_rule_selects_the_fallback_which_edits_only_its_icon() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = editor_app(Some(directory.path()));
        let count = app.custom_status_rules.rules.len();
        for _ in 0..count + 3 {
            press(&mut app, KeyCode::Down, KeyModifiers::NONE);
        }
        assert!(editor(&app).fallback_selected);
        let before = app.custom_status_rules.rules.clone();
        press(&mut app, KeyCode::Delete, KeyModifiers::NONE);
        assert_eq!(
            app.custom_status_rules.rules, before,
            "fallback is not deleted"
        );
        assert!(editor(&app)
            .save_error
            .as_deref()
            .is_some_and(|message| message.contains("cannot be deleted")));
        press(&mut app, KeyCode::Char(']'), KeyModifiers::NONE);
        assert_eq!(
            app.custom_status_rules.rules, before,
            "fallback never moves"
        );
        press(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        press(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);
        let fallback = app
            .custom_status_rules
            .fallback
            .clone()
            .expect("fallback set");
        assert_ne!(fallback.kind, VisualKind::Hidden);
        assert_eq!(app.custom_status_rules.rules, before, "rules untouched");
        press(&mut app, KeyCode::Char('n'), KeyModifiers::NONE);
        assert!(
            editor(&app).save_error.is_some(),
            "NOT is refused with a reason"
        );
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        press(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);
        assert_eq!(app.custom_status_rules.fallback, None, "undo restores it");
        press(&mut app, KeyCode::Up, KeyModifiers::NONE);
        assert!(!editor(&app).fallback_selected);
        assert_eq!(editor(&app).selected_rule, Some(count - 1));
    }

    #[test]
    fn deleting_a_rule_shows_an_undo_notice_until_the_next_key() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = editor_app(Some(directory.path()));
        let count = app.custom_status_rules.rules.len();
        press(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(app.custom_status_rules.rules.len(), count - 1);
        assert!(editor(&app)
            .notice
            .as_deref()
            .is_some_and(|notice| notice.starts_with("Deleted \"") && notice.contains("undoes")));
        press(&mut app, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(editor(&app).notice, None, "the next key clears the notice");
        press(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
        press(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);
        assert_eq!(
            app.custom_status_rules.rules.len(),
            count - 1,
            "two deletes, one undo"
        );
    }

    #[test]
    fn new_rule_comes_from_a_searchable_template_list() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = editor_app(Some(directory.path()));
        let count = app.custom_status_rules.rules.len();
        press(&mut app, KeyCode::Char('A'), KeyModifiers::SHIFT);
        assert!(editor(&app).picker.is_some(), "A opens the template list");
        for character in "paused".chars() {
            press(&mut app, KeyCode::Char(character), KeyModifiers::NONE);
        }
        assert_eq!(editor(&app).picker.as_ref().unwrap().query, "paused");
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(editor(&app).picker.is_none());
        assert_eq!(app.custom_status_rules.rules.len(), count + 1);
        let added = app.custom_status_rules.rules.last().unwrap();
        assert_eq!(added.name, "Goal paused");
        assert_eq!(editor(&app).selected_rule, Some(count));
        assert_eq!(editor(&app).column, EditorColumn::ConditionAndVisual);
        // Esc closes an open list without changing anything.
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        press(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(editor(&app).picker.is_none());
        assert_eq!(app.custom_status_rules.rules.len(), count + 1);
    }

    #[test]
    fn reorder_keys_and_uppercase_letters_work_like_the_lowercase_ones() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = editor_app(Some(directory.path()));
        let first = app.custom_status_rules.rules[0].id.clone();
        press(&mut app, KeyCode::Down, KeyModifiers::ALT);
        assert_eq!(app.custom_status_rules.rules[1].id, first);
        press(&mut app, KeyCode::Char(']'), KeyModifiers::NONE);
        assert_eq!(app.custom_status_rules.rules[2].id, first);
        press(&mut app, KeyCode::Char('['), KeyModifiers::NONE);
        press(&mut app, KeyCode::Up, KeyModifiers::ALT);
        assert_eq!(app.custom_status_rules.rules[0].id, first);
        let count = app.custom_status_rules.rules.len();
        press(&mut app, KeyCode::Char('D'), KeyModifiers::SHIFT);
        assert_eq!(app.custom_status_rules.rules.len(), count + 1);
        press(&mut app, KeyCode::Char('U'), KeyModifiers::SHIFT);
        assert_eq!(app.custom_status_rules.rules.len(), count);
    }

    #[test]
    fn refused_condition_edits_say_why_and_keep_the_rule() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = editor_app(Some(directory.path()));
        press(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);
        let blank = crate::status_rules_editor::templates::templates().len() - 1;
        for _ in 0..blank {
            press(&mut app, KeyCode::Down, KeyModifiers::NONE);
        }
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        let rules = app.custom_status_rules.clone();
        press(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(app.custom_status_rules, rules);
        assert_eq!(
            editor(&app).save_error.as_deref(),
            Some("A rule needs at least one condition")
        );
        press(&mut app, KeyCode::Char('i'), KeyModifiers::NONE);
        assert_eq!(
            editor(&app).save_error.as_deref(),
            Some("Select a screen pattern")
        );
        press(&mut app, KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(editor(&app).save_error, None, "the next key clears it");
    }

    #[test]
    fn enter_on_a_condition_picks_a_replacement_from_the_grouped_list() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = editor_app(Some(directory.path()));
        press(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        let picker = editor(&app).picker.clone().expect("condition list opens");
        assert_eq!(picker.title, " Choose a condition ");
        for character in "progress is at least".chars() {
            press(&mut app, KeyCode::Char(character), KeyModifiers::NONE);
        }
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        let rule = &app.custom_status_rules.rules[0];
        let (terms, _) = ilium_core::status_rule_expression::to_line(&rule.any_of).unwrap();
        assert!(
            matches!(terms[0].condition, Condition::ProgressAtLeast(_)),
            "{:?}",
            terms[0].condition
        );
    }

    #[test]
    fn without_a_configuration_folder_edits_apply_and_say_they_are_not_saved() {
        let mut app = editor_app(None);
        let enabled = app.custom_status_rules.rules[0].enabled;
        press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        assert_eq!(
            app.custom_status_rules.rules[0].enabled, !enabled,
            "applied"
        );
        assert_eq!(
            editor(&app).save_error.as_deref(),
            Some(crate::status_rules_save::STATUS_RULES_SESSION_ONLY)
        );
        press(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);
        assert_eq!(
            app.custom_status_rules.rules[0].enabled, enabled,
            "undo works"
        );
    }
}
