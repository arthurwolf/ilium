//! Renders `ilium_core::Tree` as a `tui_tree_widget::Tree<NodeId>`.
//!
//! Expand/collapse state belongs to `tui_tree_widget::TreeState`, because
//! that widget also owns its visible-row and scrolling bookkeeping. Mouse
//! and keyboard handlers mutate the same state, so a collapsed group stays
//! collapsed until the user opens it again.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use ilium_core::{AgentActivity, AgentClass, Node, NodeId, NodeKind, PaneStatus, Tree, ROOT_ID};
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::Frame;
use tui_tree_widget::{Tree as TreeWidget, TreeItem, TreeState};
use unicode_width::UnicodeWidthStr;

use crate::config::{AgentIdentifierMode, AgentIdentifierSettings};
use crate::theme;

/// Braille spinner frames for an actively-`Working` agent pane, cycled by
/// elapsed wall-clock time so it animates smoothly across redraws
/// regardless of the (much slower) detection poll interval.
const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const SPINNER_FRAME_MS: u128 = 90;

/// Every half-hour clock face in chronological order for a
/// `WaitingBackground` agent pane. The slower cadence keeps the full clock
/// sweep distinct from `Working`'s dense braille churn while clearly reading
/// as time passing for background work.
const BACKGROUND_CLOCK_FRAMES: &[char] = &[
    '🕛', '🕧', '🕐', '🕜', '🕑', '🕝', '🕒', '🕞', '🕓', '🕟', '🕔', '🕠', '🕕', '🕡', '🕖', '🕢',
    '🕗', '🕣', '🕘', '🕤', '🕙', '🕥', '🕚', '🕦',
];
const BACKGROUND_FRAME_MS: u128 = 220;

/// How long each half-cycle of the `Done` bell pulse lasts. Same glyph
/// every frame (a bell), only its boldness pulses -- reads as "ringing"
/// without any change in rendered width or color, so the tree row never
/// jitters and every agent status still shares one base text color.
const DONE_PULSE_MS: u128 = 450;

/// How long a freshly created node (a new shell/agent/editor pane, or a new
/// group) visually flashes after this client first observes it, so a click
/// on the creation toolbar is obviously followed by something appearing --
/// including every node from a multi-create burst, each flashing
/// independently from its own creation moment. See `App::recently_created`.
pub(crate) const RECENTLY_CREATED_PULSE_MS: u128 = 1400;
/// Half-cycle of the on/off flash within the pulse window (~4 flashes
/// total), the same wall-clock-driven-frame approach as `DONE_PULSE_MS`.
const RECENTLY_CREATED_PULSE_PHASE_MS: u128 = 175;

/// Fallback identifier for agent classes without their own configurable icon.
const AGENT_ICON: &str = "\u{1F916}";
/// Plain terminal panes and the matching creation action share this glyph.
const TERMINAL_ICON: &str = "📟";
/// Text editor panes and their creation action share this glyph; rename keeps
/// the pen icon because it represents an action rather than a pane kind.
const TEXT_EDITOR_ICON: &str = "🗒️";
/// Settings stays anchored at the bottom-right of the left panel.
const SETTINGS_ICON: &str = "🎚️";

/// Hard ceiling on how many filesystem entries `folder_children` will
/// materialize into `TreeItem`s across one `Folder` node's entire subtree in
/// a single call. `render` rebuilds the whole tree (including every open
/// `Folder` node's descendants) on every frame -- see `build_tree_items` --
/// and animation (the Working spinner, WaitingBackground clock, and Done
/// pulse) forces frequent redraws even when nothing about the folder itself
/// changed. Without a cap, opening a folder whose subtree contains something
/// like `node_modules` or a build-output directory would re-walk and
/// re-allocate a `TreeItem` (plus its owned label `String`) for every entry,
/// every redraw, unboundedly.
///
/// This is a mitigation, not the complete fix: the complete fix is to only
/// recurse into folders the user has actually expanded, driven by
/// `TreeState`'s own expansion set instead of the raw filesystem shape. That
/// needs `TreeState` threaded into `build_tree_items`'s public signature and
/// `TreeItemCache`'s version bumped on expand/collapse, both of which live
/// in `app.rs` (outside this file).
const MAX_FOLDER_TREE_ENTRIES: usize = 2_000;

/// Every row reserves the same display-cell width for its node-kind icon.
/// Emoji glyphs such as the robot/folder/pen often occupy two terminal
/// cells, while shell and activity glyphs occupy one; padding by display
/// width rather than byte or character count keeps the following text in a
/// single vertical column on terminals that support those glyphs.
const NODE_ICON_COLUMN_WIDTH: usize = 3;

/// Agent activity is a separate visual dimension from a row's node kind.
/// Keeping its column on every row means a shell/group/editor title starts
/// at the same horizontal position as an agent's class label.
const ACTIVITY_ICON_COLUMN_WIDTH: usize = 2;

/// Panel width (the outer tree `Rect`'s column count, borders and icon
/// columns included) at or above which a pane shows its long-form title
/// instead of its short-form one -- see `crate::naming::DualTitle` and
/// `display_title` below. Sits between the tree panel's default collapsed
/// width (33 columns: `layout::DEFAULT_TREE_WIDTH` plus the shared-border
/// column) and its default expanded (focused/hovered) width of 65 columns,
/// so the default collapsed<->expanded transition is exactly the
/// thin<->wide switch this constant draws -- see `crate::layout::TreeWidthAnimation`.
/// A user-configured wider base width naturally shows the long title even
/// while collapsed, which is correct: the panel genuinely has the room.
const WIDE_TITLE_MIN_COLUMNS: u16 = 40;

/// The two-row action strip is always reserved at the bottom of the tree
/// panel so a mouse move can reveal it without shifting the tree rows: a
/// glyph-button row, plus a caption row that names whichever button the
/// pointer is actually over (blank otherwise, so it never nags).
const TOOLBAR_HEIGHT: u16 = 2;
const TOOLBAR_BUTTON_WIDTH: u16 = 4;
/// Two display cells hold each emoji and the third is a guaranteed blank gap
/// before the next action. Drawing clears the complete combined strip first,
/// so this gap can never reveal title text rendered underneath.
pub(crate) const ROW_ACTION_WIDTH: u16 = 3;
const ROW_ACTION_TOTAL_WIDTH: u16 = ROW_ACTION_WIDTH * ROW_ACTION_COUNT;
/// A visibly empty, single-cell spacer that the terminal backend must emit.
/// Unlike an ASCII space, it cannot be optimized away while diff-rendering
/// over an existing title character, which is the bleed-through this strip
/// specifically prevents.
const ROW_ACTION_BLANK: &str = "\u{00a0}";
/// Edit, up, down, close, then retitle -- reserved as the trailing cells of
/// a hovered row (see `row_action_at`/`draw_row_actions`).
pub(crate) const ROW_ACTION_COUNT: u16 = 5;

/// Actions available from the tree toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeToolbarAction {
    Shell,
    Claude,
    Codex,
    Editor,
    Board,
    Group,
    Split,
    Folder,
    Settings,
}

impl TreeToolbarAction {
    /// Ordered left-aligned creation actions. Settings is intentionally not
    /// part of this list because its geometry is anchored independently.
    const LEFT_ALIGNED: [Self; 8] = [
        Self::Shell,
        Self::Claude,
        Self::Codex,
        Self::Editor,
        Self::Board,
        Self::Group,
        Self::Split,
        Self::Folder,
    ];

