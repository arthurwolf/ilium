//! State and geometry for the custom status-icon rules editor.

use ilium_core::status_rule_expression::{
    AllOf, Condition, ConditionTerm, Connector, Rule, StatusRules, StatusVisual, VisualKind,
    VisualStyle, from_line, to_line,
};
use ratatui::layout::Rect;
use ratatui::{Frame, style::{Modifier, Style}, text::{Line, Span}, widgets::{Block, Borders, Paragraph}};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PreviewTab {
    #[default]
    Live,
    Demo,
}

const DEMO_SCENARIOS: [&str; 10] = [
    "Working · goal active", "Needs approval", "Idle · goal paused", "Waiting for subagents",
    "Idle · no goal", "Idle · task 50%", "Idle · task failed", "Idle · monitor failed",
    "Idle · task done unread", "Idle · turn done unread",
];

pub fn preview_tabs(area: Rect) -> [(PreviewTab, Rect); 2] {
    let left = area.width / 2;
    let height = u16::from(area.height > 0);
    [
        (PreviewTab::Live, Rect::new(area.x, area.y, left, height)),
        (PreviewTab::Demo, Rect::new(area.x.saturating_add(left), area.y, area.width - left, height)),
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
pub enum DraftTarget { Condition, Visual, Colour, RuleName }

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct EditorLayout {
    pub rules: Rect,
    pub editor: Rect,
    pub preview: Rect,
    pub back: Rect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuleAction { Toggle, Up, Down, Edit, Delete, Rename }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConditionAction { Previous, Next, Not, Connector, Add, Delete, Edit, Case }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VisualAction { Type, Value, Colour, Bold, Dim, Slower, Faster, PingPong }

pub fn visual_actions(area: Rect, condition_count: usize) -> Option<Rect> {
    let row = area.y.saturating_add(2).saturating_add(condition_count as u16);
    if area.width < 17 || row >= area.bottom().saturating_sub(1) { return None; }
    Some(Rect::new(area.x.saturating_add(1), row, 15, 1))
}

pub fn visual_action_at(area: Rect, count: usize, position: ratatui::layout::Position) -> Option<VisualAction> {
    let rect = visual_actions(area, count)?;
    if !rect.contains(position) { return None; }
    match position.x - rect.x {
        0 => Some(VisualAction::Type), 2 => Some(VisualAction::Value),
        4 => Some(VisualAction::Colour), 6 => Some(VisualAction::Bold),
        8 => Some(VisualAction::Dim), 10 => Some(VisualAction::Slower),
        12 => Some(VisualAction::Faster), 14 => Some(VisualAction::PingPong),
        _ => None,
    }
}

pub fn condition_actions(area: Rect, row: u16) -> Option<Rect> {
    if area.width < 19 || row <= area.y || row >= area.bottom().saturating_sub(1) { return None; }
    Some(Rect::new(area.right().saturating_sub(9), row, 8, 1))
}

pub fn condition_action_at(area: Rect, row: u16, position: ratatui::layout::Position) -> Option<ConditionAction> {
    let rect = condition_actions(area, row)?;
    if !rect.contains(position) { return None; }
    match position.x - rect.x {
        0 => Some(ConditionAction::Previous), 1 => Some(ConditionAction::Next),
        2 => Some(ConditionAction::Not), 3 => Some(ConditionAction::Connector),
        4 => Some(ConditionAction::Add), 5 => Some(ConditionAction::Delete),
        6 => Some(ConditionAction::Edit), 7 => Some(ConditionAction::Case), _ => None,
    }
}

pub fn add_rule_button(area: Rect) -> Option<Rect> {
    if area.width < 12 || area.height < 2 { return None; }
    Some(Rect::new(area.right().saturating_sub(7), area.y, 6, 1))
}

pub fn reset_rule_button(area: Rect) -> Option<Rect> {
    if area.width < 23 || area.height < 2 { return None; }
    Some(Rect::new(area.right().saturating_sub(15), area.y, 7, 1))
}

/// Controls occupy the last six cells of a rule's first row. Returning no
/// rectangle on compact or clipped rows keeps borders and other targets inert.
pub fn rule_actions(area: Rect, row: u16) -> Option<Rect> {
    if area.width < 16 || row <= area.y || row >= area.bottom().saturating_sub(1) {
        return None;
    }
    Some(Rect::new(area.right().saturating_sub(7), row, 6, 1))
}

pub fn rule_action_at(area: Rect, row: u16, position: ratatui::layout::Position) -> Option<RuleAction> {
    let rect = rule_actions(area, row)?;
    if !rect.contains(position) { return None; }
    match position.x - rect.x {
        0 => Some(RuleAction::Toggle), 1 => Some(RuleAction::Up),
        2 => Some(RuleAction::Down), 3 => Some(RuleAction::Edit),
        4 => Some(RuleAction::Delete), 5 => Some(RuleAction::Rename), _ => None,
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
        (EditorColumn::ConditionAndVisual, Rect::new(area.x.saturating_add(first), row, second, height)),
        (EditorColumn::Preview, Rect::new(area.x.saturating_add(first).saturating_add(second), row, area.width - first - second, height)),
    ]
}

pub fn first_visible_rule(state: &EditorState, height: u16) -> usize {
    let visible = usize::from(height.saturating_sub(2) / 2).max(1);
    state.selected_rule.unwrap_or(0).saturating_sub(visible - 1)
}

/// Keep the selected condition visible and reserve rows for the visual
/// controls and any active draft or error message.
pub fn condition_window(state: &EditorState, area: Rect, count: usize) -> (usize, usize) {
    let message_rows = usize::from(state.parameter_draft.is_some()) + usize::from(state.save_error.is_some());
    let available = usize::from(area.height.saturating_sub(4)).saturating_sub(message_rows);
    let visible = count.min(available);
    let first = if visible == 0 { 0 } else {
        state.selected_condition.min(count.saturating_sub(1)).saturating_sub(visible - 1)
    };
    (first, visible)
}

/// Preserve the configured tree width whenever the available area allows it.
/// At narrow widths each pane still has a usable rectangle, and the caller
/// may present the active pane on its own when the screen is too small.
pub fn layout(area: Rect, desired_tree_width: u16) -> EditorLayout {
    let back_width = area.width.min(10);
    let back = Rect::new(area.right().saturating_sub(back_width), area.y, back_width, u16::from(area.height > 0));
    let content_y = area.y.saturating_add(2).min(area.bottom());
    let content_height = area.bottom().saturating_sub(content_y);
    let gaps = 2u16.min(area.width);
    let available = area.width.saturating_sub(gaps);
    let minimum_editor = 25u16;
    let minimum_rules = 20u16;
    let preview_width = desired_tree_width.min(available.saturating_sub(minimum_editor + minimum_rules));
    let rest = available.saturating_sub(preview_width);
    let rules_width = (rest / 2).max(minimum_rules.min(rest));
    let editor_width = rest.saturating_sub(rules_width);
    let rules = Rect::new(area.x, content_y, rules_width, content_height);
    let editor = Rect::new(rules.right().saturating_add(u16::from(gaps > 0)), content_y, editor_width, content_height);
    let preview = Rect::new(editor.right().saturating_add(u16::from(gaps > 1)), content_y, preview_width, content_height);
    EditorLayout { rules, editor, preview, back }
}

/// On very narrow terminals present one selectable column at a time instead
/// of giving the preview zero cells or placing pointer targets off-screen.
pub fn layout_for_column(area: Rect, desired_tree_width: u16, column: EditorColumn) -> EditorLayout {
    let mut regions = layout(area, desired_tree_width);
    if area.width < 68 {
        let content = Rect::new(area.x, area.y.saturating_add(2).min(area.bottom()), area.width, area.height.saturating_sub(2));
        let empty = Rect::new(area.x, content.y, 0, 0);
        regions.rules = if column == EditorColumn::Rules { content } else { empty };
        regions.editor = if column == EditorColumn::ConditionAndVisual { content } else { empty };
        regions.preview = if column == EditorColumn::Preview { content } else { empty };
    }
    regions
}

#[derive(Debug, Clone)]
pub struct EditorState {
    pub selected_rule: Option<usize>,
    pub column: EditorColumn,
    pub preview_tab: PreviewTab,
    pub demo_scroll: u16,
    pub selected_condition: usize,
    /// A deleted rule is kept for the session's undo action.
    pub deleted: Option<(usize, Rule<Condition, StatusVisual>)>,
    pub save_error: Option<String>,
    pub confirm_reset: bool,
    pub parameter_draft: Option<String>,
    pub draft_target: DraftTarget,
}

impl EditorState {
    pub fn new(rules: &StatusRules) -> Self {
        Self {
            selected_rule: (!rules.rules.is_empty()).then_some(0),
            column: EditorColumn::Rules,
            preview_tab: PreviewTab::Live,
            demo_scroll: 0,
            selected_condition: 0,
            deleted: None,
            save_error: None,
            confirm_reset: false,
            parameter_draft: None,
            draft_target: DraftTarget::Condition,
        }
    }

    pub fn scroll_demo(&mut self, direction: i32) {
        self.demo_scroll = if direction < 0 {
            self.demo_scroll.saturating_sub(1)
        } else if direction > 0 {
            self.demo_scroll.saturating_add(1).min(DEMO_SCENARIOS.len() as u16)
        } else {
            self.demo_scroll
        };
    }

    pub fn move_selected(&mut self, rules: &mut StatusRules, direction: i32) -> bool {
        let Some(index) = self.selected_rule.filter(|index| *index < rules.rules.len()) else { return false; };
        let destination = if direction < 0 { index.checked_sub(1) } else if direction > 0 { index.checked_add(1).filter(|next| *next < rules.rules.len()) } else { None };
        let Some(destination) = destination else { return false; };
        rules.rules.swap(index, destination);
        self.selected_rule = Some(destination);
        true
    }

    pub fn delete_selected(&mut self, rules: &mut StatusRules) -> bool {
        let Some(index) = self.selected_rule.filter(|index| *index < rules.rules.len()) else { return false; };
        self.deleted = Some((index, rules.rules.remove(index)));
        self.selected_rule = (!rules.rules.is_empty()).then(|| index.min(rules.rules.len() - 1));
        true
    }

    pub fn undo_delete(&mut self, rules: &mut StatusRules) -> bool {
        let Some((index, rule)) = self.deleted.take() else { return false; };
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
        let mut number = 1usize;
        while rules.rules.iter().any(|rule| rule.id == format!("custom-{number}"))
            || self.deleted.as_ref().is_some_and(|(_, rule)| rule.id == format!("custom-{number}"))
        {
            number += 1;
        }
        rules.rules.push(Rule {
            id: format!("custom-{number}"),
            name: format!("Custom rule {number}"),
            enabled: true,
            any_of: vec![AllOf { all_of: vec![ConditionTerm { condition: Condition::GoalSet, negated: false }] }],
            then: StatusVisual { kind: VisualKind::Glyph("●".to_owned()), style: VisualStyle::default() },
        });
        self.selected_rule = Some(rules.rules.len() - 1);
        true
    }

    pub fn toggle_selected(&self, rules: &mut StatusRules) -> bool {
        let Some(rule) = self.selected_rule.and_then(|index| rules.rules.get_mut(index)) else { return false; };
        rule.enabled = !rule.enabled;
        true
    }

    pub fn set_rule_name(&self, rules: &mut StatusRules, input: &str) -> Result<(), String> {
        let name = input.trim();
        if name.is_empty() || name.chars().count() > 48 || name.chars().any(char::is_control) {
            return Err("Enter a name of 1 to 48 printable characters".into());
        }
        let rule = self.selected_rule.and_then(|index| rules.rules.get_mut(index)).ok_or("Select a rule first")?;
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
        if groups.is_empty() || groups.len() > 8 || groups.iter().any(|group| group.all_of.len() > 8) {
            return Err("Choose at least one condition and at most eight conditions per group");
        }
        let rule = self.selected_rule.and_then(|index| rules.rules.get_mut(index)).ok_or("Select a rule first")?;
        rule.any_of = groups;
        Ok(())
    }

    pub fn expression(&self, rules: &StatusRules) -> Result<(Vec<ConditionTerm<Condition>>, Vec<Connector>), &'static str> {
        let rule = self.selected_rule.and_then(|index| rules.rules.get(index)).ok_or("Select a rule first")?;
        to_line(&rule.any_of)
    }

    pub fn add_condition(&mut self, rules: &mut StatusRules) -> Result<(), &'static str> {
        let (mut terms, mut connectors) = self.expression(rules)?;
        let index = self.selected_condition.min(terms.len().saturating_sub(1)) + 1;
        terms.insert(index, ConditionTerm { condition: Condition::GoalSet, negated: false });
        connectors.insert(index - 1, Connector::And);
        self.replace_expression(rules, terms, &connectors)?;
        self.selected_condition = index;
        Ok(())
    }

    pub fn remove_condition(&mut self, rules: &mut StatusRules) -> Result<(), &'static str> {
        let (mut terms, mut connectors) = self.expression(rules)?;
        if terms.len() <= 1 { return Err("A rule needs at least one condition"); }
        let index = self.selected_condition.min(terms.len() - 1);
        terms.remove(index);
        connectors.remove(index.saturating_sub(1));
        self.replace_expression(rules, terms, &connectors)?;
        self.selected_condition = index.min(self.expression(rules)?.0.len() - 1);
        Ok(())
    }

    pub fn negate_condition(&self, rules: &mut StatusRules) -> Result<(), &'static str> {
        let (mut terms, connectors) = self.expression(rules)?;
        let term = terms.get_mut(self.selected_condition).ok_or("Select a condition")?;
        term.negated = !term.negated;
        self.replace_expression(rules, terms, &connectors)
    }

    pub fn toggle_connector(&self, rules: &mut StatusRules) -> Result<(), &'static str> {
        let (terms, mut connectors) = self.expression(rules)?;
        let index = self.selected_condition.checked_sub(1).ok_or("The first condition has no preceding connector")?;
        let connector = connectors.get_mut(index).ok_or("Select a condition")?;
        *connector = match connector { Connector::And => Connector::Or, Connector::Or => Connector::And };
        self.replace_expression(rules, terms, &connectors)
    }

    pub fn cycle_condition(&self, rules: &mut StatusRules, direction: i32) -> Result<(), &'static str> {
        let catalog = condition_catalog();
        let (mut terms, connectors) = self.expression(rules)?;
        let term = terms.get_mut(self.selected_condition).ok_or("Select a condition")?;
        // Parameter changes do not change the condition's place in the
        // catalog. Compare the variant for editable conditions and the full
        // value for states such as individual goal or agent states.
        let current = catalog.iter().position(|condition| {
            match (condition, &term.condition) {
                (Condition::ScreenMatches(_), Condition::ScreenMatches(_))
                | (Condition::MonitorCountAtLeast(_), Condition::MonitorCountAtLeast(_))
                | (Condition::ProgressAtLeast(_), Condition::ProgressAtLeast(_))
                | (Condition::ProgressBelow(_), Condition::ProgressBelow(_))
                | (Condition::ProgressBetween { .. }, Condition::ProgressBetween { .. }) => true,
                _ => *condition == term.condition,
            }
        }).ok_or("This condition is not available in the selector")?;
        let next = (current as i32 + direction.signum()).rem_euclid(catalog.len() as i32) as usize;
        term.condition = catalog[next].clone();
        self.replace_expression(rules, terms, &connectors)
    }

    pub fn parameter(&self, rules: &StatusRules) -> Option<String> {
        let (terms, _) = self.expression(rules).ok()?;
        match &terms.get(self.selected_condition)?.condition {
            Condition::ScreenMatches(pattern) => Some(pattern.pattern.clone()),
            Condition::MonitorCountAtLeast(value) => Some(value.to_string()),
            Condition::ProgressAtLeast(value) | Condition::ProgressBelow(value) => Some(value.to_string()),
            Condition::ProgressBetween { minimum, maximum } => Some(format!("{minimum},{maximum}")),
            _ => None,
        }
    }

    pub fn set_parameter(&self, rules: &mut StatusRules, input: &str) -> Result<(), String> {
        let (mut terms, connectors) = self.expression(rules).map_err(str::to_owned)?;
        let term = terms.get_mut(self.selected_condition).ok_or("Select a condition")?;
        match &mut term.condition {
            Condition::ScreenMatches(pattern) => {
                if input.chars().count() > 256 { return Err("Pattern must have at most 256 characters".into()); }
                regex::RegexBuilder::new(input).case_insensitive(pattern.ignore_case).multi_line(true)
                    .size_limit(1 << 20).dfa_size_limit(1 << 20).build().map_err(|error| error.to_string())?;
                pattern.pattern = input.to_owned();
            }
            Condition::MonitorCountAtLeast(value) => *value = input.parse().map_err(|_| "Enter a nonnegative monitor count")?,
            Condition::ProgressAtLeast(value) | Condition::ProgressBelow(value) => {
                let parsed: u8 = input.parse().map_err(|_| "Enter a percentage from 0 to 100")?;
                if parsed > 100 { return Err("Enter a percentage from 0 to 100".into()); }
                *value = parsed;
            }
            Condition::ProgressBetween { minimum, maximum } => {
                let (low, high) = input.split_once(',').ok_or("Enter minimum,maximum")?;
                let low: u8 = low.trim().parse().map_err(|_| "Enter minimum,maximum from 0 to 100")?;
                let high: u8 = high.trim().parse().map_err(|_| "Enter minimum,maximum from 0 to 100")?;
                if low > high || high > 100 { return Err("Minimum must not exceed maximum; both must be at most 100".into()); }
                *minimum = low; *maximum = high;
            }
            _ => return Err("This condition has no editable parameter".into()),
        }
        self.replace_expression(rules, terms, &connectors).map_err(str::to_owned)
    }

    pub fn toggle_screen_case(&self, rules: &mut StatusRules) -> Result<(), &'static str> {
        let (mut terms, connectors) = self.expression(rules)?;
        let term = terms.get_mut(self.selected_condition).ok_or("Select a condition")?;
        let Condition::ScreenMatches(pattern) = &mut term.condition else { return Err("Select a screen pattern"); };
        pattern.ignore_case = !pattern.ignore_case;
        self.replace_expression(rules, terms, &connectors)
    }

    pub fn cycle_visual(&self, rules: &mut StatusRules) -> bool {
        let Some(rule) = self.selected_rule.and_then(|index| rules.rules.get_mut(index)) else { return false; };
        rule.then.kind = match &rule.then.kind {
            VisualKind::Hidden => VisualKind::BuiltIn("working".into()),
            VisualKind::BuiltIn(_) => VisualKind::Glyph("●".into()),
            VisualKind::Glyph(_) => VisualKind::BrailleAnimation { preset: "spinner".into(), frames_per_second: 8 },
            VisualKind::BrailleAnimation { .. } => VisualKind::FrameAnimation { frames: ["◐", "◓", "◑", "◒"].into_iter().map(str::to_owned).collect(), frames_per_second: 4, ping_pong: false },
            VisualKind::FrameAnimation { .. } => VisualKind::ProgressFrames { frames: ["🌑", "🌒", "🌓", "🌔", "🌕"].into_iter().map(str::to_owned).collect() },
            VisualKind::ProgressFrames { .. } => VisualKind::Hidden,
        };
        true
    }

    pub fn visual_value(&self, rules: &StatusRules) -> Option<String> {
        let visual = &rules.rules.get(self.selected_rule?)?.then;
        match &visual.kind {
            VisualKind::Hidden => None,
            VisualKind::BuiltIn(key) | VisualKind::Glyph(key) => Some(key.clone()),
            VisualKind::BrailleAnimation { preset, .. } => Some(preset.clone()),
            VisualKind::FrameAnimation { frames, .. } | VisualKind::ProgressFrames { frames } => Some(frames.join("|")),
        }
    }

    pub fn set_visual_value(&self, rules: &mut StatusRules, input: &str) -> Result<(), String> {
        let rule = self.selected_rule.and_then(|index| rules.rules.get_mut(index)).ok_or("Select a rule")?;
        let mut visual = rule.then.clone();
        match &mut visual.kind {
            VisualKind::Hidden => return Err("Choose a visible icon type first".into()),
            VisualKind::BuiltIn(key) | VisualKind::Glyph(key) => *key = input.to_owned(),
            VisualKind::BrailleAnimation { preset, .. } => *preset = input.to_owned(),
            VisualKind::FrameAnimation { frames, .. } | VisualKind::ProgressFrames { frames } => *frames = input.split('|').map(str::to_owned).collect(),
        }
        crate::status_rule_visual::validate_visual(&visual, 2).map_err(str::to_owned)?;
        rule.then = visual;
        Ok(())
    }

    pub fn set_colour(&self, rules: &mut StatusRules, input: &str) -> Result<(), String> {
        let rule = self.selected_rule.and_then(|index| rules.rules.get_mut(index)).ok_or("Select a rule")?;
        let mut visual = rule.then.clone();
        visual.style.color = (!input.is_empty()).then(|| input.to_owned());
        crate::status_rule_visual::validate_visual(&visual, 2).map_err(str::to_owned)?;
        rule.then = visual;
        Ok(())
    }

    pub fn toggle_weight(&self, rules: &mut StatusRules, bold: bool) -> bool {
        let Some(rule) = self.selected_rule.and_then(|index| rules.rules.get_mut(index)) else { return false; };
        if bold { rule.then.style.bold = !rule.then.style.bold; } else { rule.then.style.dim = !rule.then.style.dim; }
        true
    }

    pub fn adjust_animation_speed(&self, rules: &mut StatusRules, direction: i8) -> bool {
        let Some(rule) = self.selected_rule.and_then(|index| rules.rules.get_mut(index)) else { return false; };
        let frames_per_second = match &mut rule.then.kind {
            VisualKind::BrailleAnimation { frames_per_second, .. } | VisualKind::FrameAnimation { frames_per_second, .. } => frames_per_second,
            _ => return false,
        };
        let updated = (i16::from(*frames_per_second) + i16::from(direction.signum())).clamp(1, 20) as u8;
        let changed = *frames_per_second != updated;
        *frames_per_second = updated;
        changed
    }

    pub fn toggle_ping_pong(&self, rules: &mut StatusRules) -> bool {
        let Some(rule) = self.selected_rule.and_then(|index| rules.rules.get_mut(index)) else { return false; };
        let VisualKind::FrameAnimation { ping_pong, .. } = &mut rule.then.kind else { return false; };
        *ping_pong = !*ping_pong;
        true
    }
}

