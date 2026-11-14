//! The server's kernel facade for the plugin host (foundation contract sections 2.3 and 12).
//!
//! Every method here runs on the plugin host's own thread, which is not a tokio worker, so the
//! facade may block: tree reads take the lock with `blocking_read`, and an intent is applied by
//! handing the equivalent `ClientRequest` to the same `handle_request` path an attached client
//! uses, driven through the server's tokio handle. A plugin therefore gets exactly the
//! validation, snapshot scheduling and broadcasts a user action gets, and nothing in this file
//! owns or touches a PTY: a trap or a stalled plugin stops here (COR-218).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use ilium_core::{
    ContainerKind as CoreContainerKind, NodeKind, PaneContentKind, SplitOrientation, Tree,
};
use ilium_ipc::{
    ClientRequest, NewPaneKind, NewPaneWorkingDirectory, PaneResizeCause, ServerEvent,
};
use ilium_plugin_api::generated::types::events::MessageEvent;
use ilium_plugin_api::generated::types::intents::{
    CreatePaneSpec, ExpandTarget, MessageSpec, MoveNodeSpec,
};
use ilium_plugin_api::generated::types::tree::{
    AgentActivity, AgentView, ContainerKind, NodeClass,
};
use ilium_plugin_api::generated::types::ui::ViewTarget;
use ilium_plugin_api::generated::types::{
    Color, ErrorCode, Event, Intent, Modifiers, NodeId, Outcome, Placement, PluginError,
    PluginSide, SplitDirection,
};
use ilium_plugin_api::kernel::{
    Cell, FacadeError, IntentResult, KernelFacade, Origin, PaneRead, SessionAccess, TerminalAccess,
    TreeAnnotations, TreeNode, TreeRead, WorkspaceAccess,
};
use ilium_plugin_api::platform::ProcessKind;
use ilium_plugin_api::side::Side;
use ilium_plugin_host::imports::basic::{BasicBackend, BasicChange};
use ilium_plugin_host::runtime::RuntimeThread;

use crate::ipc::handlers::handle_request;
use crate::ipc::DirectEventSender;
use crate::state::ServerState;

/// How long one translated request may take before the plugin sees `timed-out`. Pane creation
/// includes spawning the process, so this is far above an ordinary tree change.
const REQUEST_BUDGET: Duration = Duration::from_secs(30);
/// How long one screen read may take; the reader itself bounds a resize in flight.
const SCREEN_BUDGET: Duration = Duration::from_secs(3);
/// Replies one translated request can queue before the handler waits for the drain.
const REPLY_CAPACITY: usize = 16;

/// Content kind names of the built-in pane kinds (`NodeView::content_kind`).
pub(crate) const TERMINAL_KIND: &str = "ilium.terminal";
const EDITOR_KIND: &str = "ilium.editor";
const BOARD_KIND: &str = "ilium.board";

/// Where the runtime thread lives. The facade posts to it (a server-side message), the host
/// handle takes it out at shutdown; nobody blocks on the runtime while holding this lock.
pub(crate) type RuntimeSlot = Arc<Mutex<Option<RuntimeThread>>>;

/// The kernel as server-side plugins see it.
pub(crate) struct ServerFacade {
    // Weak: the state owns the host handle, which owns the runtime, which owns this facade.
    state: Weak<ServerState>,
    tokio: tokio::runtime::Handle,
    runtime: RuntimeSlot,
    epoch: Arc<AtomicU64>,
    next_request: AtomicU64,
    next_envelope: Arc<AtomicU64>,
    project_dir: PathBuf,
    session_name: String,
    /// What plugins keep on tree nodes, by plugin id and node id. Lives as long as the
    /// server; an entry whose node is gone is dropped the next time the store is written.
    node_metadata: Mutex<BTreeMap<(String, u64), Vec<u8>>>,
    /// The kernel settings tables: the user's `config.toml` (contract section 20.3).
    settings: ilium_plugin_host::config_settings::ConfigFileSettings,
    /// Status segments and tree decorations server plugins set, waiting for the publisher
    /// (contract section 20.4). The publisher drains them after every runtime job.
    live_chrome: Mutex<Vec<ilium_plugin_host::contributions::ContributionOp>>,
}

/// Most nodes one plugin keeps metadata on.
const METADATA_NODES_PER_PLUGIN: usize = 4096;

fn to_core(node: NodeId) -> ilium_core::NodeId {
    ilium_core::NodeId(node.0)
}

fn to_plugin(node: ilium_core::NodeId) -> NodeId {
    NodeId(node.0)
}

fn refused(code: ErrorCode, message: impl Into<String>) -> Outcome {
    Outcome::Refused(PluginError::new(code, message.into()))
}

fn failed(code: ErrorCode, message: impl Into<String>) -> Outcome {
    Outcome::Failed(PluginError::new(code, message.into()))
}

/// The most chrome changes that wait for the publisher at once.
const LIVE_CHROME_PENDING_MAX: usize = 4096;

fn unsupported(what: &str) -> Outcome {
    refused(
        ErrorCode::Unsupported,
        format!("the server kernel does not apply {what} for plugins yet"),
    )
}

