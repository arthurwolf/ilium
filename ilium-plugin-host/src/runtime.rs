//! The composed plugin host of one process (reference-host-runtime.md, reference-host-architecture.md).
//!
//! `HostRuntime` owns the engine, the process linker, the loaded plugins with their
//! instances, and the contribution registry. The server and the client each build one, for
//! their own side, from the bundled catalogue.
//!
//! Threading: a guest call parks the calling thread, so every method that can call a guest
//! must run on the runtime's own executor thread, never on a tokio worker. `RuntimeThread`
//! is that thread: it owns the runtime, runs posted closures in order and is joined in
//! `shutdown`.
//!
//! PTYs are not reachable from here. A trap, a refused import or a failed activation changes
//! only the plugin record; panes keep running (COR-218).

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ilium_plugin_api::generated::exports::service_provider::{ServiceCall, ServiceReply};
use ilium_plugin_api::generated::host::log::Level;
use ilium_plugin_api::generated::types::events::{
    CommandContext, CommandInvokedEvent, CommandOrigin, Event, TopicPublishedEvent,
};
use ilium_plugin_api::generated::types::ids::PluginId;
use ilium_plugin_api::generated::types::intents::Submission;
use ilium_plugin_api::generated::types::layout::Area;
use ilium_plugin_api::generated::types::lifecycle::{Activation, DeactivationReason};
use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef, RequestResult};
use ilium_plugin_api::generated::types::ui::{UiTree, ViewTarget};
use ilium_plugin_api::grants::Grant;
use ilium_plugin_api::kernel::{KernelFacade, Origin};
use ilium_plugin_api::limits::Limits;
use ilium_plugin_api::{ErrorCode, PluginError, Side, Tier};
use serde_json::Value;

use crate::calls::{ResolvedLimits, Trap};
use crate::contributions::{ContributionOp, ContributionRegistry, Entry};
use crate::engine::{CompiledComponent, PluginEngine};
use crate::error::HostError;
use crate::imports::backed::{
    command_result, CommandQueue, KernelAgent, KernelHelp, KernelIntents, KernelKeymap,
    KernelPanes, KernelSessions, KernelSettingsTables, KernelTerminal, KernelTree, LogSink,
    RegistryCommands, ResultQueue, SinkLog,
};
use crate::imports::cli::{CliBook, CliImport};
use crate::imports::clock::ClockImport;
use crate::imports::extension_points::{ExtensionBook, PluginExtensions};
use crate::imports::generated_dispatch::{ImportSet, IMPORT_INTERFACES};
use crate::imports::services::{BrokeredServices, ServiceBroker};
use crate::imports::settings::SettingsImport;
use crate::imports::timers::{self, TimerBook, TimersImport};
use crate::imports::topics::{self, PluginTopics, TopicOutbox};
use crate::imports::ui_host::UiHostImport;
use crate::instance::{ExportCall, ExportReply, PluginInstance};
use crate::linker::LinkerCache;
use crate::store_data::GuestIdentity;
use crate::wasm_guest::WasmGuest;

pub mod cli;
pub mod cross_side;
pub mod durable_timers;
mod hooks;

/// The plugin API string passed to `activate`.
const API: &str = "ilium:plugin@0.1.0";
/// Rounds of queued commands and request results one `pump` delivers; a plugin that answers
/// every event with a new request continues in the next pump instead of looping here.
const PUMP_ROUNDS: usize = 8;
/// Plugins that refuse `disable` (LIF-183, LIF-276).
pub const ESSENTIAL_PLUGINS: [&str; 4] = [
    "ilium.core.tree",
    "ilium.core.layout",
    "ilium.core.settings",
    "ilium.core.sessions",
];

/// One plugin of the catalogue, for the side of this process.
#[derive(Debug, Clone)]
pub struct CataloguePlugin {
    /// The plugin id.
    pub id: String,
    /// The text of `manifest.json`.
    pub manifest: String,
    /// The component of this process's side, when the plugin has one and it was built.
    pub component: Option<Cow<'static, [u8]>>,
}

/// What an import provider learns about the instance it equips.
#[derive(Debug, Clone)]
pub struct InstanceContext {
    /// The plugin id.
    pub plugin: String,
    /// The side.
    pub side: Side,
    /// The tier.
    pub tier: Tier,
    /// The instance number, new after each activation.
    pub instance: u64,
    /// The granted capabilities.
    pub granted: Vec<String>,
}

/// Adds the imports the embedding process backs itself (storage, basic panes, timers, ...)
/// to the set of a new instance. It may only fill interfaces that `interface_unlocked`
/// allows; the runtime removes any other afterwards.
pub type ImportProvider = Box<dyn Fn(&InstanceContext, &mut ImportSet) + Send>;

/// The configuration of a runtime.
pub struct RuntimeConfig {
    /// The side this process hosts.
    pub side: Side,
    /// The kernel.
    pub kernel: Arc<dyn KernelFacade>,
    /// Where plugin log lines go.
    pub log: Arc<dyn LogSink>,
    /// Extra imports of the embedding process.
    pub provider: Option<ImportProvider>,
    /// Plugins the user disabled (`installed.json`).
    pub disabled: BTreeSet<String>,
}

/// The state of one plugin in this process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginState {
    /// Loaded, waiting for an activation trigger.
    Loaded,
    /// Activated.
    Active,
    /// Disabled by the user; nothing of it is contributed.
    Disabled,
    /// The plugin has no component for this side; only its manifest contributes.
    ManifestOnly,
    /// Stopped by a failure; the code says why.
    Failed(String),
    /// Deactivated at shutdown.
    Stopped,
}

