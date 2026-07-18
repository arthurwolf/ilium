//! Renders `ilium_core::Tree` as a `tui_tree_widget::Tree<NodeId>`.
//!
//! Expand/collapse state belongs to `tui_tree_widget::TreeState`, because
//! that widget also owns its visible-row and scrolling bookkeeping. Mouse
//! and keyboard handlers mutate the same state, so a collapsed group stays
//! collapsed until the user opens it again.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use ilium_core::{
    AgentActivity, AgentClass, AgentProvider, BuiltinAgentProvider, ContainerKind, Node, NodeId,
    NodeKind, PaneStatus, ScheduledPaneInput, Tree, ROOT_ID,
};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::Frame;
use tui_tree_widget::{Tree as TreeWidget, TreeItem, TreeState};
use unicode_width::UnicodeWidthStr;

use crate::config::{AgentIdentifierMode, AgentIdentifierSettings, SidebarDensity, TreeOrder};
use crate::icon_settings::{IconSettings, IconTarget};
use crate::theme;
use crate::tree_ordering;
use crate::tree_transitions::{TreeRowMotion, TreeTransitions};

/// Braille spinner frames for an actively-`Working` agent pane, cycled by
/// elapsed wall-clock time so it animates smoothly across redraws
/// regardless of the (much slower) detection poll interval.
pub(crate) const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
pub(crate) const SPINNER_FRAME_MS: u128 = 90;

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
#[cfg(test)]
const AGENT_ICON: &str = "\u{1F916}";
/// Plain terminal panes and the matching creation action share this glyph.
const TERMINAL_ICON: &str = "📟";
/// Text editor panes and their creation action share this glyph; rename keeps
/// the pen icon because it represents an action rather than a pane kind.
const TEXT_EDITOR_ICON: &str = "🗒️";
/// Settings stays anchored at the bottom-right of the left panel.
const SETTINGS_ICON: &str = "🎚️";

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

/// Goal state is independent of an agent's current activity. Its fixed
/// column places a double-width checkered flag immediately after the status
/// indicator whenever a goal is present.
const GOAL_ICON_COLUMN_WIDTH: usize = 3;

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
/// Two display cells hold an action glyph and the third is its spacer.
/// Every slot is explicitly cleared before its glyph is painted, so hovered
/// controls cannot reveal the title that previously occupied the same cells.
pub(crate) const ROW_ACTION_WIDTH: u16 = 3;
const ROW_ACTION_TOTAL_WIDTH: u16 = ROW_ACTION_WIDTH * ROW_ACTION_COUNT;
/// Edit, up, down, close, then retitle -- reserved as the trailing cells of
/// a hovered row (see `row_action_at`/`draw_row_actions`).
pub(crate) const ROW_ACTION_COUNT: u16 = 6;

/// Actions available from the tree toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeToolbarAction {
    Search,
    Shell,
    /// One visible launcher for each built-in provider. Carrying the provider
    /// keeps this surface synchronized with the core agent registry.
    Agent(BuiltinAgentProvider),
    Editor,
    Board,
    Group,
    Project,
    Split,
    Folder,
    /// Triggers `App::action_request_restructure` -- one independent LLM
    /// regroup-and-retitle call per non-empty project (see
    /// `crate::restructure`'s module docs). Anchored next to `Settings`
    /// rather than joining `LEFT_ALIGNED`: it acts on the whole session, not
    /// on one new pane, the same reason `Settings` is anchored separately.
    Restructure,
    Settings,
}

impl TreeToolbarAction {
    /// Ordered left-aligned creation actions. Settings is intentionally not
    /// part of this list because its geometry is anchored independently.
    const LEFT_ALIGNED: [Self; 11] = [
        Self::Search,
        // Projects are a first-class top-level concept, so keep their
        // creation control within the narrow-panel visible prefix.
        Self::Project,
        Self::Shell,
        Self::Agent(BuiltinAgentProvider::Claude),
        Self::Agent(BuiltinAgentProvider::Codex),
        Self::Agent(BuiltinAgentProvider::Antigravity),
        Self::Editor,
        Self::Board,
        Self::Group,
        Self::Split,
        Self::Folder,
    ];

    /// Single glyph shown on the button, reusing the same symbol the tree
    /// already uses for that node kind elsewhere (group/editor) so the
    /// toolbar and the tree read as one visual language rather than two.
    fn glyph(self) -> &'static str {
        match self {
            Self::Search => "⌕",
            Self::Shell => TERMINAL_ICON,
            Self::Agent(BuiltinAgentProvider::Claude) => "Ⓒ",
            Self::Agent(BuiltinAgentProvider::Codex) => "Ⓧ",
            Self::Agent(BuiltinAgentProvider::Antigravity) => "Ⓐ",
            Self::Editor => TEXT_EDITOR_ICON,
            Self::Board => "▦",
            Self::Group => "\u{1F4C1}",
            Self::Project => "🗂️",
            Self::Split => "▥",
            Self::Folder => "\u{1F5C0}",
            // Reuses the same recycle glyph as the per-row `TreeRowAction::Retitle`
            // -- one visual language for "ask the LLM to redo a title" whether
            // it's scoped to one row or the whole tree.
            Self::Restructure => TreeRowAction::Retitle.normal_glyph(),
            Self::Settings => SETTINGS_ICON,
        }
    }

    /// Accent color identifying this action, distinct enough from the
    /// others (and from the status colors already used for pane state --
    /// yellow/working, cyan/waiting, green/idle) that the five buttons read
    /// as five different things at a glance instead of one repeated shape.
    fn accent(self) -> Color {
        match self {
            Self::Search => Color::Cyan,
            Self::Shell => Color::Gray,
            Self::Agent(BuiltinAgentProvider::Claude) => Color::Rgb(0xd9, 0x77, 0x57),
            Self::Agent(BuiltinAgentProvider::Codex) => Color::Rgb(0x2c, 0xb6, 0x7d),
            Self::Agent(BuiltinAgentProvider::Antigravity) => Color::Rgb(0x9b, 0x7b, 0xff),
            Self::Editor => Color::Magenta,
            Self::Board => Color::Cyan,
            Self::Group => Color::Rgb(0x7a, 0xa2, 0xf7),
            Self::Project => Color::Cyan,
            Self::Split => Color::Rgb(0xbb, 0x9a, 0xf7),
            Self::Folder => Color::Cyan,
            Self::Restructure => Color::Yellow,
            Self::Settings => Color::Gray,
        }
    }

    /// Human-readable text shown live in the toolbar's caption row while
    /// hovered, and reused in the status bar if the action later fails.
    pub fn description(self) -> &'static str {
        match self {
            Self::Search => "search workspace",
            Self::Shell => "new shell",
            Self::Agent(provider) => provider.label(),
            Self::Editor => "new editor",
            Self::Board => "new board",
            Self::Group => "new group",
            Self::Project => "new project",
            Self::Split => "new split view",
            Self::Folder => "open folder",
            Self::Restructure => "restructure with AI",
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
    /// Restructures exactly one project with a separate AI call.
    ProjectRestructure,
}

impl TreeRowAction {
    /// Ordered set used for both rendering and hit testing -- left to right.
    pub(crate) const ALL: [Self; 6] = [
        Self::Rename,
        Self::MoveUp,
        Self::MoveDown,
        Self::Close,
        Self::Retitle,
        Self::ProjectRestructure,
    ];

    /// The default, expressive UTF-8 icon for this action. These remain the
    /// standard sidebar controls; `stable_glyph` is only an explicit user
    /// preference for terminals that cannot render the emoji correctly.
    const fn normal_glyph(self) -> &'static str {
        match self {
            Self::Rename => theme::PEN_ICON,
            Self::MoveUp => "🔼",
            Self::MoveDown => "🔽",
            Self::Close => "🚫",
            Self::Retitle | Self::ProjectRestructure => "♻️",
        }
    }

    /// A deliberately plain, one-cell alternative for terminals where a
    /// user has opted out of rich emoji. Never use this as a rendering
    /// fallback: normal icons must be fixed at the rendering layer instead.
    const fn stable_glyph(self) -> &'static str {
        match self {
            Self::Rename => "✎",
            Self::MoveUp => "↑",
            Self::MoveDown => "↓",
            Self::Close => "×",
            Self::Retitle => "↻",
            Self::ProjectRestructure => "⟳",
        }
    }

    const fn glyph(self, use_stable_glyphs: bool) -> &'static str {
        if use_stable_glyphs {
            self.stable_glyph()
        } else {
            self.normal_glyph()
        }
    }
}

/// Geometry and ordering for the trailing action cells on one tree row.
///
/// Rendering and mouse input both go through this type, so adding, removing,
/// or resizing an action cannot silently leave its click target somewhere
/// else. The strip owns complete fixed-width slots from the list's right edge;
/// actions that do not apply keep their slot blank rather than shifting later
/// actions into a different position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TreeRowActionStrip {
    area: Rect,
}

impl TreeRowActionStrip {
    /// Resolves a complete action strip, or none when the tree is too narrow
    /// to show every action and its click target without overlap.
    fn from_tree_area(area: Rect) -> Option<Self> {
        let list = list_area(area);
        (list.width >= ROW_ACTION_TOTAL_WIDTH).then_some(Self {
            area: Rect::new(
                list.right().saturating_sub(ROW_ACTION_TOTAL_WIDTH),
                list.y,
                ROW_ACTION_TOTAL_WIDTH,
                list.height,
            ),
        })
    }