/// The agent of a pane as a plugin sees it. The signature is the provider's lower-case name;
/// the objective text is not part of the tree and stays absent.
fn agent_view(state: &ilium_core::AgentState) -> AgentView {
    let activity = match state.activity() {
        ilium_core::AgentActivity::Working
        | ilium_core::AgentActivity::WaitingBackground
        | ilium_core::AgentActivity::BackgroundTaskStillRunning => AgentActivity::Working,
        ilium_core::AgentActivity::WaitingApproval => AgentActivity::AwaitingApproval,
        ilium_core::AgentActivity::Done => AgentActivity::Done,
        ilium_core::AgentActivity::Idle => AgentActivity::Idle,
    };
    let goal = state.goal.map(|goal| {
        match goal {
            ilium_core::GoalState::Active => "active",
            ilium_core::GoalState::Paused => "paused",
            ilium_core::GoalState::Blocked => "blocked",
            ilium_core::GoalState::UsageLimited => "usage-limited",
            ilium_core::GoalState::Reached => "reached",
        }
        .to_owned()
    });
    AgentView {
        signature: match &state.class {
            ilium_core::AgentClass::Claude => "claude".to_owned(),
            ilium_core::AgentClass::Codex => "codex".to_owned(),
            ilium_core::AgentClass::Antigravity => "antigravity".to_owned(),
            ilium_core::AgentClass::Other(name) => name.to_lowercase(),
        },
        activity,
        objective: None,
        goal,
    }
}

/// One tree node as a plugin sees it.
fn node_view(tree: &Tree, id: ilium_core::NodeId) -> Option<TreeNode> {
    let node = tree.get(id)?;
    let container = match &node.kind {
        NodeKind::Container(container) => Some(match &container.kind {
            CoreContainerKind::Project { .. } => ContainerKind::Project,
            CoreContainerKind::Group => ContainerKind::Group,
            CoreContainerKind::SplitView {
                orientation: SplitOrientation::Horizontal,
            } => ContainerKind::SplitHorizontal,
            CoreContainerKind::SplitView {
                orientation: SplitOrientation::Vertical,
            } => ContainerKind::SplitVertical,
            CoreContainerKind::Worktree { .. } => ContainerKind::Worktree,
        }),
        NodeKind::Folder { .. } | NodeKind::Pane { .. } => None,
    };
    let agent = match &node.kind {
        // An agent whose composer is not available keeps its identity and reads with the
        // activity `none`, so a plugin can tell it from a plain shell and from a live agent.
        NodeKind::Pane { status, .. } => status.known_agent_state().map(|state| {
            let mut view = agent_view(state);
            if status.agent_state().is_none() {
                view.activity = AgentActivity::None;
            }
            view
        }),
        NodeKind::Container(_) | NodeKind::Folder { .. } => None,
    };
    let (class, content_kind) = match &node.kind {
        NodeKind::Container(_) => (NodeClass::Container, None),
        NodeKind::Folder { .. } => (NodeClass::Folder, None),
        NodeKind::Pane { content, .. } => (
            NodeClass::Pane,
            Some(
                match content {
                    PaneContentKind::Terminal => TERMINAL_KIND,
                    PaneContentKind::Editor => EDITOR_KIND,
                    PaneContentKind::Board => BOARD_KIND,
                }
                .to_owned(),
            ),
        ),
    };
    Some(TreeNode {
        id: to_plugin(id),
        parent: node.parent.map(to_plugin),
        class,
        content_kind,
        title: node.name.clone(),
        cwd: tree
            .pane_cwd(id)
            .map(|path| path.to_string_lossy().into_owned()),
        agent,
        // `None` for a pane, which has no closed state to lock.
        locked: node.is_locked_closed().unwrap_or(false),
        revision: node.presentation_revision,
        container,
    })
}

/// Every node id in pre-order: roots by ascending id, children in the order the tree draws
/// them. `TreeRead::nodes` promises this order, and `tree.children` and `tree.roots` inherit
/// their sibling order from it.
fn preorder_ids(tree: &Tree) -> Vec<ilium_core::NodeId> {
    let mut roots: Vec<_> = tree
        .all_ids()
        .filter(|id| tree.parent_of(*id).is_none())
        .collect();
    roots.sort_by_key(|id| id.0);
    let mut ordered = Vec::new();
    let mut stack: Vec<_> = roots.into_iter().rev().collect();
    while let Some(id) = stack.pop() {
        ordered.push(id);
        if let Ok(children) = tree.children_of(id) {
            stack.extend(children.iter().rev().copied());
        }
    }
    // A node no root reaches is still readable by id, so it is still listed, last.
    let mut stray: Vec<_> = tree.all_ids().filter(|id| !ordered.contains(id)).collect();
    stray.sort_by_key(|id| id.0);
    ordered.extend(stray);
    ordered
}

