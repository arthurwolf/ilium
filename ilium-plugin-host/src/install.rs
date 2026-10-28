//! Check, install, update, rollback and uninstall in eleven admission stages (reference-packaging.md,
//! reference-check-and-lint-rules.md, reference-install-and-update.md).
//!
//! Package checks are offline and never instantiate guest components.

use std::path::{Path, PathBuf};

use ilium_plugin_api::grants::Grant;
use ilium_plugin_api::ids::PluginId;
use ilium_plugin_api::limits::Limits;
use ilium_plugin_api::manifest::{CheckMode, Diagnostic};
use ilium_plugin_api::Tier;

use crate::error::HostError;
use crate::registry::PackageStore;
use ilium_plugin_api::{Code, Severity, Side};
use ilium_plugin_api::manifest::{self, ComponentSummary, Manifest, Stage, ValidationContext};
use sha2::{Digest, Sha256};
use wasmparser::{Parser, Payload, Validator};

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
    /// The last stage that ran (1..=11).
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

/// Policy results for one real component side. Requested and currently retained
/// authorization remain distinct; neither result records consent or grants access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidePolicyReview {
    pub side: Side,
    pub requested: Vec<(Grant, crate::policy::PolicyVerdict)>,
    pub retained: Vec<(Grant, crate::policy::PolicyVerdict)>,
}

/// Offline byte validation followed by explicit-context policy evaluation.
/// This is not an installation receipt or a substitute for trusted admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagePolicyReview {
    pub check: CheckReport,
    pub installation: crate::policy::PolicyVerdict,
    pub sides: Vec<SidePolicyReview>,
}

/// Provenance already admitted by the file/URL/registry transport. These values
/// are host facts, not fields copied from an untrusted package manifest.
#[derive(Debug, Clone)]
pub struct InstallProvenance {
    pub source: String,
    pub kind: crate::policy::PolicySourceKind,
    pub channel: String,
}
/// A durable user decision bound to one exact package, signer, side set and epoch.
#[derive(Debug, Clone)]
pub struct InstallGrantDecision {
    pub plugin: String,
    pub manifest_sha256: String,
    pub key_id: Option<String>,
    pub tier: Tier,
    pub grants: manifest::Capabilities,
    pub epoch: u64,
    pub grants_sha256: String,
}
/// Host-owned audit and notification adapters. Approval recording must be durable
/// and must derive from the actual Consent response, never a plugin-supplied actor.
pub trait InstallEffects {
    fn record_approval(&self, decision: &InstallGrantDecision) -> Result<(), Diagnostic>;
    fn plugins_changed(&self) -> Result<(), Diagnostic>;
}
/// Complete, explicit host context. The legacy installer never invents this.
/// Lock order is installed.lock -> stores.lock; effects must not reacquire either.
pub struct TrustedInstallContext<'a> {
    pub trust: &'a crate::trust::TrustSnapshot,
    pub policy: &'a crate::policy::PolicyEngine,
    pub validation: ValidationContext,
    pub host_version: ilium_plugin_api::ApiVersion,
    pub provenance: InstallProvenance,
    pub grant_store: &'a mut crate::stores::GrantStore,
    pub effects: &'a dyn InstallEffects,
}
#[derive(Debug, Clone)]
pub struct TrustedInstallReport {
    pub installed: InstallReport,
    pub tier: Tier,
    pub grants: manifest::Capabilities,
    pub epoch: u64,
    pub warnings: Vec<Diagnostic>,
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
pub fn install_with_limits(_dir: &Path, tier: Tier, requested: &Limits) -> LimitsInstallReport {
    // This compatibility helper resolves the limits portion of admission only. It does
    // not install package bytes or infer package validity from the directory's existence.
    match ilium_plugin_api::limits::clamp(requested, tier) {
        Ok(effective) => LimitsInstallReport {
            installed: true,
            log: ilium_plugin_api::limits::LimitKey::ALL.into_iter().filter_map(|key| {
                let value = requested.0.get(key.as_key())?;
                let actual = effective.get(&key)?;
                (*value != *actual).then(|| format!("limits.clamped {} requested={} effective={}", key.as_key(), value, actual))
            }).collect(),
        },
        Err(error) => LimitsInstallReport { installed: false, log: vec![error.to_string()] },
    }
}

/// The installer (reference-packaging.md).
#[derive(Debug)]
pub struct Installer {
    store: PackageStore,
    state_dir: Option<PathBuf>,
    options: InstallOptions,
    check_context: Option<ValidationContext>,
    public_keys: Vec<crate::signing::TrustedKey>,
}

impl Installer {
    /// An installer over a package store.
    pub fn new(store: PackageStore) -> Installer {
        Self { store, state_dir: None, options: InstallOptions::default(), check_context: None, public_keys: Vec::new() }
    }

    /// An installer over a package store with a state directory and options (PKG-262).
    pub fn with_options(
        store: PackageStore,
        state_dir: PathBuf,
        options: InstallOptions,
    ) -> Installer {
        Self { store, state_dir: Some(state_dir), options, check_context: None, public_keys: Vec::new() }
    }

    /// Supplies read-only manifest context and public keys for cryptographic checking.
    /// Key availability does not establish trust or authorize installation.
    pub fn with_check_context(mut self, context: ValidationContext, public_keys: Vec<crate::signing::TrustedKey>) -> Self {
        self.check_context = Some(context);
        self.public_keys = public_keys;
        self
    }

