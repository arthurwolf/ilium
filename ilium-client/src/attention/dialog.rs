//! The attention overview dialog: every queued agent as a card, grouped by
//! kind, in a large centred overlay opened from the `[+]` beside the
//! "Next: needs you" button (or `Ctrl+B O`).
//!
//! This file is pure: layout, scrolling, navigation, hit testing and drawing
//! are functions of plain values (a screen rectangle and a list of
//! [`CardData`]), so none of it needs an `App`. `crate::app_attention_dialog`
//! builds the cards from live state and applies the [`DialogAction`] a key or
//! mouse event produces.
//!
//! Geometry. The dialog covers about 90% of the screen. The card grid has one
//! column below 100 terminal columns, two from 100 and three from 160. Below
//! 60 columns or 16 rows the dialog is the whole screen and shows a compact
//! two-line list instead of cards. Card positions are kept in *virtual*
//! coordinates (`y` counts from the top of the scrollable content) and are
//! mapped to the screen through the scroll offset, so drawing, scrolling and
//! pointer hit testing all read the same [`DialogLayout`].

use crossterm::event::{KeyCode, KeyModifiers, MouseEventKind};
use ilium_core::NodeId;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Clear, Widget};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use super::item::{AttentionKind, AttentionSource};
use super::panel::{clip, format_age, kind_glyph, wrap, ATTENTION_GLYPH};
use crate::theme::{self, ColorScheme};

/// Below this terminal width (or [`COMPACT_HEIGHT`] rows) the dialog is the
/// whole screen and lists items in two compact lines.
pub const COMPACT_WIDTH: u16 = 60;
pub const COMPACT_HEIGHT: u16 = 16;
/// Terminal width from which the card grid has two columns.
pub const TWO_COLUMN_WIDTH: u16 = 100;
/// Terminal width from which the card grid has three columns.
pub const THREE_COLUMN_WIDTH: u16 = 160;
/// A screen smaller than this gets no dialog at all (nothing fits).
pub const MINIMUM_DRAWN_WIDTH: u16 = 10;
pub const MINIMUM_DRAWN_HEIGHT: u16 = 5;
/// Rows moved by one wheel notch.
pub const WHEEL_ROWS: u32 = 3;
/// Cells of the `[+]` button beside the "Next: needs you" button.
pub const PLUS_WIDTH: u16 = 3;
/// The `[+]` is not drawn on a button row narrower than this.
pub const MINIMUM_BUTTON_WIDTH_FOR_PLUS: u16 = 14;
/// Rows a card needs besides its wrapped text: two borders, the title row,
/// the project row and the footer row.
const CARD_CHROME_ROWS: u16 = 5;
const REASON_MAXIMUM_LINES: usize = 2;
const QUESTION_MAXIMUM_LINES: usize = 3;
/// Rows of one item in the compact list.
const COMPACT_ITEM_ROWS: u16 = 2;
/// Cells between neighbouring cards of one grid row.
const COLUMN_GAP: u16 = 1;
/// A card is never narrower than this many cells.
const MINIMUM_CARD_WIDTH: u16 = 20;
const PRIORITY_PIPS: usize = 5;

// ----------------------------------------------------------------- cards

/// One queue item with everything the dialog shows about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardData {
    pub pane_id: NodeId,
    pub kind: AttentionKind,
    pub title: String,
    pub project: String,
    pub agent_class: String,
    pub reason: String,
    pub question: Option<String>,
    /// Name of the agent this one waits on.
    pub waiting_on: Option<String>,
    pub priority: u8,
    pub source: AttentionSource,
    pub age_ms: u64,
    pub is_looked: bool,
    pub is_snoozed: bool,
    pub snooze_icon: String,
}

/// Accent colour of a kind. Named ANSI colours, like the status icons, so they
/// follow the terminal's own palette in both colour schemes.
pub const fn kind_color(kind: AttentionKind) -> Color {
    match kind {
        AttentionKind::Question => Color::Yellow,
        AttentionKind::Stuck => Color::Red,
        AttentionKind::WaitingOn => Color::Cyan,
        AttentionKind::Changed => Color::Green,
    }
}

const fn kind_hint(kind: AttentionKind) -> &'static str {
    match kind {
        AttentionKind::Question => "waiting for your answer",
        AttentionKind::Stuck => "keeps failing or is blocked",
        AttentionKind::WaitingOn => "blocked on another agent",
        AttentionKind::Changed => "finished or changed since you looked",
    }
}

/// Wrapped text of one card for a given text width.
struct CardBody {
    reason: Vec<String>,
    question: Vec<String>,
}

fn card_body(card: &CardData, text_width: usize) -> CardBody {
    let reason = wrap(&card.reason, text_width, REASON_MAXIMUM_LINES);
    // The quote glyph and the accent bar take two cells of every line.
    let question = card
        .question
        .as_deref()
        .map(|text| {
            wrap(
                &format!("\u{201c}{}\u{201d}", text.trim()),
                text_width.saturating_sub(2),
                QUESTION_MAXIMUM_LINES,
            )
        })
        .unwrap_or_default();
    CardBody { reason, question }
}

fn card_text_width(card_width: u16) -> usize {
    usize::from(card_width.saturating_sub(4))
}

fn card_height(card: &CardData, card_width: u16) -> u16 {
    let body = card_body(card, card_text_width(card_width));
    let text_rows = u16::try_from(body.reason.len() + body.question.len()).unwrap_or(u16::MAX);
    CARD_CHROME_ROWS.saturating_add(text_rows)
}

// ---------------------------------------------------------------- layout

/// True when the dialog falls back to the whole-screen compact list.
pub fn is_compact(screen: Rect) -> bool {
    screen.width < COMPACT_WIDTH || screen.height < COMPACT_HEIGHT
}

/// Card columns for a terminal of `screen_width` columns.
pub fn column_count(screen_width: u16) -> usize {
    if screen_width >= THREE_COLUMN_WIDTH {
        3
    } else if screen_width >= TWO_COLUMN_WIDTH {
        2
    } else {
        1
    }
}

/// The dialog's outer rectangle: about 90% of the screen, centred; the whole
/// screen when compact; empty when nothing would fit.
pub fn dialog_area(screen: Rect) -> Rect {
    if screen.width < MINIMUM_DRAWN_WIDTH || screen.height < MINIMUM_DRAWN_HEIGHT {
        return Rect::new(screen.x, screen.y, 0, 0);
    }
    if is_compact(screen) {
        return screen;
    }
    let width = u16::try_from(u32::from(screen.width) * 9 / 10).unwrap_or(screen.width);
    let height = u16::try_from(u32::from(screen.height) * 9 / 10).unwrap_or(screen.height);
    Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + (screen.height - height) / 2,
        width,
        height,
    )
}

/// What a laid-out slot holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotTarget {
    Header { kind: AttentionKind, count: usize },
    Card { index: usize },
}

/// One header or card in virtual coordinates: `x` is relative to the
/// viewport, `y` counts from the top of the scrollable content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub target: SlotTarget,
    pub x: u16,
    pub width: u16,
    pub y: u32,
    pub height: u16,
}

/// A row of the card grid: `count` cards from `first_card` on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridRow {
    pub first_card: usize,
    pub count: usize,
    pub y: u32,
    pub height: u16,
}

/// A slot clipped to the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleSlot {
    pub slot: Slot,
    /// Where it lands on the screen.
    pub rect: Rect,
    /// Rows of the slot cut off above the viewport.
    pub skipped_rows: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogLayout {
    /// Outer rectangle including the border; empty when nothing is drawn.
    pub area: Rect,
    pub is_compact: bool,
    pub columns: usize,
    /// Row below the title that holds the per-kind counts.
    pub strip: Rect,
    /// The scrolling region the slots are drawn in.
    pub viewport: Rect,
    /// One-cell column right of the viewport holding the scrollbar.
    pub scrollbar: Rect,
    /// Row below the viewport that holds the key legend.
    pub legend: Rect,
    /// The `[x]` button on the top border.
    pub close: Rect,
    pub slots: Vec<Slot>,
    pub grid_rows: Vec<GridRow>,
    /// Slot index of each card, by card index.
    pub card_slots: Vec<usize>,
    /// Total height of the scrollable content.
    pub content_height: u32,
}

/// Where a pointer position lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Outside,
    Inside,
    Close,
    Card(usize),
    /// A row (counted from the track top) of the scrollbar track.
    Scrollbar(u16),
}

impl DialogLayout {
    pub fn is_drawn(&self) -> bool {
        self.area.width > 0 && self.area.height > 0
    }

    pub fn max_scroll(&self) -> u32 {
        self.content_height
            .saturating_sub(u32::from(self.viewport.height))
    }

    pub fn clamp_scroll(&self, scroll: u32) -> u32 {
        scroll.min(self.max_scroll())
    }

    /// Slots that intersect the viewport at `scroll`, with their screen rects.
    pub fn visible(&self, scroll: u32) -> Vec<VisibleSlot> {
        let view_top = scroll;
        let view_bottom = scroll + u32::from(self.viewport.height);
        self.slots
            .iter()
            .filter_map(|slot| {
                let top = slot.y.max(view_top);
                let bottom = (slot.y + u32::from(slot.height)).min(view_bottom);
                if top >= bottom || slot.width == 0 {
                    return None;
                }
                let rect = Rect::new(
                    self.viewport.x.saturating_add(slot.x),
                    self.viewport
                        .y
                        .saturating_add(u16::try_from(top - view_top).unwrap_or(u16::MAX)),
                    slot.width,
                    u16::try_from(bottom - top).unwrap_or(u16::MAX),
                );
                Some(VisibleSlot {
                    slot: *slot,
                    rect,
                    skipped_rows: u16::try_from(top - slot.y).unwrap_or(0),
                })
            })
            .collect()
    }

