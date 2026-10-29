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

/// The reply of one JSON-argument import call (`HarnessHandle::call_import_json`, HST-*/API-*).
#[derive(Debug, Clone, Default)]
pub struct ImportReply;

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
pub struct CompletionView;

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
pub fn applying_side(_case: &str) -> Side {
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
        let wasm_bytes = match (self.wat_text, self.wasm_bytes) {
            (Some(text), None) => Some(wat::parse_str(&text).map_err(|error| {
                HostError::EngineError(format!("invalid component text: {error}"))
            })?),
            (None, bytes) => bytes,
            _ => unreachable!("multiple guest sources were rejected above"),
        };
        // Fail closed for builder options whose adapters have not landed yet.
        if self.both_sides
            || self.two_plugins
            || self.activate_error.is_some()
            || self.session_memory_mib.is_some()
            || self.session_helpers.is_some()
            || self.session_pending_requests.is_some()
            || self.state_dir.is_some()
            || self.net.is_some()
            || self.offline
            || self.system_policy.is_some()
            || self.user_policy.is_some()
            || self.effective_uid.is_some()
            || !self.secrets.is_empty()
            || !self.options.is_empty()
            || self.panic_in_import.is_some()
            || self.tracing
            || self.profiling
            || !self.detector_plugins.is_empty()
            || !self.other_side_grants.is_empty()
        {
            return Err(HostError::EngineError(
                "requested harness adapter is not implemented".into(),
            ));
        }
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
        let plugin = PluginId("test.plugin".into());
        let admission = Arc::new(HarnessAdmission {
            grants: Arc::new(Mutex::new(grants)),
        });
        let dispatcher = Dispatcher::with_admission(
            kernel.clone(),
            DispatchConfig {
                max_pending_per_plugin: resolved[&LimitKey::PendingRequests] as usize,
                max_submissions_per_call: 256,
                dedup_window_ms: 600_000,
            },
            admission.clone(),
        );
        let quota = ilium_execution::QuotaGroup::new(ilium_execution::QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: 9,
            worker_bytes: 32 * 1024 * 1024,
        });
        let scheduler = Scheduler::try_with_config(
            process,
            1,
            crate::calls::ResolvedLimits(resolved.clone()),
            &quota,
        )
        .map_err(|error| HostError::EngineError(error.to_string()))?;
        scheduler
            .register_side_with_limits(&plugin, crate::calls::ResolvedLimits(resolved.clone()));
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
            Box::new(super::native_guest::NativeGuestAdapter::new(
                self.native_guest.unwrap_or_else(|| Box::new(NullGuest)),
            ))
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
                ..HarnessObservations::default()
            }),
            clock: Mutex::new(self.clock.now_millis),
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
struct HarnessAdmission {
    grants: Arc<Mutex<Vec<Grant>>>,
}
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
                Event::SettingsChanged(_)
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
        unimplemented!("host::testing::HarnessHandle::audit")
    }

    /// The instance state.
    pub fn state(&self) -> InstanceState {
        *harness_lock(&self.lifecycle_state)
    }

    /// Moves the manual clock forward and fires due timers.
    pub fn advance_clock(&self, by: Duration) {
        let mut clock = harness_lock(&self.clock);
        *clock = clock.saturating_add(by.as_millis().min(u128::from(u64::MAX)) as u64);
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
    pub fn call_import(&self, _name: &str, _args: Value) -> ImportResult {
        unimplemented!("host::testing::HarnessHandle::call_import")
    }

    /// Calls an import and returns its JSON value (SEC-* value checks).
    pub fn call_import_value(&self, _name: &str, _args: Value) -> Result<Value, PluginError> {
        unimplemented!("host::testing::HarnessHandle::call_import_value")
    }

    /// Calls an import by interface and function with a JSON argument text (HST-*/API-*).
    pub fn call_import_json(
        &self,
        _interface: &str,
        _function: &str,
        _args_json: &str,
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
    pub fn peer(&self, _plugin: &str, _grants: &[&str]) -> HarnessHandle {
        unimplemented!("host::testing::HarnessHandle::peer")
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
        }
        result
    }

    /// The error code of the last failed completion for this request (PRI editor completions).
    pub fn last_completion_failure(&self, _request: RequestId) -> Option<ErrorCode> {
        unimplemented!("host::testing::HarnessHandle::last_completion_failure")
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

    /// Starts a request the kernel never answers (API-073 pending requests).
    pub fn start_slow_request(&self) -> ImportReply {
        unimplemented!("host::testing::HarnessHandle::start_slow_request")
    }

    /// Completes the oldest pending request (API-073).
    pub fn complete_oldest_request(&self) {
        unimplemented!("host::testing::HarnessHandle::complete_oldest_request")
    }

    /// The completion enqueued for a request id (API-005).
    pub fn completion_of(&self, _request: u64) -> Option<CompletionView> {
        unimplemented!("host::testing::HarnessHandle::completion_of")
    }

    /// Pending requests of the plugin (API-073).
    pub fn pending_requests(&self) -> usize {
        unimplemented!("host::testing::HarnessHandle::pending_requests")
    }

    /// Fills the session pending-request budget to this count (API-073).
    pub fn fill_session_pending(&self, _count: u64) {
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
    pub fn forward_latency_of(&self, _correlation: u64) -> Duration {
        unimplemented!("host::testing::HarnessHandle::forward_latency_of")
    }

    /// The client mirror's update index (HST-49x mirror ordering).
    pub fn mirror_update_index(&self) -> u64 {
        unimplemented!("host::testing::HarnessHandle::mirror_update_index")
    }

    /// The mirror update index at which this request completed (HST-49x).
    pub fn request_completed_index(&self, _correlation: u64) -> u64 {
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
    pub fn restart_instance(&self, _reason: DeactivationReason) {
        unimplemented!("host::testing::HarnessHandle::restart_instance")
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
        let mut observations = harness_lock(&self.observations);
        self.scheduler.take_events(&self.plugin, usize::MAX);
        observations.restored = bytes.clone();
        if let Some(trap) = harness_lock(&self.instance).last_trap().cloned() {
            observations.failures.push(trap);
        }
        bytes
    }

    /// Shuts the host down (HST-49x shutdown with pending requests).
    pub fn shutdown(&self) {
        self.deactivate(DeactivationReason::Shutdown);
    }

    /// Adds a second plugin with these grants and returns its handle (HST-49x).
    pub fn add_second_plugin(&self, _grants: &[&str]) -> HarnessHandle {
        unimplemented!("host::testing::HarnessHandle::add_second_plugin")
    }

    /// Activates a second plugin that reserves this much memory (HST-4xx session memory).
    pub fn activate_second_plugin_with_memory(&self, _mib: u64) {
        unimplemented!("host::testing::HarnessHandle::activate_second_plugin_with_memory")
    }

    /// Activates a second plugin that needs a helper process (HST-4xx session helpers).
    pub fn activate_second_plugin_with_helper(&self) {
        unimplemented!("host::testing::HarnessHandle::activate_second_plugin_with_helper")
    }

    /// Frees this much of the session memory budget (HST-4xx session memory).
    pub fn free_session_memory_mib(&self, _mib: u64) {
        unimplemented!("host::testing::HarnessHandle::free_session_memory_mib")
    }

    /// Enables the plugin (HST-40x restart after enable).
    pub fn enable(&self) {
        unimplemented!("host::testing::HarnessHandle::enable")
    }

    /// Disables the plugin (HST-40x).
    pub fn disable(&self) {
        self.deactivate_plugin();
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
    pub fn set_guest(&self, _guest: Box<dyn NativeGuest>) {
        unimplemented!("host::testing::HarnessHandle::set_guest")
    }

    /// Makes the next `deactivate` export return this (HST-402/403 restore bytes).
    pub fn set_deactivate_result(&self, _result: Result<Vec<u8>, Trap>) {
        unimplemented!("host::testing::HarnessHandle::set_deactivate_result")
    }

    /// The restore bytes kept from the last clean deactivate (HST-403).
    pub fn restore_bytes(&self) -> Option<Vec<u8>> {
        harness_lock(&self.observations).restored.clone()
    }

    /// Makes the next `render` export return this (HST-40x invalid ui tree).
    pub fn set_render_result(&self, _result: Result<UiTree, Trap>) {
        unimplemented!("host::testing::HarnessHandle::set_render_result")
    }

    /// Renders the named view (HST-40x).
    pub fn render_view(&self, _view: &str) {
        unimplemented!("host::testing::HarnessHandle::render_view")
    }

    /// Makes the next scene frame fail (HST-40x scene frames).
    pub fn fail_scene_frame(&self) {
        unimplemented!("host::testing::HarnessHandle::fail_scene_frame")
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
    pub fn failure_count_of(&self, _side: Side) -> u32 {
        unimplemented!("host::testing::HarnessHandle::failure_count_of")
    }

    /// Counted failures of the second plugin (HST-4xx).
    pub fn second_plugin_failure_count(&self) -> u32 {
        unimplemented!("host::testing::HarnessHandle::second_plugin_failure_count")
    }

    /// Records one failure on the given side (HST-411).
    pub fn fail_side(&self, _side: Side) {
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
    pub fn seed_retained(&self, _key: &str) {
        unimplemented!("host::testing::HarnessHandle::seed_retained")
    }

    /// Replaces the retained contribution with this key (HST scheduling trap).
    pub fn replace_retained(&self, _key: &str) {
        unimplemented!("host::testing::HarnessHandle::replace_retained")
    }

    /// Retained contributions and their staleness (HST scheduling trap).
    pub fn retained_contributions(&self) -> Vec<RetainedContribution> {
        unimplemented!("host::testing::HarnessHandle::retained_contributions")
    }

    // ---- grants at runtime (CAP-*, HST-069)

    /// Revokes a grant; the plugin is re-activated when the capability was active (CAP-0xx).
    pub fn revoke(&self, _grant: &str) {
        unimplemented!("host::testing::HarnessHandle::revoke")
    }

    /// Revokes a grant without re-activating (HST-069).
    pub fn revoke_without_reactivation(&self, grant: &str) {
        if let Ok(removed) = Grant::parse(grant) {
            harness_lock(&self.admission.grants).retain(|existing| existing != &removed);
        }
    }

    /// Grants a capability now (CAP-0xx, HST-069).
    pub fn grant_now(&self, _grant: &str) {
        unimplemented!("host::testing::HarnessHandle::grant_now")
    }

    /// Declines a capability (HST-069).
    pub fn decline(&self, _grant: &str) {
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
    pub fn charge_cpu_ms(&self, _millis: u64) {
        unimplemented!("host::testing::HarnessHandle::charge_cpu_ms")
    }

    /// The remaining CPU quota in milliseconds (API-07x).
    pub fn quota_balance_ms(&self) -> i64 {
        unimplemented!("host::testing::HarnessHandle::quota_balance_ms")
    }

    /// Events queued for the plugin and not yet delivered (API-074).
    pub fn queued_events(&self) -> Vec<Event> {
        self.scheduler.snapshot_events(&self.plugin)
    }

    /// Timer ticks queued and not yet delivered (API-075).
    pub fn queued_timer_ticks(&self) -> Vec<u64> {
        unimplemented!("host::testing::HarnessHandle::queued_timer_ticks")
    }

    /// Queues a tick of this timer (API-075).
    pub fn queue_timer_tick(&self, _timer: u64) {
        unimplemented!("host::testing::HarnessHandle::queue_timer_tick")
    }

    // ---- inference provider export (WIT:ilium:exports/inference-provider)

    /// Provider calls delivered to the plugin's export.
    pub fn provider_calls(&self) -> Vec<ProviderCall> {
        unimplemented!("host::testing::HarnessHandle::provider_calls")
    }

    /// The models the plugin's provider export reports.
    pub fn set_provider_models(&self, _models: Vec<String>) {
        unimplemented!("host::testing::HarnessHandle::set_provider_models")
    }

    /// Calls the provider `complete` export directly.
    pub fn call_provider_complete(&self, _call: ProviderCall) -> Result<(), PluginError> {
        unimplemented!("host::testing::HarnessHandle::call_provider_complete")
    }

    /// Calls the provider `models` export directly.
    pub fn call_provider_models(&self) -> Result<Vec<String>, PluginError> {
        unimplemented!("host::testing::HarnessHandle::call_provider_models")
    }

    // ---- detectors (PRI-0xx agent signatures, WIT:ilium:exports/detector)

    /// Loads a detector component for the named plugin.
    pub fn load_detector_wasm(&self, _plugin: &str, _wasm: &[u8]) {
        unimplemented!("host::testing::HarnessHandle::load_detector_wasm")
    }

    /// Runs one detection cycle over these panes.
    pub fn detect_cycle(&self, _panes: &[NodeId]) -> DetectReport {
        unimplemented!("host::testing::HarnessHandle::detect_cycle")
    }

    /// Names of the threads the named plugin's detector ran on.
    pub fn detector_thread_names(&self, _plugin: &str) -> Vec<String> {
        unimplemented!("host::testing::HarnessHandle::detector_thread_names")
    }

    /// The name of the detection loop thread.
    pub fn detection_loop_thread_name(&self) -> String {
        unimplemented!("host::testing::HarnessHandle::detection_loop_thread_name")
    }

    /// How often the named plugin's detector was called.
    pub fn detector_call_count(&self, _plugin: &str) -> u32 {
        unimplemented!("host::testing::HarnessHandle::detector_call_count")
    }

    /// The plugin ids asked for this pane, in order.
    pub fn detector_ask_order(&self, _pane: NodeId) -> Vec<String> {
        unimplemented!("host::testing::HarnessHandle::detector_ask_order")
    }

    /// Makes the named detector spend this long per call.
    pub fn set_detector_cost_ms(&self, _plugin: &str, _millis: u64) {
        unimplemented!("host::testing::HarnessHandle::set_detector_cost_ms")
    }

    /// Makes the named detector identify this signature, or none.
    pub fn set_detector_identity(&self, _plugin: &str, _signature: Option<&str>) {
        unimplemented!("host::testing::HarnessHandle::set_detector_identity")
    }

    /// Makes the named detector classify panes as this activity.
    pub fn set_detector_classification(&self, _plugin: &str, _activity: AgentActivity) {
        unimplemented!("host::testing::HarnessHandle::set_detector_classification")
    }

    /// The signature identified for this pane.
    pub fn identified_signature(&self, _pane: NodeId) -> Option<String> {
        unimplemented!("host::testing::HarnessHandle::identified_signature")
    }

    /// The activity the detectors assigned to this pane.
    pub fn pane_activity(&self, _pane: NodeId) -> Option<AgentActivity> {
        unimplemented!("host::testing::HarnessHandle::pane_activity")
    }

    /// Activity events emitted so far.
    pub fn activity_events_emitted(&self) -> u32 {
        unimplemented!("host::testing::HarnessHandle::activity_events_emitted")
    }

    /// Deadline breaches of the named detector.
    pub fn detector_breaches(&self, _plugin: &str) -> u32 {
        unimplemented!("host::testing::HarnessHandle::detector_breaches")
    }

    /// Sets (with `Some`) and returns the named detector's deadline in milliseconds.
    pub fn detector_deadline_ms(&self, _plugin: &str, _millis: Option<u64>) -> u64 {
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
    pub fn run_hostile_probe(&self, _who: &str, _attempt: &str) -> String {
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
    pub fn inject_helper_message(&self, _bytes: &[u8]) {
        unimplemented!("host::testing::HarnessHandle::inject_helper_message")
    }

    /// Delivers a peer message from the named sender (SEC-01x).
    pub fn deliver_peer_message(&self, _sender: &str, _payload: &[u8]) {
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
    pub fn host_egress(&self, _feature: &str, _url: &str) -> Result<(), PluginError> {
        unimplemented!("host::testing::HarnessHandle::host_egress")
    }

    /// The close reason of an HTTP stream (NET streams).
    pub fn stream_close_reason(&self, _stream: u64) -> Option<String> {
        unimplemented!("host::testing::HarnessHandle::stream_close_reason")
    }

    /// Bytes delivered on an HTTP stream (NET streams).
    pub fn stream_bytes_delivered(&self, _stream: u64) -> u64 {
        unimplemented!("host::testing::HarnessHandle::stream_bytes_delivered")
    }

    /// The largest single read on an HTTP stream (NET streams).
    pub fn stream_largest_read(&self, _stream: u64) -> u64 {
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
    pub fn http_api_request(&self, _path: &str, _headers: &[(&str, &str)]) -> u16 {
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
    pub fn audit_append(&self, _event: &str, _detail: &str) {
        unimplemented!("host::testing::HarnessHandle::audit_append")
    }

    /// Pads the audit log to this size in bytes.
    pub fn audit_fill_to(&self, _bytes: u64) {
        unimplemented!("host::testing::HarnessHandle::audit_fill_to")
    }

    /// The audit limits (size in MiB, age in days).
    pub fn audit_limits(&self) -> (u64, u64) {
        unimplemented!("host::testing::HarnessHandle::audit_limits")
    }

    /// Makes audit writes fail (or succeed again).
    pub fn fail_audit_writes(&self, _fail: bool) {
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
    pub fn edit_store_file_externally(&self, _file: &str) {
        unimplemented!("host::testing::HarnessHandle::edit_store_file_externally")
    }

    /// Writes every store to disk.
    pub fn persist_all_stores(&self) {
        unimplemented!("host::testing::HarnessHandle::persist_all_stores")
    }

    /// Pads the privacy log to this size in bytes (SEC privacy log).
    pub fn privacy_log_fill_to(&self, _bytes: u64) {
        unimplemented!("host::testing::HarnessHandle::privacy_log_fill_to")
    }

    // ---- secrets and settings (SEC secrets, POL-*)

    /// Logs a line as the plugin would.
    pub fn log_from_plugin(&self, _text: &str) {
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
    pub fn keyring_value(&self, _plugin: &str, _key: &str) -> Option<String> {
        unimplemented!("host::testing::HarnessHandle::keyring_value")
    }

    /// Marks a project as trusted.
    pub fn trust_project(&self, _path: &str) {
        unimplemented!("host::testing::HarnessHandle::trust_project")
    }

    /// Writes a plugin setting into a project settings file.
    pub fn write_project_value(&self, _project: &str, _plugin: &str, _key: &str, _value: Value) {
        unimplemented!("host::testing::HarnessHandle::write_project_value")
    }

    /// The effective value of a plugin setting.
    pub fn effective_setting(&self, _key: &str) -> Option<Value> {
        unimplemented!("host::testing::HarnessHandle::effective_setting")
    }

    /// Sets a kernel setting as the user would (POL-* locked settings).
    pub fn user_set_setting(&self, _key: &str, _value: Value) -> Result<(), PluginError> {
        unimplemented!("host::testing::HarnessHandle::user_set_setting")
    }

    /// An error string after secret redaction.
    pub fn error_string_for(&self, _text: &str) -> String {
        unimplemented!("host::testing::HarnessHandle::error_string_for")
    }

    /// A trap message after secret redaction.
    pub fn trap_message_for(&self, _text: &str) -> String {
        unimplemented!("host::testing::HarnessHandle::trap_message_for")
    }

    /// Replaces a secret value now.
    pub fn set_secret_now(&self, _key: &str, _value: &str) {
        unimplemented!("host::testing::HarnessHandle::set_secret_now")
    }

    /// The CLI's JSON output text.
    pub fn cli_json_text(&self) -> String {
        unimplemented!("host::testing::HarnessHandle::cli_json_text")
    }

    /// The payload text sent on the named channel.
    pub fn channel_payload_text(&self, _channel: &str) -> String {
        unimplemented!("host::testing::HarnessHandle::channel_payload_text")
    }

    /// The stored form of all secrets.
    pub fn stored_secret_text(&self) -> String {
        unimplemented!("host::testing::HarnessHandle::stored_secret_text")
    }

    /// The consent prompt shown for this URL.
    pub fn consent_prompt_text_for_url(&self, _url: &str) -> String {
        unimplemented!("host::testing::HarnessHandle::consent_prompt_text_for_url")
    }

    /// Forgets the secrets of one plugin, or of all with `None`.
    pub fn forget_secrets(&self, _plugin: Option<&str>) {
        unimplemented!("host::testing::HarnessHandle::forget_secrets")
    }

    /// The secrets screen text.
    pub fn secrets_screen_text(&self) -> String {
        unimplemented!("host::testing::HarnessHandle::secrets_screen_text")
    }

    /// A diagnostics report with this extra section text.
    pub fn diagnostics_report_with_section(&self, _text: &str) -> DiagnosticsSection {
        unimplemented!("host::testing::HarnessHandle::diagnostics_report_with_section")
    }

    /// Restores a session snapshot from its text.
    pub fn restore_snapshot(&self, _text: &str) {
        unimplemented!("host::testing::HarnessHandle::restore_snapshot")
    }

    /// The current value of a secret.
    pub fn secret_value(&self, _key: &str) -> Option<String> {
        unimplemented!("host::testing::HarnessHandle::secret_value")
    }

    // ---- installation, policy and review (SEC-*, POL-*, PKG-*)

    /// The plugin details text with this security contact.
    pub fn details_text_with_contact(&self, _contact: &str) -> String {
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
    pub fn plugin_disabled(&self, _plugin: &str) -> bool {
        unimplemented!("host::testing::HarnessHandle::plugin_disabled")
    }

    /// Writes into the plugin's data directory.
    pub fn write_plugin_data(&self, _name: &str, _bytes: &[u8]) {
        unimplemented!("host::testing::HarnessHandle::write_plugin_data")
    }

    /// Reads from the plugin's data directory.
    pub fn read_plugin_data(&self, _name: &str) -> Option<Vec<u8>> {
        unimplemented!("host::testing::HarnessHandle::read_plugin_data")
    }

    /// Writes into another plugin's data directory.
    pub fn write_other_plugin_data(&self, _plugin: &str, _name: &str, _bytes: &[u8]) {
        unimplemented!("host::testing::HarnessHandle::write_other_plugin_data")
    }

    /// Uninstalls the named plugin.
    pub fn uninstall(&self, _plugin: &str) -> Result<(), String> {
        unimplemented!("host::testing::HarnessHandle::uninstall")
    }

    /// Whether the named plugin is installed.
    pub fn is_installed(&self, _plugin: &str) -> bool {
        unimplemented!("host::testing::HarnessHandle::is_installed")
    }

    /// Enables the named plugin (SEC blocked plugins).
    pub fn enable_plugin(&self, _plugin: &str) -> Result<(), String> {
        unimplemented!("host::testing::HarnessHandle::enable_plugin")
    }

    /// Whether the named plugin's replacement of a core feature is active.
    pub fn replacement_active(&self, _plugin: &str) -> bool {
        unimplemented!("host::testing::HarnessHandle::replacement_active")
    }

    /// Checks whether a plugin may be installed at this tier (POL-*).
    pub fn install_check(&self, _plugin: &str, _tier: Tier) -> Result<(), String> {
        unimplemented!("host::testing::HarnessHandle::install_check")
    }

    /// Install check for a plugin fetched from a URL (POL-* source policy).
    pub fn install_check_from_url(&self, _plugin: &str, _tier: Tier) -> Result<(), String> {
        unimplemented!("host::testing::HarnessHandle::install_check_from_url")
    }

    /// Install check for a plugin signed by this publisher fingerprint (POL-*).
    pub fn install_check_signed_by(&self, _plugin: &str, _fingerprint: &str) -> Result<(), String> {
        unimplemented!("host::testing::HarnessHandle::install_check_signed_by")
    }

    /// Install check for a plugin replacing this core feature (POL-*).
    pub fn install_check_replacing(&self, _plugin: &str, _feature: &str) -> Result<(), String> {
        unimplemented!("host::testing::HarnessHandle::install_check_replacing")
    }

    /// Install check with these requested grants (SEC-004 toxic pairs).
    pub fn install_check_with_grants(
        &self,
        _plugin: &str,
        _tier: Tier,
        _grants: &[&str],
    ) -> Result<(), String> {
        unimplemented!("host::testing::HarnessHandle::install_check_with_grants")
    }

    /// Install check with this display name (PKG display-name rules).
    pub fn install_check_with_name(
        &self,
        _plugin: &str,
        _tier: Tier,
        _name: &str,
    ) -> Result<(), String> {
        unimplemented!("host::testing::HarnessHandle::install_check_with_name")
    }

    /// Installs a plugin from a system directory into the user install dir (POL-*).
    pub fn install_from_system_dir(
        &self,
        _directory: &Path,
        _plugin: &str,
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
    pub fn add_session(&self, _name: &str, _owner: &str) {
        unimplemented!("host::testing::HarnessHandle::add_session")
    }

    /// Replaces the system policy file text (POL-*).
    pub fn replace_system_policy_file(&self, _text: &str) {
        unimplemented!("host::testing::HarnessHandle::replace_system_policy_file")
    }

    /// Tries to check a locked review row (POL-* locked grants).
    pub fn try_check_row(&self, _grant: &str) -> bool {
        unimplemented!("host::testing::HarnessHandle::try_check_row")
    }

    /// Review rows for these requested grants (SEC-004).
    pub fn review_rows(&self, _grants: &[&str]) -> Vec<ReviewRow> {
        unimplemented!("host::testing::HarnessHandle::review_rows")
    }

    /// Review text for these requested grants (SEC-007).
    pub fn review_text_for(&self, _grants: &[&str]) -> String {
        unimplemented!("host::testing::HarnessHandle::review_text_for")
    }

    /// Review text of the installed plugin (SEC secrets in review).
    pub fn review_text(&self) -> String {
        unimplemented!("host::testing::HarnessHandle::review_text")
    }

    /// Review text with these `data-use` manifest entries (SEC privacy).
    pub fn review_text_with_data_use(&self, _entries: &[Value]) -> String {
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
    pub fn plugin_draw_rows(&self, _rows: &[&str]) -> Result<(), PluginError> {
        unimplemented!("host::testing::HarnessHandle::plugin_draw_rows")
    }

    /// Opens a kernel dialog of this kind (`review`, `trust`, `consent`).
    pub fn open_kernel_dialog(&self, _kind: &str) {
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
    pub fn dialog_is_topmost(&self, _kind: &str) -> bool {
        unimplemented!("host::testing::HarnessHandle::dialog_is_topmost")
    }

    /// Sets the plugin's status bar segment text.
    pub fn plugin_status_segment(&self, _text: &str) {
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
    pub fn set_user_typing_in_terminal(&self, _typing: bool) {
        unimplemented!("host::testing::HarnessHandle::set_user_typing_in_terminal")
    }

    /// Presses the dialog's confirmation key.
    pub fn press_dialog_key(&self) {
        unimplemented!("host::testing::HarnessHandle::press_dialog_key")
    }

    /// Types these keys into the focused surface.
    pub fn type_keys(&self, _keys: &str) {
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
    pub fn render_plugin_chrome(&self, _kind: &str, _text: &str) -> String {
        unimplemented!("host::testing::HarnessHandle::render_plugin_chrome")
    }

    /// Renders plugin text of this kind (SEC chrome spoofing).
    pub fn render_plugin_text(&self, _kind: &str, _text: &str) -> String {
        unimplemented!("host::testing::HarnessHandle::render_plugin_text")
    }

    /// Renders a kernel dialog at this width and theme, one row per line (SEC-346).
    pub fn render_dialog(&self, _kind: &str, _columns: u16, _dark: bool) -> Vec<String> {
        unimplemented!("host::testing::HarnessHandle::render_dialog")
    }

    /// The screen-reader text of a kernel dialog (SEC-346).
    pub fn screen_reader_text(&self, _kind: &str) -> String {
        unimplemented!("host::testing::HarnessHandle::screen_reader_text")
    }

    /// The cells of a kernel dialog, one row per line.
    pub fn dialog_cells(&self, _kind: &str) -> Vec<String> {
        unimplemented!("host::testing::HarnessHandle::dialog_cells")
    }

    /// What the plugin captured of the screen.
    pub fn plugin_screen_capture(&self) -> Vec<String> {
        unimplemented!("host::testing::HarnessHandle::plugin_screen_capture")
    }

    /// Writes pane output as the terminal would (SEC screen-changed events).
    pub fn pane_output(&self, _text: &str) {
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
    pub fn set_pane_echo(&self, _echo: bool) {
        unimplemented!("host::testing::HarnessHandle::set_pane_echo")
    }
}

impl HarnessHandle {
    /// The instance state of one side of the harness plugin (reference-errors-and-limits.md).
    pub fn state_of(&self, _side: Side) -> InstanceState {
        unimplemented!("host::testing::HarnessHandle::state_of")
    }

    /// The instance state of the second plugin (reference-errors-and-limits.md host capacity).
    pub fn second_plugin_state(&self) -> InstanceState {
        unimplemented!("host::testing::HarnessHandle::second_plugin_state")
    }

    /// Why the second plugin is not active, such as `host-capacity`.
    pub fn second_plugin_reason(&self) -> Option<String> {
        unimplemented!("host::testing::HarnessHandle::second_plugin_reason")
    }

    /// Opens an HTTP stream as plugin `plugin_index` of a two-plugin harness (API-070).
    pub fn open_stream_as(&self, _plugin_index: usize) -> ImportReply {
        unimplemented!("host::testing::HarnessHandle::open_stream_as")
    }

    /// Calls an import as plugin `plugin_index` of a two-plugin harness with JSON arguments.
    pub fn call_import_as(
        &self,
        _plugin_index: usize,
        _interface: &str,
        _function: &str,
        _args_json: &str,
    ) -> ImportReply {
        unimplemented!("host::testing::HarnessHandle::call_import_as")
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
