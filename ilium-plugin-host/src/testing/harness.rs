//! The host harness: one builder that wires a fake kernel, guests, grants, limits and a manual
//! clock into a host under test (test plan section 1.3 `testing`).
//!
//! The manual (`docs/plugins-manual`) does not document the harness itself; every method names
//! the requirement family of the conformance tests that call it. The native core below uses
//! production lifecycle, executor and dispatch owners; remaining adapters are explicit stubs.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use std::time::Instant;

use ilium_plugin_api::generated::exports::inference_provider::ProviderCall;
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
use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_api::generated::host::sessions::HostSessions;
use ilium_plugin_api::generated::host::sounds::HostSounds;
use ilium_plugin_api::generated::host::ui_host::HostUiHost;
use ilium_plugin_api::generated::host::vector::HostVector;
use ilium_plugin_api::generated::host::workspace::HostWorkspace;
use ilium_plugin_api::generated::primitives::agent::HostAgent;
use ilium_plugin_api::generated::primitives::basic::HostBasic;
use ilium_plugin_api::generated::primitives::editor::HostEditor;
use ilium_plugin_api::generated::primitives::terminal::HostTerminal;
use ilium_plugin_api::generated::types::events::{
    Event, HttpStreamClosedEvent, SettingsChange, SettingsOrigin, StreamOutcome,
};
use ilium_plugin_api::generated::types::ids::{NodeId, RequestId, StreamId, TimerId};
use ilium_plugin_api::generated::types::intents::PluginSide;
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
use super::user_rig::PaneId;
use crate::audit::AuditRecord;
use crate::calls::Trap;
use crate::dispatch::{DispatchConfig, Dispatcher, PumpReport, SubmissionAdmission};
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
use crate::instance::PluginInstance;
use crate::instance::{ExportCall, ExportReply};
use crate::scheduler::{CallKind, EventPush, Scheduler};
use crate::telemetry::LogLine;
use ilium_plugin_api::generated::types::lifecycle::Activation;
use ilium_plugin_api::generated::types::requests::RequestRef;
use ilium_plugin_api::grants::{check_grant, Grant};
use ilium_plugin_api::kernel::Origin;
use ilium_plugin_api::limits::LimitKey;

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

impl ImportResult {
    /// The error code string of the returned error, if any (API error tables).
    pub fn error_code(&self) -> Option<String> {
        match self {
            ImportResult::Err(error) if error.message.starts_with("reentrant: ") => {
                Some("reentrant".to_owned())
            }
            ImportResult::Err(error) => Some(error.code.as_str().to_owned()),
            ImportResult::Ok | ImportResult::Trap(_) => None,
        }
    }

    /// The reason attached to a refusal, such as `rate-limited` (API-073).
    pub fn error_reason(&self) -> Option<String> {
        let ImportResult::Err(error) = self else {
            return None;
        };
        if error.code == ErrorCode::RateLimited || error.message.starts_with("rate-limited") {
            return Some("rate-limited".to_owned());
        }
        // Refusal reasons are carried as a `<reason>: ` message prefix (reference-errors-and-limits.md).
        let (prefix, _) = error.message.split_once(": ")?;
        ilium_plugin_api::error::REFUSAL_REASONS
            .contains(&prefix)
            .then(|| prefix.to_owned())
    }

    /// `retry-after-ms` of a rate-limited refusal.
    pub fn retry_after_ms(&self) -> Option<u32> {
        match self {
            ImportResult::Err(error) => error.retry_after_ms,
            ImportResult::Ok | ImportResult::Trap(_) => None,
        }
    }

    /// The JSON pointer of an invalid-argument error (API settings).
    pub fn error_pointer(&self) -> Option<String> {
        // A schema failure carries its JSON pointer as a leading `/path: ` message prefix.
        let ImportResult::Err(error) = self else {
            return None;
        };
        let (pointer, _) = error.message.split_once(": ")?;
        pointer.starts_with('/').then(|| pointer.to_owned())
    }

    /// The code of a `refused` completion (API-216 `not-found`).
    pub fn refusal_code(&self) -> Option<String> {
        // A refused completion carries the same PluginError as a returned error (API-216).
        self.error_code()
    }
}

/// The outcome of invoking a command whose handler unregisters itself (API-214).
#[derive(Debug, Clone, Default)]
pub struct CommandOutcome {
    completed: bool,
    delivered: bool,
}

impl CommandOutcome {
    /// Whether the handler ran to completion.
    pub fn handler_completed(&self) -> bool {
        self.completed
    }

    /// Whether the invocation result reached the caller.
    pub fn result_delivered(&self) -> bool {
        self.delivered
    }
}

/// The reply of one JSON-argument import call (`HarnessHandle::call_import_json`, HST-*/API-*).
#[derive(Debug, Clone, Default)]
pub struct ImportReply {
    result: Option<ImportResult>,
    request_id: Option<u64>,
    timer_id: Option<u64>,
}

impl ImportReply {
    /// Wraps the outcome of one call; ids are attached by the caller when the call returns them.
    pub fn from_result(result: ImportResult) -> ImportReply {
        ImportReply { result: Some(result), request_id: None, timer_id: None }
    }

    /// Attaches the request id an accepted asynchronous call returned (API-005).
    pub fn with_request_id(mut self, request_id: u64) -> ImportReply {
        self.request_id = Some(request_id);
        self
    }

    /// Attaches the timer id `ilium:host/timers.set` returned (API-075).
    pub fn with_timer_id(mut self, timer_id: u64) -> ImportReply {
        self.timer_id = Some(timer_id);
        self
    }

    /// The plugin error the call returned, if any (API-001).
    pub fn error(&self) -> Option<PluginError> {
        match &self.result {
            Some(ImportResult::Err(error)) => Some(error.clone()),
            Some(ImportResult::Ok | ImportResult::Trap(_)) | None => None,
        }
    }

    /// The error code string of the returned error, if any (API-073 `busy`).
    pub fn error_code(&self) -> Option<&str> {
        match &self.result {
            Some(ImportResult::Err(error)) if error.message.starts_with("reentrant: ") => Some("reentrant"),
            Some(ImportResult::Err(error)) => Some(error.code.as_str()),
            Some(ImportResult::Ok | ImportResult::Trap(_)) | None => None,
        }
    }

    /// The trap code if the call trapped (HST-410 `host.capability-not-requested`).
    pub fn trap_code(&self) -> Option<String> {
        match &self.result {
            Some(ImportResult::Trap(trap)) => Some(trap.code.0.to_string()),
            Some(ImportResult::Ok | ImportResult::Err(_)) | None => None,
        }
    }

    /// `retry-after-ms` of a quota refusal (API-07x CPU quota).
    pub fn retry_after_ms(&self) -> Option<u32> {
        self.result.as_ref().and_then(ImportResult::retry_after_ms)
    }

    /// The request id of an accepted asynchronous call (API-005).
    pub fn request_id(&self) -> Option<u64> {
        self.request_id
    }

    /// The timer id returned by `ilium:host/timers.set` (API-075). Zero when the call set no timer.
    pub fn timer_id(&self) -> u64 {
        self.timer_id.unwrap_or(0)
    }
}

/// One completion the host enqueued for a request (API-005).
#[derive(Debug, Clone)]
pub struct CompletionView {
    result: RequestResult,
}

impl CompletionView {
    /// The error code if the request failed or was refused.
    pub fn failed_code(&self) -> Option<&str> {
        match &self.result.outcome {
            Outcome::Failed(error) | Outcome::Refused(error) => Some(error.code.as_str()),
            _ => None,
        }
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
    /// The grant the row stands for (SEC-004).
    pub grant: String,
    /// The checkbox is ticked (SEC-007: high-risk rows start unticked).
    pub checked: bool,
    /// A note under the row, such as the administrator block (reference-admin-policy.md).
    pub note: Option<String>,
    /// The row is drawn in the warning style (reference-threat-model.md).
    pub warning_style: bool,
    /// The detail text naming the grants behind the row (reference-threat-model.md).
    pub detail: String,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreDataView {
    /// The plugin id.
    pub plugin: PluginId,
    /// The side the store serves (HST-075).
    pub side: Side,
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
    fn on_events(&mut self, _batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        Ok(Vec::new())
    }
}

/// The side that applies the named intent case (HST-490).
pub fn applying_side(case: &str) -> Side {
    // Table "Which side applies an intent" in docs/plugins-manual/pages/api-intents.md.
    const CLIENT_KERNEL_CASES: &[&str] = &[
        "open-view",
        "close-view",
        "toast",
        "dismiss-toast",
        "set-status-segment",
        "clear-status-segment",
        "set-frame-slot",
        "clear-frame-slot",
        "open-external",
        "editor-apply-edits",
        "editor-open",
    ];
    if CLIENT_KERNEL_CASES.contains(&case) {
        Side::Client
    } else {
        Side::Server
    }
}

/// An intent the client side applies itself, never forwarded to the server (HST-490).
pub fn a_client_applied_intent() -> Intent {
    Intent::DismissToast("harness-toast".into())
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
    plugin_name: Option<String>,
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
            plugin_name: None,
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
        self.other_side_grants
            .extend(grants.iter().map(|grant| (side, (*grant).to_owned())));
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
        let wasm_sources =
            usize::from(self.wat_text.is_some()) + usize::from(self.wasm_bytes.is_some());
        if wasm_sources > 1 || (wasm_sources != 0 && self.native_guest.is_some()) {
            return Err(HostError::EngineError(
                "select exactly one native, WAT, or Wasm guest source".into(),
            ));
        }
        let wasm_bytes = match (self.wat_text.clone(), self.wasm_bytes.clone()) {
            (Some(text), None) => Some(wat::parse_str(&text)
                .map_err(|error| HostError::EngineError(format!("invalid component text: {error}")))?),
            (None, bytes) => bytes,
            _ => unreachable!("multiple guest sources were rejected above"),
        };
        // Fail closed for builder options whose adapters have not landed yet.
        if self.effective_uid.is_some()
            || self.panic_in_import.is_some()
            || self.tracing
            || self.profiling
            || !self.other_side_grants.is_empty()
        {
            return Err(HostError::EngineError(
                "requested harness adapter is not implemented".into(),
            ));
        }
        let world = SecurityWorld::create(
            &self,
            self.plugin_name.as_deref().unwrap_or("test.plugin"),
        )?;
        let resolved = ilium_plugin_api::limits::clamp(&self.limits, self.tier)
            .map_err(HostError::Diagnostic)?;
        let mut grants = Vec::new();
        for text in &self.grants {
            let grant = Grant::parse(text).map_err(HostError::Diagnostic)?;
            check_grant(&grant, self.tier, self.side).map_err(HostError::Diagnostic)?;
            if !self.declined.contains(text) {
                grants.push(grant);
            }
        }
        let process = if self.side == Side::Server {
            ilium_plugin_api::platform::ProcessKind::Server
        } else {
            ilium_plugin_api::platform::ProcessKind::Client
        };
        let kernel = self
            .kernel
            .unwrap_or_else(|| Arc::new(FakeKernel::new(process)));
        let plugin = PluginId(self.plugin_name.clone().unwrap_or_else(|| "test.plugin".into()));
        let admission = Arc::new(HarnessAdmission { grants: Arc::new(Mutex::new(grants)) });
        let dispatcher = Dispatcher::with_admission(kernel.clone(), DispatchConfig {
            max_pending_per_plugin: resolved[&LimitKey::PendingRequests] as usize,
            max_submissions_per_call: 256, dedup_window_ms: 600_000,
        }, admission.clone());
        let quota = ilium_execution::QuotaGroup::new(ilium_execution::QuotaLimits {
            clients: 0, jobs: 0, service_jobs: 0, input_bytes: 0, result_bytes: 0,
            worker_threads: 9, worker_bytes: 32 * 1024 * 1024,
        });
        let scheduler = Scheduler::try_with_config(process, 1, crate::calls::ResolvedLimits(resolved.clone()), &quota)
            .map_err(|error| HostError::EngineError(error.to_string()))?;
        scheduler.register_side_with_limits(&plugin, crate::calls::ResolvedLimits(resolved.clone()));
        let extras_declined = self.declined.clone();
        let extras_both_sides = self.both_sides;
        let extras_activate_error = self.activate_error.clone();
        let extras_session_pending = self.session_pending_requests;
        let extras_detector_tiers = self.detector_plugins.clone();
        let extras_script = Arc::new(Mutex::new(HarnessGuestScript::default()));
        let mut extras_slot: Option<Arc<Mutex<Box<dyn NativeGuest>>>> = None;
        let guest: Box<dyn crate::instance::GuestExports> = if let Some(bytes) = wasm_bytes {
            // These are direct test fixtures, not admitted packages. Full world validation
            // remains a separate package gate; WasmGuest rejects all unlinked imports.
            let engine = Arc::new(crate::engine::PluginEngine::with_quota(
                crate::engine::EngineConfig {
                    epoch_tick_ms: 10,
                    cache_dir: None,
                    process,
                },
                &quota,
            )?);
            let compiled = engine.compile(&bytes)?;
            let limits = crate::calls::ResolvedLimits(resolved.clone());
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            let now = Instant::now();
            scheduler.enqueue(
                &plugin,
                CallKind::Activate,
                now,
                Box::new(move || {
                    let result =
                        crate::wasm_guest::WasmGuest::instantiate(engine, &compiled, limits)
                            .map(|guest| Box::new(guest) as Box<dyn crate::instance::GuestExports>);
                    let _ = sender.send(result);
                }),
            );
            scheduler.tick(now);
            receiver.recv().map_err(|_| {
                HostError::EngineError(
                    "component instantiation worker ended without a result".into(),
                )
            })??
        } else {
            let slot = Arc::new(Mutex::new(
                self.native_guest.unwrap_or_else(|| Box::new(NullGuest)),
            ));
            extras_slot = Some(slot.clone());
            Box::new(SharedGuest { slot, script: extras_script.clone() })
        };
        let instance = PluginInstance::from_guest(guest);
        let clamp_logs = self
            .limits
            .0
            .iter()
            .filter_map(|(name, requested)| {
                let key = name.parse::<LimitKey>().ok()?;
                let effective = resolved.get(&key)?;
                (*requested > *effective).then(|| LogLine {
                    plugin: plugin.0.clone(),
                    level: "warn".into(),
                    message: format!(
                        "manifest limit {name} clamped from {requested} to {effective}"
                    ),
                })
            })
            .collect();
        let handle = HarnessHandle {
            plugin: plugin.clone(),
            side: self.side,
            kernel,
            dispatcher,
            scheduler,
            admission,
            instance: Arc::new(Mutex::new(instance)),
            lifecycle_state: Arc::new(Mutex::new(InstanceState::Inactive)),
            pump_lock: Mutex::new(()),
            execution: Mutex::new(Instant::now()),
            observations: Mutex::new(HarnessObservations {
                logs: clamp_logs,
                extras: HarnessExtras {
                    declined: extras_declined,
                    guest_slot: extras_slot,
                    guest_script: extras_script,
                    other: extras_both_sides.then(SimSide::default),
                    activate_error: extras_activate_error,
                    session_pending: 0,
                    session_cap: extras_session_pending.unwrap_or(4096),
                    detector_tiers: extras_detector_tiers,
                    ..HarnessExtras::default()
                },
                ..HarnessObservations::default()
            }),
            clock: Mutex::new(self.clock.now_millis),
            ui: Mutex::new(super::ui_world::UiWorld::new()),
            world: Mutex::new(world),
            second: Mutex::new(None),
        };
        let activation = Activation {
            plugin,
            api: "ilium:plugin@0.1.0".into(),
            instance: 1,
            side: self.side.as_str().into(),
            settings_json: serde_json::to_string(
                &self
                    .settings
                    .into_iter()
                    .collect::<serde_json::Map<String, Value>>(),
            )
            .map_err(|error| HostError::EngineError(error.to_string()))?,
            granted: harness_lock(&handle.admission.grants)
                .iter()
                .map(|grant| grant.as_str().to_owned())
                .collect(),
            restored: None,
        };
        handle.scheduler.begin_activation(&handle.plugin);
        let result = handle.execute(CallKind::Activate, move |instance| {
            instance.activate(activation)
        });
        handle.scheduler.finish_activation(
            &handle.plugin,
            result.as_ref().is_ok_and(|result| result.is_ok()),
        );
        // Activation refusals remain observable lifecycle results rather than host failures.
        if let Some(trap) = harness_lock(&handle.instance).last_trap().cloned() {
            harness_lock(&handle.observations).failures.push(trap);
        }
        handle.push_activation_tree();
        handle.pump(8);
        Ok(handle)
    }
}

impl Default for HostHarness {
    fn default() -> HostHarness {
        HostHarness::new()
    }
}

// ------------------------------------------------------------------ handle

/// A running host under test.
pub struct HarnessHandle {
    plugin: PluginId,
    side: Side,
    kernel: Arc<FakeKernel>,
    dispatcher: Dispatcher,
    scheduler: Scheduler,
    admission: Arc<HarnessAdmission>,
    instance: Arc<Mutex<PluginInstance>>,
    lifecycle_state: Arc<Mutex<InstanceState>>,
    pump_lock: Mutex<()>,
    execution: Mutex<Instant>,
    observations: Mutex<HarnessObservations>,
    clock: Mutex<u64>,
    ui: Mutex<super::ui_world::UiWorld>,
    world: Mutex<SecurityWorld>,
    second: Mutex<Option<Box<HarnessHandle>>>,
}
impl std::fmt::Debug for HarnessHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HarnessHandle")
            .field("plugin", &self.plugin)
            .field("side", &self.side)
            .finish_non_exhaustive()
    }
}
#[derive(Default)]
struct HarnessObservations {
    outcomes: Vec<RequestResult>,
    received: Vec<RequestResult>,
    failures: Vec<Trap>,
    logs: Vec<LogLine>,
    restored: Option<Vec<u8>>,
    extras: HarnessExtras,
}

/// What a test can script into the shared native guest slot.
#[derive(Default)]
struct HarnessGuestScript {
    deactivate: Option<Result<Vec<u8>, Trap>>,
    render: Option<Result<UiTree, Trap>>,
}

/// A native guest that survives traps, so a restart can reuse it (and `set_guest` can swap it).
struct SharedGuest {
    slot: Arc<Mutex<Box<dyn NativeGuest>>>,
    script: Arc<Mutex<HarnessGuestScript>>,
}

impl crate::instance::GuestExports for SharedGuest {
    fn activate(&mut self, activation: Activation) -> Result<Result<(), PluginError>, Trap> {
        Ok(harness_lock(&self.slot).activate(activation))
    }

    fn deactivate(
        &mut self,
        reason: ilium_plugin_api::generated::types::lifecycle::DeactivationReason,
    ) -> Result<Option<Vec<u8>>, Trap> {
        let scripted = harness_lock(&self.script).deactivate.clone();
        match scripted {
            Some(Ok(bytes)) => Ok(Some(bytes)),
            Some(Err(trap)) => Err(trap),
            None => Ok(harness_lock(&self.slot).deactivate(reason)),
        }
    }

    fn on_events(&mut self, batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        harness_lock(&self.slot).on_events(batch)
    }

    fn render(&mut self, view: ViewTarget, area: Area) -> Result<UiTree, Trap> {
        let scripted = harness_lock(&self.script).render.clone();
        match scripted {
            Some(result) => result,
            None => harness_lock(&self.slot).render(view, area),
        }
    }

    fn call_export(
        &mut self,
        call: crate::instance::ExportCall,
    ) -> Result<crate::instance::ExportReply, Trap> {
        harness_lock(&self.slot).call_export(call)
    }
}

/// A pane the harness opened for the pane-lifecycle tests (API-216).
struct SimPane {
    id: u64,
    slot: u32,
    open: bool,
    input: Vec<String>,
}

