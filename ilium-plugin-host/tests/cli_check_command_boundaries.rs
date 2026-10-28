use ilium_plugin_host::{
    cli::{check_command::run, CliEnv},
    package::{read_package, write_package},
    registry_client::{FetchError, Fetcher},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc, time::SystemTime};

const CHECK_COMPONENT: &str = r#"(component

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

struct NoNetwork;
impl Fetcher for NoNetwork {
    fn get(&self, _: &str) -> Result<Vec<u8>, FetchError> {
        panic!("offline check must not fetch")
    }
}
fn env(root: &Path) -> CliEnv {
    CliEnv::new(
        root.join("data"),
        root.join("state"),
        root.join("config"),
        root.into(),
        SystemTime::UNIX_EPOCH,
        true,
        Arc::new(NoNetwork),
    )
}
fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).into()).collect()
}
fn files() -> Vec<(String, Vec<u8>)> {
    let hex: String = include_str!("package/golden/minimal_unsigned.hex")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).unwrap())
        .collect();
    let mut entries: Vec<_> = read_package(&bytes)
        .unwrap()
        .entries
        .into_iter()
        .map(|entry| (entry.path, entry.bytes))
        .collect();
    let component = entries
        .iter_mut()
        .find(|(path, _)| path == "server.wasm")
        .unwrap();
    let section = component.1[8..].to_vec();
    // Actual valid component exports, never instantiated by an offline check.
    component.1 = wat::parse_str(CHECK_COMPONENT).unwrap();
    component.1.extend_from_slice(&section);
    let length = component.1.len();
    let hash = format!("{:x}", Sha256::digest(&component.1));
    let raw = entries
        .iter_mut()
        .find(|(path, _)| path == "manifest.json")
        .unwrap();
    let mut manifest: Value = serde_json::from_slice(&raw.1).unwrap();
    for row in manifest["files"].as_array_mut().unwrap() {
        if row["path"] == "server.wasm" {
            row["length"] = json!(length);
            row["sha256"] = json!(hash);
        }
    }
    raw.1 = serde_json::to_vec(&manifest).unwrap();
    entries
}
fn project(root: &Path) {
    std::fs::create_dir(root.join("dist")).unwrap();
    for (path, bytes) in files() {
        std::fs::write(
            if path == "manifest.json" {
                root.join(path)
            } else {
                root.join("dist").join(path)
            },
            bytes,
        )
        .unwrap();
    }
    std::fs::write(root.join("README.md"), "Example").unwrap();
    std::fs::write(root.join("CHANGELOG.md"), "Changes").unwrap();
}
#[test]
fn actual_package_and_project_pass_offline_without_creating_host_state() {
    let root = tempfile::tempdir().unwrap();
    project(root.path());
    let env = env(root.path());
    let output = run("check", &[], &env);
    assert_eq!(output.exit_code, 0, "{:?}", output.stdout_jsonl);
    std::fs::write(
        root.path().join("valid.iliumpkg"),
        write_package(&files()).unwrap(),
    )
    .unwrap();
    let output = run("check", &args(&["valid.iliumpkg"]), &env);
    assert_eq!(output.exit_code, 0, "{:?}", output.stdout_jsonl);
    assert!(!env.data_dir.exists());
    assert!(!env.state_dir.exists());
    assert!(!env.config_dir.exists());
}
#[test]
fn missing_component_and_bad_archive_are_findings_not_command_errors() {
    let root = tempfile::tempdir().unwrap();
    project(root.path());
    let env = env(root.path());
    std::fs::remove_file(root.path().join("dist/server.wasm")).unwrap();
    let output = run("check", &[], &env);
    assert_eq!(output.exit_code, 3);
    assert!(output
        .stdout_jsonl
        .iter()
        .any(|r| r["rule"] == "component.missing"));
    assert!(!output.stdout_jsonl.iter().any(|r| r["type"] == "error"));
    assert!(output
        .stdout_jsonl
        .iter()
        .any(|r| r["type"] == "summary" && r["errors"] == 0));
    std::fs::write(root.path().join("bad.iliumpkg"), b"broken archive").unwrap();
    let output = run("check", &args(&["bad.iliumpkg"]), &env);
    assert_eq!(output.exit_code, 3);
    assert!(output
        .stdout_jsonl
        .iter()
        .any(|r| r["type"] == "check" && r["severity"] == "error"));
}
#[test]
fn selected_lint_warn_denial_and_reasoned_suppression_use_real_manifest() {
    let root = tempfile::tempdir().unwrap();
    project(root.path());
    let env = env(root.path());
    let path = root.path().join("manifest.json");
    let mut value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["description"] = json!("");
    std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    let options = args(&["--rule", "lint.description-missing"]);
    let output = run("lint", &options, &env);
    assert_eq!(output.exit_code, 0);
    let findings: Vec<_> = output
        .stdout_jsonl
        .iter()
        .filter(|r| r["type"] == "lint")
        .collect();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["suppressed"], false);
    assert_eq!(
        run(
            "lint",
            &args(&["--rule", "lint.description-missing", "--deny", "warnings"]),
            &env
        )
        .exit_code,
        3
    );
    std::fs::write(root.path().join("lint.toml"),"[[suppress]]\nrule='lint.description-missing'\nreason='Intentional fixture'\nkey_path='/description'\n").unwrap();
    let output = run(
        "lint",
        &args(&["--deny", "warnings", "--rule", "lint.description-missing"]),
        &env,
    );
    assert_eq!(output.exit_code, 0);
    assert!(output
        .stdout_jsonl
        .iter()
        .any(|r| r["rule"] == "lint.description-missing" && r["suppressed"] == true));
}
#[test]
fn unknown_rule_and_invalid_suppression_fail_without_hiding_findings() {
    let root = tempfile::tempdir().unwrap();
    project(root.path());
    let env = env(root.path());
    let output = run("lint", &args(&["--rule", "lint.not-real"]), &env);
    assert_eq!(output.exit_code, 2);
    assert!(output
        .stdout_jsonl
        .iter()
        .any(|r| r["code"] == "usage.invalid-value"));
    std::fs::write(root.path().join("lint.toml"),"[[suppress]]\nrule='manifest.schema'\nreason='Cannot suppress checks'\n[[suppress]]\nrule='lint.description-missing'\n").unwrap();
    let output = run("lint", &[], &env);
    assert_eq!(output.exit_code, 3);
    for rule in [
        "lint.suppression-unknown",
        "lint.suppression-reason-missing",
    ] {
        assert!(output.stdout_jsonl.iter().any(|r| r["rule"] == rule));
    }
}
#[test]
fn freshness_uses_actual_input_hash_and_warn_does_not_fail_check() {
    let root = tempfile::tempdir().unwrap();
    project(root.path());
    let env = env(root.path());
    let hash = ilium_plugin_host::cli::project_input_hash(root.path()).unwrap();
    std::fs::write(
        root.path().join("dist/build.json"),
        serde_json::to_vec(&json!({"input_hash":hash})).unwrap(),
    )
    .unwrap();
    assert_eq!(run("check", &[], &env).exit_code, 0);
    std::fs::write(root.path().join("README.md"), "Changed source input").unwrap();
    let output = run("check", &[], &env);
    assert_eq!(output.exit_code, 0);
    assert!(output
        .stdout_jsonl
        .iter()
        .any(|r| r["rule"] == "check.stale-build" && r["severity"] == "warn"));
}
#[test]
fn malformed_source_manifest_is_a_manifest_finding_and_unknown_flags_are_usage_errors() {
    let root = tempfile::tempdir().unwrap();
    let env = env(root.path());
    std::fs::write(root.path().join("manifest.json"), "{ broken").unwrap();
    let output = run("check", &[], &env);
    assert_eq!(output.exit_code, 3);
    assert!(output.stdout_jsonl.iter().any(|r| r["type"] == "check"
        && r["rule"]
            .as_str()
            .is_some_and(|rule| rule.starts_with("manifest."))));
    let output = run("check", &args(&["--deny", "warnings"]), &env);
    assert_eq!(output.exit_code, 2);
    assert!(output
        .stdout_jsonl
        .iter()
        .any(|r| r["code"] == "usage.unknown-flag"));
}
#[test]
fn malformed_suppression_scope_cannot_widen_to_the_whole_rule() {
    let root = tempfile::tempdir().unwrap();
    project(root.path());
    let env = env(root.path());
    std::fs::write(
        root.path().join("lint.toml"),
        "[[suppress]]\nrule='lint.license-missing'\nreason='test'\nkey_path=123\n",
    )
    .unwrap();
    let output = run("lint", &[], &env);
    assert_eq!(output.exit_code, 3);
    assert!(output
        .stdout_jsonl
        .iter()
        .any(|r| r["rule"] == "lint.suppression-unknown"));
    assert!(!output.stdout_jsonl.iter().any(|r| r["suppressed"] == true));
}
