//! Boot, enable, disable, health and shutdown of all plugins (reference-runtime-model.md,
//! reference-host-architecture.md).

use std::time::Duration;

use ilium_plugin_api::generated::host::plugins::{PluginInfo, SideState};
use ilium_plugin_api::ids::PluginId;
use ilium_plugin_api::Side;

use crate::error::HostError;

/// How the host starts (reference-host-architecture.md "Safe start").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StartMode {
    /// Everything enabled starts.
    Normal,
    /// Only core plugins start (`--safe-plugins`).
    SafePlugins,
    /// No plugin starts (`--no-plugins`).
    NoPlugins,
}

/// What a start did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartReport {
    /// Plugin ids that reached `active`.
    pub started: Vec<String>,
    /// Plugin ids that failed to start, with their code.
    pub failed: Vec<(String, String)>,
}

/// What a shutdown did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownReport {
    /// Plugin ids deactivated cleanly.
    pub deactivated: Vec<String>,
    /// Plugin ids stopped by force at the budget.
    pub forced: Vec<String>,
}

/// Owns the instances of all plugins (reference-runtime-model.md).
#[derive(Debug)]
pub struct LifecycleManager {
    mode: StartMode,
}

impl LifecycleManager {
    /// Starts plugins in dependency order, core first (reference-host-architecture.md).
    pub fn start(&self, mode: StartMode) -> StartReport {
        unimplemented!("host::lifecycle::LifecycleManager::start")
    }

    /// Deactivates everything within the budget, reverse of start order.
    pub fn shutdown(&self, budget: Duration) -> ShutdownReport {
        unimplemented!("host::lifecycle::LifecycleManager::shutdown")
    }

    /// Enables a plugin and starts it.
    pub fn enable(&self, plugin: &PluginId) -> Result<(), HostError> {
        unimplemented!("host::lifecycle::LifecycleManager::enable")
    }

    /// Disables a plugin and stops it.
    pub fn disable(&self, plugin: &PluginId) -> Result<(), HostError> {
        unimplemented!("host::lifecycle::LifecycleManager::disable")
    }

    /// The state of one side of a plugin.
    pub fn state_of(&self, plugin: &PluginId, side: Side) -> SideState {
        unimplemented!("host::lifecycle::LifecycleManager::state_of")
    }

    /// The `plugin-info` record of a plugin.
    pub fn plugin_info(&self, plugin: &PluginId) -> Option<PluginInfo> {
        unimplemented!("host::lifecycle::LifecycleManager::plugin_info")
    }
}
