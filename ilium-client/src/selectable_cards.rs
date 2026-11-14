//! Shared geometry, chrome and density rules for mutually exclusive settings
//! cards: the Agent Monitoring display-mode cards, the Left panel sizing
//! cards and the Padding mode cards all draw through this module so a card
//! looks and clicks the same on every screen.
//!
//! The module stays presentation-only: it knows nothing about the setting a
//! card changes. Screens hand it the card count, the chrome state (selected,
//! hovered, keyboard focus) and their own body lines.

use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::BorderType;
use unicode_width::UnicodeWidthStr;

use crate::theme;

/// Divide a row into cards of exactly equal width, as the user asked for
/// ("3 separate columns of equal width"). Cells that do not divide evenly are
/// left as trailing slack rather than making one card wider than its
/// neighbours. Empty geometry remains empty even on extremely small
/// terminals.
pub fn layout(area: Rect, count: usize, gap: u16) -> Vec<Rect> {
    if count == 0 {
        return Vec::new();
    }
    let count_u16 = u16::try_from(count).unwrap_or(u16::MAX);
    let usable = area
        .width
        .saturating_sub(gap.saturating_mul(count_u16.saturating_sub(1)));
    let width = usable / count_u16;
    let mut x = area.x;
    (0..count_u16)
        .map(|_| {
            let card = Rect::new(x, area.y, width, area.height);
            x = x.saturating_add(width).saturating_add(gap);
            card
        })
        .collect()
}

/// Lay the same cards out one above the other, each taking the full width.
/// Used when the content is too narrow for side-by-side cards.
pub fn stacked_layout(area: Rect, count: usize, card_height: u16, gap: u16) -> Vec<Rect> {
    let mut y = area.y;
    (0..count)
        .map(|_| {
            let card = Rect::new(area.x, y, area.width, card_height);
            y = y.saturating_add(card_height).saturating_add(gap);
            card
        })
        .collect()
}

/// Index of the card under `position`; gaps between cards are inert.
pub fn hit(cards: &[Rect], position: Position) -> Option<usize> {
    cards
        .iter()
        .position(|card| card.width > 0 && card.height > 0 && card.contains(position))
}

/// The visual state a card is drawn in. `focused` means the keyboard
/// selection sits on the row the cards stand for, so the active card also
/// carries the focus ring.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CardChrome {
    pub selected: bool,
    pub hovered: bool,
    pub focused: bool,
}

/// Box-drawing glyphs for a text-built card frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameGlyphs {
    pub top_left: &'static str,
    pub top_right: &'static str,
    pub bottom_left: &'static str,
    pub bottom_right: &'static str,
    pub side: &'static str,
    pub rule: &'static str,
}

const ROUNDED: FrameGlyphs = FrameGlyphs {
    top_left: "╭",
    top_right: "╮",
    bottom_left: "╰",
    bottom_right: "╯",
    side: "│",
    rule: "─",
};

const HEAVY: FrameGlyphs = FrameGlyphs {
    top_left: "┏",
    top_right: "┓",
    bottom_left: "┗",
    bottom_right: "┛",
    side: "┃",
    rule: "━",
};

impl CardChrome {
    /// The selected card is highlighted in two ways at once: an accent,
    /// bold border and a filled radio (see [`radio`]); hover brightens the
    /// border of an unselected card.
    pub fn border_style(self) -> Style {
        border_style(self.selected, self.hovered)
    }

    /// A heavy frame marks the card that keyboard arrows will move away
    /// from; every other card is rounded.
    pub const fn glyphs(self) -> FrameGlyphs {
        if self.focused && self.selected {
            HEAVY
        } else {
            ROUNDED
        }
    }

    /// The matching `ratatui` border type for cards drawn with a `Block`.
    pub const fn border_type(self) -> BorderType {
        if self.focused && self.selected {
            BorderType::Thick
        } else {
            BorderType::Rounded
        }
    }

    /// Title text: bold and accented when selected, bold on hover.
    pub fn title_style(self) -> Style {
        if self.selected {
            theme::selected_style().add_modifier(Modifier::BOLD)
        } else if self.hovered {
            Style::new().add_modifier(Modifier::BOLD)
        } else {
            Style::new()
        }
    }

    /// Body text: plain for the selected or hovered card, dim otherwise so
    /// the chosen option stands out without colour alone.
    pub fn body_style(self) -> Style {
        if self.selected || self.hovered {
            Style::new()
        } else {
            Style::new().add_modifier(Modifier::DIM)
        }
    }