    /// Returns the fixed slot for an action's stable index in `ALL`.
    fn slot(self, index: u16, row: u16) -> Rect {
        Rect::new(
            self.area
                .x
                .saturating_add(index.saturating_mul(ROW_ACTION_WIDTH)),
            row,
            ROW_ACTION_WIDTH,
            1,
        )
    }

    /// Maps a coordinate back through the exact slot geometry used to draw.
    fn action_at(self, position: Position) -> Option<TreeRowAction> {
        if !self.area.contains(position) {
            return None;
        }
        let index = usize::from((position.x - self.area.x) / ROW_ACTION_WIDTH);
        TreeRowAction::ALL.get(index).copied()
    }

    /// Paints each action and its cleanup spaces as one natural-width run.
    /// A wide emoji that replaces title text must carry the trailing blanks
    /// in the same terminal print operation: Ratatui otherwise treats its
    /// continuation cell as covered and can skip its separate cleanup diff.
    fn draw(
        self,
        buffer: &mut Buffer,
        row: u16,
        actions: &[TreeRowAction],
        style: Style,
        use_stable_glyphs: bool,
    ) {
        for (index, action) in TreeRowAction::ALL.iter().enumerate() {
            let Ok(index) = u16::try_from(index) else {
                continue;
            };
            let slot = self.slot(index, row);

            // Clear every physical cell first. The title normally occupies
            // this region, so merely writing an emoji at the slot origin
            // would leave its old second/third characters visible.
            for column in slot.x..slot.right() {
                buffer[(column, row)].reset();
                buffer[(column, row)].set_symbol(" ").set_style(style);
            }

            if actions.contains(action) {
                let glyph = action.glyph(use_stable_glyphs);
                let padding =
                    usize::from(ROW_ACTION_WIDTH).saturating_sub(UnicodeWidthStr::width(glyph));
                // This is intentionally one `Cell` holding one full terminal
                // run. Its natural measured width is exactly the slot width,
                // so the emitted emoji and blanks replace every old title
                // character without synthetic forced-width behavior.
                let terminal_run = format!("{glyph}{}", " ".repeat(padding));
                buffer[(slot.x, row)]
                    .set_symbol(&terminal_run)
                    .set_style(style);
            }
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
    /// Current wall-clock time used only for durable absolute countdowns.
    pub current_unix_millis: u64,
    pub project_name: Option<&'a str>,
    pub is_project_name_loading: bool,
    /// Terminal panes currently awaiting `session_naming::infer_pane_title`
    /// (see `App::titles_loading`) -- their name renders as the shared
    /// braille spinner instead, same visual language as `is_project_name_loading`.
    pub titles_loading: &'a HashSet<NodeId>,
    /// Node id -> `elapsed_ms`-relative offset (ms) at which this client
    /// finished sliding into place -- see `App::recently_created`.
    pub recently_created: &'a HashMap<NodeId, u128>,
    /// Client-only structural transition state. It may temporarily supply the
    /// previous snapshot for exit rendering while `tree` remains authoritative.
    pub transitions: &'a TreeTransitions,
    /// Resolved user-global presentation settings for detected agent types.
    pub agent_identifiers: &'a AgentIdentifierSettings,
    pub icons: &'a IconSettings,
    pub tree_order: TreeOrder,
    pub sidebar_density: SidebarDensity,
    /// An explicit opt-in for text-only row-action symbols. The default is
    /// false so normal UTF-8 icons are always the primary experience.
    pub use_stable_glyphs: bool,
    pub hover: TreeHoverState,
    /// Live pane runtimes (see `App::panes`), keyed by pane id. Used only to
    /// look up each open editor pane's backing file path on demand while
    /// building tree rows -- borrowed directly rather than pre-copied into a
    /// `NodeId -> PathBuf` map, since that map would need rebuilding from
    /// scratch every single redraw for no reason (the pane-id -> path
    /// mapping only actually changes on editor open/close/Save-As). A
    /// restructure's title is now a genuinely descriptive title distinct
    /// from an editor's filename (unlike a plain rename/automatic titler,
    /// which historically left an editor's `Node::name` equal to its
    /// filename) -- see `pane_label`'s Editor arms for how the two are
    /// composed.
    pub panes: &'a HashMap<NodeId, crate::app::PaneRuntime>,
}

/// Shared immutable inputs for one recursive item-tree build. Keeping these
/// together prevents each new presentation concern from widening every
/// recursive helper's signature independently.
struct TreeItemBuildContext<'a> {
    elapsed_ms: u128,
    current_unix_millis: u64,
    titles_loading: &'a HashSet<NodeId>,
    recently_created: &'a HashMap<NodeId, u128>,
    agent_identifiers: &'a AgentIdentifierSettings,
    icons: &'a IconSettings,
    tree_order: TreeOrder,
    sidebar_density: SidebarDensity,
    panel_width: u16,
    /// Full widget identifier paths that users have expanded. Folder nodes
    /// are materialized only along these paths, so a large unopened subtree
    /// never becomes render work merely because another row animates.
    opened_paths: &'a HashSet<Vec<NodeId>>,
    panes: &'a HashMap<NodeId, crate::app::PaneRuntime>,
}

/// Builds the full recursive `TreeItem` tree from the root group's
/// children (the root group itself is never shown as a node -- its
/// children are ilium's top-level groups/panes). `elapsed_ms` drives the
/// Working spinner, WaitingBackground clock, and Done pulse animations.
/// `panel_width` is the tree panel's current outer `Rect` width, used to
/// pick each pane's short- vs long-form title -- see `display_title`.
fn build_tree_items(
    tree: &Tree,
    context: TreeItemBuildContext<'_>,
) -> Vec<TreeItem<'static, NodeId>> {
    build_children(tree, ROOT_ID, &context, &[])
}

/// Returns the authoritative tree-node ids in the exact order currently
/// visible in the sidebar. Virtual filesystem descendants are deliberately
/// omitted: they cannot be closed through IPC and never survive the removal
/// of their owning [`NodeKind::Folder`] as independent tree nodes.
pub(crate) fn visible_tree_node_ids(
    tree: &Tree,
    state: &TreeState<NodeId>,
    tree_order: TreeOrder,
) -> Vec<NodeId> {
    let opened_paths = HashSet::new();
    let items = build_tree_items(
        tree,
        TreeItemBuildContext {
            elapsed_ms: 0,
            current_unix_millis: 0,
            titles_loading: &HashSet::new(),
            recently_created: &HashMap::new(),
            agent_identifiers: &AgentIdentifierSettings::default(),
            icons: &IconSettings::default(),
            tree_order,
            sidebar_density: SidebarDensity::default(),
            panel_width: 0,
            opened_paths: &opened_paths,
            panes: &HashMap::new(),
        },
    );

    state
        .flatten(&items)
        .into_iter()
        .filter_map(|flattened| flattened.identifier.last().copied())
        .filter(|node_id| tree.get(*node_id).is_some())
        .collect()
}

/// Recursively builds `TreeItem`s for every child of `parent`.
fn build_children(
    tree: &Tree,
    parent: NodeId,
    context: &TreeItemBuildContext<'_>,
    ancestor_path: &[NodeId],
) -> Vec<TreeItem<'static, NodeId>> {
    let children = tree_ordering::ordered_children(tree, parent, context.tree_order);
    children
        .into_iter()
        .filter_map(|child_id| {
            let mut identifier_path = ancestor_path.to_vec();
            identifier_path.push(child_id);
            tree.get(child_id)
                .map(|node| build_item(tree, node, context, &identifier_path))
        })
        .collect()
}

/// Builds one `TreeItem` (recursing into children for a Group).
fn build_item(
    tree: &Tree,
    node: &Node,
    context: &TreeItemBuildContext<'_>,
    identifier_path: &[NodeId],
) -> TreeItem<'static, NodeId> {
    let flash_on = should_flash(node.id, context.recently_created, context.elapsed_ms);
    match &node.kind {
        NodeKind::Container(container) => {
            let children = build_children(tree, node.id, context, identifier_path);
            let icon = match &container.kind {
                ContainerKind::Project { .. } => "🗂️",
                ContainerKind::Group => context.icons.glyph(IconTarget::Group),
                ContainerKind::SplitView {
                    orientation: ilium_core::SplitOrientation::Vertical,
                } => context.icons.glyph(IconTarget::SplitVertical),
                ContainerKind::SplitView {
                    orientation: ilium_core::SplitOrientation::Horizontal,
                } => context.icons.glyph(IconTarget::SplitHorizontal),
            };
            let label = node_label(
                Span::raw(icon.to_string()),
                None,
                Span::raw(node.name.clone()),
            );
            let label = apply_recent_pulse(label, flash_on);
            // `NodeId`s are unique across the whole `Tree` (its own
            // invariant), so they can't collide among siblings here --
            // the `Result` this returns is unreachable in practice.
            TreeItem::new(
                node.id,
                apply_sidebar_density(label, context.sidebar_density),
                children,
            )
            .expect("sibling NodeIds are always unique")
        }
        NodeKind::Pane {
            status,
            scheduled_input,
            ..
        } => {
            let editor_filename = context
                .panes
                .get(&node.id)
                .and_then(|runtime| match runtime {
                    crate::app::PaneRuntime::Editor(editor) => editor.path.as_deref(),
                    _ => None,
                })
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().into_owned());
            let label = scheduled_input.as_ref().map_or_else(
                || {
                    pane_label_with_icons(
                        status,
                        display_title(node, context.panel_width),
                        context.elapsed_ms,
                        context.titles_loading.contains(&node.id),
                        context.agent_identifiers,
                        context.icons,
                        editor_filename.as_deref(),
                    )
                },
                |scheduled_input| {
                    scheduled_pane_label(
                        status,
                        display_title(node, context.panel_width),
                        context.elapsed_ms,
                        context.current_unix_millis,
                        scheduled_input,
                        ScheduledPaneLabelContext {
                            is_title_loading: context.titles_loading.contains(&node.id),
                            agent_identifiers: context.agent_identifiers,
                            editor_filename: editor_filename.as_deref(),
                        },
                    )
                },
            );
            TreeItem::new_leaf(
                node.id,
                apply_sidebar_density(apply_recent_pulse(label, flash_on), context.sidebar_density),
            )
        }
        NodeKind::Folder { path } => {
            let label = node_label(
                Span::raw(context.icons.glyph(IconTarget::Folder).to_string()),
                None,
                Span::raw(node.name.clone()),
            );
            // The root itself is a normal tree row. Once opened, its direct
            // children are listed; each child directory recurses only after
            // *that* exact virtual path has been expanded.
            let children = if context.opened_paths.contains(identifier_path) {
                folder_children(node.id, path, context, identifier_path)
            } else {
                Vec::new()
            };
            TreeItem::new(
                node.id,
                apply_sidebar_density(apply_recent_pulse(label, flash_on), context.sidebar_density),
                children,
            )
            .expect("folder node ids are unique")
        }
    }
}

