//! The real plugin host of the server (foundation contract sections 2.2 and 12).
//!
//! `start` builds the engine, the kernel facade and the `HostRuntime`, moves the runtime onto
//! its own thread and queues loading and startup activation as the first job, so `run()` never
//! waits for a component to compile. Everything the rest of the server does with plugins is a
//! job posted to that thread: tokio tasks post and never block on it, and the thread reaches
//! the server only through the facade. Terminal output never enters this module, so a trapped
//! or stalled plugin cannot delay a PTY (COR-218).
//!
//! After each job the host publishes what changed: contribution increments as
//! `ServerEvent::Contributions` and plugin state changes as plugin control envelopes.

mod facade;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use ilium_ipc::plugin::{
    ContributionDelta, ContributionOp as WireOp, ControlBody, DeltaKind, EnvelopeBody,
    OutcomeRecord, PluginEnvelope,
};
use ilium_ipc::ServerEvent;
use ilium_plugin_api::generated::host::log::Level;
use ilium_plugin_api::generated::types::events::MessageEvent;
use ilium_plugin_api::generated::types::{
    ErrorCode, Event, Intent, NodeId, Outcome, PluginError, PluginId, PluginSide,
};
use ilium_plugin_api::kernel::{KernelFacade, Origin};
use ilium_plugin_api::platform::ProcessKind;
use ilium_plugin_api::side::Side;
use ilium_plugin_host::contributions::ContributionOp;
use ilium_plugin_host::engine::{EngineConfig, PluginEngine};
use ilium_plugin_host::imports::backed::LogSink;
use ilium_plugin_host::imports::basic::BasicBackend;
use ilium_plugin_host::lifecycle::StartMode;
use ilium_plugin_host::runtime::{
    CataloguePlugin, HostRuntime, RuntimeConfig, RuntimeThread, ESSENTIAL_PLUGINS,
};
use ilium_plugin_host::storage::StorageContext;
use ilium_plugin_host::{registry, wiring};

use crate::ipc::DirectEventSender;
use crate::state::ServerState;
use facade::{RuntimeSlot, ServerFacade};

/// How long plugins get to deactivate when the server stops (contract section 2.2).
const SHUTDOWN_BUDGET: Duration = Duration::from_secs(5);
/// How long the server waits for the runtime thread to hand back its shutdown report. Above
/// the budget, because a job that was already running finishes first.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(8);
/// How long a client submission may take; above the facade's own request budget.
const SUBMISSION_WAIT: Duration = Duration::from_secs(35);
/// Kernel events that may wait for the runtime before the feed collapses them into one
/// "everything may have changed" event.
const PENDING_EVENT_LIMIT: usize = 1024;
/// The epoch tick of the server engine in milliseconds.
const EPOCH_TICK_MS: u64 = 10;

/// What the composition root hands the server to run plugins.
pub struct PluginStart {
    /// The plugins this build ships, manifests and server components.
    pub catalogue: Vec<CataloguePlugin>,
    /// The user data directory; the installed registry, the plugin storage and the compiled
    /// code cache live under `<data_dir>/plugins`.
    pub data_dir: PathBuf,
    /// Normal, safe or no plugins. `NoPlugins` starts no host at all.
    pub mode: StartMode,
}

impl std::fmt::Debug for PluginStart {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PluginStart")
            .field("plugins", &self.catalogue.len())
            .field("data_dir", &self.data_dir)
            .field("mode", &self.mode)
            .finish()
    }
}

/// Plugin log lines join the server log.
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

/// What attached clients are told about plugins, kept so a new client gets a snapshot without
/// asking the runtime thread.
#[derive(Default)]
struct Published {
    entries: BTreeMap<String, Vec<u8>>,
    states: BTreeMap<String, String>,
    sequence: u64,
}

/// Publishes runtime changes to clients. Lives on the runtime thread and in the handle.
#[derive(Clone)]
struct Publisher {
    state: Weak<ServerState>,
    published: Arc<Mutex<Published>>,
    next_envelope: Arc<AtomicU64>,
}

