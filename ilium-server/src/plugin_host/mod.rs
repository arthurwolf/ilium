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

mod cli_bridge;
mod facade;
mod http_routes;
mod remote;
mod signatures;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use tokio::task::JoinSet;

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
use ilium_plugin_host::imports::http_api::{HttpResponseSink, RoutedHttpApi};
use ilium_plugin_host::lifecycle::StartMode;
use ilium_plugin_host::runtime::cross_side::{RemoteMessage, REMOTE_ENVELOPE_PLUGIN};
use ilium_plugin_host::runtime::{
    CataloguePlugin, HostRuntime, ImportProvider, RuntimeConfig, RuntimeThread, ESSENTIAL_PLUGINS,
};
use ilium_plugin_host::storage::StorageContext;
use ilium_plugin_host::{registry, wiring};

use crate::ipc::DirectEventSender;
use crate::state::ServerState;
use cli_bridge::{CliInvocation, CliRoutes};
use facade::{RuntimeSlot, ServerFacade};
use http_routes::{HttpCalls, HttpRoutes};
pub(crate) use http_routes::{PluginRequest, Reply, MAX_REQUEST_BODY_BYTES};

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
    /// The kernel facade, whose queue holds the chrome server plugins set.
    facade: Arc<ServerFacade>,
    /// Where the output of running CLI commands goes.
    cli: Arc<CliRoutes>,
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
        let mut ops = runtime.take_contribution_ops();
        // Status segments and decorations of server plugins travel with the contributions,
        // after them: a `Clear` of a disabled plugin has then already removed its chrome
        // (contract section 20.4).
        ops.extend(self.facade.take_live_chrome());
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
        for chunk in runtime.take_cli_output() {
            self.cli.route(chunk);
        }
        for message in runtime.take_remote_outbound() {
            events.push(ServerEvent::Plugin(PluginEnvelope {
                plugin: REMOTE_ENVELOPE_PLUGIN.to_owned(),
                seq: self.next_envelope.fetch_add(1, Ordering::Relaxed),
                body: remote::body_of(message.to_wire()),
            }));
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
    /// The running CLI invocations; the same table the publisher routes output through.
    cli: Arc<CliRoutes>,
    /// The forwarding task of each running CLI invocation, aborted when the host stops.
    cli_tasks: Mutex<JoinSet<()>>,
    /// The routes plugins registered on the loopback HTTP API and the requests they answer.
    http: Arc<HttpRoutes>,
    /// The agent signatures of the catalogue plugins, republished on enable and disable.
    signatures: Mutex<signatures::ContributedSignatures>,
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
    // The detection loop matches the enabled plugins' agent signatures (gap G6).
    let contributed = signatures::ContributedSignatures::new(
        start
            .catalogue
            .iter()
            .map(|plugin| (plugin.id.as_str(), plugin.manifest.as_str())),
        disabled.clone(),
        matches!(start.mode, StartMode::SafePlugins),
    );
    publish_signatures(state, &contributed);
    let log: Arc<dyn LogSink> = Arc::new(TracingLog);
    let http_calls = Arc::new(HttpCalls::default());
    let base_provider = wiring::import_provider(wiring::ProcessImports {
        storage_root: wiring::storage_root(&start.data_dir),
        scope: StorageContext {
            project: Some(state.session_cwd.clone()),
            session: Some(state.session_name.clone()),
            ..StorageContext::default()
        },
        basic: Some(Arc::clone(&facade) as Arc<dyn BasicBackend>),
        log: Arc::clone(&log),
        backends: wiring::wave_two::Backends::server(
            state.session_cwd.clone(),
            start.data_dir.clone(),
            wiring::wave_two::ServerBackendInputs {
                // `ilium:host/inference` answers through the provider selected in Settings.
                inference: Some(Arc::new(
                    ilium_plugin_host::imports::inference_backend::SettingsInference::of_user(),
                )),
                agent_scopes: directories::BaseDirs::new()
                    .map(|dirs| wiring::wave_two::agent_scopes_in(dirs.home_dir()))
                    .unwrap_or_default(),
            },
        ),
    });
    // A plugin that holds an `http-api.route` grant answers its requests through the server's
    // open-call table; the runtime removes the import again for an instance without the grant.
    let response_sink: Arc<dyn HttpResponseSink> =
        Arc::clone(&http_calls) as Arc<dyn HttpResponseSink>;
    let provider: ImportProvider = Box::new(move |context, set| {
        base_provider(context, set);
        set.http_api = Some(Box::new(RoutedHttpApi::new(
            &context.plugin,
            Arc::clone(&response_sink),
        )));
    });
    let mut runtime = HostRuntime::new(
        engine,
        RuntimeConfig {
            side: Side::Server,
            kernel: Arc::clone(&facade) as Arc<dyn KernelFacade>,
            log,
            provider: Some(provider),
            disabled,
        },
    );
    runtime.set_durable_timer_file(wiring::durable_timers_path(&start.data_dir, "server"));
    // The plugin journal belongs to the session: it sits beside the session snapshot, so its
    // entries survive a restart of this session's server (capability-journal-read).
    runtime.set_journal_file(state.snapshot_path.with_extension("plugin-journal.jsonl"));
    // Every service call is checked against the provider's schema file (gap G13).
    for schema in start
        .catalogue
        .iter()
        .filter_map(|plugin| ilium_plugin_bundle::find(&plugin.id))
        .flat_map(|bundled| bundled.schemas)
    {
        if let Err(error) = runtime.install_schema(schema.service, schema.text) {
            tracing::warn!(
                service = schema.service,
                "service schema not installed: {error}"
            );
        }
    }
    let thread = match RuntimeThread::spawn(runtime) {
        Ok(thread) => thread,
        Err(error) => {
            tracing::error!("plugin host thread did not start, plugins are off: {error}");
            return None;
        }
    };
    let cli = Arc::new(CliRoutes::default());
    let publisher = Publisher {
        state: Arc::downgrade(state),
        published: Arc::new(Mutex::new(Published::default())),
        next_envelope,
        facade: Arc::clone(&facade),
        cli: Arc::clone(&cli),
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
    let http = Arc::new(HttpRoutes::new(&start.catalogue, http_calls));
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
        cli,
        cli_tasks: Mutex::new(JoinSet::new()),
        http,
        signatures: Mutex::new(contributed),
    }))
}

