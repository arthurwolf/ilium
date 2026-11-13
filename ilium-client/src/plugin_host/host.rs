//! The real plugin host of the client (foundation contract sections 2.4, 2.5 and 12).
//!
//! The client runs the client sides of plugins on the host's own thread. The event loop never
//! waits for that thread: it posts work to it and collects what it produced (requests for the
//! server, toasts, redraws) from an outbox when the host signals. The host thread sees the
//! session only through a mirror the event loop refreshes, never through the `App`, and this
//! module touches no PTY, terminal parser or process table: the client still renders only what
//! the server sends.
//!
//! An intent of a client-side plugin is applied by the server. The facade sends it as a plugin
//! envelope and holds the plugin's call until the outcome envelope returns or a short timeout
//! passes, so a plugin is never told `ok` for something the server did not do.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use ilium_core::{
    ContainerKind as CoreContainerKind, NodeKind, PaneContentKind, SplitOrientation, Tree,
};
use ilium_ipc::plugin::{
    ContributionDelta, ContributionOp as WireOp, ControlBody, DeltaKind, EnvelopeBody,
    InstalledPlugin, PluginEnvelope, PluginHello, PluginMessage, Submission as WireSubmission,
    PLUGIN_PROTOCOL_VERSION,
};
use ilium_ipc::{ClientRequest, ServerEvent};
use ilium_plugin_api::generated::host::log::Level;
use ilium_plugin_api::generated::types::events::MessageEvent;
use ilium_plugin_api::generated::types::tree::{
    AgentActivity, AgentView, ContainerKind, NodeClass,
};
use ilium_plugin_api::generated::types::{
    ActionKind, Area, CommandContext, CommandOrigin, ErrorCode, Event, Intent, NodeId,
    OpenViewSpec, Outcome, PluginError, PluginId, PluginSide, UiAction, UiTree, ViewTarget,
};
use ilium_plugin_api::kernel::{
    Cell, FacadeError, IntentResult, KernelFacade, Origin, PaneRead, TreeNode, TreeRead, UiSink,
    WorkspaceAccess,
};
use ilium_plugin_api::platform::ProcessKind;
use ilium_plugin_api::side::Side;
use ilium_plugin_host::contributions::Entry;
use ilium_plugin_host::engine::{EngineConfig, PluginEngine};
use ilium_plugin_host::imports::backed::LogSink;
use ilium_plugin_host::lifecycle::StartMode;
use ilium_plugin_host::runtime::{
    CataloguePlugin, HostRuntime, RuntimeConfig, RuntimeError, RuntimeThread,
};
use ilium_plugin_host::storage::StorageContext;
use ilium_plugin_host::wiring::{self, PluginRow};

/// How long a client-side plugin waits for the server's outcome of one intent.
const OUTCOME_WAIT: Duration = Duration::from_secs(2);
/// How long plugins get to deactivate when the client exits (contract section 2.4).
const SHUTDOWN_BUDGET: Duration = Duration::from_secs(2);
/// How long the client waits for the host thread to hand back its shutdown report.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(3);
/// Outputs the event loop has not collected yet; beyond this the oldest are dropped.
const OUTBOX_LIMIT: usize = 1024;
/// The epoch tick of the client engine in milliseconds.
const EPOCH_TICK_MS: u64 = 10;

/// What the composition root hands the client to run plugins.
#[derive(Debug, Clone)]
pub struct ClientPluginStart {
    /// The plugins this build ships, manifests and client components.
    pub catalogue: Vec<CataloguePlugin>,
    /// The user data directory (`<data_dir>/plugins` holds the registry, storage and cache).
    pub data_dir: PathBuf,
    /// Normal, safe or no plugins. `NoPlugins` starts no host at all.
    pub mode: StartMode,
}

impl ClientPluginStart {
    /// What the client tells the server about its plugins when it attaches.
    pub fn hello(&self) -> PluginHello {
        PluginHello {
            plugin_protocol: PLUGIN_PROTOCOL_VERSION,
            contract: ilium_plugin_api::CONTRACT.to_string(),
            installed: self
                .catalogue
                .iter()
                .map(|plugin| InstalledPlugin {
                    id: plugin.id.clone(),
                    version: wiring::manifest_version(&plugin.manifest),
                    // The content hash is filled once packages are verified at start
                    // (foundation contract section 12.4); bundled plugins match by build.
                    content_hash: [0; 32],
                })
                .collect(),
        }
    }
}

