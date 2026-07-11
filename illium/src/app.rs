//! The central `App` state machine: owns the session `Tree`, every pane's
//! live runtime (PTY or text buffer), focus/mode state, and the leader-key
//! input dispatch. `main.rs` only drives the event loop and terminal
//! lifecycle; `ui.rs` only reads this state to render it. All state
//! transitions happen here.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use illium_core::{
    AgentActivity, AgentClass, GroupListing, NodeId, NodeKind, PaneContentKind, PaneStatus, Tree,
    TreeMoveDirection, ROOT_ID,
};
use ratatui::layout::{Position, Rect};
use sysinfo::{Pid, System};
use tui_tree_widget::TreeState;

use crate::agent_detect;
use crate::editor_pane::{EditorPane, EditorViewMode};
use crate::explorer_overlay::ExplorerOverlay;
use crate::keymap::{self, Action};
use crate::layout::UiLayout;
use crate::modal;
use crate::term_pane::TerminalPane;
use crate::text_prompt::{self, PromptOutcome, TextPromptState};
use crate::tree_ui::{self, TreeNodeHit, TreeRowAction, TreeToolbarAction};
use crate::{editor_chrome, editor_toolbar, markdown, minimap, workspace_file};

/// Which side of the UI currently has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusTarget {
    Tree,
    Pane,
}

/// The input-handling mode. Most keys are dispatched differently
/// depending on this; see `App::handle_event`.
pub enum Mode {
    Normal,
    LeaderPending,
    Move,
    /// In-progress rename prompt for the selected node, shown as a modal
    /// (see `modal::render_text_prompt`) rather than in the status bar.
    Rename(TextPromptState),
    /// In-progress command-line prompt for `Action::RunCommand`, shown the
    /// same way as `Rename`.
    CommandPrompt(TextPromptState),
    /// In-progress "Save As" filename prompt for the editor pane `NodeId`,
    /// shown the same way as `Rename`. Pre-filled with the pane's current
    /// path (see `App::action_start_save_as`).
    SaveAs(NodeId, TextPromptState),
    Help,
    /// A file-picker overlay is open. The `NodeId` is a placeholder Editor
    /// pane node (already added to the tree) that gets filled in with the
    /// picked file once the picker closes, or removed if the user cancels.
    Explorer(Box<ExplorerOverlay>, NodeId),
    /// A mouse-anchored action menu for one tree node.
    ContextMenu(ContextMenu),
    /// The "New group" destination picker is open (see
    /// `App::open_create_group_dialog`) -- every group-creation entry point
    /// (leader key, toolbar button, right-click) funnels through this one
    /// dialog rather than each guessing a placement on its own.
    CreateGroup(CreateGroupState),
    /// A Yes/No confirmation is pending before closing `NodeId` -- only
    /// entered when closing it would actually lose something (see
    /// `App::close_confirmation_message`).
    ConfirmClose(NodeId),
}

/// Actions exposed by a right-click on a tree entry. These deliberately map
/// to the same focused-node operations as the keyboard, so neither input
/// path can drift into a different tree mutation policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextMenuAction {
    FocusPane,
    ToggleGroup,
    NewTerminal,
    NewEditor,
    NewGroup,
    Rename,
    MoveUp,
    MoveDown,
    Indent,
    Outdent,
    Close,
}

impl ContextMenuAction {
    /// The concise label rendered in the popup menu.
    pub const fn label(self) -> &'static str {
        match self {
            Self::FocusPane => "Focus pane",
            Self::ToggleGroup => "Expand / collapse",
            Self::NewTerminal => "New terminal here",
            Self::NewEditor => "New editor here",
            Self::NewGroup => "New group\u{2026}",
            Self::Rename => "Rename",
            Self::MoveUp => "Move up",
            Self::MoveDown => "Move down",
            Self::Indent => "Indent into previous group",
            Self::Outdent => "Outdent",
            Self::Close => "Close",
        }
    }
}

/// What one toolbar click needs done after `App::execute_editor_toolbar_action`
/// releases its mutable borrow of the editor pane -- some actions
/// (Save/Save As) need `&mut self.tree` or `self.mode`, which can't be
/// borrowed at the same time as `self.panes.get_mut(&id)`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ToolbarEffect {
    None,
    RebuildRendered,
    Saved,
    SaveFailed(String),
    OpenSaveAs,
}

/// State of a context menu: its tree target, screen position, and keyboard
/// or mouse selection. The renderer only reads this state; all effects stay
/// in `App`.
pub struct ContextMenu {
    pub target: NodeId,
    pub area: Rect,
    pub actions: Vec<ContextMenuAction>,
    pub selected_index: usize,
}

/// State of the "New group" destination-picker dialog (see
/// `App::open_create_group_dialog`). `destinations` is a snapshot taken when
/// the dialog opened -- it does not track further tree mutations, matching
/// every other modal in illium (a `Rename` in progress doesn't react to the
/// node being renamed by another path either).
pub struct CreateGroupState {
    pub area: Rect,
    pub destinations: Vec<GroupListing>,
    pub selected_index: usize,
    pub name: TextPromptState,
}

/// The live, non-serializable half of a pane: whatever OS/UI resource
/// actually backs it. `illium_core::Tree` only knows a pane's *logical*
/// existence and status (name, `PaneStatus`); this map is where the
/// running PTY or text buffer actually lives, keyed by the same `NodeId`.
pub enum PaneRuntime {
    Terminal(TerminalPane),
    Editor(Box<EditorPane>),
}

/// Default size a freshly spawned terminal pane starts at, before the
/// first real resize (from `main.rs`, once the actual terminal size is
/// known) corrects it.
#[cfg(test)]
const DEFAULT_PANE_ROWS: u16 = 24;
#[cfg(test)]
const DEFAULT_PANE_COLS: u16 = 80;

/// How long a captured prompt waits for a matching transcript file before
/// `tick_detection` gives up on it -- transcripts are normally written
/// within a second or two of submission, so this is generous headroom
/// rather than a tight deadline.
const PENDING_PROMPT_MATCH_TIMEOUT: Duration = Duration::from_secs(20);

/// One agent pane `App::panes_needing_title_inference` has determined is
/// ready for `main.rs` to spawn a `session_naming::infer_pane_title` worker
/// for -- everything that worker needs to run, without giving `main.rs` (or
/// its worker thread) direct access to `App`'s internal maps.
pub struct PendingTitleInference {
    pub pane_id: NodeId,
    pub agent_class: AgentClass,
    pub session_id: String,
}

pub struct App {
    pub tree: Tree,
    pub panes: HashMap<NodeId, PaneRuntime>,
    /// Volatile OS metadata for terminal panes with a currently detected
    /// agent. Kept separate from the pure tree's logical `PaneStatus`.
    pub agent_processes: HashMap<NodeId, agent_detect::AgentProcessInfo>,
    /// Prompt text captured the moment `Enter` was forwarded to a pane
    /// whose session ID was still unknown, paired with the capture time --
    /// fed to `agent_detect::extract_session_id_from_recent_prompt_match`
    /// on the next few detection ticks (see `tick_detection`) until it
    /// resolves or expires (`PENDING_PROMPT_MATCH_TIMEOUT`).
    pending_prompt_captures: HashMap<NodeId, (String, SystemTime)>,
    /// Session IDs illium already knows with certainty because it just used
    /// them to build a `codex resume <id>` / `claude --resume <id>`
    /// invocation while restoring a saved workspace (see
    /// `materialize_saved_node`). Consulted by `tick_detection` ahead of
    /// every live-detection tier: re-deriving an ID we ourselves just typed
    /// into the pane is strictly weaker evidence than the ID we typed, and
    /// for Codex in particular the live tiers can come up empty for a
    /// while after a resume (no fresh prompt yet for tier 5, and tier 3's
    /// fd scan depends on Codex still holding the rollout file open in a
    /// recognizable way). Never cleared -- a resumed session's ID never
    /// changes for the lifetime of the pane, so there is no staleness risk
    /// in keeping it around, and keeping it separate from `agent_processes`
    /// means a single tick where `identify_agent` misses (which clears
    /// `agent_processes`) can never make illium forget an ID it already
    /// knew for certain.
    known_resumed_session_ids: HashMap<NodeId, String>,
    /// Who established each terminal pane's current title -- absent means
    /// "never touched by the user or by auto-inference", which is exactly
    /// the condition `panes_needing_title_inference` looks for. Populated
    /// either by `handle_rename_event` (always `User`) or by
    /// `apply_inferred_title` (always `Auto`), and persisted through
    /// `snapshot_node`/`materialize_saved_node` so a restored pane never
    /// re-triggers inference for a title it already has.
    title_sources: HashMap<NodeId, workspace_file::TitleSource>,
    /// Terminal panes currently awaiting a `session_naming::infer_pane_title`
    /// result -- `main.rs` inserts on spawning the worker, `App` clears it
    /// on `apply_inferred_title`/`fail_title_inference`. The tree renders
    /// the shared braille spinner in place of the name for any pane in here
    /// (see `tree_ui::pane_label`).
    pub(crate) titles_loading: HashSet<NodeId>,
    /// Panes whose title inference failed this run -- kept separate from
    /// `title_sources` (which only ever records an *established* title) so
    /// `panes_needing_title_inference` doesn't retry a failing pane on
    /// every detection tick, while a fresh illium restart still gets one
    /// more attempt.
    titles_inference_failed: HashSet<NodeId>,
    pub tree_state: TreeState<NodeId>,
    pub focused_pane: Option<NodeId>,
    pub focus: FocusTarget,
    pub mode: Mode,
    pub status_message: Option<String>,
    pub should_quit: bool,
    /// Stable reference for purely visual animations in the tree.
    pub started_at: Instant,
    pub last_detect_tick: Instant,
    /// (rows, cols) of the pane *content* area last reported by `main.rs`
    /// (the real terminal size minus the tree column, status bar, and pane
    /// border). Freshly created panes spawn at this size instead of the
    /// `DEFAULT_PANE_*` fallback, so a pane created after startup doesn't
    /// wait for the next terminal resize event to match its actual box.
    pub last_known_pane_size: (u16, u16),
    /// Geometry from the last terminal-size calculation. `main.rs` updates
    /// it before dispatching events, so mouse coordinates always use the
    /// same borders and split as the renderer.
    pub layout: UiLayout,
    /// A tree node held by the left button until release. A release on a
    /// different tree node performs a structural drag-and-drop move.
    tree_drag_source: Option<NodeId>,
    /// The exact visible tree row under the pointer. This is kept in app
    /// state because rendering needs it to reveal row-level arrow controls.
    pub hovered_tree_node: Option<TreeNodeHit>,
    /// True only while the pointer is over the reserved bottom toolbar.
    pub tree_toolbar_hovered: bool,
    /// The exact creation action under the pointer, used as a compact
    /// hover caption in the tree footer.
    pub hovered_tree_toolbar_action: Option<TreeToolbarAction>,
    /// True right after `Ctrl+A` is pressed while `mode == Mode::Help`,
    /// meaning the next letter decides whether to close the help popup
    /// (only `?` does; every other letter is swallowed and Help stays
    /// open). Kept separate from `Mode` so `Mode::Help` doesn't need a
    /// sub-state just for this.
    help_leader_pending: bool,
    /// The session's project directory (from `--cwd`, defaulting to the
    /// directory illium was launched from). Every new terminal pane spawns
    /// here, and the file-picker overlay always opens rooted here too --
    /// deliberately fixed for the whole session rather than following
    /// whatever directory a shell inside a pane has `cd`'d to, so "new
    /// terminal" and "new file" always land back in the project.
    pub session_cwd: PathBuf,
    /// Durable project metadata inferred once at boot and stored separately
    /// from the volatile workspace snapshot in `.illium/config.yaml`.
    pub project_name: Option<String>,
    /// True only while the one-shot project-name worker is awaiting Kilo
    /// Gateway. The tree title renders the shared braille spinner for this.
    pub is_project_name_loading: bool,
    /// Terminal graphics-protocol capability, detected once at startup.
    /// Shared by every editor pane's Rendered-mode markdown view.
    pub markdown_picker: ratatui_image::picker::Picker,
    /// `cosmic-text` font state for rasterizing markdown headers. Kept on
    /// `App` (not per-pane) since building it is the expensive part, not
    /// using it.
    pub markdown_rasterizer: crate::markdown::raster::HeaderRasterizer,
}

impl App {
    /// Entry point: if `<session_cwd>/.illium/sessions.yml` exists (see
    /// `workspace_file`), rebuilds the tree/panes it describes --
    /// recreating groups, reopening editor files, and resuming detected
    /// agent CLIs with their CLI-specific resume syntax where one was known --
    /// instead of booting the plain single-shell default. A save file
    /// that fails to parse doesn't block startup: it falls back to the
    /// default boot and surfaces the problem as a status message rather
    /// than an error, since a corrupted `.illium/` shouldn't ever be able
    /// to stop illium from opening at all.
    #[cfg(test)]
    pub fn new(session_cwd: PathBuf) -> anyhow::Result<Self> {
        Self::new_with_project_name(session_cwd, None)
    }

    /// Starts a session with the project name already resolved by the boot
    /// workflow. Keeping inference outside `App` prevents TUI tests and
    /// pane construction from ever making an HTTP request.
    #[cfg(test)]
    pub fn new_with_project_name(
        session_cwd: PathBuf,
        project_name: Option<String>,
    ) -> anyhow::Result<Self> {
        Self::new_with_project_name_and_pane_size(
            session_cwd,
            project_name,
            DEFAULT_PANE_ROWS,
            DEFAULT_PANE_COLS,
        )
    }

    /// Starts a session with the dimensions of the pane content box already
    /// known. The initial dimensions must reach the PTY *before* a restored
    /// interactive program starts: Claude's full-screen renderer can retain
    /// the terminal geometry it observed during startup rather than filling
    /// the pane after a later resize notification.
    pub fn new_with_project_name_and_pane_size(
        session_cwd: PathBuf,
        project_name: Option<String>,
        pane_rows: u16,
        pane_cols: u16,
    ) -> anyhow::Result<Self> {
        match workspace_file::load(&session_cwd) {
            Ok(Some(saved)) => Self::from_saved_workspace_with_project_name_and_pane_size(
                session_cwd,
                saved,
                project_name,
                (pane_rows, pane_cols),
            ),
            Ok(None) => Self::with_default_pane(session_cwd, project_name, (pane_rows, pane_cols)),
            Err(err) => {
                let mut app =
                    Self::with_default_pane(session_cwd, project_name, (pane_rows, pane_cols))?;
                app.status_message = Some(format!(
                    "Could not load saved workspace ({err}); starting fresh"
                ));
                Ok(app)
            }
        }
    }

    /// Assembles a fully-initialized `App` from an already-built tree and
    /// pane map, filling in every other field with its startup default.
    /// The one thing both `with_default_pane` and `from_saved_workspace`
    /// share, so neither has to repeat this struct literal.
    fn assemble(
        cwd: PathBuf,
        tree: Tree,
        panes: HashMap<NodeId, PaneRuntime>,
        tree_state: TreeState<NodeId>,
        focused_pane: Option<NodeId>,
        project_name: Option<String>,
        pane_size: (u16, u16),
    ) -> Self {
        Self {
            tree,
            panes,
            agent_processes: HashMap::new(),
            pending_prompt_captures: HashMap::new(),
            known_resumed_session_ids: HashMap::new(),
            title_sources: HashMap::new(),
            titles_loading: HashSet::new(),
            titles_inference_failed: HashSet::new(),
            tree_state,
            focused_pane,
            focus: FocusTarget::Pane,
            mode: Mode::Normal,
            status_message: None,
            should_quit: false,
            started_at: Instant::now(),
            last_detect_tick: Instant::now(),
            help_leader_pending: false,
            last_known_pane_size: pane_size,
            layout: UiLayout::default(),
            tree_drag_source: None,
            hovered_tree_node: None,
            tree_toolbar_hovered: false,
            hovered_tree_toolbar_action: None,
            session_cwd: cwd,
            project_name,
            is_project_name_loading: false,
            // Shared once per session rather than per editor pane: the
            // graphics-protocol probe (`from_query_stdio`) talks to stdio
            // once at startup, and the font rasterizer's cosmic-text state
            // is expensive enough to build that every markdown pane reuses
            // the same instance. Falls back to half-block rendering (works
            // everywhere, no protocol needed) if the terminal doesn't
            // answer the capability query -- rendered markdown still
            // works, just without pixel-perfect images.
            markdown_picker: ratatui_image::picker::Picker::from_query_stdio()
                .unwrap_or_else(|_| ratatui_image::picker::Picker::halfblocks()),
            markdown_rasterizer: crate::markdown::raster::HeaderRasterizer::new(),
        }
    }

