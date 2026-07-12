//! `App`: illium-client's thin orchestrator state. Owns a read-only
//! render-cache mirror of the session tree (kept in sync from
//! `ServerEvent::TreeSnapshot`/`PaneStatusChanged`/`ScreenUpdate` -- see
//! `crate::render_cache`), plus purely local UI state (focus, input mode,
//! hover/animation state, editor pane buffers).
//!
//! Unlike the pre-client/server `App`, this one never spawns a PTY and
//! never mutates `self.tree` directly in response to user input -- every
//! structural change (new/close/move/rename a node) is expressed as an
//! `illium_ipc::ClientRequest` pushed onto `outbox` for the connection
//! task to actually send; `self.tree` only changes when the server's own
//! `TreeSnapshot` confirms it did. This is what keeps there being exactly
//! one writable tree (the server's) -- see the crate's module docs.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use illium_core::{GroupListing, NodeId, NodeKind, PaneContentKind, PaneStatus, Tree, ROOT_ID};
use illium_ipc::ClientRequest;
use ratatui::layout::{Position, Rect};
use tui_tree_widget::TreeState;

use crate::editor_pane::{EditorPane, EditorViewMode};
use crate::explorer_overlay::ExplorerOverlay;
use crate::layout::{TreeWidthAnimation, UiLayout};
use crate::terminal_view::{self, TerminalView};
use crate::text_prompt::TextPromptState;
use crate::tree_ui::{self, TreeNodeHit, TreeToolbarAction};

/// Which side of the UI currently has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusTarget {
    Tree,
    Pane,
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
    /// A mouse-anchored action menu for one tree node.
    ContextMenu(ContextMenu),
    /// The "New group" destination picker is open.
    CreateGroup(CreateGroupState),
    /// A Yes/No confirmation is pending before closing `NodeId`.
    ConfirmClose(NodeId),
}

/// Actions exposed by a right-click on a tree entry. These deliberately map
/// to the same focused-node operations as the keyboard, so neither input
/// path can drift into a different tree mutation policy.
///
/// This still has no "Indent into previous group" / "Outdent" entry:
/// `illium_ipc::ClientRequest::ReparentNode` (backing `Tree::move_node`)
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
    ToggleGroup,
    NewTerminal,
    NewEditor,
    NewGroup,
    Rename,
    MoveUp,
    MoveDown,
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
            Self::Close => "Close",
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

/// State of the "New group" destination-picker dialog. `destinations` is a
/// snapshot taken when the dialog opened -- it does not track further tree
/// mutations, matching every other modal in illium.
pub struct CreateGroupState {
    pub area: Rect,
    pub destinations: Vec<GroupListing>,
    pub selected_index: usize,
    pub name: TextPromptState,
}

