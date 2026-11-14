//! State and geometry for the custom status-icon rules editor.
//!
//! Layout contract (shared by rendering and pointer hit tests):
//! - The heading row and the section-tab row take the first two lines; the
//!   three columns (rules, condition and icon, preview) start below them.
//! - Side by side, the preview keeps the configured tree width as long as
//!   the rules column gets at least [`MINIMUM_RULES_WIDTH`] (20) cells and
//!   the editor column at least [`MINIMUM_EDITOR_WIDTH`] (25); the rules
//!   column takes 55% of what the preview leaves.
//! - Below [`SINGLE_COLUMN_BELOW`] (68) cells the editor shows one column at
//!   a time, the one owning keyboard focus, so an 80-column terminal still
//!   gets three columns and a 60-column one never gets zero-width panes.
//! - Rules take three rows each; the list scrolls with the wheel and keeps
//!   the keyboard selection visible.

mod inspector;
pub(crate) mod keyboard;
pub mod picker;
pub(crate) mod pointer;
pub mod templates;

pub use inspector::{
    focused_pane_explanation, matching_pane_count, pane_explanation, winning_pane_counts,
    PaneExplanation,
};

use ilium_core::status_rule_expression::{
    from_line, to_line, why, AllOf, Condition, ConditionTerm, Connector, Rule, ScreenRegion,
    StatusRules, StatusVisual, VisualKind, VisualStyle,
};
use ratatui::layout::Rect;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph},
    Frame,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PreviewTab {
    #[default]
    Live,
    Demo,
}

/// Narrowest rules column shown beside the other two.
pub const MINIMUM_RULES_WIDTH: u16 = 20;
/// Narrowest condition-and-icon column shown beside the other two.
pub const MINIMUM_EDITOR_WIDTH: u16 = 25;
/// Below this content width one column is shown at a time.
pub const SINGLE_COLUMN_BELOW: u16 = 68;
/// Rows per rule in the list: name, summary, spacer.
pub const RULE_ROWS: u16 = 3;
/// Rows pinned under the list for the "no rule matches" fallback: a
/// divider and the fallback row itself.
pub const FALLBACK_ROWS: u16 = 2;
/// Blank cells between each column's border and its text, so content never
/// touches the frame (CLAUDE.md "Settings tabs design and aeration").
pub const INNER_MARGIN: u16 = 1;
/// Most recent rule lists kept for undo.
pub const UNDO_DEPTH: usize = 20;
/// Breadcrumb shown when the heading row is wide enough for it.
pub const BREADCRUMB: &str = "Settings › Agent Monitoring › Custom status icons";
pub const SHORT_HEADING: &str = "Custom status icons";

