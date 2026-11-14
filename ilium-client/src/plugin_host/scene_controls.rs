//! The settings rows of a plugin scene, derived from the `settings_schema` of its manifest
//! entry (`contributes.scenes[].settings_schema`).
//!
//! A scene declares only what is specific to it (CLAUDE.md, "Scene settings are only what is
//! specific to the scene"): an `integer` or `number` property becomes a slider, a `boolean` a
//! toggle, a `string` with `enum` a choice (`enum_titles` are the labels) and any other
//! `string` a free text row. `title`, `help` (or `description`), `minimum`, `maximum`, `step`,
//! `unit`, `default` and `order` shape the row; `visible_when` (`{"key": <choice property>,
//! "in": [<enum string>, ...]}`) shows the row only while that choice has one of those values; properties without an `order` follow the
//! ordered ones, alphabetically. The edited values live in the project settings
//! (`ilium_ambient::external::SceneValues`), and [`SceneControlSchema::settings_json`] is the
//! `settings` object the guest receives in its spec and in `reconfigure`.

use ilium_ambient::control::{Control, ControlValue};
use ilium_ambient::external::{intern, SceneValues};
use serde_json::{json, Map, Value};

/// Properties a scene may declare, at most; a manifest with more is cut here.
const MAXIMUM_ROWS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
enum RowKind {
    Number {
        minimum: i32,
        maximum: i32,
        step: i32,
        unit: String,
        default: i32,
    },
    Toggle {
        default: bool,
    },
    Choice {
        values: Vec<String>,
        titles: Vec<String>,
        default: usize,
    },
    Text {
        default: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    key: String,
    label: String,
    help: String,
    kind: RowKind,
    /// Shown only while the choice row `key` has one of the `enum` strings `values`.
    visible_when: Option<(String, Vec<String>)>,
}

/// The rows of one scene.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SceneControlSchema {
    rows: Vec<Row>,
}

fn integer(value: Option<&Value>) -> Option<i32> {
    let value = value?;
    let number = value
        .as_i64()
        .or_else(|| value.as_f64().map(|float| float.round() as i64))?;
    i32::try_from(number).ok()
}

