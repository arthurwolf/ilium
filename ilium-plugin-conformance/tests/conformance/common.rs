//! Shared helpers: scripted guests for the host harness, event and submission builders,
//! fixture loading, and the CLI environment.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::intents::{Intent, Submission};
use ilium_plugin_api::generated::types::lifecycle::{Activation, DeactivationReason};
use ilium_plugin_api::generated::types::requests::RequestResult;
use ilium_plugin_api::ids::PluginId;
use ilium_plugin_api::{Code, PluginError};
use ilium_plugin_host::calls::Trap;
use ilium_plugin_host::cli::{CliEnv, CliOutput};
use ilium_plugin_host::registry_client::{FetchError, Fetcher};
use ilium_plugin_host::testing::{HarnessHandle, HostHarness, NativeGuest};

/// Fake-clock and wall limits copied from reference-conformance-suite.md (DEV-606).
pub const CASE_WALL_LIMIT_MS: u64 = 10_000;

/// Longest real wait any test performs; every loop is bounded by it.
pub const BOUND: Duration = Duration::from_secs(5);

/// The plugin id every harness test uses; matches `fixtures/manifests/minimal.json`.
pub fn plugin_id() -> PluginId {
    PluginId("com.example.conf".to_string())
}

/// Absolute path of a manifest fixture of this crate.
pub fn manifest_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures").join("manifests").join(name)
}

/// Text of a manifest fixture of this crate.
pub fn manifest_text(name: &str) -> String {
    std::fs::read_to_string(manifest_path(name))
        .unwrap_or_else(|error| panic!("fixture.missing: manifest {name}: {error}"))
}

/// Path of a built guest fixture component. A missing file is a red failure with
/// `fixture.missing`, never a skip (test-plan.md 0.5).
pub fn fixture_component(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("target").join("plugin-fixtures");
    let path = root.join(format!("{name}.wasm"));
    assert!(path.is_file(), "fixture.missing: {}", path.display());
    path
}

/// A valid event that needs no grant.
pub fn theme_event() -> Event {
    Event::ThemeChanged
}

/// `count` distinct timer events (each keyed by its own timer, so none coalesce).
pub fn timer_events(count: usize) -> Vec<Event> {
    (0..count as u64).map(|n| Event::TimerFired(ilium_plugin_api::ids::TimerId(n + 1))).collect()
}

/// A `play-sound` submission with a plugin-owned correlation number.
pub fn play_sound(correlation: u64, dedup_key: Option<&str>) -> Submission {
    Submission {
        intent: Intent::PlaySound("click".to_string()),
        correlation,
        dedup_key: dedup_key.map(str::to_string),
    }
}

/// What the scripted guest saw, in call order.
#[derive(Debug, Default)]
pub struct Recorder {
    /// Names of the calls: `activate`, `on-events`, `deactivate:<reason>`.
    pub calls: Vec<String>,
    /// Number of events in each `on-events` batch.
    pub batch_sizes: Vec<usize>,
    /// Every event, in delivery order.
    pub events: Vec<Event>,
    /// Reasons passed to `deactivate`.
    pub reasons: Vec<DeactivationReason>,
}

/// How a scripted guest answers one `on-events` batch.
pub type Answer = Box<dyn FnMut(&[Event]) -> Result<Vec<Submission>, Trap> + Send>;

/// A guest that records its calls and answers from a closure.
pub struct ScriptedGuest {
    pub recorder: Arc<Mutex<Recorder>>,
    pub activate_error: Option<PluginError>,
    pub answer: Answer,
}

impl ScriptedGuest {
    /// A guest that accepts activation and returns no submissions.
    pub fn quiet() -> (ScriptedGuest, Arc<Mutex<Recorder>>) {
        let recorder = Arc::new(Mutex::new(Recorder::default()));
        let guest = ScriptedGuest {
            recorder: Arc::clone(&recorder),
            activate_error: None,
            answer: Box::new(|_| Ok(Vec::new())),
        };
        (guest, recorder)
    }

