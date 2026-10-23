//! Local file admission with explicit user consent and durable host audit effects.
//! This service never connects to a session or treats a command flag as approval.
use std::{io::Read, path::{Path, PathBuf}, sync::{Arc, Mutex}, time::UNIX_EPOCH};

use ilium_plugin_api::{ApiVersion, Code, Tier, manifest::{CheckMode, Diagnostic, ValidationContext}};
use serde_json::json;
use sha2::{Digest, Sha256};

use super::{CliEnv, CliOutput, install_command::{self, InstallCommandBackend}, offline};
use crate::{audit::{AuditContext, AuditLog, AuditRecord, AuditRetention}, install::{Consent, InstallEffects, InstallGrantDecision, InstallProvenance, Installer, TrustedInstallContext}, policy::{PolicyEngine, PolicyInput, PolicySourceKind}, registry::{InstalledRegistry, PackageStore}, stores::GrantStore, trust::{TrustDocument, TrustRepository}};

fn error(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(Code::from_static(code), "", message)
}
fn io(error: impl std::fmt::Display) -> Diagnostic { self::error("install.io", error.to_string()) }

/// Build a local service from actual CLI roots and caller-owned interaction.
/// `audit_context` identifies the real CLI invocation/session; it is never guessed.
/// System authority requires an ownership-verified loader, absent from CliEnv.
/// Existing high-risk grant files without verified audit evidence fail closed.
pub fn factory(env: &CliEnv, consent: Arc<dyn Consent + Send + Sync>, audit_context: AuditContext)
    -> Result<Arc<dyn InstallCommandBackend>, Diagnostic>
{
    for root in [&env.data_dir, &env.state_dir, &env.config_dir] {
        if !root.is_absolute() || root.components().any(|part| matches!(part, std::path::Component::ParentDir | std::path::Component::CurDir)) {
            return Err(error("install.context-required", "Absolute CLI roots without dot traversal are required"));
        }
    }
    if env.system_dir.is_some() || env.policy_file.is_some() {
        return Err(error("install.context-required", "System trust and policy require ownership-verified platform inputs"));
    }
    if audit_context.actor.kind != "cli" {
        return Err(error("audit.context-required", "Local install requires actual CLI actor provenance"));
    }
    let version = env.host_release.strip_prefix('v').unwrap_or(&env.host_release);
    let host_version = ApiVersion::parse(&format!("ilium:plugin@{version}"))?;
    let trust_path = env.env.get("ILIUM_TRUST_STORE").map(PathBuf::from)
        .unwrap_or_else(|| env.state_dir.join("trust.json"));
    // Constructor validates the absolute override before any mutation.
    TrustRepository::new(trust_path.clone(), TrustDocument::default())?;
    Ok(Arc::new(LocalInstall {
        env: env.clone(), consent, audit_context, host_version, trust_path,
        gate: Mutex::new(()),
    }))
}