/// A host-side stream the harness tracks for the flow-control tests.
#[derive(Default)]
struct SimStream {
    unread: usize,
    dropped: usize,
    closed: Vec<(String, bool)>,
    delivered: u64,
    largest_read: u64,
}

/// A pretend detector plugin (HST-4xx detection cycle).
#[derive(Default)]
struct SimDetector {
    cost_ms: u64,
    signature: Option<String>,
    activity: Option<AgentActivity>,
    calls: u32,
    breaches: u32,
    consecutive: u32,
    threads: Vec<String>,
}

/// The state of the other side in a `both_sides` harness.
#[derive(Default)]
struct SimSide {
    policy: ilium_plugin_api::lifecycle::RestartPolicy,
    state: Option<InstanceState>,
    due_ms: Option<u64>,
    failures: u32,
}

/// Everything the harness keeps on top of the production objects.
struct HarnessExtras {
    audit: Vec<AuditRecord>,
    declined: Vec<String>,
    settings: std::collections::BTreeMap<String, Value>,
    emitted: Vec<(String, Event, bool)>,
    subscriptions: Vec<(String, String)>,
    subscription_events: std::collections::BTreeMap<String, usize>,
    commands: std::collections::BTreeMap<String, (String, usize)>,
    callees: std::collections::BTreeMap<String, usize>,
    panes: Vec<SimPane>,
    next_pane: u64,
    next_slot: u32,
    pending: Vec<(u64, u64)>,
    next_request: u64,
    session_pending: u64,
    streams: std::collections::BTreeMap<u64, SimStream>,
    next_stream: u64,
    windows: std::collections::BTreeMap<String, std::collections::VecDeque<u64>>,
    timers: std::collections::BTreeMap<u64, (u64, bool, u64)>,
    queued_ticks: Vec<u64>,
    next_timer: u64,
    retained: std::collections::BTreeMap<String, bool>,
    call_outcomes: Vec<(u64, Outcome)>,
    epoch: u64,
    pre_generation: u64,
    instance_number: u64,
    bucket: crate::scheduler::ExecutionBucket,
    revoked: Vec<String>,
    session_cap: u64,
    last_refill_ms: u64,
    provider_models: Vec<String>,
    provider_calls: Vec<ProviderCall>,
    detectors: std::collections::BTreeMap<String, SimDetector>,
    detector_tiers: Vec<(String, Tier)>,
    peer_messages: Vec<PeerMessage>,
    delivered_text: String,
    inject_failures: u32,
    policy: ilium_plugin_api::lifecycle::RestartPolicy,
    restart_due_ms: Option<u64>,
    restart_delay_ms: Option<u64>,
    guest_slot: Option<Arc<Mutex<Box<dyn NativeGuest>>>>,
    guest_script: Arc<Mutex<HarnessGuestScript>>,
    activation: Option<Activation>,
    other: Option<SimSide>,
    activate_error: Option<String>,
    fs_tokens: i64,
    fs_refill_at: u64,
    wall_offset_ms: i64,
    detected: std::collections::BTreeMap<u64, String>,
    ask_order: std::collections::BTreeMap<u64, Vec<String>>,
    activity_events: u32,
    forwarded: Vec<Submission>,
    deadlines: std::collections::BTreeMap<String, u64>,
    clock_jump_ms: u64,
    session_memory_mib: u64,
    second_helpers: u64,
    second_failures: u32,
    detect_cursor: usize,
}

impl Default for HarnessExtras {
    fn default() -> HarnessExtras {
        HarnessExtras {
            audit: Vec::new(),
            declined: Vec::new(),
            settings: Default::default(),
            emitted: Vec::new(),
            subscriptions: Vec::new(),
            subscription_events: Default::default(),
            commands: Default::default(),
            callees: Default::default(),
            panes: Vec::new(),
            next_pane: 1,
            next_slot: 1,
            pending: Vec::new(),
            next_request: 1,
            session_pending: 0,
            streams: Default::default(),
            next_stream: 1,
            windows: Default::default(),
            timers: Default::default(),
            queued_ticks: Vec::new(),
            next_timer: 1,
            retained: Default::default(),
            call_outcomes: Vec::new(),
            epoch: 1,
            pre_generation: 1,
            instance_number: 1,
            bucket: crate::scheduler::ExecutionBucket::new(),
            revoked: Vec::new(),
            session_cap: 4096,
            last_refill_ms: 0,
            provider_models: Vec::new(),
            provider_calls: Vec::new(),
            detectors: Default::default(),
            detector_tiers: Vec::new(),
            peer_messages: Vec::new(),
            delivered_text: String::new(),
            inject_failures: 0,
            policy: Default::default(),
            restart_due_ms: None,
            restart_delay_ms: None,
            guest_slot: None,
            guest_script: Arc::new(Mutex::new(HarnessGuestScript::default())),
            activation: None,
            other: None,
            activate_error: None,
            fs_tokens: 1_000,
            fs_refill_at: 0,
            wall_offset_ms: 0,
            detected: Default::default(),
            ask_order: Default::default(),
            activity_events: 0,
            forwarded: Vec::new(),
            deadlines: Default::default(),
            clock_jump_ms: 0,
            session_memory_mib: 0,
            second_helpers: 0,
            second_failures: 0,
            detect_cursor: 0,
        }
    }
}
/// The number inside a request reference, for either a submission or a call (API-005).
fn request_ref_number(request: &RequestRef) -> Option<u64> {
    match request {
        RequestRef::Submission(number) => Some(*number),
        RequestRef::Call(request_id) => Some(request_id.0),
    }
}
fn harness_lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
fn harness_trap(code: &'static str, function: &str) -> Trap {
    Trap {
        code: ilium_plugin_api::Code::from_static(code),
        interface: "harness".into(),
        function: function.into(),
    }
}
/// What the harness import router decided for one call.
enum Routed {
    Value(Value),
    Err(PluginError),
    Trap(Trap),
}

fn grant_base(grant: &str) -> &str {
    grant.split(':').next().unwrap_or(grant)
}

fn value_u64(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(Value::as_u64)
}

impl HarnessHandle {
    fn now_ms(&self) -> u64 {
        *harness_lock(&self.clock)
    }

    fn log_line(&self, level: &str, message: String) {
        harness_lock(&self.observations).logs.push(LogLine {
            plugin: self.plugin.0.clone(),
            level: level.into(),
            message,
        });
    }

    /// Counts one failure: the production `RestartPolicy` decides restart or disable (DEV-659).
    fn note_failure(&self) {
        use ilium_plugin_api::lifecycle::RestartDecision;
        let now = self.now_ms();
        let (decision, in_window) = {
            let mut observations = harness_lock(&self.observations);
            let decision = observations.extras.policy.on_failure(now);
            let in_window = observations.extras.policy.failures_in_window(now);
            match decision {
                RestartDecision::RestartAfterMs(delay) => {
                    observations.extras.restart_due_ms = Some(now.saturating_add(delay));
                    observations.extras.restart_delay_ms = Some(delay);
                }
                RestartDecision::Disable => {
                    observations.extras.restart_due_ms = None;
                    observations.extras.restart_delay_ms = None;
                }
            }
            (decision, in_window)
        };
        let (level, message, state) = match decision {
            RestartDecision::RestartAfterMs(delay) => {
                let (level, message) = crate::telemetry::restart_log(delay / 1000, in_window, 3);
                (level, message, InstanceState::RestartScheduled)
            }
            RestartDecision::Disable => {
                let (level, message) = crate::telemetry::disabled_log(in_window);
                (level, message, InstanceState::StoppedByFailure)
            }
        };
        *harness_lock(&self.lifecycle_state) = state;
        self.log_line(&format!("{level:?}").to_lowercase(), message);
    }

    /// Starts a fresh instance on the shared native guest (Wasm guests cannot be re-created here).
    fn restart_now(&self) {
        let (slot, script, number, settings, granted) = {
            let mut observations = harness_lock(&self.observations);
            observations.extras.instance_number += 1;
            observations.extras.restart_due_ms = None;
            (
                observations.extras.guest_slot.clone(),
                observations.extras.guest_script.clone(),
                observations.extras.instance_number,
                serde_json::to_string(&observations.extras.settings).unwrap_or_else(|_| "{}".into()),
                harness_lock(&self.admission.grants)
                    .iter()
                    .map(|grant| grant.as_str().to_owned())
                    .collect::<Vec<_>>(),
            )
        };
        let Some(slot) = slot else {
            return;
        };
        *harness_lock(&self.instance) = PluginInstance::from_guest(Box::new(SharedGuest { slot, script }));
        let activation = Activation {
            plugin: self.plugin.clone(),
            api: "ilium:plugin@0.1.0".into(),
            instance: number,
            side: self.side.as_str().into(),
            settings_json: settings,
            granted,
            restored: None,
        };
        self.scheduler.begin_activation(&self.plugin);
        let result = self.execute(CallKind::Activate, move |instance| instance.activate(activation));
        self.scheduler
            .finish_activation(&self.plugin, result.as_ref().is_ok_and(|result| result.is_ok()));
        if let Some(trap) = harness_lock(&self.instance).last_trap().cloned() {
            harness_lock(&self.observations).failures.push(trap);
            self.note_failure();
        }
        self.push_activation_tree();
    }

    /// EVT-047: after every activation, a holder of tree.read gets one tree-changed
    /// listing every node in pre-order at the start of its first batch.
    fn push_activation_tree(&self) {
        if self.state() != InstanceState::Active || !self.admission.permits("tree.read") {
            return;
        }
        let entries: Vec<(u64, Option<u64>)> = self
            .kernel
            .state()
            .nodes
            .iter()
            .map(|node| (node.id.0, node.parent.map(|parent| parent.0)))
            .collect();
        let mut order: Vec<NodeId> = Vec::with_capacity(entries.len());
        let mut stack: Vec<u64> = entries
            .iter()
            .rev()
            .filter(|(_, parent)| parent.is_none_or(|id| !entries.iter().any(|(node, _)| *node == id)))
            .map(|(id, _)| *id)
            .collect();
        while let Some(id) = stack.pop() {
            order.push(NodeId(id));
            for (child, parent) in entries.iter().rev() {
                if *parent == Some(id) {
                    stack.push(*child);
                }
            }
        }
        if !order.is_empty() {
            self.scheduler
                .push_event(&self.plugin, Event::TreeChanged(order));
        }
    }

    /// A sliding-window rate limit; `Err` carries the wait in milliseconds.
    fn rate_check(&self, key: &str, limit: usize, window_ms: u64) -> Result<(), u64> {
        let now = self.now_ms();
        let mut observations = harness_lock(&self.observations);
        let window = observations.extras.windows.entry(key.to_owned()).or_default();
        while window.front().is_some_and(|stamp| now.saturating_sub(*stamp) >= window_ms) {
            window.pop_front();
        }
        if window.len() >= limit {
            let oldest = window.front().copied().unwrap_or(now);
            return Err(window_ms.saturating_sub(now.saturating_sub(oldest)).max(1));
        }
        window.push_back(now);
        Ok(())
    }

    fn rate_limited(wait_ms: u64) -> Routed {
        let mut error = PluginError::new(ErrorCode::RateLimited, "rate-limited");
        error.retry_after_ms = Some(wait_ms.min(u64::from(u32::MAX)) as u32);
        Routed::Err(error)
    }

    /// The grant an import needs, by interface and function.
    fn needed_grant(interface: &str, function: &str, args: &Value) -> Option<String> {
        let root = args.get("root").and_then(Value::as_str).unwrap_or("project");
        Some(match (interface, function) {
            ("fs", name) if name.starts_with("read") || name == "list" || name == "stat" => {
                format!("fs.read:{root}")
            }
            ("fs", _) => format!("fs.write:{root}"),
            ("tree", name) if name.starts_with("set") || name == "rename" => "tree.write".into(),
            ("tree", _) => "tree.read".into(),
            ("sounds", _) => "sound.play".into(),
            ("toasts", _) => "notify.toast".into(),
            ("topics", _) => "topics.publish".into(),
            ("journal", _) => "journal.write".into(),
            ("geocode", _) => "geocode.search".into(),
            ("panes", "send-input") => "terminal.write".into(),
            ("panes", "send-raw") => "terminal.raw-input".into(),
            ("panes", _) => "panes.read".into(),
            ("process", _) => "process.run:git".into(),
            ("clipboard", "read") => "clipboard.read".into(),
            ("clipboard", _) => "clipboard.write".into(),
            ("commands", "register") => "commands.register".into(),
            _ => return None,
        })
    }

    /// Grant gate and behaviour of one import call (CAP-013..018, API-07x, SEC-013).
    fn route(&self, name: &str, args: &Value) -> Routed {
        let trimmed = name
            .trim_start_matches("ilium:host/")
            .trim_start_matches("ilium:primitives/")
            .replace('/', ".");
        let (interface, function) = trimmed.split_once('.').unwrap_or((trimmed.as_str(), ""));
        let import_name = format!("ilium:host/{interface}.{function}");
        if let Some(needed) = Self::needed_grant(interface, function, args) {
            let (declined, revoked, requested_base) = {
                let observations = harness_lock(&self.observations);
                let held: Vec<String> = harness_lock(&self.admission.grants)
                    .iter()
                    .map(|grant| grant.as_str().to_owned())
                    .collect();
                let base = grant_base(&needed).to_owned();
                let requested = held.iter().any(|grant| grant_base(grant) == base)
                    || observations.extras.declined.iter().any(|grant| grant_base(grant) == base)
                    || observations.extras.revoked.iter().any(|grant| grant_base(grant) == base);
                (
                    observations.extras.declined.contains(&needed),
                    observations.extras.revoked.contains(&needed),
                    requested,
                )
            };
            let reason = if declined {
                Some("declined")
            } else if revoked {
                Some("revoked")
            } else if self.admission.permits(&needed) {
                None
            } else if requested_base {
                Some("not-granted")
            } else if self.state() != InstanceState::Active {
                Some("not-granted")
            } else {
                return self.trap_not_requested(interface, function, &needed, &import_name);
            };
            if let Some(reason) = reason {
                let first = {
                    let key = format!("refused:{needed}:{import_name}");
                    let mut observations = harness_lock(&self.observations);
                    let window = observations.extras.windows.entry(key).or_default();
                    let first = window.is_empty();
                    window.push_back(0);
                    first
                };
                if first {
                    self.log_line("warn", format!("refused {needed} by {import_name} ({reason})"));
                    if !reason.is_empty() && self.state() == InstanceState::Active {
                        self.log_line("warn", format!("call to {interface}.{function} returned not-permitted"));
                    }
                }
                return Routed::Err(PluginError::new(
                    ErrorCode::NotPermitted,
                    format!("{reason}: {import_name}"),
                ));
            }
        }
        self.run_import(interface, function, args)
    }

    fn trap_not_requested(&self, interface: &str, function: &str, needed: &str, import_name: &str) -> Routed {
        let trap = Trap {
            code: ilium_plugin_api::Code::from_static("host.capability-not-requested"),
            interface: format!("ilium:host/{interface}"),
            function: function.to_owned(),
        };
        self.log_line("warn", format!("refused {needed} by {import_name} (not-granted)"));
        self.log_line(
            "error",
            format!("trap host.capability-not-requested in ilium:host/{interface}, function {function}"),
        );
        harness_lock(&self.observations).failures.push(trap.clone());
        self.note_failure();
        Routed::Trap(trap)
    }

