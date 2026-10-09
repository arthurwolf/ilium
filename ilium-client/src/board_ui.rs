//! Shared Kanban board rendering and pointer geometry.
//!
//! Card height, spaced placement, detail-panel allocation, and mouse
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
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::board::{checkbox_occurrences, BoardCard, BoardPane, CardDetailEditor, CardEditorField};
use crate::theme;

const DETAIL_PANEL_WIDTH_DIVISOR: u16 = 3;
const CARD_BORDER_ROWS: u16 = 2;
const CARD_GAP_ROWS: u16 = 1;
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
    let hint_height = board_hint_lines(is_detail_panel_open, area.width).len() as u16;
    let content_height = area.height.saturating_sub(hint_height);
    let content_area = Rect::new(area.x, area.y, area.width, content_height);
    let hint_area = Rect::new(
        area.x,
        area.y.saturating_add(content_height),
        area.width,
        area.height.min(hint_height),
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

fn card_stride(preview_lines: u16) -> u16 {
    preview_lines
        .saturating_add(CARD_BORDER_ROWS)
        .saturating_add(CARD_GAP_ROWS)
}

/// Keep the keyboard selection visible without persisting presentation state.
/// Rendering and all pointer geometry derive the same first visible card.
fn first_visible_card(
    board: &BoardPane,
    column_index: usize,
    inner: Rect,
    preview_lines: u16,
) -> usize {
    if column_index != board.selected_column || inner.height == 0 {
        return 0;
    }
    let card_height = preview_lines.saturating_add(CARD_BORDER_ROWS);
    let visible_count =
        usize::from(inner.height.saturating_sub(card_height) / card_stride(preview_lines) + 1);
    board
        .selected_card
        .unwrap_or(0)
        .min(board.columns[column_index].cards.len().saturating_sub(1))
        .saturating_sub(visible_count.saturating_sub(1))
}

fn cards_overflow(inner: Rect, card_count: usize, preview_lines: u16) -> bool {
    card_count
        .saturating_mul(usize::from(card_stride(preview_lines)))
        .saturating_sub(usize::from(CARD_GAP_ROWS))
        > usize::from(inner.height)
}

fn card_content_area(inner: Rect, card_count: usize, preview_lines: u16) -> Rect {
    let gutter = u16::from(inner.width >= 2 && cards_overflow(inner, card_count, preview_lines));
    Rect {
        width: inner.width.saturating_sub(gutter),
        ..inner
    }
}

fn column_title(title: &str, count: usize, width: u16, style: Style) -> Line<'static> {
    let count = count.to_string();
    let available = usize::from(width.saturating_sub(2));
    if available < count.len() + 3 {
        return Line::default();
    }
    let title_width = available - count.len() - 3;
    let mut label = String::new();
    if title.width() <= title_width {
        label.push_str(title);
    } else if title_width > 0 {
        let mut used = 0;
        for grapheme in title.graphemes(true) {
            let cells = grapheme.width();
            if used + cells > title_width - 1 {
                break;
            }
            label.push_str(grapheme);
            used += cells;
        }
        label.push('…');
    }
    Line::from(vec![
        Span::styled(format!(" {label} "), style),
        Span::styled(
            format!("{count} "),
            Style::new().add_modifier(Modifier::DIM),
        ),
    ])
}

