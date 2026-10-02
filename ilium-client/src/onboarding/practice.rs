//! Keyboard practice is a private, pure domain tree. It has no connection,
//! request sender, application action executor, filesystem or PTY handle.

use std::{path::PathBuf, time::Duration};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ilium_core::{NodeId, PaneContentKind, SplitOrientation, Tree, TreeMoveDirection, ROOT_ID};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::{
    config::{KeyboardSettings, MotionLevel},
    keymap::{self, Action, BindingKey, KeyBinding},
    split_layout,
};

const ACCENT: Color = Color::Rgb(242, 188, 105);
const INK: Color = Color::Rgb(224, 231, 244);
const MUTED: Color = Color::Rgb(144, 157, 179);
const PANEL: Color = Color::Rgb(24, 29, 40);
const SUCCESS: Color = Color::Rgb(138, 211, 162);

// Four independently progressing slots replace learned basics with advanced
// actions. Paging exposes every slot, even when the available height is tiny.
const LESSONS: &[Action] = &[
    Action::NewTerminal,
    Action::FocusNextPane,
    Action::NewSplitView,
    Action::ClosePane,
    Action::NewGroup,
    Action::ToggleMove,
    Action::FocusTree,
    Action::Rename,
    Action::JumpNextGroup,
    Action::CycleNextInGroup,
    Action::FocusPaneRight,
    Action::NewEditor,
    Action::JumpPreviousGroup,
    Action::CyclePreviousInGroup,
    Action::FocusPaneDown,
    Action::NewBoard,
];

#[derive(Debug, Clone, Copy)]
enum Prefix {
    General,
    Navigation,
}

#[derive(Debug, Clone)]
enum Interaction {
    Normal,
    Moving,
    Rename(String),
    Split(SplitOrientation),
}

#[derive(Debug)]
pub struct PracticeState {
    tree: Tree,
    selected: Option<NodeId>,
    group: Option<NodeId>,
    prefix: Option<Prefix>,
    interaction: Interaction,
    tree_focus: bool,
    learned: Vec<(Action, Duration)>,
    last_success: Option<(Action, Duration)>,
    lesson_page: usize,
    serial: usize,
    message: String,
}

impl Default for PracticeState {
    fn default() -> Self {
        let mut tree = Tree::new();
        // This path is only a domain label; no directory is read or created.
        let project = tree.add_project(PathBuf::from("/practice/demo")).ok();
        let group = project.and_then(|id| tree.add_group(id, "Workspace").ok());
        let selected =
            group.and_then(|id| tree.add_pane(id, "Welcome", PaneContentKind::Terminal).ok());
        if let Some(group) = group {
            let _ = tree.add_pane(group, "Notes", PaneContentKind::Editor);
        }
        if let Some(project) = project {
            if let Ok(group) = tree.add_group(project, "Ideas") {
                let _ = tree.add_pane(group, "Sketch", PaneContentKind::Board);
            }
        }
        Self {
            tree,
            selected,
            group,
            prefix: None,
            interaction: Interaction::Normal,
            tree_focus: false,
            learned: Vec::new(),
            last_success: None,
            lesson_page: 0,
            serial: 3,
            message: "Demo only. Try a shortcut below.".into(),
        }
    }
}

impl PracticeState {
    pub fn prefix_pending(&self) -> bool {
        self.prefix.is_some()
    }

    /// The wizard can reserve Tab and footer navigation when this is false.
    /// While an internal prompt is open, its Enter/Escape belong to practice.
    pub fn interaction_pending(&self) -> bool {
        self.prefix_pending() || !matches!(self.interaction, Interaction::Normal)
    }

    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    pub fn selected(&self) -> Option<NodeId> {
        self.selected
    }

    /// Finite redraw demand, independent of ambient animation settings.
    pub fn is_animating(&self, now: Duration, motion: MotionLevel) -> bool {
        let lifetime = match motion {
            MotionLevel::Full => Duration::from_millis(1800),
            MotionLevel::Reduced => Duration::from_millis(600),
            MotionLevel::Off => return false,
        };
        self.last_success
            .is_some_and(|(_, time)| now.saturating_sub(time) < lifetime)
            || self
                .learned
                .iter()
                .any(|(_, time)| now.saturating_sub(*time) < lifetime)
    }