/// A fingerprint of everything a plugin can read about one node, plus its place among its
/// siblings. A node whose fingerprint differs between two snapshots is listed in
/// `tree-changed`, so a rename, a move, a reorder, an agent change and a lock all reach plugins.
fn node_mark(tree: &Tree, id: ilium_core::NodeId) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let position = tree.parent_of(id).and_then(|parent| {
        tree.children_of(parent)
            .ok()
            .and_then(|children| children.iter().position(|child| *child == id))
    });
    position.hash(&mut hasher);
    format!("{:?}", node_view(tree, id)).hash(&mut hasher);
    hasher.finish()
}

/// The fingerprint of every node of a tree, by node id.
pub(crate) fn tree_marks(tree: &Tree) -> BTreeMap<u64, u64> {
    tree.all_ids()
        .map(|id| (id.0, node_mark(tree, id)))
        .collect()
}

/// What changed between two fingerprint maps: one `node-removed` per node that went away, then
/// one `tree-changed` naming every node that is new or reads differently.
pub(crate) fn tree_delta(known: &BTreeMap<u64, u64>, current: &BTreeMap<u64, u64>) -> Vec<Event> {
    let mut batch: Vec<Event> = known
        .keys()
        .filter(|id| !current.contains_key(id))
        .map(|id| Event::NodeRemoved(NodeId(*id)))
        .collect();
    batch.push(Event::TreeChanged(
        current
            .iter()
            .filter(|(id, mark)| known.get(id) != Some(mark))
            .map(|(id, _)| NodeId(*id))
            .collect(),
    ));
    batch
}

