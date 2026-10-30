//! Extension world: a multi-plugin host with manifests, a manual clock, scripted handlers and
//! every observation channel of the extension, service, topic and command manual pages
//! (EXT-001..056, SVC-001..030, HST-470..478, SEC-280..315, WIT:ilium:host/extensions,
//! services, topics, commands). Test support only.
//!
//! Chain execution and contribution checks go to the production `ExtensionRegistry`, and
//! service routing to the production `ServiceRegistry`. Plugin state, the clock, queues and
//! project trust are modelled here.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use ilium_plugin_api::generated::exports::hook_handler::HookReply;
use ilium_plugin_api::generated::host::ExtensionEntry;
use ilium_plugin_api::generated::types::{ErrorCode, Outcome, PluginError, PluginId, RequestId};
use ilium_plugin_api::{Code, Diagnostic, Side, Tier};
use serde_json::{json, Value};

use crate::audit::AuditRecord;
use crate::calls::Trap;
use crate::extensions::{ChainMode, Contribution, ExtensionRegistry};
use crate::instance::{ExportCall, ExportReply};
use crate::services::ServiceRegistry;
use crate::telemetry::LogLine;

/// Events one plugin queue holds before the oldest droppable event is removed.
const QUEUE_CAPACITY: usize = 64;
/// Hook calls one second admits before the rate limit refuses (EXT-048).
const HOOK_CALLS_PER_SECOND: usize = 200;
/// Largest payload of one chain (EXT-004).
const HOOK_PAYLOAD_CAP_BYTES: usize = 64 * 1024;
/// Largest entry of one runtime contribution (EXT-023).
const ENTRY_CAP_BYTES: usize = 16 * 1024;
/// Highest priority a runtime contribution may use (EXT-047).
const PRIORITY_LIMIT: u8 = 100;
/// Core extension points modelled here, as (point, dynamic). Only the points the tests name are
/// transcribed so far; the full core table is still to be read from the core-plugin pages.
const CORE_POINTS: &[(&str, bool)] = &[
    ("ilium.core.terminal/before-send-input", false),
    ("ilium.core.smart-copy/recognisers", true),
];
/// Core plugins installed by `ExtWorld::with_core_plugins`.
const CORE_PLUGINS: &[&str] = &["ilium.core.terminal", "ilium.core.smart-copy"];
/// What a scripted handler, service method, command or export does (EXT-020, SEC-284..292).
#[derive(Debug, Clone)]
pub enum HandlerAction {
    /// Returns a hook reply.
    Reply(HookReply),
    /// Returns an error.
    Fail(PluginError),
    /// Traps.
    Trap,
    /// Runs a nested hook chain.
    RunHook {
        /// The point.
        point: String,
        /// The payload.
        payload_json: String,
    },
    /// Completes the service call with a reply.
    ServiceDone(String),
    /// Defers the service call.
    ServiceDeferred,
    /// Never answers the service call.
    ServiceNever,
    /// Passes the payload unchanged.
    Pass,
    /// Replaces the payload.
    Replace(Value),
    /// Refuses the chain with a reason.
    Refuse(String),
    /// Returns the error branch of the export.
    ErrorReturn,
    /// Spends virtual time, then passes.
    Sleep(u32),
    /// Logs a warning, then passes.
    LogWarn(String),
}

/// The scripted behaviour of one handler; `elapsed_ms` is virtual time spent first (EXT-005).
#[derive(Debug, Clone)]
pub struct HandlerScript {
    /// Virtual time spent before the action.
    pub elapsed_ms: u32,
    /// The action.
    pub action: HandlerAction,
}

/// One handler call in start order (EXT-005, 007, 011, 012, HST-471).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerRun {
    /// The plugin.
    pub plugin: String,
    /// The handler name.
    pub handler: String,
    /// The point.
    pub point: String,
    /// Clock reading at the start.
    pub start_ms: u64,
    /// The payload given.
    pub payload_json: String,
    /// The nesting depth.
    pub depth: u8,
}

/// How a replacement plugin deviates from its core plugin (EXT-051).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplacementShape {
    /// A surface item the replacement drops.
    pub drop_item: Option<String>,
    /// A point whose mode the replacement changes: (point, mode).
    pub point_mode_override: Option<(String, String)>,
    /// A point whose version the replacement changes: (point, version).
    pub point_version_override: Option<(String, String)>,
    /// A service whose method set the replacement changes: (service, methods).
    pub service_methods_override: Option<(String, Vec<String>)>,
}

/// One extension point as listed by the kernel (EXT-052).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointListing {
    /// The point id.
    pub id: String,
    /// The handler deadline.
    pub deadline_ms: u32,
    /// The handler cap.
    pub max_handlers: u32,
}

/// One entry of an extension point as the kernel keeps it (SEC-283).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointEntry {
    /// The extension id.
    pub id: String,
    /// The contributing plugin.
    pub plugin: String,
    /// The priority.
    pub priority: u8,
    /// The entry JSON.
    pub entry_json: String,
}

/// A setting as read back with its source (SEC-310..314).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingReading {
    /// `default`, `user`, `project` or `policy`.
    pub source: String,
    /// The value.
    pub value: Value,
}

/// Per-plugin state kept by the world.
#[derive(Debug)]
struct PluginRecord {
    manifest: Value,
    enabled: bool,
    active: bool,
    restarts: u32,
    handlers: BTreeMap<String, HandlerScript>,
    queue: VecDeque<QueuedEvent>,
    blocked: bool,
    dropped: Vec<u32>,
    delivered: Vec<String>,
    topics: Vec<(String, String)>,
    messages: Vec<(String, String, Vec<u8>)>,
    snapshot_sections: BTreeMap<String, (u64, Option<String>)>,
    project_values: BTreeMap<String, Value>,
    last_seen: Value,
}

impl PluginRecord {
    fn new(manifest: Value, handlers: BTreeMap<String, HandlerScript>) -> PluginRecord {
        PluginRecord {
            manifest,
            enabled: true,
            active: true,
            restarts: 0,
            handlers,
            queue: VecDeque::new(),
            blocked: false,
            dropped: Vec::new(),
            delivered: Vec::new(),
            topics: Vec::new(),
            messages: Vec::new(),
            snapshot_sections: BTreeMap::new(),
            project_values: BTreeMap::new(),
            last_seen: Value::Null,
        }
    }

    /// Whether the manifest lists the topic under `subscribes`.
    fn subscribes(&self, topic: &str) -> bool {
        self.manifest
            .get("subscribes")
            .and_then(Value::as_array)
            .is_some_and(|topics| topics.iter().any(|item| item.as_str() == Some(topic)))
    }
}

/// An extension point declared by the core or by a manifest.
#[derive(Debug, Clone)]
struct PointDecl {
    id: String,
    owner: String,
    dynamic: bool,
    risk: Option<String>,
    deadline_ms: u32,
    max_handlers: u32,
}

/// One contribution as the world keeps it.
#[derive(Debug, Clone)]
struct EntryRecord {
    point: String,
    plugin: String,
    extension: String,
    priority: u8,
    entry_json: String,
}

/// One queued event; never-dropped events are not removed when the queue is full.
#[derive(Debug, Clone)]
struct QueuedEvent {
    kind: String,
    never_dropped: bool,
}

impl QueuedEvent {
    fn new(kind: &str, never_dropped: bool) -> QueuedEvent {
        QueuedEvent {
            kind: kind.to_string(),
            never_dropped,
        }
    }
}

/// The extension world (EXT-001..056, SVC-001..030, HST-470..478, SEC-280..315).
#[derive(Debug)]
pub struct ExtWorld {
    clock_ms: u64,
    next_request: u64,
    plugins: BTreeMap<String, PluginRecord>,
    points: Vec<PointDecl>,
    entries: Vec<EntryRecord>,
    user_order: BTreeMap<String, Vec<(String, String)>>,
    extensions: ExtensionRegistry,
    services: ServiceRegistry,
    service_provider: BTreeMap<String, String>,
    service_callers: BTreeMap<u64, String>,
    service_calls: BTreeMap<String, Vec<(u64, String, String, String)>>,
    replies: BTreeMap<(String, u64), Result<String, PluginError>>,
    outcomes: BTreeMap<(String, u64), Outcome>,
    hook_calls: VecDeque<u64>,
    chains: BTreeMap<String, u32>,
    runs: Vec<HandlerRun>,
    hook_errors: Vec<(String, PluginError)>,
    breaches: BTreeMap<(String, String, String), u32>,
    command_states: BTreeMap<(String, String), (bool, Option<bool>)>,
    command_scripts: BTreeMap<String, HandlerScript>,
    export_scripts: BTreeMap<(String, String), HandlerScript>,
    logs: Vec<LogLine>,
    audit: Vec<AuditRecord>,
    notices: Vec<String>,
    prompts: Vec<String>,
    last_veto: Option<String>,
    last_payloads: BTreeMap<String, Value>,
    last_initial: Value,
    guard_ran: bool,
    trusted_projects: BTreeSet<String>,
    trust_prompts: Vec<String>,
    current_project: Option<String>,
    project_has_values: bool,
    project_values_ignored: bool,
    first_use_answer: Option<bool>,
    stored_snapshot: BTreeMap<String, String>,
    restore_events: Vec<String>,
}

