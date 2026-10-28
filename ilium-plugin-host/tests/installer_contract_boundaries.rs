//! Actual-byte admission ordering and pure limit resolution.
use ilium_plugin_api::{limits::Limits, manifest::CheckMode, Tier};
use ilium_plugin_host::{
    install::{install_with_limits, Installer},
    package::{read_package, write_package},
    registry::PackageStore,
};

fn vector() -> Vec<u8> {
    let digits: String = include_str!("package/golden/minimal_unsigned.hex")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    (0..digits.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&digits[i..i + 2], 16).unwrap())
        .collect()
}
fn installer() -> Installer {
    Installer::new(PackageStore {
        root: "/not-written-by-offline-check".into(),
    })
}
fn rewrite(
    mut files: Vec<(String, Vec<u8>)>,
    manifest: impl FnOnce(&mut serde_json::Value),
) -> Vec<u8> {
    let raw = files
        .iter_mut()
        .find(|(name, _)| name == "manifest.json")
        .unwrap();
    let mut document: serde_json::Value = serde_json::from_slice(&raw.1).unwrap();
    manifest(&mut document);
    raw.1 = serde_json::to_vec(&document).unwrap();
    write_package(&files).unwrap()
}
fn files() -> Vec<(String, Vec<u8>)> {
    read_package(&vector())
        .unwrap()
        .entries
        .into_iter()
        .map(|entry| (entry.path, entry.bytes))
        .collect()
}

#[test]
fn archive_errors_precede_manifest_checks_and_never_instantiate() {
    let installer = installer();
    let report = installer.check(b"not a zip", CheckMode::Check);
    assert_eq!(report.last_stage, 1);
    assert!(!report.diagnostics.is_empty());
    assert_eq!(installer.instantiated_count(), 0);
}
#[test]
fn malformed_manifest_stops_at_parse_stage() {
    let package = write_package(&[("manifest.json".into(), b"{".to_vec())]).unwrap();
    let report = installer().check(&package, CheckMode::Check);
    assert_eq!(report.last_stage, 2);
    assert_eq!(report.diagnostics[0].code.as_str(), "manifest.json-invalid");
}
#[test]
fn schema_errors_precede_corrupt_signature_documents() {
    let mut entries = files();
    entries.push(("signature".into(), b"invalid".to_vec()));
    let package = rewrite(entries, |manifest| {
        manifest["not_a_manifest_key"] = serde_json::json!(true);
    });
    let report = installer().check(&package, CheckMode::Check);
    assert_eq!(report.last_stage, 3);
    assert!(report
        .diagnostics
        .iter()
        .any(|d| d.code.as_str() == "manifest.unknown-key"));
    assert!(!report
        .diagnostics
        .iter()
        .any(|d| d.code.as_str().contains("signature")));
}
#[test]
fn inventory_hash_errors_precede_components_and_signatures() {
    let mut entries = files();
    entries.push(("signature".into(), b"invalid".to_vec()));
    let package = rewrite(entries, |manifest| {
        manifest["files"][0]["sha256"] = serde_json::json!("0".repeat(64));
    });
    let report = installer().check(&package, CheckMode::Check);
    assert_eq!(report.last_stage, 5);
    assert!(report
        .diagnostics
        .iter()
        .any(|d| d.code.as_str() == "manifest.file-hash"));
    assert!(!report
        .diagnostics
        .iter()
        .any(|d| d.code.as_str().contains("signature")));
}
#[test]
fn limits_clamp_is_pure_and_reports_each_changed_key_once() {
    let requested = Limits(std::collections::BTreeMap::from([(
        "http_streams".into(),
        99,
    )]));
    let result = install_with_limits(
        std::path::Path::new("/not-a-package-install"),
        Tier::T2,
        &requested,
    );
    assert!(result.installed);
    assert_eq!(result.log.len(), 1);
    assert!(result.log[0].contains("http_streams"));
    assert!(result.log[0].contains("effective=4"));
}