/// A POSIX single-quoted word, so an argument vector survives the shell that runs a pane's
/// command line.
fn shell_word(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:@%+,".contains(c));
    if plain {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

/// What a translated request produced.
enum RequestEnd {
    Done,
    Created(ilium_core::NodeId),
    Failed(String),
    TimedOut,
}

impl ServerFacade {
    pub(crate) fn new(
        state: &Arc<ServerState>,
        tokio: tokio::runtime::Handle,
        runtime: RuntimeSlot,
        epoch: Arc<AtomicU64>,
        next_envelope: Arc<AtomicU64>,
    ) -> Self {
        Self {
            state: Arc::downgrade(state),
            tokio,
            runtime,
            epoch,
            next_request: AtomicU64::new(1),
            next_envelope,
            project_dir: state.session_cwd.clone(),
            session_name: state.session_name.clone(),
            node_metadata: Mutex::new(BTreeMap::new()),
            settings: ilium_plugin_host::config_settings::ConfigFileSettings::of_user(),
            live_chrome: Mutex::new(Vec::new()),
        }
    }

    /// The chrome changes server plugins made since the last call, in order.
    pub(crate) fn take_live_chrome(&self) -> Vec<ilium_plugin_host::contributions::ContributionOp> {
        self.live_chrome
            .lock()
            .map(|mut pending| std::mem::take(&mut *pending))
            .unwrap_or_default()
    }

    /// Queues one chrome change of a plugin for the publisher. Only a plugin has chrome.
    fn queue_live_chrome(
        &self,
        origin: &Origin,
        change: impl FnOnce(&str) -> ilium_plugin_host::contributions::ContributionOp,
    ) -> Outcome {
        let Origin::Plugin { plugin, .. } = origin else {
            return refused(
                ErrorCode::NotPermitted,
                "only a plugin sets status segments and decorations".to_owned(),
            );
        };
        match self.live_chrome.lock() {
            Ok(mut pending) => {
                // A plugin that floods is bounded: the oldest change of the burst is dropped,
                // and the next set of the same segment or decoration replaces it anyway.
                if pending.len() >= LIVE_CHROME_PENDING_MAX {
                    pending.remove(0);
                }
                pending.push(change(&plugin.0));
                Outcome::Ok
            }
            Err(_) => refused(
                ErrorCode::Internal,
                "the chrome queue is poisoned".to_owned(),
            ),
        }
    }

    fn with_tree<R>(&self, read: impl FnOnce(&Tree) -> R) -> Option<R> {
        let state = self.state.upgrade()?;
        // The host thread is not a tokio worker, so the blocking lock is allowed here.
        let tree = state.tree.blocking_read();
        Some(read(&tree))
    }

    /// Runs one client request through the ordinary handler and reads its direct replies.
    /// `created` is the request id of a pane creation whose finish event carries the node.
    fn request(&self, request: ClientRequest, created: Option<u64>) -> RequestEnd {
        let Some(state) = self.state.upgrade() else {
            return RequestEnd::Failed("the server is shutting down".to_owned());
        };
        let Some(execution) = state.execution.get() else {
            return RequestEnd::Failed("the server execution bank is not started".to_owned());
        };
        let (sender, mut receiver) =
            DirectEventSender::admitted_channel(REPLY_CAPACITY, execution.client.clone());
        let work = async {
            let handler = async {
                let sender = sender;
                let _ = handle_request(&state, request, &sender).await;
                // Dropping the last sender ends the drain below for requests that reply inline.
            };
            let drain = async {
                let mut end = RequestEnd::Done;
                while let Some(queued) = receiver.recv_queued().await {
                    match queued.event {
                        ServerEvent::Error { message } => {
                            if matches!(end, RequestEnd::Done) {
                                end = RequestEnd::Failed(message);
                            }
                        }
                        ServerEvent::PaneCreateFinished { request_id, result }
                            if Some(request_id) == created =>
                        {
                            return match result {
                                Ok(node) => RequestEnd::Created(node),
                                Err(message) => RequestEnd::Failed(message),
                            };
                        }
                        _ => {}
                    }
                }
                end
            };
            let ((), end) = tokio::join!(handler, drain);
            end
        };
        self.tokio.block_on(async {
            match tokio::time::timeout(REQUEST_BUDGET, work).await {
                Ok(end) => end,
                Err(_) => RequestEnd::TimedOut,
            }
        })
    }

    fn simple(&self, request: ClientRequest) -> Outcome {
        match self.request(request, None) {
            RequestEnd::Done | RequestEnd::Created(_) => Outcome::Ok,
            RequestEnd::Failed(message) => failed(ErrorCode::Internal, message),
            RequestEnd::TimedOut => failed(
                ErrorCode::TimedOut,
                "the server did not finish the request in time",
            ),
        }
    }

    /// Forwards a view intent of a server plugin to every attached client, which opens the
    /// panel, settings page, onboarding step or popup (gap G3). A pane view belongs to the
    /// tree and is not forwarded.
    fn forward_view(&self, origin: &Origin, intent: &Intent) -> Outcome {
        let Origin::Plugin { plugin, .. } = origin else {
            return refused(
                ErrorCode::NotPermitted,
                "only a plugin can ask for a view to open",
            );
        };
        let target = match intent {
            Intent::OpenView(spec) => &spec.view,
            Intent::CloseView(view) => view,
            _ => return failed(ErrorCode::Internal, "not a view intent"),
        };
        if matches!(target, ViewTarget::Pane(_)) {
            return unsupported("a pane view for a server plugin");
        }
        let intent_json = match serde_json::to_vec(intent) {
            Ok(bytes) => bytes,
            Err(error) => {
                return failed(
                    ErrorCode::Internal,
                    format!("the intent does not serialize: {error}"),
                );
            }
        };
        let Some(state) = self.state.upgrade() else {
            return failed(ErrorCode::Internal, "the server is shutting down");
        };
        state.broadcast(ServerEvent::Plugin(ilium_ipc::plugin::PluginEnvelope {
            plugin: plugin.0.clone(),
            seq: self.next_envelope.fetch_add(1, Ordering::Relaxed),
            body: ilium_ipc::plugin::EnvelopeBody::ForwardedIntent(
                ilium_ipc::plugin::ForwardedIntent {
                    plugin: plugin.0.clone(),
                    target: ilium_ipc::plugin::ForwardTarget::AllClients,
                    intent_json,
                },
            ),
        }));
        Outcome::Ok
    }

    fn create_pane(&self, spec: CreatePaneSpec) -> Outcome {
        if !spec.content_kind.is_empty() && spec.content_kind != TERMINAL_KIND {
            return unsupported("create-pane for a content kind other than ilium.terminal");
        }
        let working_directory = match &spec.cwd {
            None => NewPaneWorkingDirectory::ProjectRoot,
            Some(path) if path.scope == "project" => {
                // The handler canonicalizes the directory and refuses a missing one.
                NewPaneWorkingDirectory::Directory(self.project_dir.join(&path.path))
            }
            Some(path) => {
                return refused(
                    ErrorCode::Unsupported,
                    format!("a pane cannot start in path scope {:?}", path.scope),
                );
            }
        };
        let command_line = spec.command.as_ref().map(|words| {
            words
                .iter()
                .map(|word| shell_word(word))
                .collect::<Vec<_>>()
                .join(" ")
        });
        let kind = match (command_line, spec.initial_input.clone()) {
            (None, None) => NewPaneKind::PlainShell,
            (Some(command_line), None) => NewPaneKind::Command(command_line),
            (Some(command_line), Some(initial_input)) => NewPaneKind::CommandWithInitialInput {
                command_line,
                initial_input,
            },
            (None, Some(_)) => {
                return refused(ErrorCode::InvalidArgument, "initial-input needs a command");
            }
        };
        let request_id = self.next_request.fetch_add(1, Ordering::Relaxed);
        let request = ClientRequest::NewPane {
            request_id,
            parent_group: spec.parent.map_or(ilium_core::ROOT_ID, to_core),
            kind,
            working_directory,
        };
        match self.request(request, Some(request_id)) {
            RequestEnd::Created(node) => {
                if !spec.title.is_empty() {
                    // A failed title is not a failed pane: the pane exists and is reported.
                    let _ = self.request(
                        ClientRequest::RenameNode {
                            node_id: node,
                            title: spec.title,
                            short_title: None,
                            inferred_icon: None,
                        },
                        None,
                    );
                }
                Outcome::OkNode(to_plugin(node))
            }
            RequestEnd::Done => failed(
                ErrorCode::Internal,
                "the server did not report the created pane",
            ),
            RequestEnd::Failed(message) => failed(ErrorCode::Internal, message),
            RequestEnd::TimedOut => failed(ErrorCode::TimedOut, "pane creation timed out"),
        }
    }

    fn move_node(&self, spec: MoveNodeSpec) -> Outcome {
        let node = to_core(spec.node);
        let target = to_core(spec.target);
        let place = self.with_tree(|tree| match spec.placement {
            Placement::InsideFirst => Some((target, Some(0))),
            Placement::InsideLast => Some((target, None)),
            Placement::Before | Placement::After => {
                let parent = tree.parent_of(target)?;
                let siblings = tree.children_of(parent).ok()?;
                let mut index = siblings.iter().position(|id| *id == target)?;
                // The moved node leaves its slot first when it already sits before the target
                // under the same parent.
                if tree.parent_of(node) == Some(parent)
                    && siblings.iter().position(|id| *id == node) < Some(index)
                {
                    index = index.saturating_sub(1);
                }
                if matches!(spec.placement, Placement::After) {
                    index += 1;
                }
                Some((parent, Some(index)))
            }
        });
        match place {
            Some(Some((new_parent, index))) => self.simple(ClientRequest::ReparentNode {
                node_id: node,
                new_parent,
                index,
            }),
            Some(None) => refused(ErrorCode::NotFound, "the move target has no parent"),
            None => failed(ErrorCode::Internal, "the server is shutting down"),
        }
    }

    fn message(&self, origin: &Origin, spec: MessageSpec) -> Outcome {
        let (from, from_side) = match origin {
            Origin::Plugin { plugin, side, .. } => (
                plugin.clone(),
                match side {
                    Side::Server => PluginSide::Server,
                    _ => PluginSide::Client,
                },
            ),
            _ => return refused(ErrorCode::NotPermitted, "only a plugin sends a message"),
        };
        if spec.to.plugin.0 != from.0 {
            // Plugin-to-plugin messages go through services and topics, not this intent.
            return refused(
                ErrorCode::NotPermitted,
                "a message intent reaches only the sender's own other side",
            );
        }
        match spec.to.side {
            PluginSide::Client => {
                let Some(state) = self.state.upgrade() else {
                    return failed(ErrorCode::Internal, "the server is shutting down");
                };
                state.broadcast(ServerEvent::Plugin(ilium_ipc::plugin::PluginEnvelope {
                    plugin: spec.to.plugin.0.clone(),
                    seq: self.next_envelope.fetch_add(1, Ordering::Relaxed),
                    body: ilium_ipc::plugin::EnvelopeBody::Message(
                        ilium_ipc::plugin::PluginMessage {
                            topic: spec.channel,
                            payload: spec.payload,
                        },
                    ),
                }));
                Outcome::Ok
            }
            PluginSide::Server => {
                let target = spec.to.plugin.0.clone();
                let event = Event::Message(MessageEvent {
                    from,
                    from_side,
                    from_client: None,
                    channel: spec.channel,
                    payload: spec.payload,
                });
                // Queued, never delivered inline: this call already runs inside the runtime.
                let posted = self.runtime.lock().ok().is_some_and(|slot| {
                    slot.as_ref().is_some_and(|thread| {
                        thread.post(move |runtime| {
                            if let Err(error) = runtime.deliver(&target, vec![event]) {
                                tracing::debug!("plugin message not delivered: {error}");
                            }
                            for error in runtime.pump() {
                                tracing::warn!("plugin host: {error}");
                            }
                        })
                    })
                });
                if posted {
                    Outcome::Ok
                } else {
                    failed(ErrorCode::Busy, "the plugin host is stopping")
                }
            }
        }
    }

    fn screen<R: Send>(
        &self,
        pane: NodeId,
        read: impl Fn(&vt100::Screen) -> R + Send,
    ) -> Option<R> {
        let state = self.state.upgrade()?;
        self.tokio.block_on(async {
            tokio::time::timeout(
                SCREEN_BUDGET,
                crate::pane::read_current_terminal_screen(&state, to_core(pane), read),
            )
            .await
            .ok()
            .flatten()
        })
    }
}

impl TreeAnnotations for ServerFacade {
    fn metadata(&self, plugin: &str, node: NodeId) -> Option<Vec<u8>> {
        // A removed node has no metadata, whatever the store still holds for its id.
        self.with_tree(|tree| tree.get(to_core(node)).is_some())
            .filter(|exists| *exists)?;
        let store = self
            .node_metadata
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        store.get(&(plugin.to_owned(), node.0)).cloned()
    }

    fn set_metadata(&self, plugin: &str, node: NodeId, bytes: Vec<u8>) -> Result<(), FacadeError> {
        let Some(live) = self.with_tree(|tree| {
            preorder_ids(tree)
                .into_iter()
                .map(|id| id.0)
                .collect::<std::collections::BTreeSet<u64>>()
        }) else {
            return Err(FacadeError::new(ErrorCode::NotFound, "no such node"));
        };
        if !live.contains(&node.0) {
            return Err(FacadeError::new(ErrorCode::NotFound, "no such node"));
        }
        let mut store = self
            .node_metadata
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        store.retain(|(_, kept), _| live.contains(kept));
        let key = (plugin.to_owned(), node.0);
        if bytes.is_empty() {
            store.remove(&key);
            return Ok(());
        }
        let held = store.keys().filter(|(owner, _)| owner == plugin).count();
        if !store.contains_key(&key) && held >= METADATA_NODES_PER_PLUGIN {
            return Err(FacadeError::new(
                ErrorCode::QuotaExceeded,
                "the plugin keeps metadata on too many nodes",
            ));
        }
        store.insert(key, bytes);
        Ok(())
    }
}

/// One cell of a terminal screen as plugins read it.
fn plugin_cell(cell: &vt100::Cell) -> Cell {
    let colour = |colour: vt100::Color| match colour {
        vt100::Color::Default => None,
        vt100::Color::Idx(index) => Some(Color::Indexed(index)),
        vt100::Color::Rgb(red, green, blue) => Some(Color::Rgb(
            (u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue),
        )),
    };
    let mut add = 0;
    for (set, flag) in [
        (cell.bold(), Modifiers::BOLD),
        (cell.dim(), Modifiers::DIM),
        (cell.italic(), Modifiers::ITALIC),
        (cell.underline(), Modifiers::UNDERLINED),
        (cell.inverse(), Modifiers::REVERSED),
    ] {
        if set {
            add |= flag.0;
        }
    }
    Cell {
        text: cell.contents().to_owned(),
        fg: colour(cell.fgcolor()),
        bg: colour(cell.bgcolor()),
        add: Modifiers(add),
        wide: cell.is_wide(),
    }
}

impl KernelFacade for ServerFacade {
    fn annotations(&self) -> Option<&dyn TreeAnnotations> {
        Some(self)
    }

    fn apply_intent(&self, origin: &Origin, intent: Intent) -> IntentResult {
        match intent {
            Intent::RenameNode(spec) => {
                let node = to_core(spec.node);
                if let Some(expected) = spec.expected_revision {
                    let current = self
                        .with_tree(|tree| tree.get(node).map(|node| node.presentation_revision))
                        .flatten();
                    match current {
                        None => return refused(ErrorCode::NotFound, "no such node"),
                        Some(current) if current != expected => {
                            return refused(
                                ErrorCode::Conflict,
                                "the node changed since the plugin read it",
                            );
                        }
                        Some(_) => {}
                    }
                }
                self.simple(ClientRequest::RenameNode {
                    node_id: node,
                    title: spec.title,
                    short_title: None,
                    inferred_icon: None,
                })
            }
            Intent::MoveNode(spec) => self.move_node(spec),
            Intent::CloseNode(node) => self.simple(ClientRequest::ClosePane {
                pane_id: to_core(node),
            }),
            Intent::SetNodeLocked(spec) => self.simple(ClientRequest::SetNodeLockedClosed {
                node_id: to_core(spec.node),
                locked_closed: spec.locked,
            }),
            Intent::CreatePane(spec) => self.create_pane(spec),
            Intent::ResizePane(spec) => {
                let current = self.panes().size(spec.pane);
                let columns = spec.columns.or(current.map(|size| size.0));
                let rows = spec.rows.or(current.map(|size| size.1));
                match (columns, rows) {
                    (Some(cols), Some(rows)) => self.simple(ClientRequest::ResizePane {
                        pane_id: to_core(spec.pane),
                        rows,
                        cols,
                        cause: PaneResizeCause::HostTerminal,
                    }),
                    _ => refused(ErrorCode::NotFound, "the pane has no terminal to resize"),
                }
            }
            Intent::CreateGroup(spec) => {
                if !spec.children.is_empty() {
                    return unsupported("create-group with initial children");
                }
                self.simple(ClientRequest::NewGroup {
                    parent_group: spec.parent.map_or(ilium_core::ROOT_ID, to_core),
                    name: spec.title,
                })
            }
            Intent::CreateSplit(spec) => {
                if spec.panes.len() > ilium_core::MAXIMUM_SPLIT_VIEW_PANES {
                    return refused(
                        ErrorCode::InvalidArgument,
                        "a split view holds at most four panes",
                    );
                }
                // The named group wins; else the group of the first pane; an empty split with
                // no group goes to the root.
                let parent = match (spec.parent, spec.panes.first().copied()) {
                    (Some(parent), _) => to_core(parent),
                    (None, Some(first)) => self
                        .with_tree(|tree| tree.parent_of(to_core(first)))
                        .flatten()
                        .unwrap_or(ilium_core::ROOT_ID),
                    (None, None) => ilium_core::ROOT_ID,
                };
                let (name, orientation) = match spec.direction {
                    SplitDirection::Horizontal => {
                        ("Horizontal split", SplitOrientation::Horizontal)
                    }
                    SplitDirection::Vertical => ("Vertical split", SplitOrientation::Vertical),
                };
                self.simple(ClientRequest::CreateSplitView {
                    parent_group: parent,
                    name: name.to_owned(),
                    orientation,
                    pane_ids: spec.panes.into_iter().map(to_core).collect(),
                })
            }
            Intent::SetBookmarked(spec) => self.simple(ClientRequest::SetNodeBookmarked {
                node_id: to_core(spec.node),
                is_bookmarked: spec.bookmarked,
            }),
            Intent::SetExpanded(spec) => match spec.target {
                ExpandTarget::Node(node) => self.simple(ClientRequest::SetNodeExpanded {
                    node_id: to_core(node),
                    expanded: spec.expanded,
                }),
                ExpandTarget::VirtualRow(_) => unsupported("set-expanded on a virtual row"),
            },
            Intent::Message(spec) => self.message(origin, spec),
            // Chrome of a server plugin is drawn by the clients: the publisher carries it
            // to them as `live.*` entries (contract section 20.4).
            Intent::SetStatusSegment(spec) => self.queue_live_chrome(origin, |plugin| {
                ilium_plugin_host::contributions::ContributionOp::Put(
                    ilium_plugin_host::wiring::live_segment_entry(plugin, &spec),
                )
            }),
            Intent::ClearStatusSegment(segment) => self.queue_live_chrome(origin, |plugin| {
                ilium_plugin_host::contributions::ContributionOp::Remove(
                    ilium_plugin_host::wiring::live_segment_id(plugin, &segment),
                )
            }),
            Intent::SetDecoration(spec) => self.queue_live_chrome(origin, |plugin| {
                ilium_plugin_host::contributions::ContributionOp::Put(
                    ilium_plugin_host::wiring::live_decoration_entry(plugin, &spec),
                )
            }),
            Intent::ClearDecoration(key) => self.queue_live_chrome(origin, |plugin| {
                ilium_plugin_host::contributions::ContributionOp::Remove(
                    ilium_plugin_host::wiring::live_decoration_id(plugin, &key.slot, key.node.0),
                )
            }),
            Intent::SignalPane(spec) => {
                use ilium_plugin_api::generated::types::tree::Signal;
                let signal = match spec.signal {
                    Signal::Interrupt => ilium_ipc::PaneSignal::Interrupt,
                    Signal::Terminate => ilium_ipc::PaneSignal::Terminate,
                    Signal::Kill => ilium_ipc::PaneSignal::Kill,
                    Signal::Hangup => ilium_ipc::PaneSignal::Hangup,
                };
                self.simple(ClientRequest::SignalPaneProcess {
                    pane_id: to_core(spec.pane),
                    signal,
                })
            }
            // Views of a plugin's own client side open in the clients, not in the server.
            Intent::OpenView(_) | Intent::CloseView(_) => self.forward_view(origin, &intent),
            // The remaining intents belong to features that are not ported yet; each port adds
            // its arm here together with its handler (foundation contract section 12.4).
            _ => unsupported("this intent"),
        }
    }

    fn tree(&self) -> &dyn TreeRead {
        self
    }

    fn panes(&self) -> &dyn PaneRead {
        self
    }

    fn process_kind(&self) -> ProcessKind {
        ProcessKind::Server
    }

    fn workspace(&self) -> Option<&dyn WorkspaceAccess> {
        Some(self)
    }

    fn terminal(&self) -> Option<&dyn TerminalAccess> {
        Some(self)
    }

    fn sessions(&self) -> Option<&dyn SessionAccess> {
        Some(self)
    }

    fn settings(&self) -> Option<&dyn ilium_plugin_api::kernel::KernelSettings> {
        Some(&self.settings)
    }
}

impl TreeRead for ServerFacade {
    fn nodes(&self) -> Vec<TreeNode> {
        self.with_tree(|tree| {
            preorder_ids(tree)
                .into_iter()
                .filter_map(|id| node_view(tree, id))
                .collect()
        })
        .unwrap_or_default()
    }

    fn node(&self, id: NodeId) -> Option<TreeNode> {
        self.with_tree(|tree| node_view(tree, to_core(id)))
            .flatten()
    }

    fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Relaxed)
    }
}

