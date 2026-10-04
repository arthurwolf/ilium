//! Data-driven settings rows. A scene describes its controls once; the
//! Settings UI, keyboard, mouse and persistence derive everything from them.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq)]
pub enum ControlKind {
    /// Integer range; `step` is the keyboard increment.
    Slider {
        min: i32,
        max: i32,
        step: i32,
        unit: &'static str,
    },
    /// One of a fixed list of labels; the value is the option index.
    Choice {
        options: Vec<&'static str>,
    },
    Toggle,
    /// Free text edited in a single-line prompt (path, glob, URL, address).
    Text {
        hint: &'static str,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlValue {
    Number(i32),
    Index(usize),
    Bool(bool),
    Text(String),
}

/// One settings row. `id` is stable and never shown; `label` is display text.
#[derive(Debug, Clone, PartialEq)]
pub struct Control {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: ControlKind,
    pub value: ControlValue,
    /// One or two sentences shown when the row is selected.
    pub help: &'static str,
    /// Extra runtime-dependent help appended after `help` (for example the
    /// name of the GPU adapter in use). `None` for static rows.
    pub help_detail: Option<String>,
    /// Choice options the user cannot select right now: (option index,
    /// reason). Empty for ordinary rows.
    pub disabled_options: Vec<(usize, String)>,
}

impl Control {
    pub fn slider(
        id: &'static str,
        label: &'static str,
        value: i32,
        (min, max, step): (i32, i32, i32),
        unit: &'static str,
        help: &'static str,
    ) -> Self {
        Self {
            id,
            label,
            kind: ControlKind::Slider {
                min,
                max,
                step,
                unit,
            },
            value: ControlValue::Number(value),
            help,
            help_detail: None,
            disabled_options: Vec::new(),
        }
    }

