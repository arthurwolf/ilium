//! Shared geometry for the live tree and the settings specimen.
use ratatui::text::{Line, Span};
use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TreeRowElement {
    Identity,
    Objective,
    Activity,
    Title,
}
impl TreeRowElement {
    pub const ALL: [Self; 4] = [Self::Identity, Self::Objective, Self::Activity, Self::Title];
    pub fn label(self) -> &'static str {
        match self {
            Self::Identity => "Identity",
            Self::Objective => "Objective",
            Self::Activity => "Activity",
            Self::Title => "Title / left-right divider",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TreeRowLayout {
    pub order: Vec<TreeRowElement>,
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
    pub fn order(&self) -> &[TreeRowElement] {
        &self.order
    }
    pub fn is_visible(&self, element: TreeRowElement) -> bool {
        element == TreeRowElement::Title || !self.hidden.contains(&element)
    }
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
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
    /// Icon widths are measured from the caller's padded spans, preserving compact modes.
    /// Title gets one cell before icons when the available width is constrained.
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
        // Preserve complete glyphs and the most urgent signal as space shrinks.
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
    pub fn compose(
        &self,
        identity: Span<'static>,
        objective: Span<'static>,
        activity: Span<'static>,
        title: Span<'static>,
        width: usize,
    ) -> Line<'static> {
        let placements =
            self.placements(identity.width(), objective.width(), activity.width(), width);
        let mut result = Vec::new();
        for (element, _, size) in placements {
            let span = match element {
                TreeRowElement::Identity => &identity,
                TreeRowElement::Objective => &objective,
                TreeRowElement::Activity => &activity,
                TreeRowElement::Title => &title,
            };
            let mut text = String::new();
            let mut used = 0;
            for grapheme in span.content.graphemes(true) {
                let cells = UnicodeWidthStr::width(grapheme);
                if used + cells > size {
                    break;
                }
                text.push_str(grapheme);
                used += cells;
            }
            text.push_str(&" ".repeat(size.saturating_sub(used)));
            result.push(Span::styled(text, span.style));
        }
        Line::from(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalizes_partial_duplicate_and_hidden_title() {
        let layout = TreeRowLayout {
            order: vec![TreeRowElement::Title, TreeRowElement::Title],
            hidden: vec![TreeRowElement::Title, TreeRowElement::Activity],
        }
        .normalized();
        assert_eq!(layout.order().len(), 4);
        assert!(layout.is_visible(TreeRowElement::Title));
        assert!(!layout.is_visible(TreeRowElement::Activity));
    }
    #[test]
    fn right_slots_anchor_and_title_retains_space() {
        let layout = TreeRowLayout::all_right();
        assert_eq!(
            layout.placements(3, 3, 2, 20),
            vec![
                (TreeRowElement::Title, 0, 12),
                (TreeRowElement::Identity, 12, 3),
                (TreeRowElement::Objective, 15, 3),
                (TreeRowElement::Activity, 18, 2)
            ]
        );
        assert_eq!(
            layout.placements(3, 3, 2, 2)[0],
            (TreeRowElement::Title, 0, 2)
        );
    }
    #[test]
    fn composition_keeps_graphemes_and_styles_in_bounds() {
        let line = TreeRowLayout::all_right().compose(
            Span::raw("🦀 "),
            Span::raw("🎯 "),
            Span::raw("⠋ "),
            Span::raw("café家"),
            10,
        );
        assert_eq!(line.width(), 10);
        let narrow = TreeRowLayout::default().compose(
            Span::raw("🦀"),
            Span::raw(""),
            Span::raw(""),
            Span::raw("家"),
            1,
        );
        assert_eq!(narrow.width(), 1);
    }
    #[test]
    fn serde_round_trip_preserves_order_and_visibility() {
        let layout = TreeRowLayout {
            hidden: vec![TreeRowElement::Objective],
            ..TreeRowLayout::mixed()
        };
        let encoded = toml::to_string(&layout).unwrap();
        assert_eq!(toml::from_str::<TreeRowLayout>(&encoded).unwrap(), layout);
    }
}