    pub fn hit(&self, scroll: u32, position: Position) -> Hit {
        if !self.is_drawn() || !self.area.contains(position) {
            return Hit::Outside;
        }
        if self.close.contains(position) {
            return Hit::Close;
        }
        if self.is_scrollable() && self.scrollbar.contains(position) {
            return Hit::Scrollbar(position.y - self.scrollbar.y);
        }
        for visible in self.visible(scroll) {
            if let SlotTarget::Card { index } = visible.slot.target {
                if visible.rect.contains(position) {
                    return Hit::Card(index);
                }
            }
        }
        Hit::Inside
    }

    pub fn is_scrollable(&self) -> bool {
        self.max_scroll() > 0
    }

    /// Scroll offset that puts the thumb on track row `row`.
    pub fn scroll_for_track_row(&self, row: u16) -> u32 {
        let travel = u32::from(self.scrollbar.height.saturating_sub(1)).max(1);
        (u32::from(row).min(travel) * self.max_scroll() + travel / 2) / travel
    }

    /// Smallest scroll change that brings `card` fully into view, taking its
    /// group header along when the card opens the group.
    pub fn ensure_visible(&self, card: usize, scroll: u32) -> u32 {
        let Some(&slot_index) = self.card_slots.get(card) else {
            return self.clamp_scroll(scroll);
        };
        let slot = self.slots[slot_index];
        let mut top = slot.y;
        if slot_index > 0 {
            let previous = self.slots[slot_index - 1];
            if matches!(previous.target, SlotTarget::Header { .. }) {
                top = previous.y;
            }
        }
        let bottom = slot.y + u32::from(slot.height);
        let view = u32::from(self.viewport.height);
        let adjusted = if top < scroll {
            top
        } else if bottom > scroll + view {
            // A card taller than the viewport keeps its top in view.
            (bottom - view).min(top)
        } else {
            scroll
        };
        self.clamp_scroll(adjusted)
    }

    fn grid_row_of(&self, card: usize) -> Option<usize> {
        self.grid_rows
            .iter()
            .position(|row| card >= row.first_card && card < row.first_card + row.count)
    }

    /// The card reached from `from` by one key of navigation.
    pub fn navigate(&self, from: usize, direction: Direction) -> usize {
        let total = self.card_slots.len();
        if total == 0 {
            return 0;
        }
        let from = from.min(total - 1);
        let Some(row_index) = self.grid_row_of(from) else {
            return from;
        };
        let row = self.grid_rows[row_index];
        let column = from - row.first_card;
        let direction = match (direction, self.columns) {
            (Direction::Left, 1) => Direction::Up,
            (Direction::Right, 1) => Direction::Down,
            (other, _) => other,
        };
        let card_in_row = |target_row: usize| {
            let target = self.grid_rows[target_row];
            target.first_card + column.min(target.count - 1)
        };
        match direction {
            Direction::Left => from - usize::from(column > 0),
            Direction::Right => from + usize::from(column + 1 < row.count),
            Direction::Up if row_index > 0 => card_in_row(row_index - 1),
            Direction::Down if row_index + 1 < self.grid_rows.len() => card_in_row(row_index + 1),
            Direction::Up | Direction::Down => from,
            Direction::First => 0,
            Direction::Last => total - 1,
            Direction::PageUp => {
                let target_y = row.y.saturating_sub(u32::from(self.viewport.height));
                let target_row = self
                    .grid_rows
                    .iter()
                    .position(|candidate| candidate.y + u32::from(candidate.height) > target_y)
                    .unwrap_or(0)
                    .min(row_index.saturating_sub(1));
                if row_index == 0 {
                    from
                } else {
                    card_in_row(target_row)
                }
            }
            Direction::PageDown => {
                let target_y = row.y + u32::from(self.viewport.height);
                let target_row = self
                    .grid_rows
                    .iter()
                    .rposition(|candidate| candidate.y <= target_y)
                    .unwrap_or(row_index)
                    .max((row_index + 1).min(self.grid_rows.len() - 1));
                card_in_row(target_row)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    First,
    Last,
}

/// Lays out `cards` (already in queue order) for `screen`.
pub fn compute(screen: Rect, cards: &[CardData]) -> DialogLayout {
    let area = dialog_area(screen);
    let is_compact = is_compact(screen);
    let empty = Rect::new(area.x, area.y, 0, 0);
    if area.width == 0 || area.height == 0 {
        return DialogLayout {
            area,
            is_compact,
            columns: 1,
            strip: empty,
            viewport: empty,
            scrollbar: empty,
            legend: empty,
            close: empty,
            slots: Vec::new(),
            grid_rows: Vec::new(),
            card_slots: Vec::new(),
            content_height: 0,
        };
    }
    let inner = Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    let strip_rows = u16::from(!is_compact && inner.height >= 6);
    let legend_rows = u16::from(inner.height >= 4);
    let viewport = Rect::new(
        inner.x + 1,
        inner.y + strip_rows,
        inner.width.saturating_sub(3),
        inner.height.saturating_sub(strip_rows + legend_rows),
    );
    let strip = Rect::new(
        inner.x + 1,
        inner.y,
        inner.width.saturating_sub(2),
        strip_rows,
    );
    let scrollbar = Rect::new(
        inner.right().saturating_sub(1),
        viewport.y,
        u16::from(inner.width >= 4),
        viewport.height,
    );
    let legend = Rect::new(
        inner.x + 1,
        inner.bottom().saturating_sub(legend_rows),
        inner.width.saturating_sub(2),
        legend_rows,
    );
    let close = if area.width >= 12 {
        Rect::new(area.right().saturating_sub(5), area.y, 3, 1)
    } else {
        empty
    };
    let columns = if is_compact {
        1
    } else {
        column_count(screen.width).min(usize::from((viewport.width / MINIMUM_CARD_WIDTH).max(1)))
    };
    let mut layout = DialogLayout {
        area,
        is_compact,
        columns,
        strip,
        viewport,
        scrollbar,
        legend,
        close,
        slots: Vec::new(),
        grid_rows: Vec::new(),
        card_slots: vec![0; cards.len()],
        content_height: 0,
    };
    if is_compact {
        layout_compact(&mut layout, cards);
    } else {
        layout_cards(&mut layout, cards);
    }
    layout
}

fn layout_compact(layout: &mut DialogLayout, cards: &[CardData]) {
    let width = layout.viewport.width;
    let mut y = 0u32;
    for index in 0..cards.len() {
        layout.card_slots[index] = layout.slots.len();
        layout.grid_rows.push(GridRow {
            first_card: index,
            count: 1,
            y,
            height: COMPACT_ITEM_ROWS,
        });
        layout.slots.push(Slot {
            target: SlotTarget::Card { index },
            x: 0,
            width,
            y,
            height: COMPACT_ITEM_ROWS,
        });
        y += u32::from(COMPACT_ITEM_ROWS);
    }
    layout.content_height = y;
}

fn layout_cards(layout: &mut DialogLayout, cards: &[CardData]) {
    let columns = layout.columns.max(1);
    let columns_u16 = u16::try_from(columns).unwrap_or(1);
    let gaps = COLUMN_GAP * columns_u16.saturating_sub(1);
    let available = layout.viewport.width.saturating_sub(gaps);
    let base_width = available / columns_u16;
    let extra_columns = available % columns_u16;
    let column_width = |column: usize| base_width + u16::from(column < usize::from(extra_columns));
    let column_x = |column: usize| {
        (0..column)
            .map(|before| column_width(before) + COLUMN_GAP)
            .sum::<u16>()
    };

    let mut y = 0u32;
    let mut start = 0;
    while start < cards.len() {
        let kind = cards[start].kind;
        let end = cards[start..]
            .iter()
            .position(|card| card.kind != kind)
            .map_or(cards.len(), |offset| start + offset);
        if start > 0 {
            y += 1;
        }
        layout.slots.push(Slot {
            target: SlotTarget::Header {
                kind,
                count: end - start,
            },
            x: 0,
            width: layout.viewport.width,
            y,
            height: 1,
        });
        y += 1;
        let mut row_start = start;
        while row_start < end {
            let row_end = (row_start + columns).min(end);
            let height = (row_start..row_end)
                .map(|index| card_height(&cards[index], column_width(index - row_start)))
                .max()
                .unwrap_or(CARD_CHROME_ROWS);
            for index in row_start..row_end {
                let column = index - row_start;
                layout.card_slots[index] = layout.slots.len();
                layout.slots.push(Slot {
                    target: SlotTarget::Card { index },
                    x: column_x(column),
                    width: column_width(column),
                    y,
                    height,
                });
            }
            layout.grid_rows.push(GridRow {
                first_card: row_start,
                count: row_end - row_start,
                y,
                height,
            });
            y += u32::from(height);
            row_start = row_end;
        }
        start = end;
    }
    layout.content_height = y;
}

/// Start and length of the scrollbar thumb inside a track of `track` cells.
pub fn thumb(content: u32, view: u32, scroll: u32, track: u32) -> Option<(u32, u32)> {
    if content <= view || track == 0 {
        return None;
    }
    let length = (track * view / content).clamp(1, track);
    let travel = track - length;
    let maximum = content - view;
    let start = (scroll.min(maximum) * travel + maximum / 2) / maximum;
    Some((start, length))
}

// ----------------------------------------------------------------- state

/// What the dialog asks the app to do after an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogAction {
    None,
    Close,
    /// Jump to this agent and close.
    Open(NodeId),
    Snooze(NodeId),
}

/// UI state of the open dialog. The queue itself lives in `AttentionState`;
/// the selection follows the *agent*, so a queue that reorders under the
/// dialog does not move the highlight to a different one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DialogState {
    pub selected_pane: Option<NodeId>,
    selected_hint: usize,
    pub scroll: u32,
    pub hover: Option<usize>,
    pub snooze_hover: Option<usize>,
    pub show_snoozed: bool,
    pub is_close_hovered: bool,
    pub is_scrollbar_dragging: bool,
    /// Unix second of the last age redraw, so ages tick once a second.
    pub last_redraw_second: u64,
}

impl DialogState {
    /// A freshly opened dialog; `second` is the current Unix second.
    pub fn opened_at_second(second: u64) -> Self {
        Self {
            last_redraw_second: second,
            ..Self::default()
        }
    }