    pub fn package_store(&self) -> &PackageStore { &self.store }
    pub fn state_directory(&self) -> Option<&Path> { self.state_dir.as_deref() }
    pub fn options(&self) -> &InstallOptions { &self.options }

    /// Runs the stages without changing disk (reference-check-and-lint-rules.md).
    pub fn check(&self, package: &[u8], mode: CheckMode) -> CheckReport {
        let entries = match crate::package::read_archive_entries(package) {
            Ok(entries) => entries,
            Err(errors) => return stage_report(errors, 1),
        };
        let Some(raw) = entries.iter().find(|entry| entry.path == "manifest.json") else {
            return stage_report(vec![finding("package.manifest-missing", "", "manifest.json is required", 1)], 1);
        };
        let text = match std::str::from_utf8(&raw.bytes) {
            Ok(text) => text,
            Err(_) => return stage_report(vec![finding("manifest.json-invalid", "", "manifest.json must be UTF-8", 2)], 2),
        };
        // Check mode excludes tier/platform/dependency policy, which must not run
        // before signature and trust. The default platform is an unused placeholder.
        let mut context = self.check_context.clone().unwrap_or(ValidationContext {
            tier: Tier::T3, mode: CheckMode::Check, platform: ilium_plugin_api::Platform::Linux,
            installed: Vec::new(), core_plugins: Vec::new(),
            unstable_features: ilium_plugin_api::abi::UNSTABLE_FEATURES.iter().map(|s| (*s).into()).collect(),
        });
        context.mode = CheckMode::Check;
        let errors = manifest::validate_json(text, &context);
        if errors.iter().any(|error| error.stage.is_none_or(|s| s.0 <= 3)) { return stage_report(errors, 3); }
        let manifest = match manifest::parse_manifest(text) {
            Ok(manifest) => manifest,
            Err(errors) => return stage_report(errors, 3),
        };
        let document: serde_json::Value = match serde_json::from_str(text) {
            Ok(value) => value,
            Err(_) => return stage_report(vec![finding("manifest.json-invalid", "", "invalid manifest JSON", 2)], 2),
        };
        if document.get("files").is_none() {
            return stage_report(vec![finding("manifest.key-missing", "/files", "packed manifest requires files", 3)], 3);
        }
        if !errors.is_empty() { return stage_report(errors, 4); }
        let errors = inventory(&manifest, &entries);
        if !errors.is_empty() { return stage_report(errors, 5); }
        let mut diagnostics = components(&manifest, &entries, &context);
        if diagnostics.iter().any(|d| d.severity == Severity::Error) { return stage_report(diagnostics, 6); }
        // Signature validity is a DEV-203 check rule, independent of key trust.
        if let Some(signature) = entries.iter().find(|entry| entry.path == "signature") {
            if let Err(error) = crate::signing::validate_signature_document(&signature.bytes) {
                return stage_report(vec![error], 7);
            }
            let signature_bytes = signature.bytes.clone();
            let archive = crate::package::PackageArchive { manifest, entries, signature: None };
            let result = crate::signing::verify_with_public_keys(&archive, &crate::signing::TrustStore::default(), &self.public_keys, std::time::SystemTime::now());
            match result {
                // This result means every signature verified, but no key was trusted.
                Err(error) if error.code.as_str() == "trust.key-unknown" => {
                    let document: serde_json::Value = serde_json::from_slice(&signature_bytes).unwrap_or_default();
                    if let Some(signatures) = document["signatures"].as_array() {
                        for signature in signatures {
                            let mut info = finding("signature.fingerprint", "", signature["key_id"].as_str().unwrap_or_default(), 7);
                            info.severity = Severity::Info; diagnostics.push(info);
                        }
                    }
                }
                Err(error) => return stage_report(vec![error], 7),
                Ok(_) => {}
            }
        } else {
            let mut warning = finding("signature.absent", "", "unsigned package", 7);
            warning.severity = Severity::Warning; diagnostics.push(warning);
        }
        if mode != CheckMode::Check {
            diagnostics.push(finding("install.context-required", "", "installation requires effective trust, publisher binding, administrative policy and version state; check context grants no installation authority", 8));
            return CheckReport { diagnostics, last_stage: 8 };
        }
        CheckReport { diagnostics, last_stage: 7 }
    }