    /// Single glyph shown on the button, reusing the same symbol the tree
    /// already uses for that node kind elsewhere (group/editor) so the
    /// toolbar and the tree read as one visual language rather than two.
    const fn glyph(self) -> &'static str {
        match self {
            Self::Shell => TERMINAL_ICON,
            Self::Claude => "Ⓒ",
            Self::Codex => "Ⓧ",
            Self::Editor => TEXT_EDITOR_ICON,
            Self::Board => "▦",
            Self::Group => "\u{1F4C1}",
            Self::Split => "▥",
            Self::Folder => "\u{1F5C0}",
            Self::Settings => SETTINGS_ICON,
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
            Self::Board => Color::Cyan,
            Self::Group => Color::Rgb(0x7a, 0xa2, 0xf7),
            Self::Split => Color::Rgb(0xbb, 0x9a, 0xf7),
            Self::Folder => Color::Cyan,
            Self::Settings => Color::Gray,
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
            Self::Board => "new board",
            Self::Group => "new group",
            Self::Split => "new split view",
            Self::Folder => "open folder",
            Self::Settings => "settings",
        }
    }
}

/// An action selected through the row-level hover controls (edit-pen,
/// up/down move arrows, close, then retitle).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeRowAction {
    Rename,
    MoveUp,
    MoveDown,
    Close,
    /// Manually (re-)triggers `App::action_request_retitle` -- an
    /// LLM-inferred title on demand, for any pane that has an automatic
    /// title source (an agent pane, or a plain terminal -- see
    /// `crate::terminal_naming`). A no-op with a status message for
    /// anything else (a group, an editor pane); see
    /// `App::action_request_retitle`'s own doc comment.
    Retitle,
}

impl TreeRowAction {
    /// Ordered set used for both rendering and hit testing -- left to right.
    pub(crate) const ALL: [Self; 5] = [
        Self::Rename,
        Self::MoveUp,
        Self::MoveDown,
        Self::Close,
        Self::Retitle,
    ];

    const fn glyph(self) -> &'static str {
        match self {
            Self::Rename => theme::PEN_ICON,
            Self::MoveUp => "🔼",
            Self::MoveDown => "🔽",
            Self::Close => "🚫",
            Self::Retitle => "♻️",
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
    /// Node id -> `elapsed_ms`-relative offset (ms) at which this client
    /// first observed the node existing -- see `App::recently_created`.
    pub recently_created: &'a HashMap<NodeId, u128>,
    /// Resolved user-global presentation settings for detected agent types.
    pub agent_identifiers: &'a AgentIdentifierSettings,
    pub hover: TreeHoverState,
}

/// Builds the full recursive `TreeItem` tree from the root group's
/// children (the root group itself is never shown as a node -- its
/// children are ilium's top-level groups/panes). `elapsed_ms` drives the
/// Working spinner, WaitingBackground clock, and Done pulse animations.
/// `panel_width` is the tree panel's current outer `Rect` width, used to
/// pick each pane's short- vs long-form title -- see `display_title`.
pub fn build_tree_items(
    tree: &Tree,
    elapsed_ms: u128,
    titles_loading: &HashSet<NodeId>,
    recently_created: &HashMap<NodeId, u128>,
    agent_identifiers: &AgentIdentifierSettings,
    panel_width: u16,
) -> Vec<TreeItem<'static, NodeId>> {
    build_children(
        tree,
        ROOT_ID,
        elapsed_ms,
        titles_loading,
        recently_created,
        agent_identifiers,
        panel_width,
    )
}

/// Recursively builds `TreeItem`s for every child of `parent`.
fn build_children(
    tree: &Tree,
    parent: NodeId,
    elapsed_ms: u128,
    titles_loading: &HashSet<NodeId>,
    recently_created: &HashMap<NodeId, u128>,
    agent_identifiers: &AgentIdentifierSettings,
    panel_width: u16,
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
            tree.get(child_id).map(|node| {
                build_item(
                    tree,
                    node,
                    elapsed_ms,
                    titles_loading,
                    recently_created,
                    agent_identifiers,
                    panel_width,
                )
            })
        })
        .collect()
}

/// Builds one `TreeItem` (recursing into children for a Group).
fn build_item(
    tree: &Tree,
    node: &Node,
    elapsed_ms: u128,
    titles_loading: &HashSet<NodeId>,
    recently_created: &HashMap<NodeId, u128>,
    agent_identifiers: &AgentIdentifierSettings,
    panel_width: u16,
) -> TreeItem<'static, NodeId> {
    let flash_on = should_flash(node.id, recently_created, elapsed_ms);
    match &node.kind {
        NodeKind::Container(container) => {
            let children = build_children(
                tree,
                node.id,
                elapsed_ms,
                titles_loading,
                recently_created,
                agent_identifiers,
                panel_width,
            );
            let icon = match container.split_orientation() {
                Some(ilium_core::SplitOrientation::Vertical) => "▥",
                Some(ilium_core::SplitOrientation::Horizontal) => "▤",
                None => "\u{1F4C1}",
            };
            let label = node_label(Span::raw(icon), None, Span::raw(node.name.clone()));
            let label = apply_recent_pulse(label, flash_on);
            // `NodeId`s are unique across the whole `Tree` (its own
            // invariant), so they can't collide among siblings here --
            // the `Result` this returns is unreachable in practice.
            TreeItem::new(node.id, label, children).expect("sibling NodeIds are always unique")
        }
        NodeKind::Pane { status, .. } => {
            let label = pane_label(
                status,
                display_title(node, panel_width),
                elapsed_ms,
                titles_loading.contains(&node.id),
                agent_identifiers,
            );
            TreeItem::new_leaf(node.id, apply_recent_pulse(label, flash_on))
        }
        NodeKind::Folder { path } => {
            let label = node_label(Span::raw("\u{1F5C0}"), None, Span::raw(node.name.clone()));
            // A fresh budget per folder node/render -- see
            // `MAX_FOLDER_TREE_ENTRIES` for why this exists.
            let mut budget = MAX_FOLDER_TREE_ENTRIES;
            TreeItem::new(
                node.id,
                apply_recent_pulse(label, flash_on),
                folder_children(node.id, path, &mut budget),
            )
            .expect("folder node ids are unique")
        }
    }
}

/// Virtual rows occupy the high-id range and never cross the IPC boundary.
/// This keeps filesystem paths out of the shared tree while allowing the
/// existing tree widget to own expansion, selection, and scrolling.
fn virtual_folder_node_id(root: NodeId, path: &Path) -> NodeId {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    root.hash(&mut hasher);
    path.hash(&mut hasher);
    NodeId((1_u64 << 63) | (hasher.finish() & !(1_u64 << 63)))
}

fn folder_children(
    root: NodeId,
    path: &Path,
    budget: &mut usize,
) -> Vec<TreeItem<'static, NodeId>> {
    if *budget == 0 {
        // Read no further once the whole subtree's budget is spent --
        // avoids the `read_dir` syscall entirely for branches beyond the cap.
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    // `.take(*budget)` bounds how many `DirEntry`s this level ever reads and
    // sorts, not just how many `TreeItem`s get built from them -- without it
    // a single pathologically large flat directory (millions of files) would
    // still fully enumerate and sort every entry before the per-item budget
    // check below ever ran.
    let mut entries: Vec<_> = entries
        .flatten()
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .take(*budget)
        .collect();
    entries.sort_by(|left, right| {
        let left_is_dir = left.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        let right_is_dir = right.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        right_is_dir
            .cmp(&left_is_dir)
            .then_with(|| left.file_name().cmp(&right.file_name()))
    });
    let mut children = Vec::with_capacity(entries.len());
    for entry in entries {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let path = entry.path();
        let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        let label = node_label(
            Span::raw(if is_dir { "\u{1F4C1}" } else { "\u{1F4C4}" }),
            None,
            Span::raw(entry.file_name().to_string_lossy().into_owned()),
        );
        let id = virtual_folder_node_id(root, &path);
        let item = if is_dir {
            TreeItem::new(id, label, folder_children(root, &path, budget))
                .expect("filesystem ids are unique")
        } else {
            TreeItem::new_leaf(id, label)
        };
        children.push(item);
    }
    children
}

