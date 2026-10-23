use ilium_plugin_host::cli::install_command::{install_exit_code, read_install_file};

#[test]
fn distribution_failures_preserve_distinct_exit_classes() {
    for (code, expected) in [
        ("policy.install-denied", 7),
        ("package.blocked", 7),
        ("install.locked", 4),
        ("plugin.not-found", 5),
        ("registry.metadata-expired", 8),
        ("install.review-declined", 1),
        ("manifest.schema", 1),
        ("install.unsigned", 1),
    ] {
        assert_eq!(install_exit_code(code), expected, "{code}");
    }
}

#[test]
fn missing_file_is_usage_failure_with_canonical_final_records() {
    let output = read_install_file(std::path::Path::new(
        "/media/arthur/build/ilium-cli-install-nonexistent-parent/nonexistent.iliumpkg",
    )).unwrap_err();
    assert_eq!(output.exit_code, 2);
    assert_eq!(output.stdout_jsonl[0]["code"], "usage.file-not-found");
    let count = output.stdout_jsonl.len();
    assert_eq!(output.stdout_jsonl[count - 2]["type"], "summary");
    assert_eq!(output.stdout_jsonl[count - 1]["type"], "result");
    assert_eq!(output.stdout_jsonl[count - 1]["exit_code"], 2);
    assert!(output.stdout_raw.lines().all(|line| line.starts_with("{\"type\":")));
}

#[test]
fn regular_file_reader_preserves_exact_bytes_without_package_admission() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/cli_install_context_boundaries.rs");
    assert_eq!(read_install_file(&path).unwrap(), std::fs::read(path).unwrap());
}

#[test]
fn directory_is_not_accepted_as_a_package_file() {
    let output = read_install_file(std::path::Path::new(env!("CARGO_MANIFEST_DIR")))
        .unwrap_err();
    assert_eq!(output.exit_code, 2);
    assert_eq!(output.stdout_jsonl[0]["code"], "usage.file-not-found");
}

// Genuine component fixture shared in shape with installer admission tests.
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
    let component = entries.iter_mut().find(|(path, _)| path == "server.wasm").unwrap();
    let api_section = component.1[8..].to_vec();
    component.1 = wat::parse_str(POLICY_COMPONENT).unwrap();
    // Preserve the actual API-version custom section from the normative vector.
    component.1.extend_from_slice(&api_section);
    let length = component.1.len();
    let digest = format!("{:x}", Sha256::digest(&component.1));
    let manifest = entries.iter_mut().find(|(path, _)| path == "manifest.json").unwrap();
    let mut document: serde_json::Value = serde_json::from_slice(&manifest.1).unwrap();
    let inventory = document["files"].as_array_mut().unwrap().iter_mut()
        .find(|row| row["path"] == "server.wasm").unwrap();
    inventory["length"] = serde_json::json!(length);
    inventory["sha256"] = serde_json::json!(digest);
    manifest.1 = serde_json::to_vec(&document).unwrap();
    entries
}

fn vector() -> Vec<u8> {
    let digits: String = include_str!("package/golden/minimal_unsigned.hex")
        .chars().filter(|c| !c.is_whitespace()).collect();
    (0..digits.len()).step_by(2)
        .map(|i| u8::from_str_radix(&digits[i..i + 2], 16).unwrap()).collect()
}
fn files() -> Vec<(String, Vec<u8>)> {
    ilium_plugin_host::package::read_package(&vector()).unwrap().entries
        .into_iter().map(|entry| (entry.path, entry.bytes)).collect()
}

