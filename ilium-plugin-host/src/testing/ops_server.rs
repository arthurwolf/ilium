//! The in-process server model behind `ops::ServerHandle` and its companions.
//!
//! The model keeps the lifecycle bookkeeping of one rig session (versions, enable flags, the
//! failure window and restart timers, deactivation reasons, logs) and takes the durable facts from
//! the rig's own files: `plugins/installed.json` is the single source of the installed version and
//! of a staged update (`update.candidate`). Staged updates are committed or rolled back through
//! the production [`Installer::finish_update`]. Storage goes through the production
//! [`PluginStorage`]; data files live in `plugin-data/<id>/`.
//!
//! Constructor contract for `OpsRig`: [`Core::open`] returns the shared handle; the rig calls
//! [`Core::tick`] with every virtual-clock advance so restart timers and polling servers move.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_api::lifecycle::DeactivationReason;
use ilium_plugin_api::{ErrorCode, PluginError, Side};

use crate::install::{FirstActivate, Installer};
use crate::registry::{InstalledRegistry, PackageStore};
use crate::storage::PluginStorage;
use ilium_plugin_api::ids::PluginId;

/// Failures inside this window count towards the restart limit (LIF-030).
const FAILURE_WINDOW: Duration = Duration::from_secs(600);

/// The index of a side in the per-side tables.
pub(crate) fn side_index(side: Side) -> usize {
    match side {
        Side::Server => 0,
        Side::Client => 1,
        Side::Scene => 2,
    }
}

/// One side of one plugin.
#[derive(Debug, Clone)]
pub(crate) struct SideModel {
    pub failures: Vec<Duration>,
    pub restart_at: Option<Duration>,
    pub delay_ms: Option<u64>,
    pub stopped_by_failure: bool,
}

impl SideModel {
    fn new() -> SideModel {
        SideModel { failures: Vec::new(), restart_at: None, delay_ms: None, stopped_by_failure: false }
    }
}

/// One plugin as the server knows it.
#[derive(Debug, Clone)]
pub(crate) struct PluginModel {
    pub version: Option<String>,
    pub enabled: bool,
    pub sides: [SideModel; 3],
    pub activations: Vec<String>,
    pub deactivations: Vec<(Side, DeactivationReason)>,
    pub grants: Vec<String>,
    pub restored_len: Option<usize>,
    pub client_restore: Option<Vec<u8>>,
    pub refusal: Option<String>,
    pub inactive_reason: Option<String>,
    pub retry_times: Vec<Duration>,
    pub essential: bool,
}

impl PluginModel {
    fn new(version: Option<String>) -> PluginModel {
        PluginModel {
            version,
            enabled: true,
            sides: [SideModel::new(), SideModel::new(), SideModel::new()],
            activations: Vec::new(),
            deactivations: Vec::new(),
            grants: Vec::new(),
            restored_len: None,
            client_restore: None,
            refusal: None,
            inactive_reason: None,
            retry_times: Vec::new(),
            essential: false,
        }
    }

    fn deactivate(&mut self, reason: DeactivationReason) {
        self.deactivations.push((Side::Server, reason));
        self.deactivations.push((Side::Client, reason));
    }

    pub(crate) fn running(&self) -> bool {
        self.version.is_some() && self.enabled && !self.sides[0].stopped_by_failure
    }
}

/// A request that a plugin sent and that has not been answered.
#[derive(Debug, Clone)]
pub(crate) struct RequestModel {
    pub plugin: String,
    pub deliveries: usize,
    pub outcome: Option<String>,
}

