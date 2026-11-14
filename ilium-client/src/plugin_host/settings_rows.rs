//! Editable settings rows of the plugins (contract section 20.6, manual `reference-settings-rows.md`).
//!
//! A plugin contributes `contributes.settings` rows in its manifest. This module turns them
//! into a row model the Settings plugin list draws and edits: a switch for a `toggle`, a
//! cycling choice for a `choice`, a clamped stepper for a `number`. Every edit is checked against
//! the compiled schema of the row and written through the shared [`PluginSettingsStore`], so a
//! rejected value never reaches `config.toml` and the row shows the refusal text.
//!
//! The model is pure: it takes the manifest text and the stored values and returns values and
//! patches. The store write is the only I/O, and it is one call ([`SettingRow::commit`]). A row
//! is drawn with the shared [`ValueControl`], which also answers the hit test, so what is drawn
//! and what is clickable are the same rectangles.

use ilium_plugin_host::config_settings::PluginSettingsStore;
use ilium_plugin_host::settings::{manifest_rows, SettingsRow};
use ratatui::layout::Rect;
use serde_json::{Map, Value};

use crate::value_control::{boolean_switch, ControlKind, ControlSpec, ValueControl};

/// Which way a stepper or a choice moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// To the previous option or the next lower value.
    Previous,
    /// To the next option or the next higher value.
    Next,
}

/// One value row of a plugin, with its current value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SettingRow {
    /// The plugin that contributes the row.
    pub plugin: String,
    /// The key of the value inside `[plugins."<plugin>"]`.
    pub key: String,
    /// The label of the row.
    pub title: String,
    /// The description line, when the manifest gives one.
    pub description: Option<String>,
    /// The settings page the row sits on (the tab and, when there is one, the section).
    pub page: String,
    /// The row type of the manifest.
    pub kind: String,
    /// The value in effect: the stored one, else the default.
    pub value: Value,
    /// The default of the manifest.
    pub default: Value,
    /// The compiled schema of the row.
    pub schema: Value,
    /// The refusal text of the last edit, shown under the row until the next edit.
    pub validation: Option<String>,
}

/// The editable rows of one plugin manifest, with the stored values over the defaults.
pub(crate) fn rows_for(
    plugin: &str,
    manifest: &str,
    stored: &Map<String, Value>,
) -> Vec<SettingRow> {
    let Ok(manifest) = serde_json::from_str::<Value>(manifest) else {
        return Vec::new();
    };
    manifest_rows(&manifest)
        .into_iter()
        .map(|row| SettingRow::new(plugin, row, stored))
        .collect()
}

impl SettingRow {
    fn new(plugin: &str, row: SettingsRow, stored: &Map<String, Value>) -> Self {
        let value = stored
            .get(&row.key)
            .cloned()
            .or_else(|| (!row.default.is_null()).then(|| row.default.clone()))
            .unwrap_or(Value::Null);
        let page = match &row.section {
            Some(section) => format!("{} / {section}", row.tab.trim_start_matches("self:")),
            None => row.tab.trim_start_matches("self:").to_owned(),
        };
        Self {
            plugin: plugin.to_owned(),
            key: row.key,
            title: row.title,
            description: row.description,
            page,
            kind: row.kind,
            value,
            default: row.default,
            schema: row.schema,
            validation: None,
        }
    }

    /// Whether the Settings list edits this row type with its own control. The other types
    /// (`list`, `map`, `multi-choice`, `location`, `keybinding`, `text-area`, `path`, `color`
    /// and free text) show their value and are edited in the plugin's own view or `config.toml`.
    pub(crate) fn is_editable(&self) -> bool {
        match self.kind.as_str() {
            "toggle" | "number" => true,
            "choice" => self.options().is_some(),
            _ => false,
        }
    }

