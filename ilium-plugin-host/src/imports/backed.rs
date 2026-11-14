//! Kernel-backed implementations of the pilot-set imports.
//!
//! The unit-struct imports of this directory answer for a host without a session. The types
//! here answer from the live kernel through `KernelFacade` and from the contribution
//! registry; `HostRuntime` puts them into the import set of each plugin instance.
//!
//! None of them holds a lock across a kernel call other than its own queue lock, and none
//! of them touches a PTY: terminal access goes through the facade to the pane's owner.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use ilium_plugin_api::generated::host::commands::HostCommands;
use ilium_plugin_api::generated::host::help::HostHelp;
use ilium_plugin_api::generated::host::intents_host::HostIntentsHost;
use ilium_plugin_api::generated::host::kernel_settings::HostKernelSettings;
use ilium_plugin_api::generated::host::keymap::{Binding, HostKeymap};
use ilium_plugin_api::generated::host::log::{HostLog, Level};
use ilium_plugin_api::generated::host::panes::HostPanes;
use ilium_plugin_api::generated::host::sessions::{CrossRequest, HostSessions, LiveSession};
use ilium_plugin_api::generated::host::tree::HostTree;
use ilium_plugin_api::generated::primitives::agent::{
    AgentPane, HostAgent, SessionRef, SignatureSpec, TranscriptQuery, TranscriptTurn,
};
use ilium_plugin_api::generated::primitives::terminal::{
    CellRegion, CellRows, ForegroundProcess, HistoryMatch, HostTerminal, OutputFilter, Screen,
    ScreenPart, Selection, TerminalMode,
};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::{NodeId, PluginId, RequestId};
use ilium_plugin_api::generated::types::intents::Submission;
use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef, RequestResult};
use ilium_plugin_api::generated::types::tree::{AgentModel, AgentView};
use ilium_plugin_api::generated::types::tree::{
    NodeClass, NodeQuery, NodeView, PaneInfo, PrimitiveKind,
};
use ilium_plugin_api::generated::types::ui::PointerModifiers;
use ilium_plugin_api::kernel::{KernelFacade, Origin};
use ilium_plugin_api::Side;

use crate::contributions::ContributionRegistry;

use super::env::RequestIds;

/// Largest `arguments-json` accepted by `commands.invoke`: 64 KiB.
const MAX_ARGUMENTS_JSON_BYTES: usize = 64 * 1024;
/// Most request results or command invocations kept for one runtime before the oldest
/// are dropped; a plugin that never reads its events cannot grow the host without bound.
const QUEUE_CAPACITY: usize = 4096;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A poisoned queue still holds consistent data: every writer pushes or pops one item.
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn push_bounded<T>(queue: &Mutex<VecDeque<T>>, item: T) {
    let mut queue = lock(queue);
    if queue.len() >= QUEUE_CAPACITY {
        queue.pop_front();
    }
    queue.push_back(item);
}

/// Where plugin log lines go. The embedding process supplies the real plugin log.
pub trait LogSink: Send + Sync {
    /// One line of one plugin side.
    fn line(&self, plugin: &str, side: Side, level: Level, message: &str);
}

/// A sink that drops every line.
#[derive(Debug, Default)]
pub struct NullLogSink;

impl LogSink for NullLogSink {
    fn line(&self, _plugin: &str, _side: Side, _level: Level, _message: &str) {}
}

/// `ilium:host/log` onto a `LogSink`.
pub struct SinkLog {
    sink: Arc<dyn LogSink>,
    plugin: String,
    side: Side,
}

impl SinkLog {
    /// A log import of one plugin side.
    pub fn new(sink: Arc<dyn LogSink>, plugin: &str, side: Side) -> Self {
        Self {
            sink,
            plugin: plugin.to_owned(),
            side,
        }
    }
}

impl HostLog for SinkLog {
    fn log(&mut self, level: Level, message: String) {
        self.sink.line(&self.plugin, self.side, level, &message);
    }
}

/// `ilium:host/tree` over the kernel tree reader.
pub struct KernelTree {
    kernel: Arc<dyn KernelFacade>,
    /// The plugin this import belongs to; node metadata is kept per plugin.
    plugin: String,
    /// Whether the plugin holds `tree.read`; without it every read answers nothing.
    reads_tree: bool,
    /// Whether the plugin holds `agent.read`; without it the `agent` field reads none.
    reads_agents: bool,
    /// Whether the plugin holds `tree.metadata`.
    keeps_metadata: bool,
}