/// The shared state of one rig server.
#[derive(Debug)]
pub(crate) struct Core {
    pub root: PathBuf,
    pub session: String,
    pub manual: bool,
    pub running: bool,
    pub socket_open: bool,
    pub now: Duration,
    pub plugins: BTreeMap<String, PluginModel>,
    pub pause: Option<crate::testing::ops::UpdateStage>,
    pub failing_versions: BTreeSet<String>,
    pub kernel_calls: Vec<String>,
    pub log_codes: Vec<String>,
    pub messages: Vec<String>,
    pub toasts: Vec<String>,
    pub notices: Vec<String>,
    pub nodes: BTreeMap<u64, BTreeMap<(String, String), Vec<u8>>>,
    pub decorations: BTreeMap<String, BTreeSet<u64>>,
    pub requests: Vec<RequestModel>,
    pub prompts: BTreeMap<(String, String), u32>,
    pub not_now: BTreeMap<(String, String), u32>,
    pub settings: BTreeMap<(String, String), String>,
    pub extension_stopped: BTreeSet<(String, String, String)>,
    pub extension_events: Vec<String>,
    pub held_effects: BTreeMap<String, (u64, usize)>,
    pub replaced: BTreeMap<String, String>,
    pub breaches: BTreeMap<(String, String, String), u32>,
    pub contributions: BTreeMap<String, Vec<String>>,
    pub triggers: BTreeMap<String, Vec<String>>,
    pub next_instance: u64,
    pub delivery_blocked: bool,
    pub snapshots: u32,
}

/// A handle shared by the server, its clients and the screens it rendered.
pub(crate) type Shared = Arc<Mutex<Core>>;

impl Core {
    /// Opens the model of a session over a rig root and loads the installed plugins.
    ///
    /// A `manual` server applies installs and updates only in `apply_pending`; the others apply
    /// them on every access.
    pub(crate) fn open(root: &Path, session: &str, manual: bool) -> Shared {
        let mut core = Core {
            root: root.to_owned(),
            session: session.to_owned(),
            manual,
            running: true,
            socket_open: true,
            now: Duration::ZERO,
            plugins: BTreeMap::new(),
            pause: None,
            failing_versions: BTreeSet::new(),
            kernel_calls: Vec::new(),
            log_codes: Vec::new(),
            messages: Vec::new(),
            toasts: Vec::new(),
            notices: Vec::new(),
            nodes: BTreeMap::new(),
            decorations: BTreeMap::new(),
            requests: Vec::new(),
            prompts: BTreeMap::new(),
            not_now: BTreeMap::new(),
            settings: BTreeMap::new(),
            extension_stopped: BTreeSet::new(),
            extension_events: Vec::new(),
            held_effects: BTreeMap::new(),
            replaced: BTreeMap::new(),
            breaches: BTreeMap::new(),
            contributions: BTreeMap::new(),
            triggers: BTreeMap::new(),
            next_instance: 1,
            delivery_blocked: false,
            snapshots: 0,
        };
        core.kernel_calls.push("restore_snapshot".to_owned());
        core.kernel_calls.push("plugin_host.start".to_owned());
        core.sync();
        Arc::new(Mutex::new(core))
    }

    /// Advances the virtual clock and fires the restart timers that came due.
    pub(crate) fn tick(&mut self, by: Duration) {
        self.now += by;
        if !self.running {
            return;
        }
        let now = self.now;
        for plugin in self.plugins.values_mut() {
            for side in &mut plugin.sides {
                if side.restart_at.is_some_and(|at| at <= now) {
                    side.restart_at = None;
                    side.delay_ms = None;
                }
            }
        }
        self.sync();
    }

    fn registry_path(&self) -> PathBuf {
        self.root.join("plugins").join("installed.json")
    }

    fn enable_flags_path(&self) -> PathBuf {
        self.root.join("config").join("plugins.toml")
    }

    /// Rows of `installed.json` as `(id, active version, staged candidate, failure_stopped)`.
    fn installed_rows(&self) -> Vec<(String, String, Option<String>, bool)> {
        let Ok(bytes) = std::fs::read(self.registry_path()) else { return Vec::new() };
        let Ok(document) = serde_json::from_slice::<serde_json::Value>(&bytes) else { return Vec::new() };
        let Some(rows) = document["plugins"].as_array() else { return Vec::new() };
        rows.iter()
            .filter_map(|row| {
                let id = row["id"].as_str()?.to_owned();
                let version = row["active_version"].as_str()?.to_owned();
                let candidate = row["update"]["candidate"].as_str().map(str::to_owned);
                let stopped = row["failure_stopped"].as_array().is_some_and(|list| !list.is_empty());
                Some((id, version, candidate, stopped))
            })
            .collect()
    }

    fn installer(&self) -> Installer {
        Installer::new(PackageStore { root: self.root.join("plugins").join("store") })
    }