/// Why a runtime operation was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeError {
    /// No plugin has this id.
    #[error("plugin {0} is not loaded")]
    UnknownPlugin(String),
    /// No plugin contributes this command.
    #[error("command {0} is not contributed")]
    UnknownCommand(String),
    /// The plugin is not active.
    #[error("plugin {0} is not active")]
    NotActive(String),
    /// An essential plugin cannot be disabled.
    #[error("plugin {0} is essential and cannot be disabled")]
    Essential(String),
    /// The manifest could not be used.
    #[error("manifest of {plugin}: {message}")]
    Manifest {
        /// The plugin.
        plugin: String,
        /// What is wrong.
        message: String,
    },
    /// Compiling or instantiating failed.
    #[error("plugin {plugin}: {message}")]
    Host {
        /// The plugin.
        plugin: String,
        /// The host error.
        message: String,
    },
    /// `activate` returned an error or trapped.
    #[error("plugin {plugin} failed to activate: {message}")]
    Activation {
        /// The plugin.
        plugin: String,
        /// The plugin error message.
        message: String,
    },
    /// A guest call trapped; the plugin is stopped.
    #[error("plugin {plugin} trapped: {code}")]
    Trapped {
        /// The plugin.
        plugin: String,
        /// The trap code.
        code: String,
    },
}

struct PluginRecord {
    manifest: Value,
    tier: Tier,
    granted: Vec<String>,
    triggers: Vec<String>,
    limits: ResolvedLimits,
    compiled: Option<CompiledComponent>,
    instance: Option<PluginInstance>,
    state: PluginState,
    saved: Option<Vec<u8>>,
    activation_order: u64,
    instance_number: u64,
}

/// What `shutdown` did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShutdownReport {
    /// Plugins whose `deactivate` ran.
    pub deactivated: Vec<String>,
    /// Plugins dropped without `deactivate` because the budget ran out.
    pub abandoned: Vec<String>,
}

pub use crate::linker::interface_unlocked;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // The guarded data stays consistent after a panic: every writer does one insert or pop.
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// The composed host.
pub struct HostRuntime {
    engine: Arc<PluginEngine>,
    linkers: LinkerCache,
    side: Side,
    kernel: Arc<dyn KernelFacade>,
    log: Arc<dyn LogSink>,
    provider: Option<ImportProvider>,
    disabled: BTreeSet<String>,
    plugins: BTreeMap<String, PluginRecord>,
    contributions: Arc<Mutex<ContributionRegistry>>,
    results: ResultQueue,
    commands: CommandQueue,
    /// The timers of the current instance of each plugin. Behind a lock because
    /// `build_imports` creates a book while the runtime is borrowed shared.
    timer_books: Mutex<BTreeMap<String, TimerBook>>,
    /// Durable timers on disk; `None` until the embedder names a file, and then they are only
    /// kept in memory (`set_durable_timer_file`).
    durable_timers: Mutex<Option<durable_timers::DurableTimerStore>>,
    /// Topic messages plugins published since the last pump.
    topic_outbox: TopicOutbox,
    /// What plugins registered on extension points at run time.
    extension_book: ExtensionBook,
    /// Service calls between the plugins of this runtime.
    services: ServiceBroker,
    /// Hook chain breach counters and stops.
    hooks: hooks::HookState,
    /// Topics and service calls that cross to the other side's runtime.
    cross_side: cross_side::CrossSideState,
    /// The running CLI invocations and what they printed.
    cli_book: Arc<CliBook>,
    next_instance: u64,
    job_panics: u64,
    next_activation: u64,
}