    /// Consume practice input, including release events, so it can never fall
    /// through to the normal application's keyboard dispatcher.
    pub fn handle_key(
        &mut self,
        key: &KeyEvent,
        keyboard: &KeyboardSettings,
        bindings: &[KeyBinding],
        now: Duration,
    ) -> bool {
        if key.kind == KeyEventKind::Release {
            return true;
        }
        if let Some(prefix) = self.prefix.take() {
            let base = match prefix {
                Prefix::General => keyboard.shortcut_base,
                Prefix::Navigation => keyboard.navigation_shortcut_base,
            };
            if keymap::is_leader_key(key, base) {
                self.message = "Literal prefix sent to the demo pane.".into();
                return true;
            }
            let action = BindingKey::from_key_event(key)
                .and_then(|binding| keymap::action_for_table(bindings, binding));
            if let Some(action) = action.filter(|action| {
                matches!(prefix, Prefix::General) || action.uses_navigation_prefix()
            }) {
                self.apply(action, now);
            } else {
                self.message = "No action on that prefix/key. Try a displayed shortcut.".into();
            }
            return true;
        }
        if key.code == KeyCode::Esc {
            self.interaction = Interaction::Normal;
            self.message = "Demo prompt closed.".into();
            return true;
        }
        if self.handle_prompt(key, now) {
            return true;
        }
        if keymap::is_leader_key(key, keyboard.shortcut_base) {
            self.prefix = Some(Prefix::General);
            return true;
        }
        if keymap::is_leader_key(key, keyboard.navigation_shortcut_base) {
            self.prefix = Some(Prefix::Navigation);
            return true;
        }
        match key.code {
            KeyCode::PageDown => self.lesson_page = (self.lesson_page + 1) % 4,
            KeyCode::PageUp => self.lesson_page = (self.lesson_page + 3) % 4,
            KeyCode::Up | KeyCode::Char('k') if self.tree_focus => self.cycle(-1, false),
            KeyCode::Down | KeyCode::Char('j') if self.tree_focus => self.cycle(1, false),
            _ => {}
        }
        true
    }

    fn handle_prompt(&mut self, key: &KeyEvent, now: Duration) -> bool {
        match self.interaction.clone() {
            Interaction::Normal => false,
            Interaction::Rename(mut name) => {
                match key.code {
                    KeyCode::Enter => {
                        if let Some(id) = self.selected {
                            if !name.trim().is_empty()
                                && self.tree.rename_node(id, name, None, None).is_ok()
                            {
                                self.succeed(Action::Rename, now);
                            }
                        }
                        self.interaction = Interaction::Normal;
                        return true;
                    }
                    KeyCode::Backspace => {
                        name.pop();
                    }
                    KeyCode::Char(character)
                        if !key.modifiers.contains(KeyModifiers::CONTROL)
                            && name.chars().count() < 40 =>
                    {
                        name.push(character)
                    }
                    _ => {}
                }
                self.interaction = Interaction::Rename(name);
                true
            }
            Interaction::Split(mut orientation) => {
                match key.code {
                    KeyCode::Char('v') | KeyCode::Left | KeyCode::Right => {
                        orientation = SplitOrientation::Vertical
                    }
                    KeyCode::Char('h') | KeyCode::Up | KeyCode::Down => {
                        orientation = SplitOrientation::Horizontal
                    }
                    KeyCode::Enter => {
                        self.create_split(orientation, now);
                        self.interaction = Interaction::Normal;
                        return true;
                    }
                    _ => {}
                }
                self.interaction = Interaction::Split(orientation);
                true
            }
            Interaction::Moving => {
                if key.code == KeyCode::Enter {
                    self.interaction = Interaction::Normal;
                    self.message = "Move finished.".into();
                    return true;
                }
                let Some(id) = self.selected else {
                    return true;
                };
                let moved = match key.code {
                    KeyCode::Up | KeyCode::Char('k') => self
                        .tree
                        .move_node_one_step(id, TreeMoveDirection::Up)
                        .unwrap_or(false),
                    KeyCode::Down | KeyCode::Char('j') => self
                        .tree
                        .move_node_one_step(id, TreeMoveDirection::Down)
                        .unwrap_or(false),
                    KeyCode::Right | KeyCode::Char('l') => self.indent(id),
                    KeyCode::Left | KeyCode::Char('h') => self.outdent(id),
                    _ => return false,
                };
                if moved {
                    self.group = self.tree.containing_group(id);
                    self.succeed(Action::ToggleMove, now);
                    self.message = "Demo pane moved. Arrows / h j k l; Enter ends move.".into();
                } else {
                    self.message = "Tree boundary reached. Try the other direction.".into();
                }
                true
            }
        }
    }

