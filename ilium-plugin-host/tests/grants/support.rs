//! Shared helpers of the host grants tests: a recording guest, a harness rig, package builders
//! for the install pipeline and a CLI environment. Items outside the skeleton are named in
//! API_GAPS.md.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::intents::{Intent, Submission};
use ilium_plugin_api::generated::types::lifecycle::{Activation, DeactivationReason};
use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef};
use ilium_plugin_api::ids::PluginId;
use ilium_plugin_api::{PluginError, Side, Tier};
use ilium_plugin_host::calls::Trap;
use ilium_plugin_host::cli::CliEnv;
use ilium_plugin_host::registry_client::{FetchError, Fetcher};
use ilium_plugin_host::testing::{FakeKernel, HarnessHandle, HostHarness, NativeGuest};
use sha2::{Digest, Sha256};

/// Plugin id the harness gives its plugin (DEV-420: `test.plugin`).
pub fn plugin_id() -> PluginId {
    PluginId("test.plugin".to_owned())
}

/// One event that makes the harness call `on-events` once.
pub fn one_event() -> Vec<Event> {
    vec![Event::ThemeChanged]
}

/// What a recording guest saw, shared with the test.
#[derive(Clone, Default)]
pub struct Seen {
    pub activations: Arc<Mutex<Vec<Activation>>>,
    pub deactivations: Arc<Mutex<Vec<DeactivationReason>>>,
    pub queue: Arc<Mutex<VecDeque<Submission>>>,
}

impl Seen {
    /// The `granted` list of the latest activation.
    pub fn latest_granted(&self) -> Vec<String> {
        self.activations
            .lock()
            .expect("activations lock")
            .last()
            .map(|activation| activation.granted.clone())
            .expect("the plugin was activated")
    }

    /// How often the plugin was activated.
    pub fn activation_count(&self) -> usize {
        self.activations.lock().expect("activations lock").len()
    }

    /// Deactivation reasons in order.
    pub fn reasons(&self) -> Vec<DeactivationReason> {
        self.deactivations.lock().expect("deactivations lock").clone()
    }
}

/// A guest that records activations and deactivations and submits queued intents.
pub struct RecordingGuest(pub Seen);

impl NativeGuest for RecordingGuest {
    fn activate(&mut self, activation: Activation) -> Result<(), PluginError> {
        self.0.activations.lock().expect("activations lock").push(activation);
        Ok(())
    }

    fn deactivate(&mut self, reason: DeactivationReason) -> Option<Vec<u8>> {
        self.0.deactivations.lock().expect("deactivations lock").push(reason);
        None
    }

    fn on_events(&mut self, _batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        Ok(self.0.queue.lock().expect("queue lock").drain(..).collect())
    }
}

/// A harness with a recording guest.
pub struct Rig {
    pub host: HarnessHandle,
    pub seen: Seen,
    pub kernel: Arc<FakeKernel>,
    next_correlation: u64,
}

/// Builds a rig from a configured harness builder; the guest and kernel are attached here.
pub fn rig_from(builder: HostHarness) -> Rig {
    let seen = Seen::default();
    let kernel = Arc::new(FakeKernel::new(ilium_plugin_api::ProcessKind::Server));
    let host = builder
        .kernel(kernel.clone())
        .with_native_guest(Box::new(RecordingGuest(seen.clone())))
        .build()
        .expect("the harness builds");
    Rig { host, seen, kernel, next_correlation: 0 }
}

/// A rig for `side` at `tier` holding `granted`.
pub fn rig(tier: Tier, side: Side, granted: &[&str]) -> Rig {
    rig_from(HostHarness::new().tier(tier).side(side).grants(granted))
}

impl Rig {
    /// Submits one intent through the guest, runs the dispatcher and returns its outcome, if any.
    pub fn submit(&mut self, intent: Intent) -> Option<Outcome> {
        self.next_correlation += 1;
        let correlation = self.next_correlation;
        self.seen.queue.lock().expect("queue lock").push_back(Submission {
            intent,
            correlation,
            dedup_key: None,
        });
        self.host.deliver(one_event());
        self.host.pump(8);
        self.outcome_of(correlation)
    }

    /// The outcome recorded for a correlation, if the request completed.
    pub fn outcome_of(&self, correlation: u64) -> Option<Outcome> {
        self.host
            .outcomes(&plugin_id())
            .into_iter()
            .rev()
            .find(|result| result.request == RequestRef::Submission(correlation))
            .map(|result| result.outcome)
    }

    /// Log lines of the plugin as plain messages.
    pub fn log_messages(&self) -> Vec<String> {
        self.host.logs().into_iter().map(|line| line.message).collect()
    }
}

/// The error of a refused outcome.
pub fn refusal(outcome: Option<Outcome>) -> PluginError {
    match outcome {
        Some(Outcome::Refused(error)) | Some(Outcome::Failed(error)) => error,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// A fetcher that must never be called.
pub struct NoNetwork;

impl Fetcher for NoNetwork {
    fn get(&self, _url: &str) -> Result<Vec<u8>, FetchError> {
        Err(FetchError::Unreachable)
    }
}

/// A CLI environment with all directories under `root`.
pub fn cli_env(root: &Path) -> CliEnv {
    CliEnv::new(
        root.join("data"),
        root.join("state"),
        root.join("config"),
        root.join("cwd"),
        std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000),
        false,
        Arc::new(NoNetwork),
    )
}

/// Command line arguments from string slices.
pub fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

/// Lower-case hex SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The smallest component bytes the inventory can list.
pub const WASM_HEADER: [u8; 8] = [0, 0x61, 0x73, 0x6d, 1, 0, 0, 0];

/// Package build options.
#[derive(Clone)]
pub struct PackageSpec<'a> {
    pub api: &'a str,
    pub grants: &'a [&'a str],
    pub corrupt_hash: bool,
}

impl<'a> PackageSpec<'a> {
    /// A valid package that asks for these server grants.
    pub fn with_grants(grants: &'a [&'a str]) -> PackageSpec<'a> {
        PackageSpec { api: "ilium:plugin@0.1.0", grants, corrupt_hash: false }
    }
}

/// The bytes of an unsigned `.ilp` package with `manifest.json` and `server.wasm`.
pub fn package_bytes(spec: &PackageSpec<'_>) -> Vec<u8> {
    let hash = if spec.corrupt_hash { "0".repeat(64) } else { sha256_hex(&WASM_HEADER) };
    let manifest = serde_json::json!({
        "api": spec.api,
        "id": "example.grants",
        "name": "Grants test",
        "version": "1.0.0",
        "publisher": "Example",
        "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0" },
        "capabilities": { "server": spec.grants },
        "files": [{ "path": "server.wasm", "length": WASM_HEADER.len(), "sha256": hash }],
    });
    ilium_plugin_host::package::write_package(&[
        ("manifest.json".to_owned(), manifest.to_string().into_bytes()),
        ("server.wasm".to_owned(), WASM_HEADER.to_vec()),
    ])
    .expect("the package archive is written")
}

/// A fresh scratch directory that removes itself.
pub fn scratch() -> tempfile::TempDir {
    tempfile::tempdir().expect("scratch directory")
}

/// Files directly inside a directory, sorted.
pub fn file_names(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(directory)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// The canonical manual pages: `ILIUM_PLUGIN_MANUAL_DIR`, else `<repo>/docs/plugins-manual/pages`.
pub fn manual_page(name: &str) -> String {
    let directory = match std::env::var_os("ILIUM_PLUGIN_MANUAL_DIR") {
        Some(directory) => PathBuf::from(directory),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/plugins-manual/pages"),
    };
    let path = directory.join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}