impl PaneRead for ServerFacade {
    fn screen_text(&self, pane: NodeId) -> Option<String> {
        self.screen(pane, |screen| screen.contents())
    }

    /// The styled cells of visible rows. The second half of a wide character is left out, so
    /// a row with wide characters holds fewer cells than the pane has columns.
    fn cells(&self, pane: NodeId, rows: std::ops::Range<u16>) -> Option<Vec<Vec<Cell>>> {
        self.screen(pane, move |screen| {
            let (height, width) = screen.size();
            (rows.start..rows.end.min(height))
                .map(|row| {
                    (0..width)
                        .filter_map(|column| screen.cell(row, column))
                        .filter(|cell| !cell.is_wide_continuation())
                        .map(plugin_cell)
                        .collect()
                })
                .collect()
        })
    }

    /// Scrollback lines are not readable here: the pane owner hands out its screen read-only
    /// and the terminal emulator needs a write to show a scrolled-back view. Answers none
    /// until the pane owner serves history lines (foundation contract section 17.4).
    fn history(&self, _pane: NodeId, _lines: u32) -> Option<Vec<String>> {
        None
    }

    fn size(&self, pane: NodeId) -> Option<(u16, u16)> {
        self.screen(pane, |screen| {
            let (rows, columns) = screen.size();
            (columns, rows)
        })
    }
}