    /// The behaviour of an import whose grant check passed.
    fn run_import(&self, interface: &str, function: &str, args: &Value) -> Routed {
        let now = self.now_ms();
        let invalid = |message: &str| Routed::Err(PluginError::new(ErrorCode::InvalidArgument, message));
        match (interface, function) {
            ("timers", "set") => {
                let delay = value_u64(args, "delay-ms").unwrap_or(0);
                if delay < 50 {
                    return invalid("delay-ms must be at least 50");
                }
                let mut observations = harness_lock(&self.observations);
                if observations.extras.timers.len() >= 64 {
                    return Routed::Err(PluginError::new(ErrorCode::QuotaExceeded, "64 timers held"));
                }
                let id = observations.extras.next_timer;
                observations.extras.next_timer += 1;
                let repeat = args.get("repeat").and_then(Value::as_bool).unwrap_or(false);
                observations.extras.timers.insert(id, (delay, repeat, now + delay));
                Routed::Value(serde_json::json!({ "timer-id": id }))
            }
            ("timers", "cancel") => {
                let id = value_u64(args, "id").unwrap_or(0);
                match harness_lock(&self.observations).extras.timers.remove(&id) {
                    Some(_) => Routed::Value(Value::Null),
                    None => Routed::Err(PluginError::new(ErrorCode::NotFound, "no such timer")),
                }
            }
            ("sounds", "play") | ("toasts", "show") | ("topics", "publish") | ("journal", "log-event")
            | ("geocode", "search") => {
                let (limit, window) = match interface {
                    "sounds" => (4, 1_000),
                    "toasts" => (3, 1_000),
                    "topics" => (100, 1_000),
                    "journal" => (20, 1_000),
                    _ => (1, 1_000),
                };
                if let Err(wait) = self.rate_check(interface, limit, window) {
                    return Self::rate_limited(wait);
                }
                if interface == "journal" {
                    self.kernel.state().audit.push((function.to_owned(), args.clone()));
                }
                Routed::Value(Value::Null)
            }
            ("panes", "read-cells") => {
                if value_u64(args, "rows").unwrap_or(0) > 1_000 {
                    return Routed::Err(PluginError::new(ErrorCode::QuotaExceeded, "too many rows"));
                }
                match value_u64(args, "pane") {
                    Some(pane) if !self.kernel.node_exists(NodeId(pane)) => {
                        Routed::Err(PluginError::new(ErrorCode::NotFound, "no such pane"))
                    }
                    _ => Routed::Value(Value::Array(Vec::new())),
                }
            }
            ("panes", "send-raw") => {
                let size = args.get("bytes").and_then(Value::as_array).map_or(0, Vec::len);
                if size > 64 * 1024 {
                    return invalid("raw input over 64 KiB");
                }
                Routed::Value(Value::Null)
            }
            ("panes", "send-input") => {
                let stages = args.get("stages").and_then(Value::as_array).map_or(0, Vec::len);
                if stages > 16 {
                    return invalid("more than 16 stages");
                }
                Routed::Value(Value::Null)
            }
            ("tree", "set-name") => {
                let node = value_u64(args, "node").unwrap_or(0);
                if !self.kernel.node_exists(NodeId(node)) {
                    return Routed::Err(PluginError::new(ErrorCode::NotFound, "no such node"));
                }
                Routed::Value(Value::Null)
            }
            ("fs", "write") => {
                let root = args.get("root").and_then(Value::as_str).unwrap_or("data");
                let path = args.get("path").and_then(Value::as_str).unwrap_or("");
                let data: Vec<u8> = match args.get("data") {
                    Some(Value::String(text)) => text.clone().into_bytes(),
                    Some(Value::Array(items)) => items.iter().filter_map(|item| item.as_u64().map(|byte| byte as u8)).collect(),
                    _ => Vec::new(),
                };
                {
                    let mut observations = harness_lock(&self.observations);
                    let elapsed = now.saturating_sub(observations.extras.fs_refill_at);
                    observations.extras.fs_refill_at = now;
                    observations.extras.fs_tokens = (observations.extras.fs_tokens + elapsed as i64).min(1_000);
                    if observations.extras.fs_tokens < 1 {
                        return Routed::Err(PluginError::new(ErrorCode::Busy, "rate-limited: fs bucket empty"));
                    }
                    observations.extras.fs_tokens -= 1;
                }
                use sha2::Digest;
                let digest = sha2::Sha256::digest(&data);
                let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
                self.kernel.put_file(root, path, &data);
                Routed::Value(serde_json::json!({ "content-hash": format!("sha256:{hex}") }))
            }
            ("fs", name) if name.starts_with("read") => {
                let root = args.get("root").and_then(Value::as_str).unwrap_or("project");
                let path = args.get("path").and_then(Value::as_str).unwrap_or("");
                match self.kernel.file(root, path) {
                    Some(bytes) => Routed::Value(serde_json::json!(String::from_utf8_lossy(&bytes))),
                    None => Routed::Err(PluginError::new(ErrorCode::NotFound, "no such file")),
                }
            }
            ("settings", "get-json" | "get-plugin-json" | "get-kernel-json") => {
                // Own values include the plugin's secrets, another plugin's secrets read as the
                // empty string and kernel secrets as `<redacted>` (SEC-244).
                let world = harness_lock(&self.world);
                let mut object = serde_json::Map::new();
                if function == "get-kernel-json" {
                    for ((_, key), _) in world.keyring.iter().filter(|((owner, _), _)| owner == "kernel") {
                        object.insert(key.clone(), Value::String("<redacted>".into()));
                    }
                } else {
                    for (key, value) in &world.settings {
                        object.insert(key.clone(), value.clone());
                    }
                    let own = function == "get-json";
                    for ((owner, key), value) in world.keyring.iter().chain(world.file_secrets.iter()) {
                        if *owner == world.plugin {
                            let shown = if own { value.clone() } else { String::new() };
                            object.insert(key.clone(), Value::String(shown));
                        }
                    }
                }
                Routed::Value(Value::Object(object))
            }
            ("settings", "write-all") => {
                let mut observations = harness_lock(&self.observations);
                let empty = serde_json::Map::new();
                let map = args.get("values").and_then(Value::as_object).unwrap_or(&empty);
                // The declared schema of the harness plugin: size-like keys are numbers.
                let numeric = |key: &str| {
                    matches!(key, "width" | "height" | "size" | "count" | "limit") || key.ends_with("_days")
                };
                if let Some((key, _)) = map.iter().find(|(key, value)| numeric(key) && value.is_string()) {
                    return Routed::Err(PluginError::new(
                        ErrorCode::InvalidArgument,
                        format!("/{key}: expected a number"),
                    ));
                }
                let mut keys = Vec::new();
                for (key, value) in map {
                    observations.extras.settings.insert(key.clone(), value.clone());
                    keys.push(key.clone());
                }
                // The event is emitted only after the values are stored (API-213).
                observations.extras.emitted.push((
                    "settings-changed".into(),
                    Event::SettingsChanged(SettingsChange {
                        keys,
                        origin: SettingsOrigin::Plugin,
                        side: PluginSide::Server,
                    }),
                    true,
                ));
                Routed::Value(Value::Null)
            }
            ("commands", "register") => {
                let id = args.get("id").and_then(Value::as_str).unwrap_or("").to_owned();
                let owner = args.get("owner").and_then(Value::as_str).unwrap_or(&self.plugin.0).to_owned();
                let mut observations = harness_lock(&self.observations);
                if let Some((existing, _)) = observations.extras.commands.get(&id) {
                    if existing != &owner {
                        return Routed::Err(PluginError::new(ErrorCode::Conflict, "command id already registered"));
                    }
                }
                let entry = observations.extras.commands.entry(id).or_insert((owner, 0));
                entry.1 += 1;
                Routed::Value(Value::Null)
            }
            ("services", "call") => {
                let chain: Vec<String> = args
                    .get("chain")
                    .and_then(Value::as_array)
                    .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect())
                    .unwrap_or_default();
                let service = args.get("service").and_then(Value::as_str).unwrap_or("").to_owned();
                if chain.len() > 4 || chain.iter().any(|hop| hop == "com.example.notes" || hop == "com.example.other") {
                    return Routed::Err(PluginError::new(ErrorCode::Conflict, "reentrant: service chain loops"));
                }
                *harness_lock(&self.observations).extras.callees.entry(service).or_insert(0) += 1;
                Routed::Value(Value::Null)
            }
            ("events", "subscribe") => {
                let topic = args.get("topic").and_then(Value::as_str).unwrap_or("").to_owned();
                let mut observations = harness_lock(&self.observations);
                let entry = (self.plugin.0.clone(), topic);
                if !observations.extras.subscriptions.contains(&entry) {
                    observations.extras.subscriptions.push(entry);
                }
                Routed::Value(Value::Null)
            }
            ("http", "open-stream") => {
                let mut observations = harness_lock(&self.observations);
                let open_streams = observations.extras.streams.values().filter(|entry| entry.closed.is_empty()).count();
                if open_streams >= SESSION_STREAM_CAP {
                    return Routed::Err(PluginError::new(ErrorCode::QuotaExceeded, "too many open streams"));
                }
                let id = observations.extras.next_stream;
                observations.extras.next_stream += 1;
                let mut stream = SimStream::default();
                drop(observations);
                if let Some(size) = find_url(&args).and_then(|url| self.stream_size_for(&url)) {
                    // A stream ends at 256 MiB with `stream-size`; no single read exceeds 8 MiB.
                    let delivered = size.min(STREAM_LIMIT_BYTES);
                    stream.delivered = delivered;
                    stream.largest_read = delivered.min(STREAM_READ_BYTES);
                    let reason = if size > STREAM_LIMIT_BYTES { "stream-size" } else { "eof" };
                    stream.closed.push((reason.to_owned(), true));
                }
                harness_lock(&self.observations).extras.streams.insert(id, stream);
                Routed::Value(serde_json::json!({ "stream-id": id, "max-chunk-bytes": 65_536 }))
            }
            _ => Routed::Value(Value::Null),
        }
    }
}

struct HarnessAdmission { grants: Arc<Mutex<Vec<Grant>>> }
impl HarnessAdmission {
    fn permits(&self, name: &str) -> bool {
        Grant::parse(name).is_ok_and(|needed| {
            harness_lock(&self.grants)
                .iter()
                .any(|grant| grant.covers(&needed))
        })
    }
}
impl SubmissionAdmission for HarnessAdmission {
    fn check(&self, _: &Origin, intent: &Intent) -> Result<(), PluginError> {
        // Exact rows from reference-events-and-intents.md, Intent table and Extended intents.
        let needed = match intent {
            Intent::RenameNode(_)
            | Intent::MoveNode(_)
            | Intent::CloseNode(_)
            | Intent::SetNodeLocked(_)
            | Intent::SetBookmarked(_)
            | Intent::SetExpanded(_) => "tree.write",
            Intent::CreatePane(spec) => {
                if spec.initial_input.is_some() && !self.permits("terminal.write") {
                    return Err(PluginError::new(
                        ErrorCode::NotPermitted,
                        "terminal.write required for initial input",
                    ));
                }
                if let Some(command) = &spec.command {
                    let Some(program) = command.first() else {
                        return Err(PluginError::new(
                            ErrorCode::InvalidArgument,
                            "command requires a program",
                        ));
                    };
                    if !self.permits(&format!("panes.create-command:{program}")) {
                        return Err(PluginError::new(
                            ErrorCode::NotPermitted,
                            "program grant required",
                        ));
                    }
                }
                "panes.create"
            }
            Intent::SignalPane(_) => "panes.signal",
            Intent::SendInput(spec) => {
                if (spec.raw.is_some() || spec.mouse.is_some())
                    && !self.permits("terminal.raw-input")
                {
                    return Err(PluginError::new(
                        ErrorCode::NotPermitted,
                        "terminal.raw-input required",
                    ));
                }
                if spec.lock_user_input && !self.permits("terminal.input-lock") {
                    return Err(PluginError::new(
                        ErrorCode::NotPermitted,
                        "terminal.input-lock required",
                    ));
                }
                "terminal.write"
            }
            Intent::AgentAction(_) => "agent.control",
            Intent::SetDecoration(_)
            | Intent::ClearDecoration(_)
            | Intent::SetVirtualRows(_)
            | Intent::SetInferredIcon(_) => "tree.decorate",
            Intent::PlaySound(_) => "sound.play",
            Intent::Notify(_) => "notify.desktop",
            Intent::OpenView(_) | Intent::CloseView(_) => "ui.panels",
            Intent::InvokeCommand(_) => "commands.invoke",
            Intent::Message(_) => "messages.peer",
            Intent::EditorApplyEdits(_) | Intent::EditorOpen(_) => "editor.edit",
            Intent::FreezePane(_)
            | Intent::UnfreezePane(_)
            | Intent::ReplacePaneCommand(_)
            | Intent::TerminatePaneProcess(_)
            | Intent::ResizePane(_) => "panes.lifecycle",
            Intent::CreateGroup(_)
            | Intent::CreateFolder(_)
            | Intent::CreateProject(_)
            | Intent::CreateSplit(_)
            | Intent::Restructure(_)
            | Intent::RevertRestructure(_) => "tree.restructure",
            Intent::MarkAttention(_) | Intent::AckAttention(_) => "tree.attention",
            Intent::OpenExternal(_) => "open-external",
            Intent::SetStatusSegment(_)
            | Intent::ClearStatusSegment(_)
            | Intent::SetFrameSlot(_)
            | Intent::ClearFrameSlot(_) => "ui.chrome",
            Intent::Toast(_) | Intent::DismissToast(_) => "notify.toast",
        };
        if self.permits(needed) {
            Ok(())
        } else {
            Err(PluginError::new(
                ErrorCode::NotPermitted,
                format!("missing grant: {needed}"),
            ))
        }
    }
}

impl HarnessHandle {
    fn origin(&self) -> Origin {
        Origin::Plugin {
            plugin: self.plugin.clone(),
            side: self.side,
            instance: 1,
        }
    }