    /// The fixed options of a `choice`: each value with its title.
    fn options(&self) -> Option<Vec<(Value, String)>> {
        let values = self.schema.get("enum")?.as_array()?;
        let titles = self
            .schema
            .get("enum_titles")
            .and_then(Value::as_array)
            .filter(|titles| titles.len() == values.len());
        Some(
            values
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    let title = titles
                        .and_then(|titles| titles[index].as_str().map(str::to_owned))
                        .unwrap_or_else(|| plain(value));
                    (value.clone(), title)
                })
                .collect(),
        )
    }

    fn number(&self, field: &str) -> Option<f64> {
        self.schema.get(field).and_then(Value::as_f64)
    }

    fn is_integer(&self) -> bool {
        self.schema.get("type").and_then(Value::as_str) == Some("integer")
    }

    /// The ladder of a number row (`steps`), ascending; empty when the row steps evenly.
    fn ladder(&self) -> Vec<f64> {
        let mut steps: Vec<f64> = self
            .schema
            .get("x-steps")
            .and_then(Value::as_array)
            .map(|steps| steps.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default();
        steps.sort_by(f64::total_cmp);
        steps
    }

    fn number_value(&self, number: f64) -> Value {
        if self.is_integer() {
            Value::from(number.round() as i64)
        } else {
            Value::from(number)
        }
    }

    /// The text of the value as the row shows it.
    pub(crate) fn display(&self) -> String {
        match self.kind.as_str() {
            "toggle" => boolean_switch(self.value.as_bool().unwrap_or(false)).to_owned(),
            "choice" => self
                .options()
                .and_then(|options| {
                    options
                        .into_iter()
                        .find(|(value, _)| *value == self.value)
                        .map(|(_, title)| title)
                })
                .unwrap_or_else(|| plain(&self.value)),
            "number" => {
                let text = plain(&self.value);
                match self.schema.get("unit").and_then(Value::as_str) {
                    Some(unit) if !unit.is_empty() => format!("{text} {unit}"),
                    _ => text,
                }
            }
            _ => plain(&self.value),
        }
    }

    /// The value one step away, or the reason the row does not step. A toggle flips, a choice
    /// wraps around its options, a number moves by its `step` (or along its ladder) and stops
    /// at its bounds.
    pub(crate) fn stepped(&self, step: Step) -> Result<Value, String> {
        match self.kind.as_str() {
            "toggle" => Ok(Value::Bool(!self.value.as_bool().unwrap_or(false))),
            "choice" => {
                let options = self.options().ok_or_else(|| {
                    "this choice takes any text; type it in the plugin's page".to_owned()
                })?;
                let current = options
                    .iter()
                    .position(|(value, _)| *value == self.value)
                    .unwrap_or(0);
                let next = match step {
                    Step::Next => (current + 1) % options.len(),
                    Step::Previous => (current + options.len() - 1) % options.len(),
                };
                Ok(options[next].0.clone())
            }
            "number" => Ok(self.number_value(self.stepped_number(step))),
            other => Err(format!("a {other} row is edited in the plugin's own page")),
        }
    }

    fn stepped_number(&self, step: Step) -> f64 {
        let current = self
            .value
            .as_f64()
            .or(self.number("minimum"))
            .unwrap_or(0.0);
        let ladder = self.ladder();
        let moved = if ladder.is_empty() {
            let unit = self
                .number("step")
                .filter(|unit| *unit > 0.0)
                .unwrap_or(if self.is_integer() { 1.0 } else { 0.1 });
            match step {
                Step::Next => current + unit,
                Step::Previous => current - unit,
            }
        } else {
            match step {
                Step::Next => ladder
                    .iter()
                    .copied()
                    .find(|rung| *rung > current)
                    .unwrap_or(current),
                Step::Previous => ladder
                    .iter()
                    .rev()
                    .copied()
                    .find(|rung| *rung < current)
                    .unwrap_or(current),
            }
        };
        self.clamped(moved)
    }

    /// The number held inside the bounds of the row.
    fn clamped(&self, number: f64) -> f64 {
        let low = self.number("minimum").unwrap_or(f64::NEG_INFINITY);
        let high = self.number("maximum").unwrap_or(f64::INFINITY);
        // Float steps accumulate error (0.1 + 0.2); round to the precision of the step.
        let rounded = match self.number("step") {
            Some(unit) if unit > 0.0 && !self.is_integer() => {
                let places = (-unit.log10().floor()).clamp(0.0, 9.0);
                let scale = 10f64.powf(places);
                (number * scale).round() / scale
            }
            _ => number,
        };
        rounded.clamp(low, high)
    }

    /// Whether the row's value is typed: a number, or one of the text kinds (`text`,
    /// `text-area`, `path`, `color`, and a `choice` without fixed options). Enter on such a row
    /// opens a text prompt in Settings.
    pub(crate) fn takes_text(&self) -> bool {
        match self.kind.as_str() {
            "number" | "text" | "text-area" | "path" | "color" => true,
            "choice" => self.options().is_none(),
            _ => false,
        }
    }

    /// The text a prompt for this row starts with: the value in effect as plain text.
    pub(crate) fn prompt_text(&self) -> String {
        match &self.value {
            Value::Null => String::new(),
            other => plain(other),
        }
    }

    /// The value for text a person typed: a number row parses and clamps it, a text row takes
    /// it as it is (the store checks it against the row's schema on commit). An empty text
    /// resets the row to its default.
    pub(crate) fn typed(&self, text: &str) -> Result<Value, String> {
        if !self.takes_text() {
            return Err(format!("a {} row does not take typed text here", self.kind));
        }
        if self.kind != "number" {
            return Ok(if text.is_empty() {
                Value::Null
            } else {
                Value::String(text.to_owned())
            });
        }
        let number: f64 = text
            .trim()
            .parse()
            .map_err(|_| format!("`{}` is not a number", text.trim()))?;
        if !number.is_finite() {
            return Err("the number must be finite".to_owned());
        }
        Ok(self.number_value(self.clamped(number)))
    }

    /// The schema of the whole plugin as far as this row goes: an object with this property,
    /// which is what the store checks the patch against.
    fn single_schema(&self) -> String {
        serde_json::json!({"type": "object", "properties": {self.key.clone(): self.schema.clone()}})
            .to_string()
    }

    /// Writes `value` (`null` resets to the default) through the store. On success the row
    /// holds the new value and no refusal; on a refusal it keeps the old value and holds the
    /// refusal text. Returns whether the stored value changed.
    pub(crate) fn commit(&mut self, store: &PluginSettingsStore, value: Value) -> bool {
        let mut patch = Map::new();
        patch.insert(self.key.clone(), value.clone());
        match store.apply_patch(&self.plugin, &self.single_schema(), &patch) {
            Ok(changed) => {
                self.value = if value.is_null() {
                    self.default.clone()
                } else {
                    value
                };
                self.validation = None;
                !changed.is_empty()
            }
            Err(failure) => {
                self.validation = Some(match failure {
                    ilium_plugin_host::config_settings::PluginSettingsError::Invalid(text) => text,
                    ilium_plugin_host::config_settings::PluginSettingsError::Store(error) => {
                        error.message
                    }
                });
                false
            }
        }
    }

    /// Steps the row and writes the result; the refusal, if any, stays on the row.
    pub(crate) fn step_and_commit(&mut self, store: &PluginSettingsStore, step: Step) -> bool {
        match self.stepped(step) {
            Ok(value) => self.commit(store, value),
            Err(reason) => {
                self.validation = Some(reason);
                false
            }
        }
    }

    /// The control that draws the row and answers its clicks; the same rectangles serve both.
    pub(crate) fn control(&self, area: Rect, label_width: u16, value_text: &str) -> ValueControl {
        let editable = self.is_editable();
        let (kind, previous, next) = match self.kind.as_str() {
            "number" => (
                ControlKind::Number,
                self.stepped_number(Step::Previous) != self.value.as_f64().unwrap_or(0.0),
                self.stepped_number(Step::Next) != self.value.as_f64().unwrap_or(0.0),
            ),
            _ => (ControlKind::Choice, editable, editable),
        };
        ValueControl::new(
            area,
            ControlSpec {
                kind,
                label: &self.title,
                value: value_text,
                label_width,
                previous_enabled: previous,
                next_enabled: next,
                open_enabled: false,
            },
        )
    }
}

