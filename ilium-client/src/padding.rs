//! Panel padding: the empty cells between a panel's border and its content.
//!
//! This module is the pure domain for the `[padding]` settings: the three
//! activation modes, the per-panel side values (0 to [`MAX_PADDING`]) and the
//! resolution of "how many cells, on which side, for which panel, at this
//! terminal width". It performs no I/O and no drawing, so layout code, the
//! settings screen and the fine-tuning preview all ask the same questions of
//! the same functions.
//!
//! The tree panel reads its padding through a process-wide cell
//! ([`set_tree_padding`] / [`tree_padding`]), the same pattern as
//! `theme::section_frames_enabled`: the tree's geometry helpers are called
//! from dozens of places that only receive a `Rect`, and the cell lets them
//! agree without threading a new argument through every signature. The app
//! refreshes it whenever the settings or the terminal width change.

use std::sync::atomic::{AtomicU32, Ordering};

use ratatui::layout::Rect;
use serde::{Deserialize, Serialize};

/// Largest padding, in cells, on any one side.
pub const MAX_PADDING: u8 = 3;
/// Padding a freshly enabled panel gets on every side.
pub const DEFAULT_PADDING: u8 = 1;
/// The terminal width at or above which [`PaddingMode::WidthDependent`]
/// enables padding, until the user changes it.
pub const DEFAULT_WIDTH_THRESHOLD: u16 = 200;
/// The threshold must be above 10 columns.
pub const MIN_WIDTH_THRESHOLD: u16 = 11;
/// Upper bound that keeps the threshold a sensible terminal width.
pub const MAX_WIDTH_THRESHOLD: u16 = 1000;

/// When padding applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaddingMode {
    /// No padding anywhere (the original look).
    #[default]
    Disabled,
    /// Padding is always applied.
    Enabled,
    /// Padding is applied when the terminal is at least the threshold wide.
    WidthDependent,
}

impl PaddingMode {
    pub const ALL: [Self; 3] = [Self::Disabled, Self::WidthDependent, Self::Enabled];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Disabled => "Disabled",
            Self::Enabled => "Enabled",
            Self::WidthDependent => "Size dependent",
        }
    }

    pub const fn summary(self) -> &'static str {
        match self {
            Self::Disabled => "Content touches the panel borders. No padding is ever applied.",
            Self::Enabled => "Padding is always applied inside the panel borders.",
            Self::WidthDependent => {
                "Padding is applied only while the terminal is at least this many columns wide."
            }
        }
    }
}

/// Cells of padding on each side of a panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PaddingSides {
    pub left: u8,
    pub right: u8,
    pub top: u8,
    pub bottom: u8,
}

impl PaddingSides {
    pub const NONE: Self = Self {
        left: 0,
        right: 0,
        top: 0,
        bottom: 0,
    };

    pub const fn uniform(cells: u8) -> Self {
        Self {
            left: cells,
            right: cells,
            top: cells,
            bottom: cells,
        }
    }

    pub const fn is_none(self) -> bool {
        self.left == 0 && self.right == 0 && self.top == 0 && self.bottom == 0
    }

    /// Shrinks `area` by this padding. An axis that would leave no cell at
    /// all keeps its full extent, so a narrow terminal never loses a panel
    /// (or a PTY its last column) to padding.
    pub fn inset(self, area: Rect) -> Rect {
        let horizontal = u16::from(self.left) + u16::from(self.right);
        let vertical = u16::from(self.top) + u16::from(self.bottom);
        let (x, width) = if area.width > horizontal {
            (area.x + u16::from(self.left), area.width - horizontal)
        } else {
            (area.x, area.width)
        };
        let (y, height) = if area.height > vertical {
            (area.y + u16::from(self.top), area.height - vertical)
        } else {
            (area.y, area.height)
        };
        Rect::new(x, y, width, height)
    }

    fn clamped(self) -> Self {
        Self {
            left: self.left.min(MAX_PADDING),
            right: self.right.min(MAX_PADDING),
            top: self.top.min(MAX_PADDING),
            bottom: self.bottom.min(MAX_PADDING),
        }
    }
}

/// One panel's padding choice: either a single value for all four sides or
/// four independent ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelPadding {
    /// When set, `all` applies to every side and `sides` is ignored (but
    /// kept, so unlinking restores the earlier individual values).
    pub linked: bool,
    pub all: u8,
    pub sides: PaddingSides,
}

impl Default for PanelPadding {
    fn default() -> Self {
        Self {
            linked: true,
            all: DEFAULT_PADDING,
            sides: PaddingSides::uniform(DEFAULT_PADDING),
        }
    }
}

impl PanelPadding {
    /// The sides this choice resolves to.
    pub const fn resolved(self) -> PaddingSides {
        if self.linked {
            PaddingSides::uniform(self.all)
        } else {
            self.sides
        }
    }
}

/// A panel whose padding can be tuned on its own. The right-hand panel kinds
/// follow the pane content kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaddingPanel {
    /// The left tree panel.
    Tree,
    /// Terminals and agents.
    Terminal,
    Editor,
    Board,
}

