//! Requirements INS-003, INS-004, INS-008, INS-009, INS-010, INS-012 and INS-017: flag
//! values and flag combinations that the install command refuses before any work starts
//! (manual page reference-plugin-distribution-cli, install section and combination table).

use ilium_plugin_host::cli::CliOutput;

use crate::support::*;

fn mentions(out: &CliOutput, text: &str) -> bool {
    out.stderr.contains(text) || out.stdout_jsonl.iter().any(|r| r.to_string().contains(text))
}

fn package_path(fx: &Fx, spec: &Spec) -> String {
    fx.write_file(&format!("{}-{}.iliumpkg", spec.id, spec.version), &spec.build())
}

/// REQ: INS-003
#[test]
fn inst_003_grant_not_requested_refused_a() {
    let fx = Fx::new();
    let spec = Spec::new("com.example.notes", "1.0.0").grant("present");
    let path = package_path(&fx, &spec);
    let out = fx.run(&["install", &path, "--allow-unsigned", "--yes", "--grant", "missing"]);
    assert_eq!(out.exit_code, 2);
    assert!(mentions(&out, "usage.invalid-value"));
}

/// REQ: INS-003
#[test]
fn inst_003_grant_refusal_leaves_store_unchanged_b() {
    let fx = Fx::new();
    let spec = Spec::new("com.example.notes", "1.0.0").grant("present");
    let path = package_path(&fx, &spec);
    let before = fx.snapshot_tree(&fx.root());
    let out = fx.run(&["install", &path, "--allow-unsigned", "--yes", "--grant", "missing"]);
    assert_eq!(out.exit_code, 2);
    assert_eq!(fx.snapshot_tree(&fx.root()), before);
}

/// REQ: INS-004
#[test]
fn inst_004_trust_malformed_key_id_refused_a() {
    let fx = Fx::new();
    let spec = Spec::new("com.example.notes", "1.0.0");
    let path = package_path(&fx, &spec);
    let out = fx.run(&["install", &path, "--yes", "--trust", "abc"]);
    assert_eq!(out.exit_code, 2);
    assert!(mentions(&out, "usage.invalid-value"));
}

/// REQ: INS-004
#[test]
fn inst_004_trust_malformed_key_id_writes_no_trust_b() {
    let fx = Fx::new();
    let spec = Spec::new("com.example.notes", "1.0.0");
    let path = package_path(&fx, &spec);
    let before = fx.snapshot_tree(&fx.state_dir());
    let _ = fx.run(&["install", &path, "--yes", "--trust", "abc"]);
    assert_eq!(fx.snapshot_tree(&fx.state_dir()), before);
}

/// REQ: INS-008
#[test]
fn inst_008_locked_with_channel_refused_a() {
    let fx = Fx::new();
    let out = fx.run(&["install", "com.example.notes", "--locked", "--channel", "beta"]);
    assert_eq!(out.exit_code, 2);
    assert!(mentions(&out, "usage.conflict"));
}

/// REQ: INS-008
#[test]
fn inst_008_locked_with_channel_no_work_started_b() {
    let fx = Fx::new();
    let before = fx.snapshot_tree(&fx.root());
    let _ = fx.run(&["install", "com.example.notes", "--locked", "--channel", "beta"]);
    assert_eq!(fx.snapshot_tree(&fx.root()), before);
    assert_eq!(fx.installed(), serde_json::Value::Null);
}

/// REQ: INS-009
#[test]
fn inst_009_locked_with_local_file_refused_a() {
    let fx = Fx::new();
    let spec = Spec::new("com.example.notes", "1.0.0");
    let path = package_path(&fx, &spec);
    let out = fx.run(&["install", &path, "--locked"]);
    assert_eq!(out.exit_code, 2);
    assert!(mentions(&out, "usage.conflict"));
}

/// REQ: INS-009
#[test]
fn inst_009_locked_with_https_url_refused_b() {
    let fx = Fx::new();
    let out = fx.run(&["install", "https://registry.example.test/pkg.iliumpkg", "--locked"]);
    assert_eq!(out.exit_code, 2);
    assert!(mentions(&out, "usage.conflict"));
    assert!(fx.fetcher.calls().is_empty());
}

/// REQ: INS-010
#[test]
fn inst_010_from_bundle_with_registry_refused_a() {
    let fx = Fx::new();
    let out = fx.run(&["install", "--from-bundle", "./bundle", "--registry", REGISTRY_URL, "com.example.notes"]);
    assert_eq!(out.exit_code, 2);
    assert!(mentions(&out, "usage.conflict"));
}

/// REQ: INS-010
#[test]
fn inst_010_from_bundle_with_registry_no_fetch_b() {
    let fx = Fx::new();
    let before = fx.snapshot_tree(&fx.root());
    let _ = fx.run(&["install", "--from-bundle", "./bundle", "--registry", REGISTRY_URL, "com.example.notes"]);
    assert!(fx.fetcher.calls().is_empty());
    assert_eq!(fx.snapshot_tree(&fx.root()), before);
}

/// REQ: INS-012
#[test]
fn inst_012_channel_with_local_file_refused_a() {
    let fx = Fx::new();
    let spec = Spec::new("com.example.notes", "1.0.0");
    let path = package_path(&fx, &spec);
    let out = fx.run(&["install", &path, "--channel", "beta", "--allow-unsigned", "--yes"]);
    assert_eq!(out.exit_code, 2);
    assert!(mentions(&out, "usage.conflict"));
}

/// REQ: INS-012
#[test]
fn inst_012_channel_with_https_url_no_install_b() {
    let fx = Fx::new();
    let out = fx.run(&["install", "https://registry.example.test/pkg.iliumpkg", "--channel", "beta"]);
    assert_eq!(out.exit_code, 2);
    assert_eq!(fx.installed(), serde_json::Value::Null);
}

/// REQ: INS-017
#[test]
fn inst_017_unaccepted_flag_pair_refused_a() {
    let fx = Fx::new();
    let spec = Spec::new("com.example.notes", "1.0.0");
    let path = package_path(&fx, &spec);
    let out = fx.run(&["install", &path, "--dry-run", "--locked"]);
    assert_eq!(out.exit_code, 2);
    assert!(mentions(&out, "usage.conflict"));
}

/// REQ: INS-017
#[test]
fn inst_017_refused_pair_no_file_or_network_access_b() {
    let fx = Fx::new();
    let before = (fx.snapshot_tree(&fx.root()), fx.snapshot_tree(&fx.state_dir()));
    let out = fx.run(&["install", "https://registry.example.test/pkg.iliumpkg", "--from-bundle", "./bundle"]);
    assert_eq!(out.exit_code, 2);
    assert!(fx.fetcher.calls().is_empty());
    assert_eq!((fx.snapshot_tree(&fx.root()), fx.snapshot_tree(&fx.state_dir())), before);
}
