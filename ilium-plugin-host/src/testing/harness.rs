//! The host harness: one builder that wires a fake kernel, guests, grants, limits and a manual
//! clock into a host under test (test plan section 1.3 `testing`).
//!
//! The manual (`docs/plugins-manual`) does not document the harness itself; every method names
//! the requirement family of the conformance tests that call it. Bodies are `unimplemented!()`
//! placeholders until the host behaviour lands.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ilium_plugin_api::generated::exports::inference_provider::ProviderCall;
use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_api::generated::primitives::agent::HostAgent;
use ilium_plugin_api::generated::primitives::basic::HostBasic;
use ilium_plugin_api::generated::primitives::editor::HostEditor;
use ilium_plugin_api::generated::primitives::terminal::HostTerminal;
use ilium_plugin_api::generated::host::audio::HostAudio;
use ilium_plugin_api::generated::host::catalogs::HostCatalogs;
use ilium_plugin_api::generated::host::cli::HostCli;
use ilium_plugin_api::generated::host::client_requests::HostClientRequests;
use ilium_plugin_api::generated::host::clipboard::HostClipboard;
use ilium_plugin_api::generated::host::geocode::HostGeocode;
use ilium_plugin_api::generated::host::gpu::HostGpu;
use ilium_plugin_api::generated::host::help::HostHelp;
use ilium_plugin_api::generated::host::http_api::HostHttpApi;
use ilium_plugin_api::generated::host::inference::HostInference;
use ilium_plugin_api::generated::host::kernel_settings::HostKernelSettings;
use ilium_plugin_api::generated::host::keymap::HostKeymap;
use ilium_plugin_api::generated::host::keys::HostKeys;
use ilium_plugin_api::generated::host::location::HostLocation;
use ilium_plugin_api::generated::host::media::HostMedia;
use ilium_plugin_api::generated::host::plugins::HostPlugins;
use ilium_plugin_api::generated::host::sessions::HostSessions;
use ilium_plugin_api::generated::host::sounds::HostSounds;
use ilium_plugin_api::generated::host::ui_host::HostUiHost;
use ilium_plugin_api::generated::host::vector::HostVector;
use ilium_plugin_api::generated::host::workspace::HostWorkspace;
use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::ids::{NodeId, RequestId};
use ilium_plugin_api::generated::types::intents::{Intent, Submission};
use ilium_plugin_api::generated::types::layout::Area;
use ilium_plugin_api::generated::types::lifecycle::DeactivationReason;
use ilium_plugin_api::generated::types::requests::{Outcome, RequestResult};
use ilium_plugin_api::generated::types::tree::{AgentActivity, NodeView};
use ilium_plugin_api::generated::types::ui::{UiTree, ViewTarget};
use ilium_plugin_api::ids::PluginId;
use ilium_plugin_api::limits::Limits;
use ilium_plugin_api::{ErrorCode, PluginError, Side, Tier};
use serde_json::Value;

use super::fake_kernel::FakeKernel;
use super::fake_net::FakeNet;
use super::native_guest::NativeGuest;
use crate::audit::AuditRecord;
use crate::calls::Trap;
use crate::dispatch::PumpReport;
use crate::error::HostError;
use crate::imports::agent::AgentImport;
use crate::imports::audio::AudioImport;
use crate::imports::basic::BasicImport;
use crate::imports::catalogs::CatalogsImport;
use crate::imports::cli::CliImport;
use crate::imports::client_requests::ClientRequestsImport;
use crate::imports::clipboard::ClipboardImport;
use crate::imports::editor::EditorImport;
use crate::imports::geocode::GeocodeImport;
use crate::imports::gpu::GpuImport;
use crate::imports::help::HelpImport;
use crate::imports::http_api::HttpApiImport;
use crate::imports::inference::InferenceImport;
use crate::imports::kernel_settings::KernelSettingsImport;
use crate::imports::keymap::KeymapImport;
use crate::imports::keys::KeysImport;
use crate::imports::location::LocationImport;
use crate::imports::media::MediaImport;
use crate::imports::plugins::PluginsImport;
use crate::imports::sessions::SessionsImport;
use crate::imports::sounds::SoundsImport;
use crate::imports::terminal::TerminalImport;
use crate::imports::ui_host::UiHostImport;
use crate::imports::vector::VectorImport;
use crate::imports::workspace::WorkspaceImport;
use crate::telemetry::LogLine;

/// A clock the test advances by hand; milliseconds since an arbitrary start.
#[derive(Debug, Clone, Default)]
pub struct ManualClock {
    now_millis: u64,
}

impl ManualClock {
    /// A clock at time zero.
    pub fn new() -> ManualClock {
        ManualClock { now_millis: 0 }
    }

    /// The current reading in milliseconds.
    pub fn now_millis(&self) -> u64 {
        self.now_millis
    }
}

// ------------------------------------------------------------------ result and record types

/// The outcome of one import call made through `HarnessHandle::call_import` (CAP-*, SEC-*).
#[derive(Debug, Clone, PartialEq)]
pub enum ImportResult {
    /// The call returned a value.
    Ok,
    /// The call returned a plugin error.
    Err(PluginError),
    /// The call trapped.
    Trap(Trap),
}

/// The reply of one JSON-argument import call (`HarnessHandle::call_import_json`, HST-*/API-*).
#[derive(Debug, Clone, Default)]
pub struct ImportReply {
    error: Option<PluginError>,
    trap: Option<String>,
    request_id: Option<u64>,
    timer_id: Option<u64>,
}

impl ImportReply {
    /// The plugin error the call returned, if any (API-001).
    pub fn error(&self) -> Option<PluginError> {
        unimplemented!("host::testing::ImportReply::error")
    }

    /// The error code string of the returned error, if any (API-073 `busy`).
    pub fn error_code(&self) -> Option<&str> {
        unimplemented!("host::testing::ImportReply::error_code")
    }

    /// The trap code if the call trapped (HST-410 `host.capability-not-requested`).
    pub fn trap_code(&self) -> Option<String> {
        unimplemented!("host::testing::ImportReply::trap_code")
    }

    /// `retry-after-ms` of a quota refusal (API-07x CPU quota).
    pub fn retry_after_ms(&self) -> Option<u32> {
        unimplemented!("host::testing::ImportReply::retry_after_ms")
    }

    /// The request id of an accepted asynchronous call (API-005).
    pub fn request_id(&self) -> Option<u64> {
        unimplemented!("host::testing::ImportReply::request_id")
    }

    /// The timer id returned by `ilium:host/timers.set` (API-075).
    pub fn timer_id(&self) -> u64 {
        unimplemented!("host::testing::ImportReply::timer_id")
    }
}

/// One completion the host enqueued for a request (API-005).
#[derive(Debug, Clone, Default)]
pub struct CompletionView {
    failed_code: Option<String>,
}

impl CompletionView {
    /// The error code if the request failed.
    pub fn failed_code(&self) -> Option<&str> {
        unimplemented!("host::testing::CompletionView::failed_code")
    }
}

/// Where the guest runs (SEC-00x isolation by tier).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Isolation {
    /// Same process as the host.
    InProcess,
    /// A sandboxed helper process.
    HelperProcess,
}

/// One row of the grant review dialog (SEC-004, SEC-007).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReviewRow {
    /// The row is the "read your data and send data out" warning.
    pub toxic: bool,
    /// The visible wording.
    pub text: String,
    /// Risk label (`low`, `medium`, `high`).
    pub risk: String,
    /// The user cannot toggle the row.
    pub locked: bool,
    /// The row has a checkbox.
    pub checkable: bool,
}

/// One message delivered to a plugin by a peer (SEC-01x peer messages).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PeerMessage {
    /// The sending plugin id.
    pub sender: String,
    /// The raw payload.
    pub payload: Vec<u8>,
}

/// One message the host wrote to the system log (AUD-* audit forwarding).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SystemLogMessage {
    /// The syslog identifier (`ilium-audit`).
    pub identifier: String,
    /// The message text.
    pub text: String,
}

/// One section of the diagnostics report, as shown and as saved (SEC secrets redaction).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiagnosticsSection {
    /// Text shown on screen.
    pub shown: String,
    /// Text written to the saved report.
    pub saved: String,
}

