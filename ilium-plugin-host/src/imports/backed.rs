//! Kernel-backed implementations of the pilot-set imports.
//!
//! The unit-struct imports of this directory answer for a host without a session. The types
//! here answer from the live kernel through `KernelFacade` and from the contribution
//! registry; `HostRuntime` puts them into the import set of each plugin instance.
//!
//! None of them holds a lock across a kernel call other than its own queue lock, and none
//! of them touches a PTY: terminal access goes through the facade to the pane's owner.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use ilium_plugin_api::generated::host::commands::HostCommands;
use ilium_plugin_api::generated::host::intents_host::HostIntentsHost;
use ilium_plugin_api::generated::host::log::{HostLog, Level};
use ilium_plugin_api::generated::host::panes::HostPanes;
use ilium_plugin_api::generated::host::tree::HostTree;
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::{NodeId, PluginId, RequestId};
use ilium_plugin_api::generated::types::intents::Submission;
use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef, RequestResult};
use ilium_plugin_api::generated::types::tree::{
    NodeClass, NodeQuery, NodeView, PaneInfo, PrimitiveKind,
};
use ilium_plugin_api::kernel::{KernelFacade, Origin};
use ilium_plugin_api::Side;

use crate::contributions::ContributionRegistry;

/// Largest `arguments-json` accepted by `commands.invoke`: 64 KiB.
const MAX_ARGUMENTS_JSON_BYTES: usize = 64 * 1024;
/// Most request results or command invocations kept for one runtime before the oldest
/// are dropped; a plugin that never reads its events cannot grow the host without bound.
const QUEUE_CAPACITY: usize = 4096;

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

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
}

impl KernelTree {
    /// A tree import reading the given kernel.
    pub fn new(kernel: Arc<dyn KernelFacade>) -> Self {
        Self { kernel }
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
        self.kernel.tree().node(node)
    }

    fn children(&mut self, node: NodeId) -> Vec<NodeId> {
        self.kernel
            .tree()
            .nodes()
            .into_iter()
            .filter(|candidate| candidate.parent == Some(node))
            .map(|candidate| candidate.id)
            .collect()
    }

    fn roots(&mut self) -> Vec<NodeId> {
        self.kernel
            .tree()
            .nodes()
            .into_iter()
            .filter(|candidate| candidate.parent.is_none())
            .map(|candidate| candidate.id)
            .collect()
    }

    fn find(&mut self, query: NodeQuery) -> Vec<NodeId> {
        let nodes = self.kernel.tree().nodes();
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

    fn metadata(&mut self, _node: NodeId) -> Option<Vec<u8>> {
        // Node metadata has no kernel store yet (contract section 11, open item).
        None
    }

    fn set_metadata(&mut self, node: NodeId, _bytes: Vec<u8>) -> Result<(), PluginError> {
        if self.kernel.tree().node(node).is_none() {
            return Err(PluginError::new(
                ErrorCode::NotFound,
                "the node does not exist",
            ));
        }
        Err(PluginError::new(
            ErrorCode::Unsupported,
            "node metadata is not stored by this kernel yet",
        ))
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
        }
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
        let request = RequestId(NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed));
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
        }
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
        let request = RequestId(NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed));
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