struct LocalInstall {
    env: CliEnv,
    consent: Arc<dyn Consent + Send + Sync>,
    audit_context: AuditContext,
    host_version: ApiVersion,
    trust_path: PathBuf,
    gate: Mutex<()>,
}
impl InstallCommandBackend for LocalInstall {
    fn install(&self, path: &Path) -> CliOutput {
        let result = self.perform(path);
        match result {
            Ok(report) => {
                let mut records = vec![json!({"type":"plugin", "id":report.installed.plugin,
                    "version":report.installed.version, "tier":format!("{:?}", report.tier),
                    "state":"enabled", "dev":false})];
                records.extend(report.warnings.into_iter().map(|warning| json!({"type":"warning", "code":warning.code.as_str(), "message":warning.message})));
                offline::finish(records, 0)
            }
            Err(output) => output,
        }
    }
}
impl LocalInstall {
    fn perform(&self, path: &Path) -> Result<crate::install::TrustedInstallReport, CliOutput> {
        let fail = |diagnostic: Diagnostic| offline::failure(diagnostic.code.as_str(), diagnostic.message.clone(), install_command::install_exit_code(diagnostic.code.as_str()));
        let _gate = self.gate.lock().map_err(|_| fail(error("install.locked", "Local install service lock is poisoned")))?;
        let bytes = install_command::read_install_file(path)?;
        let absolute = std::fs::canonicalize(path).map_err(|e| fail(io(e)))?;
        let source = absolute.to_str().ok_or_else(|| fail(error("install.source-invalid", "Package path must be UTF-8")))?;
        let package = crate::package::read_package(&bytes).map_err(|diagnostics| {
            diagnostics.into_iter().next().map(&fail).unwrap_or_else(|| fail(error("package.zip-invalid", "Package parsing failed")))
        })?;
        let trust = TrustRepository::new(self.trust_path.clone(), TrustDocument::default()).and_then(|repository| repository.snapshot_at(self.env.now)).map_err(&fail)?;
        let policy = PolicyEngine::from_inputs(PolicyInput::Missing, read_policy(&self.env.config_dir.join("policy.toml")));
        if !policy.is_valid() { return Err(fail(error("policy.invalid", policy.errors().join("; ")))); }
        let plugin_root = self.env.data_dir.join("plugins");
        private_root(&plugin_root).map_err(&fail)?;
        private_root(&self.env.state_dir).map_err(&fail)?;
        let registry = InstalledRegistry::open(&plugin_root.join("installed.json")).map_err(|e| fail(io(e)))?;
        // Registry projections omit provider/settings metadata: dependency-bearing existing
        // installations need a richer snapshot, rather than fabricated empty metadata.
        if !registry.list().map_err(|e| fail(io(e)))?.is_empty() {
            return Err(fail(error("install.context-required", "Existing installations require a complete dependency and grant-evidence snapshot")));
        }
        let mut grants = GrantStore::open(&self.env.state_dir).map_err(|e| fail(io(e)))?;
        let audit = AuditLog::open_with_context(self.env.state_dir.join("audit/audit.jsonl"), Some(self.audit_context.clone()), AuditRetention::default()).map_err(|e| fail(io(e)))?;
        let raw_manifest = package.entries.iter().find(|entry| entry.path == "manifest.json")
            .ok_or_else(|| fail(error("package.manifest-missing", "Exact manifest bytes are required")))?;
        let effects = Effects {
            audit: Mutex::new(audit),
            plugin: package.manifest.id.clone(),
            tier: Mutex::new(None),
            notification_required: self.env.session.is_some(),
            version: package.manifest.version.to_string(),
            manifest_hash: format!("{:x}", Sha256::digest(&raw_manifest.bytes)),
            package_hash: format!("{:x}", Sha256::digest(&bytes)),
            now: u64::try_from(self.env.now.duration_since(UNIX_EPOCH).map_err(|e| fail(io(e)))?.as_millis()).map_err(|e| fail(io(e)))?,
        };
        let validation = ValidationContext { tier: Tier::T2, mode: CheckMode::Install, platform: self.env.platform,
            installed: Vec::new(), core_plugins: Vec::new(), unstable_features: Vec::new() };
        let mut context = TrustedInstallContext { trust: &trust, policy: &policy, validation,
            host_version: self.host_version, provenance: InstallProvenance { source: source.into(), kind: PolicySourceKind::File, channel: "file".into() },
            grant_store: &mut grants, effects: &effects };
        Installer::new(PackageStore { root: plugin_root.join("store") })
            .install_with_context(&bytes, &mut context, self.consent.as_ref()).map_err(fail)
    }
}

