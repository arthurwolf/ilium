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
pub fn install_with_limits(_dir: &Path, _tier: Tier, _requested: &Limits) -> LimitsInstallReport {
    unimplemented!("host::install::install_with_limits")
}

/// The installer (reference-packaging.md).
#[derive(Debug)]
pub struct Installer;

impl Installer {
    /// An installer over a package store.
    pub fn new(_store: PackageStore) -> Installer {
        Installer
    }

    /// An installer over a package store with a state directory and options (PKG-262).
    pub fn with_options(
        _store: PackageStore,
        _state_dir: PathBuf,
        _options: InstallOptions,
    ) -> Installer {
        Installer
    }

    /// Runs the stages without changing disk (reference-check-and-lint-rules.md).
    pub fn check(&self, _package: &[u8], _mode: CheckMode) -> CheckReport {
        unimplemented!("host::install::Installer::check")
    }

    /// Installs a package after consent.
    pub fn install(
        &self,
        _package: &[u8],
        _consent: &dyn Consent,
    ) -> Result<InstallReport, Diagnostic> {
        unimplemented!("host::install::Installer::install")
    }

    /// Installs, then stops as if the process died right after `step` (testing hook, PKG-270..272).
    pub fn install_crashing_after(
        &self,
        _package: &[u8],
        _consent: &dyn Consent,
        _step: InstallStep,
    ) -> Result<(), Diagnostic> {
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
    pub fn update(
        &self,
        _package: &[u8],
        _consent: &dyn Consent,
    ) -> Result<InstallReport, Diagnostic> {
        unimplemented!("host::install::Installer::update")
    }

    /// Commits or rolls back a staged update after its first activation (PKG-280, LIF-075).
    pub fn finish_update(
        &self,
        _plugin: &PluginId,
        _first: FirstActivate,
    ) -> Result<UpdateOutcome, Diagnostic> {
        unimplemented!("host::install::Installer::finish_update")
    }

    /// Returns to the previous version.
    pub fn rollback(&self, _plugin: &PluginId) -> Result<InstallReport, Diagnostic> {
        unimplemented!("host::install::Installer::rollback")
    }

    /// Removes a plugin; `purge` also deletes its data, storage, settings and backups (PKG-294).
    pub fn uninstall(&self, _plugin: &PluginId, _purge: bool) -> Result<(), Diagnostic> {
        unimplemented!("host::install::Installer::uninstall")
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::Path;

    use ilium_plugin_api::ids::PluginId;
    use ilium_plugin_api::manifest::{CheckMode, Diagnostic};

    use super::{Consent, ConsentDecision, ConsentRequest, InstallOptions, Installer};
    use crate::registry::PackageStore;

    /// A consent double that counts how often the review was asked and always refuses.
    struct CountingConsent {
        asked: Cell<usize>,
    }

    impl CountingConsent {
        fn new() -> CountingConsent {
            CountingConsent {
                asked: Cell::new(0),
            }
        }
    }

    impl Consent for CountingConsent {
        fn decide(&self, _request: &ConsentRequest) -> ConsentDecision {
            self.asked.set(self.asked.get() + 1);
            ConsentDecision::Deny
        }
    }

    /// An installer over an empty package store rooted at `root`.
    fn installer_over(root: &Path) -> Installer {
        Installer::new(PackageStore {
            root: root.to_path_buf(),
        })
    }

    fn code_of(diagnostic: &Diagnostic) -> &str {
        diagnostic.code.as_str()
    }

    fn unknown_plugin() -> PluginId {
        PluginId("com.example.notes".to_owned())
    }

    /// Bytes that are not a ZIP archive, so stage 1 (archive) refuses them.
    const NOT_A_ZIP: &[u8] = b"this is not a zip archive at all";

    /// REQ: PKG-262
    #[test]
    fn default_install_options_refuse_unsigned_packages() {
        assert!(!InstallOptions::default().allow_unsigned);
    }

    /// REQ: PKG-165
    #[test]
    fn check_reports_zip_invalid_for_empty_input_at_stage_one() {
        let root = tempfile::tempdir().expect("temp dir");
        let report = installer_over(root.path()).check(&[], CheckMode::Install);
        assert_eq!(report.last_stage, 1);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| code_of(diagnostic) == "package.zip-invalid"),
            "expected package.zip-invalid, got {:?}",
            report.diagnostics
        );
    }