/// A starter value for each supported condition; parameter editors can then
/// refine thresholds and patterns without changing the condition's identity.
pub fn condition_catalog() -> Vec<Condition> {
    use ilium_core::{AgentClass, AgentTurn, GoalState, ProgressTaskStatus};
    use ilium_core::status_rule_expression::{AvailabilityKind, PaneKind, ScreenPattern, ScreenRegion};
    vec![
        Condition::Always, Condition::PaneKind(PaneKind::Agent), Condition::PaneKind(PaneKind::AgentUnavailable),
        Condition::PaneKind(PaneKind::PlainShell), Condition::PaneKind(PaneKind::Editor), Condition::EditorDirty,
        Condition::PaneKind(PaneKind::Board), Condition::AgentClass(AgentClass::Claude), Condition::AgentClass(AgentClass::Codex),
        Condition::AgentClass(AgentClass::Antigravity), Condition::AgentAvailability(AvailabilityKind::Unverified),
        Condition::AgentAvailability(AvailabilityKind::ShellForeground), Condition::AgentAvailability(AvailabilityKind::Exited),
        Condition::AgentAvailability(AvailabilityKind::ExitSuccess), Condition::AgentAvailability(AvailabilityKind::ExitFailure),
        Condition::AgentTurn(AgentTurn::Working), Condition::AgentTurn(AgentTurn::WaitingApproval),
        Condition::AgentTurn(AgentTurn::WaitingSubagents), Condition::AgentTurn(AgentTurn::Settling), Condition::AgentTurn(AgentTurn::Idle),
        Condition::GoalSet, Condition::GoalState(GoalState::Active), Condition::GoalState(GoalState::Paused),
        Condition::GoalState(GoalState::Blocked), Condition::GoalState(GoalState::UsageLimited), Condition::GoalState(GoalState::Reached),
        Condition::CompletionUnread, Condition::ActivityStale, Condition::MonitorCountAtLeast(1),
        Condition::TaskStatus(ProgressTaskStatus::NotStartedYet), Condition::TaskStatus(ProgressTaskStatus::Running),
        Condition::TaskStatus(ProgressTaskStatus::Error), Condition::TaskStatus(ProgressTaskStatus::Done),
        Condition::AnyTaskStatus(ProgressTaskStatus::NotStartedYet), Condition::AnyTaskStatus(ProgressTaskStatus::Running),
        Condition::AnyTaskStatus(ProgressTaskStatus::Error), Condition::AnyTaskStatus(ProgressTaskStatus::Done), Condition::AnyMonitorFailed,
        Condition::MonitorHealthy, Condition::MonitorDegraded, Condition::MonitorFailed, Condition::MonitorUnread,
        Condition::ProgressMeasurable, Condition::ProgressAtLeast(50), Condition::ProgressBelow(50),
        Condition::ProgressBetween { minimum: 25, maximum: 75 }, Condition::Parked, Condition::ScheduledInput,
        Condition::ShellOutputRecent, Condition::ShellOutputFast, Condition::ShellOutputSlow,
        Condition::ScreenMatches(ScreenPattern { pattern: "error".to_owned(), ignore_case: true, region: ScreenRegion::All }),
    ]
}