    /// Checks real package bytes before evaluating policy. The caller must supply
    /// authenticated tier/publisher/source facts and actual retained grants from
    /// the authorization store. No package metadata is promoted into trust.
    /// Each side is evaluated separately (SEC-106); denied grants lock rows off
    /// without failing package admission (SEC-198). This never invokes consent,
    /// persists grants, stages files, or bypasses `install`'s missing-context gate.
    pub fn review_package_policy(
        &self,
        package: &[u8],
        policy: &crate::policy::PolicyEngine,
        context: &crate::policy::InstallPolicyContext<'_>,
        retained: &manifest::Capabilities,
    ) -> Result<PackagePolicyReview, Diagnostic> {
        let check = self.check(package, CheckMode::Check);
        if let Some(error) = check.diagnostics.iter().find(|d| d.severity == Severity::Error) {
            return Err(error.clone());
        }
        let entries = crate::package::read_archive_entries(package)
            .map_err(|errors| errors.into_iter().next().unwrap_or_else(|| finding("package.invalid", "", "archive could not be read", 1)))?;
        let raw = entries.iter().find(|entry| entry.path == "manifest.json")
            .ok_or_else(|| finding("package.manifest-missing", "", "manifest.json is required", 1))?;
        let text = std::str::from_utf8(&raw.bytes)
            .map_err(|_| finding("manifest.json-invalid", "", "manifest.json must be UTF-8", 2))?;
        let declared = manifest::parse_manifest(text)
            .map_err(|errors| errors.into_iter().next().unwrap_or_else(|| finding("manifest.json-invalid", "", "manifest could not be parsed", 2)))?;
        if declared.id != context.plugin {
            return Err(finding("install.context-required", "/id", "policy identity must match the checked package", 8));
        }
        let installation = policy.check_install(context);
        let mut sides = Vec::new();
        for (side, requested, held) in [
            (Side::Server, &declared.capabilities.server, &retained.server),
            (Side::Client, &declared.capabilities.client, &retained.client),
            (Side::Scene, &declared.capabilities.scene, &retained.scene),
        ] {
            let review = policy.review_install_grants_with_retained(context, requested, held);
            // Evaluate retained rows against the same union, so admission never
            // hides an existing read permission behind a newly denied egress row.
            let mut combined = held.clone();
            for grant in requested {
                if !combined.contains(grant) { combined.push(grant.clone()); }
            }
            let retained = held.iter().map(|grant| (grant.clone(),
                policy.check_grant_in_set_for(context.plugin, context.tier, grant, &combined))).collect();
            sides.push(SidePolicyReview { side, requested: review.grants, retained });
        }
        Ok(PackagePolicyReview { check, installation, sides })
    }