#[test]
fn rejected_archive_never_opens_consent_or_reports_installation() {
    struct UnexpectedConsent;
    impl ilium_plugin_host::install::Consent for UnexpectedConsent {
        fn decide(
            &self,
            _: &ilium_plugin_host::install::ConsentRequest,
        ) -> ilium_plugin_host::install::ConsentDecision {
            panic!("consent must follow every admission stage");
        }
    }
    let installer = installer();
    let error = installer
        .install(b"invalid archive", &UnexpectedConsent)
        .unwrap_err();
    assert_eq!(error.stage.unwrap().0, 1);
    assert_eq!(installer.instantiated_count(), 0);
}

// Real import-free component with callable lifecycle and handler exports. This
// fixture exercises the installer's structural export check, not full WIT linkage.
const POLICY_COMPONENT: &str = r#"(component

  (core module $activation_m
    (memory (export "memory") 1)
    (global $heap (mut i32) (i32.const 1024))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      global.get $heap
      global.get $heap local.get 3 i32.add i32.const 7 i32.add i32.const -8 i32.and global.set $heap)
    (func (export "activate") (param i32 i32 i32 i32 i64 i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)
      local.get 4 i64.const 1 i64.ne if unreachable end
      i32.const 0))
  (core instance $activation_m (instantiate $activation_m))
  (type $activation (record
    (field "plugin" string) (field "api" string) (field "instance" u64)
    (field "side" string) (field "settings-json" string)
    (field "granted" (list string)) (field "restored" (option (list u8)))))
  (type $code (enum "not-permitted" "not-found" "invalid-argument" "conflict" "quota-exceeded" "rate-limited" "busy" "timed-out" "cancelled" "unsupported" "pane-not-ready" "revoked" "io" "network" "internal"))
  (type $error (record (field "code" $code) (field "message" string) (field "retry-after-ms" (option u32))))
  (func $activate (param "context" $activation) (result (result (error $error)))
    (canon lift (core func $activation_m "activate") (memory $activation_m "memory") (realloc (func $activation_m "realloc"))))
  (instance $lifecycle
    (export "activation" (type $activation)) (export "error-code" (type $code)) (export "plugin-error" (type $error))
    (export "activate" (func $activate)))
  (export "ilium:exports/lifecycle@0.1.0" (instance $lifecycle))
  (core module $handler_m
    (memory (export "memory") 1)
    (global $heap (mut i32) (i32.const 1024))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      global.get $heap
      global.get $heap local.get 3 i32.add i32.const 7 i32.add i32.const -8 i32.and global.set $heap)
    (data (i32.const 0) "\20\00\00\00\01\00\00\00")
    (data (i32.const 40) "\0d\00\00\00\00\00\00\00\2a\00\00\00\00\00\00\00")
    (func (export "events") (param i32 i32) (result i32)
      local.get 1 i32.const 1 i32.ne if unreachable end
      i32.const 0))
  (core instance $handler_m (instantiate $handler_m))
  (type $event (variant (case "node-removed" u64)))
  (type $intent (variant (case "close-node" u64)))
  (type $submission (record (field "intent" $intent) (field "correlation" u64) (field "dedup-key" (option string))))
  (func $events (param "batch" (list $event)) (result (list $submission))
    (canon lift (core func $handler_m "events") (memory $handler_m "memory") (realloc (func $handler_m "realloc"))))
  (instance $handler (export "event" (type $event)) (export "intent" (type $intent)) (export "submission" (type $submission)) (export "on-events" (func $events)))
  (export "ilium:exports/handler@0.1.0" (instance $handler))
)"#;
fn policy_files() -> Vec<(String, Vec<u8>)> {
    use sha2::{Digest, Sha256};
    let mut entries = files();
    let component = entries
        .iter_mut()
        .find(|(path, _)| path == "server.wasm")
        .unwrap();
    let api_section = component.1[8..].to_vec();
    component.1 = wat::parse_str(POLICY_COMPONENT).unwrap();
    // Preserve the actual API-version custom section from the normative vector.
    component.1.extend_from_slice(&api_section);
    let length = component.1.len();
    let digest = format!("{:x}", Sha256::digest(&component.1));
    let manifest = entries
        .iter_mut()
        .find(|(path, _)| path == "manifest.json")
        .unwrap();
    let mut document: serde_json::Value = serde_json::from_slice(&manifest.1).unwrap();
    let inventory = document["files"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row["path"] == "server.wasm")
        .unwrap();
    inventory["length"] = serde_json::json!(length);
    inventory["sha256"] = serde_json::json!(digest);
    manifest.1 = serde_json::to_vec(&document).unwrap();
    entries
}