/// Whether a plugin holds a capability. An interface unlocks as a whole with any capability
/// of its family, so each call that needs a specific one checks it here.
///
/// A held grant counts when it is the capability itself or a wildcard the grant grammar
/// allows for its family (`commands.invoke:*`, `topics.subscribe:room/*`, `http:*.host`,
/// `fs.read:path:/x/**`): the rule is `Grant::covers`, the same one the manifest check uses.
/// Families without a wildcard form, such as `settings.read-kernel`, match exactly; a
/// manifest that asks for `settings.read-kernel:*` is refused before it is ever granted
/// (`manifest.grant-wildcard`).
fn holds(granted: &[String], name: &str) -> bool {
    if granted.iter().any(|grant| grant == name) {
        return true;
    }
    // Only a held grant with a wildcard can cover a different name, and only a name the
    // grammar accepts can be covered.
    let Ok(wanted) = ilium_plugin_api::grants::Grant::parse(name) else {
        return false;
    };
    granted
        .iter()
        .filter(|grant| grant.contains('*'))
        .filter_map(|grant| ilium_plugin_api::grants::Grant::parse(grant).ok())
        .any(|held| held.covers(&wanted))
}

/// Most bytes one plugin keeps on one node: the default of the limit `node_metadata_kib`.
const NODE_METADATA_LIMIT_BYTES: usize = 64 * 1024;

impl KernelTree {
    /// A tree import of one plugin instance reading the given kernel. `granted` is what the
    /// plugin holds on this side.
    pub fn new(kernel: Arc<dyn KernelFacade>, plugin: &str, granted: &[String]) -> Self {
        Self {
            kernel,
            plugin: plugin.to_owned(),
            reads_tree: holds(granted, "tree.read"),
            reads_agents: holds(granted, "agent.read"),
            keeps_metadata: holds(granted, "tree.metadata"),
        }
    }

    /// The nodes as this plugin may see them.
    fn nodes(&self) -> Vec<NodeView> {
        if !self.reads_tree {
            return Vec::new();
        }
        let mut nodes = self.kernel.tree().nodes();
        if !self.reads_agents {
            for node in &mut nodes {
                node.agent = None;
            }
        }
        nodes
    }
}

fn is_under(nodes: &[NodeView], node: &NodeView, ancestor: NodeId) -> bool {
    let mut parent = node.parent;
    // The depth bound guards against a cyclic parent chain in a faulty reader.
    for _ in 0..nodes.len() {
        match parent {
            Some(id) if id == ancestor => return true,
            Some(id) => {
                parent = nodes
                    .iter()
                    .find(|other| other.id == id)
                    .and_then(|other| other.parent)
            }
            None => return false,
        }
    }
    false
}

impl HostTree for KernelTree {
    fn get(&mut self, node: NodeId) -> Option<NodeView> {
        if !self.reads_tree {
            return None;
        }
        let mut view = self.kernel.tree().node(node)?;
        if !self.reads_agents {
            view.agent = None;
        }
        Some(view)
    }

    fn children(&mut self, node: NodeId) -> Vec<NodeId> {
        self.nodes()
            .into_iter()
            .filter(|candidate| candidate.parent == Some(node))
            .map(|candidate| candidate.id)
            .collect()
    }

    fn roots(&mut self) -> Vec<NodeId> {
        self.nodes()
            .into_iter()
            .filter(|candidate| candidate.parent.is_none())
            .map(|candidate| candidate.id)
            .collect()
    }

    fn find(&mut self, query: NodeQuery) -> Vec<NodeId> {
        let nodes = self.nodes();
        nodes
            .iter()
            .filter(|node| {
                query
                    .content_kind
                    .as_ref()
                    .is_none_or(|kind| node.content_kind.as_ref() == Some(kind))
            })
            .filter(|node| {
                query.signature.as_ref().is_none_or(|signature| {
                    node.agent
                        .as_ref()
                        .is_some_and(|agent| &agent.signature == signature)
                })
            })
            .filter(|node| {
                query
                    .under
                    .is_none_or(|ancestor| is_under(&nodes, node, ancestor))
            })
            .map(|node| node.id)
            .collect()
    }

    fn metadata(&mut self, node: NodeId) -> Option<Vec<u8>> {
        if !self.keeps_metadata {
            return None;
        }
        self.kernel.annotations()?.metadata(&self.plugin, node)
    }

    fn set_metadata(&mut self, node: NodeId, bytes: Vec<u8>) -> Result<(), PluginError> {
        if !self.keeps_metadata {
            return Err(PluginError::new(
                ErrorCode::NotPermitted,
                "the grant tree.metadata is missing",
            ));
        }
        if bytes.len() > NODE_METADATA_LIMIT_BYTES {
            return Err(PluginError::new(
                ErrorCode::QuotaExceeded,
                "node metadata is limited to 64 KiB for each node",
            ));
        }
        if self.kernel.tree().node(node).is_none() {
            return Err(PluginError::new(
                ErrorCode::NotFound,
                "the node does not exist",
            ));
        }
        let Some(store) = self.kernel.annotations() else {
            return Err(PluginError::new(
                ErrorCode::Unsupported,
                "node metadata is not stored on this side",
            ));
        };
        store
            .set_metadata(&self.plugin, node, bytes)
            .map_err(|error| PluginError::new(error.code, error.message))
    }
}