/// Returns one visible card rectangle, leaving a quiet row between borders.
pub fn card_area(column_inner: Rect, card_index: usize, preview_lines: u16) -> Option<Rect> {
    let card_height = preview_lines.saturating_add(CARD_BORDER_ROWS);
    let offset = u16::try_from(card_index)
        .ok()?
        .saturating_mul(card_stride(preview_lines));
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
        let inner = card_content_area(
            Block::bordered().inner(column_area),
            board.columns[column_index].cards.len(),
            preview_lines,
        );
        if position.x == inner.right() && inner.width < column_area.width.saturating_sub(2) {
            return None;
        }
        let first_card = first_visible_card(board, column_index, inner, preview_lines);
        for card_index in first_card..board.columns[column_index].cards.len() {
            let Some(card_area) = card_area(inner, card_index - first_card, preview_lines) else {
                break;
            };
            if !card_area.contains(position) {
                continue;
            }
            if let Some(checkbox_index) =
                card_checkbox_areas(&board.columns[column_index].cards[card_index], card_area)
                    .into_iter()
                    .find(|(_, checkbox_area)| checkbox_area.contains(position))
                    .map(|(occurrence_index, _)| occurrence_index)
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
        let inner = card_content_area(
            Block::bordered().inner(column_area),
            board.columns[column_index].cards.len(),
            preview_lines,
        );
        let first_card = first_visible_card(board, column_index, inner, preview_lines);
        if position.y <= inner.y {
            return Some((column_index, first_card));
        }
        let stride = card_stride(preview_lines);
        let offset = position.y.saturating_sub(inner.y);
        let card_index = (first_card
            + usize::from(offset / stride)
            + usize::from(offset % stride >= stride.saturating_sub(CARD_GAP_ROWS)))
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

/// Draws columns, spaced card previews, the optional editor panel, and
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
    frame.render_widget(
        Paragraph::new(
            board_hint_lines(board.is_detail_panel_open, layout.hint_area.width)
                .into_iter()
                .map(|line| {
                    Line::from(Span::styled(line, Style::new().add_modifier(Modifier::DIM)))
                })
                .collect::<Vec<_>>(),
        ),
        layout.hint_area,
    );
}

fn board_hint_lines(is_detail_open: bool, width: u16) -> Vec<&'static str> {
    if is_detail_open {
        return match width {
            0..=19 => vec!["Esc"],
            20..=21 => vec!["Tab edits", "Autosave", "Esc closes"],
            22..=25 => vec!["Tab edits", "Autosaves · Esc closes"],
            26..=37 => vec!["Tab field", "Autosaves", "Esc closes"],
            38..=68 => vec![
                "Tab field · type to edit",
                "Changes save immediately · Esc closes",
            ],
            _ => vec!["Tab field · type to edit · every change saves immediately · Esc close"],
        };
    }
    match width {
        0..=10 => vec![],
        11..=18 => vec!["Enter opens"],
        19..=28 => vec!["Enter opens", "n new card", "c/e edit · d delete"],
        29..=38 => vec!["Arrows move · Enter details", "n card · c col · e name", "d delete · Shift/drag move"],
        39..=118 => vec!["←→ cols · ↑↓ cards · Enter details", "n card · c column · e rename · d delete", "Shift+←/→ move · drag cards"],
        _ => vec!["←/→ column · ↑/↓ header/card · Enter details · n card · c column · e rename · d delete · Shift+arrows move · drag cards"],
    }
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
        let block = theme::block(is_selected_column).title(column_title(
            &column.title,
            column.cards.len(),
            column_area.width,
            title_style,
        ));
        let column_inner = block.inner(column_area);
        let inner = card_content_area(column_inner, column.cards.len(), preview_lines);
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
            let first_card = first_visible_card(board, column_index, inner, preview_lines);
            for (card_index, card) in column.cards.iter().enumerate().skip(first_card) {
                let Some(area) = card_area(inner, card_index - first_card, preview_lines) else {
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
                        .block(theme::block(is_selected).border_style(border_style))
                        .wrap(Wrap { trim: true }),
                    area,
                );
            }
        }
        if inner.width < column_inner.width && inner.height > 0 {
            let first = first_visible_card(board, column_index, inner, preview_lines);
            let complete_cards = usize::from(
                inner
                    .height
                    .saturating_sub(preview_lines.saturating_add(CARD_BORDER_ROWS))
                    / card_stride(preview_lines)
                    + 1,
            );
            let mut state = ScrollbarState::new(column.cards.len())
                .position(first)
                .viewport_content_length(complete_cards);
            frame.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(None)
                    .end_symbol(None)
                    .track_symbol(Some("│"))
                    .style(theme::border_style(false)),
                Rect::new(inner.right(), inner.y, 1, inner.height),
                &mut state,
            );
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
    let first_card = first_visible_card(board, column_index, column_inner, preview_lines);
    let visible_insertion_index = insertion_index.saturating_sub(first_card);
    let stride = card_stride(preview_lines);
    let requested_y = column_inner.y.saturating_add(
        u16::try_from(visible_insertion_index)
            .unwrap_or(u16::MAX)
            .saturating_mul(stride)
            .saturating_sub(u16::from(visible_insertion_index > 0)),
    );
    let y = requested_y.min(column_inner.bottom().saturating_sub(1));
    frame.render_widget(
        Paragraph::new("━".repeat(usize::from(column_inner.width)))
            .style(Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Rect::new(column_inner.x, y, column_inner.width, 1),
    );
}

