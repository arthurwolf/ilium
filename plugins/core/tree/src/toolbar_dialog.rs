//! The tree toolbar editor: which buttons the bottom row of the tree shows, in which order, and
//! after how many idle minutes the activity filter hides a pane.
//!
//! The Settings "Toolbar" page asks for this dialog by running the `tree-toolbar` command with
//! `{"entries": [{"key", "label", "glyph"}], "catalogue": [...], "defaults": ["key", ...],
//! "minutes": n}`: the buttons in use now, every button that exists (built-ins and one per
//! custom agent), the keys of the default layout and the inactivity threshold. The labels and
//! glyphs are kernel facts (icon settings, custom agent profiles); this module holds the rules and
//! the popup, and `lib.rs` connects it to events and intents. Every change is sent to the kernel
//! at once as `kernel.set-tree-toolbar`, which saves it and redraws the tree behind the dialog.
//! Cancel puts back what was in use when the dialog opened; Done and Escape keep what is shown.

use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, ButtonRole, ChoiceSpec, ChoiceStyle, Constraint, DialogKind,
    DialogSpec, DisabledMarker, UiNode, UiTree, ViewTarget, Widget,
};
use serde_json::Value;

use crate::new_group::{button, node, span};
use crate::toolbar_rows;

/// Local id of the command that opens the dialog.
pub const COMMAND: &str = "tree-toolbar";
/// Popup id of the dialog.
pub const POPUP: &str = "tree-toolbar";
/// Key of the list of buttons in use.
pub const BUTTONS_KEY: &str = "buttons";
/// Key of the list of buttons that can still be added.
pub const ADD_KEY: &str = "add";
/// The kernel command that sets the toolbar: `{"entries": ["key", ...], "minutes": n}`.
pub const SET_TOOLBAR_COMMAND: &str = "kernel.set-tree-toolbar";
/// Title of the dialog.
pub const TITLE: &str = "Tree toolbar";
/// What the lists show when they hold nothing.
const NO_BUTTONS: &str = "(no buttons)";
const NO_MORE_BUTTONS: &str = "(every button is already on the toolbar)";

/// The popup view of the dialog.
pub fn view() -> ViewTarget {
    ViewTarget::Popup(POPUP.to_owned())
}

/// One toolbar button as the kernel names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Stable persisted key (`search`, `claude`, `custom:<profile id>`, ...).
    pub key: String,
    /// Glyph and name shown in the lists.
    pub label: String,
    /// The glyph alone, for the preview row.
    pub glyph: String,
}

impl Entry {
    fn from_value(value: &Value) -> Option<Self> {
        let text = |name: &str| value.get(name)?.as_str().map(str::to_owned);
        let key = text("key").filter(|key| !key.is_empty())?;
        Some(Self {
            key,
            label: text("label")?,
            glyph: text("glyph")?,
        })
    }
}

/// What the toolbar is set to: the keys of the buttons in order, and the inactivity minutes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting {
    pub keys: Vec<String>,
    pub minutes: u32,
}

/// What the command asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub entries: Vec<Entry>,
    pub catalogue: Vec<Entry>,
    pub defaults: Vec<String>,
    pub minutes: u32,
}

impl Request {
    /// Reads the command arguments; `None` for anything malformed, so nothing opens.
    pub fn from_arguments(arguments_json: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(arguments_json).ok()?;
        let entries = |name: &str| -> Option<Vec<Entry>> {
            value
                .get(name)?
                .as_array()?
                .iter()
                .map(Entry::from_value)
                .collect()
        };
        let defaults = value
            .get("defaults")?
            .as_array()?
            .iter()
            .map(|key| key.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()?;
        let minutes = u32::try_from(value.get("minutes")?.as_u64()?).ok()?;
        Some(Self {
            entries: entries("entries")?,
            catalogue: entries("catalogue")?,
            defaults,
            minutes: minutes.clamp(
                toolbar_rows::MINIMUM_INACTIVITY_MINUTES,
                toolbar_rows::MAXIMUM_INACTIVITY_MINUTES,
            ),
        })
    }
}

/// The arguments of `kernel.set-tree-toolbar` for one setting.
pub fn set_toolbar_arguments(setting: &Setting) -> String {
    serde_json::json!({ "entries": setting.keys, "minutes": setting.minutes }).to_string()
}

/// What an action did to the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Nothing for the kernel; the dialog stays open.
    Stay,
    /// The toolbar changed: send it to the kernel. The dialog stays open.
    Set(Setting),
    /// Finished; what is applied stays.
    Done,
    /// Cancelled: send the setting back that was in use when the dialog opened (`None` when
    /// nothing changed since) and close.
    Cancel(Option<Setting>),
}

