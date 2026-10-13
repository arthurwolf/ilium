//! Check, install, update, rollback and uninstall in twelve stages (reference-packaging.md,
//! reference-check-and-lint-rules.md).

use ilium_plugin_api::grants::Grant;
use ilium_plugin_api::ids::PluginId;
use ilium_plugin_api::manifest::{CheckMode, Diagnostic};

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

/// The installer (reference-packaging.md).
#[derive(Debug)]
pub struct Installer {
    store: PackageStore,
}

impl Installer {
    /// An installer over a package store.
    pub fn new(store: PackageStore) -> Installer {
        Installer { store }
    }

    /// Runs the stages without changing disk (reference-check-and-lint-rules.md).
    pub fn check(&self, package: &[u8], mode: CheckMode) -> CheckReport {
        unimplemented!("host::install::Installer::check")
    }

    /// Installs a package after consent.
    pub fn install(&self, package: &[u8], consent: &dyn Consent) -> Result<InstallReport, Diagnostic> {
        unimplemented!("host::install::Installer::install")
    }

    /// Stages an update; the swap happens at the next safe point.
    pub fn update(&self, package: &[u8], consent: &dyn Consent) -> Result<InstallReport, Diagnostic> {
        unimplemented!("host::install::Installer::update")
    }

    /// Returns to the previous version.
    pub fn rollback(&self, plugin: &PluginId) -> Result<InstallReport, Diagnostic> {
        unimplemented!("host::install::Installer::rollback")
    }

    /// Removes a plugin, keeping or deleting its data.
    pub fn uninstall(&self, plugin: &PluginId, keep_data: bool) -> Result<(), Diagnostic> {
        unimplemented!("host::install::Installer::uninstall")
    }
}
