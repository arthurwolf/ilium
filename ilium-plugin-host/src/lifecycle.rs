//! Boot, enable, disable, health and shutdown of all plugins (reference-runtime-model.md,
//! reference-host-architecture.md).

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use ilium_plugin_api::Side;
use ilium_plugin_api::generated::host::plugins::{PluginInfo, SideState};
use ilium_plugin_api::ids::PluginId;

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
pub struct LifecycleManager;

impl LifecycleManager {
    /// Starts plugins in dependency order, core first (reference-host-architecture.md).
    pub fn start(&self, _mode: StartMode) -> StartReport {
        unimplemented!("host::lifecycle::LifecycleManager::start")
    }

    /// Deactivates everything within the budget, reverse of start order.
    pub fn shutdown(&self, _budget: Duration) -> ShutdownReport {
        unimplemented!("host::lifecycle::LifecycleManager::shutdown")
    }

    /// Enables a plugin and starts it.
    pub fn enable(&self, _plugin: &PluginId) -> Result<(), HostError> {
        unimplemented!("host::lifecycle::LifecycleManager::enable")
    }

    /// Disables a plugin and stops it.
    pub fn disable(&self, _plugin: &PluginId) -> Result<(), HostError> {
        unimplemented!("host::lifecycle::LifecycleManager::disable")
    }

    /// The state of one side of a plugin.
    pub fn state_of(&self, _plugin: &PluginId, _side: Side) -> SideState {
        unimplemented!("host::lifecycle::LifecycleManager::state_of")
    }

    /// The `plugin-info` record of a plugin.
    pub fn plugin_info(&self, _plugin: &PluginId) -> Option<PluginInfo> {
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
    let invalid = || UsageError {
        exit_code: 2,
        code: "usage.invalid-value".into(),
    };
    let mut selected = None;
    let mut no_plugins = false;
    for argument in args.iter().take_while(|argument| argument.as_str() != "--") {
        match argument.as_str() {
            "--no-plugins" => no_plugins = true,
            "--safe-mode" | "--safe-plugins" => selected = Some(SafeMode::ThirdParty),
            argument if argument.starts_with("--safe-mode=") => {
                selected = Some(match argument.strip_prefix("--safe-mode=") {
                    Some("third-party") => SafeMode::ThirdParty,
                    Some("all-optional") => SafeMode::AllOptional,
                    _ => return Err(invalid()),
                });
            }
            argument
                if argument.starts_with("--safe-plugins=")
                    || argument.starts_with("--no-plugins=") =>
            {
                return Err(invalid());
            }
            _ => {}
        }
    }
    // Disabling the host dominates other valid flags; an explicit flag completely
    // overrides the environment, including an invalid environment value.
    if no_plugins {
        return Ok(SafeMode::NoPlugins);
    }
    if let Some(mode) = selected {
        return Ok(mode);
    }
    match env_value {
        None | Some("0") => Ok(SafeMode::Off),
        Some("third-party" | "1") => Ok(SafeMode::ThirdParty),
        Some("all-optional") => Ok(SafeMode::AllOptional),
        _ => Err(invalid()),
    }
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
    let mut by_id = BTreeMap::new();
    let mut errors = Vec::new();
    for node in nodes {
        if by_id.insert(node.id.as_str(), node).is_some() {
            errors.push(order_diagnostic(
                "dependency.conflict",
                format!("Duplicate start node {:?}.", node.id),
            ));
        }
        if !(1..=3).contains(&node.tier) {
            errors.push(order_diagnostic(
                "dependency.conflict",
                format!("Plugin {:?} has invalid tier {}.", node.id, node.tier),
            ));
        }
    }
    let mut requirements = BTreeMap::new();
    let mut consumers: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for node in by_id.values() {
        // One provider can satisfy both a plugin and a service requirement.
        let required: BTreeSet<&str> = node
            .requires_plugins
            .iter()
            .chain(&node.requires_service_providers)
            .map(String::as_str)
            .collect();
        for provider in &required {
            if !by_id.contains_key(provider) {
                errors.push(order_diagnostic(
                    "dependency.unresolved",
                    format!(
                        "Plugin {:?} requires missing provider {provider:?}.",
                        node.id
                    ),
                ));
            }
            consumers
                .entry(provider)
                .or_default()
                .insert(node.id.as_str());
        }
        requirements.insert(node.id.as_str(), required);
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let mut ready: BTreeSet<(u8, &str)> = by_id
        .iter()
        .filter(|(id, _)| requirements[*id].is_empty())
        .map(|(id, node)| (node.tier, *id))
        .collect();
    let mut ordered = Vec::with_capacity(nodes.len());
    while let Some((_, id)) = ready.pop_first() {
        ordered.push(id.to_owned());
        if let Some(dependants) = consumers.get(id) {
            for dependant in dependants {
                if let Some(required) = requirements.get_mut(dependant) {
                    required.remove(id);
                    if required.is_empty() {
                        ready.insert((by_id[dependant].tier, dependant));
                    }
                }
            }
        }
    }
    if ordered.len() != nodes.len() {
        // Every blocked node still has a blocked requirement. Following those
        // edges finds an actual cycle rather than reporting all blocked consumers.
        let Some(mut cursor) = requirements
            .iter()
            .find(|(_, needs)| !needs.is_empty())
            .map(|(id, _)| *id)
        else {
            return Err(vec![order_diagnostic(
                "dependency.cycle",
                "Activation graph could not be ordered.",
            )]);
        };
        let mut path = Vec::new();
        let mut positions = BTreeMap::new();
        loop {
            if let Some(position) = positions.insert(cursor, path.len()) {
                let mut cycle = path[position..].to_vec();
                cycle.push(cursor);
                let service_cycle = cycle.windows(2).any(|edge| {
                    by_id[edge[0]]
                        .requires_service_providers
                        .iter()
                        .any(|provider| provider == edge[1])
                });
                return Err(vec![order_diagnostic(
                    if service_cycle {
                        "manifest.service-cycle"
                    } else {
                        "dependency.cycle"
                    },
                    format!("Activation cycle: {}.", cycle.join(" -> ")),
                )]);
            }
            path.push(cursor);
            let Some(next) = requirements.get(cursor).and_then(|needs| needs.first()) else {
                return Err(vec![order_diagnostic(
                    "dependency.cycle",
                    "Activation graph contains an unresolved cycle.",
                )]);
            };
            cursor = next;
        }
    }
    Ok(ordered)
}

fn order_diagnostic(
    code: &'static str,
    message: impl Into<String>,
) -> ilium_plugin_api::Diagnostic {
    ilium_plugin_api::Diagnostic::error(
        ilium_plugin_api::error::Code::from_static(code),
        "requires",
        message,
    )
}

/// The shutdown order: the exact reverse of [`start_order`] (PKG-351).
pub fn shutdown_order(
    nodes: &[StartNode],
) -> Result<Vec<String>, Vec<ilium_plugin_api::Diagnostic>> {
    let mut ordered = start_order(nodes)?;
    ordered.reverse();
    Ok(ordered)
}