fn combination_policy() -> ilium_plugin_host::policy::PolicyEngine {
    use ilium_plugin_host::policy::{PolicyEngine, PolicyInput, PolicyOwnership};
    PolicyEngine::from_inputs(
        PolicyInput::Document {
            text: "version=1\n[grants]\ndeny_combinations=true".into(),
            ownership: Some(PolicyOwnership {
                administrator_owned: true,
                non_administrator_writable: false,
            }),
        },
        PolicyInput::Missing,
    )
}
fn admitted_context() -> ilium_plugin_host::policy::InstallPolicyContext<'static> {
    use ilium_plugin_host::policy::{InstallPolicyContext, PolicySourceKind};
    // Explicit caller facts for pure policy testing; this is not a trust-store grant.
    InstallPolicyContext {
        plugin: "example.minimal",
        tier: Tier::T3,
        publisher_key: None,
        source: "https://registry.example/plugins",
        source_kind: PolicySourceKind::Registry,
        channel: "stable",
        development: false,
        replaces: false,
    }
}
fn capability(text: &str) -> ilium_plugin_api::grants::Grant {
    ilium_plugin_api::grants::Grant::parse(text).unwrap()
}

#[test]
fn package_policy_reviews_real_requested_grants_with_retained_same_side_grants() {
    use ilium_plugin_api::{manifest::Capabilities, Side};
    use ilium_plugin_host::policy::PolicyVerdict;
    let bytes = rewrite(policy_files(), |manifest| {
        manifest["capabilities"] = serde_json::json!({"server":["http:example.com","sound.play"]});
    });
    let retained = Capabilities {
        server: vec![capability("tree.read")],
        ..Capabilities::default()
    };
    let installer = installer();
    let result = installer
        .review_package_policy(
            &bytes,
            &combination_policy(),
            &admitted_context(),
            &retained,
        )
        .unwrap();
    assert_eq!(result.installation, PolicyVerdict::Allow);
    let server = result
        .sides
        .iter()
        .find(|side| side.side == Side::Server)
        .unwrap();
    assert_eq!(
        server.requested,
        vec![
            (
                capability("http:example.com"),
                PolicyVerdict::Deny("policy.grant-forbidden".into())
            ),
            (capability("sound.play"), PolicyVerdict::Allow),
        ]
    );
    assert_eq!(
        server.retained,
        vec![(
            capability("tree.read"),
            PolicyVerdict::Deny("policy.grant-forbidden".into())
        )]
    );
    struct NoConsent;
    impl ilium_plugin_host::install::Consent for NoConsent {
        fn decide(
            &self,
            _: &ilium_plugin_host::install::ConsentRequest,
        ) -> ilium_plugin_host::install::ConsentDecision {
            panic!("policy review does not establish trusted durable admission");
        }
    }
    let not_installed = installer.install(&bytes, &NoConsent).unwrap_err();
    assert_eq!(not_installed.code.as_str(), "install.context-required");
    assert_eq!(installer.instantiated_count(), 0);
}

#[test]
fn installer_policy_never_combines_retained_grants_from_different_sides() {
    use ilium_plugin_api::{manifest::Capabilities, Side};
    use ilium_plugin_host::policy::PolicyVerdict;
    let bytes = rewrite(policy_files(), |manifest| {
        manifest["capabilities"] = serde_json::json!({"server":["tree.read"]});
    });
    let retained = Capabilities {
        client: vec![capability("http:example.com")],
        ..Capabilities::default()
    };
    let result = installer()
        .review_package_policy(
            &bytes,
            &combination_policy(),
            &admitted_context(),
            &retained,
        )
        .unwrap();
    assert_eq!(result.installation, PolicyVerdict::Allow);
    for side in result.sides {
        assert!(side
            .requested
            .iter()
            .chain(&side.retained)
            .all(|(_, verdict)| *verdict == PolicyVerdict::Allow));
        if side.side == Side::Server {
            assert_eq!(side.requested.len(), 1);
        }
    }
}