/// What the host thread asks the event loop to do.
#[derive(Debug)]
pub(crate) enum Output {
    /// Send this to the server.
    Request(ClientRequest),
    /// Show a toast a plugin asked for (toast spec as JSON).
    Toast { plugin: String, toast_json: String },
    /// A plugin view changed.
    Redraw,
    /// A plugin opened a popup; the event loop gives it the screen if no other modal has it.
    PopupOpened { plugin: String, popup: String },
    /// The open popup of a plugin went away (the plugin closed it or its view failed).
    PopupClosed { plugin: String, popup: String },
}

/// The one popup a plugin has open, with the view the plugin last rendered for it.
#[derive(Debug, Clone)]
struct PopupSlot {
    plugin: String,
    popup: String,
    tree: Option<UiTree>,
}

/// State the host thread and the event loop share. Every lock is held for a few statements
/// and never across a call into the runtime.
struct Shared {
    nodes: Mutex<Vec<TreeNode>>,
    focused: Mutex<Option<NodeId>>,
    epoch: AtomicU64,
    outbox: Mutex<VecDeque<Output>>,
    notification: Arc<tokio::sync::Notify>,
    waiting: Mutex<HashMap<u64, mpsc::Sender<Outcome>>>,
    next_seq: AtomicU64,
    stopping: AtomicBool,
    panels: Mutex<BTreeMap<(String, String), String>>,
    /// The open plugin popup, if any. One at a time: the kernel has one modal layer.
    popup: Mutex<Option<PopupSlot>>,
    /// Columns and rows of the screen, as the event loop last reported them.
    screen: Mutex<(u16, u16)>,
    runtime: Mutex<Option<RuntimeThread>>,
    project_dir: String,
    session_name: String,
}

impl Shared {
    fn push(&self, output: Output) {
        if let Ok(mut outbox) = self.outbox.lock() {
            if outbox.len() >= OUTBOX_LIMIT {
                outbox.pop_front();
            }
            outbox.push_back(output);
        }
        self.notification.notify_one();
    }

    fn envelope(&self, plugin: &str, body: EnvelopeBody) -> (u64, ClientRequest) {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        (
            seq,
            ClientRequest::Plugin(PluginEnvelope {
                plugin: plugin.to_owned(),
                seq,
                body,
            }),
        )
    }

    fn post(&self, job: impl FnOnce(&mut HostRuntime) + Send + 'static) -> bool {
        self.runtime
            .lock()
            .ok()
            .is_some_and(|slot| slot.as_ref().is_some_and(|thread| thread.post(job)))
    }

    /// Records the popup a plugin opened and tells the event loop.
    fn open_popup(&self, plugin: &str, popup: String) {
        if let Ok(mut slot) = self.popup.lock() {
            *slot = Some(PopupSlot {
                plugin: plugin.to_owned(),
                popup: popup.clone(),
                tree: None,
            });
        }
        self.push(Output::PopupOpened {
            plugin: plugin.to_owned(),
            popup,
        });
    }

    /// Forgets the popup if it is the open one. Returns whether it was.
    fn close_popup(&self, plugin: &str, popup: &str) -> bool {
        let was_open = self.popup.lock().is_ok_and(|mut slot| {
            let is_open = slot
                .as_ref()
                .is_some_and(|open| open.plugin == plugin && open.popup == popup);
            if is_open {
                *slot = None;
            }
            is_open
        });
        if was_open {
            self.push(Output::PopupClosed {
                plugin: plugin.to_owned(),
                popup: popup.to_owned(),
            });
        }
        was_open
    }

    fn set_screen(&self, screen: (u16, u16)) {
        if let Ok(mut slot) = self.screen.lock() {
            *slot = screen;
        }
    }