impl Publisher {
    fn state_event(&self, plugin: &str, state: &str) -> ServerEvent {
        ServerEvent::Plugin(PluginEnvelope {
            plugin: plugin.to_owned(),
            seq: self.next_envelope.fetch_add(1, Ordering::Relaxed),
            body: EnvelopeBody::Control(ControlBody::PluginState {
                plugin: plugin.to_owned(),
                state: state.to_owned(),
            }),
        })
    }

    /// Broadcasts the contribution increments and state changes since the last call.
    fn flush(&self, runtime: &mut HostRuntime) {
        let ops = runtime.take_contribution_ops();
        let observed: Vec<(String, String)> = runtime
            .plugin_ids()
            .into_iter()
            .filter_map(|id| {
                let state = runtime.state(&id)?;
                Some((id, wiring::state_name(&state)))
            })
            .collect();
        let Ok(mut published) = self.published.lock() else {
            return;
        };
        let mut events = Vec::new();
        if !ops.is_empty() {
            let mut wire = Vec::with_capacity(ops.len());
            for op in ops {
                match op {
                    ContributionOp::Put(entry) => {
                        let id = entry.id();
                        let bytes = wiring::encode_entry(&entry);
                        published.entries.insert(id.clone(), bytes.clone());
                        wire.push(WireOp::Put { id, entry: bytes });
                    }
                    ContributionOp::Remove(id) => {
                        published.entries.remove(&id);
                        wire.push(WireOp::Remove { id });
                    }
                    ContributionOp::Clear(plugin) => {
                        // A contribution id is `<plugin>/<kind>/<name>`.
                        let prefix = format!("{plugin}/");
                        published.entries.retain(|id, _| !id.starts_with(&prefix));
                        wire.push(WireOp::Clear { plugin });
                    }
                }
            }
            published.sequence += 1;
            events.push(ServerEvent::Contributions(ContributionDelta {
                sequence: published.sequence,
                kind: DeltaKind::Increment,
                ops: wire,
            }));
        }
        for (plugin, state) in observed {
            if published.states.get(&plugin) != Some(&state) {
                events.push(self.state_event(&plugin, &state));
                published.states.insert(plugin, state);
            }
        }
        drop(published);
        if let Some(state) = self.state.upgrade() {
            for event in events {
                state.broadcast(event);
            }
        }
    }

    /// The events that bring a newly attached client up to date.
    fn snapshot(&self) -> Vec<ServerEvent> {
        let Ok(published) = self.published.lock() else {
            return Vec::new();
        };
        let mut events = vec![ServerEvent::Contributions(ContributionDelta {
            sequence: published.sequence,
            kind: DeltaKind::Snapshot,
            ops: published
                .entries
                .iter()
                .map(|(id, entry)| WireOp::Put {
                    id: id.clone(),
                    entry: entry.clone(),
                })
                .collect(),
        })];
        for (plugin, state) in &published.states {
            events.push(self.state_event(plugin, state));
        }
        events
    }
}

/// The running plugin host of one server. Owned by `ServerState::plugins`.
pub(crate) struct PluginHostHandle {
    runtime: RuntimeSlot,
    facade: Arc<ServerFacade>,
    publisher: Publisher,
    /// Id and manifest version of every catalogue plugin, for the enable store.
    versions: BTreeMap<String, String>,
    data_dir: PathBuf,
    epoch: Arc<AtomicU64>,
    pending: Arc<Mutex<Vec<Event>>>,
    feed_scheduled: Arc<AtomicBool>,
}

fn log_errors(what: &str, errors: Vec<ilium_plugin_host::runtime::RuntimeError>) {
    for error in errors {
        tracing::warn!("plugin host, {what}: {error}");
    }
}

