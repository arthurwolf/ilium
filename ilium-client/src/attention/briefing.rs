//! The briefing box: a short account, shown at the top of an agent pane, of
//! why the user was brought to that agent and what its state is.
//!
//! Pure logic with no `App` access, so every rule is unit-testable:
//! - [`BriefingState`]: which panes show a briefing, collapse and dismiss
//!   state, and when an entry ends.
//! - [`reserved_rows`] / [`body_rows`]: the one place that decides how many
//!   rows the box needs. Layout (`App::pane_viewports`) and drawing both call
//!   it, so the PTY size and the drawn box can never disagree.
//! - [`render`], [`buttons`], [`button_at`]: drawing and hit-testing.
//!
//! An entry is taken from the queue item at the moment the user arrives,
//! because the evidence behind some items (a finished agent that the user has
//! now read) disappears the moment the agent is focused. While the queue still
//! holds an item for the pane the entry follows it, so a late LLM reply
//! updates the box in place.

use std::collections::HashMap;

use ilium_core::{AgentTurn, NodeId};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use super::item::AttentionItem;
use super::panel::{clip, format_age, kind_glyph, wrap};
use crate::theme::{self, ColorScheme};

/// Most rows the box ever takes, header included.
pub const MAXIMUM_ROWS: u16 = 6;
/// Narrowest pane that still gets a box; below this nothing is reserved.
pub const MINIMUM_WIDTH: u16 = 24;
/// Terminal rows that must stay below the box (and its rule).
pub const MINIMUM_TERMINAL_ROWS: u16 = 3;
/// Width from which section labels get their own column.
const LABELLED_WIDTH: u16 = 40;
/// Columns of the label column: the longest label plus one blank.
const LABEL_COLUMN: usize = 8;
/// Columns left of the text: one blank, the gutter, one blank.
const INSET_LEFT: u16 = 3;
const INSET_RIGHT: u16 = 2;
/// Most lines one section may take before it is cut with an ellipsis.
const MAXIMUM_SECTION_LINES: usize = 3;
const GUTTER: &str = "\u{258e}";
const COLLAPSE_GLYPH_OPEN: &str = "\u{25be}";
const COLLAPSE_GLYPH_CLOSED: &str = "\u{25b8}";
const DISMISS_GLYPH: &str = "\u{2715}";

/// One click target of the box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BriefingButton {
    /// Folds the box to its header line, or unfolds it.
    Collapse,
    /// Hides the box for this item until a newer item arrives.
    Dismiss,
}

impl BriefingButton {
    pub const fn glyph(self, collapsed: bool) -> &'static str {
        match (self, collapsed) {
            (Self::Collapse, false) => COLLAPSE_GLYPH_OPEN,
            (Self::Collapse, true) => COLLAPSE_GLYPH_CLOSED,
            (Self::Dismiss, _) => DISMISS_GLYPH,
        }
    }

    pub const fn tooltip(self, collapsed: bool) -> &'static str {
        match (self, collapsed) {
            (Self::Collapse, false) => "Fold the briefing to one line",
            (Self::Collapse, true) => "Show the whole briefing",
            (Self::Dismiss, _) => "Hide this briefing",
        }
    }
}

/// Everything the box shows, borrowed from the item it describes.
#[derive(Debug, Clone, Copy)]
pub struct View<'a> {
    pub item: &'a AttentionItem,
    /// Title of the agent this one waits on, when the item names one.
    pub waiting_on: Option<&'a str>,
    /// Milliseconds since the item first appeared.
    pub age_ms: u64,
    pub collapsed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextStyle {
    Plain,
    Quote,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Section {
    label: &'static str,
    text: String,
    style: TextStyle,
}

/// One drawn body row: the label on a section's first row, then its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyRow {
    pub label: Option<&'static str>,
    pub text: String,
    is_quote: bool,
}

fn non_blank(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|text| !text.is_empty())
}

