//! Clickable `/goal resume` hint in a Codex pane's status footer.
//!
//! When Codex pauses a goal its footer shows the exact command that resumes
//! it (`/goal resume`). That text is only ever the provider's own chrome, so
//! detection is limited to the last two non-empty screen rows (footer plus
//! the `? for shortcuts` row) of a Codex pane: transcript prose that quotes
//! the command, or text typed into the composer higher up, never becomes a
//! link. Detection is pure and works on a `vt100::Screen`; painting and
//! activation belong to `ui` and `App`.

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ilium_core::{AgentClass, NodeId, NodeKind};
use ilium_ipc::PromptSubmissionSource;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier};

use crate::app::{App, PaneRuntime};
use crate::terminal_view::TerminalView;

/// Exact text Codex shows, and the exact text submitted when clicked.
pub const GOAL_RESUME_COMMAND: &str = "/goal resume";

/// Tooltip text shown while the pointer rests on the link.
pub const GOAL_RESUME_TOOLTIP: &str = "click to run";

const FOOTER_ROWS_SCANNED: usize = 2;

/// Screen-relative cell span of the link: `row` and the half-open column
/// range `start_column..end_column`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoalResumeLink {
    pub row: u16,
    pub start_column: u16,
    pub end_column: u16,
}

impl GoalResumeLink {
    /// Whether a screen-relative cell lies on the link.
    pub fn contains(&self, row: u16, column: u16) -> bool {
        row == self.row && (self.start_column..self.end_column).contains(&column)
    }
}

/// Finds the `/goal resume` text in the footer rows of `screen`.
pub fn find_goal_resume_link(screen: &vt100::Screen) -> Option<GoalResumeLink> {
    let (rows, columns) = screen.size();
    let row_cells: Vec<Vec<(u16, char)>> = (0..rows)
        .map(|row| {
            (0..columns)
                .filter_map(|column| {
                    let contents = screen.cell(row, column)?.contents();
                    contents.chars().next().map(|character| (column, character))
                })
                .collect()
        })
        .collect();
    let footer_rows = (0..row_cells.len())
        .rev()
        .filter(|&row| row_cells[row].iter().any(|(_, ch)| !ch.is_whitespace()))
        .take(FOOTER_ROWS_SCANNED);
    footer_rows.into_iter().find_map(|row| {
        let link = find_in_row(&row_cells[row])?;
        Some(GoalResumeLink {
            row: row as u16,
            ..link
        })
    })
}

fn find_in_row(cells: &[(u16, char)]) -> Option<GoalResumeLink> {
    let needle: Vec<char> = GOAL_RESUME_COMMAND.chars().collect();
    let start = cells.windows(needle.len()).position(|window| {
        window
            .iter()
            .zip(&needle)
            .all(|((_, found), wanted)| found == wanted)
    })?;
    let first = cells[start].0;
    let last = cells[start + needle.len() - 1].0;
    // Cells must be contiguous: a gap means the text is two unrelated runs.
    if usize::from(last - first) + 1 != needle.len() {
        return None;
    }
    // Reject a longer token such as `/goal resumed` or `x/goal resume`.
    let before = start.checked_sub(1).map(|index| cells[index]);
    if before.is_some_and(|(column, character)| column + 1 == first && !is_boundary(character)) {
        return None;
    }
    let after = cells.get(start + needle.len());
    if after.is_some_and(|&(column, character)| column == last + 1 && !is_boundary(character)) {
        return None;
    }
    Some(GoalResumeLink {
        row: 0,
        start_column: first,
        end_column: last + 1,
    })
}

fn is_boundary(character: char) -> bool {
    !(character.is_alphanumeric() || matches!(character, '/' | '-' | '_'))
}

/// Paints the link blue and underlined (bold and brighter while hovered)
/// over cells already drawn into `buffer` for a terminal area at `area`.
pub fn paint_goal_resume_link(
    buffer: &mut Buffer,
    area: Rect,
    link: GoalResumeLink,
    is_hovered: bool,
) {
    let y = area.y.saturating_add(link.row);
    if link.row >= area.height {
        return;
    }
    let (color, extra) = if is_hovered {
        (Color::LightBlue, Modifier::BOLD)
    } else {
        (Color::Blue, Modifier::empty())
    };
    for column in link.start_column..link.end_column {
        let x = area.x.saturating_add(column);
        if column >= area.width {
            break;
        }
        if let Some(cell) = buffer.cell_mut(Position::new(x, y)) {
            cell.set_fg(color);
            cell.modifier.insert(Modifier::UNDERLINED | extra);
        }
    }
}

