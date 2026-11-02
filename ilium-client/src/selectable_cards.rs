//! Shared geometry and chrome for mutually exclusive settings cards.

use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};

use crate::theme;

/// Divide a row into equally sized cards, distributing spare cells from left
/// to right. Empty geometry remains empty even on extremely small terminals.
pub fn layout(area: Rect, count: usize, gap: u16) -> Vec<Rect> {
    if count == 0 {
        return Vec::new();
    }
    let count_u16 = u16::try_from(count).unwrap_or(u16::MAX);
    let usable = area.width.saturating_sub(gap.saturating_mul(count_u16.saturating_sub(1)));
    let base = usable / count_u16;
    let remainder = usable % count_u16;
    let mut x = area.x;
    (0..count_u16).map(|index| {
        let width = base + u16::from(index < remainder);
        let card = Rect::new(x, area.y, width, area.height);
        x = x.saturating_add(width).saturating_add(gap);
        card
    }).collect()
}

pub fn hit(cards: &[Rect], position: Position) -> Option<usize> {
    cards.iter().position(|card| card.width > 0 && card.height > 0 && card.contains(position))
}

pub fn border_style(selected: bool, hovered: bool) -> Style {
    if selected {
        Style::new().fg(theme::accent_bg()).add_modifier(Modifier::BOLD)
    } else {
        theme::border_style(hovered)
    }
}

pub const fn radio(selected: bool) -> &'static str {
    if selected { "(●)" } else { "( )" }
}

#[cfg(test)]
mod tests {
    use super::{hit, layout, radio};
    use ratatui::layout::{Position, Rect};

    #[test]
    fn cards_share_available_width_and_gaps_are_not_clickable() {
        let cards = layout(Rect::new(2, 3, 14, 4), 3, 1);
        assert_eq!(cards.iter().map(|card| card.width).collect::<Vec<_>>(), vec![4, 4, 4]);
        assert_eq!(hit(&cards, Position::new(2, 3)), Some(0));
        assert_eq!(hit(&cards, Position::new(6, 3)), None);
        assert_ne!(radio(true), radio(false));
    }

    #[test]
    fn remainder_is_distributed_and_empty_input_is_safe() {
        let cards = layout(Rect::new(0, 0, 13, 2), 3, 1);
        assert_eq!(cards.iter().map(|card| card.width).collect::<Vec<_>>(), vec![4, 4, 3]);
        assert!(layout(Rect::default(), 0, 1).is_empty());
    }
}