/// The open dialog.
#[derive(Debug, Clone)]
pub struct Dialog {
    original: Setting,
    entries: Vec<Entry>,
    catalogue: Vec<Entry>,
    defaults: Vec<String>,
    minutes: u32,
    selected: usize,
    add_selected: usize,
}

impl Dialog {
    /// Opens the dialog on the toolbar in use, with the first button selected.
    pub fn new(request: Request) -> Self {
        let original = Setting {
            keys: request
                .entries
                .iter()
                .map(|entry| entry.key.clone())
                .collect(),
            minutes: request.minutes,
        };
        Self {
            original,
            entries: request.entries,
            catalogue: request.catalogue,
            defaults: request.defaults,
            minutes: request.minutes,
            selected: 0,
            add_selected: 0,
        }
    }

    /// The setting the dialog shows now.
    pub fn setting(&self) -> Setting {
        Setting {
            keys: self.entries.iter().map(|entry| entry.key.clone()).collect(),
            minutes: self.minutes,
        }
    }

    /// The index of the selected button.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The buttons that can still be added: the catalogue without those in use.
    pub fn available(&self) -> Vec<&Entry> {
        self.catalogue
            .iter()
            .filter(|candidate| !self.entries.iter().any(|entry| entry.key == candidate.key))
            .collect()
    }

    fn changed(&self) -> Step {
        Step::Set(self.setting())
    }

    /// Applies one action the kernel reported for the popup. An out-of-range choice index is
    /// ignored; the kernel closes the popup itself on Escape (a `Closed` action), which keeps
    /// the toolbar.
    pub fn act(
        &mut self,
        key: &str,
        action_id: &str,
        kind: ActionKind,
        value: Option<ActionValue>,
    ) -> Step {
        match (kind, value) {
            (ActionKind::Closed, _) => Step::Done,
            (ActionKind::Changed | ActionKind::Selected, Some(ActionValue::Index(index))) => {
                let Ok(index) = usize::try_from(index) else {
                    return Step::Stay;
                };
                if key == BUTTONS_KEY && index < self.entries.len() {
                    self.selected = index;
                } else if key == ADD_KEY && index < self.available().len() {
                    self.add_selected = index;
                }
                Step::Stay
            }
            (ActionKind::Activated, _) => match action_id {
                "done" => Step::Done,
                "cancel" => {
                    Step::Cancel((self.setting() != self.original).then(|| self.original.clone()))
                }
                "up" => self.move_selected(true),
                "down" => self.move_selected(false),
                "remove" => self.remove_selected(),
                "add" => self.add_chosen(),
                "reset" => self.reset(),
                "shorter" => self.step_minutes(false),
                "longer" => self.step_minutes(true),
                _ => Step::Stay,
            },
            _ => Step::Stay,
        }
    }

    fn move_selected(&mut self, up: bool) -> Step {
        let next = toolbar_rows::next_selection(self.selected, up, self.entries.len());
        if !toolbar_rows::reorders(self.selected, next, self.entries.len()) || next == self.selected
        {
            return Step::Stay;
        }
        self.entries.swap(self.selected, next);
        self.selected = next;
        self.changed()
    }

    fn remove_selected(&mut self) -> Step {
        if self.selected >= self.entries.len() {
            return Step::Stay;
        }
        self.entries.remove(self.selected);
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
        self.add_selected = self
            .add_selected
            .min(self.available().len().saturating_sub(1));
        self.changed()
    }