    fn apply(&mut self, action: Action, now: Duration) {
        let success = match action {
            Action::NewTerminal | Action::NewEditor | Action::NewBoard => {
                let content = match action {
                    Action::NewEditor => PaneContentKind::Editor,
                    Action::NewBoard => PaneContentKind::Board,
                    _ => PaneContentKind::Terminal,
                };
                let result = self.group.and_then(|group| {
                    self.tree
                        .add_pane(group, format!("Demo {}", self.serial + 1), content)
                        .ok()
                });
                if let Some(id) = result {
                    self.serial += 1;
                    self.select(id);
                }
                result.is_some()
            }
            Action::ClosePane => {
                let removed = self
                    .selected
                    .is_some_and(|id| self.tree.remove_node(id).is_ok());
                self.selected = self.tree.pane_ids_in_tree_order().first().copied();
                if let Some(id) = self.selected {
                    self.select(id);
                }
                removed
            }
            Action::NewGroup => {
                let result = self
                    .tree
                    .project_ids()
                    .first()
                    .copied()
                    .and_then(|project| {
                        self.tree
                            .add_group(project, format!("Group {}", self.serial + 1))
                            .ok()
                    });
                if let Some(group) = result {
                    self.serial += 1;
                    self.group = Some(group);
                    self.selected = self
                        .tree
                        .add_pane(group, "New group's demo", PaneContentKind::Terminal)
                        .ok();
                }
                result.is_some()
            }
            Action::NewSplitView => {
                self.interaction = Interaction::Split(SplitOrientation::Vertical);
                self.message = "Split: v side by side / h stacked; Enter creates.".into();
                return;
            }
            Action::Rename => {
                self.interaction = Interaction::Rename(String::new());
                self.message = "Type a demo name; Enter saves, Esc cancels.".into();
                return;
            }
            Action::ToggleMove => {
                self.tree_focus = true;
                self.interaction = if matches!(self.interaction, Interaction::Moving) {
                    Interaction::Normal
                } else {
                    Interaction::Moving
                };
                self.message =
                    "Move: arrows / h j k l. Enter ends; right indents, left outdents.".into();
                return;
            }
            Action::FocusTree => {
                self.tree_focus = true;
                true
            }
            Action::FocusPane => {
                self.tree_focus = false;
                true
            }
            Action::FocusNextPane | Action::FocusPreviousPane => {
                let previous = self.selected;
                self.cycle(
                    if action == Action::FocusNextPane {
                        1
                    } else {
                        -1
                    },
                    false,
                );
                self.selected != previous
            }
            Action::CycleNextInGroup | Action::CyclePreviousInGroup => {
                let previous = self.selected;
                self.cycle(
                    if action == Action::CycleNextInGroup {
                        1
                    } else {
                        -1
                    },
                    true,
                );
                self.selected != previous
            }
            Action::JumpNextGroup | Action::JumpPreviousGroup => {
                let groups = self.tree.group_ids_in_tree_order();
                let current = groups
                    .iter()
                    .position(|id| Some(*id) == self.group)
                    .unwrap_or(0);
                let next = wrapped_index(
                    current,
                    groups.len(),
                    if action == Action::JumpNextGroup {
                        1
                    } else {
                        -1
                    },
                );
                self.group = groups.get(next).copied();
                self.selected = self
                    .group
                    .and_then(|group| self.tree.pane_ids_in_subtree(group).first().copied());
                true
            }
            Action::FocusPaneLeft
            | Action::FocusPaneRight
            | Action::FocusPaneUp
            | Action::FocusPaneDown => self.focus_direction(action),
            _ => {
                self.message = format!(
                    "{} is outside this tree playground.",
                    keymap::action_label(action)
                );
                return;
            }
        };
        if success {
            self.succeed(action, now);
        } else {
            self.message = "No eligible demo target. Create or focus a pane first.".into();
        }
    }

    fn select(&mut self, id: NodeId) {
        self.selected = Some(id);
        self.group = self.tree.containing_group(id).or(self.group);
    }

