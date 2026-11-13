//! Shared geometry for an editor pane's chrome: the always-visible top
//! toolbar (`editor_toolbar.rs`), the main content area (source text or
//! rendered markdown), and the optional right-hand minimap column.
//! Kept as one small module (rather than folded into `layout.rs`) since
//! this geometry only applies to `PaneRuntime::Editor` panes -- terminal
//! panes never see a toolbar or minimap, and `layout.rs`'s `UiLayout` is
//! the PTY-sizing source of truth that every pane kind shares.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub const TOOLBAR_HEIGHT: u16 = 1;
pub const MINIMAP_WIDTH: u16 = 8;

/// The three regions inside an editor pane's content box.
pub struct EditorChromeLayout {
    pub toolbar_area: Rect,
    pub content_area: Rect,
    /// The one-column strip right of the text, reserved so the scrollbar
    /// never draws over text. Zero-width when the body is too narrow.
    pub scrollbar_area: Rect,
    /// `None` when `show_minimap` is off, or the pane is too narrow to
    /// spare `MINIMAP_WIDTH` columns without leaving no room for content.
    pub minimap_area: Option<Rect>,
}

/// Splits `pane_content_area` (the editor pane's interior, inside its
/// border -- `UiLayout::pane_content_area`) into toolbar/content/minimap.
pub fn compute(pane_content_area: Rect, show_minimap: bool) -> EditorChromeLayout {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(TOOLBAR_HEIGHT), Constraint::Min(1)])
        .split(pane_content_area);
    let toolbar_area = rows[0];
    let body_area = rows[1];

    let can_fit_minimap = show_minimap && body_area.width > MINIMAP_WIDTH;
    let (text_and_scrollbar_area, minimap_area) = if can_fit_minimap {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(MINIMAP_WIDTH)])
            .split(body_area);
        (cols[0], Some(cols[1]))
    } else {
        (body_area, None)
    };
    let (content_area, scrollbar_area) = split_scrollbar_column(text_and_scrollbar_area);
    EditorChromeLayout {
        toolbar_area,
        content_area,
        scrollbar_area,
        minimap_area,
    }
}

/// Splits off the rightmost column of `area` for the scrollbar. Text keeps
/// every other column, so scrollbar glyphs never overwrite source or
/// rendered markdown.
fn split_scrollbar_column(area: Rect) -> (Rect, Rect) {
    if area.width < 2 {
        return (area, Rect::new(area.right(), area.y, 0, area.height));
    }
    let content_area = Rect {
        width: area.width - 1,
        ..area
    };
    let scrollbar_area = Rect::new(area.right() - 1, area.y, 1, area.height);
    (content_area, scrollbar_area)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserves_toolbar_row_and_minimap_column() {
        let layout = compute(Rect::new(0, 0, 40, 20), true);
        assert_eq!(layout.toolbar_area, Rect::new(0, 0, 40, 1));
        assert_eq!(layout.content_area, Rect::new(0, 1, 31, 19));
        assert_eq!(layout.scrollbar_area, Rect::new(31, 1, 1, 19));
        assert_eq!(layout.minimap_area, Some(Rect::new(32, 1, 8, 19)));
    }

    #[test]
    fn drops_minimap_when_disabled() {
        let layout = compute(Rect::new(0, 0, 40, 20), false);
        assert_eq!(layout.content_area, Rect::new(0, 1, 39, 19));
        assert_eq!(layout.scrollbar_area, Rect::new(39, 1, 1, 19));
        assert_eq!(layout.minimap_area, None);
    }

    #[test]
    fn drops_minimap_when_too_narrow() {
        let layout = compute(Rect::new(0, 0, 8, 20), true);
        assert_eq!(layout.content_area.width, 7);
        assert_eq!(layout.scrollbar_area.width, 1);
        assert_eq!(layout.minimap_area, None);
    }
}
