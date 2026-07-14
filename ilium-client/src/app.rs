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
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

use ilium_core::{
    AgentActivity, AgentClass, AgentProvider, BuiltinAgentProvider, GroupListing, Node, NodeId,
    NodeKind, PaneContentKind, PaneStatus, PaneTitleSource, SplitOrientation, Tree, ROOT_ID,
};
use ilium_ipc::ClientRequest;
use ratatui::layout::{Position, Rect};
use tui_tree_widget::TreeState;

use crate::agent_from_line::{
    CreateAgentFromLineState, EditorLineContextAction, EditorLineContextMenu, EditorSourceLine,
};
use crate::board::BoardPane;
use crate::config::{KanbanBoardSettings, KeyboardSettings, TreeOrder, UiSettings};
use crate::editor_pane::{EditorPane, EditorViewMode};
use crate::explorer_overlay::ExplorerOverlay;
use crate::layout::{TreeWidthAnimation, UiLayout};
use crate::naming_workers::TitleTrigger;
use crate::scheduled_input::ScheduledInputDialogState;
use crate::search_ui::{
    self, SearchLocation, SearchObjectKind, SearchResult, SearchState, WorkspaceSearchContent,
    WorkspaceSearchRequest, WorkspaceSearchSource, WorkspaceSearchText,
};
use crate::search_workers::{SearchWorkerEvent, SearchWorkers};
use crate::split_layout::{self, PaneViewport};
use crate::terminal_title_inference;
use crate::terminal_view::{self, TerminalView};
use crate::text_prompt::TextPromptState;
use crate::theme::{self, ColorScheme, Theme};
use crate::tree_transitions::TreeTransitions;
use crate::tree_ui::{self, TreeNodeHit, TreeToolbarAction};

/// Rows scrolled per wheel notch over a terminal pane's own scrollback --
/// matches `tree_state.scroll_up(3)`/`scroll_down(3)`'s existing per-notch
/// amount elsewhere in this crate.
const TERMINAL_WHEEL_SCROLL_LINES: u16 = 3;

/// Collapses lexical `.` and `..` components without requiring the target to
/// exist. Board creation needs a stable absolute identity before it creates a
/// new backing file, while `std::fs::canonicalize` can only handle paths that
/// already exist.
fn normalize_path_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

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
    /// Form for one server-owned terminal input countdown.
    SchedulePaneInput(Box<ScheduledInputDialogState>),
    /// A mouse-anchored action menu for one physical editor source line.
    EditorLineContextMenu(EditorLineContextMenu),
    /// Agent selector and editable task prompt opened from an editor line.
    CreateAgentFromLine(Box<CreateAgentFromLineState>),
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
    /// Full-screen finder over terminal replay and locally-open buffers.
    Search(Box<SearchState>),
}

/// Why the interactive client event loop should return to the CLI wrapper.
/// Keeping restart distinct from an ordinary quit lets the wrapper replace
/// only this process without ever expressing a server lifecycle operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientExitReason {
    Quit,
    RestartRequested,
}

/// Which tab is selected in the full-screen settings view. Add a new
/// variant here -- and a matching arm in every `match` over this type --
/// before adding another tab; see `crate::settings_ui`'s module doc comment
/// for the tab-list-left/content-right layout this drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Appearance,
    Keyboard,
    KanbanBoard,
    Sound,
    About,
}

impl SettingsTab {
    /// Every tab, in the order the tab list renders them.
    pub const ALL: [SettingsTab; 5] = [
        Self::Appearance,
        Self::Keyboard,
        Self::KanbanBoard,
        Self::Sound,
        Self::About,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Appearance => "User Appearance",
            Self::Keyboard => "Keyboard",
            Self::KanbanBoard => "Kanban Board",
            Self::Sound => "Sound",
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
    TreeOrder,
    AgentIdentifierMode,
    ClaudeAgentIcon,
    CodexAgentIcon,
    AntigravityAgentIcon,
    ColorScheme,
}

impl AppearanceRow {
    pub const ALL: [AppearanceRow; 8] = [
        Self::AutoResizeTree,
        Self::TreeWidth,
        Self::TreeOrder,
        Self::AgentIdentifierMode,
        Self::ClaudeAgentIcon,
        Self::CodexAgentIcon,
        Self::AntigravityAgentIcon,
        Self::ColorScheme,
    ];
}

/// One live-persisted layout control in the Kanban Board settings tab.
/// Keeping both controls in this registry gives keyboard and mouse input the
/// same selected-row contract as the Appearance and Sound tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KanbanBoardRow {
    CardPreviewLines,
    MinimumColumnWidth,
}

impl KanbanBoardRow {
    pub const ALL: [KanbanBoardRow; 2] = [Self::CardPreviewLines, Self::MinimumColumnWidth];
}

/// Rows in the Sound tab. Keeping source, selected file, preview, and event
/// toggles in one registry makes keyboard and mouse interaction exhaustive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundRow {
    Source,
    File,
    Preview,
    AgentFinished,
    ApprovalRequired,
    AgentStarted,
    WaitingBackground,
}

impl SoundRow {
    pub const ALL: [SoundRow; 7] = [
        Self::Source,
        Self::File,
        Self::Preview,
        Self::AgentFinished,
        Self::ApprovalRequired,
        Self::AgentStarted,
        Self::WaitingBackground,
    ];

    pub const fn event(self) -> Option<ilium_sound::SoundEvent> {
        match self {
            Self::AgentFinished => Some(ilium_sound::SoundEvent::AgentFinished),
            Self::ApprovalRequired => Some(ilium_sound::SoundEvent::ApprovalRequired),
            Self::AgentStarted => Some(ilium_sound::SoundEvent::AgentStarted),
            Self::WaitingBackground => Some(ilium_sound::SoundEvent::WaitingBackground),
            Self::Source | Self::File | Self::Preview => None,
        }
    }
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
/// (see `App::apply_and_persist_ui_settings` and
/// `App::apply_and_persist_keyboard_settings`) -- there is no buffered
/// "Cancel" path, so a value changes the instant it's touched, the same way
/// a rename or a theme hex edit in `config.toml` would. `tab`/`selected_row`
/// are pure navigation state; the actual settings values live in
/// `App::ui_settings`/`App::keyboard_settings`, not here, so nothing here
/// needs its own persistence.
pub struct SettingsState {
    pub tab: SettingsTab,
    /// Selected row within the active tab's list. Appearance currently has
    /// three rows and Keyboard one; About has none.
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
    /// Opens full-screen workspace search regardless of the clicked node.
    Search,
    FocusPane,
    CreateBoardFromMarkdown,
    SchedulePaneInput,
    ShowSplitView,
    ToggleGroup,
    NewTerminal,
    /// Launches a registered first-party agent under the selected group.
    NewAgent(BuiltinAgentProvider),
    NewEditor,
    NewGroup,
    NewSplitView,
    NewFolder,
    Rename,
    MoveUp,
    MoveDown,
    Close,
    /// Opens the adjacent checked ordering submenu.
    OrderBy,
    /// Replaces only the attached client process. The detached server keeps
    /// its PTYs and authoritative session tree alive throughout the handoff.
    Restart,
    /// Opens the full-screen settings view -- present in every right-click
    /// menu regardless of what was clicked (a pane, a group, or empty tree
    /// space), since it isn't a per-node action. This is deliberately the
    /// *only* mouse entry point into settings (plus the `Settings` leader
    /// action) -- see `Mode::Settings`'s doc comment.
    Settings,
}

impl ContextMenuAction {
    /// The concise label rendered in the popup menu.
    pub fn label(self) -> String {
        match self {
            Self::Search => "Search workspace…".to_string(),
            Self::FocusPane => "Focus pane".to_string(),
            Self::CreateBoardFromMarkdown => "Create board from Markdown".to_string(),
            Self::SchedulePaneInput => "Hit key(s) X time from now".to_string(),
            Self::ShowSplitView => "Show split view".to_string(),
            Self::ToggleGroup => "Expand / collapse".to_string(),
            Self::NewTerminal => "New terminal here".to_string(),
            Self::NewAgent(provider) => format!("New {} agent here", provider.label()),
            Self::NewEditor => "New editor here".to_string(),
            Self::NewGroup => "New group\u{2026}".to_string(),
            Self::NewSplitView => "New split view\u{2026}".to_string(),
            Self::NewFolder => "Open folder\u{2026}".to_string(),
            Self::Rename => "Rename".to_string(),
            Self::MoveUp => "Move up".to_string(),
            Self::MoveDown => "Move down".to_string(),
            Self::Close => "Close".to_string(),
            Self::OrderBy => "Order by  ▸".to_string(),
            Self::Restart => "Restart".to_string(),
            Self::Settings => "Settings\u{2026}".to_string(),
        }
    }

    /// Actions that apply to the client/tree view as a whole rather than the
    /// node under the pointer. One registry keeps every menu target in sync.
    const GLOBAL_ACTIONS: [Self; 4] = [Self::Search, Self::OrderBy, Self::Restart, Self::Settings];

    /// Materializes provider-driven creation entries for any tree menu.
    fn new_agent_actions() -> impl Iterator<Item = Self> {
        BuiltinAgentProvider::ALL.into_iter().map(Self::NewAgent)
    }
}

/// Adjacent submenu state for the closed [`TreeOrder`] registry.
pub struct TreeOrderSubmenu {
    pub area: Rect,
    pub selected_index: usize,
}

/// State of a context menu: its tree target, screen position, and keyboard
/// or mouse selection. The renderer only reads this state; all effects stay
/// in `App`/`crate::keys`/`crate::mouse`.
pub struct ContextMenu {
    pub target: NodeId,
    pub area: Rect,
    pub actions: Vec<ContextMenuAction>,
    pub selected_index: usize,
    pub tree_order_submenu: Option<TreeOrderSubmenu>,
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

#[derive(Debug, Clone)]
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
    /// Set by input dispatch when the client should leave its event loop.
    /// `None` keeps rendering; the concrete reason controls whether the CLI
    /// exits normally or re-execs a fresh client binary afterward.
    pub exit_reason: Option<ClientExitReason>,
    /// Stable reference for purely visual animations in the tree.
    pub started_at: Instant,
    /// Structural row transitions live beside the render-cache mirror, never
    /// in the authoritative server tree or the IPC protocol.
    pub(crate) tree_transitions: TreeTransitions,
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
    /// Live card-preview settings shared by every board pane in this client.
    pub kanban_board_settings: KanbanBoardSettings,
    /// Live user-global sound choices and the system catalog discovered once
    /// before the terminal enters raw mode.
    pub sound_settings: ilium_sound::SoundSettings,
    pub sound_discovery: ilium_sound::SoundDiscovery,
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
    /// (ms) at which its insertion slide settles. Read by `tree_ui` to flash
    /// a freshly created node *after* it has moved into place, so a click (or
    /// a multi-create burst) is obviously followed by something appearing;
    /// pruned by `prune_recently_created` once its flash window elapses or
    /// the node is gone. See `track_tree_snapshot_change` for why the very
    /// first tree snapshot after attaching never populates this (a
    /// boot-time restore of a whole persisted session must not flash every
    /// node at once).
    pub recently_created: HashMap<NodeId, u128>,
    /// Whether `track_tree_snapshot_change` has processed at least one
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
    /// The built items are determined by tree structure plus `TreeOrder`.
    /// `TreeItemCache` keys both inputs; status events also bump
    /// `tree_version` because Type ordering distinguishes agent terminals
    /// from plain shells. This keeps mouse hit rows aligned with rendering
    /// without rebuilding labels on every pointer move.
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
            exit_reason: None,
            started_at,
            tree_transitions: TreeTransitions::default(),
            last_known_pane_size: (terminal_view::DEFAULT_ROWS, terminal_view::DEFAULT_COLS),
            layout: UiLayout::default(),
            tree_width_animation: TreeWidthAnimation::new(
                started_at,
                UiSettings::default().tree_width,
            ),
            ui_settings: UiSettings::default(),
            keyboard_settings: KeyboardSettings::default(),
            kanban_board_settings: KanbanBoardSettings::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_discovery: ilium_sound::SoundDiscovery::default(),
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