impl PaddingPanel {
    pub const RIGHT_KINDS: [Self; 3] = [Self::Terminal, Self::Editor, Self::Board];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Tree => "Tree",
            Self::Terminal => "Terminals and agents",
            Self::Editor => "Editors",
            Self::Board => "Kanban boards",
        }
    }
}

/// The complete `[padding]` configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PaddingSettings {
    pub mode: PaddingMode,
    /// Terminal width at which [`PaddingMode::WidthDependent`] switches on.
    pub width_threshold: u16,
    /// Left tree panel.
    pub tree: PanelPadding,
    /// When set, every right-hand panel kind uses `right_shared`.
    pub right_panels_shared: bool,
    pub right_shared: PanelPadding,
    pub terminal: PanelPadding,
    pub editor: PanelPadding,
    pub board: PanelPadding,
}

impl Default for PaddingSettings {
    fn default() -> Self {
        Self {
            mode: PaddingMode::Disabled,
            width_threshold: DEFAULT_WIDTH_THRESHOLD,
            tree: PanelPadding::default(),
            right_panels_shared: true,
            right_shared: PanelPadding::default(),
            terminal: PanelPadding::default(),
            editor: PanelPadding::default(),
            board: PanelPadding::default(),
        }
    }
}

impl PaddingSettings {
    /// Whether padding is applied at `terminal_width`.
    pub const fn is_active(self, terminal_width: u16) -> bool {
        match self.mode {
            PaddingMode::Disabled => false,
            PaddingMode::Enabled => true,
            PaddingMode::WidthDependent => terminal_width >= self.width_threshold,
        }
    }

    /// The stored choice an edit of `panel` changes: the shared right-hand
    /// choice for every right kind while sharing is on.
    pub fn editable_mut(&mut self, panel: PaddingPanel) -> &mut PanelPadding {
        match panel {
            PaddingPanel::Tree => &mut self.tree,
            _ if self.right_panels_shared => &mut self.right_shared,
            PaddingPanel::Terminal => &mut self.terminal,
            PaddingPanel::Editor => &mut self.editor,
            PaddingPanel::Board => &mut self.board,
        }
    }

    /// The stored choice that applies to `panel` (read-only twin of
    /// [`Self::editable_mut`]).
    pub const fn panel(&self, panel: PaddingPanel) -> &PanelPadding {
        match panel {
            PaddingPanel::Tree => &self.tree,
            _ if self.right_panels_shared => &self.right_shared,
            PaddingPanel::Terminal => &self.terminal,
            PaddingPanel::Editor => &self.editor,
            PaddingPanel::Board => &self.board,
        }
    }

    /// The padding `panel` gets at `terminal_width`: none while inactive.
    pub fn sides_for(&self, panel: PaddingPanel, terminal_width: u16) -> PaddingSides {
        if !self.is_active(terminal_width) {
            return PaddingSides::NONE;
        }
        self.panel(panel).resolved().clamped()
    }

    /// Checks every value against its range. Returns the offending key path.
    pub fn validate(&self) -> Result<(), String> {
        if !(MIN_WIDTH_THRESHOLD..=MAX_WIDTH_THRESHOLD).contains(&self.width_threshold) {
            return Err(format!(
                "padding.width_threshold = {} must be between {MIN_WIDTH_THRESHOLD} and {MAX_WIDTH_THRESHOLD}",
                self.width_threshold
            ));
        }
        let panels = [
            ("tree", &self.tree),
            ("right_shared", &self.right_shared),
            ("terminal", &self.terminal),
            ("editor", &self.editor),
            ("board", &self.board),
        ];
        for (name, panel) in panels {
            let values = [
                panel.all,
                panel.sides.left,
                panel.sides.right,
                panel.sides.top,
                panel.sides.bottom,
            ];
            if values.iter().any(|value| *value > MAX_PADDING) {
                return Err(format!(
                    "padding.{name} values must be between 0 and {MAX_PADDING}"
                ));
            }
        }
        Ok(())
    }
}

/// Packed tree padding: one byte per side (left, right, top, bottom).
static TREE_PADDING: AtomicU32 = AtomicU32::new(0);

fn pack(sides: PaddingSides) -> u32 {
    u32::from(sides.left)
        | (u32::from(sides.right) << 8)
        | (u32::from(sides.top) << 16)
        | (u32::from(sides.bottom) << 24)
}

fn unpack(packed: u32) -> PaddingSides {
    PaddingSides {
        left: (packed & 0xff) as u8,
        right: ((packed >> 8) & 0xff) as u8,
        top: ((packed >> 16) & 0xff) as u8,
        bottom: ((packed >> 24) & 0xff) as u8,
    }
}

/// Publishes the padding the tree panel currently applies.
pub fn set_tree_padding(sides: PaddingSides) {
    TREE_PADDING.store(pack(sides), Ordering::Relaxed);
}