/// The sections present for `view`, most important first. A section whose
/// data is absent is left out, never filled with invented text.
fn sections(view: &View) -> Vec<Section> {
    let item = view.item;
    let mut sections = Vec::new();
    let why = non_blank(item.briefing.why_here.as_deref())
        .or_else(|| non_blank(Some(item.reason.as_str())));
    if let Some(why) = why {
        sections.push(Section {
            label: "Why",
            text: why.to_string(),
            style: TextStyle::Plain,
        });
    }
    if let Some(status) = non_blank(item.briefing.status_summary.as_deref()) {
        sections.push(Section {
            label: "Status",
            text: status.to_string(),
            style: TextStyle::Plain,
        });
    }
    if let Some(question) = non_blank(item.question.as_deref()) {
        sections.push(Section {
            label: "Asked",
            text: format!("\u{201c}{question}\u{201d}"),
            style: TextStyle::Quote,
        });
    }
    if let Some(target) = non_blank(view.waiting_on) {
        sections.push(Section {
            label: "Waiting",
            text: target.to_string(),
            style: TextStyle::Plain,
        });
    }
    if let Some(action) = non_blank(item.briefing.suggested_action.as_deref()) {
        sections.push(Section {
            label: "Next",
            text: action.to_string(),
            style: TextStyle::Plain,
        });
    }
    sections
}

/// Columns a body row's text may use inside a box `width` cells wide.
fn text_width(width: u16) -> usize {
    let usable = usize::from(width.saturating_sub(INSET_LEFT + INSET_RIGHT));
    if width >= LABELLED_WIDTH {
        usable.saturating_sub(LABEL_COLUMN)
    } else {
        usable
    }
}

/// The body rows for a box `width` wide that may use at most `maximum_rows`
/// of them. Every present section gets a row first (trailing sections drop
/// when there are more sections than rows); spare rows then go to the
/// sections whose text needs them, in order.
pub fn body_rows(view: &View, width: u16, maximum_rows: usize) -> Vec<BodyRow> {
    let mut sections = sections(view);
    sections.truncate(maximum_rows);
    let text_width = text_width(width);
    if sections.is_empty() || text_width == 0 {
        return Vec::new();
    }
    let is_labelled = width >= LABELLED_WIDTH;
    let texts: Vec<String> = sections
        .iter()
        .map(|section| {
            if is_labelled {
                section.text.clone()
            } else {
                format!("{}: {}", section.label, section.text)
            }
        })
        .collect();
    let natural: Vec<usize> = texts
        .iter()
        .map(|text| wrap(text, text_width, MAXIMUM_SECTION_LINES).len().max(1))
        .collect();
    let mut allotted = vec![1usize; sections.len()];
    let mut spare = maximum_rows - sections.len();
    for _ in 1..MAXIMUM_SECTION_LINES {
        for index in 0..sections.len() {
            if spare > 0 && allotted[index] < natural[index] {
                allotted[index] += 1;
                spare -= 1;
            }
        }
    }
    let mut rows = Vec::new();
    for (index, section) in sections.iter().enumerate() {
        let lines = wrap(&texts[index], text_width, allotted[index]);
        for (line_index, text) in lines.into_iter().enumerate() {
            rows.push(BodyRow {
                label: (is_labelled && line_index == 0).then_some(section.label),
                text,
                is_quote: section.style == TextStyle::Quote,
            });
        }
    }
    rows
}

/// Rows the box takes in a pane `width` wide that can spare `spare_rows`
/// (what is left after the minimum terminal height and later reservations).
/// Zero means no box: the pane is too small to hold one without hurting the
/// terminal. A collapsed box, and a pane with a single spare row, get the
/// one-line form.
pub fn reserved_rows(view: &View, width: u16, spare_rows: u16) -> u16 {
    if width < MINIMUM_WIDTH || spare_rows == 0 {
        return 0;
    }
    if view.collapsed || spare_rows < 3 {
        return 1;
    }
    let body_budget = usize::from(MAXIMUM_ROWS - 1).min(usize::from(spare_rows) - 1);
    let body = body_rows(view, width, body_budget).len();
    // A box with nothing to say below its header is just the header line.
    u16::try_from(1 + body).unwrap_or(MAXIMUM_ROWS)
}