/// Applies the sidebar's information-density preference without changing
/// tree structure or mouse-row geometry. Tree rows remain one terminal cell
/// high; density controls the deliberate horizontal breathing room around
/// each label, preserving accessible hit targets.
fn apply_sidebar_density(mut label: Line<'static>, density: SidebarDensity) -> Line<'static> {
    let padding = match density {
        SidebarDensity::Compact => 0,
        SidebarDensity::Standard => 1,
        SidebarDensity::Comfortable => 2,
    };
    if padding > 0 {
        label.spans.insert(0, Span::raw(" ".repeat(padding)));
    }
    label
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
    context: &TreeItemBuildContext<'_>,
    ancestor_path: &[NodeId],
) -> Vec<TreeItem<'static, NodeId>> {
    let Ok(entries) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    let mut entries: Vec<_> = entries
        .flatten()
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
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
        let path = entry.path();
        let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        let label = node_label(
            Span::raw(if is_dir { "\u{1F4C1}" } else { "\u{1F4C4}" }),
            None,
            Span::raw(entry.file_name().to_string_lossy().into_owned()),
        );
        let id = virtual_folder_node_id(root, &path);
        let mut identifier_path = ancestor_path.to_vec();
        identifier_path.push(id);
        let item = if is_dir {
            let children = if context.opened_paths.contains(&identifier_path) {
                folder_children(root, &path, context, &identifier_path)
            } else {
                Vec::new()
            };
            TreeItem::new(
                id,
                apply_sidebar_density(label, context.sidebar_density),
                children,
            )
            .expect("filesystem ids are unique")
        } else {
            TreeItem::new_leaf(id, apply_sidebar_density(label, context.sidebar_density))
        };
        children.push(item);
    }
    children
}

/// A currently-resolvable virtual filesystem row. `identifier_path` is the
/// exact tree-widget path, not merely the synthetic leaf ID; nested folder
/// expansion and selection must use the complete path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderEntry {
    pub root_id: NodeId,
    pub path: PathBuf,
    pub is_directory: bool,
    pub identifier_path: Vec<NodeId>,
}

/// Resolves a virtual file/folder row to its current path and full widget
/// identifier path. Re-reading makes a click safe when the filesystem
/// changed after the previous render.
pub fn folder_entry(tree: &Tree, id: NodeId) -> Option<FolderEntry> {
    for node in tree.all_ids().filter_map(|node_id| tree.get(node_id)) {
        let NodeKind::Folder { path } = &node.kind else {
            continue;
        };
        if let Some(found) = find_folder_entry(node.id, path, &tree_path(tree, node.id), id) {
            return Some(found);
        }
    }
    None
}

/// Reconstructs one persisted node's widget identifier path without leaking
/// client `App` concerns into the filesystem renderer.
fn tree_path(tree: &Tree, id: NodeId) -> Vec<NodeId> {
    let mut path = vec![id];
    let mut current = id;
    while let Some(parent) = tree.parent_of(current) {
        if parent == ROOT_ID {
            break;
        }
        path.push(parent);
        current = parent;
    }
    path.reverse();
    path
}

fn find_folder_entry(
    root: NodeId,
    path: &Path,
    ancestor_path: &[NodeId],
    target: NodeId,
) -> Option<FolderEntry> {
    for entry in std::fs::read_dir(path).ok()?.flatten() {
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let entry_path = entry.path();
        let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        let id = virtual_folder_node_id(root, &entry_path);
        let mut identifier_path = ancestor_path.to_vec();
        identifier_path.push(id);
        if id == target {
            return Some(FolderEntry {
                root_id: root,
                path: entry_path,
                is_directory: is_dir,
                identifier_path,
            });
        }
        if is_dir {
            if let Some(found) = find_folder_entry(root, &entry_path, &identifier_path, target) {
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
        .filter(|&&created_offset_ms| elapsed_ms >= created_offset_ms)
        .map(|&created_offset_ms| elapsed_ms - created_offset_ms)
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
fn pane_label_with_icons(
    status: &PaneStatus,
    name: &str,
    elapsed_ms: u128,
    is_title_loading: bool,
    agent_identifiers: &AgentIdentifierSettings,
    icons: &IconSettings,
    editor_filename: Option<&str>,
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
    // A restructure's title is a genuinely descriptive title, distinct from
    // an editor's filename (unlike a plain rename or the pre-restructure
    // default, where an editor's `Node::name` just *is* its filename) --
    // compose "filename — title" whenever the two differ, so the filename
    // stays visible even after the pane has been given a real title.
    let editor_text = |current_title: String| -> String {
        match editor_filename {
            Some(filename) if filename != current_title => {
                format!("{filename} \u{2014} {current_title}")
            }
            _ => current_title,
        }
    };
    match status {
        PaneStatus::PlainShell => node_label(
            Span::styled(
                icons.glyph(IconTarget::Terminal).to_string(),
                Style::new().fg(Color::Gray),
            ),
            None,
            Span::raw(title()),
        ),
        PaneStatus::Agent(class, activity) => agent_pane_label(
            class,
            *activity,
            false,
            title(),
            elapsed_ms,
            agent_identifiers,
            icons,
        ),
        PaneStatus::AgentWithGoal(class, activity) => agent_pane_label(
            class,
            *activity,
            true,
            title(),
            elapsed_ms,
            agent_identifiers,
            icons,
        ),
        PaneStatus::Editor { dirty: true } => node_label(
            Span::styled(
                icons.glyph(IconTarget::Editor).to_string(),
                Style::new().fg(Color::Magenta),
            ),
            None,
            Span::styled(
                format!("{}*", editor_text(title())),
                Style::new().fg(Color::Magenta),
            ),
        ),
        PaneStatus::Editor { dirty: false } => node_label(
            Span::raw(icons.glyph(IconTarget::Editor).to_string()),
            None,
            Span::raw(editor_text(title())),
        ),
        PaneStatus::Board => node_label(
            Span::styled(
                icons.glyph(IconTarget::Board).to_string(),
                Style::new().fg(Color::Cyan),
            ),
            None,
            Span::styled(name.to_string(), Style::new().fg(Color::Cyan)),
        ),
    }
}

/// Default-icon wrapper retained for focused unit tests and callers that do
/// not own the global settings object. Production tree rendering always uses
/// `pane_label_with_icons` with the live persisted icon map.
#[cfg(test)]
fn pane_label(
    status: &PaneStatus,
    name: &str,
    elapsed_ms: u128,
    is_title_loading: bool,
    agent_identifiers: &AgentIdentifierSettings,
    editor_filename: Option<&str>,
) -> Line<'static> {
    let mut icons = IconSettings::default();
    icons.set(
        IconTarget::Claude,
        agent_identifiers.claude_icon.glyph().to_string(),
    );
    icons.set(
        IconTarget::Codex,
        agent_identifiers.codex_icon.glyph().to_string(),
    );
    icons.set(
        IconTarget::Antigravity,
        agent_identifiers.antigravity_icon.glyph().to_string(),
    );
    pane_label_with_icons(
        status,
        name,
        elapsed_ms,
        is_title_loading,
        agent_identifiers,
        &icons,
        editor_filename,
    )
}

/// Builds the agent portion of a tree row from independent activity and goal
/// signals. Every activity therefore preserves the same checkered-flag rule.
fn agent_pane_label(
    class: &AgentClass,
    activity: AgentActivity,
    has_goal: bool,
    title: String,
    elapsed_ms: u128,
    agent_identifiers: &AgentIdentifierSettings,
    icons: &IconSettings,
) -> Line<'static> {
    let node_icon = Span::raw(agent_node_icon(class, agent_identifiers, icons).to_string());
    match activity {
        AgentActivity::Working => {
            let frame_index = (elapsed_ms / SPINNER_FRAME_MS) as usize % SPINNER_FRAMES.len();
            agent_node_label(
                node_icon,
                Some(Span::raw(
                    if icons.glyph(IconTarget::Working) == IconTarget::Working.default_glyph() {
                        SPINNER_FRAMES[frame_index].to_string()
                    } else {
                        icons.glyph(IconTarget::Working).to_string()
                    },
                )),
                has_goal,
                Span::raw(agent_title(class, &title, agent_identifiers.mode)),
            )
        }
        AgentActivity::WaitingBackground => {
            let frame_index =
                (elapsed_ms / BACKGROUND_FRAME_MS) as usize % BACKGROUND_CLOCK_FRAMES.len();
            agent_node_label(
                node_icon,
                Some(Span::raw(
                    if icons.glyph(IconTarget::WaitingBackground)
                        == IconTarget::WaitingBackground.default_glyph()
                    {
                        BACKGROUND_CLOCK_FRAMES[frame_index].to_string()
                    } else {
                        icons.glyph(IconTarget::WaitingBackground).to_string()
                    },
                )),
                has_goal,
                Span::raw(agent_title(class, &title, agent_identifiers.mode)),
            )
        }
        AgentActivity::Done => {
            let style = if (elapsed_ms / DONE_PULSE_MS).is_multiple_of(2) {
                Style::new().add_modifier(Modifier::BOLD)
            } else {
                Style::new()
            };
            agent_node_label(
                node_icon,
                Some(Span::styled(
                    icons.glyph(IconTarget::Done).to_string(),
                    style,
                )),
                has_goal,
                Span::styled(
                    format!(
                        "{} — done",
                        agent_title(class, &title, agent_identifiers.mode)
                    ),
                    style,
                ),
            )
        }
        AgentActivity::WaitingApproval => {
            let style = Style::new().add_modifier(Modifier::BOLD);
            agent_node_label(
                node_icon,
                Some(Span::styled(
                    icons.glyph(IconTarget::WaitingApproval).to_string(),
                    style,
                )),
                has_goal,
                Span::styled(agent_title(class, &title, agent_identifiers.mode), style),
            )
        }
        AgentActivity::Idle => agent_node_label(
            node_icon,
            Some(Span::raw(icons.glyph(IconTarget::Idle).to_string())),
            has_goal,
            Span::raw(agent_title(class, &title, agent_identifiers.mode)),
        ),
    }
}

/// Overrides the ordinary activity glyph while a durable input is pending.
/// The clock frame is indexed by remaining time, so as the quotient decreases
/// the familiar half-hour sequence visibly runs backwards.
/// Bundles `scheduled_pane_label`'s trailing presentation-only inputs into
/// one value, purely to stay under clippy's argument-count lint -- the same
/// reason `TreeItemBuildContext` exists for `build_item`'s own growing
/// parameter list.
struct ScheduledPaneLabelContext<'a> {
    is_title_loading: bool,
    agent_identifiers: &'a AgentIdentifierSettings,
    editor_filename: Option<&'a str>,
}

fn scheduled_pane_label(
    status: &PaneStatus,
    name: &str,
    elapsed_ms: u128,
    current_unix_millis: u64,
    scheduled_input: &ScheduledPaneInput,
    label_context: ScheduledPaneLabelContext<'_>,
) -> Line<'static> {
    let ScheduledPaneLabelContext {
        is_title_loading,
        agent_identifiers,
        editor_filename,
    } = label_context;
    let icons = IconSettings::default();
    let remaining_millis = scheduled_input
        .execute_at_unix_millis
        .saturating_sub(current_unix_millis);
    let frame_index = usize::try_from(
        u128::from(remaining_millis) / BACKGROUND_FRAME_MS % BACKGROUND_CLOCK_FRAMES.len() as u128,
    )
    .unwrap_or(0);
    let clock = BACKGROUND_CLOCK_FRAMES[frame_index];
    let title = if is_title_loading {
        let frame_index = (elapsed_ms / SPINNER_FRAME_MS) as usize % SPINNER_FRAMES.len();
        SPINNER_FRAMES[frame_index].to_string()
    } else {
        name.to_string()
    };
    let countdown = human_readable_countdown(remaining_millis);
    match status {
        PaneStatus::PlainShell => node_label(
            Span::styled(
                icons.glyph(IconTarget::Terminal).to_string(),
                Style::new().fg(Color::Gray),
            ),
            Some(Span::raw(clock.to_string())),
            Span::raw(format!("{countdown} {title}")),
        ),
        PaneStatus::Agent(class, _) => agent_node_label(
            Span::raw(agent_node_icon(class, agent_identifiers, &icons).to_string()),
            Some(Span::raw(clock.to_string())),
            false,
            Span::raw(format!(
                "{countdown} {}",
                agent_title(class, &title, agent_identifiers.mode)
            )),
        ),
        PaneStatus::AgentWithGoal(class, _) => agent_node_label(
            Span::raw(agent_node_icon(class, agent_identifiers, &icons).to_string()),
            Some(Span::raw(clock.to_string())),
            true,
            Span::raw(format!(
                "{countdown} {}",
                agent_title(class, &title, agent_identifiers.mode)
            )),
        ),
        // The domain rejects scheduled input for non-terminal panes. Falling
        // back keeps a malformed old snapshot renderable instead of panicking.
        PaneStatus::Editor { .. } | PaneStatus::Board => pane_label_with_icons(
            status,
            name,
            elapsed_ms,
            is_title_loading,
            agent_identifiers,
            &icons,
            editor_filename,
        ),
    }
}