/// `ilium:host/help` over the kernel's help catalogue.
pub struct KernelHelp {
    kernel: Arc<dyn KernelFacade>,
    reads: bool,
}

impl KernelHelp {
    /// A help import reading the given kernel. `granted` is what the instance holds: the
    /// interface links with any capability of its family, so `help.read` is checked here.
    pub fn new(kernel: Arc<dyn KernelFacade>, granted: &[String]) -> Self {
        Self {
            kernel,
            reads: holds(granted, "help.read"),
        }
    }
}

impl HostHelp for KernelHelp {
    /// Unknown, malformed or local ids give `None`, never a failure; so does a missing grant
    /// and a kernel without a help catalogue.
    fn topic(&mut self, id: String) -> Option<String> {
        let global = matches!(
            id.split_once('/'),
            Some((prefix, local)) if !prefix.is_empty() && !local.is_empty()
        );
        if !self.reads || !global {
            return None;
        }
        self.kernel.help()?.topic(&id)
    }
}

/// `ilium:host/keymap` over the kernel's resolved key map (contract section 19.3).
pub struct KernelKeymap {
    kernel: Arc<dyn KernelFacade>,
    reads: bool,
}

impl KernelKeymap {
    /// A keymap import reading the given kernel. `granted` is what the instance holds.
    pub fn new(kernel: Arc<dyn KernelFacade>, granted: &[String]) -> Self {
        Self {
            kernel,
            reads: holds(granted, "keymap.read"),
        }
    }
}

impl HostKeymap for KernelKeymap {
    /// Without the grant, and in a process that has no key map, the list is empty.
    fn bindings(&mut self) -> Vec<Binding> {
        if !self.reads {
            return Vec::new();
        }
        self.kernel
            .keymap()
            .map(|keymap| keymap.bindings())
            .unwrap_or_default()
            .into_iter()
            .map(|binding| Binding {
                command: binding.command,
                chords: binding.chords,
                context: binding.context,
                is_default: binding.is_default,
            })
            .collect()
    }

    fn chord_label(&mut self, chord: String) -> String {
        // The label is a pure text rule; the plain import owns it.
        super::keymap::KeymapImport.chord_label(chord)
    }
}

/// Whether a settings key names a secret, whose value plugins never read.
fn is_secret_setting(key: &str) -> bool {
    ["key", "token", "secret", "password"]
        .iter()
        .any(|marker| key.contains(marker))
}

/// `ilium:host/kernel-settings` over the kernel's settings tables (contract section 19.3).
/// A tab is one key of `KernelSettings`; a tab key is `<tab>.<key>`.
pub struct KernelSettingsTables {
    kernel: Arc<dyn KernelFacade>,
    granted: Vec<String>,
}

impl KernelSettingsTables {
    /// A kernel-settings import of one instance. Each call checks the grant of its tab.
    pub fn new(kernel: Arc<dyn KernelFacade>, granted: &[String]) -> Self {
        Self {
            kernel,
            granted: granted.to_vec(),
        }
    }

    fn settings(
        &self,
        grant: &str,
        tab: &str,
    ) -> Result<&dyn ilium_plugin_api::kernel::KernelSettings, PluginError> {
        if !holds(&self.granted, &format!("{grant}:{tab}")) {
            return Err(PluginError::new(
                ErrorCode::NotPermitted,
                format!("{grant}:{tab} is not granted"),
            ));
        }
        self.kernel.settings().ok_or_else(|| {
            PluginError::new(
                ErrorCode::Unsupported,
                "this process has no kernel settings tables",
            )
        })
    }
}

impl HostKernelSettings for KernelSettingsTables {
    fn read(&mut self, tab: String) -> Result<String, PluginError> {
        let settings = self.settings("settings.read-kernel", &tab)?;
        let text = settings
            .get(&tab)
            .ok_or_else(|| PluginError::new(ErrorCode::NotFound, format!("unknown tab {tab:?}")))?;
        let serde_json::Value::Object(values) = serde_json::from_str(&text).map_err(|error| {
            PluginError::new(
                ErrorCode::Internal,
                format!("the tab does not parse: {error}"),
            )
        })?
        else {
            return Err(PluginError::new(
                ErrorCode::Internal,
                "the tab is not a JSON object",
            ));
        };
        let redacted: serde_json::Map<String, serde_json::Value> = values
            .into_iter()
            .map(|(key, value)| {
                if is_secret_setting(&key) {
                    (key, serde_json::Value::String("<redacted>".to_owned()))
                } else {
                    (key, value)
                }
            })
            .collect();
        Ok(serde_json::Value::Object(redacted).to_string())
    }