/// The outcome of one detection cycle (PRI-0xx detector scheduling).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DetectReport {
    /// Panes the detectors served in this cycle, in order.
    pub served: Vec<NodeId>,
    /// Wall time the detectors consumed, in milliseconds.
    pub spent_ms: u64,
    /// The length of the cycle tick, in milliseconds.
    pub tick_ms: u64,
}

/// What the plugin store holds for the harness plugin (HST-07x store data).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoreDataView {
    /// The plugin id.
    pub plugin: String,
    /// The authorization epoch at the last store write.
    pub authorization_epoch: u64,
    /// The instance generation.
    pub instance: u64,
}

/// One observation of the scheduler tick callback (HST-079).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TickObservation {
    /// The callback was installed when the tick ran.
    pub callback_installed: bool,
}

/// One retained contribution after a trap (HST scheduling trap recovery).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RetainedContribution {
    /// The contribution key, e.g. `decoration:1:badge`.
    pub key: String,
    /// The contribution is marked stale.
    pub stale: bool,
}

/// A guest that accepts every batch and returns nothing (HST-* default guest).
#[derive(Debug, Default)]
pub struct NullGuest;

impl NativeGuest for NullGuest {
    fn on_events(&mut self, batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        Ok(Vec::new())
    }
}

/// The side that applies the named intent case (HST-490).
pub fn applying_side(case: &str) -> Side {
    unimplemented!("host::testing::applying_side")
}

/// An intent the client side applies itself, never forwarded to the server (HST-490).
pub fn a_client_applied_intent() -> Intent {
    unimplemented!("host::testing::a_client_applied_intent")
}

// ------------------------------------------------------------------ builder

/// Builder for a host under test.
pub struct HostHarness {
    tier: Tier,
    side: Side,
    grants: Vec<String>,
    declined: Vec<String>,
    other_side_grants: Vec<(Side, String)>,
    limits: Limits,
    kernel: Option<Arc<FakeKernel>>,
    native_guest: Option<Box<dyn NativeGuest>>,
    wat_text: Option<String>,
    wasm_bytes: Option<Vec<u8>>,
    clock: ManualClock,
    both_sides: bool,
    two_plugins: bool,
    activate_error: Option<String>,
    session_memory_mib: Option<u64>,
    session_helpers: Option<u64>,
    session_pending_requests: Option<u64>,
    state_dir: Option<PathBuf>,
    net: Option<FakeNet>,
    offline: bool,
    system_policy: Option<(String, bool)>,
    user_policy: Option<String>,
    effective_uid: Option<u32>,
    secrets: Vec<(String, String)>,
    settings: Vec<(String, Value)>,
    options: Vec<(String, Value)>,
    panic_in_import: Option<String>,
    tracing: bool,
    profiling: bool,
    detector_plugins: Vec<(String, Tier)>,
}

impl HostHarness {
    /// A harness for a T3 server-side plugin with no grants and default limits.
    pub fn new() -> HostHarness {
        HostHarness {
            tier: Tier::T3,
            side: Side::Server,
            grants: Vec::new(),
            declined: Vec::new(),
            other_side_grants: Vec::new(),
            limits: Limits::default(),
            kernel: None,
            native_guest: None,
            wat_text: None,
            wasm_bytes: None,
            clock: ManualClock::new(),
            both_sides: false,
            two_plugins: false,
            activate_error: None,
            session_memory_mib: None,
            session_helpers: None,
            session_pending_requests: None,
            state_dir: None,
            net: None,
            offline: false,
            system_policy: None,
            user_policy: None,
            effective_uid: None,
            secrets: Vec::new(),
            settings: Vec::new(),
            options: Vec::new(),
            panic_in_import: None,
            tracing: false,
            profiling: false,
            detector_plugins: Vec::new(),
        }
    }

    /// Sets the trust tier.
    pub fn tier(mut self, tier: Tier) -> HostHarness {
        self.tier = tier;
        self
    }

    /// Sets the side.
    pub fn side(mut self, side: Side) -> HostHarness {
        self.side = side;
        self
    }

    /// Sets the granted capabilities, as grant strings.
    pub fn grants(mut self, grants: &[&str]) -> HostHarness {
        self.grants = grants.iter().map(|grant| (*grant).to_owned()).collect();
        self
    }

    /// Capabilities the user declined at review (CAP-0xx declined grants).
    pub fn declined(mut self, grants: &[&str]) -> HostHarness {
        self.declined = grants.iter().map(|grant| (*grant).to_owned()).collect();
        self
    }

    /// Grants held by the plugin's instance on the other side (CAP-0xx per-side grants).
    pub fn other_side_grants(mut self, side: Side, grants: &[&str]) -> HostHarness {
        self.other_side_grants.extend(grants.iter().map(|grant| (side, (*grant).to_owned())));
        self
    }

    /// Sets the manifest limits.
    pub fn limits(mut self, limits: Limits) -> HostHarness {
        self.limits = limits;
        self
    }

    /// Uses this kernel instead of an empty one.
    pub fn kernel(mut self, kernel: Arc<FakeKernel>) -> HostHarness {
        self.kernel = Some(kernel);
        self
    }

    /// Runs this in-process guest instead of a component.
    pub fn with_native_guest(mut self, guest: Box<dyn NativeGuest>) -> HostHarness {
        self.native_guest = Some(guest);
        self
    }

    /// Runs the component compiled from this component-model text.
    pub fn with_wat(mut self, text: &str) -> HostHarness {
        self.wat_text = Some(text.to_owned());
        self
    }

    /// Runs this compiled component (PRI-*/HST-* fixture components).
    pub fn with_wasm_bytes(mut self, bytes: &[u8]) -> HostHarness {
        self.wasm_bytes = Some(bytes.to_vec());
        self
    }

    /// Uses this manual clock.
    pub fn clock(mut self, clock: ManualClock) -> HostHarness {
        self.clock = clock;
        self
    }

    /// Activates the plugin on both sides so their failure counts can be compared (HST-411).
    pub fn both_sides(mut self) -> HostHarness {
        self.both_sides = true;
        self
    }

    /// Activates a second plugin next to the harness plugin (HST-4xx per-plugin quotas).
    pub fn two_plugins(mut self) -> HostHarness {
        self.two_plugins = true;
        self
    }

    /// Makes `activate` fail with this error code (HST-40x activation failure).
    pub fn activate_error(mut self, code: &str) -> HostHarness {
        self.activate_error = Some(code.to_owned());
        self
    }

    /// The session-wide memory budget in MiB (HST-4xx session memory).
    pub fn session_memory_mib(mut self, mib: u64) -> HostHarness {
        self.session_memory_mib = Some(mib);
        self
    }

    /// The session-wide helper process budget (HST-4xx session helpers).
    pub fn session_helpers(mut self, count: u64) -> HostHarness {
        self.session_helpers = Some(count);
        self
    }

    /// The session-wide pending request budget (API-073 `pending_requests`).
    pub fn session_pending_requests(mut self, count: u64) -> HostHarness {
        self.session_pending_requests = Some(count);
        self
    }

    /// The state directory the host persists stores, audit log and tokens into (SEC-*).
    pub fn state_dir(mut self, directory: PathBuf) -> HostHarness {
        self.state_dir = Some(directory);
        self
    }

    /// The fake network every egress goes through (NET-* egress tests).
    pub fn net(mut self, net: FakeNet) -> HostHarness {
        self.net = Some(net);
        self
    }

    /// Starts the host in offline mode (NET-* offline).
    pub fn offline(mut self, offline: bool) -> HostHarness {
        self.offline = offline;
        self
    }

    /// Installs a system policy file; `secure` marks it root-owned and unwritable (POL-*).
    pub fn system_policy(mut self, text: &str, secure: bool) -> HostHarness {
        self.system_policy = Some((text.to_owned(), secure));
        self
    }

    /// Installs a user policy file (POL-*).
    pub fn user_policy(mut self, text: &str) -> HostHarness {
        self.user_policy = Some(text.to_owned());
        self
    }