pub fn preview_tabs(area: Rect) -> [(PreviewTab, Rect); 2] {
    let left = area.width / 2;
    let height = u16::from(area.height > 0);
    [
        (PreviewTab::Live, Rect::new(area.x, area.y, left, height)),
        (
            PreviewTab::Demo,
            Rect::new(
                area.x.saturating_add(left),
                area.y,
                area.width - left,
                height,
            ),
        ),
    ]
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EditorColumn {
    #[default]
    Rules,
    ConditionAndVisual,
    Preview,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DraftTarget {
    Condition,
    Visual,
    Colour,
    RuleName,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct EditorLayout {
    pub rules: Rect,
    pub editor: Rect,
    pub preview: Rect,
    pub back: Rect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuleAction {
    Toggle,
    Up,
    Down,
    Edit,
    Duplicate,
    Delete,
    Rename,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConditionAction {
    Previous,
    Next,
    Not,
    Connector,
    Add,
    Delete,
    Edit,
    Case,
    /// Cycle the screen region a pattern is matched against.
    Region,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VisualAction {
    Type,
    Value,
    Colour,
    Bold,
    Dim,
    Slower,
    Faster,
    PingPong,
}

pub fn visual_actions(area: Rect, condition_count: usize) -> Option<Rect> {
    let row = area
        .y
        .saturating_add(2)
        .saturating_add(condition_count as u16);
    if area.width < 17 + 2 * INNER_MARGIN || row >= area.bottom().saturating_sub(1) {
        return None;
    }
    Some(Rect::new(
        area.x.saturating_add(1 + INNER_MARGIN),
        row,
        15,
        1,
    ))
}

pub fn visual_action_at(
    area: Rect,
    count: usize,
    position: ratatui::layout::Position,
) -> Option<VisualAction> {
    let rect = visual_actions(area, count)?;
    if !rect.contains(position) {
        return None;
    }
    match position.x - rect.x {
        0 => Some(VisualAction::Type),
        2 => Some(VisualAction::Value),
        4 => Some(VisualAction::Colour),
        6 => Some(VisualAction::Bold),
        8 => Some(VisualAction::Dim),
        10 => Some(VisualAction::Slower),
        12 => Some(VisualAction::Faster),
        14 => Some(VisualAction::PingPong),
        _ => None,
    }
}

/// Nine one-cell controls at the end of a condition row:
/// `‹ › ¬ ∨ + × ✎ i ≡` (the last two only for screen patterns).
pub fn condition_actions(area: Rect, row: u16) -> Option<Rect> {
    if area.width < 20 + 2 * INNER_MARGIN || row <= area.y || row >= area.bottom().saturating_sub(1)
    {
        return None;
    }
    Some(Rect::new(
        area.right().saturating_sub(10 + INNER_MARGIN),
        row,
        9,
        1,
    ))
}

pub fn condition_action_at(
    area: Rect,
    row: u16,
    position: ratatui::layout::Position,
) -> Option<ConditionAction> {
    let rect = condition_actions(area, row)?;
    if !rect.contains(position) {
        return None;
    }
    match position.x - rect.x {
        0 => Some(ConditionAction::Previous),
        1 => Some(ConditionAction::Next),
        2 => Some(ConditionAction::Not),
        3 => Some(ConditionAction::Connector),
        4 => Some(ConditionAction::Add),
        5 => Some(ConditionAction::Delete),
        6 => Some(ConditionAction::Edit),
        7 => Some(ConditionAction::Case),
        8 => Some(ConditionAction::Region),
        _ => None,
    }
}

pub fn add_rule_button(area: Rect) -> Option<Rect> {
    if area.width < 12 || area.height < 2 {
        return None;
    }
    Some(Rect::new(area.right().saturating_sub(7), area.y, 6, 1))
}

pub fn reset_rule_button(area: Rect) -> Option<Rect> {
    if area.width < 23 || area.height < 2 {
        return None;
    }
    Some(Rect::new(area.right().saturating_sub(15), area.y, 7, 1))
}

/// Controls occupy the last seven cells of a rule's first row
/// (`● ↑ ↓ ✎ ⧉ × N`). Returning no rectangle on compact or clipped rows
/// keeps borders and other targets inert.
pub fn rule_actions(area: Rect, row: u16) -> Option<Rect> {
    if area.width < 17 + 2 * INNER_MARGIN || row <= area.y || row >= area.bottom().saturating_sub(1)
    {
        return None;
    }
    Some(Rect::new(
        area.right().saturating_sub(8 + INNER_MARGIN),
        row,
        7,
        1,
    ))
}

/// The screen row of the pinned "no rule matches" fallback (the last row
/// inside the rules column), `None` when the column is too short to show
/// it beside at least one rule.
pub fn fallback_row(area: Rect) -> Option<u16> {
    (area.height >= 2 + RULE_ROWS + FALLBACK_ROWS).then(|| area.bottom().saturating_sub(2))
}

pub fn rule_action_at(
    area: Rect,
    row: u16,
    position: ratatui::layout::Position,
) -> Option<RuleAction> {
    let rect = rule_actions(area, row)?;
    if !rect.contains(position) {
        return None;
    }
    match position.x - rect.x {
        0 => Some(RuleAction::Toggle),
        1 => Some(RuleAction::Up),
        2 => Some(RuleAction::Down),
        3 => Some(RuleAction::Edit),
        4 => Some(RuleAction::Duplicate),
        5 => Some(RuleAction::Delete),
        6 => Some(RuleAction::Rename),
        _ => None,
    }
}

/// The section tabs share their exact rectangles with pointer hit testing.
pub fn section_tabs(area: Rect) -> [(EditorColumn, Rect); 3] {
    let row = area.y.saturating_add(1);
    let height = u16::from(area.height > 1);
    let first = area.width / 3;
    let second = (area.width - first) / 2;
    [
        (EditorColumn::Rules, Rect::new(area.x, row, first, height)),
        (
            EditorColumn::ConditionAndVisual,
            Rect::new(area.x.saturating_add(first), row, second, height),
        ),
        (
            EditorColumn::Preview,
            Rect::new(
                area.x.saturating_add(first).saturating_add(second),
                row,
                area.width - first - second,
                height,
            ),
        ),
    ]
}

/// Rules that fit inside a list of `height` rows (borders and, when it is
/// shown, the pinned fallback excluded).
pub fn visible_rule_rows(height: u16) -> usize {
    let fallback = if height >= 2 + RULE_ROWS + FALLBACK_ROWS {
        FALLBACK_ROWS
    } else {
        0
    };
    usize::from(height.saturating_sub(2 + fallback) / RULE_ROWS).max(1)
}

/// The first rule shown: the wheel position, clamped so the list never
/// scrolls past its end. Keyboard selection calls
/// [`EditorState::keep_selected_visible`] to move this window.
pub fn first_visible_rule(state: &EditorState, height: u16, count: usize) -> usize {
    state
        .rules_scroll
        .min(count.saturating_sub(visible_rule_rows(height)))
}

/// Which rule a pointer row inside the list belongs to.
pub fn rule_at_row(state: &EditorState, area: Rect, count: usize, row: u16) -> Option<usize> {
    if row <= area.y || row >= area.bottom().saturating_sub(1) {
        return None;
    }
    let offset = usize::from((row - area.y - 1) / RULE_ROWS);
    if offset >= visible_rule_rows(area.height) {
        return None;
    }
    let index = first_visible_rule(state, area.height, count) + offset;
    (index < count).then_some(index)
}

/// Keep the selected condition visible and reserve rows for the visual
/// controls and any active draft or error message.
pub fn condition_window(state: &EditorState, area: Rect, count: usize) -> (usize, usize) {
    // The fallback has one fixed "WHEN no rule matches" row instead of
    // conditions, so its icon controls sit where a one-condition rule's do.
    if state.fallback_selected {
        return (0, usize::from(area.height > 4));
    }
    let message_rows =
        usize::from(state.parameter_draft.is_some()) + usize::from(state.save_error.is_some());
    let available = usize::from(area.height.saturating_sub(4)).saturating_sub(message_rows);
    let visible = count.min(available);
    let first = if visible == 0 {
        0
    } else {
        state
            .selected_condition
            .min(count.saturating_sub(1))
            .saturating_sub(visible - 1)
    };
    (first, visible)
}

/// Preserve the configured tree width whenever the available area allows it.
/// At narrow widths each pane still has a usable rectangle, and the caller
/// may present the active pane on its own when the screen is too small.
pub fn layout(area: Rect, desired_tree_width: u16) -> EditorLayout {
    let back_width = area.width.min(10);
    let back = Rect::new(
        area.right().saturating_sub(back_width),
        area.y,
        back_width,
        u16::from(area.height > 0),
    );
    let content_y = area.y.saturating_add(2).min(area.bottom());
    let content_height = area.bottom().saturating_sub(content_y);
    let gaps = 2u16.min(area.width);
    let available = area.width.saturating_sub(gaps);
    let minimum_editor = MINIMUM_EDITOR_WIDTH;
    let minimum_rules = MINIMUM_RULES_WIDTH;
    let preview_width =
        desired_tree_width.min(available.saturating_sub(minimum_editor + minimum_rules));
    let rest = available.saturating_sub(preview_width);
    let preferred_rules = (u32::from(rest) * 55 / 100) as u16;
    let rules_width = preferred_rules
        .max(minimum_rules.min(rest))
        .min(rest.saturating_sub(minimum_editor.min(rest)));
    let editor_width = rest.saturating_sub(rules_width);
    let rules = Rect::new(area.x, content_y, rules_width, content_height);
    let editor = Rect::new(
        rules.right().saturating_add(u16::from(gaps > 0)),
        content_y,
        editor_width,
        content_height,
    );
    let preview = Rect::new(
        editor.right().saturating_add(u16::from(gaps > 1)),
        content_y,
        preview_width,
        content_height,
    );
    EditorLayout {
        rules,
        editor,
        preview,
        back,
    }
}

/// Where an open condition or template list is drawn: over the widest
/// visible column, the condition column on ties, so it is never squeezed
/// into a narrow rules list.
pub fn picker_area(regions: &EditorLayout) -> Rect {
    [regions.rules, regions.preview]
        .into_iter()
        .fold(regions.editor, |widest, candidate| {
            if candidate.width > widest.width {
                candidate
            } else {
                widest
            }
        })
}

/// On very narrow terminals present one selectable column at a time instead
/// of giving the preview zero cells or placing pointer targets off-screen.
pub fn layout_for_column(
    area: Rect,
    desired_tree_width: u16,
    column: EditorColumn,
) -> EditorLayout {
    let mut regions = layout(area, desired_tree_width);
    if area.width < SINGLE_COLUMN_BELOW {
        let content = Rect::new(
            area.x,
            area.y.saturating_add(2).min(area.bottom()),
            area.width,
            area.height.saturating_sub(2),
        );
        let empty = Rect::new(area.x, content.y, 0, 0);
        regions.rules = if column == EditorColumn::Rules {
            content
        } else {
            empty
        };
        regions.editor = if column == EditorColumn::ConditionAndVisual {
            content
        } else {
            empty
        };
        regions.preview = if column == EditorColumn::Preview {
            content
        } else {
            empty
        };
    }
    regions
}

#[derive(Debug, Clone)]
pub struct EditorState {
    pub selected_rule: Option<usize>,
    pub column: EditorColumn,
    pub preview_tab: PreviewTab,
    pub demo_scroll: u16,
    /// First rule the list shows, before clamping to the list length.
    pub rules_scroll: usize,
    pub selected_condition: usize,
    /// A deleted rule is kept for the session's undo action.
    pub deleted: Option<(usize, Rule<Condition, StatusVisual>)>,
    /// Rulesets (rules and fallback) as they were before each saved
    /// change, newest last, at most [`UNDO_DEPTH`] deep.
    pub history: Vec<StatusRules>,
    /// The message line under the editor: a refused edit's reason or a
    /// write problem. A refused edit never changes the rules.
    pub save_error: Option<String>,
    pub confirm_reset: bool,
    pub parameter_draft: Option<String>,
    pub draft_target: DraftTarget,
    /// The pinned "no rule matches" row is selected instead of a rule;
    /// visual edits then change `StatusRules::fallback`.
    pub fallback_selected: bool,
    /// The condition or template picker drawn over a column, when open.
    pub picker: Option<picker::Picker>,
}

impl EditorState {
    pub fn new(rules: &StatusRules) -> Self {
        Self {
            selected_rule: (!rules.rules.is_empty()).then_some(0),
            column: EditorColumn::Rules,
            preview_tab: PreviewTab::Live,
            demo_scroll: 0,
            rules_scroll: 0,
            selected_condition: 0,
            deleted: None,
            history: Vec::new(),
            save_error: None,
            confirm_reset: false,
            parameter_draft: None,
            draft_target: DraftTarget::Condition,
            fallback_selected: false,
            picker: None,
        }
    }

    /// The selected rule's index, `None` while the fallback row is selected
    /// or the selection is out of range.
    fn rule_index(&self, rules: &StatusRules) -> Option<usize> {
        self.selected_rule
            .filter(|index| !self.fallback_selected && *index < rules.rules.len())
    }

    /// The visual the designer edits: the fallback's (created blank on
    /// first edit) or the selected rule's.
    fn visual_mut<'a>(&self, rules: &'a mut StatusRules) -> Option<&'a mut StatusVisual> {
        if self.fallback_selected {
            return Some(rules.fallback.get_or_insert_with(|| StatusVisual {
                kind: VisualKind::Hidden,
                style: VisualStyle::default(),
            }));
        }
        let index = self.rule_index(rules)?;
        rules.rules.get_mut(index).map(|rule| &mut rule.then)
    }

    /// The visual the designer shows, see [`Self::visual_mut`].
    pub fn visual<'a>(&self, rules: &'a StatusRules) -> Option<&'a StatusVisual> {
        if self.fallback_selected {
            return rules.fallback.as_ref();
        }
        rules
            .rules
            .get(self.rule_index(rules)?)
            .map(|rule| &rule.then)
    }

    /// Select the pinned fallback row.
    pub fn select_fallback(&mut self) {
        self.fallback_selected = true;
        self.selected_condition = 0;
    }

    /// Open the condition picker on the selected condition.
    pub fn open_condition_picker(&mut self, rules: &StatusRules) -> Result<(), &'static str> {
        let (terms, _) = self.expression(rules)?;
        let current = terms
            .get(self.selected_condition)
            .map(|term| &term.condition);
        self.picker = Some(picker::Picker::conditions(current));
        Ok(())
    }

    /// Replace the selected condition with a catalogue entry, keeping the
    /// current parameter (pattern, percentage...) when the entry is the
    /// same kind of condition, and keeping its NOT.
    pub fn set_condition(
        &self,
        rules: &mut StatusRules,
        condition: Condition,
    ) -> Result<(), &'static str> {
        let (mut terms, connectors) = self.expression(rules)?;
        let term = terms
            .get_mut(self.selected_condition)
            .ok_or("Select a condition")?;
        if !picker::same_entry(&term.condition, &condition) {
            term.condition = condition;
        }
        self.replace_expression(rules, terms, &connectors)
    }

    /// Apply the open picker's highlighted entry and close it. Returns
    /// whether `rules` changed; an error keeps the picker closed and the
    /// rules untouched.
    pub fn choose_from_picker(&mut self, rules: &mut StatusRules) -> Result<bool, String> {
        let Some(open) = self.picker.take() else {
            return Ok(false);
        };
        match open.chosen().cloned() {
            None => Ok(false),
            Some(picker::PickerChoice::Condition(condition)) => {
                self.set_condition(rules, condition)?;
                Ok(true)
            }
            Some(picker::PickerChoice::Template(index)) => {
                let template = templates::templates()
                    .get(index)
                    .ok_or("Unknown template")?;
                if !self.add_rule_from(rules, template) {
                    return Err(format!(
                        "At most {} rules",
                        ilium_core::status_rule_expression::MAXIMUM_RULES
                    ));
                }
                Ok(true)
            }
        }
    }

    pub fn scroll_demo(&mut self, direction: i32) {
        self.demo_scroll = if direction < 0 {
            self.demo_scroll.saturating_sub(1)
        } else if direction > 0 {
            self.demo_scroll
                .saturating_add(1)
                .min(crate::status_demo::situations().len() as u16)
        } else {
            self.demo_scroll
        };
    }

    /// Wheel movement over the rules list; the selection may leave the view.
    pub fn scroll_rules(&mut self, direction: i32, count: usize, height: u16) {
        let max_first = count.saturating_sub(visible_rule_rows(height));
        self.rules_scroll = if direction < 0 {
            self.rules_scroll.saturating_sub(1)
        } else if direction > 0 {
            self.rules_scroll.saturating_add(1)
        } else {
            self.rules_scroll
        }
        .min(max_first);
    }

    /// Move the list window the least amount that shows the selected rule.
    pub fn keep_selected_visible(&mut self, height: u16) {
        let Some(selected) = self.selected_rule else {
            return;
        };
        let visible = visible_rule_rows(height);
        if selected < self.rules_scroll {
            self.rules_scroll = selected;
        } else if selected >= self.rules_scroll + visible {
            self.rules_scroll = selected + 1 - visible;
        }
    }

    /// Select by index (clamped) and keep the selection visible.
    pub fn select_rule(&mut self, index: usize, count: usize, height: u16) {
        self.selected_rule = (count > 0).then(|| index.min(count - 1));
        self.fallback_selected = false;
        self.selected_condition = 0;
        self.keep_selected_visible(height);
    }

    /// Remember the ruleset as it was before a saved change.
    pub fn record_history(&mut self, before: StatusRules) {
        if self.history.len() >= UNDO_DEPTH {
            self.history.remove(0);
        }
        self.history.push(before);
    }

    /// Restore the most recent recorded ruleset; falls back to the last
    /// deleted rule when nothing newer was recorded.
    pub fn undo(&mut self, rules: &mut StatusRules) -> bool {
        match self.history.pop() {
            Some(before) => {
                *rules = before;
                self.selected_rule = self
                    .selected_rule
                    .map(|index| index.min(rules.rules.len().saturating_sub(1)))
                    .filter(|_| !rules.rules.is_empty());
                true
            }
            None => self.undo_delete(rules),
        }
    }

    /// Save `rules`, recording the previous ruleset for undo; on failure the
    /// editor returns to `previous` and shows the error. After a success
    /// the message line keeps any lasting write problem (a failed write
    /// being retried, or no configuration folder) instead of hiding it.
    pub fn save_rules(&mut self, app: &mut crate::app::App, previous: Self, rules: StatusRules) {
        let before = app.custom_status_rules.clone();
        match app.settings_save_custom_status_rules(rules) {
            Ok(()) => {
                self.save_error = app.custom_status_rules_write_error.clone();
                self.record_history(before);
            }
            Err(error) => {
                *self = previous;
                self.save_error = Some(error);
            }
        }
    }

    /// Insert a copy of the selected rule right below it, with a fresh
    /// identifier, and select the copy.
    pub fn duplicate_selected(&mut self, rules: &mut StatusRules) -> bool {
        if rules.rules.len() >= 64 {
            return false;
        }
        let Some(index) = self.rule_index(rules) else {
            return false;
        };
        let mut copy = rules.rules[index].clone();
        copy.id = self.fresh_rule_id(rules);
        copy.name = format!("{} copy", copy.name);
        if copy.name.chars().count() > 48 {
            copy.name = copy.name.chars().take(48).collect();
        }
        rules.rules.insert(index + 1, copy);
        self.selected_rule = Some(index + 1);
        true
    }

    fn fresh_rule_id(&self, rules: &StatusRules) -> String {
        let mut number = 1usize;
        while rules
            .rules
            .iter()
            .any(|rule| rule.id == format!("custom-{number}"))
            || self
                .deleted
                .as_ref()
                .is_some_and(|(_, rule)| rule.id == format!("custom-{number}"))
        {
            number += 1;
        }
        format!("custom-{number}")
    }

    /// Screen patterns match the whole grid, its first lines or its last
    /// lines; cycle through the useful sizes.
    pub fn cycle_screen_region(&self, rules: &mut StatusRules) -> Result<(), &'static str> {
        let (mut terms, connectors) = self.expression(rules)?;
        let term = terms
            .get_mut(self.selected_condition)
            .ok_or("Select a condition")?;
        let Condition::ScreenMatches(pattern) = &mut term.condition else {
            return Err("Select a screen pattern");
        };
        pattern.region = next_region(pattern.region);
        self.replace_expression(rules, terms, &connectors)
    }

    pub fn move_selected(&mut self, rules: &mut StatusRules, direction: i32) -> bool {
        let Some(index) = self.rule_index(rules) else {
            return false;
        };
        let destination = if direction < 0 {
            index.checked_sub(1)
        } else if direction > 0 {
            index
                .checked_add(1)
                .filter(|next| *next < rules.rules.len())
        } else {
            None
        };
        let Some(destination) = destination else {
            return false;
        };
        rules.rules.swap(index, destination);
        self.selected_rule = Some(destination);
        true
    }

    pub fn delete_selected(&mut self, rules: &mut StatusRules) -> bool {
        let Some(index) = self.rule_index(rules) else {
            return false;
        };
        self.deleted = Some((index, rules.rules.remove(index)));
        self.selected_rule = (!rules.rules.is_empty()).then(|| index.min(rules.rules.len() - 1));
        true
    }

    pub fn undo_delete(&mut self, rules: &mut StatusRules) -> bool {
        let Some((index, rule)) = self.deleted.take() else {
            return false;
        };
        let index = index.min(rules.rules.len());
        rules.rules.insert(index, rule);
        self.selected_rule = Some(index);
        true
    }

    /// Append an enabled starter rule with a stable identifier unique within
    /// this ruleset. The new rule deliberately has a condition rather than
    /// matching every pane and hiding lower-priority rules by accident.
    pub fn add_rule(&mut self, rules: &mut StatusRules) -> bool {
        if rules.rules.len() >= 64 {
            return false;
        }
        let id = self.fresh_rule_id(rules);
        let number = id.trim_start_matches("custom-").to_owned();
        rules.rules.push(Rule {
            id,
            name: format!("Custom rule {number}"),
            enabled: true,
            any_of: vec![AllOf {
                all_of: vec![ConditionTerm {
                    condition: Condition::GoalSet,
                    negated: false,
                }],
            }],
            then: StatusVisual {
                kind: VisualKind::Glyph("●".to_owned()),
                style: VisualStyle::default(),
            },
        });
        self.selected_rule = Some(rules.rules.len() - 1);
        self.fallback_selected = false;
        true
    }

    /// Append a rule built from a template, select it and its first
    /// condition. Refused at the rule limit.
    pub fn add_rule_from(
        &mut self,
        rules: &mut StatusRules,
        template: &templates::RuleTemplate,
    ) -> bool {
        if rules.rules.len() >= ilium_core::status_rule_expression::MAXIMUM_RULES {
            return false;
        }
        let id = self.fresh_rule_id(rules);
        rules.rules.push(template.build(id));
        self.selected_rule = Some(rules.rules.len() - 1);
        self.fallback_selected = false;
        self.selected_condition = 0;
        true
    }

    pub fn toggle_selected(&self, rules: &mut StatusRules) -> bool {
        let Some(rule) = self
            .rule_index(rules)
            .and_then(|index| rules.rules.get_mut(index))
        else {
            return false;
        };
        rule.enabled = !rule.enabled;
        true
    }

    pub fn set_rule_name(&self, rules: &mut StatusRules, input: &str) -> Result<(), String> {
        let name = input.trim();
        if name.is_empty() || name.chars().count() > 48 || name.chars().any(char::is_control) {
            return Err("Enter a name of 1 to 48 printable characters".into());
        }
        let rule = self
            .rule_index(rules)
            .and_then(|index| rules.rules.get_mut(index))
            .ok_or("Select a rule first")?;
        rule.name = name.to_owned();
        Ok(())
    }

    /// Replace the flat expression atomically: invalid connector counts or
    /// structural limits leave the existing rule unchanged.
    pub fn replace_expression(
        &self,
        rules: &mut StatusRules,
        terms: Vec<ConditionTerm<Condition>>,
        connectors: &[Connector],
    ) -> Result<(), &'static str> {
        let groups = from_line(terms, connectors)?;
        if groups.is_empty()
            || groups.len() > 8
            || groups.iter().any(|group| group.all_of.len() > 8)
        {
            return Err("Choose at least one condition and at most eight conditions per group");
        }
        let rule = self
            .rule_index(rules)
            .and_then(|index| rules.rules.get_mut(index))
            .ok_or("Select a rule first")?;
        rule.any_of = groups;
        Ok(())
    }

    pub fn expression(
        &self,
        rules: &StatusRules,
    ) -> Result<(Vec<ConditionTerm<Condition>>, Vec<Connector>), &'static str> {
        if self.fallback_selected {
            return Err("The fallback applies when no rule matches; it has no conditions");
        }
        let rule = self
            .rule_index(rules)
            .and_then(|index| rules.rules.get(index))
            .ok_or("Select a rule first")?;
        to_line(&rule.any_of)
    }

    pub fn add_condition(&mut self, rules: &mut StatusRules) -> Result<(), &'static str> {
        let (mut terms, mut connectors) = self.expression(rules)?;
        let index = self.selected_condition.min(terms.len().saturating_sub(1)) + 1;
        terms.insert(
            index,
            ConditionTerm {
                condition: Condition::GoalSet,
                negated: false,
            },
        );
        connectors.insert(index - 1, Connector::And);
        self.replace_expression(rules, terms, &connectors)?;
        self.selected_condition = index;
        Ok(())
    }

    pub fn remove_condition(&mut self, rules: &mut StatusRules) -> Result<(), &'static str> {
        let (mut terms, mut connectors) = self.expression(rules)?;
        if terms.len() <= 1 {
            return Err("A rule needs at least one condition");
        }
        let index = self.selected_condition.min(terms.len() - 1);
        terms.remove(index);
        connectors.remove(index.saturating_sub(1));
        self.replace_expression(rules, terms, &connectors)?;
        self.selected_condition = index.min(self.expression(rules)?.0.len() - 1);
        Ok(())
    }

    pub fn negate_condition(&self, rules: &mut StatusRules) -> Result<(), &'static str> {
        let (mut terms, connectors) = self.expression(rules)?;
        let term = terms
            .get_mut(self.selected_condition)
            .ok_or("Select a condition")?;
        term.negated = !term.negated;
        self.replace_expression(rules, terms, &connectors)
    }

    pub fn toggle_connector(&self, rules: &mut StatusRules) -> Result<(), &'static str> {
        let (terms, mut connectors) = self.expression(rules)?;
        let index = self
            .selected_condition
            .checked_sub(1)
            .ok_or("The first condition has no preceding connector")?;
        let connector = connectors.get_mut(index).ok_or("Select a condition")?;
        *connector = match connector {
            Connector::And => Connector::Or,
            Connector::Or => Connector::And,
        };
        self.replace_expression(rules, terms, &connectors)
    }

    pub fn cycle_condition(
        &self,
        rules: &mut StatusRules,
        direction: i32,
    ) -> Result<(), &'static str> {
        let catalog = condition_catalog();
        let (mut terms, connectors) = self.expression(rules)?;
        let term = terms
            .get_mut(self.selected_condition)
            .ok_or("Select a condition")?;
        // Parameter changes do not change the condition's place in the
        // catalog. Compare the variant for editable conditions and the full
        // value for states such as individual goal or agent states.
        let current = catalog
            .iter()
            .position(|condition| picker::same_entry(condition, &term.condition))
            .ok_or("This condition is not available in the selector")?;
        let next = (current as i32 + direction.signum()).rem_euclid(catalog.len() as i32) as usize;
        term.condition = catalog[next].clone();
        self.replace_expression(rules, terms, &connectors)
    }

    pub fn parameter(&self, rules: &StatusRules) -> Option<String> {
        let (terms, _) = self.expression(rules).ok()?;
        match &terms.get(self.selected_condition)?.condition {
            Condition::ScreenMatches(pattern) => Some(pattern.pattern.clone()),
            Condition::MonitorCountAtLeast(value) => Some(value.to_string()),
            Condition::ProgressAtLeast(value)
            | Condition::ProgressBelow(value)
            | Condition::ContextAbovePercent(value) => Some(value.to_string()),
            Condition::ProgressBetween { minimum, maximum } => Some(format!("{minimum},{maximum}")),
            _ => None,
        }
    }

    pub fn set_parameter(&self, rules: &mut StatusRules, input: &str) -> Result<(), String> {
        let (mut terms, connectors) = self.expression(rules).map_err(str::to_owned)?;
        let term = terms
            .get_mut(self.selected_condition)
            .ok_or("Select a condition")?;
        match &mut term.condition {
            Condition::ScreenMatches(pattern) => {
                if input.chars().count() > 256 {
                    return Err("Pattern must have at most 256 characters".into());
                }
                regex::RegexBuilder::new(input)
                    .case_insensitive(pattern.ignore_case)
                    .multi_line(true)
                    .size_limit(1 << 20)
                    .dfa_size_limit(1 << 20)
                    .build()
                    .map_err(|error| error.to_string())?;
                pattern.pattern = input.to_owned();
            }
            Condition::MonitorCountAtLeast(value) => {
                *value = input
                    .parse()
                    .map_err(|_| "Enter a nonnegative monitor count")?
            }
            Condition::ProgressAtLeast(value)
            | Condition::ProgressBelow(value)
            | Condition::ContextAbovePercent(value) => {
                let parsed: u8 = input
                    .parse()
                    .map_err(|_| "Enter a percentage from 0 to 100")?;
                if parsed > 100 {
                    return Err("Enter a percentage from 0 to 100".into());
                }
                *value = parsed;
            }
            Condition::ProgressBetween { minimum, maximum } => {
                let (low, high) = input.split_once(',').ok_or("Enter minimum,maximum")?;
                let low: u8 = low
                    .trim()
                    .parse()
                    .map_err(|_| "Enter minimum,maximum from 0 to 100")?;
                let high: u8 = high
                    .trim()
                    .parse()
                    .map_err(|_| "Enter minimum,maximum from 0 to 100")?;
                if low > high || high > 100 {
                    return Err("Minimum must not exceed maximum; both must be at most 100".into());
                }
                *minimum = low;
                *maximum = high;
            }
            _ => return Err("This condition has no editable parameter".into()),
        }
        self.replace_expression(rules, terms, &connectors)
            .map_err(str::to_owned)
    }

    pub fn toggle_screen_case(&self, rules: &mut StatusRules) -> Result<(), &'static str> {
        let (mut terms, connectors) = self.expression(rules)?;
        let term = terms
            .get_mut(self.selected_condition)
            .ok_or("Select a condition")?;
        let Condition::ScreenMatches(pattern) = &mut term.condition else {
            return Err("Select a screen pattern");
        };
        pattern.ignore_case = !pattern.ignore_case;
        self.replace_expression(rules, terms, &connectors)
    }

    pub fn cycle_visual(&self, rules: &mut StatusRules) -> bool {
        let Some(visual) = self.visual_mut(rules) else {
            return false;
        };
        visual.kind = match &visual.kind {
            VisualKind::Hidden => VisualKind::BuiltIn("working".into()),
            VisualKind::BuiltIn(_) => VisualKind::Glyph("●".into()),
            VisualKind::Glyph(_) => VisualKind::BrailleAnimation {
                preset: "spinner".into(),
                frames_per_second: 8,
            },
            VisualKind::BrailleAnimation { .. } => {
                crate::status_rule_visual::FRAME_FAMILIES[0].visual_kind()
            }
            // Step through the ready-made frame families (quarters, moon
            // phases, clock hours, arrows, hearts, blocks); user-typed frames
            // or the last family move on to progress frames.
            VisualKind::FrameAnimation { frames, .. } => {
                match crate::status_rule_visual::frame_family_of(frames).and_then(|family| {
                    crate::status_rule_visual::FRAME_FAMILIES
                        .iter()
                        .skip_while(|candidate| candidate.name != family.name)
                        .nth(1)
                }) {
                    Some(next) => next.visual_kind(),
                    None => VisualKind::ProgressFrames {
                        frames: ["🌑", "🌒", "🌓", "🌔", "🌕"]
                            .into_iter()
                            .map(str::to_owned)
                            .collect(),
                    },
                }
            }
            VisualKind::ProgressFrames { .. } => VisualKind::Hidden,
        };
        true
    }

    pub fn visual_value(&self, rules: &StatusRules) -> Option<String> {
        let visual = self.visual(rules)?;
        match &visual.kind {
            VisualKind::Hidden => None,
            VisualKind::BuiltIn(key) | VisualKind::Glyph(key) => Some(key.clone()),
            VisualKind::BrailleAnimation { preset, .. } => Some(preset.clone()),
            VisualKind::FrameAnimation { frames, .. } | VisualKind::ProgressFrames { frames } => {
                Some(frames.join("|"))
            }
        }
    }

    pub fn set_visual_value(&self, rules: &mut StatusRules, input: &str) -> Result<(), String> {
        let target = self.visual_mut(rules).ok_or("Select a rule")?;
        let mut visual = target.clone();
        match &mut visual.kind {
            VisualKind::Hidden => return Err("Choose a visible icon type first".into()),
            VisualKind::BuiltIn(key) | VisualKind::Glyph(key) => *key = input.to_owned(),
            VisualKind::BrailleAnimation { preset, .. } => *preset = input.to_owned(),
            VisualKind::FrameAnimation { frames, .. } | VisualKind::ProgressFrames { frames } => {
                *frames = input.split('|').map(str::to_owned).collect()
            }
        }
        crate::status_rule_visual::validate_visual(&visual, 2).map_err(str::to_owned)?;
        *target = visual;
        Ok(())
    }

    pub fn set_colour(&self, rules: &mut StatusRules, input: &str) -> Result<(), String> {
        let target = self.visual_mut(rules).ok_or("Select a rule")?;
        let mut visual = target.clone();
        visual.style.color = (!input.is_empty()).then(|| input.to_owned());
        crate::status_rule_visual::validate_visual(&visual, 2).map_err(str::to_owned)?;
        *target = visual;
        Ok(())
    }

    pub fn toggle_weight(&self, rules: &mut StatusRules, bold: bool) -> bool {
        let Some(visual) = self.visual_mut(rules) else {
            return false;
        };
        if bold {
            visual.style.bold = !visual.style.bold;
        } else {
            visual.style.dim = !visual.style.dim;
        }
        true
    }

    pub fn adjust_animation_speed(&self, rules: &mut StatusRules, direction: i8) -> bool {
        let Some(visual) = self.visual_mut(rules) else {
            return false;
        };
        let frames_per_second = match &mut visual.kind {
            VisualKind::BrailleAnimation {
                frames_per_second, ..
            }
            | VisualKind::FrameAnimation {
                frames_per_second, ..
            } => frames_per_second,
            _ => return false,
        };
        let updated =
            (i16::from(*frames_per_second) + i16::from(direction.signum())).clamp(1, 20) as u8;
        let changed = *frames_per_second != updated;
        *frames_per_second = updated;
        changed
    }

    pub fn toggle_ping_pong(&self, rules: &mut StatusRules) -> bool {
        let Some(visual) = self.visual_mut(rules) else {
            return false;
        };
        let VisualKind::FrameAnimation { ping_pong, .. } = &mut visual.kind else {
            return false;
        };
        *ping_pong = !*ping_pong;
        true
    }
}