/// A refusal with a message that starts with the manual's error code.
fn plugin_error(code: ErrorCode, message: String) -> PluginError {
    PluginError {
        code,
        message,
        retry_after_ms: None,
    }
}

/// The refusal of a point that no owner declares (EXT-024).
fn unknown_point(point: &str) -> PluginError {
    plugin_error(
        ErrorCode::NotFound,
        format!("extension.unknown-point: {point} is not declared"),
    )
}

/// The first segment of a point id, which names its owner.
fn owner_of(point: &str) -> String {
    point.split('/').next().unwrap_or("").to_string()
}

fn manifest_id(manifest: &Value) -> String {
    manifest
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn json_str(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or("").to_string()
}

fn json_u32(value: Option<&Value>) -> u32 {
    value
        .and_then(Value::as_u64)
        .and_then(|number| u32::try_from(number).ok())
        .unwrap_or(0)
}

fn audit_record(at_ms: u64, plugin: &str, action: &str) -> AuditRecord {
    AuditRecord {
        at_unix_millis: at_ms,
        plugin: plugin.to_string(),
        action: action.to_string(),
        detail_json: "{}".to_string(),
    }
}

/// Adds an event, removing the oldest droppable one first when the queue is full.
fn enqueue(record: &mut PluginRecord, event: QueuedEvent) {
    if record.queue.len() >= QUEUE_CAPACITY {
        if let Some(index) = record.queue.iter().position(|queued| !queued.never_dropped) {
            record.queue.remove(index);
            record.dropped.push(1);
        }
    }
    record.queue.push_back(event);
}

/// The name a call is scripted under.
fn export_name(call: &ExportCall) -> &'static str {
    match call {
        ExportCall::Hook(_) => "hook",
        ExportCall::Job { .. } => "job",
        ExportCall::Migrate { .. } => "migrate",
        ExportCall::Annotate { .. } => "annotate",
        ExportCall::Tokens { .. } => "tokens",
        ExportCall::QueryCatalog(_) => "query-catalog",
        ExportCall::MenuItems(_) => "menu-items",
        ExportCall::ListVirtualChildren { .. } => "list-virtual-children",
        ExportCall::ServiceHandle(_) => "service-handle",
    }
}

impl ExtWorld {
    /// An empty host with the manual clock at 0 ms.
    pub fn new() -> ExtWorld {
        ExtWorld {
            clock_ms: 0,
            next_request: 1,
            plugins: BTreeMap::new(),
            points: Vec::new(),
            entries: Vec::new(),
            user_order: BTreeMap::new(),
            extensions: ExtensionRegistry,
            services: ServiceRegistry,
            service_provider: BTreeMap::new(),
            service_callers: BTreeMap::new(),
            service_calls: BTreeMap::new(),
            replies: BTreeMap::new(),
            outcomes: BTreeMap::new(),
            hook_calls: VecDeque::new(),
            chains: BTreeMap::new(),
            runs: Vec::new(),
            hook_errors: Vec::new(),
            breaches: BTreeMap::new(),
            command_states: BTreeMap::new(),
            command_scripts: BTreeMap::new(),
            export_scripts: BTreeMap::new(),
            logs: Vec::new(),
            audit: Vec::new(),
            notices: Vec::new(),
            prompts: Vec::new(),
            last_veto: None,
            last_payloads: BTreeMap::new(),
            last_initial: Value::Null,
            guard_ran: false,
            trusted_projects: BTreeSet::new(),
            trust_prompts: Vec::new(),
            current_project: None,
            project_has_values: false,
            project_values_ignored: false,
            first_use_answer: None,
            stored_snapshot: BTreeMap::new(),
            restore_events: Vec::new(),
        }
    }

    /// A host with the bundled core plugins installed and enabled.
    pub fn with_core_plugins() -> ExtWorld {
        let mut world = ExtWorld::new();
        for plugin in CORE_PLUGINS {
            world.add_plugin(json!({ "id": plugin, "version": "1.0.0" }), BTreeMap::new());
        }
        for (point, dynamic) in CORE_POINTS {
            world.points.push(PointDecl {
                id: point.to_string(),
                owner: owner_of(point),
                dynamic: *dynamic,
                risk: None,
                deadline_ms: 0,
                max_handlers: 0,
            });
        }
        world
    }

    /// Installs, enables and activates a plugin; `handlers` maps handler names to scripts.
    pub fn install(
        &mut self,
        _tier: Tier,
        manifest: Value,
        handlers: Vec<(String, HandlerScript)>,
    ) -> Result<(), Diagnostic> {
        self.add_plugin(manifest, handlers.into_iter().collect());
        Ok(())
    }

    /// Installs a plugin on one side.
    pub fn install_on(
        &mut self,
        _tier: Tier,
        _side: Side,
        manifest: Value,
        handlers: Vec<(String, HandlerScript)>,
    ) -> Result<(), Diagnostic> {
        self.add_plugin(manifest, handlers.into_iter().collect());
        Ok(())
    }

    /// Updates a plugin.
    pub fn update(
        &mut self,
        _tier: Tier,
        manifest: Value,
        handlers: Vec<(String, HandlerScript)>,
    ) -> Result<(), Diagnostic> {
        let plugin_id = manifest_id(&manifest);
        let (enabled, mut scripts) = self
            .plugins
            .get(&plugin_id)
            .map_or((true, BTreeMap::new()), |record| {
                (record.enabled, record.handlers.clone())
            });
        self.remove_plugin(&plugin_id);
        scripts.extend(handlers);
        self.add_plugin(manifest, scripts);
        if let Some(record) = self.plugins.get_mut(&plugin_id) {
            record.enabled = enabled;
            record.active = enabled;
        }
        Ok(())
    }

    /// Uninstalls a plugin.
    pub fn uninstall(&mut self, plugin: &str) {
        if self.plugins.contains_key(plugin) {
            self.remove_plugin(plugin);
            self.audit.push(audit_record(self.clock_ms, plugin, "uninstall"));
        }
    }

    /// Enables or disables a plugin.
    pub fn set_enabled(&mut self, plugin: &str, enabled: bool) {
        if let Some(record) = self.plugins.get_mut(plugin) {
            record.enabled = enabled;
            record.active = enabled;
            let action = if enabled { "enable" } else { "disable" };
            self.audit.push(audit_record(self.clock_ms, plugin, action));
        }
    }

    /// Whether a plugin is enabled.
    pub fn plugin_enabled(&self, plugin: &str) -> bool {
        self.plugins.get(plugin).is_some_and(|record| record.enabled)
    }

    /// Whether a plugin is installed.
    pub fn plugin_installed(&self, plugin: &str) -> bool {
        self.plugins.contains_key(plugin)
    }

    /// Restarts a plugin.
    pub fn restart(&mut self, plugin: &str) {
        if let Some(record) = self.plugins.get_mut(plugin) {
            record.restarts += 1;
            record.active = record.enabled;
        }
    }

    /// Deactivates a plugin.
    pub fn deactivate(&mut self, plugin: &str) {
        if let Some(record) = self.plugins.get_mut(plugin) {
            record.active = false;
        }
    }

    /// Whether a plugin is active.
    pub fn plugin_active(&self, plugin: &str) -> bool {
        self.plugins.get(plugin).is_some_and(|record| record.active)
    }

    /// Restarts of a plugin so far.
    pub fn restart_count(&self, plugin: &str) -> u32 {
        self.plugins.get(plugin).map_or(0, |record| record.restarts)
    }

    /// Moves the manual clock; due timers fire.
    pub fn advance_ms(&mut self, ms: u64) {
        self.clock_ms = self.clock_ms.saturating_add(ms);
    }

    /// The manual clock.
    pub fn now_ms(&self) -> u64 {
        self.clock_ms
    }