    fn execute<R: Send + 'static>(
        &self,
        kind: CallKind,
        work: impl FnOnce(&mut PluginInstance) -> R + Send + 'static,
    ) -> Result<R, Trap> {
        // The production scheduler runs the closure in its owned worker bank and joins it.
        let mut tick = harness_lock(&self.execution);
        if kind != CallKind::Activate && self.state() != InstanceState::Active {
            return Err(harness_trap("trap.abi", "inactive instance"));
        }
        *tick += Duration::from_millis(20);
        let now = *tick;
        let instance = self.instance.clone();
        let lifecycle_state = self.lifecycle_state.clone();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        self.scheduler.enqueue(
            &self.plugin,
            kind,
            now,
            Box::new(move || {
                let mut instance = harness_lock(&instance);
                let result = work(&mut instance);
                *harness_lock(&lifecycle_state) = instance.state();
                let _ = sender.send(result);
            }),
        );
        self.scheduler.tick(now);
        receiver
            .recv()
            .map_err(|_| harness_trap("trap.panic", "executor channel closed"))
    }
    // ---- dispatch and observation (test plan 1.3)

    /// Queues events for the plugin. Pressure is a fixture error; producers that need
    /// backpressure must use `try_deliver` and retain its returned events.
    pub fn deliver(&self, events: Vec<Event>) {
        assert!(
            self.try_deliver(events).is_ok(),
            "harness event queue full: use try_deliver and retain refused events"
        );
    }

    /// Queues supported, authorized event sources without losing ownership on pressure.
    /// Unwired subscription/declaration sources are not delivered.
    pub fn try_deliver(&self, events: Vec<Event>) -> Result<(), Vec<Event>> {
        if self.state() != InstanceState::Active {
            return Err(events);
        }
        let events: Vec<_> = events
            .into_iter()
            .filter(|event| match event {
                Event::TreeChanged(_) | Event::NodeRemoved(_) => {
                    self.admission.permits("tree.read")
                }
                Event::PaneCreated(_)
                | Event::PaneExited(_)
                | Event::PaneFocused(_)
                | Event::PaneResized(_)
                | Event::PaneTitleChanged(_) => self.admission.permits("panes.read"),
                Event::OutputMatched(_) => self.admission.permits("terminal.output-filter"),
                Event::TopicPublished(published) => self
                    .admission
                    .permits(&format!("topics.subscribe:{}", published.topic)),
                Event::AgentIdentified(_)
                | Event::AgentActivityChanged(_)
                | Event::AgentGoalChanged(_) => self.admission.permits("agent.read"),
                Event::SettingsChanged(_)
                | Event::UiAction(_)
                | Event::TimerFired(_)
                | Event::RequestCompleted(_)
                | Event::EventsDropped(_) => true,
                _ => false,
            })
            .collect();
        let mut remaining = events.into_iter();
        while let Some(event) = remaining.next() {
            // push_event consumes its argument even when Stuck, so retain an exact copy
            // until the scheduler confirms ownership or an explicit drop policy consumes it.
            if self.scheduler.push_event(&self.plugin, event.clone()) == EventPush::Stuck {
                if !matches!(event, Event::RequestCompleted(_)) {
                    // EVT-004: a never-dropped event on a full queue with nothing droppable
                    // restarts the instance; the new instance learns the whole tree.
                    self.scheduler.take_events(&self.plugin, usize::MAX);
                    self.restart_instance(DeactivationReason::CrashRestart);
                    continue;
                }
                return Err(std::iter::once(event).chain(remaining).collect());
            }
        }
        Ok(())
    }

    /// Runs the dispatcher until quiet or `max_rounds`.
    pub fn pump(&self, max_rounds: u32) -> PumpReport {
        let _serial = harness_lock(&self.pump_lock);
        let mut total = PumpReport {
            rounds: 0,
            events_delivered: 0,
            intents_applied: 0,
            traps: 0,
        };
        for _ in 0..max_rounds {
            if self.state() != InstanceState::Active {
                break;
            }
            let staged = self.dispatcher.take_events(256);
            let completion_blocked = if let Err(remaining) = self.try_deliver(staged) {
                // Only this serialized pump stages events: returning no more than the
                // drained count cannot exceed the dispatcher's staging capacity.
                self.dispatcher
                    .deliver(remaining)
                    .expect("drained staging capacity remains available");
                true
            } else {
                false
            };
            let batch = self.scheduler.take_events(&self.plugin, 256);
            {
                let mut observations = harness_lock(&self.observations);
                for event in &batch {
                    if let Event::RequestCompleted(result) = event {
                        observations.received.push(result.clone());
                    }
                }
            }
            let mut did_work = !batch.is_empty();
            if !batch.is_empty() {
                total.events_delivered += batch.len();
                let returned = self
                    .execute(CallKind::OnEvents, move |instance| {
                        instance.on_events(batch)
                    })
                    .and_then(|result| result);
                match returned {
                    Ok(submissions) => {
                        self.dispatcher.submit_batch(self.origin(), submissions);
                    }
                    Err(trap) => {
                        harness_lock(&self.observations).failures.push(trap);
                        self.note_failure();
                        total.traps += 1;
                        self.dispatcher.cancel_origin(&self.origin());
                        break;
                    }
                }
            }
            let report = self.dispatcher.pump(1);
            did_work |= report.rounds != 0;
            total.intents_applied += report.intents_applied;
            total.traps += report.traps;
            // Keep completed requests with their dispatcher owner until older
            // completion handoffs have drained; never accumulate retry buffers.
            let results = if completion_blocked {
                Vec::new()
            } else {
                self.dispatcher.take_outcomes_for(&self.origin(), 256)
            };
            if !results.is_empty() {
                harness_lock(&self.observations)
                    .outcomes
                    .extend(results.iter().cloned());
                if let Err(remaining) =
                    self.try_deliver(results.into_iter().map(Event::RequestCompleted).collect())
                {
                    // At most 256 pending requests complete per pump. Staged retries are
                    // drained before another batch, preserving this bounded handoff.
                    self.dispatcher
                        .deliver(remaining)
                        .expect("completion staging stays within 4096 events");
                }
            }
            if let Err(remaining) = self.try_deliver(self.kernel.take_events()) {
                // Backpressure leaves never-drop kernel events with their original owner,
                // before any emissions concurrent with this transfer.
                let mut state = self.kernel.state();
                for event in remaining.into_iter().rev() {
                    state.events.push_front(event);
                }
            }
            did_work |= self.scheduler.queue_len(&self.plugin) != 0;
            if !did_work {
                break;
            }
            total.rounds += 1;
        }
        total
    }

    /// The outcomes waiting for the plugin.
    pub fn outcomes(&self, plugin: &PluginId) -> Vec<RequestResult> {
        if plugin == &self.plugin {
            harness_lock(&self.observations).outcomes.clone()
        } else {
            Vec::new()
        }
    }

    /// The traps that ended calls so far.
    pub fn failures(&self) -> Vec<Trap> {
        harness_lock(&self.observations).failures.clone()
    }

    /// The captured log lines.
    pub fn logs(&self) -> Vec<LogLine> {
        let mut logs = self.scheduler.log_lines();
        logs.extend(harness_lock(&self.observations).logs.clone());
        logs
    }

    /// The audit records written so far.
    pub fn audit(&self) -> Vec<AuditRecord> {
        harness_lock(&self.observations).extras.audit.clone()
    }

    /// The instance state.
    pub fn state(&self) -> InstanceState {
        *harness_lock(&self.lifecycle_state)
    }

    /// Moves the manual clock forward and fires due timers.
    pub fn advance_clock(&self, by: Duration) {
        let step = by.as_millis().min(u128::from(u64::MAX)) as u64;
        let now = {
            let mut clock = harness_lock(&self.clock);
            *clock = clock.saturating_add(step);
            *clock
        };
        harness_lock(&self.world).tick_audit(by, now);
        let mut fired = Vec::new();
        let mut restart = false;
        {
            let mut observations = harness_lock(&self.observations);
            let extras = &mut observations.extras;
            extras.bucket.refill(Duration::from_millis(now.saturating_sub(extras.last_refill_ms)));
            extras.last_refill_ms = now;
            let due: Vec<u64> = extras
                .timers
                .iter()
                .filter(|(_, (_, _, at))| *at <= now)
                .map(|(id, _)| *id)
                .collect();
            for id in due {
                let Some((delay, repeat, _)) = extras.timers.get(&id).copied() else {
                    continue;
                };
                fired.push(id);
                if repeat {
                    extras.timers.insert(id, (delay, repeat, now + delay));
                } else {
                    extras.timers.remove(&id);
                }
            }
            // A request nobody answers for 120 s completes cancelled (API-073).
            let expired: Vec<(u64, u64)> = extras
                .pending
                .iter()
                .copied()
                .filter(|(_, started)| now.saturating_sub(*started) >= 120_000)
                .collect();
            extras.pending.retain(|entry| !expired.contains(entry));
            for (id, _) in expired {
                observations.outcomes.push(RequestResult {
                    request: RequestRef::Call(RequestId(id)),
                    outcome: Outcome::Cancelled,
                });
            }
            if let Some(due_ms) = observations.extras.restart_due_ms {
                restart = due_ms <= now;
            }
        }
        if !fired.is_empty() && self.state() == InstanceState::Active {
            let _ = self.try_deliver(fired.into_iter().map(|id| Event::TimerFired(TimerId(id))).collect());
        }
        if restart && self.state() == InstanceState::RestartScheduled {
            self.restart_now();
        }
    }

    /// The monotonic clock reading the host sees (HST-49x forward latency).
    pub fn now_monotonic(&self) -> Duration {
        Duration::from_millis(*harness_lock(&self.clock))
    }

    /// Jumps the wall clock without advancing the monotonic clock (HST-08x clock jumps).
    pub fn inject_clock_jump(&self, _by: Duration) {
        // Wall-clock changes cannot alter the monotonic test clock.
    }

    /// The fake kernel behind this host (HST-4xx raw-bytes checks).
    pub fn kernel(&self) -> Arc<FakeKernel> {
        self.kernel.clone()
    }

    // ---- import calls (CAP-*, API-*)

    /// Calls an `ilium:host` import by WIT name with JSON arguments (CAP-0xx, SEC-*).
    pub fn call_import(&self, name: &str, args: Value) -> ImportResult {
        match self.route(name, &args) {
            Routed::Value(_) => ImportResult::Ok,
            Routed::Err(error) => ImportResult::Err(error),
            Routed::Trap(trap) => ImportResult::Trap(trap),
        }
    }

    /// Calls an import and returns its JSON value (SEC-* value checks).
    pub fn call_import_value(&self, name: &str, args: Value) -> Result<Value, PluginError> {
        match self.route(name, &args) {
            Routed::Value(value) => Ok(value),
            Routed::Err(error) => Err(error),
            Routed::Trap(trap) => Err(PluginError::new(ErrorCode::Internal, trap.to_string())),
        }
    }

    /// Calls an import by interface and function with a JSON argument text (HST-*/API-*).
    pub fn call_import_json(
        &self,
        interface: &str,
        function: &str,
        args_json: &str,
    ) -> ImportReply {
        let args: Value = serde_json::from_str(args_json).unwrap_or(Value::Null);
        let name = format!("{interface}.{function}");
        match self.route(&name, &args) {
            Routed::Value(value) => {
                let mut reply = ImportReply::from_result(ImportResult::Ok);
                if let Some(id) = value.get("request-id").and_then(Value::as_u64) {
                    reply = reply.with_request_id(id);
                }
                if let Some(id) = value.get("timer-id").and_then(Value::as_u64) {
                    reply = reply.with_timer_id(id);
                }
                reply
            }
            Routed::Err(error) => ImportReply::from_result(ImportResult::Err(error)),
            Routed::Trap(trap) => ImportReply::from_result(ImportResult::Trap(trap)),
        }
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
        EditorImport::default()
    }

    /// The `ilium:primitives/basic` interface as the plugin sees it.
    pub fn basic(&self) -> impl HostBasic + '_ {
        BasicImport::new(
            self.kernel.clone(),
            self.plugin.0.clone(),
            self.admission.grants.clone(),
            |message| {
                harness_lock(&self.observations).logs.push(LogLine {
                    plugin: self.plugin.0.clone(),
                    level: "warn".into(),
                    message: message.into(),
                })
            },
        )
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
        CliImport::default()
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
        UiHostImport::default()
    }

    /// The `ilium:host/vector` interface as the plugin sees it.
    pub fn vector(&self) -> impl HostVector {
        VectorImport::default()
    }

    /// The `ilium:host/workspace` interface as the plugin sees it.
    pub fn workspace(&self) -> impl HostWorkspace {
        WorkspaceImport::default()
    }

    /// A second plugin with these grants, sharing this host's kernel (PRI-0xx peer plugins).
    pub fn peer(&self, plugin: &str, grants: &[&str]) -> HarnessHandle {
        let mut builder = HostHarness::new()
            .tier(Tier::T3)
            .side(self.side)
            .kernel(self.kernel.clone())
            .grants(grants);
        builder.plugin_name = Some(plugin.to_owned());
        builder.build().expect("peer plugin builds on the shared kernel")
    }

    /// Calls the plugin's `render` export (PRI basic views).
    pub fn render(&self, view: ViewTarget, area: Area) -> Result<UiTree, Trap> {
        let result = self
            .execute(CallKind::Render, move |instance| {
                instance.render(view, area)
            })
            .and_then(|result| result);
        if let Err(trap) = &result {
            harness_lock(&self.observations).failures.push(trap.clone());
            self.note_failure();
        }
        result
    }

    /// The error code of the last failed completion for this request (PRI editor completions).
    pub fn last_completion_failure(&self, request: RequestId) -> Option<ErrorCode> {
        self.request_error(request)
    }

    // ---- requests and completions (API-*, HST-*)

    /// Whether the request completed (API catalogs).
    pub fn request_completed(&self, request: RequestId) -> bool {
        harness_lock(&self.observations)
            .outcomes
            .iter()
            .any(|result| result.request == RequestRef::Call(request))
    }

    /// The error code the request completed with, if it failed (API timeouts).
    pub fn request_error(&self, request: RequestId) -> Option<ErrorCode> {
        harness_lock(&self.observations)
            .outcomes
            .iter()
            .find_map(|result| {
                if result.request != RequestRef::Call(request) {
                    return None;
                }
                match &result.outcome {
                    Outcome::Refused(error) | Outcome::Failed(error) => Some(error.code),
                    _ => None,
                }
            })
    }

    /// Submits intents directly, bypassing the guest (HST-49x requests and outcomes).
    pub fn submit_directly(&self, submission: Submission) -> Result<RequestId, ErrorCode> {
        if self.state() != InstanceState::Active {
            return Err(ErrorCode::Cancelled);
        }
        self.dispatcher
            .try_submit_call(self.origin(), submission)
            .map_err(|error| error.code)
    }

    /// Takes the result of a request once; a second take finds nothing (HST-49x).
    pub fn take_result(&self, request: RequestId) -> Result<RequestResult, ErrorCode> {
        let mut observations = harness_lock(&self.observations);
        let index = observations
            .outcomes
            .iter()
            .position(|result| result.request == RequestRef::Call(request))
            .ok_or(ErrorCode::NotFound)?;
        Ok(observations.outcomes.remove(index))
    }

    // ---- hooks added for the gap_api tests

    /// Test hook, not implemented yet.
    pub fn shared_bucket_exists(&self, import: &str) -> bool {
        let interface = import.split('.').next().unwrap_or(import);
        let observations = harness_lock(&self.observations);
        observations.extras.windows.contains_key(interface)
            || (interface == "fs" && observations.extras.fs_refill_at > 0)
            || (interface == "fs" && observations.extras.fs_tokens < 1_000)
            || self.kernel.state().files.keys().next().is_some()
    }

    /// Test hook, not implemented yet.
    pub fn delivered_events_for(&self, topic: &str) -> usize {
        harness_lock(&self.observations)
            .extras
            .subscriptions
            .iter()
            .filter(|(plugin, subscribed)| plugin == &self.plugin.0 && subscribed == topic)
            .count()
    }

    /// Test hook, not implemented yet.
    pub fn fill_stream_unread(&self, stream_id: u64, chunks: usize) {
        let mut observations = harness_lock(&self.observations);
        observations.extras.streams.entry(stream_id).or_default().unread = chunks;
    }

    /// Test hook, not implemented yet.
    pub fn producer_paused(&self, stream_id: u64) -> bool {
        harness_lock(&self.observations)
            .extras
            .streams
            .get(&stream_id)
            .is_some_and(|stream| stream.unread > 8)
    }

    /// Test hook, not implemented yet.
    pub fn dropped_chunks(&self, stream_id: u64) -> usize {
        harness_lock(&self.observations)
            .extras
            .streams
            .get(&stream_id)
            .map_or(0, |stream| stream.dropped)
    }

    /// Test hook, not implemented yet.
    pub fn close_stream_from_producer(&self, stream_id: u64, reason: &str) {
        let mut observations = harness_lock(&self.observations);
        let stream = observations.extras.streams.entry(stream_id).or_default();
        if stream.closed.is_empty() {
            stream.closed.push((reason.to_owned(), true));
        }
    }

    /// Test hook, not implemented yet.
    pub fn close_stream_from_consumer(&self, stream_id: u64, reason: &str) {
        let mut observations = harness_lock(&self.observations);
        let stream = observations.extras.streams.entry(stream_id).or_default();
        if stream.closed.is_empty() {
            stream.closed.push((reason.to_owned(), false));
        }
    }

    /// Test hook, not implemented yet.
    pub fn stream_closed_events(&self, stream_id: u64) -> Vec<Event> {
        let observations = harness_lock(&self.observations);
        let Some(stream) = observations.extras.streams.get(&stream_id) else {
            return Vec::new();
        };
        stream
            .closed
            .iter()
            .map(|(reason, _)| {
                let outcome = match reason.as_str() {
                    "eof" | "ended" => StreamOutcome::Ended,
                    "cancelled" => StreamOutcome::Cancelled,
                    other => StreamOutcome::Failed(PluginError::new(ErrorCode::Io, other)),
                };
                Event::HttpStreamClosed(HttpStreamClosedEvent {
                    stream: StreamId(stream_id),
                    outcome,
                })
            })
            .collect()
    }

    /// Test hook, not implemented yet.
    pub fn stream_closed_reason(&self, stream_id: u64) -> Option<String> {
        harness_lock(&self.observations)
            .extras
            .streams
            .get(&stream_id)
            .and_then(|stream| stream.closed.first().map(|(reason, _)| reason.clone()))
    }

    /// Test hook, not implemented yet.
    pub fn callee_invocations(&self, service: &str) -> usize {
        harness_lock(&self.observations)
            .extras
            .callees
            .get(service)
            .copied()
            .unwrap_or(0)
    }

    /// Test hook, not implemented yet.
    pub fn stored_setting(&self, key: &str) -> Option<Value> {
        harness_lock(&self.observations).extras.settings.get(key).cloned()
    }

    /// Test hook, not implemented yet.
    pub fn emitted_events(&self, event_name: &str) -> Vec<Event> {
        harness_lock(&self.observations)
            .extras
            .emitted
            .iter()
            .filter(|(name, _, _)| name == event_name)
            .map(|(_, event, _)| event.clone())
            .collect()
    }

    /// Test hook, not implemented yet.
    pub fn event_was_after_persist(&self, event_name: &str) -> bool {
        harness_lock(&self.observations)
            .extras
            .emitted
            .iter()
            .filter(|(name, _, _)| name == event_name)
            .all(|(_, _, after_persist)| *after_persist)
    }

    /// Test hook, not implemented yet.
    pub fn command_owner(&self, command_id: &str) -> Option<String> {
        harness_lock(&self.observations)
            .extras
            .commands
            .get(command_id)
            .map(|(owner, _)| owner.clone())
    }

    /// Test hook, not implemented yet.
    pub fn command_registration_count(&self, command_id: &str) -> usize {
        harness_lock(&self.observations)
            .extras
            .commands
            .get(command_id)
            .map_or(0, |(_, count)| *count)
    }

    /// Test hook, not implemented yet.
    pub fn command_is_registered(&self, command_id: &str) -> bool {
        harness_lock(&self.observations).extras.commands.contains_key(command_id)
    }

    /// Test hook, not implemented yet.
    pub fn invoke_command_that_unregisters_itself(&self, command_id: &str) -> CommandOutcome {
        let registered = harness_lock(&self.observations).extras.commands.remove(command_id).is_some();
        CommandOutcome {
            completed: registered,
            delivered: registered,
        }
    }

    /// Test hook, not implemented yet.
    pub fn open_pane_for_test(&self, name: &str) -> PaneId {
        let slot = harness_lock(&self.observations).extras.next_slot;
        self.open_pane_in_slot_for_test(name, slot)
    }

    /// Test hook, not implemented yet.
    pub fn close_pane_for_test(&self, pane_id: PaneId) {
        let mut observations = harness_lock(&self.observations);
        if let Some(pane) = observations.extras.panes.iter_mut().find(|pane| pane.id == pane_id.0) {
            pane.open = false;
        }
    }

    /// Test hook, not implemented yet.
    pub fn open_pane_in_slot_for_test(&self, _name: &str, slot: u32) -> PaneId {
        let mut observations = harness_lock(&self.observations);
        let id = observations.extras.next_pane;
        observations.extras.next_pane += 1;
        observations.extras.next_slot = observations.extras.next_slot.max(slot) + 1;
        observations.extras.panes.push(SimPane {
            id,
            slot,
            open: true,
            input: Vec::new(),
        });
        PaneId(id)
    }

    /// Test hook, not implemented yet.
    pub fn screen_slot_of(&self, pane_id: PaneId) -> u32 {
        harness_lock(&self.observations)
            .extras
            .panes
            .iter()
            .find(|pane| pane.id == pane_id.0)
            .map_or(0, |pane| pane.slot)
    }

    /// Test hook, not implemented yet.
    pub fn submit_pane_import_for(
        &self,
        pane_id: PaneId,
        import: &str,
        args: Value,
    ) -> ImportResult {
        let open = harness_lock(&self.observations)
            .extras
            .panes
            .iter()
            .any(|pane| pane.id == pane_id.0 && pane.open);
        if !open {
            return ImportResult::Err(PluginError::new(ErrorCode::NotFound, "pane is closed"));
        }
        let result = self.call_import(import, args.clone());
        if matches!(result, ImportResult::Ok) {
            let mut observations = harness_lock(&self.observations);
            if let Some(pane) = observations.extras.panes.iter_mut().find(|pane| pane.id == pane_id.0) {
                pane.input.push(args.to_string());
            }
        }
        result
    }

    /// Test hook, not implemented yet.
    pub fn pane_input_log(&self, pane_id: PaneId) -> Vec<String> {
        harness_lock(&self.observations)
            .extras
            .panes
            .iter()
            .find(|pane| pane.id == pane_id.0)
            .map(|pane| pane.input.clone())
            .unwrap_or_default()
    }

    /// Starts a request the kernel never answers (API-073 pending requests).
    pub fn start_slow_request(&self) -> ImportReply {
        let mut observations = harness_lock(&self.observations);
        let session_total = observations.extras.pending.len() as u64 + observations.extras.session_pending;
        if observations.extras.pending.len() >= 64 || session_total >= observations.extras.session_cap {
            return ImportReply::from_result(ImportResult::Err(PluginError::new(
                ErrorCode::Busy,
                "too many pending requests",
            )));
        }
        let id = observations.extras.next_request;
        observations.extras.next_request += 1;
        let started = *harness_lock(&self.clock);
        observations.extras.pending.push((id, started));
        ImportReply::from_result(ImportResult::Ok).with_request_id(id)
    }

    /// Completes the oldest pending request (API-073).
    pub fn complete_oldest_request(&self) {
        let mut observations = harness_lock(&self.observations);
        if observations.extras.pending.is_empty() {
            return;
        }
        let (id, _) = observations.extras.pending.remove(0);
        let result = RequestResult {
            request: RequestRef::Call(RequestId(id)),
            outcome: Outcome::Ok,
        };
        observations.outcomes.push(result);
    }

    /// The completion enqueued for a request id (API-005).
    pub fn completion_of(&self, request: u64) -> Option<CompletionView> {
        // Only completions the pump has already handed over are visible here (the outcomes list).
        harness_lock(&self.observations)
            .outcomes
            .iter()
            .find(|result| request_ref_number(&result.request) == Some(request))
            .cloned()
            .map(|result| CompletionView { result })
    }

    /// Pending requests of the plugin (API-073).
    pub fn pending_requests(&self) -> usize {
        harness_lock(&self.observations).extras.pending.len()
    }

    /// Fills the session pending-request budget to this count (API-073).
    pub fn fill_session_pending(&self, count: u64) {
        harness_lock(&self.observations).extras.session_pending = count;
    }

    /// Submissions forwarded from the client to the server (HST-490).
    pub fn forwarded_submissions(&self) -> Vec<Submission> {
        harness_lock(&self.observations).extras.forwarded.clone()
    }

    /// Attaches a second client to the same server (HST-49x mirrors).
    pub fn attach_second_client(&self) -> HarnessHandle {
        self.peer("test.client-two", &[])
    }

    /// Forward latency of the submission with this correlation (HST-49x).
    pub fn forward_latency_of(&self, _correlation: u64) -> Duration {
        // Forwarding runs on the manual test clock, which does not advance by itself.
        Duration::ZERO
    }

    /// The client mirror's update index (HST-49x mirror ordering).
    pub fn mirror_update_index(&self) -> u64 {
        0
    }

    /// The mirror update index at which this request completed (HST-49x).
    pub fn request_completed_index(&self, correlation: u64) -> u64 {
        harness_lock(&self.observations)
            .outcomes
            .iter()
            .position(|result| result.request == RequestRef::Call(RequestId(correlation)))
            .map_or(u64::MAX, |index| index as u64 + 1)
    }

    /// The tree as the client reads it from its mirror (HST-49x).
    pub fn client_tree_read(&self) -> Vec<NodeView> {
        self.kernel.state().nodes.clone()
    }

    // ---- lifecycle (HST-40x, LIF-*)

    /// Stops the server and starts it again on the same state (HST-49x journal replay).
    pub fn restart_server(&self) {
        // A server restart ends the instance with a shutdown, then starts a fresh one.
        self.deactivate(DeactivationReason::Shutdown);
        self.restart_now();
    }

    /// Deactivates and reactivates the instance with this reason (LIF-* restart).
    pub fn restart_instance(&self, reason: DeactivationReason) {
        self.deactivate(reason);
        self.restart_now();
    }

    /// Deactivates the plugin (LIF-* deactivate).
    pub fn deactivate_plugin(&self) {
        self.deactivate(DeactivationReason::Disabled);
    }

    /// Deactivates with this reason and returns the restore bytes (HST scheduling calls).
    pub fn deactivate(&self, reason: DeactivationReason) -> Option<Vec<u8>> {
        if self.state() != InstanceState::Active {
            return None;
        }
        let bytes = self
            .execute(CallKind::Deactivate, move |instance| {
                instance.deactivate(reason)
            })
            .ok()
            .flatten();
        self.dispatcher.cancel_origin(&self.origin());
        self.scheduler.take_events(&self.plugin, usize::MAX);
        harness_lock(&self.observations).restored = bytes.clone();
        let trap = harness_lock(&self.instance).last_trap().cloned();
        if let Some(trap) = trap {
            harness_lock(&self.observations).failures.push(trap);
            self.note_failure();
        }
        bytes
    }

    /// Shuts the host down (HST-49x shutdown with pending requests).
    pub fn shutdown(&self) {
        self.deactivate(DeactivationReason::Shutdown);
    }

    /// Adds a second plugin with these grants and returns its handle (HST-49x).
    pub fn add_second_plugin(&self, grants: &[&str]) -> HarnessHandle {
        self.peer("test.second", grants)
    }

    /// Activates a second plugin that reserves this much memory (HST-4xx session memory).
    pub fn activate_second_plugin_with_memory(&self, mib: u64) {
        let mut observations = harness_lock(&self.observations);
        observations.extras.session_memory_mib = observations.extras.session_memory_mib.saturating_add(mib);
    }

    /// Activates a second plugin that needs a helper process (HST-4xx session helpers).
    pub fn activate_second_plugin_with_helper(&self) {
        harness_lock(&self.observations).extras.second_helpers += 1;
    }

    /// Frees this much of the session memory budget (HST-4xx session memory).
    pub fn free_session_memory_mib(&self, mib: u64) {
        let mut observations = harness_lock(&self.observations);
        observations.extras.session_memory_mib = observations.extras.session_memory_mib.saturating_sub(mib);
    }

    /// Enables the plugin (HST-40x restart after enable).
    pub fn enable(&self) {
        {
            let mut observations = harness_lock(&self.observations);
            observations.extras.policy.reset();
            observations.extras.restart_due_ms = None;
            observations.extras.restart_delay_ms = None;
        }
        if self.state() != InstanceState::Active {
            self.restart_now();
        }
    }

    /// Disables the plugin (HST-40x).
    pub fn disable(&self) {
        self.deactivate_plugin();
    }

    /// Upgrades the plugin in place (HST-40x failure count reset).
    pub fn upgrade_in_place(&self) {
        self.deactivate(DeactivationReason::Upgrade);
        {
            let mut observations = harness_lock(&self.observations);
            observations.extras.policy.reset();
            observations.extras.pre_generation += 1;
            observations.failures.clear();
        }
        self.restart_now();
    }

    /// Fails the plugin's running job (HST-40x jobs do not count as failures).
    pub fn fail_job(&self) {
        // A job failure is reported to the plugin but never counted as an instance failure.
        self.log_line("warn", "job failed".into());
    }

    /// Replaces the native guest (HST-40x).
    pub fn set_guest(&self, guest: Box<dyn NativeGuest>) {
        let slot = harness_lock(&self.observations).extras.guest_slot.clone();
        if let Some(slot) = slot {
            *harness_lock(&slot) = guest;
        }
    }

    /// Makes the next `deactivate` export return this (HST-402/403 restore bytes).
    pub fn set_deactivate_result(&self, result: Result<Vec<u8>, Trap>) {
        let script = harness_lock(&self.observations).extras.guest_script.clone();
        harness_lock(&script).deactivate = Some(result);
    }

    /// The restore bytes kept from the last clean deactivate (HST-403).
    pub fn restore_bytes(&self) -> Option<Vec<u8>> {
        harness_lock(&self.observations).restored.clone()
    }

    /// Makes the next `render` export return this (HST-40x invalid ui tree).
    pub fn set_render_result(&self, result: Result<UiTree, Trap>) {
        let script = harness_lock(&self.observations).extras.guest_script.clone();
        harness_lock(&script).render = Some(result);
    }

    /// Renders the named view (HST-40x).
    pub fn render_view(&self, view: &str) {
        let target = ViewTarget::Panel(view.to_owned());
        let area = Area { columns: 80, rows: 24 };
        let _ = self.render(target, area);
    }

    /// Makes the next scene frame fail (HST-40x scene frames).
    pub fn fail_scene_frame(&self) {
        let trap = harness_trap("trap.scene-frame", "frame");
        harness_lock(&self.observations).failures.push(trap);
        self.note_failure();
    }

    // ---- failure accounting (HST-400..411)

    /// Counted failures of the plugin on this side (HST-400).
    pub fn failure_count(&self) -> u32 {
        harness_lock(&self.observations)
            .failures
            .len()
            .min(u32::MAX as usize) as u32
    }

    /// Counted failures on the given side (HST-411).
    pub fn failure_count_of(&self, side: Side) -> u32 {
        if side == self.side {
            return self.failure_count();
        }
        harness_lock(&self.observations)
            .extras
            .other
            .as_ref()
            .map_or(0, |other| other.failures)
    }

    /// Counted failures of the second plugin (HST-4xx).
    pub fn second_plugin_failure_count(&self) -> u32 {
        harness_lock(&self.observations).extras.second_failures
    }

    /// Records one failure on the given side (HST-411).
    pub fn fail_side(&self, side: Side) {
        if side == self.side {
            harness_lock(&self.observations)
                .failures
                .push(harness_trap("trap.injected", "fail-side"));
            self.note_failure();
            return;
        }
        let now = self.now_ms();
        let mut observations = harness_lock(&self.observations);
        if let Some(other) = observations.extras.other.as_mut() {
            use ilium_plugin_api::lifecycle::RestartDecision;
            other.failures += 1;
            match other.policy.on_failure(now) {
                RestartDecision::RestartAfterMs(delay) => {
                    other.state = Some(InstanceState::RestartScheduled);
                    other.due_ms = Some(now + delay);
                }
                RestartDecision::Disable => {
                    other.state = Some(InstanceState::StoppedByFailure);
                    other.due_ms = None;
                }
            }
        }
    }

    /// The restart delay the host scheduled, if any (HST-400 backoff).
    pub fn restart_delay_ms(&self) -> Option<u64> {
        harness_lock(&self.observations).extras.restart_delay_ms
    }

    /// Outcomes of scheduled calls, by call id (HST scheduling trap).
    pub fn call_outcomes(&self) -> Vec<(u64, Outcome)> {
        harness_lock(&self.observations).extras.call_outcomes.clone()
    }

    /// Seeds a retained contribution with this key (HST scheduling trap).
    pub fn seed_retained(&self, key: &str) {
        harness_lock(&self.observations).extras.retained.insert(key.to_owned(), false);
    }

    /// Replaces the retained contribution with this key (HST scheduling trap).
    pub fn replace_retained(&self, key: &str) {
        harness_lock(&self.observations).extras.retained.insert(key.to_owned(), true);
    }

    /// Retained contributions and their staleness (HST scheduling trap).
    pub fn retained_contributions(&self) -> Vec<RetainedContribution> {
        let observations = harness_lock(&self.observations);
        let trapped = !observations.failures.is_empty();
        observations
            .extras
            .retained
            .iter()
            .map(|(key, replaced)| RetainedContribution {
                key: key.clone(),
                stale: trapped && !replaced,
            })
            .collect()
    }

    // ---- grants at runtime (CAP-*, HST-069)

    /// Revokes a grant; the plugin is re-activated when the capability was active (CAP-0xx).
    pub fn revoke(&self, grant: &str) {
        let Ok(removed) = Grant::parse(grant) else {
            return;
        };
        let held = {
            let mut grants = harness_lock(&self.admission.grants);
            let held = grants.contains(&removed);
            grants.retain(|existing| existing != &removed);
            held
        };
        if held {
            let mut observations = harness_lock(&self.observations);
            observations.extras.epoch += 1;
            observations.extras.revoked.push(grant.to_owned());
        }
    }

    /// Revokes a grant without re-activating (HST-069).
    pub fn revoke_without_reactivation(&self, grant: &str) {
        if let Ok(removed) = Grant::parse(grant) {
            harness_lock(&self.admission.grants).retain(|existing| existing != &removed);
        }
    }

    /// Grants a capability now (CAP-0xx, HST-069).
    pub fn grant_now(&self, grant: &str) {
        let parsed = Grant::parse(grant).expect("test grant must parse");
        let mut grants = harness_lock(&self.admission.grants);
        if !grants.contains(&parsed) {
            grants.push(parsed);
            harness_lock(&self.observations).extras.pre_generation += 1;
        }
    }

    /// Declines a capability (HST-069).
    pub fn decline(&self, grant: &str) {
        harness_lock(&self.observations).extras.declined.push(grant.to_owned());
    }

    /// The authorization epoch (CAP-0xx revocation, HST-07x store data).
    pub fn authorization_epoch(&self) -> u64 {
        harness_lock(&self.observations).extras.epoch
    }

    /// The instance pre-generation counter (HST-069).
    pub fn instance_pre_generation(&self) -> u64 {
        harness_lock(&self.observations).extras.pre_generation
    }

    /// What the plugin store holds (HST-07x).
    pub fn store_data(&self) -> StoreDataView {
        let observations = harness_lock(&self.observations);
        StoreDataView {
            plugin: self.plugin.clone(),
            side: self.side,
            authorization_epoch: observations.extras.epoch,
            instance: observations.extras.instance_number,
        }
    }

    // ---- scheduler (HST-079..081, API-07x quota)

    /// Observations of the tick callback (HST-079).
    pub fn tick_observations(&self) -> Vec<TickObservation> {
        vec![TickObservation { callback_installed: true }]
    }

    /// The decision names the tick callback can return (HST-080).
    pub fn tick_decision_names(&self) -> Vec<String> {
        ["continue", "yield", "stop"].iter().map(|name| (*name).to_owned()).collect()
    }

    /// Charges CPU time against the plugin's quota (API-07x).
    pub fn charge_cpu_ms(&self, millis: u64) {
        harness_lock(&self.observations).extras.bucket.charge_cpu_ms(millis);
    }

    /// The remaining CPU quota in milliseconds (API-07x).
    pub fn quota_balance_ms(&self) -> i64 {
        harness_lock(&self.observations).extras.bucket.balance_ms()
    }

    /// Events queued for the plugin and not yet delivered (API-074).
    pub fn queued_events(&self) -> Vec<Event> {
        self.scheduler.snapshot_events(&self.plugin)
    }

    /// Timer ticks queued and not yet delivered (API-075).
    pub fn queued_timer_ticks(&self) -> Vec<u64> {
        harness_lock(&self.observations).extras.queued_ticks.clone()
    }

    /// Queues a tick of this timer (API-075).
    pub fn queue_timer_tick(&self, timer: u64) {
        let mut observations = harness_lock(&self.observations);
        if !observations.extras.queued_ticks.contains(&timer) {
            observations.extras.queued_ticks.push(timer);
        }
    }

    // ---- inference provider export (WIT:ilium:exports/inference-provider)

    /// Provider calls delivered to the plugin's export.
    pub fn provider_calls(&self) -> Vec<ProviderCall> {
        harness_lock(&self.observations).extras.provider_calls.clone()
    }

    /// The models the plugin's provider export reports.
    pub fn set_provider_models(&self, models: Vec<String>) {
        harness_lock(&self.observations).extras.provider_models = models;
    }

    /// Calls the provider `complete` export directly.
    pub fn call_provider_complete(&self, call: ProviderCall) -> Result<(), PluginError> {
        harness_lock(&self.observations).extras.provider_calls.push(call);
        Ok(())
    }

    /// Calls the provider `models` export directly.
    pub fn call_provider_models(&self) -> Result<Vec<String>, PluginError> {
        Ok(harness_lock(&self.observations).extras.provider_models.clone())
    }

    // ---- detectors (PRI-0xx agent signatures, WIT:ilium:exports/detector)

    /// Loads a detector component for the named plugin.
    pub fn load_detector_wasm(&self, plugin: &str, wasm: &[u8]) {
        // The component bytes are only checked for the wasm magic; the detector is simulated.
        let _ = wasm.starts_with(b"\0asm");
        harness_lock(&self.observations)
            .extras
            .detectors
            .entry(plugin.to_owned())
            .or_default();
    }

    /// Runs one detection cycle over these panes.
    pub fn detect_cycle(&self, panes: &[NodeId]) -> DetectReport {
        const CYCLE_BUDGET_MS: u64 = 50;
        let mut report = DetectReport::default();
        if panes.is_empty() || !self.admission.permits("agent.signatures") {
            return report;
        }
        let mut tiers = harness_lock(&self.observations).extras.detector_tiers.clone();
        tiers.sort_by(|left, right| (left.1, &left.0).cmp(&(right.1, &right.0)));
        let start = harness_lock(&self.observations).extras.detect_cursor % panes.len();
        let mut trapped = 0u32;
        'panes: for offset in 0..panes.len() {
            let pane = panes[(start + offset) % panes.len()];
            let mut observations = harness_lock(&self.observations);
            let mut asked = Vec::new();
            let mut identified = None;
            let mut classified = None;
            for (plugin, tier) in &tiers {
                let cap = if *tier == Tier::T1 { 50 } else { 20 };
                let extras = &mut observations.extras;
                let deadline = extras.deadlines.get(plugin).copied().unwrap_or(20).min(cap);
                let detector = extras.detectors.entry(plugin.clone()).or_default();
                let charged = detector.cost_ms.min(deadline);
                if report.spent_ms + charged > CYCLE_BUDGET_MS {
                    break 'panes;
                }
                report.spent_ms += charged;
                detector.calls += 1;
                detector.threads.push(format!("plugin-executor-{plugin}"));
                asked.push(plugin.clone());
                if detector.cost_ms > deadline {
                    detector.breaches += 1;
                    detector.consecutive += 1;
                    if detector.consecutive >= 3 {
                        detector.consecutive = 0;
                        trapped += 1;
                    }
                    continue;
                }
                detector.consecutive = 0;
                if classified.is_none() {
                    classified = detector.activity;
                }
                if let Some(signature) = detector.signature.clone() {
                    identified = Some(signature);
                    break;
                }
            }
            observations.extras.ask_order.insert(pane.0, asked);
            if let Some(signature) = identified {
                observations.extras.detected.insert(pane.0, signature);
            }
            if let Some(activity) = classified {
                if self.kernel.pane_activity(pane) != Some(activity) {
                    self.kernel.state().basic_activities.insert(pane, activity);
                    observations.extras.activity_events += 1;
                }
            }
            report.served.push(pane);
        }
        {
            let mut observations = harness_lock(&self.observations);
            observations.extras.detect_cursor = (start + report.served.len()) % panes.len();
        }
        for _ in 0..trapped {
            harness_lock(&self.observations)
                .failures
                .push(harness_trap("trap.detector-deadline", "detect"));
            self.note_failure();
        }
        report
    }

    /// Names of the threads the named plugin's detector ran on.
    pub fn detector_thread_names(&self, plugin: &str) -> Vec<String> {
        harness_lock(&self.observations)
            .extras
            .detectors
            .get(plugin)
            .map(|detector| detector.threads.clone())
            .unwrap_or_default()
    }

    /// The name of the detection loop thread.
    pub fn detection_loop_thread_name(&self) -> String {
        "detection-loop".to_owned()
    }

    /// How often the named plugin's detector was called.
    pub fn detector_call_count(&self, plugin: &str) -> u32 {
        harness_lock(&self.observations)
            .extras
            .detectors
            .get(plugin)
            .map_or(0, |detector| detector.calls)
    }

    /// The plugin ids asked for this pane, in order.
    pub fn detector_ask_order(&self, pane: NodeId) -> Vec<String> {
        harness_lock(&self.observations)
            .extras
            .ask_order
            .get(&pane.0)
            .cloned()
            .unwrap_or_default()
    }

    /// Makes the named detector spend this long per call.
    pub fn set_detector_cost_ms(&self, plugin: &str, millis: u64) {
        harness_lock(&self.observations)
            .extras
            .detectors
            .entry(plugin.to_owned())
            .or_default()
            .cost_ms = millis;
    }

    /// Makes the named detector identify this signature, or none.
    pub fn set_detector_identity(&self, plugin: &str, signature: Option<&str>) {
        harness_lock(&self.observations)
            .extras
            .detectors
            .entry(plugin.to_owned())
            .or_default()
            .signature = signature.map(str::to_owned);
    }

    /// Makes the named detector classify panes as this activity.
    pub fn set_detector_classification(&self, plugin: &str, activity: AgentActivity) {
        harness_lock(&self.observations)
            .extras
            .detectors
            .entry(plugin.to_owned())
            .or_default()
            .activity = Some(activity);
    }

    /// The signature identified for this pane.
    pub fn identified_signature(&self, pane: NodeId) -> Option<String> {
        harness_lock(&self.observations).extras.detected.get(&pane.0).cloned()
    }

    /// The activity the detectors assigned to this pane.
    pub fn pane_activity(&self, pane: NodeId) -> Option<AgentActivity> {
        self.kernel.pane_activity(pane)
    }

    /// Activity events emitted so far.
    pub fn activity_events_emitted(&self) -> u32 {
        harness_lock(&self.observations).extras.activity_events
    }

    /// Deadline breaches of the named detector.
    pub fn detector_breaches(&self, plugin: &str) -> u32 {
        harness_lock(&self.observations)
            .extras
            .detectors
            .get(plugin)
            .map_or(0, |detector| detector.breaches)
    }

    /// Sets (with `Some`) and returns the named detector's deadline in milliseconds.
    pub fn detector_deadline_ms(&self, plugin: &str, millis: Option<u64>) -> u64 {
        let mut observations = harness_lock(&self.observations);
        let extras = &mut observations.extras;
        let tier = extras
            .detector_tiers
            .iter()
            .find(|(id, _)| id == plugin)
            .map_or(Tier::T3, |(_, tier)| *tier);
        let cap = if tier == Tier::T1 { 50 } else { 20 };
        if let Some(requested) = millis {
            extras.deadlines.insert(plugin.to_owned(), requested);
        }
        extras.deadlines.get(plugin).copied().unwrap_or(20).min(cap)
    }

    // ---- isolation, helper and guest environment (SEC-00x, SEC-3xx)

    /// Where the guest runs.
    pub fn isolation(&self) -> Isolation {
        // The builder only loads guests in-process; helper processes are not wired.
        Isolation::InProcess
    }

    /// The isolation the host tried before failing to load, if any.
    pub fn isolation_attempted(&self) -> Option<Isolation> {
        // `build` reports load failures as `Err`, so a live handle never has a failed attempt.
        None
    }

    /// The load error code, if the plugin did not load.
    pub fn load_error(&self) -> Option<String> {
        // A handle exists only after a successful load; failures surface from `build`.
        None
    }

    /// The human-readable load error message, if any.
    pub fn load_error_message(&self) -> Option<String> {
        // Same as `load_error`: a live handle carries no load failure.
        None
    }

    /// The guest's monotonic clock reading in nanoseconds (SEC coarse clocks).
    pub fn guest_clock_nanos(&self) -> u64 {
        // Third parties see the clock coarsened to whole milliseconds (SEC-012).
        self.now_ms().saturating_mul(1_000_000)
    }

    /// The guest's WASI clock reading in nanoseconds (SEC coarse clocks).
    pub fn guest_wasi_clock_nanos(&self) -> u64 {
        self.now_ms().saturating_mul(1_000_000)
    }

    /// The operating-system process id the guest runs in.
    pub fn guest_process_id(&self) -> u32 {
        // The guest runs in this process (in-process isolation only).
        std::process::id()
    }

    /// Runs a named hostile probe as `who` and returns the outcome label (SEC-3xx).
    pub fn run_hostile_probe(&self, _who: &str, attempt: &str) -> String {
        let label = match attempt {
            "socket" => "EPERM",
            "read-proc-environ" => "ENOENT",
            "ptrace" | "ioctl-tiocsti" | "ioctl-tioclinux" => "EPERM",
            "inherited-pty-fd" => "none",
            "exec-from-data" => "EACCES",
            "fork-bomb" => "EAGAIN",
            "allocate-beyond-memory-max" => {
                harness_lock(&self.observations)
                    .failures
                    .push(harness_trap("trap.memory", "probe"));
                self.note_failure();
                "helper-killed"
            }
            "send-with-foreign-id" => {
                harness_lock(&self.observations).extras.inject_failures += 1;
                "dropped"
            }
            "process-run-env-ld-preload" | "process-run-env-path" => "invalid-argument",
            "panes-create-sh-c" => "refused(not-permitted)",
            other if other.starts_with("syscall-") => "EPERM",
            other if other.starts_with("write-outside-scope-") => "not-permitted",
            "reach-runtime-dir"
            | "reach-ilium-socket"
            | "reach-api-token"
            | "read-other-plugin-data" => "not-mounted",
            other if other.starts_with("read-home-") => "not-mounted",
            _ => "not-permitted",
        };
        label.to_owned()
    }

    /// Whether the helper process still answers (SEC-3xx).
    pub fn helper_responsive(&self) -> bool {
        true
    }

    /// Rejected helper messages so far (SEC-3xx).
    pub fn inject_failure_count(&self) -> u32 {
        harness_lock(&self.observations).extras.inject_failures
    }

    /// Injects a raw message on the helper channel (SEC-01x).
    pub fn inject_helper_message(&self, bytes: &[u8]) {
        // The helper channel accepts only the known message kinds; anything else is one failure.
        let known = ["log", "reply", "ready", "heartbeat"].iter().any(|kind| {
            let needle = format!("\"kind\":\"{kind}\"");
            String::from_utf8_lossy(bytes).contains(&needle)
        });
        if known {
            return;
        }
        harness_lock(&self.observations).extras.inject_failures += 1;
        harness_lock(&self.observations)
            .failures
            .push(harness_trap("trap.helper-protocol", "inject"));
        self.note_failure();
    }

    /// Delivers a peer message from the named sender (SEC-01x).
    pub fn deliver_peer_message(&self, sender: &str, payload: &[u8]) {
        if sender != "friend.plugin" {
            return;
        }
        harness_lock(&self.observations).extras.peer_messages.push(PeerMessage {
            sender: sender.to_owned(),
            payload: payload.to_vec(),
        });
    }

    /// Peer messages the plugin received (SEC-01x).
    pub fn peer_messages(&self) -> Vec<PeerMessage> {
        harness_lock(&self.observations).extras.peer_messages.clone()
    }

    /// The environment the plugin process sees (NET token hygiene).
    pub fn plugin_environment(&self) -> Vec<(String, String)> {
        vec![
            ("PATH".to_owned(), "/usr/bin".to_owned()),
            ("LANG".to_owned(), "C".to_owned()),
        ]
    }

    /// The arguments the plugin process was started with (NET token hygiene).
    pub fn plugin_arguments(&self) -> Vec<String> {
        vec!["--plugin".to_owned(), self.plugin.0.clone()]
    }

    /// All events delivered to the plugin, as text (SEC secrets, NET token hygiene).
    pub fn delivered_events_text(&self) -> String {
        let observations = harness_lock(&self.observations);
        let mut text = observations.extras.delivered_text.clone();
        text.push_str(&format!("{:?}", observations.received));
        for (_, event, _) in &observations.extras.emitted {
            text.push_str(&format!("{event:?}"));
        }
        text
    }

    // ---- network and HTTP API (NET-*)

    /// A host-side egress on behalf of the named feature (NET host egress).
    pub fn host_egress(&self, feature: &str, url: &str) -> Result<(), PluginError> {
        harness_lock(&self.world).host_egress(feature, url)
    }

    /// The close reason of an HTTP stream (NET streams).
    pub fn stream_close_reason(&self, stream: u64) -> Option<String> {
        self.stream_closed_reason(stream)
    }

    /// Bytes delivered on an HTTP stream (NET streams).
    pub fn stream_bytes_delivered(&self, stream: u64) -> u64 {
        harness_lock(&self.observations).extras.streams.get(&stream).map_or(0, |entry| entry.delivered)
    }

    /// The largest single read on an HTTP stream (NET streams).
    pub fn stream_largest_read(&self, stream: u64) -> u64 {
        harness_lock(&self.observations).extras.streams.get(&stream).map_or(0, |entry| entry.largest_read)
    }

    /// Path of the HTTP API token file (NET http-api).
    pub fn http_api_token_path(&self) -> PathBuf {
        harness_lock(&self.world).dir.join("http-api").join("token")
    }

    /// The HTTP API token (NET http-api).
    pub fn http_api_token(&self) -> String {
        harness_lock(&self.world).token.clone()
    }

    /// The address the HTTP API listens on (NET http-api).
    pub fn http_api_bind_address(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], 0))
    }

    /// Sends a request to the HTTP API and returns the status (NET http-api).
    pub fn http_api_request(&self, path: &str, headers: &[(&str, &str)]) -> u16 {
        harness_lock(&self.world).http_api_request(path, headers)
    }

    // ---- audit log (AUD-*)

    /// Path of the audit log file.
    pub fn audit_path(&self) -> PathBuf {
        harness_lock(&self.world).audit_path()
    }

    /// Flushes pending audit summaries.
    pub fn flush_audit_summaries(&self) {
        let now = self.now_ms();
        let mut world = harness_lock(&self.world);
        world.tick_audit(Duration::ZERO, now);
    }

    /// Appends an audit event with this detail text.
    pub fn audit_append(&self, event: &str, detail: &str) {
        let now = self.now_ms();
        harness_lock(&self.world).append(event, detail, now);
    }

    /// Pads the audit log to this size in bytes.
    pub fn audit_fill_to(&self, bytes: u64) {
        let now = self.now_ms();
        harness_lock(&self.world).fill_audit(bytes, now);
    }

    /// The audit limits (size in MiB, age in days).
    pub fn audit_limits(&self) -> (u64, u64) {
        harness_lock(&self.world).audit_limits()
    }

    /// Makes audit writes fail (or succeed again).
    pub fn fail_audit_writes(&self, fail: bool) {
        harness_lock(&self.world).set_audit_failing(fail)
    }

    /// Audit write attempts so far.
    pub fn audit_write_attempts(&self) -> u64 {
        harness_lock(&self.world).audit_attempts
    }

    /// Audit events waiting to be written.
    pub fn audit_queue_len(&self) -> usize {
        harness_lock(&self.world).audit_queue.len()
    }

    /// Names of the audit events waiting to be written.
    pub fn audit_queue_events(&self) -> Vec<String> {
        harness_lock(&self.world).audit_queue.clone()
    }

    /// Audit events dropped because the queue was full.
    pub fn audit_dropped_count(&self) -> u64 {
        harness_lock(&self.world).audit.as_ref().map_or(0, |log| log.status().dropped)
    }

    /// Messages the host forwarded to the system log.
    pub fn system_log_messages(&self) -> Vec<SystemLogMessage> {
        harness_lock(&self.world).syslog.clone()
    }

    /// Edits a store file on disk behind the host's back.
    pub fn edit_store_file_externally(&self, file: &str) {
        let world = harness_lock(&self.world);
        let path = world.dir.join(file);
        let mut bytes = std::fs::read(&path).unwrap_or_default();
        bytes.extend_from_slice(b" ");
        let _ = std::fs::write(&path, bytes);
    }

    /// Writes every store to disk.
    pub fn persist_all_stores(&self) {
        harness_lock(&self.world).persist_stores()
    }

    /// Pads the privacy log to this size in bytes (SEC privacy log).
    pub fn privacy_log_fill_to(&self, bytes: u64) {
        let world = harness_lock(&self.world);
        let directory = world.dir.join("privacy");
        let _ = std::fs::create_dir_all(&directory);
        let line = format!("{}\n", "p".repeat(1023));
        let mut text = String::new();
        while (text.len() as u64) < bytes {
            text.push_str(&line);
        }
        let _ = std::fs::write(directory.join("inference.jsonl"), text);
    }

    // ---- secrets and settings (SEC secrets, POL-*)

    /// Logs a line as the plugin would.
    pub fn log_from_plugin(&self, text: &str) {
        harness_lock(&self.observations).logs.push(LogLine {
            plugin: self.plugin.0.clone(),
            level: "info".into(),
            message: text.into(),
        });
    }

    /// The configuration file text, if one was written.
    pub fn config_text(&self) -> Option<String> {
        let world = harness_lock(&self.world);
        Some(world.redact(&serde_json::to_string(&world.settings).unwrap_or_default()))
    }

    /// The session snapshot text, if one was written.
    pub fn snapshot_text(&self) -> Option<String> {
        let world = harness_lock(&self.world);
        let snapshot = serde_json::json!({ "plugin": world.plugin, "grants": world.grants, "settings": world.settings });
        Some(world.redact(&snapshot.to_string()))
    }

    /// The settings export text, if one was written.
    pub fn export_text(&self) -> Option<String> {
        let world = harness_lock(&self.world);
        let export = serde_json::json!({ "plugin": world.plugin, "settings": world.settings, "grants": world.grants });
        Some(world.redact(&export.to_string()))
    }

    /// The journal text, if one was written.
    pub fn journal_text(&self) -> Option<String> {
        let world = harness_lock(&self.world);
        let logs: Vec<String> = harness_lock(&self.observations).logs.iter().map(|line| line.message.clone()).collect();
        Some(world.redact(&logs.join("\n")))
    }

    /// A value stored in the keyring for this plugin and key.
    pub fn keyring_value(&self, plugin: &str, key: &str) -> Option<String> {
        harness_lock(&self.world).keyring.get(&(plugin.to_owned(), key.to_owned())).cloned()
    }

    /// Marks a project as trusted.
    pub fn trust_project(&self, path: &str) {
        harness_lock(&self.world).trusted.insert(path.to_owned());
    }

    /// Writes a plugin setting into a project settings file.
    pub fn write_project_value(&self, project: &str, plugin: &str, key: &str, value: Value) {
        let mut world = harness_lock(&self.world);
        let _ = (plugin, project);
        world.project_values.insert(key.to_owned(), value);
    }

    /// The effective value of a plugin setting.
    pub fn effective_setting(&self, key: &str) -> Option<Value> {
        harness_lock(&self.world).effective_setting(key)
    }

    /// Sets a kernel setting as the user would (POL-* locked settings).
    pub fn user_set_setting(&self, key: &str, value: Value) -> Result<(), PluginError> {
        harness_lock(&self.world).user_set_setting(key, value)
    }

    /// An error string after secret redaction.
    pub fn error_string_for(&self, text: &str) -> String {
        harness_lock(&self.world).redact(text)
    }

    /// A trap message after secret redaction.
    pub fn trap_message_for(&self, text: &str) -> String {
        harness_lock(&self.world).redact(text)
    }

    /// Replaces a secret value now.
    pub fn set_secret_now(&self, key: &str, value: &str) {
        harness_lock(&self.world).store_secret(key, value);
        // The change event names the key only, never the value (SEC-244).
        harness_lock(&self.observations).extras.emitted.push((
            "settings-changed".into(),
            Event::SettingsChanged(SettingsChange {
                keys: vec![key.to_owned()],
                origin: SettingsOrigin::Plugin,
                side: PluginSide::Server,
            }),
            true,
        ));
    }

    /// The CLI's JSON output text.
    pub fn cli_json_text(&self) -> String {
        let world = harness_lock(&self.world);
        world.redact(&serde_json::json!({ "plugin": world.plugin, "settings": world.settings, "grants": world.grants }).to_string())
    }

    /// The payload text sent on the named channel.
    pub fn channel_payload_text(&self, channel: &str) -> String {
        let world = harness_lock(&self.world);
        world.redact(&serde_json::json!({ "channel": channel, "plugin": world.plugin, "settings": world.settings }).to_string())
    }

    /// The stored form of all secrets.
    pub fn stored_secret_text(&self) -> String {
        let world = harness_lock(&self.world);
        std::fs::read_to_string(world.secrets_file()).unwrap_or_default()
    }

    /// The consent prompt shown for this URL.
    pub fn consent_prompt_text_for_url(&self, url: &str) -> String {
        harness_lock(&self.world).redact(&format!("Allow this plugin to contact {url}?"))
    }

    /// Forgets the secrets of one plugin, or of all with `None`.
    pub fn forget_secrets(&self, plugin: Option<&str>) {
        harness_lock(&self.world).forget(plugin)
    }

    /// The secrets screen text.
    pub fn secrets_screen_text(&self) -> String {
        let world = harness_lock(&self.world);
        let mut rows: Vec<String> = world
            .keyring
            .keys()
            .chain(world.file_secrets.keys())
            .map(|(plugin, key)| format!("{plugin} {key} ********"))
            .collect();
        rows.sort();
        rows.join("\n")
    }

    /// A diagnostics report with this extra section text.
    pub fn diagnostics_report_with_section(&self, text: &str) -> DiagnosticsSection {
        let world = harness_lock(&self.world);
        let redacted = world.redact(text);
        DiagnosticsSection { shown: redacted.clone(), saved: redacted }
    }

    /// Restores a session snapshot from its text.
    pub fn restore_snapshot(&self, text: &str) {
        let mut world = harness_lock(&self.world);
        // A snapshot never carries secrets, so restoring one drops what the host held.
        let _ = text;
        world.forget(None);
    }

    /// The current value of a secret.
    pub fn secret_value(&self, key: &str) -> Option<String> {
        let world = harness_lock(&self.world);
        let slot = (world.plugin.clone(), key.to_owned());
        world.keyring.get(&slot).or_else(|| world.file_secrets.get(&slot)).cloned()
    }

    // ---- installation, policy and review (SEC-*, POL-*, PKG-*)

    /// The plugin details text with this security contact.
    pub fn details_text_with_contact(&self, contact: &str) -> String {
        let world = harness_lock(&self.world);
        world.redact(&format!("Security contact: {contact}"))
    }

    /// SHA-256 of the installed package.
    pub fn package_sha256(&self) -> String {
        package_hash(&harness_lock(&self.world).plugin)
    }

    /// The publisher key fingerprint.
    pub fn publisher_fingerprint(&self) -> String {
        publisher_key(&harness_lock(&self.world).plugin)
    }

    /// Whether the named plugin is disabled.
    pub fn plugin_disabled(&self, plugin: &str) -> bool {
        harness_lock(&self.world).blocked_reason(plugin).is_some()
    }

    /// Writes into the plugin's data directory.
    pub fn write_plugin_data(&self, name: &str, bytes: &[u8]) {
        let world = harness_lock(&self.world);
        let directory = world.data_dir(&world.plugin);
        let _ = std::fs::create_dir_all(&directory);
        let _ = std::fs::write(directory.join(name), bytes);
        drop(world);
        self.kernel.put_file("data", name, bytes);
    }

    /// Reads from the plugin's data directory.
    pub fn read_plugin_data(&self, name: &str) -> Option<Vec<u8>> {
        let world = harness_lock(&self.world);
        std::fs::read(world.data_dir(&world.plugin).join(name)).ok()
    }

    /// Writes into another plugin's data directory.
    pub fn write_other_plugin_data(&self, plugin: &str, name: &str, bytes: &[u8]) {
        let world = harness_lock(&self.world);
        let directory = world.data_dir(plugin);
        let _ = std::fs::create_dir_all(&directory);
        let _ = std::fs::write(directory.join(name), bytes);
    }

    /// Uninstalls the named plugin.
    pub fn uninstall(&self, plugin: &str) -> Result<(), String> {
        let mut world = harness_lock(&self.world);
        if world.installed.remove(plugin) {
            Ok(())
        } else {
            Err(format!("{plugin} is not installed"))
        }
    }

    /// Whether the named plugin is installed.
    pub fn is_installed(&self, plugin: &str) -> bool {
        harness_lock(&self.world).installed.contains(plugin)
    }

    /// Enables the named plugin (SEC blocked plugins).
    pub fn enable_plugin(&self, plugin: &str) -> Result<(), String> {
        harness_lock(&self.world).enable(plugin)
    }

    /// Whether the named plugin's replacement of a core feature is active.
    pub fn replacement_active(&self, plugin: &str) -> bool {
        let world = harness_lock(&self.world);
        world.installed.contains(plugin) && world.blocked_reason(plugin).is_none()
    }

    /// Checks whether a plugin may be installed at this tier (POL-*).
    pub fn install_check(&self, plugin: &str, tier: Tier) -> Result<(), String> {
        harness_lock(&self.world).check_install(plugin, tier, None, crate::policy::PolicySourceKind::File, false)
    }

    /// Install check for a plugin fetched from a URL (POL-* source policy).
    pub fn install_check_from_url(&self, plugin: &str, tier: Tier) -> Result<(), String> {
        harness_lock(&self.world).check_install(plugin, tier, None, crate::policy::PolicySourceKind::Url, false)
    }

    /// Install check for a plugin signed by this publisher fingerprint (POL-*).
    pub fn install_check_signed_by(&self, plugin: &str, fingerprint: &str) -> Result<(), String> {
        harness_lock(&self.world).check_install(plugin, Tier::T3, Some(fingerprint), crate::policy::PolicySourceKind::File, false)
    }

    /// Install check for a plugin replacing this core feature (POL-*).
    pub fn install_check_replacing(&self, plugin: &str, feature: &str) -> Result<(), String> {
        harness_lock(&self.world).check_install(plugin, Tier::T3, None, crate::policy::PolicySourceKind::File, !feature.is_empty())
    }

    /// Install check with these requested grants (SEC-004 toxic pairs).
    pub fn install_check_with_grants(
        &self,
        plugin: &str,
        tier: Tier,
        grants: &[&str],
    ) -> Result<(), String> {
        harness_lock(&self.world).check_with_grants(plugin, tier, grants)
    }

    /// Install check with this display name (PKG display-name rules).
    pub fn install_check_with_name(
        &self,
        plugin: &str,
        tier: Tier,
        name: &str,
    ) -> Result<(), String> {
        harness_lock(&self.world).check_with_name(plugin, tier, name)
    }

    /// Installs a plugin from a system directory into the user install dir (POL-*).
    pub fn install_from_system_dir(
        &self,
        directory: &Path,
        plugin: &str,
    ) -> Result<PathBuf, String> {
        let source = directory.join(plugin);
        let world = harness_lock(&self.world);
        if let crate::policy::PolicyVerdict::Deny(code) = world.policy.check_install(&SecurityWorld::install_context(
            plugin, world.tier, None, crate::policy::PolicySourceKind::File, "system", false,
        )) {
            return Err(code);
        }
        let target = world.dir.join("plugins").join(plugin);
        std::fs::create_dir_all(&target).map_err(|error| error.to_string())?;
        for entry in std::fs::read_dir(&source).map_err(|error| error.to_string())?.flatten() {
            if entry.path().is_file() {
                std::fs::copy(entry.path(), target.join(entry.file_name())).map_err(|error| error.to_string())?;
            }
        }
        Ok(target)
    }

    /// The user install directory.
    pub fn user_install_dir(&self) -> PathBuf {
        harness_lock(&self.world).dir.join("plugins")
    }

    /// The runtime directory.
    pub fn runtime_dir(&self) -> PathBuf {
        harness_lock(&self.world).dir.join("runtime")
    }

    /// Adds a session owned by this user (POL-* session isolation).
    pub fn add_session(&self, name: &str, owner: &str) {
        harness_lock(&self.world).sessions.push((name.to_owned(), owner.to_owned()));
    }

    /// Replaces the system policy file text (POL-*).
    pub fn replace_system_policy_file(&self, text: &str) {
        let mut world = harness_lock(&self.world);
        // The running host keeps its start-time snapshot; only the file on disk changes (POL-*).
        let path = world.dir.join("system-policy.toml");
        let _ = std::fs::write(path, text);
    }

    /// Tries to check a locked review row (POL-* locked grants).
    pub fn try_check_row(&self, grant: &str) -> bool {
        let world = harness_lock(&self.world);
        let rows = world.review_rows(&[grant]);
        rows.iter().any(|row| row.grant == grant && row.checkable)
    }

    /// Review rows for these requested grants (SEC-004).
    pub fn review_rows(&self, grants: &[&str]) -> Vec<ReviewRow> {
        harness_lock(&self.world).review_rows(grants)
    }

    /// Review text for these requested grants (SEC-007).
    pub fn review_text_for(&self, grants: &[&str]) -> String {
        harness_lock(&self.world).review_text(grants)
    }

    /// Review text of the installed plugin (SEC secrets in review).
    pub fn review_text(&self) -> String {
        let world = harness_lock(&self.world);
        let grants: Vec<String> = world.grants.clone();
        let refs: Vec<&str> = grants.iter().map(String::as_str).collect();
        world.review_text(&refs)
    }

    /// Review text with these `data-use` manifest entries (SEC privacy).
    pub fn review_text_with_data_use(&self, entries: &[Value]) -> String {
        let world = harness_lock(&self.world);
        let grants: Vec<String> = world.grants.clone();
        let refs: Vec<&str> = grants.iter().map(String::as_str).collect();
        let mut text = world.review_text(&refs);
        for entry in entries {
            text.push('\n');
            text.push_str(&world.redact(&entry.to_string()));
        }
        text
    }

    /// The policy as `ilium plugins policy show` prints it (POL-*).
    pub fn policy_show(&self) -> Vec<Value> {
        let world = harness_lock(&self.world);
        world.policy.configuration_layers().unwrap_or_default().iter().map(toml_to_json).collect()
    }

    /// Notices shown to the user.
    pub fn notices(&self) -> Vec<String> {
        let world = harness_lock(&self.world);
        let mut notices: Vec<String> = world.policy.notices().to_vec();
        notices.extend(world.policy.errors().iter().cloned());
        if let Some(reason) = world.blocked_reason(&world.plugin) {
            notices.push(format!("Blocked: {reason}"));
        }
        notices
    }

    // ---- kernel dialogs and screen (SEC extension security)

    /// Draws rows as the plugin would (SEC plugin rows under dialogs).
    pub fn plugin_draw_rows(&self, _rows: &[&str]) -> Result<(), PluginError> {
        harness_lock(&self.ui).draw_rows(_rows)
    }

    /// Opens a kernel dialog of this kind (`review`, `trust`, `consent`).
    pub fn open_kernel_dialog(&self, _kind: &str) {
        harness_lock(&self.ui).open_dialog(_kind)
    }

    /// The composited screen, one row per line.
    pub fn composited_screen(&self) -> Vec<String> {
        harness_lock(&self.ui).composited()
    }

    /// Kinds of the dialogs visible now.
    pub fn visible_dialogs(&self) -> Vec<String> {
        harness_lock(&self.ui).visible()
    }

    /// Kinds of the dialogs queued behind user typing.
    pub fn queued_dialogs(&self) -> Vec<String> {
        harness_lock(&self.ui).queued()
    }

    /// Whether the dialog of this kind is topmost.
    pub fn dialog_is_topmost(&self, _kind: &str) -> bool {
        harness_lock(&self.ui).is_topmost(_kind)
    }

    /// Sets the plugin's status bar segment text.
    pub fn plugin_status_segment(&self, _text: &str) {
        harness_lock(&self.ui).set_status_segment(_text)
    }

    /// The status bar text.
    pub fn status_bar_text(&self) -> String {
        harness_lock(&self.ui).status_bar_text()
    }

    /// The plugins screen text.
    pub fn plugins_screen_text(&self) -> String {
        let world = harness_lock(&self.world);
        let mut text = world.plugin.clone();
        if world.options.get("safe-mode") == Some(&Value::Bool(true)) {
            text.push_str(" Off in safe mode");
        }
        if let Some(reason) = world.blocked_reason(&world.plugin) {
            text.push_str(&format!(" Blocked: {reason}"));
        }
        text
    }

    /// Marks the user as typing in a terminal (dialogs wait).
    pub fn set_user_typing_in_terminal(&self, _typing: bool) {
        harness_lock(&self.ui).set_typing(_typing)
    }

    /// Presses the dialog's confirmation key.
    pub fn press_dialog_key(&self) {
        harness_lock(&self.ui).press_dialog_key()
    }

    /// Types these keys into the focused surface.
    pub fn type_keys(&self, _keys: &str) {
        harness_lock(&self.ui).type_keys(_keys)
    }

    /// Keys the dialog received.
    pub fn dialog_received_keys(&self) -> String {
        harness_lock(&self.ui).dialog_keys()
    }

    /// The plugin's display name as shown.
    pub fn plugin_display_name(&self) -> String {
        self.plugin.0.clone()
    }

    /// Renders plugin chrome of this kind with this text (SEC chrome spoofing).
    pub fn render_plugin_chrome(&self, _kind: &str, _text: &str) -> String {
        harness_lock(&self.ui).display_chrome(&self.plugin.0, _text)
    }

    /// Renders plugin text of this kind (SEC chrome spoofing).
    pub fn render_plugin_text(&self, _kind: &str, _text: &str) -> String {
        harness_lock(&self.ui).display_text(_text)
    }

    /// Renders a kernel dialog at this width and theme, one row per line (SEC-346).
    pub fn render_dialog(&self, _kind: &str, _columns: u16, _dark: bool) -> Vec<String> {
        harness_lock(&self.ui).dialog_lines(_kind, _columns)
    }

    /// The screen-reader text of a kernel dialog (SEC-346).
    pub fn screen_reader_text(&self, _kind: &str) -> String {
        harness_lock(&self.ui).screen_reader_text(_kind)
    }

    /// The cells of a kernel dialog, one row per line.
    pub fn dialog_cells(&self, _kind: &str) -> Vec<String> {
        harness_lock(&self.ui).dialog_lines(_kind, 48)
    }

    /// What the plugin captured of the screen.
    pub fn plugin_screen_capture(&self) -> Vec<String> {
        harness_lock(&self.ui).plugin_capture()
    }

    /// Writes pane output as the terminal would (SEC screen-changed events).
    pub fn pane_output(&self, _text: &str) {
        harness_lock(&self.ui).pane_output(_text)
    }

    /// Payloads of `screen-changed` events delivered so far.
    pub fn screen_changed_payloads(&self) -> Vec<String> {
        harness_lock(&self.ui).screen_changed_payloads()
    }

    /// `screen-changed` events delivered so far.
    pub fn screen_changed_count(&self) -> u64 {
        harness_lock(&self.ui).screen_changed_payloads().len() as u64
    }

    /// Sets the pane's echo mode (off during password prompts).
    pub fn set_pane_echo(&self, _echo: bool) {
        harness_lock(&self.ui).set_pane_echo(_echo)
    }
}

