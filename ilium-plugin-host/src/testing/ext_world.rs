//! Extension world: a multi-plugin host with manifests, a manual clock, scripted handlers and
//! every observation channel of the extension, service, topic and command manual pages
//! (EXT-001..056, SVC-001..030, HST-470..478, SEC-280..315, WIT:ilium:host/extensions,
//! services, topics, commands). Test support only.

use ilium_plugin_api::generated::exports::hook_handler::HookReply;
use ilium_plugin_api::generated::host::ExtensionEntry;
use ilium_plugin_api::generated::types::{Outcome, PluginError, RequestId};
use ilium_plugin_api::{Diagnostic, Side, Tier};
use serde_json::Value;

use crate::audit::AuditRecord;
use crate::calls::Trap;
use crate::instance::{ExportCall, ExportReply};
use crate::telemetry::LogLine;

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

/// The extension world (EXT-001..056, SVC-001..030, HST-470..478, SEC-280..315).
#[derive(Debug)]
pub struct ExtWorld {
    _private: (),
}

impl ExtWorld {
    /// An empty host with the manual clock at 0 ms.
    pub fn new() -> ExtWorld {
        unimplemented!("host::testing::ext_world::ExtWorld::new")
    }

    /// A host with the bundled core plugins installed and enabled.
    pub fn with_core_plugins() -> ExtWorld {
        unimplemented!("host::testing::ext_world::ExtWorld::with_core_plugins")
    }

    /// Installs, enables and activates a plugin; `handlers` maps handler names to scripts.
    pub fn install(&mut self, tier: Tier, manifest: Value, handlers: Vec<(String, HandlerScript)>) -> Result<(), Diagnostic> {
        unimplemented!("host::testing::ext_world::ExtWorld::install")
    }

    /// Installs a plugin on one side.
    pub fn install_on(&mut self, tier: Tier, side: Side, manifest: Value, handlers: Vec<(String, HandlerScript)>) -> Result<(), Diagnostic> {
        unimplemented!("host::testing::ext_world::ExtWorld::install_on")
    }

    /// Updates a plugin.
    pub fn update(&mut self, tier: Tier, manifest: Value, handlers: Vec<(String, HandlerScript)>) -> Result<(), Diagnostic> {
        unimplemented!("host::testing::ext_world::ExtWorld::update")
    }

    /// Uninstalls a plugin.
    pub fn uninstall(&mut self, plugin: &str) {
        unimplemented!("host::testing::ext_world::ExtWorld::uninstall")
    }

    /// Enables or disables a plugin.
    pub fn set_enabled(&mut self, plugin: &str, enabled: bool) {
        unimplemented!("host::testing::ext_world::ExtWorld::set_enabled")
    }

    /// Whether a plugin is enabled.
    pub fn plugin_enabled(&self, plugin: &str) -> bool {
        unimplemented!("host::testing::ext_world::ExtWorld::plugin_enabled")
    }

    /// Whether a plugin is installed.
    pub fn plugin_installed(&self, plugin: &str) -> bool {
        unimplemented!("host::testing::ext_world::ExtWorld::plugin_installed")
    }

    /// Restarts a plugin.
    pub fn restart(&mut self, plugin: &str) {
        unimplemented!("host::testing::ext_world::ExtWorld::restart")
    }

    /// Deactivates a plugin.
    pub fn deactivate(&mut self, plugin: &str) {
        unimplemented!("host::testing::ext_world::ExtWorld::deactivate")
    }

    /// Whether a plugin is active.
    pub fn plugin_active(&self, plugin: &str) -> bool {
        unimplemented!("host::testing::ext_world::ExtWorld::plugin_active")
    }

    /// Restarts of a plugin so far.
    pub fn restart_count(&self, plugin: &str) -> u32 {
        unimplemented!("host::testing::ext_world::ExtWorld::restart_count")
    }

    /// Moves the manual clock; due timers fire.
    pub fn advance_ms(&mut self, ms: u64) {
        unimplemented!("host::testing::ext_world::ExtWorld::advance_ms")
    }

    /// The manual clock.
    pub fn now_ms(&self) -> u64 {
        unimplemented!("host::testing::ext_world::ExtWorld::now_ms")
    }

