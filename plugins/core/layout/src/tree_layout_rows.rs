//! The tree row layout: which of the identity, objective, activity and title slots a tree row
//! shows, in which order, and the keys of the settings editor that changes both.
//!
//! Port of the pure parts of `ilium-client/src/tree_row_layout.rs` (the model and the slot
//! widths) and of `EditorState::apply` in `tree_layout_settings.rs` (the key handling). The
//! ratatui drawing of the editor and the specimen stays in the kernel (port-keep until the popup
//! views can edit text, foundation gap G8).

use serde::{Deserialize, Serialize};

/// One slot of a tree row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TreeRowElement {
    /// The icon of the node kind or agent.
    Identity,
    /// The objective (goal) icon.
    Objective,
    /// The activity icon.
    Activity,
    /// The title; also the divider between left and right slots.
    Title,
}

impl TreeRowElement {
    /// Every slot, in the default order.
    pub const ALL: [Self; 4] = [Self::Identity, Self::Objective, Self::Activity, Self::Title];

    /// The editor label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Identity => "Identity",
            Self::Objective => "Objective",
            Self::Activity => "Activity",
            Self::Title => "Title / left-right divider",
        }
    }
}

/// The order and visibility of the slots of a tree row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TreeRowLayout {
    /// Slots from left to right; the title is the divider between left and right slots.
    pub order: Vec<TreeRowElement>,
    /// Slots the user hid. The title cannot be hidden.
    pub hidden: Vec<TreeRowElement>,
}

impl Default for TreeRowLayout {
    fn default() -> Self {
        Self {
            order: TreeRowElement::ALL.to_vec(),
            hidden: Vec::new(),
        }
    }
}

impl TreeRowLayout {
    /// Completes a partial order, drops duplicates and a hidden title, and sorts `hidden`.
    pub fn normalized(mut self) -> Self {
        let mut order = Vec::new();
        for element in self.order.into_iter().chain(TreeRowElement::ALL) {
            if !order.contains(&element) {
                order.push(element);
            }
        }
        self.order = order;
        self.hidden
            .retain(|element| *element != TreeRowElement::Title);
        self.hidden
            .sort_by_key(|element| TreeRowElement::ALL.iter().position(|item| item == element));
        self.hidden.dedup();
        self
    }

    /// The slots from left to right.
    pub fn order(&self) -> &[TreeRowElement] {
        &self.order
    }

    /// Whether the slot is shown.
    pub fn is_visible(&self, element: TreeRowElement) -> bool {
        element == TreeRowElement::Title || !self.hidden.contains(&element)
    }

    /// Whether this is the stock layout.
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    /// Preset 2: the title first, every icon right-aligned after it.
    pub fn all_right() -> Self {
        Self {
            order: vec![
                TreeRowElement::Title,
                TreeRowElement::Identity,
                TreeRowElement::Objective,
                TreeRowElement::Activity,
            ],
            hidden: Vec::new(),
        }
    }

    /// Preset 3: identity left of the title, objective and activity right of it.
    pub fn mixed() -> Self {
        Self {
            order: vec![
                TreeRowElement::Identity,
                TreeRowElement::Title,
                TreeRowElement::Objective,
                TreeRowElement::Activity,
            ],
            hidden: Vec::new(),
        }
    }

    /// `(slot, start column, width)` of every visible slot of a row `width` cells wide. Icon
    /// widths are the caller's measured widths. The title keeps one cell before icons when the
    /// width is constrained; icons are dropped from the first slot on until the rest fits.
    pub fn placements(
        &self,
        identity_width: usize,
        objective_width: usize,
        activity_width: usize,
        width: usize,
    ) -> Vec<(TreeRowElement, usize, usize)> {
        let layout = self.clone().normalized();
        let available = width.saturating_sub(1);
        let mut sizes = [identity_width, objective_width, activity_width];
        for (index, element) in [
            TreeRowElement::Identity,
            TreeRowElement::Objective,
            TreeRowElement::Activity,
        ]
        .into_iter()
        .enumerate()
        {
            if !layout.is_visible(element) {
                sizes[index] = 0;
            }
        }
        for index in 0..3 {
            if sizes.iter().sum::<usize>() <= available {
                break;
            }
            sizes[index] = 0;
        }
        let slots: Vec<_> = layout
            .order
            .iter()
            .copied()
            .filter(|element| layout.is_visible(*element))
            .map(|element| {
                (
                    element,
                    match element {
                        TreeRowElement::Identity => sizes[0],
                        TreeRowElement::Objective => sizes[1],
                        TreeRowElement::Activity => sizes[2],
                        TreeRowElement::Title => 0,
                    },
                )
            })
            .collect();
        let icons: usize = sizes.iter().sum();
        let mut start = 0;
        slots
            .into_iter()
            .map(|(element, size)| {
                let size = if element == TreeRowElement::Title {
                    width.saturating_sub(icons)
                } else {
                    size
                };
                let placement = (element, start, size);
                start += size;
                placement
            })
            .collect()
    }
}

/// A key of the tree layout editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorKey {
    /// Start or finish moving the selected slot.
    Enter,
    /// Select the slot above; moves it too while moving or with `shift`.
    Up {
        /// Whether Shift is held.
        shift: bool,
    },
    /// Select the slot below; moves it too while moving or with `shift`.
    Down {
        /// Whether Shift is held.
        shift: bool,
    },
    /// Show or hide the selected slot.
    Space,
    /// Preset 1 (default), 2 (all right) or 3 (mixed).
    Preset(u8),
    /// Back to the default layout.
    Reset,
}

/// The editor selection and move state.
#[derive(Debug, Clone, Default)]
pub struct EditorState {
    /// Index of the selected slot.
    pub selected: usize,
    /// Whether the selected slot follows the arrow keys.
    pub moving: bool,
}

impl EditorState {
    /// Applies a key to `layout`. Returns whether the layout changed; navigation never does.
    pub fn apply(&mut self, layout: &mut TreeRowLayout, key: EditorKey) -> bool {
        let before = layout.clone();
        match key {
            EditorKey::Enter => self.moving = !self.moving,
            EditorKey::Up { shift } | EditorKey::Down { shift } => {
                let next = if matches!(key, EditorKey::Up { .. }) {
                    self.selected.saturating_sub(1)
                } else {
                    (self.selected + 1).min(3)
                };
                if self.moving || shift {
                    layout.order.swap(self.selected, next);
                }
                self.selected = next;
            }
            EditorKey::Space => {
                self.moving = false;
                let element = layout.order[self.selected];
                if element != TreeRowElement::Title {
                    if layout.hidden.contains(&element) {
                        layout.hidden.retain(|item| *item != element);
                    } else {
                        layout.hidden.push(element);
                    }
                }
            }
            EditorKey::Preset(1) | EditorKey::Reset => {
                self.moving = false;
                *layout = TreeRowLayout::default();
            }
            EditorKey::Preset(2) => {
                self.moving = false;
                *layout = TreeRowLayout::all_right();
            }
            EditorKey::Preset(3) => {
                self.moving = false;
                *layout = TreeRowLayout::mixed();
            }
            EditorKey::Preset(_) => {}
        }
        *layout = layout.clone().normalized();
        before != *layout
    }
}