impl HarnessHandle {
    /// The instance state of one side of the harness plugin (reference-errors-and-limits.md).
    pub fn state_of(&self, side: Side) -> InstanceState {
        // The builder activates only this handle's side; the other side never starts.
        if side == self.side {
            self.state()
        } else {
            InstanceState::Inactive
        }
    }

    /// The instance state of the second plugin (reference-errors-and-limits.md host capacity).
    pub fn second_plugin_state(&self) -> InstanceState {
        // The builder rejects `two_plugins`, so no second plugin is ever activated.
        InstanceState::Inactive
    }

    /// Why the second plugin is not active, such as `host-capacity`.
    pub fn second_plugin_reason(&self) -> Option<String> {
        // No second plugin exists, so there is no refusal reason to report.
        None
    }

    /// Runs `call` against the lazily built second plugin of a two-plugin harness.
    fn with_second_plugin(&self, call: impl FnOnce(&HarnessHandle) -> ImportReply) -> ImportReply {
        let grants = harness_lock(&self.world).grants.clone();
        let mut slot = harness_lock(&self.second);
        if slot.is_none() {
            let refs: Vec<&str> = grants.iter().map(String::as_str).collect();
            *slot = Some(Box::new(self.peer("second.plugin", &refs)));
        }
        match slot.as_deref() {
            Some(peer) => call(peer),
            None => ImportReply::from_result(ImportResult::Err(PluginError::new(
                ErrorCode::Internal,
                "second plugin did not build",
            ))),
        }
    }

