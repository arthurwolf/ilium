//! The rows of the padding settings tab as pure data.
//!
//! The kernel dialog (`padding_dialog.rs`, `padding_ui.rs`) is a ratatui editor with number
//! inputs; until a popup view can edit text (foundation gap G8) the plugin declares the same
//! values as settings rows in its manifest and this module is the model behind them: the row
//! list for the current settings, the range of each row and the application of one edit.

use crate::padding::{
    PaddingMode, PaddingPanel, PaddingSettings, MAX_PADDING, MAX_WIDTH_THRESHOLD,
    MIN_WIDTH_THRESHOLD,
};

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

/// The settings key of a panel row.
fn panel_key(panel: PaddingPanel) -> &'static str {
    match panel {
        PaddingPanel::Tree => "tree",
        PaddingPanel::Terminal => "terminal",
        PaddingPanel::Editor => "editor",
        PaddingPanel::Board => "board",
    }
}

/// The rows to show for `settings`. The per-kind right-hand rows appear only while sharing is
/// off; the threshold row only in the width-dependent mode.
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
    let panels: Vec<PaddingPanel> = if settings.right_panels_shared {
        vec![PaddingPanel::Tree]
    } else {
        [PaddingPanel::Tree]
            .into_iter()
            .chain(PaddingPanel::RIGHT_KINDS)
            .collect()
    };
    rows.push(Row {
        key: "right_panels_shared".to_string(),
        label: "Same padding for every right-hand panel".to_string(),
        kind: RowKind::Toggle,
        value: settings.right_panels_shared.to_string(),
    });
    for panel in panels {
        rows.push(number_row(
            format!("{}_all", panel_key(panel)),
            format!("{} padding", panel.label()),
            u16::from(settings.panel(panel).all),
        ));
    }
    if settings.right_panels_shared {
        rows.push(number_row(
            "right_shared_all".to_string(),
            "Right-hand panels padding".to_string(),
            u16::from(settings.right_shared.all),
        ));
    }
    rows
}

fn number_row(key: String, label: String, value: u16) -> Row {
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
            let name = other
                .strip_suffix("_all")
                .ok_or_else(|| format!("unknown padding row {other}"))?;
            let cells: u8 = value
                .parse()
                .map_err(|_| format!("{key}: {value} is not a number"))?;
            let choice = match name {
                "tree" => &mut edited.tree,
                "terminal" => &mut edited.terminal,
                "editor" => &mut edited.editor,
                "board" => &mut edited.board,
                "right_shared" => &mut edited.right_shared,
                _ => return Err(format!("unknown padding row {other}")),
            };
            choice.all = cells;
            choice.linked = true;
        }
    }
    edited.validate()?;
    *settings = edited;
    Ok(())
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
}