    /// The effective user id the host believes it runs as (POL-* root refusal).
    pub fn effective_uid(mut self, uid: u32) -> HostHarness {
        self.effective_uid = Some(uid);
        self
    }

    /// Seeds a secret setting in the keyring (SEC secrets).
    pub fn secret(mut self, key: &str, value: &str) -> HostHarness {
        self.secrets.push((key.to_owned(), value.to_owned()));
        self
    }

    /// Seeds a plugin setting (SEC settings precedence).
    pub fn setting(mut self, key: &str, value: Value) -> HostHarness {
        self.settings.push((key.to_owned(), value));
        self
    }

    /// A catch-all harness option (API_GAPS.md `HostHarness::option`).
    pub fn option(mut self, key: &str, value: Value) -> HostHarness {
        self.options.push((key.to_owned(), value));
        self
    }

    /// Makes the named import panic inside the host (KER-* panic containment).
    pub fn panic_in_import(mut self, import: &str) -> HostHarness {
        self.panic_in_import = Some(import.to_owned());
        self
    }

    /// Enables call tracing (DEV-* performance overhead).
    pub fn tracing(mut self, enabled: bool) -> HostHarness {
        self.tracing = enabled;
        self
    }

    /// Enables profiling (DEV-* performance overhead).
    pub fn profiling(mut self, enabled: bool) -> HostHarness {
        self.profiling = enabled;
        self
    }

    /// Registers an additional detector plugin by id and tier (PRI-0xx detector order).
    pub fn with_detector_plugin(mut self, plugin: &str, tier: Tier) -> HostHarness {
        self.detector_plugins.push((plugin.to_owned(), tier));
        self
    }

    /// Builds the host and activates the plugin.
    pub fn build(self) -> Result<HarnessHandle, HostError> {
        unimplemented!("host::testing::HostHarness::build")
    }
}

impl Default for HostHarness {
    fn default() -> HostHarness {
        HostHarness::new()
    }
}

// ------------------------------------------------------------------ handle

/// A running host under test.
#[derive(Debug)]
pub struct HarnessHandle {
    clock: ManualClock,
}

impl HarnessHandle {
    // ---- dispatch and observation (test plan 1.3)

    /// Queues events for the plugin.
    pub fn deliver(&self, events: Vec<Event>) {
        unimplemented!("host::testing::HarnessHandle::deliver")
    }

    /// Runs the dispatcher until quiet or `max_rounds`.
    pub fn pump(&self, max_rounds: u32) -> PumpReport {
        unimplemented!("host::testing::HarnessHandle::pump")
    }

    /// The outcomes waiting for the plugin.
    pub fn outcomes(&self, plugin: &PluginId) -> Vec<RequestResult> {
        unimplemented!("host::testing::HarnessHandle::outcomes")
    }

    /// The traps that ended calls so far.
    pub fn failures(&self) -> Vec<Trap> {
        unimplemented!("host::testing::HarnessHandle::failures")
    }

    /// The captured log lines.
    pub fn logs(&self) -> Vec<LogLine> {
        unimplemented!("host::testing::HarnessHandle::logs")
    }

    /// The audit records written so far.
    pub fn audit(&self) -> Vec<AuditRecord> {
        unimplemented!("host::testing::HarnessHandle::audit")
    }

    /// The instance state.
    pub fn state(&self) -> InstanceState {
        unimplemented!("host::testing::HarnessHandle::state")
    }

    /// Moves the manual clock forward and fires due timers.
    pub fn advance_clock(&mut self, by: Duration) {
        unimplemented!("host::testing::HarnessHandle::advance_clock")
    }

    /// The monotonic clock reading the host sees (HST-49x forward latency).
    pub fn now_monotonic(&self) -> Duration {
        unimplemented!("host::testing::HarnessHandle::now_monotonic")
    }

    /// Jumps the wall clock without advancing the monotonic clock (HST-08x clock jumps).
    pub fn inject_clock_jump(&self, by: Duration) {
        unimplemented!("host::testing::HarnessHandle::inject_clock_jump")
    }

    /// The fake kernel behind this host (HST-4xx raw-bytes checks).
    pub fn kernel(&self) -> Arc<FakeKernel> {
        unimplemented!("host::testing::HarnessHandle::kernel")
    }

    // ---- import calls (CAP-*, API-*)

    /// Calls an `ilium:host` import by WIT name with JSON arguments (CAP-0xx, SEC-*).
    pub fn call_import(&self, name: &str, args: Value) -> ImportResult {
        unimplemented!("host::testing::HarnessHandle::call_import")
    }

    /// Calls an import and returns its JSON value (SEC-* value checks).
    pub fn call_import_value(&self, name: &str, args: Value) -> Result<Value, PluginError> {
        unimplemented!("host::testing::HarnessHandle::call_import_value")
    }

    /// Calls an import by interface and function with a JSON argument text (HST-*/API-*).
    pub fn call_import_json(
        &self,
        interface: &str,
        function: &str,
        args_json: &str,
    ) -> ImportReply {
        unimplemented!("host::testing::HarnessHandle::call_import_json")
    }

    // ---- primitive and host interface accessors (PRI-*, WIT:ilium:host/*)

    /// The `ilium:primitives/terminal` interface as the plugin sees it.
    pub fn terminal(&self) -> impl HostTerminal {
        TerminalImport
    }

    /// The `ilium:primitives/agent` interface as the plugin sees it.
    pub fn agent(&self) -> impl HostAgent {
        AgentImport
    }

    /// The `ilium:primitives/editor` interface as the plugin sees it.
    pub fn editor(&self) -> impl HostEditor {
        EditorImport
    }

    /// The `ilium:primitives/basic` interface as the plugin sees it.
    pub fn basic(&self) -> impl HostBasic {
        BasicImport
    }

    /// The `ilium:host/audio` interface as the plugin sees it.
    pub fn audio(&self) -> impl HostAudio {
        AudioImport
    }

    /// The `ilium:host/catalogs` interface as the plugin sees it.
    pub fn catalogs(&self) -> impl HostCatalogs {
        CatalogsImport
    }

    /// The `ilium:host/cli` interface as the plugin sees it.
    pub fn cli(&self) -> impl HostCli {
        CliImport
    }

    /// The `ilium:host/client-requests` interface as the plugin sees it.
    pub fn client_requests(&self) -> impl HostClientRequests {
        ClientRequestsImport
    }

    /// The `ilium:host/clipboard` interface as the plugin sees it.
    pub fn clipboard(&self) -> impl HostClipboard {
        ClipboardImport
    }

    /// The `ilium:host/geocode` interface as the plugin sees it.
    pub fn geocode(&self) -> impl HostGeocode {
        GeocodeImport
    }

    /// The `ilium:host/gpu` interface as the plugin sees it.
    pub fn gpu(&self) -> impl HostGpu {
        GpuImport
    }

    /// The `ilium:host/help` interface as the plugin sees it.
    pub fn help(&self) -> impl HostHelp {
        HelpImport
    }

    /// The `ilium:host/http-api` interface as the plugin sees it.
    pub fn http_api(&self) -> impl HostHttpApi {
        HttpApiImport
    }

    /// The `ilium:host/inference` interface as the plugin sees it.
    pub fn inference(&self) -> impl HostInference {
        InferenceImport
    }

    /// The `ilium:host/kernel-settings` interface as the plugin sees it.
    pub fn kernel_settings(&self) -> impl HostKernelSettings {
        KernelSettingsImport
    }

    /// The `ilium:host/keymap` interface as the plugin sees it.
    pub fn keymap(&self) -> impl HostKeymap {
        KeymapImport
    }

    /// The `ilium:host/keys` interface as the plugin sees it.
    pub fn keys(&self) -> impl HostKeys {
        KeysImport
    }

    /// The `ilium:host/location` interface as the plugin sees it.
    pub fn location(&self) -> impl HostLocation {
        LocationImport
    }

    /// The `ilium:host/media` interface as the plugin sees it.
    pub fn media(&self) -> impl HostMedia {
        MediaImport
    }

    /// The `ilium:host/plugins` interface as the plugin sees it.
    pub fn plugins(&self) -> impl HostPlugins {
        PluginsImport
    }