/// Starts the host. `None` when plugins are off or the host could not be built; the server
/// then runs without plugins and says so in its log.
pub(crate) fn start(state: &Arc<ServerState>, start: PluginStart) -> Option<Arc<PluginHostHandle>> {
    if matches!(start.mode, StartMode::NoPlugins) {
        tracing::info!("plugins are off for this server (--no-plugins)");
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
        process: ProcessKind::Server,
    }) {
        Ok(engine) => Arc::new(engine),
        Err(error) => {
            tracing::error!("plugin engine did not start, plugins are off: {error}");
            return None;
        }
    };
    let runtime_slot: RuntimeSlot = Arc::new(Mutex::new(None));
    let epoch = Arc::new(AtomicU64::new(1));
    let next_envelope = Arc::new(AtomicU64::new(1));
    let facade = Arc::new(ServerFacade::new(
        state,
        tokio::runtime::Handle::current(),
        Arc::clone(&runtime_slot),
        Arc::clone(&epoch),
        Arc::clone(&next_envelope),
    ));
    let (disabled, warning) = wiring::disabled_at_start(&start.data_dir);
    if let Some(warning) = warning {
        tracing::warn!("installed plugin registry unreadable, every plugin enabled: {warning}");
    }
    let log: Arc<dyn LogSink> = Arc::new(TracingLog);
    let provider = wiring::import_provider(wiring::ProcessImports {
        storage_root: wiring::storage_root(&start.data_dir),
        scope: StorageContext {
            project: Some(state.session_cwd.clone()),
            session: Some(state.session_name.clone()),
            ..StorageContext::default()
        },
        basic: Some(Arc::clone(&facade) as Arc<dyn BasicBackend>),
        log: Arc::clone(&log),
    });
    let runtime = HostRuntime::new(
        engine,
        RuntimeConfig {
            side: Side::Server,
            kernel: Arc::clone(&facade) as Arc<dyn KernelFacade>,
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
    let publisher = Publisher {
        state: Arc::downgrade(state),
        published: Arc::new(Mutex::new(Published::default())),
        next_envelope,
    };
    let versions = start
        .catalogue
        .iter()
        .map(|plugin| {
            (
                plugin.id.clone(),
                wiring::manifest_version(&plugin.manifest),
            )
        })
        .collect();
    let catalogue = start.catalogue;
    let first = publisher.clone();
    // Loading compiles components; it runs on the host thread so `run()` is not held up.
    // In the slot before the first job runs, so a plugin can message itself while starting.
    let posted = runtime_slot.lock().ok().is_some_and(|mut slot| {
        slot.insert(thread).post(move |runtime| {
            log_errors("load", runtime.load(catalogue));
            log_errors("startup activation", runtime.activate_startup());
            log_errors("pump", runtime.pump());
            first.flush(runtime);
            tracing::info!(plugins = runtime.plugin_ids().len(), "plugin host started");
        })
    });
    if !posted {
        tracing::error!("plugin host thread stopped before loading, plugins are off");
        return None;
    }
    Some(Arc::new(PluginHostHandle {
        runtime: runtime_slot,
        facade,
        publisher,
        versions,
        data_dir: start.data_dir,
        epoch,
        pending: Arc::new(Mutex::new(Vec::new())),
        feed_scheduled: Arc::new(AtomicBool::new(false)),
    }))
}

impl PluginHostHandle {
    /// Queues work for the runtime and publishes what it changed. False when the host stopped.
    fn post(&self, job: impl FnOnce(&mut HostRuntime) + Send + 'static) -> bool {
        let publisher = self.publisher.clone();
        self.runtime.lock().ok().is_some_and(|slot| {
            slot.as_ref().is_some_and(|thread| {
                thread.post(move |runtime| {
                    job(runtime);
                    publisher.flush(runtime);
                })
            })
        })
    }

    /// The contribution snapshot and plugin states for a client that just attached.
    pub(crate) fn attach_events(&self) -> Vec<ServerEvent> {
        self.publisher.snapshot()
    }

    /// Hands kernel events to the plugins. Coalesced: one queued job drains everything that
    /// arrived meanwhile, and an overfull backlog becomes one unspecific tree change.
    fn feed(&self, batch: Vec<Event>) {
        {
            let Ok(mut pending) = self.pending.lock() else {
                return;
            };
            if pending.len() + batch.len() > PENDING_EVENT_LIMIT {
                pending.clear();
                pending.push(Event::TreeChanged(Vec::new()));
            } else {
                pending.extend(batch);
            }
        }
        if self.feed_scheduled.swap(true, Ordering::AcqRel) {
            return;
        }
        let pending = Arc::clone(&self.pending);
        let scheduled = Arc::clone(&self.feed_scheduled);
        let posted = self.post(move |runtime| {
            scheduled.store(false, Ordering::Release);
            let batch = pending
                .lock()
                .map(|mut pending| std::mem::take(&mut *pending))
                .unwrap_or_default();
            if !batch.is_empty() {
                log_errors("event delivery", runtime.broadcast(&batch));
                log_errors("pump", runtime.pump());
            }
        });
        if !posted {
            self.feed_scheduled.store(false, Ordering::Release);
        }
    }

    /// Routes one plugin envelope a client sent (`ClientRequest::Plugin`).
    pub(crate) async fn handle_envelope(
        &self,
        envelope: PluginEnvelope,
        direct_tx: &DirectEventSender,
    ) {
        let PluginEnvelope { plugin, seq, body } = envelope;
        match body {
            EnvelopeBody::Submit(submission) => {
                let outcome = self.submit(&plugin, &submission.intent_json).await;
                let outcome_json = serde_json::to_string(&outcome).unwrap_or_else(|error| {
                    // Outcome is plain data; this arm only keeps the reply well formed.
                    tracing::error!("plugin outcome not serializable: {error}");
                    "\"cancelled\"".to_owned()
                });
                let reply = ServerEvent::Plugin(PluginEnvelope {
                    plugin,
                    seq,
                    body: EnvelopeBody::Outcome(OutcomeRecord {
                        seq,
                        dedup_key: submission.dedup_key,
                        outcome_json,
                    }),
                });
                let _ = direct_tx.send(reply).await;
            }
            EnvelopeBody::Message(message) => {
                let event = Event::Message(MessageEvent {
                    from: PluginId(plugin.clone()),
                    from_side: PluginSide::Client,
                    from_client: None,
                    channel: message.topic,
                    payload: message.payload,
                });
                self.post(move |runtime| {
                    if let Err(error) = runtime.deliver(&plugin, vec![event]) {
                        tracing::debug!("client plugin message not delivered: {error}");
                    }
                    log_errors("pump", runtime.pump());
                });
            }
            EnvelopeBody::Control(ControlBody::Resync) => {
                for event in self.attach_events() {
                    if direct_tx.send(event).await.is_err() {
                        break;
                    }
                }
            }
            EnvelopeBody::Control(ControlBody::SetEnabled {
                plugin: target,
                enabled,
            }) => {
                let state = self.set_enabled(target.clone(), enabled).await;
                let _ = direct_tx
                    .send(self.publisher.state_event(&target, &state))
                    .await;
            }
            other => {
                tracing::debug!(
                    plugin,
                    "plugin envelope not handled by the server: {other:?}"
                );
            }
        }
    }

    /// Applies an intent a client-side plugin submitted. The intent runs on the host thread,
    /// where the facade is allowed to block.
    async fn submit(&self, plugin: &str, intent_json: &str) -> Outcome {
        let intent: Intent = match serde_json::from_str(intent_json) {
            Ok(intent) => intent,
            Err(error) => {
                return Outcome::Refused(PluginError::new(
                    ErrorCode::InvalidArgument,
                    format!("the submitted intent does not parse: {error}"),
                ));
            }
        };
        let origin = Origin::Plugin {
            plugin: PluginId(plugin.to_owned()),
            side: Side::Client,
            instance: 0,
        };
        let facade = Arc::clone(&self.facade);
        let (reply, result) = tokio::sync::oneshot::channel();
        let posted = self.post(move |_runtime| {
            // The client may have gone away; its reply channel is then closed.
            let _ = reply.send(facade.apply_intent(&origin, intent));
        });
        if !posted {
            return Outcome::Failed(PluginError::new(
                ErrorCode::Busy,
                "the plugin host is stopping",
            ));
        }
        match tokio::time::timeout(SUBMISSION_WAIT, result).await {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(_)) => Outcome::Failed(PluginError::new(
                ErrorCode::Internal,
                "the plugin host dropped the submission",
            )),
            Err(_) => Outcome::Failed(PluginError::new(
                ErrorCode::TimedOut,
                "the server did not apply the intent in time",
            )),
        }
    }

    /// Enables or disables a plugin now and for later starts. Returns the state name the
    /// plugin has afterwards, or `refused:<code>`.
    async fn set_enabled(&self, plugin: String, enabled: bool) -> String {
        let Some(version) = self.versions.get(&plugin).cloned() else {
            return "refused:plugin.unknown".to_owned();
        };
        if !enabled && ESSENTIAL_PLUGINS.contains(&plugin.as_str()) {
            return "refused:plugin.essential".to_owned();
        }
        let installed = wiring::installed_path(&self.data_dir);
        let (reply, result) = tokio::sync::oneshot::channel();
        let posted = self.post(move |runtime| {
            // The store first: a plugin never runs in a state the next start would not keep.
            let name = match registry::set_plugin_enabled(&installed, &plugin, &version, enabled) {
                Err(error) => format!("refused:{}", error.code()),
                Ok(_) => match runtime.set_enabled(&plugin, enabled) {
                    Err(error) => {
                        tracing::warn!("plugin {plugin} did not change state: {error}");
                        "refused:plugin.state".to_owned()
                    }
                    Ok(()) => {
                        if enabled {
                            log_errors("startup activation", runtime.activate_startup());
                        }
                        log_errors("pump", runtime.pump());
                        runtime.state(&plugin).map_or_else(
                            || "not-running".to_owned(),
                            |state| wiring::state_name(&state),
                        )
                    }
                },
            };
            let _ = reply.send(name);
        });
        if !posted {
            return "refused:host.stopping".to_owned();
        }
        match tokio::time::timeout(SUBMISSION_WAIT, result).await {
            Ok(Ok(name)) => name,
            _ => "refused:host.timeout".to_owned(),
        }
    }

    /// Deactivates every plugin and joins the host thread. Called once, before the session
    /// drain, so plugins can still persist through the kernel.
    pub(crate) async fn stop(&self) {
        let thread = self.runtime.lock().ok().and_then(|mut slot| slot.take());
        let Some(thread) = thread else {
            return;
        };
        let join = tokio::task::spawn_blocking(move || thread.shutdown(SHUTDOWN_BUDGET));
        match tokio::time::timeout(SHUTDOWN_WAIT, join).await {
            Ok(Ok(report)) => tracing::info!(
                deactivated = report.deactivated.len(),
                forced = report.forced.len(),
                "plugin host stopped"
            ),
            Ok(Err(error)) => tracing::warn!("plugin host shutdown task failed: {error}"),
            // The thread is left behind; the process is exiting and it owns no PTY.
            Err(_) => tracing::warn!("plugin host did not stop in time and was abandoned"),
        }
    }
}