    /// REQ: PKG-165
    #[test]
    fn check_reports_zip_invalid_for_one_byte_input_at_stage_one() {
        let root = tempfile::tempdir().expect("temp dir");
        let report = installer_over(root.path()).check(&[0u8], CheckMode::Install);
        assert_eq!(report.last_stage, 1);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| code_of(diagnostic) == "package.zip-invalid"),
            "expected package.zip-invalid, got {:?}",
            report.diagnostics
        );
    }

    /// REQ: PKG-149
    #[test]
    fn check_stops_after_the_first_failing_stage() {
        let root = tempfile::tempdir().expect("temp dir");
        let report = installer_over(root.path()).check(NOT_A_ZIP, CheckMode::Install);
        assert_eq!(report.last_stage, 1);
        assert!(!report.diagnostics.is_empty());
        assert!(
            report.diagnostics.iter().all(|diagnostic| {
                let code = code_of(diagnostic);
                !code.starts_with("manifest.") && !code.starts_with("trust.")
            }),
            "stages after the archive stage must not run, got {:?}",
            report.diagnostics
        );
    }

    /// REQ: PKG-165
    #[test]
    fn install_refuses_empty_input_with_zip_invalid() {
        let root = tempfile::tempdir().expect("temp dir");
        let consent = CountingConsent::new();
        let diagnostic = installer_over(root.path())
            .install(&[], &consent)
            .expect_err("empty bytes are not a package");
        assert_eq!(code_of(&diagnostic), "package.zip-invalid");
    }

    /// REQ: PKG-165
    #[test]
    fn install_refuses_non_zip_bytes_with_zip_invalid() {
        let root = tempfile::tempdir().expect("temp dir");
        let consent = CountingConsent::new();
        let diagnostic = installer_over(root.path())
            .install(NOT_A_ZIP, &consent)
            .expect_err("non-ZIP bytes are not a package");
        assert_eq!(code_of(&diagnostic), "package.zip-invalid");
    }

    /// REQ: LIF-071
    #[test]
    fn install_never_asks_for_consent_when_validation_refuses_the_package() {
        let root = tempfile::tempdir().expect("temp dir");
        let consent = CountingConsent::new();
        let _refused = installer_over(root.path())
            .install(NOT_A_ZIP, &consent)
            .expect_err("non-ZIP bytes are not a package");
        assert_eq!(consent.asked.get(), 0);
    }

    /// REQ: LIF-071
    #[test]
    fn update_refuses_invalid_package_before_asking_for_consent() {
        let root = tempfile::tempdir().expect("temp dir");
        let consent = CountingConsent::new();
        let diagnostic = installer_over(root.path())
            .update(NOT_A_ZIP, &consent)
            .expect_err("non-ZIP bytes are not a package");
        assert_eq!(code_of(&diagnostic), "package.zip-invalid");
        assert_eq!(consent.asked.get(), 0);
    }

    /// REQ: PKG-272
    #[test]
    fn fresh_installer_has_instantiated_no_component() {
        let root = tempfile::tempdir().expect("temp dir");
        assert_eq!(installer_over(root.path()).instantiated_count(), 0);
    }

    /// REQ: PKG-151
    #[test]
    fn refused_install_instantiates_no_component() {
        let root = tempfile::tempdir().expect("temp dir");
        let consent = CountingConsent::new();
        let installer = installer_over(root.path());
        let _refused = installer
            .install(NOT_A_ZIP, &consent)
            .expect_err("non-ZIP bytes are not a package");
        assert_eq!(installer.instantiated_count(), 0);
    }

    /// REQ: PKG-291
    #[test]
    fn rollback_with_no_earlier_version_is_refused() {
        let root = tempfile::tempdir().expect("temp dir");
        let diagnostic = installer_over(root.path())
            .rollback(&unknown_plugin())
            .expect_err("nothing is installed, so there is no earlier version");
        assert_eq!(code_of(&diagnostic), "install.no-previous");
    }

    /// REQ: PKG-293
    #[test]
    fn uninstall_of_an_unknown_id_reports_not_installed() {
        let root = tempfile::tempdir().expect("temp dir");
        let diagnostic = installer_over(root.path())
            .uninstall(&unknown_plugin(), false)
            .expect_err("the id is not installed");
        assert_eq!(code_of(&diagnostic), "install.not-installed");
    }

    /// REQ: PKG-295
    #[test]
    fn purge_uninstall_of_an_unknown_id_reports_not_installed() {
        let root = tempfile::tempdir().expect("temp dir");
        let diagnostic = installer_over(root.path())
            .uninstall(&unknown_plugin(), true)
            .expect_err("the id is not installed");
        assert_eq!(code_of(&diagnostic), "install.not-installed");
    }

    /// REQ: PKG-271
    #[test]
    fn recover_at_start_on_clean_state_removes_nothing_and_is_repeatable() {
        let root = tempfile::tempdir().expect("temp dir");
        let state = tempfile::tempdir().expect("temp dir");
        let installer = Installer::with_options(
            PackageStore {
                root: root.path().to_path_buf(),
            },
            state.path().to_path_buf(),
            InstallOptions::default(),
        );
        let first = installer
            .recover_at_start()
            .expect("recovery on a clean state must succeed");
        assert!(first.removed_staging.is_empty());
        let second = installer
            .recover_at_start()
            .expect("a second recovery must succeed too");
        assert!(second.removed_staging.is_empty());
    }
}