    /// Index of the highlighted card in `cards`.
    pub fn selected_index(&self, cards: &[CardData]) -> usize {
        let last = cards.len().saturating_sub(1);
        self.selected_pane
            .and_then(|pane_id| cards.iter().position(|card| card.pane_id == pane_id))
            .unwrap_or_else(|| self.selected_hint.min(last))
    }

    pub fn select(&mut self, index: usize, cards: &[CardData]) {
        let index = index.min(cards.len().saturating_sub(1));
        self.selected_hint = index;
        self.selected_pane = cards.get(index).map(|card| card.pane_id);
    }

    fn move_selection(&mut self, to: usize, layout: &DialogLayout, cards: &[CardData]) {
        self.select(to, cards);
        self.scroll = layout.ensure_visible(to, self.scroll);
    }

    pub fn handle_key(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
        screen: Rect,
        cards: &[CardData],
    ) -> DialogAction {
        let layout = compute(screen, cards);
        self.scroll = layout.clamp_scroll(self.scroll);
        let selected = self.selected_index(cards);
        let direction = match code {
            KeyCode::Char('s' | 'S') => {
                self.show_snoozed = !self.show_snoozed;
                self.scroll = 0;
                self.snooze_hover = None;
                return DialogAction::None;
            }
            KeyCode::Esc | KeyCode::Char('q') => return DialogAction::Close,
            KeyCode::Enter => {
                return cards
                    .get(selected)
                    .map_or(DialogAction::None, |card| DialogAction::Open(card.pane_id))
            }
            KeyCode::Up | KeyCode::Char('k') => Direction::Up,
            KeyCode::Down | KeyCode::Char('j') => Direction::Down,
            KeyCode::Left | KeyCode::Char('h') => Direction::Left,
            KeyCode::Right | KeyCode::Char('l') => Direction::Right,
            KeyCode::PageUp => Direction::PageUp,
            KeyCode::PageDown => Direction::PageDown,
            KeyCode::Home => Direction::First,
            KeyCode::End => Direction::Last,
            KeyCode::BackTab => {
                self.step(selected, false, &layout, cards);
                return DialogAction::None;
            }
            KeyCode::Tab => {
                let forward = !modifiers.contains(KeyModifiers::SHIFT);
                self.step(selected, forward, &layout, cards);
                return DialogAction::None;
            }
            KeyCode::Char(digit @ '1'..='9') => {
                let index = usize::from(digit as u8 - b'1');
                if index < cards.len() {
                    self.move_selection(index, &layout, cards);
                }
                return DialogAction::None;
            }
            _ => return DialogAction::None,
        };
        if cards.is_empty() {
            return DialogAction::None;
        }
        let target = layout.navigate(selected, direction);
        self.move_selection(target, &layout, cards);
        DialogAction::None
    }

    /// Tab order: next or previous card, wrapping around.
    fn step(&mut self, from: usize, forward: bool, layout: &DialogLayout, cards: &[CardData]) {
        if cards.is_empty() {
            return;
        }
        let total = cards.len();
        let to = if forward {
            (from + 1) % total
        } else {
            (from + total - 1) % total
        };
        self.move_selection(to, layout, cards);
    }

    pub fn handle_mouse(
        &mut self,
        kind: MouseEventKind,
        position: Position,
        screen: Rect,
        cards: &[CardData],
    ) -> DialogAction {
        use crossterm::event::MouseButton;

        let layout = compute(screen, cards);
        self.scroll = layout.clamp_scroll(self.scroll);
        let hit = layout.hit(self.scroll, position);
        let snooze_hit = layout.visible(self.scroll).iter().find_map(|visible| {
            let SlotTarget::Card { index } = visible.slot.target else {
                return None;
            };
            let card = cards.get(index)?;
            snooze_rect(visible, card)
                .filter(|rect| rect.contains(position))
                .map(|_| index)
        });
        self.snooze_hover = snooze_hit;
        if matches!(kind, MouseEventKind::Down(MouseButton::Left)) {
            if layout.legend.contains(position)
                && position.x >= layout.legend.right().saturating_sub(17)
            {
                self.show_snoozed = !self.show_snoozed;
                self.scroll = 0;
                return DialogAction::None;
            }
            if let Some(index) = snooze_hit {
                return DialogAction::Snooze(cards[index].pane_id);
            }
        }
        match kind {
            MouseEventKind::Moved => {
                self.update_hover(hit);
                DialogAction::None
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let rows = WHEEL_ROWS;
                self.scroll = if matches!(kind, MouseEventKind::ScrollUp) {
                    self.scroll.saturating_sub(rows)
                } else {
                    layout.clamp_scroll(self.scroll + rows)
                };
                self.update_hover(layout.hit(self.scroll, position));
                DialogAction::None
            }
            MouseEventKind::Down(MouseButton::Left) => match hit {
                Hit::Close | Hit::Outside => DialogAction::Close,
                Hit::Card(index) => cards
                    .get(index)
                    .map_or(DialogAction::None, |card| DialogAction::Open(card.pane_id)),
                Hit::Scrollbar(row) => {
                    self.is_scrollbar_dragging = true;
                    self.scroll = layout.scroll_for_track_row(row);
                    DialogAction::None
                }
                Hit::Inside => DialogAction::None,
            },
            MouseEventKind::Drag(MouseButton::Left) if self.is_scrollbar_dragging => {
                let row = position
                    .y
                    .saturating_sub(layout.scrollbar.y)
                    .min(layout.scrollbar.height.saturating_sub(1));
                self.scroll = layout.scroll_for_track_row(row);
                DialogAction::None
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.is_scrollbar_dragging = false;
                DialogAction::None
            }
            _ => DialogAction::None,
        }
    }

    fn update_hover(&mut self, hit: Hit) {
        self.hover = match hit {
            Hit::Card(index) => Some(index),
            _ => None,
        };
        self.is_close_hovered = matches!(hit, Hit::Close);
    }
}

// ---------------------------------------------------------------- button

/// Splits the "Next: needs you" row into the pressable main part and the
/// `[+]` that opens the overview. The `[+]` is `None` on a row too narrow.
pub fn split_button(area: Rect) -> (Rect, Option<Rect>) {
    if area.width < MINIMUM_BUTTON_WIDTH_FOR_PLUS || area.height == 0 {
        return (area, None);
    }
    let plus_width = if area.height > 1 { 5 } else { PLUS_WIDTH };
    let main_width = area.width.saturating_sub(plus_width);
    (
        Rect::new(area.x, area.y, main_width, area.height),
        Some(Rect::new(
            area.x + main_width,
            area.y,
            plus_width,
            area.height,
        )),
    )
}

/// Draws the `[+]`: accent-coloured, highlighted while the pointer is on it.
pub fn render_plus(frame: &mut Frame, area: Rect, is_hovered: bool) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    // Normal: the button's own accent fill, slightly dimmed so it reads as a
    // sub-control. Hover: the fill and text colours swap, a clear highlight.
    let style = if is_hovered {
        Style::new()
            .fg(theme::accent_bg())
            .bg(theme::accent_fg())
            .add_modifier(Modifier::BOLD)
    } else {
        theme::selected_style().add_modifier(Modifier::BOLD | Modifier::DIM)
    };
    frame.render_widget(ratatui::widgets::Paragraph::default().style(style), area);
    let label = if area.height > 1 { "+" } else { "[+]" };
    let x = area.x + area.width.saturating_sub(label.len() as u16) / 2;
    frame.buffer_mut().set_stringn(
        x,
        area.y + area.height / 2,
        label,
        usize::from(area.width),
        style,
    );
}

// ---------------------------------------------------------------- drawing

fn put(buffer: &mut Buffer, x: u16, y: u16, text: &str, max_width: u16, style: Style) -> u16 {
    if max_width == 0 {
        return 0;
    }
    let (end_x, _) = buffer.set_stringn(x, y, text, usize::from(max_width), style);
    end_x.saturating_sub(x)
}

fn dim() -> Style {
    Style::new().add_modifier(Modifier::DIM)
}

fn priority_pips(priority: u8) -> (usize, usize) {
    let filled = (usize::from(priority.min(100)) * PRIORITY_PIPS).div_ceil(100);
    (filled, PRIORITY_PIPS - filled)
}

fn age_text(age_ms: u64) -> String {
    if age_ms < 5_000 {
        "just now".to_string()
    } else {
        format!("{} ago", format_age(age_ms))
    }
}

fn source_tag(source: AttentionSource) -> &'static str {
    match source {
        AttentionSource::Instant => "\u{25c6} instant",
        AttentionSource::Llm => "\u{2726} AI",
    }
}

#[derive(Clone, Copy)]
struct CardLook {
    is_selected: bool,
    is_hovered: bool,
    scheme: ColorScheme,
}

