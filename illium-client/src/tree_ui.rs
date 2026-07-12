//! Renders `illium_core::Tree` as a `tui_tree_widget::Tree<NodeId>`.
//!
//! Expand/collapse state belongs to `tui_tree_widget::TreeState`, because
//! that widget also owns its visible-row and scrolling bookkeeping. Mouse
//! and keyboard handlers mutate the same state, so a collapsed group stays
//! collapsed until the user opens it again.

use std::collections::HashSet;

use illium_core::{AgentActivity, AgentClass, Node, NodeId, NodeKind, PaneStatus, Tree, ROOT_ID};
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::Frame;
use tui_tree_widget::{Tree as TreeWidget, TreeItem, TreeState};

use crate::theme;

/// Braille spinner frames for an actively-`Working` agent pane, cycled by
/// elapsed wall-clock time so it animates smoothly across redraws
/// regardless of the (much slower) detection poll interval.
const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const SPINNER_FRAME_MS: u128 = 90;

/// How long each half-cycle of the `Done` bell pulse lasts. Same glyph
/// every frame (a bell), only its boldness pulses -- reads as "ringing"
/// without any change in rendered width or color, so the tree row never
/// jitters and every agent status still shares one base text color.
const DONE_PULSE_MS: u128 = 450;

/// Fixed "this pane is a detected agent" glyph, prefixed before the
/// activity glyph (spinner/bell/question/dot) on every `Agent` row -- the
/// activity glyph alone doesn't say "agent" the way the tree/toolbar's
/// folder and pen glyphs say "group"/"editor".
const AGENT_ICON: &str = "\u{1F916} ";

/// The two-row action strip is always reserved at the bottom of the tree
/// panel so a mouse move can reveal it without shifting the tree rows: a
/// glyph-button row, plus a caption row that names whichever button the
/// pointer is actually over (blank otherwise, so it never nags).
const TOOLBAR_HEIGHT: u16 = 2;
const TOOLBAR_BUTTON_WIDTH: u16 = 4;
const ROW_ACTION_WIDTH: u16 = 2;
/// Edit, up, down, then close -- reserved as the trailing cells of a
/// hovered row (see `row_action_at`/`draw_row_actions`).
const ROW_ACTION_COUNT: u16 = 4;

/// Actions available from the hover-only tree toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeToolbarAction {
    Shell,
    Claude,
    Codex,
    Editor,
    Group,
}

impl TreeToolbarAction {
    /// Ordered set used for both rendering and hit testing.
    pub const ALL: [Self; 5] = [
        Self::Shell,
        Self::Claude,
        Self::Codex,
        Self::Editor,
        Self::Group,
    ];

    /// Single glyph shown on the button, reusing the same symbol the tree
    /// already uses for that node kind elsewhere (group/editor) so the
    /// toolbar and the tree read as one visual language rather than two.
    const fn glyph(self) -> &'static str {
        match self {
            Self::Shell => ">",
            Self::Claude => "Ⓒ",
            Self::Codex => "Ⓧ",
            Self::Editor => "\u{1F589}",
            Self::Group => "\u{1F4C1}",
        }
    }

    /// Accent color identifying this action, distinct enough from the
    /// others (and from the status colors already used for pane state --
    /// yellow/working, cyan/waiting, green/idle) that the five buttons read
    /// as five different things at a glance instead of one repeated shape.
    const fn accent(self) -> Color {
        match self {
            Self::Shell => Color::Gray,
            Self::Claude => Color::Rgb(0xd9, 0x77, 0x57),
            Self::Codex => Color::Rgb(0x2c, 0xb6, 0x7d),
            Self::Editor => Color::Magenta,
            Self::Group => Color::Rgb(0x7a, 0xa2, 0xf7),
        }
    }

    /// Human-readable text shown live in the toolbar's caption row while
    /// hovered, and reused in the status bar if the action later fails.
    pub const fn description(self) -> &'static str {
        match self {
            Self::Shell => "new shell",
            Self::Claude => "new Claude shell",
            Self::Codex => "new Codex shell",
            Self::Editor => "new editor",
            Self::Group => "new group",
        }
    }
}

/// An action selected through the row-level hover controls (edit-pen,
/// up/down move arrows, then close).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeRowAction {
    Rename,
    MoveUp,
    MoveDown,
    Close,
}

impl TreeRowAction {
    /// Ordered set used for both rendering and hit testing -- left to right.
    const ALL: [Self; 4] = [Self::Rename, Self::MoveUp, Self::MoveDown, Self::Close];

