//! Shared Kanban board rendering and pointer geometry.
//!
//! Card height, contiguous placement, detail-panel allocation, and mouse
//! hit-testing all originate here so changing the preview-line setting cannot
//! make clicks drift away from what the terminal actually shows.

use std::borrow::Cow;

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Widget, Wrap,
};
use ratatui::Frame;

use crate::board::{checkbox_occurrences, BoardCard, BoardPane, CardDetailEditor, CardEditorField};
use crate::theme;

const DETAIL_PANEL_WIDTH_DIVISOR: u16 = 3;
const CARD_BORDER_ROWS: u16 = 2;
const HINT_HEIGHT: u16 = 1;
const HORIZONTAL_SCROLLBAR_HEIGHT: u16 = 1;
const DETAIL_CLOSE_LABEL: &str = "×";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoardLayout {
    pub columns_area: Rect,
    pub horizontal_scrollbar_area: Option<Rect>,
    pub detail_area: Option<Rect>,
    pub hint_area: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnViewport {
    pub first_column: usize,
    pub visible_column_count: usize,
    pub maximum_scroll: usize,
    pub areas: Vec<(usize, Rect)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetailEditorLayout {
    pub title_area: Rect,
    pub body_area: Rect,
    pub footer_area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardHit {
    Column {
        column_index: usize,
    },
    Card {
        column_index: usize,
        card_index: usize,
    },
    CardCheckbox {
        column_index: usize,
        card_index: usize,
        checkbox_index: usize,
    },
    HorizontalScrollbar {
        column_scroll: usize,
    },
    DetailClose,
    DetailTitle,
    DetailBody,
}

/// Allocates the board rows and, while details are open, the rightmost third.
pub fn compute_layout(
    area: Rect,
    is_detail_panel_open: bool,
    column_count: usize,
    minimum_column_width: u16,
) -> BoardLayout {
    let content_height = area.height.saturating_sub(HINT_HEIGHT);
    let content_area = Rect::new(area.x, area.y, area.width, content_height);
    let hint_area = Rect::new(
        area.x,
        area.y.saturating_add(content_height),
        area.width,
        area.height.min(HINT_HEIGHT),
    );
    let (columns_content_area, detail_area) = if is_detail_panel_open && content_area.width >= 3 {
        let detail_width = content_area.width / DETAIL_PANEL_WIDTH_DIVISOR;
        let columns_width = content_area.width - detail_width;
        (
            Rect::new(
                content_area.x,
                content_area.y,
                columns_width,
                content_area.height,
            ),
            Some(Rect::new(
                content_area.x.saturating_add(columns_width),
                content_area.y,
                detail_width,
                content_area.height,
            )),
        )
    } else {
        (content_area, None)
    };
    let has_horizontal_overflow = column_count > 0
        && usize::from(columns_content_area.width)
            < column_count.saturating_mul(usize::from(minimum_column_width.max(1)));
    let scrollbar_height = if has_horizontal_overflow {
        HORIZONTAL_SCROLLBAR_HEIGHT.min(columns_content_area.height)
    } else {
        0
    };
    let columns_height = columns_content_area.height.saturating_sub(scrollbar_height);
    let columns_area = Rect::new(
        columns_content_area.x,
        columns_content_area.y,
        columns_content_area.width,
        columns_height,
    );
    let horizontal_scrollbar_area = has_horizontal_overflow.then(|| {
        Rect::new(
            columns_content_area.x,
            columns_content_area.y.saturating_add(columns_height),
            columns_content_area.width,
            scrollbar_height,
        )
    });
    BoardLayout {
        columns_area,
        horizontal_scrollbar_area,
        detail_area,
        hint_area,
    }
}

/// Number of complete minimum-width columns that fit in one page. A terminal
/// narrower than the configured minimum still shows one clipped column.
pub fn visible_column_count(area: Rect, column_count: usize, minimum_column_width: u16) -> usize {
    if column_count == 0 {
        return 0;
    }
    (usize::from(area.width) / usize::from(minimum_column_width.max(1)))
        .max(1)
        .min(column_count)
}

/// Returns the scrolled subset plus exact equal-width rectangles used by
/// rendering, hit-testing, drop targeting, and scrollbar interaction.
pub fn column_viewport(board: &BoardPane, area: Rect, minimum_column_width: u16) -> ColumnViewport {
    let visible_column_count =
        visible_column_count(area, board.columns.len(), minimum_column_width);
    if visible_column_count == 0 {
        return ColumnViewport {
            first_column: 0,
            visible_column_count: 0,
            maximum_scroll: 0,
            areas: Vec::new(),
        };
    }
    let maximum_scroll = board.columns.len().saturating_sub(visible_column_count);
    let first_column = board.column_scroll.min(maximum_scroll);
    let rectangles = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![
            Constraint::Ratio(1, visible_column_count as u32);
            visible_column_count
        ])
        .split(area)
        .to_vec();
    let areas = rectangles
        .into_iter()
        .enumerate()
        .map(|(visible_index, area)| (first_column + visible_index, area))
        .collect();
    ColumnViewport {
        first_column,
        visible_column_count,
        maximum_scroll,
        areas,
    }
}

/// Returns one visible card rectangle. Cards are contiguous: there is no
/// spacer row between one card's bottom border and the next card's top border.
pub fn card_area(column_inner: Rect, card_index: usize, preview_lines: u16) -> Option<Rect> {
    let card_height = preview_lines.saturating_add(CARD_BORDER_ROWS);
    let offset = u16::try_from(card_index).ok()?.saturating_mul(card_height);
    if offset >= column_inner.height {
        return None;
    }
    Some(Rect::new(
        column_inner.x,
        column_inner.y.saturating_add(offset),
        column_inner.width,
        card_height.min(column_inner.height - offset),
    ))
}

/// Aerated title/body/footer rectangles inside the detail panel's outer
/// border. Rendering and pointer focus use this exact allocation.
pub fn detail_editor_layout(detail_area: Rect) -> DetailEditorLayout {
    let inner = Block::bordered()
        .inner(detail_area)
        .inner(Margin::new(2, 1));
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(1),
            Constraint::Min(4),
            Constraint::Length(1),
        ])
        .split(inner);
    DetailEditorLayout {
        title_area: rows[0],
        body_area: rows[2],
        footer_area: rows[3],
    }
}

