//! The padding settings of the Layout plugin as pure data.
//!
//! The plugin owns the panel padding: it declares the values as settings rows in its manifest
//! (the `padding` tab), keeps them under its own `[plugins."ilium.core.layout"]` table and sends
//! the whole table to the kernel as `kernel.set-padding` when it starts and after every edit.
//! This module is the model behind that: the row list for the current settings, the range of
//! each row, the application of one edit, the table read back from the stored values and the
//! command argument the kernel receives.

use crate::padding::{
    PaddingMode, PaddingPanel, PaddingSettings, PanelPadding, MAX_PADDING, MAX_WIDTH_THRESHOLD,
    MIN_WIDTH_THRESHOLD,
};

/// The kernel command that sets the panel padding: the whole table as JSON.
pub const SET_PADDING_COMMAND: &str = "kernel.set-padding";

/// The settings key prefix of each stored panel choice, in the order the page lists them.
pub const PANEL_KEYS: [&str; 5] = ["tree", "right_shared", "terminal", "editor", "board"];

/// What kind of control a row is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowKind {
    /// One of the three modes.
    Mode,
    /// A number between `minimum` and `maximum`.
    Number {
        /// Smallest value.
        minimum: u16,
        /// Largest value.
        maximum: u16,
    },
    /// A switch.
    Toggle,
}

/// One row of the padding tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The settings key of the row.
    pub key: String,
    /// The label.
    pub label: String,
    /// The control.
    pub kind: RowKind,
    /// The current value as text: a mode id, a number or `true`/`false`.
    pub value: String,
}

/// The settings key prefix of a panel choice.
fn panel_key(panel: PaddingPanel, shared: bool) -> &'static str {
    match panel {
        PaddingPanel::Tree => "tree",
        _ if shared => "right_shared",
        PaddingPanel::Terminal => "terminal",
        PaddingPanel::Editor => "editor",
        PaddingPanel::Board => "board",
    }
}

/// The stored choice a key prefix names.
fn choice_mut<'a>(settings: &'a mut PaddingSettings, name: &str) -> Option<&'a mut PanelPadding> {
    match name {
        "tree" => Some(&mut settings.tree),
        "right_shared" => Some(&mut settings.right_shared),
        "terminal" => Some(&mut settings.terminal),
        "editor" => Some(&mut settings.editor),
        "board" => Some(&mut settings.board),
        _ => None,
    }
}

/// The rows to show for `settings`: the threshold only in the width-dependent mode, the
/// per-kind right-hand panels only while sharing is off, the four sides of a panel only while
/// its sides are not linked.
pub fn rows(settings: &PaddingSettings) -> Vec<Row> {
    let mut rows = vec![Row {
        key: "mode".to_string(),
        label: "Padding".to_string(),
        kind: RowKind::Mode,
        value: settings.mode.id().to_string(),
    }];
    if settings.mode == PaddingMode::WidthDependent {
        rows.push(Row {
            key: "width_threshold".to_string(),
            label: "Minimum terminal width".to_string(),
            kind: RowKind::Number {
                minimum: MIN_WIDTH_THRESHOLD,
                maximum: MAX_WIDTH_THRESHOLD,
            },
            value: settings.width_threshold.to_string(),
        });
    }
    rows.push(Row {
        key: "right_panels_shared".to_string(),
        label: "Same padding for every right-hand panel".to_string(),
        kind: RowKind::Toggle,
        value: settings.right_panels_shared.to_string(),
    });
    let panels: Vec<PaddingPanel> = if settings.right_panels_shared {
        vec![PaddingPanel::Tree, PaddingPanel::Terminal]
    } else {
        [PaddingPanel::Tree]
            .into_iter()
            .chain(PaddingPanel::RIGHT_KINDS)
            .collect()
    };
    for panel in panels {
        let name = panel_key(panel, settings.right_panels_shared);
        let label = if name == "right_shared" {
            "Right-hand panels"
        } else {
            panel.label()
        };
        let choice = settings.panel(panel);
        rows.push(Row {
            key: format!("{name}_linked"),
            label: format!("{label}: same padding on every side"),
            kind: RowKind::Toggle,
            value: choice.linked.to_string(),
        });
        if choice.linked {
            rows.push(number_row(
                format!("{name}_all"),
                format!("{label}: padding"),
                choice.all,
            ));
        } else {
            for (side, cells) in [
                ("left", choice.sides.left),
                ("right", choice.sides.right),
                ("top", choice.sides.top),
                ("bottom", choice.sides.bottom),
            ] {
                rows.push(number_row(
                    format!("{name}_{side}"),
                    format!("{label}: {side} padding"),
                    cells,
                ));
            }
        }
    }
    rows
}