fn tree_ids(tree: &ilium_core::Tree) -> BTreeSet<u64> {
    tree.all_ids().map(|id| id.0).collect()
}

/// Forwards tree changes to the plugins until the event bus closes or the task is aborted.
/// Terminal output and every other high-rate event are dropped here, before the host.
pub(crate) fn spawn_feed(
    state: Arc<ServerState>,
    host: Arc<PluginHostHandle>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // Subscribed before the first read so no change between the two is lost.
        let mut events = state.events.subscribe_shared();
        let mut known = tree_ids(&*state.tree.read().await);
        loop {
            let batch = match events.recv().await {
                Ok(event) => match &*event {
                    ServerEvent::TreeSnapshot(tree) => {
                        let current = tree_ids(tree);
                        let mut batch: Vec<Event> = known
                            .difference(&current)
                            .map(|id| Event::NodeRemoved(NodeId(*id)))
                            .collect();
                        batch.push(Event::TreeChanged(
                            current.difference(&known).map(|id| NodeId(*id)).collect(),
                        ));
                        known = current;
                        batch
                    }
                    ServerEvent::PaneNodeChanged(node) => {
                        known.insert(node.id.0);
                        vec![Event::TreeChanged(vec![NodeId(node.id.0)])]
                    }
                    _ => Vec::new(),
                },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    known = tree_ids(&*state.tree.read().await);
                    vec![Event::TreeChanged(Vec::new())]
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            };
            if !batch.is_empty() {
                host.epoch.fetch_add(1, Ordering::Relaxed);
                host.feed(batch);
            }
        }
    })
}
