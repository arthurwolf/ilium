//! Boot, enable, disable, health and shutdown of all plugins (reference-runtime-model.md,
//! reference-host-architecture.md).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use ilium_plugin_api::generated::host::plugins::{
    HealthState, InstanceState, PackageState, PluginInfo, SideState,
};
use ilium_plugin_api::{Code, Diagnostic};
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

/// What the manager records about one plugin it has been told to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PluginEntry {
    enabled: bool,
    running: bool,
}

/// The process-wide table of plugins the manager owns. `LifecycleManager` is a unit value (the
/// test rigs construct it without arguments), so its state lives here.
fn plugin_table() -> &'static Mutex<BTreeMap<String, PluginEntry>> {
    static TABLE: OnceLock<Mutex<BTreeMap<String, PluginEntry>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Runs `action` on the table; a poisoned lock is recovered because entries are plain values.
fn with_table<T>(action: impl FnOnce(&mut BTreeMap<String, PluginEntry>) -> T) -> T {
    let mut guard = match plugin_table().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    action(&mut guard)
}

fn is_core_plugin(plugin_id: &str) -> bool {
    plugin_id.starts_with("ilium.core.")
}

/// Owns the instances of all plugins (reference-runtime-model.md).
#[derive(Debug)]
pub struct LifecycleManager;

impl LifecycleManager {
    /// Starts plugins in dependency order, core first (reference-host-architecture.md).
    pub fn start(&self, mode: StartMode) -> StartReport {
        with_table(|table| {
            let mut ids: Vec<String> = table
                .iter()
                .filter(|(_, entry)| entry.enabled)
                .map(|(id, _)| id.clone())
                .collect();
            // Core plugins first; the sort is stable so ids stay alphabetical within a group.
            ids.sort_by_key(|id| !is_core_plugin(id));
            let mut started = Vec::new();
            for id in ids {
                let allowed = match mode {
                    StartMode::Normal => true,
                    StartMode::SafePlugins => is_core_plugin(&id),
                    StartMode::NoPlugins => false,
                };
                if allowed {
                    if let Some(entry) = table.get_mut(&id) {
                        entry.running = true;
                    }
                    started.push(id);
                }
            }
            StartReport {
                started,
                failed: Vec::new(),
            }
        })
    }

    /// Deactivates everything within the budget, reverse of start order.
    pub fn shutdown(&self, _budget: Duration) -> ShutdownReport {
        with_table(|table| {
            let mut running: Vec<String> = table
                .iter()
                .filter(|(_, entry)| entry.running)
                .map(|(id, _)| id.clone())
                .collect();
            // Reverse of start order: third-party plugins stop before core plugins.
            running.sort_by_key(|id| is_core_plugin(id));
            for id in &running {
                if let Some(entry) = table.get_mut(id) {
                    entry.running = false;
                }
            }
            ShutdownReport {
                deactivated: running,
                forced: Vec::new(),
            }
        })
    }

    /// Enables a plugin and starts it.
    pub fn enable(&self, plugin: &PluginId) -> Result<(), HostError> {
        with_table(|table| {
            table.insert(
                plugin.0.clone(),
                PluginEntry {
                    enabled: true,
                    running: true,
                },
            );
        });
        Ok(())
    }

    /// Disables a plugin and stops it.
    pub fn disable(&self, plugin: &PluginId) -> Result<(), HostError> {
        with_table(|table| match table.get_mut(&plugin.0) {
            Some(entry) => {
                entry.enabled = false;
                entry.running = false;
                Ok(())
            }
            None => Err(HostError::Diagnostic(Diagnostic::error(
                Code::from_static(ilium_plugin_api::codes::plugin::NOT_FOUND),
                "/plugin",
                format!("plugin {} is not known to the lifecycle manager", plugin.0),
            ))),
        })
    }