    fn cycle(&mut self, delta: i32, within_group: bool) {
        let panes = if within_group {
            self.group
                .map(|id| self.tree.navigable_panes_in_group(id))
                .unwrap_or_default()
        } else {
            self.tree.pane_ids_in_tree_order()
        };
        let index = panes
            .iter()
            .position(|id| Some(*id) == self.selected)
            .unwrap_or(0);
        if let Some(id) = panes.get(wrapped_index(index, panes.len(), delta)) {
            self.select(*id);
        }
    }

    fn indent(&mut self, id: NodeId) -> bool {
        let Some(parent) = self.tree.parent_of(id) else {
            return false;
        };
        let Ok(siblings) = self.tree.children_of(parent) else {
            return false;
        };
        let Some(index) = siblings.iter().position(|sibling| *sibling == id) else {
            return false;
        };
        let destination = siblings[..index].iter().rev().copied().find(|sibling| {
            self.tree
                .get(*sibling)
                .is_some_and(|node| node.accepts_normal_children())
        });
        destination.is_some_and(|destination| self.tree.move_node(id, destination, None).is_ok())
    }

    fn outdent(&mut self, id: NodeId) -> bool {
        let Some(parent) = self.tree.parent_of(id) else {
            return false;
        };
        let Some(destination) = self
            .tree
            .parent_of(parent)
            .filter(|parent| *parent != ROOT_ID)
        else {
            return false;
        };
        let index = self
            .tree
            .children_of(destination)
            .ok()
            .and_then(|siblings| siblings.iter().position(|id| *id == parent))
            .map(|index| index + 1);
        self.tree.move_node(id, destination, index).is_ok()
    }

    fn create_split(&mut self, orientation: SplitOrientation, now: Duration) {
        let Some(group) = self.group else {
            return;
        };
        let mut panes: Vec<_> = self
            .tree
            .pane_ids_in_subtree(group)
            .into_iter()
            .filter(|id| {
                self.tree
                    .parent_of(*id)
                    .and_then(|parent| self.tree.get(parent))
                    .is_some_and(|parent| !parent.is_split_view())
            })
            .take(2)
            .collect();
        while panes.len() < 2 {
            let Ok(id) = self
                .tree
                .add_pane(group, "Split demo", PaneContentKind::Terminal)
            else {
                break;
            };
            panes.push(id);
        }
        match self
            .tree
            .create_split_view(group, "Demo split", orientation, &panes)
        {
            Ok(_) => {
                self.selected = panes.first().copied();
                self.succeed(Action::NewSplitView, now);
            }
            Err(error) => self.message = format!("Demo split: {error}"),
        }
    }

    fn visible_panes(&self) -> (SplitOrientation, Vec<NodeId>) {
        let Some(id) = self.selected else {
            return (SplitOrientation::Vertical, Vec::new());
        };
        if let Some(parent) = self.tree.parent_of(id) {
            if let Some(orientation) = self.tree.split_orientation(parent) {
                return (
                    orientation,
                    self.tree.children_of(parent).unwrap_or_default().to_vec(),
                );
            }
        }
        (SplitOrientation::Vertical, vec![id])
    }

    fn focus_direction(&mut self, action: Action) -> bool {
        let (orientation, ids) = self.visible_panes();
        let viewports =
            split_layout::allocate_viewports(Rect::new(0, 0, 120, 40), orientation, &ids);
        let direction = match action {
            Action::FocusPaneLeft => split_layout::PaneDirection::Left,
            Action::FocusPaneRight => split_layout::PaneDirection::Right,
            Action::FocusPaneUp => split_layout::PaneDirection::Up,
            _ => split_layout::PaneDirection::Down,
        };
        let target = split_layout::adjacent_pane_directions(&viewports)
            .into_iter()
            .find(|edge| Some(edge.source_pane_id) == self.selected && edge.direction == direction);
        if let Some(edge) = target {
            self.select(edge.destination_pane_id);
        }
        target.is_some()
    }

    fn succeed(&mut self, action: Action, now: Duration) {
        if !self.learned.iter().any(|(learned, _)| *learned == action) {
            self.learned.push((action, now));
        }
        self.last_success = Some((action, now));
        self.message = format!("Practised: {}", keymap::action_label(action));
    }