impl ServerFacade {
    /// Runs a request for a facade call that reports a `FacadeError`.
    fn checked(&self, request: ClientRequest) -> Result<(), FacadeError> {
        match self.request(request, None) {
            RequestEnd::Done | RequestEnd::Created(_) => Ok(()),
            RequestEnd::Failed(message) => Err(FacadeError::new(ErrorCode::Internal, message)),
            RequestEnd::TimedOut => Err(FacadeError::new(
                ErrorCode::TimedOut,
                "the server did not finish the request in time",
            )),
        }
    }

    /// The directory of this project's session snapshots.
    fn sessions_dir(&self) -> PathBuf {
        self.project_dir.join(".ilium").join("sessions")
    }
}

/// Terminal input and control (contract section 19.3). The PTY stays with its owner: every
/// call goes through the ordinary request handler, exactly as a client's would.
impl TerminalAccess for ServerFacade {
    fn write_input(&self, pane: NodeId, bytes: &[u8]) -> Result<(), FacadeError> {
        self.checked(ClientRequest::KeyInput {
            pane_id: to_core(pane),
            bytes: bytes.to_vec(),
            submission: None,
        })
    }

    fn resize(&self, pane: NodeId, columns: u16, rows: u16) -> Result<(), FacadeError> {
        self.checked(ClientRequest::ResizePane {
            pane_id: to_core(pane),
            rows,
            cols: columns,
            cause: PaneResizeCause::HostTerminal,
        })
    }