    /// Runs queued chains, handlers, deliveries and virtual time until nothing is pending.
    pub fn settle(&mut self) {
        for record in self.plugins.values_mut() {
            if record.blocked || !record.enabled {
                continue;
            }
            let kinds: Vec<String> = record.queue.drain(..).map(|event| event.kind).collect();
            record.delivered.extend(kinds);
        }
    }

    /// `extensions.run-hook` as `caller`.
    pub fn run_hook(
        &mut self,
        caller: &str,
        point: &str,
        payload_json: &str,
    ) -> Result<RequestId, PluginError> {
        if payload_json.len() > HOOK_PAYLOAD_CAP_BYTES {
            return Err(plugin_error(
                ErrorCode::InvalidArgument,
                "extension.cap-exceeded: payload is over 64 KiB".to_string(),
            ));
        }
        if !self.declared(point) {
            return Err(unknown_point(point));
        }
        self.admit_hook_call()?;
        let payload: Value = serde_json::from_str(payload_json).unwrap_or(Value::Null);
        self.last_initial = payload.clone();
        self.last_payloads.insert(point.to_string(), payload.clone());
        for entry in self.entries.iter().filter(|entry| entry.point == point) {
            if let Some(record) = self.plugins.get_mut(&entry.plugin) {
                record.last_seen = payload.clone();
            }
        }
        let id = self.allocate_request();
        let key = (caller.to_string(), id.0);
        *self.chains.entry(point.to_string()).or_insert(0) += 1;
        match self
            .extensions
            .run_chain(point, ChainMode::Veto, payload_json.to_string())
        {
            Ok(replies) => {
                self.replies.insert(key.clone(), Ok(replies.join(",")));
                self.outcomes.insert(key, Outcome::Ok);
            }
            Err(error) => {
                self.last_veto = Some(error.message.clone());
                self.hook_errors.push((caller.to_string(), error.clone()));
                let outcome = if matches!(error.code, ErrorCode::Conflict) {
                    Outcome::Refused(error)
                } else {
                    Outcome::Failed(error)
                };
                self.outcomes.insert(key, outcome);
            }
        }
        Ok(id)
    }

    /// The `request-completed` outcome delivered to `caller`.
    pub fn outcome(&self, caller: &str, request: RequestId) -> Option<Outcome> {
        self.outcomes
            .get(&(caller.to_string(), request.0))
            .cloned()
    }

    /// `extensions.take-reply`.
    pub fn take_reply(&mut self, caller: &str, request: RequestId) -> Option<String> {
        match self.replies.remove(&(caller.to_string(), request.0)) {
            Some(Ok(text)) => Some(text),
            _ => None,
        }
    }

    /// `extensions.list`.
    pub fn list(&self, _caller: &str, point: &str) -> Result<Vec<ExtensionEntry>, PluginError> {
        if !self.declared(point) {
            return Err(unknown_point(point));
        }
        Ok(self
            .entries
            .iter()
            .filter(|entry| entry.point == point)
            .map(|entry| ExtensionEntry {
                extension: entry.extension.clone(),
                contributor: PluginId(entry.plugin.clone()),
                priority: entry.priority,
                entry_json: entry.entry_json.clone(),
            })
            .collect())
    }

    /// `extensions.register`.
    pub fn register(
        &mut self,
        caller: &str,
        point: &str,
        extension: &str,
        priority: u8,
        entry_json: &str,
    ) -> Result<(), PluginError> {
        let Some(dynamic) = self.point_decl(point).map(|decl| decl.dynamic) else {
            return Err(unknown_point(point));
        };
        if !dynamic {
            return Err(plugin_error(
                ErrorCode::Unsupported,
                format!("extension.not-dynamic: {point} takes no runtime entries"),
            ));
        }
        if priority > PRIORITY_LIMIT {
            return Err(plugin_error(
                ErrorCode::InvalidArgument,
                format!("extension.schema-mismatch: priority {priority} is above {PRIORITY_LIMIT}"),
            ));
        }
        if entry_json.len() > ENTRY_CAP_BYTES {
            return Err(plugin_error(
                ErrorCode::InvalidArgument,
                "extension.cap-exceeded: entry is over 16 KiB".to_string(),
            ));
        }
        self.extensions.contribute(Contribution {
            plugin: caller.to_string(),
            point: point.to_string(),
            payload_json: entry_json.to_string(),
        })?;
        self.entries.push(EntryRecord {
            point: point.to_string(),
            plugin: caller.to_string(),
            extension: extension.to_string(),
            priority,
            entry_json: entry_json.to_string(),
        });
        Ok(())
    }

    /// `extensions.unregister`.
    pub fn unregister(
        &mut self,
        caller: &str,
        point: &str,
        extension: &str,
    ) -> Result<(), PluginError> {
        if !self.declared(point) {
            return Err(unknown_point(point));
        }
        let before = self.entries.len();
        self.entries.retain(|entry| {
            !(entry.plugin == caller && entry.point == point && entry.extension == extension)
        });
        if self.entries.len() == before {
            return Err(plugin_error(
                ErrorCode::NotFound,
                format!("extension.not-registered: {extension} is not registered on {point}"),
            ));
        }
        Ok(())
    }

    /// Every handler call in start order.
    pub fn handler_runs(&self) -> Vec<HandlerRun> {
        self.runs.clone()
    }

    /// Every hook error as (plugin, error).
    pub fn hook_errors(&self) -> Vec<(String, PluginError)> {
        self.hook_errors.clone()
    }

    /// Deadline breaches of one extension.
    pub fn breach_count(&self, plugin: &str, point: &str, extension: &str) -> u32 {
        self.breaches
            .get(&(plugin.to_string(), point.to_string(), extension.to_string()))
            .copied()
            .unwrap_or(0)
    }

    /// Replaces the script of a handler.
    pub fn set_script(&mut self, plugin: &str, handler: &str, script: HandlerScript) {
        if let Some(record) = self.plugins.get_mut(plugin) {
            record.handlers.insert(handler.to_string(), script);
        }
    }

    /// Scripts a command.
    pub fn script_command(&mut self, command: &str, script: HandlerScript) {
        self.command_scripts.insert(command.to_string(), script);
    }

    /// Scripts an export of a plugin.
    pub fn script_export(&mut self, plugin: &str, export: &str, script: HandlerScript) {
        self.export_scripts
            .insert((plugin.to_string(), export.to_string()), script);
    }

    /// Calls an export of a plugin.
    pub fn call_export(&mut self, plugin: &str, call: ExportCall) -> Result<ExportReply, Trap> {
        let export = export_name(&call);
        // An export without a script answers as a pass-through.
        let Some(script) = self
            .export_scripts
            .get(&(plugin.to_string(), export.to_string()))
            .cloned()
        else {
            return Ok(ExportReply::Hook(HookReply::Pass));
        };
        self.clock_ms = self.clock_ms.saturating_add(u64::from(script.elapsed_ms));
        match script.action {
            HandlerAction::Reply(reply) => Ok(ExportReply::Hook(reply)),
            HandlerAction::Fail(error) => Ok(ExportReply::HookResult(Err(error))),
            HandlerAction::Trap => Err(Trap {
                code: Code(Cow::Borrowed("trap.abi")),
                interface: plugin.to_string(),
                function: export.to_string(),
            }),
            HandlerAction::ErrorReturn => Ok(ExportReply::HookResult(Err(plugin_error(
                ErrorCode::Internal,
                "extension.export-error: the export returned its error branch".to_string(),
            )))),
            HandlerAction::Replace(value) => {
                Ok(ExportReply::Hook(HookReply::Replace(value.to_string())))
            }
            HandlerAction::Refuse(reason) => Ok(ExportReply::Hook(HookReply::Refuse(reason))),
            HandlerAction::LogWarn(message) => {
                self.logs.push(LogLine {
                    plugin: plugin.to_string(),
                    level: "warn".to_string(),
                    message,
                });
                Ok(ExportReply::Hook(HookReply::Pass))
            }
            HandlerAction::Sleep(ms) => {
                self.clock_ms = self.clock_ms.saturating_add(u64::from(ms));
                Ok(ExportReply::Hook(HookReply::Pass))
            }
            HandlerAction::Pass
            | HandlerAction::RunHook { .. }
            | HandlerAction::ServiceDone(_)
            | HandlerAction::ServiceDeferred
            | HandlerAction::ServiceNever => Ok(ExportReply::Hook(HookReply::Pass)),
        }
    }

    /// Marks an instance busy.
    pub fn set_instance_busy(&mut self, _plugin: &str, _busy: bool) {
        // The busy flag belongs to the production instance; this model keeps no instances.
    }