    /// Fresh-install admission through all eleven stages, followed by immutable
    /// staging and the installed-registry commit point. Updates require the
    /// separate activation/migration transaction and are explicitly refused here.
    pub fn install_with_context(&self, package: &[u8], context: &mut TrustedInstallContext<'_>, consent: &dyn Consent)
        -> Result<TrustedInstallReport, Diagnostic> {
        use crate::policy::{InstallPolicyContext, PolicySourceKind, PolicyVerdict};
        let root = self.store.root.parent().filter(|_| self.store.root.is_absolute())
            .ok_or_else(|| finding("install.context-required","","absolute package store root required",8))?;
        if self.store.root.file_name() != Some(std::ffi::OsStr::new("store")) {
            return Err(finding("install.context-required","","package store must use the canonical store directory",8));
        }
        prepare_install_root(root)?;
        let registry = crate::registry::InstalledRegistry::open(&root.join("installed.json")).map_err(host_diagnostic)?;
        let mut transaction = registry.transaction().map_err(host_diagnostic)?;
        let mut checker = Self::new(self.store.clone());
        checker.check_context = Some(context.validation.clone());
        checker.public_keys = self.public_keys.clone();
        // Revoked keys still supply public bytes for stage-7 crypto validation;
        // only stage 8 decides their authority and emits trust.key-revoked.
        for resolved in context.trust.keys() {
            let public_key = base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD,&resolved.entry.public_key)
                .map_err(|_| finding("trust.store-invalid","","invalid public key in trust snapshot",8))?;
            checker.public_keys.push(crate::signing::TrustedKey {key_id:resolved.entry.key_id.clone(),publisher_id:resolved.entry.publisher_id.clone(),public_key:crate::signing::PublicKey(public_key)});
        }
        let checked = checker.check(package,CheckMode::Check);
        if let Some(error) = checked.diagnostics.iter().find(|error| error.severity == Severity::Error) { return Err(error.clone()); }
        let archive = crate::package::read_package(package).map_err(|errors| errors.into_iter().next()
            .unwrap_or_else(|| finding("package.invalid","","archive unavailable",1)))?;
        let manifest = &archive.manifest;
        let raw = archive.entries.iter().find(|entry| entry.path == "manifest.json")
            .ok_or_else(|| finding("package.manifest-missing","","manifest missing",1))?;
        let manifest_hash = format!("{:x}",Sha256::digest(&raw.bytes));
        let signed = archive.entries.iter().any(|entry| entry.path == "signature");
        let verified = if signed { Some(context.trust.verify_package(&archive).map_err(|mut error| {error.stage=Some(Stage(8));error})?) }
            else if self.options.allow_unsigned { None }
            else { return Err(finding("install.unsigned","","unsigned install requires explicit allow_unsigned",8)); };
        let tier = verified.as_ref().map(|verified| verified.tier).unwrap_or(Tier::T3);
        let key = verified.as_ref().map(|verified| verified.key_id.as_str());
        if tier == Tier::T1 { return Err(finding("install.context-required","","bundled tier requires the release installer",8)); }
        let provenance = &context.provenance;
        if !matches!(provenance.channel.as_str(),"stable"|"beta"|"dev"|"file") || provenance.source.is_empty()
            || matches!(provenance.kind,PolicySourceKind::Bundle|PolicySourceKind::Development) {
            return Err(finding("install.context-required","","distribution provenance required; bundled/development use separate admission",8));
        }
        if context.validation.platform != ilium_plugin_api::Platform::Linux {
            return Err(finding("package.platform-unsupported","","third-party plugins require Linux",9));
        }
        if let Some(old) = transaction.record(&manifest.id) {
            if old["active_version"] == manifest.version && old["manifest_sha256"] != manifest_hash {
                return Err(finding("package.version-reused","/version","same version has a different manifest",9));
            }
            return Err(finding("install.context-required","","existing installations require update/activation and data-version context",9));
        }
        for installed in transaction.document()["plugins"].as_array().into_iter().flatten() {
            if !context.validation.installed.iter().any(|known| Some(known.id.as_str()) == installed["id"].as_str() && Some(known.version.as_str()) == installed["active_version"].as_str()) {
                return Err(finding("install.context-required","","installed dependency context is stale or incomplete",9));
            }
        }
        let policy_context = InstallPolicyContext {plugin:&manifest.id,tier,publisher_key:key,source:&provenance.source,
            source_kind:provenance.kind,channel:&provenance.channel,development:false,replaces:!manifest.replaces.is_empty()};
        if let PolicyVerdict::Deny(reason) = context.policy.check_install(&policy_context) {
            let mut diagnostic = Diagnostic::error(Code(std::borrow::Cow::Owned(reason.clone())),"",reason);
            diagnostic.stage = Some(Stage(9));
            return Err(diagnostic);
        }
        let mut validation = context.validation.clone(); validation.tier=tier; validation.mode=CheckMode::Install;
        let text = std::str::from_utf8(&raw.bytes).map_err(|_| finding("manifest.json-invalid","","manifest UTF-8",2))?;
        if let Some(error) = manifest::validate_json(text,&validation).into_iter().find(|error| error.severity == Severity::Error) { return Err(error); }
        crate::resolve::resolve(std::slice::from_ref(manifest),&validation.installed,context.host_version)
            .map_err(|errors| {let mut error=errors.into_iter().next().unwrap_or_else(|| finding("dependency.invalid","","dependency resolution failed",10));error.stage=Some(Stage(10));error})?;
        let tier_text = if tier == Tier::T2 {"t2"} else {"t3"};
        let identity_key = key.unwrap_or("");
        let mut reviewed = manifest::Capabilities::default();
        for (side, requested, output) in [(Side::Server,&manifest.capabilities.server,&mut reviewed.server),
            (Side::Client,&manifest.capabilities.client,&mut reviewed.client),(Side::Scene,&manifest.capabilities.scene,&mut reviewed.scene)] {
            let held = context.grant_store.load_for_side(&manifest.id,identity_key,tier_text,side).map_err(store_diagnostic)?;
            let review = context.policy.review_install_grants_with_retained(&policy_context,requested,&held);
            *output = review.grants.into_iter().filter_map(|(grant, verdict)| (verdict == PolicyVerdict::Allow).then_some(grant)).collect();
        }
        let mut requested: Vec<Grant> = reviewed.server.iter().chain(&reviewed.client).chain(&reviewed.scene).cloned().collect();
        requested.sort(); requested.dedup();
        let approved = match consent.decide(&ConsentRequest {plugin:manifest.id.clone(),version:manifest.version.clone(),requested:requested.clone()}) {
            ConsentDecision::Approve(approved) => approved,
            ConsentDecision::Deny => return Err(finding("install.declined","","user declined installation",11)),
        };
        if approved.iter().any(|grant| !requested.contains(grant)) { return Err(finding("install.approval-invalid","","consent approved an unrequested or policy-denied grant",11)); }
        for entries in [&mut reviewed.server,&mut reviewed.client,&mut reviewed.scene] { entries.retain(|grant| approved.contains(grant)); }
        let plugin = PluginId(manifest.id.clone());
        let destination = self.store.checked_version_dir(&plugin,&manifest.version).map_err(host_diagnostic)?;
        let mut grant_transaction = context.grant_store.install_transaction().map_err(store_diagnostic)?;
        let staged = stage_package(root,&destination,package,&archive.entries,&manifest_hash)?;
        let result = (|| {
            let epoch = grant_transaction.replace_all_sides(&manifest.id,identity_key,tier_text,&reviewed).map_err(store_diagnostic)?;
            context.effects.record_approval(&InstallGrantDecision {plugin:manifest.id.clone(),manifest_sha256:manifest_hash.clone(),
                key_id:key.map(str::to_owned),tier,grants:reviewed.clone(),epoch,grants_sha256:grant_transaction.sha256().into()})?;
            let source = match provenance.kind {PolicySourceKind::File=>"file",PolicySourceKind::Url=>"url",PolicySourceKind::Registry=>"registry",_=>return Err(finding("install.context-required","","unsupported provenance",8))};
            let installed_at = format!("{}Z",crate::trust::utc_timestamp(std::time::SystemTime::now())?.replace(' ',"T"));
            transaction.put_complete_record(serde_json::json!({"id":manifest.id,"active_version":manifest.version,
                "previous_version":null,"state":"installed-enabled","source":source,
                "source_url":if source == "file" {None} else {Some(&provenance.source)},"channel":provenance.channel,
                "publisher_id":verified.as_ref().map(|verified| &verified.publisher_id),"key_id":key,
                "package_sha256":format!("{:x}",Sha256::digest(package)),"manifest_sha256":manifest_hash,
                "pinned":false,"skip":[],"declined":[],"failure_stopped":[],"installed_at":installed_at,"update":null})).map_err(host_diagnostic)?;
            transaction.commit().map_err(host_diagnostic)?;
            Ok(epoch)
        })();
        let epoch = match result {
            Ok(epoch) => epoch,
            Err(error) => {
                // Registry has not committed on ordinary failure. Report rollback
                // failure instead of silently claiming the old authorization survived.
                if let Err(rollback) = grant_transaction.rollback() { return Err(finding("install.rollback-failed","",format!("{error}; grants: {rollback:?}"),11)); }
                if let Err(rollback) = transaction.rollback() { return Err(finding("install.rollback-failed","",format!("{error}; registry: {rollback}"),11)); }
                if let Err(rollback) = std::fs::remove_dir_all(&staged) { return Err(finding("install.rollback-failed","",format!("{error}; staged package: {rollback}"),11)); }
                return Err(error);
            }
        };
        let warnings = context.effects.plugins_changed().err().into_iter().map(|mut error| {error.severity=Severity::Warning;error}).collect();
        let mut granted = approved; granted.sort(); granted.dedup();
        Ok(TrustedInstallReport {installed:InstallReport {plugin:manifest.id.clone(),version:manifest.version.clone(),granted},tier,grants:reviewed,epoch,warnings})
    }

