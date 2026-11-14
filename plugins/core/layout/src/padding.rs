//! Panel padding: the empty cells between a panel's border and its content.
//!
//! The pure domain of the `padding` settings: the three activation modes, the per-panel side
//! values (0 to [`MAX_PADDING`]) and the resolution of "how many cells, on which side, for which
//! panel, at this terminal width". It does no I/O and no drawing. The plugin owns and validates
//! the values and sends the whole table to the kernel as `kernel.set-padding`; the kernel keeps
//! a mirror of these types (`ilium-client/src/padding.rs`) and applies the result to the
//! geometry of every frame.

use serde::{Deserialize, Serialize};

/// Largest padding, in cells, on any one side.
pub const MAX_PADDING: u8 = 3;
/// Padding a freshly enabled panel gets on every side.
pub const DEFAULT_PADDING: u8 = 1;
/// The terminal width at or above which [`PaddingMode::WidthDependent`] enables padding.
pub const DEFAULT_WIDTH_THRESHOLD: u16 = 200;
/// The threshold must be above 10 columns.
pub const MIN_WIDTH_THRESHOLD: u16 = 11;
/// Upper bound that keeps the threshold a sensible terminal width.
pub const MAX_WIDTH_THRESHOLD: u16 = 1000;

/// A rectangle of terminal cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    /// Left column.
    pub x: u16,
    /// Top row.
    pub y: u16,
    /// Width in columns.
    pub width: u16,
    /// Height in rows.
    pub height: u16,
}

impl Area {
    /// A rectangle from its corner and size.
    pub const fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

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
    /// Every mode, in the order the dialog lists them.
    pub const ALL: [Self; 3] = [Self::Disabled, Self::WidthDependent, Self::Enabled];

    /// The settings value of the mode.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Enabled => "enabled",
            Self::WidthDependent => "width_dependent",
        }
    }

    /// Parses a settings value.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.id() == id)
    }

    /// The dialog label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Disabled => "Disabled",
            Self::Enabled => "Enabled",
            Self::WidthDependent => "Size dependent",
        }
    }

    /// The one-line explanation shown under the choice.
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
    /// Left cells.
    pub left: u8,
    /// Right cells.
    pub right: u8,
    /// Top cells.
    pub top: u8,
    /// Bottom cells.
    pub bottom: u8,
}

impl PaddingSides {
    /// No padding.
    pub const NONE: Self = Self {
        left: 0,
        right: 0,
        top: 0,
        bottom: 0,
    };

    /// The same value on every side.
    pub const fn uniform(cells: u8) -> Self {
        Self {
            left: cells,
            right: cells,
            top: cells,
            bottom: cells,
        }
    }

    /// Whether every side is zero.
    pub const fn is_none(self) -> bool {
        self.left == 0 && self.right == 0 && self.top == 0 && self.bottom == 0
    }

    /// Shrinks `area` by this padding. An axis that would leave no cell at all keeps its full
    /// extent, so a narrow terminal never loses a panel (or a PTY its last column) to padding.
    pub fn inset(self, area: Area) -> Area {
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
        Area::new(x, y, width, height)
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

/// One panel's padding choice: a single value for all four sides or four independent ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelPadding {
    /// When set, `all` applies to every side and `sides` is ignored (but kept, so unlinking
    /// restores the earlier individual values).
    pub linked: bool,
    /// The linked value.
    pub all: u8,
    /// The individual values.
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

/// A panel whose padding can be tuned on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaddingPanel {
    /// The left tree panel.
    Tree,
    /// Terminals and agents.
    Terminal,
    /// Editors.
    Editor,
    /// Kanban boards.
    Board,
}

impl PaddingPanel {
    /// The three right-hand panel kinds.
    pub const RIGHT_KINDS: [Self; 3] = [Self::Terminal, Self::Editor, Self::Board];

    /// The dialog label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Tree => "Tree",
            Self::Terminal => "Terminals and agents",
            Self::Editor => "Editors",
            Self::Board => "Kanban boards",
        }
    }
}

/// The complete `padding` configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PaddingSettings {
    /// When padding applies.
    pub mode: PaddingMode,
    /// Terminal width at which [`PaddingMode::WidthDependent`] switches on.
    pub width_threshold: u16,
    /// Left tree panel.
    pub tree: PanelPadding,
    /// When set, every right-hand panel kind uses `right_shared`.
    pub right_panels_shared: bool,
    /// The choice shared by the right-hand panel kinds.
    pub right_shared: PanelPadding,
    /// Terminals and agents.
    pub terminal: PanelPadding,
    /// Editors.
    pub editor: PanelPadding,
    /// Kanban boards.
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

    /// The stored choice an edit of `panel` changes: the shared right-hand choice for every
    /// right kind while sharing is on.
    pub fn editable_mut(&mut self, panel: PaddingPanel) -> &mut PanelPadding {
        match panel {
            PaddingPanel::Tree => &mut self.tree,
            _ if self.right_panels_shared => &mut self.right_shared,
            PaddingPanel::Terminal => &mut self.terminal,
            PaddingPanel::Editor => &mut self.editor,
            PaddingPanel::Board => &mut self.board,
        }
    }

    /// The stored choice that applies to `panel` (read-only twin of [`Self::editable_mut`]).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mode_id_round_trips() {
        for mode in PaddingMode::ALL {
            assert_eq!(PaddingMode::from_id(mode.id()), Some(mode));
        }
        assert_eq!(PaddingMode::from_id("sometimes"), None);
    }
}