    fn write(&mut self, tab: String, key: String, value: String) -> Result<(), PluginError> {
        let settings = self.settings("settings.write-kernel", &tab)?;
        if serde_json::from_str::<serde_json::Value>(&value).is_err() {
            return Err(PluginError::new(
                ErrorCode::InvalidArgument,
                "value is not JSON",
            ));
        }
        settings
            .set(&format!("{tab}.{key}"), &value)
            .map_err(|error| PluginError::new(error.code, error.message))
    }
}

/// `ilium:host/sessions` over the kernel (contract section 19.3): the calling session is
/// listed with its real name, project and pane count. Requests to other sessions are refused
/// until the cross-session transport exists; nothing is queued that would never be answered.
pub struct KernelSessions {
    kernel: Arc<dyn KernelFacade>,
    crosses: bool,
}

impl KernelSessions {
    /// A sessions import of one instance.
    pub fn new(kernel: Arc<dyn KernelFacade>, granted: &[String]) -> Self {
        Self {
            kernel,
            crosses: holds(granted, "sessions.cross"),
        }
    }
}

impl HostSessions for KernelSessions {
    fn list_live(&mut self) -> Vec<LiveSession> {
        let workspace = self.kernel.workspace();
        let name = workspace
            .and_then(|workspace| workspace.session_name())
            .unwrap_or_else(|| "default".to_owned());
        let project = workspace
            .and_then(|workspace| workspace.project_dir())
            .unwrap_or_default();
        let panes = self
            .kernel
            .tree()
            .nodes()
            .iter()
            .filter(|node| node.class == NodeClass::Pane)
            .count();
        vec![LiveSession {
            socket_id: format!("{project}#{name}"),
            name,
            project,
            panes: u32::try_from(panes).unwrap_or(u32::MAX),
            attached_clients: 0,
            is_self: true,
        }]
    }

    fn send(&mut self, request: CrossRequest) -> Result<RequestId, PluginError> {
        if !self.crosses {
            return Err(PluginError::new(
                ErrorCode::NotPermitted,
                "sessions.cross is not granted",
            ));
        }
        if !self
            .list_live()
            .iter()
            .any(|session| session.name == request.session)
        {
            return Err(PluginError::new(
                ErrorCode::NotFound,
                "no live session has that name",
            ));
        }
        Err(PluginError::new(
            ErrorCode::Unsupported,
            "requests between sessions are not delivered yet",
        ))
    }

    fn take_reply(&mut self, _request: RequestId) -> Option<String> {
        None
    }
}

/// `ilium:host/panes` over the kernel tree and pane readers.
pub struct KernelPanes {
    kernel: Arc<dyn KernelFacade>,
}

impl KernelPanes {
    /// A panes import reading the given kernel.
    pub fn new(kernel: Arc<dyn KernelFacade>) -> Self {
        Self { kernel }
    }
}

impl HostPanes for KernelPanes {
    fn info(&mut self, pane: NodeId) -> Option<PaneInfo> {
        let node = self.kernel.tree().node(pane)?;
        if node.class != NodeClass::Pane {
            return None;
        }
        let content_kind = node.content_kind.unwrap_or_else(|| "terminal".to_owned());
        let primitive = match content_kind.as_str() {
            "terminal" => PrimitiveKind::Terminal,
            "editor" => PrimitiveKind::Editor,
            _ => PrimitiveKind::Basic,
        };
        let (columns, rows) = self.kernel.panes().size(pane).unwrap_or((0, 0));
        Some(PaneInfo {
            node: pane,
            content_kind,
            primitive,
            columns,
            rows,
            focused: self.kernel.focused_pane() == Some(pane),
            title: node.title,
            is_restored: false,
        })
    }

    fn focused(&mut self) -> Option<NodeId> {
        self.kernel.focused_pane()
    }
}

/// Request results waiting to be delivered to plugins as `request-completed` events.
pub type ResultQueue = Arc<Mutex<VecDeque<(String, RequestResult)>>>;

/// `ilium:host/intents-host`: applies each submission through the kernel at once and
/// queues its outcome for the submitting plugin.
pub struct KernelIntents {
    kernel: Arc<dyn KernelFacade>,
    origin: Origin,
    plugin: String,
    results: ResultQueue,
    ids: RequestIds,
}

impl KernelIntents {
    /// An intents import of one plugin instance.
    pub fn new(
        kernel: Arc<dyn KernelFacade>,
        plugin: &str,
        side: Side,
        instance: u64,
        results: ResultQueue,
    ) -> Self {
        Self {
            kernel,
            origin: Origin::Plugin {
                plugin: PluginId(plugin.to_owned()),
                side,
                instance,
            },
            plugin: plugin.to_owned(),
            results,
            ids: RequestIds::new(),
        }
    }