    /// Top border line, exactly `width` cells.
    pub fn top_line(self, width: u16) -> Vec<Span<'static>> {
        let glyphs = self.glyphs();
        frame_rule(
            glyphs.top_left,
            glyphs.rule,
            glyphs.top_right,
            width,
            self.border_style(),
        )
    }

    /// Bottom border line, exactly `width` cells.
    pub fn bottom_line(self, width: u16) -> Vec<Span<'static>> {
        let glyphs = self.glyphs();
        frame_rule(
            glyphs.bottom_left,
            glyphs.rule,
            glyphs.bottom_right,
            width,
            self.border_style(),
        )
    }

    /// One body line between two side borders, padded (never clipped) to
    /// exactly `width` cells; callers truncate their own content first.
    pub fn framed_line(self, content: Vec<Span<'static>>, width: u16) -> Vec<Span<'static>> {
        let side = Span::styled(self.glyphs().side, self.border_style());
        let inner = usize::from(width.saturating_sub(2));
        let used: usize = content
            .iter()
            .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
            .sum();
        let mut line = Vec::with_capacity(content.len() + 3);
        line.push(side.clone());
        line.extend(content);
        line.push(Span::raw(" ".repeat(inner.saturating_sub(used))));
        line.push(side);
        line
    }

    /// The radio marker followed by the title, as the first body line.
    pub fn title_line(self, title: &str, width: u16) -> Vec<Span<'static>> {
        let inner = usize::from(width.saturating_sub(2));
        let text = format!(" {} {title}", radio(self.selected));
        let text = crate::settings_ui::truncate_icon_text(&text, inner);
        self.framed_line(vec![Span::styled(text, self.title_style())], width)
    }
}

fn frame_rule(
    left: &'static str,
    rule: &'static str,
    right: &'static str,
    width: u16,
    style: Style,
) -> Vec<Span<'static>> {
    let inner = usize::from(width.saturating_sub(2));
    if width < 2 {
        return vec![Span::styled(rule.repeat(usize::from(width)), style)];
    }
    vec![Span::styled(
        format!("{left}{}{right}", rule.repeat(inner)),
        style,
    )]
}

pub fn border_style(selected: bool, hovered: bool) -> Style {
    if selected {
        Style::new()
            .fg(theme::accent_bg())
            .add_modifier(Modifier::BOLD)
    } else {
        theme::border_style(hovered)
    }
}

pub const fn radio(selected: bool) -> &'static str {
    if selected {
        "(●)"
    } else {
        "( )"
    }
}

/// How much of a card survives at the current width. Decided once by a pure
/// function so rendering and hit testing cannot disagree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CardDensity {
    /// Title, description and the whole demo tree.
    Full,
    /// Title, description and a shortened demo tree.
    ShortDemo,
    /// Title and description only.
    DescriptionOnly,
    /// Cards one above the other, each full width with its description.
    Stacked,
}

/// Narrowest card (border included) that still fits a demo row such as
/// ` ├ ◐ Working toward goal` without clipping the name.
pub const FULL_DEMO_CARD_WIDTH: u16 = 26;
/// Narrowest card that still fits a demo row with a short name.
pub const SHORT_DEMO_CARD_WIDTH: u16 = 18;
/// Narrowest card whose description stays readable.
pub const DESCRIPTION_CARD_WIDTH: u16 = 14;

/// Choose the density for `count` cards side by side in `content_width`
/// cells with `gap` cells between them.
pub fn card_density(content_width: u16, count: usize, gap: u16) -> CardDensity {
    let count_u16 = u16::try_from(count.max(1)).unwrap_or(u16::MAX);
    let card_width =
        content_width.saturating_sub(gap.saturating_mul(count_u16.saturating_sub(1))) / count_u16;
    if card_width >= FULL_DEMO_CARD_WIDTH {
        CardDensity::Full
    } else if card_width >= SHORT_DEMO_CARD_WIDTH {
        CardDensity::ShortDemo
    } else if card_width >= DESCRIPTION_CARD_WIDTH {
        CardDensity::DescriptionOnly
    } else {
        CardDensity::Stacked
    }
}

#[cfg(test)]
mod tests {
    use super::{
        card_density, hit, layout, radio, stacked_layout, CardChrome, CardDensity,
        DESCRIPTION_CARD_WIDTH, FULL_DEMO_CARD_WIDTH, SHORT_DEMO_CARD_WIDTH,
    };
    use ratatui::layout::{Position, Rect};
    use unicode_width::UnicodeWidthStr;