#[test]
fn policy_context_cannot_be_applied_to_a_different_checked_plugin() {
    use ilium_plugin_api::manifest::Capabilities;
    let mut context = admitted_context();
    context.plugin = "other.plugin";
    let error = installer()
        .review_package_policy(
            &write_package(&policy_files()).unwrap(),
            &combination_policy(),
            &context,
            &Capabilities::default(),
        )
        .unwrap_err();
    assert_eq!(error.code.as_str(), "install.context-required");
}

#[test]
fn corrupt_archive_cannot_reach_explicit_policy_review() {
    use ilium_plugin_api::manifest::Capabilities;
    let error = installer()
        .review_package_policy(
            b"invalid archive",
            &combination_policy(),
            &admitted_context(),
            &Capabilities::default(),
        )
        .unwrap_err();
    assert_eq!(error.stage.unwrap().0, 1);
}

#[derive(Default)]
struct InstallEffects {
    decisions: std::sync::Mutex<Vec<ilium_plugin_host::install::InstallGrantDecision>>,
    fail_audit: bool,
    fail_notify: bool,
}
impl ilium_plugin_host::install::InstallEffects for InstallEffects {
    fn record_approval(
        &self,
        decision: &ilium_plugin_host::install::InstallGrantDecision,
    ) -> Result<(), ilium_plugin_api::Diagnostic> {
        if self.fail_audit {
            return Err(ilium_plugin_api::Diagnostic::error(
                ilium_plugin_api::Code::from_static("audit.io"),
                "",
                "injected audit failure",
            ));
        }
        self.decisions.lock().unwrap().push(decision.clone());
        Ok(())
    }
    fn plugins_changed(&self) -> Result<(), ilium_plugin_api::Diagnostic> {
        if self.fail_notify {
            return Err(ilium_plugin_api::Diagnostic::error(
                ilium_plugin_api::Code::from_static("notify.io"),
                "",
                "injected notification failure",
            ));
        }
        Ok(())
    }
}
struct ApproveRequested;
impl ilium_plugin_host::install::Consent for ApproveRequested {
    fn decide(
        &self,
        request: &ilium_plugin_host::install::ConsentRequest,
    ) -> ilium_plugin_host::install::ConsentDecision {
        ilium_plugin_host::install::ConsentDecision::Approve(request.requested.clone())
    }
}
fn trusted_context<'a>(
    trust: &'a ilium_plugin_host::trust::TrustSnapshot,
    policy: &'a ilium_plugin_host::policy::PolicyEngine,
    grants: &'a mut ilium_plugin_host::stores::GrantStore,
    effects: &'a InstallEffects,
) -> ilium_plugin_host::install::TrustedInstallContext<'a> {
    ilium_plugin_host::install::TrustedInstallContext {
        trust,
        policy,
        validation: ilium_plugin_api::manifest::ValidationContext {
            tier: Tier::T3,
            mode: CheckMode::Install,
            platform: ilium_plugin_api::Platform::Linux,
            installed: vec![],
            core_plugins: vec![],
            unstable_features: vec![],
        },
        host_version: ilium_plugin_api::ApiVersion {
            major: 0,
            minor: 1,
            patch: 0,
        },
        provenance: ilium_plugin_host::install::InstallProvenance {
            source: "/explicit/test-package.iliumpkg".into(),
            kind: ilium_plugin_host::policy::PolicySourceKind::File,
            channel: "file".into(),
        },
        grant_store: grants,
        effects,
    }
}
fn durable_installer(root: &std::path::Path, allow_unsigned: bool) -> Installer {
    Installer::with_options(
        PackageStore {
            root: root.join("plugins/store"),
        },
        root.join("state"),
        ilium_plugin_host::install::InstallOptions { allow_unsigned },
    )
}
fn installable_bytes() -> Vec<u8> {
    rewrite(policy_files(), |manifest| {
        manifest["capabilities"] = serde_json::json!({"server":["tree.read"]});
    })
}