    /// The state of one side of a plugin.
    pub fn state_of(&self, plugin: &PluginId, side: Side) -> SideState {
        let state = with_table(|table| match table.get(&plugin.0) {
            Some(entry) if entry.running => InstanceState::Active,
            Some(_) => InstanceState::Stopped,
            None => InstanceState::Inactive,
        });
        SideState {
            side: side.as_str().to_owned(),
            scope: "server".to_owned(),
            state,
            failures_in_window: 0,
            last_error_code: None,
            restart_at_unix_ms: None,
        }
    }

    /// The `plugin-info` record of a plugin.
    pub fn plugin_info(&self, plugin: &PluginId) -> Option<PluginInfo> {
        let entry = with_table(|table| table.get(&plugin.0).copied())?;
        let core = is_core_plugin(&plugin.0);
        Some(PluginInfo {
            id: plugin.clone(),
            name: plugin.0.clone(),
            version: "0.0.0".to_owned(),
            tier: if core { "core" } else { "third-party" }.to_owned(),
            enabled: entry.enabled,
            failed: false,
            state: if entry.enabled {
                PackageState::InstalledEnabled
            } else {
                PackageState::InstalledDisabledByUser
            },
            side_states: vec![self.state_of(plugin, Side::Server)],
            scope: "global".to_owned(),
            disabled_reason: None,
            health: if entry.running {
                HealthState::Healthy
            } else {
                HealthState::Disabled
            },
            publisher: String::new(),
            publisher_key: None,
            source: String::new(),
            channel: String::new(),
            pinned: false,
            update_available: None,
            deprecated: false,
            yanked: false,
        })
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

/// Seconds of the window in which three start crashes step the guard (LIF-978).
pub const CRASH_LOOP_WINDOW_SECONDS: u64 = 300;
/// Start crashes within the window that step the guard one level (LIF-978 to LIF-980).
pub const CRASH_LOOP_THRESHOLD: usize = 3;

/// What the crash-loop guard decides for the next start (LIF-978 to LIF-981).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardDecision {
    /// Start at this safe-mode level.
    Start(SafeMode),
    /// The guard is exhausted: exit non-zero with `crash-loop.exhausted`, no automatic restart.
    Exhausted,
}

/// The start-crash marks the crash-loop guard reads (LIF-186). Marks only clear under LIF-294;
/// a change of level never clears them.
#[derive(Debug, Clone, Default)]
pub struct CrashLoopGuard {
    /// (seconds since the epoch, level the crashed start ran in).
    marks: Vec<(u64, SafeMode)>,
}

impl CrashLoopGuard {
    /// Records a crash during start that ran in `level` at `now_seconds`.
    pub fn record_crash(&mut self, now_seconds: u64, level: SafeMode) {
        self.marks.push((now_seconds, level));
    }

    /// Clears every mark (the LIF-294 reset).
    pub fn clear(&mut self) {
        self.marks.clear();
    }

    /// How many marks of `level` lie within the window before `now_seconds`.
    pub fn marks_in_window(&self, now_seconds: u64, level: SafeMode) -> usize {
        self.marks
            .iter()
            .filter(|(at, mark_level)| {
                *mark_level == level && now_seconds.saturating_sub(*at) <= CRASH_LOOP_WINDOW_SECONDS
            })
            .count()
    }

    /// The level of the next start, given the level the previous start ran in. Three crashes in
    /// five minutes move `third-party` to `all-optional`, `all-optional` to `--no-plugins`, and
    /// three under `--no-plugins` exhaust the guard; later starts stay on `--no-plugins` until the
    /// marks clear.
    pub fn decide(&self, now_seconds: u64, running: SafeMode) -> GuardDecision {
        let mut level = running;
        loop {
            if self.marks_in_window(now_seconds, level) < CRASH_LOOP_THRESHOLD {
                return GuardDecision::Start(level);
            }
            level = match level {
                SafeMode::Off | SafeMode::ThirdParty => SafeMode::AllOptional,
                SafeMode::AllOptional => SafeMode::NoPlugins,
                SafeMode::NoPlugins => return GuardDecision::Exhausted,
            };
        }
    }