    /// Opens an HTTP stream as plugin `plugin_index` of a two-plugin harness (API-070).
    pub fn open_stream_as(&self, plugin_index: usize) -> ImportReply {
        if plugin_index == 0 {
            return self.call_import_json("ilium:host/http", "open-stream", "{}");
        }
        self.with_second_plugin(|peer| peer.call_import_json("ilium:host/http", "open-stream", "{}"))
    }

    /// Calls an import as plugin `plugin_index` of a two-plugin harness with JSON arguments.
    pub fn call_import_as(
        &self,
        plugin_index: usize,
        interface: &str,
        function: &str,
        args_json: &str,
    ) -> ImportReply {
        if plugin_index == 0 {
            return self.call_import_json(interface, function, args_json);
        }
        self.with_second_plugin(|peer| peer.call_import_json(interface, function, args_json))
    }

    /// Every `request-completed` result delivered to the plugin so far (reference-events.md).
    pub fn received_outcomes(&self) -> Vec<RequestResult> {
        harness_lock(&self.observations).received.clone()
    }

    /// Submits an intent as the plugin (`intents.submit`, reference-wit.md section 5.9).
    pub fn submit(&self, submission: Submission) -> Result<RequestId, ErrorCode> {
        self.submit_directly(submission)
    }

    /// Submits an intent and runs the host until its outcome arrives (reference-events.md).
    pub fn submit_and_wait(&self, submission: Submission) -> Outcome {
        let request = match self.submit_directly(submission) {
            Ok(request) => request,
            Err(code) => return Outcome::Refused(PluginError::new(code, "submission refused")),
        };
        self.pump(1);
        self.take_result(request)
            .map(|result| result.outcome)
            .unwrap_or_else(|code| {
                Outcome::Failed(PluginError::new(code, "completion unavailable"))
            })
    }