/// Hands the active plugin signatures to the detection loop.
fn publish_signatures(state: &ServerState, contributed: &signatures::ContributedSignatures) {
    if let Ok(mut slot) = state.contributed_signatures.write() {
        *slot = contributed.active();
    }
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

    /// True when at least one plugin registered an HTTP route.
    pub(crate) fn has_http_routes(&self) -> bool {
        self.http.has_routes()
    }

    /// The answer for a request that matches no plugin route, `None` when one matches.
    pub(crate) fn http_precheck(&self, method: &str, target: &str) -> Option<Reply> {
        self.http.precheck(method, target)
    }

    /// Serves one request for a plugin route (`/plugins/<plugin-id>/<path>`).
    pub(crate) async fn http_request(&self, request: PluginRequest) -> Reply {
        self.http.dispatch(self, request).await
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
            EnvelopeBody::Control(ControlBody::CliInvoke {
                invocation_id,
                plugin: owner,
                command,
                args,
                stdin,
                cwd,
                columns: _,
            }) => {
                let invocation = CliInvocation {
                    wire_id: invocation_id,
                    plugin: owner,
                    command,
                    args,
                    stdin,
                    cwd,
                };
                self.cli_invoke(invocation, direct_tx).await;
            }
            EnvelopeBody::Control(ControlBody::CliCancel { invocation_id }) => {
                self.cli_cancel(invocation_id, direct_tx);
            }
            EnvelopeBody::Control(ControlBody::SettingsChanged {
                plugin: target,
                keys,
            }) => {
                // The user changed settings of the plugin in a client; its server side hears
                // the same `settings-changed` the client side does. A plugin whose server side
                // is not running reads the settings when it starts.
                let change = settings_changed_event(keys);
                self.post(move |runtime| {
                    if let Err(error) = runtime.deliver(&target, vec![change]) {
                        tracing::debug!("settings-changed not delivered to {target}: {error}");
                    }
                    log_errors("pump", runtime.pump());
                });
            }
            body @ (EnvelopeBody::Event(_)
            | EnvelopeBody::ClientRequest(_)
            | EnvelopeBody::ClientReply(_))
                if plugin == REMOTE_ENVELOPE_PLUGIN =>
            {
                // A topic message or service traffic of the client runtime (gap G4).
                let Some(message) =
                    remote::wire_of(body).and_then(|wire| RemoteMessage::from_wire(&wire))
                else {
                    tracing::debug!("malformed cross-side envelope from a client dropped");
                    return;
                };
                self.post(move |runtime| {
                    runtime.deliver_remote(vec![message]);
                    log_errors("pump", runtime.pump());
                });
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
        let plugin_id = plugin.clone();
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
        let name = match tokio::time::timeout(SUBMISSION_WAIT, result).await {
            Ok(Ok(name)) => name,
            _ => "refused:host.timeout".to_owned(),
        };
        if !name.starts_with("refused:") {
            // The lock is released before the await: the new set is taken out first.
            let active = self.signatures.lock().ok().map(|mut contributed| {
                contributed.set_enabled(&plugin_id, enabled);
                contributed.active()
            });
            if let (Some(active), Some(state)) = (active, self.publisher.state.upgrade()) {
                // Panes identify again with the new set at once, not at their next slow
                // check (gap G6).
                state.replace_contributed_signatures(active).await;
            }
        }
        name
    }

    /// Deactivates every plugin and joins the host thread. Called once, before the session
    /// drain, so plugins can still persist through the kernel.
    pub(crate) async fn stop(&self) {
        self.cli_stop();
        let thread = self.runtime.lock().ok().and_then(|mut slot| slot.take());
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
            // The thread is left behind; the process is exiting and it owns no PTY.
            Err(_) => tracing::warn!("plugin host did not stop in time and was abandoned"),
        }
    }
}

