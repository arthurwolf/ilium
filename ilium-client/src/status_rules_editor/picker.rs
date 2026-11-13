//! A grouped list with type-to-search, drawn over one editor column.
//!
//! Two pickers use it: the condition picker (the whole condition catalogue
//! under category headings, each entry with its plain-language explanation)
//! and the "New rule" template picker. Rendering and pointer hit testing
//! share [`list_rows`], so a click always lands on the entry drawn under it.
//!
//! Keys while open: printable characters filter, Backspace removes one,
//! Up/Down move (Page Up/Down by a screen), Enter chooses, Esc cancels.

use ilium_core::status_rule_expression::{Condition, ConditionCategory};
use ratatui::{
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph},
    Frame,
};

use super::templates::templates;

/// What choosing an entry does.
#[derive(Clone, Debug, PartialEq)]
pub enum PickerChoice {
    /// Replace the selected condition with this catalogue entry.
    Condition(Condition),
    /// Add a rule built from the template at this index of `templates()`.
    Template(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PickerItem {
    pub group: &'static str,
    pub color: Color,
    pub label: String,
    /// One sentence explaining the entry, shown while it is highlighted.
    pub detail: String,
    pub choice: PickerChoice,
}

/// One drawn row of the list: a group heading or an entry (item index).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PickerLine {
    Heading(usize),
    Item(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Picker {
    pub title: &'static str,
    pub items: Vec<PickerItem>,
    pub query: String,
    /// Index into `items` of the highlighted entry; always a visible one
    /// while any entry matches the query.
    pub highlighted: Option<usize>,
}

/// One colour per condition category, shared by the picker headings, the
/// condition chips in the rules list and the condition column.
pub const fn category_color(category: ConditionCategory) -> Color {
    match category {
        ConditionCategory::Pane => Color::LightBlue,
        ConditionCategory::Agent => Color::Cyan,
        ConditionCategory::Goal => Color::LightMagenta,
        ConditionCategory::Monitor => Color::Yellow,
        ConditionCategory::Progress => Color::LightGreen,
        ConditionCategory::Shell => Color::Blue,
        ConditionCategory::Screen => Color::Magenta,
        ConditionCategory::Client => Color::Gray,
    }
}

/// Two catalogue entries are the same choice when they are the same
/// condition, ignoring parameters (a pattern, a percentage, a count), so
/// the picker can highlight the current condition whatever its value.
pub fn same_entry(left: &Condition, right: &Condition) -> bool {
    match (left, right) {
        (Condition::ScreenMatches(_), Condition::ScreenMatches(_))
        | (Condition::MonitorCountAtLeast(_), Condition::MonitorCountAtLeast(_))
        | (Condition::ProgressAtLeast(_), Condition::ProgressAtLeast(_))
        | (Condition::ProgressBelow(_), Condition::ProgressBelow(_))
        | (Condition::ProgressBetween { .. }, Condition::ProgressBetween { .. })
        | (Condition::EtaBelowMinutes(_), Condition::EtaBelowMinutes(_))
        | (Condition::SessionCostAbove(_), Condition::SessionCostAbove(_))
        | (Condition::ContextAbovePercent(_), Condition::ContextAbovePercent(_)) => true,
        _ => left == right,
    }
}

impl Picker {
    /// Every catalogue condition under its category heading (headings in
    /// `ConditionCategory::ALL` order), highlighting `current`.
    pub fn conditions(current: Option<&Condition>) -> Self {
        let catalog = super::condition_catalog();
        let mut items = Vec::with_capacity(catalog.len());
        for category in ConditionCategory::ALL {
            for condition in catalog.iter().filter(|entry| entry.category() == category) {
                items.push(PickerItem {
                    group: category.label(),
                    color: category_color(category),
                    label: condition.label(),
                    detail: condition.explain().to_owned(),
                    choice: PickerChoice::Condition(condition.clone()),
                });
            }
        }
        let highlighted = current
            .and_then(|current| {
                items.iter().position(|item| {
                    matches!(&item.choice, PickerChoice::Condition(entry) if same_entry(entry, current))
                })
            })
            .or_else(|| (!items.is_empty()).then_some(0));
        Self {
            title: " Choose a condition ",
            items,
            query: String::new(),
            highlighted,
        }
    }

    /// The "New rule" templates, first one highlighted.
    pub fn templates() -> Self {
        let items: Vec<PickerItem> = templates()
            .iter()
            .enumerate()
            .map(|(index, template)| PickerItem {
                group: "Start from",
                color: Color::White,
                label: template.title.to_owned(),
                detail: template.description.to_owned(),
                choice: PickerChoice::Template(index),
            })
            .collect();
        Self {
            title: " New rule ",
            highlighted: (!items.is_empty()).then_some(0),
            items,
            query: String::new(),
        }
    }

    /// Whether an entry matches every word of the query (case-insensitive,
    /// against its label, group and explanation).
    fn matches(&self, item: &PickerItem) -> bool {
        let haystack = format!("{} {} {}", item.label, item.group, item.detail).to_lowercase();
        self.query
            .to_lowercase()
            .split_whitespace()
            .all(|word| haystack.contains(word))
    }

    /// Indices of the entries matching the query, in list order.
    pub fn visible_items(&self) -> Vec<usize> {
        (0..self.items.len())
            .filter(|index| self.matches(&self.items[*index]))
            .collect()
    }

    /// Headings and entries as drawn, a heading before each group change.
    pub fn lines(&self) -> Vec<PickerLine> {
        let mut lines = Vec::new();
        let mut group = None;
        for index in self.visible_items() {
            if group != Some(self.items[index].group) {
                group = Some(self.items[index].group);
                lines.push(PickerLine::Heading(index));
            }
            lines.push(PickerLine::Item(index));
        }
        lines
    }

    /// Keep the highlight on a visible entry after the query changed.
    /// Typing jumps to the first entry whose name contains the whole query
    /// (a match only in the explanation ranks lower); otherwise the
    /// highlight stays put while it still matches.
    fn refit_highlight(&mut self) {
        let visible = self.visible_items();
        let phrase = self.query.trim().to_lowercase();
        if !phrase.is_empty() {
            if let Some(best) = visible
                .iter()
                .copied()
                .find(|index| self.items[*index].label.to_lowercase().contains(&phrase))
            {
                self.highlighted = Some(best);
                return;
            }
        }
        if !self
            .highlighted
            .is_some_and(|highlighted| visible.contains(&highlighted))
        {
            self.highlighted = visible.first().copied();
        }
    }

    pub fn push_char(&mut self, character: char) {
        if self.query.chars().count() < 40 && !character.is_control() {
            self.query.push(character);
            self.refit_highlight();
        }
    }

    pub fn pop_char(&mut self) {
        self.query.pop();
        self.refit_highlight();
    }

    /// Move the highlight by `delta` visible entries, stopping at the ends.
    pub fn move_by(&mut self, delta: i32) {
        let visible = self.visible_items();
        if visible.is_empty() {
            self.highlighted = None;
            return;
        }
        let position = self
            .highlighted
            .and_then(|highlighted| visible.iter().position(|index| *index == highlighted))
            .unwrap_or(0);
        let last = visible.len() as i64 - 1;
        let next = (position as i64 + i64::from(delta)).clamp(0, last) as usize;
        self.highlighted = Some(visible[next]);
    }

    /// The highlighted entry's choice, when one matches the query.
    pub fn chosen(&self) -> Option<&PickerChoice> {
        let highlighted = self.highlighted?;
        self.visible_items()
            .contains(&highlighted)
            .then(|| &self.items[highlighted].choice)
    }
}

/// Rows of the overlay: borders, the search row, the list, then the
/// highlighted entry's explanation and the key hint.
const CHROME_ROWS: u16 = 2 + 1 + 2;

/// The rectangle the list rows occupy inside the overlay, `None` when the
/// column is too small to show any entry.
fn list_area(area: Rect) -> Option<Rect> {
    if area.width < 12 || area.height < CHROME_ROWS + 1 {
        return None;
    }
    Some(Rect::new(
        area.x.saturating_add(2),
        area.y.saturating_add(2),
        area.width.saturating_sub(4),
        area.height - CHROME_ROWS,
    ))
}

/// The drawn list rows with their screen row, scrolled so the highlighted
/// entry stays visible. Shared by rendering and [`item_at`].
pub fn list_rows(area: Rect, picker: &Picker) -> Vec<(PickerLine, u16)> {
    let Some(list) = list_area(area) else {
        return Vec::new();
    };
    let lines = picker.lines();
    let height = usize::from(list.height);
    let highlighted_line = picker
        .highlighted
        .and_then(|highlighted| {
            lines
                .iter()
                .position(|line| *line == PickerLine::Item(highlighted))
        })
        .unwrap_or(0);
    let first = (highlighted_line + 1).saturating_sub(height);
    lines
        .into_iter()
        .skip(first)
        .take(height)
        .enumerate()
        .map(|(offset, line)| (line, list.y + offset as u16))
        .collect()
}

/// The entry under a pointer position, if any.
pub fn item_at(area: Rect, picker: &Picker, position: Position) -> Option<usize> {
    let list = list_area(area)?;
    if position.x < list.x || position.x >= list.right() {
        return None;
    }
    list_rows(area, picker)
        .into_iter()
        .find_map(|(line, row)| match line {
            PickerLine::Item(index) if row == position.y => Some(index),
            _ => None,
        })
}

fn truncate(text: &str, width: usize) -> String {
    crate::settings_ui::truncate_icon_text(text, width)
}

pub fn render(frame: &mut Frame<'_>, area: Rect, picker: &Picker) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    frame.render_widget(Clear, area);
    let inner_width = usize::from(area.width.saturating_sub(4));
    let dim = Style::new().add_modifier(Modifier::DIM);
    let mut lines = vec![Line::from(if picker.query.is_empty() {
        vec![
            Span::styled("Search ", Style::new().add_modifier(Modifier::BOLD)),
            Span::styled("type to filter", dim),
        ]
    } else {
        vec![
            Span::styled("Search ", Style::new().add_modifier(Modifier::BOLD)),
            Span::raw(truncate(&picker.query, inner_width.saturating_sub(8))),
            Span::raw("▏"),
        ]
    })];
    let rows = list_rows(area, picker);
    for (line, _) in &rows {
        lines.push(match *line {
            PickerLine::Heading(index) => {
                let item = &picker.items[index];
                Line::from(Span::styled(
                    truncate(item.group, inner_width),
                    Style::new().fg(item.color).add_modifier(Modifier::BOLD),
                ))
            }
            PickerLine::Item(index) => {
                let item = &picker.items[index];
                let style = if picker.highlighted == Some(index) {
                    crate::theme::selected_style().add_modifier(Modifier::BOLD)
                } else {
                    Style::new()
                };
                Line::from(vec![
                    Span::styled("  ", style),
                    Span::styled(truncate(&item.label, inner_width.saturating_sub(2)), style),
                ])
            }
        });
    }
    if rows.is_empty() && list_area(area).is_some() {
        lines.push(Line::from(Span::styled("Nothing matches", dim)));
    }
    let list_height = list_area(area).map_or(0, |list| usize::from(list.height));
    while lines.len() < 1 + list_height {
        lines.push(Line::from(""));
    }
    let detail = picker
        .highlighted
        .filter(|highlighted| picker.visible_items().contains(highlighted))
        .map(|highlighted| picker.items[highlighted].detail.clone())
        .unwrap_or_default();
    lines.push(Line::from(Span::styled(
        truncate(&detail, inner_width),
        dim,
    )));
    lines.push(Line::from(Span::styled(
        truncate("↑↓ move · Enter choose · Esc cancel", inner_width),
        dim,
    )));
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(picker.title)
                .padding(Padding::horizontal(1)),
        ),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::{item_at, list_rows, Picker, PickerChoice, PickerLine};
    use ilium_core::status_rule_expression::{Condition, ConditionCategory};
    use ilium_core::GoalState;
    use ratatui::layout::{Position, Rect};

    #[test]
    fn condition_picker_groups_every_catalogue_entry_under_its_category() {
        let picker = Picker::conditions(None);
        assert_eq!(picker.items.len(), super::super::condition_catalog().len());
        let headings: Vec<&str> = picker
            .lines()
            .into_iter()
            .filter_map(|line| match line {
                PickerLine::Heading(index) => Some(picker.items[index].group),
                PickerLine::Item(_) => None,
            })
            .collect();
        let expected: Vec<&str> = ConditionCategory::ALL
            .iter()
            .map(|category| category.label())
            .collect();
        assert_eq!(headings, expected, "one heading per category, in order");
        assert!(picker.items.iter().all(|item| !item.detail.is_empty()));
    }

    #[test]
    fn current_condition_is_highlighted_even_with_another_parameter() {
        let picker = Picker::conditions(Some(&Condition::ProgressAtLeast(90)));
        assert!(matches!(
            picker.chosen(),
            Some(PickerChoice::Condition(Condition::ProgressAtLeast(_)))
        ));
        let paused = Picker::conditions(Some(&Condition::GoalState(GoalState::Paused)));
        assert_eq!(
            paused.chosen(),
            Some(&PickerChoice::Condition(Condition::GoalState(
                GoalState::Paused
            )))
        );
    }

    #[test]
    fn typing_filters_by_every_word_and_keeps_a_visible_highlight() {
        let mut picker = Picker::conditions(None);
        for character in "goal paus".chars() {
            picker.push_char(character);
        }
        let visible = picker.visible_items();
        assert!(!visible.is_empty());
        assert!(visible.iter().all(|index| picker.items[*index]
            .label
            .to_lowercase()
            .contains("pause")
            || picker.items[*index].detail.to_lowercase().contains("pause")));
        assert!(visible.contains(&picker.highlighted.unwrap()));
        for character in "zzzz".chars() {
            picker.push_char(character);
        }
        assert!(picker.visible_items().is_empty());
        assert_eq!(
            picker.chosen(),
            None,
            "nothing to choose when nothing matches"
        );
        for _ in 0..4 {
            picker.pop_char();
        }
        assert!(picker.chosen().is_some(), "Backspace brings entries back");
    }

    #[test]
    fn moving_stops_at_both_ends() {
        let mut picker = Picker::templates();
        picker.move_by(-5);
        assert_eq!(picker.highlighted, Some(0));
        picker.move_by(100);
        assert_eq!(picker.highlighted, Some(picker.items.len() - 1));
        assert_eq!(
            picker.chosen(),
            Some(&PickerChoice::Template(picker.items.len() - 1))
        );
    }

    #[test]
    fn clicks_hit_the_drawn_entry_and_never_headings_or_borders() {
        let area = Rect::new(10, 5, 40, 20);
        let mut picker = Picker::conditions(None);
        picker.move_by(30);
        let rows = list_rows(area, &picker);
        assert!(rows.len() <= 15, "rows fit between the chrome rows");
        let highlighted = picker.highlighted.unwrap();
        let (_, row) = rows
            .iter()
            .find(|(line, _)| *line == PickerLine::Item(highlighted))
            .copied()
            .expect("the highlighted entry is scrolled into view");
        assert_eq!(
            item_at(area, &picker, Position::new(15, row)),
            Some(highlighted)
        );
        for (line, row) in &rows {
            if let PickerLine::Heading(_) = line {
                assert_eq!(item_at(area, &picker, Position::new(15, *row)), None);
            }
        }
        assert_eq!(
            item_at(area, &picker, Position::new(10, row)),
            None,
            "border"
        );
        assert_eq!(
            item_at(area, &Picker::templates(), Position::new(15, 5)),
            None
        );
        assert!(list_rows(Rect::new(0, 0, 8, 4), &picker).is_empty());
    }
}