/// Human-readable condition text shared by the list and selected rule.
pub fn condition_label(condition: &Condition) -> String {
    use ilium_core::{AgentTurn, GoalState, ProgressTaskStatus};
    match condition {
        Condition::Always => "always".into(),
        Condition::PaneKind(kind) => format!("pane is {kind:?}"),
        Condition::EditorDirty => "editor has unsaved changes".into(),
        Condition::AgentClass(class) => format!("agent is {class:?}"),
        Condition::AgentAvailability(availability) => format!("agent availability is {availability:?}"),
        Condition::AgentTurn(turn) => match turn {
            AgentTurn::Working => "agent is working", AgentTurn::WaitingApproval => "agent needs approval",
            AgentTurn::WaitingSubagents => "agent is waiting for subagents", AgentTurn::Settling => "agent is settling",
            AgentTurn::Idle => "agent is idle",
        }.into(),
        Condition::GoalSet => "goal is set".into(),
        Condition::GoalState(goal) => format!("goal is {}", match goal {
            GoalState::Active => "active", GoalState::Paused => "paused", GoalState::Blocked => "blocked",
            GoalState::UsageLimited => "usage limited", GoalState::Reached => "reached",
        }),
        Condition::CompletionUnread => "completion is unread".into(),
        Condition::ActivityStale => "activity check is stale".into(),
        Condition::MonitorCountAtLeast(count) => format!("at least {count} monitors"),
        Condition::TaskStatus(status) => format!("selected monitored task is {}", match status {
            ProgressTaskStatus::NotStartedYet => "pending", ProgressTaskStatus::Running => "running",
            ProgressTaskStatus::Error => "failed", ProgressTaskStatus::Done => "done",
        }),
        Condition::AnyTaskStatus(status) => format!("any monitored task is {}", match status {
            ProgressTaskStatus::NotStartedYet => "pending", ProgressTaskStatus::Running => "running",
            ProgressTaskStatus::Error => "failed", ProgressTaskStatus::Done => "done",
        }),
        Condition::MonitorHealthy => "monitor is healthy".into(),
        Condition::MonitorDegraded => "monitor is degraded".into(),
        Condition::MonitorFailed => "monitor has failed".into(),
        Condition::AnyMonitorFailed => "any monitor has failed".into(),
        Condition::MonitorUnread => "monitor result is unread".into(),
        Condition::ProgressMeasurable => "progress is measurable".into(),
        Condition::ProgressAtLeast(value) => format!("progress is at least {value}%"),
        Condition::ProgressBelow(value) => format!("progress is below {value}%"),
        Condition::ProgressBetween { minimum, maximum } => format!("progress is {minimum}–{maximum}%"),
        Condition::Parked => "agent is parked".into(),
        Condition::ScheduledInput => "input is scheduled".into(),
        Condition::ShellOutputRecent => "shell has recent output".into(),
        Condition::ShellOutputFast => "shell output is fast".into(),
        Condition::ShellOutputSlow => "shell output is slow".into(),
        Condition::ScreenMatches(pattern) => format!("visible screen matches /{}/{}", pattern.pattern, if pattern.ignore_case { "i" } else { "" }),
    }
}