/// Rounds partial seconds up so a newly accepted 30-second timer says `30s`
/// immediately, then chooses the shortest unambiguous clock-style phrase.
fn human_readable_countdown(remaining_millis: u64) -> String {
    let total_seconds = remaining_millis.saturating_add(999) / 1000;
    let hours = total_seconds / 3600;
    let minutes = total_seconds % 3600 / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        return format!("{hours}h {minutes:02}m {seconds:02}s");
    }
    if minutes > 0 {
        return format!("{minutes}m {seconds:02}s");
    }
    if seconds > 0 {
        return format!("{seconds}s");
    }
    "now".to_string()
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

/// Builds an agent row with the persistent-goal badge beside its activity.
fn agent_node_label(
    node_icon: Span<'static>,
    activity_icon: Option<Span<'static>>,
    has_goal: bool,
    text: Span<'static>,
) -> Line<'static> {
    if !has_goal {
        return node_label(node_icon, activity_icon, text);
    }
    Line::from(vec![
        fixed_width_icon_span(node_icon, NODE_ICON_COLUMN_WIDTH),
        fixed_width_icon_span(
            activity_icon.unwrap_or_default(),
            ACTIVITY_ICON_COLUMN_WIDTH,
        ),
        fixed_width_icon_span(Span::raw("🏁"), GOAL_ICON_COLUMN_WIDTH),
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
    format!("{}:", class.label())
}

/// Chooses the first fixed-width column's glyph. Name/letter/hidden modes
/// deliberately leave it blank; their representation lives in the text
/// prefix, while the activity column remains independent and visible.
fn agent_node_icon<'a>(
    class: &AgentClass,
    settings: &AgentIdentifierSettings,
    icons: &'a IconSettings,
) -> &'a str {
    if settings.mode != AgentIdentifierMode::Icon {
        return "";
    }
    match class {
        AgentClass::Claude => icons.glyph(IconTarget::Claude),
        AgentClass::Codex => icons.glyph(IconTarget::Codex),
        AgentClass::Antigravity => icons.glyph(IconTarget::Antigravity),
        AgentClass::Other(_) => icons.glyph(IconTarget::OtherAgent),
    }
}

/// Produces the optional type prefix that precedes a pane's inferred/user
/// title. The provider registry reserves `C:`, `X:`, and `A:` for Claude,
/// Codex, and Antigravity respectively, keeping each built-in distinct at one
/// letter while arbitrary custom signatures retain their initial.
fn agent_text_prefix(class: &AgentClass, mode: AgentIdentifierMode) -> String {
    match mode {
        AgentIdentifierMode::FullName => agent_class_label(class),
        AgentIdentifierMode::Letter => match class {
            AgentClass::Claude => "C:".to_string(),
            AgentClass::Codex => "X:".to_string(),
            AgentClass::Antigravity => "A:".to_string(),
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
/// flow from the left; `Restructure` and `Settings` are anchored at the far
/// right (Restructure immediately before Settings) and never move as the
/// panel expands. On a narrow panel, only left actions that fit before
/// Restructure are included.
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
    let restructure_area = Rect::new(
        settings_area.x.saturating_sub(TOOLBAR_BUTTON_WIDTH),
        toolbar.y,
        BUTTON_VISIBLE_WIDTH,
        toolbar.height,
    );
    let mut buttons = Vec::with_capacity(TreeToolbarAction::LEFT_ALIGNED.len() + 2);
    for (index, action) in TreeToolbarAction::LEFT_ALIGNED.iter().enumerate() {
        let x = toolbar.x + index as u16 * TOOLBAR_BUTTON_WIDTH;
        let button_area = Rect::new(x, toolbar.y, BUTTON_VISIBLE_WIDTH, toolbar.height);
        if button_area.right() > restructure_area.x {
            break;
        }
        buttons.push((*action, button_area));
    }
    buttons.push((TreeToolbarAction::Restructure, restructure_area));
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
            status: PaneStatus::Agent(..) | PaneStatus::AgentWithGoal(..) | PaneStatus::PlainShell,
            ..
        })
    )
}