    /// Brings the model in line with `installed.json`: new plugins activate, removed ones
    /// deactivate, staged updates run their trial and commit or roll back.
    pub(crate) fn sync(&mut self) {
        if !self.running {
            return;
        }
        let rows = self.installed_rows();
        let present: BTreeSet<String> = rows.iter().map(|row| row.0.clone()).collect();
        let removed: Vec<String> = self.plugins.keys().filter(|id| !present.contains(*id)).cloned().collect();
        for id in removed {
            if let Some(mut plugin) = self.plugins.remove(&id) {
                if plugin.version.is_some() {
                    plugin.deactivate(DeactivationReason::Disabled);
                }
            }
        }
        for (id, version, candidate, stopped) in rows {
            if !self.plugins.contains_key(&id) {
                let mut plugin = PluginModel::new(Some(version.clone()));
                plugin.essential = id.starts_with("ilium.core.");
                plugin.enabled = self.read_enabled(&id);
                plugin.sides[0].stopped_by_failure = stopped;
                if plugin.running() {
                    plugin.activations.push("server:global".to_owned());
                    self.kernel_calls.push(format!("activate {id}"));
                }
                self.plugins.insert(id.clone(), plugin);
            }
            if let Some(candidate) = candidate {
                self.run_update(&id, &version, &candidate);
            } else if let Some(plugin) = self.plugins.get_mut(&id) {
                if plugin.version.as_deref() != Some(version.as_str()) {
                    plugin.deactivate(DeactivationReason::Upgrade);
                    plugin.version = Some(version.clone());
                    plugin.activations.push("server:global".to_owned());
                    self.reset_not_now(&id);
                }
            }
        }
    }

    fn read_enabled(&self, id: &str) -> bool {
        let plugin_id = PluginId(id.to_owned());
        crate::registry::EnableFlags::open(&self.enable_flags_path())
            .map(|flags| flags.is_enabled(&plugin_id))
            .unwrap_or(true)
    }