/// The padding the tree panel currently applies.
pub fn tree_padding() -> PaddingSides {
    unpack(TREE_PADDING.load(Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_disabled_with_a_200_column_threshold() {
        let settings = PaddingSettings::default();
        assert_eq!(settings.mode, PaddingMode::Disabled);
        assert_eq!(settings.width_threshold, 200);
        assert!(!settings.is_active(10_000));
        assert_eq!(
            settings.sides_for(PaddingPanel::Tree, 10_000),
            PaddingSides::NONE
        );
        assert_eq!(settings.validate(), Ok(()));
    }

    #[test]
    fn enabled_always_pads_and_width_dependent_switches_at_the_threshold() {
        let mut settings = PaddingSettings {
            mode: PaddingMode::Enabled,
            ..PaddingSettings::default()
        };
        assert!(settings.is_active(1));
        assert_eq!(
            settings.sides_for(PaddingPanel::Terminal, 1),
            PaddingSides::uniform(1)
        );
        settings.mode = PaddingMode::WidthDependent;
        assert!(!settings.is_active(199));
        assert!(settings.is_active(200));
        assert!(settings.is_active(201));
        settings.width_threshold = 120;
        assert!(settings.is_active(120));
        assert!(!settings.is_active(119));
    }

    #[test]
    fn linked_panels_use_one_value_and_unlinked_ones_keep_each_side() {
        let mut settings = PaddingSettings {
            mode: PaddingMode::Enabled,
            ..PaddingSettings::default()
        };
        settings.tree.all = 3;
        assert_eq!(
            settings.sides_for(PaddingPanel::Tree, 80),
            PaddingSides::uniform(3)
        );
        settings.tree.linked = false;
        settings.tree.sides = PaddingSides {
            left: 0,
            right: 1,
            top: 2,
            bottom: 3,
        };
        assert_eq!(
            settings.sides_for(PaddingPanel::Tree, 80),
            PaddingSides {
                left: 0,
                right: 1,
                top: 2,
                bottom: 3
            }
        );
    }

    #[test]
    fn shared_right_panels_ignore_per_kind_values_until_unshared() {
        let mut settings = PaddingSettings {
            mode: PaddingMode::Enabled,
            ..PaddingSettings::default()
        };
        settings.right_shared.all = 2;
        settings.editor.all = 3;
        assert_eq!(
            settings.sides_for(PaddingPanel::Editor, 80),
            PaddingSides::uniform(2)
        );
        settings.right_panels_shared = false;
        assert_eq!(
            settings.sides_for(PaddingPanel::Editor, 80),
            PaddingSides::uniform(3)
        );
        assert_eq!(
            settings.sides_for(PaddingPanel::Terminal, 80),
            PaddingSides::uniform(1)
        );
    }

    #[test]
    fn inset_shrinks_by_each_side_and_never_removes_the_last_cell() {
        let area = Rect::new(2, 3, 20, 10);
        let sides = PaddingSides {
            left: 1,
            right: 2,
            top: 3,
            bottom: 0,
        };
        assert_eq!(sides.inset(area), Rect::new(3, 6, 17, 7));
        assert_eq!(PaddingSides::NONE.inset(area), area);
        let narrow = Rect::new(0, 0, 3, 3);
        assert_eq!(PaddingSides::uniform(2).inset(narrow), narrow);
        assert_eq!(
            PaddingSides::uniform(1).inset(Rect::new(0, 0, 3, 3)),
            Rect::new(1, 1, 1, 1)
        );
    }

    #[test]
    fn validation_rejects_small_thresholds_and_large_sides() {
        let mut settings = PaddingSettings {
            width_threshold: 10,
            ..PaddingSettings::default()
        };
        assert!(settings.validate().is_err());
        settings.width_threshold = 11;
        assert_eq!(settings.validate(), Ok(()));
        settings.editor.sides.top = 4;
        assert!(settings.validate().is_err());
    }

    #[test]
    fn settings_round_trip_through_toml_and_partial_tables_use_defaults() {
        let mut settings = PaddingSettings {
            mode: PaddingMode::WidthDependent,
            width_threshold: 150,
            ..PaddingSettings::default()
        };
        settings.tree.linked = false;
        settings.tree.sides.left = 3;
        let text = toml::to_string(&settings).unwrap();
        let parsed: PaddingSettings = toml::from_str(&text).unwrap();
        assert_eq!(parsed, settings);
        let partial: PaddingSettings = toml::from_str("mode = \"enabled\"").unwrap();
        assert_eq!(partial.mode, PaddingMode::Enabled);
        assert_eq!(partial.width_threshold, 200);
    }

    #[test]
    fn packed_tree_padding_round_trips_every_side() {
        let sides = PaddingSides {
            left: 1,
            right: 2,
            top: 3,
            bottom: 0,
        };
        assert_eq!(unpack(pack(sides)), sides);
        assert_eq!(unpack(pack(PaddingSides::NONE)), PaddingSides::NONE);
    }
}