    /// A signal is sent as the control byte the terminal line discipline turns into it, so
    /// it reaches the foreground process the way a key press would. Other signals need
    /// process control the facade does not have.
    fn signal(&self, pane: NodeId, signal: &str) -> Result<(), FacadeError> {
        let byte = match signal {
            "interrupt" => 0x03,
            "end-of-file" => 0x04,
            "quit" => 0x1c,
            "suspend" => 0x1a,
            other => {
                return Err(FacadeError::new(
                    ErrorCode::Unsupported,
                    format!("signal {other} cannot be sent to a pane"),
                ));
            }
        };
        self.write_input(pane, &[byte])
    }
}

/// Session snapshots of this project (contract section 19.3).
impl SessionAccess for ServerFacade {
    fn names(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(self.sessions_dir()) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .filter_map(|path| {
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(str::to_owned)
            })
            .collect();
        names.sort();
        names
    }

    fn snapshot_json(&self, name: &str) -> Option<String> {
        // A session name is one file name; anything that could leave the directory is refused.
        let plain = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
            && !name.starts_with('.');
        if !plain {
            return None;
        }
        std::fs::read_to_string(self.sessions_dir().join(format!("{name}.json"))).ok()
    }

    /// The server writes its snapshot on its own schedule; there is no request that forces
    /// one, so this is refused instead of pretending.
    fn save(&self) -> Result<(), FacadeError> {
        Err(FacadeError::new(
            ErrorCode::Unsupported,
            "the server saves the session snapshot itself",
        ))
    }
}