    fn add_chosen(&mut self) -> Step {
        let Some(entry) = self
            .available()
            .get(self.add_selected)
            .map(|entry| (*entry).clone())
        else {
            return Step::Stay;
        };
        self.entries.push(entry);
        self.selected = self.entries.len() - 1;
        self.add_selected = self
            .add_selected
            .min(self.available().len().saturating_sub(1));
        self.changed()
    }

    fn reset(&mut self) -> Step {
        let defaults: Vec<Entry> = self
            .defaults
            .iter()
            .filter_map(|key| self.catalogue.iter().find(|entry| &entry.key == key))
            .cloned()
            .collect();
        if defaults == self.entries {
            return Step::Stay;
        }
        self.entries = defaults;
        self.selected = toolbar_rows::clamp_selection(self.selected, self.entries.len())
            .min(self.entries.len().saturating_sub(1));
        self.add_selected = 0;
        self.changed()
    }

    fn step_minutes(&mut self, increase: bool) -> Step {
        let minutes = toolbar_rows::step_inactivity(self.minutes, increase);
        if minutes == self.minutes {
            return Step::Stay;
        }
        self.minutes = minutes;
        self.changed()
    }

    /// The popup view.
    pub fn render(&self) -> UiTree {
        let preview: Vec<&str> = self
            .entries
            .iter()
            .map(|entry| entry.glyph.trim())
            .collect();
        let message = vec![
            vec![span(
                "Buttons run left to right along the bottom of the tree.",
            )],
            vec![span(&format!("Preview · {}", preview.join("  ")))],
            vec![span(&format!(
                "Inactive after {} minutes (what the eye button hides)",
                self.minutes
            ))],
        ];
        let dialog = DialogSpec {
            title: vec![span(TITLE)],
            kind: DialogKind::Plain,
            buttons: vec![
                button("done", "Done", ButtonRole::Primary, false),
                button("cancel", "Cancel", ButtonRole::Secondary, true),
                button("up", "Up", ButtonRole::Secondary, false),
                button("down", "Down", ButtonRole::Secondary, false),
                button("remove", "Remove", ButtonRole::Secondary, false),
                button("add", "Add", ButtonRole::Secondary, false),
                button("shorter", "Shorter", ButtonRole::Secondary, false),
                button("longer", "Longer", ButtonRole::Secondary, false),
                button("reset", "Default", ButtonRole::Secondary, false),
            ],
            width: Constraint::Length(70),
            height: Constraint::Min(0),
            message: Some(message),
        };
        let buttons = choice(
            "Buttons in use (Up and Down move the selected button)",
            self.entries
                .iter()
                .map(|entry| entry.label.clone())
                .collect(),
            NO_BUTTONS,
            self.selected,
        );
        let available = choice(
            "Buttons to add",
            self.available()
                .iter()
                .map(|entry| entry.label.clone())
                .collect(),
            NO_MORE_BUTTONS,
            self.add_selected,
        );
        UiTree {
            version: 1,
            nodes: vec![
                UiNode {
                    children: vec![1, 2],
                    ..node("dialog", Widget::Dialog(dialog))
                },
                node(BUTTONS_KEY, Widget::Choice(buttons)),
                node(ADD_KEY, Widget::Choice(available)),
            ],
            root: 0,
        }
    }
}

