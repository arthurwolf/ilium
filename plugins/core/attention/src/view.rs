//! What the two views draw: plain lines of text from the queue, so the layout is testable without a
//! host. The panel is a list; the overview is a grid of cards (one, two or three columns by width).

use crate::item::{AttentionItem, AttentionKind};

/// Columns of the overview grid at the given width: 1 below 100 columns, 2 from 100, 3 from 160.
pub const fn overview_columns(width: usize) -> usize {
    if width >= 160 {
        3
    } else if width >= 100 {
        2
    } else {
        1
    }
}

/// Lines one card of the overview takes, separator included.
pub const CARD_LINES: usize = 7;
/// Gap between two cards of a row.
const COLUMN_GAP: usize = 3;
/// The marker of the selected row.
const SELECTED: &str = "> ";
const UNSELECTED: &str = "  ";

/// Cuts `text` to `width` characters; a cut ends in an ellipsis.
pub fn clip(text: &str, width: usize) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if flat.chars().count() <= width {
        return flat;
    }
    if width == 0 {
        return String::new();
    }
    let mut cut: String = flat.chars().take(width - 1).collect();
    cut.push('…');
    cut
}

/// Pads or cuts `text` to exactly `width` characters.
pub fn fit(text: &str, width: usize) -> String {
    let clipped = clip(text, width);
    let missing = width.saturating_sub(clipped.chars().count());
    format!("{clipped}{}", " ".repeat(missing))
}

/// "now", "3m" or "2h": how long ago an item first appeared.
pub fn age_label(now_ms: u64, created_ms: u64) -> String {
    let seconds = now_ms.saturating_sub(created_ms) / 1_000;
    if seconds < 60 {
        "now".to_string()
    } else if seconds < 3_600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h", seconds / 3_600)
    } else {
        format!("{}d", seconds / 86_400)
    }
}

fn kind_glyph(kind: AttentionKind) -> &'static str {
    match kind {
        AttentionKind::Question => "?",
        AttentionKind::Stuck => "!",
        AttentionKind::WaitingOn => "~",
        AttentionKind::Changed => "*",
    }
}

fn first_line(item: &AttentionItem, now_ms: u64) -> String {
    format!(
        "{} {}  {}  ({})",
        kind_glyph(item.kind),
        item.kind.label(),
        item.title,
        age_label(now_ms, item.created_ms)
    )
}

/// The one-line status the chrome shows: the count and the most urgent kind, `None` when nothing
/// needs the user.
pub fn status_text(items: &[&AttentionItem]) -> Option<String> {
    let first = items.first()?;
    Some(format!(
        "Needs you: {} ({})",
        items.len(),
        first.kind.label()
    ))
}

/// The docked panel: a header, two lines per item and a key legend.
pub fn panel_lines(
    items: &[&AttentionItem],
    selected: usize,
    width: usize,
    rows: usize,
    now_ms: u64,
) -> Vec<String> {
    let mut lines = Vec::new();
    if items.is_empty() {
        lines.push("All clear".to_string());
        lines.push(String::new());
        lines.push("No agent needs you right now.".to_string());
        lines.push(String::new());
        lines.push("Esc closes.".to_string());
        return lines.into_iter().map(|line| clip(&line, width)).collect();
    }
    lines.push(format!("Needs you ({})", items.len()));
    lines.push(String::new());
    let legend = "Up/Down select, Enter go, S snooze, R refresh, Esc close";
    let body_rows = rows.saturating_sub(lines.len() + 2).max(2);
    let visible = (body_rows / 2).max(1);
    let selected = selected.min(items.len() - 1);
    let start = (selected + 1).saturating_sub(visible);
    for (index, item) in items.iter().enumerate().skip(start).take(visible) {
        let marker = if index == selected {
            SELECTED
        } else {
            UNSELECTED
        };
        lines.push(format!("{marker}{}", first_line(item, now_ms)));
        lines.push(format!("{UNSELECTED}  {}", item.reason));
    }
    lines.push(String::new());
    lines.push(legend.to_string());
    lines.into_iter().map(|line| clip(&line, width)).collect()
}