/// Paints one card into `buffer` (whose area is exactly the card).
fn paint_card(buffer: &mut Buffer, card: &CardData, look: CardLook) {
    let area = buffer.area;
    let (width, height) = (area.width, area.height);
    if width < 6 || height < 3 {
        return;
    }
    let accent = kind_color(card.kind);
    if look.is_selected || look.is_hovered {
        buffer.set_style(area, Style::new().bg(theme::muted_accent_bg(look.scheme)));
    }
    let border = if look.is_selected {
        Style::new().fg(accent).add_modifier(Modifier::BOLD)
    } else {
        theme::border_style(false)
    };
    let last_x = width - 1;
    let last_y = height - 1;
    for x in 1..last_x {
        buffer[(x, 0)].set_symbol("\u{2500}").set_style(border);
        buffer[(x, last_y)].set_symbol("\u{2500}").set_style(border);
    }
    for y in 1..last_y {
        buffer[(last_x, y)].set_symbol("\u{2502}").set_style(border);
        // The accent bar: the card's kind colour down its whole left edge.
        buffer[(0, y)]
            .set_symbol("\u{2503}")
            .set_style(Style::new().fg(accent));
    }
    for (x, y, symbol) in [
        (0, 0, "\u{256d}"),
        (last_x, 0, "\u{256e}"),
        (0, last_y, "\u{2570}"),
        (last_x, last_y, "\u{256f}"),
    ] {
        buffer[(x, y)].set_symbol(symbol).set_style(border);
    }

    let text_x = 2;
    let text_width = width.saturating_sub(4);
    let body = card_body(card, usize::from(text_width));

    // Title row: kind chip, agent title, unread dot and age.
    let chip = format!(" {} {} ", kind_glyph(card.kind), card.kind.label());
    let chip_style = Style::new()
        .fg(Color::Black)
        .bg(accent)
        .add_modifier(Modifier::BOLD);
    let chip_width = put(buffer, text_x, 1, &chip, text_width, chip_style);
    let age = age_text(card.age_ms);
    let age_marker = if card.is_looked { "" } else { "\u{25cf} " };
    let age_width = u16::try_from(age.width() + age_marker.width()).unwrap_or(0);
    let title_x = text_x + chip_width + 1;
    let title_room = text_width.saturating_sub(chip_width + 1 + age_width + 1);
    if title_room > 0 {
        let title = clip(&card.title, usize::from(title_room));
        put(
            buffer,
            title_x,
            1,
            &title,
            title_room,
            Style::new().add_modifier(Modifier::BOLD),
        );
    }
    if text_width > chip_width + age_width {
        let age_x = text_x + text_width - age_width;
        let marker_width = put(
            buffer,
            age_x,
            1,
            age_marker,
            age_width,
            Style::new().fg(accent),
        );
        put(buffer, age_x + marker_width, 1, &age, age_width, dim());
    }

    // Project row: where it lives, what runs it, how it was found; the
    // priority pips sit at the right end.
    let mut place = String::new();
    for part in [card.project.as_str(), card.agent_class.as_str()] {
        if part.is_empty() {
            continue;
        }
        if !place.is_empty() {
            place.push_str(" \u{b7} ");
        }
        place.push_str(part);
    }
    if !place.is_empty() {
        place.push_str(" \u{b7} ");
    }
    place.push_str(source_tag(card.source));
    let pips_width = u16::try_from(PRIORITY_PIPS).unwrap_or(5);
    let place_room = if text_width > pips_width + 3 {
        text_width - pips_width - 1
    } else {
        text_width
    };
    put(
        buffer,
        text_x,
        2,
        &clip(&place, usize::from(place_room)),
        place_room,
        dim(),
    );
    if text_width > pips_width + 3 {
        let (filled, empty) = priority_pips(card.priority);
        let pips_x = text_x + text_width - pips_width;
        let used = put(
            buffer,
            pips_x,
            2,
            &"\u{25cf}".repeat(filled),
            pips_width,
            Style::new().fg(accent),
        );
        put(
            buffer,
            pips_x + used,
            2,
            &"\u{25cb}".repeat(empty),
            pips_width.saturating_sub(used),
            dim(),
        );
    }

    // Text rows: reason, then the exact question in the kind colour.
    let mut y = 3;
    for line in &body.reason {
        put(buffer, text_x, y, line, text_width, Style::new());
        y += 1;
    }
    let question_style = Style::new().fg(accent).add_modifier(Modifier::ITALIC);
    for line in &body.question {
        put(buffer, text_x, y, "\u{258e}", 1, Style::new().fg(accent));
        put(
            buffer,
            text_x + 2,
            y,
            line,
            text_width.saturating_sub(2),
            question_style,
        );
        y += 1;
    }

    // Footer: who it waits on, and the jump affordance.
    let footer_y = height.saturating_sub(2);
    let jump = "Jump \u{25b8}";
    let jump_width = u16::try_from(jump.width()).unwrap_or(0);
    let jump_style = if look.is_selected || look.is_hovered {
        Style::new().fg(accent).add_modifier(Modifier::BOLD)
    } else {
        dim()
    };
    if text_width > jump_width {
        put(
            buffer,
            text_x + text_width - jump_width,
            footer_y,
            jump,
            jump_width,
            jump_style,
        );
    }
    if let Some(target) = &card.waiting_on {
        let room = text_width.saturating_sub(jump_width + 1);
        let waiting = format!(
            "{} waiting on {target}",
            kind_glyph(AttentionKind::WaitingOn)
        );
        put(
            buffer,
            text_x,
            footer_y,
            &clip(&waiting, usize::from(room)),
            room,
            Style::new().fg(kind_color(AttentionKind::WaitingOn)),
        );
    }
}

/// One item of the compact list: two lines.
fn paint_compact_item(buffer: &mut Buffer, card: &CardData, look: CardLook) {
    let area = buffer.area;
    if area.width < 4 || area.height == 0 {
        return;
    }
    let accent = kind_color(card.kind);
    let base = if look.is_selected {
        theme::selected_style()
    } else if look.is_hovered {
        Style::new().bg(theme::muted_accent_bg(look.scheme))
    } else {
        Style::new()
    };
    buffer.set_style(area, base);
    let width = area.width;
    let age = age_text(card.age_ms);
    let age_width = u16::try_from(age.width()).unwrap_or(0);
    let glyph_style = if look.is_selected {
        base
    } else {
        base.fg(accent)
    };
    let glyph_width = put(buffer, 0, 0, kind_glyph(card.kind), width, glyph_style);
    let title_room = width.saturating_sub(glyph_width + 1 + age_width + 1);
    put(
        buffer,
        glyph_width + 1,
        0,
        &clip(&card.title, usize::from(title_room)),
        title_room,
        base.add_modifier(Modifier::BOLD),
    );
    if width > age_width {
        put(
            buffer,
            width - age_width,
            0,
            &age,
            age_width,
            base.add_modifier(Modifier::DIM),
        );
    }
    if area.height > 1 {
        let detail = card
            .question
            .as_deref()
            .map(|question| format!("\u{201c}{}\u{201d}", question.trim()))
            .unwrap_or_else(|| card.reason.clone());
        let room = width.saturating_sub(glyph_width + 1);
        put(
            buffer,
            glyph_width + 1,
            1,
            &clip(&detail, usize::from(room)),
            room,
            base.add_modifier(Modifier::DIM),
        );
    }
}

fn paint_header(buffer: &mut Buffer, kind: AttentionKind, count: usize) {
    let width = buffer.area.width;
    if width < 4 {
        return;
    }
    let accent = kind_color(kind);
    let rule = Style::new().fg(accent).add_modifier(Modifier::DIM);
    let mut x = put(buffer, 0, 0, "\u{2500}\u{2500} ", width, rule);
    x += put(
        buffer,
        x,
        0,
        &format!("{} {} ", kind_glyph(kind), kind.label().to_uppercase()),
        width.saturating_sub(x),
        Style::new().fg(accent).add_modifier(Modifier::BOLD),
    );
    x += put(
        buffer,
        x,
        0,
        &format!(" {count} "),
        width.saturating_sub(x),
        Style::new()
            .fg(Color::Black)
            .bg(accent)
            .add_modifier(Modifier::BOLD),
    );
    x += put(
        buffer,
        x,
        0,
        &format!("  {}  ", kind_hint(kind)),
        width.saturating_sub(x),
        dim(),
    );
    for column in x..width {
        buffer[(column, 0)].set_symbol("\u{2500}").set_style(rule);
    }
}

fn blit(target: &mut Buffer, source: &Buffer, destination: Rect, skipped_rows: u16) {
    for dy in 0..destination.height {
        for dx in 0..destination.width {
            let from = (dx, dy + skipped_rows);
            if from.0 >= source.area.width || from.1 >= source.area.height {
                continue;
            }
            let (x, y) = (destination.x + dx, destination.y + dy);
            if x >= target.area.right() || y >= target.area.bottom() {
                continue;
            }
            target[(x, y)] = source[from].clone();
        }
    }
}

/// Dims everything under the dialog so it reads as modal.
pub fn dim_backdrop(buffer: &mut Buffer, area: Rect) {
    buffer.set_style(area, Style::new().add_modifier(Modifier::DIM));
}