/// A list of options; an empty list shows `placeholder` as its only row.
fn choice(label: &str, mut options: Vec<String>, placeholder: &str, selected: usize) -> ChoiceSpec {
    if options.is_empty() {
        options.push(placeholder.to_owned());
    }
    ChoiceSpec {
        label: label.to_owned(),
        options,
        selected: u32::try_from(selected).unwrap_or(0),
        style: ChoiceStyle::List,
        items: Vec::new(),
        source: None,
        selected_id: None,
        filter: false,
        view_modes: Vec::new(),
        disabled_marker: DisabledMarker::Reason,
        allow_custom: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str) -> Entry {
        Entry {
            key: key.to_owned(),
            label: format!("{key} label"),
            glyph: format!("{key}-glyph"),
        }
    }

    fn request() -> Request {
        let keys = ["search", "shell", "claude", "settings"];
        Request {
            entries: keys.iter().map(|key| entry(key)).collect(),
            catalogue: [
                "search",
                "shell",
                "claude",
                "codex",
                "settings",
                "custom:mine",
            ]
            .iter()
            .map(|key| entry(key))
            .collect(),
            defaults: ["search", "shell", "claude", "codex", "settings"]
                .iter()
                .map(|key| (*key).to_owned())
                .collect(),
            minutes: 15,
        }
    }

    fn dialog() -> Dialog {
        Dialog::new(request())
    }

    fn press(dialog: &mut Dialog, id: &str) -> Step {
        dialog.act("dialog", id, ActionKind::Activated, None)
    }

    fn choose(dialog: &mut Dialog, key: &str, index: u32) -> Step {
        dialog.act(
            key,
            "",
            ActionKind::Changed,
            Some(ActionValue::Index(index)),
        )
    }

    fn keys(step: Step) -> Vec<String> {
        let Step::Set(setting) = step else {
            panic!("expected a change, got {step:?}");
        };
        setting.keys
    }

    fn arguments() -> String {
        serde_json::json!({
            "entries": [{"key": "search", "label": "S search", "glyph": "S"}],
            "catalogue": [
                {"key": "search", "label": "S search", "glyph": "S"},
                {"key": "custom:mine", "label": "M My agent", "glyph": "M"},
            ],
            "defaults": ["search"],
            "minutes": 15,
        })
        .to_string()
    }

    #[test]
    fn arguments_carry_the_buttons_the_catalogue_and_the_minutes() {
        let request = Request::from_arguments(&arguments()).expect("valid arguments");
        assert_eq!(request.entries.len(), 1);
        assert_eq!(request.catalogue[1].key, "custom:mine");
        assert_eq!(request.defaults, ["search"]);
        assert_eq!(request.minutes, 15);
        for bad in [
            "",
            "{}",
            "[]",
            r#"{"entries": [], "catalogue": [], "defaults": []}"#,
            r#"{"entries": [{"key": ""}], "catalogue": [], "defaults": [], "minutes": 5}"#,
            r#"{"entries": [], "catalogue": [], "defaults": [3], "minutes": 5}"#,
        ] {
            assert_eq!(Request::from_arguments(bad), None, "{bad}");
        }
    }

    #[test]
    fn minutes_in_the_arguments_are_kept_within_one_to_1440() {
        let low = arguments().replace("\"minutes\":15", "\"minutes\":0");
        assert_eq!(Request::from_arguments(&low).map(|r| r.minutes), Some(1));
        let high = arguments().replace("\"minutes\":15", "\"minutes\":99999");
        assert_eq!(
            Request::from_arguments(&high).map(|r| r.minutes),
            Some(1440)
        );
    }

    #[test]
    fn set_toolbar_arguments_name_the_keys_and_the_minutes() {
        let value: Value = serde_json::from_str(&set_toolbar_arguments(&dialog().setting()))
            .expect("arguments are JSON");
        assert_eq!(
            value["entries"],
            serde_json::json!(["search", "shell", "claude", "settings"])
        );
        assert_eq!(value["minutes"], 15);
    }

    #[test]
    fn remove_then_add_puts_the_button_last_and_selects_it() {
        let mut dialog = dialog();
        assert_eq!(
            keys(press(&mut dialog, "remove")),
            ["shell", "claude", "settings"]
        );
        // The removed button is offered again, in catalogue order: search, codex, custom.
        let offered: Vec<&str> = dialog
            .available()
            .iter()
            .map(|entry| entry.key.as_str())
            .collect();
        assert_eq!(offered, ["search", "codex", "custom:mine"]);
        assert_eq!(
            keys(press(&mut dialog, "add")),
            ["shell", "claude", "settings", "search"]
        );
        assert_eq!(dialog.selected(), 3);
    }

    #[test]
    fn up_and_down_move_the_selected_button_and_follow_it() {
        let mut dialog = dialog();
        choose(&mut dialog, BUTTONS_KEY, 2);
        assert_eq!(
            keys(press(&mut dialog, "up")),
            ["search", "claude", "shell", "settings"]
        );
        assert_eq!(dialog.selected(), 1);
        let mut top = self::dialog();
        assert_eq!(press(&mut top, "up"), Step::Stay);
        let mut bottom = self::dialog();
        choose(&mut bottom, BUTTONS_KEY, 3);
        assert_eq!(press(&mut bottom, "down"), Step::Stay);
    }

    #[test]
    fn a_choice_index_past_the_lists_is_ignored() {
        let mut dialog = dialog();
        assert_eq!(choose(&mut dialog, BUTTONS_KEY, 9), Step::Stay);
        assert_eq!(dialog.selected(), 0);
        assert_eq!(choose(&mut dialog, ADD_KEY, 9), Step::Stay);
    }

    #[test]
    fn adding_with_nothing_available_and_removing_from_nothing_change_nothing() {
        let mut full = Dialog::new(Request {
            entries: request().catalogue,
            ..request()
        });
        assert_eq!(press(&mut full, "add"), Step::Stay);
        let mut empty = Dialog::new(Request {
            entries: Vec::new(),
            ..request()
        });
        assert_eq!(press(&mut empty, "remove"), Step::Stay);
        assert_eq!(press(&mut empty, "up"), Step::Stay);
    }

    #[test]
    fn default_restores_the_default_buttons_and_keeps_the_minutes() {
        let mut dialog = dialog();
        assert_eq!(
            keys(press(&mut dialog, "reset")),
            ["search", "shell", "claude", "codex", "settings"]
        );
        assert_eq!(dialog.setting().minutes, 15);
        assert_eq!(press(&mut dialog, "reset"), Step::Stay);
    }

    #[test]
    fn shorter_and_longer_step_the_minutes_within_one_to_1440() {
        let mut dialog = dialog();
        let Step::Set(longer) = press(&mut dialog, "longer") else {
            panic!("longer changes the minutes");
        };
        assert_eq!(longer.minutes, 16);
        let Step::Set(shorter) = press(&mut dialog, "shorter") else {
            panic!("shorter changes the minutes");
        };
        assert_eq!(shorter.minutes, 15);
        let mut one = Dialog::new(Request {
            minutes: 1,
            ..request()
        });
        assert_eq!(press(&mut one, "shorter"), Step::Stay);
        let mut most = Dialog::new(Request {
            minutes: 1440,
            ..request()
        });
        assert_eq!(press(&mut most, "longer"), Step::Stay);
    }

    #[test]
    fn cancel_restores_only_what_changed_and_escape_keeps_it() {
        let mut untouched = dialog();
        assert_eq!(press(&mut untouched, "cancel"), Step::Cancel(None));
        let mut dialog = dialog();
        press(&mut dialog, "remove");
        press(&mut dialog, "longer");
        let Step::Cancel(Some(restore)) = press(&mut dialog, "cancel") else {
            panic!("a changed toolbar is restored");
        };
        assert_eq!(restore.keys, ["search", "shell", "claude", "settings"]);
        assert_eq!(restore.minutes, 15);
        assert_eq!(dialog.act("", "", ActionKind::Closed, None), Step::Done);
        assert_eq!(press(&mut dialog, "done"), Step::Done);
    }

    #[test]
    fn the_popup_shows_both_lists_and_a_placeholder_for_an_empty_one() {
        let tree = dialog().render();
        assert_eq!(tree.nodes.len(), 3);
        let Widget::Choice(buttons) = &tree.nodes[1].widget else {
            panic!("the buttons list is a choice");
        };
        assert_eq!(buttons.options.len(), 4);
        let Widget::Choice(add) = &tree.nodes[2].widget else {
            panic!("the add list is a choice");
        };
        assert_eq!(add.options.len(), 2);
        let empty = Dialog::new(Request {
            entries: Vec::new(),
            catalogue: Vec::new(),
            ..request()
        })
        .render();
        let Widget::Choice(buttons) = &empty.nodes[1].widget else {
            panic!("the buttons list is a choice");
        };
        assert_eq!(buttons.options, [NO_BUTTONS]);
    }
}