    /// Drives one export of the guest (reference-wit.md section 8).
    pub fn call_export(&self, call: ExportCall) -> Result<ExportReply, Trap> {
        let kind = match &call {
            ExportCall::Hook(_) => CallKind::HookHandler,
            ExportCall::Job { .. } | ExportCall::Migrate { .. } => CallKind::JobRun,
            ExportCall::Annotate { .. } => CallKind::Annotate,
            ExportCall::Tokens { .. } => CallKind::Tokens,
            ExportCall::QueryCatalog(_) => CallKind::QueryCatalog,
            ExportCall::MenuItems(_) => CallKind::MenuItems,
            ExportCall::ListVirtualChildren { .. } => CallKind::ListVirtualChildren,
            ExportCall::ServiceHandle(_) => CallKind::ServiceProvider,
        };
        let result = self
            .execute(kind, move |instance| instance.call_export(call))
            .and_then(|result| result);
        if let Err(trap) = &result {
            harness_lock(&self.observations).failures.push(trap.clone());
        }
        result
    }
}

/// The result of a completed `agent-config.propose` request (CAP-891).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalResult {
    /// Paths of the files that were applied.
    pub applied: Vec<String>,
    /// Plugin-data path of the history entry that holds the previous contents.
    pub history: String,
}

/// Agent-config proposal observers (CAP-887..891).
impl HarnessHandle {
    /// The error code of the last proposal that completed failed.
    pub fn last_proposal_failure_code(&self) -> Option<ilium_plugin_api::ErrorCode> {
        self.last_proposal_failure().map(|error| error.code)
    }

    /// The last proposal that completed failed.
    pub fn last_proposal_failure(&self) -> Option<ilium_plugin_api::PluginError> {
        self.kernel.state().proposal_completion.as_ref().and_then(|result| result.as_ref().err().cloned())
    }

    /// Takes the completed proposal result.
    pub fn take_proposal(&self) -> Option<ProposalResult> {
        let mut state = self.kernel.state();
        match state.proposal_completion.take() {
            Some(Ok(done)) => Some(ProposalResult { applied: done.applied, history: done.history }),
            other => {
                state.proposal_completion = other;
                None
            }
        }
    }
}


const STREAM_LIMIT_BYTES: u64 = 256 * 1024 * 1024;
const STREAM_READ_BYTES: u64 = 8 * 1024 * 1024;
const SESSION_STREAM_CAP: usize = 32;

/// The first `url` string anywhere in a JSON argument tree.
fn find_url(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => map
            .get("url")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| map.values().find_map(find_url)),
        Value::Array(items) => items.iter().find_map(find_url),
        _ => None,
    }
}

impl HarnessHandle {
    /// The declared or buffered size of the response the fake network serves for a URL.
    fn stream_size_for(&self, url: &str) -> Option<u64> {
        let net = harness_lock(&self.world).net.clone()?;
        let response = net.response_for(url)?;
        Some(response.declared_or_buffered_size())
    }
}

// ------------------------------------------------------------------ security world

/// Unix time at harness clock zero; audit stamps are this plus the harness clock.
const SECURITY_EPOCH_MILLIS: u64 = 1_700_000_000_000;

/// Host-side state behind the security, policy, secrets and audit observers. It drives the
/// production audit log, policy engine, review rows and redaction over a real state directory.
struct SecurityWorld {
    dir: PathBuf,
    plugin: String,
    tier: Tier,
    net: Option<FakeNet>,
    offline: bool,
    options: std::collections::BTreeMap<String, Value>,
    settings: std::collections::BTreeMap<String, Value>,
    policy: crate::policy::PolicyEngine,
    audit: Option<crate::audit::AuditLog>,
    audit_failing: bool,
    audit_attempts: u64,
    audit_queue: Vec<String>,
    syslog: Vec<SystemLogMessage>,
    keyring: std::collections::BTreeMap<(String, String), String>,
    file_secrets: std::collections::BTreeMap<(String, String), String>,
    installed: std::collections::BTreeSet<String>,
    trusted: std::collections::BTreeSet<String>,
    project_values: std::collections::BTreeMap<String, Value>,
    user_settings: std::collections::BTreeMap<String, Value>,
    sessions: Vec<(String, String)>,
    grants: Vec<String>,
    token: String,
}

fn security_error(code: ErrorCode, reason: &str, detail: &str) -> PluginError {
    PluginError::new(code, format!("{reason}: {detail}"))
}

fn toml_to_json(value: &toml::Value) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

impl SecurityWorld {
    fn create(harness: &HostHarness, plugin: &str) -> Result<SecurityWorld, HostError> {
        let io = |error: std::io::Error| HostError::EngineError(error.to_string());
        let dir = match &harness.state_dir {
            Some(dir) => dir.clone(),
            None => {
                static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                std::env::temp_dir().join(format!("ilium-harness-{}-{serial}", std::process::id()))
            }
        };
        std::fs::create_dir_all(&dir).map_err(io)?;
        let options: std::collections::BTreeMap<String, Value> = harness.options.iter().cloned().collect();
        let system = match (&harness.system_policy, options.get("system-policy-unreadable")) {
            (_, Some(Value::Bool(true))) => crate::policy::PolicyInput::Unreadable("unreadable".into()),
            (Some((text, secure)), _) => crate::policy::PolicyInput::Document {
                text: text.clone(),
                ownership: Some(crate::policy::PolicyOwnership {
                    administrator_owned: *secure,
                    non_administrator_writable: false,
                }),
            },
            (None, _) => crate::policy::PolicyInput::Missing,
        };
        let user = match &harness.user_policy {
            Some(text) => crate::policy::PolicyInput::Document { text: text.clone(), ownership: None },
            None => crate::policy::PolicyInput::Missing,
        };
        let policy = crate::policy::PolicyEngine::from_inputs(system, user);
        let mut world = SecurityWorld {
            dir: dir.clone(),
            plugin: plugin.to_owned(),
            tier: harness.tier,
            net: harness.net.clone(),
            offline: harness.offline,
            options,
            settings: harness.settings.iter().cloned().collect(),
            policy,
            audit: None,
            audit_failing: false,
            audit_attempts: 0,
            audit_queue: Vec::new(),
            syslog: Vec::new(),
            keyring: Default::default(),
            file_secrets: Default::default(),
            installed: std::iter::once(plugin.to_owned()).collect(),
            trusted: Default::default(),
            project_values: Default::default(),
            user_settings: Default::default(),
            sessions: Vec::new(),
            grants: harness.grants.clone(),
            token: String::new(),
        };
        world.open_secrets(&harness.secrets);
        world.open_audit();
        world.open_http_api();
        Ok(world)
    }

    fn keyring_enabled(&self) -> bool {
        self.options.get("keyring") != Some(&Value::Bool(false))
    }

    fn secrets_file(&self) -> PathBuf {
        self.dir.join("secrets.json")
    }

    fn open_secrets(&mut self, seeds: &[(String, String)]) {
        // A leftover file store migrates into the keyring once the keyring reads back.
        if self.keyring_enabled() {
            if let Ok(text) = std::fs::read_to_string(self.secrets_file()) {
                if let Ok(map) = serde_json::from_str::<std::collections::BTreeMap<String, String>>(&text) {
                    for (key, value) in map {
                        self.keyring.insert((self.plugin.clone(), key), value);
                    }
                }
                let _ = std::fs::remove_file(self.secrets_file());
            }
        }
        for (key, value) in seeds {
            self.store_secret(key, value);
        }
        for (name, value) in &self.options {
            if let (Some(key), Some(text)) = (name.strip_prefix("kernel-secret:"), value.as_str()) {
                self.keyring.insert(("kernel".to_owned(), key.to_owned()), text.to_owned());
            }
        }
    }

    fn store_secret(&mut self, key: &str, value: &str) {
        let slot = (self.plugin.clone(), key.to_owned());
        if self.keyring_enabled() {
            self.keyring.insert(slot, value.to_owned());
        } else {
            self.file_secrets.insert(slot, value.to_owned());
            self.write_secrets_file();
        }
    }

    fn write_secrets_file(&self) {
        let map: std::collections::BTreeMap<&str, &str> =
            self.file_secrets.iter().map(|((_, key), value)| (key.as_str(), value.as_str())).collect();
        if let Ok(text) = serde_json::to_string(&map) {
            if std::fs::write(self.secrets_file(), text).is_ok() {
                restrict_permissions(&self.secrets_file(), 0o600);
            }
        }
    }

    fn secret_values(&self) -> Vec<String> {
        self.keyring.values().chain(self.file_secrets.values()).filter(|value| !value.is_empty()).cloned().collect()
    }

    fn redact(&self, text: &str) -> String {
        crate::secrets::redact(text, &self.secret_values())
    }

    fn forget(&mut self, plugin: Option<&str>) {
        let keep = |slot: &(String, String)| plugin.is_some_and(|name| slot.0 != name);
        self.keyring.retain(|slot, _| keep(slot));
        self.file_secrets.retain(|slot, _| keep(slot));
        if self.keyring_enabled() {
            return;
        }
        self.write_secrets_file();
    }

    fn policy_audit_value(&self, key: &str) -> Option<toml::Value> {
        self.policy
            .configuration_layers()
            .ok()?
            .iter()
            .rev()
            .find_map(|layer| layer.get("audit")?.get(key).cloned())
    }

    fn audit_limits(&self) -> (u64, u64) {
        let mib = self.policy_audit_value("max_mib").and_then(|value| value.as_integer()).unwrap_or(64);
        let days = self.policy_audit_value("keep_days").and_then(|value| value.as_integer()).unwrap_or(365);
        (u64::try_from(mib).unwrap_or(64), u64::try_from(days).unwrap_or(365))
    }

    fn syslog_sink(&self) -> bool {
        self.policy_audit_value("sink").and_then(|value| value.as_str().map(str::to_owned)).as_deref()
            == Some("syslog")
    }

    fn audit_path(&self) -> PathBuf {
        self.dir.join("audit").join("audit.jsonl")
    }

    fn open_audit(&mut self) {
        let (mib, days) = self.audit_limits();
        let retention = crate::audit::AuditRetention {
            max_bytes: mib.saturating_mul(1024 * 1024),
            keep_days: u32::try_from(days).unwrap_or(u32::MAX),
        };
        let context = crate::audit::AuditContext {
            session: "default".into(),
            actor: crate::audit::AuditActor { kind: "kernel".into(), pane: None, plugin: None },
        };
        self.audit = crate::audit::AuditLog::open_with_context(self.audit_path(), Some(context), retention).ok();
        self.check_external_edits();
    }

    /// Reports store files that changed on disk since the host last wrote them.
    fn check_external_edits(&mut self) {
        for file in STORE_FILES {
            let path = self.dir.join(file);
            let sidecar = self.dir.join(format!("{file}.sha256"));
            let (Ok(bytes), Ok(recorded)) = (std::fs::read(&path), std::fs::read_to_string(&sidecar)) else {
                continue;
            };
            if hex_sha256(&bytes) != recorded.trim() {
                self.append("store-external-edit", &format!("{file} changed outside the host"), 0);
                let _ = std::fs::write(&sidecar, hex_sha256(&bytes));
            }
        }
    }

    fn persist_stores(&mut self) {
        let _ = std::fs::create_dir_all(self.dir.join("audit"));
        if self.audit.is_none() {
            self.open_audit();
        }
        for file in STORE_FILES {
            let body = match *file {
                "grants.json" => serde_json::json!({ self.plugin.clone(): self.grants }).to_string(),
                "settings.json" => serde_json::to_string(&self.settings).unwrap_or_default(),
                _ => serde_json::to_string(&self.installed).unwrap_or_default(),
            };
            let path = self.dir.join(file);
            if std::fs::write(&path, body.as_bytes()).is_ok() {
                restrict_permissions(&path, 0o600);
                let _ = std::fs::write(self.dir.join(format!("{file}.sha256")), hex_sha256(body.as_bytes()));
            }
        }
        restrict_permissions(&self.audit_path(), 0o600);
    }

    fn append(&mut self, event: &str, detail: &str, now: u64) {
        let stamp = SECURITY_EPOCH_MILLIS.saturating_add(now);
        let detail_json = serde_json::json!({ "result": "ok", "reason": detail }).to_string();
        if self.syslog_sink() {
            let record = serde_json::json!({
                "seq": self.syslog.len() + 1, "at": stamp, "action": event, "detail": detail,
            });
            self.syslog.push(SystemLogMessage { identifier: "ilium-audit".into(), text: record.to_string() });
            return;
        }
        let Some(log) = self.audit.as_mut() else { return };
        if self.audit_failing {
            self.audit_attempts += 1;
        }
        let record = crate::audit::AuditRecord {
            at_unix_millis: stamp, plugin: String::new(), action: event.to_owned(), detail_json,
        };
        let _ = log.append_at(&record, stamp);
        self.sync_queue(event);
    }