    /// Runs queued chains, handlers, deliveries and virtual time until nothing is pending.
    pub fn settle(&mut self) {
        unimplemented!("host::testing::ext_world::ExtWorld::settle")
    }

    /// `extensions.run-hook` as `caller`.
    pub fn run_hook(&mut self, caller: &str, point: &str, payload_json: &str) -> Result<RequestId, PluginError> {
        unimplemented!("host::testing::ext_world::ExtWorld::run_hook")
    }

    /// The `request-completed` outcome delivered to `caller`.
    pub fn outcome(&self, caller: &str, request: RequestId) -> Option<Outcome> {
        unimplemented!("host::testing::ext_world::ExtWorld::outcome")
    }

    /// `extensions.take-reply`.
    pub fn take_reply(&mut self, caller: &str, request: RequestId) -> Option<String> {
        unimplemented!("host::testing::ext_world::ExtWorld::take_reply")
    }

    /// `extensions.list`.
    pub fn list(&self, caller: &str, point: &str) -> Result<Vec<ExtensionEntry>, PluginError> {
        unimplemented!("host::testing::ext_world::ExtWorld::list")
    }

    /// `extensions.register`.
    pub fn register(&mut self, caller: &str, point: &str, extension: &str, priority: u8, entry_json: &str) -> Result<(), PluginError> {
        unimplemented!("host::testing::ext_world::ExtWorld::register")
    }

    /// `extensions.unregister`.
    pub fn unregister(&mut self, caller: &str, point: &str, extension: &str) -> Result<(), PluginError> {
        unimplemented!("host::testing::ext_world::ExtWorld::unregister")
    }

    /// Every handler call in start order.
    pub fn handler_runs(&self) -> Vec<HandlerRun> {
        unimplemented!("host::testing::ext_world::ExtWorld::handler_runs")
    }

    /// Every hook error as (plugin, error).
    pub fn hook_errors(&self) -> Vec<(String, PluginError)> {
        unimplemented!("host::testing::ext_world::ExtWorld::hook_errors")
    }

    /// Deadline breaches of one extension.
    pub fn breach_count(&self, plugin: &str, point: &str, extension: &str) -> u32 {
        unimplemented!("host::testing::ext_world::ExtWorld::breach_count")
    }

    /// Replaces the script of a handler.
    pub fn set_script(&mut self, plugin: &str, handler: &str, script: HandlerScript) {
        unimplemented!("host::testing::ext_world::ExtWorld::set_script")
    }

    /// Scripts a command.
    pub fn script_command(&mut self, command: &str, script: HandlerScript) {
        unimplemented!("host::testing::ext_world::ExtWorld::script_command")
    }

    /// Scripts an export of a plugin.
    pub fn script_export(&mut self, plugin: &str, export: &str, script: HandlerScript) {
        unimplemented!("host::testing::ext_world::ExtWorld::script_export")
    }

    /// Calls an export of a plugin.
    pub fn call_export(&mut self, plugin: &str, call: ExportCall) -> Result<ExportReply, Trap> {
        unimplemented!("host::testing::ext_world::ExtWorld::call_export")
    }

    /// Marks an instance busy.
    pub fn set_instance_busy(&mut self, plugin: &str, busy: bool) {
        unimplemented!("host::testing::ext_world::ExtWorld::set_instance_busy")
    }

    /// Marks an instance activating.
    pub fn set_instance_activating(&mut self, plugin: &str, activating: bool) {
        unimplemented!("host::testing::ext_world::ExtWorld::set_instance_activating")
    }

    /// The helper round trip.
    pub fn set_helper_round_trip_ms(&mut self, ms: u32) {
        unimplemented!("host::testing::ext_world::ExtWorld::set_helper_round_trip_ms")
    }

    /// CPU time per call of a plugin.
    pub fn set_cpu_ms_per_call(&mut self, plugin: &str, ms: u32) {
        unimplemented!("host::testing::ext_world::ExtWorld::set_cpu_ms_per_call")
    }

    /// Every captured log line; `plugin` is the emitter or `kernel`.
    pub fn all_logs(&self) -> Vec<LogLine> {
        unimplemented!("host::testing::ext_world::ExtWorld::all_logs")
    }