/// A value as plain text: strings without quotes, numbers without a trailing `.0`.
fn plain(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "unset".to_owned(),
        Value::Number(number) => match number.as_f64() {
            Some(float) if float.fract() == 0.0 && float.abs() < 1e15 => format!("{float:.0}"),
            _ => number.to_string(),
        },
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{rows_for, SettingRow, Step};
    use crate::value_control::{cell_width, ControlAction, PointerButton};
    use ilium_plugin_host::config_settings::PluginSettingsStore;
    use ratatui::layout::{Position, Rect};
    use serde_json::{json, Map, Value};

    const MANIFEST: &str = r#"{"contributes":{"settings":[{"kind":"tab","id":"main","title":"Demo",
      "sections":[{"title":"Sound","rows":[
        {"type":"toggle","key":"muted","title":"Muted","default":false},
        {"type":"number","key":"volume","title":"Volume","minimum":0,"maximum":10,"default":5},
        {"type":"number","key":"zoom","title":"Zoom","minimum":0.5,"maximum":2.0,"step":0.1,
         "integer":false,"default":1.0,"unit":"x"},
        {"type":"number","key":"size","title":"Size","steps":[8,12,16,24],"default":12},
        {"type":"choice","key":"mode","title":"Mode","default":"soft",
         "options":[{"value":"soft","title":"Soft"},{"value":"hard","title":"Hard"},
                    {"value":"off","title":"Off"}]},
        {"type":"text","key":"label","title":"Label","default":"ilium"}
      ]}]}]}}"#;

    fn row(key: &str) -> SettingRow {
        rows_for("acme.demo", MANIFEST, &Map::new())
            .into_iter()
            .find(|row| row.key == key)
            .expect("row exists")
    }

    fn store() -> (PluginSettingsStore, tempfile::TempDir) {
        let directory = tempfile::tempdir().expect("temporary directory");
        (
            PluginSettingsStore::at(directory.path().join("config.toml")),
            directory,
        )
    }

    #[test]
    fn rows_come_in_manifest_order_with_page_and_default_values() {
        let rows = rows_for("acme.demo", MANIFEST, &Map::new());
        let keys: Vec<&str> = rows.iter().map(|row| row.key.as_str()).collect();
        assert_eq!(keys, ["muted", "volume", "zoom", "size", "mode", "label"]);
        assert_eq!(rows[0].page, "main / Sound");
        assert_eq!(rows[1].value, json!(5));
    }

    #[test]
    fn a_stored_value_wins_over_the_default() {
        let mut stored = Map::new();
        stored.insert("volume".to_owned(), json!(9));
        let rows = rows_for("acme.demo", MANIFEST, &stored);
        assert_eq!(rows[1].value, json!(9));
        assert_eq!(rows[1].display(), "9");
    }

    #[test]
    fn only_toggle_number_and_fixed_choice_rows_have_their_own_control() {
        assert!(row("muted").is_editable());
        assert!(row("volume").is_editable());
        assert!(row("mode").is_editable());
        assert!(!row("label").is_editable());
    }

    #[test]
    fn a_toggle_flips_and_shows_the_shared_switch() {
        let mut muted = row("muted");
        assert_eq!(muted.display(), "[○ OFF]");
        assert_eq!(muted.stepped(Step::Next), Ok(json!(true)));
        let (store, _directory) = store();
        assert!(muted.step_and_commit(&store, Step::Next));
        assert_eq!(muted.display(), "[ON  ●]");
        assert_eq!(store.read("acme.demo").expect("read")["muted"], json!(true));
    }

    #[test]
    fn a_choice_wraps_around_its_options_in_both_directions() {
        let mode = row("mode");
        assert_eq!(mode.display(), "Soft");
        assert_eq!(mode.stepped(Step::Previous), Ok(json!("off")));
        assert_eq!(mode.stepped(Step::Next), Ok(json!("hard")));
    }

    #[test]
    fn a_number_stops_at_its_bounds_instead_of_wrapping() {
        let mut volume = row("volume");
        let (store, _directory) = store();
        volume.value = json!(10);
        assert_eq!(volume.stepped(Step::Next), Ok(json!(10)));
        volume.value = json!(0);
        assert_eq!(volume.stepped(Step::Previous), Ok(json!(0)));
        assert!(volume.step_and_commit(&store, Step::Next));
        assert_eq!(volume.value, json!(1));
    }

    #[test]
    fn a_decimal_number_steps_without_float_drift() {
        let mut zoom = row("zoom");
        zoom.value = json!(0.9);
        assert_eq!(zoom.stepped(Step::Next), Ok(json!(1.0)));
        zoom.value = json!(1.0);
        let mut seen = Vec::new();
        for _ in 0..3 {
            let next = zoom.stepped(Step::Next).expect("steps");
            seen.push(next.as_f64().expect("number"));
            zoom.value = next;
        }
        assert_eq!(seen, [1.1, 1.2, 1.3]);
        assert_eq!(row("zoom").display(), "1 x");
    }

    #[test]
    fn a_number_with_a_ladder_moves_rung_by_rung() {
        let mut size = row("size");
        assert_eq!(size.stepped(Step::Next), Ok(json!(16)));
        assert_eq!(size.stepped(Step::Previous), Ok(json!(8)));
        size.value = json!(24);
        assert_eq!(size.stepped(Step::Next), Ok(json!(24)));
    }

    #[test]
    fn typed_numbers_are_parsed_then_clamped() {
        let volume = row("volume");
        assert_eq!(volume.typed("7"), Ok(json!(7)));
        assert_eq!(volume.typed(" 99 "), Ok(json!(10)));
        assert_eq!(volume.typed("-4"), Ok(json!(0)));
        assert!(volume.typed("loud").is_err());
        assert!(volume.typed("NaN").is_err());
        assert!(row("muted").typed("1").is_err());
        assert!(
            !row("mode").takes_text(),
            "a choice with fixed options steps"
        );
    }

    #[test]
    fn a_text_row_takes_typed_text_and_stores_it() {
        let mut label = row("label");
        assert!(label.takes_text());
        assert_eq!(label.prompt_text(), "ilium");
        let (store, _directory) = store();
        let value = label.typed("dev box").expect("text is accepted");
        assert!(label.commit(&store, value));
        assert_eq!(label.value, json!("dev box"));
        assert_eq!(
            store.read("acme.demo").expect("read").get("label"),
            Some(&json!("dev box"))
        );
        // An empty text resets the row to its default.
        assert_eq!(label.typed(""), Ok(Value::Null));
        assert!(label.commit(&store, Value::Null));
        assert_eq!(label.value, json!("ilium"));
    }

    #[test]
    fn a_refused_write_keeps_the_value_and_shows_the_reason() {
        let mut volume = row("volume");
        let (store, _directory) = store();
        assert!(!volume.commit(&store, json!(99)));
        assert_eq!(volume.value, json!(5));
        let reason = volume.validation.clone().expect("a refusal text");
        assert!(reason.contains("volume"), "{reason}");
        assert!(store.read("acme.demo").expect("read").is_empty());
        assert!(volume.commit(&store, json!(3)));
        assert_eq!(volume.validation, None);
    }

    #[test]
    fn null_resets_the_row_to_its_default() {
        let mut volume = row("volume");
        let (store, _directory) = store();
        volume.commit(&store, json!(8));
        assert!(volume.commit(&store, Value::Null));
        assert_eq!(volume.value, json!(5));
        assert!(store.read("acme.demo").expect("read").is_empty());
    }

    #[test]
    fn a_row_that_does_not_step_says_where_it_is_edited() {
        let mut label = row("label");
        let (store, _directory) = store();
        assert!(!label.step_and_commit(&store, Step::Next));
        assert!(label.validation.expect("reason").contains("text"));
    }

    #[test]
    fn the_drawn_control_and_its_hit_targets_are_the_same_rectangles() {
        let muted = row("muted");
        let value = muted.display();
        let control = muted.control(Rect::new(2, 4, 60, 1), 20, &value);
        let geometry = control.geometry();
        // The switch text sits where the control says its value is, and a click there acts.
        assert_eq!(usize::from(geometry.value.width), cell_width(&value));
        let inside = Position::new(geometry.value.x, geometry.value.y);
        assert_eq!(
            control.hit(inside, PointerButton::Left),
            Some(ControlAction::NextChoice)
        );
        let outside = Position::new(geometry.label.x, geometry.label.y + 1);
        assert_eq!(control.hit(outside, PointerButton::Left), None);
        assert_eq!(geometry.row, Rect::new(2, 4, 60, 1));
    }

    #[test]
    fn a_number_control_disables_the_step_that_would_leave_the_range() {
        let mut volume = row("volume");
        volume.value = json!(10);
        let value = volume.display();
        let control = volume.control(Rect::new(0, 0, 60, 1), 20, &value);
        let geometry = control.geometry();
        assert_eq!(
            control.hit(Position::new(geometry.next.x, 0), PointerButton::Left),
            None
        );
        assert_eq!(
            control.hit(Position::new(geometry.previous.x, 0), PointerButton::Left),
            Some(ControlAction::Decrement)
        );
    }

    #[test]
    fn leaders_follow_the_shared_five_cell_rule() {
        use crate::value_control::leader_span;
        // One blank cell on each side; a gap shorter than five cells stays blank.
        assert!(leader_span(4).content.chars().all(|ch| ch == ' '));
        let drawn = leader_span(9).content.to_string();
        assert!(drawn.starts_with(' ') && drawn.ends_with(' '));
        assert_eq!(drawn.chars().filter(|ch| *ch == '\u{2026}').count(), 7);
    }

    #[test]
    fn a_row_keeps_its_leader_between_label_and_buttons_on_a_wide_area() {
        let muted = row("muted");
        let value = muted.display();
        let control = muted.control(Rect::new(0, 0, 70, 1), 12, &value);
        let geometry = control.geometry();
        // The label never overlaps the first button.
        assert!(geometry.label.right() < geometry.previous.x);
        assert!(geometry.next.right() <= geometry.row.right());
    }
}
