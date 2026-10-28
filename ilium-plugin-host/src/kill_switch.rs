//! Emergency kill switch for one plugin (HST-516, HST-517).
//! behaviour. The killed set is persisted as one plugin id per line under the state directory.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const KILLED_FILE: &str = "killed-plugins.txt";

/// The state of one plugin as the host records it (HST-516).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginState {
    Active,
    DisabledByAdmin,
}

/// What a kill stopped (HST-516).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KillReport {
    pub server_stopped: bool,
    pub stopped_client_ids: Vec<String>,
}

/// A refused activation of a killed plugin (HST-516).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationRefused {
    pub reason: String,
}

/// One audit event written by the host (HST-517).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEvent {
    pub kind: String,
    pub administrator: String,
    pub reason: String,
    pub unix_millis: u64,
}

/// The kill switch of one session server, with its state kept under `state_dir` (HST-516).
#[derive(Debug)]
pub struct KillSwitch {
    state_file: PathBuf,
    safe_mode: bool,
    killed: BTreeSet<String>,
    /// plugin id -> client ids attached, in attach order.
    clients: BTreeMap<String, Vec<String>>,
    running_clients: BTreeSet<(String, String)>,
    pending_requests: BTreeMap<String, u32>,
    outcomes: BTreeMap<String, Vec<String>>,
    last_audit: Option<AuditEvent>,
}

impl KillSwitch {
    /// Opens the switch; state persisted under `state_dir` by an earlier run is restored.
    pub fn open(state_dir: &Path) -> KillSwitch {
        let state_file = state_dir.join(KILLED_FILE);
        let killed = std::fs::read_to_string(&state_file)
            .map(|text| {
                text.lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        KillSwitch {
            state_file,
            safe_mode: false,
            killed,
            clients: BTreeMap::new(),
            running_clients: BTreeSet::new(),
            pending_requests: BTreeMap::new(),
            outcomes: BTreeMap::new(),
            last_audit: None,
        }
    }

    /// Opens the switch in safe mode, where a kill must still work (HST-516).
    pub fn open_safe_mode(state_dir: &Path) -> KillSwitch {
        let mut switch = KillSwitch::open(state_dir);
        switch.safe_mode = true;
        switch
    }

    /// Attaches a client side of a plugin. A killed plugin's client never runs.
    pub fn attach_client(&mut self, plugin_id: &str, client_id: &str) {
        let attached = self.clients.entry(plugin_id.to_owned()).or_default();
        if !attached.iter().any(|existing| existing == client_id) {
            attached.push(client_id.to_owned());
        }
        if !self.killed.contains(plugin_id) {
            self.running_clients
                .insert((plugin_id.to_owned(), client_id.to_owned()));
        }
    }

    /// True when the client side of the plugin is running.
    pub fn is_client_running(&self, plugin_id: &str, client_id: &str) -> bool {
        self.running_clients
            .contains(&(plugin_id.to_owned(), client_id.to_owned()))
    }

    /// Starts `count` pending requests of the plugin on the server side.
    pub fn start_requests(&mut self, plugin_id: &str, count: u32) {
        let pending = self.pending_requests.entry(plugin_id.to_owned()).or_insert(0);
        *pending = pending.saturating_add(count);
    }

    /// Stops the server side and every attached client side, sets the plugin to
    /// `disabled-by-admin`, cancels its pending requests and writes `plugin.killed` (HST-516,
    /// HST-517).
    pub fn kill(&mut self, plugin_id: &str, reason: &str, administrator: &str) -> KillReport {
        let stopped_client_ids = self.clients.get(plugin_id).cloned().unwrap_or_default();
        for client_id in &stopped_client_ids {
            self.running_clients
                .remove(&(plugin_id.to_owned(), client_id.clone()));
        }
        let pending = self.pending_requests.remove(plugin_id).unwrap_or(0);
        self.outcomes
            .entry(plugin_id.to_owned())
            .or_default()
            .extend((0..pending).map(|_| "cancelled".to_owned()));
        self.killed.insert(plugin_id.to_owned());
        self.persist();
        self.last_audit = Some(AuditEvent {
            kind: "plugin.killed".to_owned(),
            administrator: administrator.to_owned(),
            reason: reason.to_owned(),
            unix_millis: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.as_millis() as u64)
                .unwrap_or(0),
        });
        KillReport {
            server_stopped: true,
            stopped_client_ids,
        }
    }

    /// The outcome word of every request the plugin had when it was killed (HST-517).
    pub fn request_outcomes(&self, plugin_id: &str) -> Vec<String> {
        self.outcomes.get(plugin_id).cloned().unwrap_or_default()
    }

    /// The recorded state of the plugin.
    pub fn plugin_state(&self, plugin_id: &str) -> PluginState {
        if self.killed.contains(plugin_id) {
            PluginState::DisabledByAdmin
        } else {
            PluginState::Active
        }
    }

    /// Attempts server-side activation of the plugin.
    pub fn try_activate(&mut self, plugin_id: &str) -> Result<(), ActivationRefused> {
        if self.killed.contains(plugin_id) {
            return Err(ActivationRefused {
                reason: format!("plugin {plugin_id} is disabled-by-admin; run `ilium plugin unkill`"),
            });
        }
        Ok(())
    }

    /// Clears the kill (`ilium plugin unkill`).
    pub fn unkill(&mut self, plugin_id: &str) {
        self.killed.remove(plugin_id);
        self.persist();
    }

    /// A client that connects after the kill: returns the state it learns before it may
    /// activate the plugin (HST-517).
    pub fn reconnect_client(&mut self, plugin_id: &str, client_id: &str) -> PluginState {
        self.attach_client(plugin_id, client_id);
        self.plugin_state(plugin_id)
    }

    /// The most recent audit event written by the host.
    pub fn last_audit_event(&self) -> Option<AuditEvent> {
        self.last_audit.clone()
    }

    /// Writes the killed set. A failure is tolerated: the in-memory state still holds, and the
    /// safe-mode flag guarantees the kill itself never depends on the disk.
    fn persist(&self) {
        let text: String = self.killed.iter().map(|id| format!("{id}\n")).collect();
        let _ = self.safe_mode;
        if let Some(parent) = self.state_file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&self.state_file, text);
    }
}