/// Resolves a virtual file/folder row to its current path. Re-reading makes
/// a click safe when the filesystem changed after the previous render.
pub fn folder_entry_path(tree: &Tree, id: NodeId) -> Option<(NodeId, PathBuf, bool)> {
    for node in tree.all_ids().filter_map(|node_id| tree.get(node_id)) {
        let NodeKind::Folder { path } = &node.kind else {
            continue;
        };
        if let Some(found) = find_folder_entry(node.id, path, id) {
            return Some(found);
        }
    }
    None
}

fn find_folder_entry(root: NodeId, path: &Path, target: NodeId) -> Option<(NodeId, PathBuf, bool)> {
    for entry in std::fs::read_dir(path).ok()?.flatten() {
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let entry_path = entry.path();
        let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        if virtual_folder_node_id(root, &entry_path) == target {
            return Some((root, entry_path, is_dir));
        }
        if is_dir {
            if let Some(found) = find_folder_entry(root, &entry_path, target) {
                return Some(found);
            }
        }
    }
    None
}

/// Picks a pane's short- or long-form title text for `panel_width`,
/// falling back to `node.name` whenever the panel is wide enough or no
/// distinct short form exists (a plain user rename, or the shell-command-
/// echo titler -- see `Node::short_name`'s doc comment).
fn display_title(node: &Node, panel_width: u16) -> &str {
    if panel_width < WIDE_TITLE_MIN_COLUMNS {
        if let Some(short_name) = &node.short_name {
            return short_name;
        }
    }
    &node.name
}

/// Whether `node_id`'s label should render mid-flash right now: it must be
/// present in `recently_created` (this client has actually observed it as a
/// fresh node -- see that field's doc comment) and its age must fall within
/// an "on" half-cycle of the pulse. Kept as its own pure function, separate
/// from `build_item`'s widget construction, so this decision is testable
/// without needing to inspect a constructed `TreeItem`'s otherwise-opaque
/// internal style (`tui_tree_widget::TreeItem` exposes neither its text nor
/// a style getter).
fn should_flash(
    node_id: NodeId,
    recently_created: &HashMap<NodeId, u128>,
    elapsed_ms: u128,
) -> bool {
    recently_created
        .get(&node_id)
        .map(|&created_offset_ms| elapsed_ms.saturating_sub(created_offset_ms))
        .is_some_and(is_recently_created_flash_on)
}

/// Returns `true` if `age_ms` (time since this node's creation was first
/// observed by this client) falls within an "on" half-cycle of the
/// creation-pulse flash, and hasn't yet exceeded the flash's total window.
fn is_recently_created_flash_on(age_ms: u128) -> bool {
    age_ms < RECENTLY_CREATED_PULSE_MS
        && (age_ms / RECENTLY_CREATED_PULSE_PHASE_MS).is_multiple_of(2)
}

/// Flashes `line` by reversing video on every span, when `flash_on` is
/// true. Reversing whatever fg/bg a span already has (rather than
/// overlaying a fixed highlight color) keeps the flash readable no matter
/// what color/status a node already renders in, without needing a new
/// accent color that might clash with an existing one.
fn apply_recent_pulse(line: Line<'static>, flash_on: bool) -> Line<'static> {
    if !flash_on {
        return line;
    }
    Line::from(
        line.spans
            .into_iter()
            .map(|span| {
                let style = span.style.add_modifier(Modifier::REVERSED);
                Span::styled(span.content, style)
            })
            .collect::<Vec<_>>(),
    )
}

/// Builds the icon+color-prefixed label for a single pane, based on its
/// current `PaneStatus`. `elapsed_ms` selects the current animation frame
/// for `Working` (spinning braille dots), `WaitingBackground` (a slower
/// half-hour clock sweep), and `Done` (pulsing bell).
fn pane_label(
    status: &PaneStatus,
    name: &str,
    elapsed_ms: u128,
    is_title_loading: bool,
    agent_identifiers: &AgentIdentifierSettings,
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
        PaneStatus::PlainShell => node_label(
            Span::styled(TERMINAL_ICON, Style::new().fg(Color::Gray)),
            None,
            Span::raw(title()),
        ),
        PaneStatus::Agent(class, AgentActivity::Working) => {
            let frame_index = (elapsed_ms / SPINNER_FRAME_MS) as usize % SPINNER_FRAMES.len();
            let glyph = SPINNER_FRAMES[frame_index];
            node_label(
                Span::raw(agent_node_icon(class, agent_identifiers)),
                Some(Span::raw(glyph.to_string())),
                Span::raw(agent_title(class, &title(), agent_identifiers.mode)),
            )
        }
        PaneStatus::Agent(class, AgentActivity::WaitingBackground) => {
            // Slow clock sweep, not bold -- distinct from `Working`'s fast
            // braille churn (calmer, lower-urgency animation) and from
            // `WaitingApproval`'s bold "needs you" styling, since this is
            // "blocked on background work the agent itself dispatched," not
            // blocked on the user.
            let frame_index =
                (elapsed_ms / BACKGROUND_FRAME_MS) as usize % BACKGROUND_CLOCK_FRAMES.len();
            let glyph = BACKGROUND_CLOCK_FRAMES[frame_index];
            node_label(
                Span::raw(agent_node_icon(class, agent_identifiers)),
                Some(Span::raw(glyph.to_string())),
                Span::raw(agent_title(class, &title(), agent_identifiers.mode)),
            )
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
            node_label(
                Span::raw(agent_node_icon(class, agent_identifiers)),
                Some(Span::styled("\u{1F514}", style)),
                Span::styled(
                    format!(
                        "{} — done",
                        agent_title(class, &title, agent_identifiers.mode)
                    ),
                    style,
                ),
            )
        }
        PaneStatus::Agent(class, AgentActivity::WaitingApproval) => {
            // Bold, not colored -- every agent status shares the same base
            // text color; boldness alone signals "needs you."
            let style = Style::new().add_modifier(Modifier::BOLD);
            node_label(
                Span::raw(agent_node_icon(class, agent_identifiers)),
                Some(Span::styled("?", style)),
                Span::styled(agent_title(class, &title(), agent_identifiers.mode), style),
            )
        }
        PaneStatus::Agent(class, AgentActivity::Idle) => node_label(
            Span::raw(agent_node_icon(class, agent_identifiers)),
            Some(Span::raw("\u{25cf}")),
            Span::raw(agent_title(class, &title(), agent_identifiers.mode)),
        ),
        PaneStatus::Editor { dirty: true } => node_label(
            Span::styled(TEXT_EDITOR_ICON, Style::new().fg(Color::Magenta)),
            None,
            Span::styled(format!("{name}*"), Style::new().fg(Color::Magenta)),
        ),
        PaneStatus::Editor { dirty: false } => node_label(
            Span::raw(TEXT_EDITOR_ICON),
            None,
            Span::raw(name.to_string()),
        ),
        PaneStatus::Board => node_label(
            Span::styled("▦", Style::new().fg(Color::Cyan)),
            None,
            Span::styled(name.to_string(), Style::new().fg(Color::Cyan)),
        ),
    }
}

/// Builds a row label with fixed node-kind and activity icon columns, then
/// the descriptive text. The columns are based on terminal display cells,
/// not Rust string length, so a double-width emoji cannot shift one row's
/// text relative to another's.
fn node_label(
    node_icon: Span<'static>,
    activity_icon: Option<Span<'static>>,
    text: Span<'static>,
) -> Line<'static> {
    Line::from(vec![
        fixed_width_icon_span(node_icon, NODE_ICON_COLUMN_WIDTH),
        fixed_width_icon_span(
            activity_icon.unwrap_or_default(),
            ACTIVITY_ICON_COLUMN_WIDTH,
        ),
        text,
    ])
}