    /// The same import drawing its request ids from the runtime's allocator.
    #[must_use]
    pub fn with_ids(mut self, ids: RequestIds) -> Self {
        self.ids = ids;
        self
    }
}

/// Whether a dedup key has the form the manual allows: 1 to 128 of `A-Z a-z 0-9 . _ : / -`.
pub fn dedup_key_is_valid(key: &str) -> bool {
    let length = key.chars().count();
    (1..=128).contains(&length)
        && key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._:/-".contains(character))
}

impl HostIntentsHost for KernelIntents {
    fn submit(&mut self, submission: Submission) -> Result<RequestId, PluginError> {
        if submission
            .dedup_key
            .as_deref()
            .is_some_and(|key| !dedup_key_is_valid(key))
        {
            return Err(PluginError::new(
                ErrorCode::InvalidArgument,
                "dedup key must be 1 to 128 characters of A-Z a-z 0-9 . _ : / -",
            ));
        }
        let request = self.ids.next_request();
        let outcome = self.kernel.apply_intent(&self.origin, submission.intent);
        push_bounded(
            &self.results,
            (
                self.plugin.clone(),
                RequestResult {
                    request: RequestRef::Call(request),
                    outcome,
                },
            ),
        );
        Ok(request)
    }
}

/// A command a plugin asked the host to invoke.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingCommand {
    /// The plugin that asked.
    pub caller: String,
    /// The request the caller waits on.
    pub request: RequestId,
    /// The global command id `<plugin>/<local id>`.
    pub command: String,
    /// The arguments, one JSON object.
    pub arguments_json: String,
}

/// Commands waiting for the runtime to deliver them to their owner.
pub type CommandQueue = Arc<Mutex<VecDeque<PendingCommand>>>;

/// `ilium:host/commands` over the contribution registry.
pub struct RegistryCommands {
    registry: Arc<Mutex<ContributionRegistry>>,
    plugin: String,
    side: Side,
    queue: CommandQueue,
    ids: RequestIds,
}

impl RegistryCommands {
    /// A commands import of one plugin side.
    pub fn new(
        registry: Arc<Mutex<ContributionRegistry>>,
        plugin: &str,
        side: Side,
        queue: CommandQueue,
    ) -> Self {
        Self {
            registry,
            plugin: plugin.to_owned(),
            side,
            queue,
            ids: RequestIds::new(),
        }
    }

    /// The same import drawing its request ids from the runtime's allocator.
    #[must_use]
    pub fn with_ids(mut self, ids: RequestIds) -> Self {
        self.ids = ids;
        self
    }
}

fn unknown_command(command: &str) -> PluginError {
    let shown: String = command.chars().take(128).collect();
    PluginError::new(ErrorCode::NotFound, format!("unknown command {shown}"))
}

impl HostCommands for RegistryCommands {
    fn set_state(
        &mut self,
        command: String,
        enabled: bool,
        checked: Option<bool>,
    ) -> Result<(), PluginError> {
        if self.side != Side::Client {
            return Err(PluginError::new(
                ErrorCode::Unsupported,
                "set-state is a client function",
            ));
        }
        // Own commands only: the local id, or the global id of this plugin.
        let local = match command.split_once('/') {
            Some((plugin, local)) if plugin == self.plugin => local,
            Some(_) => return Err(unknown_command(&command)),
            None => command.as_str(),
        };
        let mut registry = lock(&self.registry);
        let Some(entry) = registry.get(&self.plugin, "commands", local) else {
            return Err(unknown_command(&command));
        };
        let mut payload = entry.payload.clone();
        if let Some(object) = payload.as_object_mut() {
            object.insert("enabled".to_owned(), enabled.into());
            match checked {
                Some(checked) => object.insert("checked".to_owned(), checked.into()),
                None => object.remove("checked"),
            };
        }
        registry
            .put(&self.plugin, "commands", local, self.side, payload)
            .map_err(|error| PluginError::new(ErrorCode::Internal, error.to_string()))
    }

    fn invoke(
        &mut self,
        command: String,
        arguments_json: String,
    ) -> Result<RequestId, PluginError> {
        let arguments_ok = arguments_json.len() <= MAX_ARGUMENTS_JSON_BYTES
            && serde_json::from_str::<serde_json::Value>(&arguments_json)
                .is_ok_and(|value| value.is_object());
        if !arguments_ok {
            return Err(PluginError::new(
                ErrorCode::InvalidArgument,
                "arguments-json must be one JSON object of at most 64 KiB",
            ));
        }
        if lock(&self.registry)
            .find_global("commands", &command)
            .is_none()
        {
            return Err(unknown_command(&command));
        }
        let request = self.ids.next_request();
        push_bounded(
            &self.queue,
            PendingCommand {
                caller: self.plugin.clone(),
                request,
                command,
                arguments_json,
            },
        );
        Ok(request)
    }
}