    /// Installs a package after consent.
    pub fn install(
        &self,
        package: &[u8],
        _consent: &dyn Consent,
    ) -> Result<InstallReport, Diagnostic> {
        let report = self.check(package, CheckMode::Install);
        Err(report.diagnostics.into_iter().find(|d| d.severity == Severity::Error).unwrap_or_else(||
            finding("install.context-required", "", "durable admission and notification context is unavailable", 11)))
    }

    /// The install root that holds `installed.json`, `staging/` and `store/`: the state directory, else the
    /// parent of a canonical `store` directory.
    fn install_root(&self) -> Option<PathBuf> {
        if let Some(state) = &self.state_dir { return Some(state.clone()); }
        if self.store.root.file_name() == Some(std::ffi::OsStr::new("store")) {
            return self.store.root.parent().map(Path::to_owned);
        }
        None
    }

    /// Validation first (a refused package never reaches consent, LIF-071), then the refusal that the
    /// context-free installer gives for every durable change: admission needs a [`TrustedInstallContext`].
    fn refuse_without_context(&self, package: &[u8]) -> Diagnostic {
        let report = self.check(package, CheckMode::Install);
        report.diagnostics.into_iter().find(|d| d.severity == Severity::Error).unwrap_or_else(||
            finding("install.context-required", "", "durable admission and notification context is unavailable", 11))
    }

    /// The registry of this installer's root, or `None` when nothing was ever installed there.
    fn existing_registry(&self) -> Result<Option<(PathBuf, crate::registry::InstalledRegistry)>, Diagnostic> {
        let Some(root) = self.install_root() else { return Ok(None) };
        let path = root.join("installed.json");
        if !path.is_file() { return Ok(None); }
        let registry = crate::registry::InstalledRegistry::open(&path).map_err(host_diagnostic)?;
        Ok(Some((root, registry)))
    }

    /// Installs, then stops as if the process died right after `step` (testing hook, PKG-270..272).
    ///
    /// Without a trusted context no step past validation can run, so the refusal is the same as for
    /// [`Installer::install`]; nothing is staged and no component is instantiated.
    pub fn install_crashing_after(
        &self,
        package: &[u8],
        _consent: &dyn Consent,
        _step: InstallStep,
    ) -> Result<(), Diagnostic> {
        Err(self.refuse_without_context(package))
    }

    /// Removes leftovers of interrupted installs (reference-install-and-update.md PKG-271).
    ///
    /// Deletes `staging/install-*` directories whose owning process is gone or that are older than one
    /// hour. Directories of this process and of live processes are kept; `installed.json` never
    /// references `staging/` because a publish renames the directory into `store/`.
    pub fn recover_at_start(&self) -> Result<RecoveryReport, HostError> {
        const STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(60 * 60);
        let mut report = RecoveryReport::default();
        let Some(root) = self.install_root() else { return Ok(report) };
        let staging = root.join("staging");
        let entries = match std::fs::read_dir(&staging) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(report),
            Err(error) => return Err(HostError::Diagnostic(finding("install.io", "", error.to_string(), 11))),
        };
        for entry in entries {
            let entry = entry.map_err(|error| HostError::Diagnostic(finding("install.io", "", error.to_string(), 11)))?;
            let name = entry.file_name();
            let Some(name_text) = name.to_str() else { continue };
            let Some(rest) = name_text.strip_prefix("install-") else { continue };
            let owner = rest.split('-').next().and_then(|digits| digits.parse::<u32>().ok());
            let metadata = match std::fs::symlink_metadata(entry.path()) {
                Ok(metadata) if metadata.is_dir() => metadata,
                _ => continue,
            };
            let is_old = metadata.modified().ok()
                .and_then(|modified| std::time::SystemTime::now().duration_since(modified).ok())
                .is_some_and(|age| age > STALE_AFTER);
            let owner_gone = owner.is_some_and(|process_id| process_id != std::process::id()
                && ilium_platform::process_info::executable_path(process_id).is_none());
            if !(is_old || owner_gone) { continue; }
            std::fs::remove_dir_all(entry.path())
                .map_err(|error| HostError::Diagnostic(finding("install.io", "", format!("{}: {error}", entry.path().display()), 11)))?;
            report.removed_staging.push(entry.path());
        }
        report.removed_staging.sort();
        Ok(report)
    }

    /// How many components this installer instantiated: zero before notify (PKG-151, PKG-272).
    pub fn instantiated_count(&self) -> usize { 0 }