/// Pads an icon span without losing its color or emphasis, so status cues
/// stay intact while every label shares the same text start column.
fn fixed_width_icon_span(icon: Span<'static>, column_width: usize) -> Span<'static> {
    Span::styled(
        fixed_width_icon_column(&icon.content, column_width),
        icon.style,
    )
}

/// Pads `icon` to exactly `column_width` terminal display cells. If a
/// terminal renders a glyph wider than its reserved column, leave it intact
/// rather than truncating a Unicode sequence; all project icons fit today.
fn fixed_width_icon_column(icon: &str, column_width: usize) -> String {
    let padding_width = column_width.saturating_sub(UnicodeWidthStr::width(icon));
    format!("{icon}{}", " ".repeat(padding_width))
}

/// Human-readable name for an `AgentClass`, shown as a prefix before the
/// pane's own name.
fn agent_class_label(class: &AgentClass) -> String {
    match class {
        AgentClass::Claude => "Claude:".to_string(),
        AgentClass::Codex => "Codex:".to_string(),
        AgentClass::Other(name) => format!("{name}:"),
    }
}

/// Chooses the first fixed-width column's glyph. Name/letter/hidden modes
/// deliberately leave it blank; their representation lives in the text
/// prefix, while the activity column remains independent and visible.
fn agent_node_icon(class: &AgentClass, settings: &AgentIdentifierSettings) -> &'static str {
    if settings.mode != AgentIdentifierMode::Icon {
        return "";
    }
    match class {
        AgentClass::Claude => settings.claude_icon.glyph(),
        AgentClass::Codex => settings.codex_icon.glyph(),
        AgentClass::Other(_) => AGENT_ICON,
    }
}

/// Produces the optional type prefix that precedes a pane's inferred/user
/// title. Codex uses `X:` so Claude and Codex remain distinguishable at one
/// letter, matching the settings screen's preview language.
fn agent_text_prefix(class: &AgentClass, mode: AgentIdentifierMode) -> String {
    match mode {
        AgentIdentifierMode::FullName => agent_class_label(class),
        AgentIdentifierMode::Letter => match class {
            AgentClass::Claude => "C:".to_string(),
            AgentClass::Codex => "X:".to_string(),
            AgentClass::Other(name) => name
                .chars()
                .next()
                .map(|letter| format!("{}:", letter.to_uppercase()))
                .unwrap_or_default(),
        },
        AgentIdentifierMode::Icon | AgentIdentifierMode::Hidden => String::new(),
    }
}

/// Joins the selected type representation to a pane title without leaving a
/// stray leading space in icon/hidden mode.
fn agent_title(class: &AgentClass, title: &str, mode: AgentIdentifierMode) -> String {
    let prefix = agent_text_prefix(class, mode);
    if prefix.is_empty() {
        title.to_string()
    } else {
        format!("{prefix} {title}")
    }
}

/// Returns the interior list region, excluding the persistent two-row hover
/// toolbar. Labels use this complete width; row actions temporarily overlay
/// only the hovered row's trailing cells.
pub fn list_area(area: Rect) -> Rect {
    let inner = theme::block(false).inner(area);
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(TOOLBAR_HEIGHT)])
        .split(inner)[0]
}

/// Returns the trailing strip row actions overlay while a row is hovered.
/// Keeping this geometry shared by drawing and hit testing preserves the
/// click targets without permanently taking these columns away from labels.
fn row_action_strip_area(area: Rect) -> Rect {
    let list = list_area(area);
    if list.width < ROW_ACTION_TOTAL_WIDTH {
        // Below this width, neither drawing nor hit testing can fit every
        // action. Return an empty strip so the label keeps the whole row.
        return Rect::new(list.right(), list.y, 0, list.height);
    }
    Rect::new(
        list.right().saturating_sub(ROW_ACTION_TOTAL_WIDTH),
        list.y,
        ROW_ACTION_TOTAL_WIDTH,
        list.height,
    )
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
    toolbar_button_rects(area)
        .into_iter()
        .find_map(|(action, button_area)| button_area.contains(position).then_some(action))
}

/// Button rectangles shared by drawing and hit testing. Creation actions
/// flow from the left; Settings is anchored at the far right and never moves
/// as the panel expands. On a narrow panel, only left actions that fit before
/// Settings are included.
pub fn toolbar_button_rects(area: Rect) -> Vec<(TreeToolbarAction, Rect)> {
    const BUTTON_VISIBLE_WIDTH: u16 = TOOLBAR_BUTTON_WIDTH - 1;
    let toolbar = toolbar_area(area);
    if toolbar.width < BUTTON_VISIBLE_WIDTH {
        return Vec::new();
    }
    let settings_area = Rect::new(
        toolbar.right() - BUTTON_VISIBLE_WIDTH,
        toolbar.y,
        BUTTON_VISIBLE_WIDTH,
        toolbar.height,
    );
    let mut buttons = Vec::with_capacity(TreeToolbarAction::LEFT_ALIGNED.len() + 1);
    for (index, action) in TreeToolbarAction::LEFT_ALIGNED.iter().enumerate() {
        let x = toolbar.x + index as u16 * TOOLBAR_BUTTON_WIDTH;
        let button_area = Rect::new(x, toolbar.y, BUTTON_VISIBLE_WIDTH, toolbar.height);
        if button_area.right() > settings_area.x {
            break;
        }
        buttons.push((*action, button_area));
    }
    buttons.push((TreeToolbarAction::Settings, settings_area));
    buttons
}

/// Whether `id` has an automatic title source `TreeRowAction::Retitle` can
/// actually (re-)trigger -- an agent pane, or a plain-shell terminal (see
/// `crate::terminal_naming`). A group has no title-inference worker at all,
/// and an editor pane's title is just its file name, not LLM-inferred --
/// both are filtered out of the row-action strip entirely (rendering and
/// hit-testing alike) rather than left clickable only to hit
/// `App::action_request_retitle`'s "doesn't support automatic titling"
/// fallback.
fn row_supports_retitle(tree: &Tree, id: NodeId) -> bool {
    matches!(
        tree.get(id).map(|node| &node.kind),
        Some(NodeKind::Pane {
            status: PaneStatus::Agent(..) | PaneStatus::PlainShell,
            ..
        })
    )
}

/// Returns the row actions applicable to `id`'s node kind/status, in the
/// same left-to-right order as `TreeRowAction::ALL` -- see
/// `row_supports_retitle` for why `Retitle` is sometimes excluded.
fn applicable_row_actions(tree: &Tree, id: NodeId) -> &'static [TreeRowAction] {
    if row_supports_retitle(tree, id) {
        &TreeRowAction::ALL
    } else {
        &TreeRowAction::ALL[..TreeRowAction::ALL.len() - 1]
    }
}

/// Returns the row action at `position` for a hovered row, using the
/// far-right cells of the list for the edit/up/down/close/retitle targets --
/// filtered to whichever of those actually apply to `id` (see
/// `applicable_row_actions`), so a click that lands where `Retitle` would be
/// on an eligible row is a no-op on a group or editor row instead.
pub fn row_action_at(
    tree: &Tree,
    id: NodeId,
    area: Rect,
    row: u16,
    position: Position,
) -> Option<TreeRowAction> {
    let list = list_area(area);
    if row < list.y || row >= list.bottom() || position.y != row {
        return None;
    }
    let action_strip = row_action_strip_area(area);
    if action_strip.width < ROW_ACTION_TOTAL_WIDTH || !action_strip.contains(position) {
        return None;
    }
    let index = usize::from((position.x - action_strip.x) / ROW_ACTION_WIDTH);
    let action = TreeRowAction::ALL.get(index).copied()?;
    applicable_row_actions(tree, id)
        .contains(&action)
        .then_some(action)
}