/// Region cycle: whole screen, then the first 1/5/10 lines, then the last
/// 1/5/10 lines.
pub const fn next_region(region: ScreenRegion) -> ScreenRegion {
    match region {
        ScreenRegion::All => ScreenRegion::FirstLines(1),
        ScreenRegion::FirstLines(1) => ScreenRegion::FirstLines(5),
        ScreenRegion::FirstLines(5) => ScreenRegion::FirstLines(10),
        ScreenRegion::FirstLines(_) => ScreenRegion::LastLines(1),
        ScreenRegion::LastLines(1) => ScreenRegion::LastLines(5),
        ScreenRegion::LastLines(5) => ScreenRegion::LastLines(10),
        ScreenRegion::LastLines(_) => ScreenRegion::All,
    }
}

/// Where a screen pattern looks, as a short row suffix (empty for the whole
/// screen, which the condition label already implies).
pub fn region_suffix(condition: &Condition) -> String {
    match condition {
        Condition::ScreenMatches(pattern) => match pattern.region {
            ScreenRegion::All => String::new(),
            ScreenRegion::FirstLines(lines) => format!(" · first {lines} lines"),
            ScreenRegion::LastLines(lines) => format!(" · last {lines} lines"),
        },
        _ => String::new(),
    }
}

/// The core registry's catalogue, re-exposed under the editor's historical
/// name so callers and tests keep one entry point.
pub fn condition_catalog() -> Vec<Condition> {
    ilium_core::status_rule_expression::condition_catalog()
}