/// Maps one pointer position back to the exact board element rendered there.
pub fn hit_test(
    board: &BoardPane,
    area: Rect,
    preview_lines: u16,
    minimum_column_width: u16,
    position: Position,
) -> Option<BoardHit> {
    let layout = compute_layout(
        area,
        board.is_detail_panel_open,
        board.columns.len(),
        minimum_column_width,
    );
    if let Some(detail_area) = layout.detail_area {
        if detail_close_area(detail_area).contains(position) {
            return Some(BoardHit::DetailClose);
        }
        let editor_layout = detail_editor_layout(detail_area);
        if editor_layout.title_area.contains(position) {
            return Some(BoardHit::DetailTitle);
        }
        if editor_layout.body_area.contains(position) {
            return Some(BoardHit::DetailBody);
        }
    }
    if let Some(scrollbar_area) = layout.horizontal_scrollbar_area {
        if scrollbar_area.contains(position) {
            let column_scroll =
                horizontal_scroll_target(board, scrollbar_area, minimum_column_width, position);
            return Some(BoardHit::HorizontalScrollbar { column_scroll });
        }
    }
    for (column_index, column_area) in
        column_viewport(board, layout.columns_area, minimum_column_width).areas
    {
        if !column_area.contains(position) {
            continue;
        }
        let inner = Block::bordered().inner(column_area);
        for card_index in 0..board.columns[column_index].cards.len() {
            let Some(card_area) = card_area(inner, card_index, preview_lines) else {
                break;
            };
            if !card_area.contains(position) {
                continue;
            }
            if let Some(checkbox_index) =
                card_checkbox_areas(&board.columns[column_index].cards[card_index], card_area)
                    .iter()
                    .position(|area| area.contains(position))
            {
                return Some(BoardHit::CardCheckbox {
                    column_index,
                    card_index,
                    checkbox_index,
                });
            } else {
                return Some(BoardHit::Card {
                    column_index,
                    card_index,
                });
            }
        }
        return Some(BoardHit::Column { column_index });
    }
    None
}