/// The result a caller of `commands.invoke` receives once the command was delivered.
pub fn command_result(request: RequestId, outcome: Outcome) -> RequestResult {
    RequestResult {
        request: RequestRef::Call(request),
        outcome,
    }
}

#[cfg(test)]
mod tests {
    use super::{dedup_key_is_valid, CommandQueue, RegistryCommands};
    use crate::contributions::ContributionRegistry;
    use ilium_plugin_api::generated::host::commands::HostCommands;
    use ilium_plugin_api::generated::types::errors::ErrorCode;
    use ilium_plugin_api::Side;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    fn commands(
        side: Side,
    ) -> (
        RegistryCommands,
        CommandQueue,
        Arc<Mutex<ContributionRegistry>>,
    ) {
        let mut registry = ContributionRegistry::new();
        registry
            .load_manifest(
                "ilium.core.layout",
                &json!({"contributes": {"commands": [{"id": "preset", "title": "Apply"}]}}),
            )
            .expect("valid manifest");
        let registry = Arc::new(Mutex::new(registry));
        let queue = CommandQueue::default();
        (
            RegistryCommands::new(
                Arc::clone(&registry),
                "ilium.core.layout",
                side,
                Arc::clone(&queue),
            ),
            queue,
            registry,
        )
    }

    #[test]
    fn dedup_key_bounds() {
        assert!(!dedup_key_is_valid(""));
        assert!(dedup_key_is_valid("a"));
        assert!(dedup_key_is_valid(&"a".repeat(128)));
        assert!(!dedup_key_is_valid(&"a".repeat(129)));
        assert!(!dedup_key_is_valid("a b"));
    }

    #[test]
    fn invoking_a_contributed_command_queues_it() {
        let (mut import, queue, _registry) = commands(Side::Server);
        let request = import
            .invoke("ilium.core.layout/preset".to_owned(), "{}".to_owned())
            .expect("the command is contributed");
        let queued = queue
            .lock()
            .expect("queue")
            .pop_front()
            .expect("one command");
        assert_eq!(queued.request, request);
        assert_eq!(queued.command, "ilium.core.layout/preset");
    }

    #[test]
    fn an_unknown_command_and_bad_arguments_are_refused() {
        let (mut import, queue, _registry) = commands(Side::Server);
        let unknown = import.invoke("no.such.plugin/none".to_owned(), "{}".to_owned());
        assert_eq!(
            unknown.map_err(|error| error.code),
            Err(ErrorCode::NotFound)
        );
        let bad = import.invoke("ilium.core.layout/preset".to_owned(), "[]".to_owned());
        assert_eq!(
            bad.map_err(|error| error.code),
            Err(ErrorCode::InvalidArgument)
        );
        assert!(queue.lock().expect("queue").is_empty());
    }

    #[test]
    fn set_state_changes_only_own_commands_on_the_client() {
        let (mut client, _queue, registry) = commands(Side::Client);
        client
            .set_state("preset".to_owned(), false, Some(true))
            .expect("own command");
        let payload = registry
            .lock()
            .expect("registry")
            .get("ilium.core.layout", "commands", "preset")
            .expect("entry")
            .payload
            .clone();
        assert_eq!(payload["enabled"], json!(false));
        assert_eq!(payload["checked"], json!(true));
        let foreign = client.set_state("other.plugin/preset".to_owned(), true, None);
        assert_eq!(
            foreign.map_err(|error| error.code),
            Err(ErrorCode::NotFound)
        );
        let (mut server, _queue, _registry) = commands(Side::Server);
        let refused = server.set_state("preset".to_owned(), true, None);
        assert_eq!(
            refused.map_err(|error| error.code),
            Err(ErrorCode::Unsupported)
        );
    }
}

/// `ilium:primitives/agent` with the agent of each pane read from the kernel tree. Every call
/// the tree cannot answer goes to the session-less import, except signature registration:
/// nothing in this kernel reads a registered signature yet, so it is refused instead of
/// accepted without effect.
pub struct KernelAgent {
    kernel: Arc<dyn KernelFacade>,
    rest: super::agent::AgentImport,
    /// Whether the plugin holds `agent.read`.
    reads_agents: bool,
}

impl KernelAgent {
    /// An agent import of one plugin instance reading the given kernel.
    pub fn new(kernel: Arc<dyn KernelFacade>, granted: &[String]) -> Self {
        Self {
            kernel,
            rest: super::agent::AgentImport::default(),
            reads_agents: holds(granted, "agent.read"),
        }
    }
}