    /// (topic, payload_json) of every `topic-published` event delivered to a plugin.
    pub fn topics_received(&self, plugin: &str) -> Vec<(String, String)> {
        unimplemented!("host::testing::ext_world::ExtWorld::topics_received")
    }

    /// Why an extension is dormant; `None` when active.
    pub fn dormant_reason(&self, plugin: &str, point: &str, extension: &str) -> Option<String> {
        unimplemented!("host::testing::ext_world::ExtWorld::dormant_reason")
    }

    /// Whether an extension is active.
    pub fn is_active(&self, plugin: &str, point: &str, extension: &str) -> bool {
        unimplemented!("host::testing::ext_world::ExtWorld::is_active")
    }

    /// The stored user order of a point.
    pub fn set_user_order(&mut self, point: &str, order: Vec<(String, String)>) {
        unimplemented!("host::testing::ext_world::ExtWorld::set_user_order")
    }

    /// The Reset button of an extension.
    pub fn reset_extension(&mut self, plugin: &str, point: &str, extension: &str) {
        unimplemented!("host::testing::ext_world::ExtWorld::reset_extension")
    }

    /// Kernel notices drawn for the user.
    pub fn notices(&self) -> Vec<String> {
        unimplemented!("host::testing::ext_world::ExtWorld::notices")
    }

    /// The audit log.
    pub fn audit(&self) -> Vec<AuditRecord> {
        unimplemented!("host::testing::ext_world::ExtWorld::audit")
    }

    /// Answers the pending kernel yes/no prompt.
    pub fn answer_prompt(&mut self, yes: bool) {
        unimplemented!("host::testing::ext_world::ExtWorld::answer_prompt")
    }

    /// Kernel yes/no prompts shown so far.
    pub fn prompts(&self) -> Vec<String> {
        unimplemented!("host::testing::ext_world::ExtWorld::prompts")
    }

    /// The sanitised reason of the last refusal.
    pub fn last_veto_reason(&self) -> Option<String> {
        unimplemented!("host::testing::ext_world::ExtWorld::last_veto_reason")
    }

    /// The refusal text drawn to the user.
    pub fn last_veto_ui_text(&self) -> String {
        unimplemented!("host::testing::ext_world::ExtWorld::last_veto_ui_text")
    }

    /// `services.call`.
    pub fn call_service(&mut self, caller: &str, service: &str, method: &str, payload_json: &str) -> Result<RequestId, PluginError> {
        unimplemented!("host::testing::ext_world::ExtWorld::call_service")
    }

    /// `services.take-reply`.
    pub fn service_take_reply(&mut self, caller: &str, request: RequestId) -> Option<String> {
        unimplemented!("host::testing::ext_world::ExtWorld::service_take_reply")
    }

    /// `services.reply`.
    pub fn service_reply(&mut self, provider: &str, call: u64, outcome: Result<String, PluginError>) -> Result<(), PluginError> {
        unimplemented!("host::testing::ext_world::ExtWorld::service_reply")
    }

    /// (call id, service, method, payload_json) of every call a provider saw.
    pub fn service_calls_seen(&self, provider: &str) -> Vec<(u64, String, String, String)> {
        unimplemented!("host::testing::ext_world::ExtWorld::service_calls_seen")
    }

    /// The provider of a service.
    pub fn service_provider(&self, service: &str) -> Option<String> {
        unimplemented!("host::testing::ext_world::ExtWorld::service_provider")
    }

    /// `topics.publish`.
    pub fn publish(&mut self, plugin: &str, topic: &str, payload_json: &str) -> Result<(), PluginError> {
        unimplemented!("host::testing::ext_world::ExtWorld::publish")
    }

    /// `commands.set-state`.
    pub fn set_command_state(&mut self, plugin: &str, command: &str, enabled: bool, checked: Option<bool>) -> Result<(), PluginError> {
        unimplemented!("host::testing::ext_world::ExtWorld::set_command_state")
    }

    /// `commands.invoke`.
    pub fn invoke_command(&mut self, caller: &str, command: &str, arguments_json: &str) -> Result<RequestId, PluginError> {
        unimplemented!("host::testing::ext_world::ExtWorld::invoke_command")
    }