#[test]
fn explicit_admission_persists_real_immutable_bytes_registry_and_bound_grants() {
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    let installer = durable_installer(root.path(), true);
    let mut grants =
        ilium_plugin_host::stores::GrantStore::open(&root.path().join("state")).unwrap();
    let trust = ilium_plugin_host::trust::TrustSnapshot::default();
    let policy = ilium_plugin_host::policy::PolicyEngine::new();
    let effects = InstallEffects::default();
    let bytes = installable_bytes();
    let report = installer
        .install_with_context(
            &bytes,
            &mut trusted_context(&trust, &policy, &mut grants, &effects),
            &ApproveRequested,
        )
        .unwrap();
    assert_eq!(report.tier, Tier::T3);
    let version = root.path().join("plugins/store/example.minimal/0.1.0");
    assert_eq!(
        std::fs::read(version.join("package.iliumpkg")).unwrap(),
        bytes
    );
    for entry in read_package(&bytes).unwrap().entries {
        assert_eq!(
            std::fs::read(version.join(&entry.path)).unwrap(),
            entry.bytes
        );
        assert!(std::fs::metadata(version.join(&entry.path))
            .unwrap()
            .permissions()
            .readonly());
    }
    let registry: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("plugins/installed.json")).unwrap())
            .unwrap();
    assert_eq!(
        registry["plugins"][0]["package_sha256"],
        format!("{:x}", Sha256::digest(&bytes))
    );
    let reloaded = ilium_plugin_host::stores::GrantStore::open(&root.path().join("state")).unwrap();
    assert_eq!(
        reloaded
            .load_for_side("example.minimal", "", "t3", ilium_plugin_api::Side::Server)
            .unwrap(),
        vec![capability("tree.read")]
    );
    assert_eq!(effects.decisions.lock().unwrap()[0].epoch, report.epoch);
    assert_eq!(installer.instantiated_count(), 0);
}

#[test]
fn failed_approval_audit_restores_exact_grants_and_leaves_no_install() {
    let root = tempfile::tempdir().unwrap();
    let installer = durable_installer(root.path(), true);
    let state = root.path().join("state");
    let mut grants = ilium_plugin_host::stores::GrantStore::open(&state).unwrap();
    let before = std::fs::read(grants.path()).unwrap();
    let trust = ilium_plugin_host::trust::TrustSnapshot::default();
    let policy = ilium_plugin_host::policy::PolicyEngine::new();
    let effects = InstallEffects {
        fail_audit: true,
        ..InstallEffects::default()
    };
    let error = installer
        .install_with_context(
            &installable_bytes(),
            &mut trusted_context(&trust, &policy, &mut grants, &effects),
            &ApproveRequested,
        )
        .unwrap_err();
    assert_eq!(error.code.as_str(), "audit.io");
    assert_eq!(std::fs::read(grants.path()).unwrap(), before);
    assert!(!root.path().join("plugins/installed.json").exists());
    assert!(!root
        .path()
        .join("plugins/store/example.minimal/0.1.0")
        .exists());
}

#[test]
fn unsigned_requires_explicit_option_and_denied_review_never_stages() {
    struct Deny;
    impl ilium_plugin_host::install::Consent for Deny {
        fn decide(
            &self,
            _: &ilium_plugin_host::install::ConsentRequest,
        ) -> ilium_plugin_host::install::ConsentDecision {
            ilium_plugin_host::install::ConsentDecision::Deny
        }
    }
    let root = tempfile::tempdir().unwrap();
    let mut grants =
        ilium_plugin_host::stores::GrantStore::open(&root.path().join("state")).unwrap();
    let trust = ilium_plugin_host::trust::TrustSnapshot::default();
    let policy = ilium_plugin_host::policy::PolicyEngine::new();
    let effects = InstallEffects::default();
    let bytes = installable_bytes();
    assert_eq!(
        durable_installer(root.path(), false)
            .install_with_context(
                &bytes,
                &mut trusted_context(&trust, &policy, &mut grants, &effects),
                &ApproveRequested
            )
            .unwrap_err()
            .stage
            .unwrap()
            .0,
        8
    );
    assert_eq!(
        durable_installer(root.path(), true)
            .install_with_context(
                &bytes,
                &mut trusted_context(&trust, &policy, &mut grants, &effects),
                &Deny
            )
            .unwrap_err()
            .code
            .as_str(),
        "install.declined"
    );
    assert!(!root.path().join("plugins/store").exists());
    assert!(effects.decisions.lock().unwrap().is_empty());
}