    fn lesson(
        &self,
        slot: usize,
        motion: MotionLevel,
        now: Duration,
    ) -> Option<(Action, f32, bool)> {
        let mut replacement_time = None;
        for action in LESSONS.iter().skip(slot).step_by(4).copied() {
            if let Some((_, learned)) = self.learned.iter().find(|(learned, _)| *learned == action)
            {
                let elapsed = now.saturating_sub(*learned).as_secs_f32();
                let hold = match motion {
                    MotionLevel::Full => 1.2,
                    MotionLevel::Reduced => 0.3,
                    MotionLevel::Off => 0.0,
                };
                if elapsed < hold {
                    let opacity = if motion == MotionLevel::Full {
                        1.0 - eased((elapsed - 0.6) / 0.6)
                    } else {
                        1.0
                    };
                    return Some((action, opacity, true));
                }
                replacement_time = Some(*learned + Duration::from_secs_f32(hold));
                continue;
            }
            let opacity = if motion == MotionLevel::Full {
                replacement_time
                    .map(|time| eased(now.saturating_sub(time).as_secs_f32() / 0.6))
                    .unwrap_or(1.0)
            } else {
                1.0
            };
            return Some((action, opacity, false));
        }
        None
    }
}

fn wrapped_index(index: usize, count: usize, delta: i32) -> usize {
    if count == 0 {
        return 0;
    }
    (index as i64 + i64::from(delta)).rem_euclid(count as i64) as usize
}

fn eased(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
}

fn fade(color: Color, opacity: f32) -> Color {
    let Color::Rgb(red, green, blue) = color else {
        return color;
    };
    let blend = |foreground: u8, background: u8| {
        (f32::from(background) + f32::from(foreground.saturating_sub(background)) * opacity) as u8
    };
    Color::Rgb(blend(red, 24), blend(green, 29), blend(blue, 40))
}

/// Draw within the wizard's content area, leaving its own footer untouched.
/// Wide layouts surround the tree/panes with hints; small layouts retain one
/// paged hint plus the live demo and a permanent paging instruction.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &PracticeState,
    keyboard: &KeyboardSettings,
    bindings: &[KeyBinding],
    motion: MotionLevel,
    now: Duration,
) {
    if area.is_empty() {
        return;
    }
    let surrounded = area.width >= 100 && area.height >= 12;
    let hint_rows = if surrounded {
        0
    } else if area.height >= 14 {
        5
    } else {
        2
    };
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(3),
            Constraint::Length(hint_rows),
            Constraint::Length(1),
        ])
        .split(area);
    let status = if state.prefix_pending() {
        "Prefix ready: press an action key".to_string()
    } else {
        match &state.interaction {
            Interaction::Rename(name) => format!("Rename: {name}_  Enter saves / Esc cancels"),
            Interaction::Split(orientation) => {
                format!("Split {orientation:?}: v / h, Enter confirms")
            }
            _ => state.message.clone(),
        }
    };
    frame.render_widget(
        Paragraph::new(status)
            .style(Style::default().fg(ACCENT))
            .wrap(Wrap { trim: true }),
        rows[0],
    );
    let mut hint_areas = Vec::new();
    if surrounded {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(22),
                Constraint::Min(36),
                Constraint::Length(22),
            ])
            .split(rows[1]);
        render_demo(frame, columns[1], state, now, motion);
        for column in [columns[0], columns[2]] {
            hint_areas.extend(
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(column)
                    .iter()
                    .copied(),
            );
        }
    } else {
        render_demo(frame, rows[1], state, now, motion);
        let slots = if hint_rows == 2 || area.width < 70 {
            1
        } else {
            4
        };
        hint_areas.extend(
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints(vec![Constraint::Ratio(1, slots); slots as usize])
                .split(rows[2])
                .iter()
                .copied(),
        );
    }
    for (index, rect) in hint_areas.into_iter().enumerate() {
        render_hint(
            frame,
            rect,
            state,
            (keyboard, bindings),
            motion,
            now,
            (state.lesson_page + index) % 4,
        );
    }
    frame.render_widget(
        Paragraph::new("PgUp/PgDn hints | Tab footer | Esc cancel")
            .style(Style::default().fg(MUTED)),
        rows[3],
    );
}