    /// The event queue capacity of a plugin.
    pub fn queue_capacity(&self, plugin: &str) -> usize {
        unimplemented!("host::testing::ext_world::ExtWorld::queue_capacity")
    }

    /// Queued events of a plugin.
    pub fn queue_len(&self, plugin: &str) -> usize {
        unimplemented!("host::testing::ext_world::ExtWorld::queue_len")
    }

    /// Blocks event delivery to a plugin.
    pub fn block_delivery(&mut self, plugin: &str, blocked: bool) {
        unimplemented!("host::testing::ext_world::ExtWorld::block_delivery")
    }

    /// Dropped-event counts delivered to a plugin.
    pub fn events_dropped(&self, plugin: &str) -> Vec<u32> {
        unimplemented!("host::testing::ext_world::ExtWorld::events_dropped")
    }

    /// The kind of the first event of the next batch.
    pub fn first_event_of_next_batch(&self, plugin: &str) -> Option<String> {
        unimplemented!("host::testing::ext_world::ExtWorld::first_event_of_next_batch")
    }

    /// Sends a message between plugins.
    pub fn send_message(&mut self, from: &str, to: &str, channel: &str, payload: &[u8]) -> Result<RequestId, PluginError> {
        unimplemented!("host::testing::ext_world::ExtWorld::send_message")
    }

    /// (sender, channel, payload) of every message a plugin received.
    pub fn messages_received(&self, plugin: &str) -> Vec<(String, String, Vec<u8>)> {
        unimplemented!("host::testing::ext_world::ExtWorld::messages_received")
    }

    /// The kinds of events delivered to a plugin, in order.
    pub fn event_order(&self, plugin: &str) -> Vec<String> {
        unimplemented!("host::testing::ext_world::ExtWorld::event_order")
    }

    /// Fills the queue of a plugin with never-dropped events.
    pub fn fill_queue_with_never_dropped_events(&mut self, plugin: &str) {
        unimplemented!("host::testing::ext_world::ExtWorld::fill_queue_with_never_dropped_events")
    }

    /// Fills the queue of a plugin with drop-oldest events.
    pub fn fill_queue_with_drop_oldest_events(&mut self, plugin: &str) {
        unimplemented!("host::testing::ext_world::ExtWorld::fill_queue_with_drop_oldest_events")
    }

    /// Delivers one never-dropped event to a plugin.
    pub fn deliver_never_dropped_event(&mut self, plugin: &str) {
        unimplemented!("host::testing::ext_world::ExtWorld::deliver_never_dropped_event")
    }

    /// A replacement manifest for a core plugin.
    pub fn replacement_manifest(&self, id: &str, core: &str, shape: ReplacementShape) -> Value {
        unimplemented!("host::testing::ext_world::ExtWorld::replacement_manifest")
    }

    /// The surface items of a core plugin.
    pub fn core_surface(&self, core: &str) -> Vec<String> {
        unimplemented!("host::testing::ext_world::ExtWorld::core_surface")
    }

    /// Adds a surface item to a core plugin.
    pub fn add_core_surface_item(&mut self, core: &str, item: &str) {
        unimplemented!("host::testing::ext_world::ExtWorld::add_core_surface_item")
    }

    /// Updates Ilium with a core plugin that gained a surface item.
    pub fn update_ilium_with_surface_item(&mut self, core: &str, item: &str) {
        unimplemented!("host::testing::ext_world::ExtWorld::update_ilium_with_surface_item")
    }

    /// The grants of a plugin.
    pub fn granted(&self, plugin: &str) -> Vec<String> {
        unimplemented!("host::testing::ext_world::ExtWorld::granted")
    }

    /// Whether a plugin declares a grant.
    pub fn declares(&self, plugin: &str, grant: &str) -> bool {
        unimplemented!("host::testing::ext_world::ExtWorld::declares")
    }

    /// The contributions of a plugin.
    pub fn contributions_of(&self, plugin: &str) -> Vec<String> {
        unimplemented!("host::testing::ext_world::ExtWorld::contributions_of")
    }

    /// Writes the system policy.
    pub fn set_policy(&mut self, toml: &str) {
        unimplemented!("host::testing::ext_world::ExtWorld::set_policy")
    }