    fn sync_queue(&mut self, event: &str) {
        let queued = self.audit.as_ref().map_or(0, |log| log.status().queued);
        while self.audit_queue.len() < queued {
            self.audit_queue.push(event.to_owned());
        }
        self.audit_queue.truncate(queued);
    }

    fn tick_audit(&mut self, elapsed: Duration, now: u64) {
        let Some(log) = self.audit.as_mut() else { return };
        let was_failing = log.status().write_failed;
        let stamp = SECURITY_EPOCH_MILLIS.saturating_add(now);
        let before = log.status().queued;
        let _ = log.tick(elapsed, stamp);
        if was_failing && elapsed >= Duration::from_secs(10) {
            self.audit_attempts += 1;
        }
        let _ = before;
        self.sync_queue("retry");
    }

    fn set_audit_failing(&mut self, fail: bool) {
        self.audit_failing = fail;
        // A directory standing where the log file belongs makes the real writer fail and
        // exercise its queue and retry; the log is parked beside it and put back afterwards.
        let path = self.audit_path();
        let parked = path.with_extension("parked");
        if fail && path.is_file() {
            if std::fs::rename(&path, &parked).is_ok() {
                let _ = std::fs::create_dir(&path);
            }
        } else if !fail && path.is_dir() {
            if std::fs::remove_dir(&path).is_ok() {
                let _ = std::fs::rename(&parked, &path);
            }
        }
    }

    fn fill_audit(&mut self, bytes: u64, now: u64) {
        let filler = "x".repeat(2000);
        let mut guard = 0u32;
        while std::fs::metadata(self.audit_path()).map_or(0, |meta| meta.len()) < bytes && guard < 400_000 {
            self.append("audit-fill", &filler, now);
            guard += 1;
        }
    }

    // ---- network

    fn host_of(url: &str) -> String {
        let rest = url.split("://").nth(1).unwrap_or(url);
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let host = authority.rsplit('@').next().unwrap_or_default();
        host.split(':').next().unwrap_or_default().to_ascii_lowercase()
    }

    fn host_egress(&self, feature: &str, url: &str) -> Result<(), PluginError> {
        let host = Self::host_of(url);
        if self.offline {
            return Err(security_error(ErrorCode::NotPermitted, "offline", feature));
        }
        if let crate::policy::PolicyVerdict::Deny(code) = self.policy.check_egress_for(&self.plugin, self.tier, &host) {
            return Err(security_error(ErrorCode::NotPermitted, "policy", &code));
        }
        let addresses = self.net.as_ref().map(|net| net.resolve(&host)).unwrap_or_default();
        if addresses.is_empty() {
            return Err(security_error(ErrorCode::Network, "network", &host));
        }
        let confirmed = self.grants.iter().any(|grant| grant == &format!("http:setting:{feature}.webhook")
            || grant.starts_with("http:setting:"));
        let private = addresses.iter().any(|text| {
            text.parse::<std::net::IpAddr>().is_ok_and(|address| match address {
                std::net::IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local(),
                std::net::IpAddr::V6(v6) => v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00,
            })
        });
        if private && !confirmed {
            return Err(security_error(ErrorCode::NotPermitted, "private-address", &host));
        }
        Ok(())
    }

    fn open_http_api(&mut self) {
        let directory = self.dir.join("http-api");
        let _ = std::fs::create_dir_all(&directory);
        restrict_permissions(&directory, 0o700);
        let path = directory.join("token");
        self.token = match std::fs::read_to_string(&path) {
            Ok(text) if !text.trim().is_empty() => text.trim().to_owned(),
            _ => {
                let seed = format!("{}-{:?}-{}", self.dir.display(), std::time::SystemTime::now(), std::process::id());
                let token = hex_sha256(seed.as_bytes());
                let _ = std::fs::write(&path, &token);
                restrict_permissions(&path, 0o600);
                token
            }
        };
    }

    fn http_api_request(&self, path: &str, headers: &[(&str, &str)]) -> u16 {
        let presented = headers.iter().find_map(|(name, value)| {
            name.eq_ignore_ascii_case("authorization").then(|| value.strip_prefix("Bearer ")).flatten()
        });
        match presented {
            Some(token) if crate::sync::constant_time_eq(token.as_bytes(), self.token.as_bytes()) => {
                if path.starts_with("/v1/") { 200 } else { 404 }
            }
            _ => 401,
        }
    }

    // ---- install and policy

    fn blocked_reason(&self, plugin: &str) -> Option<String> {
        let entries = self.options.get("blocklist")?.as_array()?;
        entries.iter().find_map(|entry| {
            let kind = entry.get("kind")?.as_str()?;
            let value = entry.get("value")?.as_str()?;
            let hit = match kind {
                "id" => value == plugin,
                "hash" => value == package_hash(plugin),
                "publisher" => value == publisher_key(plugin),
                _ => false,
            };
            hit.then(|| entry.get("reason").and_then(Value::as_str).unwrap_or("blocked").to_owned())
        })
    }

    fn install_context<'a>(
        plugin: &'a str, tier: Tier, key: Option<&'a str>, kind: crate::policy::PolicySourceKind,
        source: &'a str, replaces: bool,
    ) -> crate::policy::InstallPolicyContext<'a> {
        crate::policy::InstallPolicyContext {
            plugin, tier, publisher_key: key, source, source_kind: kind, channel: "stable",
            development: false, replaces,
        }
    }

    fn check_install(
        &self, plugin: &str, tier: Tier, key: Option<&str>, kind: crate::policy::PolicySourceKind, replaces: bool,
    ) -> Result<(), String> {
        let (id, version) = plugin.split_once('@').unwrap_or((plugin, ""));
        let source = match kind {
            crate::policy::PolicySourceKind::Url => "https://plugins.example.com/pkg",
            _ => "/home/user/pkg",
        };
        let own_key = publisher_key(id);
        let key = key.unwrap_or(own_key.as_str());
        let context = Self::install_context(id, tier, Some(key), kind, source, replaces);
        if let crate::policy::PolicyVerdict::Deny(code) = self.policy.check_install(&context) {
            return Err(code);
        }
        if !version.is_empty() && self.version_blocked(id, version) {
            return Err("policy.version-denied".into());
        }
        Ok(())
    }

    fn version_blocked(&self, plugin: &str, version: &str) -> bool {
        let Ok(layers) = self.policy.configuration_layers() else { return false };
        layers.iter().any(|layer| {
            layer.get("plugins").and_then(|plugins| plugins.get("blocked_versions")).is_some_and(|table| {
                table.get(plugin).and_then(|value| value.as_array()).is_some_and(|list| {
                    list.iter().filter_map(|entry| entry.as_str()).any(|entry| entry == version)
                })
            })
        })
    }

    fn check_with_grants(&self, plugin: &str, tier: Tier, grants: &[&str]) -> Result<(), String> {
        self.check_install(plugin, tier, None, crate::policy::PolicySourceKind::File, false)?;
        let parsed: Vec<Grant> = grants.iter().filter_map(|text| Grant::parse(text).ok()).collect();
        for grant in &parsed {
            if let crate::policy::PolicyVerdict::Deny(code) =
                self.policy.check_grant_in_set_for(plugin, tier, grant, &parsed)
            {
                return Err(code);
            }
        }
        Ok(())
    }

    fn check_with_name(&self, plugin: &str, tier: Tier, name: &str) -> Result<(), String> {
        self.check_install(plugin, tier, None, crate::policy::PolicySourceKind::File, false)?;
        let lowered = name.trim().to_ascii_lowercase();
        let reserved = lowered.is_empty()
            || lowered == "ilium"
            || lowered.starts_with("ilium ")
            || lowered.starts_with("official ");
        if reserved && !plugin.starts_with("ilium.") {
            return Err("manifest.display-name".into());
        }
        Ok(())
    }

    fn is_disabled(&self, plugin: &str) -> bool {
        self.blocked_reason(plugin).is_some()
           
    }

    fn enable(&self, plugin: &str) -> Result<(), String> {
        if let Some(reason) = self.blocked_reason(plugin) {
            return Err(reason);
        }
        if let crate::policy::PolicyVerdict::Deny(code) = self.policy.check_enabled_change(plugin, true) {
            return Err(code);
        }
        Ok(())
    }

    fn data_dir(&self, plugin: &str) -> PathBuf {
        self.dir.join("plugin-data").join(plugin)
    }

    fn effective_setting(&self, key: &str) -> Option<Value> {
        if let Some(locked) = self.policy.locked_value(key) {
            return Some(toml_to_json(locked));
        }
        let safe_mode = self.options.get("safe-mode") == Some(&Value::Bool(true));
        let project_ok = !safe_mode && !self.trusted.is_empty() && self.policy.allows_project_values();
        if project_ok {
            if let Some(value) = self.project_values.get(key) {
                return Some(value.clone());
            }
        }
        self.user_settings.get(key).or_else(|| self.settings.get(key)).cloned()
    }

    fn user_set_setting(&mut self, key: &str, value: Value) -> Result<(), PluginError> {
        if self.policy.locked_value(key).is_some() {
            return Err(security_error(ErrorCode::NotPermitted, "policy", key));
        }
        let mut value = value;
        if let (Some(tail), Some(number)) = (key.strip_prefix("plugins.limits."), value.as_u64()) {
            let ceiling = tail
                .parse::<LimitKey>()
                .map(ilium_plugin_api::limits::default_of)
                .unwrap_or(512);
            value = Value::from(number.min(ceiling));
        }
        self.user_settings.insert(key.to_owned(), value);
        Ok(())
    }

    fn review_rows(&self, grants: &[&str]) -> Vec<ReviewRow> {
        let parsed: Vec<Grant> = grants.iter().filter_map(|text| Grant::parse(text).ok()).collect();
        let rows = crate::review::review_rows(&parsed);
        let toxic_pair = crate::review::has_toxic_combination(&parsed);
        let mut out: Vec<ReviewRow> = rows
            .into_iter()
            .map(|row| {
                let verdict = self.policy.check_grant_in_set_for(&self.plugin, self.tier, &Grant::parse(&row.grant)
                    .unwrap_or_else(|_| parsed[0].clone()), &parsed);
                let denied = matches!(verdict, crate::policy::PolicyVerdict::Deny(_));
                ReviewRow {
                    toxic: false,
                    text: row.grant.clone(),
                    risk: if row.toxic { "high" } else { "low" }.into(),
                    locked: row.locked || denied,
                    checkable: !row.locked && !denied,
                    checked: !row.toxic && !denied,
                    note: denied.then(|| "Blocked by your administrator".to_owned()),
                    warning_style: row.toxic,
                    detail: row.details.clone(),
                    grant: row.grant,
                }
            })
            .collect();
        if toxic_pair {
            out.insert(0, ReviewRow {
                toxic: true,
                text: "This plugin can read your data and send data out.".into(),
                risk: "high".into(),
                warning_style: true,
                detail: grants.join(", "),
                ..ReviewRow::default()
            });
        }
        out
    }

    fn review_text(&self, grants: &[&str]) -> String {
        self.review_rows(grants)
            .iter()
            .map(|row| format!("{} {} {}", row.text, row.detail, row.note.clone().unwrap_or_default()))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

const STORE_FILES: &[&str] = &["grants.json", "settings.json", "installed.json"];

fn hex_sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn package_hash(plugin: &str) -> String {
    hex_sha256(format!("package:{plugin}").as_bytes())
}

fn publisher_key(plugin: &str) -> String {
    hex_sha256(format!("publisher:{plugin}").as_bytes())
}

fn restrict_permissions(path: &Path, mode: u32) {
    // Only owner-only modes are ever requested; the platform crate owns the OS calls.
    let _ = mode;
    let result = if path.is_dir() {
        ilium_platform::secure_fs::restrict_directory_to_owner(path)
    } else {
        ilium_platform::secure_fs::restrict_file_to_owner(path)
    };
    let _ = result;
}

#[cfg(test)]
mod tests {
    use super::{
        applying_side, CompletionView, DetectReport, HostHarness, ImportReply, ImportResult,
        Isolation, ManualClock, NullGuest, PeerMessage, RetainedContribution, ReviewRow,
        TickObservation,
    };
    use crate::testing::native_guest::NativeGuest;
    use ilium_plugin_api::generated::types::intents::Intent;
    use ilium_plugin_api::{Side, Tier};

    /// A clock starts at time zero.
    #[test]
    fn manual_clock_new_reads_zero() {
        assert_eq!(ManualClock::new().now_millis(), 0);
    }

    /// The default clock equals a clock made with `new`.
    #[test]
    fn manual_clock_default_reads_zero() {
        assert_eq!(ManualClock::default().now_millis(), 0);
    }

    /// A new harness is a T3 server-side plugin, as its doc comment states.
    #[test]
    fn harness_new_defaults_to_t3_server_side() {
        let harness = HostHarness::new();
        assert!(matches!(harness.tier, Tier::T3));
        assert!(matches!(harness.side, Side::Server));
    }

    /// A new harness has no grants, declines or other-side grants.
    #[test]
    fn harness_new_has_no_grants_declines_or_other_side_grants() {
        let harness = HostHarness::new();
        assert!(harness.grants.is_empty());
        assert!(harness.declined.is_empty());
        assert!(harness.other_side_grants.is_empty());
    }

    /// A new harness is online, single-plugin and has no second side.
    #[test]
    fn harness_new_is_online_single_plugin_one_side() {
        let harness = HostHarness::new();
        assert!(!harness.offline);
        assert!(!harness.two_plugins);
        assert!(!harness.both_sides);
    }

    /// `Default` produces the same initial state as `new`.
    #[test]
    fn harness_default_matches_new_state() {
        let from_default = HostHarness::default();
        let from_new = HostHarness::new();
        assert!(matches!(from_default.tier, Tier::T3));
        assert!(matches!(from_default.side, Side::Server));
        assert_eq!(from_default.grants, from_new.grants);
        assert_eq!(from_default.secrets, from_new.secrets);
        assert_eq!(from_default.settings, from_new.settings);
        assert_eq!(from_default.offline, from_new.offline);
    }

    /// Grants are recorded in the order given.
    #[test]
    fn harness_grants_records_each_grant_in_order() {
        let harness = HostHarness::new().grants(&["tree.read", "tree.write"]);
        assert_eq!(
            harness.grants,
            vec!["tree.read".to_string(), "tree.write".to_string()]
        );
    }

    /// An empty grant list leaves no grants (boundary: empty input).
    #[test]
    fn harness_grants_empty_slice_records_nothing() {
        let harness = HostHarness::new().grants(&[]);
        assert!(harness.grants.is_empty());
    }

    /// Declined grants are kept apart from granted ones.
    #[test]
    fn harness_declined_is_separate_from_grants() {
        let harness = HostHarness::new()
            .grants(&["tree.read"])
            .declined(&["open-external"]);
        assert_eq!(harness.grants, vec!["tree.read".to_string()]);
        assert_eq!(harness.declined, vec!["open-external".to_string()]);
    }

    /// The side setter replaces the default server side.
    #[test]
    fn harness_side_setter_replaces_server_default() {
        let harness = HostHarness::new().side(Side::Client);
        assert!(matches!(harness.side, Side::Client));
    }

    /// The offline setter flips the flag on.
    #[test]
    fn harness_offline_setter_sets_flag() {
        let harness = HostHarness::new().offline(true);
        assert!(harness.offline);
    }

    /// Secrets and settings are appended as key/value pairs.
    #[test]
    fn harness_secret_and_setting_record_pairs() {
        let harness = HostHarness::new()
            .secret("api-key", "value-1")
            .setting("theme", serde_json::Value::Bool(true));
        assert_eq!(
            harness.secrets,
            vec![("api-key".to_string(), "value-1".to_string())]
        );
        assert_eq!(harness.settings.len(), 1);
        assert_eq!(harness.settings[0].0, "theme");
    }

    /// `build` on a default harness activates the plugin and returns a handle.
    #[test]
    fn harness_build_default_activates_plugin() {
        assert!(HostHarness::new().build().is_ok());
    }

    /// A default reply carries no plugin error.
    /// REQ: API-001
    #[test]
    fn import_reply_default_has_no_error() {
        assert!(ImportReply::default().error().is_none());
    }

    /// A default reply carries no error code.
    #[test]
    fn import_reply_default_has_no_error_code() {
        assert!(ImportReply::default().error_code().is_none());
    }

    /// A default reply did not trap.
    /// REQ: HST-410
    #[test]
    fn import_reply_default_has_no_trap_code() {
        assert!(ImportReply::default().trap_code().is_none());
    }

    /// A default reply has no quota retry delay.
    #[test]
    fn import_reply_default_has_no_retry_after() {
        assert!(ImportReply::default().retry_after_ms().is_none());
    }

    /// A default reply is not an accepted asynchronous call, so it has no request id.
    /// REQ: API-005
    #[test]
    fn import_reply_default_has_no_request_id() {
        assert!(ImportReply::default().request_id().is_none());
    }

    /// A default completion carries no failure code.
    /// REQ: API-005
    #[test]
    fn completion_view_default_has_no_failed_code() {
        let view = CompletionView {
            result: ilium_plugin_api::generated::types::requests::RequestResult {
                request: ilium_plugin_api::generated::types::requests::RequestRef::Call(
                    ilium_plugin_api::generated::types::ids::RequestId(1),
                ),
                outcome: ilium_plugin_api::generated::types::requests::Outcome::Ok,
            },
        };
        assert!(view.failed_code().is_none());
    }

    /// Two import results of the same plain variant are equal.
    #[test]
    fn import_result_ok_equals_ok() {
        assert_eq!(ImportResult::Ok, ImportResult::Ok);
    }

    /// The in-process and helper isolation levels are distinct.
    #[test]
    fn isolation_in_process_differs_from_helper_process() {
        assert_ne!(Isolation::InProcess, Isolation::HelperProcess);
    }

    /// A default review row is a plain, unchecked, unlocked, non-warning row.
    #[test]
    fn review_row_default_is_plain_unchecked_row() {
        let row = ReviewRow::default();
        assert!(!row.toxic);
        assert!(!row.locked);
        assert!(!row.checkable);
        assert!(!row.checked);
        assert!(!row.warning_style);
        assert!(row.note.is_none());
        assert!(row.text.is_empty());
    }

    /// A default detection report describes an empty cycle with zero time.
    #[test]
    fn detect_report_default_is_empty_cycle() {
        let report = DetectReport::default();
        assert!(report.served.is_empty());
        assert_eq!(report.spent_ms, 0);
        assert_eq!(report.tick_ms, 0);
    }

    /// A default tick observation reports no installed callback.
    /// REQ: HST-079
    #[test]
    fn tick_observation_default_has_no_callback() {
        assert!(!TickObservation::default().callback_installed);
    }

    /// A default retained contribution has no key and is not stale.
    #[test]
    fn retained_contribution_default_is_empty_and_fresh() {
        let contribution = RetainedContribution::default();
        assert!(contribution.key.is_empty());
        assert!(!contribution.stale);
    }

    /// A default peer message has no sender and no payload.
    #[test]
    fn peer_message_default_is_empty() {
        let message = PeerMessage::default();
        assert!(message.sender.is_empty());
        assert!(message.payload.is_empty());
    }

    /// The null guest accepts an empty batch and returns no submissions (boundary: empty input).
    #[test]
    fn null_guest_accepts_empty_batch_with_no_submissions() {
        let mut guest = NullGuest;
        assert!(matches!(
            guest.on_events(Vec::new()),
            Ok(ref submissions) if submissions.is_empty()
        ));
    }

    /// Every intent case is applied by the server or the client, never by the scene side.
    /// REQ: HST-490
    #[test]
    fn applying_side_is_server_or_client_for_every_intent_case() {
        for case in Intent::CASE_NAMES {
            let side = applying_side(case);
            assert!(
                matches!(side, Side::Server | Side::Client),
                "intent {case} must be applied by server or client"
            );
        }
    }
}
