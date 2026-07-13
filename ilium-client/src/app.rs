//! `App`: ilium-client's thin orchestrator state. Owns a read-only
//! render-cache mirror of the session tree (kept in sync from
//! `ServerEvent::TreeSnapshot`/`PaneStatusChanged`/`ScreenUpdate` -- see
//! `crate::render_cache`), plus purely local UI state (focus, input mode,
//! hover/animation state, editor pane buffers).
//!
//! Unlike the pre-client/server `App`, this one never spawns a PTY and
//! never mutates `self.tree` directly in response to user input -- every
//! structural change (new/close/move/rename a node) is expressed as an
//! `ilium_ipc::ClientRequest` pushed onto `outbox` for the connection
//! task to actually send; `self.tree` only changes when the server's own
//! `TreeSnapshot` confirms it did. This is what keeps there being exactly
//! one writable tree (the server's) -- see the crate's module docs.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use ilium_core::{
    AgentActivity, AgentClass, GroupListing, Node, NodeId, NodeKind, PaneContentKind, PaneStatus,
    SplitOrientation, Tree, ROOT_ID,
};
use ilium_ipc::ClientRequest;
use ratatui::layout::{Position, Rect};
use tui_tree_widget::TreeState;

use crate::board::BoardPane;
use crate::config::{KeyboardSettings, UiSettings};
use crate::editor_pane::{EditorPane, EditorViewMode};
use crate::explorer_overlay::ExplorerOverlay;
use crate::layout::{TreeWidthAnimation, UiLayout};
use crate::naming_workers::TitleTrigger;
use crate::split_layout::{self, PaneViewport};
use crate::terminal_title_inference;
use crate::terminal_view::{self, TerminalView};
use crate::text_prompt::TextPromptState;
use crate::theme::{self, ColorScheme, Theme};
use crate::tree_ui::{self, TreeNodeHit, TreeToolbarAction};

/// Rows scrolled per wheel notch over a terminal pane's own scrollback --
/// matches `tree_state.scroll_up(3)`/`scroll_down(3)`'s existing per-notch
/// amount elsewhere in this crate.
const TERMINAL_WHEEL_SCROLL_LINES: u16 = 3;

/// Upper bound on `App::pending_editor_opens` -- see that field's doc
/// comment for why an entry can otherwise outlive its request forever (a
/// `NewPane` request the server rejects, or one whose confirming node never
/// arrives for any other reason, leaves nothing to ever consume it). Far
/// above any realistic number of file-picker opens in flight at once, so
/// this only ever trims genuinely abandoned entries, never a live request.
const MAX_PENDING_EDITOR_OPENS: usize = 64;

/// Which side of the UI currently has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusTarget {
    Tree,
    Pane,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RightPanelTarget {
    Empty,
    Pane {
        pane_id: NodeId,
    },
    SplitView {
        split_id: NodeId,
        active_pane_id: Option<NodeId>,
    },
}

impl RightPanelTarget {
    pub const fn active_pane_id(&self) -> Option<NodeId> {
        match self {
            Self::Empty
            | Self::SplitView {
                active_pane_id: None,
                ..
            } => None,
            Self::Pane { pane_id } => Some(*pane_id),
            Self::SplitView {
                active_pane_id: Some(pane_id),
                ..
            } => Some(*pane_id),
        }
    }
}

/// The input-handling mode. Most keys are dispatched differently
/// depending on this; see `crate::keys`.
pub enum Mode {
    Normal,
    LeaderPending,
    Move,
    /// In-progress rename prompt for the selected node.
    Rename(TextPromptState),
    /// In-progress command-line prompt for `Action::RunCommand`.
    CommandPrompt(TextPromptState),
    /// In-progress "Save As" filename prompt for the editor pane `NodeId`.
    SaveAs(NodeId, TextPromptState),
    Help,
    /// A file-picker overlay is open. Unlike the pre-client/server `App`,
    /// the `NodeId` here is the *destination group* the picked file's new
    /// editor pane will be created under -- there is no placeholder tree
    /// node to fill in anymore, since the tree only ever changes once the
    /// server confirms a `NewPane` request.
    Explorer(Box<ExplorerOverlay>, NodeId),
    /// A right-click action menu layered over a file picker. Keeping the
    /// overlay itself in the mode means Escape returns exactly to its prior
    /// directory, selection, and scroll position.
    ExplorerFileMenu(ExplorerFileMenu),
    /// A directory-only picker; the selected directory becomes one Folder node.
    FolderExplorer(Box<ExplorerOverlay>, NodeId),
    /// A mouse-anchored action menu for one tree node.
    ContextMenu(ContextMenu),
    /// The "New group" destination picker is open.
    CreateGroup(CreateGroupState),
    CreateSplitOrientation(CreateSplitOrientationState),
    CreateSplitMembers(CreateSplitMembersState),
    /// Board creation owns its storage choice and destination before a tree
    /// node exists, so a cancelled dialog cannot leave a phantom pane.
    CreateBoard(CreateBoardState),
    BoardPathPicker(Box<ExplorerOverlay>, CreateBoardState),
    BoardCardPrompt(NodeId, TextPromptState),
    BoardColumnPrompt(NodeId, TextPromptState),
    BoardRenamePrompt(NodeId, BoardRenameTarget, TextPromptState),
    BoardDeleteConfirm(NodeId, BoardDeleteTarget),
    /// A Yes/No confirmation is pending before closing `NodeId`.
    ConfirmClose(NodeId),
    /// The full-screen settings view is open, replacing the entire screen
    /// (see `crate::settings_ui`'s module doc comment for the UI/UX brief).
    /// Reached via `Action::Settings`, the tree footer's settings button, or
    /// `ContextMenuAction::Settings` (right-click the tree panel).
    Settings(SettingsState),
}

/// Which tab is selected in the full-screen settings view. Add a new
/// variant here -- and a matching arm in every `match` over this type --
/// before adding a third tab; see `crate::settings_ui`'s module doc comment
/// for the tab-list-left/content-right layout this drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Appearance,
    Keyboard,
    About,
}

impl SettingsTab {
    /// Every tab, in the order the tab list renders them.
    pub const ALL: [SettingsTab; 3] = [Self::Appearance, Self::Keyboard, Self::About];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Appearance => "User Appearance",
            Self::Keyboard => "Keyboard",
            Self::About => "About",
        }
    }

    /// The tab that follows this one, wrapping around -- drives the
    /// settings screen's `Tab`/click-another-tab navigation.
    pub fn next(self) -> Self {
        let all = Self::ALL;
        let index = all.iter().position(|tab| *tab == self).unwrap_or(0);
        all[(index + 1) % all.len()]
    }

    /// The tab that precedes this one, wrapping around.
    pub fn previous(self) -> Self {
        let all = Self::ALL;
        let index = all.iter().position(|tab| *tab == self).unwrap_or(0);
        all[(index + all.len() - 1) % all.len()]
    }
}

/// One row in the "User Appearance" tab's settings list.
///
/// Adding setting #4: add a variant here, add it to `ALL`, give it a label/
/// value string in `crate::settings_ui`, and give it a branch in
/// `App::settings_activate_row`/`App::settings_adjust_row`. Nothing else
/// needs to change -- keyboard nav, mouse clicks, and rendering all walk
/// `ALL` rather than hardcoding a row count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppearanceRow {
    AutoResizeTree,
    TreeWidth,
    ColorScheme,
}

impl AppearanceRow {
    pub const ALL: [AppearanceRow; 3] = [Self::AutoResizeTree, Self::TreeWidth, Self::ColorScheme];
}

/// Rows in the Keyboard tab. The single row is deliberately modeled as an
/// enum/`ALL` list so navigation and hit testing keep the same extension
/// point as Appearance when per-action remapping is exposed in this view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardRow {
    ShortcutBase,
}

impl KeyboardRow {
    pub const ALL: [KeyboardRow; 1] = [Self::ShortcutBase];
}

/// Full-screen settings view state (`Mode::Settings`).
///
/// Design brief for whoever adds the next setting (the user's own words,
/// kept verbatim so intent survives paraphrasing):
///
/// > when we enter the settings, the **entire** screen is replaced by the
/// > settings, and it's a nice UI with nice settings... the settings
/// > window/view should be very aerated and use the flex and other layout
/// > features of ratatui to make it nice... work at any size of the
/// > terminal including getting scrollbars if it's too small and letting
/// > you navigate it up/down with the mouse wheel... the look and feel
/// > should be a tab list on the left, and settings panel on the right, but
/// > no separation "bars"/lines, feeling more "aerated" than the main view.
///
/// Concretely: every control here is a live, self-applying toggle/stepper
/// (see `App::apply_and_persist_ui_settings`) -- there is no buffered
/// "Cancel" path, so a value changes the instant it's touched, the same way
/// a rename or a theme hex edit in `config.toml` would. `tab`/`selected_row`
/// are pure navigation state; the actual settings values live in
/// `App::ui_settings`, not here, so nothing here needs its own persistence.
pub struct SettingsState {
    pub tab: SettingsTab,
    /// Selected row within the active tab's list (an `AppearanceRow::ALL`
    /// index) -- meaningless while `tab == SettingsTab::About`, which has no
    /// rows to select.
    pub selected_row: usize,
    /// Vertical scroll offset into the active tab's content -- for a
    /// terminal too short to show every row at once. See
    /// `crate::settings_ui::content_scroll_bounds`.
    pub scroll: u16,
}

impl SettingsState {
    pub fn new() -> Self {
        Self {
            tab: SettingsTab::Appearance,
            selected_row: 0,
            scroll: 0,
        }
    }
}

impl Default for SettingsState {
    fn default() -> Self {
        Self::new()
    }
}

/// Actions exposed by a right-click on a tree entry. These deliberately map
/// to the same focused-node operations as the keyboard, so neither input
/// path can drift into a different tree mutation policy.
///
/// This still has no "Indent into previous group" / "Outdent" entry:
/// `ilium_ipc::ClientRequest::ReparentNode` (backing `Tree::move_node`)
/// makes that mutation possible, and it's what mouse drag-and-drop
/// (`crate::mouse`) and the leader/move-mode indent/outdent keys
/// (`crate::keys`) use, but a context-menu entry needs a mouse position to
/// mean anything ("indent into *which* preceding group" isn't well-defined
/// from a menu click the way it is from a specific drop position or an
/// ordered sibling walk) -- left out of the menu for that reason, not a
/// protocol gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextMenuAction {
    FocusPane,
    ShowSplitView,
    ToggleGroup,
    NewTerminal,
    NewEditor,
    NewGroup,
    NewSplitView,
    NewFolder,
    Rename,
    MoveUp,
    MoveDown,
    Close,
    /// Opens the full-screen settings view -- present in every right-click
    /// menu regardless of what was clicked (a pane, a group, or empty tree
    /// space), since it isn't a per-node action. This is deliberately the
    /// *only* mouse entry point into settings (plus the `Settings` leader
    /// action) -- see `Mode::Settings`'s doc comment.
    Settings,
}

impl ContextMenuAction {
    /// The concise label rendered in the popup menu.
    pub const fn label(self) -> &'static str {
        match self {
            Self::FocusPane => "Focus pane",
            Self::ShowSplitView => "Show split view",
            Self::ToggleGroup => "Expand / collapse",
            Self::NewTerminal => "New terminal here",
            Self::NewEditor => "New editor here",
            Self::NewGroup => "New group\u{2026}",
            Self::NewSplitView => "New split view\u{2026}",
            Self::NewFolder => "Open folder\u{2026}",
            Self::Rename => "Rename",
            Self::MoveUp => "Move up",
            Self::MoveDown => "Move down",
            Self::Close => "Close",
            Self::Settings => "Settings\u{2026}",
        }
    }
}

/// State of a context menu: its tree target, screen position, and keyboard
/// or mouse selection. The renderer only reads this state; all effects stay
/// in `App`/`crate::keys`/`crate::mouse`.
pub struct ContextMenu {
    pub target: NodeId,
    pub area: Rect,
    pub actions: Vec<ContextMenuAction>,
    pub selected_index: usize,
}

pub struct ExplorerFileMenu {
    pub overlay: Box<ExplorerOverlay>,
    pub target_group: NodeId,
    pub file_path: PathBuf,
    pub area: Rect,
}

/// State of the "New group" destination-picker dialog. `destinations` is a
/// snapshot taken when the dialog opened -- it does not track further tree
/// mutations, matching every other modal in ilium.
pub struct CreateGroupState {
    pub area: Rect,
    pub destinations: Vec<GroupListing>,
    pub selected_index: usize,
    pub name: TextPromptState,
}

pub struct CreateSplitOrientationState {
    pub orientation: SplitOrientation,
}

pub struct SplitPaneChoice {
    pub pane_id: NodeId,
    pub label: String,
    pub selected: bool,
}