    const fn glyph(self) -> &'static str {
        match self {
            Self::Rename => "\u{270e}",
            Self::MoveUp => "↑",
            Self::MoveDown => "↓",
            Self::Close => "\u{2715}",
        }
    }
}

/// Exact node and screen row identified by tree hit testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeNodeHit {
    pub id: NodeId,
    pub row: u16,
}

/// Hover-only rendering state supplied by `App`; grouping it keeps the tree
/// renderer's contract compact as additional hover affordances are added.
#[derive(Debug, Clone, Copy, Default)]
pub struct TreeHoverState {
    pub node: Option<TreeNodeHit>,
    pub toolbar_hovered: bool,
    /// The exact button under the pointer, if any -- distinct from
    /// `toolbar_hovered` because the pointer can sit in the toolbar strip's
    /// dead space (past the last button) without targeting one.
    pub toolbar_action: Option<TreeToolbarAction>,
}

/// All transient presentation state for one tree render. Grouping it keeps
/// the renderer's stable tree/state inputs distinct from redraw-only UI data.
pub struct TreeRenderOptions<'a> {
    pub focused: bool,
    pub elapsed_ms: u128,
    pub project_name: Option<&'a str>,
    pub is_project_name_loading: bool,
    /// Terminal panes currently awaiting `session_naming::infer_pane_title`
    /// (see `App::titles_loading`) -- their name renders as the shared
    /// braille spinner instead, same visual language as `is_project_name_loading`.
    pub titles_loading: &'a HashSet<NodeId>,
    pub hover: TreeHoverState,
}

/// Builds the full recursive `TreeItem` tree from the root group's
/// children (the root group itself is never shown as a node -- its
/// children are illium's top-level groups/panes). `elapsed_ms` drives the
/// Working spinner and Done pulse animations.
pub fn build_tree_items(
    tree: &Tree,
    elapsed_ms: u128,
    titles_loading: &HashSet<NodeId>,
) -> Vec<TreeItem<'static, NodeId>> {
    build_children(tree, ROOT_ID, elapsed_ms, titles_loading)
}

/// Recursively builds `TreeItem`s for every child of `parent`.
fn build_children(
    tree: &Tree,
    parent: NodeId,
    elapsed_ms: u128,
    titles_loading: &HashSet<NodeId>,
) -> Vec<TreeItem<'static, NodeId>> {
    let Ok(children) = tree.children_of(parent) else {
        // `parent` was a Pane (not a Group) or didn't exist -- neither is
        // reachable from a valid tree walk starting at ROOT_ID, but fail
        // soft rather than panic if the tree ever gets into that shape.
        return Vec::new();
    };
    children
        .iter()
        .filter_map(|&child_id| {
            tree.get(child_id)
                .map(|node| build_item(tree, node, elapsed_ms, titles_loading))
        })
        .collect()
}

/// Builds one `TreeItem` (recursing into children for a Group).
fn build_item(
    tree: &Tree,
    node: &Node,
    elapsed_ms: u128,
    titles_loading: &HashSet<NodeId>,
) -> TreeItem<'static, NodeId> {
    match &node.kind {
        NodeKind::Group { .. } => {
            let children = build_children(tree, node.id, elapsed_ms, titles_loading);
            let label = Line::from(Span::raw(format!("\u{1F4C1} {}", node.name)));
            // `NodeId`s are unique across the whole `Tree` (its own
            // invariant), so they can't collide among siblings here --
            // the `Result` this returns is unreachable in practice.
            TreeItem::new(node.id, label, children).expect("sibling NodeIds are always unique")
        }
        NodeKind::Pane { status, .. } => TreeItem::new_leaf(
            node.id,
            pane_label(
                status,
                &node.name,
                elapsed_ms,
                titles_loading.contains(&node.id),
            ),
        ),
    }
}