/// Human-readable condition text shared by the list, the selected rule and
/// the tree's "why" explanation (one definition in `ilium-core`).
pub fn condition_label(condition: &Condition) -> String {
    condition.label()
}

fn visual_label(visual: &VisualKind) -> String {
    match visual {
        VisualKind::Hidden => "no icon".into(),
        VisualKind::BuiltIn(key) => key.replace('-', " "),
        VisualKind::Glyph(glyph) => glyph.clone(),
        VisualKind::BrailleAnimation { .. } => "braille animation".into(),
        VisualKind::FrameAnimation { frames, .. } => {
            match crate::status_rule_visual::frame_family_of(frames) {
                Some(family) => format!("{} animation", family.name),
                None => format!("{} animation", frames.first().map_or("", String::as_str)),
            }
        }
        VisualKind::ProgressFrames { frames } => {
            format!("{} progress", frames.first().map_or("", String::as_str))
        }
    }
}

#[cfg(test)]
fn rule_summary(rule: &Rule<Condition, StatusVisual>) -> String {
    let Ok((terms, connectors)) = to_line(&rule.any_of) else {
        return "Invalid conditions".into();
    };
    let mut summary = String::from("IF ");
    for (index, term) in terms.iter().enumerate() {
        if index > 0 {
            summary.push_str(if connectors[index - 1] == Connector::And {
                " AND "
            } else {
                " OR "
            });
        }
        if term.negated {
            summary.push_str("NOT ");
        }
        summary.push_str(&condition_label(&term.condition));
    }
    summary.push_str(" THEN ");
    summary.push_str(&visual_label(&rule.then.kind));
    summary
}

fn rule_summary_line(
    rule: &Rule<Condition, StatusVisual>,
    width: u16,
    app: &crate::app::App,
) -> Line<'static> {
    let keyword = Style::new().fg(Color::White).add_modifier(Modifier::BOLD);
    let mut visual = crate::status_rule_visual::visual_span(
        &rule.then,
        &app.ui_settings.icons,
        app.ui_settings.use_stable_glyphs,
        u64::try_from(app.started_at.elapsed().as_millis()).unwrap_or(u64::MAX),
        app.ui_settings.motion_level,
        crate::status_rule_visual::ProgressCue::measured(50.0),
    );
    if visual.content.is_empty() {
        visual = Span::styled("∅", Style::new().fg(Color::DarkGray));
    }
    let budget = usize::from(width).saturating_sub(6 + visual.width());
    let mut spans = Vec::new();
    let mut used = 0usize;
    let mut add = |text: String, style: Style| {
        let remaining = budget.saturating_sub(used);
        let mut clipped = String::new();
        for ch in text.chars() {
            let cells = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if used + cells > budget {
                break;
            }
            clipped.push(ch);
            used += cells;
        }
        if remaining > 0 && !clipped.is_empty() {
            spans.push(Span::styled(clipped, style));
        }
    };
    add("IF ".into(), keyword);
    if let Ok((terms, connectors)) = to_line(&rule.any_of) {
        for (index, term) in terms.iter().enumerate() {
            if index > 0 {
                add(
                    if connectors[index - 1] == Connector::And {
                        " AND "
                    } else {
                        " OR "
                    }
                    .into(),
                    keyword,
                );
            }
            if term.negated {
                add(
                    "NOT ".into(),
                    Style::new()
                        .fg(Color::LightRed)
                        .add_modifier(Modifier::BOLD),
                );
            }
            add(
                condition_label(&term.condition),
                Style::new().fg(picker::category_color(term.condition.category())),
            );
        }
    }
    spans.push(Span::styled(" THEN ", keyword));
    spans.push(visual);
    Line::from(spans)
}

/// Help-rail anchors for the editor as `(topic id, row within area, selected)`:
/// the heading (opening the editor), the section tabs (the focused column's
/// topic) and the THEN row (the icon designer).
pub fn help_anchor_lines(
    area: Rect,
    app: &crate::app::App,
    state: &EditorState,
) -> Vec<(&'static str, u16, bool)> {
    let mut anchors = vec![("AM-26", 0, false)];
    if area.height > 1 {
        let topic = match state.column {
            EditorColumn::Rules => "AM-27",
            EditorColumn::ConditionAndVisual => "AM-28",
            EditorColumn::Preview => "AM-30",
        };
        anchors.push((topic, 1, true));
    }
    let desired = app
        .ui_settings
        .left_panel_sizing
        .resolved_width(app.layout.screen_area.width, true);
    let regions = layout_for_column(area, desired, state.column);
    if state.fallback_selected || state.expression(&app.custom_status_rules).is_ok() {
        let count = state
            .expression(&app.custom_status_rules)
            .map_or(0, |(terms, _)| terms.len());
        let (_, visible) = condition_window(state, regions.editor, count);
        let then_row = regions
            .editor
            .y
            .saturating_add(1)
            .saturating_add(visible as u16);
        if regions.editor.width > 0 && then_row < regions.editor.bottom().saturating_sub(1) {
            anchors.push((
                "AM-29",
                then_row - area.y,
                state.column == EditorColumn::ConditionAndVisual,
            ));
        }
    }
    anchors
}

/// The heading row: the breadcrumb when it fits, the short title otherwise.
fn heading_line(state: &EditorState, width: u16) -> Line<'static> {
    let bold = Style::new().add_modifier(Modifier::BOLD);
    if state.confirm_reset {
        return Line::from(Span::styled(
            "Reset rules? Y confirms · Esc cancels",
            bold.fg(Color::Yellow),
        ));
    }
    if usize::from(width) > BREADCRUMB.chars().count() + 1 {
        let prefix = &BREADCRUMB[..BREADCRUMB.len() - SHORT_HEADING.len()];
        Line::from(vec![
            Span::styled(prefix.to_owned(), Style::new().add_modifier(Modifier::DIM)),
            Span::styled(SHORT_HEADING, bold),
        ])
    } else {
        Line::from(Span::styled(SHORT_HEADING, bold))
    }
}

/// For each flattened term: whether it opens and whether it closes an AND
/// group that the row display should bracket. Brackets appear only when the
/// rule mixes AND and OR, where precedence matters.
fn group_brackets(rule: &Rule<Condition, StatusVisual>) -> Vec<(bool, bool)> {
    let bracket = rule.any_of.len() > 1;
    rule.any_of
        .iter()
        .flat_map(|group| {
            let len = group.all_of.len();
            (0..len).map(move |index| {
                (
                    bracket && len > 1 && index == 0,
                    bracket && len > 1 && index + 1 == len,
                )
            })
        })
        .collect()
}

fn dim() -> Style {
    Style::new().add_modifier(Modifier::DIM)
}

/// Paint the editor using the same geometry used by pointer hit tests.
pub fn render(frame: &mut Frame<'_>, area: Rect, app: &crate::app::App, state: &EditorState) {
    let desired = app
        .ui_settings
        .left_panel_sizing
        .resolved_width(app.layout.screen_area.width, true);
    let regions = layout_for_column(area, desired, state.column);
    let heading_width = area.width.saturating_sub(regions.back.width);
    frame.render_widget(
        Paragraph::new(heading_line(state, heading_width)),
        Rect::new(area.x, area.y, heading_width, 1),
    );
    frame.render_widget(
        Paragraph::new("‹ Back").style(crate::theme::selected_style()),
        regions.back,
    );
    if area.height > 1 {
        for (column, rect) in section_tabs(area) {
            let label = match column {
                EditorColumn::Rules => " Rules ",
                EditorColumn::ConditionAndVisual => " Edit ",
                EditorColumn::Preview => " Preview ",
            };
            let style = if column == state.column {
                crate::theme::selected_style().add_modifier(Modifier::BOLD)
            } else {
                Style::new()
            };
            frame.render_widget(Paragraph::new(label).style(style), rect);
        }
    }
    render_rules_column(frame, regions.rules, app, state);
    render_condition_column(frame, regions.editor, app, state);
    render_preview_column(frame, regions.preview, app, state);
    if let Some(open) = &state.picker {
        picker::render(frame, picker_area(&regions), open);
    }
}