/// Returns the row actions applicable to `id`'s node kind/status, in the
/// same left-to-right order as `TreeRowAction::ALL` -- see
/// `row_supports_retitle` for why `Retitle` is sometimes excluded.
fn applicable_row_actions(tree: &Tree, id: NodeId) -> &'static [TreeRowAction] {
    const BASIC: [TreeRowAction; 4] = [
        TreeRowAction::Rename,
        TreeRowAction::MoveUp,
        TreeRowAction::MoveDown,
        TreeRowAction::Close,
    ];
    const RETITLE: [TreeRowAction; 5] = [
        TreeRowAction::Rename,
        TreeRowAction::MoveUp,
        TreeRowAction::MoveDown,
        TreeRowAction::Close,
        TreeRowAction::Retitle,
    ];
    const PROJECT: [TreeRowAction; 5] = [
        TreeRowAction::Rename,
        TreeRowAction::MoveUp,
        TreeRowAction::MoveDown,
        TreeRowAction::Close,
        TreeRowAction::ProjectRestructure,
    ];
    if tree.get(id).is_some_and(Node::is_project) {
        &PROJECT
    } else if row_supports_retitle(tree, id) {
        &RETITLE
    } else {
        &BASIC
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
    let action = TreeRowActionStrip::from_tree_area(area)?.action_at(position)?;
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
/// Rebuilding only happens when `version` (the caller's tree-change counter)
/// or `TreeOrder` changes, so a mouse-move flood over an unchanged view costs
/// one `Vec` lookup instead of a full recursive rebuild with fresh labels.
/// See `App::tree_hit_test_cache`.
#[derive(Default)]
pub struct TreeItemCache {
    version: Option<u64>,
    tree_order: Option<TreeOrder>,
    items: Vec<TreeItem<'static, NodeId>>,
}

impl TreeItemCache {
    /// Returns the cached items for `tree`, rebuilding first if `version`
    /// doesn't match the last build this cache served.
    pub fn get_or_build(
        &mut self,
        tree: &Tree,
        version: u64,
        tree_order: TreeOrder,
        opened_paths: &HashSet<Vec<NodeId>>,
    ) -> &[TreeItem<'static, NodeId>] {
        if self.version != Some(version) || self.tree_order != Some(tree_order) {
            // `panel_width` only selects which title text a label carries;
            // hit-testing only needs row structure and node identifiers, so
            // any width is correct here.
            self.items = build_tree_items(
                tree,
                TreeItemBuildContext {
                    elapsed_ms: 0,
                    current_unix_millis: 0,
                    titles_loading: &HashSet::new(),
                    recently_created: &HashMap::new(),
                    agent_identifiers: &AgentIdentifierSettings::default(),
                    icons: &IconSettings::default(),
                    tree_order,
                    sidebar_density: SidebarDensity::default(),
                    panel_width: 0,
                    opened_paths,
                    panes: &HashMap::new(),
                },
            );
            self.version = Some(version);
            self.tree_order = Some(tree_order);
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
        elapsed_ms >= created_offset_ms
            && elapsed_ms - created_offset_ms < RECENTLY_CREATED_PULSE_MS
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
        TreeItemBuildContext {
            elapsed_ms: options.elapsed_ms,
            current_unix_millis: options.current_unix_millis,
            titles_loading: options.titles_loading,
            recently_created: options.recently_created,
            agent_identifiers: options.agent_identifiers,
            icons: options.icons,
            tree_order: options.tree_order,
            sidebar_density: options.sidebar_density,
            panel_width: area.width,
            opened_paths: state.opened(),
            panes: options.panes,
        },
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

    if let Some(presentation_tree) = options.transitions.presentation_tree(options.elapsed_ms) {
        let presentation_items = build_tree_items(
            presentation_tree,
            TreeItemBuildContext {
                elapsed_ms: options.elapsed_ms,
                current_unix_millis: options.current_unix_millis,
                titles_loading: options.titles_loading,
                recently_created: options.recently_created,
                agent_identifiers: options.agent_identifiers,
                icons: options.icons,
                tree_order: options.tree_order,
                sidebar_density: options.sidebar_density,
                panel_width: area.width,
                opened_paths: state.opened(),
                panes: options.panes,
            },
        );
        let mut presentation_state = copy_tree_state_for_presentation(state);
        frame.render_widget(Clear, list);
        let presentation_widget = TreeWidget::new(&presentation_items)
            .expect("top-level presentation items have unique identifiers")
            .highlight_style(theme::selected_style());
        frame.render_stateful_widget(presentation_widget, list, &mut presentation_state);
        apply_row_motions(
            frame,
            list,
            &presentation_items,
            &presentation_state,
            options.transitions,
            options.elapsed_ms,
        );
    } else {
        apply_row_motions(
            frame,
            list,
            &items,
            state,
            options.transitions,
            options.elapsed_ms,
        );
    }

    draw_scrollbar(frame, area, &items, state);

    if let Some(hit) = options.hover.node {
        if options
            .transitions
            .row_motion(hit.id, options.elapsed_ms)
            .is_none()
        {
            draw_row_actions(
                frame,
                area,
                hit.row,
                applicable_row_actions(tree, hit.id),
                options.use_stable_glyphs,
            );
        }
    }
    if is_toolbar_visible(options.focused, options.hover.toolbar_hovered) {
        draw_toolbar(frame, area, options.hover.toolbar_action);
    }
}

/// Copies interaction state into a throwaway renderer for the previous tree.
/// The authoritative `TreeState` is still rendered against the new tree first,
/// so hit testing and keyboard navigation never target a departing ghost row.
fn copy_tree_state_for_presentation(state: &TreeState<NodeId>) -> TreeState<NodeId> {
    let mut presentation_state = TreeState::default();
    for identifier in state.opened() {
        presentation_state.open(identifier.clone());
    }
    presentation_state.select(state.selected().to_vec());
    presentation_state.scroll_down(state.get_offset());
    presentation_state
}

/// Moves complete rendered rows at the buffer-cell level. Transforming after
/// `tui-tree-widget` draws keeps indentation, selection background, activity
/// icons, titles, and wide glyph continuation cells together as one row.
fn apply_row_motions(
    frame: &mut Frame,
    list: Rect,
    items: &[TreeItem<'static, NodeId>],
    state: &TreeState<NodeId>,
    transitions: &TreeTransitions,
    elapsed_ms: u128,
) {
    let visible_items = state.flatten(items);
    for (visible_row, item) in visible_items
        .iter()
        .skip(state.get_offset())
        .take(usize::from(list.height))
        .enumerate()
    {
        let Some(node_id) = item.identifier.last().copied() else {
            continue;
        };
        let Some(motion) = transitions.row_motion(node_id, elapsed_ms) else {
            continue;
        };
        let Ok(row_offset) = u16::try_from(visible_row) else {
            continue;
        };
        translate_row_left(frame, list, list.y.saturating_add(row_offset), motion);
    }
}

/// Applies one already-sampled transform without re-rendering or flattening.
fn translate_row_left(frame: &mut Frame, list: Rect, row: u16, motion: TreeRowMotion) {
    if motion.left_offset == 0 && !motion.is_dimmed {
        return;
    }

    let buffer = frame.buffer_mut();
    // Collect cells once, in order, then rewrite in-place after transformation.
    let cells: Vec<_> = (list.x..list.right())
        .map(|column| {
            let mut cell = buffer[(column, row)].clone();
            if motion.is_dimmed {
                cell.set_style(Style::new().add_modifier(Modifier::DIM));
            }
            cell
        })
        .collect();
    // Clear the row first.
    for column in list.x..list.right() {
        buffer[(column, row)].reset();
    }
    // Write transformed cells back at offset position.
    for (source_index, cell) in cells.into_iter().enumerate() {
        let Ok(source_column_offset) = u16::try_from(source_index) else {
            break;
        };
        if source_column_offset < motion.left_offset {
            continue;
        }
        let destination_column = list
            .x
            .saturating_add(source_column_offset - motion.left_offset);
        if destination_column < list.right() {
            buffer[(destination_column, row)] = cell;
        }
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
fn draw_row_actions(
    frame: &mut Frame,
    area: Rect,
    row: u16,
    actions: &[TreeRowAction],
    use_stable_glyphs: bool,
) {
    let Some(action_strip) = TreeRowActionStrip::from_tree_area(area) else {
        return;
    };
    let style = theme::selected_style().add_modifier(Modifier::BOLD);
    action_strip.draw(frame.buffer_mut(), row, actions, style, use_stable_glyphs);
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
    use ratatui::buffer::Buffer;
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

    fn render_tree_buffer(
        tree: &Tree,
        state: &mut TreeState<NodeId>,
        transitions: &TreeTransitions,
        recently_created: &HashMap<NodeId, u128>,
        elapsed_ms: u128,
    ) -> Buffer {
        let area = Rect::new(0, 0, 40, 8);
        let titles_loading = HashSet::new();
        let agent_identifiers = AgentIdentifierSettings::default();
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    area,
                    tree,
                    state,
                    TreeRenderOptions {
                        focused: false,
                        elapsed_ms,
                        current_unix_millis: 0,
                        project_name: None,
                        is_project_name_loading: false,
                        titles_loading: &titles_loading,
                        recently_created,
                        transitions,
                        agent_identifiers: &agent_identifiers,
                        icons: &IconSettings::default(),
                        tree_order: TreeOrder::Manual,
                        sidebar_density: SidebarDensity::default(),
                        use_stable_glyphs: false,
                        hover: TreeHoverState::default(),
                        panes: &HashMap::new(),
                    },
                );
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn buffer_row_text(buffer: &Buffer, row: u16) -> String {
        (buffer.area.x..buffer.area.right())
            .map(|column| buffer[(column, row)].symbol())
            .collect()
    }

    #[test]
    fn row_translation_moves_the_complete_buffer_run_left_and_dims_it() {
        let area = Rect::new(0, 0, 12, 2);
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        terminal
            .draw(|frame| {
                frame.render_widget(Paragraph::new("ABCDEFGHIJ"), area);
                translate_row_left(
                    frame,
                    Rect::new(0, 0, 10, 1),
                    0,
                    TreeRowMotion {
                        left_offset: 3,
                        is_dimmed: true,
                    },
                );
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(&buffer_row_text(buffer, 0)[..7], "DEFGHIJ");
        assert!(buffer[(0, 0)].modifier.contains(Modifier::DIM));
        assert_eq!(buffer[(7, 0)].symbol(), " ");
    }

    #[test]
    fn added_row_slides_into_place_before_the_creation_blink() {
        let mut previous_tree = Tree::new();
        let group_id = previous_tree.add_group(ROOT_ID, "work").unwrap();
        let mut new_tree = previous_tree.clone();
        let pane_id = new_tree
            .add_pane(
                group_id,
                "newly-created-pane",
                ilium_core::PaneContentKind::Terminal,
            )
            .unwrap();
        let mut transitions = TreeTransitions::default();
        let pulse_starts = transitions.observe_snapshot_change(&previous_tree, &new_tree, 0);
        let recently_created = pulse_starts.into_iter().collect::<HashMap<_, _>>();
        let mut state = TreeState::default();
        state.open(vec![group_id]);
        let pane_row = list_area(Rect::new(0, 0, 40, 8)).y + 1;

        let entering =
            render_tree_buffer(&new_tree, &mut state, &transitions, &recently_created, 0);
        assert!(!buffer_row_text(&entering, pane_row).contains("newly-created-pane"));
        assert!((entering.area.x..entering.area.right()).any(|column| {
            entering[(column, pane_row)]
                .modifier
                .contains(Modifier::DIM)
        }));

        let settled = render_tree_buffer(
            &new_tree,
            &mut state,
            &transitions,
            &recently_created,
            crate::tree_transitions::TREE_ENTRY_TRANSITION_MS,
        );
        assert!(buffer_row_text(&settled, pane_row).contains("newly-created-pane"));
        assert!((settled.area.x..settled.area.right()).any(|column| {
            settled[(column, pane_row)]
                .modifier
                .contains(Modifier::REVERSED)
        }));
        assert!(should_flash(
            pane_id,
            &recently_created,
            crate::tree_transitions::TREE_ENTRY_TRANSITION_MS
        ));
    }

    #[test]
    fn removed_row_slides_left_then_disappears_from_the_presentation_tree() {
        let mut previous_tree = Tree::new();
        let group_id = previous_tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = previous_tree
            .add_pane(
                group_id,
                "departing-pane",
                ilium_core::PaneContentKind::Terminal,
            )
            .unwrap();
        let mut new_tree = previous_tree.clone();
        new_tree.remove_node(pane_id).unwrap();
        let mut transitions = TreeTransitions::default();
        transitions.observe_snapshot_change(&previous_tree, &new_tree, 0);
        let mut state = TreeState::default();
        state.open(vec![group_id]);
        let pane_row = list_area(Rect::new(0, 0, 40, 8)).y + 1;

        let first = render_tree_buffer(&new_tree, &mut state, &transitions, &HashMap::new(), 0);
        assert!(buffer_row_text(&first, pane_row).contains("departing-pane"));

        let almost_gone = render_tree_buffer(
            &new_tree,
            &mut state,
            &transitions,
            &HashMap::new(),
            crate::tree_transitions::TREE_ENTRY_TRANSITION_MS - 1,
        );
        assert!(!buffer_row_text(&almost_gone, pane_row).contains("departing-pane"));
        assert!((almost_gone.area.x..almost_gone.area.right())
            .any(|column| almost_gone[(column, pane_row)]
                .modifier
                .contains(Modifier::DIM)));

        let gone = render_tree_buffer(
            &new_tree,
            &mut state,
            &transitions,
            &HashMap::new(),
            crate::tree_transitions::TREE_ENTRY_TRANSITION_MS,
        );
        assert!(!buffer_row_text(&gone, pane_row).contains("departing-pane"));
        assert_eq!(transitions.presentation_tree(220), None);
    }

    #[test]
    fn agent_identifier_modes_render_full_names_letters_icons_or_nothing() {
        let status = PaneStatus::Agent(AgentClass::Claude, AgentActivity::Idle);
        let mut settings = AgentIdentifierSettings::default();

        let full_name = pane_label(&status, "Fix auth", 0, false, &settings, None);
        assert_eq!(full_name.spans[0].content.trim(), "");
        assert!(line_text(&full_name).ends_with("Claude: Fix auth"));

        settings.mode = AgentIdentifierMode::Letter;
        let letter = pane_label(&status, "Fix auth", 0, false, &settings, None);
        assert_eq!(letter.spans[0].content.trim(), "");
        assert!(line_text(&letter).ends_with("C: Fix auth"));

        settings.mode = AgentIdentifierMode::Icon;
        settings.claude_icon = crate::config::ClaudeAgentIcon::Lobster;
        let icon = pane_label(&status, "Fix auth", 0, false, &settings, None);
        assert_eq!(icon.spans[0].content.trim_end(), "🦞");
        assert!(line_text(&icon).ends_with("Fix auth"));
        assert!(!line_text(&icon).contains("Claude:"));

        settings.mode = AgentIdentifierMode::Hidden;
        let hidden = pane_label(&status, "Fix auth", 0, false, &settings, None);
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
        let letter = pane_label(&status, "Review", 0, false, &settings, None);
        assert!(line_text(&letter).ends_with("X: Review"));

        settings.mode = AgentIdentifierMode::Icon;
        for icon in crate::config::CodexAgentIcon::ALL {
            settings.codex_icon = icon;
            let line = pane_label(&status, "Review", 0, false, &settings, None);
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
                None,
            );
            assert_eq!(line.spans[0].content.trim(), "");
            assert!(!line.spans[1].content.trim().is_empty());
            assert!(!line_text(&line).contains("Claude:"));
        }
    }

    #[test]
    fn agent_goal_flag_sits_directly_after_the_activity_indicator() {
        let settings = AgentIdentifierSettings::default();
        let goal_line = pane_label(
            &PaneStatus::AgentWithGoal(AgentClass::Codex, AgentActivity::Working),
            "Goal work",
            0,
            false,
            &settings,
            None,
        );
        let ordinary_line = pane_label(
            &PaneStatus::Agent(AgentClass::Codex, AgentActivity::Working),
            "Ordinary work",
            0,
            false,
            &settings,
            None,
        );

        assert_eq!(
            goal_line.spans[1].content.trim(),
            SPINNER_FRAMES[0].to_string()
        );
        assert_eq!(goal_line.spans[2].content.trim_end(), "🏁");
        assert_eq!(
            UnicodeWidthStr::width(goal_line.spans[2].content.as_ref()),
            GOAL_ICON_COLUMN_WIDTH
        );
        assert_eq!(ordinary_line.spans.len(), 3);
    }

    #[test]
    fn row_actions_default_to_normal_icons_and_offer_opt_in_stable_glyphs() {
        assert_eq!(theme::PEN_ICON, "✏️");
        assert_eq!(TreeToolbarAction::Shell.glyph(), TERMINAL_ICON);
        assert_eq!(TreeToolbarAction::Editor.glyph(), TEXT_EDITOR_ICON);
        assert_eq!(TreeToolbarAction::Settings.glyph(), SETTINGS_ICON);
        assert_eq!(TreeRowAction::Rename.glyph(false), theme::PEN_ICON);
        assert_eq!(TreeRowAction::MoveUp.glyph(false), "🔼");
        assert_eq!(TreeRowAction::MoveDown.glyph(false), "🔽");
        assert_eq!(TreeRowAction::Close.glyph(false), "🚫");
        assert_eq!(TreeRowAction::Retitle.glyph(false), "♻️");
        for action in TreeRowAction::ALL {
            let glyph = action.glyph(true);
            assert_eq!(
                UnicodeWidthStr::width(glyph),
                1,
                "{action:?} stable glyph must remain one terminal cell wide"
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
            None,
        );
        assert_eq!(shell_label.spans[0].content.trim_end(), TERMINAL_ICON);

        for is_dirty in [false, true] {
            let label = pane_label(
                &PaneStatus::Editor { dirty: is_dirty },
                "notes.md",
                0,
                false,
                &AgentIdentifierSettings::default(),
                None,
            );
            assert_eq!(label.spans[0].content.trim_end(), TEXT_EDITOR_ICON);
            assert_eq!(
                UnicodeWidthStr::width(label.spans[0].content.as_ref()),
                NODE_ICON_COLUMN_WIDTH
            );
        }
    }

    #[test]
    fn editor_label_composes_filename_and_title_only_when_they_differ() {
        // No distinct title yet (a fresh editor's `name` still just is its
        // filename, the pre-restructure default) -- shown plain, no dash.
        let untitled = pane_label(
            &PaneStatus::Editor { dirty: false },
            "notes.md",
            0,
            false,
            &AgentIdentifierSettings::default(),
            Some("notes.md"),
        );
        assert!(line_text(&untitled).trim_end().ends_with("notes.md"));
        assert!(!line_text(&untitled).contains('\u{2014}'));

        // A restructure-authored title distinct from the filename: both
        // must appear, filename first.
        let retitled = pane_label(
            &PaneStatus::Editor { dirty: false },
            "Auth Config Notes",
            0,
            false,
            &AgentIdentifierSettings::default(),
            Some("notes.md"),
        );
        assert!(line_text(&retitled)
            .trim_end()
            .ends_with("notes.md \u{2014} Auth Config Notes"));

        // Dirty marker still lands after the composed text, not the filename alone.
        let dirty = pane_label(
            &PaneStatus::Editor { dirty: true },
            "Auth Config Notes",
            0,
            false,
            &AgentIdentifierSettings::default(),
            Some("notes.md"),
        );
        assert!(line_text(&dirty).trim().ends_with("Auth Config Notes*"));
    }

    #[test]
    fn tree_labels_use_action_columns_until_that_specific_row_is_hovered() {
        let area = Rect::new(0, 0, 40, 8);
        let list = list_area(area);
        let action_strip = TreeRowActionStrip::from_tree_area(area).unwrap().area;
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
                        current_unix_millis: 0,
                        project_name: None,
                        is_project_name_loading: false,
                        titles_loading: &titles_loading,
                        recently_created: &recently_created,
                        transitions: &TreeTransitions::default(),
                        agent_identifiers: &agent_identifiers,
                        icons: &IconSettings::default(),
                        tree_order: TreeOrder::Manual,
                        sidebar_density: SidebarDensity::default(),
                        use_stable_glyphs: false,
                        hover: TreeHoverState::default(),
                        panes: &HashMap::new(),
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
                        current_unix_millis: 0,
                        project_name: None,
                        is_project_name_loading: false,
                        titles_loading: &titles_loading,
                        recently_created: &recently_created,
                        transitions: &TreeTransitions::default(),
                        agent_identifiers: &agent_identifiers,
                        icons: &IconSettings::default(),
                        tree_order: TreeOrder::Manual,
                        sidebar_density: SidebarDensity::default(),
                        use_stable_glyphs: false,
                        hover: TreeHoverState {
                            node: Some(TreeNodeHit {
                                id: first_group,
                                row: list.y,
                            }),
                            ..TreeHoverState::default()
                        },
                        panes: &HashMap::new(),
                    },
                );
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(
            buffer[(action_strip.x, list.y)].symbol().trim_end(),
            TreeRowAction::Rename.glyph(false)
        );
        // `TestBackend` deliberately stores one multi-cell terminal print
        // in its origin cell rather than emulating the terminal cursor. The
        // origin run is the authoritative assertion; the direct buffer-diff
        // test below proves it includes every required cleanup space.
        assert_eq!(
            UnicodeWidthStr::width(buffer[(action_strip.x, list.y)].symbol()),
            usize::from(ROW_ACTION_WIDTH)
        );
        for x in action_strip.x..action_strip.right() {
            assert_eq!(
                buffer[(x, list.y + 1)].symbol(),
                "y",
                "hovering the first row hid the second row's title at cell {x}"
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
                        current_unix_millis: 0,
                        project_name: None,
                        is_project_name_loading: false,
                        titles_loading: &titles_loading,
                        recently_created: &recently_created,
                        transitions: &TreeTransitions::default(),
                        agent_identifiers: &agent_identifiers,
                        icons: &IconSettings::default(),
                        tree_order: TreeOrder::Manual,
                        sidebar_density: SidebarDensity::default(),
                        use_stable_glyphs: false,
                        hover: TreeHoverState::default(),
                        panes: &HashMap::new(),
                    },
                );
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        for x in action_strip.x..action_strip.right() {
            assert_eq!(
                buffer[(x, list.y)].symbol(),
                "x",
                "title did not return after row actions disappeared at cell {x}"
            );
        }
    }

    #[test]
    fn row_action_strip_clears_every_slot_without_forcing_emoji_widths() {
        let area = Rect::new(0, 0, 33, 6);
        let row = 2;
        let mut previous = Buffer::empty(area);
        previous.set_string(
            area.x,
            row,
            "x".repeat(usize::from(area.width)),
            Style::default(),
        );
        let mut next = previous.clone();
        let action_strip = TreeRowActionStrip::from_tree_area(area).unwrap();
        action_strip.draw(
            &mut next,
            row,
            &TreeRowAction::ALL,
            theme::selected_style().add_modifier(Modifier::BOLD),
            false,
        );

        let controls_start = list_area(area).right() - ROW_ACTION_WIDTH * ROW_ACTION_COUNT;
        assert_eq!(next[(controls_start - 1, row)].symbol(), "x");

        // Every wide glyph and its cleanup spaces must be emitted as one
        // natural-width print. A terminal advances its cursor through the
        // whole run, so stale title characters cannot survive in its second
        // or third cell.
        let updates = previous.diff(&next);
        assert!(updates
            .iter()
            .all(|(_, _, cell)| cell.diff_option == Default::default()));

        for (action_index, action) in TreeRowAction::ALL.into_iter().enumerate() {
            let action_x = controls_start + action_index as u16 * ROW_ACTION_WIDTH;
            assert_eq!(
                next[(action_x, row)].symbol().trim_end(),
                action.glyph(false),
                "slot {action_index} did not retain its normal emoji glyph"
            );
            assert_eq!(
                UnicodeWidthStr::width(next[(action_x, row)].symbol()),
                usize::from(ROW_ACTION_WIDTH),
                "slot {action_index} did not include its cleanup spaces"
            );
            assert!(updates
                .iter()
                .any(|(update_x, update_y, _)| { *update_x == action_x && *update_y == row }));
            assert!(updates.iter().any(|(update_x, update_y, cell)| {
                *update_x == action_x
                    && *update_y == row
                    && UnicodeWidthStr::width(cell.symbol()) == usize::from(ROW_ACTION_WIDTH)
            }));

            for covered_offset in 1..ROW_ACTION_WIDTH {
                let covered_cell = &next[(action_x + covered_offset, row)];
                assert_eq!(covered_cell.symbol(), " ");
                assert_eq!(covered_cell.bg, theme::accent_bg());
            }
        }
        assert!(updates.iter().all(|(_, _, cell)| cell.symbol() != "x"));
    }

    #[test]
    fn selected_row_uses_ratatui_native_width_aware_icon_diffing() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "wide icon selection").unwrap();
        let transitions = TreeTransitions::default();
        let recently_created = HashMap::new();
        let mut unselected_state = TreeState::default();
        let unselected = render_tree_buffer(
            &tree,
            &mut unselected_state,
            &transitions,
            &recently_created,
            0,
        );

        let mut selected_state = TreeState::default();
        selected_state.select(vec![group]);
        let selected = render_tree_buffer(
            &tree,
            &mut selected_state,
            &transitions,
            &recently_created,
            0,
        );
        let list = list_area(Rect::new(0, 0, 40, 8));
        let selected_row = list.y;
        let wide_icon_column = (list.x..list.right())
            .find(|&column| UnicodeWidthStr::width(selected[(column, selected_row)].symbol()) > 1)
            .expect("the group icon is a multi-cell glyph");

        assert_eq!(
            selected[(wide_icon_column, selected_row)].diff_option,
            Default::default()
        );
        assert_eq!(
            selected[(wide_icon_column, selected_row)].bg,
            theme::accent_bg(),
        );

        let updates = unselected.diff(&selected);
        assert!(updates
            .iter()
            .any(|(column, row, _)| { *column == wide_icon_column && *row == selected_row }));
        assert!(updates
            .iter()
            .all(|(_, _, cell)| cell.diff_option == Default::default()));
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
        let nested_id = virtual_folder_node_id(folder, &root_path.join("nested"));
        let virtual_id = virtual_folder_node_id(folder, &file_path);

        assert_eq!(
            folder_entry(&tree, virtual_id),
            Some(FolderEntry {
                root_id: folder,
                path: file_path,
                is_directory: false,
                identifier_path: vec![group, folder, nested_id, virtual_id],
            })
        );
        let _ = std::fs::remove_dir_all(root_path);
    }

    #[test]
    fn folder_rows_materialize_only_along_expanded_paths_and_reach_deep_files() {
        let root_path = std::env::temp_dir().join(format!(
            "ilium-lazy-folder-tree-ui-{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root_path);
        let nested_path = root_path.join("first").join("second");
        std::fs::create_dir_all(&nested_path).expect("create nested folder fixture");
        let file_path = nested_path.join("deep.rs");
        std::fs::write(&file_path, "fn deep() {}\n").expect("write nested fixture file");

        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "workspace").unwrap();
        let folder = tree.add_folder(group, root_path.clone()).unwrap();
        let first_id = virtual_folder_node_id(folder, &root_path.join("first"));
        let second_id = virtual_folder_node_id(folder, &nested_path);

        let mut opened_paths = HashSet::new();
        let root_items = build_tree_items(
            &tree,
            TreeItemBuildContext {
                elapsed_ms: 0,
                current_unix_millis: 0,
                titles_loading: &HashSet::new(),
                recently_created: &HashMap::new(),
                agent_identifiers: &AgentIdentifierSettings::default(),
                icons: &IconSettings::default(),
                tree_order: TreeOrder::Manual,
                sidebar_density: SidebarDensity::default(),
                panel_width: 0,
                opened_paths: &opened_paths,
                panes: &HashMap::new(),
            },
        );
        assert!(root_items[0].children()[0].children().is_empty());

        opened_paths.insert(vec![group, folder]);
        let first_level_items = build_tree_items(
            &tree,
            TreeItemBuildContext {
                elapsed_ms: 0,
                current_unix_millis: 0,
                titles_loading: &HashSet::new(),
                recently_created: &HashMap::new(),
                agent_identifiers: &AgentIdentifierSettings::default(),
                icons: &IconSettings::default(),
                tree_order: TreeOrder::Manual,
                sidebar_density: SidebarDensity::default(),
                panel_width: 0,
                opened_paths: &opened_paths,
                panes: &HashMap::new(),
            },
        );
        let first_item = &first_level_items[0].children()[0].children()[0];
        assert_eq!(*first_item.identifier(), first_id);
        assert!(first_item.children().is_empty());

        opened_paths.insert(vec![group, folder, first_id]);
        opened_paths.insert(vec![group, folder, first_id, second_id]);
        let deep_items = build_tree_items(
            &tree,
            TreeItemBuildContext {
                elapsed_ms: 0,
                current_unix_millis: 0,
                titles_loading: &HashSet::new(),
                recently_created: &HashMap::new(),
                agent_identifiers: &AgentIdentifierSettings::default(),
                icons: &IconSettings::default(),
                tree_order: TreeOrder::Manual,
                sidebar_density: SidebarDensity::default(),
                panel_width: 0,
                opened_paths: &opened_paths,
                panes: &HashMap::new(),
            },
        );
        let deep_file = &deep_items[0].children()[0].children()[0].children()[0].children()[0];
        assert_eq!(
            folder_entry(&tree, *deep_file.identifier()),
            Some(FolderEntry {
                root_id: folder,
                path: file_path,
                is_directory: false,
                identifier_path: vec![group, folder, first_id, second_id, *deep_file.identifier()],
            })
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
            None,
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
                None,
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
            None,
        );
        assert_eq!(
            wrapped.spans[1].content.trim_end(),
            BACKGROUND_CLOCK_FRAMES[0].to_string()
        );
    }

    #[test]
    fn scheduled_pane_label_places_human_countdown_before_the_title() {
        let line = scheduled_pane_label(
            &PaneStatus::Agent(AgentClass::Claude, AgentActivity::Working),
            "Fix auth",
            0,
            1_000_000,
            &ScheduledPaneInput {
                execute_at_unix_millis: 4_661_000,
                text: "continue".to_string(),
                send_enter: true,
            },
            ScheduledPaneLabelContext {
                is_title_loading: false,
                agent_identifiers: &AgentIdentifierSettings::default(),
                editor_filename: None,
            },
        );

        assert_eq!(line.spans[2].content, "1h 01m 01s Claude: Fix auth");
    }

    #[test]
    fn scheduled_clock_frames_move_backwards_as_remaining_time_decreases() {
        let deadline = 10_000 + 10 * BACKGROUND_FRAME_MS as u64;
        let scheduled_input = ScheduledPaneInput {
            execute_at_unix_millis: deadline,
            text: String::new(),
            send_enter: true,
        };
        let first = scheduled_pane_label(
            &PaneStatus::PlainShell,
            "shell",
            0,
            10_000,
            &scheduled_input,
            ScheduledPaneLabelContext {
                is_title_loading: false,
                agent_identifiers: &AgentIdentifierSettings::default(),
                editor_filename: None,
            },
        );
        let second = scheduled_pane_label(
            &PaneStatus::PlainShell,
            "shell",
            0,
            10_000 + BACKGROUND_FRAME_MS as u64,
            &scheduled_input,
            ScheduledPaneLabelContext {
                is_title_loading: false,
                agent_identifiers: &AgentIdentifierSettings::default(),
                editor_filename: None,
            },
        );

        assert_eq!(
            first.spans[1].content.trim_end(),
            BACKGROUND_CLOCK_FRAMES[10].to_string()
        );
        assert_eq!(
            second.spans[1].content.trim_end(),
            BACKGROUND_CLOCK_FRAMES[9].to_string()
        );
        assert_eq!(human_readable_countdown(0), "now");
        assert_eq!(human_readable_countdown(61_001), "1m 02s");
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
            None,
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
        let opened_paths = HashSet::new();

        let items = build_tree_items(
            &tree,
            TreeItemBuildContext {
                elapsed_ms: 0,
                current_unix_millis: 0,
                titles_loading: &HashSet::new(),
                recently_created: &recently_created,
                agent_identifiers: &AgentIdentifierSettings::default(),
                icons: &IconSettings::default(),
                tree_order: TreeOrder::Manual,
                sidebar_density: SidebarDensity::default(),
                panel_width: 0,
                opened_paths: &opened_paths,
                panes: &HashMap::new(),
            },
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
    fn wide_toolbar_exposes_every_registered_agent_provider() {
        let actions: Vec<TreeToolbarAction> = toolbar_button_rects(Rect::new(0, 0, 80, 12))
            .into_iter()
            .map(|(action, _)| action)
            .collect();

        for provider in BuiltinAgentProvider::ALL {
            assert!(actions.contains(&TreeToolbarAction::Agent(provider)));
        }
    }

    #[test]
    fn toolbar_keeps_settings_right_aligned_without_overlapping_left_actions() {
        // `layout::MIN_TREE_WIDTH` (16) with a 1-cell border each side
        // leaves a 14-column-wide toolbar -- room for two left actions (the
        // search button is clipped first, then the project launcher) plus
        // the right-anchored Restructure and Settings buttons.
        let area = Rect::new(0, 0, 16, 12);
        let toolbar = toolbar_area(area);
        assert_eq!(toolbar.width, 14);

        assert_eq!(
            toolbar_action_at(area, Position::new(toolbar.x + 4, toolbar.y)),
            Some(TreeToolbarAction::Project)
        );
        assert_eq!(
            toolbar_action_at(area, Position::new(toolbar.x + 8, toolbar.y)),
            Some(TreeToolbarAction::Restructure)
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
        for (index, expected_action) in applicable_row_actions(&tree, shell)
            .iter()
            .copied()
            .enumerate()
        {
            for slot_offset in 0..ROW_ACTION_WIDTH {
                assert_eq!(
                    row_action_at(
                        &tree,
                        shell,
                        area,
                        5,
                        Position::new(
                            controls_start + index as u16 * ROW_ACTION_WIDTH + slot_offset,
                            5,
                        ),
                    ),
                    Some(expected_action),
                    "slot cell {slot_offset} did not map to {expected_action:?}",
                );
            }
        }
        assert_eq!(
            row_action_at(&tree, shell, area, 4, Position::new(controls_start, 5)),
            None
        );
    }

    #[test]
    fn row_action_strip_requires_its_complete_minimum_width() {
        let too_narrow = Rect::new(0, 0, ROW_ACTION_TOTAL_WIDTH + 1, 8);
        let exact_width = Rect::new(0, 0, ROW_ACTION_TOTAL_WIDTH + 2, 8);

        assert_eq!(list_area(too_narrow).width, ROW_ACTION_TOTAL_WIDTH - 1);
        assert!(TreeRowActionStrip::from_tree_area(too_narrow).is_none());
        assert_eq!(list_area(exact_width).width, ROW_ACTION_TOTAL_WIDTH);
        assert_eq!(
            TreeRowActionStrip::from_tree_area(exact_width)
                .unwrap()
                .area
                .width,
            ROW_ACTION_TOTAL_WIDTH,
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
        let opened_paths = HashSet::new();
        let items = build_tree_items(
            &tree,
            TreeItemBuildContext {
                elapsed_ms: 0,
                current_unix_millis: 0,
                titles_loading: &HashSet::new(),
                recently_created: &HashMap::new(),
                agent_identifiers: &AgentIdentifierSettings::default(),
                icons: &IconSettings::default(),
                tree_order: TreeOrder::Manual,
                sidebar_density: SidebarDensity::default(),
                panel_width: 0,
                opened_paths: &opened_paths,
                panes: &HashMap::new(),
            },
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
        let opened_paths = HashSet::new();
        let first = cache
            .get_or_build(&tree, 1, TreeOrder::Manual, &opened_paths)
            .len();
        assert_eq!(first, 1);

        // A second call at the same version returns the same cached
        // shape without needing the tree to still be reachable/unchanged
        // in any way this test can observe from the outside -- the real
        // guarantee (no rebuild happened) is covered by
        // `App::tree_node_at` being safe to call every mouse-move; this
        // just pins the observable contract (stable length per version).
        let second = cache
            .get_or_build(&tree, 1, TreeOrder::Manual, &opened_paths)
            .len();
        assert_eq!(second, 1);

        // A second top-level group (not a second pane nested under the same
        // one) so the rebuilt top-level item count actually differs from
        // `first`/`second` -- if `get_or_build` never rebuilt on the new
        // version, this would still (wrongly) report 1.
        let mut new_tree = Tree::new();
        new_tree.add_group(ROOT_ID, "group").unwrap();
        new_tree.add_group(ROOT_ID, "group-2").unwrap();
        let third = cache
            .get_or_build(&new_tree, 2, TreeOrder::Manual, &opened_paths)
            .len();
        assert_eq!(third, 2);
    }

    #[test]
    fn tree_item_cache_rebuilds_when_order_changes_at_the_same_tree_version() {
        let mut tree = Tree::new();
        let zebra = tree.add_group(ROOT_ID, "Zebra").unwrap();
        let alpha = tree.add_group(ROOT_ID, "alpha").unwrap();
        let mut cache = TreeItemCache::default();
        let opened_paths = HashSet::new();

        let manual_ids: Vec<NodeId> = cache
            .get_or_build(&tree, 1, TreeOrder::Manual, &opened_paths)
            .iter()
            .map(|item| *item.identifier())
            .collect();
        let alphabetical_ids: Vec<NodeId> = cache
            .get_or_build(&tree, 1, TreeOrder::NameAscending, &opened_paths)
            .iter()
            .map(|item| *item.identifier())
            .collect();

        assert_eq!(manual_ids, [zebra, alpha]);
        assert_eq!(alphabetical_ids, [alpha, zebra]);
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