fn number_row(key: String, label: String, value: u8) -> Row {
    Row {
        key,
        label,
        kind: RowKind::Number {
            minimum: 0,
            maximum: u16::from(MAX_PADDING),
        },
        value: value.to_string(),
    }
}

/// Applies one edit of a row to `settings`. A value out of range or of the wrong type is an
/// error naming the key; the settings stay as they were.
pub fn apply(settings: &mut PaddingSettings, key: &str, value: &str) -> Result<(), String> {
    let mut edited = *settings;
    match key {
        "mode" => {
            edited.mode = PaddingMode::from_id(value)
                .ok_or_else(|| format!("{key}: unknown mode {value}"))?;
        }
        "width_threshold" => {
            edited.width_threshold = value
                .parse()
                .map_err(|_| format!("{key}: {value} is not a number"))?;
        }
        "right_panels_shared" => {
            edited.right_panels_shared = value
                .parse()
                .map_err(|_| format!("{key}: {value} is not true or false"))?;
        }
        other => {
            let (name, field) = other
                .rsplit_once('_')
                .filter(|(_, field)| *field == "all" || *field == "linked" || is_side(field))
                .and_then(|(name, field)| {
                    PANEL_KEYS.contains(&name).then_some((name, field))
                })
                .ok_or_else(|| format!("unknown padding row {other}"))?;
            let choice = choice_mut(&mut edited, name)
                .ok_or_else(|| format!("unknown padding row {other}"))?;
            if field == "linked" {
                choice.linked = value
                    .parse()
                    .map_err(|_| format!("{key}: {value} is not true or false"))?;
            } else {
                let cells: u8 = value
                    .parse()
                    .map_err(|_| format!("{key}: {value} is not a number"))?;
                match field {
                    "all" => choice.all = cells,
                    "left" => choice.sides.left = cells,
                    "right" => choice.sides.right = cells,
                    "top" => choice.sides.top = cells,
                    _ => choice.sides.bottom = cells,
                }
            }
        }
    }
    edited.validate()?;
    *settings = edited;
    Ok(())
}

fn is_side(field: &str) -> bool {
    matches!(field, "left" | "right" | "top" | "bottom")
}

/// The padding table the stored settings (`{"mode": "enabled", "tree_all": 2, ...}`, the keys of
/// the manifest rows) describe. A missing, mistyped or out-of-range value keeps its default, so
/// a half-written settings file still yields a table the kernel accepts.
pub fn settings_from_json(text: &str) -> PaddingSettings {
    let values: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
    let mut settings = PaddingSettings::default();
    if let Some(mode) = values
        .get("mode")
        .and_then(serde_json::Value::as_str)
        .and_then(PaddingMode::from_id)
    {
        settings.mode = mode;
    }
    if let Some(threshold) = values
        .get("width_threshold")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .filter(|value| (MIN_WIDTH_THRESHOLD..=MAX_WIDTH_THRESHOLD).contains(value))
    {
        settings.width_threshold = threshold;
    }
    if let Some(shared) = values
        .get("right_panels_shared")
        .and_then(serde_json::Value::as_bool)
    {
        settings.right_panels_shared = shared;
    }
    for name in PANEL_KEYS {
        let Some(choice) = choice_mut(&mut settings, name) else {
            continue;
        };
        if let Some(linked) = values
            .get(format!("{name}_linked"))
            .and_then(serde_json::Value::as_bool)
        {
            choice.linked = linked;
        }
        let cells = |field: &str| {
            values
                .get(format!("{name}_{field}"))
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| u8::try_from(value).ok())
                .filter(|value| *value <= MAX_PADDING)
        };
        if let Some(all) = cells("all") {
            choice.all = all;
        }
        if let Some(left) = cells("left") {
            choice.sides.left = left;
        }
        if let Some(right) = cells("right") {
            choice.sides.right = right;
        }
        if let Some(top) = cells("top") {
            choice.sides.top = top;
        }
        if let Some(bottom) = cells("bottom") {
            choice.sides.bottom = bottom;
        }
    }
    settings
}