/// One card of the overview, `CARD_LINES` lines, each at most `width` characters.
pub fn card_lines(item: &AttentionItem, selected: bool, width: usize, now_ms: u64) -> Vec<String> {
    let marker = if selected { SELECTED } else { UNSELECTED };
    let indent = UNSELECTED;
    let mut lines = vec![
        format!("{marker}{}", first_line(item, now_ms)),
        format!("{indent}{}", item.reason),
        item.question
            .as_ref()
            .map(|question| format!("{indent}Q: {question}"))
            .unwrap_or_default(),
        item.briefing
            .status_summary
            .as_ref()
            .map(|text| format!("{indent}{text}"))
            .unwrap_or_default(),
        item.briefing
            .why_here
            .as_ref()
            .map(|text| format!("{indent}Why: {text}"))
            .unwrap_or_default(),
        item.briefing
            .suggested_action
            .as_ref()
            .map(|text| format!("{indent}Next: {text}"))
            .unwrap_or_default(),
        String::new(),
    ];
    lines.truncate(CARD_LINES);
    lines.into_iter().map(|line| fit(&line, width)).collect()
}

/// The overview: cards in a grid, scrolled so the selected card is on screen, with a legend line.
pub fn overview_lines(
    items: &[&AttentionItem],
    selected: usize,
    width: usize,
    rows: usize,
    now_ms: u64,
) -> Vec<String> {
    if items.is_empty() {
        return vec![
            "Needs you".to_string(),
            String::new(),
            "All clear. No agent needs you right now.".to_string(),
            String::new(),
            "Esc closes.".to_string(),
        ];
    }
    let columns = overview_columns(width);
    let card_width = (width.saturating_sub(COLUMN_GAP * (columns - 1)) / columns).max(10);
    let selected = selected.min(items.len() - 1);
    let grid_rows = items.len().div_ceil(columns);
    let visible_rows = (rows.saturating_sub(3) / CARD_LINES).max(1);
    let selected_row = selected / columns;
    let first_row = (selected_row + 1).saturating_sub(visible_rows);
    let mut lines = vec![format!("Needs you ({})", items.len()), String::new()];
    for row in first_row..grid_rows.min(first_row + visible_rows) {
        let cards: Vec<Vec<String>> = (0..columns)
            .map(|column| {
                let index = row * columns + column;
                items
                    .get(index)
                    .map(|item| card_lines(item, index == selected, card_width, now_ms))
                    .unwrap_or_else(|| vec![" ".repeat(card_width); CARD_LINES])
            })
            .collect();
        for line in 0..CARD_LINES {
            let joined = cards
                .iter()
                .map(|card| card[line].clone())
                .collect::<Vec<_>>()
                .join(&" ".repeat(COLUMN_GAP));
            lines.push(joined.trim_end().to_string());
        }
    }
    lines.push("Arrows select, Enter go, S snooze, R refresh, Esc close".to_string());
    lines.into_iter().map(|line| clip(&line, width)).collect()
}

