//! Emergency kill switch for one plugin (HST-516, HST-517). Skeleton: every function with
//! behaviour is `unimplemented!` until the implementation phase.

use std::path::Path;

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
pub struct KillSwitch;

impl KillSwitch {
    /// Opens the switch; state persisted under `state_dir` by an earlier run is restored.
    pub fn open(_state_dir: &Path) -> KillSwitch {
        unimplemented!("host::kill_switch::KillSwitch::open")
    }

    /// Opens the switch in safe mode, where a kill must still work (HST-516).
    pub fn open_safe_mode(_state_dir: &Path) -> KillSwitch {
        unimplemented!("host::kill_switch::KillSwitch::open_safe_mode")
    }

    /// Attaches a client side of a plugin.
    pub fn attach_client(&mut self, _plugin_id: &str, _client_id: &str) {
        unimplemented!("host::kill_switch::KillSwitch::attach_client")
    }

    /// True when the client side of the plugin is running.
    pub fn is_client_running(&self, _plugin_id: &str, _client_id: &str) -> bool {
        unimplemented!("host::kill_switch::KillSwitch::is_client_running")
    }

    /// Starts `count` pending requests of the plugin on the server side.
    pub fn start_requests(&mut self, _plugin_id: &str, _count: u32) {
        unimplemented!("host::kill_switch::KillSwitch::start_requests")
    }

    /// Stops the server side and every attached client side, sets the plugin to
    /// `disabled-by-admin`, cancels its pending requests and writes `plugin.killed` (HST-516,
    /// HST-517).
    pub fn kill(&mut self, _plugin_id: &str, _reason: &str, _administrator: &str) -> KillReport {
        unimplemented!("host::kill_switch::KillSwitch::kill")
    }

    /// The outcome word of every request the plugin had when it was killed (HST-517).
    pub fn request_outcomes(&self, _plugin_id: &str) -> Vec<String> {
        unimplemented!("host::kill_switch::KillSwitch::request_outcomes")
    }

    /// The recorded state of the plugin.
    pub fn plugin_state(&self, _plugin_id: &str) -> PluginState {
        unimplemented!("host::kill_switch::KillSwitch::plugin_state")
    }

    /// Attempts server-side activation of the plugin.
    pub fn try_activate(&mut self, _plugin_id: &str) -> Result<(), ActivationRefused> {
        unimplemented!("host::kill_switch::KillSwitch::try_activate")
    }

    /// Clears the kill (`ilium plugin unkill`).
    pub fn unkill(&mut self, _plugin_id: &str) {
        unimplemented!("host::kill_switch::KillSwitch::unkill")
    }

    /// A client that connects after the kill: returns the state it learns before it may
    /// activate the plugin (HST-517).
    pub fn reconnect_client(&mut self, _plugin_id: &str, _client_id: &str) -> PluginState {
        unimplemented!("host::kill_switch::KillSwitch::reconnect_client")
    }

    /// The most recent audit event written by the host.
    pub fn last_audit_event(&self) -> Option<AuditEvent> {
        unimplemented!("host::kill_switch::KillSwitch::last_audit_event")
    }
}