/// The live, client-local half of a pane. For a PTY-backed pane this is
/// only a render cache fed by `ServerEvent::ScreenUpdate` -- see
/// `crate::terminal_view`'s module docs for why illium-client owns no PTY
/// handle at all. Editor panes are unchanged from the pre-client/server
/// design: buffer content and file I/O stay entirely client-local.
pub enum PaneRuntime {
    Terminal(Box<TerminalView>),
    Editor(Box<EditorPane>),
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
    pub focused_pane: Option<NodeId>,
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
    /// to carry a path back.
    pending_editor_opens: Vec<(String, PathBuf)>,
    pub markdown_picker: ratatui_image::picker::Picker,
    pub markdown_rasterizer: crate::markdown::raster::HeaderRasterizer,
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
            focused_pane: None,
            focus: FocusTarget::Pane,
            mode: Mode::Normal,
            status_message: None,
            should_quit: false,
            started_at,
            last_known_pane_size: (terminal_view::DEFAULT_ROWS, terminal_view::DEFAULT_COLS),
            layout: UiLayout::default(),
            tree_width_animation: TreeWidthAnimation::new(started_at),
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
        }
    }

    /// Drains every `ClientRequest` queued by input handling since the
    /// last drain, for the caller to actually send over the connection.
    pub fn take_outbound_requests(&mut self) -> Vec<ClientRequest> {
        std::mem::take(&mut self.outbox)
    }

    pub(crate) fn queue_request(&mut self, request: ClientRequest) {
        self.outbox.push(request);
    }

    /// Updates the geometry shared by rendering, hit-testing, and pane
    /// sizing, and queues a `ResizePane` request for every terminal pane
    /// whose size actually changed.
    pub fn set_layout(&mut self, layout: UiLayout) {
        if self.layout == layout {
            return;
        }
        self.layout = layout;
        let (rows, cols) = layout.pane_content_size();
        self.set_pane_size(rows, cols);

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

    /// Recomputes geometry after the host terminal changes size while
    /// preserving the animation's current visible width.
    pub fn set_screen_area(&mut self, screen_area: Rect) {
        let layout = UiLayout::from_screen_area_with_tree_width(
            screen_area,
            self.tree_width_animation.current_width(),
        );
        self.set_layout(layout);
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
        let is_tree_active = self.is_terminal_focused
            && (is_pointer_over_tree || matches!(self.focus, FocusTarget::Tree));
        let tree_width = self.tree_width_animation.update(is_tree_active, now);
        let layout =
            UiLayout::from_screen_area_with_tree_width(self.layout.screen_area, tree_width);
        self.set_layout(layout);
    }

    pub const fn is_layout_animating(&self) -> bool {
        self.tree_width_animation.is_animating()
    }

    /// Records the real pane-content size and queues a `ResizePane`
    /// request for every terminal pane whose local view doesn't already
    /// match it (a freshly-created pane created at this size needs none).
    pub fn set_pane_size(&mut self, rows: u16, cols: u16) {
        self.last_known_pane_size = (rows, cols);
        let pane_ids: Vec<NodeId> = self.panes.keys().copied().collect();
        for pane_id in pane_ids {
            if let Some(PaneRuntime::Terminal(view)) = self.panes.get_mut(&pane_id) {
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
        let width = self.layout.pane_content_area.width;
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
            status: PaneStatus::Agent(class, illium_core::AgentActivity::Done),
            ..
        }) = self.tree.get(id).map(|node| &node.kind)
        {
            let class = class.clone();
            let _ = self.tree.set_pane_status(
                id,
                PaneStatus::Agent(class, illium_core::AgentActivity::Idle),
            );
        }
    }

    /// Focuses `id` (a pane) both in the tree selection and as the
    /// right-panel content, clearing a stale `Done` flag if this is the
    /// first look since it finished.
    pub fn focus_pane(&mut self, id: NodeId) {
        self.select_node(id);
        self.focused_pane = Some(id);
        self.focus = FocusTarget::Pane;
        self.mark_seen(id);
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

    /// The group a newly created node should be added under: an explicitly
    /// selected group (or the parent of an explicitly selected pane) takes
    /// priority, falling back to the focused pane's group, then the
    /// session's default group.
    pub(crate) fn group_for_new_node(&mut self) -> NodeId {
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

    /// Human-readable description of what closing `target` would lose,
    /// or `None` when it can be closed without confirmation (an empty
    /// group, a plain shell, or a clean/unsaved-nothing editor).
    pub fn close_confirmation_message(&self, target: NodeId) -> Option<String> {
        let node = self.tree.get(target)?;
        match &node.kind {
            NodeKind::Group { children, .. } if !children.is_empty() => Some(format!(
                "\"{}\" contains {} item(s). Close it and everything inside?",
                node.name,
                children.len()
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
            kind: illium_ipc::NewPaneKind::PlainShell,
        });
    }

    /// Queues a `NewPane` request for a specific command line (e.g.
    /// `claude`, `codex`) under `parent_group`.
    pub fn request_new_command_pane(&mut self, parent_group: NodeId, command_line: String) {
        self.queue_request(ClientRequest::NewPane {
            parent_group,
            kind: illium_ipc::NewPaneKind::Command(command_line),
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
        self.pending_editor_opens.push((basename, path.clone()));
        self.queue_request(ClientRequest::NewPane {
            parent_group,
            kind: illium_ipc::NewPaneKind::Editor(path),
        });
    }

    /// Queues a `ClosePane` request. The tree/pane-runtime removal itself
    /// only happens once the server confirms it via the next `TreeSnapshot`.
    pub fn request_close(&mut self, target: NodeId) {
        self.queue_request(ClientRequest::ClosePane { pane_id: target });
    }

    pub fn request_move(&mut self, node_id: NodeId, direction: illium_core::TreeMoveDirection) {
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

    pub fn request_rename(&mut self, node_id: NodeId, title: String) {
        self.queue_request(ClientRequest::RenameNode { node_id, title });
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
                Some(NodeKind::Group { .. }) => Some(id),
                Some(NodeKind::Pane { .. }) => self.tree.parent_of(id),
                None => None,
            }
        });
        let visible_pane_target = self.focused_pane.and_then(|id| self.tree.parent_of(id));
        selected_target.or(visible_pane_target).unwrap_or(ROOT_ID)
    }

    /// Destination to preselect when "New group…" is triggered from a
    /// right-click on a specific tree node.
    pub fn create_group_target_for_click(&self, target: NodeId) -> NodeId {
        if target == ROOT_ID {
            return ROOT_ID;
        }
        match self.tree.get(target).map(|node| &node.kind) {
            Some(NodeKind::Group { .. }) => target,
            Some(NodeKind::Pane { .. }) => self.tree.parent_of(target).unwrap_or(ROOT_ID),
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
    /// rather than on a real node -- only the creation actions apply
    /// there, none of the per-node ones.
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
            ContextMenuAction::Close,
        ]);
        actions
    }

    /// Executes one context-menu command, then leaves the popup unless the
    /// action opens an explicit sub-mode (e.g. Rename, the file picker).
    pub fn execute_context_action(&mut self, action: ContextMenuAction, target: NodeId) {
        self.mode = Mode::Normal;
        match action {
            ContextMenuAction::FocusPane => self.focus_pane(target),
            ContextMenuAction::ToggleGroup => {
                self.tree_state.toggle_selected();
            }
            ContextMenuAction::NewTerminal => self.action_new_terminal(),
            ContextMenuAction::NewEditor => self.action_new_editor(),
            ContextMenuAction::NewGroup => {
                let preselected = self.create_group_target_for_click(target);
                self.open_create_group_dialog(preselected);
            }
            ContextMenuAction::Rename => self.action_start_rename(),
            ContextMenuAction::MoveUp => {
                self.request_move(target, illium_core::TreeMoveDirection::Up)
            }
            ContextMenuAction::MoveDown => {
                self.request_move(target, illium_core::TreeMoveDirection::Down)
            }
            ContextMenuAction::Close => self.action_close(target),
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
        let Some(id) = self.focused_pane else {
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
                    self.request_rename(id, name.to_string());
                }
                self.status_message = Some("Saved".to_string());
            }
            Err(err) => self.status_message = Some(format!("Save failed: {err}")),
        }
    }

    pub fn action_toggle_editor_view_mode(&mut self) {
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

    pub fn action_toggle_editor_line_numbers(&mut self) {
        let Some(id) = self.focused_pane else {
            return;
        };
        if let Some(PaneRuntime::Editor(editor)) = self.panes.get_mut(&id) {
            editor.toggle_line_numbers();
        }
    }

    pub fn action_toggle_editor_minimap(&mut self) {
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

    pub fn action_toggle_editor_autosave(&mut self) {
        let Some(id) = self.focused_pane else {
            return;
        };
        if let Some(PaneRuntime::Editor(editor)) = self.panes.get_mut(&id) {
            editor.toggle_autosave();
        }
    }

    /// `main.rs`'s poll loop calls this every tick: writes any editor pane
    /// whose autosave debounce is due.
    pub fn tick_autosave(&mut self) {
        for runtime in self.panes.values_mut() {
            if let PaneRuntime::Editor(editor) = runtime {
                if let Some(Err(err)) = editor.autosave_if_due() {
                    tracing::warn!("autosave failed: {err}");
                }
            }
        }
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
    pub fn tree_node_at(&self, position: Position) -> Option<TreeNodeHit> {
        tree_ui::node_at_position(
            &self.tree,
            &self.tree_state,
            self.layout.tree_area,
            position,
        )
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
                    if matches!(
                        self.tree.get(id).map(|node| &node.kind),
                        Some(NodeKind::Pane { .. })
                    ) {
                        self.focus_pane(id);
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
    pub fn handle_pane_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        let Some(id) = self.focused_pane else {
            return;
        };
        let editor_content_area = self.editor_content_area(id);
        match self.panes.get_mut(&id) {
            Some(PaneRuntime::Terminal(_)) => {
                if let Some(bytes) = encode_key_for_terminal(&key) {
                    self.queue_request(ClientRequest::KeyInput { pane_id: id, bytes });
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
            None => {}
        }
    }

    /// Exact content rectangle for editor `id`, after its toolbar and
    /// optional minimap have been removed. Rendering and every input path
    /// must share this geometry so their wrap and scroll math cannot drift.
    pub fn editor_content_area(&self, id: NodeId) -> Rect {
        let show_minimap = self.panes.get(&id).is_some_and(
            |runtime| matches!(runtime, PaneRuntime::Editor(editor) if editor.show_minimap),
        );
        crate::editor_chrome::compute(self.layout.pane_content_area, show_minimap).content_area
    }

    /// Routes a mouse event that landed inside the focused pane's content
    /// box: an editor pane handles its own toolbar/minimap/content
    /// sub-regions, a terminal pane's coordinates (already pane-content-
    /// relative) become a `MouseInput` request.
    pub fn handle_pane_mouse(&mut self, mouse: crossterm::event::MouseEvent, position: Position) {
        self.focus = FocusTarget::Pane;
        if !self.layout.pane_content_area.contains(position) {
            return;
        }
        let Some(id) = self.focused_pane else {
            return;
        };

        if matches!(self.panes.get(&id), Some(PaneRuntime::Editor(_))) {
            self.handle_editor_pane_mouse(id, mouse, position);
            return;
        }

        let column = position.x.saturating_sub(self.layout.pane_content_area.x);
        let row = position.y.saturating_sub(self.layout.pane_content_area.y);
        let (kind, modifiers) = crate::mouse::to_ipc_mouse_event(mouse);
        self.queue_request(ClientRequest::MouseInput {
            pane_id: id,
            kind,
            column,
            row,
            modifiers,
        });
    }

    fn handle_editor_pane_mouse(
        &mut self,
        id: NodeId,
        mouse: crossterm::event::MouseEvent,
        position: Position,
    ) {
        use crossterm::event::{MouseButton, MouseEventKind};
        let Some(PaneRuntime::Editor(editor)) = self.panes.get(&id) else {
            return;
        };
        let chrome =
            crate::editor_chrome::compute(self.layout.pane_content_area, editor.show_minimap);

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
        // Source-mode text-buffer clicks (cursor placement, selection) and
        // Rendered-mode wheel scrolling are v1 scope only for the wheel --
        // see the module docs for what this stage intentionally left out.
        if let Some(PaneRuntime::Editor(editor)) = self.panes.get_mut(&id) {
            if editor.view_mode == EditorViewMode::Source {
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
        assert_eq!(app.focused_pane, None);
        assert!(app.take_outbound_requests().is_empty());
    }

    #[test]
    fn set_pane_size_queues_a_resize_request_per_terminal_pane() {
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

        app.set_pane_size(30, 100);

        let requests = app.take_outbound_requests();
        assert_eq!(
            requests,
            vec![ClientRequest::ResizePane {
                pane_id,
                rows: 30,
                cols: 100
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
                    illium_core::AgentClass::Claude,
                    illium_core::AgentActivity::Done,
                ),
            )
            .unwrap();

        app.focus_pane(pane_id);

        assert_eq!(app.focused_pane, Some(pane_id));
        assert_eq!(app.focus, FocusTarget::Pane);
        match &app.tree.get(pane_id).unwrap().kind {
            NodeKind::Pane { status, .. } => assert_eq!(
                *status,
                PaneStatus::Agent(
                    illium_core::AgentClass::Claude,
                    illium_core::AgentActivity::Idle
                )
            ),
            _ => panic!("expected a pane"),
        }
    }

    #[test]
    fn group_for_new_node_falls_back_to_the_default_group() {
        let mut app = app();
        let group = app.group_for_new_node();
        assert_eq!(
            app.tree.get(group).map(|node| node.name.as_str()),
            Some("default")
        );
    }
}