/// Returns a node only when `position` is on one of the actually visible
/// one-line rows, never on blank space below the final item. `items` is a
/// previously built `build_tree_items` result -- hit-testing only needs
/// node identifiers and row structure, neither of which depends on label
/// text (elapsed-time animation, loading spinners), so the caller is free
/// to reuse the same `items` across every mouse-move as long as the tree
/// itself hasn't changed -- see `App::tree_node_at`/`TreeItemCache`.
pub fn node_at_position(
    items: &[TreeItem<'static, NodeId>],
    state: &TreeState<NodeId>,
    area: Rect,
    position: Position,
) -> Option<TreeNodeHit> {
    let list = list_area(area);
    if !list.contains(position) {
        return None;
    }
    let visible_index = state.get_offset() + usize::from(position.y.saturating_sub(list.y));
    let id = state
        .flatten(items)
        .get(visible_index)?
        .identifier
        .last()
        .copied()?;
    Some(TreeNodeHit {
        id,
        row: position.y,
    })
}

/// Caches the structural `TreeItem` list built with fixed/empty
/// animation inputs (`elapsed_ms: 0`, no loading/recently-created state) --
/// exactly what hit-testing needs and nothing that varies frame-to-frame.
/// Rebuilding only happens when `version` (the caller's tree-change
/// counter) actually changes, so a mouse-move flood over an unchanged tree
/// costs one `Vec` lookup instead of a full recursive rebuild with a fresh
/// heap-allocated label per node. See `App::tree_hit_test_cache`.
#[derive(Default)]
pub struct TreeItemCache {
    version: Option<u64>,
    items: Vec<TreeItem<'static, NodeId>>,
}

impl TreeItemCache {
    /// Returns the cached items for `tree`, rebuilding first if `version`
    /// doesn't match the last build this cache served.
    pub fn get_or_build(&mut self, tree: &Tree, version: u64) -> &[TreeItem<'static, NodeId>] {
        if self.version != Some(version) {
            // `panel_width` only selects which title text a label carries;
            // hit-testing only needs row structure and node identifiers, so
            // any width is correct here.
            self.items = build_tree_items(
                tree,
                0,
                &HashSet::new(),
                &HashMap::new(),
                &AgentIdentifierSettings::default(),
                0,
            );
            self.version = Some(version);
        }
        &self.items
    }
}

/// Whether any entry in `recently_created` is still inside its flash
/// window at `elapsed_ms` -- used by `App::has_active_animation` to decide
/// whether a periodic tick still needs to force a redraw for the flash
/// animation, without duplicating `is_recently_created_flash_on`'s
/// half-cycle phase logic (which only matters for rendering, not for "is
/// anything still animating at all").
pub(crate) fn any_recently_created_within_window(
    recently_created: &HashMap<NodeId, u128>,
    elapsed_ms: u128,
) -> bool {
    recently_created.values().any(|&created_offset_ms| {
        elapsed_ms.saturating_sub(created_offset_ms) < RECENTLY_CREATED_PULSE_MS
    })
}

/// Draws the tree panel into `area`, bordered brighter when `focused`.
/// `elapsed_ms` drives the Working spinner, WaitingBackground clock, and Done pulse animations.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    tree: &Tree,
    state: &mut TreeState<NodeId>,
    options: TreeRenderOptions<'_>,
) {
    let items = build_tree_items(
        tree,
        options.elapsed_ms,
        options.titles_loading,
        options.recently_created,
        options.agent_identifiers,
        area.width,
    );
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
        draw_row_actions(frame, area, hit.row, applicable_row_actions(tree, hit.id));
    }
    if is_toolbar_visible(options.focused, options.hover.toolbar_hovered) {
        draw_toolbar(frame, area, options.hover.toolbar_action);
    }
}

/// Footer actions stay visible whenever the tree has keyboard focus, or while
/// the pointer is over the footer itself.
const fn is_toolbar_visible(tree_focused: bool, toolbar_hovered: bool) -> bool {
    tree_focused || toolbar_hovered
}

/// Produces the compact left-panel title from Ilium's product name and the
/// project-local metadata. The self-hosting case uses the requested squared
/// mark instead of the visually redundant `Ilium: Ilium`.
pub fn sidebar_title(
    project_name: Option<&str>,
    is_project_name_loading: bool,
    elapsed_ms: u128,
) -> String {
    match project_name {
        Some("Ilium") => "Ilium²".to_string(),
        Some(project_name) => format!("Ilium: {project_name}"),
        None if is_project_name_loading => {
            let frame_index = (elapsed_ms / SPINNER_FRAME_MS) as usize % SPINNER_FRAMES.len();
            format!("Ilium: {}", SPINNER_FRAMES[frame_index])
        }
        None => "Ilium".to_string(),
    }
}

#[cfg(test)]
mod project_title_tests {
    use super::sidebar_title;