fn render_detail_panel(frame: &mut Frame, area: Rect, editor: &CardDetailEditor) {
    let block = theme::block(true).title(theme::chrome_title("Card details"));
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(DETAIL_CLOSE_LABEL).style(theme::selected_style()),
        detail_close_area(area),
    );
    let layout = detail_editor_layout(area);
    let mut title = editor.title.clone();
    title.set_block(
        theme::block(editor.focus == CardEditorField::Title).title(theme::chrome_title("Title")),
    );
    title.set_cursor_style(if editor.focus == CardEditorField::Title {
        theme::selected_style()
    } else {
        Style::default()
    });
    frame.render_widget(&title, layout.title_area);

    let mut body = editor.body.clone();
    body.set_block(
        theme::block(editor.focus == CardEditorField::Body).title(theme::chrome_title("Notes")),
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

/// First Private Use Area code point used by `probe_checkbox_title` to tag a
/// checkbox marker's fill cell with its occurrence index.
const CHECKBOX_PROBE_FIRST_CODE_POINT: u32 = 0xE000;
/// Number of code points in the Basic Multilingual Plane's Private Use Area
/// (U+E000..=U+F8FF); occurrence indices at or past this bound cannot be
/// probe-tagged and their checkboxes simply stop being pointer targets.
const CHECKBOX_PROBE_CAPACITY: u32 = 0x1900;

/// Rewrites every checkbox marker's single fill character (the byte between
/// its "[" and "]") through `fill_for`; `None` keeps the original fill.
/// `checkbox_occurrences` guarantees the marker bytes are ASCII and the
/// occurrences are ascending and non-overlapping, so all slice points below
/// land on character boundaries.
fn substitute_checkbox_fills(
    title: &str,
    mut fill_for: impl FnMut(usize, bool) -> Option<char>,
) -> Cow<'_, str> {
    let occurrences = checkbox_occurrences(title);
    if occurrences.is_empty() {
        return Cow::Borrowed(title);
    }
    let mut substituted = String::with_capacity(title.len());
    let mut cursor = 0;
    for (occurrence_index, (byte_index, is_checked)) in occurrences.into_iter().enumerate() {
        substituted.push_str(&title[cursor..byte_index + 1]);
        match fill_for(occurrence_index, is_checked) {
            Some(fill) => substituted.push(fill),
            None => substituted.push_str(&title[byte_index + 1..byte_index + 2]),
        }
        cursor = byte_index + 2;
    }
    substituted.push_str(&title[cursor..]);
    Cow::Owned(substituted)
}

/// Replaces an *unchecked* checkbox marker's fill space with a non-breaking
/// space so Ratatui's word-wrap can never split "[ ]" across two lines (its
/// "[" and "]" are otherwise two separate words joined by an ordinary space,
/// which the wrapper is free to break between). Checked markers ("[x]"/"[X]")
/// contain no whitespace at all, so they are already one unbreakable word and
/// are left untouched -- substituting their fill character would render every
/// checked box as visually unchecked.
fn atomic_checkbox_title(title: &str) -> Cow<'_, str> {
    substitute_checkbox_fills(title, |_, is_checked| (!is_checked).then_some('\u{00A0}'))
}

/// Hit-test-only variant of `atomic_checkbox_title`: every marker's fill cell
/// becomes a Private Use Area character encoding its occurrence index, so the
/// off-screen scan in `card_checkbox_areas` can recover *which* checkbox each
/// on-screen "[·]" is, even when an earlier marker was clipped or hard-split
/// by the word wrapper. Every substitute is a width-1, non-whitespace
/// character -- exactly like the NBSP/"x" fills the visible render uses -- so
/// the wrap geometry of this probe text is identical to what the user sees.
/// Occurrence indices past `CHECKBOX_PROBE_CAPACITY` fall back to the visible
/// fill (NBSP when unchecked) to keep that geometry parity.
fn probe_checkbox_title(title: &str) -> Cow<'_, str> {
    substitute_checkbox_fills(title, |occurrence_index, is_checked| {
        u32::try_from(occurrence_index)
            .ok()
            .filter(|index| *index < CHECKBOX_PROBE_CAPACITY)
            .and_then(|index| char::from_u32(CHECKBOX_PROBE_FIRST_CODE_POINT + index))
            .or_else(|| (!is_checked).then_some('\u{00A0}'))
    })
}