    /// Stages an update; the swap happens at the next safe point.
    ///
    /// The package is validated before any consent prompt (LIF-071). Staging needs the trusted context
    /// of [`Installer::install_with_context`], so the context-free installer refuses after validation.
    pub fn update(
        &self,
        package: &[u8],
        _consent: &dyn Consent,
    ) -> Result<InstallReport, Diagnostic> {
        Err(self.refuse_without_context(package))
    }

    /// Commits or rolls back a staged update after its first activation (PKG-280, LIF-075).
    pub fn finish_update(
        &self,
        plugin: &PluginId,
        first: FirstActivate,
    ) -> Result<UpdateOutcome, Diagnostic> {
        let not_installed = || finding("install.not-installed", "", format!("{} is not installed", plugin.0), 11);
        let (_root, registry) = self.existing_registry()?.ok_or_else(not_installed)?;
        let mut transaction = registry.transaction().map_err(host_diagnostic)?;
        let mut row = transaction.record(&plugin.0).cloned().ok_or_else(not_installed)?;
        let candidate = row["update"]["candidate"].as_str().map(str::to_owned)
            .ok_or_else(|| finding("install.no-update", "", format!("{} has no staged update", plugin.0), 11))?;
        let active = row["active_version"].as_str().unwrap_or_default().to_owned();
        let outcome = if first == FirstActivate::Ok {
            row["previous_version"] = serde_json::json!(active);
            row["active_version"] = serde_json::json!(candidate);
            UpdateOutcome::Committed { version: candidate, deactivation_reason: "upgrade".to_owned() }
        } else {
            if let Some(skip) = row["skip"].as_array_mut() {
                if !skip.iter().any(|version| version == candidate.as_str()) { skip.push(serde_json::json!(candidate)); }
            }
            UpdateOutcome::RolledBack {
                restored_version: active,
                skipped_version: candidate,
                log_code: "update.rolled-back".to_owned(),
                counted_in_failure_window: false,
            }
        };
        row["update"] = serde_json::Value::Null;
        transaction.put_complete_record(row).map_err(host_diagnostic)?;
        transaction.commit().map_err(host_diagnostic)?;
        Ok(outcome)
    }

    /// Returns to the previous version.
    pub fn rollback(&self, plugin: &PluginId) -> Result<InstallReport, Diagnostic> {
        let no_previous = || finding("install.no-previous", "", format!("{} has no earlier version on disk", plugin.0), 11);
        let (_root, registry) = self.existing_registry()?.ok_or_else(no_previous)?;
        let mut transaction = registry.transaction().map_err(host_diagnostic)?;
        let mut row = transaction.record(&plugin.0).cloned().ok_or_else(no_previous)?;
        let previous = row["previous_version"].as_str().map(str::to_owned).ok_or_else(no_previous)?;
        let current = row["active_version"].as_str().unwrap_or_default().to_owned();
        let previous_dir = self.store.checked_version_dir(plugin, &previous).map_err(host_diagnostic)?;
        if !previous_dir.is_dir() { return Err(no_previous()); }
        // PKG-292: the rolled-back version is skipped until a newer one exists.
        if let Some(skip) = row["skip"].as_array_mut() {
            if !skip.iter().any(|version| version == current.as_str()) { skip.push(serde_json::json!(current)); }
        }
        row["previous_version"] = serde_json::json!(current);
        row["active_version"] = serde_json::json!(previous);
        row["update"] = serde_json::Value::Null;
        transaction.put_complete_record(row).map_err(host_diagnostic)?;
        transaction.commit().map_err(host_diagnostic)?;
        Ok(InstallReport { plugin: plugin.0.clone(), version: previous, granted: Vec::new() })
    }

    /// Removes a plugin; `purge` also deletes its data, storage, settings and backups (PKG-294).
    pub fn uninstall(&self, plugin: &PluginId, purge: bool) -> Result<(), Diagnostic> {
        let not_installed = || finding("install.not-installed", "", format!("{} is not installed", plugin.0), 11);
        ilium_plugin_api::ids::parse_plugin_id(&plugin.0)
            .map_err(|_| finding("install.not-installed", "", "invalid plugin id", 11))?;
        let (root, mut registry) = self.existing_registry()?.ok_or_else(not_installed)?;
        let record = registry.list().map_err(host_diagnostic)?.into_iter().find(|row| row.id == plugin.0)
            .ok_or_else(not_installed)?;
        if record.source == "bundled" {
            return Err(finding("install.bundled", "", "a bundled plugin cannot be uninstalled; disable it instead", 11));
        }
        registry.remove(plugin).map_err(host_diagnostic)?;
        let io = |error: std::io::Error| finding("install.io", "", error.to_string(), 11);
        let remove_dir = |path: PathBuf| match std::fs::remove_dir_all(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(io(error)),
        };
        remove_dir(self.store.root.join(&plugin.0))?;
        if purge {
            for folder in ["data", "storage", "backups"] { remove_dir(root.join(folder).join(&plugin.0))?; }
            let prefix = format!("{}.", plugin.0);
            if let Ok(entries) = std::fs::read_dir(root.join("locks")) {
                for entry in entries.flatten() {
                    if entry.file_name().to_str().is_some_and(|name| name.starts_with(&prefix)) {
                        std::fs::remove_file(entry.path()).map_err(io)?;
                    }
                }
            }
        }
        Ok(())
    }
}