impl WorkspaceAccess for ServerFacade {
    fn project_dir(&self) -> Option<String> {
        Some(self.project_dir.to_string_lossy().into_owned())
    }

    fn session_name(&self) -> Option<String> {
        Some(self.session_name.clone())
    }
}

impl BasicBackend for ServerFacade {
    fn apply_basic(
        &self,
        _plugin: &str,
        pane: NodeId,
        change: BasicChange,
    ) -> Result<(), PluginError> {
        match change {
            BasicChange::Title(title) => {
                match self.request(
                    ClientRequest::RenameNode {
                        node_id: to_core(pane),
                        title,
                        short_title: None,
                        inferred_icon: None,
                    },
                    None,
                ) {
                    RequestEnd::Done | RequestEnd::Created(_) => Ok(()),
                    RequestEnd::Failed(message) => {
                        Err(PluginError::new(ErrorCode::Internal, message))
                    }
                    RequestEnd::TimedOut => Err(PluginError::new(
                        ErrorCode::TimedOut,
                        "the server did not finish the rename in time".to_owned(),
                    )),
                }
            }
            // Status text, focus, activity and the dirty mark need pane-kind state that the
            // primitive ports add (foundation contract section 12.4).
            _ => Err(PluginError::new(
                ErrorCode::Unsupported,
                "this basic pane change is not wired on the server yet".to_owned(),
            )),
        }
    }

    fn basic_size(&self, _plugin: &str, pane: NodeId) -> Option<(u16, u16)> {
        PaneRead::size(self, pane)
    }
}

#[cfg(test)]
mod tests {
    use super::shell_word;

    #[test]
    fn plain_words_stay_bare_and_others_are_single_quoted() {
        assert_eq!(shell_word("cargo"), "cargo");
        assert_eq!(shell_word("--flag=1"), "--flag=1");
        assert_eq!(shell_word("two words"), "'two words'");
        assert_eq!(shell_word(""), "''");
        assert_eq!(shell_word("it's"), "'it'\\''s'");
    }
}