    /// Creates the tree, adds the default top-level group every fresh
    /// session boots with (panes are never direct children of the session
    /// root -- see `Tree::ensure_default_group`), spawns one initial
    /// terminal pane inside it running the user's `$SHELL` (falling back
    /// to `/bin/bash`) in `session_cwd`, and focuses it.
    fn with_default_pane(
        session_cwd: PathBuf,
        project_name: Option<String>,
        pane_size: (u16, u16),
    ) -> anyhow::Result<Self> {
        let (pane_rows, pane_cols) = pane_size;
        let mut tree = Tree::new();
        let default_group = tree.ensure_default_group("default");
        let pane_id = tree.add_pane(default_group, "shell", PaneContentKind::Terminal)?;
        let terminal =
            TerminalPane::spawn(&default_shell_cmd(), &session_cwd, pane_rows, pane_cols)?;

        let mut panes = HashMap::new();
        panes.insert(pane_id, PaneRuntime::Terminal(terminal));

        let mut tree_state = TreeState::default();
        tree_state.open(vec![default_group]);
        tree_state.select(vec![default_group, pane_id]);

        Ok(Self::assemble(
            session_cwd,
            tree,
            panes,
            tree_state,
            Some(pane_id),
            project_name,
            pane_size,
        ))
    }

    /// Rebuilds a tree/pane set from a loaded `workspace_file::WorkspaceFile`
    /// (see `save_workspace` for the inverse). Every node is recreated
    /// best-effort: a single pane that fails to spawn (its agent CLI
    /// missing, a file it can't reopen) doesn't abort the whole restore --
    /// it's dropped from the tree and noted in the startup status message,
    /// while every other pane still comes back.
    #[cfg(test)]
    fn from_saved_workspace(
        session_cwd: PathBuf,
        saved: workspace_file::WorkspaceFile,
    ) -> anyhow::Result<Self> {
        Self::from_saved_workspace_with_project_name_and_pane_size(
            session_cwd,
            saved,
            None,
            (DEFAULT_PANE_ROWS, DEFAULT_PANE_COLS),
        )
    }

    fn from_saved_workspace_with_project_name_and_pane_size(
        session_cwd: PathBuf,
        saved: workspace_file::WorkspaceFile,
        project_name: Option<String>,
        pane_size: (u16, u16),
    ) -> anyhow::Result<Self> {
        let mut tree = Tree::new();
        let mut panes = HashMap::new();
        let mut acc = RestoreAccumulator {
            first_pane: None,
            notes: Vec::new(),
            known_resumed_session_ids: HashMap::new(),
            title_sources: HashMap::new(),
        };

        for node in saved.root {
            materialize_saved_node(
                &mut tree,
                &mut panes,
                ROOT_ID,
                node,
                &session_cwd,
                pane_size,
                &mut acc,
            );
        }

        let mut tree_state = TreeState::default();
        open_all_groups(&tree, ROOT_ID, &mut Vec::new(), &mut tree_state);
        if let Some(id) = acc.first_pane {
            tree_state.select(path_to_root(&tree, id));
        }

        let mut app = Self::assemble(
            session_cwd,
            tree,
            panes,
            tree_state,
            acc.first_pane,
            project_name,
            pane_size,
        );
        app.known_resumed_session_ids = acc.known_resumed_session_ids;
        app.title_sources = acc.title_sources;
        if !acc.notes.is_empty() {
            app.status_message = Some(format!(
                "Workspace restored with {} issue(s): {}",
                acc.notes.len(),
                acc.notes.join("; ")
            ));
        }
        Ok(app)
    }

    /// Updates the geometry shared by rendering, resize handling, and mouse
    /// routing. Every PTY shares that visible pane rectangle, so each is
    /// resized together; this keeps a background pane correct when it is
    /// later focused without requiring a second host-terminal resize.
    pub fn set_layout(&mut self, layout: UiLayout) {
        self.layout = layout;
        let (rows, cols) = layout.pane_content_size();
        self.set_pane_size(rows, cols);

        // Word-wrap and image sizing in Rendered mode both depend on
        // width, so a resize invalidates the focused pane's cached render
        // (only the focused one -- see `rebuild_rendered_markdown`'s doc
        // comment for the scope tradeoff this implies for other panes).
        if let Some(id) = self.focused_pane {
            let is_rendered_editor = matches!(
                self.panes.get(&id),
                Some(PaneRuntime::Editor(editor)) if editor.view_mode == EditorViewMode::Rendered
            );
            if is_rendered_editor {
                self.rebuild_rendered_markdown(id);
            }
        }
    }

    /// Clears a `Done` (finished, unseen) agent pane back to `Idle` the
    /// moment the user actually looks at it -- that's the whole point of
    /// the flag. Call this at every site that sets `focused_pane` to
    /// `Some(id)`, so the flash never lingers past the tick it takes to
    /// draw once the user has genuinely switched to the pane; a plain
    /// shell or editor pane, or an agent pane not currently `Done`, is
    /// left untouched.
    fn mark_seen(&mut self, id: NodeId) {
        if let Some(NodeKind::Pane {
            status: PaneStatus::Agent(class, AgentActivity::Done),
            ..
        }) = self.tree.get(id).map(|node| &node.kind)
        {
            let class = class.clone();
            let _ = self
                .tree
                .set_pane_status(id, PaneStatus::Agent(class, AgentActivity::Idle));
        }
    }

    /// Records the real pane-content size (called by `main.rs` at startup
    /// and on every terminal resize) and immediately resizes every terminal
    /// pane to match. Panes created later (`action_new_terminal`) read
    /// `last_known_pane_size`, so they start correctly sized without waiting
    /// for the next resize.
    pub fn set_pane_size(&mut self, rows: u16, cols: u16) {
        self.last_known_pane_size = (rows, cols);
        for runtime in self.panes.values() {
            if let PaneRuntime::Terminal(term) = runtime {
                let _ = term.resize(rows, cols);
            }
        }
    }

    /// The full identifier path (top-level down to `id`) as
    /// `tui_tree_widget::TreeState` expects it for `select`/`open`.
    fn path_to(&self, id: NodeId) -> Vec<NodeId> {
        path_to_root(&self.tree, id)
    }

    /// Selects `id` in the tree widget's state (does not change focus).
    fn select_node(&mut self, id: NodeId) {
        let path = self.path_to(id);
        // Every ancestor must be open for a programmatic selection (new
        // panes, drag drops, context actions) to remain visible on screen.
        for depth in 1..path.len() {
            self.tree_state.open(path[..depth].to_vec());
        }
        self.tree_state.select(path);
    }

    /// The currently selected node, if any (last element of the selected
    /// path -- see `tui_tree_widget::TreeState`).
    fn selected_node_id(&self) -> Option<NodeId> {
        self.tree_state.selected().last().copied()
    }

    /// The group a newly created node should be added under: an explicitly
    /// selected group (or the parent of an explicitly selected pane) takes
    /// priority -- this is how the tree panel and right-click "New
    /// Terminal"/"New Editor" on a specific group target it. Otherwise,
    /// falls back to the group of the currently focused/visible pane, so
    /// creating a pane from the leader-key shortcut lands it next to what's
    /// on screen. Terminals and editors are never direct children of the
    /// session root (`Tree::add_pane` rejects that), so the final fallback
    /// is the session's default group.
    fn group_for_new_node(&mut self) -> NodeId {
        let selected_target = self.selected_node_id().and_then(|id| {
            if id == ROOT_ID {
                return None;
            }
            match self.tree.get(id).map(|node| &node.kind) {
                Some(NodeKind::Group { .. }) => Some(id),
                Some(NodeKind::Pane { .. }) => self.tree.parent_of(id),
                None => None,
            }
        });
        let visible_pane_target = self.focused_pane.and_then(|id| self.tree.parent_of(id));
        selected_target
            .or(visible_pane_target)
            .unwrap_or_else(|| self.tree.ensure_default_group("default"))
    }

    /// Removes every pane under `id` (including `id` itself if it's a
    /// pane) from `self.panes`, ahead of `Tree::remove_node` deleting the
    /// tree nodes themselves. Must run *before* the tree removal, since
    /// afterward the subtree can no longer be walked.
    fn drop_pane_runtimes_under(&mut self, id: NodeId) {
        let mut stack = vec![id];
        while let Some(current) = stack.pop() {
            let Some(node) = self.tree.get(current) else {
                continue;
            };
            match &node.kind {
                NodeKind::Group { children, .. } => stack.extend(children.iter().copied()),
                NodeKind::Pane { .. } => {
                    self.panes.remove(&current);
                    self.agent_processes.remove(&current);
                }
            }
        }
    }