// ------------------------------------------------------------ hit-testing

/// The two buttons in the header row, right-aligned, one blank between and
/// one blank after. `area` is the whole box.
pub fn buttons(area: Rect) -> [(BriefingButton, Rect); 2] {
    let y = area.y;
    let right = area.right();
    [
        (
            BriefingButton::Collapse,
            Rect::new(right.saturating_sub(5), y, 1, 1),
        ),
        (
            BriefingButton::Dismiss,
            Rect::new(right.saturating_sub(3), y, 1, 1),
        ),
    ]
}

/// What a click at `position` does: the dismiss glyph dismisses; the rest of
/// the header row (including the fold glyph) toggles the fold. Body rows are
/// inert.
pub fn button_at(area: Rect, position: Position) -> Option<BriefingButton> {
    if area.width < MINIMUM_WIDTH || area.height == 0 || !area.contains(position) {
        return None;
    }
    let [(_, collapse), (_, dismiss)] = buttons(area);
    if dismiss.contains(position) {
        return Some(BriefingButton::Dismiss);
    }
    if position.y == area.y {
        // Keep the blank cells around the dismiss glyph from toggling by
        // accident: only the fold glyph and the text side count.
        return (position.x <= collapse.x).then_some(BriefingButton::Collapse);
    }
    None
}

// ---------------------------------------------------------------- drawing

fn source_label(item: &AttentionItem) -> &'static str {
    if item.briefing.from_llm {
        "AI"
    } else {
        "automatic"
    }
}

/// Plain text of the header row without buttons: kind badge, the title of the
/// box and, when there is room, where the text came from and its age.
fn header_text(view: &View, available: usize) -> (String, String) {
    let item = view.item;
    let badge = format!("{} {}", kind_glyph(item.kind), item.kind.label());
    let heading = format!("{badge} \u{b7} why you're here");
    let info = format!("{} \u{b7} {}", source_label(item), format_age(view.age_ms));
    let needed = heading.width() + 2 + info.width();
    if needed <= available {
        (heading, info)
    } else if badge.width() + 2 + info.width() <= available {
        (badge, info)
    } else {
        (clip(&heading, available), String::new())
    }
}

/// The one-line form: badge, then the most useful sentence the item has.
fn compact_text(view: &View) -> String {
    let item = view.item;
    non_blank(item.briefing.why_here.as_deref())
        .or_else(|| non_blank(item.briefing.status_summary.as_deref()))
        .unwrap_or(item.reason.as_str())
        .to_string()
}