impl std::fmt::Debug for HostRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HostRuntime")
            .field("side", &self.side)
            .field("plugins", &self.plugins.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl HostRuntime {
    /// A runtime without plugins.
    pub fn new(engine: Arc<PluginEngine>, config: RuntimeConfig) -> Self {
        let results = ResultQueue::default();
        Self {
            engine,
            linkers: LinkerCache::new(),
            side: config.side,
            kernel: config.kernel,
            log: config.log,
            provider: config.provider,
            disabled: config.disabled,
            plugins: BTreeMap::new(),
            contributions: Arc::new(Mutex::new(ContributionRegistry::new())),
            cli_book: CliBook::new(Arc::clone(&results)),
            results,
            commands: CommandQueue::default(),
            timer_books: Mutex::new(BTreeMap::new()),
            durable_timers: Mutex::new(None),
            topic_outbox: TopicOutbox::default(),
            extension_book: ExtensionBook::default(),
            services: ServiceBroker::default(),
            hooks: hooks::HookState::default(),
            cross_side: cross_side::CrossSideState::default(),
            next_instance: 1,
            job_panics: 0,
            next_activation: 1,
        }
    }

    /// The side this runtime hosts.
    pub fn side(&self) -> Side {
        self.side
    }

    /// Loads the catalogue: parses each manifest, compiles the component of this side and
    /// registers the static contributions of every enabled plugin. A plugin that fails to
    /// load is reported and skipped; the others load.
    pub fn load(&mut self, catalogue: Vec<CataloguePlugin>) -> Vec<RuntimeError> {
        let mut failures = Vec::new();
        for plugin in catalogue {
            if let Err(error) = self.load_one(plugin) {
                failures.push(error);
            }
        }
        failures
    }

    fn load_one(&mut self, plugin: CataloguePlugin) -> Result<(), RuntimeError> {
        let manifest_error = |message: String| RuntimeError::Manifest {
            plugin: plugin.id.clone(),
            message,
        };
        let manifest: Value = serde_json::from_str(&plugin.manifest)
            .map_err(|error| manifest_error(error.to_string()))?;
        if manifest.get("id").and_then(Value::as_str) != Some(plugin.id.as_str()) {
            return Err(manifest_error(
                "the id differs from the catalogue id".to_owned(),
            ));
        }
        let tier = match manifest.get("limits_tier").and_then(Value::as_str) {
            Some("first-party") => Tier::T1,
            _ => Tier::T2,
        };
        let requested: BTreeMap<String, u64> = manifest
            .get("limits")
            .and_then(Value::as_object)
            .map(|limits| {
                limits
                    .iter()
                    .filter_map(|(key, value)| Some((key.clone(), value.as_u64()?)))
                    .collect()
            })
            .unwrap_or_default();
        let limits = ilium_plugin_api::limits::clamp(&Limits(requested), tier)
            .map_err(|diagnostic| manifest_error(format!("{diagnostic:?}")))?;
        let granted = strings(
            manifest
                .get("capabilities")
                .and_then(|capabilities| capabilities.get(self.side.as_str())),
        );
        let triggers = strings(manifest.get("activation"));
        let compiled = match &plugin.component {
            Some(bytes) => {
                Some(
                    self.engine
                        .compile(bytes)
                        .map_err(|error| RuntimeError::Host {
                            plugin: plugin.id.clone(),
                            message: error.to_string(),
                        })?,
                )
            }
            None => None,
        };
        let state = if self.disabled.contains(&plugin.id) {
            PluginState::Disabled
        } else {
            lock(&self.contributions)
                .load_manifest(&plugin.id, &manifest)
                .map_err(|error| manifest_error(error.to_string()))?;
            if compiled.is_some() {
                PluginState::Loaded
            } else {
                PluginState::ManifestOnly
            }
        };
        self.plugins.insert(
            plugin.id,
            PluginRecord {
                manifest,
                tier,
                granted,
                triggers,
                limits: ResolvedLimits(limits),
                compiled,
                instance: None,
                instance_number: 0,
                state,
                saved: None,
                activation_order: 0,
            },
        );
        Ok(())
    }

    /// The ids of the loaded plugins.
    pub fn plugin_ids(&self) -> Vec<String> {
        self.plugins.keys().cloned().collect()
    }

    /// The state of a plugin.
    pub fn state(&self, plugin: &str) -> Option<PluginState> {
        self.plugins.get(plugin).map(|record| record.state.clone())
    }

    /// The contribution registry, shared with the imports.
    pub fn contributions(&self) -> Arc<Mutex<ContributionRegistry>> {
        Arc::clone(&self.contributions)
    }

    /// Every contribution: the snapshot for a newly attached client.
    pub fn contribution_snapshot(&self) -> Vec<Entry> {
        lock(&self.contributions).snapshot()
    }

    /// The contribution changes since the last call.
    pub fn take_contribution_ops(&mut self) -> Vec<ContributionOp> {
        lock(&self.contributions).take_ops()
    }

    fn build_imports(&self, plugin: &str, record: &PluginRecord, instance: u64) -> ImportSet {
        // A new instance starts without ordinary timers: the book of the previous one is
        // replaced. Its durable timers are kept: the previous book is written to the store
        // first, then the new book is armed from it.
        self.sync_durable_timers();
        let timer_book = TimerBook::default();
        if let Some(store) = lock(&self.durable_timers).as_ref() {
            timers::restore_durable(&timer_book, store.entries_of(plugin));
        }
        lock(&self.timer_books).insert(plugin.to_owned(), Arc::clone(&timer_book));
        // A new instance starts without the replies and deferred calls of the last one.
        lock(&self.services).forget_plugin(plugin);
        self.extension_book.forget_instance(plugin);
        let mut set = ImportSet {
            timers: Some(Box::new(TimersImport::with_book(timer_book))),
            topics: Some(Box::new(PluginTopics::new(
                plugin,
                topics::declared_topics(&record.manifest),
                Arc::clone(&self.topic_outbox),
            ))),
            log: Some(Box::new(SinkLog::new(
                Arc::clone(&self.log),
                plugin,
                self.side,
            ))),
            clock: Some(Box::new(ClockImport)),
            cli: Some(Box::new(CliImport::with_book(
                plugin,
                Arc::clone(&self.cli_book),
            ))),
            settings: Some(Box::new(SettingsImport)),
            extensions: Some(Box::new(PluginExtensions::new(
                plugin,
                &record.granted,
                Arc::clone(&self.contributions),
                self.extension_book.clone(),
            ))),
            tree: Some(Box::new(KernelTree::new(
                Arc::clone(&self.kernel),
                plugin,
                &record.granted,
            ))),
            panes: Some(Box::new(KernelPanes::new(Arc::clone(&self.kernel)))),
            help: Some(Box::new(KernelHelp::new(
                Arc::clone(&self.kernel),
                &record.granted,
            ))),
            services: Some(Box::new(BrokeredServices::new(
                Arc::clone(&self.services),
                plugin,
                &record.granted,
            ))),
            keymap: Some(Box::new(KernelKeymap::new(
                Arc::clone(&self.kernel),
                &record.granted,
            ))),
            kernel_settings: Some(Box::new(KernelSettingsTables::new(
                Arc::clone(&self.kernel),
                &record.granted,
            ))),
            sessions: Some(Box::new(KernelSessions::new(
                Arc::clone(&self.kernel),
                &record.granted,
            ))),
            agent: Some(Box::new(KernelAgent::new(
                Arc::clone(&self.kernel),
                &record.granted,
            ))),
            terminal: Some(Box::new(KernelTerminal::new(
                Arc::clone(&self.kernel),
                &record.granted,
            ))),
            intents_host: Some(Box::new(KernelIntents::new(
                Arc::clone(&self.kernel),
                plugin,
                self.side,
                instance,
                Arc::clone(&self.results),
            ))),
            commands: Some(Box::new(RegistryCommands::new(
                Arc::clone(&self.contributions),
                plugin,
                self.side,
                Arc::clone(&self.commands),
            ))),
            ..ImportSet::default()
        };
        if self.side == Side::Client {
            let mut ui = UiHostImport::default();
            for panel in lock(&self.contributions).entries("panels") {
                if panel.plugin == plugin {
                    ui.declare_panel(&panel.name);
                }
            }
            set.ui_host = Some(Box::new(ui));
        }
        if let Some(provider) = &self.provider {
            provider(
                &InstanceContext {
                    plugin: plugin.to_owned(),
                    side: self.side,
                    tier: record.tier,
                    instance,
                    granted: record.granted.clone(),
                },
                &mut set,
            );
        }
        restrict_to_grants(&mut set, &record.granted);
        set
    }

    /// Activates every enabled plugin that lists the `startup` trigger, in id order.
    pub fn activate_startup(&mut self) -> Vec<RuntimeError> {
        self.trigger("startup")
    }

    /// Activates every loaded plugin that lists this activation trigger, for example
    /// `startup` or `command:ilium.core.layout/preset`. Already active plugins are skipped.
    pub fn trigger(&mut self, trigger: &str) -> Vec<RuntimeError> {
        let waiting: Vec<String> = self
            .plugins
            .iter()
            .filter(|(_, record)| {
                record.state == PluginState::Loaded
                    && record.triggers.iter().any(|listed| listed == trigger)
            })
            .map(|(id, _)| id.clone())
            .collect();
        waiting
            .into_iter()
            .filter_map(|id| self.activate(&id).err())
            .collect()
    }

    /// Instantiates and activates one plugin. Activating an active plugin does nothing.
    pub fn activate(&mut self, plugin: &str) -> Result<(), RuntimeError> {
        let record = self
            .plugins
            .get(plugin)
            .ok_or_else(|| RuntimeError::UnknownPlugin(plugin.to_owned()))?;
        match record.state {
            PluginState::Active | PluginState::ManifestOnly => return Ok(()),
            PluginState::Disabled => return Err(RuntimeError::NotActive(plugin.to_owned())),
            PluginState::Loaded | PluginState::Failed(_) | PluginState::Stopped => {}
        }
        let Some(compiled) = record.compiled.clone() else {
            return Ok(());
        };
        let instance_number = self.next_instance;
        self.next_instance += 1;
        let imports = self.build_imports(plugin, record, instance_number);
        let host_error = |error: HostError| RuntimeError::Host {
            plugin: plugin.to_owned(),
            message: error.to_string(),
        };
        let grants: Vec<Grant> = record
            .granted
            .iter()
            .filter_map(|grant| Grant::parse(grant).ok())
            .collect();
        let linker = self
            .linkers
            .get(&self.engine, self.side, record.tier, &grants)
            .map_err(host_error)?;
        let guest = WasmGuest::instantiate_linked(
            Arc::clone(&self.engine),
            &compiled,
            record.limits.clone(),
            &linker,
            imports,
            GuestIdentity {
                plugin: plugin.to_owned(),
                side: self.side,
                granted: record.granted.clone(),
            },
        );
        let activation = Activation {
            plugin: PluginId(plugin.to_owned()),
            api: API.to_owned(),
            instance: instance_number,
            side: self.side.as_str().to_owned(),
            settings_json: "{}".to_owned(),
            granted: record.granted.clone(),
            restored: record.saved.clone(),
        };
        let order = self.next_activation;
        // The shared borrow of the record ends here; the rest mutates it.
        let record = self
            .plugins
            .get_mut(plugin)
            .ok_or_else(|| RuntimeError::UnknownPlugin(plugin.to_owned()))?;
        let guest = match guest {
            Ok(guest) => guest,
            Err(error) => {
                record.state = PluginState::Failed(error.code().0.to_string());
                return Err(host_error(error));
            }
        };
        let mut instance = PluginInstance::from_guest(Box::new(guest));
        match instance.activate(activation) {
            Ok(()) => {
                record.instance = Some(instance);
                record.state = PluginState::Active;
                record.saved = None;
                record.activation_order = order;
                record.instance_number = instance_number;
                self.next_activation += 1;
                // A restarted instance gets its extensions back (EXT-021).
                self.hooks.clear_plugin(plugin);
                Ok(())
            }
            Err(error) => {
                record.state = PluginState::Failed("activation".to_owned());
                Err(RuntimeError::Activation {
                    plugin: plugin.to_owned(),
                    message: error.message,
                })
            }
        }
    }

    /// How many jobs of the executor thread panicked since start.
    pub fn job_panics(&self) -> u64 {
        self.job_panics
    }

    fn note_job_panic(&mut self) {
        self.job_panics += 1;
    }

    fn fail(&mut self, plugin: &str, trap: &Trap) -> RuntimeError {
        if let Some(record) = self.plugins.get_mut(plugin) {
            record.instance = None;
            record.state = PluginState::Failed(trap.code.0.to_string());
        }
        lock(&self.services).forget_plugin(plugin);
        self.cross_side.forget_plugin(plugin);
        self.extension_book.forget_instance(plugin);
        self.abandon_cli(plugin);
        RuntimeError::Trapped {
            plugin: plugin.to_owned(),
            code: trap.code.0.to_string(),
        }
    }

    fn active_instance(&mut self, plugin: &str) -> Result<&mut PluginInstance, RuntimeError> {
        let record = self
            .plugins
            .get_mut(plugin)
            .ok_or_else(|| RuntimeError::UnknownPlugin(plugin.to_owned()))?;
        match (&record.state, record.instance.as_mut()) {
            (PluginState::Active, Some(instance)) => Ok(instance),
            _ => Err(RuntimeError::NotActive(plugin.to_owned())),
        }
    }

    /// Delivers one batch of events to an active plugin and applies the submissions it
    /// returns through the kernel. The outcomes are queued; `pump` delivers them.
    pub fn deliver(&mut self, plugin: &str, batch: Vec<Event>) -> Result<(), RuntimeError> {
        let submissions = match self.active_instance(plugin)?.on_events(batch) {
            Ok(submissions) => submissions,
            Err(trap) => return Err(self.fail(plugin, &trap)),
        };
        self.apply_submissions(plugin, submissions);
        Ok(())
    }

    /// Delivers one batch to every active plugin; returns the failures.
    pub fn broadcast(&mut self, batch: &[Event]) -> Vec<RuntimeError> {
        let active: Vec<String> = self
            .plugins
            .iter()
            .filter(|(_, record)| record.state == PluginState::Active)
            .map(|(id, _)| id.clone())
            .collect();
        active
            .into_iter()
            .filter_map(|id| self.deliver(&id, batch.to_vec()).err())
            .collect()
    }

    fn apply_submissions(&mut self, plugin: &str, submissions: Vec<Submission>) {
        let origin = Origin::Plugin {
            plugin: PluginId(plugin.to_owned()),
            side: self.side,
            instance: self
                .plugins
                .get(plugin)
                .map_or(0, |record| record.instance_number),
        };
        for submission in submissions {
            let outcome = self.kernel.apply_intent(&origin, submission.intent);
            lock(&self.results).push_back((
                plugin.to_owned(),
                RequestResult {
                    request: RequestRef::Submission(submission.correlation),
                    outcome,
                },
            ));
        }
    }

    /// Invokes a contributed command by its global id `<plugin>/<local id>`: activates the
    /// owner when it waits for the `command:` trigger and delivers `command-invoked`.
    pub fn invoke_command(
        &mut self,
        command: &str,
        arguments_json: &str,
        origin: CommandOrigin,
        context: CommandContext,
    ) -> Result<(), RuntimeError> {
        let (owner, local) = command
            .split_once('/')
            .ok_or_else(|| RuntimeError::UnknownCommand(command.to_owned()))?;
        if lock(&self.contributions)
            .get(owner, "commands", local)
            .is_none()
        {
            return Err(RuntimeError::UnknownCommand(command.to_owned()));
        }
        if self.state(owner) == Some(PluginState::Loaded) {
            self.activate(owner)?;
        }
        self.deliver(
            owner,
            vec![Event::CommandInvoked(CommandInvokedEvent {
                command: local.to_owned(),
                arguments_json: arguments_json.to_owned(),
                origin,
                context,
            })],
        )
    }

    /// The active plugins, in id order.
    fn active_plugins(&self) -> Vec<String> {
        self.plugins
            .iter()
            .filter(|(_, record)| record.state == PluginState::Active)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Delivers events to one plugin in batches the ABI accepts; stops at the first failure.
    fn deliver_chunked(&mut self, plugin: &str, events: &[Event]) -> Option<RuntimeError> {
        events
            .chunks(ilium_plugin_api::abi::MAX_EVENTS_PER_BATCH)
            .find_map(|chunk| self.deliver(plugin, chunk.to_vec()).err())
    }

    /// Delivers `timer-fired` and `durable-timer-fired` for every due timer of an active
    /// plugin. The timers of a plugin that is not active are dropped with its book.
    fn fire_timers(&mut self) -> Vec<RuntimeError> {
        let mut failures = self.activate_for_durable_timers();
        // The books of inactive plugins are dropped below; their durable timers stay stored.
        self.sync_durable_timers();
        let active = self.active_plugins();
        let books: Vec<(String, TimerBook)> = {
            let mut books = lock(&self.timer_books);
            books.retain(|plugin, _| active.contains(plugin));
            books
                .iter()
                .map(|(plugin, book)| (plugin.clone(), Arc::clone(book)))
                .collect()
        };
        let now = Instant::now();
        let now_unix_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| {
                u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
            });
        for (plugin, book) in books {
            let events = timers::take_due(&book, now, now_unix_millis);
            if events.is_empty() {
                continue;
            }
            failures.extend(self.deliver_chunked(&plugin, &events));
        }
        // Timers that fired are gone from their books; the store follows.
        self.sync_durable_timers();
        failures
    }

    /// Names the file that keeps durable timers across restarts and loads what it holds. A file
    /// that cannot be read gives an empty store and one log line. Call it before the first
    /// activation.
    pub fn set_durable_timer_file(&mut self, path: std::path::PathBuf) {
        let (store, warning) = durable_timers::DurableTimerStore::open(path);
        if let Some(warning) = warning {
            self.log.line("", self.side, Level::Warn, &warning);
        }
        *lock(&self.durable_timers) = Some(store);
    }

    /// Copies the durable timers of every held book into the store and writes the file when
    /// something changed. A plugin without a book keeps what is stored.
    fn sync_durable_timers(&self) {
        let mut guard = lock(&self.durable_timers);
        let Some(store) = guard.as_mut() else {
            return;
        };
        for (plugin, book) in lock(&self.timer_books).iter() {
            store.replace(plugin, timers::durable_entries(book));
        }
        if let Err(error) = store.save_if_changed() {
            self.log.line(
                "",
                self.side,
                Level::Warn,
                &format!(
                    "the durable timer file {} was not written: {error}",
                    store.path().display()
                ),
            );
        }
    }

    /// Activates every plugin that is not running and has a durable timer due: the
    /// `durable-timer` activation trigger. The new instance gets its timers back from the store
    /// and the pump that follows delivers the due ones.
    fn activate_for_durable_timers(&mut self) -> Vec<RuntimeError> {
        let now_unix_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| {
                u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
            });
        let due: Vec<String> = match lock(&self.durable_timers).as_ref() {
            Some(store) => store.plugins_due(now_unix_millis),
            None => return Vec::new(),
        };
        let dormant: Vec<String> = due
            .into_iter()
            .filter(|plugin| {
                matches!(
                    self.plugins.get(plugin).map(|record| &record.state),
                    Some(PluginState::Loaded)
                )
            })
            .collect();
        dormant
            .iter()
            .filter_map(|plugin| self.activate(plugin).err())
            .collect()
    }

    /// A pump nobody waits for: each failure goes to the log of the plugin it stopped.
    fn pump_and_log(&mut self) {
        for failure in self.pump() {
            let plugin = match &failure {
                RuntimeError::Trapped { plugin, .. } => plugin.clone(),
                _ => String::new(),
            };
            self.log
                .line(&plugin, self.side, Level::Error, &failure.to_string());
        }
    }

    /// When a held timer next needs a pump; `None` when no plugin holds a timer. The
    /// runtime thread wakes no later than this.
    pub fn next_timer_due(&self) -> Option<Instant> {
        let book_due = lock(&self.timer_books)
            .values()
            .filter_map(timers::next_due)
            .min();
        // A stored durable timer of a plugin that is not running is looked at once a second.
        let stored_due = lock(&self.durable_timers)
            .as_ref()
            .is_some_and(|store| !store.is_empty())
            .then(|| Instant::now() + std::time::Duration::from_secs(1));
        let timers_due = [book_due, stored_due].into_iter().flatten().min();
        [timers_due, self.cross_side.next_deadline()]
            .into_iter()
            .flatten()
            .min()
    }

    /// Delivers `topic-published` for every queued message to the active plugins of this
    /// runtime that hold `topics.subscribe:<topic>` or `topics.subscribe:*`, then offers the
    /// messages to the other side's runtime (`cross_side`).
    fn route_topics(&mut self) -> Vec<RuntimeError> {
        let published: Vec<topics::Published> = lock(&self.topic_outbox).drain(..).collect();
        if published.is_empty() {
            return Vec::new();
        }
        let failures = self.deliver_published(&published);
        for message in &published {
            self.cross_side.export_topic(message);
        }
        failures
    }

    /// Delivers topic messages to the local subscribers. Used for the messages plugins of
    /// this runtime published and for those that arrived from the other side; only the
    /// first kind is exported again.
    fn deliver_published(&mut self, published: &[topics::Published]) -> Vec<RuntimeError> {
        if published.is_empty() {
            return Vec::new();
        }
        let mut by_plugin: BTreeMap<String, Vec<Event>> = BTreeMap::new();
        for (plugin, record) in &self.plugins {
            if record.state != PluginState::Active {
                continue;
            }
            for message in published {
                let subscribed = record.granted.iter().any(|grant| {
                    grant == "topics.subscribe:*"
                        || grant
                            .strip_prefix("topics.subscribe:")
                            .is_some_and(|topic| topic == message.topic)
                });
                if subscribed {
                    by_plugin
                        .entry(plugin.clone())
                        .or_default()
                        .push(Event::TopicPublished(TopicPublishedEvent {
                            topic: message.topic.clone(),
                            from: PluginId(message.from.clone()),
                            payload_json: message.payload_json.clone(),
                        }));
                }
            }
        }
        by_plugin
            .into_iter()
            .filter_map(|(plugin, events)| self.deliver_chunked(&plugin, &events))
            .collect()
    }

    /// Delivers the queued service calls to their providers and moves every call that ended
    /// to the result queue. The provider is the plugin of this runtime whose manifest lists
    /// the service under `contributes.services.provides`. Calls do not cross to the other
    /// side's runtime here.
    fn route_services(&mut self) -> Vec<RuntimeError> {
        let mut failures = Vec::new();
        let pending = lock(&self.services).take_pending();
        for call in pending {
            let provider = self
                .plugins
                .iter()
                .find(|(_, record)| {
                    crate::imports::services::provided_services(&record.manifest)
                        .iter()
                        .any(|name| *name == call.service)
                })
                .map(|(plugin, _)| plugin.clone());
            let Some(provider) = provider else {
                lock(&self.services).fail(
                    &call,
                    PluginError::new(ErrorCode::NotFound, "no plugin provides that service"),
                );
                continue;
            };
            // A provider with no component on this side lives on the other side.
            if self.state(&provider) == Some(PluginState::ManifestOnly) {
                match self.cross_side.send_call(&call, Instant::now()) {
                    Ok(()) => {}
                    Err(error) => lock(&self.services).fail(&call, error),
                }
                continue;
            }
            // A provider that is installed but not running starts with the service trigger.
            if self.state(&provider) == Some(PluginState::Loaded) {
                let local = call.service.rsplit('/').next().unwrap_or(&call.service);
                failures.extend(self.trigger(&format!("service:{}", call.service)));
                failures.extend(self.trigger(&format!("service:{local}")));
            }
            if self.state(&provider) != Some(PluginState::Active) {
                lock(&self.services).fail(
                    &call,
                    PluginError::new(ErrorCode::Busy, "the service provider is not running"),
                );
                continue;
            }
            let export = ExportCall::ServiceHandle(ServiceCall {
                call: call.call,
                service: call.service.clone(),
                method: call.method.clone(),
                payload_json: call.payload_json.clone(),
                caller: PluginId(call.caller.clone()),
            });
            match self.call_export(&provider, export) {
                Ok(ExportReply::ServiceReply(ServiceReply::Done(reply))) => {
                    lock(&self.services).complete(&call, reply);
                }
                Ok(ExportReply::ServiceReply(ServiceReply::Deferred)) => {
                    lock(&self.services).defer(&call, &provider);
                }
                Ok(_) => lock(&self.services).fail(
                    &call,
                    PluginError::new(ErrorCode::Internal, "the provider gave no service reply"),
                ),
                Err(error) => {
                    lock(&self.services).fail(
                        &call,
                        PluginError::new(ErrorCode::Internal, "the service provider failed"),
                    );
                    failures.push(error);
                }
            }
        }
        let finished = lock(&self.services).take_finished();
        let mut results = lock(&self.results);
        for end in finished {
            if self
                .cross_side
                .finish_served(&mut lock(&self.services), &end)
            {
                continue;
            }
            let outcome = end.error.map_or(Outcome::Ok, Outcome::Failed);
            results.push_back((end.caller, command_result(end.request, outcome)));
        }
        failures
    }

    /// Takes what is ready to go to the other side's runtime.
    pub fn take_remote_outbound(&mut self) -> Vec<cross_side::RemoteMessage> {
        self.cross_side.take_outbound()
    }

    /// Queues what arrived from the other side's runtime; the next `pump` handles it.
    pub fn deliver_remote(&mut self, messages: Vec<cross_side::RemoteMessage>) {
        self.cross_side.receive(messages);
    }

    /// Handles queued messages and timeouts of the crossing state: topics go to the local
    /// subscribers, calls to the broker, replies to the calls that waited for them.
    fn pump_cross_side(&mut self) -> Vec<RuntimeError> {
        if !self.cross_side.has_work() {
            return Vec::new();
        }
        let topics = self
            .cross_side
            .process_inbound(&mut lock(&self.services), Instant::now());
        self.deliver_published(&topics)
    }

    /// Fires due timers, routes published topic messages, then delivers the commands plugins
    /// asked for and the queued request results. Returns the failures; a failed plugin is
    /// stopped and the others continue.
    pub fn pump(&mut self) -> Vec<RuntimeError> {
        let mut failures = self.fire_timers();
        for _ in 0..PUMP_ROUNDS {
            // A plugin may publish while it handles a timer, a command or a result, so the
            // outbox is routed in every round.
            failures.extend(self.route_hooks());
            failures.extend(self.pump_cross_side());
            let had_topics = !lock(&self.topic_outbox).is_empty();
            failures.extend(self.route_topics());
            failures.extend(self.route_services());
            let commands: Vec<_> = lock(&self.commands).drain(..).collect();
            let results: VecDeque<_> = std::mem::take(&mut *lock(&self.results));
            if commands.is_empty() && results.is_empty() && !had_topics {
                break;
            }
            for pending in commands {
                let context = CommandContext {
                    pane: None,
                    view: None,
                    buffer: None,
                    line: None,
                    virtual_row: None,
                    menu_item: None,
                };
                // The WIT origin enum has no case for a plugin caller; the palette case is
                // the closest (contract section 11, open item).
                let outcome = match self.invoke_command(
                    &pending.command,
                    &pending.arguments_json,
                    CommandOrigin::Palette,
                    context,
                ) {
                    Ok(()) => Outcome::Ok,
                    Err(error) => {
                        let outcome = Outcome::Failed(PluginError::new(
                            ErrorCode::Internal,
                            "the command could not be delivered",
                        ));
                        failures.push(error);
                        outcome
                    }
                };
                lock(&self.results)
                    .push_back((pending.caller, command_result(pending.request, outcome)));
            }
            let mut by_plugin: BTreeMap<String, Vec<Event>> = BTreeMap::new();
            for (plugin, result) in results {
                by_plugin
                    .entry(plugin)
                    .or_default()
                    .push(Event::RequestCompleted(result));
            }
            for (plugin, events) in by_plugin {
                // Results for a plugin that stopped meanwhile are dropped.
                if self.state(&plugin) != Some(PluginState::Active) {
                    continue;
                }
                for chunk in events.chunks(ilium_plugin_api::abi::MAX_EVENTS_PER_BATCH) {
                    if let Err(error) = self.deliver(&plugin, chunk.to_vec()) {
                        failures.push(error);
                        break;
                    }
                }
            }
        }
        failures
    }

    /// Renders a view of an active plugin.
    pub fn render(
        &mut self,
        plugin: &str,
        view: ViewTarget,
        area: Area,
    ) -> Result<UiTree, RuntimeError> {
        match self.active_instance(plugin)?.render(view, area) {
            Ok(tree) => Ok(tree),
            Err(trap) => Err(self.fail(plugin, &trap)),
        }
    }

    /// Calls another export of an active plugin.
    pub fn call_export(
        &mut self,
        plugin: &str,
        call: ExportCall,
    ) -> Result<ExportReply, RuntimeError> {
        match self.active_instance(plugin)?.call_export(call) {
            Ok(reply) => Ok(reply),
            Err(trap) => Err(self.fail(plugin, &trap)),
        }
    }

    /// Deactivates one plugin and keeps the bytes it saved for its next activation.
    pub fn deactivate(
        &mut self,
        plugin: &str,
        reason: DeactivationReason,
    ) -> Result<(), RuntimeError> {
        let record = self
            .plugins
            .get_mut(plugin)
            .ok_or_else(|| RuntimeError::UnknownPlugin(plugin.to_owned()))?;
        if let Some(mut instance) = record.instance.take() {
            record.saved = instance.deactivate(reason);
        }
        if record.state == PluginState::Active {
            record.state = if record.compiled.is_some() {
                PluginState::Loaded
            } else {
                PluginState::ManifestOnly
            };
        }
        self.abandon_cli(plugin);
        // Timers armed since the last pump must not be lost with the instance.
        self.sync_durable_timers();
        Ok(())
    }

    /// Disables or enables a plugin while the host runs. Disabling deactivates it and
    /// removes every contribution in one `Clear` operation; its stored data is untouched.
    /// Persisting the choice in `installed.json` is the caller's part.
    pub fn set_enabled(&mut self, plugin: &str, enabled: bool) -> Result<(), RuntimeError> {
        if !self.plugins.contains_key(plugin) {
            return Err(RuntimeError::UnknownPlugin(plugin.to_owned()));
        }
        if !enabled {
            if ESSENTIAL_PLUGINS.contains(&plugin) {
                return Err(RuntimeError::Essential(plugin.to_owned()));
            }
            self.deactivate(plugin, DeactivationReason::Disabled)?;
            lock(&self.contributions).clear(plugin);
            self.extension_book.forget_plugin(plugin);
            lock(&self.results).retain(|(owner, _)| owner != plugin);
            self.disabled.insert(plugin.to_owned());
            if let Some(record) = self.plugins.get_mut(plugin) {
                record.state = PluginState::Disabled;
                record.saved = None;
            }
            return Ok(());
        }
        self.disabled.remove(plugin);
        let record = self
            .plugins
            .get_mut(plugin)
            .ok_or_else(|| RuntimeError::UnknownPlugin(plugin.to_owned()))?;
        if record.state != PluginState::Disabled {
            return Ok(());
        }
        lock(&self.contributions)
            .load_manifest(plugin, &record.manifest)
            .map_err(|error| RuntimeError::Manifest {
                plugin: plugin.to_owned(),
                message: error.to_string(),
            })?;
        let starts_now = record.triggers.iter().any(|trigger| trigger == "startup");
        record.state = if record.compiled.is_some() {
            PluginState::Loaded
        } else {
            PluginState::ManifestOnly
        };
        if starts_now {
            self.activate(plugin)?;
        }
        Ok(())
    }

    /// Deactivates every active plugin, last activated first, within a time budget. When
    /// the budget runs out the remaining instances are dropped without `deactivate`.
    pub fn shutdown(&mut self, budget: Duration) -> ShutdownReport {
        let started = Instant::now();
        let mut active: Vec<(u64, String)> = self
            .plugins
            .iter()
            .filter(|(_, record)| record.instance.is_some())
            .map(|(id, record)| (record.activation_order, id.clone()))
            .collect();
        active.sort_unstable_by(|left, right| right.cmp(left));
        let mut report = ShutdownReport::default();
        for (_, id) in active {
            let Some(record) = self.plugins.get_mut(&id) else {
                continue;
            };
            let Some(mut instance) = record.instance.take() else {
                continue;
            };
            if started.elapsed() < budget {
                record.saved = instance.deactivate(DeactivationReason::Shutdown);
                report.deactivated.push(id.clone());
            } else {
                report.abandoned.push(id.clone());
            }
            record.state = PluginState::Stopped;
            self.abandon_cli(&id);
        }
        report
    }
}

