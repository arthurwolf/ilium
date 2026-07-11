//! Shared screen geometry for rendering, resize handling, and mouse
//! hit-testing. Keeping these rectangles in one module prevents the PTY size,
//! rendered pane, and pointer-coordinate conversion from drifting apart.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Width of the left tree column, in terminal columns.
pub const TREE_WIDTH: u16 = 32;

/// The complete geometry of one rendered illium frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct UiLayout {
    pub screen_area: Rect,
    pub tree_area: Rect,
    pub pane_area: Rect,
    pub pane_content_area: Rect,
    pub status_area: Rect,
}

impl UiLayout {
    /// Computes every stable region from the terminal's full area.
    pub fn from_screen_area(screen_area: Rect) -> Self {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(screen_area);
        let main_area = rows[0];
        let status_area = rows[1];

        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(TREE_WIDTH), Constraint::Min(1)])
            .split(main_area);

        // Widened by one column so the tree's right border lands on the
        // same column as the pane's left border -- `theme::block`'s
        // `merge_borders` then fuses the two into a single connected
        // divider (a `┬`/`┴` joint at top/bottom) instead of drawing two
        // separate borders side by side.
        let tree_area = Rect::new(
            columns[0].x,
            columns[0].y,
            columns[0].width.saturating_add(1),
            columns[0].height,
        );
        let pane_area = columns[1];

        // The pane's Block consumes one cell on every edge. Saturating math
        // keeps this valid even in a terminal narrower than the tree column.
        let pane_content_area = Rect::new(
            pane_area.x.saturating_add(1),
            pane_area.y.saturating_add(1),
            pane_area.width.saturating_sub(2).max(1),
            pane_area.height.saturating_sub(2).max(1),
        );

        Self {
            screen_area,
            tree_area,
            pane_area,
            pane_content_area,
            status_area,
        }
    }

    /// Returns the rows/columns that the focused PTY must use.
    pub fn pane_content_size(self) -> (u16, u16) {
        (self.pane_content_area.height, self.pane_content_area.width)
    }
}

/// Standard ratatui "centered popup" layout helper: `percent_x` /
/// `percent_y` of `area`, centered on both axes. Shared by every
/// percentage-sized overlay (`ui::draw`'s explorer popup, `help::render`)
/// so their geometry can never drift apart from each other.
pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_content_excludes_status_tree_and_border() {
        let layout = UiLayout::from_screen_area(Rect::new(0, 0, 120, 40));
        assert_eq!(layout.tree_area, Rect::new(0, 0, TREE_WIDTH + 1, 39));
        assert_eq!(layout.pane_area, Rect::new(TREE_WIDTH, 0, 88, 39));
        assert_eq!(layout.pane_content_size(), (37, 86));
    }
}