fn render_rules_column(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &crate::app::App,
    state: &EditorState,
) {
    let count = app.custom_status_rules.rules.len();
    let first = first_visible_rule(state, area.height, count);
    let visible = visible_rule_rows(area.height);
    // Text sits inside the border and the inner margin on both sides; the
    // seven rule action cells take the end of each name row.
    let inner_width = usize::from(area.width.saturating_sub(2 + 2 * INNER_MARGIN));
    let issues = crate::config::validate_status_rules(&app.custom_status_rules);
    let live = winning_pane_counts(app);
    let mut lines = Vec::new();
    for (index, rule) in app
        .custom_status_rules
        .rules
        .iter()
        .enumerate()
        .skip(first)
        .take(visible)
    {
        let marker = if rule.enabled { "●" } else { "○" };
        let selected = state.selected_rule == Some(index) && !state.fallback_selected;
        let style = if selected {
            crate::theme::selected_style().add_modifier(Modifier::BOLD)
        } else if rule.enabled {
            Style::new()
        } else {
            dim()
        };
        let number = format!("{:>2} ", index + 1);
        let mut badges = Vec::new();
        if let Some(issue) = issues.iter().find(|issue| issue.rule == Some(index)) {
            let colour =
                if issue.severity == ilium_core::status_rule_expression::IssueSeverity::Error {
                    Color::LightRed
                } else {
                    Color::Yellow
                };
            badges.push(Span::styled("⚠ ", Style::new().fg(colour)));
        }
        let matched = live.get(index).copied().unwrap_or(0);
        if matched > 0 {
            badges.push(Span::styled(
                format!("{matched} live"),
                Style::new().fg(Color::Green),
            ));
        }
        let badge_width: usize = badges.iter().map(Span::width).sum();
        let room = inner_width.saturating_sub(7 + 1 + 2 + number.len() + badge_width);
        let name = crate::settings_ui::truncate_icon_text(&rule.name, room);
        let used = 2 + number.len() + unicode_width::UnicodeWidthStr::width(name.as_str());
        let gap = inner_width.saturating_sub(7 + 1 + used + badge_width);
        let mut spans = vec![
            Span::styled(format!("{marker} "), style),
            Span::styled(number, style.add_modifier(Modifier::DIM)),
            Span::styled(name, style),
            crate::value_control::leader_span(gap),
        ];
        spans.extend(badges);
        lines.push(Line::from(spans));
        lines.push(rule_summary_line(rule, inner_width as u16, app));
        lines.push(Line::from(""));
    }
    if count == 0 {
        lines.push(Line::from(Span::styled("No rules yet: A adds one", dim())));
    }
    let title = if count > visible {
        format!(
            " Rules {}–{} of {count} ",
            first + 1,
            (first + visible).min(count)
        )
    } else {
        " Rules ".to_owned()
    };
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .padding(Padding::horizontal(INNER_MARGIN)),
        ),
        area,
    );
    if let Some(row) = fallback_row(area) {
        render_fallback_row(frame, area, row, app, state, inner_width);
    }
    if let Some(button) = add_rule_button(area) {
        frame.render_widget(
            Paragraph::new("+ Add ").style(crate::theme::selected_style()),
            button,
        );
    }
    if let Some(button) = reset_rule_button(area) {
        frame.render_widget(
            Paragraph::new(if state.confirm_reset {
                "Confirm"
            } else {
                "Reset  "
            })
            .style(crate::theme::selected_style()),
            button,
        );
    }
    for (offset, rule) in app
        .custom_status_rules
        .rules
        .iter()
        .skip(first)
        .take(visible)
        .enumerate()
    {
        let row = area
            .y
            .saturating_add(1)
            .saturating_add((offset as u16).saturating_mul(RULE_ROWS));
        if let Some(rect) = rule_actions(area, row) {
            let enabled = if rule.enabled { "●" } else { "○" };
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(enabled, Style::new().fg(Color::Green)),
                    Span::styled("↑↓", Style::new().fg(Color::Cyan)),
                    Span::styled("✎", Style::new().fg(Color::Yellow)),
                    Span::styled("⧉", Style::new().fg(Color::Blue)),
                    Span::styled("×", Style::new().fg(Color::LightRed)),
                    Span::styled("N", Style::new().fg(Color::Magenta)),
                ])),
                rect,
            );
        }
    }
}

/// The pinned "no rule matches" row under a thin divider: the icon panes
/// get when every rule misses. It is selectable and edited like a rule's
/// icon, but has no conditions, cannot move and cannot be deleted.
fn render_fallback_row(
    frame: &mut Frame<'_>,
    area: Rect,
    row: u16,
    app: &crate::app::App,
    state: &EditorState,
    inner_width: usize,
) {
    let x = area.x.saturating_add(1 + INNER_MARGIN);
    let width = inner_width as u16;
    frame.render_widget(
        Paragraph::new(Span::styled("─".repeat(inner_width), dim())),
        Rect::new(x, row.saturating_sub(1), width, 1),
    );
    let style = if state.fallback_selected {
        crate::theme::selected_style().add_modifier(Modifier::BOLD)
    } else {
        Style::new()
    };
    let hidden = StatusVisual {
        kind: VisualKind::Hidden,
        style: VisualStyle::default(),
    };
    let visual = app.custom_status_rules.fallback.as_ref().unwrap_or(&hidden);
    let mut icon = crate::status_rule_visual::visual_span(
        visual,
        &app.ui_settings.icons,
        app.ui_settings.use_stable_glyphs,
        u64::try_from(app.started_at.elapsed().as_millis()).unwrap_or(u64::MAX),
        app.ui_settings.motion_level,
        crate::status_rule_visual::ProgressCue::measured(50.0),
    );
    if icon.content.is_empty() {
        icon = Span::styled("∅ no icon", dim());
    }
    let label = "Otherwise (no rule matches) ";
    let room = inner_width.saturating_sub(icon.width() + 5);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(crate::settings_ui::truncate_icon_text(label, room), style),
            Span::styled("THEN ", style.fg(Color::White)),
            icon,
        ])),
        Rect::new(x, row, width, 1),
    );
}

fn render_condition_column(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &crate::app::App,
    state: &EditorState,
) {
    let keyword_style = Style::new().fg(Color::White).add_modifier(Modifier::BOLD);
    let inner_width = usize::from(area.width.saturating_sub(2 + 2 * INNER_MARGIN));
    let mut details = Vec::new();
    let selected_rule = state
        .rule_index(&app.custom_status_rules)
        .map(|index| (index, &app.custom_status_rules.rules[index]));
    let hidden = StatusVisual {
        kind: VisualKind::Hidden,
        style: VisualStyle::default(),
    };
    let visual = if state.fallback_selected {
        Some(app.custom_status_rules.fallback.as_ref().unwrap_or(&hidden))
    } else {
        selected_rule.map(|(_, rule)| &rule.then)
    };
    if let Some(visual) = visual {
        if let Some((_, rule)) = selected_rule {
            let brackets = group_brackets(rule);
            if let Ok((terms, connectors)) = state.expression(&app.custom_status_rules) {
                let (first, visible) = condition_window(state, area, terms.len());
                for (index, term) in terms.iter().enumerate().skip(first).take(visible) {
                    let prefix = if index == 0 {
                        "IF"
                    } else if connectors[index - 1] == Connector::And {
                        "AND"
                    } else {
                        "OR"
                    };
                    let selected = state.column == EditorColumn::ConditionAndVisual
                        && state.selected_condition == index;
                    let background = if selected {
                        crate::theme::selected_style()
                    } else {
                        Style::new()
                    };
                    let keyword = background.fg(Color::White).add_modifier(Modifier::BOLD);
                    let (opens, closes) = brackets.get(index).copied().unwrap_or((false, false));
                    let mut spans = vec![Span::styled(format!("{prefix} "), keyword)];
                    if opens {
                        spans.push(Span::styled("(", keyword.fg(Color::Yellow)));
                    }
                    if term.negated {
                        spans.push(Span::styled(
                            "NOT ",
                            background.fg(Color::LightRed).add_modifier(Modifier::BOLD),
                        ));
                    }
                    spans.push(Span::styled(
                        format!(
                            "{}{}",
                            condition_label(&term.condition),
                            region_suffix(&term.condition)
                        ),
                        background.fg(picker::category_color(term.condition.category())),
                    ));
                    if closes {
                        spans.push(Span::styled(")", keyword.fg(Color::Yellow)));
                    }
                    details.push(Line::from(spans));
                }
            }
        } else if condition_window(state, area, 0).1 > 0 {
            details.push(Line::from(vec![
                Span::styled("WHEN ", keyword_style),
                Span::styled("no rule matches", Style::new().fg(Color::Gray)),
            ]));
        }
        details.push(Line::from(vec![
            Span::styled("THEN ", keyword_style),
            crate::status_rule_visual::visual_span(
                visual,
                &app.ui_settings.icons,
                app.ui_settings.use_stable_glyphs,
                u64::try_from(app.started_at.elapsed().as_millis()).unwrap_or(u64::MAX),
                app.ui_settings.motion_level,
                crate::status_rule_visual::ProgressCue::measured(50.0),
            ),
            Span::styled(
                format!("  {}", visual_label(&visual.kind)),
                Style::new().fg(Color::Cyan),
            ),
        ]));
        // The visual controls are painted over this row.
        details.push(Line::from(""));
        let explanation = match selected_rule {
            Some((rule_index, rule)) => {
                let matches = matching_pane_count(app, rule);
                details.push(Line::from(Span::styled(
                    format!(
                        "Matches {matches} pane{} now{}",
                        if matches == 1 { "" } else { "s" },
                        if rule.enabled { "" } else { " (rule disabled)" }
                    ),
                    dim(),
                )));
                if rule.any_of.len() > 1 {
                    details.push(Line::from(Span::styled(
                        "( ) AND groups are checked before OR",
                        dim().fg(Color::Yellow),
                    )));
                }
                why(rule_index + 1, rule, None)
            }
            None => {
                "Panes that no enabled rule matches show this icon. Hidden shows none.".to_owned()
            }
        };
        for text in crate::selectable_cards::wrap(&explanation, inner_width.max(8))
            .into_iter()
            .take(3)
        {
            details.push(Line::from(Span::styled(text, dim())));
        }
        details.push(Line::from(""));
        if let Some(draft) = &state.parameter_draft {
            details.push(Line::from(format!(
                "Value: {draft}▏  Enter saves · Esc cancels"
            )));
        }
        if let Some(message) = &state.save_error {
            details.push(Line::from(Span::styled(
                message.clone(),
                Style::new().fg(Color::LightRed),
            )));
        }
        let hints: &[&str] = match state.column {
            EditorColumn::Rules => &keyboard::HINTS_RULES,
            EditorColumn::ConditionAndVisual => &keyboard::HINTS_CONDITION,
            EditorColumn::Preview => &keyboard::HINTS_PREVIEW,
        };
        for hint in hints {
            details.push(Line::from(Span::styled(*hint, dim())));
        }
    } else {
        details.push(Line::from("Select a rule"));
        if let Some(message) = &state.save_error {
            details.push(Line::from(Span::styled(
                message.clone(),
                Style::new().fg(Color::LightRed),
            )));
        }
    }
    frame.render_widget(
        Paragraph::new(details).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Condition and icon ")
                .padding(Padding::horizontal(INNER_MARGIN)),
        ),
        area,
    );
    let count = state
        .expression(&app.custom_status_rules)
        .map_or(0, |(terms, _)| terms.len());
    if let Ok((terms, _)) = state.expression(&app.custom_status_rules) {
        let (first, visible) = condition_window(state, area, terms.len());
        for (offset, term) in terms.iter().skip(first).take(visible).enumerate() {
            let row = area.y.saturating_add(1).saturating_add(offset as u16);
            if let Some(rect) = condition_actions(area, row) {
                let controls = if matches!(&term.condition, Condition::ScreenMatches(_)) {
                    "‹›¬∨+×✎i≡"
                } else {
                    "‹›¬∨+×✎  "
                };
                frame.render_widget(
                    Paragraph::new(controls).style(Style::new().fg(Color::Cyan)),
                    rect,
                );
            }
        }
    }
    if visual.is_some() {
        let (_, visible) = condition_window(state, area, count);
        if let Some(rect) = visual_actions(area, visible) {
            let buttons = ["V", "G", "C", "B", "D", "−", "+", "P"];
            let mut spans = Vec::new();
            for (index, button) in buttons.into_iter().enumerate() {
                if index > 0 {
                    spans.push(Span::raw(" "));
                }
                spans.push(Span::styled(
                    button,
                    crate::theme::selected_style().add_modifier(Modifier::BOLD),
                ));
            }
            frame.render_widget(Paragraph::new(Line::from(spans)), rect);
        }
    }
}