    /// Asks the owning plugin for the current view of the open popup. Runs on the host
    /// thread after anything that may have changed the plugin's state; never called from
    /// inside a plugin call. A view that fails to render closes the popup.
    fn refresh_popup(&self, runtime: &mut HostRuntime) {
        let Some((plugin, popup)) = self.popup.lock().ok().and_then(|slot| {
            slot.as_ref()
                .map(|open| (open.plugin.clone(), open.popup.clone()))
        }) else {
            return;
        };
        let (columns, rows) = self.screen.lock().map(|screen| *screen).unwrap_or((0, 0));
        match runtime.render(
            &plugin,
            ViewTarget::Popup(popup.clone()),
            Area { columns, rows },
        ) {
            Ok(tree) => {
                if let Ok(mut slot) = self.popup.lock() {
                    if let Some(open) = slot
                        .as_mut()
                        .filter(|open| open.plugin == plugin && open.popup == popup)
                    {
                        open.tree = Some(tree);
                    }
                }
                self.push(Output::Redraw);
            }
            Err(error) => {
                tracing::warn!("popup {popup} of {plugin} did not render: {error}");
                self.close_popup(&plugin, &popup);
            }
        }
    }
}

fn failed(code: ErrorCode, message: &str) -> Outcome {
    Outcome::Failed(PluginError::new(code, message))
}

/// The kernel as client-side plugins see it.
struct ClientFacade {
    shared: Arc<Shared>,
}

impl ClientFacade {
    /// Sends an intent to the server and waits for its outcome.
    fn submit(&self, plugin: &str, intent: &Intent) -> Outcome {
        if self.shared.stopping.load(Ordering::Acquire) {
            return Outcome::Cancelled;
        }
        let intent_json = match serde_json::to_string(intent) {
            Ok(text) => text,
            Err(error) => {
                return failed(
                    ErrorCode::Internal,
                    &format!("the intent does not serialize: {error}"),
                );
            }
        };
        let seq = self.shared.next_seq.fetch_add(1, Ordering::Relaxed);
        let (reply, outcome) = mpsc::channel();
        match self.shared.waiting.lock() {
            Ok(mut waiting) => {
                waiting.insert(seq, reply);
            }
            Err(_) => return failed(ErrorCode::Internal, "the plugin host state is poisoned"),
        }
        self.shared
            .push(Output::Request(ClientRequest::Plugin(PluginEnvelope {
                plugin: plugin.to_owned(),
                seq,
                body: EnvelopeBody::Submit(WireSubmission {
                    client_id: String::new(),
                    dedup_key: seq.to_string(),
                    intent_json,
                }),
            })));
        let result = outcome.recv_timeout(OUTCOME_WAIT);
        if let Ok(mut waiting) = self.shared.waiting.lock() {
            waiting.remove(&seq);
        }
        result.unwrap_or_else(|_| {
            failed(
                ErrorCode::TimedOut,
                "the server did not answer the intent in time; it may still be applied",
            )
        })
    }

    fn message(&self, plugin: &PluginId, intent: Intent) -> Outcome {
        let Intent::Message(spec) = intent else {
            return failed(ErrorCode::Internal, "not a message");
        };
        if spec.to.plugin.0 != plugin.0 {
            return Outcome::Refused(PluginError::new(
                ErrorCode::NotPermitted,
                "a message intent reaches only the sender's own other side",
            ));
        }
        match spec.to.side {
            PluginSide::Server => {
                let (_, request) = self.shared.envelope(
                    &plugin.0,
                    EnvelopeBody::Message(PluginMessage {
                        topic: spec.channel,
                        payload: spec.payload,
                    }),
                );
                self.shared.push(Output::Request(request));
                Outcome::Ok
            }
            PluginSide::Client => {
                let target = plugin.0.clone();
                let event = Event::Message(MessageEvent {
                    from: plugin.clone(),
                    from_side: PluginSide::Client,
                    from_client: None,
                    channel: spec.channel,
                    payload: spec.payload,
                });
                // Queued, never delivered inline: this call already runs inside the runtime.
                let posted = self.shared.post(move |runtime| {
                    if let Err(error) = runtime.deliver(&target, vec![event]) {
                        tracing::debug!("plugin message not delivered: {error}");
                    }
                    log_errors("pump", runtime.pump());
                });
                if posted {
                    Outcome::Ok
                } else {
                    failed(ErrorCode::Busy, "the plugin host is stopping")
                }
            }
        }
    }
}