fn read_policy(path: &Path) -> PolicyInput {
    let mut file = match ilium_platform::secure_fs::open_regular_file(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return PolicyInput::Missing,
        Err(e) => return PolicyInput::Unreadable(e.to_string()),
    };
    let mut text = String::new();
    match file.by_ref().take(1024 * 1024 + 1).read_to_string(&mut text) {
        Ok(_) if text.len() <= 1024 * 1024 => PolicyInput::Document { text, ownership: None },
        Ok(_) => PolicyInput::Unreadable("User policy exceeds 1 MiB".into()),
        Err(e) => PolicyInput::Unreadable(e.to_string()),
    }
}
fn private_root(path: &Path) -> Result<(), Diagnostic> {
    use ilium_platform::secure_fs::NoFollowDirectory;
    let root = path.ancestors().last().ok_or_else(|| error("install.context-required", "Missing filesystem root"))?;
    let mut directory = NoFollowDirectory::open_root(root).map_err(io)?;
    for component in path.components() {
        if let std::path::Component::Normal(name) = component {
            directory.create_directory_if_missing(name).map_err(io)?;
            directory = directory.open_directory(name).map_err(io)?;
        }
    }
    directory.sync_all().map_err(io)
}
struct Effects {
    audit: Mutex<AuditLog>, plugin: String, tier: Mutex<Option<Tier>>, notification_required: bool, version: String, manifest_hash: String, package_hash: String, now: u64,
}
impl InstallEffects for Effects {
    fn record_approval(&self, decision: &InstallGrantDecision) -> Result<(), Diagnostic> {
        if decision.manifest_sha256 != self.manifest_hash {
            return Err(error("audit.context-required", "Approval does not match exact package manifest"));
        }
        *self.tier.lock().map_err(|_| error("audit.io", "Audit tier lock is poisoned"))? = Some(decision.tier);
        let mut audit = self.audit.lock().map_err(|_| error("audit.io", "Audit lock is poisoned"))?;
        let detail = json!({"result":"ok", "version":self.version,
            "tier":format!("{:?}", decision.tier).to_lowercase(),
            "publisher_fingerprint":decision.key_id, "package_sha256":self.package_hash,
            "target":decision.manifest_sha256,
            "arguments":[serde_json::to_string(&json!({"epoch":decision.epoch,
                "grants_sha256":decision.grants_sha256, "grants":decision.grants})).map_err(io)?]});
        // Audit append may queue after an I/O failure; queued is not durable consent.
        audit.append_at(&AuditRecord { at_unix_millis:self.now, plugin:decision.plugin.clone(),
            action:"consent-given".into(), detail_json:detail.to_string() }, self.now).map_err(io)?;
        for (side, grants) in [("server", &decision.grants.server), ("client", &decision.grants.client), ("scene", &decision.grants.scene)] {
            for grant in grants {
                let mut grant_detail = detail.clone();
                grant_detail["grant"] = json!(grant.as_str());
                grant_detail["target"] = json!(side);
                audit.append_at(&AuditRecord { at_unix_millis:self.now, plugin:decision.plugin.clone(),
                    action:"grant-given".into(), detail_json:grant_detail.to_string() }, self.now).map_err(io)?;
            }
        }
        let status = audit.status();
        if status.queued != 0 || status.write_failed || status.durability_uncertain || status.dropped != 0 {
            return Err(error("audit.io", "Approval audit record is not durably stored"));
        }
        Ok(())
    }
    fn plugins_changed(&self) -> Result<(), Diagnostic> {
        let tier = self.tier.lock().map_err(|_| error("audit.io", "Audit tier lock is poisoned"))?
            .ok_or_else(|| error("audit.context-required", "Committed install has no approval audit context"))?;
        let mut audit = self.audit.lock().map_err(|_| error("audit.io", "Audit lock is poisoned"))?;
        audit.append_at(&AuditRecord { at_unix_millis:self.now, plugin:self.plugin.clone(),
            action:"install".into(), detail_json:json!({"result":"ok", "version":self.version,
                "tier":format!("{tier:?}").to_lowercase(), "package_sha256":self.package_hash}).to_string() },self.now).map_err(io)?;
        let status = audit.status();
        if status.queued != 0 || status.write_failed || status.durability_uncertain {
            return Err(error("audit.io", "Install committed but completion audit is not durable"));
        }
        if self.notification_required {
            return Err(error("install.notification-context-required", "Install committed; connected session notification requires a runtime adapter"));
        }
        Ok(())
    }
}