/// Builds the icon+color-prefixed label for a single pane, based on its
/// current `PaneStatus`. `elapsed_ms` selects the current animation frame
/// for `Working` (spinning braille dots) and `Done` (pulsing bell).
fn pane_label(
    status: &PaneStatus,
    name: &str,
    elapsed_ms: u128,
    is_title_loading: bool,
) -> Line<'static> {
    // While `session_naming::infer_pane_title` is still awaiting a result
    // for this pane, its name renders as the same braille spinner
    // `sidebar_title` uses for the project name -- the activity glyph
    // ahead of it (spinner/bell/question mark/dot) is a separate concept
    // and keeps animating independently.
    let title = || -> String {
        if is_title_loading {
            let frame_index = (elapsed_ms / SPINNER_FRAME_MS) as usize % SPINNER_FRAMES.len();
            SPINNER_FRAMES[frame_index].to_string()
        } else {
            name.to_string()
        }
    };
    match status {
        PaneStatus::PlainShell => Line::from(vec![
            Span::styled("> ", Style::new().fg(Color::Gray)),
            Span::raw(name.to_string()),
        ]),
        PaneStatus::Agent(class, AgentActivity::Working) => {
            let frame_index = (elapsed_ms / SPINNER_FRAME_MS) as usize % SPINNER_FRAMES.len();
            let glyph = SPINNER_FRAMES[frame_index];
            Line::from(vec![
                Span::raw(AGENT_ICON),
                Span::raw(format!("{glyph} ")),
                Span::raw(format!("{} ", agent_class_label(class))),
                Span::raw(title()),
            ])
        }
        PaneStatus::Agent(class, AgentActivity::Done) => {
            // Pulses bold on/off rather than changing color -- every agent
            // status shares the same base text color, so "come look, I'm
            // done" reads through boldness alone.
            let bright = (elapsed_ms / DONE_PULSE_MS).is_multiple_of(2);
            let style = if bright {
                Style::new().add_modifier(Modifier::BOLD)
            } else {
                Style::new()
            };
            let title = title();
            Line::from(vec![
                Span::raw(AGENT_ICON),
                Span::styled("\u{1F514} ", style),
                Span::styled(format!("{} ", agent_class_label(class)), style),
                Span::styled(format!("{title} — done"), style),
            ])
        }
        PaneStatus::Agent(class, AgentActivity::WaitingApproval) => {
            // Bold, not colored -- every agent status shares the same base
            // text color; boldness alone signals "needs you."
            let style = Style::new().add_modifier(Modifier::BOLD);
            Line::from(vec![
                Span::raw(AGENT_ICON),
                Span::styled("? ", style),
                Span::styled(format!("{} ", agent_class_label(class)), style),
                Span::styled(title(), style),
            ])
        }
        PaneStatus::Agent(class, AgentActivity::Idle) => Line::from(vec![
            Span::raw(AGENT_ICON),
            Span::raw("\u{25cf} "),
            Span::raw(format!("{} ", agent_class_label(class))),
            Span::raw(title()),
        ]),
        PaneStatus::Editor { dirty: true } => Line::from(vec![
            Span::styled("\u{1F589} ", Style::new().fg(Color::Magenta)),
            Span::styled(format!("{name}*"), Style::new().fg(Color::Magenta)),
        ]),
        PaneStatus::Editor { dirty: false } => {
            Line::from(vec![Span::raw("\u{1F589} "), Span::raw(name.to_string())])
        }
    }
}

/// Human-readable name for an `AgentClass`, shown as a prefix before the
/// pane's own name.
fn agent_class_label(class: &AgentClass) -> String {
    match class {
        AgentClass::Claude => "Claude:".to_string(),
        AgentClass::Codex => "Codex:".to_string(),
        AgentClass::Other(name) => name.clone(),
    }
}

/// Returns the interior list region, excluding the persistent two-row hover
/// toolbar. Both rendering and hit testing use this exact rectangle.
pub fn list_area(area: Rect) -> Rect {
    let inner = theme::block(false).inner(area);
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(TOOLBAR_HEIGHT)])
        .split(inner)[0]
}

/// Returns the bottom two rows that reveal creation icons on hover.
pub fn toolbar_area(area: Rect) -> Rect {
    let inner = theme::block(false).inner(area);
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(TOOLBAR_HEIGHT)])
        .split(inner)[1]
}

/// Returns the toolbar action at a terminal coordinate, if any.
pub fn toolbar_action_at(area: Rect, position: Position) -> Option<TreeToolbarAction> {
    let toolbar = toolbar_area(area);
    if !toolbar.contains(position) {
        return None;
    }
    let index = usize::from(position.x.saturating_sub(toolbar.x) / TOOLBAR_BUTTON_WIDTH);
    TreeToolbarAction::ALL.get(index).copied()
}

/// Returns the row action at `position` for a hovered row, reserving the
/// far-right six cells of the list for the edit/up/down targets.
pub fn row_action_at(area: Rect, row: u16, position: Position) -> Option<TreeRowAction> {
    let list = list_area(area);
    if row < list.y || row >= list.bottom() || position.y != row {
        return None;
    }
    let controls_start = list
        .right()
        .saturating_sub(ROW_ACTION_WIDTH * ROW_ACTION_COUNT);
    if position.x < controls_start || position.x >= list.right() {
        return None;
    }
    let index = usize::from((position.x - controls_start) / ROW_ACTION_WIDTH);
    TreeRowAction::ALL.get(index).copied()
}