/// Draws the box into `area`. A one-row `area` shows the compact form; a
/// taller one the header and `body_rows` for its height.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    view: &View,
    scheme: ColorScheme,
    hovered: Option<BriefingButton>,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let fill = theme::briefing_style(scheme);
    frame.render_widget(Paragraph::new("").style(fill), area);
    if area.width < MINIMUM_WIDTH {
        return;
    }
    let gutter = theme::briefing_gutter_style(scheme);
    let label_style = gutter.add_modifier(Modifier::BOLD);
    for row in 0..area.height {
        frame.render_widget(
            Paragraph::new(Span::styled(GUTTER, gutter)),
            Rect::new(area.x + 1, area.y + row, 1, 1),
        );
    }
    let text_x = area.x + INSET_LEFT;
    // The header text ends one blank before the fold glyph.
    let header_width = usize::from(area.width.saturating_sub(INSET_LEFT + 6));
    let header_area = Rect::new(text_x, area.y, area.width.saturating_sub(INSET_LEFT), 1);
    let header = if area.height == 1 {
        let badge = format!("{} {}", kind_glyph(view.item.kind), view.item.kind.label());
        let text = compact_text(view);
        let budget = header_width.saturating_sub(badge.width() + 3);
        Line::from(vec![
            Span::styled(badge, label_style),
            Span::styled(format!(" \u{b7} {}", clip(&text, budget)), fill),
        ])
    } else {
        let (heading, info) = header_text(view, header_width);
        let gap = header_width.saturating_sub(heading.width() + info.width());
        Line::from(vec![
            Span::styled(heading, label_style),
            Span::styled(" ".repeat(gap), fill),
            Span::styled(info, fill.add_modifier(Modifier::DIM)),
        ])
    };
    frame.render_widget(Paragraph::new(header).style(fill), header_area);
    for (button, rect) in buttons(area) {
        let mut style = fill;
        if hovered == Some(button) {
            style = style.add_modifier(Modifier::BOLD | Modifier::REVERSED);
        }
        frame.render_widget(
            Paragraph::new(Span::styled(button.glyph(view.collapsed), style)),
            rect,
        );
    }
    if area.height < 2 {
        return;
    }
    let rows = body_rows(view, area.width, usize::from(area.height - 1));
    let label_x = text_x;
    let body_x = if area.width >= LABELLED_WIDTH {
        text_x + LABEL_COLUMN as u16
    } else {
        text_x
    };
    let body_width = area.right().saturating_sub(body_x + INSET_RIGHT);
    for (index, row) in rows.into_iter().enumerate() {
        let y = area.y + 1 + index as u16;
        if let Some(label) = row.label {
            frame.render_widget(
                Paragraph::new(Span::styled(label, label_style)),
                Rect::new(label_x, y, LABEL_COLUMN as u16 - 1, 1),
            );
        }
        let style = if row.is_quote {
            Style::new().add_modifier(Modifier::ITALIC)
        } else {
            Style::new()
        };
        frame.render_widget(
            Paragraph::new(Span::styled(row.text, style)).style(fill),
            Rect::new(body_x, y, body_width, 1),
        );
    }
}

// -------------------------------------------------------------- lifecycle

/// One pane's briefing, taken when the user arrived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BriefingEntry {
    /// The item as last seen: its snapshot at arrival, then the live queue
    /// item for as long as the queue keeps one.
    pub item: AttentionItem,
    pub shown_ms: u64,
    /// The agent's turn when the user arrived. A finished agent's item
    /// vanishes once the user has read it; the briefing then stays until the
    /// agent starts something new.
    pub arrival_turn: Option<AgentTurn>,
    pub collapsed: bool,
}

impl BriefingEntry {
    /// Whether the entry still applies. `live` is the queue's present item
    /// for the pane; `turn` is the agent's present turn.
    fn is_current(
        &self,
        live: Option<&AttentionItem>,
        turn: Option<AgentTurn>,
        now_ms: u64,
        expiry_ms: u64,
    ) -> bool {
        if live.is_some() {
            return true;
        }
        turn == self.arrival_turn && now_ms.saturating_sub(self.shown_ms) <= expiry_ms
    }
}

/// Briefings of every pane, plus what the user dismissed. Lives for one
/// client run; nothing here is persisted.
#[derive(Debug, Default)]
pub struct BriefingState {
    entries: HashMap<NodeId, BriefingEntry>,
    /// `created_ms` of the item whose briefing the user dismissed, per pane.
    /// A newer item for the pane is a new subject and may show again.
    dismissed: HashMap<NodeId, u64>,
    /// The button the pointer rests on.
    pub hovered: Option<(NodeId, BriefingButton)>,
}