fn finding(code: &'static str, pointer: impl Into<String>, message: impl Into<String>, stage: u8) -> Diagnostic {
    let mut error = Diagnostic::error(Code::from_static(code), pointer, message);
    error.stage = Some(Stage(stage)); error
}
fn stage_report(mut diagnostics: Vec<Diagnostic>, fallback: u8) -> CheckReport {
    let stage = diagnostics.iter().filter(|d| d.severity == Severity::Error).filter_map(|d| d.stage.map(|stage| stage.0)).min().unwrap_or(fallback);
    diagnostics.retain(|d| d.stage.is_none_or(|s| s.0 == stage));
    for diagnostic in &mut diagnostics { diagnostic.stage = Some(Stage(stage)); }
    if diagnostics.len() > 100 {
        let omitted = diagnostics.len() - 100;
        diagnostics.truncate(100);
        let mut warning = finding("check.errors-truncated", "", format!("{omitted} additional errors in stage {stage}"), stage);
        warning.severity = Severity::Warning; diagnostics.push(warning);
    }
    CheckReport { diagnostics, last_stage: stage }
}
fn inventory(manifest: &Manifest, entries: &[crate::package::ArchiveEntry]) -> Vec<Diagnostic> {
    let mut errors = Vec::new();
    let mut previous: Option<&str> = None;
    let mut listed = std::collections::BTreeSet::new();
    for (index, file) in manifest.files.iter().enumerate() {
        let pointer = format!("/files/{index}");
        if previous.is_some_and(|path| path >= file.path.as_str()) {
            errors.push(finding("manifest.files-order", &pointer, "files must be unique and bytewise sorted", 5));
        }
        previous = Some(&file.path); listed.insert(file.path.as_str());
        if matches!(file.path.as_str(), "manifest.json" | "signature") {
            errors.push(finding("manifest.file-self", &pointer, "inventory cannot list manifest.json or signature", 5)); continue;
        }
        let Some(entry) = entries.iter().find(|entry| entry.path == file.path) else {
            errors.push(finding("manifest.file-missing", &pointer, format!("{} is absent from archive", file.path), 5)); continue;
        };
        if entry.bytes.len() as u64 != file.length {
            errors.push(finding("manifest.file-length", &pointer, format!("{} has a different byte length", file.path), 5));
        }
        if format!("{:x}", Sha256::digest(&entry.bytes)) != file.sha256 {
            errors.push(finding("manifest.file-hash", &pointer, format!("{} has a different SHA-256", file.path), 5));
        }
    }
    for entry in entries {
        if !matches!(entry.path.as_str(), "manifest.json" | "signature") && !listed.contains(entry.path.as_str()) {
            errors.push(finding("manifest.file-unlisted", "/files", format!("{} is not listed", entry.path), 5));
        }
    }
    // Required manifest references must also be present in the byte inventory.
    for declared in [&manifest.server, &manifest.client, &manifest.scene].into_iter().flatten() {
        if !listed.contains(declared.component.as_str()) {
            errors.push(finding("manifest.file-missing", "/files", format!("{} is not inventoried", declared.component), 5));
        }
    }
    errors
}
fn components(manifest: &Manifest, entries: &[crate::package::ArchiveEntry], context: &ValidationContext) -> Vec<Diagnostic> {
    let mut errors = Vec::new();
    for (side, declaration) in [(Side::Server,&manifest.server),(Side::Client,&manifest.client),(Side::Scene,&manifest.scene)] {
        let Some(declaration) = declaration else { continue; };
        let pointer = format!("/{}",side.as_str());
        let Some(entry) = entries.iter().find(|entry| entry.path == declaration.component) else {
            errors.push(finding("component.missing", &pointer, "declared component is missing", 6)); continue;
        };
        if !Parser::is_component(&entry.bytes) {
            errors.push(finding("package.component-invalid", &pointer, "expected a WebAssembly component", 6)); continue;
        }
        if let Err(error) = Validator::new().validate_all(&entry.bytes) {
            errors.push(finding("package.component-invalid", &pointer, error.to_string(), 6)); continue;
        }
        let mut summary = ComponentSummary::default();
        let mut nested = 0usize;
        for payload in Parser::new(0).parse_all(&entry.bytes) {
            match payload {
                Ok(Payload::ModuleSection { .. } | Payload::ComponentSection { .. }) => nested += 1,
                Ok(Payload::End(_)) if nested > 0 => nested -= 1,
                Ok(Payload::CustomSection(section)) if nested == 0 && section.name() == "ilium:api-version" => summary.api_sections.push(section.data().to_vec()),
                Ok(Payload::ComponentImportSection(section)) if nested == 0 => {
                    for import in section.into_iter().flatten() { summary.imports.push(import.name.name.into()); }
                }
                Ok(Payload::ComponentExportSection(section)) if nested == 0 => {
                    for export in section.into_iter().flatten() {
                        // API summary uses short interface ids. Preserve unknown names so
                        // extra exports fail the exact-set check instead of disappearing.
                        let name = export.name.name.split('@').next().unwrap_or(export.name.name);
                        summary.exports.push(name.strip_prefix("ilium:exports/").unwrap_or(name).into());
                    }
                }
                _ => {}
            }
        }
        let findings = manifest::check_component(manifest, side, &summary, context);
        if !findings.is_empty() { errors.extend(findings); continue; }
        if entry.bytes.len() > 8 * 1024 * 1024 {
            let mut size = finding("component.size-warning", &pointer, format!("component is {} bytes",entry.bytes.len()), 6);
            if entry.bytes.len() <= 32 * 1024 * 1024 { size.severity = Severity::Warning; }
            errors.push(size);
        }
    }
    errors
}