impl HostAgent for KernelAgent {
    fn agents(&mut self) -> Vec<AgentPane> {
        if !self.reads_agents {
            return Vec::new();
        }
        self.kernel
            .tree()
            .nodes()
            .into_iter()
            .filter_map(|node| {
                node.agent.map(|view| AgentPane {
                    pane: node.id,
                    view,
                })
            })
            .collect()
    }

    fn view(&mut self, pane: NodeId) -> Option<AgentView> {
        if !self.reads_agents {
            return None;
        }
        self.kernel.tree().node(pane)?.agent
    }

    fn session(&mut self, pane: NodeId) -> Option<SessionRef> {
        self.rest.session(pane)
    }

    fn last_prompt(&mut self, pane: NodeId) -> Option<String> {
        self.rest.last_prompt(pane)
    }

    fn register_signature(&mut self, _spec: SignatureSpec) -> Result<(), PluginError> {
        Err(PluginError::new(
            ErrorCode::Unsupported,
            "agent signatures from plugins are not used by this kernel yet",
        ))
    }

    fn set_toolbar_state(
        &mut self,
        button: String,
        pane: Option<NodeId>,
        enabled: bool,
        label: Option<String>,
    ) -> Result<(), PluginError> {
        self.rest.set_toolbar_state(button, pane, enabled, label)
    }

    fn model(&mut self, pane: NodeId) -> Option<AgentModel> {
        self.rest.model(pane)
    }

    fn prompt_epoch(&mut self, pane: NodeId) -> Option<u64> {
        self.rest.prompt_epoch(pane)
    }

    fn transcript(&mut self, query: TranscriptQuery) -> Result<RequestId, PluginError> {
        self.rest.transcript(query)
    }

    fn take_transcript(&mut self, request: RequestId) -> Option<Vec<TranscriptTurn>> {
        self.rest.take_transcript(request)
    }
}

/// `ilium:primitives/terminal` with screen reads answered by the kernel pane reader. The pane
/// reader asks the pane's owner; nothing here holds or touches a PTY. Every other call goes to
/// the session-less import, except input locks, which are refused until the kernel enforces
/// them.
pub struct KernelTerminal {
    kernel: Arc<dyn KernelFacade>,
    rest: super::terminal::TerminalImport,
    /// What the plugin holds of `terminal.read-screen`, `terminal.history` and
    /// `terminal.read-cells`, in that order.
    reads: (bool, bool, bool),
}

impl KernelTerminal {
    /// A terminal import of one plugin instance reading the given kernel.
    pub fn new(kernel: Arc<dyn KernelFacade>, granted: &[String]) -> Self {
        Self {
            kernel,
            rest: super::terminal::TerminalImport::default(),
            reads: (
                holds(granted, "terminal.read-screen"),
                holds(granted, "terminal.history"),
                holds(granted, "terminal.read-cells"),
            ),
        }
    }
}

impl HostTerminal for KernelTerminal {
    /// The visible rows or the last `max_rows` lines of history, with the cursor and the screen
    /// revision the pane reader reports (both 0 when it reports none).
    fn read_screen(&mut self, pane: NodeId, part: ScreenPart, max_rows: u32) -> Option<Screen> {
        let (screen, history, _) = self.reads;
        let permitted = match part {
            ScreenPart::Visible => screen,
            ScreenPart::Scrollback => history,
        };
        if !permitted {
            return None;
        }
        let panes = self.kernel.panes();
        let rows: Vec<String> = match part {
            ScreenPart::Visible => panes
                .screen_text(pane)?
                .lines()
                .take(max_rows as usize)
                .map(str::to_owned)
                .collect(),
            ScreenPart::Scrollback => panes.history(pane, max_rows)?,
        };
        let columns = panes.size(pane).map_or(0, |(columns, _)| columns);
        let state = panes.screen_state(pane).unwrap_or_default();
        Some(Screen {
            columns,
            rows,
            cursor_row: state.cursor_row,
            cursor_column: state.cursor_column,
            title: self.kernel.tree().node(pane).map(|node| node.title),
            revision: state.revision,
        })
    }

    fn get_selection(&mut self, pane: NodeId) -> Option<Selection> {
        self.rest.get_selection(pane)
    }

    fn add_output_filter(&mut self, pane: NodeId, filter: OutputFilter) -> Result<(), PluginError> {
        self.rest.add_output_filter(pane, filter)
    }

    fn remove_output_filter(&mut self, pane: NodeId, filter_id: String) {
        self.rest.remove_output_filter(pane, filter_id);
    }

    fn claim_pointer(&mut self, pane: NodeId) -> Result<(), PluginError> {
        self.rest.claim_pointer(pane)
    }

    fn claim_pointer_gesture(
        &mut self,
        pane: NodeId,
        modifiers: PointerModifiers,
    ) -> Result<(), PluginError> {
        self.rest.claim_pointer_gesture(pane, modifiers)
    }

