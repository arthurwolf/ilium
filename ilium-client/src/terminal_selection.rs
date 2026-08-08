//! Client-local, mouse-driven text selection over one terminal pane's
//! current `vt100` screen.
//!
//! Every terminal-pane mouse event is otherwise forwarded to the server as
//! `ClientRequest::MouseInput` (see `mouse::to_ipc_mouse_event`'s doc
//! comment), which only reaches the child PTY if the foreground app itself
//! negotiated an xterm mouse protocol -- a plain shell prompt never does, so
//! a drag there was previously silent both locally and remotely (see
//! `ilium-platform::open_external`'s sibling investigation into why the
//! outer terminal's own selection breaks once `EnableMouseCapture` claims
//! these events for ilium). While
//! `UiSettings::terminal_text_selection_enabled` is on, a left-button drag
//! over a terminal pane's content becomes this local highlight and clipboard
//! copy instead -- a substitute for both the (usually absent) forwarding and
//! the outer terminal's own selection, not an addition to either.

use ilium_core::NodeId;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::Modifier;
use ratatui::widgets::Widget;
use ratatui::Frame;

/// One cell position within a terminal pane's current screen, relative to
/// its content area's top-left visible cell (row 0/column 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionPoint {
    pub row: u16,
    pub column: u16,
}

impl SelectionPoint {
    pub const fn new(row: u16, column: u16) -> Self {
        Self { row, column }
    }
}

/// An in-progress or finished drag selection. `anchor` is where the left
/// button went down; `cursor` follows the pointer through `Drag` events (and
/// stays put once the button is released).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSelection {
    pub pane_id: NodeId,
    pub anchor: SelectionPoint,
    pub cursor: SelectionPoint,
}

impl TerminalSelection {
    /// Starts a new selection with both ends at `at` -- the state a fresh
    /// mouse-down produces before any `Drag` event has moved the cursor.
    pub const fn new(pane_id: NodeId, at: SelectionPoint) -> Self {
        Self {
            pane_id,
            anchor: at,
            cursor: at,
        }
    }

    /// `true` when no drag has moved `cursor` away from `anchor` -- a plain
    /// click, which every caller treats as "nothing selected" rather than a
    /// one-cell selection.
    pub fn is_empty(&self) -> bool {
        self.anchor == self.cursor
    }

    /// `(start, end)` in top-to-bottom, left-to-right screen order,
    /// regardless of which direction the drag actually went.
    pub fn normalized(&self) -> (SelectionPoint, SelectionPoint) {
        let anchor_key = (self.anchor.row, self.anchor.column);
        let cursor_key = (self.cursor.row, self.cursor.column);
        if anchor_key <= cursor_key {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        }
    }
}

/// Flattens `start..=end` (inclusive of the cell under the cursor at `end`)
/// into copyable plain text via `vt100::Screen::contents_between`, which
/// already implements the "linear selection" model a normal terminal
/// emulator uses -- the first row keeps only its tail from `start.column`,
/// the last row keeps only its head through `end.column`, every row between
/// is taken whole, and a soft-wrapped row never gets an inserted newline.
/// Returns `None` for an empty (unmoved) selection.
pub fn text(screen: &vt100::Screen, selection: &TerminalSelection) -> Option<String> {
    if selection.is_empty() {
        return None;
    }
    let (start, end) = selection.normalized();
    let (_, cols) = screen.size();
    // `contents_between`'s `end_col` is exclusive -- without the +1 the cell
    // the cursor is actually sitting on would be dropped from the copy.
    let end_col = end.column.saturating_add(1).min(cols);
    Some(screen.contents_between(start.row, start.column, end.row, end_col))
}

/// Draws the active selection as reversed-video cells over `content_area`.
/// A no-op for an empty selection or one that belongs to a different pane
/// than the one currently rendering.
pub fn render_highlight(
    frame: &mut Frame,
    content_area: Rect,
    screen_size: (u16, u16),
    selection: &TerminalSelection,
) {
    if selection.is_empty() {
        return;
    }
    let (start, end) = selection.normalized();
    frame.render_widget(
        SelectionHighlight {
            content_area,
            screen_size,
            start,
            end,
        },
        content_area,
    );
}

struct SelectionHighlight {
    content_area: Rect,
    screen_size: (u16, u16),
    start: SelectionPoint,
    end: SelectionPoint,
}

impl Widget for SelectionHighlight {
    fn render(self, _area: Rect, buf: &mut Buffer) {
        let (rows, cols) = self.screen_size;
        if rows == 0 || cols == 0 {
            return;
        }
        let last_row = rows - 1;
        let last_col = cols - 1;
        let end_row = self.end.row.min(last_row);
        for row in self.start.row..=end_row {
            let row_start = if row == self.start.row {
                self.start.column
            } else {
                0
            };
            let row_end = if row == self.end.row {
                self.end.column.min(last_col)
            } else {
                last_col
            };
            for column in row_start..=row_end {
                let position = Position::new(
                    self.content_area.x.saturating_add(column),
                    self.content_area.y.saturating_add(row),
                );
                if let Some(cell) = buf.cell_mut(position) {
                    cell.modifier.insert(Modifier::REVERSED);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::NodeId;

    fn any_pane_id() -> NodeId {
        NodeId(1)
    }

    fn screen_with(lines: &[&str]) -> vt100::Screen {
        let mut parser = vt100::Parser::new(lines.len() as u16, 40, 0);
        parser.process(lines.join("\r\n").as_bytes());
        parser.screen().clone()
    }

    #[test]
    fn a_fresh_selection_is_empty() {
        let pane_id = any_pane_id();
        let selection = TerminalSelection::new(pane_id, SelectionPoint::new(0, 0));
        assert!(selection.is_empty());
        assert_eq!(text(&screen_with(&["hello"]), &selection), None);
    }

    #[test]
    fn normalized_orders_a_backward_drag() {
        let pane_id = any_pane_id();
        let selection = TerminalSelection {
            pane_id,
            anchor: SelectionPoint::new(2, 5),
            cursor: SelectionPoint::new(0, 1),
        };
        assert_eq!(
            selection.normalized(),
            (SelectionPoint::new(0, 1), SelectionPoint::new(2, 5))
        );
    }

    #[test]
    fn extracts_text_within_a_single_row() {
        let pane_id = any_pane_id();
        let screen = screen_with(&["hello world"]);
        let selection = TerminalSelection {
            pane_id,
            anchor: SelectionPoint::new(0, 0),
            cursor: SelectionPoint::new(0, 4),
        };
        assert_eq!(text(&screen, &selection), Some("hello".to_string()));
    }

    #[test]
    fn extracts_text_spanning_multiple_rows() {
        let pane_id = any_pane_id();
        let screen = screen_with(&["first line", "second line", "third line"]);
        let selection = TerminalSelection {
            pane_id,
            anchor: SelectionPoint::new(0, 6),
            cursor: SelectionPoint::new(2, 4),
        };
        assert_eq!(
            text(&screen, &selection),
            Some("line\nsecond line\nthird".to_string())
        );
    }
}