fn visual_label(visual: &VisualKind) -> String {
    match visual {
        VisualKind::Hidden => "no icon".into(),
        VisualKind::BuiltIn(key) => key.replace('-', " "),
        VisualKind::Glyph(glyph) => glyph.clone(),
        VisualKind::BrailleAnimation { .. } => "braille animation".into(),
        VisualKind::FrameAnimation { frames, .. } => format!("{} animation", frames.first().map_or("", String::as_str)),
        VisualKind::ProgressFrames { frames } => format!("{} progress", frames.first().map_or("", String::as_str)),
    }
}

fn rule_summary(rule: &Rule<Condition, StatusVisual>) -> String {
    let Ok((terms, connectors)) = to_line(&rule.any_of) else { return "Invalid conditions".into(); };
    let mut summary = String::from("IF ");
    for (index, term) in terms.iter().enumerate() {
        if index > 0 {
            summary.push_str(if connectors[index - 1] == Connector::And { " AND " } else { " OR " });
        }
        if term.negated { summary.push_str("NOT "); }
        summary.push_str(&condition_label(&term.condition));
    }
    summary.push_str(" THEN ");
    summary.push_str(&visual_label(&rule.then.kind));
    summary
}

/// Paint the editor using the same geometry used by pointer hit tests.
pub fn render(frame: &mut Frame<'_>, area: Rect, app: &crate::app::App, state: &EditorState) {
    let desired = app.ui_settings.left_panel_sizing.resolved_width(app.layout.screen_area.width, true);
    let regions = layout_for_column(area, desired, state.column);
    let heading = if state.confirm_reset { "Reset rules? Y confirms · Esc cancels" } else { "Custom status icons" };
    frame.render_widget(Paragraph::new(heading).style(Style::new().add_modifier(Modifier::BOLD)), Rect::new(area.x, area.y, area.width.saturating_sub(regions.back.width), 1));
    frame.render_widget(Paragraph::new("‹ Back").style(crate::theme::selected_style()), regions.back);
    if area.height > 1 {
        for (column, rect) in section_tabs(area) {
            let label = match column { EditorColumn::Rules => " Rules ", EditorColumn::ConditionAndVisual => " Edit ", EditorColumn::Preview => " Preview " };
            let style = if column == state.column { crate::theme::selected_style().add_modifier(Modifier::BOLD) } else { Style::new() };
            frame.render_widget(Paragraph::new(label).style(style), rect);
        }
    }
    let mut lines = Vec::new();
    let first = first_visible_rule(state, regions.rules.height);
    let visible = usize::from(regions.rules.height.saturating_sub(2) / 2).max(1);
    for (index, rule) in app.custom_status_rules.rules.iter().enumerate().skip(first).take(visible) {
        let marker = if rule.enabled { "●" } else { "○" };
        let style = if state.selected_rule == Some(index) { crate::theme::selected_style().add_modifier(Modifier::BOLD) } else { Style::new() };
        lines.push(Line::from(Span::styled(format!("{marker} {}", rule.name), style)));
        lines.push(Line::from(Span::styled(rule_summary(rule), style.add_modifier(Modifier::DIM))));
    }
    frame.render_widget(Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Rules ")), regions.rules);
    if let Some(button) = add_rule_button(regions.rules) {
        frame.render_widget(Paragraph::new("+ Add ").style(crate::theme::selected_style()), button);
    }
    if let Some(button) = reset_rule_button(regions.rules) {
        frame.render_widget(Paragraph::new(if state.confirm_reset { "Confirm" } else { "Reset  " }).style(crate::theme::selected_style()), button);
    }
    for (offset, rule) in app.custom_status_rules.rules.iter().skip(first).take(visible).enumerate() {
        let row = regions.rules.y.saturating_add(1).saturating_add((offset as u16).saturating_mul(2));
        if let Some(rect) = rule_actions(regions.rules, row) {
            let enabled = if rule.enabled { "●" } else { "○" };
            frame.render_widget(Paragraph::new(format!("{enabled}↑↓✎×N")), rect);
        }
    }
    let mut details = Vec::new();
    if let Some(rule) = state.selected_rule.and_then(|index| app.custom_status_rules.rules.get(index)) {
        if let Ok((terms, connectors)) = state.expression(&app.custom_status_rules) {
            let (first, visible) = condition_window(state, regions.editor, terms.len());
            for (index, term) in terms.iter().enumerate().skip(first).take(visible) {
                let prefix = if index == 0 { "IF" } else if connectors[index - 1] == Connector::And { "AND" } else { "OR" };
                let text = format!("{prefix} {}{}", if term.negated { "NOT " } else { "" }, condition_label(&term.condition));
                let style = if state.column == EditorColumn::ConditionAndVisual && state.selected_condition == index { crate::theme::selected_style() } else { Style::new() };
                details.push(Line::from(Span::styled(text, style)));
            }
        }
        details.push(Line::from(format!("THEN {}", visual_label(&rule.then.kind))));
        details.push(Line::from(""));
        if let Some(draft) = &state.parameter_draft { details.push(Line::from(format!("Value: {draft}▏  Enter saves · Esc cancels"))); }
        if let Some(error) = &state.save_error { details.push(Line::from(format!("Save failed: {error}"))); }
        details.push(Line::from("Tab changes column · Esc back"));
        if state.column == EditorColumn::ConditionAndVisual {
            details.push(Line::from("↑/↓ condition · ←/→ type"));
            details.push(Line::from("A add · Delete remove · N NOT · O AND/OR · E edit value · I case"));
            details.push(Line::from("V visual type · G glyph/frames · C colour · B bold · D dim"));
            details.push(Line::from("+/− speed · P ping-pong"));
        } else {
            details.push(Line::from("A add · N rename · Space enable · Delete remove"));
            details.push(Line::from("Ctrl+↑/↓ reorder · U undo · R reset"));
        }
    } else {
        details.push(Line::from("Select a rule"));
    }
    frame.render_widget(Paragraph::new(details).block(Block::default().borders(Borders::ALL).title(" Condition and icon ")), regions.editor);
    if let Ok((terms, _)) = state.expression(&app.custom_status_rules) {
        let (first, visible) = condition_window(state, regions.editor, terms.len());
        for (offset, term) in terms.iter().skip(first).take(visible).enumerate() {
            let row = regions.editor.y.saturating_add(1).saturating_add(offset as u16);
            if let Some(rect) = condition_actions(regions.editor, row) {
                let controls = if matches!(&term.condition, Condition::ScreenMatches(_)) { "‹›¬∨+×✎i" } else { "‹›¬∨+×✎ " };
                frame.render_widget(Paragraph::new(controls), rect);
            }
        }
        if let Some(rect) = visual_actions(regions.editor, visible) {
            frame.render_widget(Paragraph::new("V G C B D - + P").style(crate::theme::selected_style()), rect);
        }
    }
    for (tab, rect) in preview_tabs(regions.preview) {
        let label = match tab { PreviewTab::Live => " Live tree ", PreviewTab::Demo => " Demo tree " };
        let style = if tab == state.preview_tab { crate::theme::selected_style().add_modifier(Modifier::BOLD) } else { Style::new() };
        frame.render_widget(Paragraph::new(label).style(style), rect);
    }
    let tree_area = Rect::new(regions.preview.x, regions.preview.y.saturating_add(1), regions.preview.width, regions.preview.height.saturating_sub(1));
    match state.preview_tab {
        PreviewTab::Live => {
            crate::settings_ui::render_live_tree_preview(frame, tree_area, app, crate::agent_monitoring::AgentMonitoringMode::Custom);
        }
        PreviewTab::Demo => {
            let mut lines = vec![Line::from("Demo project")];
            for (index, name) in DEMO_SCENARIOS.into_iter().enumerate() {
                let icon = crate::settings_ui::monitoring_preview_span(app, crate::agent_monitoring::AgentMonitoringMode::Custom, index);
                lines.push(Line::from(vec![Span::raw("├ "), icon, Span::raw(format!(" {name}"))]));
            }
            frame.render_widget(Paragraph::new(lines).scroll((state.demo_scroll, 0)).block(Block::default().borders(Borders::ALL).title(" Demo · ↑↓ scroll ")), tree_area);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{EditorState, layout};
    use ilium_core::status_rule_expression::{Condition, ConditionTerm, Connector, default_rules};
    use ratatui::layout::Rect;

    #[test]
    fn cycling_an_edited_condition_starts_at_its_kind() {
        use ilium_core::status_rule_expression::{ScreenPattern, ScreenRegion};

        let mut rules = default_rules();
        let mut editor = EditorState::new(&rules);
        assert!(editor.add_rule(&mut rules));
        let cases = [
            (Condition::MonitorCountAtLeast(9), Condition::TaskStatus(ilium_core::ProgressTaskStatus::NotStartedYet)),
            (Condition::ProgressAtLeast(73), Condition::ProgressBelow(50)),
            (Condition::ScreenMatches(ScreenPattern { pattern: "custom".into(), ignore_case: false, region: ScreenRegion::LastLines(3) }), Condition::Always),
        ];
        for (condition, expected) in cases {
            editor.replace_expression(&mut rules, vec![ConditionTerm { condition, negated: false }], &[]).unwrap();
            editor.cycle_condition(&mut rules, 1).unwrap();
            assert_eq!(editor.expression(&rules).unwrap().0[0].condition, expected);
        }
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
        for _ in 0..100 { editor.scroll_demo(1); }
        assert_eq!(usize::from(editor.demo_scroll), super::DEMO_SCENARIOS.len());
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
        assert_eq!(super::rule_action_at(area, 6, Position::new(controls.x, 6)), Some(super::RuleAction::Toggle));
        assert_eq!(super::rule_action_at(area, 6, Position::new(controls.x + 4, 6)), Some(super::RuleAction::Delete));
        assert_eq!(super::rule_action_at(area, 6, Position::new(controls.x + 5, 6)), Some(super::RuleAction::Rename));
        assert_eq!(super::rule_action_at(area, 6, Position::new(controls.right(), 6)), None);
        assert_eq!(super::rule_action_at(area, 6, Position::new(controls.x, 7)), None);
        assert!(super::rule_actions(Rect::new(0, 0, 10, 10), 1).is_none());
        let add = super::add_rule_button(area).unwrap();
        assert_eq!(add.y, area.y);
        assert_eq!(add.right(), area.right() - 1);
        assert!(super::add_rule_button(Rect::new(0, 0, 10, 10)).is_none());
        let reset = super::reset_rule_button(area).unwrap();
        assert!(reset.right() < add.x);
        assert!(super::reset_rule_button(Rect::new(0, 0, 22, 10)).is_none());
        let conditions = super::condition_actions(area, 6).unwrap();
        assert_eq!(super::condition_action_at(area, 6, Position::new(conditions.x + 2, 6)), Some(super::ConditionAction::Not));
        assert_eq!(super::condition_action_at(area, 6, Position::new(conditions.x + 6, 6)), Some(super::ConditionAction::Edit));
        assert_eq!(super::condition_action_at(area, 6, Position::new(conditions.x + 7, 6)), Some(super::ConditionAction::Case));
        assert_eq!(super::condition_action_at(area, 6, Position::new(conditions.right(), 6)), None);
        let visual = super::visual_actions(area, 2).unwrap();
        assert_eq!(visual.y, area.y + 4);
        assert_eq!(super::visual_action_at(area, 2, Position::new(visual.x, visual.y)), Some(super::VisualAction::Type));
        assert_eq!(super::visual_action_at(area, 2, Position::new(visual.x + 1, visual.y)), None);
        assert_eq!(super::visual_action_at(area, 2, Position::new(visual.x + 14, visual.y)), Some(super::VisualAction::PingPong));
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
        editor.set_rule_name(&mut rules, "  Awaiting approval  ").unwrap();
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
        assert!(super::section_tabs(Rect::new(0, 0, 10, 1)).iter().all(|(_, rect)| rect.height == 0));
    }

    #[test]
    fn preview_preserves_requested_tree_width_when_there_is_room() {
        let wide = layout(Rect::new(0, 0, 130, 30), 32);
        assert_eq!(wide.preview.width, 32);
        let narrow = layout(Rect::new(0, 0, 70, 20), 32);
        assert_eq!(narrow.preview.width, 23);
        assert_eq!(wide.back.right(), 130);
        let compact = super::layout_for_column(Rect::new(0, 0, 38, 20), 32, super::EditorColumn::Preview);
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
        assert_eq!(rules.rules.iter().filter(|rule| rule.id == deleted_id).count(), 1);
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
            ConditionTerm { condition: Condition::GoalSet, negated: false },
            ConditionTerm { condition: Condition::ActivityStale, negated: true },
            ConditionTerm { condition: Condition::ScheduledInput, negated: false },
        ];
        assert!(editor.replace_expression(&mut rules, terms.clone(), &[Connector::Or, Connector::And]).is_ok());
        let original = editor.expression(&rules).unwrap();
        assert_eq!(original.1, [Connector::Or, Connector::And]);
        assert!(editor.replace_expression(&mut rules, terms, &[Connector::And]).is_err());
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
        terms[0].condition = Condition::ProgressBetween { minimum: 20, maximum: 80 };
        editor.replace_expression(&mut rules, terms, &connectors).unwrap();
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
    fn live_preview_renders_the_actual_session_tree() {
        use ratatui::{Terminal, backend::TestBackend};
        let mut app = crate::app::App::new("preview".into(), std::env::temp_dir());
        app.tree.add_group(ilium_core::ROOT_ID, "Unique live project").unwrap();
        let editor = EditorState::new(&app.custom_status_rules);
        let mut terminal = Terminal::new(TestBackend::new(130, 24)).unwrap();
        terminal.draw(|frame| super::render(frame, frame.area(), &app, &editor)).unwrap();
        let text: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();
        assert!(text.contains("Unique live project"));
        assert!(!text.contains("Live preview integration pending"));
    }
}
