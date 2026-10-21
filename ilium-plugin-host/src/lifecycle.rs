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
pub fn resolve_safe_mode(
    _args: &[String],
    _env_value: Option<&str>,
) -> Result<SafeMode, UsageError> {
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
pub fn start_order(_nodes: &[StartNode]) -> Result<Vec<String>, Vec<ilium_plugin_api::Diagnostic>> {
    unimplemented!("host::lifecycle::start_order")
}

/// The shutdown order: the exact reverse of [`start_order`] (PKG-351).
pub fn shutdown_order(
    _nodes: &[StartNode],
) -> Result<Vec<String>, Vec<ilium_plugin_api::Diagnostic>> {
    unimplemented!("host::lifecycle::shutdown_order")
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
            requires_plugins: requires_plugins.iter().map(|text| text.to_string()).collect(),
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
            panic!("expected an activation order, got {} diagnostics", diagnostics.len())
        })
    }

    fn cycle_diagnostics(result: Result<Vec<String>, Vec<Diagnostic>>) -> Vec<Diagnostic> {
        match result {
            Ok(order) => panic!("expected a cycle diagnostic, got the order {order:?}"),
            Err(diagnostics) => diagnostics,
        }
    }

    fn code_text(diagnostic: &Diagnostic) -> &str {
        &*diagnostic.code.0
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
        assert_eq!(order_or_panic(start_order(&nodes)), vec!["solo".to_string()]);
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
        let nodes = [
            node("app", 1, &[], &["svc"]),
            node("svc", 2, &[], &[]),
        ];
        assert_eq!(
            order_or_panic(start_order(&nodes)),
            vec!["svc".to_string(), "app".to_string()]
        );
    }

    /// REQ: LIF-048
    #[test]
    fn plugin_requirement_cycle_is_reported_as_dependency_cycle() {
        let nodes = [
            node("a", 1, &["b"], &[]),
            node("b", 1, &["a"], &[]),
        ];
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
        let nodes = [
            node("a", 1, &[], &["b"]),
            node("b", 1, &[], &["a"]),
        ];
        let diagnostics = cycle_diagnostics(start_order(&nodes));
        assert!(diagnostics
            .iter()
            .any(|diagnostic| code_text(diagnostic) == "manifest.service-cycle"));
    }

    /// REQ: LIF-047
    #[test]
    fn independent_plugins_start_in_key_order() {
        let ids: Vec<String> = (0..12usize).map(|index| format!("plugin-{index:02}")).collect();
        let nodes: Vec<StartNode> = (0..12usize)
            .map(|index| {
                let tier = if index % 2 == 0 { 1 } else { 2 };
                node(&ids[index], tier, &[], &[])
            })
            .collect();
        let expected = vec![
            "plugin-00", "plugin-02", "plugin-04", "plugin-06", "plugin-08", "plugin-10",
            "plugin-01", "plugin-03", "plugin-05", "plugin-07", "plugin-09", "plugin-11",
        ];
        assert_eq!(
            order_or_panic(start_order(&nodes)),
            expected.into_iter().map(str::to_string).collect::<Vec<String>>()
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
        assert_eq!(order_or_panic(shutdown_order(&nodes)), vec!["solo".to_string()]);
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
        let nodes = [
            node("a", 1, &["b"], &[]),
            node("b", 1, &["a"], &[]),
        ];
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