    /// A guest that answers every batch with `answer`.
    pub fn answering(answer: Answer) -> (ScriptedGuest, Arc<Mutex<Recorder>>) {
        let (mut guest, recorder) = ScriptedGuest::quiet();
        guest.answer = answer;
        (guest, recorder)
    }
}

impl NativeGuest for ScriptedGuest {
    fn activate(&mut self, _activation: Activation) -> Result<(), PluginError> {
        self.recorder.lock().expect("recorder lock").calls.push("activate".to_string());
        match self.activate_error.clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn deactivate(&mut self, reason: DeactivationReason) -> Option<Vec<u8>> {
        let mut recorder = self.recorder.lock().expect("recorder lock");
        recorder.calls.push(format!("deactivate:{}", reason_name(&reason)));
        recorder.reasons.push(reason);
        None
    }

    fn on_events(&mut self, batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        {
            let mut recorder = self.recorder.lock().expect("recorder lock");
            recorder.calls.push("on-events".to_string());
            recorder.batch_sizes.push(batch.len());
            recorder.events.extend(batch.iter().cloned());
        }
        (self.answer)(&batch)
    }
}

/// The WIT case name of a deactivation reason.
pub fn reason_name(reason: &DeactivationReason) -> &'static str {
    match reason {
        DeactivationReason::Shutdown => "shutdown",
        DeactivationReason::Disabled => "disabled",
        DeactivationReason::Upgrade => "upgrade",
        DeactivationReason::CrashRestart => "crash-restart",
        DeactivationReason::Revoked => "revoked",
        DeactivationReason::Granted => "granted",
    }
}

/// A trap with the given code, as a guest call would end.
pub fn trap(code: &'static str) -> Trap {
    Trap { code: Code::from_static(code), interface: "handler".to_string(), function: "on-events".to_string() }
}

/// A built harness around a scripted guest; the grants are those given.
pub fn harness_with(guest: ScriptedGuest, grants: &[&str]) -> HarnessHandle {
    HostHarness::new()
        .grants(grants)
        .with_native_guest(Box::new(guest))
        .build()
        .unwrap_or_else(|error| panic!("harness.build: {error}"))
}

/// Delivers `events`, then pumps until quiet but never more than 64 rounds.
pub fn deliver_and_pump(handle: &HarnessHandle, events: Vec<Event>) {
    handle.deliver(events);
    let _ = handle.pump(64);
}

/// All `request-completed` results the plugin received.
pub fn results(handle: &HarnessHandle) -> Vec<RequestResult> {
    handle.outcomes(&plugin_id())
}

/// True when the harness reports the instance as active.
pub fn is_active(handle: &HarnessHandle) -> bool {
    handle.state() == InstanceState::Active
}

/// A fetcher that fails every request; conformance cases never use the network.
pub struct NoNetwork;

impl Fetcher for NoNetwork {
    fn get(&self, _url: &str) -> Result<Vec<u8>, FetchError> {
        Err(FetchError::Unreachable)
    }
}

/// A CLI environment rooted in a throwaway folder.
pub fn cli_env(root: &Path) -> CliEnv {
    CliEnv {
        data_dir: root.join("data"),
        state_dir: root.join("state"),
        config_dir: root.join("config"),
        cwd: root.to_path_buf(),
        now: SystemTime::UNIX_EPOCH + Duration::from_secs(1_893_499_200),
        is_ci: true,
        fetcher: Arc::new(NoNetwork),
    }
}

/// Runs `ilium plugin <args...>` in `env`.
pub fn run_cli(env: &CliEnv, args: &[&str]) -> CliOutput {
    let owned: Vec<String> = args.iter().map(|argument| argument.to_string()).collect();
    ilium_plugin_host::cli::run(&owned, env)
}

/// The records of one `type` in the output, in order.
pub fn records_of<'a>(output: &'a CliOutput, kind: &str) -> Vec<&'a serde_json::Value> {
    output.stdout_jsonl.iter().filter(|record| record["type"] == kind).collect()
}