/// Rows the "Why this icon" box takes under the live tree, when there is room.
pub const WHY_BOX_HEIGHT: u16 = 5;

fn render_preview_column(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &crate::app::App,
    state: &EditorState,
) {
    for (tab, rect) in preview_tabs(area) {
        let label = match tab {
            PreviewTab::Live => " Live tree ",
            PreviewTab::Demo => " Demo tree ",
        };
        let style = if tab == state.preview_tab {
            crate::theme::selected_style().add_modifier(Modifier::BOLD)
        } else {
            Style::new()
        };
        frame.render_widget(Paragraph::new(label).style(style), rect);
    }
    let tree_area = Rect::new(
        area.x,
        area.y.saturating_add(1),
        area.width,
        area.height.saturating_sub(1),
    );
    if tree_area.width == 0 || tree_area.height == 0 {
        return;
    }
    match state.preview_tab {
        PreviewTab::Live => {
            if app.tree.panes().next().is_none() {
                frame.render_widget(
                    Paragraph::new(vec![
                        Line::from("No panes in this session yet."),
                        Line::from(Span::styled(
                            "Open a pane, or switch to the Demo tree to see every situation.",
                            dim(),
                        )),
                    ])
                    .wrap(ratatui::widgets::Wrap { trim: true })
                    .block(Block::default().borders(Borders::ALL).title(" Live tree ")),
                    tree_area,
                );
                return;
            }
            let explanation = (tree_area.height >= WHY_BOX_HEIGHT + 5)
                .then(|| focused_pane_explanation(app))
                .flatten();
            let tree_rows = match &explanation {
                Some(_) => tree_area.height - WHY_BOX_HEIGHT,
                None => tree_area.height,
            };
            crate::settings_ui::render_live_tree_preview(
                frame,
                Rect::new(tree_area.x, tree_area.y, tree_area.width, tree_rows),
                app,
                crate::agent_monitoring::AgentMonitoringMode::Custom,
            );
            if let Some(explanation) = explanation {
                let width = usize::from(tree_area.width.saturating_sub(2)).max(8);
                let mut lines = vec![Line::from(Span::styled(
                    crate::settings_ui::truncate_icon_text(&explanation.pane_name, width),
                    Style::new().add_modifier(Modifier::BOLD),
                ))];
                for text in explanation
                    .lines
                    .iter()
                    .flat_map(|line| crate::selectable_cards::wrap(line, width))
                    .take(usize::from(WHY_BOX_HEIGHT - 3))
                {
                    lines.push(Line::from(text));
                }
                frame.render_widget(
                    Paragraph::new(lines).block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(" Why this icon "),
                    ),
                    Rect::new(
                        tree_area.x,
                        tree_area.y + tree_rows,
                        tree_area.width,
                        WHY_BOX_HEIGHT,
                    ),
                );
            }
        }
        PreviewTab::Demo => {
            let situations = crate::status_demo::situations();
            let mut lines = vec![Line::from("Demo project")];
            for (index, situation) in situations.iter().enumerate() {
                let icon = crate::status_demo::preview_span(
                    app,
                    crate::agent_monitoring::AgentMonitoringMode::Custom,
                    index,
                );
                let branch = if index + 1 == situations.len() {
                    "└ "
                } else {
                    "├ "
                };
                lines.push(Line::from(vec![
                    Span::raw(branch),
                    icon,
                    Span::raw(format!(" {}", situation.name)),
                ]));
            }
            frame.render_widget(
                Paragraph::new(lines).scroll((state.demo_scroll, 0)).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Demo · ↑↓ scroll "),
                ),
                tree_area,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{layout, EditorState};
    use ilium_core::status_rule_expression::{default_rules, Condition, ConditionTerm, Connector};
    use ratatui::layout::Rect;

    #[test]
    fn cycling_an_edited_condition_starts_at_its_kind() {
        use ilium_core::status_rule_expression::{ScreenPattern, ScreenRegion};

        let mut rules = default_rules();
        let mut editor = EditorState::new(&rules);
        assert!(editor.add_rule(&mut rules));
        let cases = [
            (
                Condition::MonitorCountAtLeast(9),
                Condition::TaskStatus(ilium_core::ProgressTaskStatus::NotStartedYet),
            ),
            (Condition::ProgressAtLeast(73), Condition::ProgressBelow(50)),
            (
                // A changed threshold still finds its catalogue entry.
                Condition::ContextAbovePercent(95),
                Condition::ScreenMatches(ScreenPattern {
                    pattern: "error".into(),
                    ignore_case: true,
                    region: ScreenRegion::All,
                }),
            ),
            (
                Condition::ScreenMatches(ScreenPattern {
                    pattern: "custom".into(),
                    ignore_case: false,
                    region: ScreenRegion::LastLines(3),
                }),
                Condition::Always,
            ),
        ];
        for (condition, expected) in cases {
            editor
                .replace_expression(
                    &mut rules,
                    vec![ConditionTerm {
                        condition,
                        negated: false,
                    }],
                    &[],
                )
                .unwrap();
            editor.cycle_condition(&mut rules, 1).unwrap();
            assert_eq!(editor.expression(&rules).unwrap().0[0].condition, expected);
        }
    }

    #[test]
    fn context_threshold_is_an_editable_percentage() {
        let mut rules = default_rules();
        let mut editor = EditorState::new(&rules);
        assert!(editor.add_rule(&mut rules));
        editor
            .replace_expression(
                &mut rules,
                vec![ConditionTerm {
                    condition: Condition::ContextAbovePercent(80),
                    negated: false,
                }],
                &[],
            )
            .unwrap();
        assert_eq!(editor.parameter(&rules).as_deref(), Some("80"));
        editor.set_parameter(&mut rules, "100").unwrap();
        assert_eq!(
            editor.expression(&rules).unwrap().0[0].condition,
            Condition::ContextAbovePercent(100)
        );
        for invalid in ["101", "-1", "", "ninety"] {
            assert!(
                editor.set_parameter(&mut rules, invalid).is_err(),
                "{invalid:?}"
            );
        }
        assert_eq!(editor.parameter(&rules).as_deref(), Some("100"));
    }

    #[test]
    fn preview_tabs_share_exact_display_and_hit_regions() {
        for width in [1, 9, 20, 41] {
            let area = Rect::new(5, 3, width, 8);
            let tabs = super::preview_tabs(area);
            assert_eq!(tabs[0].1.x, area.x);
            assert_eq!(tabs[0].1.right(), tabs[1].1.x);
            assert_eq!(tabs[1].1.right(), area.right());
            assert_eq!(tabs[0].1.height, 1);
        }
    }

    #[test]
    fn demo_scroll_stays_within_scenarios() {
        let mut editor = EditorState::new(&default_rules());
        editor.scroll_demo(-1);
        assert_eq!(editor.demo_scroll, 0);
        for _ in 0..100 {
            editor.scroll_demo(1);
        }
        assert_eq!(
            usize::from(editor.demo_scroll),
            crate::status_demo::situations().len()
        );
    }

    #[test]
    fn rules_list_scrolls_with_the_wheel_and_follows_the_keyboard_selection() {
        let rules = default_rules();
        let count = rules.rules.len();
        assert!(count >= 10, "defaults should fill more than one window");
        let mut editor = EditorState::new(&rules);
        // Height 13 shows three rules above the pinned fallback.
        assert_eq!(super::visible_rule_rows(13), 3);
        editor.scroll_rules(-1, count, 13);
        assert_eq!(super::first_visible_rule(&editor, 13, count), 0);
        for _ in 0..100 {
            editor.scroll_rules(1, count, 13);
        }
        assert_eq!(super::first_visible_rule(&editor, 13, count), count - 3);
        // The wheel leaves the selection at rule 0, off-screen; selecting
        // rule 1 by keyboard brings the window back to it minimally.
        editor.select_rule(1, count, 13);
        assert_eq!(super::first_visible_rule(&editor, 13, count), 1);
        editor.select_rule(5, count, 13);
        assert_eq!(super::first_visible_rule(&editor, 13, count), 3);
        let area = Rect::new(0, 2, 40, 13);
        assert_eq!(super::rule_at_row(&editor, area, count, 3), Some(3));
        assert_eq!(super::rule_at_row(&editor, area, count, 5), Some(3));
        assert_eq!(super::rule_at_row(&editor, area, count, 9), Some(5));
        assert_eq!(super::rule_at_row(&editor, area, count, 2), None);
        assert_eq!(super::rule_at_row(&editor, area, count, 12), None);
    }

    #[test]
    fn duplicate_inserts_a_fresh_copy_below_and_undo_history_restores_lists() {
        let mut rules = default_rules();
        let mut editor = EditorState::new(&rules);
        let count = rules.rules.len();
        let before = rules.rules.clone();
        assert!(editor.duplicate_selected(&mut rules));
        editor.record_history(ilium_core::status_rule_expression::StatusRules {
            rules: before.clone(),
            ..rules.clone()
        });
        assert_eq!(rules.rules.len(), count + 1);
        assert_eq!(editor.selected_rule, Some(1));
        assert_eq!(rules.rules[1].name, format!("{} copy", before[0].name));
        assert_ne!(rules.rules[1].id, rules.rules[0].id);
        assert_eq!(rules.rules[1].any_of, before[0].any_of);
        assert!(editor.undo(&mut rules));
        assert_eq!(rules.rules, before);
        // Without history, undo falls back to the deleted rule.
        assert!(editor.delete_selected(&mut rules));
        assert!(editor.undo(&mut rules));
        assert_eq!(rules.rules, before);
        assert!(!editor.undo(&mut rules));
        // History never grows past the undo depth.
        for _ in 0..(super::UNDO_DEPTH + 5) {
            editor.record_history(ilium_core::status_rule_expression::StatusRules {
                rules: before.clone(),
                ..rules.clone()
            });
        }
        assert_eq!(editor.history.len(), super::UNDO_DEPTH);
    }

    #[test]
    fn screen_region_cycles_through_first_and_last_lines_and_back() {
        use ilium_core::status_rule_expression::{ScreenPattern, ScreenRegion};
        let mut rules = default_rules();
        let mut editor = EditorState::new(&rules);
        assert!(editor.add_rule(&mut rules));
        assert!(editor.cycle_screen_region(&mut rules).is_err());
        editor
            .replace_expression(
                &mut rules,
                vec![ConditionTerm {
                    condition: Condition::ScreenMatches(ScreenPattern {
                        pattern: "ready".into(),
                        ignore_case: false,
                        region: ScreenRegion::All,
                    }),
                    negated: false,
                }],
                &[],
            )
            .unwrap();
        let mut seen = Vec::new();
        for _ in 0..7 {
            editor.cycle_screen_region(&mut rules).unwrap();
            let Condition::ScreenMatches(pattern) =
                editor.expression(&rules).unwrap().0[0].condition.clone()
            else {
                panic!("still a screen pattern");
            };
            seen.push(pattern.region);
        }
        assert_eq!(
            seen,
            [
                ScreenRegion::FirstLines(1),
                ScreenRegion::FirstLines(5),
                ScreenRegion::FirstLines(10),
                ScreenRegion::LastLines(1),
                ScreenRegion::LastLines(5),
                ScreenRegion::LastLines(10),
                ScreenRegion::All,
            ]
        );
        let last_five = Condition::ScreenMatches(ScreenPattern {
            pattern: "x".into(),
            ignore_case: true,
            region: ScreenRegion::LastLines(5),
        });
        assert_eq!(super::region_suffix(&last_five), " · last 5 lines");
        assert_eq!(super::region_suffix(&Condition::Always), "");
    }

    #[test]
    fn layout_keeps_minimum_column_widths_across_terminal_widths() {
        // Content width is roughly the terminal width minus the settings
        // navigation rail (about 30 cells at 80 columns).
        for (terminal, content) in [(80, 50), (100, 70), (120, 92), (160, 132), (200, 170)] {
            let area = Rect::new(30, 3, content, 30);
            let regions = layout(area, 32);
            let single = content < super::SINGLE_COLUMN_BELOW;
            assert_eq!(
                regions.rules.x, area.x,
                "rules start at the left edge at {terminal} columns"
            );
            assert!(
                regions.rules.width >= super::MINIMUM_RULES_WIDTH.min(content),
                "rules column too narrow at {terminal} columns: {regions:?}"
            );
            assert!(
                regions.editor.width
                    >= super::MINIMUM_EDITOR_WIDTH
                        .min(content.saturating_sub(super::MINIMUM_RULES_WIDTH + 2)),
                "editor column too narrow at {terminal} columns: {regions:?}"
            );
            assert!(
                regions.preview.right() <= area.right(),
                "preview overflows at {terminal} columns: {regions:?}"
            );
            assert_eq!(regions.rules.y, area.y + 2);
            for column in [
                super::EditorColumn::Rules,
                super::EditorColumn::ConditionAndVisual,
                super::EditorColumn::Preview,
            ] {
                let focused = super::layout_for_column(area, 32, column);
                let widths = [
                    focused.rules.width,
                    focused.editor.width,
                    focused.preview.width,
                ];
                if single {
                    assert_eq!(widths.iter().filter(|width| **width > 0).count(), 1);
                    assert_eq!(widths.iter().max().copied(), Some(content));
                } else {
                    assert!(
                        widths.iter().all(|width| *width > 0),
                        "{terminal}: {widths:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn editor_renders_at_capture_widths_with_breadcrumb_or_short_heading() {
        use ratatui::{backend::TestBackend, Terminal};
        let mut app = crate::app::App::new("widths".into(), std::env::temp_dir());
        app.custom_status_rules = default_rules();
        let group = app
            .tree
            .add_group(ilium_core::ROOT_ID, "Capture project")
            .unwrap();
        app.tree
            .add_pane(group, "agent one", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        let mut editor = EditorState::new(&app.custom_status_rules);
        editor.column = super::EditorColumn::ConditionAndVisual;
        for (terminal, content) in [(80u16, 50u16), (120, 92), (200, 170)] {
            app.set_screen_area(Rect::new(0, 0, terminal, 40));
            let mut backend_terminal = Terminal::new(TestBackend::new(content, 36)).unwrap();
            backend_terminal
                .draw(|frame| super::render(frame, frame.area(), &app, &editor))
                .unwrap();
            let buffer = backend_terminal.backend().buffer();
            let rows: Vec<String> = (0..buffer.area.height)
                .map(|y| {
                    (0..buffer.area.width)
                        .map(|x| buffer[(x, y)].symbol().to_owned())
                        .collect::<String>()
                })
                .collect();
            let text = rows.join("\n");
            if usize::from(content) > super::BREADCRUMB.chars().count() + 11 {
                assert!(
                    rows[0].contains(super::BREADCRUMB),
                    "{terminal}: {:?}",
                    rows[0]
                );
            } else {
                assert!(
                    rows[0].contains(super::SHORT_HEADING),
                    "{terminal}: {:?}",
                    rows[0]
                );
            }
            assert!(rows[0].contains("‹ Back"), "{terminal}: {:?}", rows[0]);
            assert!(text.contains("THEN"), "{terminal}: THEN row missing");
            assert!(text.contains("Matches "), "{terminal}: match count missing");
            assert!(
                text.contains("Custom rule 1"),
                "{terminal}: plain-English footer missing"
            );
            if content >= super::SINGLE_COLUMN_BELOW {
                assert!(text.contains("Rules "), "{terminal}: rules column missing");
                assert!(text.contains("Live tree"), "{terminal}: preview missing");
            }
        }
    }

    #[test]
    fn help_anchor_lines_name_the_focused_column_and_the_then_row() {
        let mut app = crate::app::App::new("anchors".into(), std::env::temp_dir());
        app.custom_status_rules = default_rules();
        app.set_screen_area(Rect::new(0, 0, 160, 40));
        let mut editor = EditorState::new(&app.custom_status_rules);
        let area = Rect::new(20, 4, 120, 30);
        let anchors = super::help_anchor_lines(area, &app, &editor);
        assert_eq!(anchors[0], ("AM-26", 0, false));
        assert_eq!(anchors[1], ("AM-27", 1, true));
        let then = anchors
            .iter()
            .find(|(topic, _, _)| *topic == "AM-29")
            .unwrap();
        let conditions = editor.expression(&app.custom_status_rules).unwrap().0.len() as u16;
        assert_eq!(then.1, 2 + 1 + conditions);
        assert!(!then.2);
        editor.column = super::EditorColumn::Preview;
        let anchors = super::help_anchor_lines(area, &app, &editor);
        assert_eq!(anchors[1].0, "AM-30");
    }

    #[test]
    fn every_help_anchor_of_the_editor_resolves_in_the_catalog() {
        let mut app = crate::app::App::new("anchor-catalog".into(), std::env::temp_dir());
        app.custom_status_rules = default_rules();
        app.set_screen_area(Rect::new(0, 0, 160, 40));
        let area = Rect::new(20, 4, 120, 30);
        for column in [
            super::EditorColumn::Rules,
            super::EditorColumn::ConditionAndVisual,
            super::EditorColumn::Preview,
        ] {
            for fallback_selected in [false, true] {
                let mut editor = EditorState::new(&app.custom_status_rules);
                editor.column = column;
                editor.fallback_selected = fallback_selected;
                for (topic, _, _) in super::help_anchor_lines(area, &app, &editor) {
                    assert!(
                        crate::settings_help::catalog::by_id(topic).is_some(),
                        "{topic} ({column:?}, fallback {fallback_selected}) is missing from the help catalog"
                    );
                }
            }
        }
        // The Settings page links the Configure button to AM-26.
        assert!(crate::settings_help::catalog::by_id("AM-26").is_some());
    }

    #[test]
    fn empty_live_tree_shows_guidance_and_demo_lists_every_core_situation() {
        use ratatui::{backend::TestBackend, Terminal};
        let app = crate::app::App::new("empty".into(), std::env::temp_dir());
        let mut editor = EditorState::new(&app.custom_status_rules);
        editor.column = super::EditorColumn::Preview;
        let mut terminal = Terminal::new(TestBackend::new(60, 30)).unwrap();
        terminal
            .draw(|frame| super::render(frame, frame.area(), &app, &editor))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("No panes in this session yet."));
        editor.preview_tab = super::PreviewTab::Demo;
        terminal
            .draw(|frame| super::render(frame, frame.area(), &app, &editor))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        for situation in crate::status_demo::situations().iter().take(6) {
            assert!(
                text.contains(situation.name),
                "missing {:?}",
                situation.name
            );
        }
    }

    #[test]
    fn condition_window_keeps_selection_and_visual_controls_visible() {
        let rules = default_rules();
        let mut editor = EditorState::new(&rules);
        editor.selected_condition = 12;
        let area = Rect::new(3, 5, 30, 10);
        let (first, visible) = super::condition_window(&editor, area, 20);
        assert!(first <= 12 && 12 < first + visible);
        assert!(super::visual_actions(area, visible).is_some());
        editor.parameter_draft = Some("pattern".into());
        let (_, with_draft) = super::condition_window(&editor, area, 20);
        assert_eq!(with_draft + 1, visible);
    }

    #[test]
    fn rule_actions_exclude_border_and_other_rows() {
        use ratatui::layout::Position;
        let area = Rect::new(3, 5, 24, 10);
        let controls = super::rule_actions(area, 6).unwrap();
        assert_eq!(
            super::rule_action_at(area, 6, Position::new(controls.x, 6)),
            Some(super::RuleAction::Toggle)
        );
        assert_eq!(
            super::rule_action_at(area, 6, Position::new(controls.x + 4, 6)),
            Some(super::RuleAction::Duplicate)
        );
        assert_eq!(
            super::rule_action_at(area, 6, Position::new(controls.x + 5, 6)),
            Some(super::RuleAction::Delete)
        );
        assert_eq!(
            super::rule_action_at(area, 6, Position::new(controls.x + 6, 6)),
            Some(super::RuleAction::Rename)
        );
        assert_eq!(
            super::rule_action_at(area, 6, Position::new(controls.right(), 6)),
            None
        );
        assert_eq!(
            super::rule_action_at(area, 6, Position::new(controls.x, 7)),
            None
        );
        assert!(super::rule_actions(Rect::new(0, 0, 10, 10), 1).is_none());
        let add = super::add_rule_button(area).unwrap();
        assert_eq!(add.y, area.y);
        assert_eq!(add.right(), area.right() - 1);
        assert!(super::add_rule_button(Rect::new(0, 0, 10, 10)).is_none());
        let reset = super::reset_rule_button(area).unwrap();
        assert!(reset.right() < add.x);
        assert!(super::reset_rule_button(Rect::new(0, 0, 22, 10)).is_none());
        let conditions = super::condition_actions(area, 6).unwrap();
        assert_eq!(
            super::condition_action_at(area, 6, Position::new(conditions.x + 2, 6)),
            Some(super::ConditionAction::Not)
        );
        assert_eq!(
            super::condition_action_at(area, 6, Position::new(conditions.x + 6, 6)),
            Some(super::ConditionAction::Edit)
        );
        assert_eq!(
            super::condition_action_at(area, 6, Position::new(conditions.x + 7, 6)),
            Some(super::ConditionAction::Case)
        );
        assert_eq!(
            super::condition_action_at(area, 6, Position::new(conditions.x + 8, 6)),
            Some(super::ConditionAction::Region)
        );
        assert_eq!(
            super::condition_action_at(area, 6, Position::new(conditions.right(), 6)),
            None
        );
        let visual = super::visual_actions(area, 2).unwrap();
        assert_eq!(visual.y, area.y + 4);
        assert_eq!(
            super::visual_action_at(area, 2, Position::new(visual.x, visual.y)),
            Some(super::VisualAction::Type)
        );
        assert_eq!(
            super::visual_action_at(area, 2, Position::new(visual.x + 1, visual.y)),
            None
        );
        assert_eq!(
            super::visual_action_at(area, 2, Position::new(visual.x + 14, visual.y)),
            Some(super::VisualAction::PingPong)
        );
    }

    #[test]
    fn rule_summary_shows_conditions_connectors_and_result() {
        let mut rules = default_rules();
        let mut editor = EditorState::new(&rules);
        assert!(editor.add_rule(&mut rules));
        editor.add_condition(&mut rules).unwrap();
        editor.negate_condition(&mut rules).unwrap();
        editor.toggle_connector(&mut rules).unwrap();
        let summary = super::rule_summary(&rules.rules[editor.selected_rule.unwrap()]);
        assert!(summary.starts_with("IF "));
        assert!(summary.contains(" OR NOT "));
        assert!(summary.contains(" THEN "));
    }

    #[test]
    fn rule_name_must_be_printable_and_nonempty() {
        let mut rules = default_rules();
        let editor = EditorState::new(&rules);
        let original = rules.rules[0].name.clone();
        assert!(editor.set_rule_name(&mut rules, "  ").is_err());
        assert!(editor.set_rule_name(&mut rules, "bad\nname").is_err());
        assert_eq!(rules.rules[0].name, original);
        editor
            .set_rule_name(&mut rules, "  Awaiting approval  ")
            .unwrap();
        assert_eq!(rules.rules[0].name, "Awaiting approval");
    }

    #[test]
    fn section_tabs_cover_compact_header_without_overlapping() {
        let tabs = super::section_tabs(Rect::new(3, 5, 38, 20));
        assert_eq!(tabs[0].1.x, 3);
        assert_eq!(tabs[0].1.right(), tabs[1].1.x);
        assert_eq!(tabs[1].1.right(), tabs[2].1.x);
        assert_eq!(tabs[2].1.right(), 41);
        assert!(tabs.iter().all(|(_, rect)| rect.y == 6 && rect.height == 1));
        assert!(super::section_tabs(Rect::new(0, 0, 10, 1))
            .iter()
            .all(|(_, rect)| rect.height == 0));
    }

    #[test]
    fn preview_preserves_requested_tree_width_when_there_is_room() {
        let wide = layout(Rect::new(0, 0, 130, 30), 32);
        assert_eq!(wide.preview.width, 32);
        let narrow = layout(Rect::new(0, 0, 70, 20), 32);
        assert_eq!(narrow.preview.width, 23);
        assert_eq!(wide.back.right(), 130);
        let compact =
            super::layout_for_column(Rect::new(0, 0, 38, 20), 32, super::EditorColumn::Preview);
        assert_eq!(compact.preview.width, 38);
        assert_eq!(compact.rules.width, 0);
    }

    #[test]
    fn reorder_and_undo_delete_preserve_rule_identity() {
        let mut rules = default_rules();
        let mut editor = EditorState::new(&rules);
        let original_id = rules.rules[0].id.clone();
        assert!(!editor.move_selected(&mut rules, -1));
        assert!(editor.move_selected(&mut rules, 1));
        assert_eq!(rules.rules[1].id, original_id);
        assert!(editor.delete_selected(&mut rules));
        assert!(editor.undo_delete(&mut rules));
        assert_eq!(rules.rules[1].id, original_id);
    }

    #[test]
    fn adding_after_delete_keeps_the_deleted_rule_id_available_for_undo() {
        let mut rules = default_rules();
        let mut editor = EditorState::new(&rules);
        assert!(editor.add_rule(&mut rules));
        let deleted_id = rules.rules.last().unwrap().id.clone();
        assert!(editor.delete_selected(&mut rules));
        assert!(editor.add_rule(&mut rules));
        assert_ne!(rules.rules.last().unwrap().id, deleted_id);
        assert!(editor.undo_delete(&mut rules));
        assert_eq!(
            rules
                .rules
                .iter()
                .filter(|rule| rule.id == deleted_id)
                .count(),
            1
        );
    }

    #[test]
    fn adding_and_editing_a_rule_preserves_expression_on_invalid_input() {
        let mut rules = default_rules();
        let mut editor = EditorState::new(&rules);
        assert!(editor.add_rule(&mut rules));
        let first_id = rules.rules.last().unwrap().id.clone();
        assert!(editor.add_rule(&mut rules));
        assert_ne!(rules.rules.last().unwrap().id, first_id);
        let terms = vec![
            ConditionTerm {
                condition: Condition::GoalSet,
                negated: false,
            },
            ConditionTerm {
                condition: Condition::ActivityStale,
                negated: true,
            },
            ConditionTerm {
                condition: Condition::ScheduledInput,
                negated: false,
            },
        ];
        assert!(editor
            .replace_expression(&mut rules, terms.clone(), &[Connector::Or, Connector::And])
            .is_ok());
        let original = editor.expression(&rules).unwrap();
        assert_eq!(original.1, [Connector::Or, Connector::And]);
        assert!(editor
            .replace_expression(&mut rules, terms, &[Connector::And])
            .is_err());
        assert_eq!(editor.expression(&rules).unwrap(), original);
        assert!(editor.toggle_selected(&mut rules));
        assert!(!rules.rules.last().unwrap().enabled);
    }

    #[test]
    fn condition_controls_preserve_precedence_and_reject_empty_rules() {
        let mut rules = default_rules();
        let mut editor = EditorState::new(&rules);
        assert!(editor.add_rule(&mut rules));
        editor.add_condition(&mut rules).unwrap();
        editor.negate_condition(&mut rules).unwrap();
        editor.toggle_connector(&mut rules).unwrap();
        let (terms, connectors) = editor.expression(&rules).unwrap();
        assert!(terms[1].negated);
        assert_eq!(connectors, [Connector::Or]);
        editor.remove_condition(&mut rules).unwrap();
        assert_eq!(editor.expression(&rules).unwrap().0.len(), 1);
        assert!(editor.remove_condition(&mut rules).is_err());
    }

    #[test]
    fn parameter_edits_validate_before_replacing_the_existing_value() {
        let mut rules = default_rules();
        let mut editor = EditorState::new(&rules);
        editor.selected_rule = Some(rules.rules.len() - 1);
        let original = editor.parameter(&rules).unwrap();
        assert!(editor.set_parameter(&mut rules, "[").is_err());
        assert_eq!(editor.parameter(&rules).unwrap(), original);
        editor.set_parameter(&mut rules, "^waiting").unwrap();
        assert_eq!(editor.parameter(&rules).as_deref(), Some("^waiting"));
        let (mut terms, connectors) = editor.expression(&rules).unwrap();
        terms[0].condition = Condition::ProgressBetween {
            minimum: 20,
            maximum: 80,
        };
        editor
            .replace_expression(&mut rules, terms, &connectors)
            .unwrap();
        assert!(editor.set_parameter(&mut rules, "90,30").is_err());
        assert_eq!(editor.parameter(&rules).as_deref(), Some("20,80"));
        editor.set_parameter(&mut rules, "10,40").unwrap();
        assert_eq!(editor.parameter(&rules).as_deref(), Some("10,40"));
    }

    #[test]
    fn visual_edits_validate_frames_and_colour_before_mutation() {
        use ilium_core::status_rule_expression::VisualKind;
        let mut rules = default_rules();
        let editor = EditorState::new(&rules);
        assert!(editor.cycle_visual(&mut rules));
        assert!(matches!(&rules.rules[0].then.kind, VisualKind::Glyph(_)));
        editor.set_visual_value(&mut rules, "★").unwrap();
        assert!(editor.set_visual_value(&mut rules, "\n").is_err());
        assert_eq!(editor.visual_value(&rules).as_deref(), Some("★"));
        editor.set_colour(&mut rules, "#aabbcc").unwrap();
        assert!(editor.set_colour(&mut rules, "not-a-colour").is_err());
        assert_eq!(rules.rules[0].then.style.color.as_deref(), Some("#aabbcc"));
        assert!(editor.cycle_visual(&mut rules));
        assert!(editor.adjust_animation_speed(&mut rules, 1));
        assert!(editor.cycle_visual(&mut rules));
        assert!(editor.toggle_ping_pong(&mut rules));
        assert!(editor.set_visual_value(&mut rules, "a|🌕").is_err());
    }

    #[test]
    fn the_visual_picker_steps_through_every_frame_family() {
        use crate::status_rule_visual::{frame_family_of, FRAME_FAMILIES};
        use ilium_core::status_rule_expression::VisualKind;
        let mut rules = default_rules();
        let editor = EditorState::new(&rules);
        rules.rules[0].then.kind = VisualKind::BrailleAnimation {
            preset: "spinner".into(),
            frames_per_second: 8,
        };
        for family in FRAME_FAMILIES {
            assert!(editor.cycle_visual(&mut rules));
            let VisualKind::FrameAnimation { frames, .. } = &rules.rules[0].then.kind else {
                panic!("expected the {} family", family.name);
            };
            assert_eq!(
                frame_family_of(frames).map(|found| found.name),
                Some(family.name)
            );
        }
        assert!(editor.cycle_visual(&mut rules));
        assert!(matches!(
            rules.rules[0].then.kind,
            VisualKind::ProgressFrames { .. }
        ));
        // Hand-typed frames move straight on to progress frames.
        rules.rules[0].then.kind = VisualKind::FrameAnimation {
            frames: vec!["a".into(), "b".into()],
            frames_per_second: 4,
            ping_pong: false,
        };
        assert!(editor.cycle_visual(&mut rules));
        assert!(matches!(
            rules.rules[0].then.kind,
            VisualKind::ProgressFrames { .. }
        ));
    }

    #[test]
    fn live_preview_renders_the_actual_session_tree() {
        use ratatui::{backend::TestBackend, Terminal};
        let mut app = crate::app::App::new("preview".into(), std::env::temp_dir());
        let group = app
            .tree
            .add_group(ilium_core::ROOT_ID, "Unique live project")
            .unwrap();
        // A session with no panes shows guidance instead of a tree.
        app.tree
            .add_pane(group, "agent one", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        let editor = EditorState::new(&app.custom_status_rules);
        let mut terminal = Terminal::new(TestBackend::new(130, 24)).unwrap();
        terminal
            .draw(|frame| super::render(frame, frame.area(), &app, &editor))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Unique live project"));
        assert!(!text.contains("Live preview integration pending"));
    }
}

#[cfg(test)]
mod fallback_and_picker_geometry_tests {
    use super::{fallback_row, picker_area, visible_rule_rows, EditorLayout, EditorState};
    use ilium_core::status_rule_expression::default_rules;
    use ratatui::layout::Rect;

    #[test]
    fn fallback_row_sits_under_the_list_and_takes_two_rows_from_it() {
        let area = Rect::new(0, 2, 40, 13);
        assert_eq!(fallback_row(area), Some(area.bottom() - 2));
        assert!(fallback_row(Rect::new(0, 0, 40, 6)).is_none());
        assert_eq!(visible_rule_rows(13), 3);
    }

    #[test]
    fn list_overlay_uses_the_widest_column_and_prefers_the_editor() {
        let regions = EditorLayout {
            rules: Rect::new(0, 1, 30, 20),
            editor: Rect::new(30, 1, 30, 20),
            preview: Rect::new(60, 1, 20, 20),
            back: Rect::new(70, 0, 8, 1),
        };
        assert_eq!(picker_area(&regions), regions.editor);
        let wide = EditorLayout {
            rules: Rect::new(0, 1, 50, 20),
            ..regions
        };
        assert_eq!(picker_area(&wide), wide.rules);
    }

    #[test]
    fn fallback_selection_has_no_conditions_and_edits_only_the_fallback_icon() {
        let mut rules = default_rules();
        let mut editor = EditorState::new(&rules);
        editor.select_fallback();
        assert!(editor.expression(&rules).is_err());
        let before = rules.rules.clone();
        assert!(editor.cycle_visual(&mut rules));
        assert_eq!(rules.rules, before);
        assert!(rules.fallback.is_some());
    }
}