/// Removes every import the granted capabilities do not unlock.
fn restrict_to_grants(set: &mut ImportSet, granted: &[String]) {
    for interface in IMPORT_INTERFACES {
        if !interface_unlocked(interface.instance, granted) {
            set.remove(interface.instance);
        }
    }
}

type RuntimeJob = Box<dyn FnOnce(&mut HostRuntime) + Send>;

/// The executor thread of a runtime: the only thread that calls guests.
pub struct RuntimeThread {
    sender: Option<mpsc::Sender<RuntimeJob>>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for RuntimeThread {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeThread")
            .field("running", &self.thread.is_some())
            .finish()
    }
}

impl RuntimeThread {
    /// Moves a runtime onto its own thread.
    pub fn spawn(mut runtime: HostRuntime) -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::channel::<RuntimeJob>();
        let thread = std::thread::Builder::new()
            .name("ilium-plugin-host".to_owned())
            .spawn(move || {
                // The loop ends when every sender is dropped, which `shutdown` does.
                loop {
                    // With a timer held the thread wakes when it is due and pumps, so a
                    // timer fires even when the host posts nothing.
                    let job: RuntimeJob = match runtime.next_timer_due() {
                        None => match receiver.recv() {
                            Ok(job) => job,
                            Err(_) => break,
                        },
                        Some(due) => {
                            let wait = due.saturating_duration_since(Instant::now());
                            match receiver.recv_timeout(wait) {
                                Ok(job) => job,
                                Err(mpsc::RecvTimeoutError::Timeout) => {
                                    Box::new(HostRuntime::pump_and_log)
                                }
                                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                            }
                        }
                    };
                    // A panicking job must not end the only executor thread: the job is
                    // lost, its caller sees a closed reply channel, the loop goes on.
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        job(&mut runtime);
                    }));
                    if outcome.is_err() {
                        runtime.note_job_panic();
                    }
                }
            })?;
        Ok(Self {
            sender: Some(sender),
            thread: Some(thread),
        })
    }

    /// Queues work for the runtime. Returns false when the thread has stopped.
    pub fn post(&self, job: impl FnOnce(&mut HostRuntime) + Send + 'static) -> bool {
        self.sender
            .as_ref()
            .is_some_and(|sender| sender.send(Box::new(job)).is_ok())
    }

    /// Runs work on the runtime and waits for its result. `None` when the thread has
    /// stopped. Never call this from a thread that holds a lock a facade call needs.
    pub fn call<T: Send + 'static>(
        &self,
        job: impl FnOnce(&mut HostRuntime) -> T + Send + 'static,
    ) -> Option<T> {
        let (reply, result) = mpsc::channel();
        let posted = self.post(move |runtime| {
            // The caller may have given up; its reply channel is then closed.
            let _ = reply.send(job(runtime));
        });
        if posted {
            result.recv().ok()
        } else {
            None
        }
    }

    /// Deactivates every plugin within the budget, stops the thread and joins it.
    pub fn shutdown(mut self, budget: Duration) -> ShutdownReport {
        let report = self
            .call(move |runtime| runtime.shutdown(budget))
            .unwrap_or_default();
        self.stop();
        report
    }

    fn stop(&mut self) {
        self.sender = None;
        if let Some(thread) = self.thread.take() {
            // A panic of a posted job already ended the loop; there is nothing to recover.
            let _ = thread.join();
        }
    }
}

impl Drop for RuntimeThread {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::interface_unlocked;

    #[test]
    fn an_interface_no_capability_names_is_always_unlocked() {
        assert!(interface_unlocked("ilium:host/log@0.1.0", &[]));
    }

    #[test]
    fn a_gated_interface_needs_one_of_its_capabilities() {
        assert!(!interface_unlocked("ilium:host/tree@0.1.0", &[]));
        assert!(interface_unlocked(
            "ilium:host/tree@0.1.0",
            &["tree.read".to_owned()]
        ));
        assert!(interface_unlocked(
            "ilium:host/tree@0.1.0",
            &["tree.metadata".to_owned()]
        ));
    }
}