    /// Starts client-only row transitions ahead of `render_cache` swapping in
    /// `new_tree`. The first snapshot is deliberately adopted without motion:
    /// attaching to a restored session must not animate the whole tree as new.
    pub(crate) fn track_tree_snapshot_change(&mut self, new_tree: &Tree) {
        let now_offset_ms = self.started_at.elapsed().as_millis();
        self.track_tree_snapshot_change_at(new_tree, now_offset_ms);
    }

    /// Deterministic form of [`Self::track_tree_snapshot_change`] for focused
    /// tests of sequencing and pulse timing.
    fn track_tree_snapshot_change_at(&mut self, new_tree: &Tree, now_offset_ms: u128) {
        if !self.has_applied_first_snapshot {
            self.has_applied_first_snapshot = true;
            return;
        }

        let pulse_starts =
            self.tree_transitions
                .observe_snapshot_change(&self.tree, new_tree, now_offset_ms);
        for (node_id, pulse_started_offset_ms) in pulse_starts {
            self.recently_created
                .entry(node_id)
                .or_insert(pulse_started_offset_ms);
        }
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

    /// Detaches this connection cleanly and records how the CLI wrapper should
    /// proceed once the terminal has been restored by `TerminalGuard`.
    pub fn request_client_exit(&mut self, reason: ClientExitReason) {
        self.queue_request(ClientRequest::Detach);
        self.exit_reason = Some(reason);
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

    /// Activates the surviving sidebar row chosen after a close. Panes and
    /// split views reopen their right-panel content; ordinary containers and
    /// folder roots become the tree selection without changing expansion.
    pub(crate) fn activate_tree_successor(&mut self, node_id: NodeId) {
        let Some(node) = self.tree.get(node_id) else {
            return;
        };
        let is_split_view = node.is_split_view();
        let is_pane = node.is_pane();

        if is_split_view {
            self.show_split_view(node_id);
        } else if is_pane {
            self.focus_pane(node_id);
        } else {
            self.select_node(node_id);
            self.leave_pane_focus();
        }
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

    /// Applies and persists the global card-preview height used by all boards.
    pub fn settings_adjust_card_preview_lines(&mut self, direction: i32) {
        let current = i32::from(self.kanban_board_settings.card_preview_lines);
        let card_preview_lines = (current + direction).clamp(
            i32::from(crate::config::MIN_CARD_PREVIEW_LINES),
            i32::from(crate::config::MAX_CARD_PREVIEW_LINES),
        ) as u16;
        self.kanban_board_settings.card_preview_lines = card_preview_lines;
        self.persist_kanban_board_settings();
    }

    /// Applies and persists the minimum terminal width shared by every board
    /// column before horizontal scrolling takes over.
    pub fn settings_adjust_board_column_width(&mut self, direction: i32) {
        let current = i32::from(self.kanban_board_settings.minimum_column_width);
        let minimum_column_width = (current + direction).clamp(
            i32::from(crate::config::MIN_BOARD_COLUMN_WIDTH),
            i32::from(crate::config::MAX_BOARD_COLUMN_WIDTH),
        ) as u16;
        self.kanban_board_settings.minimum_column_width = minimum_column_width;
        self.persist_kanban_board_settings();
    }

    /// Writes the complete Kanban presentation table after either live
    /// control changes, preserving unrelated global configuration tables.
    fn persist_kanban_board_settings(&mut self) {
        if let Some(config_dir) = self.config_dir.clone() {
            if let Err(error) =
                crate::config::save_kanban_board_settings(&config_dir, &self.kanban_board_settings)
            {
                self.status_message =
                    Some(format!("Could not save Kanban Board settings: {error}"));
            }
        }
    }

    /// Adjusts the selected Kanban row without duplicating persistence logic
    /// in keyboard and mouse dispatch.
    pub fn settings_adjust_kanban_board_row(&mut self, row: KanbanBoardRow, direction: i32) {
        match row {
            KanbanBoardRow::CardPreviewLines => self.settings_adjust_card_preview_lines(direction),
            KanbanBoardRow::MinimumColumnWidth => {
                self.settings_adjust_board_column_width(direction)
            }
        }
    }

    /// Installs startup sound settings without emitting an IPC update. The
    /// server has loaded the same global config before the client connects.
    pub fn apply_sound_settings(&mut self, sound: ilium_sound::SoundSettings) {
        self.sound_settings = sound;
    }

    /// Applies, persists, and sends one live update to the current detached
    /// server. Other project servers observe the atomic config-file change
    /// through their owned watchers.
    fn apply_and_persist_sound_settings(&mut self, sound: ilium_sound::SoundSettings) {
        self.sound_settings = sound;
        if let Some(config_dir) = self.config_dir.clone() {
            if let Err(error) =
                crate::config::save_sound_settings(&config_dir, &self.sound_settings)
            {
                self.status_message = Some(format!("Could not save sound settings: {error}"));
            }
        }
        self.queue_request(ClientRequest::UpdateSoundSettings {
            settings: self.sound_settings.clone(),
        });
    }

    /// Switches between the portable system beep and a discovered sound file.
    pub fn settings_toggle_sound_source(&mut self) {
        let mut sound = self.sound_settings.clone();
        sound.source = match sound.source {
            ilium_sound::SoundSourceKind::SystemBeep => {
                if sound.file.is_none() {
                    sound.file = self
                        .sound_discovery
                        .sounds
                        .first()
                        .map(|entry| entry.path.clone());
                }
                ilium_sound::SoundSourceKind::SoundFile
            }
            ilium_sound::SoundSourceKind::SoundFile => ilium_sound::SoundSourceKind::SystemBeep,
        };
        self.apply_and_persist_sound_settings(sound);
    }

    /// Cycles through the actual sound files discovered on this machine.
    pub fn settings_adjust_sound_file(&mut self, direction: i32) {
        if self.sound_discovery.sounds.is_empty() {
            self.status_message = Some("No system sound files were found".to_string());
            return;
        }
        let current_index = self
            .sound_settings
            .file
            .as_ref()
            .and_then(|path| {
                self.sound_discovery
                    .sounds
                    .iter()
                    .position(|entry| &entry.path == path)
            })
            .unwrap_or(0);
        let count = self.sound_discovery.sounds.len() as i32;
        let next_index = (current_index as i32 + direction.signum()).rem_euclid(count) as usize;
        self.settings_select_sound_file(next_index);
    }

    /// Selects one catalog entry directly, used by mouse clicks on the
    /// discovered-sounds list.
    pub fn settings_select_sound_file(&mut self, index: usize) {
        let Some(entry) = self.sound_discovery.sounds.get(index) else {
            return;
        };
        let mut sound = self.sound_settings.clone();
        sound.source = ilium_sound::SoundSourceKind::SoundFile;
        sound.file = Some(entry.path.clone());
        self.apply_and_persist_sound_settings(sound);
    }

    pub fn settings_toggle_sound_event(&mut self, event: ilium_sound::SoundEvent) {
        let mut sound = self.sound_settings.clone();
        sound.events.toggle(event);
        self.apply_and_persist_sound_settings(sound);
    }

    /// Asks the server-owned sound actor to play once. Preview uses the same
    /// backend and selected path as real transition alerts.
    pub fn settings_preview_sound(&mut self) {
        if self.sound_settings.source == ilium_sound::SoundSourceKind::SoundFile
            && !self
                .sound_settings
                .file
                .as_ref()
                .is_some_and(|path| path.is_file())
        {
            self.status_message =
                Some("Cannot preview: select an available sound file first".to_string());
            return;
        }
        self.queue_request(ClientRequest::PreviewSound {
            source: self.sound_settings.source,
            file: self.sound_settings.file.clone(),
        });
        self.status_message = Some("Playing sound preview".to_string());
    }

    pub fn settings_adjust_sound_row(&mut self, row: SoundRow, direction: i32) {
        match row {
            SoundRow::Source => self.settings_toggle_sound_source(),
            SoundRow::File => self.settings_adjust_sound_file(direction),
            SoundRow::Preview => self.settings_preview_sound(),
            event_row => {
                if let Some(event) = event_row.event() {
                    self.settings_toggle_sound_event(event);
                }
            }
        }
    }

    /// Opens the full-screen settings view. See `Mode::Settings`'s and
    /// `SettingsState`'s doc comments for the UI/UX brief this screen (and
    /// every setting added to it) must keep matching.
    pub fn action_open_settings(&mut self) {
        self.mode = Mode::Settings(SettingsState::new());
    }

    /// Opens the full-screen workspace finder. Its index is intentionally
    /// built only after the user pauses typing, so opening the finder and
    /// every query edit remain immediate even with large retained journals.
    pub fn action_open_search(&mut self) {
        self.mode = Mode::Search(Box::default());
    }

    /// Starts one debounced background scan when the active finder has been
    /// quiet for at least `SEARCH_DEBOUNCE`. The worker receives immutable
    /// snapshots and never borrows `App`, leaving the event loop free to
    /// render each key immediately.
    pub fn tick_workspace_search(&mut self, now: Instant, workers: &mut SearchWorkers) -> bool {
        if !workers.is_idle() {
            return false;
        }
        let Mode::Search(mut state) = std::mem::replace(&mut self.mode, Mode::Normal) else {
            return false;
        };
        let Some((revision, query)) = state.take_due_search(now) else {
            self.mode = Mode::Search(state);
            return false;
        };
        let request = WorkspaceSearchRequest {
            revision,
            query,
            sources: self.workspace_search_sources(),
        };
        let started = match workers.start(request) {
            Ok(()) => true,
            Err(error) => {
                state.cancel_in_flight_search(revision);
                self.status_message = Some(format!("Could not start workspace search: {error}"));
                false
            }
        };
        self.mode = Mode::Search(state);
        started
    }

    /// Receives one owned worker result. A revision mismatch is expected when
    /// the user continued typing while a previous scan was still running.
    pub fn apply_workspace_search_result(&mut self, event: SearchWorkerEvent) -> bool {
        let Mode::Search(mut state) = std::mem::replace(&mut self.mode, Mode::Normal) else {
            return false;
        };
        let changed = state.complete_search(event.revision, event.results);
        self.mode = Mode::Search(state);
        changed
    }

    /// Synchronous helper retained for focused unit tests. Interactive
    /// search uses `tick_workspace_search` and `SearchWorkers` instead.
    pub fn refresh_search_results(&self, state: &mut SearchState) {
        let query = state.query.buf.trim();
        if query.is_empty() {
            state.replace_results(Vec::new());
            return;
        }

        let request = WorkspaceSearchRequest {
            revision: 0,
            query: query.to_string(),
            sources: self.workspace_search_sources(),
        };
        state.replace_results(search_ui::search_workspace(&request, || false));
    }

    /// Captures all searchable client-owned state. The terminal half is an
    /// O(1) `Arc` snapshot of server-replayed history; editor and board text
    /// is copied only after the debounce has elapsed and on a background scan.
    fn workspace_search_sources(&self) -> Vec<WorkspaceSearchSource> {
        let mut sources = Vec::new();
        for (pane_id, runtime) in &self.panes {
            let Some(node) = self.tree.get(*pane_id) else {
                continue;
            };
            let NodeKind::Pane {
                content,
                status,
                title_source,
                board_storage,
                ..
            } = &node.kind
            else {
                continue;
            };
            let automatic_title =
                (*title_source == PaneTitleSource::Automatic).then(|| node.name.clone());

            match (content, runtime) {
                (PaneContentKind::Terminal, PaneRuntime::Terminal(view)) => {
                    let kind = if matches!(
                        status,
                        PaneStatus::Agent(..) | PaneStatus::AgentWithGoal(..)
                    ) {
                        SearchObjectKind::Agent
                    } else {
                        SearchObjectKind::Shell
                    };
                    sources.push(WorkspaceSearchSource {
                        pane_id: *pane_id,
                        kind,
                        object_name: node.name.clone(),
                        automatic_title,
                        path: None,
                        // Terminal parsing belongs exclusively to the worker;
                        // extracting this metadata here would rescan retained
                        // output on the input/UI thread.
                        last_command: None,
                        content: WorkspaceSearchContent::Terminal(
                            view.searchable_history_snapshot(),
                        ),
                    });
                }
                (PaneContentKind::Editor, PaneRuntime::Editor(editor)) => {
                    let path = editor
                        .path
                        .clone()
                        .or_else(|| self.restored_editor_paths.get(pane_id).cloned());
                    let text = editor
                        .textarea
                        .lines()
                        .iter()
                        .enumerate()
                        .map(|(line, text)| WorkspaceSearchText {
                            text: text.clone(),
                            location: SearchLocation::Editor { line },
                        })
                        .collect();
                    sources.push(WorkspaceSearchSource {
                        pane_id: *pane_id,
                        kind: SearchObjectKind::File,
                        object_name: node.name.clone(),
                        automatic_title,
                        path,
                        last_command: None,
                        content: WorkspaceSearchContent::Text(text),
                    });
                }
                (PaneContentKind::Board, PaneRuntime::Board(board)) => {
                    let path = board_storage
                        .as_ref()
                        .map(|storage| storage.path().to_path_buf());
                    let text = board
                        .columns
                        .iter()
                        .flat_map(|column| {
                            column.cards.iter().map(move |card| WorkspaceSearchText {
                                text: format!("{}\n{}\n{}", column.title, card.title, card.body),
                                location: SearchLocation::Board,
                            })
                        })
                        .collect();
                    sources.push(WorkspaceSearchSource {
                        pane_id: *pane_id,
                        kind: SearchObjectKind::Board,
                        object_name: node.name.clone(),
                        automatic_title,
                        path,
                        last_command: None,
                        content: WorkspaceSearchContent::Text(text),
                    });
                }
                _ => {}
            }
        }
        sources
    }

    /// Leaves search and navigates to the selected pane at the exact recorded
    /// editor line or terminal-history byte position.
    pub fn activate_search_result(&mut self, result: SearchResult) {
        self.mode = Mode::Normal;
        self.focus_pane(result.pane_id);
        match result.location {
            SearchLocation::Terminal { history_end_byte } => {
                if let Some(PaneRuntime::Terminal(view)) = self.panes.get_mut(&result.pane_id) {
                    view.jump_to_history_byte(history_end_byte);
                }
            }
            SearchLocation::Editor { line } => {
                if let Some(PaneRuntime::Editor(editor)) = self.panes.get_mut(&result.pane_id) {
                    editor.jump_to_line(line);
                }
            }
            SearchLocation::Board => {}
        }
        self.status_message = Some(format!("Opened search result in {}", result.object_name));
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

    /// Selects the tree's presentation order immediately and persists it in
    /// `[ui]`, shared by the settings row and context-menu submenu.
    pub fn settings_set_tree_order(&mut self, tree_order: crate::config::TreeOrder) {
        let mut ui = self.ui_settings;
        ui.tree_order = tree_order;
        self.apply_and_persist_ui_settings(ui);
    }

    /// Cycles through every registered tree-order mode.
    pub fn settings_adjust_tree_order(&mut self, direction: i32) {
        self.settings_set_tree_order(self.ui_settings.tree_order.stepped(direction));
    }

    /// Cycles among full names, single letters, chosen icons, and no agent
    /// identifier. The activity column remains visible in every mode.
    pub fn settings_adjust_agent_identifier_mode(&mut self, direction: i32) {
        let mut ui = self.ui_settings;
        ui.agent_identifiers.mode = ui.agent_identifiers.mode.stepped(direction);
        self.apply_and_persist_ui_settings(ui);
    }

    /// Selects which curated Claude glyph the tree uses in icon mode.
    pub fn settings_adjust_claude_agent_icon(&mut self, direction: i32) {
        let mut ui = self.ui_settings;
        ui.agent_identifiers.claude_icon = ui.agent_identifiers.claude_icon.stepped(direction);
        self.apply_and_persist_ui_settings(ui);
    }

    /// Selects which curated Codex glyph the tree uses in icon mode.
    pub fn settings_adjust_codex_agent_icon(&mut self, direction: i32) {
        let mut ui = self.ui_settings;
        ui.agent_identifiers.codex_icon = ui.agent_identifiers.codex_icon.stepped(direction);
        self.apply_and_persist_ui_settings(ui);
    }

    /// Selects which curated Antigravity glyph the tree uses in icon mode.
    pub fn settings_adjust_antigravity_agent_icon(&mut self, direction: i32) {
        let mut ui = self.ui_settings;
        ui.agent_identifiers.antigravity_icon =
            ui.agent_identifiers.antigravity_icon.stepped(direction);
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
            AppearanceRow::TreeOrder => self.settings_adjust_tree_order(direction),
            AppearanceRow::AgentIdentifierMode => {
                self.settings_adjust_agent_identifier_mode(direction)
            }
            AppearanceRow::ClaudeAgentIcon => self.settings_adjust_claude_agent_icon(direction),
            AppearanceRow::CodexAgentIcon => self.settings_adjust_codex_agent_icon(direction),
            AppearanceRow::AntigravityAgentIcon => {
                self.settings_adjust_antigravity_agent_icon(direction)
            }
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

    /// Whether a transition benefits from the event loop's 16 ms cadence.
    /// Spinners and pulses intentionally remain on the ordinary 50 ms tick;
    /// spatial movement needs finer frames to read as motion rather than jumps.
    pub fn is_spatial_animation_active(&self) -> bool {
        self.is_layout_animating()
            || self
                .tree_transitions
                .is_active(self.started_at.elapsed().as_millis())
    }

    /// Prunes completed entry/exit presentation state after its final frame.
    pub fn tick_tree_transitions(&mut self, now: Instant) -> bool {
        let now_offset_ms = now.saturating_duration_since(self.started_at).as_millis();
        self.tree_transitions.prune(now_offset_ms, &self.tree)
    }

    /// Whether any wall-clock-driven visual (the tree-width hover
    /// animation, a "Working" spinner, a waiting-background clock, a "Done"
    /// bell pulse, a recently-created flash, or the project-name/pane-title
    /// loading spinner) is currently active -- i.e. whether the next
    /// scheduled tick still needs to force a redraw even though no event
    /// actually changed anything. See `crate::tick::on_tick`, which is the
    /// only caller: everything else that changes visible state (input, a
    /// `ServerEvent`, a finished naming worker) already marks the frame dirty
    /// on its own.
    pub fn has_active_animation(&self) -> bool {
        if self.is_layout_animating() {
            return true;
        }
        if self.is_project_name_loading || !self.titles_loading.is_empty() {
            return true;
        }
        let elapsed_ms = self.started_at.elapsed().as_millis();
        if self.tree_transitions.is_active(elapsed_ms) {
            return true;
        }
        if tree_ui::any_recently_created_within_window(&self.recently_created, elapsed_ms) {
            return true;
        }
        self.tree.panes().any(|node| {
            matches!(
                node.kind,
                NodeKind::Pane {
                    status: PaneStatus::Agent(
                        _,
                        AgentActivity::Working
                            | AgentActivity::WaitingBackground
                            | AgentActivity::Done
                    ) | PaneStatus::AgentWithGoal(
                        _,
                        AgentActivity::Working
                            | AgentActivity::WaitingBackground
                            | AgentActivity::Done
                    ),
                    ..
                }
            ) || matches!(
                node.kind,
                NodeKind::Pane {
                    scheduled_input: Some(_),
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
        if let Some(NodeKind::Pane { status, .. }) = self.tree.get(id).map(|node| &node.kind) {
            let cleared_status = match status {
                PaneStatus::Agent(class, ilium_core::AgentActivity::Done) => Some(
                    PaneStatus::Agent(class.clone(), ilium_core::AgentActivity::Idle),
                ),
                PaneStatus::AgentWithGoal(class, ilium_core::AgentActivity::Done) => Some(
                    PaneStatus::AgentWithGoal(class.clone(), ilium_core::AgentActivity::Idle),
                ),
                _ => None,
            };
            let Some(cleared_status) = cleared_status else {
                return;
            };
            let _ = self.tree.set_pane_status(id, cleared_status);
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

    /// Selects a complete widget identifier path (does not change focus),
    /// opening every ancestor so the selection is actually visible. Virtual
    /// folder descendants use paths that cannot be reconstructed from the
    /// server-owned tree alone, so callers that already have one must use
    /// this rather than reducing it to its synthetic final ID.
    pub(crate) fn select_tree_path(&mut self, path: Vec<NodeId>) {
        let mut expanded_any_ancestor = false;
        for depth in 1..path.len() {
            expanded_any_ancestor |= self.tree_state.open(path[..depth].to_vec());
        }
        self.tree_state.select(path);
        if expanded_any_ancestor {
            self.bump_tree_version();
        }
    }

    /// Selects `id` in the tree widget's state (does not change focus),
    /// opening every ancestor so the selection is actually visible.
    pub(crate) fn select_node(&mut self, id: NodeId) {
        self.select_tree_path(self.path_to(id));
    }

    /// Toggles the selected tree path and invalidates virtual folder hit
    /// testing when the set of materialized descendants changes.
    pub(crate) fn toggle_selected_tree_node(&mut self) -> bool {
        let changed = self.tree_state.toggle_selected();
        if changed {
            self.bump_tree_version();
        }
        changed
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
                Some(leaf) => {
                    if self.tree.get(*leaf).is_some() {
                        self.path_to(*leaf) != *path
                    } else {
                        crate::tree_ui::folder_entry(&self.tree, *leaf)
                            .is_none_or(|entry| entry.identifier_path != *path)
                    }
                }
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
                Some(NodeKind::Container(_)) => Some(id),
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
    /// `claude`, `codex`, `agy`) under `parent_group`.
    pub fn request_new_command_pane(&mut self, parent_group: NodeId, command_line: String) {
        self.queue_request(ClientRequest::NewPane {
            parent_group,
            kind: ilium_ipc::NewPaneKind::Command(command_line),
        });
    }

    /// Queues one server-owned spawn-and-submit operation. Keeping the first
    /// input in the same request removes the race where the client would need
    /// a new pane id before it could address the prompt's `KeyInput` frames.
    pub fn request_new_command_pane_with_input(
        &mut self,
        parent_group: NodeId,
        command_line: String,
        initial_input: String,
    ) {
        self.queue_request(ClientRequest::NewPane {
            parent_group,
            kind: ilium_ipc::NewPaneKind::CommandWithInitialInput {
                command_line,
                initial_input,
            },
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
        self.restore_manual_tree_order_for_mutation();
        self.queue_request(ClientRequest::MoveNode { node_id, direction });
    }

    /// Queues a `ReparentNode` request -- an arbitrary move to `new_parent`
    /// at `index` (`None` appends at the end), backing both mouse
    /// drag-and-drop (`crate::mouse`) and the leader/move-mode indent/outdent
    /// keybindings (`crate::keys`). The tree itself only changes once the
    /// server confirms it via the next `TreeSnapshot`, same as every other
    /// structural request.
    pub fn request_reparent(&mut self, node_id: NodeId, new_parent: NodeId, index: Option<usize>) {
        self.restore_manual_tree_order_for_mutation();
        self.queue_request(ClientRequest::ReparentNode {
            node_id,
            new_parent,
            index,
        });
    }

    /// Every direct tree-order mutation opts out of an automatic view first.
    /// Centralizing this in the two structural request methods covers row
    /// arrows, context actions, drag/drop, and keyboard move mode uniformly.
    fn restore_manual_tree_order_for_mutation(&mut self) {
        if self.ui_settings.tree_order != TreeOrder::Manual {
            self.settings_set_tree_order(TreeOrder::Manual);
        }
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

    /// Queues a title proposed by an automatic source with no agent-session
    /// dependency (currently the plain-shell command titler). Unlike
    /// `request_rename`, the server applies it only while the pane hasn't
    /// been genuinely user-renamed, and it never marks the pane
    /// user-specified -- see `ilium_ipc::ClientRequest::SetAutomaticPaneTitle`.
    /// Session-derived LLM titles use `request_session_pane_title` instead.
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

    /// Queues an agent-session title with an expected-ID compare-and-set.
    /// The detached server, not client event ordering, decides whether the
    /// summarized session is still current when the result arrives.
    pub fn request_session_pane_title(
        &mut self,
        pane_id: NodeId,
        expected_session_id: String,
        title: String,
        short_title: Option<String>,
        title_source: ilium_core::PaneTitleSource,
    ) {
        self.queue_request(ClientRequest::SetSessionPaneTitle {
            pane_id,
            expected_session_id,
            title,
            short_title,
            title_source,
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
        let path = self.resolve_board_path(path);
        if let Some(existing_pane) = self.board_pane_for_path(&path) {
            self.focus_pane(existing_pane);
            self.status_message = Some("That board file is already open".to_string());
            return;
        }
        if let Err(error) =
            BoardPane::load(ilium_core::BoardStorage::MarkdownFile { path: path.clone() })
        {
            self.status_message = Some(format!("Could not open board: {error}"));
            return;
        }
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

    /// Creates a board beside an existing Markdown editor, first flushing
    /// any unsaved editor buffer so the board reads the exact contents the
    /// user sees instead of an older on-disk revision.
    pub fn request_board_from_markdown_editor(&mut self, editor_pane_id: NodeId) {
        let path = match self.panes.get_mut(&editor_pane_id) {
            Some(PaneRuntime::Editor(editor)) if editor.is_markdown() => {
                if editor.dirty {
                    if let Err(error) = editor.save() {
                        self.status_message = Some(format!(
                            "Could not save Markdown before opening board: {error}"
                        ));
                        return;
                    }
                }
                editor.path.clone()
            }
            _ => self.restored_editor_paths.get(&editor_pane_id).cloned(),
        };
        let Some(path) = path.filter(|path| crate::editor_pane::is_markdown_path(path)) else {
            self.status_message = Some("This editor is not backed by a Markdown file".to_string());
            return;
        };
        let parent_group = self
            .normal_group_for_node(editor_pane_id)
            .unwrap_or(ROOT_ID);
        self.request_new_markdown_board(parent_group, path);
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
        let board_directory = self.session_cwd.join(".ilium").join("boards");
        let mut suffix = 1_u32;
        let default_path = loop {
            let filename = if suffix == 1 {
                "board.md".to_string()
            } else {
                format!("board-{suffix}.md")
            };
            let candidate = board_directory.join(filename);
            if !candidate.exists() && self.board_pane_for_path(&candidate).is_none() {
                break candidate;
            }
            suffix = suffix.saturating_add(1);
        };
        self.mode = Mode::CreateBoard(CreateBoardState {
            name: TextPromptState::new("Board"),
            path: TextPromptState::new(default_path.display().to_string()),
            storage_kind: BoardStorageKind::MarkdownFile,
            editing_path: false,
        });
    }

    pub fn commit_create_board(&mut self, state: &CreateBoardState) {
        use ilium_core::BoardStorage;
        let entered_path = PathBuf::from(state.path.buf.trim());
        if entered_path.as_os_str().is_empty() {
            self.status_message = Some("Board storage path is required".to_string());
            self.mode = Mode::CreateBoard(state.clone());
            return;
        }
        let path = self.resolve_board_path(entered_path);
        let storage = match state.storage_kind {
            BoardStorageKind::Folder => BoardStorage::Folder { path },
            BoardStorageKind::MarkdownFile => BoardStorage::MarkdownFile { path },
        };
        if let Some(existing_pane) = self.board_pane_for_path(storage.path()) {
            self.focus_pane(existing_pane);
            self.status_message = Some("That board storage is already open".to_string());
            self.mode = Mode::Normal;
            return;
        }
        let board_result = if storage.path().exists() {
            BoardPane::load(storage.clone())
        } else {
            BoardPane::create(storage.clone())
        };
        if let Err(error) = board_result {
            self.status_message = Some(format!("Could not create board: {error}"));
            self.mode = Mode::CreateBoard(state.clone());
            return;
        }
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

    /// Resolves user-entered relative board storage against the canonical
    /// project directory and normalizes lexical `.`/`..` components. Existing
    /// paths additionally use the filesystem's canonical identity.
    fn resolve_board_path(&self, path: PathBuf) -> PathBuf {
        let absolute_path = if path.is_absolute() {
            path
        } else {
            self.session_cwd.join(path)
        };
        absolute_path
            .canonicalize()
            .unwrap_or_else(|_| normalize_path_lexically(&absolute_path))
    }

    /// Finds the one board pane already owning `path`, if any. Board storage
    /// is single-owner inside a session because each pane otherwise carries an
    /// independent client-local document copy capable of stale overwrites.
    fn board_pane_for_path(&self, path: &Path) -> Option<NodeId> {
        let candidate = self.resolve_board_path(path.to_path_buf());
        self.tree.panes().find_map(|node| {
            let NodeKind::Pane {
                board_storage: Some(storage),
                ..
            } = &node.kind
            else {
                return None;
            };
            (self.resolve_board_path(storage.path().to_path_buf()) == candidate).then_some(node.id)
        })
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
        let (target, title) = match board.columns.get(board.selected_column).and_then(|column| {
            board
                .selected_card
                .and_then(|selected_card| column.cards.get(selected_card))
        }) {
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
        let target = if board.selected_card.is_some() {
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

    /// Produces the stable user-facing pane category shown beside each path
    /// in the split member picker. Agent terminals are identified separately
    /// because that distinction is useful when assembling a mixed view.
    fn split_choice_kind_label(&self, pane_id: NodeId) -> &'static str {
        match self.tree.get(pane_id).map(|node| &node.kind) {
            Some(NodeKind::Pane {
                status: PaneStatus::Agent(_, _) | PaneStatus::AgentWithGoal(_, _),
                ..
            }) => "agent",
            Some(NodeKind::Pane {
                content: PaneContentKind::Terminal,
                ..
            }) => "terminal",
            Some(NodeKind::Pane {
                content: PaneContentKind::Editor,
                ..
            }) => "editor",
            Some(NodeKind::Pane {
                content: PaneContentKind::Board,
                ..
            }) => "board",
            _ => "pane",
        }
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
                label: format!(
                    "[{}] {}",
                    self.split_choice_kind_label(pane_id),
                    self.pane_tree_path_label(pane_id)
                ),
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

    /// Creates a split without opening the optional pane picker. This is
    /// deliberately a first-class path rather than a synthetic empty picker
    /// state, so the orientation dialog can truthfully offer both workflows.
    pub fn commit_empty_split(&mut self, orientation: SplitOrientation) {
        self.queue_create_split(self.split_parent_group(), orientation, Vec::new());
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
        self.queue_create_split(state.parent_group, state.orientation, pane_ids);
    }

    /// Queues the single atomic server mutation shared by the empty and
    /// member-picker creation paths, keeping naming and modal teardown in one
    /// place so the two paths cannot drift.
    fn queue_create_split(
        &mut self,
        parent_group: NodeId,
        orientation: SplitOrientation,
        pane_ids: Vec<NodeId>,
    ) {
        let name = match orientation {
            SplitOrientation::Vertical => "Vertical split",
            SplitOrientation::Horizontal => "Horizontal split",
        };
        self.queue_request(ClientRequest::CreateSplitView {
            parent_group,
            name: name.to_string(),
            orientation,
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
        let width = 36.min(self.layout.screen_area.width.max(1));
        let height = (actions.len() as u16 + 2).min(self.layout.screen_area.height.max(1));
        let max_x = self.layout.screen_area.right().saturating_sub(width);
        let max_y = self.layout.screen_area.bottom().saturating_sub(height);
        let area = Rect::new(column.min(max_x), row.min(max_y), width, height);
        self.mode = Mode::ContextMenu(ContextMenu {
            target,
            area,
            actions,
            selected_index: 0,
            tree_order_submenu: None,
        });
    }

    /// Opens the Order by submenu beside its parent row, flipping it to the
    /// left only when the terminal has no room on the right.
    pub fn open_context_tree_order_submenu(&self, menu: &mut ContextMenu) {
        let submenu_width = 32.min(self.layout.screen_area.width.max(1));
        let submenu_height =
            (TreeOrder::ALL.len() as u16 + 2).min(self.layout.screen_area.height.max(1));
        let order_row = menu
            .actions
            .iter()
            .position(|action| *action == ContextMenuAction::OrderBy)
            .unwrap_or(0) as u16;
        let preferred_x = menu.area.right();
        let x = if preferred_x.saturating_add(submenu_width) <= self.layout.screen_area.right() {
            preferred_x
        } else {
            menu.area.x.saturating_sub(submenu_width)
        };
        let preferred_y = menu.area.y.saturating_add(1).saturating_add(order_row);
        let max_y = self
            .layout
            .screen_area
            .bottom()
            .saturating_sub(submenu_height);
        let selected_index = TreeOrder::ALL
            .iter()
            .position(|tree_order| *tree_order == self.ui_settings.tree_order)
            .unwrap_or(0);
        menu.tree_order_submenu = Some(TreeOrderSubmenu {
            area: Rect::new(x, preferred_y.min(max_y), submenu_width, submenu_height),
            selected_index,
        });
    }

    /// Opens the dedicated one-line editor menu at the right-click position.
    pub fn open_editor_line_context_menu(
        &mut self,
        source: EditorSourceLine,
        column: u16,
        row: u16,
    ) {
        let actions = vec![EditorLineContextAction::CreateAgentFromLine];
        let width = 34.min(self.layout.screen_area.width.max(1));
        let height = (actions.len() as u16 + 2).min(self.layout.screen_area.height.max(1));
        let max_x = self.layout.screen_area.right().saturating_sub(width);
        let max_y = self.layout.screen_area.bottom().saturating_sub(height);
        self.mode = Mode::EditorLineContextMenu(EditorLineContextMenu {
            source,
            area: Rect::new(column.min(max_x), row.min(max_y), width, height),
            actions,
            selected_index: 0,
        });
    }

    /// Executes a source-line context action without routing through tree
    /// selection, because the originating editor may not be selected there.
    pub fn execute_editor_line_context_action(
        &mut self,
        action: EditorLineContextAction,
        source: EditorSourceLine,
    ) {
        match action {
            EditorLineContextAction::CreateAgentFromLine => {
                let parent_group = self
                    .tree
                    .parent_of(source.pane_id)
                    .unwrap_or_else(|| self.group_for_new_node());
                self.mode = Mode::CreateAgentFromLine(Box::new(CreateAgentFromLineState::new(
                    source,
                    parent_group,
                )));
            }
        }
    }

    /// Validates and queues the dialog's selected agent plus edited prompt.
    pub fn commit_create_agent_from_line(&mut self, state: Box<CreateAgentFromLineState>) {
        let initial_input = state.prompt_text();
        if initial_input.trim().is_empty() {
            self.status_message = Some("Agent task cannot be empty".to_string());
            self.mode = Mode::CreateAgentFromLine(state);
            return;
        }

        let agent_type = state.agent_type;
        self.request_new_command_pane_with_input(
            state.parent_group,
            agent_type.command_line().to_string(),
            initial_input,
        );
        self.status_message = Some(format!(
            "Creating {} agent from {}:{}",
            agent_type.label(),
            state.source.path.display(),
            state.source.line_number
        ));
        self.mode = Mode::Normal;
    }

    /// The node-appropriate command set for a context menu. `ROOT_ID`
    /// means the click landed on empty space below the tree entries
    /// rather than on a real node -- only the creation actions (plus
    /// `Settings`, which applies everywhere) apply there, none of the
    /// per-node ones.
    fn context_actions_for(&self, target: NodeId) -> Vec<ContextMenuAction> {
        if target == ROOT_ID {
            let mut actions = vec![
                ContextMenuAction::NewTerminal,
                ContextMenuAction::NewEditor,
                ContextMenuAction::NewGroup,
                ContextMenuAction::NewSplitView,
                ContextMenuAction::NewFolder,
            ];
            actions.extend(ContextMenuAction::new_agent_actions());
            actions.extend(ContextMenuAction::GLOBAL_ACTIONS);
            return actions;
        }
        let mut actions = vec![
            ContextMenuAction::NewTerminal,
            ContextMenuAction::NewEditor,
            ContextMenuAction::NewGroup,
            ContextMenuAction::NewSplitView,
            ContextMenuAction::NewFolder,
        ];
        actions.extend(ContextMenuAction::new_agent_actions());
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
            Some(Node {
                kind:
                    NodeKind::Pane {
                        content: PaneContentKind::Terminal,
                        ..
                    },
                ..
            }) => {
                actions.insert(0, ContextMenuAction::FocusPane);
                actions.insert(1, ContextMenuAction::SchedulePaneInput);
            }
            Some(Node {
                kind:
                    NodeKind::Pane {
                        content: PaneContentKind::Editor,
                        ..
                    },
                ..
            }) => {
                actions.insert(0, ContextMenuAction::FocusPane);
                let is_markdown_editor = matches!(
                    self.panes.get(&target),
                    Some(PaneRuntime::Editor(editor)) if editor.is_markdown()
                ) || self
                    .restored_editor_paths
                    .get(&target)
                    .is_some_and(|path| crate::editor_pane::is_markdown_path(path));
                if is_markdown_editor {
                    actions.insert(1, ContextMenuAction::CreateBoardFromMarkdown);
                }
            }
            Some(node) if node.is_pane() => actions.insert(0, ContextMenuAction::FocusPane),
            Some(Node {
                kind: NodeKind::Folder { .. },
                ..
            }) => actions.insert(0, ContextMenuAction::ToggleGroup),
            Some(_) => {
                return ContextMenuAction::GLOBAL_ACTIONS.to_vec();
            }
            // A stale/unrecognized target (e.g. a race with a concurrent
            // structural change) still gets a menu -- just the one action
            // that never depends on the target actually existing.
            None => return ContextMenuAction::GLOBAL_ACTIONS.to_vec(),
        }
        actions.extend([
            ContextMenuAction::Rename,
            ContextMenuAction::MoveUp,
            ContextMenuAction::MoveDown,
            ContextMenuAction::Close,
        ]);
        actions.extend(ContextMenuAction::GLOBAL_ACTIONS);
        actions
    }

    /// Executes one context-menu command, then leaves the popup unless the
    /// action opens an explicit sub-mode (e.g. Rename, the file picker).
    pub fn execute_context_action(&mut self, action: ContextMenuAction, target: NodeId) {
        self.mode = Mode::Normal;
        match action {
            ContextMenuAction::Search => self.action_open_search(),
            ContextMenuAction::FocusPane => self.focus_pane(target),
            ContextMenuAction::CreateBoardFromMarkdown => {
                self.request_board_from_markdown_editor(target)
            }
            ContextMenuAction::SchedulePaneInput => {
                self.mode =
                    Mode::SchedulePaneInput(Box::new(ScheduledInputDialogState::new(target)));
            }
            ContextMenuAction::ShowSplitView => self.show_split_view(target),
            ContextMenuAction::ToggleGroup => {
                self.toggle_selected_tree_node();
            }
            ContextMenuAction::NewTerminal => self.action_new_terminal(),
            ContextMenuAction::NewAgent(provider) => {
                self.action_new_command_pane(provider.command_line())
            }
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
            // Input handlers open this entry without leaving the parent menu;
            // direct execution is intentionally a harmless no-op.
            ContextMenuAction::OrderBy => {}
            ContextMenuAction::Restart => {
                self.request_client_exit(ClientExitReason::RestartRequested)
            }
            ContextMenuAction::Settings => self.action_open_settings(),
        }
    }

    /// Validates the complete form before queueing one atomic request. An
    /// invalid field keeps the dialog open and surfaces its exact correction
    /// in the status bar instead of silently normalizing user input.
    pub fn commit_scheduled_pane_input(&mut self, state: Box<ScheduledInputDialogState>) {
        let (delay_seconds, text, send_enter) = match state.validated_request() {
            Ok(request) => request,
            Err(message) => {
                self.status_message = Some(message);
                self.mode = Mode::SchedulePaneInput(state);
                return;
            }
        };
        self.queue_request(ClientRequest::SchedulePaneInput {
            pane_id: state.pane_id,
            delay_seconds,
            text,
            send_enter,
        });
        self.status_message = Some("Scheduled pane input".to_string());
        self.mode = Mode::Normal;
    }

    /// Creates a plain shell pane under the currently targeted group and
    /// focuses the create-group/normal dialog back to `Normal`.
    pub fn action_new_terminal(&mut self) {
        let parent = self.group_for_new_node();
        self.request_new_terminal(parent);
    }

    /// Creates a specific command-line pane (e.g. `claude`, `codex`, `agy`) under
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
        let new_path = new_path.trim();
        if new_path.is_empty() {
            self.status_message = Some("Save As: no filename given".to_string());
            return;
        }
        // Built from the already-trimmed string -- otherwise a leading space
        // survives into the saved filename, and a leading space before a
        // leading `/` defeats `is_absolute()` entirely, silently nesting an
        // intended-absolute path under `session_cwd` instead.
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
        if self
            .tree_transitions
            .presentation_tree(self.started_at.elapsed().as_millis())
            .is_some()
        {
            // The old snapshot's rows intentionally differ from the logical
            // new tree during a removal. Withhold pointer hits for this brief
            // window instead of applying a click to a shifted, invisible row.
            return None;
        }
        let items = self.tree_hit_test_cache.get_or_build(
            &self.tree,
            self.tree_version,
            self.ui_settings.tree_order,
            self.tree_state.opened(),
        );
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
                if self.tree_state.key_left() {
                    self.bump_tree_version();
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.tree_state.key_right() {
                    self.bump_tree_version();
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(id) = self.selected_node_id() {
                    if let Some(entry) = crate::tree_ui::folder_entry(&self.tree, id) {
                        if entry.is_directory {
                            self.toggle_selected_tree_node();
                        } else {
                            self.request_new_editor(
                                self.tree.parent_of(entry.root_id).unwrap_or(ROOT_ID),
                                entry.path,
                            );
                        }
                        return;
                    }
                    self.toggle_selected_tree_node();
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
        let pane_content_area = self
            .pane_viewport(id)
            .map(|viewport| viewport.content_area)
            .unwrap_or(self.layout.pane_content_area);
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
                if board.is_detail_panel_open {
                    let result = match key.code {
                        KeyCode::Esc => {
                            board.close_card_details();
                            Ok(false)
                        }
                        KeyCode::Tab => {
                            board.cycle_detail_editor_focus(false);
                            Ok(false)
                        }
                        KeyCode::BackTab => {
                            board.cycle_detail_editor_focus(true);
                            Ok(false)
                        }
                        KeyCode::Enter
                            if board.detail_editor.as_ref().is_some_and(|editor| {
                                editor.focus == crate::board::CardEditorField::Title
                            }) =>
                        {
                            board.cycle_detail_editor_focus(false);
                            Ok(false)
                        }
                        _ => board.input_detail_editor(ratatui_textarea::Input::from(
                            crossterm::event::Event::Key(key),
                        )),
                    };
                    if let Err(error) = result {
                        self.status_message = Some(error);
                    } else if result == Ok(true) {
                        self.status_message = Some("Card saved".to_string());
                    }
                    return;
                }
                let result = match key.code {
                    KeyCode::Enter => {
                        if let Some(card_index) = board.selected_card {
                            board.open_card_details(board.selected_column, card_index);
                        }
                        Ok(())
                    }
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
                    KeyCode::Up | KeyCode::Char('k')
                        if key.modifiers.contains(KeyModifiers::SHIFT) =>
                    {
                        board.move_selected_card_vertically(-1)
                    }
                    KeyCode::Down | KeyCode::Char('j')
                        if key.modifiers.contains(KeyModifiers::SHIFT) =>
                    {
                        board.move_selected_card_vertically(1)
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
                        return;
                    }
                    KeyCode::Char('c') => {
                        self.action_add_board_column(id);
                        return;
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
                        return;
                    }
                    KeyCode::Char('d') => {
                        self.action_delete_board_selection(id);
                        return;
                    }
                    _ => Ok(()),
                };
                if result.is_ok() {
                    let layout = crate::board_ui::compute_layout(
                        pane_content_area,
                        board.is_detail_panel_open,
                        board.columns.len(),
                        self.kanban_board_settings.minimum_column_width,
                    );
                    let visible_column_count = crate::board_ui::visible_column_count(
                        layout.columns_area,
                        board.columns.len(),
                        self.kanban_board_settings.minimum_column_width,
                    );
                    board.ensure_selected_column_visible(visible_column_count);
                }
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
                status: PaneStatus::Agent(class, _) | PaneStatus::AgentWithGoal(class, _),
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
            crossterm::event::MouseEventKind::Down(
                crossterm::event::MouseButton::Left | crossterm::event::MouseButton::Right
            )
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
        let preview_lines = self.kanban_board_settings.card_preview_lines;
        let minimum_column_width = self.kanban_board_settings.minimum_column_width;
        let Some(PaneRuntime::Board(board)) = self.panes.get_mut(&id) else {
            return;
        };
        if board.columns.is_empty() {
            return;
        }
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                match crate::board_ui::hit_test(
                    board,
                    area,
                    preview_lines,
                    minimum_column_width,
                    position,
                ) {
                    Some(crate::board_ui::BoardHit::Column { column_index }) => {
                        board.select_column(column_index);
                        board.drag_source = None;
                        board.drag_target = None;
                    }
                    Some(crate::board_ui::BoardHit::Card {
                        column_index,
                        card_index,
                    }) => {
                        board.select_card(column_index, card_index);
                        board.drag_source = Some((column_index, card_index));
                        board.drag_target = None;
                    }
                    Some(crate::board_ui::BoardHit::CardCheckbox {
                        column_index,
                        card_index,
                        checkbox_index,
                    }) => {
                        board.select_card(column_index, card_index);
                        board.drag_source = None;
                        board.drag_target = None;
                        match board.toggle_card_checkbox(column_index, card_index, checkbox_index) {
                            Ok(()) => self.status_message = Some("Card saved".to_string()),
                            Err(error) => self.status_message = Some(error),
                        }
                    }
                    Some(crate::board_ui::BoardHit::HorizontalScrollbar { column_scroll }) => {
                        let layout = crate::board_ui::compute_layout(
                            area,
                            board.is_detail_panel_open,
                            board.columns.len(),
                            minimum_column_width,
                        );
                        let visible_column_count = crate::board_ui::visible_column_count(
                            layout.columns_area,
                            board.columns.len(),
                            minimum_column_width,
                        );
                        board.set_column_scroll(column_scroll, visible_column_count);
                        board.drag_source = None;
                        board.drag_target = None;
                    }
                    Some(crate::board_ui::BoardHit::DetailClose) => {
                        board.drag_source = None;
                        board.drag_target = None;
                        board.close_card_details();
                    }
                    Some(crate::board_ui::BoardHit::DetailTitle) => {
                        board.drag_source = None;
                        board.drag_target = None;
                        board.set_detail_editor_focus(crate::board::CardEditorField::Title);
                    }
                    Some(crate::board_ui::BoardHit::DetailBody) => {
                        board.drag_source = None;
                        board.drag_target = None;
                        board.set_detail_editor_focus(crate::board::CardEditorField::Body);
                    }
                    None => {
                        board.drag_source = None;
                        board.drag_target = None;
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                board.drag_target = crate::board_ui::card_drop_target(
                    board,
                    area,
                    preview_lines,
                    minimum_column_width,
                    position,
                );
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some((source_column, source_card)) = board.drag_source.take() {
                    if let Some((destination_column, insertion_index)) = board.drag_target.take() {
                        let result = board.move_dragged_card(
                            source_column,
                            source_card,
                            destination_column,
                            insertion_index,
                        );
                        if let Err(error) = result {
                            self.status_message = Some(error);
                        }
                    } else if matches!(
                        crate::board_ui::hit_test(
                            board,
                            area,
                            preview_lines,
                            minimum_column_width,
                            position,
                        ),
                        Some(crate::board_ui::BoardHit::Card {
                            column_index,
                            card_index,
                        }) if (column_index, card_index) == (source_column, source_card)
                    ) {
                        board.open_card_details(source_column, source_card);
                        let layout = crate::board_ui::compute_layout(
                            area,
                            true,
                            board.columns.len(),
                            minimum_column_width,
                        );
                        let visible_column_count = crate::board_ui::visible_column_count(
                            layout.columns_area,
                            board.columns.len(),
                            minimum_column_width,
                        );
                        board.ensure_selected_column_visible(visible_column_count);
                    }
                }
                board.drag_target = None;
            }
            MouseEventKind::ScrollUp
                if matches!(
                    crate::board_ui::hit_test(
                        board,
                        area,
                        preview_lines,
                        minimum_column_width,
                        position,
                    ),
                    Some(crate::board_ui::BoardHit::DetailBody)
                ) =>
            {
                board.scroll_detail_body(-3);
            }
            MouseEventKind::ScrollDown
                if matches!(
                    crate::board_ui::hit_test(
                        board,
                        area,
                        preview_lines,
                        minimum_column_width,
                        position,
                    ),
                    Some(crate::board_ui::BoardHit::DetailBody)
                ) =>
            {
                board.scroll_detail_body(3);
            }
            MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight => {
                let layout = crate::board_ui::compute_layout(
                    area,
                    board.is_detail_panel_open,
                    board.columns.len(),
                    minimum_column_width,
                );
                let visible_column_count = crate::board_ui::visible_column_count(
                    layout.columns_area,
                    board.columns.len(),
                    minimum_column_width,
                );
                let next_scroll = match mouse.kind {
                    MouseEventKind::ScrollLeft => board.column_scroll.saturating_sub(1),
                    MouseEventKind::ScrollRight => board.column_scroll.saturating_add(1),
                    _ => unreachable!("matched horizontal wheel events"),
                };
                board.set_column_scroll(next_scroll, visible_column_count);
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
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Right)) {
            if editor.view_mode != EditorViewMode::Source {
                self.status_message =
                    Some("Switch to Source view to select a physical file line".to_string());
                return;
            }
            let source_row = usize::from(editor.source_scroll_row())
                + usize::from(position.y.saturating_sub(chrome.content_area.y));
            let Some(line_text) = editor.textarea.lines().get(source_row).cloned() else {
                return;
            };
            let Some(path) = editor.path.clone() else {
                self.status_message = Some("This editor has no file path".to_string());
                return;
            };
            self.open_editor_line_context_menu(
                EditorSourceLine {
                    pane_id: id,
                    path,
                    line_number: source_row + 1,
                    text: line_text,
                },
                mouse.column,
                mouse.row,
            );
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
    fn waiting_background_keeps_wall_clock_animation_redraws_active() {
        let mut app = app();
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "claude", PaneContentKind::Terminal)
            .unwrap();
        app.tree
            .set_pane_status(
                pane_id,
                PaneStatus::Agent(AgentClass::Claude, AgentActivity::WaitingBackground),
            )
            .unwrap();

        assert!(app.has_active_animation());
    }

    #[test]
    fn scheduled_input_action_is_available_only_for_terminal_panes() {
        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let terminal = app
            .tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        let editor = app
            .tree
            .add_pane(group, "notes", PaneContentKind::Editor)
            .unwrap();

        assert!(app
            .context_actions_for(terminal)
            .contains(&ContextMenuAction::SchedulePaneInput));
        assert!(!app
            .context_actions_for(editor)
            .contains(&ContextMenuAction::SchedulePaneInput));
        assert!(!app
            .context_actions_for(group)
            .contains(&ContextMenuAction::SchedulePaneInput));
    }

    #[test]
    fn every_builtin_agent_is_available_from_tree_context_menus() {
        let mut app = app();
        let actions = app.context_actions_for(ROOT_ID);

        for provider in BuiltinAgentProvider::ALL {
            assert!(actions.contains(&ContextMenuAction::NewAgent(provider)));
        }

        app.execute_context_action(
            ContextMenuAction::NewAgent(BuiltinAgentProvider::Antigravity),
            ROOT_ID,
        );
        assert!(matches!(
            app.take_outbound_requests().as_slice(),
            [ClientRequest::NewPane {
                parent_group: ROOT_ID,
                kind: ilium_ipc::NewPaneKind::Command(command_line),
            }] if command_line == "agy"
        ));
    }

    #[test]
    fn markdown_editor_tree_action_creates_a_sibling_board_from_the_same_file() {
        let path = std::env::temp_dir().join(format!(
            "ilium-board-context-{}-{}.markdown",
            std::process::id(),
            crate::scheduled_input::unix_millis_now()
        ));
        std::fs::write(&path, "# Work\n\n* [ ] Keep this task\n").unwrap();

        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let editor_id = app
            .tree
            .add_pane(group, "work.markdown", PaneContentKind::Editor)
            .unwrap();
        let editor = EditorPane::load(path.clone()).unwrap();
        app.panes
            .insert(editor_id, PaneRuntime::Editor(Box::new(editor)));

        assert!(app
            .context_actions_for(editor_id)
            .contains(&ContextMenuAction::CreateBoardFromMarkdown));

        app.execute_context_action(ContextMenuAction::CreateBoardFromMarkdown, editor_id);

        assert_eq!(
            app.take_outbound_requests(),
            vec![ClientRequest::NewBoard {
                parent_group: group,
                name: path.file_stem().unwrap().to_string_lossy().into_owned(),
                storage: ilium_core::BoardStorage::MarkdownFile { path: path.clone() },
            }]
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn create_board_tree_action_is_hidden_for_non_markdown_editors() {
        let path = std::env::temp_dir().join(format!(
            "ilium-board-context-{}-{}.txt",
            std::process::id(),
            crate::scheduled_input::unix_millis_now()
        ));
        std::fs::write(&path, "not markdown").unwrap();

        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let editor_id = app
            .tree
            .add_pane(group, "notes.txt", PaneContentKind::Editor)
            .unwrap();
        let editor = EditorPane::load(path.clone()).unwrap();
        app.panes
            .insert(editor_id, PaneRuntime::Editor(Box::new(editor)));

        assert!(!app
            .context_actions_for(editor_id)
            .contains(&ContextMenuAction::CreateBoardFromMarkdown));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn every_tree_context_exposes_order_by_and_submenu_selects_the_live_mode() {
        let mut app = app();
        app.set_screen_area(Rect::new(0, 0, 100, 30));
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane = app
            .tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        app.settings_set_tree_order(TreeOrder::AgeDescending);

        for target in [ROOT_ID, group, pane] {
            assert!(app
                .context_actions_for(target)
                .contains(&ContextMenuAction::OrderBy));
        }

        app.open_context_menu(pane, 10, 4);
        let Mode::ContextMenu(mut menu) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("right-click should open the tree context menu");
        };
        app.open_context_tree_order_submenu(&mut menu);
        let submenu = menu
            .tree_order_submenu
            .expect("Order by should open an adjacent submenu");

        assert_eq!(
            TreeOrder::ALL[submenu.selected_index],
            TreeOrder::AgeDescending
        );
        assert!(submenu.area.x >= menu.area.right() || submenu.area.right() <= menu.area.x);
    }

    #[test]
    fn restart_is_global_and_requests_only_a_client_detach() {
        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane = app
            .tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();

        for target in [ROOT_ID, group, pane, NodeId(u64::MAX)] {
            assert!(app
                .context_actions_for(target)
                .contains(&ContextMenuAction::Restart));
        }

        app.execute_context_action(ContextMenuAction::Restart, pane);

        assert_eq!(app.exit_reason, Some(ClientExitReason::RestartRequested));
        assert!(matches!(
            app.take_outbound_requests().as_slice(),
            [ClientRequest::Detach]
        ));
    }

    #[test]
    fn manual_move_and_reparent_restore_and_persist_manual_tree_order() {
        let config_dir = std::env::temp_dir()
            .join("ilium-app-tree-order-tests")
            .join(format!("{:?}", std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&config_dir);
        std::fs::create_dir_all(&config_dir).unwrap();

        let mut app = app();
        app.config_dir = Some(config_dir.clone());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane = app
            .tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        app.settings_set_tree_order(TreeOrder::NameAscending);
        app.request_move(pane, ilium_core::TreeMoveDirection::Up);

        assert_eq!(app.ui_settings.tree_order, TreeOrder::Manual);
        assert!(matches!(
            app.take_outbound_requests().as_slice(),
            [ClientRequest::MoveNode { node_id, .. }] if *node_id == pane
        ));
        assert_eq!(
            crate::config::load(&config_dir).unwrap().ui.tree_order,
            TreeOrder::Manual
        );

        app.settings_set_tree_order(TreeOrder::Type);
        app.request_reparent(pane, group, Some(0));
        assert_eq!(app.ui_settings.tree_order, TreeOrder::Manual);
        assert!(matches!(
            app.take_outbound_requests().as_slice(),
            [ClientRequest::ReparentNode { node_id, .. }] if *node_id == pane
        ));

        let _ = std::fs::remove_dir_all(config_dir);
    }

    #[test]
    fn scheduled_input_dialog_commit_queues_the_complete_request() {
        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let terminal = app
            .tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        app.execute_context_action(ContextMenuAction::SchedulePaneInput, terminal);
        let Mode::SchedulePaneInput(mut state) = std::mem::replace(&mut app.mode, Mode::Normal)
        else {
            panic!("scheduled input action should open its dialog");
        };
        state.hours = TextPromptState::new("1");
        state.minutes = TextPromptState::new("2");
        state.seconds = TextPromptState::new("3");
        state.text = TextPromptState::new("cargo test");
        state.send_enter = true;

        app.commit_scheduled_pane_input(state);

        assert_eq!(
            app.take_outbound_requests(),
            vec![ClientRequest::SchedulePaneInput {
                pane_id: terminal,
                delay_seconds: 3723,
                text: "cargo test".to_string(),
                send_enter: true,
            }]
        );
        assert!(matches!(app.mode, Mode::Normal));
    }

    #[test]
    fn pending_scheduled_input_keeps_countdown_redraws_active() {
        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let terminal = app
            .tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        app.tree
            .schedule_pane_input(
                terminal,
                ilium_core::ScheduledPaneInput {
                    execute_at_unix_millis: u64::MAX,
                    text: String::new(),
                    send_enter: true,
                },
            )
            .unwrap();

        assert!(app.has_active_animation());
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
    fn new_board_dialog_skips_existing_and_already_open_default_paths() {
        let project_path = std::env::temp_dir().join(format!(
            "ilium-board-defaults-{}-{}",
            std::process::id(),
            crate::scheduled_input::unix_millis_now()
        ));
        let board_directory = project_path.join(".ilium").join("boards");
        std::fs::create_dir_all(&board_directory).unwrap();
        std::fs::write(board_directory.join("board.md"), "# Existing\n").unwrap();
        let mut app = App::new("test".to_string(), project_path.clone());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        app.tree
            .add_board(
                group,
                "Second".to_string(),
                ilium_core::BoardStorage::MarkdownFile {
                    path: board_directory.join("board-2.md"),
                },
            )
            .unwrap();

        app.open_create_board_dialog();

        let Mode::CreateBoard(state) = &app.mode else {
            panic!("new board should open its creation dialog");
        };
        assert_eq!(
            PathBuf::from(&state.path.buf),
            board_directory.join("board-3.md")
        );
        let _ = std::fs::remove_dir_all(project_path);
    }

    #[test]
    fn board_creation_resolves_relative_path_and_preflights_storage_before_request() {
        let project_path = std::env::temp_dir().join(format!(
            "ilium-board-relative-{}-{}",
            std::process::id(),
            crate::scheduled_input::unix_millis_now()
        ));
        std::fs::create_dir_all(&project_path).unwrap();
        let mut app = App::new("test".to_string(), project_path.clone());
        let state = CreateBoardState {
            name: TextPromptState::new("Sprint"),
            path: TextPromptState::new("plans/sprint.md"),
            storage_kind: BoardStorageKind::MarkdownFile,
            editing_path: true,
        };

        app.commit_create_board(&state);

        let board_path = project_path.join("plans").join("sprint.md");
        assert!(board_path.is_file());
        assert!(std::fs::read_to_string(&board_path)
            .unwrap()
            .contains("## To do"));
        assert!(matches!(
            app.take_outbound_requests().as_slice(),
            [ClientRequest::NewBoard {
                name,
                storage: ilium_core::BoardStorage::MarkdownFile { path },
                ..
            }] if name == "Sprint" && path == &board_path
        ));
        let _ = std::fs::remove_dir_all(project_path);
    }

    #[test]
    fn board_creation_rejects_an_unreadable_storage_shape_without_queuing_a_dead_pane() {
        let project_path = std::env::temp_dir().join(format!(
            "ilium-board-invalid-{}-{}",
            std::process::id(),
            crate::scheduled_input::unix_millis_now()
        ));
        let directory_path = project_path.join("not-a-markdown-file");
        std::fs::create_dir_all(&directory_path).unwrap();
        let mut app = App::new("test".to_string(), project_path.clone());
        let state = CreateBoardState {
            name: TextPromptState::new("Broken"),
            path: TextPromptState::new(directory_path.display().to_string()),
            storage_kind: BoardStorageKind::MarkdownFile,
            editing_path: true,
        };

        app.commit_create_board(&state);

        assert!(app.take_outbound_requests().is_empty());
        assert!(matches!(app.mode, Mode::CreateBoard(_)));
        assert!(app
            .status_message
            .as_deref()
            .is_some_and(|message| message.contains("Could not create board")));
        let _ = std::fs::remove_dir_all(project_path);
    }

    #[test]
    fn tree_snapshot_transitions_ignore_the_very_first_snapshot() {
        let mut app = app();
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();

        // The first snapshot after attaching (e.g. a boot-time restore of a
        // whole persisted session) must not flash every node in it.
        app.track_tree_snapshot_change_at(&tree, 0);

        assert!(app.recently_created.is_empty());
        assert!(!app.recently_created.contains_key(&group));
        assert!(!app.recently_created.contains_key(&pane_id));
    }

    #[test]
    fn tree_snapshot_transitions_flag_only_ids_absent_from_the_previous_tree() {
        let mut app = app();
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let existing_pane = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        app.track_tree_snapshot_change_at(&tree, 0);
        app.tree = tree.clone();

        let new_pane = tree
            .add_pane(group, "claude", PaneContentKind::Terminal)
            .unwrap();
        app.track_tree_snapshot_change_at(&tree, 100);

        assert!(!app.recently_created.contains_key(&existing_pane));
        assert_eq!(app.recently_created.get(&new_pane), Some(&320));
    }

    #[test]
    fn tree_snapshot_transitions_flag_every_node_from_a_multi_create_burst_independently() {
        let mut app = app();
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        app.track_tree_snapshot_change_at(&tree, 0);
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
        app.track_tree_snapshot_change_at(&tree, 50);

        assert!(app.recently_created.contains_key(&first));
        assert!(app.recently_created.contains_key(&second));
        assert!(app.recently_created.contains_key(&third_group));
    }

    #[test]
    fn removal_transition_withholds_pointer_hits_until_visual_rows_match_the_new_tree() {
        let mut app = app();
        app.set_screen_area(Rect::new(0, 0, 100, 30));
        let mut previous_tree = Tree::new();
        let group_id = previous_tree.add_group(ROOT_ID, "work").unwrap();
        let first_pane = previous_tree
            .add_pane(group_id, "first", PaneContentKind::Terminal)
            .unwrap();
        previous_tree
            .add_pane(group_id, "second", PaneContentKind::Terminal)
            .unwrap();
        app.tree = previous_tree.clone();
        app.tree_state.open(vec![group_id]);
        let mut new_tree = previous_tree.clone();
        new_tree.remove_node(first_pane).unwrap();
        app.tree_transitions
            .observe_snapshot_change(&previous_tree, &new_tree, 0);
        app.tree = new_tree;
        let list = tree_ui::list_area(app.layout.tree_area);

        assert_eq!(app.tree_node_at(Position::new(list.x, list.y + 1)), None);
    }

    #[test]
    fn prune_recently_created_drops_ids_no_longer_in_the_tree() {
        let mut app = app();
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        app.track_tree_snapshot_change_at(&tree, 0);
        app.tree = tree.clone();

        let pane_id = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        app.track_tree_snapshot_change_at(&tree, 10);
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
        app.track_tree_snapshot_change_at(&tree, 0);
        app.tree = tree.clone();

        let pane_id = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        app.track_tree_snapshot_change_at(&tree, 10);
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
    fn group_for_new_node_uses_a_selected_split_as_the_pane_container() {
        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let split = app
            .tree
            .create_split_view(group, "Vertical split", SplitOrientation::Vertical, &[])
            .unwrap();
        app.tree_state.select(vec![group, split]);

        assert_eq!(app.group_for_new_node(), split);
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
    fn split_target_reconciliation_clears_a_member_moved_by_another_client() {
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
        app.right_panel_target = RightPanelTarget::SplitView {
            split_id: split,
            active_pane_id: Some(second),
        };

        app.tree.move_node(second, group, None).unwrap();
        app.reconcile_right_panel_target();

        assert_eq!(
            app.right_panel_target,
            RightPanelTarget::SplitView {
                split_id: split,
                active_pane_id: None,
            }
        );
        assert_eq!(app.displayed_pane_ids(), vec![first]);
    }

    #[test]
    fn split_target_reconciliation_clears_a_removed_split() {
        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let split = app
            .tree
            .create_split_view(group, "Vertical split", SplitOrientation::Vertical, &[])
            .unwrap();
        app.right_panel_target = RightPanelTarget::SplitView {
            split_id: split,
            active_pane_id: None,
        };

        app.tree.remove_node(split).unwrap();
        app.reconcile_right_panel_target();

        assert_eq!(app.right_panel_target, RightPanelTarget::Empty);
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
        assert_eq!(state.choices[0].label, "[editor] work / available");
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
    fn skipping_the_member_picker_queues_an_empty_split_directly() {
        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        app.tree_state.select(vec![group]);
        app.open_create_split_dialog();

        app.commit_empty_split(SplitOrientation::Vertical);

        assert!(matches!(app.mode, Mode::Normal));
        assert_eq!(
            app.take_outbound_requests(),
            vec![ClientRequest::CreateSplitView {
                parent_group: group,
                name: "Vertical split".to_string(),
                orientation: SplitOrientation::Vertical,
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

    #[test]
    fn split_slot_click_focuses_that_member_and_uses_slot_relative_coordinates() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

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
        for pane_id in [first, second] {
            app.panes.insert(
                pane_id,
                PaneRuntime::Terminal(Box::new(TerminalView::new(24, 80))),
            );
        }
        app.right_panel_target = RightPanelTarget::SplitView {
            split_id: split,
            active_pane_id: Some(first),
        };
        app.focus = FocusTarget::Pane;
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        let second_viewport = app.pane_viewport(second).unwrap();
        app.take_outbound_requests();
        let position = Position::new(
            second_viewport.content_area.x + 5,
            second_viewport.content_area.y + 4,
        );

        app.handle_pane_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: position.x,
                row: position.y,
                modifiers: KeyModifiers::NONE,
            },
            position,
        );

        assert_eq!(app.active_pane_id(), Some(second));
        assert!(app.take_outbound_requests().iter().any(|request| {
            matches!(
                request,
                ClientRequest::MouseInput {
                    pane_id,
                    column: 5,
                    row: 4,
                    ..
                } if *pane_id == second
            )
        }));
    }

    #[test]
    fn editor_right_click_captures_the_physical_source_line_and_opens_its_menu() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let editor_id = app
            .tree
            .add_pane(group, "main.rs", PaneContentKind::Editor)
            .unwrap();
        let mut editor = EditorPane::empty();
        editor.path = Some(PathBuf::from("/work/src/main.rs"));
        editor.textarea = ratatui_textarea::TextArea::from([
            "first();",
            "create_the_agent_from_this();",
            "third();",
        ]);
        app.panes
            .insert(editor_id, PaneRuntime::Editor(Box::new(editor)));
        app.right_panel_target = RightPanelTarget::Pane { pane_id: editor_id };
        app.focus = FocusTarget::Pane;
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        app.take_outbound_requests();
        let viewport = app.pane_viewport(editor_id).unwrap();
        let content = crate::editor_chrome::compute(viewport.content_area, true).content_area;
        let position = Position::new(content.x + 5, content.y + 1);

        app.handle_pane_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Right),
                column: position.x,
                row: position.y,
                modifiers: KeyModifiers::NONE,
            },
            position,
        );

        let Mode::EditorLineContextMenu(menu) = &app.mode else {
            panic!("right-click should open a source-line context menu");
        };
        assert_eq!(menu.source.pane_id, editor_id);
        assert_eq!(menu.source.line_number, 2);
        assert_eq!(menu.source.text, "create_the_agent_from_this();");
        assert_eq!(menu.source.path, PathBuf::from("/work/src/main.rs"));
    }

    #[test]
    fn create_agent_dialog_queues_selected_command_prompt_and_submission() {
        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let editor_id = app
            .tree
            .add_pane(group, "main.rs", PaneContentKind::Editor)
            .unwrap();
        let source = EditorSourceLine {
            pane_id: editor_id,
            path: PathBuf::from("/work/src/main.rs"),
            line_number: 7,
            text: "repair_authentication();".to_string(),
        };
        app.execute_editor_line_context_action(
            EditorLineContextAction::CreateAgentFromLine,
            source,
        );
        let Mode::CreateAgentFromLine(mut state) = std::mem::replace(&mut app.mode, Mode::Normal)
        else {
            panic!("line action should open the create-agent dialog");
        };
        assert!(state.prompt_text().contains("repair_authentication();"));
        assert!(state.prompt_text().contains("/work/src/main.rs at line 7"));
        state.agent_type = crate::agent_from_line::AgentLaunchType::Codex;
        state.prompt = ratatui_textarea::TextArea::from(["/goal custom task"]);

        app.commit_create_agent_from_line(state);

        assert_eq!(
            app.take_outbound_requests(),
            vec![ClientRequest::NewPane {
                parent_group: group,
                kind: ilium_ipc::NewPaneKind::CommandWithInitialInput {
                    command_line: "codex".to_string(),
                    initial_input: "/goal custom task".to_string(),
                },
            }]
        );
        assert!(matches!(app.mode, Mode::Normal));
    }

    #[test]
    fn settings_tabs_cycle_through_kanban_board_sound_and_about() {
        assert_eq!(SettingsTab::Appearance.next(), SettingsTab::Keyboard);
        assert_eq!(SettingsTab::Keyboard.next(), SettingsTab::KanbanBoard);
        assert_eq!(SettingsTab::KanbanBoard.next(), SettingsTab::Sound);
        assert_eq!(SettingsTab::Sound.next(), SettingsTab::About);
        assert_eq!(SettingsTab::About.next(), SettingsTab::Appearance);
        assert_eq!(SettingsTab::Appearance.previous(), SettingsTab::About);
    }

    #[test]
    fn kanban_layout_settings_persist_with_bounded_adjustments() {
        let config_dir = tempfile::tempdir().unwrap();
        let mut app = app();
        app.config_dir = Some(config_dir.path().to_path_buf());

        for _ in 0..20 {
            app.settings_adjust_card_preview_lines(1);
        }
        app.settings_adjust_board_column_width(100);

        assert_eq!(app.kanban_board_settings.card_preview_lines, 10);
        assert_eq!(app.kanban_board_settings.minimum_column_width, 80);
        let persisted = crate::config::load(config_dir.path()).unwrap().kanban_board;
        assert_eq!(persisted.card_preview_lines, 10);
        assert_eq!(persisted.minimum_column_width, 80);
    }

    #[test]
    fn clicking_a_board_card_opens_scrolls_and_closes_its_detail_panel() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

        let directory = tempfile::tempdir().unwrap();
        let storage = ilium_core::BoardStorage::MarkdownFile {
            path: directory.path().join("board.md"),
        };
        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let board_id = app
            .tree
            .add_board(group, "Board".to_string(), storage.clone())
            .unwrap();
        let mut board = BoardPane::create(storage).unwrap();
        board.add_card("Detailed card".to_string()).unwrap();
        board.columns[0].cards[0].body = (0..80)
            .map(|line| format!("detail line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        app.panes
            .insert(board_id, PaneRuntime::Board(Box::new(board)));
        app.right_panel_target = RightPanelTarget::Pane { pane_id: board_id };
        app.focus = FocusTarget::Pane;
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        let board_area = app.pane_viewport(board_id).unwrap().content_area;
        let PaneRuntime::Board(board) = app.panes.get(&board_id).unwrap() else {
            unreachable!();
        };
        let columns_layout = crate::board_ui::compute_layout(board_area, false, 3, 20);
        let column_area =
            crate::board_ui::column_viewport(board, columns_layout.columns_area, 20).areas[0].1;
        let column_inner = ratatui::widgets::Block::bordered().inner(column_area);
        let card_area = crate::board_ui::card_area(column_inner, 0, 3).unwrap();
        let card_position = Position::new(card_area.x + 2, card_area.y + 2);
        let mouse = |kind, position: Position| MouseEvent {
            kind,
            column: position.x,
            row: position.y,
            modifiers: KeyModifiers::NONE,
        };

        app.handle_pane_mouse(
            mouse(MouseEventKind::Down(MouseButton::Left), card_position),
            card_position,
        );
        app.handle_pane_mouse(
            mouse(MouseEventKind::Up(MouseButton::Left), card_position),
            card_position,
        );

        let PaneRuntime::Board(board) = app.panes.get(&board_id).unwrap() else {
            panic!("board runtime should remain available");
        };
        assert!(board.is_detail_panel_open);
        let detail_area = crate::board_ui::compute_layout(board_area, true, 3, 20)
            .detail_area
            .unwrap();
        assert_eq!(detail_area.width, board_area.width / 3);

        let detail_body = crate::board_ui::detail_editor_layout(detail_area).body_area;
        let detail_position = Position::new(detail_body.x + 1, detail_body.y + 1);
        app.handle_pane_mouse(
            mouse(MouseEventKind::ScrollDown, detail_position),
            detail_position,
        );
        let PaneRuntime::Board(board) = app.panes.get(&board_id).unwrap() else {
            unreachable!();
        };
        assert_eq!(
            board.detail_editor.as_ref().unwrap().focus,
            crate::board::CardEditorField::Body
        );

        let close_position = Position::new(detail_area.right() - 2, detail_area.y);
        app.handle_pane_mouse(
            mouse(MouseEventKind::Down(MouseButton::Left), close_position),
            close_position,
        );
        let PaneRuntime::Board(board) = app.panes.get(&board_id).unwrap() else {
            unreachable!();
        };
        assert!(!board.is_detail_panel_open);
    }

    #[test]
    fn sound_changes_persist_and_queue_one_live_server_update() {
        let config_dir = std::env::temp_dir()
            .join("ilium-app-sound-settings-tests")
            .join(format!("{:?}", std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&config_dir);
        std::fs::create_dir_all(&config_dir).unwrap();

        let mut app = app();
        app.config_dir = Some(config_dir.clone());
        app.sound_discovery = ilium_sound::SoundDiscovery {
            sounds: vec![ilium_sound::SystemSound {
                path: PathBuf::from("/usr/share/sounds/complete.oga"),
                display_name: "complete".to_string(),
                collection: "freedesktop".to_string(),
            }],
            ..ilium_sound::SoundDiscovery::default()
        };
        app.settings_toggle_sound_source();

        assert_eq!(
            app.sound_settings.source,
            ilium_sound::SoundSourceKind::SoundFile
        );
        assert_eq!(
            app.sound_settings.file,
            Some(PathBuf::from("/usr/share/sounds/complete.oga"))
        );
        assert!(matches!(
            app.take_outbound_requests().as_slice(),
            [ClientRequest::UpdateSoundSettings { settings }]
                if settings == &app.sound_settings
        ));
        let persisted = crate::config::load(&config_dir).unwrap();
        assert_eq!(persisted.sound, app.sound_settings);

        let _ = std::fs::remove_dir_all(config_dir);
    }

    #[test]
    fn workspace_search_finds_terminal_history_and_activates_its_exact_location() {
        let mut app = app();
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        let mut terminal = TerminalView::new(4, 40);
        terminal.feed(b"before\r\nWORKSPACE_SEARCH_NEEDLE\r\nafter\r\n");
        app.panes
            .insert(pane_id, PaneRuntime::Terminal(Box::new(terminal)));

        let mut state = SearchState::new();
        state.query = TextPromptState::new("workspace_search_needle");
        app.refresh_search_results(&mut state);

        let result = state.selected_result().cloned().expect("terminal result");
        assert_eq!(result.pane_id, pane_id);
        assert!(matches!(result.location, SearchLocation::Terminal { .. }));

        app.activate_search_result(result);

        assert_eq!(app.active_pane_id(), Some(pane_id));
        let PaneRuntime::Terminal(terminal) = app.panes.get(&pane_id).unwrap() else {
            unreachable!();
        };
        assert!(terminal
            .with_screen(|screen| screen.contents())
            .contains("WORKSPACE_SEARCH_NEEDLE"));
    }

    #[test]
    fn workspace_search_waits_then_applies_a_worker_result_for_the_current_query() {
        let mut app = app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        let mut terminal = TerminalView::new(4, 40);
        terminal.feed(b"background WORKSPACE_DEBOUNCE_NEEDLE\r\n");
        app.panes
            .insert(pane_id, PaneRuntime::Terminal(Box::new(terminal)));
        app.action_open_search();
        let started = Instant::now();
        let Mode::Search(state) = &mut app.mode else {
            panic!("workspace finder should be open");
        };
        state.query = TextPromptState::new("workspace_debounce_needle");
        state.note_query_changed(started);

        let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(1);
        let mut workers = SearchWorkers::new(events_tx);
        assert!(!app.tick_workspace_search(
            started + search_ui::SEARCH_DEBOUNCE - std::time::Duration::from_millis(1),
            &mut workers,
        ));
        assert!(workers.is_idle());

        assert!(app.tick_workspace_search(started + search_ui::SEARCH_DEBOUNCE, &mut workers));
        let event = events_rx.blocking_recv().expect("worker result");
        workers.finish();
        assert!(app.apply_workspace_search_result(event));

        let Mode::Search(state) = &app.mode else {
            panic!("workspace finder should remain open");
        };
        assert_eq!(state.results.len(), 1);
        assert_eq!(state.results[0].pane_id, pane_id);
    }

    #[test]
    fn every_tree_context_menu_exposes_workspace_search() {
        let mut app = app();
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        app.open_context_menu(ROOT_ID, 2, 2);

        let Mode::ContextMenu(menu) = &app.mode else {
            panic!("tree context menu should open");
        };
        assert!(menu.actions.contains(&ContextMenuAction::Search));
    }
}