#[test]
fn failed_notification_reports_warning_after_durable_commit() {
    let root = tempfile::tempdir().unwrap();
    let installer = durable_installer(root.path(), true);
    let mut grants =
        ilium_plugin_host::stores::GrantStore::open(&root.path().join("state")).unwrap();
    let trust = ilium_plugin_host::trust::TrustSnapshot::default();
    let policy = ilium_plugin_host::policy::PolicyEngine::new();
    let effects = InstallEffects {
        fail_notify: true,
        ..InstallEffects::default()
    };
    let report = installer
        .install_with_context(
            &installable_bytes(),
            &mut trusted_context(&trust, &policy, &mut grants, &effects),
            &ApproveRequested,
        )
        .unwrap();
    assert_eq!(report.warnings[0].code.as_str(), "notify.io");
    assert!(root.path().join("plugins/installed.json").exists());
}

#[test]
fn signed_install_derives_tier_from_trust_and_revoked_key_is_not_unsigned_fallback() {
    use base64::Engine as _;
    use ilium_plugin_host::trust::{
        AddedBy, Revocation, RevocationReason, Status, TrustDocument, TrustEntry, TrustSnapshot,
    };
    use sha2::{Digest, Sha256};
    let now = "2026-10-22 12:00:00";
    let seed = [19; 32];
    let public = ed25519_dalek::SigningKey::from_bytes(&seed)
        .verifying_key()
        .to_bytes();
    let entry = TrustEntry {
        key_id: format!("{:x}", Sha256::digest(public)),
        publisher_id: "example".into(),
        publisher: "Example Labs".into(),
        public_key: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public),
        added: now.into(),
        added_by: AddedBy::User,
        status: Status::Trusted,
        revoked: None,
    };
    let trust = TrustSnapshot::resolve(
        &TrustDocument {
            keys: vec![entry.clone()],
            ..TrustDocument::default()
        },
        &TrustDocument::default(),
        now,
    )
    .unwrap();
    let signed = ilium_plugin_host::signing::sign(
        &installable_bytes(),
        &ilium_plugin_host::signing::SigningKey::from_seed(seed),
        ilium_plugin_host::signing::SignedAt(now.into()),
    )
    .unwrap();
    let policy = ilium_plugin_host::policy::PolicyEngine::new();
    let effects = InstallEffects::default();
    let root = tempfile::tempdir().unwrap();
    let mut grants =
        ilium_plugin_host::stores::GrantStore::open(&root.path().join("state")).unwrap();
    let report = durable_installer(root.path(), false)
        .install_with_context(
            &signed,
            &mut trusted_context(&trust, &policy, &mut grants, &effects),
            &ApproveRequested,
        )
        .unwrap();
    assert_eq!(report.tier, Tier::T2);
    let revoked = TrustSnapshot::resolve(
        &TrustDocument {
            keys: vec![TrustEntry {
                status: Status::Revoked,
                revoked: Some(Revocation {
                    reason: RevocationReason::Compromised,
                    at: now.into(),
                }),
                ..entry
            }],
            ..TrustDocument::default()
        },
        &TrustDocument::default(),
        now,
    )
    .unwrap();
    let other = tempfile::tempdir().unwrap();
    let mut grants =
        ilium_plugin_host::stores::GrantStore::open(&other.path().join("state")).unwrap();
    let error = durable_installer(other.path(), true)
        .install_with_context(
            &signed,
            &mut trusted_context(&revoked, &policy, &mut grants, &effects),
            &ApproveRequested,
        )
        .unwrap_err();
    assert_eq!(error.code.as_str(), "trust.key-revoked");
    assert_eq!(error.stage.unwrap().0, 8);
    assert!(!other.path().join("plugins/store").exists());
}