    fn text(spans: &[ratatui::text::Span<'static>]) -> String {
        spans.iter().map(|span| span.content.to_string()).collect()
    }

    #[test]
    fn cards_share_available_width_and_gaps_are_not_clickable() {
        let cards = layout(Rect::new(2, 3, 14, 4), 3, 1);
        assert_eq!(
            cards.iter().map(|card| card.width).collect::<Vec<_>>(),
            vec![4, 4, 4]
        );
        assert_eq!(hit(&cards, Position::new(2, 3)), Some(0));
        assert_eq!(hit(&cards, Position::new(6, 3)), None);
        assert_ne!(radio(true), radio(false));
    }

    #[test]
    fn cards_are_exactly_equal_and_spare_cells_become_trailing_slack() {
        let cards = layout(Rect::new(0, 0, 13, 2), 3, 1);
        assert_eq!(
            cards.iter().map(|card| card.width).collect::<Vec<_>>(),
            vec![3, 3, 3]
        );
        assert_eq!(cards[2].right(), 11);
        assert!(layout(Rect::default(), 0, 1).is_empty());
        assert!(layout(Rect::new(0, 0, 1, 1), 3, 1)
            .iter()
            .all(|card| card.width == 0));
    }

    #[test]
    fn stacked_cards_take_the_full_width_in_order() {
        let cards = stacked_layout(Rect::new(1, 2, 30, 20), 3, 5, 1);
        assert_eq!(cards.len(), 3);
        assert!(cards.iter().all(|card| card.x == 1 && card.width == 30));
        assert_eq!(
            cards.iter().map(|card| card.y).collect::<Vec<_>>(),
            vec![2, 8, 14]
        );
        assert_eq!(hit(&cards, Position::new(5, 7)), None);
        assert_eq!(hit(&cards, Position::new(5, 8)), Some(1));
    }

    #[test]
    fn frame_lines_are_exactly_as_wide_as_the_card() {
        for chrome in [
            CardChrome::default(),
            CardChrome {
                selected: true,
                hovered: false,
                focused: true,
            },
            CardChrome {
                selected: false,
                hovered: true,
                focused: false,
            },
        ] {
            for width in [2u16, 9, 26] {
                for line in [
                    chrome.top_line(width),
                    chrome.title_line("Custom", width),
                    // framed_line pads but never clips: callers truncate to the inner width.
                    chrome.framed_line(
                        vec![ratatui::text::Span::raw(
                            crate::settings_ui::truncate_icon_text(
                                " x",
                                usize::from(width.saturating_sub(2)),
                            ),
                        )],
                        width,
                    ),
                    chrome.bottom_line(width),
                ] {
                    assert_eq!(
                        UnicodeWidthStr::width(text(&line).as_str()),
                        usize::from(width),
                        "{chrome:?} at {width}: {}",
                        text(&line)
                    );
                }
            }
        }
        let focused = CardChrome {
            selected: true,
            hovered: false,
            focused: true,
        };
        assert!(text(&focused.top_line(10)).starts_with('┏'));
        assert!(text(&CardChrome::default().top_line(10)).starts_with('╭'));
        assert!(text(&focused.title_line("Normal", 20)).contains("(●) Normal"));
        assert!(text(&CardChrome::default().title_line("Normal", 20)).contains("( ) Normal"));
    }

    #[test]
    fn density_tiers_follow_the_per_card_width() {
        let width_for = |card: u16| card * 3 + 2;
        assert_eq!(
            card_density(width_for(FULL_DEMO_CARD_WIDTH), 3, 1),
            CardDensity::Full
        );
        assert_eq!(
            card_density(width_for(FULL_DEMO_CARD_WIDTH) - 1, 3, 1),
            CardDensity::ShortDemo
        );
        assert_eq!(
            card_density(width_for(SHORT_DEMO_CARD_WIDTH), 3, 1),
            CardDensity::ShortDemo
        );
        assert_eq!(
            card_density(width_for(SHORT_DEMO_CARD_WIDTH) - 1, 3, 1),
            CardDensity::DescriptionOnly
        );
        assert_eq!(
            card_density(width_for(DESCRIPTION_CARD_WIDTH), 3, 1),
            CardDensity::DescriptionOnly
        );
        assert_eq!(
            card_density(width_for(DESCRIPTION_CARD_WIDTH) - 1, 3, 1),
            CardDensity::Stacked
        );
        assert_eq!(card_density(0, 3, 1), CardDensity::Stacked);
        assert_eq!(card_density(200, 0, 1), CardDensity::Full);
    }
}
