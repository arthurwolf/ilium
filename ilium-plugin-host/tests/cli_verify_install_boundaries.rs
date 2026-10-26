use ilium_plugin_host::{
    audit::{AuditActor, AuditContext},
    cli::{local_install::factory, verify_install::run, CliEnv},
    install::{Consent, ConsentDecision, ConsentRequest},
    package::{read_package, write_package},
    registry_client::{FetchError, Fetcher},
};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, SystemTime},
};
struct NoNetwork;
impl Fetcher for NoNetwork {
    fn get(&self, _: &str) -> Result<Vec<u8>, FetchError> {
        panic!("local install must not fetch")
    }
}
struct Answer {
    calls: AtomicUsize,
    approve: bool,
}
impl Consent for Answer {
    fn decide(&self, request: &ConsentRequest) -> ConsentDecision {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.approve {
            ConsentDecision::Approve(request.requested.clone())
        } else {
            ConsentDecision::Deny
        }
    }
}
fn context() -> AuditContext {
    AuditContext {
        session: "local-install-test".into(),
        actor: AuditActor {
            kind: "cli".into(),
            pane: None,
            plugin: None,
        },
    }
}
fn environment(root: &std::path::Path) -> CliEnv {
    CliEnv::new(
        root.join("data"),
        root.join("state"),
        root.join("config"),
        root.into(),
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_792_700_000),
        false,
        Arc::new(NoNetwork),
    )
}
fn vector() -> Vec<u8> {
    let hex: String = include_str!("package/golden/minimal_unsigned.hex")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}
fn files() -> Vec<(String, Vec<u8>)> {
    read_package(&vector())
        .unwrap()
        .entries
        .into_iter()
        .map(|entry| (entry.path, entry.bytes))
        .collect()
}
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

fn signed_package(root: &std::path::Path) -> std::path::PathBuf {
    use base64::Engine as _;
    use ilium_plugin_host::trust::{AddedBy, Status, TrustDocument, TrustEntry};
    use sha2::{Digest, Sha256};
    let public = ed25519_dalek::SigningKey::from_bytes(&[19; 32])
        .verifying_key()
        .to_bytes();
    let entry = TrustEntry {
        key_id: format!("{:x}", Sha256::digest(public)),
        publisher_id: "example".into(),
        publisher: "Example Labs".into(),
        public_key: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public),
        added: "2026-10-22 12:00:00".into(),
        added_by: AddedBy::User,
        status: Status::Trusted,
        revoked: None,
    };
    std::fs::create_dir_all(root.join("state")).unwrap();
    std::fs::write(
        root.join("state/trust.json"),
        serde_json::to_vec(&TrustDocument {
            keys: vec![entry],
            ..TrustDocument::default()
        })
        .unwrap(),
    )
    .unwrap();
    let bytes = write_package(&policy_files()).unwrap();
    let signed = ilium_plugin_host::signing::sign(
        &bytes,
        &ilium_plugin_host::signing::SigningKey::from_seed([19; 32]),
        ilium_plugin_host::signing::SignedAt("2026-10-22 12:00:00".into()),
    )
    .unwrap();
    let path = root.join("signed.iliumpkg");
    std::fs::write(&path, signed).unwrap();
    path
}