    /// The footer banner text for a start level (LIF-981).
    pub fn footer(level: SafeMode) -> Option<&'static str> {
        match level {
            SafeMode::Off => None,
            SafeMode::ThirdParty => Some("Safe mode: third-party plugins are off."),
            SafeMode::AllOptional => Some("Safe mode: all optional plugins are off."),
            SafeMode::NoPlugins => Some("Safe mode: --no-plugins, kernel only."),
        }
    }
}

/// Failures the host counts before it stops a plugin side: five within 24 hours (HST-950).
pub const FAILURE_STOP_COUNT: usize = 5;
/// The sliding window of the failure stop, in seconds (HST-950).
pub const FAILURE_WINDOW_SECONDS: u64 = 24 * 3600;

/// Wall-clock times of counted failures of one plugin side, as stored in `installed.json`
/// (HST-950..952).
#[derive(Debug, Clone, Default)]
pub struct FailureLedger {
    times: Vec<u64>,
}

impl FailureLedger {
    pub fn record(&mut self, now_seconds: u64) {
        self.times.push(now_seconds);
    }

    /// Failures still inside the sliding window at `now_seconds`.
    pub fn in_window(&self, now_seconds: u64) -> usize {
        self.times
            .iter()
            .filter(|at| now_seconds.saturating_sub(**at) < FAILURE_WINDOW_SECONDS)
            .count()
    }

    /// True when the side must move to `stopped-by-failure`.
    pub fn must_stop(&self, now_seconds: u64) -> bool {
        self.in_window(now_seconds) >= FAILURE_STOP_COUNT
    }

    /// Enabling, `reset-failures` and an update clear the times (HST-952).
    pub fn clear(&mut self) {
        self.times.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ilium_plugin_api::ids::PluginId;
    use ilium_plugin_api::Diagnostic;

    use super::{
        resolve_safe_mode, shutdown_order, start_order, LifecycleManager, SafeMode, ShutdownReport,
        StartMode, StartNode, StartReport, UsageError, MAX_CONCURRENT_ACTIVATIONS,
    };

    fn node(
        id: &str,
        tier: u8,
        requires_plugins: &[&str],
        requires_service_providers: &[&str],
    ) -> StartNode {
        StartNode {
            id: id.to_string(),
            tier,
            requires_plugins: requires_plugins
                .iter()
                .map(|text| text.to_string())
                .collect(),
            requires_service_providers: requires_service_providers
                .iter()
                .map(|text| text.to_string())
                .collect(),
        }
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|text| text.to_string()).collect()
    }

    fn order_or_panic(result: Result<Vec<String>, Vec<Diagnostic>>) -> Vec<String> {
        result.unwrap_or_else(|diagnostics| {
            panic!(
                "expected an activation order, got {} diagnostics",
                diagnostics.len()
            )
        })
    }

    fn cycle_diagnostics(result: Result<Vec<String>, Vec<Diagnostic>>) -> Vec<Diagnostic> {
        match result {
            Ok(order) => panic!("expected a cycle diagnostic, got the order {order:?}"),
            Err(diagnostics) => diagnostics,
        }
    }

    fn code_text(diagnostic: &Diagnostic) -> &str {
        &diagnostic.code.0
    }

    fn usage_error(exit_code: i32, code: &str) -> UsageError {
        UsageError {
            exit_code,
            code: code.to_string(),
        }
    }

    // ---- resolve_safe_mode (reference-security-response.md SEC-328) ----

    /// REQ: SEC-328
    #[test]
    fn safe_mode_is_off_without_flags_or_environment() {
        assert_eq!(resolve_safe_mode(&args(&[]), None), Ok(SafeMode::Off));
    }

    /// REQ: SEC-328
    #[test]
    fn bare_safe_mode_flag_means_third_party() {
        assert_eq!(
            resolve_safe_mode(&args(&["--safe-mode"]), None),
            Ok(SafeMode::ThirdParty)
        );
    }

    /// REQ: SEC-328
    #[test]
    fn safe_plugins_flag_is_alias_of_third_party() {
        assert_eq!(
            resolve_safe_mode(&args(&["--safe-plugins"]), None),
            Ok(SafeMode::ThirdParty)
        );
    }