impl BriefingState {
    pub fn entry(&self, pane_id: NodeId) -> Option<&BriefingEntry> {
        self.entries.get(&pane_id)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Panes that currently have a briefing.
    pub fn pane_ids(&self) -> Vec<NodeId> {
        self.entries.keys().copied().collect()
    }

    /// Starts (or replaces) the briefing of `item`'s pane. Returns false,
    /// changing nothing, when this very item is already shown or was
    /// dismissed.
    pub fn activate(&mut self, item: &AttentionItem, now_ms: u64, turn: Option<AgentTurn>) -> bool {
        if self.dismissed.get(&item.pane_id) == Some(&item.created_ms) {
            return false;
        }
        if self
            .entries
            .get(&item.pane_id)
            .is_some_and(|entry| entry.item.created_ms == item.created_ms)
        {
            return false;
        }
        self.entries.insert(
            item.pane_id,
            BriefingEntry {
                item: item.clone(),
                shown_ms: now_ms,
                arrival_turn: turn,
                collapsed: false,
            },
        );
        true
    }

    /// Adopts the queue's present item for `pane_id`; true when the entry
    /// changed (so a redraw is due).
    pub fn follow(&mut self, pane_id: NodeId, live: &AttentionItem) -> bool {
        let Some(entry) = self.entries.get_mut(&pane_id) else {
            return false;
        };
        if entry.item == *live {
            return false;
        }
        entry.item = live.clone();
        true
    }

    /// Hides the pane's briefing until a newer item arrives.
    pub fn dismiss(&mut self, pane_id: NodeId) {
        if let Some(entry) = self.entries.remove(&pane_id) {
            self.dismissed.insert(pane_id, entry.item.created_ms);
        }
        if self.hovered.is_some_and(|(hovered, _)| hovered == pane_id) {
            self.hovered = None;
        }
    }

    pub fn toggle_collapsed(&mut self, pane_id: NodeId) {
        if let Some(entry) = self.entries.get_mut(&pane_id) {
            entry.collapsed = !entry.collapsed;
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.hovered = None;
    }

    /// Drops every entry that no longer applies. `lookup` gives, per pane,
    /// the queue's present item and the agent's present turn, or `None` when
    /// the pane is gone. Returns true when anything was dropped.
    pub fn retain_current<'a>(
        &mut self,
        now_ms: u64,
        expiry_ms: u64,
        mut lookup: impl FnMut(NodeId) -> Option<(Option<&'a AttentionItem>, Option<AgentTurn>)>,
    ) -> bool {
        let before = self.entries.len();
        self.entries.retain(|pane_id, entry| {
            lookup(*pane_id)
                .is_some_and(|(live, turn)| entry.is_current(live, turn, now_ms, expiry_ms))
        });
        // Dismissals only matter while their pane exists.
        self.dismissed
            .retain(|pane_id, _| lookup(*pane_id).is_some());
        if self
            .hovered
            .is_some_and(|(pane_id, _)| !self.entries.contains_key(&pane_id))
        {
            self.hovered = None;
        }
        self.entries.len() != before
    }
}

#[cfg(test)]
mod tests {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use super::*;
    use crate::attention::instant::fallback_briefing;
    use crate::attention::item::{AttentionKind, AttentionSource, Briefing};

    fn item(pane: u64, created_ms: u64) -> AttentionItem {
        AttentionItem {
            pane_id: NodeId(pane),
            project_id: None,
            title: "build agent".into(),
            kind: AttentionKind::Question,
            reason: "Waiting for your answer or approval".into(),
            question: Some("Delete the generated files?".into()),
            waiting_on: None,
            briefing: Briefing {
                status_summary: Some("Finished the refactor and ran the tests.".into()),
                why_here: Some("It needs a yes before it removes files.".into()),
                suggested_action: Some("Approve if the list looks right.".into()),
                from_llm: true,
            },
            priority: 50,
            source: AttentionSource::Llm,
            fingerprint: 1,
            generation: 1,
            created_ms,
        }
    }

    fn view(item: &AttentionItem, collapsed: bool) -> View<'_> {
        View {
            item,
            waiting_on: None,
            age_ms: 125_000,
            collapsed,
        }
    }

    fn draw(width: u16, height: u16, view: &View, hovered: Option<BriefingButton>) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), view, ColorScheme::Dark, hovered))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn all_sections_show_with_their_labels_when_there_is_room() {
        let item = item(1, 10);
        let rows = body_rows(&view(&item, false), 80, 5);
        let labels: Vec<_> = rows.iter().filter_map(|row| row.label).collect();
        assert_eq!(labels, ["Why", "Status", "Asked", "Next"]);
        assert!(rows.iter().any(|row| row.text.starts_with('\u{201c}')));
    }

    #[test]
    fn absent_data_is_left_out_not_invented() {
        let mut bare = item(1, 10);
        bare.question = None;
        bare.briefing = Briefing::default();
        let rows = body_rows(&view(&bare, false), 80, 5);
        let labels: Vec<_> = rows.iter().filter_map(|row| row.label).collect();
        // Only the item's own reason remains.
        assert_eq!(labels, ["Why"]);
        assert_eq!(rows[0].text, "Waiting for your answer or approval");
    }

    #[test]
    fn waiting_target_has_its_own_row() {
        let mut waiting = item(1, 10);
        waiting.kind = AttentionKind::WaitingOn;
        let with_target = View {
            waiting_on: Some("test runner"),
            ..view(&waiting, false)
        };
        let rows = body_rows(&with_target, 80, 5);
        assert!(rows
            .iter()
            .any(|row| row.label == Some("Waiting") && row.text == "test runner"));
    }

    #[test]
    fn long_text_wraps_and_never_exceeds_the_row_budget() {
        let mut long = item(1, 10);
        long.briefing.status_summary = Some("word ".repeat(120));
        for width in [24, 39, 40, 60, 120] {
            for budget in 1..=5 {
                let rows = body_rows(&view(&long, false), width, budget);
                assert!(rows.len() <= budget, "width {width} budget {budget}");
                for row in &rows {
                    assert!(
                        row.text.width() <= text_width(width),
                        "row too wide at {width}: {:?}",
                        row.text
                    );
                }
            }
        }
    }

    #[test]
    fn narrow_boxes_put_the_label_inside_the_text() {
        let item = item(1, 10);
        let rows = body_rows(&view(&item, false), 30, 5);
        assert!(rows.iter().all(|row| row.label.is_none()));
        assert!(rows[0].text.starts_with("Why: "));
    }

    #[test]
    fn reserved_rows_follow_the_content_and_the_available_space() {
        let item = item(1, 10);
        let full = reserved_rows(&view(&item, false), 80, 20);
        assert!((3..=MAXIMUM_ROWS).contains(&full), "{full}");
        assert_eq!(reserved_rows(&view(&item, true), 80, 20), 1);
        assert_eq!(reserved_rows(&view(&item, false), 80, 0), 0);
        assert_eq!(reserved_rows(&view(&item, false), 80, 1), 1);
        assert_eq!(reserved_rows(&view(&item, false), 80, 2), 1);
        assert_eq!(reserved_rows(&view(&item, false), MINIMUM_WIDTH - 1, 20), 0);
        assert!(reserved_rows(&view(&item, false), 80, 4) <= 4);
        assert!(reserved_rows(&view(&item, false), 80, 100) <= MAXIMUM_ROWS);
    }

    #[test]
    fn the_rows_drawn_fit_the_rows_reserved() {
        let item = item(1, 10);
        for width in [24u16, 40, 80] {
            for spare in 0..12u16 {
                let rows = reserved_rows(&view(&item, false), width, spare);
                assert!(rows <= spare);
                if rows > 0 {
                    let drawn = draw(width, rows, &view(&item, false), None);
                    assert_eq!(drawn.len(), usize::from(rows));
                }
            }
        }
    }

    #[test]
    fn render_shows_header_labels_and_both_buttons() {
        let item = item(1, 10);
        let rows = reserved_rows(&view(&item, false), 80, 20);
        let drawn = draw(80, rows, &view(&item, false), None);
        let header = &drawn[0];
        assert!(header.contains("Question"), "{header}");
        assert!(header.contains("why you're here"), "{header}");
        assert!(header.contains("AI"), "{header}");
        assert!(header.contains("2m"), "{header}");
        assert!(header.contains(COLLAPSE_GLYPH_OPEN), "{header}");
        assert!(header.contains(DISMISS_GLYPH), "{header}");
        let all = drawn.join("\n");
        assert!(all.contains("It needs a yes before it removes files."));
        assert!(all.contains("Finished the refactor"));
        assert!(all.contains("Delete the generated files?"));
        assert!(all.contains("Approve if the list looks right."));
    }

    #[test]
    fn automatic_text_is_labelled_automatic() {
        let mut automatic = item(1, 10);
        automatic.source = AttentionSource::Instant;
        automatic.briefing = fallback_briefing("Claude Code", AttentionKind::Question, true);
        let drawn = draw(80, 4, &view(&automatic, false), None);
        assert!(drawn[0].contains("automatic"), "{}", drawn[0]);
    }

    #[test]
    fn collapsed_box_is_one_line_with_the_unfold_glyph() {
        let item = item(1, 10);
        let drawn = draw(60, 1, &view(&item, true), None);
        assert!(drawn[0].contains("Question"));
        assert!(drawn[0].contains("It needs a yes"), "{}", drawn[0]);
        assert!(drawn[0].contains(COLLAPSE_GLYPH_CLOSED));
    }

    #[test]
    fn tiny_sizes_never_panic() {
        let item = item(1, 10);
        for width in 0..30u16 {
            for height in 0..8u16 {
                for collapsed in [false, true] {
                    let mut terminal =
                        Terminal::new(TestBackend::new(width.max(1), height.max(1))).unwrap();
                    terminal
                        .draw(|frame| {
                            let area = Rect::new(0, 0, width, height);
                            render(
                                frame,
                                area,
                                &view(&item, collapsed),
                                ColorScheme::Dark,
                                None,
                            );
                        })
                        .unwrap();
                }
            }
        }
    }

    #[test]
    fn hover_marks_the_button_under_the_pointer() {
        let item = item(1, 10);
        let area = Rect::new(0, 0, 60, 3);
        let mut terminal = Terminal::new(TestBackend::new(60, 3)).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    area,
                    &view(&item, false),
                    ColorScheme::Dark,
                    Some(BriefingButton::Dismiss),
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let [(_, collapse), (_, dismiss)] = buttons(area);
        let dismiss_cell = &buffer[(dismiss.x, dismiss.y)];
        let collapse_cell = &buffer[(collapse.x, collapse.y)];
        assert!(dismiss_cell.modifier.contains(Modifier::REVERSED));
        assert!(!collapse_cell.modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn dark_scheme_fill_stays_dark() {
        let item = item(1, 10);
        let mut terminal = Terminal::new(TestBackend::new(60, 3)).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    frame.area(),
                    &view(&item, false),
                    ColorScheme::Dark,
                    None,
                );
            })
            .unwrap();
        let cell = &terminal.backend().buffer()[(30, 2)];
        let ratatui::style::Color::Rgb(red, green, blue) = cell.bg else {
            panic!(
                "briefing fill must be an explicit colour, got {:?}",
                cell.bg
            );
        };
        assert!(u16::from(red) + u16::from(green) + u16::from(blue) < 3 * 110);
    }

    #[test]
    fn button_hit_testing_matches_the_drawn_glyphs() {
        let area = Rect::new(4, 2, 60, 4);
        let [(_, collapse), (_, dismiss)] = buttons(area);
        assert_eq!(
            button_at(area, Position::new(dismiss.x, dismiss.y)),
            Some(BriefingButton::Dismiss)
        );
        assert_eq!(
            button_at(area, Position::new(collapse.x, collapse.y)),
            Some(BriefingButton::Collapse)
        );
        // Header text toggles the fold; the blank between the buttons and
        // the margin after the dismiss glyph do nothing.
        assert_eq!(
            button_at(area, Position::new(area.x + 5, area.y)),
            Some(BriefingButton::Collapse)
        );
        assert_eq!(
            button_at(area, Position::new(dismiss.x - 1, dismiss.y)),
            None
        );
        assert_eq!(
            button_at(area, Position::new(dismiss.x + 1, dismiss.y)),
            None
        );
        // Body rows and anything outside are inert.
        assert_eq!(button_at(area, Position::new(area.x + 5, area.y + 1)), None);
        assert_eq!(button_at(area, Position::new(area.x + 5, area.y - 1)), None);
        assert_eq!(
            button_at(Rect::new(0, 0, MINIMUM_WIDTH - 1, 3), Position::new(0, 0)),
            None
        );
    }

    #[test]
    fn activation_is_once_per_item_and_dismissal_sticks_until_a_newer_item() {
        let mut state = BriefingState::default();
        let first = item(1, 10);
        assert!(state.activate(&first, 1_000, Some(AgentTurn::WaitingApproval)));
        assert!(!state.activate(&first, 2_000, Some(AgentTurn::WaitingApproval)));
        state.toggle_collapsed(NodeId(1));
        assert!(state.entry(NodeId(1)).unwrap().collapsed);
        // The same item again keeps the user's fold.
        assert!(!state.activate(&first, 3_000, None));
        assert!(state.entry(NodeId(1)).unwrap().collapsed);

        state.dismiss(NodeId(1));
        assert!(state.entry(NodeId(1)).is_none());
        assert!(
            !state.activate(&first, 4_000, None),
            "dismissed item stays hidden"
        );
        let newer = item(1, 99);
        assert!(
            state.activate(&newer, 5_000, None),
            "a newer item shows again"
        );
        assert!(!state.entry(NodeId(1)).unwrap().collapsed);
    }

    #[test]
    fn follow_adopts_the_live_item_so_a_late_reply_updates_the_box() {
        let mut state = BriefingState::default();
        let mut shown = item(1, 10);
        shown.briefing = fallback_briefing("Codex", AttentionKind::Question, true);
        state.activate(&shown, 0, None);
        let upgraded = item(1, 10);
        assert!(state.follow(NodeId(1), &upgraded));
        assert!(state.entry(NodeId(1)).unwrap().item.briefing.from_llm);
        assert!(
            !state.follow(NodeId(1), &upgraded),
            "unchanged item: no redraw"
        );
        assert!(
            !state.follow(NodeId(2), &upgraded),
            "unknown pane is ignored"
        );
    }

    #[test]
    fn entries_end_when_the_item_and_the_arrival_state_are_both_gone() {
        let mut state = BriefingState::default();
        let changed = item(1, 10);
        state.activate(&changed, 1_000, Some(AgentTurn::Idle));
        let expiry = 60_000;

        // The queue item vanished (the user read the finished agent) but the
        // agent is still idle: the briefing stays.
        assert!(!state.retain_current(2_000, expiry, |_| Some((None, Some(AgentTurn::Idle)))));
        assert!(state.entry(NodeId(1)).is_some());

        // Past the expiry the snapshot is dropped.
        assert!(state.retain_current(1_000 + expiry + 1, expiry, |_| Some((
            None,
            Some(AgentTurn::Idle)
        ))));
        assert!(state.entry(NodeId(1)).is_none());

        // A live queue item keeps it whatever the age or turn.
        state.activate(&changed, 1_000, Some(AgentTurn::Idle));
        let live = item(1, 10);
        assert!(!state.retain_current(10_000_000, expiry, |_| Some((
            Some(&live),
            Some(AgentTurn::Working)
        ))));

        // The agent moved on and nothing in the queue refers to it.
        assert!(state.retain_current(2_000, expiry, |_| Some((None, Some(AgentTurn::Working)))));
        assert!(state.entry(NodeId(1)).is_none());
    }

    #[test]
    fn entries_of_closed_panes_are_dropped() {
        let mut state = BriefingState::default();
        state.activate(&item(1, 10), 0, None);
        state.dismiss(NodeId(1));
        state.activate(&item(2, 10), 0, None);
        assert!(state.retain_current(1, 60_000, |_| None));
        assert!(state.is_empty());
        // The dismissal went with the pane, so a reused id starts clean.
        assert!(state.activate(&item(1, 10), 5, None));
    }
}