impl SceneControlSchema {
    /// The rows of a `settings_schema` object; an unusable schema gives no rows.
    pub(crate) fn from_schema(schema: &Value) -> Self {
        let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
            return Self::default();
        };
        let mut ordered: Vec<(i64, &String, &Value)> = properties
            .iter()
            .map(|(key, property)| {
                let order = property
                    .get("order")
                    .and_then(Value::as_i64)
                    .unwrap_or(i64::MAX);
                (order, key, property)
            })
            .collect();
        ordered.sort_by(|left, right| (left.0, left.1).cmp(&(right.0, right.1)));
        let rows = ordered
            .into_iter()
            .take(MAXIMUM_ROWS)
            .filter_map(|(_, key, property)| Self::row(key, property))
            .collect();
        Self { rows }
    }

    fn row(key: &str, property: &Value) -> Option<Row> {
        let label = property
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or(key)
            .to_owned();
        let help = property
            .get("help")
            .or_else(|| property.get("description"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let kind = match property.get("type").and_then(Value::as_str)? {
            "integer" | "number" => {
                let minimum = integer(property.get("minimum")).unwrap_or(0);
                let maximum = integer(property.get("maximum")).unwrap_or(100).max(minimum);
                RowKind::Number {
                    minimum,
                    maximum,
                    step: integer(property.get("step")).unwrap_or(1).max(1),
                    unit: property
                        .get("unit")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    default: integer(property.get("default"))
                        .unwrap_or(minimum)
                        .clamp(minimum, maximum),
                }
            }
            "boolean" => RowKind::Toggle {
                default: property
                    .get("default")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            },
            "string" => match property.get("enum").and_then(Value::as_array) {
                Some(options) => {
                    let values: Vec<String> = options
                        .iter()
                        .filter_map(|option| option.as_str().map(str::to_owned))
                        .collect();
                    if values.is_empty() {
                        return None;
                    }
                    let titles = property
                        .get("enum_titles")
                        .and_then(Value::as_array)
                        .filter(|titles| titles.len() == values.len())
                        .map(|titles| {
                            titles
                                .iter()
                                .map(|title| title.as_str().unwrap_or_default().to_owned())
                                .collect()
                        })
                        .unwrap_or_else(|| values.clone());
                    let default = property
                        .get("default")
                        .and_then(Value::as_str)
                        .and_then(|wanted| values.iter().position(|value| value == wanted))
                        .unwrap_or(0);
                    RowKind::Choice {
                        values,
                        titles,
                        default,
                    }
                }
                None => RowKind::Text {
                    default: property
                        .get("default")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                },
            },
            _ => return None,
        };
        let visible_when = property.get("visible_when").and_then(|rule| {
            let controlling = rule.get("key")?.as_str()?.to_owned();
            let values = rule
                .get("in")?
                .as_array()?
                .iter()
                .filter_map(|value| value.as_str().map(str::to_owned))
                .collect();
            Some((controlling, values))
        });
        Some(Row {
            key: key.to_owned(),
            label,
            help,
            kind,
            visible_when,
        })
    }

    /// Whether the scene declares no settings.
    pub(crate) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The value of `row` in effect: the stored one when it fits the row, else the default.
    fn effective(row: &Row, values: &SceneValues) -> ControlValue {
        let stored = values.get(&row.key);
        match (&row.kind, stored) {
            (
                RowKind::Number {
                    minimum, maximum, ..
                },
                Some(ControlValue::Number(number)),
            ) => ControlValue::Number((*number).clamp(*minimum, *maximum)),
            (RowKind::Number { default, .. }, _) => ControlValue::Number(*default),
            (RowKind::Toggle { .. }, Some(ControlValue::Bool(on))) => ControlValue::Bool(*on),
            (RowKind::Toggle { default }, _) => ControlValue::Bool(*default),
            (RowKind::Choice { values, .. }, Some(ControlValue::Index(index)))
                if *index < values.len() =>
            {
                ControlValue::Index(*index)
            }
            (RowKind::Choice { default, .. }, _) => ControlValue::Index(*default),
            (RowKind::Text { .. }, Some(ControlValue::Text(text))) => {
                ControlValue::Text(text.clone())
            }
            (RowKind::Text { default }, _) => ControlValue::Text(default.clone()),
        }
    }

    /// Whether `row` shows with `values`: a row with no rule always does, a row whose rule
    /// names a missing or non-choice row never does.
    fn visible(&self, row: &Row, values: &SceneValues) -> bool {
        let Some((controlling, wanted)) = &row.visible_when else {
            return true;
        };
        self.rows
            .iter()
            .find(|other| &other.key == controlling)
            .is_some_and(
                |other| match (&other.kind, Self::effective(other, values)) {
                    (
                        RowKind::Choice {
                            values: options, ..
                        },
                        ControlValue::Index(index),
                    ) => options
                        .get(index)
                        .is_some_and(|option| wanted.contains(option)),
                    _ => false,
                },
            )
    }

    fn default_value(row: &Row) -> ControlValue {
        Self::effective(row, &SceneValues::new())
    }

    /// The rows showing `values`.
    pub(crate) fn controls(&self, values: &SceneValues) -> Vec<Control> {
        self.rows
            .iter()
            .filter(|row| self.visible(row, values))
            .map(|row| {
                let id = intern(&row.key);
                let label = intern(&row.label);
                let help = intern(&row.help);
                match (&row.kind, Self::effective(row, values)) {
                    (
                        RowKind::Number {
                            minimum,
                            maximum,
                            step,
                            unit,
                            ..
                        },
                        ControlValue::Number(number),
                    ) => Control::slider(
                        id,
                        label,
                        number,
                        (*minimum, *maximum, *step),
                        intern(unit),
                        help,
                    ),
                    (RowKind::Toggle { .. }, ControlValue::Bool(on)) => {
                        Control::toggle(id, label, on, help)
                    }
                    (RowKind::Choice { titles, .. }, ControlValue::Index(index)) => {
                        let options: Vec<&'static str> =
                            titles.iter().map(|title| intern(title)).collect();
                        Control::choice(id, label, index, &options, help)
                    }
                    (RowKind::Text { .. }, ControlValue::Text(text)) => {
                        Control::text(id, label, &text, "text", help)
                    }
                    // `effective` returns the value kind of the row.
                    _ => Control::toggle(id, label, false, help),
                }
            })
            .collect()
    }

    /// Applies one edit to `values`. Values equal to the default are not stored. `Ok(false)`
    /// for an unknown id or an unchanged value, `Err` for a value of the wrong kind.
    pub(crate) fn set_control(
        &self,
        values: &mut SceneValues,
        id: &str,
        value: ControlValue,
    ) -> Result<bool, String> {
        let Some(row) = self.rows.iter().find(|row| row.key == id) else {
            return Ok(false);
        };
        let new = match (&row.kind, value) {
            (
                RowKind::Number {
                    minimum, maximum, ..
                },
                ControlValue::Number(number),
            ) => ControlValue::Number(number.clamp(*minimum, *maximum)),
            (RowKind::Toggle { .. }, ControlValue::Bool(on)) => ControlValue::Bool(on),
            (RowKind::Choice { values, .. }, ControlValue::Index(index)) => {
                if index >= values.len() {
                    return Err(format!("{}: option {index} does not exist", row.label));
                }
                ControlValue::Index(index)
            }
            (RowKind::Text { .. }, ControlValue::Text(text)) => ControlValue::Text(text),
            (RowKind::Number { .. }, _) => return Err(format!("{} expects a number", row.label)),
            (RowKind::Toggle { .. }, _) => return Err(format!("{} expects On or Off", row.label)),
            (RowKind::Choice { .. }, _) => return Err(format!("{} expects an option", row.label)),
            (RowKind::Text { .. }, _) => return Err(format!("{} expects text", row.label)),
        };
        let before = Self::effective(row, values);
        if before == new {
            return Ok(false);
        }
        if new == Self::default_value(row) {
            values.remove(&row.key);
        } else {
            values.insert(row.key.clone(), new);
        }
        Ok(true)
    }

    /// The `settings` object of the scene spec: every row's effective value, keyed by
    /// property; a choice is its `enum` string.
    pub(crate) fn settings_json(&self, values: &SceneValues) -> Value {
        let mut settings = Map::new();
        for row in &self.rows {
            let value = match (&row.kind, Self::effective(row, values)) {
                (_, ControlValue::Number(number)) => json!(number),
                (_, ControlValue::Bool(on)) => json!(on),
                (RowKind::Choice { values, .. }, ControlValue::Index(index)) => {
                    json!(values.get(index).cloned().unwrap_or_default())
                }
                (_, ControlValue::Index(index)) => json!(index),
                (_, ControlValue::Text(text)) => json!(text),
            };
            settings.insert(row.key.clone(), value);
        }
        Value::Object(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> SceneControlSchema {
        SceneControlSchema::from_schema(&json!({
            "type": "object",
            "properties": {
                "zoom": {"type": "integer", "title": "Zoom", "minimum": 10, "maximum": 50,
                         "step": 5, "unit": "%", "default": 20, "order": 2, "help": "How far."},
                "ripple": {"type": "boolean", "title": "Ripple", "order": 1},
                "pattern": {"type": "string", "enum": ["a", "b", "c"],
                            "enum_titles": ["A", "B", "C"], "default": "b", "order": 3},
                "label": {"type": "string", "order": 4},
                "ignored": {"type": "array"}
            }
        }))
    }

    #[test]
    fn rows_follow_order_then_name_and_unusable_properties_are_skipped() {
        let rows = schema().controls(&SceneValues::new());
        let ids: Vec<&str> = rows.iter().map(|row| row.id).collect();
        assert_eq!(ids, ["ripple", "zoom", "pattern", "label"]);
        assert_eq!(rows[1].value, ControlValue::Number(20));
        assert_eq!(rows[1].help, "How far.");
        assert_eq!(rows[2].value, ControlValue::Index(1));
        assert_eq!(rows[2].display_value(), "B");
        assert!(SceneControlSchema::from_schema(&json!({})).is_empty());
        assert!(SceneControlSchema::from_schema(&json!(7)).is_empty());
    }

    #[test]
    fn edits_clamp_store_only_non_default_values_and_report_changes() {
        let schema = schema();
        let mut values = SceneValues::new();
        assert_eq!(
            schema.set_control(&mut values, "zoom", ControlValue::Number(20)),
            Ok(false)
        );
        assert_eq!(
            schema.set_control(&mut values, "zoom", ControlValue::Number(999)),
            Ok(true)
        );
        assert_eq!(values.get("zoom"), Some(&ControlValue::Number(50)));
        assert_eq!(
            schema.set_control(&mut values, "zoom", ControlValue::Number(20)),
            Ok(true)
        );
        assert!(values.is_empty(), "the default is not stored");
        assert_eq!(
            schema.set_control(&mut values, "missing", ControlValue::Number(1)),
            Ok(false)
        );
        assert!(schema
            .set_control(&mut values, "zoom", ControlValue::Bool(true))
            .is_err());
        assert!(schema
            .set_control(&mut values, "pattern", ControlValue::Index(3))
            .is_err());
    }

    #[test]
    fn the_spec_settings_carry_every_effective_value() {
        let schema = schema();
        let mut values = SceneValues::new();
        schema
            .set_control(&mut values, "pattern", ControlValue::Index(2))
            .unwrap();
        schema
            .set_control(&mut values, "ripple", ControlValue::Bool(true))
            .unwrap();
        assert_eq!(
            schema.settings_json(&values),
            json!({"label": "", "pattern": "c", "ripple": true, "zoom": 20})
        );
    }

    #[test]
    fn a_stored_value_that_no_longer_fits_falls_back() {
        let schema = schema();
        let mut values = SceneValues::new();
        values.insert("zoom".into(), ControlValue::Number(500));
        values.insert("pattern".into(), ControlValue::Index(9));
        values.insert("ripple".into(), ControlValue::Number(1));
        let rows = schema.controls(&values);
        assert_eq!(rows[0].value, ControlValue::Bool(false));
        assert_eq!(rows[1].value, ControlValue::Number(50));
        assert_eq!(rows[2].value, ControlValue::Index(1));
    }

    #[test]
    fn a_row_with_visible_when_shows_only_for_its_choices_but_stays_in_the_spec() {
        let schema = SceneControlSchema::from_schema(&json!({
            "type": "object",
            "properties": {
                "mode": {"type": "string", "enum": ["a", "b"], "order": 1},
                "depth": {"type": "integer", "minimum": 1, "maximum": 9, "default": 3,
                          "order": 2, "visible_when": {"key": "mode", "in": ["b"]}},
                "orphan": {"type": "boolean", "order": 3,
                           "visible_when": {"key": "missing", "in": ["b"]}}
            }
        }));
        let mut values = SceneValues::new();
        let ids = |values: &SceneValues| -> Vec<&'static str> {
            schema.controls(values).iter().map(|row| row.id).collect()
        };
        assert_eq!(ids(&values), ["mode"]);
        schema
            .set_control(&mut values, "mode", ControlValue::Index(1))
            .unwrap();
        assert_eq!(ids(&values), ["mode", "depth"]);
        assert_eq!(schema.settings_json(&values)["orphan"], json!(false));
        assert_eq!(schema.settings_json(&values)["depth"], json!(3));
    }
}
