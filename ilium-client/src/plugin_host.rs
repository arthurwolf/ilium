//! The client's plugin host: the only client module that touches `ilium-plugin-host`
//! (test plan section 1.2). Client-side instances render views and handle input.
//! Nothing calls it yet, so no existing code path changes behaviour.

pub mod screens_rig;
pub mod widget_rig;

use std::sync::Mutex;
use std::time::Duration;

use ilium_plugin_api::ids::PluginId;
use ilium_plugin_host::lifecycle::{LifecycleManager, ShutdownReport, StartMode, StartReport};

/// Core plugins whose client side always starts unless `--no-plugins` is given (the Tree plugin
/// is in the client startup group of reference-kernel-and-boot).
const CORE_CLIENT_PLUGINS: &[&str] = &["ilium.core.tree"];

/// How long a shutdown may take before the remaining instances are stopped by force.
const SHUTDOWN_BUDGET: Duration = Duration::from_secs(5);

/// Owns the client-side plugin instances.
#[derive(Debug)]
pub struct ClientPluginHost {
    start_mode: StartMode,
    /// Plugin ids this host started, in start order; shutdown reports only these.
    started: Mutex<Vec<String>>,
}

impl ClientPluginHost {
    /// A host that will start in the given mode.
    pub fn new(start_mode: StartMode) -> ClientPluginHost {
        ClientPluginHost {
            start_mode,
            started: Mutex::new(Vec::new()),
        }
    }

    /// Locks the started list; a poisoned lock is recovered because the list is plain data.
    fn started_ids(&self) -> std::sync::MutexGuard<'_, Vec<String>> {
        match self.started.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// Starts the client-side instances of enabled plugins.
    pub fn start(&self) -> StartReport {
        let manager = LifecycleManager;
        let mut failed: Vec<(String, String)> = Vec::new();
        if self.start_mode != StartMode::NoPlugins {
            for plugin_id in CORE_CLIENT_PLUGINS {
                if let Err(error) = manager.enable(&PluginId((*plugin_id).to_owned())) {
                    failed.push(((*plugin_id).to_owned(), format!("{error:?}")));
                }
            }
        }
        let mut report = manager.start(self.start_mode);
        report
            .started
            .retain(|plugin_id| !failed.iter().any(|(failed_id, _)| failed_id == plugin_id));
        report.failed.extend(failed);
        *self.started_ids() = report.started.clone();
        report
    }

    /// Stops all client-side instances within the shutdown budget.
    pub fn shutdown(&self) -> ShutdownReport {
        let own: Vec<String> = std::mem::take(&mut *self.started_ids());
        if own.is_empty() {
            return ShutdownReport {
                deactivated: Vec::new(),
                forced: Vec::new(),
            };
        }
        let mut report = LifecycleManager.shutdown(SHUTDOWN_BUDGET);
        report.deactivated.retain(|plugin_id| own.contains(plugin_id));
        report.forced.retain(|plugin_id| own.contains(plugin_id));
        report
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    /// REQ: HST-033, HST-034
    #[test]
    fn no_plugins_start_starts_and_fails_nothing() {
        let report = ClientPluginHost::new(StartMode::NoPlugins).start();
        assert!(report.started.is_empty());
        assert!(report.failed.is_empty());
    }

    /// the Tree plugin is in the client startup group of reference-kernel-and-boot)
    /// REQ: HST-033
    #[test]
    fn safe_plugins_start_runs_only_core_plugins() {
        let report = ClientPluginHost::new(StartMode::SafePlugins).start();
        assert!(report
            .started
            .iter()
            .all(|plugin_id| plugin_id.starts_with("ilium.core.")));
        assert!(report
            .started
            .iter()
            .any(|plugin_id| plugin_id == "ilium.core.tree"));
    }

    /// REQ: LIF-001
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

    /// REQ: LIF-003
    #[test]
    fn normal_start_gives_every_failed_plugin_a_code() {
        let report = ClientPluginHost::new(StartMode::Normal).start();
        for (_failed_plugin_id, code) in &report.failed {
            assert!(!code.is_empty());
        }
    }

    /// started reports nothing deactivated and nothing forced)
    /// REQ: LIF-050
    #[test]
    fn shutdown_before_start_deactivates_nothing() {
        let report = ClientPluginHost::new(StartMode::Normal).shutdown();
        assert!(report.deactivated.is_empty());
        assert!(report.forced.is_empty());
    }

    /// deactivated cleanly or forced at the budget, never both)
    /// REQ: HST-030, HST-032
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