/// How often the server side looks at `config.toml` for kernel settings, theme and keymap
/// changes (gap G2). A stat per tick; the file is read only when it changed.
const CONFIG_CHECK_INTERVAL: Duration = Duration::from_secs(2);

/// How long the feed waits for a new pane's screen to learn its size for `pane-created`.
const PANE_SIZE_BUDGET: Duration = Duration::from_millis(200);

/// Gives each `pane-created` event the size of its pane's terminal screen. A pane with no
/// terminal yet, or one that does not answer within the budget, keeps 0 x 0.
async fn fill_pane_sizes(state: &ServerState, events: &mut [Event]) {
    for event in events.iter_mut() {
        let Event::PaneCreated(info) = event else {
            continue;
        };
        let read = crate::pane::read_current_terminal_screen(
            state,
            ilium_core::NodeId(info.node.0),
            |screen| screen.size(),
        );
        if let Ok(Some((rows, columns))) = tokio::time::timeout(PANE_SIZE_BUDGET, read).await {
            info.columns = columns;
            info.rows = rows;
        }
    }
}

/// The modification time and length of a file, `None` when it cannot be read.
fn file_stamp(path: &std::path::Path) -> Option<(std::time::SystemTime, u64)> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}

/// Forwards kernel changes to the plugins until the event bus closes or the task is aborted:
/// tree changes with the pane, title and agent events derived from them, and the kernel
/// settings, theme and keymap changes of `config.toml` (gap G2). Terminal output and every
/// other high-rate event are dropped here, before the host.
pub(crate) fn spawn_feed(
    state: Arc<ServerState>,
    host: Arc<PluginHostHandle>,
) -> tokio::task::JoinHandle<()> {
    use ilium_plugin_host::runtime::events_feed;
    tokio::spawn(async move {
        // Subscribed before the first read so no change between the two is lost.
        let mut events = state.events.subscribe_shared();
        let (mut known, mut views) = {
            let tree = state.tree.read().await;
            (facade::tree_marks(&tree), facade::tree_views(&tree))
        };
        let config_path = ilium_platform::paths::config_dir().map(|dir| dir.join("config.toml"));
        let mut config_stamp = config_path.as_deref().and_then(file_stamp);
        let mut config = config_path
            .as_deref()
            .map(events_feed::read_config)
            .unwrap_or_default();
        let mut config_check = tokio::time::interval(CONFIG_CHECK_INTERVAL);
        config_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let received = tokio::select! {
                received = events.recv() => received,
                _ = config_check.tick() => {
                    let Some(path) = config_path.as_deref() else {
                        continue;
                    };
                    let stamp = file_stamp(path);
                    if stamp == config_stamp {
                        continue;
                    }
                    config_stamp = stamp;
                    let current = events_feed::read_config(path);
                    let batch = events_feed::config_events(&config, &current);
                    config = current;
                    if !batch.is_empty() {
                        host.feed(batch);
                    }
                    continue;
                }
            };
            let batch = match received {
                Ok(event) => match &*event {
                    ServerEvent::TreeSnapshot(tree) => {
                        let current = facade::tree_marks(tree);
                        let mut batch = facade::tree_delta(&known, &current);
                        known = current;
                        let current_views = facade::tree_views(tree);
                        let mut node_events =
                            events_feed::node_events(&views, &current_views, None, |id| {
                                facade::title_origin(tree, id)
                            });
                        views = current_views;
                        fill_pane_sizes(&state, &mut node_events).await;
                        batch.extend(node_events);
                        batch
                    }
                    ServerEvent::PaneNodeChanged(node) => {
                        // The fingerprint is unknown until the next snapshot, which then
                        // names the node once more: a change is never lost, only repeated.
                        known.insert(node.id.0, 0);
                        vec![Event::TreeChanged(vec![NodeId(node.id.0)])]
                    }
                    _ => Vec::new(),
                },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let tree = state.tree.read().await;
                    known = facade::tree_marks(&tree);
                    views = facade::tree_views(&tree);
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

/// A client attached to this session, as plugins see it: `client-attached` when made,
/// `client-detached` when dropped with its connection (gap G2).
pub(crate) struct ClientPresence {
    host: Weak<PluginHostHandle>,
    client: u64,
}

impl ClientPresence {
    /// Announces the client; `None` when the session runs without plugins.
    pub(crate) fn attach(state: &ServerState, client: u64) -> Option<Self> {
        let host = state.plugins.get()?;
        host.feed(vec![Event::ClientAttached(
            ilium_plugin_api::generated::types::ClientId(client),
        )]);
        Some(Self {
            host: Arc::downgrade(host),
            client,
        })
    }
}

impl Drop for ClientPresence {
    fn drop(&mut self) {
        if let Some(host) = self.host.upgrade() {
            host.feed(vec![Event::ClientDetached(
                ilium_plugin_api::generated::types::ClientId(self.client),
            )]);
        }
    }
}

/// The `settings-changed` event a client reported for a plugin (`ControlBody::SettingsChanged`):
/// the same change, origin and side the client side of the plugin is given.
fn settings_changed_event(keys: Vec<String>) -> Event {
    Event::SettingsChanged(ilium_plugin_api::generated::types::SettingsChange {
        keys,
        origin: ilium_plugin_api::generated::types::SettingsOrigin::User,
        side: PluginSide::Client,
    })
}

#[cfg(test)]
mod settings_changed_tests {
    use ilium_plugin_api::generated::types::{Event, SettingsOrigin};

    #[test]
    fn a_client_settings_change_reaches_the_server_side_as_settings_changed() {
        let Event::SettingsChanged(change) =
            super::settings_changed_event(vec!["font-size".to_owned()])
        else {
            panic!("a settings-changed event");
        };
        assert_eq!(change.keys, ["font-size"]);
        assert_eq!(change.origin, SettingsOrigin::User);
    }
}