fn is_codex_pane(app: &App, pane_id: NodeId) -> bool {
    let Some(node) = app.tree.get(pane_id) else {
        return false;
    };
    let NodeKind::Pane { status, .. } = &node.kind else {
        return false;
    };
    status
        .agent_state()
        .is_some_and(|state| state.class == AgentClass::Codex)
}

/// Link in the live (not frozen by selection or smart copy) Codex screen.
fn live_link(app: &App, pane_id: NodeId) -> Option<GoalResumeLink> {
    if !is_codex_pane(app, pane_id)
        || app.selection_terminal_source(pane_id).is_some()
        || app
            .smart_copy_session
            .as_ref()
            .is_some_and(|session| session.pane_id == pane_id)
    {
        return None;
    }
    let Some(PaneRuntime::Terminal(view)) = app.panes.get(&pane_id) else {
        return None;
    };
    view.with_screen(find_goal_resume_link)
}

/// Paints the link into the terminal area just drawn for `pane_id`.
pub(crate) fn draw_goal_resume_link(
    app: &App,
    pane_id: NodeId,
    _view: &TerminalView,
    area: Rect,
    buffer: &mut Buffer,
) {
    let Some(link) = live_link(app, pane_id) else {
        return;
    };
    let is_hovered = app
        .hovered_goal_resume
        .is_some_and(|(hovered, _)| hovered == pane_id);
    paint_goal_resume_link(buffer, area, link, is_hovered);
}

impl App {
    /// Hover and click handling for the link. Returns true when the event was
    /// consumed (a plain left click on the link).
    pub(crate) fn handle_goal_resume_mouse(
        &mut self,
        pane_id: NodeId,
        content_area: Rect,
        mouse: MouseEvent,
        position: Position,
    ) -> bool {
        if mouse.modifiers.contains(KeyModifiers::CONTROL) || !content_area.contains(position) {
            return false;
        }
        let Some(link) = live_link(self, pane_id) else {
            return false;
        };
        let row = position.y - content_area.y;
        let column = position.x - content_area.x;
        if !link.contains(row, column) {
            return false;
        }
        match mouse.kind {
            MouseEventKind::Moved => {
                self.hovered_goal_resume = Some((pane_id, position));
                false
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.hovered_goal_resume = Some((pane_id, position));
                // Same path as toolbar commands: the server inserts the text
                // and then sends a real Enter key under the pane input gate.
                if let Err(error) = self.send_terminal_submission(
                    pane_id,
                    GOAL_RESUME_COMMAND.to_owned(),
                    PromptSubmissionSource::ToolbarAction,
                ) {
                    self.status_message =
                        Some(format!("Could not queue {GOAL_RESUME_COMMAND}: {error:?}"));
                }
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen_with(rows: u16, columns: u16, text: &str) -> vt100::Parser {
        let mut parser = vt100::Parser::new(rows, columns, 0);
        parser.process(text.replace('\n', "\r\n").as_bytes());
        parser
    }

    #[test]
    fn finds_command_in_footer_with_exact_columns() {
        let parser = screen_with(
            6,
            60,
            "> composer\n\n  gpt Paused · /goal resume · ok\n  ? for shortcuts",
        );
        let link = find_goal_resume_link(parser.screen()).expect("link");
        assert_eq!(link.row, 2);
        assert_eq!(
            link.start_column,
            2 + "gpt Paused · ".chars().count() as u16
        );
        assert_eq!(link.end_column - link.start_column, 12);
        assert!(link.contains(2, link.start_column));
        assert!(!link.contains(2, link.end_column));
    }

    #[test]
    fn ignores_transcript_text_above_the_footer() {
        let parser = screen_with(
            8,
            60,
            "run /goal resume later\nmore\nmore\n> composer\n\n  footer line\n  ? for shortcuts",
        );
        assert_eq!(find_goal_resume_link(parser.screen()), None);
    }

    #[test]
    fn rejects_longer_tokens() {
        let parser = screen_with(3, 40, "  /goal resumed now\n  ? for shortcuts");
        assert_eq!(find_goal_resume_link(parser.screen()), None);
    }

    #[test]
    fn paints_blue_underline_and_hover_variant() {
        let area = Rect::new(1, 1, 20, 3);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 30, 6));
        let link = GoalResumeLink {
            row: 1,
            start_column: 2,
            end_column: 14,
        };
        paint_goal_resume_link(&mut buffer, area, link, false);
        let cell = buffer.cell(Position::new(3, 2)).unwrap();
        assert_eq!(cell.fg, Color::Blue);
        assert!(cell.modifier.contains(Modifier::UNDERLINED));
        paint_goal_resume_link(&mut buffer, area, link, true);
        let cell = buffer.cell(Position::new(3, 2)).unwrap();
        assert_eq!(cell.fg, Color::LightBlue);
        assert!(cell.modifier.contains(Modifier::BOLD));
    }
}