/// Resolves a drag release into an insertion index using the same card grid.
pub fn card_drop_target(
    board: &BoardPane,
    area: Rect,
    preview_lines: u16,
    minimum_column_width: u16,
    position: Position,
) -> Option<(usize, usize)> {
    let layout = compute_layout(
        area,
        board.is_detail_panel_open,
        board.columns.len(),
        minimum_column_width,
    );
    for (column_index, column_area) in
        column_viewport(board, layout.columns_area, minimum_column_width).areas
    {
        if !column_area.contains(position) {
            continue;
        }
        let inner = Block::bordered().inner(column_area);
        if position.y <= inner.y {
            return Some((column_index, 0));
        }
        let card_height = preview_lines.saturating_add(CARD_BORDER_ROWS).max(1);
        let card_index = usize::from(position.y.saturating_sub(inner.y) / card_height)
            .min(board.columns[column_index].cards.len());
        return Some((column_index, card_index));
    }
    None
}

/// Maps a click on the scrollbar track to a valid first-column index.
fn horizontal_scroll_target(
    board: &BoardPane,
    scrollbar_area: Rect,
    minimum_column_width: u16,
    position: Position,
) -> usize {
    let visible_column_count =
        visible_column_count(scrollbar_area, board.columns.len(), minimum_column_width);
    let maximum_scroll = board.columns.len().saturating_sub(visible_column_count);
    if maximum_scroll == 0 || scrollbar_area.width <= 1 {
        return 0;
    }
    let position_in_track = usize::from(position.x.saturating_sub(scrollbar_area.x));
    position_in_track.saturating_mul(maximum_scroll)
        / usize::from(scrollbar_area.width.saturating_sub(1))
}

/// Draws columns, contiguous card previews, the optional editor panel, and
/// the horizontal viewport affordance.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    board: &BoardPane,
    preview_lines: u16,
    minimum_column_width: u16,
) {
    let layout = compute_layout(
        area,
        board.is_detail_panel_open,
        board.columns.len(),
        minimum_column_width,
    );
    let viewport = column_viewport(board, layout.columns_area, minimum_column_width);
    render_columns(frame, layout.columns_area, board, preview_lines, &viewport);
    if let (Some(detail_area), Some(editor)) = (layout.detail_area, board.detail_editor.as_ref()) {
        render_detail_panel(frame, detail_area, editor);
    }
    if let Some(scrollbar_area) = layout.horizontal_scrollbar_area {
        let mut state = ScrollbarState::new(board.columns.len())
            .position(viewport.first_column)
            .viewport_content_length(viewport.visible_column_count);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::HorizontalBottom)
            .begin_symbol(Some("◀"))
            .end_symbol(Some("▶"))
            .track_symbol(Some("─"))
            .style(theme::border_style(false));
        frame.render_stateful_widget(scrollbar, scrollbar_area, &mut state);
    }
    let hint = if board.is_detail_panel_open {
        "Tab field · type to edit · every change saves immediately · Esc close"
    } else {
        "←/→ column · ↑/↓ header/card · Enter details · n card · c column · e rename · d delete · Shift+arrows move · drag cards"
    };
    frame.render_widget(
        Paragraph::new(Span::styled(hint, Style::new().add_modifier(Modifier::DIM))),
        layout.hint_area,
    );
}