pub struct CreateSplitMembersState {
    pub parent_group: NodeId,
    pub orientation: SplitOrientation,
    pub choices: Vec<SplitPaneChoice>,
    pub selected_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardStorageKind {
    Folder,
    MarkdownFile,
}

pub struct CreateBoardState {
    pub name: TextPromptState,
    pub path: TextPromptState,
    pub storage_kind: BoardStorageKind,
    pub editing_path: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardRenameTarget {
    Card,
    Column,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardDeleteTarget {
    Card,
    Column,
}

/// The live, client-local half of a pane. For a PTY-backed pane this is
/// only a render cache fed by `ServerEvent::ScreenUpdate` -- see
/// `crate::terminal_view`'s module docs for why ilium-client owns no PTY
/// handle at all. Editor panes are unchanged from the pre-client/server
/// design: buffer content and file I/O stay entirely client-local.
pub enum PaneRuntime {
    Terminal(Box<TerminalView>),
    Editor(Box<EditorPane>),
    Board(Box<BoardPane>),
}

/// A background LLM title-inference worker `App` has decided to start but
/// can't spawn itself -- `App` never holds a `NamingWorkers` handle (see
/// its module docs on why input dispatch stays synchronous and
/// unit-testable). Queued onto `App::pending_retitle_requests` and drained
/// by `crate::run`'s event loop right after the input event that produced
/// it, the same "propose here, spawn there" split `outbox`/`ClientRequest`
/// already uses for server requests.
pub enum PendingRetitleRequest {
    /// An agent pane whose session ID is already known (see
    /// `App::agent_session_ids`) -- mirrors what `crate::title_inference`
    /// resolves automatically, but only ever queued here for a `Manual`
    /// trigger (the automatic path spawns directly from `crate::run`, since
    /// it isn't input-driven).
    Session {
        pane_id: NodeId,
        agent_class: AgentClass,
        session_id: String,
        trigger: TitleTrigger,
    },
    /// A plain-shell terminal pane, with its screen text already captured
    /// (a `vt100::Parser` isn't shareable across the worker-thread
    /// boundary, so the text must be read out on the main thread before
    /// queuing).
    Terminal {
        pane_id: NodeId,
        screen_text: String,
        trigger: TitleTrigger,
    },
}

pub struct App {
    /// The session this client is attached to (used to resolve the UDS
    /// socket path and to label the window/status bar).
    pub session_name: String,
    /// Read-only mirror of the server's tree -- see the module docs.
    pub tree: Tree,
    pub panes: HashMap<NodeId, PaneRuntime>,
    /// `ClientRequest`s produced by input handling this tick, drained and
    /// sent by the connection task after each event is dispatched (see
    /// `crate::run`). Keeping this a plain outbox instead of giving `App`
    /// a channel/socket handle directly is what keeps input dispatch
    /// synchronous and unit-testable without a real connection.
    outbox: Vec<ClientRequest>,
    pub tree_state: TreeState<NodeId>,
    pub right_panel_target: RightPanelTarget,
    pub focus: FocusTarget,
    pub mode: Mode,
    pub status_message: Option<String>,
    pub should_quit: bool,
    /// Stable reference for purely visual animations in the tree.
    pub started_at: Instant,
    /// (rows, cols) of the pane *content* area last reported by the event
    /// loop. Freshly created panes are asked for at this size, so a pane
    /// created after startup doesn't wait for the next resize event.
    pub last_known_pane_size: (u16, u16),
    /// Geometry from the last terminal-size calculation.
    pub layout: UiLayout,
    tree_width_animation: TreeWidthAnimation,
    /// This session's live `[ui]` settings -- the settings screen's own
    /// working state (`Mode::Settings` only holds navigation state; the
    /// actual values live here). See `apply_ui_settings`/
    /// `apply_and_persist_ui_settings`.
    pub ui_settings: UiSettings,
    /// This session's live shortcut-base setting. Input dispatch and every
    /// displayed shortcut label read this same value.
    pub keyboard_settings: KeyboardSettings,
    /// Where to write `config.toml` when a setting changes -- `None` when
    /// `crate::paths::config_dir` couldn't be resolved at startup, in which
    /// case settings changes still apply live but can't be persisted (see
    /// `apply_and_persist_ui_settings`).
    pub config_dir: Option<PathBuf>,
    pointer_position: Option<Position>,
    is_terminal_focused: bool,
    tree_drag_source: Option<NodeId>,
    pub hovered_tree_node: Option<TreeNodeHit>,
    pub tree_toolbar_hovered: bool,
    pub hovered_tree_toolbar_action: Option<TreeToolbarAction>,
    help_leader_pending: bool,
    /// The session's project directory. Every new terminal pane spawns
    /// here (server-side), and the file-picker overlay always opens
    /// rooted here.
    pub session_cwd: PathBuf,
    pub project_name: Option<String>,
    pub is_project_name_loading: bool,
    /// Terminal panes currently awaiting `session_naming::infer_pane_title`
    /// -- see `crate::naming_workers`.
    pub titles_loading: HashSet<NodeId>,
    /// Files this client itself just asked the server to open as a new
    /// editor pane (via `request_new_editor`), keyed by file basename --
    /// consumed by `crate::render_cache::apply_tree_snapshot` to load the
    /// matching new tree node's content locally once the server confirms
    /// it. See that function's doc comment for why an editor pane's
    /// content can't be reconstructed from the tree snapshot alone (the
    /// tree only ever records a pane's display name, not a file path).
    /// A `Vec` in request order, not a map, since two picks of
    /// same-named files in different directories are legitimately
    /// ambiguous by basename alone -- first-requested, first-matched is
    /// the best available heuristic without extending the wire protocol
    /// to carry a path back. Capped at `MAX_PENDING_EDITOR_OPENS`: a
    /// request whose `NewPane` the server rejects (or that otherwise never
    /// gets a confirming tree node) has nothing to ever call
    /// `take_matching_pending_editor_open` for it, so without a bound this
    /// would grow by one abandoned entry per failed open for the life of
    /// the client process.
    pending_editor_opens: Vec<(String, PathBuf)>,
    pub markdown_picker: ratatui_image::picker::Picker,
    pub markdown_rasterizer: crate::markdown::raster::HeaderRasterizer,
    /// Creation-pulse bookkeeping: node id -> `started_at`-relative offset
    /// (ms) at which this client first observed it existing. Read by
    /// `tree_ui` to flash a freshly created node so a click (or a
    /// multi-create burst) is obviously followed by something appearing;
    /// pruned by `prune_recently_created` once its flash window elapses or
    /// the node is gone. See `track_newly_created_nodes` for why the very
    /// first tree snapshot after attaching never populates this (a
    /// boot-time restore of a whole persisted session must not flash every
    /// node at once).
    pub recently_created: HashMap<NodeId, u128>,
    /// Whether `track_newly_created_nodes` has processed at least one
    /// snapshot yet -- see that method's doc comment.
    has_applied_first_snapshot: bool,
    /// Agent pane session/thread IDs the server has discovered (see
    /// `ilium-server`'s `session_id` module and
    /// `ServerEvent::PaneSessionIdResolved`), cached client-side so a later
    /// retry (`crate::title_inference`'s `PaneBecameDone` trigger) doesn't
    /// need the server to resend it.
    pub agent_session_ids: HashMap<NodeId, String>,
    /// Server-replayed paths for editor panes that existed before this
    /// client attached. They let this client construct its own local editor
    /// buffer instead of treating a restored editor as a missing pane.
    pub restored_editor_paths: HashMap<NodeId, PathBuf>,
    /// How many times background session-title inference has been
    /// attempted per pane/session pair -- bounds `crate::title_inference`'s retry path
    /// (`title_inference::MAX_ATTEMPTS`) so a pane whose transcript never
    /// has anything summarizable doesn't retry forever.
    pub title_inference_attempts: HashMap<(NodeId, String), u32>,
    /// The session ID for which each pane's current automatic title was
    /// inferred. A `/resume` can replace an agent session inside one pane,
    /// so a completed title must not suppress inference for the new one.
    pub inferred_title_session_ids: HashMap<NodeId, String>,
    /// How many completed Enter presses have been observed in a plain
    /// terminal pane since its last LLM retitle -- drives
    /// `maybe_trigger_terminal_retitle`'s "every second command" cadence.
    /// Only ever bumped while `terminal_title_inference::terminal_ready_for_retitle`
    /// says the pane is eligible, so a pane that becomes an agent, gets a
    /// user-specified title, or already has a retitle in flight simply
    /// stops accumulating rather than firing on a stale count once it
    /// becomes eligible again.
    pub enter_press_counts: HashMap<NodeId, u32>,
    /// The hash (`terminal_title_inference::hash_screen_text`) of the screen
    /// text last used for a terminal pane's automatic retitle -- lets
    /// `maybe_trigger_terminal_retitle` skip firing again on cadence alone
    /// when the pane's visible content is unchanged since that call, since
    /// unlike agent panes a plain shell has no "already titled for this
    /// session" cache to fall back on.
    pub terminal_retitle_content_hashes: HashMap<NodeId, u64>,
    /// Retitle workers `App` has decided to start but can't spawn itself --
    /// see `PendingRetitleRequest`'s doc comment.
    pending_retitle_requests: Vec<PendingRetitleRequest>,
    /// Bumped every time `render_cache::apply_tree_snapshot` replaces
    /// `self.tree`. Exists purely so `tree_hit_test_cache` can tell whether
    /// its cached `TreeItem`s are still valid without comparing the whole
    /// tree -- see that field's doc comment.
    tree_version: u64,
    /// Caches the structural `TreeItem` list used for mouse hit-testing
    /// (`tree_node_at`), which -- unlike the labels `tree_ui::render`
    /// builds every frame -- never depends on animation state: hit-testing
    /// only reads each item's identifier/nesting, so it always builds with
    /// the same fixed `elapsed_ms: 0` and empty loading/recently-created
    /// inputs (see `tree_ui::node_at_position`'s previous direct call).
    /// That means the built items are fully determined by tree structure
    /// alone, so it's safe to reuse them across every mouse-move hit test
    /// until `tree_version` actually changes, instead of re-walking the
    /// whole tree and re-allocating a fresh label per node on every mouse
    /// move.
    tree_hit_test_cache: tree_ui::TreeItemCache,
}

impl App {
    /// Starts a client session with an empty render-cache tree -- the real
    /// tree arrives moments later as the first `ServerEvent::TreeSnapshot`
    /// once `Attach` completes (see `crate::render_cache::apply`).
    pub fn new(session_name: String, session_cwd: PathBuf) -> Self {
        let started_at = Instant::now();
        Self {
            session_name,
            tree: Tree::new(),
            panes: HashMap::new(),
            outbox: Vec::new(),
            tree_state: TreeState::default(),
            right_panel_target: RightPanelTarget::Empty,
            focus: FocusTarget::Pane,
            mode: Mode::Normal,
            status_message: None,
            should_quit: false,
            started_at,
            last_known_pane_size: (terminal_view::DEFAULT_ROWS, terminal_view::DEFAULT_COLS),
            layout: UiLayout::default(),
            tree_width_animation: TreeWidthAnimation::new(
                started_at,
                UiSettings::default().tree_width,
            ),
            ui_settings: UiSettings::default(),
            keyboard_settings: KeyboardSettings::default(),
            config_dir: None,
            pointer_position: None,
            is_terminal_focused: true,
            tree_drag_source: None,
            hovered_tree_node: None,
            tree_toolbar_hovered: false,
            hovered_tree_toolbar_action: None,
            help_leader_pending: false,
            session_cwd,
            project_name: None,
            is_project_name_loading: false,
            titles_loading: HashSet::new(),
            pending_editor_opens: Vec::new(),
            // Talks to stdio once at startup; falls back to half-block
            // rendering (works everywhere, no protocol needed) if the
            // terminal doesn't answer the capability query.
            markdown_picker: ratatui_image::picker::Picker::from_query_stdio()
                .unwrap_or_else(|_| ratatui_image::picker::Picker::halfblocks()),
            markdown_rasterizer: crate::markdown::raster::HeaderRasterizer::new(),
            recently_created: HashMap::new(),
            has_applied_first_snapshot: false,
            agent_session_ids: HashMap::new(),
            restored_editor_paths: HashMap::new(),
            title_inference_attempts: HashMap::new(),
            inferred_title_session_ids: HashMap::new(),
            enter_press_counts: HashMap::new(),
            terminal_retitle_content_hashes: HashMap::new(),
            pending_retitle_requests: Vec::new(),
            tree_version: 0,
            tree_hit_test_cache: tree_ui::TreeItemCache::default(),
        }
    }

    /// Updates creation-pulse bookkeeping ahead of `render_cache` swapping in
    /// `new_tree`: any node id it carries that wasn't already in `self.tree`
    /// gets a flash start time recorded, *except* on the very first snapshot
    /// this client ever applies -- a boot-time restore of a whole persisted
    /// session must not flash every node in it at once. Multiple nodes
    /// created in the same snapshot (a multi-create burst) all get recorded
    /// together here, so each still flashes independently once rendered.
    pub(crate) fn track_newly_created_nodes(&mut self, new_tree: &Tree) {
        if self.has_applied_first_snapshot {
            let previous_ids: HashSet<NodeId> = self.tree.all_ids().collect();
            let created_offset_ms = self.started_at.elapsed().as_millis();
            for id in new_tree.all_ids() {
                if !previous_ids.contains(&id) {
                    self.recently_created.entry(id).or_insert(created_offset_ms);
                }
            }
        }
        self.has_applied_first_snapshot = true;
    }

    /// Invalidates `tree_hit_test_cache` -- called once per
    /// `ServerEvent::TreeSnapshot` applied, regardless of whether the new
    /// tree actually differs from the old one (the server only ever sends
    /// a snapshot after an actual structural change or on attach, so
    /// treating every snapshot as a potential structural change costs at
    /// most one extra rebuild on an idle session, never a stale hit test).
    pub(crate) fn bump_tree_version(&mut self) {
        self.tree_version = self.tree_version.wrapping_add(1);
    }

    /// Drops creation-pulse bookkeeping for nodes no longer in the tree, or
    /// whose flash window has fully elapsed, keeping the map bounded
    /// independent of how often snapshots arrive.
    pub(crate) fn prune_recently_created(&mut self) {
        let now_offset = self.started_at.elapsed().as_millis();
        self.prune_recently_created_at(now_offset);
    }

    /// `prune_recently_created`'s logic with an explicit "now" offset, so
    /// tests can exercise flash-window expiry without a real clock.
    fn prune_recently_created_at(&mut self, now_offset_ms: u128) {
        let live_ids: HashSet<NodeId> = self.tree.all_ids().collect();
        self.recently_created.retain(|id, created_offset_ms| {
            live_ids.contains(id)
                && now_offset_ms.saturating_sub(*created_offset_ms)
                    < tree_ui::RECENTLY_CREATED_PULSE_MS
        });
    }

    /// Drains every `ClientRequest` queued by input handling since the
    /// last drain, for the caller to actually send over the connection.
    pub fn take_outbound_requests(&mut self) -> Vec<ClientRequest> {
        std::mem::take(&mut self.outbox)
    }

    /// Drains every `PendingRetitleRequest` queued since the last drain,
    /// for `crate::run::dispatch_input_event` to actually spawn a worker
    /// for -- see that type's doc comment.
    pub fn take_pending_retitle_requests(&mut self) -> Vec<PendingRetitleRequest> {
        std::mem::take(&mut self.pending_retitle_requests)
    }

    pub(crate) fn queue_request(&mut self, request: ClientRequest) {
        self.outbox.push(request);
    }

    pub fn active_pane_id(&self) -> Option<NodeId> {
        self.right_panel_target.active_pane_id()
    }

    pub fn displayed_pane_ids(&self) -> Vec<NodeId> {
        match self.right_panel_target {
            RightPanelTarget::Empty => Vec::new(),
            RightPanelTarget::Pane { pane_id } => vec![pane_id],
            RightPanelTarget::SplitView { split_id, .. } => self
                .tree
                .children_of(split_id)
                .map(|children| {
                    children
                        .iter()
                        .copied()
                        .filter(|pane_id| self.tree.get(*pane_id).is_some_and(Node::is_pane))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    pub fn pane_viewports(&self) -> Vec<PaneViewport> {
        match self.right_panel_target {
            RightPanelTarget::Empty => Vec::new(),
            RightPanelTarget::Pane { pane_id } => split_layout::allocate_viewports(
                self.layout.pane_area,
                SplitOrientation::Vertical,
                &[pane_id],
            ),
            RightPanelTarget::SplitView { split_id, .. } => {
                let orientation = self
                    .tree
                    .split_orientation(split_id)
                    .unwrap_or(SplitOrientation::Vertical);
                split_layout::allocate_viewports(
                    self.layout.pane_area,
                    orientation,
                    &self.displayed_pane_ids(),
                )
            }
        }
    }

    pub fn pane_viewport(&self, pane_id: NodeId) -> Option<PaneViewport> {
        self.pane_viewports()
            .into_iter()
            .find(|viewport| viewport.pane_id == pane_id)
    }

    pub fn pane_viewport_at(&self, position: Position) -> Option<PaneViewport> {
        split_layout::viewport_at(&self.pane_viewports(), position)
    }

    pub fn reconcile_right_panel_target(&mut self) {
        self.right_panel_target = match self.right_panel_target.clone() {
            RightPanelTarget::Empty => RightPanelTarget::Empty,
            RightPanelTarget::Pane { pane_id }
                if self.tree.get(pane_id).is_some_and(Node::is_pane) =>
            {
                RightPanelTarget::Pane { pane_id }
            }
            RightPanelTarget::SplitView {
                split_id,
                active_pane_id,
            } if self.tree.get(split_id).is_some_and(Node::is_split_view) => {
                let active_pane_id = active_pane_id.filter(|pane_id| {
                    self.tree.parent_of(*pane_id) == Some(split_id)
                        && self.tree.get(*pane_id).is_some_and(Node::is_pane)
                });
                RightPanelTarget::SplitView {
                    split_id,
                    active_pane_id,
                }
            }
            _ => RightPanelTarget::Empty,
        };
    }

    /// Updates the geometry shared by rendering, hit-testing, and pane
    /// sizing, and queues a `ResizePane` request for every terminal pane
    /// whose size actually changed.
    pub fn set_layout(&mut self, layout: UiLayout) {
        if self.layout == layout {
            return;
        }
        self.layout = layout;
        self.resize_displayed_panes();

        for id in self.displayed_pane_ids() {
            let is_rendered_editor = matches!(
                self.panes.get(&id),
                Some(PaneRuntime::Editor(editor)) if editor.view_mode == EditorViewMode::Rendered
            );
            if is_rendered_editor {
                self.rebuild_rendered_markdown(id);
            }
        }
    }

    /// Recomputes geometry after the host terminal changes size while
    /// preserving the animation's current visible width.
    pub fn set_screen_area(&mut self, screen_area: Rect) {
        let layout = UiLayout::from_screen_area_with_tree_width(
            screen_area,
            self.tree_width_animation.current_width(),
        );
        self.set_layout(layout);
    }

    /// Applies `ui` as this session's live settings: threads the change
    /// through to every runtime piece that isn't purely presentational --
    /// the tree-width animation's base width and the active color theme
    /// (`crate::theme::set`) both live outside `self.ui_settings` itself, so
    /// they need to be told explicitly rather than just reading the new
    /// value next frame. Called once at startup (`crate::run`, after
    /// `crate::config::load`) and again by every settings-screen control
    /// that changes a value (see `apply_and_persist_ui_settings`).
    pub fn apply_ui_settings(&mut self, ui: UiSettings) {
        self.ui_settings = ui;
        let now = Instant::now();
        self.tree_width_animation.set_base_width(ui.tree_width, now);
        theme::set(Theme::for_scheme(ui.color_scheme));
        let layout = UiLayout::from_screen_area_with_tree_width(
            self.layout.screen_area,
            self.tree_width_animation.current_width(),
        );
        self.set_layout(layout);
    }

    /// [`apply_ui_settings`] plus a best-effort write-through to
    /// `config.toml` -- what every settings-screen control actually calls.
    /// A failed write (missing config dir, permissions, disk full) reports
    /// a status message but never rolls back the in-memory change: the
    /// setting stays live for the rest of this session either way, matching
    /// every other "config write is a nice-to-have, not a gate" policy in
    /// this crate (see `ConfigLoadError`'s doc comments).
    fn apply_and_persist_ui_settings(&mut self, ui: UiSettings) {
        self.apply_ui_settings(ui);
        if let Some(config_dir) = self.config_dir.clone() {
            if let Err(error) = crate::config::save_ui_settings(&config_dir, &self.ui_settings) {
                self.status_message = Some(format!("Could not save settings: {error}"));
            }
        }
    }

    /// Applies a validated shortcut base immediately and persists it without
    /// disturbing other `config.toml` tables.
    pub fn apply_and_persist_keyboard_settings(&mut self, keyboard: KeyboardSettings) {
        self.keyboard_settings = keyboard;
        if let Some(config_dir) = self.config_dir.clone() {
            if let Err(error) =
                crate::config::save_keyboard_settings(&config_dir, &self.keyboard_settings)
            {
                self.status_message = Some(format!("Could not save keyboard settings: {error}"));
            }
        }
    }

    /// Selects an explicit A-Z shortcut base, used by direct letter entry in
    /// the Keyboard settings tab.
    pub fn settings_set_shortcut_base(&mut self, shortcut_base: crate::keymap::ShortcutBase) {
        self.apply_and_persist_keyboard_settings(KeyboardSettings { shortcut_base });
    }

    /// Cycles the current shortcut base through all allowed letters.
    pub fn settings_adjust_shortcut_base(&mut self, direction: i32) {
        self.settings_set_shortcut_base(self.keyboard_settings.shortcut_base.stepped(direction));
    }

    /// Opens the full-screen settings view. See `Mode::Settings`'s and
    /// `SettingsState`'s doc comments for the UI/UX brief this screen (and
    /// every setting added to it) must keep matching.
    pub fn action_open_settings(&mut self) {
        self.mode = Mode::Settings(SettingsState::new());
    }

    /// Flips the "auto-resize tree panel on focus" toggle -- the Appearance
    /// tab's first row.
    pub fn settings_toggle_auto_resize_tree(&mut self) {
        let mut ui = self.ui_settings;
        ui.auto_resize_tree_on_focus = !ui.auto_resize_tree_on_focus;
        self.apply_and_persist_ui_settings(ui);
    }

    /// Adjusts the tree panel's base width by `delta` columns, clamped to
    /// `[MIN_TREE_WIDTH, MAX_TREE_WIDTH]` -- the Appearance tab's second row.
    pub fn settings_adjust_tree_width(&mut self, delta: i32) {
        let mut ui = self.ui_settings;
        let clamped = (i32::from(ui.tree_width) + delta).clamp(
            i32::from(crate::layout::MIN_TREE_WIDTH),
            i32::from(crate::layout::MAX_TREE_WIDTH),
        );
        ui.tree_width = clamped as u16;
        self.apply_and_persist_ui_settings(ui);
    }

    /// Switches between ilium's two built-in presets -- the Appearance
    /// tab's third row. Just the two for now; see `ColorScheme`'s doc
    /// comment on why a free-form theme picker is out of scope.
    pub fn settings_toggle_color_scheme(&mut self) {
        let mut ui = self.ui_settings;
        ui.color_scheme = match ui.color_scheme {
            ColorScheme::Dark => ColorScheme::Light,
            ColorScheme::Light => ColorScheme::Dark,
        };
        self.apply_and_persist_ui_settings(ui);
    }

    /// Dispatches a keyboard/mouse "adjust this row" gesture to the right
    /// per-row action, per `AppearanceRow`'s doc comment. `direction` is
    /// `-1`/`+1` (decrement/increment); for the two-state rows
    /// (`AutoResizeTree`, `ColorScheme`) either direction just flips the
    /// value -- there's nothing to distinguish between "previous" and
    /// "next" with only two states.
    pub fn settings_adjust_row(&mut self, row: AppearanceRow, direction: i32) {
        match row {
            AppearanceRow::AutoResizeTree => self.settings_toggle_auto_resize_tree(),
            AppearanceRow::TreeWidth => self.settings_adjust_tree_width(direction),
            AppearanceRow::ColorScheme => self.settings_toggle_color_scheme(),
        }
    }

    /// Records the pointer's last reported cell, driving the tree-panel
    /// hover-expand animation in `tick_layout_animation`.
    pub fn set_pointer_position(&mut self, position: Option<Position>) {
        self.pointer_position = position;
    }

    pub fn set_terminal_focused(&mut self, focused: bool) {
        self.is_terminal_focused = focused;
    }

    /// Top-level input entry point, called once per crossterm `Event` from
    /// the event loop (see `crate::run`). Mouse events go straight to
    /// `crate::mouse`; everything else (host focus tracking, then
    /// mode-dispatched key handling) goes to `crate::keys`.
    pub fn handle_event(&mut self, event: crossterm::event::Event) {
        use crossterm::event::Event;
        if let Event::Mouse(mouse) = event {
            crate::mouse::handle_mouse_event(self, mouse);
            return;
        }

        match &event {
            // Host focus is part of the activation contract: an internal
            // tree focus becomes active again on FocusGained, without
            // stale pointer coordinates pretending the mouse never left.
            Event::FocusLost => {
                self.is_terminal_focused = false;
                self.pointer_position = None;
                self.hovered_tree_node = None;
                self.tree_toolbar_hovered = false;
                self.hovered_tree_toolbar_action = None;
            }
            Event::FocusGained => self.is_terminal_focused = true,
            _ => {}
        }

        crate::keys::handle_event(self, event);
    }

    /// Advances the tree width from the two independent activation signals:
    /// pointer-over-panel and keyboard focus.
    pub fn tick_layout_animation(&mut self, now: Instant) {
        let is_pointer_over_tree = self
            .pointer_position
            .is_some_and(|position| self.layout.tree_area.contains(position));
        // Gated on `ui_settings.auto_resize_tree_on_focus`: with the setting
        // off, `is_tree_active` is always false, so `update` only ever
        // targets the base width -- the panel simply never expands, and any
        // expansion already in progress eases back down to it.
        let is_tree_active = self.ui_settings.auto_resize_tree_on_focus
            && self.is_terminal_focused
            && (is_pointer_over_tree || matches!(self.focus, FocusTarget::Tree));
        let tree_width = self.tree_width_animation.update(is_tree_active, now);
        let layout =
            UiLayout::from_screen_area_with_tree_width(self.layout.screen_area, tree_width);
        self.set_layout(layout);
    }

    pub const fn is_layout_animating(&self) -> bool {
        self.tree_width_animation.is_animating()
    }

    /// Whether any wall-clock-driven visual (the tree-width hover
    /// animation, a "Working" spinner, a "Done" bell pulse, a
    /// recently-created flash, or the project-name/pane-title loading
    /// spinner) is currently active -- i.e. whether the next scheduled
    /// tick still needs to force a redraw even though no event actually
    /// changed anything. See `crate::tick::on_tick`, which is the only
    /// caller: everything else that changes visible state (input, a
    /// `ServerEvent`, a finished naming worker) already marks the frame
    /// dirty on its own.
    pub fn has_active_animation(&self) -> bool {
        if self.is_layout_animating() {
            return true;
        }
        if self.is_project_name_loading || !self.titles_loading.is_empty() {
            return true;
        }
        let elapsed_ms = self.started_at.elapsed().as_millis();
        if tree_ui::any_recently_created_within_window(&self.recently_created, elapsed_ms) {
            return true;
        }
        self.tree.panes().any(|node| {
            matches!(
                node.kind,
                NodeKind::Pane {
                    status: PaneStatus::Agent(_, AgentActivity::Working | AgentActivity::Done),
                    ..
                }
            )
        })
    }

    /// Resizes only the terminal panes currently visible in the right panel,
    /// using the exact content rectangle allocated to each slot.
    pub fn resize_displayed_panes(&mut self) {
        let viewports = self.pane_viewports();
        if let Some(active_pane_id) = self.active_pane_id() {
            if let Some(active_viewport) = viewports
                .iter()
                .find(|viewport| viewport.pane_id == active_pane_id)
            {
                self.last_known_pane_size = (
                    active_viewport.content_area.height.max(1),
                    active_viewport.content_area.width.max(1),
                );
            }
        }
        for viewport in viewports {
            let pane_id = viewport.pane_id;
            let rows = viewport.content_area.height.max(1);
            let cols = viewport.content_area.width.max(1);
            if let Some(PaneRuntime::Terminal(view)) = self.panes.get_mut(&pane_id) {
                // Skip panes already at this size: without this guard every
                // tree-width animation tick re-sends a `ResizePane` (and the
                // server relays a real PTY resize/SIGWINCH) for every
                // terminal pane, not just ones whose size actually changed.
                if view.with_screen(|screen| screen.size()) == (rows, cols) {
                    continue;
                }
                view.resize(rows, cols);
                self.queue_request(ClientRequest::ResizePane {
                    pane_id,
                    rows,
                    cols,
                });
            }
        }
    }

    /// Rebuilds a focused editor pane's Rendered-mode document at the
    /// current layout width. A no-op for anything but an editor pane
    /// actually in `EditorViewMode::Rendered`.
    pub fn rebuild_rendered_markdown(&mut self, id: NodeId) {
        let width = self
            .pane_viewport(id)
            .map(|viewport| viewport.content_area.width)
            .unwrap_or(self.layout.pane_content_area.width);
        let Some(PaneRuntime::Editor(editor)) = self.panes.get_mut(&id) else {
            return;
        };
        if editor.view_mode != EditorViewMode::Rendered || !editor.is_markdown() {
            return;
        }
        let Some(path) = editor.path.clone() else {
            return;
        };
        let source = editor.textarea.lines().join("\n");
        let base_dir = path.parent().unwrap_or(&self.session_cwd).to_path_buf();
        let document = crate::markdown::document::parse(&source, &base_dir);
        editor.rendered = Some(crate::markdown::render::render(
            &document,
            &self.markdown_picker,
            &mut self.markdown_rasterizer,
            width,
        ));
        editor.rendered_width = width;
    }

    /// Clears a `Done` (finished, unseen) agent pane back to `Idle` the
    /// moment the user actually looks at it. The tree itself is
    /// server-owned, so this only updates the render-cache locally --
    /// the server's own detection tick will reach the same conclusion (a
    /// focused pane a user is actively looking at doesn't stay `Done`)
    /// and broadcast the authoritative `PaneStatusChanged` in time.
    fn mark_seen(&mut self, id: NodeId) {
        if let Some(NodeKind::Pane {
            status: PaneStatus::Agent(class, ilium_core::AgentActivity::Done),
            ..
        }) = self.tree.get(id).map(|node| &node.kind)
        {
            let class = class.clone();
            let _ = self.tree.set_pane_status(
                id,
                PaneStatus::Agent(class, ilium_core::AgentActivity::Idle),
            );
        }
    }

    /// Focuses `id` (a pane) both in the tree selection and as the
    /// right-panel content, clearing a stale `Done` flag if this is the
    /// first look since it finished. Notifies the server of the focus
    /// transition (`ClientRequest::SetPaneFocus`) so its adaptive
    /// detection schedule can pin `id` to the fastest poll tier and force
    /// an immediate recheck -- see `ilium-server::detection` -- covering
    /// both "entered pane focus from the tree" and "switched directly from
    /// one pane to another" (the old pane never passes through
    /// `leave_pane_focus` in that second case, so the exit notice has to
    /// happen here instead).
    pub fn focus_pane(&mut self, id: NodeId) {
        let previously_focused_pane = matches!(self.focus, FocusTarget::Pane)
            .then(|| self.active_pane_id())
            .flatten();
        if previously_focused_pane != Some(id) {
            if let Some(old_id) = previously_focused_pane {
                self.queue_request(ClientRequest::SetPaneFocus {
                    pane_id: old_id,
                    focused: false,
                });
            }
            self.queue_request(ClientRequest::SetPaneFocus {
                pane_id: id,
                focused: true,
            });
        }
        self.select_node(id);
        self.right_panel_target = match self.tree.parent_of(id) {
            Some(parent) if self.tree.get(parent).is_some_and(Node::is_split_view) => {
                RightPanelTarget::SplitView {
                    split_id: parent,
                    active_pane_id: Some(id),
                }
            }
            _ => RightPanelTarget::Pane { pane_id: id },
        };
        self.focus = FocusTarget::Pane;
        self.mark_seen(id);
        self.resize_displayed_panes();
    }

    pub fn show_split_view(&mut self, split_id: NodeId) {
        if matches!(self.focus, FocusTarget::Pane) {
            if let Some(active_pane_id) = self.active_pane_id() {
                self.queue_request(ClientRequest::SetPaneFocus {
                    pane_id: active_pane_id,
                    focused: false,
                });
            }
        }
        self.select_node(split_id);
        self.right_panel_target = RightPanelTarget::SplitView {
            split_id,
            active_pane_id: None,
        };
        self.focus = FocusTarget::Tree;
        if self.displayed_pane_ids().is_empty() {
            self.status_message = Some("Split view is empty; add up to four panes".to_string());
        }
        self.resize_displayed_panes();
    }

    /// Leaves pane focus for the tree panel, notifying the server
    /// (`ClientRequest::SetPaneFocus { focused: false }`) that the
    /// previously-focused pane, if any, is no longer the client's active
    /// view. Guards on the *old* focus target before overwriting it, so
    /// this stays a no-op beyond the plain flip when called repeatedly
    /// while already tree-focused (e.g. `handle_tree_mouse` calls this on
    /// every hover, not just the first) -- only the true
    /// pane-focus-to-tree-focus transition edge notifies the server.
    pub(crate) fn leave_pane_focus(&mut self) {
        if matches!(self.focus, FocusTarget::Pane) {
            if let Some(id) = self.active_pane_id() {
                self.queue_request(ClientRequest::SetPaneFocus {
                    pane_id: id,
                    focused: false,
                });
            }
        }
        self.focus = FocusTarget::Tree;
    }

    /// The full identifier path (top-level down to `id`) as
    /// `tui_tree_widget::TreeState` expects it for `select`/`open`.
    fn path_to(&self, id: NodeId) -> Vec<NodeId> {
        let mut path = vec![id];
        let mut current = id;
        while let Some(parent) = self.tree.parent_of(current) {
            if parent == ROOT_ID {
                break;
            }
            path.push(parent);
            current = parent;
        }
        path.reverse();
        path
    }

    /// Selects `id` in the tree widget's state (does not change focus),
    /// opening every ancestor so the selection is actually visible.
    pub(crate) fn select_node(&mut self, id: NodeId) {
        let path = self.path_to(id);
        for depth in 1..path.len() {
            self.tree_state.open(path[..depth].to_vec());
        }
        self.tree_state.select(path);
    }

    pub(crate) fn selected_node_id(&self) -> Option<NodeId> {
        self.tree_state.selected().last().copied()
    }

    /// Applies persisted group expansion to the widget state after a server
    /// snapshot replaces the local tree. The domain tree owns the persisted
    /// preference; `TreeState` owns the widget's visible-row bookkeeping.
    ///
    /// Also prunes `tree_state`'s `opened` set of any path that no longer
    /// matches the tree. `tui_tree_widget::TreeState` keys `opened` by the
    /// *whole* ancestor path (`HashSet<Vec<NodeId>>`) and exposes no way to
    /// expire an entry on its own; since `NodeId` is never reused, every
    /// group this client ever expanded -- including ones since
    /// closed/removed, or reparented to a different ancestor chain --
    /// would otherwise stay in `opened` forever (a removed node's id
    /// resolves to nothing; a merely-moved node's old path is simply never
    /// the current one), growing by one entry per such change for the life
    /// of the client process. This runs on every tree snapshot (the same
    /// reconciliation point `render_cache::apply_tree_snapshot` uses to
    /// prune every other pane-keyed cache), so a stale path never survives
    /// past the structural change that orphaned it.
    pub(crate) fn restore_expanded_groups(&mut self) {
        // Collected into an owned `Vec` first (rather than filtering the
        // borrowed `opened()` set directly) so the read of `self.tree_state`
        // is fully finished before `self.tree`/`self.path_to` are read and
        // `self.tree_state.close` is called below.
        let opened_paths: Vec<Vec<NodeId>> = self.tree_state.opened().iter().cloned().collect();
        let stale_paths: Vec<Vec<NodeId>> = opened_paths
            .into_iter()
            .filter(|path| match path.last() {
                None => true,
                Some(leaf) => self.tree.get(*leaf).is_none() || self.path_to(*leaf) != *path,
            })
            .collect();
        for path in stale_paths {
            self.tree_state.close(&path);
        }

        let expanded_group_ids: Vec<NodeId> = self
            .tree
            .all_ids()
            .filter(|id| {
                matches!(
                    self.tree.get(*id).map(|node| &node.kind),
                    Some(NodeKind::Container(container)) if container.expanded && *id != ROOT_ID
                )
            })
            .collect();
        for group_id in expanded_group_ids {
            self.tree_state.open(self.path_to(group_id));
        }
    }

    /// The group a newly created node should be added under: an explicitly
    /// selected group (or the parent of an explicitly selected pane) takes
    /// priority, falling back to the focused pane's group, then
    /// `ROOT_ID` -- which server-side `resolve_parent_group` treats as "no
    /// specific target, use (or create) the session's default group".
    ///
    /// That fallback must be resolved server-side, not here: this client
    /// only ever holds a *mirror* of the tree, so calling
    /// `Tree::ensure_default_group` on it directly would invent a
    /// client-local `NodeId` the server's own tree has no matching node
    /// for yet (most visibly on a brand new session with no groups at
    /// all), and the `NewPane` request would fail with "node not found"
    /// instead of ever creating anything.
    pub(crate) fn group_for_new_node(&mut self) -> NodeId {
        let selected_target = self.selected_node_id().and_then(|id| {
            if id == ROOT_ID {
                return None;
            }
            match self.tree.get(id).map(|node| &node.kind) {
                Some(NodeKind::Container(container)) if container.is_group() => Some(id),
                Some(NodeKind::Container(_)) => self.tree.parent_of(id),
                Some(NodeKind::Pane { .. }) => self.tree.parent_of(id),
                Some(NodeKind::Folder { .. }) => self.tree.parent_of(id),
                None => None,
            }
        });
        let visible_pane_target = self.active_pane_id().and_then(|id| self.tree.parent_of(id));
        selected_target.or(visible_pane_target).unwrap_or(ROOT_ID)
    }

    /// Human-readable description of what closing `target` would lose,
    /// or `None` when it can be closed without confirmation (an empty
    /// group, a plain shell, or a clean/unsaved-nothing editor).
    pub fn close_confirmation_message(&self, target: NodeId) -> Option<String> {
        let node = self.tree.get(target)?;
        match &node.kind {
            NodeKind::Container(container) if !container.children.is_empty() => Some(format!(
                "\"{}\" contains {} item(s). Close it and everything inside?",
                node.name,
                container.children.len()
            )),
            NodeKind::Pane {
                content: PaneContentKind::Editor,
                ..
            } => {
                let is_dirty = matches!(
                    self.panes.get(&target),
                    Some(PaneRuntime::Editor(editor)) if editor.dirty
                );
                is_dirty.then(|| format!("\"{}\" has unsaved changes. Close anyway?", node.name))
            }
            _ => None,
        }
    }

    /// Queues a `NewPane` request for a plain shell under `parent_group`
    /// (the caller resolves that group -- see `group_for_new_node`).
    pub fn request_new_terminal(&mut self, parent_group: NodeId) {
        self.queue_request(ClientRequest::NewPane {
            parent_group,
            kind: ilium_ipc::NewPaneKind::PlainShell,
        });
    }

    /// Queues a `NewPane` request for a specific command line (e.g.
    /// `claude`, `codex`) under `parent_group`.
    pub fn request_new_command_pane(&mut self, parent_group: NodeId, command_line: String) {
        self.queue_request(ClientRequest::NewPane {
            parent_group,
            kind: ilium_ipc::NewPaneKind::Command(command_line),
        });
    }

    /// Queues a `NewPane` request for the file picked in `Mode::Explorer`,
    /// and remembers it in `pending_editor_opens` so this client can load
    /// its own content locally once the server confirms the new node.
    pub fn request_new_editor(&mut self, parent_group: NodeId, path: PathBuf) {
        let basename = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        // Bounded, oldest-first eviction: an entry only survives this long
        // if its `NewPane` request was never confirmed (see
        // `pending_editor_opens`'s doc comment), so the oldest entry is the
        // best candidate to drop when the queue is full.
        if self.pending_editor_opens.len() >= MAX_PENDING_EDITOR_OPENS {
            self.pending_editor_opens.remove(0);
        }
        self.pending_editor_opens.push((basename, path.clone()));
        self.queue_request(ClientRequest::NewPane {
            parent_group,
            kind: ilium_ipc::NewPaneKind::Editor(path),
        });
    }

    /// Queues a `ClosePane` request. The tree/pane-runtime removal itself
    /// only happens once the server confirms it via the next `TreeSnapshot`.
    pub fn request_close(&mut self, target: NodeId) {
        self.queue_request(ClientRequest::ClosePane { pane_id: target });
    }

    pub fn request_move(&mut self, node_id: NodeId, direction: ilium_core::TreeMoveDirection) {
        self.queue_request(ClientRequest::MoveNode { node_id, direction });
    }

    /// Queues a `ReparentNode` request -- an arbitrary move to `new_parent`
    /// at `index` (`None` appends at the end), backing both mouse
    /// drag-and-drop (`crate::mouse`) and the leader/move-mode indent/outdent
    /// keybindings (`crate::keys`). The tree itself only changes once the
    /// server confirms it via the next `TreeSnapshot`, same as every other
    /// structural request.
    pub fn request_reparent(&mut self, node_id: NodeId, new_parent: NodeId, index: Option<usize>) {
        self.queue_request(ClientRequest::ReparentNode {
            node_id,
            new_parent,
            index,
        });
    }

    /// `short_title` is the short-form alternative shown when the tree
    /// panel is narrow (see `crate::tree_ui`) -- `None` when this rename's
    /// source has no distinct short form (e.g. the user typed `title`
    /// directly into the rename dialog).
    pub fn request_rename(&mut self, node_id: NodeId, title: String, short_title: Option<String>) {
        self.queue_request(ClientRequest::RenameNode {
            node_id,
            title,
            short_title,
        });
    }

    /// Queues a title proposed by a background automatic source (e.g.
    /// `crate::title_inference`'s LLM session-title inference). Unlike
    /// `request_rename`, the server applies it only while the pane hasn't
    /// been genuinely user-renamed, and it never marks the pane
    /// user-specified -- see `ilium_ipc::ClientRequest::SetAutomaticPaneTitle`.
    /// `short_title` mirrors `request_rename`'s field.
    pub fn request_automatic_pane_title(
        &mut self,
        pane_id: NodeId,
        title: String,
        short_title: Option<String>,
    ) {
        self.queue_request(ClientRequest::SetAutomaticPaneTitle {
            pane_id,
            title,
            short_title,
        });
    }

    /// Queues a `NewGroup` request. `name` defaults to `"group"` when
    /// blank, matching the create-group dialog's placeholder text.
    pub fn request_new_group(&mut self, parent_group: NodeId, name: String) {
        let name = if name.trim().is_empty() {
            "group".to_string()
        } else {
            name
        };
        self.queue_request(ClientRequest::NewGroup { parent_group, name });
    }

    pub fn request_new_folder(&mut self, parent_group: NodeId, path: PathBuf) {
        self.queue_request(ClientRequest::NewFolder { parent_group, path });
    }

    /// Adds a board pane backed by an existing Markdown file. The file is
    /// not modified while creating the pane; the board adapter reads the
    /// existing headings/bullets on the next tree snapshot.
    pub fn request_new_markdown_board(&mut self, parent_group: NodeId, path: PathBuf) {
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Board".to_string());
        self.queue_request(ClientRequest::NewBoard {
            parent_group,
            name,
            storage: ilium_core::BoardStorage::MarkdownFile { path },
        });
    }

    pub fn open_explorer_file_menu(
        &mut self,
        overlay: Box<ExplorerOverlay>,
        target_group: NodeId,
        file_path: PathBuf,
        position: Position,
    ) {
        let menu_width = 34;
        let menu_height = 3;
        let screen = self.layout.screen_area;
        let x = position.x.min(screen.right().saturating_sub(menu_width));
        let y = position.y.min(screen.bottom().saturating_sub(menu_height));
        self.mode = Mode::ExplorerFileMenu(ExplorerFileMenu {
            overlay,
            target_group,
            file_path,
            area: Rect::new(
                x,
                y,
                menu_width.min(screen.width),
                menu_height.min(screen.height),
            ),
        });
    }

    pub fn open_create_board_dialog(&mut self) {
        let default_path = self
            .session_cwd
            .join(".ilium")
            .join("boards")
            .join("board.md");
        self.mode = Mode::CreateBoard(CreateBoardState {
            name: TextPromptState::new("Board"),
            path: TextPromptState::new(default_path.display().to_string()),
            storage_kind: BoardStorageKind::MarkdownFile,
            editing_path: false,
        });
    }

    pub fn commit_create_board(&mut self, state: &CreateBoardState) {
        use ilium_core::BoardStorage;
        let path = PathBuf::from(state.path.buf.trim());
        if path.as_os_str().is_empty() {
            self.status_message = Some("Board storage path is required".to_string());
            return;
        }
        let storage = match state.storage_kind {
            BoardStorageKind::Folder => BoardStorage::Folder { path },
            BoardStorageKind::MarkdownFile => BoardStorage::MarkdownFile { path },
        };
        let name = if state.name.buf.trim().is_empty() {
            "Board".to_string()
        } else {
            state.name.buf.trim().to_string()
        };
        let parent_group = self.group_for_new_node();
        self.queue_request(ClientRequest::NewBoard {
            parent_group,
            name,
            storage,
        });
        self.mode = Mode::Normal;
    }

    pub fn open_board_path_picker(&mut self, state: CreateBoardState) {
        let picker = match state.storage_kind {
            BoardStorageKind::Folder => ExplorerOverlay::open_folder_at(&self.session_cwd),
            BoardStorageKind::MarkdownFile => ExplorerOverlay::open_at(&self.session_cwd),
        };
        match picker {
            Ok(picker) => self.mode = Mode::BoardPathPicker(Box::new(picker), state),
            Err(error) => {
                self.status_message = Some(format!("Could not open board path picker: {error}"));
                self.mode = Mode::CreateBoard(state);
            }
        }
    }

    pub fn action_add_board_card(&mut self, pane_id: NodeId) {
        self.mode = Mode::BoardCardPrompt(pane_id, TextPromptState::new(""));
    }

    pub fn commit_board_card(&mut self, pane_id: NodeId, title: String) {
        let Some(PaneRuntime::Board(board)) = self.panes.get_mut(&pane_id) else {
            return;
        };
        match board.add_card(title) {
            Ok(()) => self.status_message = Some("Card saved".to_string()),
            Err(error) => self.status_message = Some(error),
        }
    }

    pub fn action_add_board_column(&mut self, pane_id: NodeId) {
        self.mode = Mode::BoardColumnPrompt(pane_id, TextPromptState::new(""));
    }

    pub fn commit_board_column(&mut self, pane_id: NodeId, title: String) {
        let Some(PaneRuntime::Board(board)) = self.panes.get_mut(&pane_id) else {
            return;
        };
        match board.add_column(title) {
            Ok(()) => self.status_message = Some("Column saved".to_string()),
            Err(error) => self.status_message = Some(error),
        }
    }

    pub fn action_rename_board_selection(&mut self, pane_id: NodeId) {
        let Some(PaneRuntime::Board(board)) = self.panes.get(&pane_id) else {
            return;
        };
        let (target, title) = match board
            .columns
            .get(board.selected_column)
            .and_then(|column| column.cards.get(board.selected_card))
        {
            Some(card) => (BoardRenameTarget::Card, card.title.clone()),
            None => match board.columns.get(board.selected_column) {
                Some(column) => (BoardRenameTarget::Column, column.title.clone()),
                None => return,
            },
        };
        self.mode = Mode::BoardRenamePrompt(pane_id, target, TextPromptState::new(title));
    }

    pub fn commit_board_rename(
        &mut self,
        pane_id: NodeId,
        target: BoardRenameTarget,
        title: String,
    ) {
        let Some(PaneRuntime::Board(board)) = self.panes.get_mut(&pane_id) else {
            return;
        };
        let result = match target {
            BoardRenameTarget::Card => board.rename_selected_card(title),
            BoardRenameTarget::Column => board.rename_selected_column(title),
        };
        if let Err(error) = result {
            self.status_message = Some(error);
        }
    }

    pub fn action_delete_board_selection(&mut self, pane_id: NodeId) {
        let Some(PaneRuntime::Board(board)) = self.panes.get(&pane_id) else {
            return;
        };
        let target = if board
            .columns
            .get(board.selected_column)
            .is_some_and(|column| board.selected_card < column.cards.len())
        {
            BoardDeleteTarget::Card
        } else {
            BoardDeleteTarget::Column
        };
        self.mode = Mode::BoardDeleteConfirm(pane_id, target);
    }

    pub fn commit_board_delete(&mut self, pane_id: NodeId, target: BoardDeleteTarget) {
        let Some(PaneRuntime::Board(board)) = self.panes.get_mut(&pane_id) else {
            return;
        };
        let result = match target {
            BoardDeleteTarget::Card => board.delete_selected_card(),
            BoardDeleteTarget::Column => board.delete_selected_column(),
        };
        if let Err(error) = result {
            self.status_message = Some(error);
        }
    }

    /// Builds the "New group" destination-picker state, snapshotting the
    /// tree's current group listing, with `preselected` (a group id, or
    /// `ROOT_ID` for the top level) highlighted. Falls back to index `0`
    /// if `preselected` isn't a group (e.g. a stale id) -- the dialog
    /// always has at least the top-level entry.
    pub fn open_create_group_dialog(&mut self, preselected: NodeId) {
        let destinations = self.tree.list_groups();
        let selected_index = destinations
            .iter()
            .position(|destination| destination.id == preselected)
            .unwrap_or(0);
        let area =
            crate::modal::create_group_dialog_area(self.layout.screen_area, destinations.len());
        self.mode = Mode::CreateGroup(CreateGroupState {
            area,
            destinations,
            selected_index,
            name: TextPromptState::new(""),
        });
    }

    /// Destination to preselect when "New group" is triggered without a
    /// specific click target (leader key, toolbar button): the explicitly
    /// selected group (or the parent of a selected pane) if there is one,
    /// otherwise the group of whatever pane is currently focused, otherwise
    /// the top level. Deliberately read-only (unlike `group_for_new_node`,
    /// which this mirrors) -- merely opening a picker must never create a
    /// default group as a side effect.
    pub fn create_group_preselect_target(&self) -> NodeId {
        let selected_target = self.selected_node_id().and_then(|id| {
            if id == ROOT_ID {
                return None;
            }
            match self.tree.get(id).map(|node| &node.kind) {
                Some(NodeKind::Container(container)) if container.is_group() => Some(id),
                Some(NodeKind::Container(_)) => self.tree.parent_of(id),
                Some(NodeKind::Pane { .. }) => self.tree.parent_of(id),
                Some(NodeKind::Folder { .. }) => self.tree.parent_of(id),
                None => None,
            }
        });
        let visible_pane_target = self.active_pane_id().and_then(|id| self.tree.parent_of(id));
        selected_target.or(visible_pane_target).unwrap_or(ROOT_ID)
    }

    /// Destination to preselect when "New group…" is triggered from a
    /// right-click on a specific tree node.
    pub fn create_group_target_for_click(&self, target: NodeId) -> NodeId {
        if target == ROOT_ID {
            return ROOT_ID;
        }
        match self.tree.get(target).map(|node| &node.kind) {
            Some(NodeKind::Container(container)) if container.is_group() => target,
            Some(NodeKind::Container(_)) => self.tree.parent_of(target).unwrap_or(ROOT_ID),
            Some(NodeKind::Pane { .. }) => self.tree.parent_of(target).unwrap_or(ROOT_ID),
            Some(NodeKind::Folder { .. }) => self.tree.parent_of(target).unwrap_or(ROOT_ID),
            None => ROOT_ID,
        }
    }

    /// Queues a `NewGroup` request for the destination currently selected
    /// in `state`, named from its text field, and returns to `Mode::Normal`.
    pub fn commit_create_group(&mut self, state: &CreateGroupState) {
        self.mode = Mode::Normal;
        let Some(destination) = state.destinations.get(state.selected_index) else {
            return;
        };
        self.request_new_group(destination.id, state.name.buf.clone());
    }

    pub fn open_create_split_dialog(&mut self) {
        self.mode = Mode::CreateSplitOrientation(CreateSplitOrientationState {
            orientation: SplitOrientation::Vertical,
        });
    }

    fn normal_group_for_node(&self, node_id: NodeId) -> Option<NodeId> {
        let node = self.tree.get(node_id)?;
        if node.is_group() {
            return Some(node_id);
        }
        let parent = node.parent?;
        if self.tree.get(parent).is_some_and(Node::is_group) {
            return Some(parent);
        }
        self.tree.parent_of(parent)
    }

    fn split_parent_group(&self) -> NodeId {
        self.selected_node_id()
            .and_then(|node_id| self.normal_group_for_node(node_id))
            .or_else(|| {
                self.active_pane_id()
                    .and_then(|pane_id| self.normal_group_for_node(pane_id))
            })
            .or_else(|| {
                self.tree.children_of(ROOT_ID).ok().and_then(|children| {
                    children
                        .iter()
                        .copied()
                        .find(|node_id| self.tree.get(*node_id).is_some_and(Node::is_group))
                })
            })
            .unwrap_or(ROOT_ID)
    }

    fn pane_tree_path_label(&self, pane_id: NodeId) -> String {
        let mut names = Vec::new();
        let mut current = Some(pane_id);
        while let Some(node_id) = current {
            let Some(node) = self.tree.get(node_id) else {
                break;
            };
            if node_id != ROOT_ID {
                names.push(node.name.clone());
            }
            current = node.parent;
        }
        names.reverse();
        names.join(" / ")
    }

    pub fn continue_create_split(&mut self, orientation: SplitOrientation) {
        let choices = self
            .tree
            .pane_ids_in_tree_order()
            .into_iter()
            .filter(|pane_id| {
                self.tree
                    .parent_of(*pane_id)
                    .and_then(|parent| self.tree.get(parent))
                    .is_none_or(|parent| !parent.is_split_view())
            })
            .map(|pane_id| SplitPaneChoice {
                pane_id,
                label: self.pane_tree_path_label(pane_id),
                selected: false,
            })
            .collect();
        self.mode = Mode::CreateSplitMembers(CreateSplitMembersState {
            parent_group: self.split_parent_group(),
            orientation,
            choices,
            selected_index: 0,
        });
    }

    pub fn toggle_create_split_member(&mut self, state: &mut CreateSplitMembersState) {
        let selected_count = state
            .choices
            .iter()
            .filter(|choice| choice.selected)
            .count();
        let Some(choice) = state.choices.get_mut(state.selected_index) else {
            return;
        };
        if !choice.selected && selected_count >= ilium_core::MAXIMUM_SPLIT_VIEW_PANES {
            self.status_message = Some("A split view can contain at most four panes".to_string());
            return;
        }
        choice.selected = !choice.selected;
    }

    pub fn commit_create_split(&mut self, state: CreateSplitMembersState) {
        let pane_ids = state
            .choices
            .into_iter()
            .filter(|choice| choice.selected)
            .map(|choice| choice.pane_id)
            .collect();
        let name = match state.orientation {
            SplitOrientation::Vertical => "Vertical split",
            SplitOrientation::Horizontal => "Horizontal split",
        };
        self.queue_request(ClientRequest::CreateSplitView {
            parent_group: state.parent_group,
            name: name.to_string(),
            orientation: state.orientation,
            pane_ids,
        });
        self.mode = Mode::Normal;
    }

    /// Builds the right-click context menu for `target`, anchored at the
    /// mouse position `(column, row)` -- sized to fit its action list and
    /// clamped inside the screen.
    pub fn open_context_menu(&mut self, target: NodeId, column: u16, row: u16) {
        let actions = self.context_actions_for(target);
        if actions.is_empty() {
            return;
        }
        let width = 28.min(self.layout.screen_area.width.max(1));
        let height = (actions.len() as u16 + 2).min(self.layout.screen_area.height.max(1));
        let max_x = self.layout.screen_area.right().saturating_sub(width);
        let max_y = self.layout.screen_area.bottom().saturating_sub(height);
        let area = Rect::new(column.min(max_x), row.min(max_y), width, height);
        self.mode = Mode::ContextMenu(ContextMenu {
            target,
            area,
            actions,
            selected_index: 0,
        });
    }

    /// The node-appropriate command set for a context menu. `ROOT_ID`
    /// means the click landed on empty space below the tree entries
    /// rather than on a real node -- only the creation actions (plus
    /// `Settings`, which applies everywhere) apply there, none of the
    /// per-node ones.
    fn context_actions_for(&self, target: NodeId) -> Vec<ContextMenuAction> {
        if target == ROOT_ID {
            return vec![
                ContextMenuAction::NewTerminal,
                ContextMenuAction::NewEditor,
                ContextMenuAction::NewGroup,
                ContextMenuAction::NewSplitView,
                ContextMenuAction::NewFolder,
                ContextMenuAction::Settings,
            ];
        }
        let mut actions = vec![
            ContextMenuAction::NewTerminal,
            ContextMenuAction::NewEditor,
            ContextMenuAction::NewGroup,
            ContextMenuAction::NewSplitView,
            ContextMenuAction::NewFolder,
        ];
        match self.tree.get(target) {
            Some(node) if node.is_split_view() => {
                actions.retain(|action| {
                    !matches!(
                        action,
                        ContextMenuAction::NewGroup | ContextMenuAction::NewFolder
                    )
                });
                actions.insert(0, ContextMenuAction::ShowSplitView);
            }
            Some(node) if node.is_group() => actions.insert(0, ContextMenuAction::ToggleGroup),
            Some(node) if node.is_pane() => actions.insert(0, ContextMenuAction::FocusPane),
            Some(Node {
                kind: NodeKind::Folder { .. },
                ..
            }) => actions.insert(0, ContextMenuAction::ToggleGroup),
            Some(_) => return vec![ContextMenuAction::Settings],
            // A stale/unrecognized target (e.g. a race with a concurrent
            // structural change) still gets a menu -- just the one action
            // that never depends on the target actually existing.
            None => return vec![ContextMenuAction::Settings],
        }
        actions.extend([
            ContextMenuAction::Rename,
            ContextMenuAction::MoveUp,
            ContextMenuAction::MoveDown,
            ContextMenuAction::Close,
            ContextMenuAction::Settings,
        ]);
        actions
    }

    /// Executes one context-menu command, then leaves the popup unless the
    /// action opens an explicit sub-mode (e.g. Rename, the file picker).
    pub fn execute_context_action(&mut self, action: ContextMenuAction, target: NodeId) {
        self.mode = Mode::Normal;
        match action {
            ContextMenuAction::FocusPane => self.focus_pane(target),
            ContextMenuAction::ShowSplitView => self.show_split_view(target),
            ContextMenuAction::ToggleGroup => {
                self.tree_state.toggle_selected();
            }
            ContextMenuAction::NewTerminal => self.action_new_terminal(),
            ContextMenuAction::NewEditor => self.action_new_editor(),
            ContextMenuAction::NewGroup => {
                let preselected = self.create_group_target_for_click(target);
                self.open_create_group_dialog(preselected);
            }
            ContextMenuAction::NewSplitView => self.open_create_split_dialog(),
            ContextMenuAction::NewFolder => self.action_new_folder(),
            ContextMenuAction::Rename => self.action_start_rename(),
            ContextMenuAction::MoveUp => {
                self.request_move(target, ilium_core::TreeMoveDirection::Up)
            }
            ContextMenuAction::MoveDown => {
                self.request_move(target, ilium_core::TreeMoveDirection::Down)
            }
            ContextMenuAction::Close => self.action_close(target),
            ContextMenuAction::Settings => self.action_open_settings(),
        }
    }

    /// Creates a plain shell pane under the currently targeted group and
    /// focuses the create-group/normal dialog back to `Normal`.
    pub fn action_new_terminal(&mut self) {
        let parent = self.group_for_new_node();
        self.request_new_terminal(parent);
    }

    /// Creates a specific command-line pane (e.g. `claude`, `codex`) under
    /// the currently targeted group.
    pub fn action_new_command_pane(&mut self, command_line: impl Into<String>) {
        let parent = self.group_for_new_node();
        self.request_new_command_pane(parent, command_line.into());
    }

    /// Opens the file-picker overlay rooted at the session's cwd; the
    /// picked file (if any) becomes a `NewPane` request under the
    /// currently targeted group -- see `Mode::Explorer`'s doc comment for
    /// why there is no placeholder tree node anymore.
    pub fn action_new_editor(&mut self) {
        let parent = self.group_for_new_node();
        match ExplorerOverlay::open_at(&self.session_cwd) {
            Ok(overlay) => self.mode = Mode::Explorer(Box::new(overlay), parent),
            Err(err) => self.status_message = Some(format!("Could not open file picker: {err}")),
        }
    }

    /// Adds a sidebar folder root selected from a directory-only picker.
    pub fn action_new_folder(&mut self) {
        let parent = self.split_parent_group();
        match ExplorerOverlay::open_folder_at(&self.session_cwd) {
            Ok(overlay) => self.mode = Mode::FolderExplorer(Box::new(overlay), parent),
            Err(err) => self.status_message = Some(format!("Could not open folder picker: {err}")),
        }
    }

    /// Entry point for closing the selected node (leader `x`, right-click
    /// Close): closes immediately when there's nothing to lose, otherwise
    /// opens `Mode::ConfirmClose` and waits for the user's answer.
    pub fn action_close_selected(&mut self) {
        let Some(id) = self.selected_node_id() else {
            return;
        };
        self.action_close(id)
    }

    fn action_close(&mut self, id: NodeId) {
        if id == ROOT_ID {
            self.status_message = Some("Cannot close the root group".to_string());
            return;
        }
        match self.close_confirmation_message(id) {
            Some(_) => self.mode = Mode::ConfirmClose(id),
            None => self.request_close(id),
        }
    }

    pub fn action_start_rename(&mut self) {
        let Some(id) = self.selected_node_id() else {
            return;
        };
        let current_name = self
            .tree
            .get(id)
            .map(|node| node.name.clone())
            .unwrap_or_default();
        self.mode = Mode::Rename(TextPromptState::new(current_name));
    }

    pub fn action_save_focused_editor(&mut self) {
        let Some(id) = self.active_pane_id() else {
            self.status_message = Some("No pane focused".to_string());
            return;
        };
        let Some(PaneRuntime::Editor(editor)) = self.panes.get_mut(&id) else {
            self.status_message = Some("Focused pane is not an editor".to_string());
            return;
        };
        if editor.path.is_none() {
            self.action_start_save_as(id);
            return;
        }
        match editor.save() {
            Ok(()) => self.status_message = Some("Saved".to_string()),
            Err(err) => self.status_message = Some(format!("Save failed: {err}")),
        }
    }

    /// Opens the "Save As" filename prompt for `id`'s editor pane,
    /// pre-filled with its current path (or empty for a pane that was
    /// never saved).
    pub fn action_start_save_as(&mut self, id: NodeId) {
        let Some(PaneRuntime::Editor(editor)) = self.panes.get(&id) else {
            return;
        };
        let current_path = editor
            .path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        self.mode = Mode::SaveAs(id, TextPromptState::new(current_path));
    }

    /// Retargets `id`'s editor pane to `new_path` and writes the buffer
    /// there, then queues a `RenameNode` so the sidebar and pane title
    /// reflect the new file name once the server confirms it.
    pub fn action_save_as(&mut self, id: NodeId, new_path: String) {
        if new_path.trim().is_empty() {
            self.status_message = Some("Save As: no filename given".to_string());
            return;
        }
        let path = PathBuf::from(new_path);
        let path = if path.is_absolute() {
            path
        } else {
            self.session_cwd.join(path)
        };

        let Some(PaneRuntime::Editor(editor)) = self.panes.get_mut(&id) else {
            return;
        };
        match editor.save_to(&path) {
            Ok(()) => {
                editor.path = Some(path.clone());
                if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                    self.request_rename(id, name.to_string(), None);
                }
                self.status_message = Some("Saved".to_string());
            }
            Err(err) => self.status_message = Some(format!("Save failed: {err}")),
        }
    }

    pub fn action_toggle_editor_view_mode(&mut self) {
        let Some(id) = self.active_pane_id() else {
            return;
        };
        let transitioned_to_rendered = match self.panes.get_mut(&id) {
            Some(PaneRuntime::Editor(editor)) => {
                let was_source = editor.view_mode == EditorViewMode::Source;
                editor.toggle_view_mode();
                was_source && editor.view_mode == EditorViewMode::Rendered
            }
            _ => false,
        };
        if transitioned_to_rendered {
            self.rebuild_rendered_markdown(id);
        }
    }

    pub fn action_toggle_editor_line_numbers(&mut self) {
        let Some(id) = self.active_pane_id() else {
            return;
        };
        if let Some(PaneRuntime::Editor(editor)) = self.panes.get_mut(&id) {
            editor.toggle_line_numbers();
        }
    }

    pub fn action_toggle_editor_minimap(&mut self) {
        let Some(id) = self.active_pane_id() else {
            return;
        };
        let should_rebuild = if let Some(PaneRuntime::Editor(editor)) = self.panes.get_mut(&id) {
            editor.toggle_minimap();
            editor.view_mode == EditorViewMode::Rendered
        } else {
            false
        };
        if should_rebuild {
            self.rebuild_rendered_markdown(id);
        }
    }

    pub fn action_toggle_editor_autosave(&mut self) {
        let Some(id) = self.active_pane_id() else {
            return;
        };
        if let Some(PaneRuntime::Editor(editor)) = self.panes.get_mut(&id) {
            editor.toggle_autosave();
        }
    }

    /// `crate::tick::on_tick` calls this every tick: writes any editor pane
    /// whose autosave debounce is due. Returns whether any pane actually
    /// attempted a write (successful or not, its dirty indicator changes
    /// either way this tick), so the caller knows whether this otherwise
    /// silent tick still needs to force a redraw.
    pub fn tick_autosave(&mut self) -> bool {
        let mut wrote_any = false;
        for runtime in self.panes.values_mut() {
            if let PaneRuntime::Editor(editor) = runtime {
                if let Some(result) = editor.autosave_if_due() {
                    wrote_any = true;
                    if let Err(err) = result {
                        tracing::warn!("autosave failed: {err}");
                    }
                }
            }
        }
        wrote_any
    }

    /// Records the tree node currently being drag-held by the left mouse
    /// button, if any -- see `crate::mouse`, which reads this back out on
    /// mouse-up to compute the `ReparentNode` request a drop should send.
    pub(crate) fn set_drag_source(&mut self, source: Option<NodeId>) {
        self.tree_drag_source = source;
    }

    /// The tree node currently being drag-held, if any -- see
    /// `set_drag_source`.
    pub(crate) fn drag_source(&self) -> Option<NodeId> {
        self.tree_drag_source
    }

    pub(crate) fn help_leader_pending(&self) -> bool {
        self.help_leader_pending
    }

    pub(crate) fn set_help_leader_pending(&mut self, pending: bool) {
        self.help_leader_pending = pending;
    }

    /// Row hover highlight used by `tree_ui::render`'s hover-only row
    /// action controls (edit/move/close).
    pub fn set_hovered_tree_node(&mut self, hit: Option<TreeNodeHit>) {
        self.hovered_tree_node = hit;
    }

    pub fn set_tree_toolbar_hover(&mut self, hovered: bool, action: Option<TreeToolbarAction>) {
        self.tree_toolbar_hovered = hovered;
        self.hovered_tree_toolbar_action = action;
    }

    /// Node under `position` in the tree panel, or `None` outside its rows.
    /// Called on every mouse-move over the tree panel, so the underlying
    /// `TreeItem` list is served from `tree_hit_test_cache` rather than
    /// rebuilt (see that field's doc comment).
    pub fn tree_node_at(&mut self, position: Position) -> Option<TreeNodeHit> {
        let items = self
            .tree_hit_test_cache
            .get_or_build(&self.tree, self.tree_version);
        tree_ui::node_at_position(items, &self.tree_state, self.layout.tree_area, position)
    }

    /// Consumes the oldest still-pending editor-open request whose
    /// remembered basename matches `name`, if any -- see
    /// `pending_editor_opens`'s doc comment.
    pub(crate) fn take_matching_pending_editor_open(&mut self, name: &str) -> Option<PathBuf> {
        let index = self
            .pending_editor_opens
            .iter()
            .position(|(basename, _)| basename == name)?;
        Some(self.pending_editor_opens.remove(index).1)
    }

    /// Ordinary (non-leader) key handling while the tree panel has focus.
    pub fn handle_tree_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.tree_state.key_up();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.tree_state.key_down();
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.tree_state.key_left();
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.tree_state.key_right();
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.tree_state.toggle_selected();
                if let Some(id) = self.selected_node_id() {
                    match self.tree.get(id) {
                        Some(node) if node.is_split_view() => self.show_split_view(id),
                        Some(node) if node.is_pane() => self.focus_pane(id),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    /// Ordinary (non-leader) key handling while a pane has focus: forwards
    /// raw input to the focused pane's runtime -- a `KeyInput` request for
    /// a terminal pane, or a direct buffer edit for an editor pane in
    /// Source mode (Rendered mode is read-only; only scroll navigation
    /// applies).
    ///
    /// A terminal pane intercepts Shift+PageUp/Shift+PageDown as local
    /// scrollback navigation rather than forwarding them -- plain
    /// PageUp/PageDown aren't in `encode_key_for_terminal`'s table at all
    /// (dropped silently), so this claims only the Shift-held variants,
    /// leaving room for plain PageUp/PageDown to reach the foreground app
    /// if that table ever grows to cover them. Any other key that *does*
    /// forward resets the view back to the live tail first, matching how
    /// an ordinary terminal emulator returns you to the prompt the moment
    /// you type.
    pub fn handle_pane_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};
        let Some(id) = self.active_pane_id() else {
            return;
        };
        let editor_content_area = self.editor_content_area(id);
        let mut pending_key_input = None;
        let mut is_enter_press = false;
        match self.panes.get_mut(&id) {
            Some(PaneRuntime::Terminal(view)) => {
                let page_lines = self.last_known_pane_size.0;
                let is_scroll_key = is_press(&key)
                    && key.modifiers.contains(KeyModifiers::SHIFT)
                    && matches!(key.code, KeyCode::PageUp | KeyCode::PageDown);
                if is_scroll_key {
                    match key.code {
                        KeyCode::PageUp => view.scroll_up(page_lines),
                        KeyCode::PageDown => view.scroll_down(page_lines),
                        _ => unreachable!("is_scroll_key only matches PageUp/PageDown"),
                    }
                } else if let Some(bytes) = encode_key_for_terminal(&key) {
                    view.scroll_to_bottom();
                    is_enter_press = bytes == b"\r";
                    pending_key_input = Some(bytes);
                }
            }
            Some(PaneRuntime::Editor(editor)) if editor.view_mode == EditorViewMode::Source => {
                editor.input(ratatui_textarea::Input::from(crossterm::event::Event::Key(
                    key,
                )));
            }
            Some(PaneRuntime::Editor(editor)) => {
                if !is_press(&key) {
                    return;
                }
                let max_scroll = editor
                    .rendered
                    .as_ref()
                    .map(|document| {
                        crate::markdown::view::max_scroll(
                            document,
                            editor_content_area.width,
                            editor_content_area.height,
                        )
                    })
                    .unwrap_or(0);
                match key.code {
                    KeyCode::Up => {
                        editor.rendered_scroll = editor.rendered_scroll.saturating_sub(1)
                    }
                    KeyCode::Down => {
                        editor.rendered_scroll = (editor.rendered_scroll + 1).min(max_scroll)
                    }
                    KeyCode::PageUp => {
                        editor.rendered_scroll = editor
                            .rendered_scroll
                            .saturating_sub(editor_content_area.height)
                    }
                    KeyCode::PageDown => {
                        editor.rendered_scroll =
                            (editor.rendered_scroll + editor_content_area.height).min(max_scroll)
                    }
                    _ => {}
                }
            }
            Some(PaneRuntime::Board(board)) => {
                if !is_press(&key) {
                    return;
                }
                let result = match key.code {
                    KeyCode::Left | KeyCode::Char('h')
                        if key.modifiers.contains(KeyModifiers::SHIFT) =>
                    {
                        board.move_selected_card(-1)
                    }
                    KeyCode::Right | KeyCode::Char('l')
                        if key.modifiers.contains(KeyModifiers::SHIFT) =>
                    {
                        board.move_selected_card(1)
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        board.select_previous_column();
                        Ok(())
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        board.select_next_column();
                        Ok(())
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        board.select_previous_card();
                        Ok(())
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        board.select_next_card();
                        Ok(())
                    }
                    KeyCode::Char('n') => {
                        self.action_add_board_card(id);
                        Ok(())
                    }
                    KeyCode::Char('c') => {
                        self.action_add_board_column(id);
                        Ok(())
                    }
                    KeyCode::Char('r') => match BoardPane::load(board.storage.clone()) {
                        Ok(reloaded) => {
                            **board = reloaded;
                            Ok(())
                        }
                        Err(error) => Err(error),
                    },
                    KeyCode::Char('e') => {
                        self.action_rename_board_selection(id);
                        Ok(())
                    }
                    KeyCode::Char('d') => {
                        self.action_delete_board_selection(id);
                        Ok(())
                    }
                    _ => Ok(()),
                };
                if let Err(error) = result {
                    self.status_message = Some(error);
                }
            }
            None => {}
        }
        if let Some(bytes) = pending_key_input {
            self.queue_request(ClientRequest::KeyInput { pane_id: id, bytes });
        }
        if is_enter_press {
            self.maybe_trigger_terminal_retitle(id);
        }
    }

    /// Bumps `id`'s Enter-press counter and, once it reaches
    /// `terminal_title_inference::RETITLE_ENTER_INTERVAL` (and only while
    /// the pane is actually eligible), captures its current screen text and
    /// queues a background LLM retitle -- the terminal-pane analogue of the
    /// server-driven triggers `crate::title_inference` reacts to for agent
    /// panes, since a plain shell has no session/turn-completion event to
    /// key off instead.
    ///
    /// Cadence alone isn't enough of a gate: a pane sitting through several
    /// short, visually-identical commands (`ls`, `ls -la`, a blank prompt)
    /// would otherwise re-run the LLM every `RETITLE_ENTER_INTERVAL`
    /// commands for no summarizable change, which is exactly the "retitled
    /// too often for no reason" failure mode. So the screen text is hashed
    /// and compared against the hash from this pane's last automatic
    /// retitle (`App::terminal_retitle_content_hashes`) before firing --
    /// unchanged content resets the counter (so the next interval gets a
    /// fresh chance) without spending an LLM call on it.
    fn maybe_trigger_terminal_retitle(&mut self, id: NodeId) {
        if !terminal_title_inference::terminal_ready_for_retitle(self, id) {
            return;
        }
        let counter = self.enter_press_counts.entry(id).or_insert(0);
        *counter += 1;
        if *counter < terminal_title_inference::RETITLE_ENTER_INTERVAL {
            return;
        }
        *counter = 0;
        let Some(PaneRuntime::Terminal(view)) = self.panes.get(&id) else {
            return;
        };
        let screen_text = view.with_screen(|screen| screen.contents());
        let content_hash = terminal_title_inference::hash_screen_text(&screen_text);
        if self.terminal_retitle_content_hashes.get(&id) == Some(&content_hash) {
            return;
        }
        self.terminal_retitle_content_hashes
            .insert(id, content_hash);
        self.titles_loading.insert(id);
        self.pending_retitle_requests
            .push(PendingRetitleRequest::Terminal {
                pane_id: id,
                screen_text,
                trigger: TitleTrigger::Automatic,
            });
    }

    /// Entry point for the tree row's manual "retitle" icon
    /// (`TreeRowAction::Retitle`): asks the LLM for a fresh title right
    /// now, bypassing the passive triggers' cadence/retry caps and, unlike
    /// them, overriding even a previously user-specified title -- an
    /// explicit click is itself a genuine user decision, the same as if
    /// they'd typed a name in directly (see `TitleTrigger::Manual`'s doc
    /// comment for how the result is applied once the worker finishes).
    pub fn action_request_retitle(&mut self, id: NodeId) {
        if self.titles_loading.contains(&id) {
            self.status_message = Some("Title inference already in progress".to_string());
            return;
        }
        match self.tree.get(id).map(|node| &node.kind) {
            Some(NodeKind::Pane {
                status: PaneStatus::Agent(class, _),
                ..
            }) => {
                let Some(session_id) = self.agent_session_ids.get(&id).cloned() else {
                    self.status_message = Some("No session detected yet for this pane".to_string());
                    return;
                };
                self.titles_loading.insert(id);
                self.pending_retitle_requests
                    .push(PendingRetitleRequest::Session {
                        pane_id: id,
                        agent_class: class.clone(),
                        session_id,
                        trigger: TitleTrigger::Manual,
                    });
            }
            Some(NodeKind::Pane {
                content: PaneContentKind::Terminal,
                status: PaneStatus::PlainShell,
                ..
            }) => {
                let Some(PaneRuntime::Terminal(view)) = self.panes.get(&id) else {
                    self.status_message = Some("No terminal content available yet".to_string());
                    return;
                };
                let screen_text = view.with_screen(|screen| screen.contents());
                // Keeps the automatic path's dedup baseline in sync, so it
                // doesn't immediately re-fire on the same content right
                // after this manual retitle completes.
                self.terminal_retitle_content_hashes
                    .insert(id, terminal_title_inference::hash_screen_text(&screen_text));
                self.titles_loading.insert(id);
                self.pending_retitle_requests
                    .push(PendingRetitleRequest::Terminal {
                        pane_id: id,
                        screen_text,
                        trigger: TitleTrigger::Manual,
                    });
            }
            _ => {
                self.status_message =
                    Some("This item doesn't support automatic titling".to_string());
            }
        }
    }

    /// Exact content rectangle for editor `id`, after its toolbar and
    /// optional minimap have been removed. Rendering and every input path
    /// must share this geometry so their wrap and scroll math cannot drift.
    pub fn editor_content_area(&self, id: NodeId) -> Rect {
        let show_minimap = self.panes.get(&id).is_some_and(
            |runtime| matches!(runtime, PaneRuntime::Editor(editor) if editor.show_minimap),
        );
        let pane_content_area = self
            .pane_viewport(id)
            .map(|viewport| viewport.content_area)
            .unwrap_or(self.layout.pane_content_area);
        crate::editor_chrome::compute(pane_content_area, show_minimap).content_area
    }

    /// Routes a mouse event that landed inside the focused pane's content
    /// box: an editor pane handles its own toolbar/minimap/content
    /// sub-regions, a terminal pane's coordinates (already pane-content-
    /// relative) become a `MouseInput` request.
    pub fn handle_pane_mouse(&mut self, mouse: crossterm::event::MouseEvent, position: Position) {
        let Some(viewport) = self.pane_viewport_at(position) else {
            return;
        };
        let id = viewport.pane_id;
        if matches!(
            mouse.kind,
            crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left)
        ) {
            self.focus_pane(id);
        }
        if !viewport.content_area.contains(position) {
            return;
        }

        if matches!(self.panes.get(&id), Some(PaneRuntime::Editor(_))) {
            self.handle_editor_pane_mouse(id, viewport.content_area, mouse, position);
            return;
        }
        if matches!(self.panes.get(&id), Some(PaneRuntime::Board(_))) {
            self.handle_board_pane_mouse(id, viewport.content_area, mouse, position);
            return;
        }

        // The wheel scrolls this view's own scrollback rather than being
        // forwarded, unless the foreground app has actually negotiated a
        // mouse protocol (e.g. `htop`, `vim`) and is asking to receive
        // wheel events itself. Most agent CLIs and a plain shell prompt
        // never negotiate one, which is exactly the "no scrollback"
        // complaint this feature addresses.
        use crossterm::event::MouseEventKind;
        if matches!(
            mouse.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) {
            if let Some(PaneRuntime::Terminal(view)) = self.panes.get_mut(&id) {
                if !view.wants_mouse_protocol() {
                    match mouse.kind {
                        MouseEventKind::ScrollUp => view.scroll_up(TERMINAL_WHEEL_SCROLL_LINES),
                        MouseEventKind::ScrollDown => view.scroll_down(TERMINAL_WHEEL_SCROLL_LINES),
                        _ => unreachable!("just matched ScrollUp/ScrollDown above"),
                    }
                    return;
                }
            }
        }

        let column = position.x.saturating_sub(viewport.content_area.x);
        let row = position.y.saturating_sub(viewport.content_area.y);
        let (kind, modifiers) = crate::mouse::to_ipc_mouse_event(mouse);
        self.queue_request(ClientRequest::MouseInput {
            pane_id: id,
            kind,
            column,
            row,
            modifiers,
        });
    }

    fn handle_board_pane_mouse(
        &mut self,
        id: NodeId,
        area: Rect,
        mouse: crossterm::event::MouseEvent,
        position: Position,
    ) {
        use crossterm::event::{MouseButton, MouseEventKind};
        let Some(PaneRuntime::Board(board)) = self.panes.get_mut(&id) else {
            return;
        };
        if board.columns.is_empty() {
            return;
        }
        let relative_x = position.x.saturating_sub(area.x) as usize;
        let column_width = (usize::from(area.width).max(1) / board.columns.len()).max(1);
        let column_index = (relative_x / column_width).min(board.columns.len() - 1);
        // `ui::draw_board` starts the first three-row card just inside the
        // column border and reserves one spacer row after each card.
        let relative_y = position.y.saturating_sub(area.y);
        let card_index = relative_y.saturating_sub(1) as usize / 4;
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                board.selected_column = column_index;
                if card_index < board.columns[column_index].cards.len() {
                    board.selected_card = card_index;
                    board.drag_source = Some((column_index, card_index));
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some((source_column, source_card)) = board.drag_source.take() {
                    if source_column != column_index
                        && source_card < board.columns[source_column].cards.len()
                    {
                        let card = board.columns[source_column].cards.remove(source_card);
                        board.columns[column_index].cards.push(card);
                        board.selected_column = column_index;
                        board.selected_card = board.columns[column_index].cards.len() - 1;
                        if let Err(error) = board.save() {
                            self.status_message = Some(error);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_editor_pane_mouse(
        &mut self,
        id: NodeId,
        pane_content_area: Rect,
        mouse: crossterm::event::MouseEvent,
        position: Position,
    ) {
        use crossterm::event::{MouseButton, MouseEventKind};
        let Some(PaneRuntime::Editor(editor)) = self.panes.get(&id) else {
            return;
        };
        let chrome = crate::editor_chrome::compute(pane_content_area, editor.show_minimap);

        if let Some(action) =
            crate::editor_toolbar::action_at(chrome.toolbar_area, editor, position)
        {
            if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                self.execute_editor_toolbar_action(id, action);
            }
            return;
        }

        if let Some(minimap_area) = chrome.minimap_area {
            if minimap_area.contains(position)
                && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            {
                self.handle_editor_minimap_click(id, minimap_area, position.y);
                return;
            }
        }

        if !chrome.content_area.contains(position) {
            return;
        }
        // The source editor owns its own viewport: wheel events must be
        // handled before checkbox hit-testing, otherwise the old early return
        // made scrolling work only over the minimap.
        if let Some(PaneRuntime::Editor(editor)) = self.panes.get_mut(&id) {
            if editor.view_mode == EditorViewMode::Source {
                match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        editor.scroll_source_view(
                            -(TERMINAL_WHEEL_SCROLL_LINES as i16),
                            chrome.content_area.height,
                        );
                        return;
                    }
                    MouseEventKind::ScrollDown => {
                        editor.scroll_source_view(
                            TERMINAL_WHEEL_SCROLL_LINES as i16,
                            chrome.content_area.height,
                        );
                        return;
                    }
                    _ => {}
                }
                let clicked_checkbox =
                    matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                        .then(|| checkbox_at(editor, chrome.content_area, position))
                        .flatten();
                if let Some((row, bracket_col, checked)) = clicked_checkbox {
                    editor.toggle_checkbox(row, bracket_col, checked);
                    let _ = self.tree.set_pane_status(
                        id,
                        PaneStatus::Editor {
                            dirty: editor.dirty,
                        },
                    );
                }
                return;
            }
            let max_scroll = editor
                .rendered
                .as_ref()
                .map(|document| {
                    crate::markdown::view::max_scroll(
                        document,
                        chrome.content_area.width,
                        chrome.content_area.height,
                    )
                })
                .unwrap_or(0);
            match mouse.kind {
                MouseEventKind::ScrollUp => {
                    editor.rendered_scroll = editor.rendered_scroll.saturating_sub(3)
                }
                MouseEventKind::ScrollDown => {
                    editor.rendered_scroll = (editor.rendered_scroll + 3).min(max_scroll)
                }
                _ => {}
            }
        }
    }

    /// A left-click inside the minimap column jumps the editor to the
    /// clicked source line.
    fn handle_editor_minimap_click(&mut self, id: NodeId, minimap_area: Rect, click_row: u16) {
        let editor_content_area = self.editor_content_area(id);
        let Some(PaneRuntime::Editor(editor)) = self.panes.get_mut(&id) else {
            return;
        };
        let total_lines = editor.textarea.lines().len();
        let relative_row = click_row.saturating_sub(minimap_area.y);
        let target_line =
            crate::minimap::line_for_click(relative_row, minimap_area.height, total_lines);

        match editor.view_mode {
            EditorViewMode::Source => editor.jump_to_line(target_line),
            EditorViewMode::Rendered => {
                let Some(document) = &editor.rendered else {
                    return;
                };
                let total_height =
                    crate::markdown::view::content_height(document, editor_content_area.width);
                let max_scroll = crate::markdown::view::max_scroll(
                    document,
                    editor_content_area.width,
                    editor_content_area.height,
                );
                let fraction = target_line as f64 / total_lines.max(1) as f64;
                editor.rendered_scroll =
                    ((fraction * f64::from(total_height)).round() as u16).min(max_scroll);
            }
        }
    }

    /// Applies one editor-toolbar click's effect, rebuilding the rendered
    /// document only on an actual Source -> Rendered transition.
    pub fn execute_editor_toolbar_action(
        &mut self,
        id: NodeId,
        action: crate::editor_toolbar::ToolbarAction,
    ) {
        use crate::editor_toolbar::ToolbarAction;
        let Some(PaneRuntime::Editor(editor)) = self.panes.get_mut(&id) else {
            return;
        };
        match action {
            ToolbarAction::ViewSource => {
                if editor.view_mode == EditorViewMode::Rendered {
                    editor.toggle_view_mode();
                }
            }
            ToolbarAction::ViewRendered => {
                let was_source = editor.view_mode == EditorViewMode::Source;
                if was_source {
                    editor.toggle_view_mode();
                }
                if was_source && editor.view_mode == EditorViewMode::Rendered {
                    self.rebuild_rendered_markdown(id);
                }
            }
            ToolbarAction::ToggleLineNumbers => editor.toggle_line_numbers(),
            ToolbarAction::ToggleMinimap => {
                editor.toggle_minimap();
                if editor.view_mode == EditorViewMode::Rendered {
                    self.rebuild_rendered_markdown(id);
                }
            }
            ToolbarAction::ToggleAutosave => editor.toggle_autosave(),
            ToolbarAction::Save => match editor.save() {
                Ok(()) => self.status_message = Some("Saved".to_string()),
                Err(err) => self.status_message = Some(format!("Save failed: {err}")),
            },
            ToolbarAction::SaveAs => self.action_start_save_as(id),
        }
    }
}

/// If `position` lands on a markdown task-list checkbox glyph in `editor`'s
/// Source-mode content, returns the buffer row, the checkbox's `[` char
/// column, and its current checked state -- ready for
/// `EditorPane::toggle_checkbox`. `content_area` must be the exact rect the
/// `TextArea` widget itself was rendered into (`editor_chrome::compute`'s
/// `content_area`), since column/row math is anchored to it.
///
/// Column mapping assumes no horizontal scroll (`ratatui_textarea` exposes
/// no public way to read its actual horizontal scroll offset). A checkbox
/// on a line long enough to have scrolled horizontally (rare for a short
/// "- [ ] ..." item) simply won't be found rather than risk mutating the
/// wrong character.
fn checkbox_at(
    editor: &EditorPane,
    content_area: Rect,
    position: Position,
) -> Option<(usize, usize, bool)> {
    if !editor.is_markdown() || !content_area.contains(position) {
        return None;
    }

    let clicked_row_on_screen = position.y - content_area.y;
    let row = editor.source_scroll_row() as usize + clicked_row_on_screen as usize;
    let line = editor.textarea.lines().get(row)?;
    let (bracket_col, checked) = crate::markdown::checkbox::find_checkbox(line)?;

    let gutter_width = if editor.show_line_numbers {
        usize::from(crate::editor_highlight::line_number_gutter_width(
            editor.textarea.lines().len(),
        ))
    } else {
        0
    };
    let clicked_col_on_screen = (position.x - content_area.x) as usize;
    let expected_col = gutter_width + bracket_col;
    let checkbox_span = expected_col..expected_col + 3; // "[ ]" / "[x]"
    checkbox_span
        .contains(&clicked_col_on_screen)
        .then_some((row, bracket_col, checked))
}

fn is_press(key: &crossterm::event::KeyEvent) -> bool {
    matches!(
        key.kind,
        crossterm::event::KeyEventKind::Press | crossterm::event::KeyEventKind::Repeat
    )
}

/// Hand-rolled crossterm-key -> terminal-input-bytes encoding. Not
/// exhaustive (v1 scope): covers printable characters, the common
/// control/navigation keys, and Ctrl+<letter>.
fn encode_key_for_terminal(key: &crossterm::event::KeyEvent) -> Option<Vec<u8>> {
    use crossterm::event::{KeyCode, KeyModifiers};
    if !is_press(key) {
        return None;
    }

    if key.modifiers.contains(KeyModifiers::CONTROL) {
        if let KeyCode::Char(c) = key.code {
            let lower = c.to_ascii_lowercase();
            if lower.is_ascii_lowercase() {
                return Some(vec![lower as u8 - b'a' + 1]);
            }
        }
    }

    match key.code {
        KeyCode::Char(c) => {
            let mut buf = [0u8; 4];
            Some(c.encode_utf8(&mut buf).as_bytes().to_vec())
        }
        KeyCode::Enter => Some(b"\r".to_vec()),
        KeyCode::Backspace => Some(b"\x7f".to_vec()),
        KeyCode::Tab => Some(b"\t".to_vec()),
        KeyCode::Esc => Some(b"\x1b".to_vec()),
        KeyCode::Up => Some(b"\x1b[A".to_vec()),
        KeyCode::Down => Some(b"\x1b[B".to_vec()),
        KeyCode::Right => Some(b"\x1b[C".to_vec()),
        KeyCode::Left => Some(b"\x1b[D".to_vec()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        App::new("test-session".to_string(), PathBuf::from("/tmp"))
    }

    #[test]
    fn new_app_starts_with_an_empty_tree_and_no_focused_pane() {
        let mut app = app();
        assert!(app.tree.children_of(ROOT_ID).unwrap().is_empty());
        assert_eq!(app.active_pane_id(), None);
        assert!(app.take_outbound_requests().is_empty());
    }

    #[test]
    fn request_new_markdown_board_preserves_the_existing_file_as_its_storage() {
        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let path = PathBuf::from("/tmp/project/release-plan.md");

        app.request_new_markdown_board(group, path.clone());

        assert_eq!(
            app.take_outbound_requests(),
            vec![ClientRequest::NewBoard {
                parent_group: group,
                name: "release-plan".to_string(),
                storage: ilium_core::BoardStorage::MarkdownFile { path },
            }]
        );
    }

    #[test]
    fn track_newly_created_nodes_ignores_the_very_first_snapshot() {
        let mut app = app();
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();

        // The first snapshot after attaching (e.g. a boot-time restore of a
        // whole persisted session) must not flash every node in it.
        app.track_newly_created_nodes(&tree);

        assert!(app.recently_created.is_empty());
        assert!(!app.recently_created.contains_key(&group));
        assert!(!app.recently_created.contains_key(&pane_id));
    }

    #[test]
    fn track_newly_created_nodes_flags_only_ids_absent_from_the_previous_tree() {
        let mut app = app();
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let existing_pane = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        app.track_newly_created_nodes(&tree);
        app.tree = tree.clone();

        let new_pane = tree
            .add_pane(group, "claude", PaneContentKind::Terminal)
            .unwrap();
        app.track_newly_created_nodes(&tree);

        assert!(!app.recently_created.contains_key(&existing_pane));
        assert!(app.recently_created.contains_key(&new_pane));
    }

    #[test]
    fn track_newly_created_nodes_flags_every_node_from_a_multi_create_burst_independently() {
        let mut app = app();
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        app.track_newly_created_nodes(&tree);
        app.tree = tree.clone();

        // Several panes created in one action (e.g. rapid clicks on the
        // creation toolbar) land in the same snapshot -- every one of them
        // must still be recorded, not just the first/last.
        let first = tree
            .add_pane(group, "shell-1", PaneContentKind::Terminal)
            .unwrap();
        let second = tree
            .add_pane(group, "shell-2", PaneContentKind::Terminal)
            .unwrap();
        let third_group = tree.add_group(ROOT_ID, "another group").unwrap();
        app.track_newly_created_nodes(&tree);

        assert!(app.recently_created.contains_key(&first));
        assert!(app.recently_created.contains_key(&second));
        assert!(app.recently_created.contains_key(&third_group));
    }

    #[test]
    fn prune_recently_created_drops_ids_no_longer_in_the_tree() {
        let mut app = app();
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        app.track_newly_created_nodes(&tree);
        app.tree = tree.clone();

        let pane_id = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        app.track_newly_created_nodes(&tree);
        app.tree = tree.clone();
        assert!(app.recently_created.contains_key(&pane_id));

        tree.remove_node(pane_id).unwrap();
        app.tree = tree;
        app.prune_recently_created();

        assert!(!app.recently_created.contains_key(&pane_id));
    }

    #[test]
    fn prune_recently_created_drops_ids_whose_flash_window_has_elapsed() {
        let mut app = app();
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        app.track_newly_created_nodes(&tree);
        app.tree = tree.clone();

        let pane_id = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        app.track_newly_created_nodes(&tree);
        app.tree = tree;

        // Simulate the flash window having fully elapsed without waiting
        // on a real clock: the entry was created at offset 0, "now" is well
        // past `RECENTLY_CREATED_PULSE_MS`.
        app.recently_created.insert(pane_id, 0);
        app.prune_recently_created_at(tree_ui::RECENTLY_CREATED_PULSE_MS * 2);

        assert!(!app.recently_created.contains_key(&pane_id));
    }

    #[test]
    fn action_request_retitle_queues_a_terminal_request_for_a_plain_shell() {
        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        app.panes.insert(
            pane_id,
            PaneRuntime::Terminal(Box::new(TerminalView::new(24, 80))),
        );

        app.action_request_retitle(pane_id);

        assert_eq!(app.status_message, None);
        assert!(app.titles_loading.contains(&pane_id));
        assert_eq!(app.take_pending_retitle_requests().len(), 1);
    }

    #[test]
    fn terminal_retitle_does_not_refire_on_unchanged_screen_content() {
        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        let mut view = TerminalView::new(24, 80);
        view.feed(b"$ ls\r\n");
        app.panes
            .insert(pane_id, PaneRuntime::Terminal(Box::new(view)));

        // `RETITLE_ENTER_INTERVAL` completed Enter presses fire the first
        // automatic retitle.
        for _ in 0..terminal_title_inference::RETITLE_ENTER_INTERVAL {
            app.maybe_trigger_terminal_retitle(pane_id);
        }
        assert_eq!(app.take_pending_retitle_requests().len(), 1);
        // Simulate the worker completing so the pane is eligible again.
        app.titles_loading.remove(&pane_id);

        // Same screen content, another full interval of Enter presses:
        // nothing new to summarize, so no second LLM call is queued.
        for _ in 0..terminal_title_inference::RETITLE_ENTER_INTERVAL {
            app.maybe_trigger_terminal_retitle(pane_id);
        }
        assert_eq!(app.take_pending_retitle_requests().len(), 0);

        // Once the screen actually changes, the next interval fires again.
        if let Some(PaneRuntime::Terminal(view)) = app.panes.get_mut(&pane_id) {
            view.feed(b"$ git status\r\n");
        }
        for _ in 0..terminal_title_inference::RETITLE_ENTER_INTERVAL {
            app.maybe_trigger_terminal_retitle(pane_id);
        }
        assert_eq!(app.take_pending_retitle_requests().len(), 1);
    }

    #[test]
    fn setting_layout_resizes_the_visible_terminal_pane() {
        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        app.panes.insert(
            pane_id,
            PaneRuntime::Terminal(Box::new(TerminalView::new(24, 80))),
        );

        app.right_panel_target = RightPanelTarget::Pane { pane_id };
        app.set_screen_area(Rect::new(0, 0, 140, 40));
        let viewport = app.pane_viewport(pane_id).unwrap();

        let requests = app.take_outbound_requests();
        assert_eq!(
            requests,
            vec![ClientRequest::ResizePane {
                pane_id,
                rows: viewport.content_area.height,
                cols: viewport.content_area.width
            }]
        );
    }

    #[test]
    fn close_confirmation_is_none_for_an_empty_group() {
        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "empty").unwrap();
        assert_eq!(app.close_confirmation_message(group), None);
    }

    #[test]
    fn close_confirmation_warns_about_a_nonempty_group() {
        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        app.tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        assert!(app.close_confirmation_message(group).is_some());
    }

    #[test]
    fn close_confirmation_warns_about_a_dirty_editor() {
        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "notes.md", PaneContentKind::Editor)
            .unwrap();
        let mut editor = EditorPane::empty();
        editor.dirty = true;
        app.panes
            .insert(pane_id, PaneRuntime::Editor(Box::new(editor)));
        assert!(app.close_confirmation_message(pane_id).is_some());
    }

    #[test]
    fn close_confirmation_is_none_for_a_clean_editor() {
        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "notes.md", PaneContentKind::Editor)
            .unwrap();
        app.panes
            .insert(pane_id, PaneRuntime::Editor(Box::new(EditorPane::empty())));
        assert_eq!(app.close_confirmation_message(pane_id), None);
    }

    #[test]
    fn focus_pane_selects_it_and_clears_a_stale_done_flag() {
        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "claude", PaneContentKind::Terminal)
            .unwrap();
        app.tree
            .set_pane_status(
                pane_id,
                PaneStatus::Agent(
                    ilium_core::AgentClass::Claude,
                    ilium_core::AgentActivity::Done,
                ),
            )
            .unwrap();

        app.focus_pane(pane_id);

        assert_eq!(app.active_pane_id(), Some(pane_id));
        assert_eq!(app.focus, FocusTarget::Pane);
        match &app.tree.get(pane_id).unwrap().kind {
            NodeKind::Pane { status, .. } => assert_eq!(
                *status,
                PaneStatus::Agent(
                    ilium_core::AgentClass::Claude,
                    ilium_core::AgentActivity::Idle
                )
            ),
            _ => panic!("expected a pane"),
        }
    }

    #[test]
    fn restore_expanded_groups_prunes_opened_paths_for_removed_groups() {
        // A group's `opened` entry (`TreeState` keys it by the whole
        // ancestor path, not just the id) must not survive the group being
        // removed from the tree -- otherwise it would sit in `tree_state`
        // forever, since `NodeId` is never reused and nothing else ever
        // expires it. See `restore_expanded_groups`'s doc comment.
        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        app.restore_expanded_groups();
        assert!(app.tree_state.opened().contains(&vec![group]));

        app.tree.remove_node(group).unwrap();
        app.restore_expanded_groups();

        assert!(app.tree_state.opened().is_empty());
    }

    #[test]
    fn restore_expanded_groups_prunes_opened_paths_for_reparented_groups() {
        // A group moved under a different parent (same `NodeId`, new
        // ancestor chain) must have its *old* path dropped from `opened`,
        // not just gain a second, now-stale entry alongside the fresh one
        // -- see `restore_expanded_groups`'s doc comment on why matching
        // by id alone isn't enough.
        let mut app = app();
        let outer = app.tree.add_group(ROOT_ID, "outer").unwrap();
        let inner = app.tree.add_group(outer, "inner").unwrap();
        app.restore_expanded_groups();
        assert!(app.tree_state.opened().contains(&vec![outer, inner]));

        app.tree.move_node(inner, ROOT_ID, None).unwrap();
        app.restore_expanded_groups();

        assert!(!app.tree_state.opened().contains(&vec![outer, inner]));
        assert!(app.tree_state.opened().contains(&vec![inner]));
    }

    #[test]
    fn group_for_new_node_falls_back_to_root_id_for_the_server_to_resolve() {
        // With nothing selected/focused and no group yet in this client's
        // tree mirror, the fallback must be `ROOT_ID` -- never a
        // client-invented `NodeId` from mutating the local mirror, which
        // the server's tree wouldn't recognize (see this method's doc
        // comment).
        let mut app = app();
        assert_eq!(app.group_for_new_node(), ROOT_ID);
    }

    #[test]
    fn group_for_new_node_prefers_the_selected_group() {
        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        app.tree_state.select(vec![group]);
        assert_eq!(app.group_for_new_node(), group);
    }

    #[test]
    fn focusing_a_split_child_displays_every_member_and_activates_only_that_child() {
        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let first = app
            .tree
            .add_pane(group, "first", PaneContentKind::Terminal)
            .unwrap();
        let second = app
            .tree
            .add_pane(group, "second", PaneContentKind::Terminal)
            .unwrap();
        let split = app
            .tree
            .create_split_view(
                group,
                "Vertical split",
                SplitOrientation::Vertical,
                &[first, second],
            )
            .unwrap();

        app.focus_pane(second);

        assert_eq!(app.displayed_pane_ids(), vec![first, second]);
        assert_eq!(app.active_pane_id(), Some(second));
        assert_eq!(
            app.right_panel_target,
            RightPanelTarget::SplitView {
                split_id: split,
                active_pane_id: Some(second)
            }
        );
    }

    #[test]
    fn split_creation_choices_exclude_panes_already_in_a_split() {
        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let available = app
            .tree
            .add_pane(group, "available", PaneContentKind::Editor)
            .unwrap();
        let contained = app
            .tree
            .add_pane(group, "contained", PaneContentKind::Terminal)
            .unwrap();
        app.tree
            .create_split_view(
                group,
                "Vertical split",
                SplitOrientation::Vertical,
                &[contained],
            )
            .unwrap();

        app.continue_create_split(SplitOrientation::Horizontal);

        let Mode::CreateSplitMembers(state) = &app.mode else {
            panic!("expected split member selector");
        };
        assert_eq!(state.choices.len(), 1);
        assert_eq!(state.choices[0].pane_id, available);
    }

    #[test]
    fn committing_an_empty_split_queues_one_atomic_request() {
        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        app.tree_state.select(vec![group]);
        app.continue_create_split(SplitOrientation::Horizontal);
        let Mode::CreateSplitMembers(state) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("expected split member selector");
        };

        app.commit_create_split(state);

        assert_eq!(
            app.take_outbound_requests(),
            vec![ClientRequest::CreateSplitView {
                parent_group: group,
                name: "Horizontal split".to_string(),
                orientation: SplitOrientation::Horizontal,
                pane_ids: Vec::new(),
            }]
        );
    }

    #[test]
    fn split_layout_resizes_only_visible_terminal_members_to_their_slots() {
        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let first = app
            .tree
            .add_pane(group, "first", PaneContentKind::Terminal)
            .unwrap();
        let second = app
            .tree
            .add_pane(group, "second", PaneContentKind::Terminal)
            .unwrap();
        let hidden = app
            .tree
            .add_pane(group, "hidden", PaneContentKind::Terminal)
            .unwrap();
        let split = app
            .tree
            .create_split_view(
                group,
                "Vertical split",
                SplitOrientation::Vertical,
                &[first, second],
            )
            .unwrap();
        for pane_id in [first, second, hidden] {
            app.panes.insert(
                pane_id,
                PaneRuntime::Terminal(Box::new(TerminalView::new(24, 80))),
            );
        }
        app.right_panel_target = RightPanelTarget::SplitView {
            split_id: split,
            active_pane_id: Some(first),
        };

        app.set_screen_area(Rect::new(0, 0, 120, 40));

        let resize_requests = app
            .take_outbound_requests()
            .into_iter()
            .filter_map(|request| match request {
                ClientRequest::ResizePane {
                    pane_id,
                    rows,
                    cols,
                } => Some((pane_id, rows, cols)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(resize_requests.len(), 2);
        assert!(resize_requests
            .iter()
            .all(|(_, rows, cols)| *rows == 37 && *cols == 42));
        assert!(!resize_requests
            .iter()
            .any(|(pane_id, _, _)| *pane_id == hidden));
    }
}