    /// The `ilium:host/sessions` interface as the plugin sees it.
    pub fn sessions(&self) -> impl HostSessions {
        SessionsImport
    }

    /// The `ilium:host/sounds` interface as the plugin sees it.
    pub fn sounds(&self) -> impl HostSounds {
        SoundsImport
    }

    /// The `ilium:host/ui-host` interface as the plugin sees it.
    pub fn ui_host(&self) -> impl HostUiHost {
        UiHostImport
    }

    /// The `ilium:host/vector` interface as the plugin sees it.
    pub fn vector(&self) -> impl HostVector {
        VectorImport
    }

    /// The `ilium:host/workspace` interface as the plugin sees it.
    pub fn workspace(&self) -> impl HostWorkspace {
        WorkspaceImport
    }

    /// A second plugin with these grants, sharing this host's kernel (PRI-0xx peer plugins).
    pub fn peer(&self, plugin: &str, grants: &[&str]) -> HarnessHandle {
        unimplemented!("host::testing::HarnessHandle::peer")
    }

    /// Calls the plugin's `render` export (PRI basic views).
    pub fn render(&self, view: ViewTarget, area: Area) -> Result<UiTree, Trap> {
        unimplemented!("host::testing::HarnessHandle::render")
    }

    /// The error code of the last failed completion for this request (PRI editor completions).
    pub fn last_completion_failure(&self, request: RequestId) -> Option<ErrorCode> {
        unimplemented!("host::testing::HarnessHandle::last_completion_failure")
    }

    // ---- requests and completions (API-*, HST-*)

    /// Whether the request completed (API catalogs).
    pub fn request_completed(&self, request: RequestId) -> bool {
        unimplemented!("host::testing::HarnessHandle::request_completed")
    }

    /// The error code the request completed with, if it failed (API timeouts).
    pub fn request_error(&self, request: RequestId) -> Option<ErrorCode> {
        unimplemented!("host::testing::HarnessHandle::request_error")
    }

    /// Submits intents directly, bypassing the guest (HST-49x requests and outcomes).
    pub fn submit_directly(&self, submission: Submission) -> Result<RequestId, ErrorCode> {
        unimplemented!("host::testing::HarnessHandle::submit_directly")
    }

    /// Takes the result of a request once; a second take finds nothing (HST-49x).
    pub fn take_result(&self, request: RequestId) -> Result<RequestResult, ErrorCode> {
        unimplemented!("host::testing::HarnessHandle::take_result")
    }

    /// Starts a request the kernel never answers (API-073 pending requests).
    pub fn start_slow_request(&self) -> ImportReply {
        unimplemented!("host::testing::HarnessHandle::start_slow_request")
    }

    /// Completes the oldest pending request (API-073).
    pub fn complete_oldest_request(&self) {
        unimplemented!("host::testing::HarnessHandle::complete_oldest_request")
    }

    /// The completion enqueued for a request id (API-005).
    pub fn completion_of(&self, request: u64) -> Option<CompletionView> {
        unimplemented!("host::testing::HarnessHandle::completion_of")
    }

    /// Pending requests of the plugin (API-073).
    pub fn pending_requests(&self) -> usize {
        unimplemented!("host::testing::HarnessHandle::pending_requests")
    }

    /// Fills the session pending-request budget to this count (API-073).
    pub fn fill_session_pending(&self, count: u64) {
        unimplemented!("host::testing::HarnessHandle::fill_session_pending")
    }

    /// Submissions forwarded from the client to the server (HST-490).
    pub fn forwarded_submissions(&self) -> Vec<Submission> {
        unimplemented!("host::testing::HarnessHandle::forwarded_submissions")
    }

    /// Attaches a second client to the same server (HST-49x mirrors).
    pub fn attach_second_client(&self) -> HarnessHandle {
        unimplemented!("host::testing::HarnessHandle::attach_second_client")
    }

    /// Forward latency of the submission with this correlation (HST-49x).
    pub fn forward_latency_of(&self, correlation: u64) -> Duration {
        unimplemented!("host::testing::HarnessHandle::forward_latency_of")
    }

    /// The client mirror's update index (HST-49x mirror ordering).
    pub fn mirror_update_index(&self) -> u64 {
        unimplemented!("host::testing::HarnessHandle::mirror_update_index")
    }

    /// The mirror update index at which this request completed (HST-49x).
    pub fn request_completed_index(&self, correlation: u64) -> u64 {
        unimplemented!("host::testing::HarnessHandle::request_completed_index")
    }

    /// The tree as the client reads it from its mirror (HST-49x).
    pub fn client_tree_read(&self) -> Vec<NodeView> {
        unimplemented!("host::testing::HarnessHandle::client_tree_read")
    }

    // ---- lifecycle (HST-40x, LIF-*)

    /// Stops the server and starts it again on the same state (HST-49x journal replay).
    pub fn restart_server(&self) {
        unimplemented!("host::testing::HarnessHandle::restart_server")
    }

    /// Deactivates and reactivates the instance with this reason (LIF-* restart).
    pub fn restart_instance(&self, reason: DeactivationReason) {
        unimplemented!("host::testing::HarnessHandle::restart_instance")
    }

    /// Deactivates the plugin (LIF-* deactivate).
    pub fn deactivate_plugin(&self) {
        unimplemented!("host::testing::HarnessHandle::deactivate_plugin")
    }

    /// Deactivates with this reason and returns the restore bytes (HST scheduling calls).
    pub fn deactivate(&self, reason: DeactivationReason) -> Option<Vec<u8>> {
        unimplemented!("host::testing::HarnessHandle::deactivate")
    }

    /// Shuts the host down (HST-49x shutdown with pending requests).
    pub fn shutdown(&self) {
        unimplemented!("host::testing::HarnessHandle::shutdown")
    }

    /// Adds a second plugin with these grants and returns its handle (HST-49x).
    pub fn add_second_plugin(&self, grants: &[&str]) -> HarnessHandle {
        unimplemented!("host::testing::HarnessHandle::add_second_plugin")
    }

    /// Activates a second plugin that reserves this much memory (HST-4xx session memory).
    pub fn activate_second_plugin_with_memory(&self, mib: u64) {
        unimplemented!("host::testing::HarnessHandle::activate_second_plugin_with_memory")
    }

    /// Activates a second plugin that needs a helper process (HST-4xx session helpers).
    pub fn activate_second_plugin_with_helper(&self) {
        unimplemented!("host::testing::HarnessHandle::activate_second_plugin_with_helper")
    }

    /// Frees this much of the session memory budget (HST-4xx session memory).
    pub fn free_session_memory_mib(&self, mib: u64) {
        unimplemented!("host::testing::HarnessHandle::free_session_memory_mib")
    }

    /// Enables the plugin (HST-40x restart after enable).
    pub fn enable(&self) {
        unimplemented!("host::testing::HarnessHandle::enable")
    }

    /// Disables the plugin (HST-40x).
    pub fn disable(&self) {
        unimplemented!("host::testing::HarnessHandle::disable")
    }

    /// Upgrades the plugin in place (HST-40x failure count reset).
    pub fn upgrade_in_place(&self) {
        unimplemented!("host::testing::HarnessHandle::upgrade_in_place")
    }

    /// Fails the plugin's running job (HST-40x jobs do not count as failures).
    pub fn fail_job(&self) {
        unimplemented!("host::testing::HarnessHandle::fail_job")
    }

    /// Replaces the native guest (HST-40x).
    pub fn set_guest(&self, guest: Box<dyn NativeGuest>) {
        unimplemented!("host::testing::HarnessHandle::set_guest")
    }

    /// Makes the next `deactivate` export return this (HST-402/403 restore bytes).
    pub fn set_deactivate_result(&self, result: Result<Vec<u8>, Trap>) {
        unimplemented!("host::testing::HarnessHandle::set_deactivate_result")
    }

    /// The restore bytes kept from the last clean deactivate (HST-403).
    pub fn restore_bytes(&self) -> Option<Vec<u8>> {
        unimplemented!("host::testing::HarnessHandle::restore_bytes")
    }

