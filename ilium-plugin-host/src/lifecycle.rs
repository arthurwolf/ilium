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

/// The safe mode chosen from the command line and environment (reference-security-response.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SafeMode {
    /// Every plugin starts.
    Off,
    /// Only core plugins start.
    ThirdParty,
    /// Only the core plugins that cannot be turned off start.
    AllOptional,
    /// No plugin starts.
    NoPlugins,
}

/// A command-line usage error: exit code and diagnostic code (reference-security-response.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageError {
    /// The process exit code.
    pub exit_code: i32,
    /// The diagnostic code.
    pub code: String,
}

/// Resolves `--no-plugins`, `--safe-mode=<value>` and the environment into a safe mode
/// (reference-security-response.md).
pub fn resolve_safe_mode(args: &[String], env_value: Option<&str>) -> Result<SafeMode, UsageError> {
    unimplemented!("host::lifecycle::resolve_safe_mode")
}

/// Activations that may run at once (reference-install-and-update.md PKG-350).
pub const MAX_CONCURRENT_ACTIVATIONS: usize = 4;

/// One plugin as the start-order computation sees it (PKG-350, 351).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartNode {
    /// The plugin id.
    pub id: String,
    /// The tier number.
    pub tier: u8,
    /// Plugins this one requires.
    pub requires_plugins: Vec<String>,
    /// Service providers this one requires.
    pub requires_service_providers: Vec<String>,
}

/// The activation order: requirements first, ties by (tier, id byte order); a cycle is a
/// diagnostic (PKG-350).
pub fn start_order(nodes: &[StartNode]) -> Result<Vec<String>, Vec<ilium_plugin_api::Diagnostic>> {
    unimplemented!("host::lifecycle::start_order")
}

/// The shutdown order: the exact reverse of [`start_order`] (PKG-351).
pub fn shutdown_order(nodes: &[StartNode]) -> Result<Vec<String>, Vec<ilium_plugin_api::Diagnostic>> {
    unimplemented!("host::lifecycle::shutdown_order")
}