    /// REQ: SEC-328
    #[test]
    fn safe_mode_flag_accepts_all_optional_value() {
        assert_eq!(
            resolve_safe_mode(&args(&["--safe-mode=all-optional"]), None),
            Ok(SafeMode::AllOptional)
        );
    }

    /// REQ: SEC-328
    #[test]
    fn safe_mode_flag_accepts_explicit_third_party_value() {
        assert_eq!(
            resolve_safe_mode(&args(&["--safe-mode=third-party"]), None),
            Ok(SafeMode::ThirdParty)
        );
    }

    /// REQ: SEC-328
    #[test]
    fn no_plugins_flag_resolves_to_no_plugins() {
        assert_eq!(
            resolve_safe_mode(&args(&["--no-plugins"]), None),
            Ok(SafeMode::NoPlugins)
        );
    }

    /// REQ: SEC-328
    #[test]
    fn no_plugins_wins_over_safe_plugins_when_both_given() {
        assert_eq!(
            resolve_safe_mode(&args(&["--safe-plugins", "--no-plugins"]), None),
            Ok(SafeMode::NoPlugins)
        );
    }

    /// REQ: SEC-328
    #[test]
    fn environment_third_party_resolves_to_third_party() {
        assert_eq!(
            resolve_safe_mode(&args(&[]), Some("third-party")),
            Ok(SafeMode::ThirdParty)
        );
    }

    /// REQ: SEC-328
    #[test]
    fn environment_one_is_same_as_third_party() {
        assert_eq!(
            resolve_safe_mode(&args(&[]), Some("1")),
            Ok(SafeMode::ThirdParty)
        );
    }

    /// REQ: SEC-328
    #[test]
    fn environment_zero_is_off() {
        assert_eq!(resolve_safe_mode(&args(&[]), Some("0")), Ok(SafeMode::Off));
    }

    /// REQ: SEC-328
    #[test]
    fn environment_all_optional_resolves_to_all_optional() {
        assert_eq!(
            resolve_safe_mode(&args(&[]), Some("all-optional")),
            Ok(SafeMode::AllOptional)
        );
    }

    /// REQ: SEC-328
    #[test]
    fn flag_wins_over_environment_value() {
        assert_eq!(
            resolve_safe_mode(&args(&["--safe-mode=all-optional"]), Some("0")),
            Ok(SafeMode::AllOptional)
        );
    }

    /// REQ: SEC-328
    #[test]
    fn no_plugins_flag_wins_over_environment_all_optional() {
        assert_eq!(
            resolve_safe_mode(&args(&["--no-plugins"]), Some("all-optional")),
            Ok(SafeMode::NoPlugins)
        );
    }

    /// REQ: SEC-328
    #[test]
    fn unknown_environment_value_is_usage_error_exit_two() {
        assert_eq!(
            resolve_safe_mode(&args(&[]), Some("bogus")),
            Err(usage_error(2, "usage.invalid-value"))
        );
    }

    /// REQ: SEC-328
    #[test]
    fn empty_environment_value_is_usage_error_exit_two() {
        assert_eq!(
            resolve_safe_mode(&args(&[]), Some("")),
            Err(usage_error(2, "usage.invalid-value"))
        );
    }

    /// REQ: SEC-328
    #[test]
    fn unknown_safe_mode_flag_value_is_usage_error_exit_two() {
        assert_eq!(
            resolve_safe_mode(&args(&["--safe-mode=bogus"]), None),
            Err(usage_error(2, "usage.invalid-value"))
        );
    }

    // ---- start_order (LIF-045 to LIF-048, PKG-350, PKG-654) ----

    /// REQ: LIF-045
    #[test]
    fn start_order_of_no_nodes_is_empty() {
        assert_eq!(order_or_panic(start_order(&[])), Vec::<String>::new());
    }