    /// The palette labels of a plugin.
    pub fn palette_labels_of(&self, plugin: &str) -> Vec<String> {
        unimplemented!("host::testing::ext_world::ExtWorld::palette_labels_of")
    }

    /// The lines of the Plugins screen.
    pub fn plugins_screen_lines(&self) -> Vec<String> {
        unimplemented!("host::testing::ext_world::ExtWorld::plugins_screen_lines")
    }

    /// Who owns a kernel function.
    pub fn owner_of_kernel_function(&self, function: &str) -> String {
        unimplemented!("host::testing::ext_world::ExtWorld::owner_of_kernel_function")
    }

    /// Whether the data folder of a plugin exists.
    pub fn data_folder_exists(&self, plugin: &str) -> bool {
        unimplemented!("host::testing::ext_world::ExtWorld::data_folder_exists")
    }

    /// Whether a plugin can read the data folder of another.
    pub fn can_read_data_folder(&self, plugin: &str, other: &str) -> bool {
        unimplemented!("host::testing::ext_world::ExtWorld::can_read_data_folder")
    }

    /// Every extension point.
    pub fn list_points(&self) -> Vec<PointListing> {
        unimplemented!("host::testing::ext_world::ExtWorld::list_points")
    }

    /// Emits a log record as a plugin.
    pub fn emit_log(&mut self, plugin: &str, level: &str, message: &str) {
        unimplemented!("host::testing::ext_world::ExtWorld::emit_log")
    }

    /// Chains started on a point.
    pub fn chains_started(&self, point: &str) -> u32 {
        unimplemented!("host::testing::ext_world::ExtWorld::chains_started")
    }

    /// Log records delivered through a point.
    pub fn log_records_delivered_to(&self, point: &str) -> Vec<String> {
        unimplemented!("host::testing::ext_world::ExtWorld::log_records_delivered_to")
    }

    /// Batch sizes delivered through a point.
    pub fn batch_sizes(&self, point: &str) -> Vec<usize> {
        unimplemented!("host::testing::ext_world::ExtWorld::batch_sizes")
    }

    /// The user types a key into a pane.
    pub fn type_key_into_pane(&mut self, pane: u64, key: char) {
        unimplemented!("host::testing::ext_world::ExtWorld::type_key_into_pane")
    }

    /// The user pastes into a pane.
    pub fn paste_into_pane(&mut self, pane: u64, text: &str) {
        unimplemented!("host::testing::ext_world::ExtWorld::paste_into_pane")
    }

    /// A plugin writes input into a pane.
    pub fn plugin_write_input(&mut self, plugin: &str, pane: u64, text: &str) {
        unimplemented!("host::testing::ext_world::ExtWorld::plugin_write_input")
    }

    /// The payload of the last chain on a point.
    pub fn last_chain_payload(&self, point: &str) -> Value {
        unimplemented!("host::testing::ext_world::ExtWorld::last_chain_payload")
    }

    /// Registers a snapshot section with a size cap.
    pub fn register_snapshot_section(&mut self, plugin: &str, section: &str, max_bytes: u64) {
        unimplemented!("host::testing::ext_world::ExtWorld::register_snapshot_section")
    }

    /// What a plugin answers for a snapshot section.
    pub fn set_snapshot_reply(&mut self, plugin: &str, section: &str, json: &str) {
        unimplemented!("host::testing::ext_world::ExtWorld::set_snapshot_reply")
    }

    /// Takes a snapshot.
    pub fn take_snapshot(&mut self) {
        unimplemented!("host::testing::ext_world::ExtWorld::take_snapshot")
    }

    /// Restores the snapshot.
    pub fn restore_snapshot(&mut self) {
        unimplemented!("host::testing::ext_world::ExtWorld::restore_snapshot")
    }

    /// A stored snapshot section.
    pub fn stored_snapshot_section(&self, section: &str) -> Option<String> {
        unimplemented!("host::testing::ext_world::ExtWorld::stored_snapshot_section")
    }

    /// The restore event names in order.
    pub fn restore_event_order(&self) -> Vec<String> {
        unimplemented!("host::testing::ext_world::ExtWorld::restore_event_order")
    }

    /// How long a plugin takes to restore.
    pub fn set_restore_duration_ms(&mut self, plugin: &str, ms: u64) {
        unimplemented!("host::testing::ext_world::ExtWorld::set_restore_duration_ms")
    }