fn host_diagnostic(error: HostError) -> Diagnostic {
    match error { HostError::Diagnostic(error) => error,
        other => finding("install.io","",other.to_string(),11) }
}
fn store_diagnostic(error: crate::stores::StoreError) -> Diagnostic {
    finding("install.grants-store","",format!("{error:?}"),11)
}

/// Stage only verified bytes under a newly created private directory. The caller
/// holds installed.lock. Existing version directories are immutable and never
/// overwritten. A failure removes only the directory this operation created.
fn stage_package(root: &Path, destination: &Path, package: &[u8], entries: &[crate::package::ArchiveEntry], manifest_hash: &str) -> Result<PathBuf, Diagnostic> {
    use ilium_platform::secure_fs;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64,Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let io = |error: std::io::Error| finding("install.io","",error.to_string(),11);
    let root_directory = secure_fs::NoFollowDirectory::open_root(root).map_err(io)?;
    root_directory.create_directory_if_missing(std::ffi::OsStr::new("staging")).map_err(io)?;
    let staging_root = root_directory.open_directory(std::ffi::OsStr::new("staging")).map_err(io)?;
    let staging_name = format!("install-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed));
    if !staging_root.create_directory_if_missing(std::ffi::OsStr::new(&staging_name)).map_err(io)? {
        return Err(finding("install.staging-conflict","","staging name already exists; no existing directory was changed",11));
    }
    let staging = root.join("staging").join(&staging_name);
    let mut published = false;
    let result = (|| -> Result<(),Diagnostic> {
        let directory = staging_root.open_directory(std::ffi::OsStr::new(&staging_name)).map_err(io)?;
        for (path,bytes) in entries.iter().map(|entry| (entry.path.as_str(),entry.bytes.as_slice()))
            .chain([("package.iliumpkg",package),("manifest.sha256",manifest_hash.as_bytes())]) {
            let mut parent = secure_fs::NoFollowDirectory::open_root(&staging).map_err(io)?;
            let mut parts = path.split('/').peekable();
            while let Some(part) = parts.next() {
                if part.is_empty() || matches!(part,"."|"..") { return Err(finding("package.path-invalid","",path,1)); }
                if parts.peek().is_some() {
                    parent.create_directory_if_missing(std::ffi::OsStr::new(part)).map_err(io)?;
                    parent.sync_all().map_err(io)?;
                    parent = parent.open_directory(std::ffi::OsStr::new(part)).map_err(io)?;
                } else {
                    let mut file = parent.create_regular(std::ffi::OsStr::new(part)).map_err(io)?;
                    file.write_all(bytes).map_err(io)?;
                    let mut permissions = file.metadata().map_err(io)?.permissions();
                    permissions.set_readonly(true); file.set_permissions(permissions).map_err(io)?;
                    file.sync_all().map_err(io)?;
                    parent.sync_all().map_err(io)?;
                }
            }
        }
        directory.sync_all().map_err(io)?;
        let parent = destination.parent().ok_or_else(|| finding("install.io","","version directory has no parent",11))?;
        let plugin_name = parent.file_name().ok_or_else(|| finding("install.io","","plugin directory has no name",11))?;
        root_directory.create_directory_if_missing(std::ffi::OsStr::new("store")).map_err(io)?;
        let store_directory = root_directory.open_directory(std::ffi::OsStr::new("store")).map_err(io)?;
        store_directory.create_directory_if_missing(plugin_name).map_err(io)?;
        let plugin_directory = store_directory.open_directory(plugin_name).map_err(io)?;
        plugin_directory.sync_all().map_err(io)?;
        match std::fs::symlink_metadata(destination) {
            Ok(_) => return Err(finding("install.version-exists","","immutable version directory already exists",9)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            Err(error) => return Err(io(error)),
        }
        std::fs::rename(&staging,destination).map_err(io)?;
        published = true;
        secure_fs::sync_parent_directory(destination).map_err(io)?;
        Ok(())
    })();
    if let Err(error) = result {
        if let Err(cleanup) = std::fs::remove_dir_all(if published { destination } else { &staging }) {
            if cleanup.kind() != std::io::ErrorKind::NotFound {
                return Err(finding("install.rollback-failed","",format!("{error}; staging cleanup: {cleanup}"),11));
            }
        }
        return Err(error);
    }
    Ok(destination.to_owned())
}

/// Refuse symlinks and dot traversal in every existing ancestor before the
/// transaction creates a lock or any package data. New ancestors are private.
fn prepare_install_root(path: &Path) -> Result<(), Diagnostic> {
    use ilium_platform::secure_fs::NoFollowDirectory;
    let io = |error: std::io::Error| finding("install.io","",error.to_string(),11);
    let filesystem_root = path.ancestors().last().ok_or_else(|| finding("install.context-required","","missing absolute root",8))?;
    let mut directory = NoFollowDirectory::open_root(filesystem_root).map_err(io)?;
    for component in path.components() {
        match component {
            std::path::Component::Normal(name) => {
                directory.create_directory_if_missing(name).map_err(io)?;
                directory = directory.open_directory(name).map_err(io)?;
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {},
            _ => return Err(finding("install.context-required","","install root cannot traverse dot components",8)),
        }
    }
    directory.sync_all().map_err(io)
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