fn render_hint(
    frame: &mut Frame,
    area: Rect,
    state: &PracticeState,
    configured: (&KeyboardSettings, &[KeyBinding]),
    motion: MotionLevel,
    now: Duration,
    slot: usize,
) {
    let (keyboard, bindings) = configured;
    let Some((action, opacity, learned)) = state.lesson(slot, motion, now) else {
        frame.render_widget(
            Paragraph::new("All practised").style(Style::default().fg(SUCCESS)),
            area,
        );
        return;
    };
    let key = bindings
        .iter()
        .find(|binding| binding.action == action)
        .map(|binding| keymap::key_label(binding.key))
        .unwrap_or_else(|| "unbound".into());
    let prefix = keymap::action_prefix_label(
        action,
        keyboard.shortcut_base,
        keyboard.navigation_shortcut_base,
    );
    let color = fade(if learned { SUCCESS } else { ACCENT }, opacity);
    let lines = vec![
        Line::from(Span::styled(
            format!("{prefix} {key}"),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            keymap::action_label(action),
            Style::default().fg(fade(INK, opacity)),
        )),
        Line::from(if learned { "Learned" } else { "Try this" }),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .style(Style::default().bg(PANEL).fg(MUTED)),
        area,
    );
}

fn render_demo(
    frame: &mut Frame,
    area: Rect,
    state: &PracticeState,
    now: Duration,
    motion: MotionLevel,
) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);
    let tree_block = Block::default()
        .borders(Borders::ALL)
        .title("Demo tree")
        .border_style(Style::default().fg(if state.tree_focus { ACCENT } else { MUTED }));
    let tree_inner = tree_block.inner(columns[0]);
    let mut lines = Vec::new();
    let mut selected_row = 0;
    fn visit(
        state: &PracticeState,
        id: NodeId,
        depth: usize,
        lines: &mut Vec<Line<'static>>,
        selected_row: &mut usize,
    ) {
        let Some(node) = state.tree.get(id) else {
            return;
        };
        let selected = Some(id) == state.selected;
        if selected {
            *selected_row = lines.len();
        }
        lines.push(Line::from(Span::styled(
            format!(
                "{}{} {}",
                " ".repeat(depth.min(5)),
                if selected {
                    "›"
                } else if node.is_container() {
                    "▾"
                } else {
                    "·"
                },
                node.name
            ),
            Style::default().fg(if selected { ACCENT } else { INK }),
        )));
        for child in state.tree.children_of(id).unwrap_or_default() {
            visit(state, *child, depth + 1, lines, selected_row);
        }
    }
    for project in state.tree.children_of(ROOT_ID).unwrap_or_default() {
        visit(state, *project, 0, &mut lines, &mut selected_row);
    }
    let scroll = selected_row
        .saturating_sub(tree_inner.height.saturating_sub(1) as usize)
        .min(u16::MAX as usize) as u16;
    frame.render_widget(tree_block, columns[0]);
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), tree_inner);
    let (orientation, panes) = state.visible_panes();
    if panes.is_empty() {
        frame.render_widget(
            Paragraph::new("No demo panes. Try New terminal.").wrap(Wrap { trim: true }),
            columns[1],
        );
    }
    for viewport in split_layout::allocate_viewports(columns[1], orientation, &panes) {
        let selected = state.selected == Some(viewport.pane_id);
        let highlight = state.last_success.is_some_and(|(_, time)| {
            now.saturating_sub(time)
                < Duration::from_millis(if motion == MotionLevel::Off { 0 } else { 600 })
        });
        let color = if selected && highlight {
            SUCCESS
        } else if selected {
            ACCENT
        } else {
            MUTED
        };
        let name = state
            .tree
            .get(viewport.pane_id)
            .map(|node| node.name.as_str())
            .unwrap_or("Demo");
        frame.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .title(name)
                .border_style(Style::default().fg(color)),
            viewport.outer_area,
        );
        frame.render_widget(
            Paragraph::new(if selected {
                "Simulated pane\nYou are here\n$ _"
            } else {
                "Simulated pane\nReady"
            })
            .style(Style::default().fg(INK))
            .wrap(Wrap { trim: true }),
            viewport.content_area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::{KeymapPreset, ShortcutBase};
    use ratatui::{backend::TestBackend, Terminal};

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
    fn shortcut(
        state: &mut PracticeState,
        keyboard: &KeyboardSettings,
        bindings: &[KeyBinding],
        action: Action,
        now: Duration,
    ) {
        let base = if action.uses_navigation_prefix() {
            keyboard.navigation_shortcut_base
        } else {
            keyboard.shortcut_base
        };
        state.handle_key(
            &KeyEvent::new(KeyCode::Char(base.letter()), KeyModifiers::CONTROL),
            keyboard,
            bindings,
            now,
        );
        let binding = bindings
            .iter()
            .find(|binding| binding.action == action)
            .unwrap();
        let code = match binding.key {
            BindingKey::Character(character) => KeyCode::Char(character),
            BindingKey::Up => KeyCode::Up,
            BindingKey::Down => KeyCode::Down,
            BindingKey::Left => KeyCode::Left,
            BindingKey::Right => KeyCode::Right,
            BindingKey::PageUp => KeyCode::PageUp,
            BindingKey::PageDown => KeyCode::PageDown,
        };
        state.handle_key(&press(code), keyboard, bindings, now);
    }

    #[test]
    fn both_real_presets_and_custom_mapping_change_only_the_private_tree() {
        for preset in KeymapPreset::ALL {
            let keyboard = KeyboardSettings {
                shortcut_base: preset.shortcut_base(),
                ..Default::default()
            };
            let mut bindings = keymap::preset_bindings(preset);
            let mut state = PracticeState::default();
            let original = state.tree.clone();
            shortcut(
                &mut state,
                &keyboard,
                &bindings,
                Action::NewTerminal,
                Duration::ZERO,
            );
            assert_eq!(state.tree.panes().count(), original.panes().count() + 1);
            let free = keymap::available_keys(&bindings)[0];
            keymap::assign_key(
                &mut bindings,
                Action::ClosePane,
                BindingKey::Character(free),
            )
            .unwrap();
            shortcut(
                &mut state,
                &keyboard,
                &bindings,
                Action::ClosePane,
                Duration::ZERO,
            );
            assert_eq!(state.tree.panes().count(), original.panes().count());
            assert_eq!(original.panes().count(), 3);
            state.tree.validate().unwrap();
        }
    }

    #[test]
    fn split_focus_move_and_reparent_use_domain_geometry_and_invariants() {
        let keyboard = KeyboardSettings::default();
        let bindings = keymap::preset_bindings(KeymapPreset::Tmux);
        let mut state = PracticeState::default();
        shortcut(
            &mut state,
            &keyboard,
            &bindings,
            Action::NewSplitView,
            Duration::ZERO,
        );
        state.handle_key(&press(KeyCode::Enter), &keyboard, &bindings, Duration::ZERO);
        let first = state.selected;
        let split = state.tree.parent_of(first.unwrap()).unwrap();
        assert!(state.tree.get(split).unwrap().is_split_view());
        shortcut(
            &mut state,
            &keyboard,
            &bindings,
            Action::FocusPaneRight,
            Duration::ZERO,
        );
        assert_ne!(state.selected, first);
        let moving = state.selected.unwrap();
        shortcut(
            &mut state,
            &keyboard,
            &bindings,
            Action::ToggleMove,
            Duration::ZERO,
        );
        state.handle_key(&press(KeyCode::Left), &keyboard, &bindings, Duration::ZERO);
        assert_ne!(state.tree.parent_of(moving), Some(split));
        state.tree.validate().unwrap();
    }

    #[test]
    fn releases_do_not_consume_prefix_and_navigation_prefix_filters_actions() {
        let keyboard = KeyboardSettings {
            shortcut_base: ShortcutBase::A,
            navigation_shortcut_base: ShortcutBase::B,
        };
        let bindings = keymap::preset_bindings(KeymapPreset::Screen);
        let mut state = PracticeState::default();
        let prefix = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);
        state.handle_key(&prefix, &keyboard, &bindings, Duration::ZERO);
        state.handle_key(
            &KeyEvent {
                kind: KeyEventKind::Release,
                ..prefix
            },
            &keyboard,
            &bindings,
            Duration::ZERO,
        );
        assert!(state.prefix_pending());
        state.handle_key(&prefix, &keyboard, &bindings, Duration::ZERO);
        assert!(!state.prefix_pending());
        state.handle_key(
            &KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL),
            &keyboard,
            &bindings,
            Duration::ZERO,
        );
        state.handle_key(
            &press(KeyCode::Char('c')),
            &keyboard,
            &bindings,
            Duration::ZERO,
        );
        assert_eq!(state.tree.panes().count(), 3);
        shortcut(
            &mut state,
            &keyboard,
            &bindings,
            Action::JumpNextGroup,
            Duration::ZERO,
        );
        assert_eq!(state.tree.get(state.group.unwrap()).unwrap().name, "Ideas");
    }

    #[test]
    fn learned_callouts_fade_then_advance_and_respect_motion_settings() {
        let mut state = PracticeState::default();
        state.succeed(Action::NewTerminal, Duration::ZERO);
        let midway = state
            .lesson(0, MotionLevel::Full, Duration::from_millis(900))
            .unwrap();
        assert_eq!(midway.0, Action::NewTerminal);
        assert!((midway.1 - 0.5).abs() < 0.01);
        assert!(midway.2);
        let next = state
            .lesson(0, MotionLevel::Full, Duration::from_millis(1500))
            .unwrap();
        assert_eq!(next.0, Action::NewGroup);
        assert!((next.1 - 0.5).abs() < 0.01);
        assert_eq!(
            state.lesson(0, MotionLevel::Off, Duration::ZERO).unwrap(),
            (Action::NewGroup, 1.0, false)
        );
        assert_eq!(
            state
                .lesson(0, MotionLevel::Reduced, Duration::from_millis(400))
                .unwrap(),
            (Action::NewGroup, 1.0, false)
        );
        assert!(state.is_animating(Duration::from_millis(1500), MotionLevel::Full));
        assert!(!state.is_animating(Duration::from_secs(2), MotionLevel::Full));
        assert!(!state.is_animating(Duration::ZERO, MotionLevel::Off));
    }

    #[test]
    fn resizing_preserves_demo_and_all_hint_slots_remain_reachable() {
        let keyboard = KeyboardSettings::default();
        let bindings = keymap::preset_bindings(KeymapPreset::Tmux);
        let mut state = PracticeState::default();
        let selected = state.selected;
        for (width, height) in [(40, 9), (40, 16), (80, 24), (120, 40), (200, 60)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            for action in LESSONS.iter().take(4) {
                terminal
                    .draw(|frame| {
                        render(
                            frame,
                            frame.area(),
                            &state,
                            &keyboard,
                            &bindings,
                            MotionLevel::Off,
                            Duration::ZERO,
                        )
                    })
                    .unwrap();
                let text: String = terminal
                    .backend()
                    .buffer()
                    .content()
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect();
                assert!(text.contains("PgUp/PgDn"));
                assert!(text.contains(keymap::action_label(*action)) || height >= 14);
                state.handle_key(
                    &press(KeyCode::PageDown),
                    &keyboard,
                    &bindings,
                    Duration::ZERO,
                );
            }
            assert_eq!(state.selected, selected);
            state.tree.validate().unwrap();
        }
    }

    #[test]
    fn minimum_wizard_content_pages_every_advanced_lesson_after_basics() {
        let keyboard = KeyboardSettings::default();
        let bindings = keymap::preset_bindings(KeymapPreset::Tmux);
        let mut state = PracticeState::default();
        let mut terminal = Terminal::new(TestBackend::new(38, 9)).unwrap();
        for tier in 0..4 {
            for slot in 0..4 {
                let action = LESSONS[tier * 4 + slot];
                terminal
                    .draw(|frame| {
                        render(
                            frame,
                            frame.area(),
                            &state,
                            &keyboard,
                            &bindings,
                            MotionLevel::Off,
                            Duration::ZERO,
                        )
                    })
                    .unwrap();
                let text: String = terminal
                    .backend()
                    .buffer()
                    .content()
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect();
                assert!(
                    text.contains(keymap::action_label(action)),
                    "Missing {action:?}: {text}"
                );
                state.handle_key(
                    &press(KeyCode::PageDown),
                    &keyboard,
                    &bindings,
                    Duration::ZERO,
                );
            }
            for action in &LESSONS[tier * 4..tier * 4 + 4] {
                state.succeed(*action, Duration::ZERO);
            }
        }
    }

    #[test]
    fn session_and_provider_actions_are_consumed_without_changing_demo() {
        let keyboard = KeyboardSettings::default();
        let bindings = keymap::preset_bindings(KeymapPreset::Tmux);
        let mut state = PracticeState::default();
        let ids = state.tree.pane_ids_in_tree_order();
        for action in [
            Action::Quit,
            Action::Detach,
            Action::Settings,
            Action::NewAgentWorktree,
            Action::RunCommand,
        ] {
            shortcut(&mut state, &keyboard, &bindings, action, Duration::ZERO);
            assert_eq!(state.tree.pane_ids_in_tree_order(), ids);
            assert!(state.message.contains("outside this tree playground"));
        }
    }
}