fn render_columns(
    frame: &mut Frame,
    area: Rect,
    board: &BoardPane,
    preview_lines: u16,
    viewport: &ColumnViewport,
) {
    if board.columns.is_empty() {
        frame.render_widget(
            Paragraph::new("No columns yet. Press c to create one."),
            area,
        );
        return;
    }
    for (column_index, column_area) in viewport.areas.iter().copied() {
        let column = &board.columns[column_index];
        let is_selected_column = column_index == board.selected_column;
        let title_style = if is_selected_column {
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::new().add_modifier(Modifier::BOLD)
        };
        let block = Block::bordered()
            .border_style(theme::border_style(is_selected_column))
            .title(Line::from(vec![
                Span::styled(format!(" {} ", column.title), title_style),
                Span::styled(
                    column.cards.len().to_string(),
                    Style::new().add_modifier(Modifier::DIM),
                ),
            ]));
        let inner = block.inner(column_area);
        frame.render_widget(block, column_area);
        if column.cards.is_empty() {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    "drop a card here",
                    Style::new().add_modifier(Modifier::DIM | Modifier::ITALIC),
                )),
                inner,
            );
        } else {
            for (card_index, card) in column.cards.iter().enumerate() {
                let Some(area) = card_area(inner, card_index, preview_lines) else {
                    break;
                };
                let is_selected = is_selected_column && board.selected_card == Some(card_index);
                let is_drag_source = board.drag_source == Some((column_index, card_index));
                let border_style = if is_drag_source {
                    Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    theme::border_style(is_selected)
                };
                let text_style = if is_drag_source {
                    Style::new().add_modifier(Modifier::DIM)
                } else if is_selected {
                    Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                } else {
                    Style::new()
                };
                frame.render_widget(
                    Paragraph::new(Span::styled(atomic_checkbox_title(&card.title), text_style))
                        .block(Block::bordered().border_style(border_style))
                        .wrap(Wrap { trim: true }),
                    area,
                );
            }
        }
        render_drop_indicator(frame, board, column_index, inner, preview_lines);
    }
}

fn render_drop_indicator(
    frame: &mut Frame,
    board: &BoardPane,
    column_index: usize,
    column_inner: Rect,
    preview_lines: u16,
) {
    let Some((target_column, insertion_index)) = board.drag_target else {
        return;
    };
    if target_column != column_index || column_inner.height == 0 {
        return;
    }
    let card_height = preview_lines.saturating_add(CARD_BORDER_ROWS).max(1);
    let requested_y = column_inner.y.saturating_add(
        u16::try_from(insertion_index)
            .unwrap_or(u16::MAX)
            .saturating_mul(card_height),
    );
    let y = requested_y.min(column_inner.bottom().saturating_sub(1));
    frame.render_widget(
        Paragraph::new("━".repeat(usize::from(column_inner.width)))
            .style(Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Rect::new(column_inner.x, y, column_inner.width, 1),
    );
}

fn render_detail_panel(frame: &mut Frame, area: Rect, editor: &CardDetailEditor) {
    let block = Block::bordered()
        .border_style(theme::border_style(true))
        .title(Span::styled(
            " Card details ",
            Style::new().add_modifier(Modifier::BOLD),
        ));
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(DETAIL_CLOSE_LABEL).style(theme::selected_style()),
        detail_close_area(area),
    );
    let layout = detail_editor_layout(area);
    let mut title = editor.title.clone();
    title.set_block(
        Block::bordered()
            .title(" Title ")
            .border_style(theme::border_style(editor.focus == CardEditorField::Title)),
    );
    title.set_cursor_style(if editor.focus == CardEditorField::Title {
        theme::selected_style()
    } else {
        Style::default()
    });
    frame.render_widget(&title, layout.title_area);

    let mut body = editor.body.clone();
    body.set_block(
        Block::bordered()
            .title(" Notes ")
            .border_style(theme::border_style(editor.focus == CardEditorField::Body)),
    );
    body.set_cursor_style(if editor.focus == CardEditorField::Body {
        theme::selected_style()
    } else {
        Style::default()
    });
    frame.render_widget(&body, layout.body_area);
    frame.render_widget(
        Paragraph::new("Tab switches field · changes save immediately")
            .style(Style::new().add_modifier(Modifier::DIM)),
        layout.footer_area,
    );
}

