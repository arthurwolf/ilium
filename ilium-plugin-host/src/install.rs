//! Check, install, update, rollback and uninstall in twelve stages (reference-packaging.md,
//! reference-check-and-lint-rules.md, reference-install-and-update.md).
//!
//! Skeleton: every function with behaviour is `unimplemented!` until the implementation phase.

use std::path::{Path, PathBuf};

use ilium_plugin_api::grants::Grant;
use ilium_plugin_api::ids::PluginId;
use ilium_plugin_api::limits::Limits;
use ilium_plugin_api::manifest::{CheckMode, Diagnostic};
use ilium_plugin_api::Tier;

use crate::error::HostError;
use crate::registry::PackageStore;

/// What a consent prompt shows (reference-security.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentRequest {
    /// The plugin id.
    pub plugin: String,
    /// The version being installed.
    pub version: String,
    /// Grants the package requests.
    pub requested: Vec<Grant>,
}

/// The user's answer (reference-security.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsentDecision {
    /// Approve these grants.
    Approve(Vec<Grant>),
    /// Refuse the install.
    Deny,
}

/// Asks the user for consent (injected; tests use a scripted one).
pub trait Consent {
    /// Decides on a request.
    fn decide(&self, request: &ConsentRequest) -> ConsentDecision;
}

/// The findings of a check (reference-check-and-lint-rules.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    /// Every diagnostic, in stage order.
    pub diagnostics: Vec<Diagnostic>,
    /// The last stage that ran (1..=12).
    pub last_stage: u8,
}

/// The result of a completed install, update or rollback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReport {
    /// The plugin id.
    pub plugin: String,
    /// The version now active.
    pub version: String,
    /// The grants approved.
    pub granted: Vec<Grant>,
}

/// Installer switches (reference-install-and-update.md PKG-262).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstallOptions {
    /// Accept unsigned packages as T3 (reference-packaging.md PKG-262).
    pub allow_unsigned: bool,
}

/// The steps of an install, in order (reference-install-and-update.md PKG-270..272).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstallStep {
    /// Acquire the package.
    Acquire,
    /// Fetch its bytes.
    Fetch,
    /// Validate it.
    Validate,
    /// Review grants with the user.
    Review,
    /// Stage it on disk.
    Stage,
    /// Commit the record.
    Commit,
    /// Notify the sides.
    Notify,
}

/// What recovery at start removed (reference-install-and-update.md PKG-271).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    /// Staging directories of interrupted installs that were removed.
    pub removed_staging: Vec<PathBuf>,
}

/// How a staged update's first activation went (reference-install-and-update.md LIF-075).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstActivate {
    /// Activation succeeded.
    Ok,
    /// Activation returned an error.
    Error,
    /// Activation trapped.
    Trap,
    /// Activation missed its deadline.
    DeadlineMissed,
}

/// The end of an update (reference-install-and-update.md PKG-280, LIF-075).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateOutcome {
    /// The new version is active.
    Committed {
        /// The version now active.
        version: String,
        /// The reason given to the old version's `deactivate`.
        deactivation_reason: String,
    },
    /// The old version is active again and the new one is skipped.
    RolledBack {
        /// The version restored.
        restored_version: String,
        /// The version added to the skip list.
        skipped_version: String,
        /// The log code written.
        log_code: String,
        /// Whether the failure counts in the plugin's failure window (LIF-075: it does not).
        counted_in_failure_window: bool,
    },
}

/// The result of an install that clamps requested limits (reference-errors-and-limits.md HST-404).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LimitsInstallReport {
    /// Whether the install completed.
    pub installed: bool,
    /// The log lines written, oldest first.
    pub log: Vec<String>,
}

/// Installs a package directory, clamping its requested limits to the tier (HST-404).
pub fn install_with_limits(dir: &Path, tier: Tier, requested: &Limits) -> LimitsInstallReport {
    unimplemented!("host::install::install_with_limits")
}

/// The installer (reference-packaging.md).
#[derive(Debug)]
pub struct Installer {
    store: PackageStore,
    state_dir: Option<PathBuf>,
    options: InstallOptions,
}

impl Installer {
    /// An installer over a package store.
    pub fn new(store: PackageStore) -> Installer {
        Installer { store, state_dir: None, options: InstallOptions::default() }
    }

    /// An installer over a package store with a state directory and options (PKG-262).
    pub fn with_options(store: PackageStore, state_dir: PathBuf, options: InstallOptions) -> Installer {
        Installer { store, state_dir: Some(state_dir), options }
    }

    /// Runs the stages without changing disk (reference-check-and-lint-rules.md).
    pub fn check(&self, package: &[u8], mode: CheckMode) -> CheckReport {
        unimplemented!("host::install::Installer::check")
    }

    /// Installs a package after consent.
    pub fn install(&self, package: &[u8], consent: &dyn Consent) -> Result<InstallReport, Diagnostic> {
        unimplemented!("host::install::Installer::install")
    }

    /// Installs, then stops as if the process died right after `step` (testing hook, PKG-270..272).
    pub fn install_crashing_after(&self, package: &[u8], consent: &dyn Consent, step: InstallStep) -> Result<(), Diagnostic> {
        unimplemented!("host::install::Installer::install_crashing_after")
    }

    /// Removes leftovers of interrupted installs (reference-install-and-update.md PKG-271).
    pub fn recover_at_start(&self) -> Result<RecoveryReport, HostError> {
        unimplemented!("host::install::Installer::recover_at_start")
    }

    /// How many components this installer instantiated: zero before notify (PKG-151, PKG-272).
    pub fn instantiated_count(&self) -> usize {
        unimplemented!("host::install::Installer::instantiated_count")
    }

    /// Stages an update; the swap happens at the next safe point.
    pub fn update(&self, package: &[u8], consent: &dyn Consent) -> Result<InstallReport, Diagnostic> {
        unimplemented!("host::install::Installer::update")
    }

    /// Commits or rolls back a staged update after its first activation (PKG-280, LIF-075).
    pub fn finish_update(&self, plugin: &PluginId, first: FirstActivate) -> Result<UpdateOutcome, Diagnostic> {
        unimplemented!("host::install::Installer::finish_update")
    }

    /// Returns to the previous version.
    pub fn rollback(&self, plugin: &PluginId) -> Result<InstallReport, Diagnostic> {
        unimplemented!("host::install::Installer::rollback")
    }

    /// Removes a plugin; `purge` also deletes its data, storage, settings and backups (PKG-294).
    pub fn uninstall(&self, plugin: &PluginId, purge: bool) -> Result<(), Diagnostic> {
        unimplemented!("host::install::Installer::uninstall")
    }
}