    /// Makes the next `render` export return this (HST-40x invalid ui tree).
    pub fn set_render_result(&self, result: Result<UiTree, Trap>) {
        unimplemented!("host::testing::HarnessHandle::set_render_result")
    }

    /// Renders the named view (HST-40x).
    pub fn render_view(&self, view: &str) {
        unimplemented!("host::testing::HarnessHandle::render_view")
    }

    /// Makes the next scene frame fail (HST-40x scene frames).
    pub fn fail_scene_frame(&self) {
        unimplemented!("host::testing::HarnessHandle::fail_scene_frame")
    }

    // ---- failure accounting (HST-400..411)

    /// Counted failures of the plugin on this side (HST-400).
    pub fn failure_count(&self) -> u32 {
        unimplemented!("host::testing::HarnessHandle::failure_count")
    }

    /// Counted failures on the given side (HST-411).
    pub fn failure_count_of(&self, side: Side) -> u32 {
        unimplemented!("host::testing::HarnessHandle::failure_count_of")
    }

    /// Counted failures of the second plugin (HST-4xx).
    pub fn second_plugin_failure_count(&self) -> u32 {
        unimplemented!("host::testing::HarnessHandle::second_plugin_failure_count")
    }

    /// Records one failure on the given side (HST-411).
    pub fn fail_side(&self, side: Side) {
        unimplemented!("host::testing::HarnessHandle::fail_side")
    }

    /// The restart delay the host scheduled, if any (HST-400 backoff).
    pub fn restart_delay_ms(&self) -> Option<u64> {
        unimplemented!("host::testing::HarnessHandle::restart_delay_ms")
    }

    /// Outcomes of scheduled calls, by call id (HST scheduling trap).
    pub fn call_outcomes(&self) -> Vec<(u64, Outcome)> {
        unimplemented!("host::testing::HarnessHandle::call_outcomes")
    }

    /// Seeds a retained contribution with this key (HST scheduling trap).
    pub fn seed_retained(&self, key: &str) {
        unimplemented!("host::testing::HarnessHandle::seed_retained")
    }

    /// Replaces the retained contribution with this key (HST scheduling trap).
    pub fn replace_retained(&self, key: &str) {
        unimplemented!("host::testing::HarnessHandle::replace_retained")
    }

    /// Retained contributions and their staleness (HST scheduling trap).
    pub fn retained_contributions(&self) -> Vec<RetainedContribution> {
        unimplemented!("host::testing::HarnessHandle::retained_contributions")
    }

    // ---- grants at runtime (CAP-*, HST-069)

    /// Revokes a grant; the plugin is re-activated when the capability was active (CAP-0xx).
    pub fn revoke(&self, grant: &str) {
        unimplemented!("host::testing::HarnessHandle::revoke")
    }

    /// Revokes a grant without re-activating (HST-069).
    pub fn revoke_without_reactivation(&self, grant: &str) {
        unimplemented!("host::testing::HarnessHandle::revoke_without_reactivation")
    }

    /// Grants a capability now (CAP-0xx, HST-069).
    pub fn grant_now(&self, grant: &str) {
        unimplemented!("host::testing::HarnessHandle::grant_now")
    }

    /// Declines a capability (HST-069).
    pub fn decline(&self, grant: &str) {
        unimplemented!("host::testing::HarnessHandle::decline")
    }

    /// The authorization epoch (CAP-0xx revocation, HST-07x store data).
    pub fn authorization_epoch(&self) -> u64 {
        unimplemented!("host::testing::HarnessHandle::authorization_epoch")
    }

    /// The instance pre-generation counter (HST-069).
    pub fn instance_pre_generation(&self) -> u64 {
        unimplemented!("host::testing::HarnessHandle::instance_pre_generation")
    }

    /// What the plugin store holds (HST-07x).
    pub fn store_data(&self) -> StoreDataView {
        unimplemented!("host::testing::HarnessHandle::store_data")
    }

    // ---- scheduler (HST-079..081, API-07x quota)

    /// Observations of the tick callback (HST-079).
    pub fn tick_observations(&self) -> Vec<TickObservation> {
        unimplemented!("host::testing::HarnessHandle::tick_observations")
    }

    /// The decision names the tick callback can return (HST-080).
    pub fn tick_decision_names(&self) -> Vec<String> {
        unimplemented!("host::testing::HarnessHandle::tick_decision_names")
    }

    /// Charges CPU time against the plugin's quota (API-07x).
    pub fn charge_cpu_ms(&self, millis: u64) {
        unimplemented!("host::testing::HarnessHandle::charge_cpu_ms")
    }

    /// The remaining CPU quota in milliseconds (API-07x).
    pub fn quota_balance_ms(&self) -> i64 {
        unimplemented!("host::testing::HarnessHandle::quota_balance_ms")
    }

    /// Events queued for the plugin and not yet delivered (API-074).
    pub fn queued_events(&self) -> Vec<Event> {
        unimplemented!("host::testing::HarnessHandle::queued_events")
    }

    /// Timer ticks queued and not yet delivered (API-075).
    pub fn queued_timer_ticks(&self) -> Vec<u64> {
        unimplemented!("host::testing::HarnessHandle::queued_timer_ticks")
    }

    /// Queues a tick of this timer (API-075).
    pub fn queue_timer_tick(&self, timer: u64) {
        unimplemented!("host::testing::HarnessHandle::queue_timer_tick")
    }

    // ---- inference provider export (WIT:ilium:exports/inference-provider)

    /// Provider calls delivered to the plugin's export.
    pub fn provider_calls(&self) -> Vec<ProviderCall> {
        unimplemented!("host::testing::HarnessHandle::provider_calls")
    }

    /// The models the plugin's provider export reports.
    pub fn set_provider_models(&self, models: Vec<String>) {
        unimplemented!("host::testing::HarnessHandle::set_provider_models")
    }

    /// Calls the provider `complete` export directly.
    pub fn call_provider_complete(&self, call: ProviderCall) -> Result<(), PluginError> {
        unimplemented!("host::testing::HarnessHandle::call_provider_complete")
    }

    /// Calls the provider `models` export directly.
    pub fn call_provider_models(&self) -> Result<Vec<String>, PluginError> {
        unimplemented!("host::testing::HarnessHandle::call_provider_models")
    }

    // ---- detectors (PRI-0xx agent signatures, WIT:ilium:exports/detector)

    /// Loads a detector component for the named plugin.
    pub fn load_detector_wasm(&self, plugin: &str, wasm: &[u8]) {
        unimplemented!("host::testing::HarnessHandle::load_detector_wasm")
    }

    /// Runs one detection cycle over these panes.
    pub fn detect_cycle(&self, panes: &[NodeId]) -> DetectReport {
        unimplemented!("host::testing::HarnessHandle::detect_cycle")
    }

    /// Names of the threads the named plugin's detector ran on.
    pub fn detector_thread_names(&self, plugin: &str) -> Vec<String> {
        unimplemented!("host::testing::HarnessHandle::detector_thread_names")
    }

    /// The name of the detection loop thread.
    pub fn detection_loop_thread_name(&self) -> String {
        unimplemented!("host::testing::HarnessHandle::detection_loop_thread_name")
    }

    /// How often the named plugin's detector was called.
    pub fn detector_call_count(&self, plugin: &str) -> u32 {
        unimplemented!("host::testing::HarnessHandle::detector_call_count")
    }

    /// The plugin ids asked for this pane, in order.
    pub fn detector_ask_order(&self, pane: NodeId) -> Vec<String> {
        unimplemented!("host::testing::HarnessHandle::detector_ask_order")
    }

    /// Makes the named detector spend this long per call.
    pub fn set_detector_cost_ms(&self, plugin: &str, millis: u64) {
        unimplemented!("host::testing::HarnessHandle::set_detector_cost_ms")
    }

    /// Makes the named detector identify this signature, or none.
    pub fn set_detector_identity(&self, plugin: &str, signature: Option<&str>) {
        unimplemented!("host::testing::HarnessHandle::set_detector_identity")
    }