    /// Top-level input entry point, called once per crossterm `Event`
    /// from the main loop. Dispatches by `self.mode` first, then (for the
    /// ordinary `Normal`/`LeaderPending` modes) by `self.focus`.
    pub fn handle_event(&mut self, event: Event) -> anyhow::Result<()> {
        if let Event::Mouse(mouse) = event {
            return self.handle_mouse_event(mouse);
        }

        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Help => self.handle_help_event(&event),
            Mode::Explorer(overlay, target) => {
                self.handle_explorer_event(overlay, target, &event)?
            }
            Mode::Rename(state) => self.handle_rename_event(state, &event),
            Mode::CommandPrompt(state) => self.handle_command_prompt_event(state, &event)?,
            Mode::SaveAs(id, state) => self.handle_save_as_event(id, state, &event),
            Mode::ContextMenu(menu) => self.handle_context_menu_event(menu, &event)?,
            Mode::CreateGroup(state) => self.handle_create_group_event(state, &event),
            Mode::ConfirmClose(target) => self.handle_confirm_close_event(target, &event),
            Mode::Move => {
                self.mode = Mode::Move;
                if let Event::Key(key) = &event {
                    if is_press(key) {
                        self.handle_move_mode_key(key);
                    }
                }
            }
            Mode::Normal => {
                self.mode = Mode::Normal;
                self.handle_normal_or_leader(event)?;
            }
            Mode::LeaderPending => {
                self.mode = Mode::LeaderPending;
                self.handle_normal_or_leader(event)?;
            }
        }
        Ok(())
    }

    /// Routes a mouse event according to the most recently rendered layout.
    /// The sidebar owns its clicks and drags; the pane receives coordinates
    /// relative to its content rectangle only after it is focused.
    fn handle_mouse_event(&mut self, mouse: MouseEvent) -> anyhow::Result<()> {
        // Only actually take `self.mode` out when it's the one variant this
        // function handles. The previous unconditional `mem::replace` swapped
        // *any* mode (Explorer included) out for `Normal` here and never put
        // it back unless it happened to be `ContextMenu` -- so the very next
        // mouse event after opening the file picker (typically the same
        // click's button-release) silently destroyed it before a single
        // keypress could reach it, leaving an orphaned "(new file)" tree
        // node with no editor behind it.
        if matches!(self.mode, Mode::ContextMenu(_)) {
            let Mode::ContextMenu(menu) = std::mem::replace(&mut self.mode, Mode::Normal) else {
                unreachable!("just matched Mode::ContextMenu above");
            };
            return self.handle_context_menu_mouse(menu, mouse);
        }

        // Same "only take the mode we actually handle" reasoning as
        // `ContextMenu` just above: the create-group dialog owns click
        // selection of a destination row, so it must not fall through to
        // the generic "modal overlays ignore pointer input" branch below.
        if matches!(self.mode, Mode::CreateGroup(_)) {
            let Mode::CreateGroup(state) = std::mem::replace(&mut self.mode, Mode::Normal) else {
                unreachable!("just matched Mode::CreateGroup above");
            };
            self.handle_create_group_mouse(state, mouse);
            return Ok(());
        }

        // The file picker owns its own mouse hit-testing (row selection,
        // scroll-wheel, click-to-activate); route it there the same way
        // `Mode::ContextMenu` is routed above, instead of the generic
        // "modal overlays ignore pointer input" fallback below.
        if matches!(self.mode, Mode::Explorer(..)) {
            let Mode::Explorer(overlay, target) = std::mem::replace(&mut self.mode, Mode::Normal)
            else {
                unreachable!("just matched Mode::Explorer above");
            };
            return self.handle_explorer_event(overlay, target, &Event::Mouse(mouse));
        }

        // Modal keyboard overlays intentionally ignore pointer input until
        // they grow their own hit-testing contract.
        if matches!(
            self.mode,
            Mode::Help
                | Mode::Rename(_)
                | Mode::CommandPrompt(_)
                | Mode::SaveAs(..)
                | Mode::ConfirmClose(_)
        ) {
            return Ok(());
        }

        let position = Position::new(mouse.column, mouse.row);
        if self.layout.tree_area.contains(position) {
            self.handle_tree_mouse(mouse, position);
            return Ok(());
        }

        self.clear_tree_hover();

        if self.layout.pane_area.contains(position) {
            self.handle_pane_mouse(mouse, position)?;
            if matches!(mouse.kind, MouseEventKind::Up(MouseButton::Left)) {
                self.tree_drag_source = None;
            }
            return Ok(());
        }

        // A drag released outside the tree is a cancelled tree move. Clear
        // it here so a later unrelated click can never complete a stale drop.
        if matches!(mouse.kind, MouseEventKind::Up(MouseButton::Left)) {
            self.tree_drag_source = None;
        }
        Ok(())
    }

    /// Selects tree rows, opens a right-click menu, scrolls, and completes
    /// a drag-and-drop tree move on left-button release.
    fn handle_tree_mouse(&mut self, mouse: MouseEvent, position: Position) {
        self.focus = FocusTarget::Tree;
        self.update_tree_hover(position);

        if matches!(mouse.kind, MouseEventKind::Moved) {
            return;
        }

        if let Some(action) = tree_ui::toolbar_action_at(self.layout.tree_area, position) {
            if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                if let Err(err) = self.execute_tree_toolbar_action(action) {
                    self.status_message =
                        Some(format!("Could not create {}: {err}", action.description()));
                }
            }
            return;
        }

        if let Some(hit) = self.hovered_tree_node {
            if let Some(action) = tree_ui::row_action_at(self.layout.tree_area, hit.row, position) {
                if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                    self.handle_tree_row_action(hit.id, action);
                }
                return;
            }
        }

        match mouse.kind {
            MouseEventKind::ScrollUp => {
                self.tree_state.scroll_up(3);
            }
            MouseEventKind::ScrollDown => {
                self.tree_state.scroll_down(3);
            }
            MouseEventKind::Down(MouseButton::Right) => {
                // No node under the click means empty space below the
                // last entry -- fall back to ROOT_ID so "New group" lands
                // at the top level instead of doing nothing.
                let target = self.tree_node_at(position).unwrap_or(ROOT_ID);
                self.select_node(target);
                self.open_context_menu(target, mouse.column, mouse.row);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(id) = self.tree_node_at(position) {
                    self.select_node(id);
                    self.tree_drag_source = Some(id);
                    if matches!(
                        self.tree.get(id).map(|node| &node.kind),
                        Some(NodeKind::Group { .. })
                    ) {
                        self.tree_state.toggle_selected();
                    } else {
                        self.focused_pane = Some(id);
                        self.mark_seen(id);
                        // A pane entry is a direct focus target; clicking
                        // unused tree space still selects the tree panel.
                        self.focus = FocusTarget::Pane;
                    }
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let target = self.tree_node_at(position);
                if let (Some(source), Some(target)) = (self.tree_drag_source.take(), target) {
                    if source != target {
                        self.move_node_to_drop_target(source, target);
                    }
                }
            }
            _ => {}
        }
    }

    /// Updates the two independent hover affordances without changing tree
    /// selection: row arrows belong to actual visible entries, while the
    /// bottom strip only appears when its reserved cells are hovered.
    fn update_tree_hover(&mut self, position: Position) {
        self.tree_toolbar_hovered = tree_ui::toolbar_area(self.layout.tree_area).contains(position);
        self.hovered_tree_toolbar_action = self
            .tree_toolbar_hovered
            .then(|| tree_ui::toolbar_action_at(self.layout.tree_area, position))
            .flatten();
        self.hovered_tree_node = (!self.tree_toolbar_hovered)
            .then(|| {
                tree_ui::node_at_position(
                    &self.tree,
                    &self.tree_state,
                    self.layout.tree_area,
                    position,
                )
            })
            .flatten();
    }

    /// Clears hover-only rendering when the pointer leaves the tree.
    fn clear_tree_hover(&mut self) {
        self.hovered_tree_node = None;
        self.tree_toolbar_hovered = false;
        self.hovered_tree_toolbar_action = None;
    }

    /// Performs the action requested by a row's hover controls: the
    /// domain-level one-step move (including the pane-only cross-group
    /// boundary behavior) for the arrows, the rename modal for the
    /// edit-pen icon, or the same close/confirm flow as the context menu's
    /// "Close" for the close icon -- selecting `id` first in every case,
    /// since the hovered row need not be the currently selected one.
    fn handle_tree_row_action(&mut self, id: NodeId, action: TreeRowAction) {
        let direction = match action {
            TreeRowAction::Rename => {
                self.select_node(id);
                self.action_start_rename();
                return;
            }
            TreeRowAction::Close => {
                self.select_node(id);
                self.action_close_selected();
                return;
            }
            TreeRowAction::MoveUp => TreeMoveDirection::Up,
            TreeRowAction::MoveDown => TreeMoveDirection::Down,
        };
        match self.tree.move_node_one_step(id, direction) {
            Ok(true) => {
                self.select_node(id);
                self.clear_tree_hover();
            }
            Ok(false) => self.status_message = Some("Already at the tree boundary".to_string()),
            Err(err) => self.status_message = Some(format!("Could not move: {err}")),
        }
    }

    /// Focuses the right panel and forwards PTY mouse reports only for its
    /// content cells. Border clicks are useful for focus but never become
    /// bogus terminal coordinates.
    fn handle_pane_mouse(&mut self, mouse: MouseEvent, position: Position) -> anyhow::Result<()> {
        self.focus = FocusTarget::Pane;
        if !self.layout.pane_content_area.contains(position) {
            return Ok(());
        }

        let Some(id) = self.focused_pane else {
            return Ok(());
        };

        // Editor panes have their own chrome (toolbar/content/minimap)
        // inside the shared pane content rect; Terminal panes forward
        // raw pane-relative coordinates straight to the PTY.
        if matches!(self.panes.get(&id), Some(PaneRuntime::Editor(_))) {
            return self.handle_editor_pane_mouse(id, mouse, position);
        }

        let column = position.x.saturating_sub(self.layout.pane_content_area.x);
        let row = position.y.saturating_sub(self.layout.pane_content_area.y);
        if let Some(PaneRuntime::Terminal(term)) = self.panes.get_mut(&id) {
            term.write_mouse_input(mouse, column, row)?;
        }
        Ok(())
    }

    /// Routes a mouse event that landed inside a focused editor pane: a
    /// click on the always-visible toolbar toggles that setting; a click in
    /// the minimap column jumps the editor to the corresponding source
    /// line; anything in the main content area forwards to the `TextArea`
    /// in Source mode or scrolls the rendered document in Rendered mode
    /// (which is read-only, so clicks/keys never reach the buffer).
    fn handle_editor_pane_mouse(
        &mut self,
        id: NodeId,
        mouse: MouseEvent,
        position: Position,
    ) -> anyhow::Result<()> {
        let Some(PaneRuntime::Editor(editor)) = self.panes.get(&id) else {
            return Ok(());
        };
        let chrome = editor_chrome::compute(self.layout.pane_content_area, editor.show_minimap);

        if let Some(action) = editor_toolbar::action_at(chrome.toolbar_area, editor, position) {
            if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                self.execute_editor_toolbar_action(id, action);
            }
            return Ok(());
        }

        if let Some(minimap_area) = chrome.minimap_area {
            if minimap_area.contains(position)
                && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            {
                self.handle_editor_minimap_click(id, minimap_area, position.y);
                return Ok(());
            }
        }

        if chrome.content_area.contains(position) {
            self.handle_editor_content_mouse(id, mouse, chrome.content_area)?;
        }
        Ok(())
    }

    /// A left-click inside the minimap column jumps the editor to the
    /// clicked source line: in Source mode that's a direct cursor move (the
    /// `TextArea`'s own viewport then scrolls to keep it visible, same as
    /// any other cursor move); in Rendered mode there is no cursor, so it
    /// scrolls the rendered document to the proportional position instead,
    /// inverting `ui::minimap_highlight_line`'s forward (line -> scroll)
    /// mapping the same way `minimap::line_for_click` inverts `render`'s
    /// (line -> row) bucketing.
    fn handle_editor_minimap_click(&mut self, id: NodeId, minimap_area: Rect, click_row: u16) {
        let editor_content_area = self.editor_content_area(id);
        let Some(PaneRuntime::Editor(editor)) = self.panes.get_mut(&id) else {
            return;
        };
        let total_lines = editor.textarea.lines().len();
        let relative_row = click_row.saturating_sub(minimap_area.y);
        let target_line = minimap::line_for_click(relative_row, minimap_area.height, total_lines);

        match editor.view_mode {
            EditorViewMode::Source => editor.jump_to_line(target_line),
            EditorViewMode::Rendered => {
                let Some(document) = &editor.rendered else {
                    return;
                };
                let total_height =
                    markdown::view::content_height(document, editor_content_area.width);
                let max_scroll = markdown::view::max_scroll(
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

    /// Applies one toolbar click's effect, rebuilding the rendered
    /// document only on an actual Source -> Rendered transition (mirrors
    /// `action_toggle_editor_view_mode`'s leader-key counterpart).
    fn execute_editor_toolbar_action(&mut self, id: NodeId, action: editor_toolbar::ToolbarAction) {
        let effect = match self.panes.get_mut(&id) {
            Some(PaneRuntime::Editor(editor)) => match action {
                editor_toolbar::ToolbarAction::ViewSource => {
                    if editor.view_mode == EditorViewMode::Rendered {
                        editor.toggle_view_mode();
                    }
                    ToolbarEffect::None
                }
                editor_toolbar::ToolbarAction::ViewRendered => {
                    let was_source = editor.view_mode == EditorViewMode::Source;
                    if was_source {
                        editor.toggle_view_mode();
                    }
                    if was_source && editor.view_mode == EditorViewMode::Rendered {
                        ToolbarEffect::RebuildRendered
                    } else {
                        ToolbarEffect::None
                    }
                }
                editor_toolbar::ToolbarAction::ToggleLineNumbers => {
                    editor.toggle_line_numbers();
                    ToolbarEffect::None
                }
                editor_toolbar::ToolbarAction::ToggleMinimap => {
                    editor.toggle_minimap();
                    if editor.view_mode == EditorViewMode::Rendered {
                        ToolbarEffect::RebuildRendered
                    } else {
                        ToolbarEffect::None
                    }
                }
                editor_toolbar::ToolbarAction::ToggleAutosave => {
                    editor.toggle_autosave();
                    ToolbarEffect::None
                }
                editor_toolbar::ToolbarAction::Save => match editor.save() {
                    Ok(()) => ToolbarEffect::Saved,
                    Err(err) => ToolbarEffect::SaveFailed(err.to_string()),
                },
                editor_toolbar::ToolbarAction::SaveAs => ToolbarEffect::OpenSaveAs,
            },
            _ => ToolbarEffect::None,
        };
        match effect {
            ToolbarEffect::None => {}
            ToolbarEffect::RebuildRendered => self.rebuild_rendered_markdown(id),
            ToolbarEffect::Saved => {
                let _ = self
                    .tree
                    .set_pane_status(id, PaneStatus::Editor { dirty: false });
                self.status_message = Some("Saved".to_string());
            }
            ToolbarEffect::SaveFailed(err) => {
                self.status_message = Some(format!("Save failed: {err}"));
            }
            ToolbarEffect::OpenSaveAs => self.action_start_save_as(id),
        }
    }

    /// A mouse event inside an editor pane's main content rect (past the
    /// toolbar and minimap). A left-click landing on a markdown task-list
    /// checkbox flips it (see `checkbox_at`); otherwise Source mode
    /// forwards the event to the `TextArea` exactly as before this pane
    /// grew a toolbar (the widget tracks its own last-rendered area, so
    /// absolute screen coordinates still land correctly even though that
    /// area is now smaller); Rendered mode only reacts to the scroll wheel.
    fn handle_editor_content_mouse(
        &mut self,
        id: NodeId,
        mouse: MouseEvent,
        content_area: Rect,
    ) -> anyhow::Result<()> {
        let Some(PaneRuntime::Editor(editor)) = self.panes.get_mut(&id) else {
            return Ok(());
        };
        match editor.view_mode {
            EditorViewMode::Source => {
                let clicked_checkbox =
                    matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                        .then(|| {
                            checkbox_at(
                                editor,
                                content_area,
                                Position::new(mouse.column, mouse.row),
                            )
                        })
                        .flatten();
                if let Some((row, bracket_col, checked)) = clicked_checkbox {
                    editor.toggle_checkbox(row, bracket_col, checked);
                    let _ = self.tree.set_pane_status(
                        id,
                        PaneStatus::Editor {
                            dirty: editor.dirty,
                        },
                    );
                    return Ok(());
                }

                let modified = editor.input(ratatui_textarea::Input::from(Event::Mouse(mouse)));
                if modified {
                    let _ = self.tree.set_pane_status(
                        id,
                        PaneStatus::Editor {
                            dirty: editor.dirty,
                        },
                    );
                }
            }
            EditorViewMode::Rendered => {
                let max_scroll = editor
                    .rendered
                    .as_ref()
                    .map(|document| {
                        markdown::view::max_scroll(
                            document,
                            content_area.width,
                            content_area.height,
                        )
                    })
                    .unwrap_or(0);
                match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        editor.rendered_scroll = editor.rendered_scroll.saturating_sub(3);
                    }
                    MouseEventKind::ScrollDown => {
                        editor.rendered_scroll = (editor.rendered_scroll + 3).min(max_scroll);
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// Finds the tree node rendered at a mouse coordinate in the previous
    /// draw. `TreeState` owns the row/scroll knowledge, avoiding duplicate
    /// flattening and keeping hit-testing exact for clipped trees.
    fn tree_node_at(&self, position: Position) -> Option<NodeId> {
        self.tree_state
            .rendered_at(position)
            .and_then(|path| path.last())
            .copied()
    }

    /// Moves a dragged node into a group target, or immediately after a pane
    /// target. Invalid descendant drops remain rejected by `Tree::move_node`.
    fn move_node_to_drop_target(&mut self, source: NodeId, target: NodeId) {
        let result = match self.tree.get(target).map(|node| &node.kind) {
            Some(NodeKind::Group { .. }) => self.tree.move_node(source, target, None),
            Some(NodeKind::Pane { .. }) => {
                let Some(parent) = self.tree.parent_of(target) else {
                    return;
                };
                let target_index = self
                    .tree
                    .children_of(parent)
                    .ok()
                    .and_then(|siblings| siblings.iter().position(|id| *id == target));
                let index = target_index.map(|target_index| {
                    // `Tree::move_node` removes the source before inserting
                    // it. Compensate when it was earlier in these siblings,
                    // otherwise a drop "after B" in [A, B, C] would land
                    // after C once A is removed.
                    let source_was_before_target = self.tree.parent_of(source) == Some(parent)
                        && self
                            .tree
                            .children_of(parent)
                            .ok()
                            .and_then(|siblings| siblings.iter().position(|id| *id == source))
                            .is_some_and(|source_index| source_index < target_index);
                    insertion_index_after_target(target_index, source_was_before_target)
                });
                self.tree.move_node(source, parent, index)
            }
            None => return,
        };
        match result {
            Ok(()) => self.select_node(source),
            Err(err) => self.status_message = Some(format!("Could not move: {err}")),
        }
    }

    /// Opens a popup clamped to the terminal screen so all actions remain
    /// clickable even near the bottom or right edge.
    fn open_context_menu(&mut self, target: NodeId, column: u16, row: u16) {
        let actions = self.context_actions_for(target);
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

    /// Returns the node-appropriate command set for a context menu.
    /// `ROOT_ID` means the click landed on empty space below the tree
    /// entries rather than on a real node -- only the creation actions
    /// apply there, none of the per-node ones.
    fn context_actions_for(&self, target: NodeId) -> Vec<ContextMenuAction> {
        let mut actions = vec![
            ContextMenuAction::NewTerminal,
            ContextMenuAction::NewEditor,
            ContextMenuAction::NewGroup,
        ];
        if target == ROOT_ID {
            return actions;
        }
        match self.tree.get(target).map(|node| &node.kind) {
            Some(NodeKind::Group { .. }) => actions.insert(0, ContextMenuAction::ToggleGroup),
            Some(NodeKind::Pane { .. }) => actions.insert(0, ContextMenuAction::FocusPane),
            None => return Vec::new(),
        }
        actions.extend([
            ContextMenuAction::Rename,
            ContextMenuAction::MoveUp,
            ContextMenuAction::MoveDown,
            ContextMenuAction::Indent,
            ContextMenuAction::Outdent,
            ContextMenuAction::Close,
        ]);
        actions
    }

    /// Handles an activated context-menu entry or dismisses a click outside.
    fn handle_context_menu_mouse(
        &mut self,
        mut menu: ContextMenu,
        mouse: MouseEvent,
    ) -> anyhow::Result<()> {
        if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            self.mode = Mode::ContextMenu(menu);
            return Ok(());
        }
        let position = Position::new(mouse.column, mouse.row);
        if !menu.area.contains(position) {
            self.mode = Mode::Normal;
            return Ok(());
        }
        let item_row = position.y.saturating_sub(menu.area.y.saturating_add(1)) as usize;
        if item_row >= menu.actions.len() {
            self.mode = Mode::ContextMenu(menu);
            return Ok(());
        }
        menu.selected_index = item_row;
        self.select_node(menu.target);
        self.execute_context_action(menu.actions[item_row], menu.target)
    }

    /// Keyboard fallback for the mouse menu: arrows select, Enter performs,
    /// and Esc cancels. This makes a menu opened by click fully usable when
    /// the pointer becomes unavailable.
    fn handle_context_menu_event(
        &mut self,
        mut menu: ContextMenu,
        event: &Event,
    ) -> anyhow::Result<()> {
        let Event::Key(key) = event else {
            self.mode = Mode::ContextMenu(menu);
            return Ok(());
        };
        if !is_press(key) {
            self.mode = Mode::ContextMenu(menu);
            return Ok(());
        }
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Up | KeyCode::Char('k') => {
                menu.selected_index = menu.selected_index.saturating_sub(1);
                self.mode = Mode::ContextMenu(menu);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                menu.selected_index =
                    (menu.selected_index + 1).min(menu.actions.len().saturating_sub(1));
                self.mode = Mode::ContextMenu(menu);
            }
            KeyCode::Enter => {
                self.select_node(menu.target);
                self.execute_context_action(menu.actions[menu.selected_index], menu.target)?;
            }
            _ => self.mode = Mode::ContextMenu(menu),
        }
        Ok(())
    }

    /// Executes one context command through the same domain operations as
    /// leader-key actions, then leaves the popup unless an action opens an
    /// explicit sub-mode such as Rename or the file picker.
    fn execute_context_action(
        &mut self,
        action: ContextMenuAction,
        target: NodeId,
    ) -> anyhow::Result<()> {
        self.mode = Mode::Normal;
        match action {
            ContextMenuAction::FocusPane => {
                if let Some(id) = self.selected_node_id() {
                    self.focused_pane = Some(id);
                    self.mark_seen(id);
                    self.focus = FocusTarget::Pane;
                }
            }
            ContextMenuAction::ToggleGroup => {
                self.tree_state.toggle_selected();
            }
            ContextMenuAction::NewTerminal => self.action_new_terminal()?,
            ContextMenuAction::NewEditor => self.action_new_editor()?,
            ContextMenuAction::NewGroup => {
                let preselected = self.create_group_target_for_click(target);
                self.open_create_group_dialog(preselected);
            }
            ContextMenuAction::Rename => self.action_start_rename(),
            ContextMenuAction::MoveUp => {
                if let Some(id) = self.selected_node_id() {
                    let _ = self.tree.reorder_sibling(id, -1);
                }
            }
            ContextMenuAction::MoveDown => {
                if let Some(id) = self.selected_node_id() {
                    let _ = self.tree.reorder_sibling(id, 1);
                }
            }
            ContextMenuAction::Indent => {
                if let Some(id) = self.selected_node_id() {
                    self.indent(id);
                    self.select_node(id);
                }
            }
            ContextMenuAction::Outdent => {
                if let Some(id) = self.selected_node_id() {
                    self.outdent(id);
                    self.select_node(id);
                }
            }
            ContextMenuAction::Close => self.action_close_selected(),
        }
        Ok(())
    }

    /// While `Mode::Help` is active: only `Esc`, or `Ctrl+A` followed by
    /// `?`, closes it -- every other key (including other leader letters)
    /// is swallowed and Help stays open.
    fn handle_help_event(&mut self, event: &Event) {
        self.mode = Mode::Help;
        let Event::Key(key) = event else {
            return;
        };
        if !is_press(key) {
            return;
        }

        if self.help_leader_pending {
            self.help_leader_pending = false;
            if key.code == KeyCode::Char('?') {
                self.mode = Mode::Normal;
            }
            return;
        }

        if key.code == KeyCode::Esc {
            self.mode = Mode::Normal;
        } else if keymap::is_leader_key(key) {
            self.help_leader_pending = true;
        }
    }

    /// While `Mode::Explorer` is active: forwards the event to the
    /// overlay, and on a file pick, loads it into the placeholder pane
    /// and switches focus there. `Esc` cancels and removes the
    /// placeholder pane.
    fn handle_explorer_event(
        &mut self,
        mut overlay: Box<ExplorerOverlay>,
        target: NodeId,
        event: &Event,
    ) -> anyhow::Result<()> {
        match overlay.handle(event, self.layout.screen_area) {
            Ok(Some(path)) => {
                let file_name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.to_string_lossy().into_owned());
                match EditorPane::load(path) {
                    Ok(editor) => {
                        self.panes
                            .insert(target, PaneRuntime::Editor(Box::new(editor)));
                        let _ = self.tree.rename_node(target, file_name);
                        self.focused_pane = Some(target);
                        self.focus = FocusTarget::Pane;
                        self.select_node(target);
                    }
                    Err(err) => {
                        self.status_message = Some(format!("Failed to open file: {err}"));
                        self.drop_pane_runtimes_under(target);
                        let _ = self.tree.remove_node(target);
                    }
                }
                self.mode = Mode::Normal;
            }
            Ok(None) => {
                if is_escape(event) {
                    self.drop_pane_runtimes_under(target);
                    let _ = self.tree.remove_node(target);
                    self.mode = Mode::Normal;
                } else {
                    self.mode = Mode::Explorer(overlay, target);
                }
            }
            Err(err) => {
                self.status_message = Some(format!("File picker error: {err}"));
                self.mode = Mode::Explorer(overlay, target);
            }
        }
        Ok(())
    }

    /// While `Mode::Rename(state)` is active (shown as a modal, see
    /// `modal::render_text_prompt`): `Enter` commits via
    /// `Tree::rename_node`, `Esc` cancels. Every other key is delegated to
    /// `text_prompt::handle_key`, shared with `handle_command_prompt_event`.
    fn handle_rename_event(&mut self, mut state: TextPromptState, event: &Event) {
        let Event::Key(key) = event else {
            self.mode = Mode::Rename(state);
            return;
        };
        if !is_press(key) {
            self.mode = Mode::Rename(state);
            return;
        }

        match text_prompt::handle_key(&mut state, key.code) {
            PromptOutcome::Commit => {
                if let Some(id) = self.selected_node_id() {
                    if self.tree.rename_node(id, state.buf).is_ok() {
                        // A user-typed rename is authoritative from now on:
                        // never let auto title-inference (which only ever
                        // runs when `title_sources` has no entry yet, see
                        // `panes_needing_title_inference`) touch this pane
                        // again, even if it was still in flight.
                        self.title_sources
                            .insert(id, workspace_file::TitleSource::User);
                        self.titles_loading.remove(&id);
                    }
                }
                self.mode = Mode::Normal;
            }
            PromptOutcome::Cancel => self.mode = Mode::Normal,
            PromptOutcome::Continue => self.mode = Mode::Rename(state),
        }
    }

    /// While `Mode::CommandPrompt(state)` is active: `Enter` runs the
    /// command via `action_run_command`, `Esc` cancels without spawning
    /// anything. Same shared editing behavior as `handle_rename_event`.
    fn handle_command_prompt_event(
        &mut self,
        mut state: TextPromptState,
        event: &Event,
    ) -> anyhow::Result<()> {
        let Event::Key(key) = event else {
            self.mode = Mode::CommandPrompt(state);
            return Ok(());
        };
        if !is_press(key) {
            self.mode = Mode::CommandPrompt(state);
            return Ok(());
        }

        match text_prompt::handle_key(&mut state, key.code) {
            PromptOutcome::Commit => {
                self.mode = Mode::Normal;
                if !state.buf.trim().is_empty() {
                    self.action_run_command(&state.buf)?;
                }
            }
            PromptOutcome::Cancel => self.mode = Mode::Normal,
            PromptOutcome::Continue => self.mode = Mode::CommandPrompt(state),
        }
        Ok(())
    }

    /// While `Mode::SaveAs(id, state)` is active: `Enter` writes `id`'s
    /// editor pane to the typed path via `action_save_as`, `Esc` cancels
    /// without touching the file. Same shared editing behavior as
    /// `handle_rename_event`.
    fn handle_save_as_event(&mut self, id: NodeId, mut state: TextPromptState, event: &Event) {
        let Event::Key(key) = event else {
            self.mode = Mode::SaveAs(id, state);
            return;
        };
        if !is_press(key) {
            self.mode = Mode::SaveAs(id, state);
            return;
        }

        match text_prompt::handle_key(&mut state, key.code) {
            PromptOutcome::Commit => {
                self.action_save_as(id, state.buf);
                self.mode = Mode::Normal;
            }
            PromptOutcome::Cancel => self.mode = Mode::Normal,
            PromptOutcome::Continue => self.mode = Mode::SaveAs(id, state),
        }
    }

    /// While `Mode::ConfirmClose(target)` is active: `y`/`Enter` confirms
    /// (actually closes `target`), `n`/`Esc` cancels back to `Normal`.
    fn handle_confirm_close_event(&mut self, target: NodeId, event: &Event) {
        let Event::Key(key) = event else {
            self.mode = Mode::ConfirmClose(target);
            return;
        };
        if !is_press(key) {
            self.mode = Mode::ConfirmClose(target);
            return;
        }
        match key.code {
            KeyCode::Char('y' | 'Y') | KeyCode::Enter => self.execute_close(target),
            KeyCode::Char('n' | 'N') | KeyCode::Esc => self.mode = Mode::Normal,
            _ => self.mode = Mode::ConfirmClose(target),
        }
    }

    /// While `Mode::CreateGroup(state)` is active: `Up`/`Down` move the
    /// destination selection, `Enter` confirms it (creating the group), and
    /// every other key edits the name field -- there is only one text-input
    /// surface in this dialog, so unlike `Mode::ContextMenu` there is no
    /// separate keyboard focus to toggle between typing and choosing.
    fn handle_create_group_event(&mut self, mut state: CreateGroupState, event: &Event) {
        let Event::Key(key) = event else {
            self.mode = Mode::CreateGroup(state);
            return;
        };
        if !is_press(key) {
            self.mode = Mode::CreateGroup(state);
            return;
        }
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Up => {
                state.selected_index = state.selected_index.saturating_sub(1);
                self.mode = Mode::CreateGroup(state);
            }
            KeyCode::Down => {
                state.selected_index =
                    (state.selected_index + 1).min(state.destinations.len().saturating_sub(1));
                self.mode = Mode::CreateGroup(state);
            }
            KeyCode::Enter => self.commit_create_group(state),
            _ => {
                text_prompt::handle_key(&mut state.name, key.code);
                self.mode = Mode::CreateGroup(state);
            }
        }
    }

    /// Mouse handling for the create-group dialog: clicking a destination
    /// row immediately creates the group there (matching the keyboard's
    /// `Enter`), clicking outside the popup cancels, and anything else
    /// (e.g. the name field) is a no-op -- there's no click-to-position
    /// cursor support for the name text, only typing.
    fn handle_create_group_mouse(&mut self, state: CreateGroupState, mouse: MouseEvent) {
        if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            self.mode = Mode::CreateGroup(state);
            return;
        }
        let position = Position::new(mouse.column, mouse.row);
        if !state.area.contains(position) {
            self.mode = Mode::Normal;
            return;
        }
        let layout = modal::create_group_layout(state.area);
        let window = modal::create_group_visible_window(
            state.selected_index,
            state.destinations.len(),
            modal::CREATE_GROUP_MAX_VISIBLE,
        );
        match modal::create_group_row_at(&layout, window, position) {
            Some(index) => {
                let mut state = state;
                state.selected_index = index;
                self.commit_create_group(state);
            }
            None => self.mode = Mode::CreateGroup(state),
        }
    }

    /// Creates the group at the currently selected destination, named from
    /// the dialog's name field (falling back to "group" when left blank --
    /// naming is optional, per `App::open_create_group_dialog`).
    fn commit_create_group(&mut self, state: CreateGroupState) {
        self.mode = Mode::Normal;
        let Some(destination) = state.destinations.get(state.selected_index) else {
            return;
        };
        let name = state.name.buf.trim();
        let name = if name.is_empty() { "group" } else { name };
        match self.tree.add_group(destination.id, name) {
            Ok(id) => self.select_node(id),
            Err(err) => self.status_message = Some(format!("Could not create group: {err}")),
        }
    }

    /// Opens the "New group" destination picker with `preselected` (a group
    /// id, or `ROOT_ID` for the top level) highlighted -- the single entry
    /// point every group-creation UI (leader key `g`, the tree toolbar's
    /// group button, and right-click "New group…") funnels through, so they
    /// can never drift into different placement rules again. Falls back to
    /// index `0` (the top level) if `preselected` is somehow not a group
    /// (e.g. a stale id) -- the dialog itself always has at least that one
    /// entry.
    fn open_create_group_dialog(&mut self, preselected: NodeId) {
        let destinations = self.tree.list_groups();
        let selected_index = destinations
            .iter()
            .position(|destination| destination.id == preselected)
            .unwrap_or(0);
        let area = modal::create_group_dialog_area(self.layout.screen_area, destinations.len());
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
    /// default group as a side effect; the dialog already offers "top
    /// level" as a real, always-available choice.
    fn create_group_preselect_target(&self) -> NodeId {
        let selected_target = self.selected_node_id().and_then(|id| {
            if id == ROOT_ID {
                return None;
            }
            match self.tree.get(id).map(|node| &node.kind) {
                Some(NodeKind::Group { .. }) => Some(id),
                Some(NodeKind::Pane { .. }) => self.tree.parent_of(id),
                None => None,
            }
        });
        let visible_pane_target = self.focused_pane.and_then(|id| self.tree.parent_of(id));
        selected_target.or(visible_pane_target).unwrap_or(ROOT_ID)
    }

    /// Destination to preselect when "New group…" is triggered from a
    /// right-click on a specific tree node: empty space below the tree
    /// (`target == ROOT_ID`) preselects the top level, a pane preselects
    /// its parent group, and a group preselects itself -- the same mapping
    /// `action_new_group_at` used to apply directly; now it only decides
    /// the dialog's starting highlight, since the user can still pick a
    /// different destination before confirming.
    fn create_group_target_for_click(&self, target: NodeId) -> NodeId {
        if target == ROOT_ID {
            return ROOT_ID;
        }
        match self.tree.get(target).map(|node| &node.kind) {
            Some(NodeKind::Group { .. }) => target,
            Some(NodeKind::Pane { .. }) => self.tree.parent_of(target).unwrap_or(ROOT_ID),
            None => ROOT_ID,
        }
    }

    /// While `Mode::Move` is active: arrow/hjkl keys reorder or
    /// re-parent the selected node; `Enter` or `m` exits back to Normal.
    fn handle_move_mode_key(&mut self, key: &KeyEvent) {
        let Some(id) = self.selected_node_id() else {
            return;
        };
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                let _ = self.tree.reorder_sibling(id, -1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let _ = self.tree.reorder_sibling(id, 1);
            }
            KeyCode::Left | KeyCode::Char('h') => self.outdent(id),
            KeyCode::Right | KeyCode::Char('l') => self.indent(id),
            KeyCode::Enter | KeyCode::Char('m') | KeyCode::Esc => {
                self.mode = Mode::Normal;
            }
            _ => {}
        }
        // The node's parent (and therefore its path) may have changed;
        // keep the selection pointed at it so the highlighted row follows
        // the move instead of pointing at whatever is now in its place.
        self.select_node(id);
    }

    /// Moves `id` out to become a sibling of its current parent, placed
    /// immediately after that parent (one level up the tree).
    fn outdent(&mut self, id: NodeId) {
        let Some(parent) = self.tree.parent_of(id) else {
            return;
        };
        if parent == ROOT_ID {
            return; // already top-level, nothing to outdent into
        }
        let Some(grandparent) = self.tree.parent_of(parent) else {
            return;
        };
        let index_after_parent = self
            .tree
            .children_of(grandparent)
            .ok()
            .and_then(|siblings| siblings.iter().position(|c| *c == parent))
            .map(|pos| pos + 1);
        let _ = self.tree.move_node(id, grandparent, index_after_parent);
    }

    /// Moves `id` to become the last child of its previous sibling, if
    /// that sibling is a group (one level down the tree).
    fn indent(&mut self, id: NodeId) {
        let Some(parent) = self.tree.parent_of(id) else {
            return;
        };
        let Ok(siblings) = self.tree.children_of(parent) else {
            return;
        };
        let Some(pos) = siblings.iter().position(|c| *c == id) else {
            return;
        };
        if pos == 0 {
            return; // no previous sibling to indent into
        }
        let previous_sibling = siblings[pos - 1];
        if matches!(
            self.tree.get(previous_sibling).map(|n| &n.kind),
            Some(NodeKind::Group { .. })
        ) {
            let _ = self.tree.move_node(id, previous_sibling, None);
        }
    }

    /// `Mode::Normal` / `Mode::LeaderPending` dispatch: leader-key
    /// detection and letter lookup, falling through to ordinary
    /// tree-navigation or pane-input handling otherwise.
    fn handle_normal_or_leader(&mut self, event: Event) -> anyhow::Result<()> {
        let Event::Key(key) = event else {
            return Ok(());
        };
        if !is_press(&key) {
            return Ok(());
        }

        if matches!(self.mode, Mode::LeaderPending) {
            if let KeyCode::Char(c) = key.code {
                if let Some(action) = keymap::action_for(c) {
                    self.execute_action(action)?;
                }
            }
            // Actions that open a sub-mode (Rename/MoveMode/Help/Explorer)
            // already moved `self.mode` on; only fall back to Normal if
            // nothing did.
            if matches!(self.mode, Mode::LeaderPending) {
                self.mode = Mode::Normal;
            }
            return Ok(());
        }

        if keymap::is_leader_key(&key) {
            self.mode = Mode::LeaderPending;
            return Ok(());
        }

        match self.focus {
            FocusTarget::Tree => self.handle_tree_key(key),
            FocusTarget::Pane => self.handle_pane_key(key)?,
        }
        Ok(())
    }

    /// Ordinary (non-leader) key handling while the tree panel has focus.
    fn handle_tree_key(&mut self, key: KeyEvent) {
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
                    if matches!(
                        self.tree.get(id).map(|n| &n.kind),
                        Some(NodeKind::Pane { .. })
                    ) {
                        self.focused_pane = Some(id);
                        self.mark_seen(id);
                        self.focus = FocusTarget::Pane;
                    }
                }
            }
            _ => {}
        }
    }

    /// Ordinary (non-leader) key handling while a pane has focus: forwards
    /// raw input to the focused pane's runtime.
    fn handle_pane_key(&mut self, key: KeyEvent) -> anyhow::Result<()> {
        let Some(id) = self.focused_pane else {
            return Ok(());
        };
        let editor_content_area = self.editor_content_area(id);
        let Some(runtime) = self.panes.get_mut(&id) else {
            return Ok(());
        };
        match runtime {
            PaneRuntime::Terminal(term) => {
                // Captured *before* forwarding the keystroke, while the
                // submitted line is still on screen -- see
                // `pending_prompt_captures`'s doc comment and tier 5 in
                // `agent_detect::extract_session_id`'s doc comment.
                if key.code == KeyCode::Enter
                    && is_press(&key)
                    && self
                        .agent_processes
                        .get(&id)
                        .is_some_and(|process| process.session_id.is_none())
                {
                    if let Some(text) =
                        agent_detect::capture_submitted_prompt_text(&term.screen_text())
                    {
                        self.pending_prompt_captures
                            .insert(id, (text, SystemTime::now()));
                    }
                }
                if let Some(bytes) = encode_key_for_terminal(&key) {
                    term.write_input(&bytes)?;
                }
            }
            PaneRuntime::Editor(editor) if editor.view_mode == EditorViewMode::Source => {
                let modified = editor.input(ratatui_textarea::Input::from(Event::Key(key)));
                if modified {
                    let _ = self.tree.set_pane_status(
                        id,
                        PaneStatus::Editor {
                            dirty: editor.dirty,
                        },
                    );
                }
            }
            // Rendered mode is read-only (it's a preview, not a second
            // place to type) -- only scroll navigation applies.
            PaneRuntime::Editor(editor) => {
                if !is_press(&key) {
                    return Ok(());
                }
                let max_scroll = editor
                    .rendered
                    .as_ref()
                    .map(|document| {
                        markdown::view::max_scroll(
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
        }
        Ok(())
    }

    /// Runs one leader-key action. On success, `self.mode` is left either
    /// as whatever sub-mode the action opened, or unchanged (the caller
    /// resets to `Normal` when nothing else claimed it).
    fn execute_action(&mut self, action: Action) -> anyhow::Result<()> {
        match action {
            Action::NewTerminal => self.action_new_terminal()?,
            Action::NewEditor => self.action_new_editor()?,
            Action::ClosePane => self.action_close_selected(),
            Action::NewGroup => {
                let preselected = self.create_group_preselect_target();
                self.open_create_group_dialog(preselected);
            }
            Action::Rename => self.action_start_rename(),
            Action::ToggleMove => self.mode = Mode::Move,
            Action::FocusTree => self.focus = FocusTarget::Tree,
            Action::FocusPane => self.focus = FocusTarget::Pane,
            Action::Save => self.action_save_focused_editor(),
            Action::RunCommand => self.mode = Mode::CommandPrompt(TextPromptState::new("")),
            // Only reachable when Help *wasn't* already open (see
            // `handle_help_event`, which intercepts everything while it
            // is), so this always means "open it".
            Action::Help => self.mode = Mode::Help,
            Action::Quit => self.action_quit(),
            Action::ToggleEditorViewMode => self.action_toggle_editor_view_mode(),
            Action::ToggleLineNumbers => self.action_toggle_editor_line_numbers(),
            Action::ToggleMinimap => self.action_toggle_editor_minimap(),
            Action::ToggleAutosave => self.action_toggle_editor_autosave(),
        }
        Ok(())
    }

    /// Toggles the focused editor pane between Source and Rendered (a
    /// no-op for non-markdown files -- `EditorPane::toggle_view_mode`
    /// already guards that). Rebuilds the rendered document only on an
    /// actual Source -> Rendered transition, not on every leader-key
    /// press while already Rendered.
    fn action_toggle_editor_view_mode(&mut self) {
        let Some(id) = self.focused_pane else {
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

    fn action_toggle_editor_line_numbers(&mut self) {
        let Some(id) = self.focused_pane else {
            return;
        };
        if let Some(PaneRuntime::Editor(editor)) = self.panes.get_mut(&id) {
            editor.toggle_line_numbers();
        }
    }

    fn action_toggle_editor_minimap(&mut self) {
        let Some(id) = self.focused_pane else {
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

    fn action_toggle_editor_autosave(&mut self) {
        let Some(id) = self.focused_pane else {
            return;
        };
        if let Some(PaneRuntime::Editor(editor)) = self.panes.get_mut(&id) {
            editor.toggle_autosave();
        }
    }

    /// Exact content rectangle for editor `id`, after its toolbar and
    /// optional minimap have been removed. Rendering and every input path
    /// must share this geometry so their wrap and scroll math cannot drift.
    fn editor_content_area(&self, id: NodeId) -> Rect {
        let show_minimap = self.panes.get(&id).is_some_and(
            |runtime| matches!(runtime, PaneRuntime::Editor(editor) if editor.show_minimap),
        );
        editor_chrome::compute(self.layout.pane_content_area, show_minimap).content_area
    }

    /// The focused editor's content width -- what Rendered mode's word-wrap
    /// and image sizing must target while rebuilding its cached document.
    fn editor_content_width(&self) -> u16 {
        self.focused_pane
            .map(|id| self.editor_content_area(id).width)
            .unwrap_or(self.layout.pane_content_area.width)
    }

    /// Re-parses and re-rasterizes editor pane `id`'s current buffer text
    /// into `EditorPane::rendered`, at the focused pane's current content
    /// width. Called on a Source -> Rendered transition and again on
    /// resize while the focused pane is already Rendered (`set_layout`) --
    /// deliberately *not* on every focus change, so switching to a
    /// different already-Rendered pane after a resize can show it at a
    /// stale width until the next resize or view-mode toggle; see the
    /// module-level scope note in `markdown/render.rs` for why this
    /// renders synchronously rather than through a background worker.
    fn rebuild_rendered_markdown(&mut self, id: NodeId) {
        let width = self.editor_content_width();
        let Some(PaneRuntime::Editor(editor)) = self.panes.get_mut(&id) else {
            return;
        };
        if editor.view_mode != EditorViewMode::Rendered {
            return;
        }
        let Some(path) = editor.path.clone() else {
            return;
        };
        let base_dir = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.session_cwd.clone());
        let text = editor.textarea.lines().join("\n");
        let document = markdown::document::parse(&text, &base_dir);
        editor.rendered = Some(markdown::render::render(
            &document,
            &self.markdown_picker,
            &mut self.markdown_rasterizer,
            width.max(1),
        ));
        editor.rendered_width = width;
        editor.rendered_scroll = 0;
    }

    fn action_new_terminal(&mut self) -> anyhow::Result<()> {
        self.action_new_terminal_with_input("shell", None)
    }

    /// Creates a normal interactive shell pane and optionally writes the
    /// supplied initial terminal input. Agent toolbar actions include their
    /// terminating carriage return in this input, so their requested CLI
    /// starts immediately in the new shell.
    fn action_new_terminal_with_input(
        &mut self,
        pane_name: &str,
        initial_input: Option<&[u8]>,
    ) -> anyhow::Result<()> {
        let parent = self.group_for_new_node();
        let id = self
            .tree
            .add_pane(parent, pane_name, PaneContentKind::Terminal)?;
        let (rows, cols) = self.last_known_pane_size;
        let mut terminal =
            TerminalPane::spawn(&default_shell_cmd(), &self.session_cwd, rows, cols)?;
        if let Some(initial_input) = initial_input {
            terminal.write_input(initial_input)?;
        }
        self.panes.insert(id, PaneRuntime::Terminal(terminal));
        self.focused_pane = Some(id);
        self.focus = FocusTarget::Pane;
        self.select_node(id);
        Ok(())
    }

    /// Executes a bottom-toolbar creation action. Agent entries start a
    /// normal shell, write their exact command, then submit it with the
    /// terminal's standard carriage-return key sequence.
    fn execute_tree_toolbar_action(&mut self, action: TreeToolbarAction) -> anyhow::Result<()> {
        // Group is a picker, not an immediate creation -- it opens the
        // dialog and returns before the generic "Created {description}"
        // status message below, which would otherwise be a lie (nothing
        // has been created yet, and might not be if the user cancels).
        if matches!(action, TreeToolbarAction::Group) {
            let preselected = self.create_group_preselect_target();
            self.open_create_group_dialog(preselected);
            return Ok(());
        }
        match action {
            TreeToolbarAction::Shell => self.action_new_terminal()?,
            TreeToolbarAction::Claude => self.action_new_terminal_with_input(
                "claude",
                Some(&interactive_agent_input_for(&AgentClass::Claude)),
            )?,
            TreeToolbarAction::Codex => self.action_new_terminal_with_input(
                "codex",
                Some(&interactive_agent_input_for(&AgentClass::Codex)),
            )?,
            TreeToolbarAction::Editor => self.action_new_editor()?,
            TreeToolbarAction::Group => unreachable!("Group returned early above"),
        }
        self.status_message = Some(format!("Created {}", action.description()));
        Ok(())
    }

    /// Runs `command_line` (via `$SHELL -c command_line`, so ordinary
    /// shell syntax works) in a brand-new terminal pane under the
    /// selected group, and focuses it -- the `Action::RunCommand` leader
    /// shortcut's effect once the command prompt commits.
    fn action_run_command(&mut self, command_line: &str) -> anyhow::Result<()> {
        let parent = self.group_for_new_node();
        let id = self
            .tree
            .add_pane(parent, command_line, PaneContentKind::Terminal)?;
        let (rows, cols) = self.last_known_pane_size;
        let terminal = TerminalPane::spawn_shell_command(
            &default_shell_cmd(),
            command_line,
            &self.session_cwd,
            rows,
            cols,
        )?;
        self.panes.insert(id, PaneRuntime::Terminal(terminal));
        self.focused_pane = Some(id);
        self.focus = FocusTarget::Pane;
        self.select_node(id);
        Ok(())
    }

    fn action_new_editor(&mut self) -> anyhow::Result<()> {
        let parent = self.group_for_new_node();
        let overlay = ExplorerOverlay::open_at(&self.session_cwd)?;
        let placeholder = self
            .tree
            .add_pane(parent, "(new file)", PaneContentKind::Editor)?;
        self.select_node(placeholder);
        self.mode = Mode::Explorer(Box::new(overlay), placeholder);
        Ok(())
    }

    /// Entry point for closing the selected node (leader `x`, right-click
    /// Close): closes immediately when there's nothing to lose, otherwise
    /// opens `Mode::ConfirmClose` and waits for the user's answer.
    fn action_close_selected(&mut self) {
        let Some(id) = self.selected_node_id() else {
            return;
        };
        if id == ROOT_ID {
            self.status_message = Some("Cannot close the root group".to_string());
            return;
        }
        match self.close_confirmation_message(id) {
            Some(_) => self.mode = Mode::ConfirmClose(id),
            None => self.execute_close(id),
        }
    }

    /// A closing confirmation is only worth interrupting the user for when
    /// it would actually lose something: unsaved editor content, or a
    /// non-empty group taking every descendant pane down with it. Closing a
    /// plain shell, an idle/saved pane, or an empty group stays instant.
    /// `pub(crate)` so `ui::draw_confirm_close` can render the same message
    /// the decision to open `Mode::ConfirmClose` was based on.
    pub(crate) fn close_confirmation_message(&self, id: NodeId) -> Option<String> {
        let node = self.tree.get(id)?;
        match &node.kind {
            NodeKind::Pane {
                status: PaneStatus::Editor { dirty: true },
                ..
            } => Some(format!(
                "\"{}\" has unsaved changes. Close anyway?",
                node.name
            )),
            NodeKind::Group { children, .. } if !children.is_empty() => Some(format!(
                "\"{}\" contains {} item(s). Close anyway?",
                node.name,
                children.len()
            )),
            _ => None,
        }
    }

    /// Actually removes `id` (and every pane runtime under it) from the
    /// tree, unconditionally -- called either directly (nothing to confirm)
    /// or once `Mode::ConfirmClose` has been answered "yes".
    fn execute_close(&mut self, id: NodeId) {
        self.drop_pane_runtimes_under(id);
        if let Err(err) = self.tree.remove_node(id) {
            self.status_message = Some(format!("Could not close: {err}"));
            self.mode = Mode::Normal;
            return;
        }
        if self.focused_pane == Some(id) {
            self.focused_pane = None;
        }
        self.tree_state.select(Vec::new());
        self.mode = Mode::Normal;
    }

    fn action_start_rename(&mut self) {
        let Some(id) = self.selected_node_id() else {
            return;
        };
        let current_name = self
            .tree
            .get(id)
            .map(|n| n.name.clone())
            .unwrap_or_default();
        self.mode = Mode::Rename(TextPromptState::new(current_name));
    }

    fn action_save_focused_editor(&mut self) {
        let Some(id) = self.focused_pane else {
            self.status_message = Some("No pane focused".to_string());
            return;
        };
        let Some(PaneRuntime::Editor(editor)) = self.panes.get_mut(&id) else {
            self.status_message = Some("Focused pane is not an editor".to_string());
            return;
        };
        match editor.save() {
            Ok(()) => {
                let _ = self
                    .tree
                    .set_pane_status(id, PaneStatus::Editor { dirty: false });
                self.status_message = Some("Saved".to_string());
            }
            Err(err) => self.status_message = Some(format!("Save failed: {err}")),
        }
    }

    /// Opens the "Save As" filename prompt for `id`'s editor pane,
    /// pre-filled with its current path (or empty for a pane that was
    /// never saved) -- mirrors `action_start_rename`'s use of
    /// `TextPromptState`.
    fn action_start_save_as(&mut self, id: NodeId) {
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
    /// there. A relative path is resolved against `session_cwd`, matching
    /// where the file picker (`ExplorerOverlay::open_at`) is rooted. The
    /// tree node is renamed to the new file name so the sidebar and pane
    /// title immediately reflect it, same as a freshly opened file would
    /// show.
    fn action_save_as(&mut self, id: NodeId, new_path: String) {
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
        // Write to `path` before touching `editor.path` -- a failed write
        // (bad parent directory, permissions) must leave the pane pointed
        // at wherever it actually last saved successfully, not at a path
        // nothing was written to.
        match editor.save_to(&path) {
            Ok(()) => {
                editor.path = Some(path.clone());
                let _ = self
                    .tree
                    .set_pane_status(id, PaneStatus::Editor { dirty: false });
                if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                    let _ = self.tree.rename_node(id, name.to_string());
                }
                self.status_message = Some("Saved".to_string());
            }
            Err(err) => self.status_message = Some(format!("Save failed: {err}")),
        }
    }

    /// v1 scope: no unsaved-changes confirmation dialog -- quits
    /// unconditionally. A follow-up milestone can add a block+warn (or
    /// press-twice) confirmation if editor panes have unsaved changes.
    fn action_quit(&mut self) {
        self.save_workspace();
        self.should_quit = true;
    }

    /// Polled every `main.rs` loop iteration (cheap: an `Instant`
    /// comparison per editor pane unless a debounce has actually elapsed):
    /// writes any editor pane whose autosave debounce is due, then syncs
    /// `Tree::set_pane_status` for the ones that succeeded. Collected into
    /// `Vec`s first rather than touching `self.tree` mid-iteration, since
    /// that would borrow `self.tree` and `self.panes` at the same time.
    pub fn tick_autosave(&mut self) {
        let mut saved_ids = Vec::new();
        let mut last_failure: Option<String> = None;
        for (&id, runtime) in self.panes.iter_mut() {
            let PaneRuntime::Editor(editor) = runtime else {
                continue;
            };
            match editor.autosave_if_due() {
                Some(Ok(())) => saved_ids.push(id),
                Some(Err(err)) => last_failure = Some(err.to_string()),
                None => {}
            }
        }
        for id in saved_ids {
            let _ = self
                .tree
                .set_pane_status(id, PaneStatus::Editor { dirty: false });
        }
        if let Some(err) = last_failure {
            self.status_message = Some(format!("Autosave failed: {err}"));
        }
    }

    /// Periodic (every ~5s, driven by `main.rs`) agent-detection pass over
    /// every terminal pane: refreshes the shared `System` once, then
    /// re-classifies each pane's activity/identity and writes the result
    /// back via `Tree::set_pane_status`.
    pub fn tick_detection(&mut self, system: &mut System) {
        agent_detect::refresh(system);
        // Resolved once per tick rather than per pane; only actually used
        // by the tier-5 prompt-match fallback below, and cheap either way.
        let home_dir = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf());

        let pane_ids: Vec<NodeId> = self.panes.keys().copied().collect();
        for id in pane_ids {
            let Some(PaneRuntime::Terminal(term)) = self.panes.get_mut(&id) else {
                continue;
            };

            let raw_activity = agent_detect::classify_activity(&term.screen_text());
            let mut agent_process = term
                .shell_pid()
                .and_then(|pid| agent_detect::identify_agent(&mut *system, Pid::from_u32(pid)));
            if let Some(process) = &mut agent_process {
                // Tier 0: an ID illium already knows for certain because it
                // just typed `resume <id>` / `--resume <id>` into this exact
                // pane while restoring the saved workspace -- stronger
                // evidence than anything re-derived from the live process,
                // and the only tier Codex can rely on immediately after a
                // resume (see `known_resumed_session_ids`'s doc comment).
                if process.session_id.is_none() {
                    process.session_id = self.known_resumed_session_ids.get(&id).cloned();
                }
                if process.session_id.is_none() {
                    process.session_id =
                        agent_detect::extract_session_id_from_screen(&term.screen_text());
                }
                // Tier 5: resolve against a prompt captured on Enter (see
                // `handle_pane_key`), as long as it hasn't expired.
                if process.session_id.is_none() {
                    if let Some((prompt_text, captured_at)) =
                        self.pending_prompt_captures.get(&id).cloned()
                    {
                        let expired = captured_at
                            .elapsed()
                            .is_ok_and(|elapsed| elapsed > PENDING_PROMPT_MATCH_TIMEOUT);
                        let cwd = system
                            .process(Pid::from_u32(process.pid))
                            .and_then(|p| p.cwd());
                        if expired {
                            self.pending_prompt_captures.remove(&id);
                        } else if let (Some(home), Some(cwd)) = (&home_dir, cwd) {
                            process.session_id =
                                agent_detect::extract_session_id_from_recent_prompt_match(
                                    &process.class,
                                    home,
                                    cwd,
                                    &prompt_text,
                                    captured_at,
                                );
                        }
                    }
                }
                // Sticky fallback: a session ID never changes once a pane
                // has one, but `identify_agent` re-derives it from scratch
                // every tick (tiers 1-4 are typically stable so this never
                // mattered before, but tier 5 is a one-shot event -- its
                // `pending_prompt_captures` entry is consumed the moment it
                // resolves, so without this the very next tick would find
                // nothing new and silently blank out an already-resolved
                // ID). Keep whatever `self.agent_processes` already
                // recorded if this tick's fresh lookup came up empty.
                if process.session_id.is_none() {
                    process.session_id = self
                        .agent_processes
                        .get(&id)
                        .and_then(|previous| previous.session_id.clone());
                }
                if process.session_id.is_some() {
                    self.pending_prompt_captures.remove(&id);
                }
            } else {
                self.pending_prompt_captures.remove(&id);
            }
            let status = match agent_process {
                Some(process) => {
                    let previous_activity = match self.tree.get(id).map(|node| &node.kind) {
                        Some(NodeKind::Pane {
                            status: PaneStatus::Agent(_, activity),
                            ..
                        }) => *activity,
                        _ => AgentActivity::Idle,
                    };
                    let activity = agent_detect::next_activity(
                        previous_activity,
                        raw_activity,
                        self.focused_pane == Some(id),
                    );
                    let class = process.class.clone();
                    self.agent_processes.insert(id, process);
                    PaneStatus::Agent(class, activity)
                }
                None => {
                    self.agent_processes.remove(&id);
                    PaneStatus::PlainShell
                }
            };
            let _ = self.tree.set_pane_status(id, status);

            // v1 scope: exited panes are left in place (not auto-closed) --
            // a follow-up milestone can surface/close them.
            let _ = term.has_exited();
        }

        self.save_workspace();
    }

    /// Every agent pane `main.rs` should spawn a `session_naming::infer_pane_title`
    /// worker for right now: its session ID is known, its class has a known
    /// transcript format (Claude/Codex -- `AgentClass::Other` has none), and
    /// its title has never been established (by the user, by a previous
    /// successful auto-inference, or by a still-in-flight worker) or given
    /// up on this run. Called once per detection tick, right after
    /// `tick_detection`.
    pub fn panes_needing_title_inference(&self) -> Vec<PendingTitleInference> {
        self.agent_processes
            .iter()
            .filter_map(|(&pane_id, process)| {
                let session_id = process.session_id.clone()?;
                if !matches!(process.class, AgentClass::Claude | AgentClass::Codex) {
                    return None;
                }
                if self.title_sources.contains_key(&pane_id)
                    || self.titles_loading.contains(&pane_id)
                    || self.titles_inference_failed.contains(&pane_id)
                {
                    return None;
                }
                Some(PendingTitleInference {
                    pane_id,
                    agent_class: process.class.clone(),
                    session_id,
                })
            })
            .collect()
    }

    /// Marks `pane_id` as awaiting a title-inference result -- `tree_ui`
    /// renders the shared braille spinner in place of its name until
    /// `apply_inferred_title` or `fail_title_inference` clears it.
    pub fn mark_title_inference_started(&mut self, pane_id: NodeId) {
        self.titles_loading.insert(pane_id);
    }

    /// Applies a successfully inferred title, unless the user renamed the
    /// pane themselves (or another inference already landed) while this one
    /// was still in flight -- their edit always wins.
    pub fn apply_inferred_title(&mut self, pane_id: NodeId, title: String) {
        self.titles_loading.remove(&pane_id);
        if self.title_sources.contains_key(&pane_id) {
            return;
        }
        if self.tree.rename_node(pane_id, title).is_ok() {
            self.title_sources
                .insert(pane_id, workspace_file::TitleSource::Auto);
        }
    }

    /// Records a failed inference attempt so `panes_needing_title_inference`
    /// doesn't retry it every detection tick for the rest of this run.
    pub fn fail_title_inference(&mut self, pane_id: NodeId, error: &str) {
        self.titles_loading.remove(&pane_id);
        self.titles_inference_failed.insert(pane_id);
        self.status_message = Some(format!("Could not infer a session title: {error}"));
    }

    /// Writes the current tree/pane structure to
    /// `<cwd>/.illium/sessions.yml` (see `workspace_file`), so a killed or
    /// closed illium can be reopened in the same folder and come back
    /// close to how it was left. Called after every detection tick (so a
    /// freshly resolved session ID gets persisted within a few seconds)
    /// and once more right before quitting. A failed save doesn't stop
    /// the app -- it's surfaced as a status message, not an error the
    /// user has to do anything about.
    fn save_workspace(&mut self) {
        let snapshot = workspace_file::WorkspaceFile::new(self.snapshot_children(ROOT_ID));
        if let Err(err) = workspace_file::save(&self.session_cwd, &snapshot) {
            self.status_message = Some(format!("Could not save workspace: {err}"));
        }
    }

    fn snapshot_children(&self, parent: NodeId) -> Vec<workspace_file::SavedNode> {
        let Ok(children) = self.tree.children_of(parent) else {
            return Vec::new();
        };
        children
            .iter()
            .filter_map(|&id| self.snapshot_node(id))
            .collect()
    }

    fn snapshot_node(&self, id: NodeId) -> Option<workspace_file::SavedNode> {
        let node = self.tree.get(id)?;
        match &node.kind {
            NodeKind::Group { .. } => Some(workspace_file::SavedNode::Group {
                name: node.name.clone(),
                children: self.snapshot_children(id),
            }),
            NodeKind::Pane {
                content: PaneContentKind::Terminal,
                ..
            } => {
                let agent =
                    self.agent_processes
                        .get(&id)
                        .map(|process| workspace_file::SavedAgent {
                            command: agent_command_for(&process.class),
                            session_id: process.session_id.clone(),
                        });
                Some(workspace_file::SavedNode::Terminal {
                    name: node.name.clone(),
                    agent,
                    title_source: self.title_sources.get(&id).copied(),
                })
            }
            NodeKind::Pane {
                content: PaneContentKind::Editor,
                ..
            } => {
                let path = match self.panes.get(&id) {
                    Some(PaneRuntime::Editor(editor)) => editor.path.clone(),
                    _ => None,
                };
                Some(workspace_file::SavedNode::Editor {
                    name: node.name.clone(),
                    path,
                })
            }
        }
    }
}

/// The literal command to invoke to start this agent again -- `claude`/
/// `codex` directly, or (for `AgentClass::Other`) the CLI's own recorded
/// name, which `identify_agent` only ever sets to the process name it
/// actually matched -- already the right invocation.
fn agent_command_for(class: &AgentClass) -> String {
    match class {
        AgentClass::Claude => "claude".to_string(),
        AgentClass::Codex => "codex".to_string(),
        AgentClass::Other(name) => name.clone(),
    }
}

/// Returns the command line written by the toolbar for a new agent pane.
/// Keeping agent-specific CLI syntax here makes it auditable alongside the
/// agent-specific resume construction below.
fn interactive_agent_command_for(class: &AgentClass) -> String {
    match class {
        AgentClass::Claude => "claude --dangerously-skip-permissions".to_string(),
        AgentClass::Codex => "codex --yolo".to_string(),
        AgentClass::Other(_) => agent_command_for(class),
    }
}

/// Encodes a toolbar agent launch exactly as an interactive terminal would:
/// the command line followed by Enter. `\r` is the legacy terminal encoding
/// emitted by `encode_key_for_terminal` for `KeyCode::Enter`, so this keeps
/// synthesized and physical submissions equivalent.
fn interactive_agent_input_for(class: &AgentClass) -> Vec<u8> {
    let mut input = interactive_agent_command_for(class).into_bytes();
    input.push(b'\r');
    input
}

/// Rebuilds a saved agent invocation. Claude accepts its session through
/// `--resume <id>`, while Codex requires `resume <id>` as a subcommand.
/// The saved command is canonicalized by `agent_command_for`, so matching
/// `codex` also keeps existing workspace snapshots valid.
fn command_line_for_saved_agent(saved_agent: &workspace_file::SavedAgent) -> String {
    let Some(session_id) = &saved_agent.session_id else {
        return saved_agent.command.clone();
    };

    if saved_agent.command == "codex" {
        return format!("codex resume {session_id}");
    }

    format!("{} --resume {session_id}", saved_agent.command)
}

/// Accumulates everything `materialize_saved_node` discovers across a whole
/// restore that isn't itself part of the tree/pane structure it's building:
/// which pane to focus first, human-readable notes about nodes it had to
/// skip, and session IDs it already knows for certain because it just used
/// them to build a `resume`/`--resume` invocation. Bundled into one struct
/// (rather than three loose `&mut` out-params) purely to keep
/// `materialize_saved_node`'s signature from growing every time restore
/// gains one more thing to track.
struct RestoreAccumulator {
    first_pane: Option<NodeId>,
    notes: Vec<String>,
    /// See `App::known_resumed_session_ids`'s doc comment for why
    /// `tick_detection` trusts this over live detection.
    known_resumed_session_ids: HashMap<NodeId, String>,
    /// See `App::title_sources`'s doc comment.
    title_sources: HashMap<NodeId, workspace_file::TitleSource>,
}

/// Recursively recreates one saved node (and, for a group, everything
/// under it) as a child of `parent` in `tree`, spawning a real
/// `PaneRuntime` for every pane along the way. Failures are pushed onto
/// `acc.notes` and skipped rather than propagated, so one bad node can
/// never take the rest of the restore down with it -- see
/// `App::from_saved_workspace`.
fn materialize_saved_node(
    tree: &mut Tree,
    panes: &mut HashMap<NodeId, PaneRuntime>,
    parent: NodeId,
    node: workspace_file::SavedNode,
    cwd: &std::path::Path,
    pane_size: (u16, u16),
    acc: &mut RestoreAccumulator,
) {
    let (pane_rows, pane_cols) = pane_size;
    match node {
        workspace_file::SavedNode::Group { name, children } => {
            match tree.add_group(parent, name.clone()) {
                Ok(group_id) => {
                    for child in children {
                        materialize_saved_node(tree, panes, group_id, child, cwd, pane_size, acc);
                    }
                }
                Err(err) => acc.notes.push(format!("group {name:?}: {err}")),
            }
        }
        workspace_file::SavedNode::Terminal {
            name,
            agent,
            title_source,
        } => {
            let parent = ensure_group_parent(tree, parent);
            match tree.add_pane(parent, name.clone(), PaneContentKind::Terminal) {
                Ok(id) => {
                    let spawned = match &agent {
                        Some(saved_agent) => {
                            let command_line = command_line_for_saved_agent(saved_agent);
                            TerminalPane::spawn_shell_command(
                                &default_shell_cmd(),
                                &command_line,
                                cwd,
                                pane_rows,
                                pane_cols,
                            )
                        }
                        None => {
                            TerminalPane::spawn(&default_shell_cmd(), cwd, pane_rows, pane_cols)
                        }
                    };
                    match spawned {
                        Ok(terminal) => {
                            panes.insert(id, PaneRuntime::Terminal(terminal));
                            acc.first_pane.get_or_insert(id);
                            if let Some(session_id) = agent
                                .as_ref()
                                .and_then(|saved_agent| saved_agent.session_id.clone())
                            {
                                acc.known_resumed_session_ids.insert(id, session_id);
                            }
                            if let Some(source) = title_source {
                                acc.title_sources.insert(id, source);
                            }
                        }
                        Err(err) => {
                            let _ = tree.remove_node(id);
                            acc.notes.push(format!("could not restart {name:?}: {err}"));
                        }
                    }
                }
                Err(err) => acc.notes.push(format!("terminal pane {name:?}: {err}")),
            }
        }
        workspace_file::SavedNode::Editor { name, path } => {
            // Production editor panes are always backed by a real path
            // (the file picker is the only normal way to create one); a
            // saved node with no path means it was captured mid-pick,
            // before a file was actually chosen -- nothing to restore.
            let Some(path) = path else {
                acc.notes.push(format!(
                    "editor pane {name:?}: no file was selected when it was saved, skipped"
                ));
                return;
            };
            let parent = ensure_group_parent(tree, parent);
            match tree.add_pane(parent, name.clone(), PaneContentKind::Editor) {
                Ok(id) => match EditorPane::load(path) {
                    Ok(editor) => {
                        panes.insert(id, PaneRuntime::Editor(Box::new(editor)));
                        acc.first_pane.get_or_insert(id);
                    }
                    Err(err) => {
                        let _ = tree.remove_node(id);
                        acc.notes.push(format!("could not reopen {name:?}: {err}"));
                    }
                },
                Err(err) => acc.notes.push(format!("editor pane {name:?}: {err}")),
            }
        }
    }
}

/// Panes can never be direct children of the session root (see
/// `illium_core::Tree::add_pane`'s own invariant) -- a saved workspace
/// should never actually have one there since `App::snapshot_children`
/// only ever writes groups at the top level, but a hand-edited or
/// otherwise unusual save file might, so this makes restoring one safe by
/// routing it into the default group instead of failing the whole node.
fn ensure_group_parent(tree: &mut Tree, parent: NodeId) -> NodeId {
    if parent == ROOT_ID {
        tree.ensure_default_group("default")
    } else {
        parent
    }
}

/// Opens every group in the tree in `tree_state`, so a restored workspace
/// (which may be arbitrarily deep, unlike the single default group a
/// fresh session boots with) renders fully expanded rather than hiding
/// everything behind collapsed groups the user has to open by hand.
fn open_all_groups(
    tree: &Tree,
    parent: NodeId,
    path: &mut Vec<NodeId>,
    tree_state: &mut TreeState<NodeId>,
) {
    let Ok(children) = tree.children_of(parent) else {
        return;
    };
    for &child in children {
        if matches!(
            tree.get(child).map(|node| &node.kind),
            Some(NodeKind::Group { .. })
        ) {
            path.push(child);
            tree_state.open(path.clone());
            open_all_groups(tree, child, path, tree_state);
            path.pop();
        }
    }
}

/// The full identifier path (top-level down to `id`), as
/// `tui_tree_widget::TreeState` expects it for `select`/`open`. Free
/// function so it can build a `TreeState` before an `App` exists yet (see
/// `App::from_saved_workspace`); `App::path_to` delegates here.
fn path_to_root(tree: &Tree, id: NodeId) -> Vec<NodeId> {
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

/// `$SHELL`, falling back to `/bin/bash` if unset.
fn default_shell_cmd() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
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
    let (bracket_col, checked) = markdown::checkbox::find_checkbox(line)?;

    let gutter_width = if editor.show_line_numbers {
        line_number_gutter_width(editor.textarea.lines().len())
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

/// Mirrors `ratatui_textarea`'s own line-number gutter width formula
/// (digit count of the highest line number, plus one column of margin
/// before the text starts) -- the crate applies this internally when
/// `show_line_number_style` is set but doesn't expose it, so `checkbox_at`
/// needs its own copy to know how many columns real content is pushed
/// over by.
fn line_number_gutter_width(total_lines: usize) -> usize {
    let digits = total_lines.max(1).ilog10() as usize + 1;
    digits + 2
}

/// True for a "this key was actually pressed" event (as opposed to a key
/// *release*, which some terminals report when the Kitty keyboard
/// protocol is enabled).
fn is_press(key: &KeyEvent) -> bool {
    matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
}

fn is_escape(event: &Event) -> bool {
    matches!(event, Event::Key(key) if key.code == KeyCode::Esc && is_press(key))
}

/// Returns the post-removal insertion index for a drop immediately after a
/// target sibling. Moving an earlier sibling first shortens the list, so its
/// apparent "after target" index must be reduced by one.
fn insertion_index_after_target(target_index: usize, source_was_before_target: bool) -> usize {
    target_index + usize::from(!source_was_before_target)
}

/// Hand-rolled crossterm-key -> terminal-input-bytes encoding. Not
/// exhaustive (v1 scope): covers printable characters, the common
/// control/navigation keys, and Ctrl+<letter>.
fn encode_key_for_terminal(key: &KeyEvent) -> Option<Vec<u8>> {
    if !is_press(key) {
        return None;
    }

    if key.modifiers.contains(KeyModifiers::CONTROL) {
        if let KeyCode::Char(c) = key.code {
            let lower = c.to_ascii_lowercase();
            if lower.is_ascii_lowercase() {
                // Ctrl+a..=Ctrl+z -> control bytes 1..=26.
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

    #[test]
    fn saved_agent_commands_use_each_cli_resume_syntax() {
        let session_id = "95fd0645-3331-408b-a7e5-36e6007bfb78".to_string();
        let codex = workspace_file::SavedAgent {
            command: "codex".to_string(),
            session_id: Some(session_id.clone()),
        };
        let claude = workspace_file::SavedAgent {
            command: "claude".to_string(),
            session_id: Some(session_id.clone()),
        };
        let fresh = workspace_file::SavedAgent {
            command: "codex".to_string(),
            session_id: None,
        };

        assert_eq!(
            command_line_for_saved_agent(&codex),
            format!("codex resume {session_id}")
        );
        assert_eq!(
            command_line_for_saved_agent(&claude),
            format!("claude --resume {session_id}")
        );
        assert_eq!(command_line_for_saved_agent(&fresh), "codex");
    }

    /// Regression test for the bug this was built to fix: illium builds
    /// `codex resume <id>` from a saved workspace's known session ID, so it
    /// must not then forget that ID and rely on re-deriving it live from
    /// the freshly spawned process -- see `known_resumed_session_ids`'s doc
    /// comment on `App`. Restoring a saved terminal pane with a known
    /// `SavedAgent::session_id` must record that ID against the pane's
    /// fresh `NodeId`, independent of whether live detection ever manages
    /// to re-derive it (which the real `codex`/`claude` binaries don't
    /// even need to be installed for this test to exercise -- the ID comes
    /// entirely from the saved workspace, not from the spawned process).
    #[test]
    fn restoring_a_resumed_agent_pane_remembers_its_known_session_id() {
        let dir = std::env::temp_dir().join("illium-app-tests").join(format!(
            "{:?}-resume-session-id",
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("create scratch dir");

        let session_id = "95fd0645-3331-408b-a7e5-36e6007bfb78".to_string();
        let saved = workspace_file::WorkspaceFile::new(vec![workspace_file::SavedNode::Group {
            name: "default".to_string(),
            children: vec![workspace_file::SavedNode::Terminal {
                name: "codex".to_string(),
                agent: Some(workspace_file::SavedAgent {
                    command: "codex".to_string(),
                    session_id: Some(session_id.clone()),
                }),
                title_source: None,
            }],
        }]);

        let app = App::from_saved_workspace(dir, saved).expect("restore should succeed");
        let pane_id = app
            .focused_pane
            .expect("the resumed terminal pane should be focused");

        assert_eq!(
            app.known_resumed_session_ids.get(&pane_id),
            Some(&session_id)
        );
    }

    /// A restored agent starts while its command is being spawned, so its
    /// first PTY size must already be the real pane size. In particular, an
    /// editor can be the first restored/focused pane, leaving this terminal
    /// in the background during startup; it must still never retain 24x80.
    #[test]
    fn restored_background_agent_starts_at_the_real_pane_size_and_tracks_resizes() {
        let dir = std::env::temp_dir().join("illium-app-tests").join(format!(
            "{:?}-restored-agent-pane-size",
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let editor_path = dir.join("notes.md");
        std::fs::write(&editor_path, "# Notes\n").expect("write editor fixture");

        let saved = workspace_file::WorkspaceFile::new(vec![workspace_file::SavedNode::Group {
            name: "default".to_string(),
            children: vec![
                workspace_file::SavedNode::Editor {
                    name: "notes.md".to_string(),
                    path: Some(editor_path),
                },
                workspace_file::SavedNode::Terminal {
                    name: "claude".to_string(),
                    agent: Some(workspace_file::SavedAgent {
                        command: "claude".to_string(),
                        session_id: Some("95fd0645-3331-408b-a7e5-36e6007bfb78".to_string()),
                    }),
                    title_source: None,
                },
            ],
        }]);

        let mut app =
            App::from_saved_workspace_with_project_name_and_pane_size(dir, saved, None, (41, 137))
                .expect("restore should succeed");
        assert!(matches!(
            app.panes
                .get(&app.focused_pane.expect("editor should be focused")),
            Some(PaneRuntime::Editor(_))
        ));

        let terminal_size = app
            .panes
            .values()
            .find_map(|runtime| match runtime {
                PaneRuntime::Terminal(terminal) => {
                    Some(terminal.with_screen(|screen| screen.size()))
                }
                PaneRuntime::Editor(_) => None,
            })
            .expect("restored terminal should exist");
        assert_eq!(terminal_size, (41, 137));

        app.set_pane_size(55, 150);
        let resized_terminal_size = app
            .panes
            .values()
            .find_map(|runtime| match runtime {
                PaneRuntime::Terminal(terminal) => {
                    Some(terminal.with_screen(|screen| screen.size()))
                }
                PaneRuntime::Editor(_) => None,
            })
            .expect("restored terminal should still exist");
        assert_eq!(resized_terminal_size, (55, 150));
    }

    #[test]
    fn toolbar_agent_launches_use_the_correct_command_and_submit_it() {
        assert_eq!(
            interactive_agent_input_for(&AgentClass::Codex),
            b"codex --yolo\r"
        );
        assert_eq!(
            interactive_agent_input_for(&AgentClass::Claude),
            b"claude --dangerously-skip-permissions\r"
        );
    }

    /// End-to-end render check: the preselected destination's row actually
    /// carries the tree's own selection accent (so the dialog visually
    /// previews where the group will land), an unselected row doesn't, and
    /// the real terminal cursor sits in the name field -- the dialog's only
    /// text-input surface.
    #[test]
    fn create_group_dialog_highlights_the_preselected_destination_and_places_cursor_in_the_name_field(
    ) {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let dir = std::env::temp_dir()
            .join("illium-app-tests")
            .join("render-create-group");
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = App::new(dir).unwrap();
        app.set_layout(UiLayout::from_screen_area(Rect::new(0, 0, 100, 30)));
        let backend_group = app.tree.add_group(ROOT_ID, "backend").unwrap();
        app.tree.add_group(ROOT_ID, "frontend").unwrap();
        app.open_create_group_dialog(backend_group);

        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| crate::ui::draw(frame, &mut app))
            .unwrap();
        let buffer = terminal.backend().buffer();

        // Destination order is [Top level, default, backend, frontend] --
        // "backend" (index 2) is preselected, "frontend" (index 3) isn't.
        let Mode::CreateGroup(state) = &app.mode else {
            panic!("expected Mode::CreateGroup");
        };
        let layout = modal::create_group_layout(state.area);
        let backend_row_bg = buffer[(layout.list_area.x, layout.list_area.y + 2)].bg;
        let frontend_row_bg = buffer[(layout.list_area.x, layout.list_area.y + 3)].bg;
        assert_eq!(
            backend_row_bg,
            crate::theme::selected_style().bg.expect("style has a bg")
        );
        assert_ne!(backend_row_bg, frontend_row_bg);

        let cursor = terminal.get_cursor_position().unwrap();
        assert_eq!(cursor.y, layout.name_row.y);
    }

    #[test]
    fn drop_after_target_accounts_for_removing_an_earlier_sibling() {
        assert_eq!(insertion_index_after_target(1, true), 1);
        assert_eq!(insertion_index_after_target(1, false), 2);
    }

    fn enter_key_event() -> Event {
        Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
    }

    /// Right-click "New group…" always opens the same destination-picker
    /// dialog rather than creating a group directly; what was clicked only
    /// decides which destination starts highlighted: empty space below the
    /// tree (`ROOT_ID`) preselects the top level, a pane preselects its
    /// parent group, and a group preselects itself. Confirming with Enter
    /// then actually creates the group under whichever destination is
    /// selected at that point.
    #[test]
    fn new_group_dialog_preselects_the_right_click_target() {
        let dir = std::env::temp_dir().join("illium-app-tests").join(format!(
            "{:?}-new-group-placement",
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("create scratch dir");

        let mut app = App::new(dir).expect("app should start");
        let default_group = app
            .tree
            .children_of(ROOT_ID)
            .expect("root has children")
            .first()
            .copied()
            .expect("default group exists");
        let pane_id = app
            .tree
            .children_of(default_group)
            .expect("default group has children")
            .first()
            .copied()
            .expect("default pane exists");

        // Empty space -> top level preselected and confirmed.
        app.execute_context_action(ContextMenuAction::NewGroup, ROOT_ID)
            .expect("opening the dialog should not error");
        match &app.mode {
            Mode::CreateGroup(state) => {
                assert_eq!(state.destinations[state.selected_index].id, ROOT_ID);
            }
            _ => panic!("expected Mode::CreateGroup"),
        }
        app.handle_event(enter_key_event())
            .expect("confirming should not error");
        let root_children = app.tree.children_of(ROOT_ID).unwrap().to_vec();
        assert_eq!(root_children.len(), 2);
        assert_eq!(app.tree.parent_of(root_children[1]), Some(ROOT_ID));

        // Pane -> its parent group preselected and confirmed.
        app.execute_context_action(ContextMenuAction::NewGroup, pane_id)
            .expect("opening the dialog should not error");
        match &app.mode {
            Mode::CreateGroup(state) => {
                assert_eq!(state.destinations[state.selected_index].id, default_group);
            }
            _ => panic!("expected Mode::CreateGroup"),
        }
        app.handle_event(enter_key_event())
            .expect("confirming should not error");
        let sibling_group = *app.tree.children_of(default_group).unwrap().last().unwrap();
        assert_eq!(app.tree.parent_of(sibling_group), Some(default_group));

        // Group -> itself preselected and confirmed, nesting inside it.
        app.execute_context_action(ContextMenuAction::NewGroup, default_group)
            .expect("opening the dialog should not error");
        match &app.mode {
            Mode::CreateGroup(state) => {
                assert_eq!(state.destinations[state.selected_index].id, default_group);
            }
            _ => panic!("expected Mode::CreateGroup"),
        }
        app.handle_event(enter_key_event())
            .expect("confirming should not error");
        let nested_group = *app.tree.children_of(default_group).unwrap().last().unwrap();
        assert_eq!(app.tree.parent_of(nested_group), Some(default_group));
    }

    /// Blank name confirms as "group" (naming is optional); a typed name is
    /// used verbatim, trimmed of surrounding whitespace.
    #[test]
    fn new_group_dialog_defaults_the_name_when_left_blank() {
        let dir = std::env::temp_dir().join("illium-app-tests").join(format!(
            "{:?}-new-group-naming",
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let mut app = App::new(dir).expect("app should start");

        app.open_create_group_dialog(ROOT_ID);
        app.handle_event(enter_key_event())
            .expect("confirming should not error");
        let created = *app.tree.children_of(ROOT_ID).unwrap().last().unwrap();
        assert_eq!(app.tree.get(created).unwrap().name, "group");

        app.open_create_group_dialog(ROOT_ID);
        for c in "scratch".chars() {
            app.handle_event(Event::Key(KeyEvent::new(
                KeyCode::Char(c),
                KeyModifiers::NONE,
            )))
            .expect("typing should not error");
        }
        app.handle_event(enter_key_event())
            .expect("confirming should not error");
        let named = *app.tree.children_of(ROOT_ID).unwrap().last().unwrap();
        assert_eq!(app.tree.get(named).unwrap().name, "scratch");
    }

    /// Esc cancels the dialog without creating anything, from any point in
    /// the flow (even mid-typed-name).
    #[test]
    fn new_group_dialog_esc_cancels_without_creating() {
        let dir = std::env::temp_dir().join("illium-app-tests").join(format!(
            "{:?}-new-group-cancel",
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let mut app = App::new(dir).expect("app should start");

        let root_children_before = app.tree.children_of(ROOT_ID).unwrap().len();
        app.open_create_group_dialog(ROOT_ID);
        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        )))
        .expect("typing should not error");
        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))
            .expect("cancelling should not error");

        assert!(matches!(app.mode, Mode::Normal));
        assert_eq!(
            app.tree.children_of(ROOT_ID).unwrap().len(),
            root_children_before
        );
    }

    /// Regression test for clicking "New editor here": the button-release
    /// of the very click that opens the file picker used to reach
    /// `handle_mouse_event`, which unconditionally `mem::replace`d
    /// `self.mode` with `Normal` before checking what it actually was --
    /// destroying `Mode::Explorer` before a single keypress could reach it.
    /// The user saw the picker "flash" for one frame, then land on an
    /// orphaned "(new file)" tree node with no editor behind it.
    #[test]
    fn context_menu_editor_pick_survives_the_click_release() {
        let dir = std::env::temp_dir()
            .join("illium-app-tests")
            .join(format!("{:?}", std::thread::current().id()));
        std::fs::create_dir_all(&dir).expect("create scratch dir");

        let mut app = App::new(dir).expect("app should start");
        app.set_layout(UiLayout::from_screen_area(Rect::new(0, 0, 120, 40)));

        let pane_id = app.focused_pane.expect("initial shell pane is focused");
        app.open_context_menu(pane_id, 10, 10);
        let menu_area = match &app.mode {
            Mode::ContextMenu(menu) => menu.area,
            _ => panic!("open_context_menu should enter Mode::ContextMenu"),
        };
        // A pane target's menu is [FocusPane, NewTerminal, NewEditor, ...];
        // row 0 is the border, so item index 2 is at +1 +2.
        let new_editor_row = menu_area.y + 3;
        let click = |kind: MouseEventKind| {
            Event::Mouse(MouseEvent {
                kind,
                column: menu_area.x + 1,
                row: new_editor_row,
                modifiers: crossterm::event::KeyModifiers::NONE,
            })
        };

        app.handle_event(click(MouseEventKind::Down(MouseButton::Left)))
            .expect("click on 'New editor here' should not error");
        assert!(
            matches!(app.mode, Mode::Explorer(..)),
            "clicking 'New editor here' should open the file picker"
        );

        // The button-release of that same click is the very next mouse
        // event delivered.
        app.handle_event(click(MouseEventKind::Up(MouseButton::Left)))
            .expect("click release should not error");
        assert!(
            matches!(app.mode, Mode::Explorer(..)),
            "the file picker must survive its own opening click's button-release"
        );
    }

    #[test]
    fn checkbox_at_detects_click_on_the_bracket_span_only() {
        let mut editor = EditorPane::empty();
        editor.path = Some(PathBuf::from("notes.md"));
        editor.show_line_numbers = false;
        editor.textarea = ratatui_textarea::TextArea::new(vec![
            "- [ ] buy milk".to_string(),
            "not a checkbox line".to_string(),
        ]);
        let content_area = Rect::new(0, 0, 40, 10);

        // "[ ]" sits at columns 2..5 with no gutter.
        assert_eq!(
            checkbox_at(&editor, content_area, Position::new(2, 0)),
            Some((0, 2, false))
        );
        assert_eq!(
            checkbox_at(&editor, content_area, Position::new(4, 0)),
            Some((0, 2, false))
        );
        assert_eq!(
            checkbox_at(&editor, content_area, Position::new(10, 0)),
            None,
            "clicking elsewhere on the same line must not toggle it"
        );
        assert_eq!(
            checkbox_at(&editor, content_area, Position::new(2, 1)),
            None
        );
    }

    #[test]
    fn checkbox_at_accounts_for_the_line_number_gutter() {
        let mut editor = EditorPane::empty();
        editor.path = Some(PathBuf::from("notes.md"));
        editor.show_line_numbers = true; // gutter width = digits(1) + 2 = 3
        editor.textarea = ratatui_textarea::TextArea::new(vec!["- [ ] buy milk".to_string()]);
        let content_area = Rect::new(0, 0, 40, 10);

        assert_eq!(
            checkbox_at(&editor, content_area, Position::new(2, 0)),
            None,
            "column 2 lands in the gutter, not the checkbox"
        );
        assert_eq!(
            checkbox_at(&editor, content_area, Position::new(5, 0)),
            Some((0, 2, false))
        );
    }

    #[test]
    fn checkbox_at_ignores_non_markdown_files() {
        let mut editor = EditorPane::empty();
        editor.path = Some(PathBuf::from("notes.txt"));
        editor.show_line_numbers = false;
        editor.textarea = ratatui_textarea::TextArea::new(vec!["- [ ] buy milk".to_string()]);
        let content_area = Rect::new(0, 0, 40, 10);
        assert_eq!(
            checkbox_at(&editor, content_area, Position::new(2, 0)),
            None
        );
    }

    /// End-to-end through `App::handle_event`: a real left-click on a
    /// rendered checkbox's screen position flips its source text and
    /// marks the pane dirty, exactly like the top-toolbar Save button
    /// would then need to persist.
    #[test]
    fn clicking_a_checkbox_flips_it_through_the_full_mouse_dispatch() {
        let dir = std::env::temp_dir()
            .join("illium-app-tests")
            .join(format!("{:?}-checkbox", std::thread::current().id()));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let file_path = dir.join("todo.md");
        std::fs::write(&file_path, "- [ ] buy milk\n").expect("write fixture file");

        let mut app = App::new(dir.clone()).expect("app should start");
        app.set_layout(UiLayout::from_screen_area(Rect::new(0, 0, 120, 40)));

        let pane_id = app.focused_pane.expect("initial shell pane is focused");
        let editor = EditorPane::load(file_path.clone()).expect("load fixture");
        app.panes
            .insert(pane_id, PaneRuntime::Editor(Box::new(editor)));

        let content_area = editor_chrome::compute(app.layout.pane_content_area, true).content_area;
        // Gutter width 3 (one-digit line numbers) + bracket col 2 = column 5.
        let click = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: content_area.x + 5,
            row: content_area.y,
            modifiers: crossterm::event::KeyModifiers::NONE,
        });
        app.handle_event(click)
            .expect("clicking the checkbox should not error");

        let Some(PaneRuntime::Editor(editor)) = app.panes.get(&pane_id) else {
            panic!("expected the replaced pane to still be an editor");
        };
        assert_eq!(editor.textarea.lines(), &["- [x] buy milk".to_string()]);
        assert!(editor.dirty, "toggling a checkbox is an edit");

        let _ = std::fs::remove_file(&file_path);
    }

    /// End-to-end through `App::handle_event`: a real left-click on the
    /// minimap column jumps the Source-mode cursor to the corresponding
    /// buffer line, inverting `minimap::render`'s bucketing the same way
    /// `minimap::line_for_click` does in isolation.
    #[test]
    fn clicking_the_minimap_jumps_the_cursor_through_the_full_mouse_dispatch() {
        let dir = std::env::temp_dir()
            .join("illium-app-tests")
            .join(format!("{:?}-minimap-click", std::thread::current().id()));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let file_path = dir.join("notes.txt");
        let contents: String = (0..50).map(|n| format!("line {n}\n")).collect();
        std::fs::write(&file_path, &contents).expect("write fixture file");

        let mut app = App::new(dir.clone()).expect("app should start");
        app.set_layout(UiLayout::from_screen_area(Rect::new(0, 0, 120, 40)));

        let pane_id = app.focused_pane.expect("initial shell pane is focused");
        let editor = EditorPane::load(file_path.clone()).expect("load fixture");
        app.panes
            .insert(pane_id, PaneRuntime::Editor(Box::new(editor)));

        let chrome = editor_chrome::compute(app.layout.pane_content_area, true);
        let minimap_area = chrome
            .minimap_area
            .expect("pane is wide enough for a minimap");
        let click_row = minimap_area.y + 3;
        let expected_line =
            minimap::line_for_click(click_row - minimap_area.y, minimap_area.height, 50);

        let click = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: minimap_area.x,
            row: click_row,
            modifiers: crossterm::event::KeyModifiers::NONE,
        });
        app.handle_event(click)
            .expect("clicking the minimap should not error");

        let Some(PaneRuntime::Editor(editor)) = app.panes.get(&pane_id) else {
            panic!("expected the replaced pane to still be an editor");
        };
        assert_eq!(editor.textarea.cursor().0, expected_line);

        let _ = std::fs::remove_file(&file_path);
    }

    /// Regression coverage for the top-toolbar Save button: clicking it
    /// should write the buffer to disk and clear the dirty flag, exactly
    /// like the pre-existing leader-key `Action::Save`.
    #[test]
    fn toolbar_save_writes_the_file_and_clears_dirty() {
        let dir = std::env::temp_dir()
            .join("illium-app-tests")
            .join(format!("{:?}-toolbar-save", std::thread::current().id()));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let file_path = dir.join("notes.txt");
        std::fs::write(&file_path, "original\n").expect("write fixture file");

        let mut app = App::new(dir.clone()).expect("app should start");
        let pane_id = app.focused_pane.expect("initial shell pane is focused");
        let mut editor = EditorPane::load(file_path.clone()).expect("load fixture");
        editor.textarea = ratatui_textarea::TextArea::new(vec!["edited".to_string()]);
        editor.dirty = true;
        app.panes
            .insert(pane_id, PaneRuntime::Editor(Box::new(editor)));

        app.execute_editor_toolbar_action(pane_id, editor_toolbar::ToolbarAction::Save);

        let Some(PaneRuntime::Editor(editor)) = app.panes.get(&pane_id) else {
            panic!("expected the replaced pane to still be an editor");
        };
        assert!(!editor.dirty);
        assert_eq!(
            std::fs::read_to_string(&file_path).expect("read saved file"),
            "edited\n"
        );

        let _ = std::fs::remove_file(&file_path);
    }

    /// Clicking Save As opens the filename prompt pre-filled with the
    /// pane's current path; committing it retargets the pane to the new
    /// path, writes the buffer there, and renames the tree node to match.
    #[test]
    fn save_as_retargets_the_pane_and_renames_the_tree_node() {
        let dir = std::env::temp_dir()
            .join("illium-app-tests")
            .join(format!("{:?}-save-as", std::thread::current().id()));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let original_path = dir.join("original.txt");
        std::fs::write(&original_path, "hello\n").expect("write fixture file");
        let new_path = dir.join("renamed.txt");
        let _ = std::fs::remove_file(&new_path);

        let mut app = App::new(dir.clone()).expect("app should start");
        let pane_id = app.focused_pane.expect("initial shell pane is focused");
        let editor = EditorPane::load(original_path.clone()).expect("load fixture");
        app.panes
            .insert(pane_id, PaneRuntime::Editor(Box::new(editor)));

        app.execute_editor_toolbar_action(pane_id, editor_toolbar::ToolbarAction::SaveAs);
        match &app.mode {
            Mode::SaveAs(id, state) => {
                assert_eq!(*id, pane_id);
                assert_eq!(state.buf, original_path.display().to_string());
            }
            _ => panic!("expected Mode::SaveAs after clicking Save As"),
        }

        app.action_save_as(pane_id, new_path.display().to_string());

        let Some(PaneRuntime::Editor(editor)) = app.panes.get(&pane_id) else {
            panic!("expected the replaced pane to still be an editor");
        };
        assert_eq!(editor.path, Some(new_path.clone()));
        assert!(!editor.dirty);
        assert_eq!(
            std::fs::read_to_string(&new_path).expect("read file at the new path"),
            "hello\n"
        );
        assert_eq!(
            app.tree.get(pane_id).map(|node| node.name.as_str()),
            Some("renamed.txt")
        );

        let _ = std::fs::remove_file(&original_path);
        let _ = std::fs::remove_file(&new_path);
    }

    /// Regression test: a Save As that fails to write (bad target
    /// directory) must leave the pane pointed at its last-good path, not
    /// silently retargeted at a location nothing was ever written to.
    #[test]
    fn failed_save_as_does_not_corrupt_the_pane_path() {
        let dir = std::env::temp_dir()
            .join("illium-app-tests")
            .join(format!("{:?}-failed-save-as", std::thread::current().id()));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let original_path = dir.join("original.txt");
        std::fs::write(&original_path, "hello\n").expect("write fixture file");
        // `original.txt` is a file, so treating it as a parent directory
        // (as the bad "todo.md/../renamed.md" prompt input would) fails
        // with ENOTDIR instead of writing anywhere.
        let bad_path = original_path.join("nested.txt");

        let mut app = App::new(dir.clone()).expect("app should start");
        let pane_id = app.focused_pane.expect("initial shell pane is focused");
        let editor = EditorPane::load(original_path.clone()).expect("load fixture");
        app.panes
            .insert(pane_id, PaneRuntime::Editor(Box::new(editor)));

        app.action_save_as(pane_id, bad_path.display().to_string());
        assert!(
            app.status_message
                .as_deref()
                .is_some_and(|message| message.starts_with("Save failed")),
            "expected a Save failed status message, got {:?}",
            app.status_message
        );

        let Some(PaneRuntime::Editor(editor)) = app.panes.get(&pane_id) else {
            panic!("expected the replaced pane to still be an editor");
        };
        assert_eq!(
            editor.path,
            Some(original_path.clone()),
            "a failed Save As must not retarget the pane"
        );

        let _ = std::fs::remove_file(&original_path);
    }

    fn scratch_session_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("illium-app-tests")
            .join(format!("{name}-{:?}", std::thread::current().id()));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    #[test]
    fn panes_needing_title_inference_gates_on_session_id_class_and_prior_state() {
        let mut app =
            App::new(scratch_session_dir("title-inference-gating")).expect("app should start");
        let pane_id = app.focused_pane.expect("initial shell pane is focused");

        // A plain shell (no detected agent) is never a candidate.
        assert!(app.panes_needing_title_inference().is_empty());

        // An agent without a session ID yet still isn't a candidate.
        app.agent_processes.insert(
            pane_id,
            agent_detect::AgentProcessInfo {
                class: AgentClass::Claude,
                pid: 1,
                session_id: None,
            },
        );
        assert!(app.panes_needing_title_inference().is_empty());

        // A detected agent with a known transcript format and a fresh
        // session ID is exactly the intended candidate.
        app.agent_processes.get_mut(&pane_id).unwrap().session_id = Some("session-1".to_string());
        let pending = app.panes_needing_title_inference();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].pane_id, pane_id);
        assert_eq!(pending[0].session_id, "session-1");
        assert_eq!(pending[0].agent_class, AgentClass::Claude);

        // Once a worker is marked started, it must not be offered again.
        app.mark_title_inference_started(pane_id);
        assert!(app.panes_needing_title_inference().is_empty());
        assert!(app.titles_loading.contains(&pane_id));

        // `AgentClass::Other` has no known transcript format, so it's never
        // a candidate even with a session ID.
        app.titles_loading.clear();
        app.agent_processes.get_mut(&pane_id).unwrap().class =
            AgentClass::Other("opencode".to_string());
        assert!(app.panes_needing_title_inference().is_empty());
    }

    #[test]
    fn apply_inferred_title_renames_the_pane_and_blocks_further_inference() {
        let mut app =
            App::new(scratch_session_dir("apply-inferred-title")).expect("app should start");
        let pane_id = app.focused_pane.expect("initial shell pane is focused");
        app.agent_processes.insert(
            pane_id,
            agent_detect::AgentProcessInfo {
                class: AgentClass::Claude,
                pid: 1,
                session_id: Some("session-1".to_string()),
            },
        );
        app.mark_title_inference_started(pane_id);

        app.apply_inferred_title(pane_id, "Fix Auth Bug".to_string());

        assert_eq!(app.tree.get(pane_id).unwrap().name, "Fix Auth Bug");
        assert!(!app.titles_loading.contains(&pane_id));
        assert!(
            app.panes_needing_title_inference().is_empty(),
            "an established auto title must not be re-offered for inference"
        );
    }

    #[test]
    fn apply_inferred_title_never_overwrites_a_user_set_title() {
        let mut app = App::new(scratch_session_dir("apply-inferred-title-user-wins"))
            .expect("app should start");
        let pane_id = app.focused_pane.expect("initial shell pane is focused");
        app.agent_processes.insert(
            pane_id,
            agent_detect::AgentProcessInfo {
                class: AgentClass::Claude,
                pid: 1,
                session_id: Some("session-1".to_string()),
            },
        );
        app.mark_title_inference_started(pane_id);

        // The user renames the pane while inference is still in flight.
        app.title_sources
            .insert(pane_id, workspace_file::TitleSource::User);
        let renamed = app.tree.rename_node(pane_id, "My Custom Title").is_ok();
        assert!(renamed);

        // The stale inference result must not clobber it.
        app.apply_inferred_title(pane_id, "Fix Auth Bug".to_string());
        assert_eq!(app.tree.get(pane_id).unwrap().name, "My Custom Title");
    }

    #[test]
    fn fail_title_inference_stops_further_retries_this_run() {
        let mut app =
            App::new(scratch_session_dir("fail-title-inference")).expect("app should start");
        let pane_id = app.focused_pane.expect("initial shell pane is focused");
        app.agent_processes.insert(
            pane_id,
            agent_detect::AgentProcessInfo {
                class: AgentClass::Claude,
                pid: 1,
                session_id: Some("session-1".to_string()),
            },
        );
        app.mark_title_inference_started(pane_id);

        app.fail_title_inference(pane_id, "gateway timed out");

        assert!(!app.titles_loading.contains(&pane_id));
        assert!(app.panes_needing_title_inference().is_empty());
        assert_eq!(
            app.status_message.as_deref(),
            Some("Could not infer a session title: gateway timed out")
        );
    }

    #[test]
    fn committing_a_manual_rename_marks_the_title_user_set_and_blocks_future_auto_inference() {
        let mut app =
            App::new(scratch_session_dir("manual-rename-marks-user")).expect("app should start");
        let pane_id = app.focused_pane.expect("initial shell pane is focused");
        app.agent_processes.insert(
            pane_id,
            agent_detect::AgentProcessInfo {
                class: AgentClass::Claude,
                pid: 1,
                session_id: Some("session-1".to_string()),
            },
        );
        assert_eq!(app.panes_needing_title_inference().len(), 1);

        app.handle_rename_event(TextPromptState::new("My Renamed Pane"), &enter_key_event());

        assert_eq!(app.tree.get(pane_id).unwrap().name, "My Renamed Pane");
        assert_eq!(
            app.title_sources.get(&pane_id),
            Some(&workspace_file::TitleSource::User)
        );
        assert!(
            app.panes_needing_title_inference().is_empty(),
            "a user-set title must never be offered for auto-inference"
        );
    }
}