/// The briefing shown when the user is taken to an agent: a title and one body of at most 200
/// characters (the toast limit). `None` when the item has no briefing at all.
pub fn briefing_toast(item: &AttentionItem) -> Option<(String, String)> {
    if item.briefing.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    if let Some(text) = &item.briefing.status_summary {
        parts.push(text.clone());
    }
    if let Some(text) = &item.briefing.why_here {
        parts.push(text.clone());
    }
    if let Some(text) = &item.briefing.suggested_action {
        parts.push(format!("Next: {text}"));
    }
    let title = clip(&format!("{}: {}", item.kind.label(), item.title), 60);
    Some((title, clip(&parts.join(" "), 200)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{AttentionSource, Briefing};
    use ilium_plugin_sdk::types::NodeId;

    fn item(id: u64, kind: AttentionKind, title: &str) -> AttentionItem {
        AttentionItem {
            pane_id: NodeId(id),
            project_id: None,
            title: title.to_string(),
            kind,
            reason: format!("reason {id}"),
            question: None,
            waiting_on: None,
            briefing: Briefing::default(),
            priority: 50,
            source: AttentionSource::Instant,
            fingerprint: id,
            generation: 1,
            created_ms: 0,
        }
    }

    /// REQ: attention-B08
    #[test]
    fn the_overview_grid_has_one_two_or_three_columns_by_width() {
        assert_eq!(overview_columns(99), 1);
        assert_eq!(overview_columns(100), 2);
        assert_eq!(overview_columns(159), 2);
        assert_eq!(overview_columns(160), 3);
    }

    #[test]
    fn clip_and_fit_respect_the_width_and_mark_a_cut() {
        assert_eq!(clip("short", 10), "short");
        assert_eq!(clip("0123456789", 5), "0123…");
        assert_eq!(clip("a\nb", 10), "a b");
        assert_eq!(fit("ab", 4), "ab  ");
        assert_eq!(fit("abcdef", 4).chars().count(), 4);
    }

    #[test]
    fn ages_read_as_now_minutes_hours_days() {
        assert_eq!(age_label(30_000, 0), "now");
        assert_eq!(age_label(180_000, 0), "3m");
        assert_eq!(age_label(7_200_000, 0), "2h");
        assert_eq!(age_label(172_800_000, 0), "2d");
    }

    /// REQ: attention-B12
    #[test]
    fn an_empty_queue_reads_all_clear_in_both_views() {
        assert_eq!(panel_lines(&[], 0, 40, 20, 0)[0], "All clear");
        assert!(overview_lines(&[], 0, 120, 30, 0)[2].starts_with("All clear"));
        assert_eq!(status_text(&[]), None);
    }

    #[test]
    fn the_status_names_the_count_and_the_most_urgent_kind() {
        let first = item(1, AttentionKind::Question, "a");
        let second = item(2, AttentionKind::Changed, "b");
        assert_eq!(
            status_text(&[&first, &second]),
            Some("Needs you: 2 (Question)".to_string())
        );
    }

    #[test]
    fn the_panel_marks_the_selected_item_and_scrolls_to_keep_it_visible() {
        let owned: Vec<AttentionItem> = (1..=10)
            .map(|id| item(id, AttentionKind::Changed, &format!("agent-{id}")))
            .collect();
        let items: Vec<&AttentionItem> = owned.iter().collect();
        let lines = panel_lines(&items, 9, 60, 12, 0);
        assert!(lines
            .iter()
            .any(|line| line.starts_with("> ") && line.contains("agent-10")));
        assert!(!lines.iter().any(|line| line.contains("agent-1 ")));
        assert!(
            lines.len() <= 12,
            "the panel never overruns its rows: {}",
            lines.len()
        );
    }

    /// REQ: attention-B08
    #[test]
    fn overview_cards_sit_side_by_side_at_a_wide_width() {
        let owned: Vec<AttentionItem> = (1..=3)
            .map(|id| item(id, AttentionKind::Question, &format!("agent-{id}")))
            .collect();
        let items: Vec<&AttentionItem> = owned.iter().collect();
        let wide = overview_lines(&items, 0, 170, 40, 0);
        let header = wide
            .iter()
            .find(|line| line.contains("agent-1"))
            .expect("a card header line");
        assert!(header.contains("agent-2") && header.contains("agent-3"));
        let narrow = overview_lines(&items, 0, 60, 40, 0);
        assert!(narrow.iter().filter(|line| line.contains("agent-")).count() >= 3);
        assert!(narrow.iter().all(|line| line.chars().count() <= 60));
    }

    #[test]
    fn a_card_shows_the_question_and_the_briefing_fields() {
        let mut full = item(1, AttentionKind::Question, "api");
        full.question = Some("Overwrite?".to_string());
        full.briefing = Briefing {
            status_summary: Some("It edits a file.".to_string()),
            why_here: Some("It waits for you.".to_string()),
            suggested_action: Some("Answer.".to_string()),
            from_llm: true,
        };
        let card = card_lines(&full, true, 60, 0);
        assert_eq!(card.len(), CARD_LINES);
        assert!(card[0].starts_with("> "));
        assert!(card[2].contains("Q: Overwrite?"));
        assert!(card[4].contains("Why: It waits for you."));
        assert!(card[5].contains("Next: Answer."));
    }

    /// REQ: attention-B10
    #[test]
    fn the_briefing_toast_fits_its_limits_and_skips_an_empty_briefing() {
        let mut bare = item(1, AttentionKind::Changed, "api");
        assert!(briefing_toast(&bare).is_none());
        bare.briefing = Briefing {
            status_summary: Some("x".repeat(300)),
            why_here: Some("y".repeat(300)),
            suggested_action: Some("z".repeat(300)),
            from_llm: true,
        };
        let (title, body) = briefing_toast(&bare).expect("a briefing");
        assert!(title.chars().count() <= 60);
        assert!(body.chars().count() <= 200);
        assert!(title.starts_with("Changed: api"));
    }
}