impl KernelFacade for ClientFacade {
    fn apply_intent(&self, origin: &Origin, intent: Intent) -> IntentResult {
        let Origin::Plugin { plugin, .. } = origin else {
            return Outcome::Refused(PluginError::new(
                ErrorCode::NotPermitted,
                "the client kernel applies intents of plugins only",
            ));
        };
        match intent {
            Intent::Message(_) => self.message(plugin, intent),
            // A popup is client-local: the kernel's modal layer shows it, the server never
            // hears of it. Other views still go to the server until their seams exist.
            Intent::OpenView(OpenViewSpec {
                view: ViewTarget::Popup(popup),
                ..
            }) => {
                self.shared.open_popup(&plugin.0, popup);
                Outcome::Ok
            }
            Intent::CloseView(ViewTarget::Popup(popup)) => {
                self.shared.close_popup(&plugin.0, &popup);
                Outcome::Ok
            }
            other => self.submit(&plugin.0, &other),
        }
    }

    fn tree(&self) -> &dyn TreeRead {
        self
    }

    fn panes(&self) -> &dyn PaneRead {
        self
    }

    fn process_kind(&self) -> ProcessKind {
        ProcessKind::Client
    }

    fn workspace(&self) -> Option<&dyn WorkspaceAccess> {
        Some(self)
    }

    fn ui(&self) -> Option<&dyn UiSink> {
        Some(self)
    }

    fn focused_pane(&self) -> Option<NodeId> {
        self.shared.focused.lock().ok().and_then(|focused| *focused)
    }
}

impl TreeRead for ClientFacade {
    fn nodes(&self) -> Vec<TreeNode> {
        self.shared
            .nodes
            .lock()
            .map(|nodes| nodes.clone())
            .unwrap_or_default()
    }

    fn node(&self, id: NodeId) -> Option<TreeNode> {
        self.shared
            .nodes
            .lock()
            .ok()?
            .iter()
            .find(|node| node.id == id)
            .cloned()
    }

    fn epoch(&self) -> u64 {
        self.shared.epoch.load(Ordering::Relaxed)
    }
}

/// Pane content is not mirrored for client plugins yet: the client renders screens from the
/// server's stream and hands none of it to the host (foundation contract section 12.4).
impl PaneRead for ClientFacade {
    fn screen_text(&self, _pane: NodeId) -> Option<String> {
        None
    }

    fn cells(&self, _pane: NodeId, _rows: std::ops::Range<u16>) -> Option<Vec<Vec<Cell>>> {
        None
    }

    fn history(&self, _pane: NodeId, _lines: u32) -> Option<Vec<String>> {
        None
    }
}

impl WorkspaceAccess for ClientFacade {
    fn project_dir(&self) -> Option<String> {
        Some(self.shared.project_dir.clone())
    }

    fn session_name(&self) -> Option<String> {
        Some(self.shared.session_name.clone())
    }
}

impl UiSink for ClientFacade {
    fn put_panel(
        &self,
        plugin: &PluginId,
        panel: &str,
        tree_json: &str,
    ) -> Result<(), FacadeError> {
        let mut panels =
            self.shared.panels.lock().map_err(|_| {
                FacadeError::new(ErrorCode::Internal, "the panel store is poisoned")
            })?;
        panels.insert((plugin.0.clone(), panel.to_owned()), tree_json.to_owned());
        drop(panels);
        self.shared.push(Output::Redraw);
        Ok(())
    }

    fn remove_panel(&self, plugin: &PluginId, panel: &str) {
        if let Ok(mut panels) = self.shared.panels.lock() {
            panels.remove(&(plugin.0.clone(), panel.to_owned()));
        }
        self.shared.push(Output::Redraw);
    }

    fn toast(&self, plugin: &PluginId, toast_json: &str) -> Result<(), FacadeError> {
        self.shared.push(Output::Toast {
            plugin: plugin.0.clone(),
            toast_json: toast_json.to_owned(),
        });
        Ok(())
    }

    fn invalidate(&self, _plugin: &PluginId, _view: &str) {
        self.shared.push(Output::Redraw);
    }
}

/// Plugin log lines join the client log.
struct TracingLog;

impl LogSink for TracingLog {
    fn line(&self, plugin: &str, side: Side, level: Level, message: &str) {
        match level {
            Level::Trace => tracing::trace!(plugin, ?side, "{message}"),
            Level::Debug => tracing::debug!(plugin, ?side, "{message}"),
            Level::Info => tracing::info!(plugin, ?side, "{message}"),
            Level::Warn => tracing::warn!(plugin, ?side, "{message}"),
            Level::Error => tracing::error!(plugin, ?side, "{message}"),
        }
    }
}