    /// Makes the named detector classify panes as this activity.
    pub fn set_detector_classification(&self, plugin: &str, activity: AgentActivity) {
        unimplemented!("host::testing::HarnessHandle::set_detector_classification")
    }

    /// The signature identified for this pane.
    pub fn identified_signature(&self, pane: NodeId) -> Option<String> {
        unimplemented!("host::testing::HarnessHandle::identified_signature")
    }

    /// The activity the detectors assigned to this pane.
    pub fn pane_activity(&self, pane: NodeId) -> Option<AgentActivity> {
        unimplemented!("host::testing::HarnessHandle::pane_activity")
    }

    /// Activity events emitted so far.
    pub fn activity_events_emitted(&self) -> u32 {
        unimplemented!("host::testing::HarnessHandle::activity_events_emitted")
    }

    /// Deadline breaches of the named detector.
    pub fn detector_breaches(&self, plugin: &str) -> u32 {
        unimplemented!("host::testing::HarnessHandle::detector_breaches")
    }

    /// Sets (with `Some`) and returns the named detector's deadline in milliseconds.
    pub fn detector_deadline_ms(&self, plugin: &str, millis: Option<u64>) -> u64 {
        unimplemented!("host::testing::HarnessHandle::detector_deadline_ms")
    }

    // ---- isolation, helper and guest environment (SEC-00x, SEC-3xx)

    /// Where the guest runs.
    pub fn isolation(&self) -> Isolation {
        unimplemented!("host::testing::HarnessHandle::isolation")
    }

    /// The isolation the host tried before failing to load, if any.
    pub fn isolation_attempted(&self) -> Option<Isolation> {
        unimplemented!("host::testing::HarnessHandle::isolation_attempted")
    }

    /// The load error code, if the plugin did not load.
    pub fn load_error(&self) -> Option<String> {
        unimplemented!("host::testing::HarnessHandle::load_error")
    }

    /// The human-readable load error message, if any.
    pub fn load_error_message(&self) -> Option<String> {
        unimplemented!("host::testing::HarnessHandle::load_error_message")
    }

    /// The guest's monotonic clock reading in nanoseconds (SEC coarse clocks).
    pub fn guest_clock_nanos(&self) -> u64 {
        unimplemented!("host::testing::HarnessHandle::guest_clock_nanos")
    }

    /// The guest's WASI clock reading in nanoseconds (SEC coarse clocks).
    pub fn guest_wasi_clock_nanos(&self) -> u64 {
        unimplemented!("host::testing::HarnessHandle::guest_wasi_clock_nanos")
    }

    /// The operating-system process id the guest runs in.
    pub fn guest_process_id(&self) -> u32 {
        unimplemented!("host::testing::HarnessHandle::guest_process_id")
    }

    /// Runs a named hostile probe as `who` and returns the outcome label (SEC-3xx).
    pub fn run_hostile_probe(&self, who: &str, attempt: &str) -> String {
        unimplemented!("host::testing::HarnessHandle::run_hostile_probe")
    }

    /// Whether the helper process still answers (SEC-3xx).
    pub fn helper_responsive(&self) -> bool {
        unimplemented!("host::testing::HarnessHandle::helper_responsive")
    }

    /// Rejected helper messages so far (SEC-3xx).
    pub fn inject_failure_count(&self) -> u32 {
        unimplemented!("host::testing::HarnessHandle::inject_failure_count")
    }

    /// Injects a raw message on the helper channel (SEC-01x).
    pub fn inject_helper_message(&self, bytes: &[u8]) {
        unimplemented!("host::testing::HarnessHandle::inject_helper_message")
    }

    /// Delivers a peer message from the named sender (SEC-01x).
    pub fn deliver_peer_message(&self, sender: &str, payload: &[u8]) {
        unimplemented!("host::testing::HarnessHandle::deliver_peer_message")
    }

    /// Peer messages the plugin received (SEC-01x).
    pub fn peer_messages(&self) -> Vec<PeerMessage> {
        unimplemented!("host::testing::HarnessHandle::peer_messages")
    }

    /// The environment the plugin process sees (NET token hygiene).
    pub fn plugin_environment(&self) -> Vec<(String, String)> {
        unimplemented!("host::testing::HarnessHandle::plugin_environment")
    }

    /// The arguments the plugin process was started with (NET token hygiene).
    pub fn plugin_arguments(&self) -> Vec<String> {
        unimplemented!("host::testing::HarnessHandle::plugin_arguments")
    }

    /// All events delivered to the plugin, as text (SEC secrets, NET token hygiene).
    pub fn delivered_events_text(&self) -> String {
        unimplemented!("host::testing::HarnessHandle::delivered_events_text")
    }

    // ---- network and HTTP API (NET-*)

    /// A host-side egress on behalf of the named feature (NET host egress).
    pub fn host_egress(&self, feature: &str, url: &str) -> Result<(), PluginError> {
        unimplemented!("host::testing::HarnessHandle::host_egress")
    }

    /// The close reason of an HTTP stream (NET streams).
    pub fn stream_close_reason(&self, stream: u64) -> Option<String> {
        unimplemented!("host::testing::HarnessHandle::stream_close_reason")
    }

    /// Bytes delivered on an HTTP stream (NET streams).
    pub fn stream_bytes_delivered(&self, stream: u64) -> u64 {
        unimplemented!("host::testing::HarnessHandle::stream_bytes_delivered")
    }

    /// The largest single read on an HTTP stream (NET streams).
    pub fn stream_largest_read(&self, stream: u64) -> u64 {
        unimplemented!("host::testing::HarnessHandle::stream_largest_read")
    }

    /// Path of the HTTP API token file (NET http-api).
    pub fn http_api_token_path(&self) -> PathBuf {
        unimplemented!("host::testing::HarnessHandle::http_api_token_path")
    }

    /// The HTTP API token (NET http-api).
    pub fn http_api_token(&self) -> String {
        unimplemented!("host::testing::HarnessHandle::http_api_token")
    }

    /// The address the HTTP API listens on (NET http-api).
    pub fn http_api_bind_address(&self) -> SocketAddr {
        unimplemented!("host::testing::HarnessHandle::http_api_bind_address")
    }

    /// Sends a request to the HTTP API and returns the status (NET http-api).
    pub fn http_api_request(&self, path: &str, headers: &[(&str, &str)]) -> u16 {
        unimplemented!("host::testing::HarnessHandle::http_api_request")
    }

    // ---- audit log (AUD-*)

    /// Path of the audit log file.
    pub fn audit_path(&self) -> PathBuf {
        unimplemented!("host::testing::HarnessHandle::audit_path")
    }

    /// Flushes pending audit summaries.
    pub fn flush_audit_summaries(&self) {
        unimplemented!("host::testing::HarnessHandle::flush_audit_summaries")
    }

    /// Appends an audit event with this detail text.
    pub fn audit_append(&self, event: &str, detail: &str) {
        unimplemented!("host::testing::HarnessHandle::audit_append")
    }

    /// Pads the audit log to this size in bytes.
    pub fn audit_fill_to(&self, bytes: u64) {
        unimplemented!("host::testing::HarnessHandle::audit_fill_to")
    }

    /// The audit limits (size in MiB, age in days).
    pub fn audit_limits(&self) -> (u64, u64) {
        unimplemented!("host::testing::HarnessHandle::audit_limits")
    }

    /// Makes audit writes fail (or succeed again).
    pub fn fail_audit_writes(&self, fail: bool) {
        unimplemented!("host::testing::HarnessHandle::fail_audit_writes")
    }

    /// Audit write attempts so far.
    pub fn audit_write_attempts(&self) -> u64 {
        unimplemented!("host::testing::HarnessHandle::audit_write_attempts")
    }

    /// Audit events waiting to be written.
    pub fn audit_queue_len(&self) -> usize {
        unimplemented!("host::testing::HarnessHandle::audit_queue_len")
    }

    /// Names of the audit events waiting to be written.
    pub fn audit_queue_events(&self) -> Vec<String> {
        unimplemented!("host::testing::HarnessHandle::audit_queue_events")
    }

    /// Audit events dropped because the queue was full.
    pub fn audit_dropped_count(&self) -> u64 {
        unimplemented!("host::testing::HarnessHandle::audit_dropped_count")
    }