    fn release_pointer(&mut self, pane: NodeId) {
        self.rest.release_pointer(pane);
    }

    fn set_highlights(
        &mut self,
        pane: NodeId,
        layer: String,
        regions: Vec<CellRegion>,
    ) -> Result<(), PluginError> {
        self.rest.set_highlights(pane, layer, regions)
    }

    fn clear_highlights(&mut self, pane: NodeId, layer: String) {
        self.rest.clear_highlights(pane, layer);
    }

    /// Styled cells of visible rows; scrollback cells are not readable through the pane reader.
    fn read_cells(
        &mut self,
        pane: NodeId,
        part: ScreenPart,
        first_row: u32,
        count: u32,
    ) -> Option<CellRows> {
        if part != ScreenPart::Visible || !self.reads.2 {
            return None;
        }
        let panes = self.kernel.panes();
        let first = u16::try_from(first_row).ok()?;
        let last = first.saturating_add(u16::try_from(count).unwrap_or(u16::MAX));
        let rows = panes.cells(pane, first..last)?;
        Some(CellRows {
            first_row,
            columns: panes.size(pane).map_or(0, |(columns, _)| columns),
            rows,
            revision: panes.screen_state(pane).map_or(0, |state| state.revision),
        })
    }

    fn modes(&mut self, pane: NodeId) -> Option<TerminalMode> {
        self.rest.modes(pane)
    }

    /// The process the kernel reports for the pane; part of `terminal.read-cells`
    /// (primitives-terminal-reads.md).
    fn get_foreground_process(&mut self, pane: NodeId) -> Option<ForegroundProcess> {
        if !self.reads.2 {
            return None;
        }
        self.kernel
            .panes()
            .foreground_process(pane)
            .or_else(|| self.rest.get_foreground_process(pane))
    }

    fn freeze_snapshot(&mut self, pane: NodeId) -> Result<u64, PluginError> {
        self.rest.freeze_snapshot(pane)
    }

    fn read_snapshot_cells(
        &mut self,
        snapshot: u64,
        first_row: u32,
        count: u32,
    ) -> Option<CellRows> {
        self.rest.read_snapshot_cells(snapshot, first_row, count)
    }

    fn drop_snapshot(&mut self, snapshot: u64) {
        self.rest.drop_snapshot(snapshot);
    }

    fn history_text(&mut self, pane: NodeId, max_bytes: u32) -> Result<RequestId, PluginError> {
        self.rest.history_text(pane, max_bytes)
    }

    fn take_history(&mut self, request: RequestId) -> Option<String> {
        self.rest.take_history(request)
    }

    fn search_history(
        &mut self,
        pane: NodeId,
        regex: String,
        max_matches: u16,
    ) -> Result<RequestId, PluginError> {
        self.rest.search_history(pane, regex, max_matches)
    }

    fn take_matches(&mut self, request: RequestId) -> Option<Vec<HistoryMatch>> {
        self.rest.take_matches(request)
    }

    /// Refused: nothing in this kernel holds back input for a locked pane yet, and a lock that
    /// answers `ok` without holding anything would mislead the plugin.
    fn lock_input(&mut self, _pane: NodeId, _millis: u16) -> Result<(), PluginError> {
        Err(PluginError::new(
            ErrorCode::Unsupported,
            "input locks are not enforced by this kernel yet",
        ))
    }

    fn unlock_input(&mut self, pane: NodeId) {
        self.rest.unlock_input(pane);
    }
}

#[cfg(test)]
mod grant_tests {
    use super::holds;

    fn granted(grants: &[&str]) -> Vec<String> {
        grants.iter().map(|grant| (*grant).to_owned()).collect()
    }

    #[test]
    fn a_grant_is_held_when_it_is_named_exactly() {
        assert!(holds(&granted(&["tree.read"]), "tree.read"));
        assert!(!holds(&granted(&["tree.read"]), "tree.metadata"));
        assert!(!holds(&[], "tree.read"));
    }

    #[test]
    fn a_wildcard_of_the_grant_grammar_covers_its_family() {
        let held = granted(&["topics.subscribe:*", "commands.invoke:com.example.notes/*"]);
        assert!(holds(&held, "topics.subscribe:room/news"));
        assert!(holds(&held, "commands.invoke:com.example.notes/open"));
        assert!(!holds(&held, "commands.invoke:com.example.other/open"));
    }

    #[test]
    fn a_family_without_a_wildcard_form_matches_exactly() {
        // The manifest check refuses this grant; holding it by accident unlocks nothing.
        let held = granted(&["settings.read-kernel:*"]);
        assert!(!holds(&held, "settings.read-kernel:terminal"));
        assert!(holds(
            &granted(&["settings.read-kernel:terminal"]),
            "settings.read-kernel:terminal"
        ));
    }
}