fn log_errors(what: &str, errors: Vec<RuntimeError>) {
    for error in errors {
        tracing::warn!("plugin host, {what}: {error}");
    }
}

fn to_plugin(node: ilium_core::NodeId) -> NodeId {
    NodeId(node.0)
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

/// One tree node as a plugin sees it. The server facade builds the same view from its tree.
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
                    PaneContentKind::Terminal => "ilium.terminal",
                    PaneContentKind::Editor => "ilium.editor",
                    PaneContentKind::Board => "ilium.board",
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
fn tree_marks(tree: &Tree) -> BTreeMap<u64, u64> {
    tree.all_ids()
        .map(|id| (id.0, node_mark(tree, id)))
        .collect()
}

/// What changed between two fingerprint maps: one `node-removed` per node that went away, then
/// one `tree-changed` naming every node that is new or reads differently.
fn tree_delta(known: &BTreeMap<u64, u64>, current: &BTreeMap<u64, u64>) -> Vec<Event> {
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

/// The running plugin host of one client. Owned by the `App`; every method returns at once.
pub(crate) struct ClientPlugins {
    shared: Arc<Shared>,
    /// Manifests of the catalogue, for the plugin list.
    catalogue: Vec<CataloguePlugin>,
    /// Contributions of the server sides, as the server last reported them.
    server_entries: BTreeMap<String, Entry>,
    /// Plugin states as the server last reported them.
    server_states: BTreeMap<String, String>,
    /// Node ids of the last mirrored tree, to tell plugins what was removed.
    known: BTreeMap<u64, u64>,
    /// The last refusal of an enable or disable request, for the plugin list.
    last_refusal: Option<String>,
}

impl std::fmt::Debug for ClientPlugins {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ClientPlugins")
            .field("plugins", &self.catalogue.len())
            .finish_non_exhaustive()
    }
}

impl ClientPlugins {
    /// Starts the host. `None` when plugins are off or the host could not be built; the
    /// client then runs without plugins and says so in its log.
    pub(crate) fn start(
        start: ClientPluginStart,
        session_name: &str,
        session_cwd: &std::path::Path,
    ) -> Option<Self> {
        if matches!(start.mode, StartMode::NoPlugins) {
            tracing::info!("plugins are off for this client (--no-plugins)");
            return None;
        }
        let cache_dir = start.data_dir.join("plugins").join("cache");
        if let Err(error) = std::fs::create_dir_all(&cache_dir) {
            tracing::warn!(
                "plugin code cache {} is unavailable: {error}",
                cache_dir.display()
            );
        }
        let engine = match PluginEngine::new(EngineConfig {
            epoch_tick_ms: EPOCH_TICK_MS,
            cache_dir: Some(cache_dir),
            process: ProcessKind::Client,
        }) {
            Ok(engine) => Arc::new(engine),
            Err(error) => {
                tracing::error!("plugin engine did not start, plugins are off: {error}");
                return None;
            }
        };
        let shared = Arc::new(Shared {
            nodes: Mutex::new(Vec::new()),
            focused: Mutex::new(None),
            epoch: AtomicU64::new(1),
            outbox: Mutex::new(VecDeque::new()),
            notification: Arc::new(tokio::sync::Notify::new()),
            waiting: Mutex::new(HashMap::new()),
            next_seq: AtomicU64::new(1),
            stopping: AtomicBool::new(false),
            panels: Mutex::new(BTreeMap::new()),
            popup: Mutex::new(None),
            screen: Mutex::new((0, 0)),
            runtime: Mutex::new(None),
            project_dir: session_cwd.to_string_lossy().into_owned(),
            session_name: session_name.to_owned(),
        });
        let (disabled, warning) = wiring::disabled_at_start(&start.data_dir);
        if let Some(warning) = warning {
            tracing::warn!("installed plugin registry unreadable, every plugin enabled: {warning}");
        }
        let log: Arc<dyn LogSink> = Arc::new(TracingLog);
        let provider = wiring::import_provider(wiring::ProcessImports {
            storage_root: wiring::storage_root(&start.data_dir),
            scope: StorageContext {
                project: Some(session_cwd.to_path_buf()),
                session: Some(session_name.to_owned()),
                ..StorageContext::default()
            },
            // Basic panes are a server primitive; the client links no backend for them.
            basic: None,
            log: Arc::clone(&log),
        });
        let runtime = HostRuntime::new(
            engine,
            RuntimeConfig {
                side: Side::Client,
                kernel: Arc::new(ClientFacade {
                    shared: Arc::clone(&shared),
                }),
                log,
                provider: Some(provider),
                disabled,
            },
        );
        let thread = match RuntimeThread::spawn(runtime) {
            Ok(thread) => thread,
            Err(error) => {
                tracing::error!("plugin host thread did not start, plugins are off: {error}");
                return None;
            }
        };
        let manifests = start
            .catalogue
            .iter()
            .map(|plugin| CataloguePlugin {
                id: plugin.id.clone(),
                manifest: plugin.manifest.clone(),
                component: None,
            })
            .collect();
        let catalogue = start.catalogue;
        let wake = Arc::clone(&shared);
        // In the slot before the first job runs, so a plugin can message itself while starting.
        let posted = shared.runtime.lock().ok().is_some_and(|mut slot| {
            slot.insert(thread).post(move |runtime| {
                log_errors("load", runtime.load(catalogue));
                log_errors("startup activation", runtime.activate_startup());
                log_errors("pump", runtime.pump());
                tracing::info!(plugins = runtime.plugin_ids().len(), "plugin host started");
                wake.push(Output::Redraw);
            })
        });
        if !posted {
            tracing::error!("plugin host thread stopped before loading, plugins are off");
            return None;
        }
        Some(Self {
            shared,
            catalogue: manifests,
            server_entries: BTreeMap::new(),
            server_states: BTreeMap::new(),
            known: BTreeMap::new(),
            last_refusal: None,
        })
    }

    /// Signalled whenever the outbox has something for the event loop.
    pub(crate) fn notification(&self) -> Arc<tokio::sync::Notify> {
        Arc::clone(&self.shared.notification)
    }

    /// Takes what the host produced since the last call.
    pub(crate) fn take_outputs(&self) -> Vec<Output> {
        self.shared
            .outbox
            .lock()
            .map(|mut outbox| outbox.drain(..).collect())
            .unwrap_or_default()
    }

    /// Refreshes the tree mirror after the client applied a tree snapshot and tells the
    /// plugins which nodes appeared, went away or read differently.
    pub(crate) fn sync_tree(&mut self, tree: &Tree, focused: Option<ilium_core::NodeId>) {
        let nodes: Vec<TreeNode> = preorder_ids(tree)
            .into_iter()
            .filter_map(|id| node_view(tree, id))
            .collect();
        let current = tree_marks(tree);
        if let Ok(mut mirror) = self.shared.nodes.lock() {
            *mirror = nodes;
        }
        if let Ok(mut slot) = self.shared.focused.lock() {
            *slot = focused.map(to_plugin);
        }
        self.shared.epoch.fetch_add(1, Ordering::Relaxed);
        let batch = tree_delta(&self.known, &current);
        self.known = current;
        self.shared.post(move |runtime| {
            log_errors("event delivery", runtime.broadcast(&batch));
            log_errors("pump", runtime.pump());
        });
    }

    /// Routes plugin traffic from the server. Returns whether the plugin list changed.
    pub(crate) fn on_server_event(&mut self, event: ServerEvent) -> bool {
        match event {
            ServerEvent::Plugin(envelope) => self.on_envelope(envelope),
            ServerEvent::Contributions(delta) => {
                self.apply_delta(delta);
                true
            }
            ServerEvent::Rejected(rejection) => {
                tracing::warn!(
                    "the server rejected plugin traffic: {} ({})",
                    rejection.message,
                    rejection.code
                );
                false
            }
            _ => false,
        }
    }

    fn on_envelope(&mut self, envelope: PluginEnvelope) -> bool {
        let PluginEnvelope { plugin, seq, body } = envelope;
        match body {
            EnvelopeBody::Outcome(record) => {
                let outcome =
                    serde_json::from_str::<Outcome>(&record.outcome_json).unwrap_or_else(|error| {
                        failed(
                            ErrorCode::Internal,
                            &format!("the server's outcome does not parse: {error}"),
                        )
                    });
                let waiter = self
                    .shared
                    .waiting
                    .lock()
                    .ok()
                    .and_then(|mut waiting| waiting.remove(&seq));
                if let Some(waiter) = waiter {
                    // The plugin may have timed out already; its receiver is then gone.
                    let _ = waiter.send(outcome);
                }
                false
            }
            EnvelopeBody::Message(message) => {
                let event = Event::Message(MessageEvent {
                    from: PluginId(plugin.clone()),
                    from_side: PluginSide::Server,
                    from_client: None,
                    channel: message.topic,
                    payload: message.payload,
                });
                self.shared.post(move |runtime| {
                    if let Err(error) = runtime.deliver(&plugin, vec![event]) {
                        tracing::debug!("server plugin message not delivered: {error}");
                    }
                    log_errors("pump", runtime.pump());
                });
                false
            }
            EnvelopeBody::Control(ControlBody::PluginState {
                plugin: target,
                state,
            }) => {
                if let Some(code) = state.strip_prefix("refused:") {
                    self.last_refusal = Some(format!("{target}: {code}"));
                    return true;
                }
                self.last_refusal = None;
                let enabled = !wiring::is_disabled_state(&state);
                let was_enabled = self
                    .server_states
                    .get(&target)
                    .is_none_or(|previous| !wiring::is_disabled_state(previous));
                self.server_states.insert(target.clone(), state);
                if enabled != was_enabled {
                    // The client side of a plugin follows its server side.
                    self.shared.post(move |runtime| {
                        if let Err(error) = runtime.set_enabled(&target, enabled) {
                            tracing::debug!("client side of {target} kept its state: {error}");
                        } else if enabled {
                            log_errors("startup activation", runtime.activate_startup());
                        }
                        log_errors("pump", runtime.pump());
                    });
                }
                true
            }
            other => {
                tracing::debug!(
                    plugin,
                    "plugin envelope not handled by the client: {other:?}"
                );
                false
            }
        }
    }

    fn apply_delta(&mut self, delta: ContributionDelta) {
        if matches!(delta.kind, DeltaKind::Snapshot) {
            self.server_entries.clear();
        }
        for op in delta.ops {
            match op {
                WireOp::Put { id, entry } => match wiring::decode_entry(&entry) {
                    Some(entry) => {
                        self.server_entries.insert(id, entry);
                    }
                    None => tracing::warn!("contribution {id} from the server does not decode"),
                },
                WireOp::Remove { id } => {
                    self.server_entries.remove(&id);
                }
                WireOp::Clear { plugin } => {
                    // A contribution id is `<plugin>/<kind>/<name>`.
                    let prefix = format!("{plugin}/");
                    self.server_entries.retain(|id, _| !id.starts_with(&prefix));
                }
            }
        }
    }

    /// Runs a plugin command (`<plugin id>/<command>`) on the client side of its plugin, with
    /// the focused pane as context. Returns at once; false when no plugin host is running.
    /// A command no enabled plugin contributes becomes a toast, so the user is told.
    pub(crate) fn invoke_command(
        &self,
        command: &str,
        arguments_json: String,
        origin: CommandOrigin,
        screen: (u16, u16),
    ) -> bool {
        self.shared.set_screen(screen);
        let shared = Arc::clone(&self.shared);
        let command = command.to_owned();
        self.shared.post(move |runtime| {
            let context = CommandContext {
                pane: shared.focused.lock().ok().and_then(|focused| *focused),
                ..CommandContext::default()
            };
            if let Err(error) = runtime.invoke_command(&command, &arguments_json, origin, context) {
                let toast = serde_json::json!({
                    "message": format!("{command} is not available: {error}"),
                });
                shared.push(Output::Toast {
                    plugin: "plugins".to_owned(),
                    toast_json: toast.to_string(),
                });
            }
            log_errors("pump", runtime.pump());
            shared.refresh_popup(runtime);
        })
    }

    /// The view the plugin last rendered for its open popup; `None` before the first one.
    pub(crate) fn popup_tree(&self, plugin: &str, popup: &str) -> Option<UiTree> {
        self.shared.popup.lock().ok().and_then(|slot| {
            slot.as_ref()
                .filter(|open| open.plugin == plugin && open.popup == popup)
                .and_then(|open| open.tree.clone())
        })
    }

    /// Delivers one action of the open popup to its plugin and refreshes the view.
    pub(crate) fn popup_action(&self, plugin: &str, action: UiAction, screen: (u16, u16)) {
        self.shared.set_screen(screen);
        let shared = Arc::clone(&self.shared);
        let plugin = plugin.to_owned();
        self.shared.post(move |runtime| {
            if let Err(error) = runtime.deliver(&plugin, vec![Event::UiAction(action)]) {
                tracing::debug!("popup action not delivered to {plugin}: {error}");
            }
            log_errors("pump", runtime.pump());
            shared.refresh_popup(runtime);
        });
    }

    /// Closes the open popup on the kernel's own authority (Escape, a click outside, another
    /// modal taking the screen) and tells the plugin with a `closed` action.
    pub(crate) fn dismiss_popup(&self, plugin: &str, popup: &str) {
        if !self.shared.close_popup(plugin, popup) {
            return;
        }
        let closed = UiAction {
            view: ViewTarget::Popup(popup.to_owned()),
            key: String::new(),
            action_id: String::new(),
            kind: ActionKind::Closed,
            value: None,
        };
        let plugin = plugin.to_owned();
        self.shared.post(move |runtime| {
            if let Err(error) = runtime.deliver(&plugin, vec![Event::UiAction(closed)]) {
                tracing::debug!("popup close not delivered to {plugin}: {error}");
            }
            log_errors("pump", runtime.pump());
        });
    }

    /// The rows of the Settings plugin list.
    pub(crate) fn rows(&self) -> Vec<PluginRow> {
        let disabled = self
            .server_states
            .iter()
            .filter(|(_, state)| wiring::is_disabled_state(state))
            .map(|(id, _)| id.clone())
            .collect();
        wiring::plugin_rows(&self.catalogue, &disabled, &self.server_states)
    }

    /// The command the first enabled plugin binds to a post-leader key, in catalogue order.
    pub(crate) fn prefix_binding_command(&self, key: crate::keymap::BindingKey) -> Option<String> {
        self.catalogue
            .iter()
            .filter(|plugin| {
                !self
                    .server_states
                    .get(&plugin.id)
                    .is_some_and(|state| wiring::is_disabled_state(state))
            })
            .flat_map(|plugin| super::keybindings::prefix_bindings(&plugin.id, &plugin.manifest))
            .find(|binding| binding.key == key)
            .map(|binding| binding.command)
    }

    /// The last refused enable or disable request, if the newest request was refused.
    pub(crate) fn last_refusal(&self) -> Option<&str> {
        self.last_refusal.as_deref()
    }

    /// How many contributions the server sides currently hold.
    pub(crate) fn server_contribution_count(&self) -> usize {
        self.server_entries.len()
    }

    /// The request that asks the server to enable or disable a plugin. The server persists
    /// the choice and broadcasts the new state, which `on_server_event` then applies here.
    pub(crate) fn enable_request(&self, plugin: &str, enabled: bool) -> ClientRequest {
        self.shared
            .envelope(
                plugin,
                EnvelopeBody::Control(ControlBody::SetEnabled {
                    plugin: plugin.to_owned(),
                    enabled,
                }),
            )
            .1
    }

    /// Deactivates every plugin and joins the host thread. Intents submitted while stopping
    /// are cancelled at once, because the event loop no longer carries them to the server.
    pub(crate) async fn stop(&self) {
        self.shared.stopping.store(true, Ordering::Release);
        let thread = self
            .shared
            .runtime
            .lock()
            .ok()
            .and_then(|mut slot| slot.take());
        let Some(thread) = thread else {
            return;
        };
        let join = tokio::task::spawn_blocking(move || thread.shutdown(SHUTDOWN_BUDGET));
        match tokio::time::timeout(SHUTDOWN_WAIT, join).await {
            Ok(Ok(report)) => tracing::info!(
                deactivated = report.deactivated.len(),
                abandoned = report.abandoned.len(),
                "plugin host stopped"
            ),
            Ok(Err(error)) => tracing::warn!("plugin host shutdown task failed: {error}"),
            Err(_) => tracing::warn!("plugin host did not stop in time and was abandoned"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tree_node_is_mirrored_with_its_class_and_parent() {
        let tree = Tree::new();
        let root = node_view(&tree, ilium_core::ROOT_ID).expect("the root exists");
        assert_eq!(root.id, NodeId(ilium_core::ROOT_ID.0));
        assert_eq!(root.parent, None);
        assert!(matches!(root.class, NodeClass::Container));
        assert_eq!(root.content_kind, None);
    }
}