/// Draws the whole dialog over the already-drawn screen.
pub fn render(
    frame: &mut Frame,
    screen: Rect,
    state: &DialogState,
    cards: &[CardData],
    scheme: ColorScheme,
) {
    let layout = compute(screen, cards);
    if !layout.is_drawn() {
        return;
    }
    dim_backdrop(frame.buffer_mut(), screen);
    Clear.render(layout.area, frame.buffer_mut());
    frame.render_widget(
        theme::block(true).borders(ratatui::widgets::Borders::ALL),
        layout.area,
    );
    let scroll = layout.clamp_scroll(state.scroll);
    let selected = state.selected_index(cards);

    paint_title(frame.buffer_mut(), &layout, cards);
    paint_strip(frame.buffer_mut(), &layout, cards);

    if cards.is_empty() {
        paint_empty_state(frame.buffer_mut(), layout.viewport);
    }
    for visible in layout.visible(scroll) {
        let mut tile = Buffer::empty(Rect::new(0, 0, visible.slot.width, visible.slot.height));
        match visible.slot.target {
            SlotTarget::Header { kind, count } => paint_header(&mut tile, kind, count),
            SlotTarget::Card { index } => {
                let Some(card) = cards.get(index) else {
                    continue;
                };
                let look = CardLook {
                    is_selected: index == selected,
                    is_hovered: state.hover == Some(index),
                    scheme,
                };
                if layout.is_compact {
                    paint_compact_item(&mut tile, card, look);
                } else {
                    paint_card(&mut tile, card, look);
                }
            }
        }
        blit(
            frame.buffer_mut(),
            &tile,
            visible.rect,
            visible.skipped_rows,
        );
    }
    paint_scrollbar(frame.buffer_mut(), &layout, scroll);
    paint_legend(frame.buffer_mut(), &layout);
    let toggle = if state.show_snoozed {
        "S hide snoozed"
    } else {
        "S show snoozed"
    };
    if layout.legend.width >= 17 {
        put(
            frame.buffer_mut(),
            layout.legend.right() - 17,
            layout.legend.y,
            toggle,
            17,
            dim(),
        );
    }
    for visible in layout.visible(scroll) {
        let SlotTarget::Card { index } = visible.slot.target else {
            continue;
        };
        let Some(card) = cards.get(index) else {
            continue;
        };
        if let Some(rect) = snooze_rect(&visible, card) {
            put(
                frame.buffer_mut(),
                rect.x,
                rect.y,
                &card.snooze_icon,
                rect.width,
                Style::new().fg(if card.is_snoozed {
                    Color::Yellow
                } else {
                    Color::Cyan
                }),
            );
            if card.is_snoozed && rect.x >= visible.rect.x + 10 {
                put(frame.buffer_mut(), rect.x - 9, rect.y, "snoozed", 8, dim());
            }
            if state.snooze_hover == Some(index) {
                let text = " Snooze until attention category changes ";
                let width = u16::try_from(text.width())
                    .unwrap_or(u16::MAX)
                    .min(screen.width);
                let popup = Rect::new(
                    rect.x.min(screen.right().saturating_sub(width)),
                    (rect.y + 1).min(screen.bottom().saturating_sub(1)),
                    width,
                    1,
                );
                frame
                    .buffer_mut()
                    .set_style(popup, Style::new().fg(Color::Black).bg(Color::Yellow));
                put(
                    frame.buffer_mut(),
                    popup.x,
                    popup.y,
                    text,
                    width,
                    Style::new().fg(Color::Black).bg(Color::Yellow),
                );
            }
        }
    }
    paint_close(frame.buffer_mut(), &layout, state.is_close_hovered);
}

fn snooze_rect(visible: &VisibleSlot, card: &CardData) -> Option<Rect> {
    if visible.skipped_rows != 0 || visible.rect.height == 0 {
        return None;
    }
    let width = u16::try_from(card.snooze_icon.width())
        .unwrap_or(u16::MAX)
        .max(2)
        .min(visible.rect.width.saturating_sub(2));
    if width == 0 {
        return None;
    }
    Some(Rect::new(
        visible.rect.right().saturating_sub(width + 1),
        visible.rect.y,
        width,
        1,
    ))
}

fn paint_title(buffer: &mut Buffer, layout: &DialogLayout, cards: &[CardData]) {
    let area = layout.area;
    let room = area.width.saturating_sub(10);
    if room < 8 {
        return;
    }
    let title = format!(" {ATTENTION_GLYPH} Needs you ");
    let mut x = area.x + 2;
    x += put(
        buffer,
        x,
        area.y,
        &title,
        room,
        theme::selected_style().add_modifier(Modifier::BOLD),
    );
    let badge_color = cards
        .first()
        .map_or_else(theme::accent_bg, |card| kind_color(card.kind));
    put(
        buffer,
        x,
        area.y,
        &format!(" {} ", cards.len()),
        room.saturating_sub(x - area.x - 2),
        Style::new()
            .fg(Color::Black)
            .bg(badge_color)
            .add_modifier(Modifier::BOLD),
    );
}

fn paint_close(buffer: &mut Buffer, layout: &DialogLayout, is_hovered: bool) {
    if layout.close.width == 0 {
        return;
    }
    let style = if is_hovered {
        Style::new()
            .fg(Color::Black)
            .bg(Color::Red)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().add_modifier(Modifier::BOLD)
    };
    put(buffer, layout.close.x, layout.close.y, "[x]", 3, style);
}

fn paint_strip(buffer: &mut Buffer, layout: &DialogLayout, cards: &[CardData]) {
    let strip = layout.strip;
    if strip.height == 0 || strip.width == 0 {
        return;
    }
    let mut x = strip.x;
    let right = strip.right();
    for kind in [
        AttentionKind::Question,
        AttentionKind::Stuck,
        AttentionKind::WaitingOn,
        AttentionKind::Changed,
    ] {
        let count = cards.iter().filter(|card| card.kind == kind).count();
        if count == 0 {
            continue;
        }
        let text = format!(" {} {count} {} ", kind_glyph(kind), kind.label());
        if x + u16::try_from(text.width()).unwrap_or(u16::MAX) > right {
            break;
        }
        x += put(
            buffer,
            x,
            strip.y,
            &text,
            right - x,
            Style::new()
                .fg(kind_color(kind))
                .add_modifier(Modifier::BOLD),
        );
        x += 1;
    }
    let summary = match cards.len() {
        0 => "nothing waiting".to_string(),
        1 => "1 agent needs you".to_string(),
        count => format!("{count} agents need you"),
    };
    let summary_width = u16::try_from(summary.width()).unwrap_or(u16::MAX);
    if right.saturating_sub(x) > summary_width + 1 {
        put(
            buffer,
            right - summary_width,
            strip.y,
            &summary,
            summary_width,
            dim(),
        );
    }
}

fn paint_scrollbar(buffer: &mut Buffer, layout: &DialogLayout, scroll: u32) {
    let bar = layout.scrollbar;
    if bar.width == 0 || bar.height == 0 {
        return;
    }
    let Some((start, length)) = thumb(
        layout.content_height,
        u32::from(layout.viewport.height),
        scroll,
        u32::from(bar.height),
    ) else {
        return;
    };
    for row in 0..u32::from(bar.height) {
        let is_thumb = row >= start && row < start + length;
        let (symbol, style) = if is_thumb {
            ("\u{2503}", Style::new().fg(theme::accent_bg()))
        } else {
            ("\u{2502}", dim())
        };
        let y = bar.y + u16::try_from(row).unwrap_or(0);
        buffer[(bar.x, y)].set_symbol(symbol).set_style(style);
    }
}

/// Key legend entries, most important first; the tail is dropped to fit.
const LEGEND: [(&str, &str); 6] = [
    ("Enter", "jump"),
    ("\u{2191}\u{2193}\u{2190}\u{2192}", "move"),
    ("PgUp/PgDn", "scroll"),
    ("Tab", "next"),
    ("Esc", "close"),
    ("1-9", "pick"),
];

fn paint_legend(buffer: &mut Buffer, layout: &DialogLayout) {
    let mut legend = layout.legend;
    if legend.width >= 17 {
        legend.width = legend.width.saturating_sub(18);
    }
    if legend.height == 0 || legend.width == 0 {
        return;
    }
    let mut entries: Vec<(&str, &str)> = LEGEND.to_vec();
    let width_of = |entries: &[(&str, &str)]| {
        entries
            .iter()
            .map(|(key, action)| key.width() + 1 + action.width())
            .sum::<usize>()
            + entries.len().saturating_sub(1) * 3
    };
    // "Esc close" is the one entry that must survive; drop the others first.
    while width_of(&entries) > usize::from(legend.width) && entries.len() > 1 {
        let droppable = entries
            .iter()
            .rposition(|(key, _)| *key != "Esc" && *key != "Enter")
            .unwrap_or(entries.len() - 1);
        entries.remove(droppable);
    }
    let mut x = legend.x;
    for (index, (key, action)) in entries.iter().enumerate() {
        if index > 0 {
            x += put(buffer, x, legend.y, " \u{b7} ", legend.right() - x, dim());
        }
        x += put(
            buffer,
            x,
            legend.y,
            key,
            legend.right().saturating_sub(x),
            Style::new().add_modifier(Modifier::BOLD),
        );
        x += put(
            buffer,
            x,
            legend.y,
            &format!(" {action}"),
            legend.right().saturating_sub(x),
            dim(),
        );
    }
}

fn paint_empty_state(buffer: &mut Buffer, viewport: Rect) {
    if viewport.width < 12 || viewport.height < 3 {
        if viewport.width > 0 && viewport.height > 0 {
            let text = clip("All clear", usize::from(viewport.width));
            let x = viewport.x + (viewport.width - u16::try_from(text.width()).unwrap_or(0)) / 2;
            put(
                buffer,
                x,
                viewport.y + viewport.height / 2,
                &text,
                viewport.width,
                dim(),
            );
        }
        return;
    }
    let green = Style::new().fg(kind_color(AttentionKind::Changed));
    let art = [
        "\u{256d}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256e}",
        "\u{2502}   \u{2713}   \u{2502}",
        "\u{2570}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256f}",
    ];
    let text = [
        ("All clear", Style::new().add_modifier(Modifier::BOLD)),
        ("Nothing needs you right now.", Style::new()),
        (
            "Questions, blockers and finished work will appear here by themselves.",
            dim(),
        ),
    ];
    let with_art = viewport.height >= 8;
    let total_rows = text.len() + if with_art { art.len() + 1 } else { 0 };
    let first_row = viewport.y
        + viewport
            .height
            .saturating_sub(u16::try_from(total_rows).unwrap_or(0))
            / 2;
    let mut y = first_row;
    let centre = |text: &str| {
        let width = u16::try_from(text.width()).unwrap_or(0).min(viewport.width);
        viewport.x + (viewport.width - width) / 2
    };
    if with_art {
        for line in art {
            put(buffer, centre(line), y, line, viewport.width, green);
            y += 1;
        }
        y += 1;
    }
    for (line, style) in text {
        let shown = clip(line, usize::from(viewport.width));
        put(buffer, centre(&shown), y, &shown, viewport.width, style);
        y += 1;
    }
}