    #[test]
    fn sidebar_title_includes_project_name_or_uses_the_self_hosting_mark() {
        assert_eq!(sidebar_title(None, false, 0), "Ilium");
        assert_eq!(sidebar_title(Some("Money"), false, 0), "Ilium: Money");
        assert_eq!(sidebar_title(Some("Ilium"), false, 0), "Ilium²");
        assert_eq!(sidebar_title(None, true, 0), "Ilium: ⠋");
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

/// Draws the edit/up/down/close/retitle controls over the hovered tree
/// row's trailing cells, left to right -- only those present in `actions`
/// (see `applicable_row_actions`), so e.g. a group or editor row never
/// shows the retitle glyph it has no title-inference worker to back.
fn draw_row_actions(frame: &mut Frame, area: Rect, row: u16, actions: &[TreeRowAction]) {
    let action_strip = row_action_strip_area(area);
    if action_strip.width < ROW_ACTION_TOTAL_WIDTH {
        return;
    }
    let style = theme::selected_style().add_modifier(Modifier::BOLD);

    // Build one complete fixed-width line rather than overlaying five small
    // widgets. Clearing exactly this hovered row keeps title glyphs out of
    // the icon gaps while leaving every non-hovered row's full label visible.
    let controls_area = Rect::new(action_strip.x, row, ROW_ACTION_TOTAL_WIDTH, 1);
    frame.render_widget(Clear, controls_area);

    let action_spans = TreeRowAction::ALL
        .iter()
        .map(|action| {
            let glyph = if actions.contains(action) {
                action.glyph()
            } else {
                ""
            };
            let blank_width =
                usize::from(ROW_ACTION_WIDTH).saturating_sub(UnicodeWidthStr::width(glyph));
            Span::styled(
                format!("{glyph}{}", ROW_ACTION_BLANK.repeat(blank_width)),
                style,
            )
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(Line::from(action_spans)), controls_area);
}

/// Draws the five creation buttons plus a caption row naming whichever one
/// the pointer currently sits over. Every button keeps its own accent color
/// at all times (so the five read as distinct actions on sight, not one
/// repeated shape); the hovered button additionally brightens and bolds to
/// confirm exactly what a click would do before it happens.
fn draw_toolbar(frame: &mut Frame, area: Rect, hovered: Option<TreeToolbarAction>) {
    let toolbar = toolbar_area(area);
    let caption_row = Rect::new(toolbar.x, toolbar.y + 1, toolbar.width, 1);

    for (action, button_area) in toolbar_button_rects(area) {
        let is_hovered = hovered == Some(action);
        let style = if is_hovered {
            Style::new()
                .fg(theme::accent_fg())
                .bg(action.accent())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(action.accent())
        };
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
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use super::*;

    #[test]
    fn node_label_aligns_text_after_narrow_and_wide_icons() {
        let labels = [
            node_label(Span::raw(">"), None, Span::raw("shell")),
            node_label(Span::raw("\u{1F4C1}"), None, Span::raw("group")),
            node_label(
                Span::raw(AGENT_ICON),
                Some(Span::raw("\u{25cf}")),
                Span::raw("Codex: agent"),
            ),
        ];

        for label in labels {
            let icon_width = label.spans[..2]
                .iter()
                .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
                .sum::<usize>();
            assert_eq!(
                icon_width,
                NODE_ICON_COLUMN_WIDTH + ACTIVITY_ICON_COLUMN_WIDTH
            );
        }
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn agent_identifier_modes_render_full_names_letters_icons_or_nothing() {
        let status = PaneStatus::Agent(AgentClass::Claude, AgentActivity::Idle);
        let mut settings = AgentIdentifierSettings::default();

        let full_name = pane_label(&status, "Fix auth", 0, false, &settings);
        assert_eq!(full_name.spans[0].content.trim(), "");
        assert!(line_text(&full_name).ends_with("Claude: Fix auth"));

        settings.mode = AgentIdentifierMode::Letter;
        let letter = pane_label(&status, "Fix auth", 0, false, &settings);
        assert_eq!(letter.spans[0].content.trim(), "");
        assert!(line_text(&letter).ends_with("C: Fix auth"));

        settings.mode = AgentIdentifierMode::Icon;
        settings.claude_icon = crate::config::ClaudeAgentIcon::Lobster;
        let icon = pane_label(&status, "Fix auth", 0, false, &settings);
        assert_eq!(icon.spans[0].content.trim_end(), "🦞");
        assert!(line_text(&icon).ends_with("Fix auth"));
        assert!(!line_text(&icon).contains("Claude:"));

        settings.mode = AgentIdentifierMode::Hidden;
        let hidden = pane_label(&status, "Fix auth", 0, false, &settings);
        assert_eq!(hidden.spans[0].content.trim(), "");
        assert!(line_text(&hidden).ends_with("Fix auth"));
        assert!(!line_text(&hidden).contains("Claude:"));
    }

    #[test]
    fn codex_letter_is_x_and_every_curated_icon_fits_the_fixed_column() {
        let status = PaneStatus::Agent(AgentClass::Codex, AgentActivity::Working);
        let mut settings = AgentIdentifierSettings {
            mode: AgentIdentifierMode::Letter,
            ..AgentIdentifierSettings::default()
        };
        let letter = pane_label(&status, "Review", 0, false, &settings);
        assert!(line_text(&letter).ends_with("X: Review"));

        settings.mode = AgentIdentifierMode::Icon;
        for icon in crate::config::CodexAgentIcon::ALL {
            settings.codex_icon = icon;
            let line = pane_label(&status, "Review", 0, false, &settings);
            assert_eq!(line.spans[0].content.trim_end(), icon.glyph());
            assert_eq!(
                UnicodeWidthStr::width(line.spans[0].content.as_ref()),
                NODE_ICON_COLUMN_WIDTH
            );
        }
        for icon in crate::config::ClaudeAgentIcon::ALL {
            assert!(UnicodeWidthStr::width(icon.glyph()) <= NODE_ICON_COLUMN_WIDTH);
        }
    }

    #[test]
    fn hidden_agent_identifier_preserves_every_activity_indicator() {
        let settings = AgentIdentifierSettings {
            mode: AgentIdentifierMode::Hidden,
            ..AgentIdentifierSettings::default()
        };
        for activity in [
            AgentActivity::Working,
            AgentActivity::WaitingBackground,
            AgentActivity::WaitingApproval,
            AgentActivity::Idle,
            AgentActivity::Done,
        ] {
            let line = pane_label(
                &PaneStatus::Agent(AgentClass::Claude, activity),
                "Task",
                0,
                false,
                &settings,
            );
            assert_eq!(line.spans[0].content.trim(), "");
            assert!(!line.spans[1].content.trim().is_empty());
            assert!(!line_text(&line).contains("Claude:"));
        }
    }

    #[test]
    fn left_panel_uses_requested_icons_without_changing_the_rename_action() {
        assert_eq!(theme::PEN_ICON, "✏️");
        assert_eq!(TreeToolbarAction::Shell.glyph(), TERMINAL_ICON);
        assert_eq!(TreeToolbarAction::Editor.glyph(), TEXT_EDITOR_ICON);
        assert_eq!(TreeToolbarAction::Settings.glyph(), SETTINGS_ICON);
        assert_eq!(TreeRowAction::Rename.glyph(), theme::PEN_ICON);
        assert_eq!(TreeRowAction::MoveUp.glyph(), "🔼");
        assert_eq!(TreeRowAction::MoveDown.glyph(), "🔽");
        assert_eq!(TreeRowAction::Close.glyph(), "🚫");
        assert_eq!(TreeRowAction::Retitle.glyph(), "♻️");
        for action in TreeRowAction::ALL {
            assert!(
                UnicodeWidthStr::width(action.glyph()) < usize::from(ROW_ACTION_WIDTH),
                "{:?} must leave one blank display cell in its slot",
                action
            );
        }
        assert!(UnicodeWidthStr::width(TERMINAL_ICON) < usize::from(TOOLBAR_BUTTON_WIDTH));
        assert!(UnicodeWidthStr::width(TEXT_EDITOR_ICON) < usize::from(TOOLBAR_BUTTON_WIDTH));
        assert!(UnicodeWidthStr::width(SETTINGS_ICON) < usize::from(TOOLBAR_BUTTON_WIDTH));

        let shell_label = pane_label(
            &PaneStatus::PlainShell,
            "shell",
            0,
            false,
            &AgentIdentifierSettings::default(),
        );
        assert_eq!(shell_label.spans[0].content.trim_end(), TERMINAL_ICON);

        for is_dirty in [false, true] {
            let label = pane_label(
                &PaneStatus::Editor { dirty: is_dirty },
                "notes.md",
                0,
                false,
                &AgentIdentifierSettings::default(),
            );
            assert_eq!(label.spans[0].content.trim_end(), TEXT_EDITOR_ICON);
            assert_eq!(
                UnicodeWidthStr::width(label.spans[0].content.as_ref()),
                NODE_ICON_COLUMN_WIDTH
            );
        }
    }

    #[test]
    fn tree_labels_use_action_columns_until_that_specific_row_is_hovered() {
        let area = Rect::new(0, 0, 40, 8);
        let list = list_area(area);
        let action_strip = row_action_strip_area(area);
        assert_eq!(action_strip.right(), list.right());
        assert_eq!(action_strip.width, ROW_ACTION_TOTAL_WIDTH);

        let mut tree = Tree::new();
        let first_group = tree.add_group(ROOT_ID, "x".repeat(100)).unwrap();
        tree.add_group(ROOT_ID, "y".repeat(100)).unwrap();
        let mut state = TreeState::default();
        let titles_loading = HashSet::new();
        let recently_created = HashMap::new();
        let agent_identifiers = AgentIdentifierSettings::default();
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();

        terminal
            .draw(|frame| {
                render(
                    frame,
                    area,
                    &tree,
                    &mut state,
                    TreeRenderOptions {
                        focused: false,
                        elapsed_ms: 0,
                        project_name: None,
                        is_project_name_loading: false,
                        titles_loading: &titles_loading,
                        recently_created: &recently_created,
                        agent_identifiers: &agent_identifiers,
                        hover: TreeHoverState::default(),
                    },
                );
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        for x in action_strip.x..action_strip.right() {
            assert_eq!(
                buffer[(x, list.y)].symbol(),
                "x",
                "unhovered title did not use available action-strip cell {x}"
            );
        }

        terminal
            .draw(|frame| {
                render(
                    frame,
                    area,
                    &tree,
                    &mut state,
                    TreeRenderOptions {
                        focused: false,
                        elapsed_ms: 0,
                        project_name: None,
                        is_project_name_loading: false,
                        titles_loading: &titles_loading,
                        recently_created: &recently_created,
                        agent_identifiers: &agent_identifiers,
                        hover: TreeHoverState {
                            node: Some(TreeNodeHit {
                                id: first_group,
                                row: list.y,
                            }),
                            ..TreeHoverState::default()
                        },
                    },
                );
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(
            buffer[(action_strip.x, list.y)].symbol(),
            TreeRowAction::Rename.glyph()
        );
        for x in action_strip.x..action_strip.right() {
            assert_eq!(
                buffer[(x, list.y + 1)].symbol(),
                "y",
                "hovering the first row hid the second row's title at cell {x}"
            );
        }
    }

    #[test]
    fn row_action_strip_erases_underlying_text_and_leaves_real_blank_gaps() {
        let area = Rect::new(0, 0, 33, 6);
        let row = 2;
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();

        terminal
            .draw(|frame| {
                frame.render_widget(
                    Paragraph::new("x".repeat(usize::from(area.width))),
                    Rect::new(area.x, row, area.width, 1),
                );
                draw_row_actions(frame, area, row, &TreeRowAction::ALL);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let controls_start = list_area(area).right() - ROW_ACTION_WIDTH * ROW_ACTION_COUNT;
        assert_eq!(buffer[(controls_start - 1, row)].symbol(), "x");

        for (action_index, action) in TreeRowAction::ALL.into_iter().enumerate() {
            let action_x = controls_start + action_index as u16 * ROW_ACTION_WIDTH;
            assert_eq!(
                buffer[(action_x, row)].symbol(),
                action.glyph(),
                "slot {action_index} did not render its action glyph"
            );
            let gap_x = controls_start
                + action_index as u16 * ROW_ACTION_WIDTH
                + ROW_ACTION_WIDTH.saturating_sub(1);
            assert_eq!(
                buffer[(gap_x, row)].symbol(),
                ROW_ACTION_BLANK,
                "slot {action_index} did not erase its inter-icon gap"
            );
        }
    }

    #[test]
    fn folder_virtual_rows_resolve_to_the_live_file_path() {
        let root_path = std::env::temp_dir().join(format!(
            "ilium-folder-tree-ui-{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root_path);
        std::fs::create_dir_all(root_path.join("nested")).expect("create folder fixture");
        let file_path = root_path.join("nested").join("note.rs");
        std::fs::write(&file_path, "fn main() {}\n").expect("write fixture file");

        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "workspace").unwrap();
        let folder = tree.add_folder(group, root_path.clone()).unwrap();
        let virtual_id = virtual_folder_node_id(folder, &file_path);

        assert_eq!(
            folder_entry_path(&tree, virtual_id),
            Some((folder, file_path, false))
        );
        let _ = std::fs::remove_dir_all(root_path);
    }

    #[test]
    fn display_title_picks_short_name_only_below_the_wide_threshold_when_one_exists() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane = tree
            .add_pane(group, "shell", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        tree.set_automatic_pane_title(
            pane,
            "Fix Auth Bug In Login Flow",
            Some("Auth Bug".to_string()),
        )
        .unwrap();
        let node = tree.get(pane).unwrap();

        assert_eq!(display_title(node, WIDE_TITLE_MIN_COLUMNS - 1), "Auth Bug");
        assert_eq!(
            display_title(node, WIDE_TITLE_MIN_COLUMNS),
            "Fix Auth Bug In Login Flow"
        );
    }

    #[test]
    fn display_title_falls_back_to_name_when_there_is_no_short_form() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane = tree
            .add_pane(group, "shell", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        tree.rename_node(pane, "manual name", None).unwrap();
        let node = tree.get(pane).unwrap();

        assert_eq!(display_title(node, 0), "manual name");
        assert_eq!(display_title(node, WIDE_TITLE_MIN_COLUMNS), "manual name");
    }

    #[test]
    fn pane_label_shows_the_name_normally_when_no_title_inference_is_in_flight() {
        let line = pane_label(
            &PaneStatus::Agent(AgentClass::Claude, AgentActivity::Idle),
            "claude",
            0,
            false,
            &AgentIdentifierSettings::default(),
        );
        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(text.ends_with("claude"));
    }

    #[test]
    fn pane_label_cycles_through_every_half_hour_clock_for_waiting_background() {
        assert_eq!(BACKGROUND_CLOCK_FRAMES.len(), 24);

        for (frame_index, expected_clock) in BACKGROUND_CLOCK_FRAMES.iter().enumerate() {
            let line = pane_label(
                &PaneStatus::Agent(AgentClass::Claude, AgentActivity::WaitingBackground),
                "claude",
                frame_index as u128 * BACKGROUND_FRAME_MS,
                false,
                &AgentIdentifierSettings::default(),
            );
            assert_eq!(line.spans[1].content.trim_end(), expected_clock.to_string());
            assert_eq!(
                UnicodeWidthStr::width(line.spans[1].content.as_ref()),
                ACTIVITY_ICON_COLUMN_WIDTH
            );
        }

        let wrapped = pane_label(
            &PaneStatus::Agent(AgentClass::Claude, AgentActivity::WaitingBackground),
            "claude",
            BACKGROUND_CLOCK_FRAMES.len() as u128 * BACKGROUND_FRAME_MS,
            false,
            &AgentIdentifierSettings::default(),
        );
        assert_eq!(
            wrapped.spans[1].content.trim_end(),
            BACKGROUND_CLOCK_FRAMES[0].to_string()
        );
    }

    #[test]
    fn pane_label_shows_the_braille_spinner_instead_of_the_name_while_title_inference_is_in_flight()
    {
        let line = pane_label(
            &PaneStatus::Agent(AgentClass::Claude, AgentActivity::Idle),
            "claude",
            0,
            true,
            &AgentIdentifierSettings::default(),
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
    fn recently_created_flash_starts_on_and_alternates_until_the_window_elapses() {
        assert!(is_recently_created_flash_on(0));
        assert!(is_recently_created_flash_on(
            RECENTLY_CREATED_PULSE_PHASE_MS - 1
        ));
        assert!(!is_recently_created_flash_on(
            RECENTLY_CREATED_PULSE_PHASE_MS
        ));
        assert!(!is_recently_created_flash_on(
            RECENTLY_CREATED_PULSE_PHASE_MS * 2 - 1
        ));
        assert!(is_recently_created_flash_on(
            RECENTLY_CREATED_PULSE_PHASE_MS * 2
        ));
        // Once the whole window has elapsed, the flash stops entirely --
        // never re-lights, even on what would otherwise be an "on" phase.
        assert!(!is_recently_created_flash_on(RECENTLY_CREATED_PULSE_MS));
        assert!(!is_recently_created_flash_on(
            RECENTLY_CREATED_PULSE_MS * 10
        ));
    }

    #[test]
    fn apply_recent_pulse_reverses_every_span_only_while_flashing() {
        let line = Line::from(vec![
            Span::styled("> ", Style::new().fg(Color::Gray)),
            Span::raw("shell"),
        ]);

        let unflashed = apply_recent_pulse(line.clone(), false);
        assert!(!unflashed.spans[0]
            .style
            .add_modifier
            .contains(Modifier::REVERSED));

        let flashed = apply_recent_pulse(line, true);
        for span in &flashed.spans {
            assert!(span.style.add_modifier.contains(Modifier::REVERSED));
        }
        // The original fg color survives the flash -- it reverses whatever
        // was already there rather than overwriting it.
        assert_eq!(flashed.spans[0].style.fg, Some(Color::Gray));
    }

    #[test]
    fn build_item_flashes_a_freshly_created_pane_and_leaves_others_alone() {
        // `tui_tree_widget::TreeItem` exposes neither its text nor a style
        // getter, so this asserts through `should_flash` (the same pure
        // decision `build_item` calls) rather than trying to inspect a
        // constructed item's opaque internal style -- `build_tree_items` is
        // still exercised here to prove it doesn't panic on a tree with a
        // `recently_created` entry, and `apply_recent_pulse_reverses_every_span_only_while_flashing`
        // above covers the actual styling once `flash_on` is known.
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let fresh_pane = tree
            .add_pane(group, "shell", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        let old_pane = tree
            .add_pane(group, "other", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        let mut recently_created = HashMap::new();
        recently_created.insert(fresh_pane, 0u128);

        let items = build_tree_items(
            &tree,
            0,
            &HashSet::new(),
            &recently_created,
            &AgentIdentifierSettings::default(),
            0,
        );
        assert_eq!(items[0].children().len(), 2);

        assert!(should_flash(fresh_pane, &recently_created, 0));
        assert!(!should_flash(old_pane, &recently_created, 0));
    }

    #[test]
    fn toolbar_hit_testing_maps_each_compact_icon() {
        let area = Rect::new(0, 0, 32, 12);
        for (action, button_area) in toolbar_button_rects(area) {
            assert_eq!(
                toolbar_action_at(area, Position::new(button_area.x, button_area.y)),
                Some(action)
            );
        }
    }

    #[test]
    fn toolbar_keeps_settings_right_aligned_without_overlapping_left_actions() {
        // `layout::MIN_TREE_WIDTH` (16) with a 1-cell border each side
        // leaves a 14-column-wide toolbar -- room for three left actions and
        // the right-anchored settings button.
        let area = Rect::new(0, 0, 16, 12);
        let toolbar = toolbar_area(area);
        assert_eq!(toolbar.width, 14);

        assert_eq!(
            toolbar_action_at(area, Position::new(toolbar.x + 8, toolbar.y)),
            Some(TreeToolbarAction::Codex)
        );
        assert_eq!(
            toolbar_action_at(area, Position::new(toolbar.x + 12, toolbar.y)),
            Some(TreeToolbarAction::Settings)
        );
        assert_eq!(
            toolbar_action_at(area, Position::new(toolbar.x + 13, toolbar.y)),
            Some(TreeToolbarAction::Settings)
        );

        let buttons = toolbar_button_rects(area);
        let settings_area = buttons
            .iter()
            .find(|(action, _)| *action == TreeToolbarAction::Settings)
            .map(|(_, area)| *area)
            .unwrap();
        assert_eq!(settings_area.right(), toolbar.right());
        for (_, left_area) in buttons
            .iter()
            .filter(|(action, _)| *action != TreeToolbarAction::Settings)
        {
            assert!(left_area.right() <= settings_area.x);
        }
    }

    #[test]
    fn toolbar_is_visible_for_tree_focus_or_footer_hover() {
        assert!(is_toolbar_visible(true, false));
        assert!(is_toolbar_visible(false, true));
        assert!(is_toolbar_visible(true, true));
        assert!(!is_toolbar_visible(false, false));
    }

    #[test]
    fn row_action_hit_testing_uses_right_edge_of_hovered_row() {
        let area = Rect::new(0, 0, 33, 20);
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "group").unwrap();
        let shell = tree
            .add_pane(group, "shell", ilium_core::PaneContentKind::Terminal)
            .unwrap();

        let controls_start = list_area(area).right() - ROW_ACTION_WIDTH * ROW_ACTION_COUNT;
        for (index, expected_action) in TreeRowAction::ALL.into_iter().enumerate() {
            assert_eq!(
                row_action_at(
                    &tree,
                    shell,
                    area,
                    5,
                    Position::new(controls_start + index as u16 * ROW_ACTION_WIDTH, 5)
                ),
                Some(expected_action)
            );
        }
        assert_eq!(
            row_action_at(&tree, shell, area, 4, Position::new(controls_start, 5)),
            None
        );
    }

    #[test]
    fn row_action_hit_testing_excludes_retitle_for_a_group_or_editor_row() {
        let area = Rect::new(0, 0, 33, 20);
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "group").unwrap();
        let editor = tree
            .add_pane(group, "notes.md", ilium_core::PaneContentKind::Editor)
            .unwrap();

        // The trailing cell -- where `Retitle` lands on an eligible row --
        // must be a no-op on a group row, not `Rename` shifted into it.
        let controls_start = list_area(area).right() - ROW_ACTION_WIDTH * ROW_ACTION_COUNT;
        let retitle_x = controls_start + 4 * ROW_ACTION_WIDTH;
        assert_eq!(
            row_action_at(&tree, group, area, 5, Position::new(retitle_x, 5)),
            None
        );
        assert_eq!(
            row_action_at(&tree, editor, area, 5, Position::new(retitle_x, 5)),
            None
        );
        // The other four actions still apply to both.
        let close_x = controls_start + 3 * ROW_ACTION_WIDTH;
        assert_eq!(
            row_action_at(&tree, group, area, 5, Position::new(close_x, 5)),
            Some(TreeRowAction::Close)
        );
        assert_eq!(
            row_action_at(&tree, editor, area, 5, Position::new(close_x, 5)),
            Some(TreeRowAction::Close)
        );
    }

    #[test]
    fn node_hit_testing_excludes_blank_rows_below_the_tree() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "group").unwrap();
        let pane = tree
            .add_pane(group, "pane", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        let mut state = TreeState::default();
        state.open(vec![group]);
        let area = Rect::new(0, 0, 32, 12);
        let list = list_area(area);
        let items = build_tree_items(
            &tree,
            0,
            &HashSet::new(),
            &HashMap::new(),
            &AgentIdentifierSettings::default(),
            0,
        );

        assert_eq!(
            node_at_position(&items, &state, area, Position::new(list.x, list.y)),
            Some(TreeNodeHit {
                id: group,
                row: list.y
            })
        );
        assert_eq!(
            node_at_position(&items, &state, area, Position::new(list.x, list.y + 1)),
            Some(TreeNodeHit {
                id: pane,
                row: list.y + 1
            })
        );
        assert_eq!(
            node_at_position(&items, &state, area, Position::new(list.x, list.y + 2)),
            None
        );
    }

    #[test]
    fn tree_item_cache_only_rebuilds_when_the_version_changes() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "group").unwrap();
        tree.add_pane(group, "pane", ilium_core::PaneContentKind::Terminal)
            .unwrap();

        let mut cache = TreeItemCache::default();
        let first = cache.get_or_build(&tree, 1).len();
        assert_eq!(first, 1);

        // A second call at the same version returns the same cached
        // shape without needing the tree to still be reachable/unchanged
        // in any way this test can observe from the outside -- the real
        // guarantee (no rebuild happened) is covered by
        // `App::tree_node_at` being safe to call every mouse-move; this
        // just pins the observable contract (stable length per version).
        let second = cache.get_or_build(&tree, 1).len();
        assert_eq!(second, 1);

        // A second top-level group (not a second pane nested under the same
        // one) so the rebuilt top-level item count actually differs from
        // `first`/`second` -- if `get_or_build` never rebuilt on the new
        // version, this would still (wrongly) report 1.
        let mut new_tree = Tree::new();
        new_tree.add_group(ROOT_ID, "group").unwrap();
        new_tree.add_group(ROOT_ID, "group-2").unwrap();
        let third = cache.get_or_build(&new_tree, 2).len();
        assert_eq!(third, 2);
    }

    #[test]
    fn any_recently_created_within_window_reflects_the_flash_window() {
        let mut recently_created = HashMap::new();
        assert!(!any_recently_created_within_window(&recently_created, 0));

        recently_created.insert(NodeId(0), 0u128);
        assert!(any_recently_created_within_window(&recently_created, 0));
        assert!(!any_recently_created_within_window(
            &recently_created,
            RECENTLY_CREATED_PULSE_MS
        ));
    }
}