    /// Messages the host forwarded to the system log.
    pub fn system_log_messages(&self) -> Vec<SystemLogMessage> {
        unimplemented!("host::testing::HarnessHandle::system_log_messages")
    }

    /// Edits a store file on disk behind the host's back.
    pub fn edit_store_file_externally(&self, file: &str) {
        unimplemented!("host::testing::HarnessHandle::edit_store_file_externally")
    }

    /// Writes every store to disk.
    pub fn persist_all_stores(&self) {
        unimplemented!("host::testing::HarnessHandle::persist_all_stores")
    }

    /// Pads the privacy log to this size in bytes (SEC privacy log).
    pub fn privacy_log_fill_to(&self, bytes: u64) {
        unimplemented!("host::testing::HarnessHandle::privacy_log_fill_to")
    }

    // ---- secrets and settings (SEC secrets, POL-*)

    /// Logs a line as the plugin would.
    pub fn log_from_plugin(&self, text: &str) {
        unimplemented!("host::testing::HarnessHandle::log_from_plugin")
    }

    /// The configuration file text, if one was written.
    pub fn config_text(&self) -> Option<String> {
        unimplemented!("host::testing::HarnessHandle::config_text")
    }

    /// The session snapshot text, if one was written.
    pub fn snapshot_text(&self) -> Option<String> {
        unimplemented!("host::testing::HarnessHandle::snapshot_text")
    }

    /// The settings export text, if one was written.
    pub fn export_text(&self) -> Option<String> {
        unimplemented!("host::testing::HarnessHandle::export_text")
    }

    /// The journal text, if one was written.
    pub fn journal_text(&self) -> Option<String> {
        unimplemented!("host::testing::HarnessHandle::journal_text")
    }

    /// A value stored in the keyring for this plugin and key.
    pub fn keyring_value(&self, plugin: &str, key: &str) -> Option<String> {
        unimplemented!("host::testing::HarnessHandle::keyring_value")
    }

    /// Marks a project as trusted.
    pub fn trust_project(&self, path: &str) {
        unimplemented!("host::testing::HarnessHandle::trust_project")
    }

    /// Writes a plugin setting into a project settings file.
    pub fn write_project_value(&self, project: &str, plugin: &str, key: &str, value: Value) {
        unimplemented!("host::testing::HarnessHandle::write_project_value")
    }

    /// The effective value of a plugin setting.
    pub fn effective_setting(&self, key: &str) -> Option<Value> {
        unimplemented!("host::testing::HarnessHandle::effective_setting")
    }

    /// Sets a kernel setting as the user would (POL-* locked settings).
    pub fn user_set_setting(&self, key: &str, value: Value) -> Result<(), PluginError> {
        unimplemented!("host::testing::HarnessHandle::user_set_setting")
    }

    /// An error string after secret redaction.
    pub fn error_string_for(&self, text: &str) -> String {
        unimplemented!("host::testing::HarnessHandle::error_string_for")
    }

    /// A trap message after secret redaction.
    pub fn trap_message_for(&self, text: &str) -> String {
        unimplemented!("host::testing::HarnessHandle::trap_message_for")
    }

    /// Replaces a secret value now.
    pub fn set_secret_now(&self, key: &str, value: &str) {
        unimplemented!("host::testing::HarnessHandle::set_secret_now")
    }

    /// The CLI's JSON output text.
    pub fn cli_json_text(&self) -> String {
        unimplemented!("host::testing::HarnessHandle::cli_json_text")
    }

    /// The payload text sent on the named channel.
    pub fn channel_payload_text(&self, channel: &str) -> String {
        unimplemented!("host::testing::HarnessHandle::channel_payload_text")
    }

    /// The stored form of all secrets.
    pub fn stored_secret_text(&self) -> String {
        unimplemented!("host::testing::HarnessHandle::stored_secret_text")
    }

    /// The consent prompt shown for this URL.
    pub fn consent_prompt_text_for_url(&self, url: &str) -> String {
        unimplemented!("host::testing::HarnessHandle::consent_prompt_text_for_url")
    }

    /// Forgets the secrets of one plugin, or of all with `None`.
    pub fn forget_secrets(&self, plugin: Option<&str>) {
        unimplemented!("host::testing::HarnessHandle::forget_secrets")
    }

    /// The secrets screen text.
    pub fn secrets_screen_text(&self) -> String {
        unimplemented!("host::testing::HarnessHandle::secrets_screen_text")
    }

    /// A diagnostics report with this extra section text.
    pub fn diagnostics_report_with_section(&self, text: &str) -> DiagnosticsSection {
        unimplemented!("host::testing::HarnessHandle::diagnostics_report_with_section")
    }

    /// Restores a session snapshot from its text.
    pub fn restore_snapshot(&self, text: &str) {
        unimplemented!("host::testing::HarnessHandle::restore_snapshot")
    }

    /// The current value of a secret.
    pub fn secret_value(&self, key: &str) -> Option<String> {
        unimplemented!("host::testing::HarnessHandle::secret_value")
    }

    // ---- installation, policy and review (SEC-*, POL-*, PKG-*)

    /// The plugin details text with this security contact.
    pub fn details_text_with_contact(&self, contact: &str) -> String {
        unimplemented!("host::testing::HarnessHandle::details_text_with_contact")
    }

    /// SHA-256 of the installed package.
    pub fn package_sha256(&self) -> String {
        unimplemented!("host::testing::HarnessHandle::package_sha256")
    }

    /// The publisher key fingerprint.
    pub fn publisher_fingerprint(&self) -> String {
        unimplemented!("host::testing::HarnessHandle::publisher_fingerprint")
    }

    /// Whether the named plugin is disabled.
    pub fn plugin_disabled(&self, plugin: &str) -> bool {
        unimplemented!("host::testing::HarnessHandle::plugin_disabled")
    }

    /// Writes into the plugin's data directory.
    pub fn write_plugin_data(&self, name: &str, bytes: &[u8]) {
        unimplemented!("host::testing::HarnessHandle::write_plugin_data")
    }

    /// Reads from the plugin's data directory.
    pub fn read_plugin_data(&self, name: &str) -> Option<Vec<u8>> {
        unimplemented!("host::testing::HarnessHandle::read_plugin_data")
    }

    /// Writes into another plugin's data directory.
    pub fn write_other_plugin_data(&self, plugin: &str, name: &str, bytes: &[u8]) {
        unimplemented!("host::testing::HarnessHandle::write_other_plugin_data")
    }

    /// Uninstalls the named plugin.
    pub fn uninstall(&self, plugin: &str) -> Result<(), String> {
        unimplemented!("host::testing::HarnessHandle::uninstall")
    }

    /// Whether the named plugin is installed.
    pub fn is_installed(&self, plugin: &str) -> bool {
        unimplemented!("host::testing::HarnessHandle::is_installed")
    }

    /// Enables the named plugin (SEC blocked plugins).
    pub fn enable_plugin(&self, plugin: &str) -> Result<(), String> {
        unimplemented!("host::testing::HarnessHandle::enable_plugin")
    }

    /// Whether the named plugin's replacement of a core feature is active.
    pub fn replacement_active(&self, plugin: &str) -> bool {
        unimplemented!("host::testing::HarnessHandle::replacement_active")
    }

    /// Checks whether a plugin may be installed at this tier (POL-*).
    pub fn install_check(&self, plugin: &str, tier: Tier) -> Result<(), String> {
        unimplemented!("host::testing::HarnessHandle::install_check")
    }

    /// Install check for a plugin fetched from a URL (POL-* source policy).
    pub fn install_check_from_url(&self, plugin: &str, tier: Tier) -> Result<(), String> {
        unimplemented!("host::testing::HarnessHandle::install_check_from_url")
    }

    /// Install check for a plugin signed by this publisher fingerprint (POL-*).
    pub fn install_check_signed_by(&self, plugin: &str, fingerprint: &str) -> Result<(), String> {
        unimplemented!("host::testing::HarnessHandle::install_check_signed_by")
    }