    fn write_enabled(&mut self, id: &str, enabled: bool) {
        let plugin_id = PluginId(id.to_owned());
        let path = self.enable_flags_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut flags) = crate::registry::EnableFlags::open(&path) {
            let _ = flags.set_enabled(&plugin_id, enabled);
        }
    }

    /// The trial of a staged update through the production installer.
    fn run_update(&mut self, id: &str, active: &str, candidate: &str) {
        if self.pause.is_some() {
            return;
        }
        let first = if self.failing_versions.contains(candidate) { FirstActivate::Error } else { FirstActivate::Ok };
        let outcome = self.installer().finish_update(&PluginId(id.to_owned()), first);
        let Some(plugin) = self.plugins.get_mut(id) else { return };
        match outcome {
            Ok(crate::install::UpdateOutcome::Committed { version, .. }) => {
                plugin.deactivate(DeactivationReason::Upgrade);
                plugin.version = Some(version);
                for side in &mut plugin.sides {
                    side.failures.clear();
                    side.stopped_by_failure = false;
                }
                plugin.activations.push("server:global".to_owned());
                self.messages.push("plugins-changed".to_owned());
                self.reset_not_now(id);
            }
            Ok(_) => {
                plugin.version = Some(active.to_owned());
                self.toasts.push("Update failed, rolled back".to_owned());
                self.log_codes.push("update.rolled-back".to_owned());
            }
            Err(_) => self.log_codes.push("update.finish-failed".to_owned()),
        }
    }

    /// Whether the server applies disk changes by itself.
    pub(crate) fn refresh(&mut self) {
        if !self.manual {
            self.sync();
        }
    }

    pub(crate) fn plugin(&self, id: &str) -> Option<&PluginModel> {
        self.plugins.get(id)
    }

    /// Records one failure of a side and schedules the restart or the stop (LIF-030..042).
    pub(crate) fn fail(&mut self, id: &str, side: Side) {
        let now = self.now;
        let Some(plugin) = self.plugins.get_mut(id) else { return };
        if !plugin.running() {
            return;
        }
        let essential = plugin.essential;
        let model = &mut plugin.sides[side_index(side)];
        model.failures.retain(|at| now.saturating_sub(*at) <= FAILURE_WINDOW);
        model.failures.push(now);
        let count = model.failures.len();
        let delay_ms = match count {
            1 => Some(1000),
            2 => Some(5000),
            _ if essential => Some(5000),
            _ => None,
        };
        let mut stopped = false;
        match delay_ms {
            Some(delay) => {
                model.delay_ms = Some(delay);
                model.restart_at = Some(now + Duration::from_millis(delay));
            }
            None => {
                model.delay_ms = None;
                model.restart_at = None;
                model.stopped_by_failure = true;
                stopped = true;
            }
        }
        plugin.activations.push(format!("{}:global", side_name(side)));
        if stopped {
            self.log_codes.push("plugin.stopped-by-failure".to_owned());
        }
    }

    /// The state of a side, derived from the flags.
    pub(crate) fn state_of(&self, id: &str, side: Side) -> InstanceState {
        let Some(plugin) = self.plugins.get(id) else { return InstanceState::Inactive };
        let model = &plugin.sides[side_index(side)];
        if !self.running {
            return InstanceState::Stopped;
        }
        if model.stopped_by_failure {
            return InstanceState::StoppedByFailure;
        }
        if !plugin.enabled || plugin.version.is_none() {
            return InstanceState::Inactive;
        }
        if model.restart_at.is_some() {
            return InstanceState::RestartScheduled;
        }
        InstanceState::Active
    }

    pub(crate) fn set_enabled(&mut self, id: &str, enabled: bool) {
        let Some(plugin) = self.plugins.get_mut(id) else { return };
        if plugin.enabled == enabled {
            return;
        }
        plugin.enabled = enabled;
        if enabled {
            for side in &mut plugin.sides {
                side.failures.clear();
                side.stopped_by_failure = false;
                side.restart_at = None;
                side.delay_ms = None;
            }
            plugin.activations.push("server:global".to_owned());
        } else {
            plugin.deactivate(DeactivationReason::Disabled);
            for side in &mut plugin.sides {
                side.restart_at = None;
                side.delay_ms = None;
            }
        }
        self.write_enabled(id, enabled);
        self.messages.push("plugins-changed".to_owned());
    }

    pub(crate) fn reset_failures(&mut self, id: &str) {
        if let Some(plugin) = self.plugins.get_mut(id) {
            for side in &mut plugin.sides {
                side.failures.clear();
            }
        }
    }

    /// Stops every plugin with the given reason.
    pub(crate) fn stop_all(&mut self, reason: Option<DeactivationReason>) {
        if !self.running {
            return;
        }
        if let Some(reason) = reason {
            for plugin in self.plugins.values_mut() {
                if plugin.running() {
                    plugin.deactivate(reason);
                }
            }
            self.kernel_calls.push("plugin_host.stop".to_owned());
            self.kernel_calls.push("drain_session_work".to_owned());
        }
        self.running = false;
        self.pause = None;
    }

    fn first_use_path(&self) -> PathBuf {
        self.root.join("state").join("first-use.json")
    }

    fn first_use_document(&self) -> serde_json::Value {
        std::fs::read(self.first_use_path())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_else(|| serde_json::json!({}))
    }

    /// Answers shared by every session of the rig: `(allowed, not_now count)`.
    pub(crate) fn first_use(&self, id: &str, grant: &str) -> (bool, u32) {
        let entry = &self.first_use_document()[format!("{id}|{grant}")];
        (entry["allowed"].as_bool().unwrap_or(false), entry["not_now"].as_u64().unwrap_or(0) as u32)
    }

    pub(crate) fn set_first_use(&self, id: &str, grant: &str, allowed: bool, not_now: u32) {
        let mut document = self.first_use_document();
        document[format!("{id}|{grant}")] = serde_json::json!({"allowed": allowed, "not_now": not_now});
        let _ = std::fs::create_dir_all(self.root.join("state"));
        let _ = std::fs::write(self.first_use_path(), document.to_string());
    }

    /// The grants of a plugin: model grants plus first-use grants answered with allow.
    pub(crate) fn grants_of(&self, id: &str) -> Vec<String> {
        let mut grants = self.plugins.get(id).map(|plugin| plugin.grants.clone()).unwrap_or_default();
        if let Some(object) = self.first_use_document().as_object() {
            for (key, entry) in object {
                if let Some(grant) = key.strip_prefix(&format!("{id}|")) {
                    if entry["allowed"].as_bool() == Some(true) && !grants.iter().any(|known| known == grant) {
                        grants.push(grant.to_owned());
                    }
                }
            }
        }
        grants
    }

    /// A bundled plugin's update resets the "not now" counters (LIF-275).
    fn reset_not_now(&self, id: &str) {
        if !id.starts_with("ilium.bundled.") {
            return;
        }
        let mut document = self.first_use_document();
        if let Some(object) = document.as_object_mut() {
            for (key, entry) in object.iter_mut() {
                if key.starts_with(&format!("{id}|")) {
                    entry["not_now"] = serde_json::json!(0);
                }
            }
        }
        let _ = std::fs::write(self.first_use_path(), document.to_string());
    }

    fn storage(&self, id: &str) -> Result<PluginStorage, PluginError> {
        PluginStorage::open(&self.root.join("plugins").join("storage"), PluginId(id.to_owned()))
    }

    pub(crate) fn write_storage(&self, id: &str, bucket: &str, key: &str, bytes: &[u8]) -> Result<(), PluginError> {
        self.storage(id)?.put(bucket, key, bytes.to_vec())
    }

    pub(crate) fn read_storage(&self, id: &str, bucket: &str, key: &str) -> Option<Vec<u8>> {
        self.storage(id).ok()?.get(bucket, key).ok().flatten()
    }

    pub(crate) fn data_path(&self, id: &str, name: &str) -> PathBuf {
        self.root.join("plugin-data").join(id).join(name)
    }

    pub(crate) fn registry_exists(&self) -> bool {
        InstalledRegistry::open(&self.registry_path()).is_ok()
    }
}