fn detail_close_area(detail_area: Rect) -> Rect {
    // For a narrow panel (width 1 or 2 -- content_area.width in 3..=5 yields
    // a detail_width of 1 in `compute_layout`), `right() - 2` lands to the
    // left of `detail_area.x`, i.e. inside the columns area. Clamping to
    // `detail_area.x` keeps both the rendered "x" and its hit-test target
    // inside the panel instead of drifting onto whatever is drawn behind it.
    let x = detail_area.right().saturating_sub(2).max(detail_area.x);
    Rect::new(
        x,
        detail_area.y,
        detail_area.width.min(1),
        detail_area.height.min(1),
    )
}

/// Replaces an *unchecked* checkbox marker's fill space with a non-breaking
/// space so Ratatui's word-wrap can never split "[ ]" across two lines (its
/// "[" and "]" are otherwise two separate words joined by an ordinary space,
/// which the wrapper is free to break between). Checked markers ("[x]"/"[X]")
/// contain no whitespace at all, so they are already one unbreakable word and
/// are left untouched -- substituting their fill character would render every
/// checked box as visually unchecked. Both the visible card render and the
/// hit-test buffer in `card_checkbox_areas` must render this exact text: if
/// only one of them substituted, a checkbox that wraps mid-marker would be
/// found in one but not the other, and every checkbox_index after the
/// skipped one would point at the wrong checkbox.
fn atomic_checkbox_title(title: &str) -> Cow<'_, str> {
    let occurrences = checkbox_occurrences(title);
    if occurrences.is_empty() {
        return Cow::Borrowed(title);
    }
    let mut atomic = String::with_capacity(title.len());
    let mut cursor = 0;
    for (byte_index, is_checked) in occurrences {
        // `byte_index` is always an ASCII '[' byte (see `checkbox_occurrences`),
        // so these slice points can never land mid-character.
        atomic.push_str(&title[cursor..byte_index + 1]);
        if is_checked {
            atomic.push_str(&title[byte_index + 1..byte_index + 2]);
        } else {
            atomic.push('\u{00A0}');
        }
        cursor = byte_index + 2;
    }
    atomic.push_str(&title[cursor..]);
    Cow::Owned(atomic)
}

/// Uses Ratatui itself to wrap the card title into an off-screen buffer, then
/// reports the exact visible three-cell checkbox rectangles, in on-screen
/// reading order -- which, thanks to `atomic_checkbox_title`, is always the
/// same order `checkbox_occurrences` returns. Pointer targets therefore
/// cannot drift from word wrapping or wide-character behavior, and the
/// position of a found rectangle in this list is always its true occurrence
/// index (used directly as `checkbox_index` by callers).
fn card_checkbox_areas(card: &BoardCard, area: Rect) -> Vec<Rect> {
    let expected_count = checkbox_occurrences(&card.title).len();
    if expected_count == 0 {
        return Vec::new();
    }
    let inner = Block::bordered().inner(area);
    if inner.width < 3 || inner.height == 0 {
        return Vec::new();
    }
    let mut buffer = Buffer::empty(inner);
    Paragraph::new(atomic_checkbox_title(&card.title))
        .wrap(Wrap { trim: true })
        .render(inner, &mut buffer);
    let mut areas = Vec::new();
    for y in inner.y..inner.bottom() {
        for x in inner.x..inner.right().saturating_sub(2) {
            let left = buffer[(x, y)].symbol();
            let middle = buffer[(x + 1, y)].symbol();
            let right = buffer[(x + 2, y)].symbol();
            // The non-breaking space only ever appears here because
            // `atomic_checkbox_title` put it there for a real occurrence. A
            // title containing a literal, pre-existing "[<NBSP>]" (e.g.
            // pasted from a browser) would also match and be counted before
            // any real checkbox after it in reading order, but
            // `checkbox_occurrences` never counts it, so `expected_count` is
            // reached early: the affected click falls back to selecting the
            // card rather than toggling the wrong checkbox. Rare and
            // fails safe; a full fix would require normalizing pre-existing
            // NBSPs out of the title before rendering.
            if left == "[" && matches!(middle, " " | "\u{00A0}" | "x" | "X") && right == "]" {
                areas.push(Rect::new(x, y, 3, 1));
                if areas.len() == expected_count {
                    return areas;
                }
            }
        }
    }
    areas
}