/// Returns a node only when `position` is on one of the actually visible
/// one-line rows, never on blank space below the final item.
pub fn node_at_position(
    tree: &Tree,
    state: &TreeState<NodeId>,
    area: Rect,
    position: Position,
) -> Option<TreeNodeHit> {
    let list = list_area(area);
    if !list.contains(position) {
        return None;
    }
    // Hit-testing only needs node identifiers and row structure, neither of
    // which depends on label text -- an empty set is fine here.
    let items = build_tree_items(tree, 0, &HashSet::new());
    let visible_index = state.get_offset() + usize::from(position.y.saturating_sub(list.y));
    let id = state
        .flatten(&items)
        .get(visible_index)?
        .identifier
        .last()
        .copied()?;
    Some(TreeNodeHit {
        id,
        row: position.y,
    })
}

/// Draws the tree panel into `area`, bordered brighter when `focused`.
/// `elapsed_ms` drives the Working spinner and Done pulse animations.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    tree: &Tree,
    state: &mut TreeState<NodeId>,
    options: TreeRenderOptions<'_>,
) {
    let items = build_tree_items(tree, options.elapsed_ms, options.titles_loading);
    let block = theme::block(options.focused).title(theme::chrome_title(&sidebar_title(
        options.project_name,
        options.is_project_name_loading,
        options.elapsed_ms,
    )));
    let list = list_area(area);
    frame.render_widget(block, area);

    // Top-level items are siblings under the (unrendered) root group, and
    // `NodeId`s are unique across the whole tree, so duplicate identifiers
    // among them is unreachable.
    let widget = TreeWidget::new(&items)
        .expect("top-level items have unique identifiers")
        .highlight_style(theme::selected_style());
    frame.render_stateful_widget(widget, list, state);

    draw_scrollbar(frame, area, &items, state);

    if let Some(hit) = options.hover.node {
        draw_row_actions(frame, area, hit.row);
    }
    if options.hover.toolbar_hovered {
        draw_toolbar(frame, area, options.hover.toolbar_action);
    }
}

/// Produces the compact left-panel title from Illium's product name and the
/// project-local metadata. The self-hosting case uses the requested squared
/// mark instead of the visually redundant `Illium: Illium`.
pub fn sidebar_title(
    project_name: Option<&str>,
    is_project_name_loading: bool,
    elapsed_ms: u128,
) -> String {
    match project_name {
        Some("Illium") => "Illium²".to_string(),
        Some(project_name) => format!("Illium: {project_name}"),
        None if is_project_name_loading => {
            let frame_index = (elapsed_ms / SPINNER_FRAME_MS) as usize % SPINNER_FRAMES.len();
            format!("Illium: {}", SPINNER_FRAMES[frame_index])
        }
        None => "Illium".to_string(),
    }
}

#[cfg(test)]
mod project_title_tests {
    use super::sidebar_title;

    #[test]
    fn sidebar_title_includes_project_name_or_uses_the_self_hosting_mark() {
        assert_eq!(sidebar_title(None, false, 0), "Illium");
        assert_eq!(sidebar_title(Some("Money"), false, 0), "Illium: Money");
        assert_eq!(sidebar_title(Some("Illium"), false, 0), "Illium²");
        assert_eq!(sidebar_title(None, true, 0), "Illium: ⠋");
    }
}

/// Draws a vertical scrollbar along the list's right edge, but only once
/// the tree actually has more rows than fit -- a tree that fits entirely on
/// screen has nothing to scroll, so the track would just be a distracting,
/// always-full bar.
fn draw_scrollbar(
    frame: &mut Frame,
    area: Rect,
    items: &[TreeItem<'static, NodeId>],
    state: &TreeState<NodeId>,
) {
    let list = list_area(area);
    let total_rows = state.flatten(items).len();
    if total_rows <= usize::from(list.height) {
        return;
    }
    let mut scrollbar_state = ScrollbarState::new(total_rows).position(state.get_offset());
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some(" "))
        .style(theme::border_style(false));
    frame.render_stateful_widget(scrollbar, list, &mut scrollbar_state);
}

/// Draws the edit/up/down controls over the hovered tree row's trailing
/// cells, left to right.
fn draw_row_actions(frame: &mut Frame, area: Rect, row: u16) {
    let list = list_area(area);
    let controls_start = list
        .right()
        .saturating_sub(ROW_ACTION_WIDTH * ROW_ACTION_COUNT);
    let style = theme::selected_style().add_modifier(Modifier::BOLD);
    for (index, action) in TreeRowAction::ALL.iter().enumerate() {
        let x = controls_start + index as u16 * ROW_ACTION_WIDTH;
        let cell = Rect::new(x, row, ROW_ACTION_WIDTH, 1);
        frame.render_widget(Paragraph::new(action.glyph()).style(style), cell);
    }
}