/// Locks the shared model, ignoring poison: a panicking test must not hide later reads.
pub(crate) fn lock(shared: &Shared) -> MutexGuard<'_, Core> {
    shared.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The text of a side, as used in activation texts.
pub(crate) fn side_name(side: Side) -> &'static str {
    match side {
        Side::Server => "server",
        Side::Client => "client",
        Side::Scene => "scene",
    }
}

/// The error of a call into a plugin that is not running.
pub(crate) fn not_running(id: &str) -> PluginError {
    PluginError::new(ErrorCode::Cancelled, format!("{id} is not running"))
}

/// The text of an instance state.
pub(crate) fn state_text(state: InstanceState) -> &'static str {
    match state {
        InstanceState::Inactive => "inactive",
        InstanceState::Activating => "activating",
        InstanceState::WaitingDependency => "waiting-dependency",
        InstanceState::Active => "active",
        InstanceState::Deactivating => "deactivating",
        InstanceState::Failed => "failed",
        InstanceState::RestartScheduled => "restart-scheduled",
        InstanceState::StoppedByFailure => "stopped-by-failure",
        InstanceState::Stopped => "stopped",
    }
}

/// The state of one `plugin dev` session.
#[derive(Debug)]
pub(crate) struct DevState {
    pub session: String,
    pub exit_code: Option<i32>,
    pub renewals: u32,
    pub renewing: bool,
    pub builds: u32,
    pub reload_log: Vec<String>,
    pub output: Vec<serde_json::Value>,
    pub prompts: Vec<String>,
    pub connected: bool,
}

impl DevState {
    pub(crate) fn new(session: &str) -> DevState {
        DevState {
            session: session.to_owned(),
            exit_code: None,
            renewals: 0,
            renewing: true,
            builds: 1,
            reload_log: Vec::new(),
            output: vec![serde_json::json!({"type": "dev", "event": "attached", "session": session})],
            prompts: Vec::new(),
            connected: true,
        }
    }
}

impl PluginModel {
    /// A dev reload deactivates the running instance with `upgrade` and activates the new build.
    pub(crate) fn deactivate_for_reload(&mut self) {
        self.deactivate(DeactivationReason::Upgrade);
        self.activations.push("server:global".to_owned());
    }
}

impl Core {
    /// Effects held by a plugin: all released once its instance was deactivated after the hold.
    pub(crate) fn effects_of(&self, id: &str) -> super::ops::Effects {
        let Some((instance, seen)) = self.held_effects.get(id).copied() else {
            return super::ops::Effects { instance: 0, input_lock: false, stream_open: false, child_running: false };
        };
        let released = self.plugin(id).is_none_or(|plugin| plugin.deactivations.len() > seen);
        super::ops::Effects { instance, input_lock: !released, stream_open: !released, child_running: !released }
    }
}