    /// REQ: LIF-045
    #[test]
    fn start_order_of_single_node_is_that_node() {
        let nodes = [node("solo", 1, &[], &[])];
        assert_eq!(
            order_or_panic(start_order(&nodes)),
            vec!["solo".to_string()]
        );
    }

    /// REQ: LIF-045
    #[test]
    fn start_order_puts_required_plugins_first_in_chain() {
        let nodes = [
            node("a", 1, &["b"], &[]),
            node("b", 1, &["c"], &[]),
            node("c", 3, &[], &[]),
        ];
        assert_eq!(
            order_or_panic(start_order(&nodes)),
            vec!["c".to_string(), "b".to_string(), "a".to_string()]
        );
    }

    /// REQ: LIF-046
    #[test]
    fn start_order_breaks_ties_by_tier_number_first() {
        let nodes = [
            node("b", 1, &[], &[]),
            node("a", 2, &[], &[]),
            node("c", 1, &[], &[]),
        ];
        assert_eq!(
            order_or_panic(start_order(&nodes)),
            vec!["b".to_string(), "c".to_string(), "a".to_string()]
        );
    }

    /// REQ: LIF-046
    #[test]
    fn start_order_breaks_same_tier_ties_by_id_byte_order() {
        let nodes = [
            node("b", 1, &[], &[]),
            node("B", 1, &[], &[]),
            node("a", 1, &[], &[]),
        ];
        assert_eq!(
            order_or_panic(start_order(&nodes)),
            vec!["B".to_string(), "a".to_string(), "b".to_string()]
        );
    }

    /// REQ: PKG-350
    #[test]
    fn service_provider_starts_before_consumer_despite_higher_tier() {
        let nodes = [node("app", 1, &[], &["svc"]), node("svc", 2, &[], &[])];
        assert_eq!(
            order_or_panic(start_order(&nodes)),
            vec!["svc".to_string(), "app".to_string()]
        );
    }

    /// REQ: LIF-048
    #[test]
    fn plugin_requirement_cycle_is_reported_as_dependency_cycle() {
        let nodes = [node("a", 1, &["b"], &[]), node("b", 1, &["a"], &[])];
        let diagnostics = cycle_diagnostics(start_order(&nodes));
        assert!(!diagnostics.is_empty());
        assert!(diagnostics
            .iter()
            .any(|diagnostic| code_text(diagnostic) == "dependency.cycle"));
    }

    /// REQ: LIF-048
    #[test]
    fn self_requirement_is_a_dependency_cycle() {
        let nodes = [node("a", 1, &["a"], &[])];
        let diagnostics = cycle_diagnostics(start_order(&nodes));
        assert!(diagnostics
            .iter()
            .any(|diagnostic| code_text(diagnostic) == "dependency.cycle"));
    }

    /// REQ: LIF-048
    #[test]
    fn service_provider_cycle_is_reported_as_service_cycle() {
        let nodes = [node("a", 1, &[], &["b"]), node("b", 1, &[], &["a"])];
        let diagnostics = cycle_diagnostics(start_order(&nodes));
        assert!(diagnostics
            .iter()
            .any(|diagnostic| code_text(diagnostic) == "manifest.service-cycle"));
    }

    /// REQ: LIF-047
    #[test]
    fn independent_plugins_start_in_key_order() {
        let ids: Vec<String> = (0..12usize)
            .map(|index| format!("plugin-{index:02}"))
            .collect();
        let nodes: Vec<StartNode> = (0..12usize)
            .map(|index| {
                let tier = if index % 2 == 0 { 1 } else { 2 };
                node(&ids[index], tier, &[], &[])
            })
            .collect();
        let expected = vec![
            "plugin-00",
            "plugin-02",
            "plugin-04",
            "plugin-06",
            "plugin-08",
            "plugin-10",
            "plugin-01",
            "plugin-03",
            "plugin-05",
            "plugin-07",
            "plugin-09",
            "plugin-11",
        ];
        assert_eq!(
            order_or_panic(start_order(&nodes)),
            expected
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<String>>()
        );
    }