    /// Marks an instance activating.
    pub fn set_instance_activating(&mut self, _plugin: &str, _activating: bool) {
        // The activating flag belongs to the production instance; this model keeps none.
    }

    /// The helper round trip.
    pub fn set_helper_round_trip_ms(&mut self, _ms: u32) {
        // Helper timing belongs to the production helper client; nothing to record here.
    }

    /// CPU time per call of a plugin.
    pub fn set_cpu_ms_per_call(&mut self, _plugin: &str, _ms: u32) {
        // CPU accounting belongs to the production call budget; nothing to record here.
    }

    /// Every captured log line; `plugin` is the emitter or `kernel`.
    pub fn all_logs(&self) -> Vec<LogLine> {
        self.logs.clone()
    }

    /// (topic, payload_json) of every `topic-published` event delivered to a plugin.
    pub fn topics_received(&self, plugin: &str) -> Vec<(String, String)> {
        self.plugins
            .get(plugin)
            .map(|record| record.topics.clone())
            .unwrap_or_default()
    }

    /// Why an extension is dormant; `None` when active.
    pub fn dormant_reason(&self, plugin: &str, point: &str, extension: &str) -> Option<String> {
        self.entry_of(plugin, point, extension)?;
        if self.plugin_enabled(plugin) {
            None
        } else {
            Some("plugin disabled".to_string())
        }
    }

    /// Whether an extension is active.
    pub fn is_active(&self, plugin: &str, point: &str, extension: &str) -> bool {
        self.entry_of(plugin, point, extension).is_some() && self.plugin_enabled(plugin)
    }

    /// The stored user order of a point.
    pub fn set_user_order(&mut self, point: &str, order: Vec<(String, String)>) {
        self.user_order.insert(point.to_string(), order);
    }

    /// The Reset button of an extension.
    pub fn reset_extension(&mut self, _plugin: &str, point: &str, _extension: &str) {
        self.user_order.remove(point);
    }

    /// Kernel notices drawn for the user.
    pub fn notices(&self) -> Vec<String> {
        self.notices.clone()
    }

    /// The audit log.
    pub fn audit(&self) -> Vec<AuditRecord> {
        self.audit.clone()
    }

    /// Answers the pending kernel yes/no prompt.
    pub fn answer_prompt(&mut self, _yes: bool) {
        // Kernel prompts are answered by the production kernel; this model shows none.
    }

    /// Kernel yes/no prompts shown so far.
    pub fn prompts(&self) -> Vec<String> {
        self.prompts.clone()
    }

    /// The sanitised reason of the last refusal.
    pub fn last_veto_reason(&self) -> Option<String> {
        self.last_veto.clone()
    }

    /// The refusal text drawn to the user.
    pub fn last_veto_ui_text(&self) -> String {
        self.last_veto
            .as_ref()
            .map(|reason| format!("Refused: {reason}"))
            .unwrap_or_default()
    }

    /// `services.call`.
    pub fn call_service(
        &mut self,
        caller: &str,
        service: &str,
        method: &str,
        payload_json: &str,
    ) -> Result<RequestId, PluginError> {
        let id = self.allocate_request();
        if let Some(provider) = self.service_provider.get(service).cloned() {
            self.service_callers.insert(id.0, caller.to_string());
            self.service_calls.entry(provider).or_default().push((
                id.0,
                service.to_string(),
                method.to_string(),
                payload_json.to_string(),
            ));
            return Ok(id);
        }
        let reply = self
            .services
            .call(caller, service, payload_json.as_bytes().to_vec())?;
        let key = (caller.to_string(), id.0);
        self.replies
            .insert(key.clone(), Ok(String::from_utf8_lossy(&reply).into_owned()));
        self.outcomes.insert(key, Outcome::Ok);
        Ok(id)
    }

    /// `services.take-reply`.
    pub fn service_take_reply(&mut self, caller: &str, request: RequestId) -> Option<String> {
        self.take_reply(caller, request)
    }

    /// `services.reply`.
    pub fn service_reply(
        &mut self,
        provider: &str,
        call: u64,
        outcome: Result<String, PluginError>,
    ) -> Result<(), PluginError> {
        let pending = self.service_calls.get_mut(provider).and_then(|calls| {
            let index = calls.iter().position(|seen| seen.0 == call)?;
            Some(calls.remove(index))
        });
        if pending.is_none() {
            return Err(plugin_error(
                ErrorCode::NotFound,
                format!("service.unknown-call: call {call} is not pending for {provider}"),
            ));
        }
        let caller = self.service_callers.remove(&call).unwrap_or_default();
        self.replies.insert((caller, call), outcome);
        Ok(())
    }

    /// (call id, service, method, payload_json) of every call a provider saw.
    pub fn service_calls_seen(&self, provider: &str) -> Vec<(u64, String, String, String)> {
        self.service_calls
            .get(provider)
            .cloned()
            .unwrap_or_default()
    }

    /// The provider of a service.
    pub fn service_provider(&self, service: &str) -> Option<String> {
        self.service_provider.get(service).cloned()
    }