/// Decodes a probe fill cell back to its checkbox occurrence index.
fn probe_fill_occurrence_index(symbol: &str) -> Option<usize> {
    let mut chars = symbol.chars();
    let fill = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let offset = u32::from(fill).checked_sub(CHECKBOX_PROBE_FIRST_CODE_POINT)?;
    (offset < CHECKBOX_PROBE_CAPACITY).then_some(offset as usize)
}

/// Uses Ratatui itself to wrap the card title into an off-screen buffer, then
/// reports each visible three-cell checkbox rectangle together with its true
/// occurrence index (the `checkbox_index` callers pass to
/// `toggle_card_checkbox`). The buffer is rendered from
/// `probe_checkbox_title`, whose wrap geometry matches the visible render
/// cell-for-cell, so pointer targets cannot drift from word wrapping or
/// wide-character behavior -- and because the index is decoded from the fill
/// cell itself rather than inferred from scan order, a marker that the
/// wrapper hard-split mid-word (or that fell below the preview clip) can
/// never shift a later checkbox onto the wrong index.
fn card_checkbox_areas(card: &BoardCard, area: Rect) -> Vec<(usize, Rect)> {
    let expected_count = checkbox_occurrences(&card.title).len();
    if expected_count == 0 {
        return Vec::new();
    }
    let inner = Block::bordered().inner(area);
    if inner.width < 3 || inner.height == 0 {
        return Vec::new();
    }
    let mut buffer = Buffer::empty(inner);
    Paragraph::new(probe_checkbox_title(&card.title))
        .wrap(Wrap { trim: true })
        .render(inner, &mut buffer);
    let mut areas: Vec<(usize, Rect)> = Vec::new();
    for y in inner.y..inner.bottom() {
        for x in inner.x..inner.right().saturating_sub(2) {
            let left = buffer[(x, y)].symbol();
            let middle = buffer[(x + 1, y)].symbol();
            let right = buffer[(x + 2, y)].symbol();
            if left != "[" || right != "]" {
                continue;
            }
            let Some(occurrence_index) = probe_fill_occurrence_index(middle) else {
                continue;
            };
            // A probe fill can only come from `probe_checkbox_title`, unless
            // the title itself contained a literal bracketed PUA character.
            // Rejecting out-of-range and duplicate indices keeps that
            // pathological case fail-safe: the bogus cell is ignored and a
            // click there falls back to selecting the card.
            let is_duplicate = areas
                .iter()
                .any(|(existing_index, _)| *existing_index == occurrence_index);
            if occurrence_index >= expected_count || is_duplicate {
                continue;
            }
            areas.push((occurrence_index, Rect::new(x, y, 3, 1)));
            if areas.len() == expected_count {
                return areas;
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

    #[test]
    fn board_footer_hints_fit_and_keep_their_viewport_rows() {
        for width in 1..=180 {
            for detail in [false, true] {
                let lines = board_hint_lines(detail, width);
                assert!(
                    lines
                        .iter()
                        .all(|line| UnicodeWidthStr::width(*line) <= usize::from(width)),
                    "hint clips at width {width}: {lines:?}"
                );
                let layout = compute_layout(Rect::new(0, 0, width, 24), detail, 3, 18);
                assert_eq!(layout.hint_area.height, lines.len() as u16);
                let viewport_bottom = layout
                    .horizontal_scrollbar_area
                    .map_or(layout.columns_area.bottom(), Rect::bottom);
                assert_eq!(
                    viewport_bottom.max(layout.detail_area.map_or(0, Rect::bottom)),
                    layout.hint_area.y
                );
            }
        }
        let wide = board_hint_lines(false, 120).join(" ");
        for expected in [
            "column", "card", "details", "new", "rename", "delete", "move", "drag",
        ] {
            assert!(
                wide.contains(expected),
                "missing board action {expected}: {wide}"
            );
        }
    }

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
    fn cards_leave_a_spacer_row_and_follow_the_preview_line_setting() {
        let inner = Rect::new(1, 1, 30, 20);

        let first = card_area(inner, 0, 3).unwrap();
        let second = card_area(inner, 1, 3).unwrap();

        assert_eq!(first.height, 5);
        assert_eq!(second.y, first.bottom() + CARD_GAP_ROWS);
    }

    #[test]
    fn long_unicode_column_titles_leave_room_for_the_card_count() {
        for width in 1..=40 {
            let title = column_title("制作中 👩‍💻 Very long column name", 123, width, Style::new());
            assert!(title.width() <= usize::from(width.saturating_sub(2)));
            if width >= 8 {
                let text: String = title
                    .spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect();
                assert!(text.ends_with("123 "));
            }
        }
    }

    #[test]
    fn selected_cards_beyond_the_first_page_remain_visible_and_clickable() {
        let (_directory, mut board) = board();
        board.columns[0].cards = (0..8)
            .map(|index| BoardCard {
                title: format!("Card {index}"),
                body: String::new(),
            })
            .collect();
        board.selected_card = Some(7);
        let area = Rect::new(0, 0, 60, 14);
        let layout = compute_layout(area, false, 1, 20);
        let column_inner = Block::bordered().inner(layout.columns_area);
        let inner = card_content_area(column_inner, 8, 3);
        assert_eq!(inner.right() + 1, column_inner.right());
        let first = first_visible_card(&board, 0, inner, 3);
        assert_eq!(first, 6);
        let selected = card_area(inner, 7 - first, 3).unwrap();
        assert_eq!(selected.height, 5);
        let position = Position::new(selected.x + 1, selected.y + 1);
        assert_eq!(
            hit_test(&board, area, 3, 20, position),
            Some(BoardHit::Card {
                column_index: 0,
                card_index: 7
            })
        );
        assert_eq!(
            card_drop_target(&board, area, 3, 20, position),
            Some((0, 7))
        );
        assert_eq!(
            hit_test(&board, area, 3, 20, Position::new(inner.right(), inner.y)),
            None
        );
        let mut terminal = Terminal::new(TestBackend::new(60, 14)).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), &board, 3, 20))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let text: String = buffer.content().iter().map(|cell| cell.symbol()).collect();
        assert!(text.contains("Card 7"));
        assert!(!text.contains("Card 0"));
    }

    #[test]
    fn hit_testing_uses_the_same_three_line_card_geometry() {
        let (_directory, board) = board();
        let area = Rect::new(0, 0, 60, 20);

        assert_eq!(
            hit_test(&board, area, 3, 20, Position::new(3, 6)),
            Some(BoardHit::Column { column_index: 0 }),
        );
        assert_eq!(
            card_drop_target(&board, area, 3, 20, Position::new(3, 6)),
            Some((0, 1)),
        );

        assert_eq!(
            hit_test(&board, area, 3, 20, Position::new(3, 7)),
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
        assert!(rows[8].contains("second item"));
        assert!(!rows[1].contains(" card "));
    }

    #[test]
    fn checkbox_hit_uses_the_wrapped_card_rendering() {
        let (_directory, mut board) = board();
        board.columns[0].cards[0].title = "prefix [ ] complete this".to_string();
        let area = Rect::new(0, 0, 30, 20);
        let layout = compute_layout(area, false, 1, 20);
        let card_area = card_area(Block::bordered().inner(layout.columns_area), 0, 3).unwrap();
        let checkbox = card_checkbox_areas(&board.columns[0].cards[0], card_area)[0].1;

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
        assert_eq!(areas[0].0, 0);
    }

    #[test]
    fn checkbox_index_survives_an_earlier_marker_hard_split_inside_a_long_word() {
        // "AAAAAA[ ]" is one unbreakable word (the fill substitution joins
        // the brackets), but at 9 cells it is wider than the 7-cell inner
        // width, so the word wrapper hard-splits it mid-marker and the first
        // checkbox has no complete on-screen rectangle. The second checkbox
        // must still report its true occurrence index of 1 -- scan order
        // alone would misreport it as 0 and toggle the wrong checkbox.
        let card = BoardCard {
            title: "AAAAAA[ ] [x]".to_string(),
            body: String::new(),
        };
        let area = Rect::new(0, 0, 9, 8);

        let areas = card_checkbox_areas(&card, area);

        assert_eq!(areas.len(), 1);
        assert_eq!(
            areas[0].0, 1,
            "a surviving checkbox must keep its true occurrence index"
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
        assert_eq!(areas[1].0, 1);

        let hit = hit_test(
            &board,
            area,
            preview_lines,
            minimum_column_width,
            Position::new(areas[1].1.x + 1, areas[1].1.y),
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