    /// REQ: LIF-047
    #[test]
    fn at_most_four_activations_run_at_once() {
        assert_eq!(MAX_CONCURRENT_ACTIVATIONS, 4);
    }

    // ---- shutdown_order (LIF-050, PKG-351) ----

    /// REQ: LIF-050
    #[test]
    fn shutdown_order_of_no_nodes_is_empty() {
        assert_eq!(order_or_panic(shutdown_order(&[])), Vec::<String>::new());
    }

    /// REQ: LIF-050
    #[test]
    fn shutdown_order_of_single_node_is_that_node() {
        let nodes = [node("solo", 1, &[], &[])];
        assert_eq!(
            order_or_panic(shutdown_order(&nodes)),
            vec!["solo".to_string()]
        );
    }

    /// REQ: PKG-351
    #[test]
    fn shutdown_order_is_exact_reverse_of_start_order_for_chain() {
        let nodes = [
            node("a", 1, &["b"], &[]),
            node("b", 1, &["c"], &[]),
            node("c", 3, &[], &[]),
        ];
        assert_eq!(
            order_or_panic(shutdown_order(&nodes)),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    /// REQ: PKG-351
    #[test]
    fn shutdown_order_reverses_tie_broken_start_order() {
        let nodes = [
            node("b", 1, &[], &[]),
            node("a", 2, &[], &[]),
            node("c", 1, &[], &[]),
        ];
        assert_eq!(
            order_or_panic(shutdown_order(&nodes)),
            vec!["a".to_string(), "c".to_string(), "b".to_string()]
        );
    }

    /// REQ: LIF-048
    #[test]
    fn shutdown_order_reports_the_same_cycle_as_start_order() {
        let nodes = [node("a", 1, &["b"], &[]), node("b", 1, &["a"], &[])];
        let diagnostics = cycle_diagnostics(shutdown_order(&nodes));
        assert!(diagnostics
            .iter()
            .any(|diagnostic| code_text(diagnostic) == "dependency.cycle"));
    }

    // ---- LifecycleManager (LIF-017, LIF-045, LIF-050, SEC-328) ----

    /// REQ: LIF-045
    #[test]
    fn start_with_no_installed_plugins_reports_nothing_started_or_failed() {
        let manager = LifecycleManager;
        assert_eq!(
            manager.start(StartMode::Normal),
            StartReport {
                started: Vec::new(),
                failed: Vec::new(),
            }
        );
    }

    /// REQ: SEC-328
    #[test]
    fn no_plugins_start_mode_starts_nothing() {
        let manager = LifecycleManager;
        assert_eq!(
            manager.start(StartMode::NoPlugins),
            StartReport {
                started: Vec::new(),
                failed: Vec::new(),
            }
        );
    }

    /// REQ: SEC-328
    #[test]
    fn safe_plugins_start_mode_with_no_installed_plugins_starts_nothing() {
        let manager = LifecycleManager;
        assert_eq!(
            manager.start(StartMode::SafePlugins),
            StartReport {
                started: Vec::new(),
                failed: Vec::new(),
            }
        );
    }

    /// REQ: LIF-050
    #[test]
    fn shutdown_with_zero_budget_and_no_plugins_deactivates_nothing() {
        let manager = LifecycleManager;
        assert_eq!(
            manager.shutdown(Duration::ZERO),
            ShutdownReport {
                deactivated: Vec::new(),
                forced: Vec::new(),
            }
        );
    }

    /// REQ: LIF-050
    #[test]
    fn shutdown_with_budget_and_no_plugins_forces_nothing() {
        let manager = LifecycleManager;
        let report = manager.shutdown(Duration::from_secs(5));
        assert!(report.deactivated.is_empty());
        assert!(report.forced.is_empty());
    }

    /// REQ: LIF-017
    #[test]
    fn plugin_info_of_unknown_plugin_is_none() {
        let manager = LifecycleManager;
        let absent = PluginId("ilium.test.absent".to_string());
        assert!(manager.plugin_info(&absent).is_none());
    }
}