/// Writes a minimal plugin project (manifest and one dist component) into `root` and returns
/// `root`. The component bytes come from the built fixture `name`.
pub fn project_with_dist(root: &Path, manifest: &str, fixture: &str) -> PathBuf {
    std::fs::write(root.join("manifest.json"), manifest).expect("write manifest");
    let dist = root.join("dist");
    std::fs::create_dir_all(&dist).expect("create dist");
    std::fs::copy(fixture_component(fixture), dist.join("server.wasm")).expect("copy fixture");
    root.to_path_buf()
}

/// The catalogue of case ids in the order of reference-conformance-suite.md (DEV-642), with
/// the subjects that can run each (DEV-600 reads the `P` column).
pub const CATALOGUE: &[(&str, &str)] = &[
    ("CONF-LIFE-01", "PSH"),
    ("CONF-LIFE-02", "PH"),
    ("CONF-LIFE-03", "H"),
    ("CONF-LIFE-04", "PSH"),
    ("CONF-LIFE-05", "SH"),
    ("CONF-EVT-01", "H"),
    ("CONF-EVT-02", "PH"),
    ("CONF-EVT-03", "H"),
    ("CONF-EVT-04", "H"),
    ("CONF-EVT-05", "PH"),
    ("CONF-EVT-06", "PSH"),
    ("CONF-EVT-07", "SH"),
    ("CONF-EVT-08", "SH"),
    ("CONF-EVT-09", "SH"),
    ("CONF-EVT-10", "PSH"),
    ("CONF-GRANT-01", "PSH"),
    ("CONF-GRANT-02", "H"),
    ("CONF-GRANT-03", "H"),
    ("CONF-GRANT-04", "H"),
    ("CONF-GRANT-05", "H"),
    ("CONF-VIEW-01", "PH"),
    ("CONF-VIEW-02", "PS"),
    ("CONF-VIEW-03", "H"),
    ("CONF-VIEW-04", "H"),
    ("CONF-STORE-01", "H"),
    ("CONF-STORE-02", "SH"),
    ("CONF-STORE-03", "SH"),
    ("CONF-MEM-01", "H"),
    ("CONF-MEM-02", "H"),
    ("CONF-RESTART-01", "H"),
    ("CONF-RESTART-02", "H"),
    ("CONF-RESTART-03", "H"),
    ("CONF-RESTART-04", "H"),
    ("CONF-RESTART-05", "H"),
    ("CONF-PKG-01", "P"),
    ("CONF-PKG-02", "P"),
    ("CONF-PKG-03", "H"),
    ("CONF-PKG-04", "H"),
    ("CONF-PKG-05", "H"),
];

/// Builds a harness around the built probe component `conformance-probe.wasm`. The probe reads
/// its burn plan from the activation settings JSON (see `plugins/fixtures/conformance-probe/
/// CONTRACT.txt`): `{"burn_ms":{"activate":2100,"on-events":25},"alloc_mib":40,...}`.
/// Uses `with_component` and `settings_json`, both missing from the skeleton (gap.rs).
pub fn probe_harness(plan: serde_json::Value, grants: &[&str]) -> HostHarness {
    let bytes = std::fs::read(fixture_component("conformance-probe")).expect("read probe component");
    let harness = crate::gap::with_component(HostHarness::new().grants(grants), bytes);
    crate::gap::settings_json(harness, plan.to_string())
}

/// True when some trap in `handle.failures()` carries `code`.
pub fn has_failure(handle: &HarnessHandle, code: &str) -> bool {
    handle.failures().iter().any(|trap| trap.code.as_str() == code)
}

/// The `refused(...)` results as (correlation, error code name, message). Results of calls
/// (not submissions) are reported with correlation 0.
pub fn refused_codes(results: &[RequestResult]) -> Vec<(u64, String, String)> {
    use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef};
    results
        .iter()
        .filter_map(|result| match &result.outcome {
            Outcome::Refused(error) => {
                let correlation = match &result.request {
                    RequestRef::Submission(number) => *number,
                    RequestRef::Call(_) => 0,
                };
                Some((correlation, error.code.as_str().to_string(), error.message.clone()))
            }
            _ => None,
        })
        .collect()
}