#[cfg(test)]
mod tests {
    use ilium_core::BoardStorage;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use super::*;
    use crate::board::{BoardColumn, BoardPane};

    // Returns the backing `TempDir` alongside the board: dropping it deletes
    // the directory `board.storage`'s path points at, so it must outlive
    // every use of the returned `BoardPane` in the calling test.
    fn board() -> (tempfile::TempDir, BoardPane) {
        let directory = tempfile::tempdir().unwrap();
        let mut board = BoardPane::create(BoardStorage::MarkdownFile {
            path: directory.path().join("board.md"),
        })
        .unwrap();
        board.columns = vec![BoardColumn {
            title: "To do".to_string(),
            cards: vec![
                BoardCard {
                    title: "one two three four five six seven".to_string(),
                    body: "complete body".to_string(),
                },
                BoardCard {
                    title: "second item".to_string(),
                    body: String::new(),
                },
            ],
        }];
        board.selected_column = 0;
        board.selected_card = Some(0);
        (directory, board)
    }

    #[test]
    fn detail_panel_owns_exactly_the_rightmost_third() {
        let layout = compute_layout(Rect::new(0, 0, 120, 30), true, 6, 20);

        assert_eq!(layout.columns_area.width, 80);
        assert_eq!(layout.detail_area.unwrap().width, 40);
        assert!(layout.horizontal_scrollbar_area.is_some());
    }