// ----------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn card(id: u64, kind: AttentionKind) -> CardData {
        CardData {
            pane_id: NodeId(id),
            kind,
            title: format!("agent-{id}"),
            project: "ilium".into(),
            agent_class: "Claude".into(),
            reason: "Waiting for your answer or approval".into(),
            question: None,
            waiting_on: None,
            priority: 80,
            source: AttentionSource::Instant,
            age_ms: 125_000,
            is_looked: false,
            is_snoozed: false,
            snooze_icon: "💤".into(),
        }
    }

    fn mixed_cards() -> Vec<CardData> {
        let mut cards = vec![
            card(1, AttentionKind::Question),
            card(2, AttentionKind::Question),
            card(3, AttentionKind::Stuck),
            card(4, AttentionKind::WaitingOn),
            card(5, AttentionKind::Changed),
            card(6, AttentionKind::Changed),
        ];
        cards[0].question = Some("Delete the build folder?".into());
        cards[3].waiting_on = Some("builder".into());
        cards
    }

    fn many_cards(count: u64) -> Vec<CardData> {
        (1..=count)
            .map(|id| card(id, AttentionKind::Question))
            .collect()
    }

    fn screen(width: u16, height: u16) -> Rect {
        Rect::new(0, 0, width, height)
    }

    fn draw_text(width: u16, height: u16, state: &DialogState, cards: &[CardData]) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), state, cards, ColorScheme::Dark))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn click(layout: &DialogLayout, scroll: u32, card: usize) -> Position {
        let visible = layout
            .visible(scroll)
            .into_iter()
            .find(|visible| visible.slot.target == SlotTarget::Card { index: card })
            .expect("card is visible");
        Position::new(visible.rect.x + 2, visible.rect.y + visible.rect.height / 2)
    }

    #[test]
    fn snooze_icon_has_its_own_click_and_hover_target() {
        let cards = vec![card(1, AttentionKind::Changed)];
        let screen = Rect::new(0, 0, 120, 40);
        let layout = compute(screen, &cards);
        let visible = layout
            .visible(0)
            .into_iter()
            .find(|slot| matches!(slot.slot.target, SlotTarget::Card { .. }))
            .unwrap();
        let rect = snooze_rect(&visible, &cards[0]).unwrap();
        let position = Position::new(rect.x, rect.y);
        let mut state = DialogState::default();
        state.handle_mouse(MouseEventKind::Moved, position, screen, &cards);
        assert_eq!(state.snooze_hover, Some(0));
        assert_eq!(
            state.handle_mouse(
                MouseEventKind::Down(crossterm::event::MouseButton::Left),
                position,
                screen,
                &cards
            ),
            DialogAction::Snooze(cards[0].pane_id)
        );
        let toggle = Position::new(layout.legend.right() - 17, layout.legend.y);
        state.handle_mouse(
            MouseEventKind::Down(crossterm::event::MouseButton::Left),
            toggle,
            screen,
            &cards,
        );
        assert!(state.show_snoozed);
    }

    #[test]
    fn snoozed_visibility_toggles_with_s_even_when_empty() {
        let mut state = DialogState::default();
        state.handle_key(
            KeyCode::Char('s'),
            KeyModifiers::NONE,
            Rect::new(0, 0, 100, 40),
            &[],
        );
        assert!(state.show_snoozed);
        state.handle_key(
            KeyCode::Char('S'),
            KeyModifiers::SHIFT,
            Rect::new(0, 0, 100, 40),
            &[],
        );
        assert!(!state.show_snoozed);
    }

    #[test]
    fn large_footer_plus_uses_the_whole_three_by_five_hit_target() {
        let (main, plus) = split_button(Rect::new(2, 20, 30, 3));
        assert_eq!(main, Rect::new(2, 20, 25, 3));
        assert_eq!(plus, Some(Rect::new(27, 20, 5, 3)));
    }

    #[test]
    fn dialog_fills_about_ninety_percent_centred_and_goes_full_screen_when_compact() {
        for (width, height) in [(80, 24), (120, 40), (200, 60)] {
            let area = dialog_area(screen(width, height));
            assert_eq!(area.width, width * 9 / 10);
            assert_eq!(area.height, height * 9 / 10);
            assert_eq!(area.x, (width - area.width) / 2);
            assert_eq!(area.y, (height - area.height) / 2);
        }
        assert_eq!(dialog_area(screen(40, 12)), screen(40, 12));
        assert_eq!(dialog_area(screen(59, 40)), screen(59, 40));
        assert_eq!(dialog_area(screen(120, 15)), screen(120, 15));
        assert_eq!(dialog_area(screen(9, 30)).width, 0);
        assert_eq!(dialog_area(screen(80, 4)).height, 0);
    }

    #[test]
    fn column_count_switches_at_one_hundred_and_one_hundred_sixty_columns() {
        assert_eq!(column_count(40), 1);
        assert_eq!(column_count(99), 1);
        assert_eq!(column_count(100), 2);
        assert_eq!(column_count(159), 2);
        assert_eq!(column_count(160), 3);
        assert_eq!(column_count(300), 3);
    }

    #[test]
    fn layout_is_sane_at_every_reference_size() {
        let cards = mixed_cards();
        for (width, height, columns, compact) in [
            (40, 12, 1, true),
            (80, 24, 1, false),
            (120, 40, 2, false),
            (200, 60, 3, false),
        ] {
            let layout = compute(screen(width, height), &cards);
            assert_eq!(layout.columns, columns, "{width}x{height}");
            assert_eq!(layout.is_compact, compact, "{width}x{height}");
            assert!(layout.is_drawn());
            assert!(screen(width, height).contains(Position::new(layout.area.x, layout.area.y)));
            assert!(layout.area.right() <= width && layout.area.bottom() <= height);
            assert!(layout.viewport.right() <= layout.scrollbar.x.max(layout.viewport.right()));
            assert!(layout.viewport.bottom() <= layout.area.bottom());
            for slot in &layout.slots {
                assert!(
                    slot.x + slot.width <= layout.viewport.width,
                    "{width}x{height} slot {slot:?} overflows"
                );
            }
            // Every card has exactly one slot and the slots never overlap.
            assert_eq!(layout.card_slots.len(), cards.len());
            for pair in layout.slots.windows(2) {
                let first_bottom = pair[0].y + u32::from(pair[0].height);
                let overlaps = pair[1].y < first_bottom
                    && pair[1].x < pair[0].x + pair[0].width
                    && pair[0].x < pair[1].x + pair[1].width;
                assert!(!overlaps, "{width}x{height}: {pair:?}");
            }
        }
    }

    #[test]
    fn cards_are_grouped_under_one_header_per_kind_in_priority_order() {
        let layout = compute(screen(120, 40), &mixed_cards());
        let headers: Vec<(AttentionKind, usize)> = layout
            .slots
            .iter()
            .filter_map(|slot| match slot.target {
                SlotTarget::Header { kind, count } => Some((kind, count)),
                SlotTarget::Card { .. } => None,
            })
            .collect();
        assert_eq!(
            headers,
            vec![
                (AttentionKind::Question, 2),
                (AttentionKind::Stuck, 1),
                (AttentionKind::WaitingOn, 1),
                (AttentionKind::Changed, 2),
            ]
        );
        // Two columns: the two questions share a grid row.
        assert_eq!(layout.grid_rows[0].count, 2);
        assert_eq!(layout.grid_rows[1].count, 1);
    }

    #[test]
    fn card_height_grows_with_the_question_and_rows_share_the_tallest() {
        let mut tall = card(1, AttentionKind::Question);
        tall.question = Some(
            "Should I delete the entire build folder and rebuild everything from scratch?".into(),
        );
        let short = card(2, AttentionKind::Question);
        assert!(card_height(&tall, 42) > card_height(&short, 42));
        let layout = compute(screen(120, 40), &[tall, short]);
        let heights: Vec<u16> = layout
            .slots
            .iter()
            .filter(|slot| matches!(slot.target, SlotTarget::Card { .. }))
            .map(|slot| slot.height)
            .collect();
        assert_eq!(heights[0], heights[1]);
    }

    #[test]
    fn scroll_bounds_clamp_and_keep_the_selection_in_view() {
        let cards = many_cards(12);
        let layout = compute(screen(80, 24), &cards);
        assert!(layout.max_scroll() > 0);
        assert_eq!(layout.clamp_scroll(u32::MAX), layout.max_scroll());
        // The first card is already visible at scroll 0.
        assert_eq!(layout.ensure_visible(0, 0), 0);
        // A card far down needs scrolling and ends up inside the viewport.
        let scroll = layout.ensure_visible(11, 0);
        assert!(scroll > 0 && scroll <= layout.max_scroll());
        let slot = layout.slots[layout.card_slots[11]];
        assert!(slot.y >= u64::from(scroll) as u32);
        assert!(slot.y + u32::from(slot.height) <= scroll + u32::from(layout.viewport.height));
        // Going back up to the first card brings its group header along.
        assert_eq!(layout.ensure_visible(0, scroll), 0);
        // Content shorter than the viewport never scrolls.
        let short = compute(screen(80, 24), &many_cards(1));
        assert_eq!(short.max_scroll(), 0);
        assert_eq!(short.clamp_scroll(50), 0);
    }

    #[test]
    fn thumb_math_stays_inside_the_track() {
        assert_eq!(thumb(10, 10, 0, 8), None);
        assert_eq!(thumb(10, 20, 0, 8), None);
        assert_eq!(thumb(100, 10, 0, 10), Some((0, 1)));
        let (start, length) = thumb(100, 10, 90, 10).unwrap();
        assert_eq!(start + length, 10);
        let (start, length) = thumb(40, 20, 10, 10).unwrap();
        assert_eq!(length, 5);
        assert!(start > 0 && start + length <= 10);
        assert_eq!(thumb(100, 10, 5, 0), None);
    }

    #[test]
    fn hit_testing_finds_cards_the_close_button_the_outside_and_the_scrollbar() {
        let cards = many_cards(12);
        let layout = compute(screen(120, 40), &cards);
        for index in 0..4 {
            assert_eq!(layout.hit(0, click(&layout, 0, index)), Hit::Card(index));
        }
        let close = Position::new(layout.close.x + 1, layout.close.y);
        assert_eq!(layout.hit(0, close), Hit::Close);
        assert_eq!(layout.hit(0, Position::new(0, 0)), Hit::Outside);
        assert_eq!(
            layout.hit(0, Position::new(layout.area.right(), layout.area.y + 3)),
            Hit::Outside
        );
        let bar = Position::new(layout.scrollbar.x, layout.scrollbar.y + 2);
        assert_eq!(layout.hit(0, bar), Hit::Scrollbar(2));
        // The title border and the legend belong to no card.
        let title = Position::new(layout.area.x + 3, layout.area.y);
        assert_eq!(layout.hit(0, title), Hit::Inside);
        assert_eq!(
            layout.hit(0, Position::new(layout.legend.x, layout.legend.y)),
            Hit::Inside
        );
        // A group header row is not a card.
        let header = Position::new(layout.viewport.x + 1, layout.viewport.y);
        assert_eq!(layout.hit(0, header), Hit::Inside);
    }

    #[test]
    fn a_scrollbar_click_maps_the_track_onto_the_scroll_range() {
        let layout = compute(screen(80, 24), &many_cards(30));
        assert_eq!(layout.scroll_for_track_row(0), 0);
        let last_row = layout.scrollbar.height - 1;
        assert_eq!(layout.scroll_for_track_row(last_row), layout.max_scroll());
        assert!(layout.scroll_for_track_row(last_row / 2) > 0);
    }

    #[test]
    fn keyboard_moves_through_the_grid_by_rows_and_columns() {
        let cards = many_cards(5);
        let mut state = DialogState::default();
        let size = screen(120, 40);
        let press = |state: &mut DialogState, code: KeyCode| {
            state.handle_key(code, KeyModifiers::NONE, size, &cards)
        };
        assert_eq!(state.selected_index(&cards), 0);
        press(&mut state, KeyCode::Right);
        assert_eq!(state.selected_index(&cards), 1);
        press(&mut state, KeyCode::Right);
        assert_eq!(state.selected_index(&cards), 1, "end of the grid row");
        press(&mut state, KeyCode::Down);
        assert_eq!(state.selected_index(&cards), 3);
        press(&mut state, KeyCode::Down);
        assert_eq!(
            state.selected_index(&cards),
            4,
            "short last row clamps the column"
        );
        press(&mut state, KeyCode::Down);
        assert_eq!(state.selected_index(&cards), 4);
        press(&mut state, KeyCode::Up);
        assert_eq!(state.selected_index(&cards), 2);
        press(&mut state, KeyCode::Left);
        assert_eq!(state.selected_index(&cards), 2);
        press(&mut state, KeyCode::Home);
        assert_eq!(state.selected_index(&cards), 0);
        press(&mut state, KeyCode::End);
        assert_eq!(state.selected_index(&cards), 4);
        press(&mut state, KeyCode::Tab);
        assert_eq!(state.selected_index(&cards), 0, "tab wraps around");
        press(&mut state, KeyCode::BackTab);
        assert_eq!(state.selected_index(&cards), 4, "shift-tab wraps back");
        press(&mut state, KeyCode::Char('2'));
        assert_eq!(state.selected_index(&cards), 1);
        press(&mut state, KeyCode::Char('9'));
        assert_eq!(state.selected_index(&cards), 1, "no ninth card");
    }

    #[test]
    fn in_a_single_column_left_and_right_act_like_up_and_down() {
        let cards = many_cards(3);
        let mut state = DialogState::default();
        let size = screen(80, 24);
        state.handle_key(KeyCode::Right, KeyModifiers::NONE, size, &cards);
        assert_eq!(state.selected_index(&cards), 1);
        state.handle_key(KeyCode::Left, KeyModifiers::NONE, size, &cards);
        assert_eq!(state.selected_index(&cards), 0);
    }

    #[test]
    fn page_keys_jump_a_viewport_and_scroll_with_the_selection() {
        let cards = many_cards(40);
        let size = screen(80, 24);
        let layout = compute(size, &cards);
        let mut state = DialogState::default();
        state.handle_key(KeyCode::PageDown, KeyModifiers::NONE, size, &cards);
        let after_down = state.selected_index(&cards);
        assert!(after_down > 1, "page down moved only {after_down}");
        assert!(state.scroll <= layout.max_scroll());
        let slot = layout.slots[layout.card_slots[after_down]];
        assert!(slot.y >= state.scroll);
        assert!(
            slot.y + u32::from(slot.height) <= state.scroll + u32::from(layout.viewport.height)
        );
        state.handle_key(KeyCode::PageUp, KeyModifiers::NONE, size, &cards);
        assert!(state.selected_index(&cards) < after_down);
        state.handle_key(KeyCode::End, KeyModifiers::NONE, size, &cards);
        assert_eq!(state.selected_index(&cards), 39);
        assert_eq!(state.scroll, layout.max_scroll());
        state.handle_key(KeyCode::Home, KeyModifiers::NONE, size, &cards);
        assert_eq!(state.scroll, 0);
    }

    #[test]
    fn enter_opens_the_selected_agent_and_escape_closes() {
        let cards = mixed_cards();
        let size = screen(120, 40);
        let mut state = DialogState::default();
        state.handle_key(KeyCode::Down, KeyModifiers::NONE, size, &cards);
        let selected = state.selected_index(&cards);
        assert_eq!(
            state.handle_key(KeyCode::Enter, KeyModifiers::NONE, size, &cards),
            DialogAction::Open(cards[selected].pane_id)
        );
        assert_eq!(
            state.handle_key(KeyCode::Esc, KeyModifiers::NONE, size, &cards),
            DialogAction::Close
        );
        assert_eq!(
            state.handle_key(KeyCode::Char('q'), KeyModifiers::NONE, size, &cards),
            DialogAction::Close
        );
        assert_eq!(
            state.handle_key(KeyCode::Char('x'), KeyModifiers::NONE, size, &cards),
            DialogAction::None
        );
    }

    #[test]
    fn the_selection_follows_the_agent_when_the_queue_reorders_or_shrinks() {
        let mut cards = many_cards(4);
        let mut state = DialogState::default();
        state.select(2, &cards);
        assert_eq!(state.selected_pane, Some(NodeId(3)));
        cards.swap(0, 2);
        assert_eq!(state.selected_index(&cards), 0, "same agent, new place");
        cards.retain(|card| card.pane_id != NodeId(3));
        assert!(
            state.selected_index(&cards) < cards.len(),
            "clamped, no panic"
        );
        assert_eq!(state.selected_index(&[]), 0);
    }

    #[test]
    fn mouse_click_on_a_card_opens_it_and_outside_or_close_closes() {
        let cards = mixed_cards();
        let size = screen(120, 40);
        let layout = compute(size, &cards);
        let mut state = DialogState::default();
        let down = MouseEventKind::Down(crossterm::event::MouseButton::Left);
        assert_eq!(
            state.handle_mouse(down, click(&layout, 0, 2), size, &cards),
            DialogAction::Open(cards[2].pane_id)
        );
        assert_eq!(
            state.handle_mouse(down, Position::new(0, 0), size, &cards),
            DialogAction::Close
        );
        let close = Position::new(layout.close.x + 1, layout.close.y);
        assert_eq!(
            state.handle_mouse(down, close, size, &cards),
            DialogAction::Close
        );
        // A click on dead space inside the dialog does nothing.
        let dead = Position::new(layout.legend.x, layout.legend.y);
        assert_eq!(
            state.handle_mouse(down, dead, size, &cards),
            DialogAction::None
        );
    }

    #[test]
    fn hover_tracks_the_card_and_the_close_button_and_the_wheel_scrolls() {
        let cards = many_cards(30);
        let size = screen(80, 24);
        let layout = compute(size, &cards);
        let mut state = DialogState::default();
        state.handle_mouse(MouseEventKind::Moved, click(&layout, 0, 1), size, &cards);
        assert_eq!(state.hover, Some(1));
        state.handle_mouse(
            MouseEventKind::Moved,
            Position::new(layout.close.x, layout.close.y),
            size,
            &cards,
        );
        assert_eq!(state.hover, None);
        assert!(state.is_close_hovered);
        state.handle_mouse(
            MouseEventKind::ScrollDown,
            click(&layout, 0, 0),
            size,
            &cards,
        );
        assert_eq!(state.scroll, WHEEL_ROWS);
        state.handle_mouse(MouseEventKind::ScrollUp, click(&layout, 0, 0), size, &cards);
        state.handle_mouse(MouseEventKind::ScrollUp, click(&layout, 0, 0), size, &cards);
        assert_eq!(state.scroll, 0, "never below zero");
        for _ in 0..200 {
            state.handle_mouse(
                MouseEventKind::ScrollDown,
                click(&layout, 0, 0),
                size,
                &cards,
            );
        }
        assert_eq!(state.scroll, layout.max_scroll(), "never past the end");
    }

    #[test]
    fn dragging_the_scrollbar_thumb_scrolls() {
        let cards = many_cards(30);
        let size = screen(80, 24);
        let layout = compute(size, &cards);
        let mut state = DialogState::default();
        let left = crossterm::event::MouseButton::Left;
        let top = Position::new(layout.scrollbar.x, layout.scrollbar.y);
        state.handle_mouse(MouseEventKind::Down(left), top, size, &cards);
        assert!(state.is_scrollbar_dragging);
        let bottom = Position::new(layout.scrollbar.x, layout.scrollbar.bottom() - 1);
        state.handle_mouse(MouseEventKind::Drag(left), bottom, size, &cards);
        assert_eq!(state.scroll, layout.max_scroll());
        state.handle_mouse(MouseEventKind::Up(left), bottom, size, &cards);
        assert!(!state.is_scrollbar_dragging);
        // Without a press on the bar a drag does nothing.
        state.scroll = 0;
        state.handle_mouse(MouseEventKind::Drag(left), bottom, size, &cards);
        assert_eq!(state.scroll, 0);
    }

    #[test]
    fn plus_button_takes_the_right_end_of_a_wide_enough_row() {
        let (main, plus) = split_button(Rect::new(2, 20, 30, 1));
        assert_eq!(main, Rect::new(2, 20, 27, 1));
        assert_eq!(plus, Some(Rect::new(29, 20, PLUS_WIDTH, 1)));
        assert_eq!(main.right(), plus.unwrap().x);
        let narrow = Rect::new(0, 0, MINIMUM_BUTTON_WIDTH_FOR_PLUS - 1, 1);
        assert_eq!(split_button(narrow), (narrow, None));
        assert_eq!(split_button(Rect::new(0, 0, 20, 0)).1, None);
    }

    #[test]
    fn plus_button_draws_and_highlights_on_hover() {
        let mut terminal = Terminal::new(TestBackend::new(10, 1)).unwrap();
        let area = Rect::new(2, 0, 3, 1);
        terminal
            .draw(|frame| render_plus(frame, area, false))
            .unwrap();
        let plain = terminal.backend().buffer().clone();
        assert_eq!(plain[(3, 0)].symbol(), "+");
        assert_eq!(plain[(2, 0)].symbol(), "[");
        terminal
            .draw(|frame| render_plus(frame, area, true))
            .unwrap();
        let hovered = terminal.backend().buffer().clone();
        assert_ne!(plain[(3, 0)].bg, hovered[(3, 0)].bg);
    }

    #[test]
    fn rendered_dialog_shows_title_kind_headers_and_the_exact_question() {
        let cards = mixed_cards();
        let text = draw_text(120, 40, &DialogState::default(), &cards);
        for needle in [
            "Needs you",
            "QUESTION",
            "STUCK",
            "WAITING",
            "CHANGED",
            "Delete the build folder?",
            "agent-1",
            "ilium",
            "Claude",
            "2m ago",
            "Jump",
            "waiting on builder",
            "[x]",
            "Esc close",
            "6 agents need you",
        ] {
            assert!(text.contains(needle), "missing {needle:?} in\n{text}");
        }
        let rows: Vec<&str> = text.lines().collect();
        let row_of = |needle: &str| rows.iter().position(|row| row.contains(needle)).unwrap();
        assert!(row_of("QUESTION") < row_of("STUCK"));
        assert!(row_of("STUCK") < row_of("WAITING"));
        assert!(row_of("WAITING") < row_of("CHANGED"));
        assert!(row_of("Needs you") < row_of("QUESTION"));
        assert!(row_of("QUESTION") < row_of("Delete the build folder?"));
        // Rounded corners and the accent bar are present.
        assert!(text.contains('\u{256d}') && text.contains('\u{2503}'));
    }

    #[test]
    fn selection_and_hover_change_how_a_card_looks() {
        let cards = many_cards(2);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let mut draw = |state: &DialogState| {
            terminal
                .draw(|frame| render(frame, frame.area(), state, &cards, ColorScheme::Dark))
                .unwrap();
            terminal.backend().buffer().clone()
        };
        let layout = compute(screen(80, 24), &cards);
        let first = layout
            .visible(0)
            .into_iter()
            .find(|v| v.slot.target == SlotTarget::Card { index: 0 })
            .unwrap();
        let second = layout
            .visible(0)
            .into_iter()
            .find(|v| v.slot.target == SlotTarget::Card { index: 1 })
            .unwrap();
        let state = DialogState::default();
        let normal = draw(&state);
        let probe = |buffer: &Buffer, rect: Rect| buffer[(rect.x + 4, rect.y + rect.height - 3)].bg;
        assert_ne!(
            probe(&normal, first.rect),
            probe(&normal, second.rect),
            "the selected card has a filled background"
        );
        let hovered = DialogState {
            hover: Some(1),
            ..DialogState::default()
        };
        let with_hover = draw(&hovered);
        assert_eq!(
            probe(&with_hover, first.rect),
            probe(&with_hover, second.rect)
        );
    }

    #[test]
    fn empty_queue_shows_a_friendly_centred_state() {
        let text = draw_text(120, 40, &DialogState::default(), &[]);
        assert!(text.contains("All clear"), "{text}");
        assert!(text.contains("Nothing needs you right now."));
        assert!(text.contains("nothing waiting"));
        assert!(text.contains("Needs you"));
        let rows: Vec<&str> = text.lines().collect();
        let row = rows.iter().find(|row| row.contains("All clear")).unwrap();
        let column = row.find("All clear").unwrap();
        // Roughly centred on the 120-column screen.
        assert!((45..75).contains(&row[..column].chars().count()), "{row}");
        // An empty dialog still closes and does not open anything.
        let mut state = DialogState::default();
        assert_eq!(
            state.handle_key(KeyCode::Enter, KeyModifiers::NONE, screen(120, 40), &[]),
            DialogAction::None
        );
        assert_eq!(
            state.handle_key(KeyCode::Down, KeyModifiers::NONE, screen(120, 40), &[]),
            DialogAction::None
        );
    }

    #[test]
    fn compact_list_is_used_on_a_tiny_terminal_and_shows_two_lines_per_item() {
        let mut cards = mixed_cards();
        cards[0].title = "a-very-long-agent-title-that-must-be-clipped-by-the-compact-list".into();
        let text = draw_text(40, 12, &DialogState::default(), &cards);
        assert!(text.contains("Needs you"), "{text}");
        assert!(text.contains("Delete the build folder?"), "{text}");
        assert!(
            text.contains('\u{2026}'),
            "long title ends in an ellipsis\n{text}"
        );
        assert!(!text.contains("QUESTION"), "compact has no group headers");
        assert!(text.lines().all(|row| row.chars().count() <= 40));
    }

    #[test]
    fn rendering_never_panics_at_any_size_with_any_content() {
        let mut cards = mixed_cards();
        cards[1].title = "\u{1F514}".repeat(80);
        cards[2].reason = "word ".repeat(200);
        cards[3].question = Some("q".repeat(500));
        cards[4].project = "p".repeat(300);
        for (width, height) in [
            (0, 0),
            (1, 1),
            (4, 3),
            (9, 5),
            (10, 5),
            (12, 6),
            (19, 8),
            (20, 8),
            (39, 11),
            (40, 12),
            (59, 15),
            (60, 16),
            (79, 23),
            (80, 24),
            (99, 30),
            (100, 30),
            (159, 50),
            (160, 50),
            (200, 60),
            (300, 80),
        ] {
            for set in [&cards[..], &cards[..1], &[][..]] {
                let state = DialogState {
                    hover: Some(0),
                    scroll: 5,
                    ..DialogState::default()
                };
                let _ = draw_text(width, height, &state, set);
                let layout = compute(screen(width, height), set);
                let _ = layout.hit(3, Position::new(1, 1));
                let mut keys = DialogState::default();
                for code in [
                    KeyCode::Down,
                    KeyCode::PageDown,
                    KeyCode::End,
                    KeyCode::PageUp,
                    KeyCode::Home,
                    KeyCode::Right,
                    KeyCode::Tab,
                ] {
                    keys.handle_key(code, KeyModifiers::NONE, screen(width, height), set);
                }
            }
        }
    }

    #[test]
    fn priority_pips_scale_from_one_to_five() {
        assert_eq!(priority_pips(0), (0, 5));
        assert_eq!(priority_pips(1), (1, 4));
        assert_eq!(priority_pips(50), (3, 2));
        assert_eq!(priority_pips(100), (5, 0));
        assert_eq!(priority_pips(250), (5, 0));
    }

    #[test]
    fn scrolled_content_is_clipped_to_the_viewport_without_touching_the_chrome() {
        let cards = many_cards(10);
        let layout = compute(screen(80, 24), &cards);
        let state = DialogState {
            scroll: 4,
            ..DialogState::default()
        };
        let text = draw_text(80, 24, &state, &cards);
        let rows: Vec<&str> = text.lines().collect();
        assert!(rows[usize::from(layout.area.y)].contains("Needs you"));
        assert!(rows[usize::from(layout.legend.y)].contains("Esc close"));
        assert!(rows[usize::from(layout.area.bottom() - 1)].contains('\u{2570}'));
    }
}