/// Draws the five creation buttons plus a caption row naming whichever one
/// the pointer currently sits over. Every button keeps its own accent color
/// at all times (so the five read as distinct actions on sight, not one
/// repeated shape); the hovered button additionally brightens and bolds to
/// confirm exactly what a click would do before it happens.
fn draw_toolbar(frame: &mut Frame, area: Rect, hovered: Option<TreeToolbarAction>) {
    let toolbar = toolbar_area(area);
    let icon_row = Rect::new(toolbar.x, toolbar.y, toolbar.width, 1);
    let caption_row = Rect::new(toolbar.x, toolbar.y + 1, toolbar.width, 1);

    for (index, action) in TreeToolbarAction::ALL.iter().enumerate() {
        let x = toolbar.x + index as u16 * TOOLBAR_BUTTON_WIDTH;
        if x + TOOLBAR_BUTTON_WIDTH > icon_row.right() {
            break;
        }
        let is_hovered = hovered == Some(*action);
        let style = if is_hovered {
            Style::new()
                .fg(theme::accent_fg())
                .bg(action.accent())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(action.accent())
        };
        let button_area = Rect::new(x, icon_row.y, TOOLBAR_BUTTON_WIDTH - 1, 1);
        let button = Paragraph::new(Line::from(Span::styled(
            format!(" {} ", action.glyph()),
            style,
        )));
        frame.render_widget(button, button_area);
    }

    if let Some(action) = hovered {
        let caption = Paragraph::new(Line::from(Span::styled(
            format!(" {}", action.description()),
            Style::new()
                .fg(action.accent())
                .add_modifier(Modifier::BOLD),
        )));
        frame.render_widget(caption, caption_row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_label_shows_the_name_normally_when_no_title_inference_is_in_flight() {
        let line = pane_label(
            &PaneStatus::Agent(AgentClass::Claude, AgentActivity::Idle),
            "claude",
            0,
            false,
        );
        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(text.ends_with("claude"));
    }

    #[test]
    fn pane_label_shows_the_braille_spinner_instead_of_the_name_while_title_inference_is_in_flight()
    {
        let line = pane_label(
            &PaneStatus::Agent(AgentClass::Claude, AgentActivity::Idle),
            "claude",
            0,
            true,
        );
        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(!text.contains("claude"));
        assert!(text.ends_with(SPINNER_FRAMES[0]));
    }

    #[test]
    fn toolbar_hit_testing_maps_each_compact_icon() {
        let area = Rect::new(0, 0, 32, 12);
        assert_eq!(
            toolbar_action_at(area, Position::new(1, 10)),
            Some(TreeToolbarAction::Shell)
        );
        assert_eq!(
            toolbar_action_at(area, Position::new(9, 10)),
            Some(TreeToolbarAction::Codex)
        );
        assert_eq!(toolbar_action_at(area, Position::new(25, 10)), None);
    }

    #[test]
    fn row_action_hit_testing_uses_right_edge_of_hovered_row() {
        let area = Rect::new(0, 0, 33, 20);
        assert_eq!(
            row_action_at(area, 5, Position::new(24, 5)),
            Some(TreeRowAction::Rename)
        );
        assert_eq!(
            row_action_at(area, 5, Position::new(26, 5)),
            Some(TreeRowAction::MoveUp)
        );
        assert_eq!(
            row_action_at(area, 5, Position::new(28, 5)),
            Some(TreeRowAction::MoveDown)
        );
        assert_eq!(
            row_action_at(area, 5, Position::new(30, 5)),
            Some(TreeRowAction::Close)
        );
        assert_eq!(row_action_at(area, 4, Position::new(28, 5)), None);
    }

    #[test]
    fn node_hit_testing_excludes_blank_rows_below_the_tree() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "group").unwrap();
        let pane = tree
            .add_pane(group, "pane", illium_core::PaneContentKind::Terminal)
            .unwrap();
        let mut state = TreeState::default();
        state.open(vec![group]);
        let area = Rect::new(0, 0, 32, 12);
        let list = list_area(area);

        assert_eq!(
            node_at_position(&tree, &state, area, Position::new(list.x, list.y)),
            Some(TreeNodeHit {
                id: group,
                row: list.y
            })
        );
        assert_eq!(
            node_at_position(&tree, &state, area, Position::new(list.x, list.y + 1)),
            Some(TreeNodeHit {
                id: pane,
                row: list.y + 1
            })
        );
        assert_eq!(
            node_at_position(&tree, &state, area, Position::new(list.x, list.y + 2)),
            None
        );
    }
}