    #[test]
    fn minimum_width_pages_columns_and_exposes_horizontal_scrollbar() {
        let (_directory, mut board) = board();
        board.columns = (0..5)
            .map(|index| BoardColumn {
                title: format!("Column {index}"),
                cards: Vec::new(),
            })
            .collect();
        board.column_scroll = 1;
        let layout = compute_layout(Rect::new(0, 0, 60, 20), false, 5, 20);
        let viewport = column_viewport(&board, layout.columns_area, 20);

        assert_eq!(viewport.visible_column_count, 3);
        assert_eq!(viewport.first_column, 1);
        assert_eq!(viewport.maximum_scroll, 2);
        assert!(viewport.areas.iter().all(|(_, area)| area.width >= 20));
        assert!(layout.horizontal_scrollbar_area.is_some());

        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), &board, 3, 20))
            .unwrap();
        assert!(terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.symbol() == "▶"));
    }

    #[test]
    fn cards_are_contiguous_and_follow_the_preview_line_setting() {
        let inner = Rect::new(1, 1, 30, 20);

        let first = card_area(inner, 0, 3).unwrap();
        let second = card_area(inner, 1, 3).unwrap();

        assert_eq!(first.height, 5);
        assert_eq!(second.y, first.bottom());
    }

    #[test]
    fn hit_testing_uses_the_same_three_line_card_geometry() {
        let (_directory, board) = board();
        let area = Rect::new(0, 0, 60, 20);

        assert_eq!(
            hit_test(&board, area, 3, 20, Position::new(3, 6)),
            Some(BoardHit::Card {
                column_index: 0,
                card_index: 1,
            })
        );
    }

    #[test]
    fn render_shows_three_wrapped_lines_without_a_blank_card_row() {
        let (_directory, board) = board();
        let backend = TestBackend::new(18, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| render(frame, frame.area(), &board, 3, 20))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let rows = (0..20)
            .map(|row| {
                (0..18)
                    .map(|column| buffer[(column, row)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(rows[2].contains("one two three"));
        assert!(rows[3].contains("four five six"));
        assert!(rows[4].contains("seven"));
        assert!(rows[7].contains("second item"));
        assert!(!rows[1].contains(" card "));
    }

    #[test]
    fn checkbox_hit_uses_the_wrapped_card_rendering() {
        let (_directory, mut board) = board();
        board.columns[0].cards[0].title = "prefix [ ] complete this".to_string();
        let area = Rect::new(0, 0, 30, 20);
        let layout = compute_layout(area, false, 1, 20);
        let card_area = card_area(Block::bordered().inner(layout.columns_area), 0, 3).unwrap();
        let checkbox = card_checkbox_areas(&board.columns[0].cards[0], card_area)[0];

        assert_eq!(
            hit_test(
                &board,
                area,
                3,
                20,
                Position::new(checkbox.x + 1, checkbox.y)
            ),
            Some(BoardHit::CardCheckbox {
                column_index: 0,
                card_index: 0,
                checkbox_index: 0,
            })
        );
    }

    #[test]
    fn active_drag_renders_a_visible_insertion_line() {
        let (_directory, mut board) = board();
        board.drag_source = Some((0, 0));
        board.drag_target = Some((0, 1));
        let backend = TestBackend::new(30, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| render(frame, frame.area(), &board, 3, 20))
            .unwrap();

        assert!(terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.symbol() == "━"));
    }

    #[test]
    fn atomic_checkbox_title_keeps_each_marker_as_one_unbreakable_word() {
        // An unchecked marker's "[" and "]" are separated by a literal space,
        // which word-wrap would otherwise treat as two independent words.
        let atomic = atomic_checkbox_title("aaaa [ ] bbbb [x] cccc [X] dddd");

        assert!(atomic.split(' ').all(|word| word != "[" && word != "]"));
        // Checked markers contain no whitespace, so they are already
        // unbreakable and must be left as-is -- otherwise every checked box
        // would render as visually unchecked.
        assert!(atomic.contains("[x]"));
        assert!(atomic.contains("[X]"));
        assert!(atomic.contains("[\u{00A0}]"));
    }

    #[test]
    fn checkbox_marker_survives_a_wrap_point_between_its_brackets() {
        let card = BoardCard {
            title: "AAAA [ ]".to_string(),
            body: String::new(),
        };
        // Width 9 leaves 7 inner columns after the card's own border --
        // exactly enough for "AAAA" but not "AAAA [ ]" together, so a plain
        // (non-atomic) render would wrap between "[" and "]" and this
        // checkbox would go undetected.
        let area = Rect::new(0, 0, 9, 6);

        let areas = card_checkbox_areas(&card, area);

        assert_eq!(
            areas.len(),
            1,
            "a checkbox marker must never be split across a wrap point"
        );
    }

    #[test]
    fn checkbox_index_stays_aligned_when_an_earlier_marker_would_have_wrapped() {
        let (_directory, mut board) = board();
        board.columns[0].cards[0].title = "AAAA [ ] second [x] third".to_string();
        let area = Rect::new(0, 0, 11, 20);
        let minimum_column_width = 11;
        let preview_lines = 5;

        // Same layout pipeline `hit_test` uses internally, so the computed
        // card area matches exactly what `hit_test` will hit-test against.
        let layout = compute_layout(area, false, 1, minimum_column_width);
        let card_rect = card_area(
            Block::bordered().inner(layout.columns_area),
            0,
            preview_lines,
        )
        .unwrap();
        let areas = card_checkbox_areas(&board.columns[0].cards[0], card_rect);
        // Without the atomic-marker fix the first "[ ]" would wrap between
        // its brackets and go undetected, shifting the second checkbox's
        // reported index from 1 down to 0.
        assert_eq!(areas.len(), 2);

        let hit = hit_test(
            &board,
            area,
            preview_lines,
            minimum_column_width,
            Position::new(areas[1].x + 1, areas[1].y),
        );

        assert_eq!(
            hit,
            Some(BoardHit::CardCheckbox {
                column_index: 0,
                card_index: 0,
                checkbox_index: 1,
            })
        );
    }
}