#[derive(Default)]
struct Effects {
    approvals: std::sync::Mutex<Vec<ilium_plugin_host::install::InstallGrantDecision>>,
    notifications: std::sync::atomic::AtomicUsize,
}
impl ilium_plugin_host::install::InstallEffects for Effects {
    fn record_approval(&self, decision: &ilium_plugin_host::install::InstallGrantDecision)
        -> Result<(), ilium_plugin_api::Diagnostic> {
        self.approvals.lock().unwrap().push(decision.clone());
        Ok(())
    }
    fn plugins_changed(&self) -> Result<(), ilium_plugin_api::Diagnostic> {
        self.notifications.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}
struct Review(bool);
impl ilium_plugin_host::install::Consent for Review {
    fn decide(&self, request: &ilium_plugin_host::install::ConsentRequest)
        -> ilium_plugin_host::install::ConsentDecision {
        assert_eq!(request.plugin, "example.minimal");
        assert_eq!(request.requested.len(), 1);
        if self.0 {
            ilium_plugin_host::install::ConsentDecision::Approve(request.requested.clone())
        } else {
            ilium_plugin_host::install::ConsentDecision::Deny
        }
    }
}

fn exercise_file_install(approve: bool) {
    use ilium_plugin_api::{manifest::{CheckMode, ValidationContext}, Platform, Side, Tier};
    use ilium_plugin_host::{
        cli::install_command::install_file,
        install::{Installer, InstallOptions, InstallProvenance, TrustedInstallContext},
        package::{read_package, write_package}, policy::{PolicyEngine, PolicySourceKind},
        registry::PackageStore, stores::GrantStore, trust::TrustSnapshot,
    };
    use sha2::{Digest, Sha256};
    // All writes belong to this disposable fixture; no real user state or session.
    let root = tempfile::tempdir().unwrap();
    let mut entries = policy_files();
    let manifest = entries.iter_mut().find(|(name, _)| name == "manifest.json").unwrap();
    let mut document: serde_json::Value = serde_json::from_slice(&manifest.1).unwrap();
    document["capabilities"] = serde_json::json!({"server":["tree.read"]});
    manifest.1 = serde_json::to_vec(&document).unwrap();
    let bytes = write_package(&entries).unwrap();
    let input = root.path().join("input.iliumpkg");
    std::fs::write(&input, &bytes).unwrap();
    let installer = Installer::with_options(
        PackageStore { root: root.path().join("plugins/store") },
        root.path().join("state"), InstallOptions { allow_unsigned: true });
    let mut grants = GrantStore::open(&root.path().join("state")).unwrap();
    let before_grants = std::fs::read(grants.path()).unwrap();
    let trust = TrustSnapshot::default();
    let policy = PolicyEngine::new();
    let effects = Effects::default();
    let mut context = TrustedInstallContext {
        trust: &trust, policy: &policy,
        validation: ValidationContext { tier: Tier::T3, mode: CheckMode::Install,
            platform: Platform::Linux, installed: vec![], core_plugins: vec![], unstable_features: vec![] },
        host_version: ilium_plugin_api::ApiVersion { major: 0, minor: 1, patch: 0 },
        provenance: InstallProvenance { source: input.to_str().unwrap().into(),
            kind: PolicySourceKind::File, channel: "file".into() },
        grant_store: &mut grants, effects: &effects,
    };
    let output = install_file(&input, &installer, &mut context, &Review(approve));
    assert_eq!(std::fs::read(&input).unwrap(), bytes);
    assert_eq!(installer.instantiated_count(), 0);
    let count = output.stdout_jsonl.len();
    assert_eq!(output.stdout_jsonl[count - 2]["type"], "summary");
    assert_eq!(output.stdout_jsonl[count - 1]["type"], "result");
    let version = root.path().join("plugins/store/example.minimal/0.1.0");
    if !approve {
        assert_eq!(output.exit_code, 1, "{}", output.stdout_raw);
        assert_eq!(output.stdout_jsonl[0]["code"], "install.review-declined");
        assert_eq!(std::fs::read(grants.path()).unwrap(), before_grants);
        assert!(!root.path().join("plugins/installed.json").exists());
        assert!(!version.exists());
        assert!(effects.approvals.lock().unwrap().is_empty());
        assert_eq!(effects.notifications.load(std::sync::atomic::Ordering::SeqCst), 0);
        return;
    }
    assert_eq!(output.exit_code, 0, "{}", output.stdout_raw);
    assert_eq!(output.stdout_jsonl[0]["type"], "plugin");
    assert_eq!(output.stdout_jsonl[0]["id"], "example.minimal");
    assert_eq!(output.stdout_jsonl[0]["state"], "enabled");
    assert_eq!(std::fs::read(version.join("package.iliumpkg")).unwrap(), bytes);
    for entry in read_package(&bytes).unwrap().entries {
        let installed = version.join(entry.path);
        assert_eq!(std::fs::read(&installed).unwrap(), entry.bytes);
        assert!(std::fs::metadata(installed).unwrap().permissions().readonly());
    }
    let registry: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.path().join("plugins/installed.json")).unwrap()).unwrap();
    assert_eq!(registry["plugins"][0]["package_sha256"], format!("{:x}", Sha256::digest(&bytes)));
    let reopened = GrantStore::open(&root.path().join("state")).unwrap();
    assert_eq!(reopened.load_for_side("example.minimal", "", "t3", Side::Server).unwrap(),
        vec![ilium_plugin_api::grants::Grant::parse("tree.read").unwrap()]);
    assert_eq!(effects.approvals.lock().unwrap().len(), 1);
    assert_eq!(effects.notifications.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn file_install_commits_immutable_package_and_bound_grants_without_session() {
    exercise_file_install(true);
}

#[test]
fn declined_file_install_preserves_grants_and_publishes_nothing() {
    exercise_file_install(false);
}

#[derive(Default)]
struct RecordingBackend(std::sync::Mutex<Vec<std::path::PathBuf>>);
impl ilium_plugin_host::cli::install_command::InstallCommandBackend for RecordingBackend {
    fn install(&self, path: &std::path::Path) -> ilium_plugin_host::cli::CliOutput {
        self.0.lock().unwrap().push(path.to_owned());
        ilium_plugin_host::cli::CliOutput {
            exit_code: 7,
            stdout_jsonl: vec![serde_json::json!({"type":"result","ok":false,"exit_code":7})],
            stderr: "backend diagnostic".into(),
            stdout_raw: "actual backend bytes\n".into(),
            stdout_text: "actual backend presentation".into(),
        }
    }
}

#[test]
fn argument_failures_never_reach_install_backend() {
    use ilium_plugin_host::cli::install_command::run;
    let backend = RecordingBackend::default();
    for args in [vec!["a.iliumpkg", "--unexpected"], vec!["a.iliumpkg", "b.iliumpkg"],
        vec!["a.iliumpkg", "--yes"], vec!["a.iliumpkg", "--allow-unsigned"], vec![]] {
        let args = args.into_iter().map(str::to_owned).collect::<Vec<_>>();
        assert_eq!(run(&args, std::path::Path::new("/explicit/project"), &backend).exit_code, 2);
    }
    assert!(backend.0.lock().unwrap().is_empty());
}

#[test]
fn file_arguments_resolve_against_explicit_cwd_and_preserve_backend_output() {
    use ilium_plugin_host::cli::install_command::{run, InstallCommandBackend};
    let backend = RecordingBackend::default();
    let path = std::path::Path::new("/explicit/project/package.iliumpkg");
    let expected = backend.install(path);
    backend.0.lock().unwrap().clear();
    let args = ["--quiet", "package.iliumpkg", "--json", "--no-color"].map(str::to_owned);
    let actual = run(&args, std::path::Path::new("/explicit/project"), &backend);
    assert_eq!(backend.0.lock().unwrap().as_slice(), &[path.to_owned()]);
    assert_eq!(actual.exit_code, expected.exit_code);
    assert_eq!(actual.stdout_jsonl, expected.stdout_jsonl);
    assert_eq!(actual.stdout_raw, expected.stdout_raw);
    assert_eq!(actual.stdout_text, expected.stdout_text);
    assert_eq!(actual.stderr, expected.stderr);
}

#[test]
fn registry_and_download_sources_are_refused_without_install_service_calls() {
    use ilium_plugin_host::cli::install_command::run;
    let backend = RecordingBackend::default();
    for source in ["example.plugin", "https://example.com/plugin.iliumpkg"] {
        let output = run(&[source.to_owned()], std::path::Path::new("/project"), &backend);
        assert_eq!(output.exit_code, 1);
        assert_eq!(output.stdout_jsonl[0]["code"], "install.context-required");
    }
    assert!(backend.0.lock().unwrap().is_empty());
}

#[test]
fn public_cli_dispatch_uses_explicit_backend_without_session_or_implicit_consent() {
    use ilium_plugin_host::{
        cli::{run, CliEnv},
        registry_client::{FetchError, Fetcher},
    };
    use std::{sync::Arc, time::SystemTime};
    struct NoNetwork;
    impl Fetcher for NoNetwork {
        fn get(&self, _: &str) -> Result<Vec<u8>, FetchError> {
            panic!("local file-install dispatch must not fetch");
        }
    }
    let root = tempfile::tempdir().unwrap();
    let backend = Arc::new(RecordingBackend::default());
    let env = CliEnv::new(
        root.path().join("data"), root.path().join("state"),
        root.path().join("config"), root.path().to_owned(),
        SystemTime::now(), true, Arc::new(NoNetwork),
    ).with_installer(backend.clone());
    assert!(env.session.is_none());
    let output = run(&["install".into(), "package.iliumpkg".into(), "--json".into()], &env);
    // The explicit service returns policy exit 7: neither the absent session
    // nor CLI dispatch may replace that actual service decision with success.
    assert_eq!(output.exit_code, 7);
    assert_eq!(output.stdout_jsonl.last().unwrap()["exit_code"], 7);
    assert_eq!(backend.0.lock().unwrap().as_slice(), &[root.path().join("package.iliumpkg")]);
    let rejected = run(&["install".into(), "package.iliumpkg".into(), "--yes".into()], &env);
    assert_eq!(rejected.exit_code, 2);
    assert_eq!(rejected.stdout_jsonl[0]["code"], "usage.unknown-flag");
    assert_eq!(backend.0.lock().unwrap().len(), 1);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