    pub fn choice(
        id: &'static str,
        label: &'static str,
        index: usize,
        options: &[&'static str],
        help: &'static str,
    ) -> Self {
        Self {
            id,
            label,
            kind: ControlKind::Choice {
                options: options.to_vec(),
            },
            value: ControlValue::Index(index),
            help,
            help_detail: None,
            disabled_options: Vec::new(),
        }
    }

    pub fn toggle(id: &'static str, label: &'static str, value: bool, help: &'static str) -> Self {
        Self {
            id,
            label,
            kind: ControlKind::Toggle,
            value: ControlValue::Bool(value),
            help,
            help_detail: None,
            disabled_options: Vec::new(),
        }
    }

    pub fn text(
        id: &'static str,
        label: &'static str,
        value: &str,
        hint: &'static str,
        help: &'static str,
    ) -> Self {
        Self {
            id,
            label,
            kind: ControlKind::Text { hint },
            value: ControlValue::Text(value.to_owned()),
            help,
            help_detail: None,
            disabled_options: Vec::new(),
        }
    }

    /// Mark one choice option as unselectable, with the reason shown to the user.
    pub fn with_disabled_option(mut self, index: usize, reason: impl Into<String>) -> Self {
        self.disabled_options
            .retain(|(existing, _)| *existing != index);
        self.disabled_options.push((index, reason.into()));
        self
    }

    /// Attach runtime-dependent help text shown after the static help.
    pub fn with_help_detail(mut self, detail: impl Into<String>) -> Self {
        self.help_detail = Some(detail.into());
        self
    }

    /// Why the given choice option is disabled, or `None` when it is enabled.
    pub fn disabled_reason(&self, index: usize) -> Option<&str> {
        self.disabled_options
            .iter()
            .find(|(disabled, _)| *disabled == index)
            .map(|(_, reason)| reason.as_str())
    }

    /// Text shown in the value column of the row.
    pub fn display_value(&self) -> String {
        match (&self.kind, &self.value) {
            (ControlKind::Slider { unit, .. }, ControlValue::Number(number)) => {
                format!("{number}{unit}")
            }
            (ControlKind::Choice { options }, ControlValue::Index(index)) => options
                .get(*index)
                .map_or_else(|| "?".to_owned(), |label| (*label).to_owned()),
            (ControlKind::Toggle, ControlValue::Bool(on)) => {
                if *on { "On" } else { "Off" }.to_owned()
            }
            (ControlKind::Text { hint }, ControlValue::Text(text)) => {
                if text.is_empty() {
                    format!("<{hint}>")
                } else {
                    text.clone()
                }
            }
            _ => "?".to_owned(),
        }
    }

    /// The value after one keyboard step. `direction` is -1 or +1. Text rows
    /// return `None`: they open a prompt instead.
    pub fn stepped(&self, direction: i32) -> Option<ControlValue> {
        let direction = direction.signum();
        match (&self.kind, &self.value) {
            (ControlKind::Slider { min, max, step, .. }, ControlValue::Number(number)) => Some(
                // Clamp in widened storage so a button at the i32 limit cannot overflow.
                ControlValue::Number(
                    (i64::from(*number) + i64::from(direction) * i64::from(*step))
                        .clamp(i64::from(*min), i64::from(*max)) as i32,
                ),
            ),
            (ControlKind::Choice { options }, ControlValue::Index(index)) => {
                let count = options.len().max(1) as i32;
                // Walk in `direction`, wrapping, past disabled options. When
                // every other option is disabled the value stays put.
                let mut candidate = *index as i32;
                for _ in 0..count {
                    candidate = (candidate + direction).rem_euclid(count);
                    if self.disabled_reason(candidate as usize).is_none() {
                        return Some(ControlValue::Index(candidate as usize));
                    }
                }
                Some(ControlValue::Index(*index))
            }
            (ControlKind::Toggle, ControlValue::Bool(on)) => Some(ControlValue::Bool(!on)),
            _ => None,
        }
    }
}

/// Per-scene settings contract. Stored data are concrete typed structs;
/// `controls`/`set_control` are the only view the UI has of them.
pub trait SceneSettings: Sized {
    /// Clamp every field to its documented range.
    fn normalized(&self) -> Self;
    /// Current rows, in display order. Rows may appear or disappear depending
    /// on other settings (for example a folder row only in folder mode).
    fn controls(&self) -> Vec<Control>;
    /// Apply one edit. `Ok(true)` when something changed, `Ok(false)` for an
    /// unknown id or unchanged value, `Err(message)` for invalid input that
    /// must be shown to the user (missing file, malformed URL, ...).
    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String>;
}

/// Clamp helper shared by settings implementations.
pub fn number(value: &ControlValue) -> Option<i32> {
    match value {
        ControlValue::Number(number) => Some(*number),
        _ => None,
    }
}

pub fn index(value: &ControlValue) -> Option<usize> {
    match value {
        ControlValue::Index(index) => Some(*index),
        _ => None,
    }
}

pub fn boolean(value: &ControlValue) -> Option<bool> {
    match value {
        ControlValue::Bool(on) => Some(*on),
        _ => None,
    }
}

pub fn text(value: &ControlValue) -> Option<&str> {
    match value {
        ControlValue::Text(text) => Some(text),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_steps_saturate_without_integer_overflow() {
        let high = Control::slider("n", "N", i32::MAX, (i32::MIN, i32::MAX, 1), "", "");
        assert_eq!(high.stepped(1), Some(ControlValue::Number(i32::MAX)));
        let low = Control::slider("n", "N", i32::MIN, (i32::MIN, i32::MAX, 1), "", "");
        assert_eq!(low.stepped(-1), Some(ControlValue::Number(i32::MIN)));
        let wide = Control::slider(
            "n",
            "N",
            i32::MAX - 1,
            (i32::MIN, i32::MAX, i32::MAX),
            "",
            "",
        );
        assert_eq!(wide.stepped(1), Some(ControlValue::Number(i32::MAX)));
        assert_eq!(wide.stepped(-1), Some(ControlValue::Number(-1)));
    }

    fn backend_row(index: usize) -> Control {
        Control::choice("backend", "Backend", index, &["Software", "GPU"], "help")
    }

    #[test]
    fn plain_rows_have_no_disabled_options() {
        let row = backend_row(0);
        assert!(row.disabled_options.is_empty());
        assert_eq!(row.disabled_reason(1), None);
        assert_eq!(row.stepped(1), Some(ControlValue::Index(1)));
    }

    #[test]
    fn disabled_option_reports_its_reason_and_is_replaced_not_duplicated() {
        let row = backend_row(0)
            .with_disabled_option(1, "first")
            .with_disabled_option(1, "second");
        assert_eq!(row.disabled_reason(1), Some("second"));
        assert_eq!(row.disabled_reason(0), None);
        assert_eq!(row.disabled_options.len(), 1);
    }

    #[test]
    fn stepping_skips_disabled_options_and_wraps() {
        let row = backend_row(0).with_disabled_option(1, "no");
        assert_eq!(row.stepped(1), Some(ControlValue::Index(0)));
        assert_eq!(row.stepped(-1), Some(ControlValue::Index(0)));

        let three =
            Control::choice("c", "C", 0, &["a", "b", "c"], "h").with_disabled_option(1, "no");
        assert_eq!(three.stepped(1), Some(ControlValue::Index(2)));
        assert_eq!(three.stepped(-1), Some(ControlValue::Index(2)));
    }

    #[test]
    fn stepping_from_a_disabled_value_moves_to_the_next_enabled_one() {
        let row = backend_row(1).with_disabled_option(1, "no");
        assert_eq!(row.stepped(1), Some(ControlValue::Index(0)));
        assert_eq!(row.stepped(-1), Some(ControlValue::Index(0)));
    }

    #[test]
    fn display_value_ignores_disabled_state() {
        let row = backend_row(1).with_disabled_option(1, "no");
        assert_eq!(row.display_value(), "GPU");
    }
}
