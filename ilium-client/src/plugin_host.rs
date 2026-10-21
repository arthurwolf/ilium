//! The client's plugin host: the only client module that touches `ilium-plugin-host`
//! (test plan section 1.2). Client-side instances render views and handle input.
//! Skeleton: nothing calls it yet, so no existing code path changes behaviour.

pub mod screens_rig;
pub mod widget_rig;

use ilium_plugin_host::lifecycle::{ShutdownReport, StartMode, StartReport};

/// Owns the client-side plugin instances.
#[derive(Debug)]
pub struct ClientPluginHost {
    start_mode: StartMode,
}

impl ClientPluginHost {
    /// A host that will start in the given mode.
    pub fn new(start_mode: StartMode) -> ClientPluginHost {
        ClientPluginHost { start_mode }
    }

    /// Starts the client-side instances of enabled plugins.
    pub fn start(&self) -> StartReport {
        let _requested_start_mode = self.start_mode;
        unimplemented!("client::plugin_host::ClientPluginHost::start")
    }

    /// Stops all client-side instances within the shutdown budget.
    pub fn shutdown(&self) -> ShutdownReport {
        unimplemented!("client::plugin_host::ClientPluginHost::shutdown")
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    /// REQ: HST-033, HST-034 (`--no-plugins` starts no plugin host, so no plugin starts or fails)
    #[test]
    fn no_plugins_start_starts_and_fails_nothing() {
        let report = ClientPluginHost::new(StartMode::NoPlugins).start();
        assert!(report.started.is_empty());
        assert!(report.failed.is_empty());
    }

    /// REQ: HST-033 (`--safe-plugins` loads no T2 or T3 package, so only bundled core plugins start;
    /// the Tree plugin is in the client startup group of reference-kernel-and-boot)
    #[test]
    fn safe_plugins_start_runs_only_core_plugins() {
        let report = ClientPluginHost::new(StartMode::SafePlugins).start();
        assert!(report
            .started
            .iter()
            .all(|plugin_id| plugin_id.starts_with("ilium.core.")));
        assert!(report.started.iter().any(|plugin_id| plugin_id == "ilium.core.tree"));
    }

    /// REQ: LIF-001 (every plugin has exactly one state, so it is either started or failed, never both)
    #[test]
    fn normal_start_lists_each_plugin_in_one_outcome_only() {
        let report = ClientPluginHost::new(StartMode::Normal).start();
        for (failed_plugin_id, _code) in &report.failed {
            assert!(!report.started.contains(failed_plugin_id));
        }
        let mut seen = report.started.clone();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), report.started.len());
    }

    /// REQ: LIF-003 (a plugin that fails to start carries a non-empty refusal or failure code)
    #[test]
    fn normal_start_gives_every_failed_plugin_a_code() {
        let report = ClientPluginHost::new(StartMode::Normal).start();
        for (_failed_plugin_id, code) in &report.failed {
            assert!(!code.is_empty());
        }
    }

    /// REQ: LIF-050 (only active plugins receive `deactivate(shutdown)`, so a host that never
    /// started reports nothing deactivated and nothing forced)
    #[test]
    fn shutdown_before_start_deactivates_nothing() {
        let report = ClientPluginHost::new(StartMode::Normal).shutdown();
        assert!(report.deactivated.is_empty());
        assert!(report.forced.is_empty());
    }

    /// REQ: HST-030, HST-032 (shutdown reaches only plugins that started, and a plugin is either
    /// deactivated cleanly or forced at the budget, never both)
    #[test]
    fn shutdown_reports_only_started_plugins_once_each() {
        let host = ClientPluginHost::new(StartMode::Normal);
        let started = host.start();
        let report = host.shutdown();
        for plugin_id in report.deactivated.iter().chain(report.forced.iter()) {
            assert!(started.started.contains(plugin_id));
        }
        for plugin_id in &report.forced {
            assert!(!report.deactivated.contains(plugin_id));
        }
    }
}