    /// Install check for a plugin replacing this core feature (POL-*).
    pub fn install_check_replacing(&self, plugin: &str, feature: &str) -> Result<(), String> {
        unimplemented!("host::testing::HarnessHandle::install_check_replacing")
    }

    /// Install check with these requested grants (SEC-004 toxic pairs).
    pub fn install_check_with_grants(
        &self,
        plugin: &str,
        tier: Tier,
        grants: &[&str],
    ) -> Result<(), String> {
        unimplemented!("host::testing::HarnessHandle::install_check_with_grants")
    }

    /// Install check with this display name (PKG display-name rules).
    pub fn install_check_with_name(
        &self,
        plugin: &str,
        tier: Tier,
        name: &str,
    ) -> Result<(), String> {
        unimplemented!("host::testing::HarnessHandle::install_check_with_name")
    }

    /// Installs a plugin from a system directory into the user install dir (POL-*).
    pub fn install_from_system_dir(
        &self,
        directory: &Path,
        plugin: &str,
    ) -> Result<PathBuf, String> {
        unimplemented!("host::testing::HarnessHandle::install_from_system_dir")
    }

    /// The user install directory.
    pub fn user_install_dir(&self) -> PathBuf {
        unimplemented!("host::testing::HarnessHandle::user_install_dir")
    }

    /// The runtime directory.
    pub fn runtime_dir(&self) -> PathBuf {
        unimplemented!("host::testing::HarnessHandle::runtime_dir")
    }

    /// Adds a session owned by this user (POL-* session isolation).
    pub fn add_session(&self, name: &str, owner: &str) {
        unimplemented!("host::testing::HarnessHandle::add_session")
    }

    /// Replaces the system policy file text (POL-*).
    pub fn replace_system_policy_file(&self, text: &str) {
        unimplemented!("host::testing::HarnessHandle::replace_system_policy_file")
    }

    /// Tries to check a locked review row (POL-* locked grants).
    pub fn try_check_row(&self, grant: &str) -> bool {
        unimplemented!("host::testing::HarnessHandle::try_check_row")
    }

    /// Review rows for these requested grants (SEC-004).
    pub fn review_rows(&self, grants: &[&str]) -> Vec<ReviewRow> {
        unimplemented!("host::testing::HarnessHandle::review_rows")
    }

    /// Review text for these requested grants (SEC-007).
    pub fn review_text_for(&self, grants: &[&str]) -> String {
        unimplemented!("host::testing::HarnessHandle::review_text_for")
    }

    /// Review text of the installed plugin (SEC secrets in review).
    pub fn review_text(&self) -> String {
        unimplemented!("host::testing::HarnessHandle::review_text")
    }

    /// Review text with these `data-use` manifest entries (SEC privacy).
    pub fn review_text_with_data_use(&self, entries: &[Value]) -> String {
        unimplemented!("host::testing::HarnessHandle::review_text_with_data_use")
    }

    /// The policy as `ilium plugins policy show` prints it (POL-*).
    pub fn policy_show(&self) -> Vec<Value> {
        unimplemented!("host::testing::HarnessHandle::policy_show")
    }

    /// Notices shown to the user.
    pub fn notices(&self) -> Vec<String> {
        unimplemented!("host::testing::HarnessHandle::notices")
    }

    // ---- kernel dialogs and screen (SEC extension security)

    /// Draws rows as the plugin would (SEC plugin rows under dialogs).
    pub fn plugin_draw_rows(&self, rows: &[&str]) -> Result<(), PluginError> {
        unimplemented!("host::testing::HarnessHandle::plugin_draw_rows")
    }

    /// Opens a kernel dialog of this kind (`review`, `trust`, `consent`).
    pub fn open_kernel_dialog(&self, kind: &str) {
        unimplemented!("host::testing::HarnessHandle::open_kernel_dialog")
    }

    /// The composited screen, one row per line.
    pub fn composited_screen(&self) -> Vec<String> {
        unimplemented!("host::testing::HarnessHandle::composited_screen")
    }

    /// Kinds of the dialogs visible now.
    pub fn visible_dialogs(&self) -> Vec<String> {
        unimplemented!("host::testing::HarnessHandle::visible_dialogs")
    }

    /// Kinds of the dialogs queued behind user typing.
    pub fn queued_dialogs(&self) -> Vec<String> {
        unimplemented!("host::testing::HarnessHandle::queued_dialogs")
    }

    /// Whether the dialog of this kind is topmost.
    pub fn dialog_is_topmost(&self, kind: &str) -> bool {
        unimplemented!("host::testing::HarnessHandle::dialog_is_topmost")
    }

    /// Sets the plugin's status bar segment text.
    pub fn plugin_status_segment(&self, text: &str) {
        unimplemented!("host::testing::HarnessHandle::plugin_status_segment")
    }

    /// The status bar text.
    pub fn status_bar_text(&self) -> String {
        unimplemented!("host::testing::HarnessHandle::status_bar_text")
    }

    /// The plugins screen text.
    pub fn plugins_screen_text(&self) -> String {
        unimplemented!("host::testing::HarnessHandle::plugins_screen_text")
    }

    /// Marks the user as typing in a terminal (dialogs wait).
    pub fn set_user_typing_in_terminal(&self, typing: bool) {
        unimplemented!("host::testing::HarnessHandle::set_user_typing_in_terminal")
    }

    /// Presses the dialog's confirmation key.
    pub fn press_dialog_key(&self) {
        unimplemented!("host::testing::HarnessHandle::press_dialog_key")
    }

    /// Types these keys into the focused surface.
    pub fn type_keys(&self, keys: &str) {
        unimplemented!("host::testing::HarnessHandle::type_keys")
    }

    /// Keys the dialog received.
    pub fn dialog_received_keys(&self) -> String {
        unimplemented!("host::testing::HarnessHandle::dialog_received_keys")
    }

    /// The plugin's display name as shown.
    pub fn plugin_display_name(&self) -> String {
        unimplemented!("host::testing::HarnessHandle::plugin_display_name")
    }

    /// Renders plugin chrome of this kind with this text (SEC chrome spoofing).
    pub fn render_plugin_chrome(&self, kind: &str, text: &str) -> String {
        unimplemented!("host::testing::HarnessHandle::render_plugin_chrome")
    }

    /// Renders plugin text of this kind (SEC chrome spoofing).
    pub fn render_plugin_text(&self, kind: &str, text: &str) -> String {
        unimplemented!("host::testing::HarnessHandle::render_plugin_text")
    }

    /// Renders a kernel dialog at this width and theme, one row per line (SEC-346).
    pub fn render_dialog(&self, kind: &str, columns: u16, dark: bool) -> Vec<String> {
        unimplemented!("host::testing::HarnessHandle::render_dialog")
    }

    /// The screen-reader text of a kernel dialog (SEC-346).
    pub fn screen_reader_text(&self, kind: &str) -> String {
        unimplemented!("host::testing::HarnessHandle::screen_reader_text")
    }

    /// The cells of a kernel dialog, one row per line.
    pub fn dialog_cells(&self, kind: &str) -> Vec<String> {
        unimplemented!("host::testing::HarnessHandle::dialog_cells")
    }

    /// What the plugin captured of the screen.
    pub fn plugin_screen_capture(&self) -> Vec<String> {
        unimplemented!("host::testing::HarnessHandle::plugin_screen_capture")
    }

    /// Writes pane output as the terminal would (SEC screen-changed events).
    pub fn pane_output(&self, text: &str) {
        unimplemented!("host::testing::HarnessHandle::pane_output")
    }

    /// Payloads of `screen-changed` events delivered so far.
    pub fn screen_changed_payloads(&self) -> Vec<String> {
        unimplemented!("host::testing::HarnessHandle::screen_changed_payloads")
    }

    /// `screen-changed` events delivered so far.
    pub fn screen_changed_count(&self) -> u64 {
        unimplemented!("host::testing::HarnessHandle::screen_changed_count")
    }

    /// Sets the pane's echo mode (off during password prompts).
    pub fn set_pane_echo(&self, echo: bool) {
        unimplemented!("host::testing::HarnessHandle::set_pane_echo")
    }
}