fn installed() -> (tempfile::TempDir, CliEnv) {
    let root = tempfile::tempdir().unwrap();
    let env = environment(root.path());
    let path = signed_package(root.path());
    let consent = Arc::new(Answer {
        calls: AtomicUsize::new(0),
        approve: true,
    });
    let output = factory(&env, consent, context()).unwrap().install(&path);
    assert_eq!(output.exit_code, 0, "{:?}", output.stdout_jsonl);
    (root, env)
}
fn component(env: &CliEnv) -> std::path::PathBuf {
    env.data_dir
        .join("plugins/store/example.minimal/0.1.0/server.wasm")
}
fn tamper(env: &CliEnv) {
    let path = component(env);
    ilium_platform::secure_fs::restrict_file_to_owner(&path).unwrap();
    std::fs::write(path, b"changed component").unwrap();
}
#[test]
fn real_install_verifies_without_changing_enable_state() {
    let (_root, env) = installed();
    let output = run("example.minimal", &env, context());
    assert_eq!(output.exit_code, 0, "{:?}", output.stdout_jsonl);
    let id = ilium_plugin_api::ids::parse_plugin_id("example.minimal").unwrap();
    assert!(
        ilium_plugin_host::registry::EnableFlags::open(&env.config_dir.join("config.toml"))
            .unwrap()
            .is_enabled(&id)
    );
}
#[test]
fn actual_tamper_disables_and_audits_with_intact_chain() {
    let (_root, env) = installed();
    tamper(&env);
    let output = run("example.minimal", &env, context());
    assert_ne!(output.exit_code, 0);
    assert_eq!(output.stdout_jsonl[0]["code"], "manifest.file-hash");
    let id = ilium_plugin_api::ids::parse_plugin_id("example.minimal").unwrap();
    assert!(
        !ilium_plugin_host::registry::EnableFlags::open(&env.config_dir.join("config.toml"))
            .unwrap()
            .is_enabled(&id)
    );
    let path = env.state_dir.join("audit/audit.jsonl");
    ilium_plugin_host::audit::AuditLog::open(path.clone())
        .unwrap()
        .verify()
        .unwrap();
    assert!(std::fs::read_to_string(path)
        .unwrap()
        .contains("integrity-failure"));
    assert_eq!(
        std::fs::read(component(&env)).unwrap(),
        b"changed component"
    );
}
#[test]
fn missing_and_unsafe_ids_do_not_create_roots() {
    let root = tempfile::tempdir().unwrap();
    let env = environment(root.path());
    assert_eq!(run("example.missing", &env, context()).exit_code, 5);
    for id in [
        "../outside",
        "/absolute",
        "example.plugin/../other",
        "example\\plugin",
    ] {
        assert_ne!(run(id, &env, context()).exit_code, 0);
    }
    assert!(!env.data_dir.exists());
    assert!(!env.state_dir.exists());
    assert!(!env.config_dir.exists());
}
#[test]
fn audit_failure_preserves_disable_but_never_reports_transition_success() {
    let (_root, env) = installed();
    tamper(&env);
    // The task-created audit file is replaced with a directory to force I/O refusal.
    let audit = env.state_dir.join("audit/audit.jsonl");
    std::fs::remove_file(&audit).unwrap();
    std::fs::create_dir(&audit).unwrap();
    let output = run("example.minimal", &env, context());
    assert_ne!(output.exit_code, 0);
    assert_eq!(output.stdout_jsonl[0]["code"], "audit.io");
    assert!(output.stdout_jsonl[0]["message"]
        .as_str()
        .unwrap()
        .contains("enable flag persisted false"));
    let id = ilium_plugin_api::ids::parse_plugin_id("example.minimal").unwrap();
    assert!(
        !ilium_plugin_host::registry::EnableFlags::open(&env.config_dir.join("config.toml"))
            .unwrap()
            .is_enabled(&id)
    );
}
#[test]
fn disable_preserves_unrelated_user_enable_flags() {
    let (_root, env) = installed();
    std::fs::create_dir_all(&env.config_dir).unwrap();
    std::fs::write(
        env.config_dir.join("config.toml"),
        "[plugins.\"example.other\"]\nenabled=false\n",
    )
    .unwrap();
    tamper(&env);
    assert_ne!(run("example.minimal", &env, context()).exit_code, 0);
    let flags = ilium_plugin_host::registry::EnableFlags::open(&env.config_dir.join("config.toml"))
        .unwrap();
    assert!(!flags.is_enabled(&ilium_plugin_api::ids::parse_plugin_id("example.other").unwrap()));
}
#[cfg(unix)]
#[test]
fn symlinked_component_is_refused_without_reading_or_repairing_target() {
    let (root, env) = installed();
    let external = root.path().join("external-component");
    std::fs::copy(component(&env), &external).unwrap();
    let original = std::fs::read(&external).unwrap();
    std::fs::remove_file(component(&env)).unwrap();
    // Test-only fixture creation; production traversal uses platform no-follow handles.
    assert!(std::process::Command::new("ln")
        .arg("-s")
        .arg(&external)
        .arg(component(&env))
        .status()
        .unwrap()
        .success());
    let output = run("example.minimal", &env, context());
    assert_ne!(output.exit_code, 0);
    assert_eq!(output.stdout_jsonl[0]["code"], "manifest.file-hash");
    assert_eq!(std::fs::read(external).unwrap(), original);
    assert!(std::fs::symlink_metadata(component(&env))
        .unwrap()
        .file_type()
        .is_symlink());
}