    /// The risk class of a point.
    pub fn point_risk(&self, point: &str) -> Option<String> {
        unimplemented!("host::testing::ext_world::ExtWorld::point_risk")
    }

    /// One review-dialog row of a manifest.
    pub fn review_row(&self, heading: &str, manifest: &Value) -> String {
        unimplemented!("host::testing::ext_world::ExtWorld::review_row")
    }

    /// The review-dialog lines of a manifest.
    pub fn review_lines(&self, manifest: &Value) -> Vec<String> {
        unimplemented!("host::testing::ext_world::ExtWorld::review_lines")
    }

    /// The entries of a point.
    pub fn entries_of(&self, point: &str) -> Vec<PointEntry> {
        unimplemented!("host::testing::ext_world::ExtWorld::entries_of")
    }

    /// The payload a contributor saw.
    pub fn seen_payload(&self, plugin: &str) -> Value {
        unimplemented!("host::testing::ext_world::ExtWorld::seen_payload")
    }

    /// The initial payload of the last chain.
    pub fn initial_payload(&self) -> Value {
        unimplemented!("host::testing::ext_world::ExtWorld::initial_payload")
    }

    /// Whether the guard ran after every transform.
    pub fn guard_ran_after_transforms(&self) -> bool {
        unimplemented!("host::testing::ext_world::ExtWorld::guard_ran_after_transforms")
    }

    /// Whether a veto on a point can be overridden by a source.
    pub fn veto_overridable(&self, point: &str, source: &str) -> bool {
        unimplemented!("host::testing::ext_world::ExtWorld::veto_overridable")
    }

    /// Opens a project.
    pub fn open_project(&mut self, path: &str, has_project_values: bool) {
        unimplemented!("host::testing::ext_world::ExtWorld::open_project")
    }

    /// Trust prompts shown so far.
    pub fn trust_prompts(&self) -> Vec<String> {
        unimplemented!("host::testing::ext_world::ExtWorld::trust_prompts")
    }

    /// Answers the pending trust prompt.
    pub fn answer_trust(&mut self, yes: bool) {
        unimplemented!("host::testing::ext_world::ExtWorld::answer_trust")
    }

    /// Revokes the trust of a project.
    pub fn revoke_trust(&mut self, path: &str) {
        unimplemented!("host::testing::ext_world::ExtWorld::revoke_trust")
    }

    /// Whether a project is trusted.
    pub fn project_trusted(&self, path: &str) -> bool {
        unimplemented!("host::testing::ext_world::ExtWorld::project_trusted")
    }

    /// Whether a project is trusted for the SDK.
    pub fn sdk_project_trusted(&self, plugin: &str) -> bool {
        unimplemented!("host::testing::ext_world::ExtWorld::sdk_project_trusted")
    }

    /// Writes a project value.
    pub fn set_project_value(&mut self, plugin: &str, key: &str, value: Value) {
        unimplemented!("host::testing::ext_world::ExtWorld::set_project_value")
    }

    /// Writes a project value of a row type.
    pub fn set_project_value_of_type(&mut self, plugin: &str, key: &str, row_type: &str, value: Value) {
        unimplemented!("host::testing::ext_world::ExtWorld::set_project_value_of_type")
    }

    /// Reads a setting with its source.
    pub fn get_setting(&self, plugin: &str, key: &str) -> SettingReading {
        unimplemented!("host::testing::ext_world::ExtWorld::get_setting")
    }

    /// The notice about ignored project values.
    pub fn ignored_project_values_notice(&self) -> Option<String> {
        unimplemented!("host::testing::ext_world::ExtWorld::ignored_project_values_notice")
    }

    /// The first-use prompt of a setting.
    pub fn first_use_prompt(&self, plugin: &str, key: &str) -> Option<String> {
        unimplemented!("host::testing::ext_world::ExtWorld::first_use_prompt")
    }

    /// Answers the pending first-use prompt.
    pub fn answer_first_use(&mut self, yes: bool) {
        unimplemented!("host::testing::ext_world::ExtWorld::answer_first_use")
    }
}

impl Default for ExtWorld {
    fn default() -> ExtWorld {
        ExtWorld::new()
    }
}