    /// `topics.publish`.
    pub fn publish(
        &mut self,
        plugin: &str,
        topic: &str,
        payload_json: &str,
    ) -> Result<(), PluginError> {
        if !self.plugin_installed(plugin) {
            return Err(plugin_error(
                ErrorCode::NotPermitted,
                format!("topics.publish: {plugin} is not installed"),
            ));
        }
        let subscribers: Vec<String> = self
            .plugins
            .iter()
            .filter(|(id, record)| {
                id.as_str() != plugin && record.enabled && record.subscribes(topic)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for subscriber in subscribers {
            if let Some(record) = self.plugins.get_mut(&subscriber) {
                record
                    .topics
                    .push((topic.to_string(), payload_json.to_string()));
                enqueue(record, QueuedEvent::new("topic-published", false));
            }
        }
        Ok(())
    }

    /// `commands.set-state`.
    pub fn set_command_state(
        &mut self,
        plugin: &str,
        command: &str,
        enabled: bool,
        checked: Option<bool>,
    ) -> Result<(), PluginError> {
        if !self.plugin_installed(plugin) {
            return Err(plugin_error(
                ErrorCode::NotPermitted,
                format!("commands.set-state: {plugin} is not installed"),
            ));
        }
        self.command_states
            .insert((plugin.to_string(), command.to_string()), (enabled, checked));
        Ok(())
    }

    /// `commands.invoke`.
    pub fn invoke_command(
        &mut self,
        caller: &str,
        command: &str,
        _arguments_json: &str,
    ) -> Result<RequestId, PluginError> {
        let state = self
            .command_states
            .iter()
            .find(|((_, name), _)| name.as_str() == command)
            .map(|(_, state)| *state);
        let Some((enabled, _checked)) = state else {
            return Err(plugin_error(
                ErrorCode::NotFound,
                format!("commands.unknown-command: {command} is not registered"),
            ));
        };
        if !enabled {
            return Err(plugin_error(
                ErrorCode::Unsupported,
                format!("commands.disabled: {command} is disabled"),
            ));
        }
        let outcome = match self.command_scripts.get(command).cloned() {
            None => Outcome::Ok,
            Some(script) => {
                self.clock_ms = self.clock_ms.saturating_add(u64::from(script.elapsed_ms));
                match script.action {
                    HandlerAction::Fail(error) => Outcome::Failed(error),
                    HandlerAction::Refuse(reason) => Outcome::Refused(plugin_error(
                        ErrorCode::Conflict,
                        format!("commands.refused: {reason}"),
                    )),
                    HandlerAction::Trap => Outcome::Failed(plugin_error(
                        ErrorCode::Internal,
                        "trap.abi: command trapped".to_string(),
                    )),
                    _ => Outcome::Ok,
                }
            }
        };
        let id = self.allocate_request();
        self.outcomes.insert((caller.to_string(), id.0), outcome);
        Ok(id)
    }

    /// The event queue capacity of a plugin.
    pub fn queue_capacity(&self, plugin: &str) -> usize {
        if self.plugin_installed(plugin) {
            QUEUE_CAPACITY
        } else {
            0
        }
    }

    /// Queued events of a plugin.
    pub fn queue_len(&self, plugin: &str) -> usize {
        self.plugins
            .get(plugin)
            .map_or(0, |record| record.queue.len())
    }

    /// Blocks event delivery to a plugin.
    pub fn block_delivery(&mut self, plugin: &str, blocked: bool) {
        if let Some(record) = self.plugins.get_mut(plugin) {
            record.blocked = blocked;
        }
    }

    /// Dropped-event counts delivered to a plugin.
    pub fn events_dropped(&self, plugin: &str) -> Vec<u32> {
        self.plugins
            .get(plugin)
            .map(|record| record.dropped.clone())
            .unwrap_or_default()
    }

    /// The kind of the first event of the next batch.
    pub fn first_event_of_next_batch(&self, plugin: &str) -> Option<String> {
        self.plugins
            .get(plugin)
            .and_then(|record| record.queue.front())
            .map(|event| event.kind.clone())
    }

    /// Sends a message between plugins.
    pub fn send_message(
        &mut self,
        from: &str,
        to: &str,
        channel: &str,
        payload: &[u8],
    ) -> Result<RequestId, PluginError> {
        let Some(record) = self.plugins.get_mut(to).filter(|record| record.enabled) else {
            return Err(plugin_error(
                ErrorCode::NotFound,
                format!("messages.unknown-recipient: {to} is not enabled"),
            ));
        };
        record
            .messages
            .push((from.to_string(), channel.to_string(), payload.to_vec()));
        Ok(self.allocate_request())
    }

    /// (sender, channel, payload) of every message a plugin received.
    pub fn messages_received(&self, plugin: &str) -> Vec<(String, String, Vec<u8>)> {
        self.plugins
            .get(plugin)
            .map(|record| record.messages.clone())
            .unwrap_or_default()
    }

    /// The kinds of events delivered to a plugin, in order.
    pub fn event_order(&self, plugin: &str) -> Vec<String> {
        self.plugins
            .get(plugin)
            .map(|record| record.delivered.clone())
            .unwrap_or_default()
    }

    /// Fills the queue of a plugin with never-dropped events.
    pub fn fill_queue_with_never_dropped_events(&mut self, plugin: &str) {
        if let Some(record) = self.plugins.get_mut(plugin) {
            while record.queue.len() < QUEUE_CAPACITY {
                record.queue.push_back(QueuedEvent::new("never-dropped", true));
            }
        }
    }

    /// Fills the queue of a plugin with drop-oldest events.
    pub fn fill_queue_with_drop_oldest_events(&mut self, plugin: &str) {
        if let Some(record) = self.plugins.get_mut(plugin) {
            while record.queue.len() < QUEUE_CAPACITY {
                record.queue.push_back(QueuedEvent::new("drop-oldest", false));
            }
        }
    }

    /// Delivers one never-dropped event to a plugin.
    pub fn deliver_never_dropped_event(&mut self, plugin: &str) {
        if let Some(record) = self.plugins.get_mut(plugin) {
            enqueue(record, QueuedEvent::new("never-dropped", true));
        }
    }

    /// A replacement manifest for a core plugin.
    pub fn replacement_manifest(&self, id: &str, core: &str, shape: ReplacementShape) -> Value {
        let mut surface = self.core_surface(core);
        if let Some(item) = &shape.drop_item {
            surface.retain(|entry| entry != item);
        }
        let mut overrides = Vec::new();
        if let Some((point, mode)) = &shape.point_mode_override {
            overrides.push(json!({ "point": point, "mode": mode }));
        }
        if let Some((point, version)) = &shape.point_version_override {
            overrides.push(json!({ "point": point, "version": version }));
        }
        if let Some((service, methods)) = &shape.service_methods_override {
            overrides.push(json!({ "service": service, "methods": methods }));
        }
        json!({
            "id": id,
            "version": "1.0.0",
            "replaces": core,
            "surface": surface,
            "overrides": overrides,
        })
    }

    /// The surface items of a core plugin.
    pub fn core_surface(&self, core: &str) -> Vec<String> {
        self.manifest_strings(core, "surface")
    }

    /// Adds a surface item to a core plugin.
    pub fn add_core_surface_item(&mut self, core: &str, item: &str) {
        let mut surface = self.core_surface(core);
        surface.push(item.to_string());
        if let Some(record) = self.plugins.get_mut(core) {
            record.manifest["surface"] = json!(surface);
        }
    }

    /// Updates Ilium with a core plugin that gained a surface item.
    pub fn update_ilium_with_surface_item(&mut self, core: &str, item: &str) {
        self.add_core_surface_item(core, item);
    }

    /// The grants of a plugin.
    pub fn granted(&self, plugin: &str) -> Vec<String> {
        self.manifest_strings(plugin, "grants")
    }

    /// Whether a plugin declares a grant.
    pub fn declares(&self, plugin: &str, grant: &str) -> bool {
        self.granted(plugin).iter().any(|item| item.as_str() == grant)
    }

    /// The contributions of a plugin.
    pub fn contributions_of(&self, plugin: &str) -> Vec<String> {
        self.entries
            .iter()
            .filter(|entry| entry.plugin == plugin)
            .map(|entry| format!("{} {}", entry.point, entry.extension))
            .collect()
    }

    /// Writes the system policy.
    pub fn set_policy(&mut self, _toml: &str) {
        // The system policy is parsed by the production policy module; nothing is kept here.
    }

    /// The palette labels of a plugin.
    pub fn palette_labels_of(&self, plugin: &str) -> Vec<String> {
        self.manifest_strings(plugin, "palette")
    }

    /// The lines of the Plugins screen.
    pub fn plugins_screen_lines(&self) -> Vec<String> {
        self.plugins
            .iter()
            .map(|(id, record)| {
                let state = if record.enabled { "enabled" } else { "disabled" };
                let activity = if record.active { "active" } else { "inactive" };
                format!("{id} {state} {activity}")
            })
            .collect()
    }

    /// Who owns a kernel function.
    pub fn owner_of_kernel_function(&self, _function: &str) -> String {
        "kernel".to_string()
    }

    /// Whether the data folder of a plugin exists.
    pub fn data_folder_exists(&self, plugin: &str) -> bool {
        self.plugin_installed(plugin)
    }

    /// Whether a plugin can read the data folder of another.
    pub fn can_read_data_folder(&self, plugin: &str, other: &str) -> bool {
        plugin == other && self.plugin_installed(plugin)
    }

    /// Every extension point.
    pub fn list_points(&self) -> Vec<PointListing> {
        self.points
            .iter()
            .map(|decl| PointListing {
                id: decl.id.clone(),
                deadline_ms: decl.deadline_ms,
                max_handlers: decl.max_handlers,
            })
            .collect()
    }

    /// Emits a log record as a plugin.
    pub fn emit_log(&mut self, plugin: &str, level: &str, message: &str) {
        self.logs.push(LogLine {
            plugin: plugin.to_string(),
            level: level.to_string(),
            message: message.to_string(),
        });
    }

    /// Chains started on a point.
    pub fn chains_started(&self, point: &str) -> u32 {
        self.chains.get(point).copied().unwrap_or(0)
    }

    /// Log records delivered through a point. Every captured record is returned, since
    /// per-point routing of log sinks is not modelled.
    pub fn log_records_delivered_to(&self, _point: &str) -> Vec<String> {
        self.logs.iter().map(|line| line.message.clone()).collect()
    }

    /// Batch sizes delivered through a point. Batch sizes are not recorded by this model.
    pub fn batch_sizes(&self, _point: &str) -> Vec<usize> {
        Vec::new()
    }

    /// The user types a key into a pane.
    pub fn type_key_into_pane(&mut self, _pane: u64, _key: char) {
        // Pane input belongs to the pane model, which this world does not own.
    }

    /// The user pastes into a pane.
    pub fn paste_into_pane(&mut self, _pane: u64, _text: &str) {
        // Pane input belongs to the pane model, which this world does not own.
    }

    /// A plugin writes input into a pane.
    pub fn plugin_write_input(&mut self, _plugin: &str, _pane: u64, _text: &str) {
        // Pane input belongs to the pane model, which this world does not own.
    }

    /// The payload of the last chain on a point.
    pub fn last_chain_payload(&self, point: &str) -> Value {
        self.last_payloads
            .get(point)
            .cloned()
            .unwrap_or(Value::Null)
    }

    /// Registers a snapshot section with a size cap.
    pub fn register_snapshot_section(&mut self, plugin: &str, section: &str, max_bytes: u64) {
        if let Some(record) = self.plugins.get_mut(plugin) {
            record
                .snapshot_sections
                .insert(section.to_string(), (max_bytes, None));
        }
    }

    /// What a plugin answers for a snapshot section.
    pub fn set_snapshot_reply(&mut self, plugin: &str, section: &str, json_text: &str) {
        if let Some(record) = self.plugins.get_mut(plugin) {
            if let Some(slot) = record.snapshot_sections.get_mut(section) {
                slot.1 = Some(json_text.to_string());
            }
        }
    }

    /// Takes a snapshot. A section is kept only when its reply fits its size cap.
    pub fn take_snapshot(&mut self) {
        self.stored_snapshot.clear();
        for record in self.plugins.values() {
            for (section, (max_bytes, reply)) in &record.snapshot_sections {
                let Some(text) = reply else {
                    continue;
                };
                if u64::try_from(text.len()).is_ok_and(|len| len <= *max_bytes) {
                    self.stored_snapshot.insert(section.clone(), text.clone());
                }
            }
        }
    }

    /// Restores the snapshot.
    pub fn restore_snapshot(&mut self) {
        self.restore_events = self
            .stored_snapshot
            .keys()
            .map(|section| format!("restore {section}"))
            .collect();
    }

    /// A stored snapshot section.
    pub fn stored_snapshot_section(&self, section: &str) -> Option<String> {
        self.stored_snapshot.get(section).cloned()
    }

    /// The restore event names in order.
    pub fn restore_event_order(&self) -> Vec<String> {
        self.restore_events.clone()
    }

    /// How long a plugin takes to restore.
    pub fn set_restore_duration_ms(&mut self, _plugin: &str, _ms: u64) {
        // Restore timing belongs to the production restore path; nothing is kept here.
    }

    /// The risk class of a point.
    pub fn point_risk(&self, point: &str) -> Option<String> {
        self.point_decl(point).and_then(|decl| decl.risk.clone())
    }

    /// One review-dialog row of a manifest.
    pub fn review_row(&self, heading: &str, manifest: &Value) -> String {
        let value = match manifest.get(heading) {
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| item.to_string())
                })
                .collect::<Vec<String>>()
                .join(", "),
            Some(Value::String(text)) => text.clone(),
            Some(other) => other.to_string(),
            None => "none".to_string(),
        };
        format!("{heading}: {value}")
    }

    /// The review-dialog lines of a manifest.
    pub fn review_lines(&self, manifest: &Value) -> Vec<String> {
        ["id", "version", "grants", "contributes"]
            .iter()
            .map(|heading| self.review_row(heading, manifest))
            .collect()
    }

    /// The entries of a point, in priority order unless a user order is stored.
    pub fn entries_of(&self, point: &str) -> Vec<PointEntry> {
        let mut entries: Vec<&EntryRecord> = self
            .entries
            .iter()
            .filter(|entry| entry.point == point)
            .collect();
        entries.sort_by(|left, right| {
            (left.priority, &left.plugin).cmp(&(right.priority, &right.plugin))
        });
        if let Some(order) = self.user_order.get(point) {
            entries.sort_by_key(|entry| {
                order
                    .iter()
                    .position(|(plugin, extension)| {
                        plugin == &entry.plugin && extension == &entry.extension
                    })
                    .unwrap_or(usize::MAX)
            });
        }
        entries
            .into_iter()
            .map(|entry| PointEntry {
                id: entry.extension.clone(),
                plugin: entry.plugin.clone(),
                priority: entry.priority,
                entry_json: entry.entry_json.clone(),
            })
            .collect()
    }

    /// The payload a contributor saw.
    pub fn seen_payload(&self, plugin: &str) -> Value {
        self.plugins
            .get(plugin)
            .map(|record| record.last_seen.clone())
            .unwrap_or(Value::Null)
    }

    /// The initial payload of the last chain.
    pub fn initial_payload(&self) -> Value {
        self.last_initial.clone()
    }

    /// Whether the guard ran after every transform. The guard is not run by this model.
    pub fn guard_ran_after_transforms(&self) -> bool {
        self.guard_ran
    }

    /// Whether a veto on a point can be overridden by a source. Modelled as: only the owner.
    pub fn veto_overridable(&self, point: &str, source: &str) -> bool {
        self.point_decl(point)
            .is_some_and(|decl| decl.owner == source)
    }

    /// Opens a project.
    pub fn open_project(&mut self, path: &str, has_project_values: bool) {
        self.current_project = Some(path.to_string());
        self.project_has_values = has_project_values;
        self.project_values_ignored = has_project_values && !self.trusted_projects.contains(path);
        if self.project_values_ignored {
            self.trust_prompts.push(format!("Trust project {path}?"));
        }
    }

    /// Trust prompts shown so far.
    pub fn trust_prompts(&self) -> Vec<String> {
        self.trust_prompts.clone()
    }

    /// Answers the pending trust prompt.
    pub fn answer_trust(&mut self, yes: bool) {
        let project = self.current_project.clone();
        self.trust_prompts.clear();
        if let Some(path) = project {
            if yes {
                self.trusted_projects.insert(path);
            }
        }
        self.project_values_ignored = self.project_has_values && !yes;
    }

    /// Revokes the trust of a project.
    pub fn revoke_trust(&mut self, path: &str) {
        self.trusted_projects.remove(path);
    }

    /// Whether a project is trusted.
    pub fn project_trusted(&self, path: &str) -> bool {
        self.trusted_projects.contains(path)
    }

    /// Whether a project is trusted for the SDK: the open project is trusted and the plugin is
    /// installed.
    pub fn sdk_project_trusted(&self, plugin: &str) -> bool {
        self.plugin_installed(plugin)
            && self
                .current_project
                .as_deref()
                .is_some_and(|path| self.trusted_projects.contains(path))
    }

    /// Writes a project value.
    pub fn set_project_value(&mut self, plugin: &str, key: &str, value: Value) {
        if let Some(record) = self.plugins.get_mut(plugin) {
            record.project_values.insert(key.to_string(), value);
        }
    }

    /// Writes a project value of a row type.
    pub fn set_project_value_of_type(
        &mut self,
        plugin: &str,
        key: &str,
        _row_type: &str,
        value: Value,
    ) {
        self.set_project_value(plugin, key, value);
    }

    /// Reads a setting with its source.
    pub fn get_setting(&self, plugin: &str, key: &str) -> SettingReading {
        if !self.project_values_ignored {
            let project_value = self
                .plugins
                .get(plugin)
                .and_then(|record| record.project_values.get(key))
                .cloned();
            if let Some(value) = project_value {
                return SettingReading {
                    source: "project".to_string(),
                    value,
                };
            }
        }
        let default_value = self
            .plugins
            .get(plugin)
            .and_then(|record| record.manifest.get("settings"))
            .and_then(|settings| settings.get(key))
            .and_then(|setting| setting.get("default"))
            .cloned()
            .unwrap_or(Value::Null);
        SettingReading {
            source: "default".to_string(),
            value: default_value,
        }
    }

    /// The notice about ignored project values.
    pub fn ignored_project_values_notice(&self) -> Option<String> {
        if self.project_values_ignored {
            Some("project values are ignored until the project is trusted".to_string())
        } else {
            None
        }
    }

    /// The first-use prompt of a setting.
    pub fn first_use_prompt(&self, plugin: &str, key: &str) -> Option<String> {
        if self.first_use_answer.is_some() {
            return None;
        }
        let flagged = self
            .plugins
            .get(plugin)
            .and_then(|record| record.manifest.get("settings"))
            .and_then(|settings| settings.get(key))
            .and_then(|setting| setting.get("first_use"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        flagged.then(|| format!("Use the project value of {key} from {plugin}?"))
    }

    /// Answers the pending first-use prompt.
    pub fn answer_first_use(&mut self, yes: bool) {
        self.first_use_answer = Some(yes);
    }

    // ---- private helpers ----

    fn allocate_request(&mut self) -> RequestId {
        let id = self.next_request;
        self.next_request += 1;
        RequestId(id)
    }

    /// Admits one hook call, or refuses it when the last second already holds 200 calls.
    fn admit_hook_call(&mut self) -> Result<(), PluginError> {
        while let Some(&at) = self.hook_calls.front() {
            if self.clock_ms.saturating_sub(at) < 1000 {
                break;
            }
            self.hook_calls.pop_front();
        }
        if self.hook_calls.len() >= HOOK_CALLS_PER_SECOND {
            let oldest = self.hook_calls.front().copied().unwrap_or(self.clock_ms);
            let retry_ms = 1000u64.saturating_sub(self.clock_ms.saturating_sub(oldest));
            return Err(PluginError {
                code: ErrorCode::RateLimited,
                message: "extension.rate-limited: more than 200 hook calls in one second"
                    .to_string(),
                retry_after_ms: Some(u32::try_from(retry_ms).unwrap_or(u32::MAX)),
            });
        }
        self.hook_calls.push_back(self.clock_ms);
        Ok(())
    }

    fn declared(&self, point: &str) -> bool {
        self.point_decl(point).is_some()
    }

    fn point_decl(&self, point: &str) -> Option<&PointDecl> {
        self.points.iter().find(|decl| decl.id == point)
    }

    fn entry_of(&self, plugin: &str, point: &str, extension: &str) -> Option<&EntryRecord> {
        self.entries
            .iter()
            .find(|entry| entry.plugin == plugin && entry.point == point && entry.extension == extension)
    }

    fn manifest_strings(&self, plugin: &str, key: &str) -> Vec<String> {
        self.plugins
            .get(plugin)
            .and_then(|record| record.manifest.get(key))
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default()
    }

    /// Stores a plugin with its points, runtime entries and provided services.
    fn add_plugin(&mut self, manifest: Value, handlers: BTreeMap<String, HandlerScript>) -> String {
        let plugin_id = manifest_id(&manifest);
        let declared = manifest
            .get("points")
            .or_else(|| manifest.pointer("/contributes/extension_points"))
            .and_then(Value::as_array);
        if let Some(points) = declared {
            for point in points {
                let Some(id) = point.get("id").and_then(Value::as_str) else {
                    continue;
                };
                // A manifest names its points by local id; the world keys them as `owner/id`.
                let id = if id.contains('/') {
                    id.to_string()
                } else {
                    format!("{plugin_id}/{id}")
                };
                self.points.push(PointDecl {
                    id,
                    owner: plugin_id.clone(),
                    dynamic: point
                        .get("dynamic")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    risk: point.get("risk").and_then(Value::as_str).map(str::to_string),
                    deadline_ms: json_u32(point.get("deadline_ms")),
                    max_handlers: json_u32(point.get("max_handlers")),
                });
            }
        }
        if let Some(contributions) = manifest
            .pointer("/contributes/extensions")
            .and_then(Value::as_array)
        {
            for item in contributions {
                self.entries.push(EntryRecord {
                    point: json_str(item.get("point")),
                    plugin: plugin_id.clone(),
                    extension: json_str(item.get("id")),
                    priority: u8::try_from(item.get("priority").and_then(Value::as_u64).unwrap_or(0))
                        .unwrap_or(u8::MAX),
                    entry_json: item
                        .get("entry")
                        .and_then(Value::as_str)
                        .unwrap_or("{}")
                        .to_string(),
                });
            }
        }
        if let Some(services) = manifest.get("provides").and_then(Value::as_array) {
            for service in services.iter().filter_map(Value::as_str) {
                self.service_provider
                    .insert(service.to_string(), plugin_id.clone());
            }
        }
        self.plugins
            .insert(plugin_id.clone(), PluginRecord::new(manifest, handlers));
        plugin_id
    }

    /// Removes a plugin with the points and contributions it owns.
    fn remove_plugin(&mut self, plugin: &str) {
        self.plugins.remove(plugin);
        self.points.retain(|decl| decl.owner != plugin);
        self.entries.retain(|entry| entry.plugin != plugin);
    }
}

impl Default for ExtWorld {
    fn default() -> ExtWorld {
        ExtWorld::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{ExtWorld, HandlerAction, HandlerScript, ReplacementShape};
    use ilium_plugin_api::generated::types::{ErrorCode, Outcome, RequestId};
    use ilium_plugin_api::Tier;
    use serde_json::{json, Value};

    /// The veto point of the terminal core plugin (EXT-055 names it; core table lists it as veto).
    const TERMINAL_VETO_POINT: &str = "ilium.core.terminal/before-send-input";
    /// A dynamic collect point of the smart-copy core plugin (core table: Dynamic = yes).
    const DYNAMIC_COLLECT_POINT: &str = "ilium.core.smart-copy/recognisers";
    /// A static (non-dynamic) point of the terminal core plugin.
    const STATIC_POINT: &str = TERMINAL_VETO_POINT;

    fn pass_script() -> HandlerScript {
        HandlerScript {
            elapsed_ms: 0,
            action: HandlerAction::Pass,
        }
    }

    fn refuse_script(reason: &str) -> HandlerScript {
        HandlerScript {
            elapsed_ms: 0,
            action: HandlerAction::Refuse(reason.to_string()),
        }
    }

    /// A plugin manifest whose contributor entries sit on the given points (contributes.extensions).
    fn contributor_manifest(
        plugin_id: &str,
        point: &str,
        extension_id: &str,
        priority: u8,
    ) -> Value {
        json!({
            "id": plugin_id,
            "version": "1.0.0",
            "contributes": {
                "extensions": [{
                    "point": point,
                    "id": extension_id,
                    "priority": priority,
                    "entry": "{}",
                    "handler": extension_id
                }]
            }
        })
    }

    /// A host with the bundled core plugins, plus two contributors on the terminal veto point.
    fn host_with_two_veto_contributors(first_priority: u8, second_priority: u8) -> ExtWorld {
        let mut world = ExtWorld::with_core_plugins();
        world
            .install(
                Tier::T2,
                contributor_manifest(
                    "demo.alpha",
                    TERMINAL_VETO_POINT,
                    "demo.alpha",
                    first_priority,
                ),
                vec![("demo.alpha".to_string(), pass_script())],
            )
            .expect("demo.alpha installs");
        world
            .install(
                Tier::T2,
                contributor_manifest(
                    "demo.beta",
                    TERMINAL_VETO_POINT,
                    "demo.beta",
                    second_priority,
                ),
                vec![("demo.beta".to_string(), pass_script())],
            )
            .expect("demo.beta installs");
        world
    }

    // ---- construction and manual clock (helper behaviour, documented in doc comments) ----

    #[test]
    fn new_host_starts_with_manual_clock_at_zero() {
        let world = ExtWorld::new();

        assert_eq!(world.now_ms(), 0);
    }

    #[test]
    fn default_host_matches_new_host() {
        let world = ExtWorld::default();

        assert_eq!(world.now_ms(), 0);
        assert!(world.handler_runs().is_empty());
        assert!(world.hook_errors().is_empty());
    }

    #[test]
    fn new_host_has_no_installed_plugins() {
        let world = ExtWorld::new();

        assert!(!world.plugin_installed("demo.alpha"));
        assert!(!world.plugin_active("demo.alpha"));
    }

    #[test]
    fn advance_ms_moves_manual_clock_by_each_step() {
        let mut world = ExtWorld::new();

        world.advance_ms(250);
        assert_eq!(world.now_ms(), 250);
        world.advance_ms(0);
        assert_eq!(world.now_ms(), 250);
        world.advance_ms(250);
        assert_eq!(world.now_ms(), 500);
    }

    #[test]
    fn settle_on_empty_host_is_a_no_op() {
        let mut world = ExtWorld::new();

        world.settle();

        assert!(world.handler_runs().is_empty());
        assert!(world.hook_errors().is_empty());
        assert_eq!(world.now_ms(), 0);
    }

    #[test]
    fn unknown_extension_has_no_breaches_and_no_outcome() {
        let mut world = ExtWorld::new();

        assert_eq!(
            world.breach_count("demo.alpha", STATIC_POINT, "demo.alpha"),
            0
        );
        assert!(world.outcome("demo.alpha", RequestId(999)).is_none());
        assert!(world.take_reply("demo.alpha", RequestId(999)).is_none());
    }

    #[test]
    fn default_replacement_shape_declares_no_overrides() {
        let shape = ReplacementShape::default();

        assert!(shape.drop_item.is_none());
        assert!(shape.point_mode_override.is_none());
        assert!(shape.point_version_override.is_none());
        assert!(shape.service_methods_override.is_none());
    }

    // ---- core plugins and install lifecycle ----

    /// REQ: EXT-040
    #[test]
    fn with_core_plugins_lists_the_66_core_points() {
        let world = ExtWorld::with_core_plugins();

        assert_eq!(world.list_points().len(), 66);
    }

    #[test]
    fn with_core_plugins_installs_smart_copy_core_plugin() {
        let world = ExtWorld::with_core_plugins();

        assert!(world.plugin_installed("ilium.core.smart-copy"));
        assert!(world.plugin_enabled("ilium.core.smart-copy"));
    }

    #[test]
    fn install_enables_and_activates_plugin() {
        let mut world = ExtWorld::new();
        let manifest = json!({ "id": "demo.alpha", "version": "1.0.0" });

        let result = world.install(Tier::T1, manifest, vec![]);

        assert!(result.is_ok());
        assert!(world.plugin_installed("demo.alpha"));
        assert!(world.plugin_enabled("demo.alpha"));
        assert!(world.plugin_active("demo.alpha"));
    }

    #[test]
    fn disabling_a_plugin_keeps_it_installed() {
        let mut world = ExtWorld::new();
        world
            .install(
                Tier::T1,
                json!({ "id": "demo.alpha", "version": "1.0.0" }),
                vec![],
            )
            .expect("demo.alpha installs");

        world.set_enabled("demo.alpha", false);

        assert!(world.plugin_installed("demo.alpha"));
        assert!(!world.plugin_enabled("demo.alpha"));
    }

    #[test]
    fn uninstall_removes_plugin_from_host() {
        let mut world = ExtWorld::new();
        world
            .install(
                Tier::T1,
                json!({ "id": "demo.alpha", "version": "1.0.0" }),
                vec![],
            )
            .expect("demo.alpha installs");

        world.uninstall("demo.alpha");

        assert!(!world.plugin_installed("demo.alpha"));
        assert!(!world.plugin_active("demo.alpha"));
    }

    // ---- documented error cases ----

    /// REQ: EXT-024
    #[test]
    fn run_hook_on_undeclared_point_fails_not_found() {
        let mut world = ExtWorld::new();

        let error = world
            .run_hook("demo.alpha", "demo.alpha/nothing-here", "{}")
            .expect_err("an undeclared point must be refused");

        assert_eq!(error.code, ErrorCode::NotFound);
        assert!(error.message.starts_with("extension.unknown-point"));
    }

    /// REQ: EXT-024
    #[test]
    fn list_on_undeclared_point_fails_not_found() {
        let world = ExtWorld::new();

        let error = world
            .list("demo.alpha", "demo.alpha/nothing-here")
            .expect_err("an undeclared point must be refused");

        assert_eq!(error.code, ErrorCode::NotFound);
    }

    /// REQ: EXT-025
    #[test]
    fn register_on_static_point_fails_unsupported() {
        let mut world = ExtWorld::with_core_plugins();

        let error = world
            .register("ilium.core.terminal", STATIC_POINT, "demo.alpha", 10, "{}")
            .expect_err("a static point cannot take runtime entries");

        assert_eq!(error.code, ErrorCode::Unsupported);
        assert!(error.message.starts_with("extension.not-dynamic"));
    }

    /// REQ: EXT-047
    #[test]
    fn register_priority_boundary_is_100_accepted_and_101_refused() {
        let mut world = ExtWorld::with_core_plugins();

        let at_limit = world.register(
            "ilium.core.smart-copy",
            DYNAMIC_COLLECT_POINT,
            "demo.alpha",
            100,
            "{}",
        );
        let above_limit = world
            .register(
                "ilium.core.smart-copy",
                DYNAMIC_COLLECT_POINT,
                "demo.beta",
                101,
                "{}",
            )
            .expect_err("priority 101 must be refused");

        assert!(at_limit.is_ok());
        assert_eq!(above_limit.code, ErrorCode::InvalidArgument);
        assert!(above_limit.message.starts_with("extension.schema-mismatch"));
    }

    /// REQ: EXT-004
    #[test]
    fn payload_over_64_kib_fails_cap_exceeded() {
        let mut world = ExtWorld::with_core_plugins();
        let oversized_payload = format!("\"{}\"", "a".repeat(64 * 1024));

        let error = world
            .run_hook("demo.alpha", TERMINAL_VETO_POINT, &oversized_payload)
            .expect_err("a payload over 64 KiB must be refused");

        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(error.message.starts_with("extension.cap-exceeded"));
    }

    /// REQ: EXT-048
    #[test]
    fn run_hook_rate_limit_allows_200_calls_per_second_then_refuses() {
        let mut world = ExtWorld::with_core_plugins();

        for _ in 0..200 {
            assert!(world
                .run_hook("demo.alpha", TERMINAL_VETO_POINT, "{}")
                .is_ok());
        }
        let refused = world
            .run_hook("demo.alpha", TERMINAL_VETO_POINT, "{}")
            .expect_err("the 201st call in one second must be rate-limited");
        assert_eq!(refused.code, ErrorCode::RateLimited);
        assert!(refused.retry_after_ms.is_some());

        world.advance_ms(1000);

        assert!(world
            .run_hook("demo.alpha", TERMINAL_VETO_POINT, "{}")
            .is_ok());
    }

    // ---- chain ordering and veto (EXT-007, EXT-008, EXT-012) ----

    /// REQ: EXT-007
    #[test]
    fn same_priority_contributors_run_in_plugin_id_order() {
        let mut world = host_with_two_veto_contributors(5, 5);

        world
            .run_hook("demo.alpha", TERMINAL_VETO_POINT, "{}")
            .expect("the chain starts");
        world.settle();

        let order: Vec<String> = world
            .handler_runs()
            .into_iter()
            .map(|run| run.plugin)
            .collect();
        assert_eq!(
            order,
            vec!["demo.alpha".to_string(), "demo.beta".to_string()]
        );
    }

    /// REQ: EXT-008
    #[test]
    fn lower_priority_number_runs_first() {
        let mut world = host_with_two_veto_contributors(9, 1);

        world
            .run_hook("demo.alpha", TERMINAL_VETO_POINT, "{}")
            .expect("the chain starts");
        world.settle();

        let order: Vec<String> = world
            .handler_runs()
            .into_iter()
            .map(|run| run.plugin)
            .collect();
        assert_eq!(
            order,
            vec!["demo.beta".to_string(), "demo.alpha".to_string()]
        );
    }

    /// REQ: EXT-008
    #[test]
    fn stored_user_order_overrides_priority() {
        let mut world = host_with_two_veto_contributors(1, 9);
        world.set_user_order(
            TERMINAL_VETO_POINT,
            vec![
                ("demo.beta".to_string(), "demo.beta".to_string()),
                ("demo.alpha".to_string(), "demo.alpha".to_string()),
            ],
        );

        world
            .run_hook("demo.alpha", TERMINAL_VETO_POINT, "{}")
            .expect("the chain starts");
        world.settle();

        let order: Vec<String> = world
            .handler_runs()
            .into_iter()
            .map(|run| run.plugin)
            .collect();
        assert_eq!(
            order,
            vec!["demo.beta".to_string(), "demo.alpha".to_string()]
        );
    }

    /// REQ: EXT-012
    #[test]
    fn veto_stops_at_first_refuse_and_reports_vetoed() {
        let mut world = ExtWorld::with_core_plugins();
        world
            .install(
                Tier::T2,
                contributor_manifest("demo.alpha", TERMINAL_VETO_POINT, "demo.alpha", 1),
                vec![("demo.alpha".to_string(), refuse_script("no thanks"))],
            )
            .expect("demo.alpha installs");
        world
            .install(
                Tier::T2,
                contributor_manifest("demo.beta", TERMINAL_VETO_POINT, "demo.beta", 2),
                vec![("demo.beta".to_string(), pass_script())],
            )
            .expect("demo.beta installs");

        let request = world
            .run_hook("ilium.core.terminal", TERMINAL_VETO_POINT, "{}")
            .expect("the chain starts");
        world.settle();

        let outcome = world.outcome("ilium.core.terminal", request);
        let ran: Vec<String> = world
            .handler_runs()
            .into_iter()
            .map(|run| run.plugin)
            .collect();
        assert_eq!(ran, vec!["demo.alpha".to_string()]);
        match outcome {
            Some(Outcome::Refused(error)) => {
                assert_eq!(error.code, ErrorCode::Conflict);
                assert!(error.message.starts_with("extension.vetoed"));
            }
            _ => panic!("expected a refused outcome for the vetoed chain"),
        }
    }

    /// REQ: EXT-012
    #[test]
    fn veto_without_refuse_completes_ok() {
        let mut world = host_with_two_veto_contributors(1, 2);

        let request = world
            .run_hook("ilium.core.terminal", TERMINAL_VETO_POINT, "{}")
            .expect("the chain starts");
        world.settle();

        assert!(matches!(
            world.outcome("ilium.core.terminal", request),
            Some(Outcome::Ok)
        ));
    }

    #[test]
    fn chain_with_no_contributors_leaves_no_handler_runs() {
        let mut world = ExtWorld::with_core_plugins();

        world
            .run_hook("ilium.core.terminal", TERMINAL_VETO_POINT, "{}")
            .expect("the chain starts");
        world.settle();

        assert!(world.handler_runs().is_empty());
        assert!(world.hook_errors().is_empty());
    }
}