/// The argument of `kernel.set-padding`: the whole table as one JSON object.
pub fn set_padding_arguments(settings: &PaddingSettings) -> String {
    serde_json::to_string(settings).unwrap_or_else(|_| "{}".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_threshold_row_appears_only_in_the_width_dependent_mode() {
        let mut settings = PaddingSettings::default();
        assert!(!rows(&settings)
            .iter()
            .any(|row| row.key == "width_threshold"));
        settings.mode = PaddingMode::WidthDependent;
        let row = rows(&settings)
            .into_iter()
            .find(|row| row.key == "width_threshold")
            .expect("a threshold row");
        assert_eq!(row.value, "200");
        assert_eq!(
            row.kind,
            RowKind::Number {
                minimum: 11,
                maximum: 1000
            }
        );
    }

    #[test]
    fn per_kind_rows_appear_only_while_sharing_is_off() {
        let mut settings = PaddingSettings::default();
        let keys = |settings: &PaddingSettings| -> Vec<String> {
            rows(settings).into_iter().map(|row| row.key).collect()
        };
        assert!(keys(&settings).contains(&"right_shared_all".to_string()));
        assert!(!keys(&settings).contains(&"editor_all".to_string()));
        settings.right_panels_shared = false;
        assert!(keys(&settings).contains(&"editor_all".to_string()));
        assert!(!keys(&settings).contains(&"right_shared_all".to_string()));
    }

    #[test]
    fn the_four_side_rows_replace_the_linked_value_when_a_panel_is_unlinked() {
        let mut settings = PaddingSettings::default();
        assert_eq!(apply(&mut settings, "tree_linked", "false"), Ok(()));
        assert_eq!(apply(&mut settings, "tree_left", "3"), Ok(()));
        let keys: Vec<String> = rows(&settings).into_iter().map(|row| row.key).collect();
        assert!(!keys.contains(&"tree_all".to_string()));
        for side in ["left", "right", "top", "bottom"] {
            assert!(keys.contains(&format!("tree_{side}")), "{side}");
        }
        assert_eq!(settings.tree.sides.left, 3);
        assert!(apply(&mut settings, "tree_top", "4").is_err());
        assert!(apply(&mut settings, "tree_diagonal", "1").is_err());
    }

    #[test]
    fn an_edit_in_range_applies_and_one_out_of_range_leaves_the_settings() {
        let mut settings = PaddingSettings::default();
        assert_eq!(apply(&mut settings, "mode", "enabled"), Ok(()));
        assert_eq!(settings.mode, PaddingMode::Enabled);
        assert_eq!(apply(&mut settings, "tree_all", "3"), Ok(()));
        assert_eq!(settings.tree.all, 3);
        let before = settings;
        assert!(apply(&mut settings, "tree_all", "4").is_err());
        assert!(apply(&mut settings, "width_threshold", "10").is_err());
        assert!(apply(&mut settings, "mode", "sometimes").is_err());
        assert!(apply(&mut settings, "frobnicate", "1").is_err());
        assert_eq!(settings, before);
    }

    #[test]
    fn the_stored_values_become_the_table_the_kernel_receives() {
        let settings = settings_from_json(
            r#"{"mode": "width_dependent", "width_threshold": 150, "tree_linked": false,
                "tree_left": 3, "terminal_all": 2, "right_panels_shared": false}"#,
        );
        assert_eq!(settings.mode, PaddingMode::WidthDependent);
        assert_eq!(settings.width_threshold, 150);
        assert!(!settings.tree.linked);
        assert_eq!(settings.tree.sides.left, 3);
        assert_eq!(settings.terminal.all, 2);
        assert!(!settings.right_panels_shared);
        let arguments = set_padding_arguments(&settings);
        let parsed: PaddingSettings = serde_json::from_str(&arguments).unwrap();
        assert_eq!(parsed, settings);
        assert!(arguments.contains(r#""mode":"width_dependent""#));
    }

    #[test]
    fn missing_mistyped_or_out_of_range_values_keep_their_defaults() {
        assert_eq!(settings_from_json(""), PaddingSettings::default());
        assert_eq!(settings_from_json("[]"), PaddingSettings::default());
        let settings = settings_from_json(
            r#"{"mode": 3, "width_threshold": 5, "tree_all": 9, "editor_top": "x"}"#,
        );
        assert_eq!(settings, PaddingSettings::default());
        assert_eq!(settings.validate(), Ok(()));
    }
}
