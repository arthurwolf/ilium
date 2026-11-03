//! Requirements INS-001, INS-002, INS-005 and INS-006: consent, grant and dry-run flags of
//! `ilium plugin install` (manual page reference-plugin-distribution-cli, install section).

use ilium_plugin_host::cli::CliOutput;

use crate::support::*;

fn mentions(out: &CliOutput, text: &str) -> bool {
    out.stderr.contains(text) || out.stdout_jsonl.iter().any(|r| r.to_string().contains(text))
}

fn package_path(fx: &Fx, spec: &Spec) -> String {
    fx.write_file(&format!("{}-{}.iliumpkg", spec.id, spec.version), &spec.build())
}

/// REQ: INS-001
#[test]
fn inst_001_yes_refuses_high_risk_grant_a() {
    let fx = Fx::new();
    let spec = Spec::new("com.example.notes", "1.0.0").grant("high-risk-grant");
    let path = package_path(&fx, &spec);
    let out = fx.run(&["install", &path, "--yes"]);
    assert_eq!(out.exit_code, 1);
    assert!(mentions(&out, "install.review-required"));
    assert!(fx.record("com.example.notes").is_none());
}

/// REQ: INS-001
#[test]
fn inst_001_yes_refuses_unknown_signing_key_b() {
    let fx = Fx::new();
    let spec = Spec::new("com.example.notes", "1.0.0");
    let path = package_path(&fx, &spec);
    let before = fx.installed();
    let out = fx.run(&["install", &path, "--yes"]);
    assert_eq!(out.exit_code, 1);
    assert!(mentions(&out, "install.review-required"));
    assert_eq!(fx.installed(), before);
}

/// REQ: INS-002
#[test]
fn inst_002_grant_accepts_only_named_grant_a() {
    let fx = Fx::new();
    let spec = Spec::new("com.example.notes", "1.0.0").grant("a").grant("b");
    let path = package_path(&fx, &spec);
    let out = fx.run(&["install", &path, "--allow-unsigned", "--yes", "--grant", "a"]);
    assert_eq!(out.exit_code, 1);
    assert!(fx.record("com.example.notes").is_none());
}

/// REQ: INS-002
#[test]
fn inst_002_grant_other_high_risk_grant_still_blocks_b() {
    let fx = Fx::new();
    let spec = Spec::new("com.example.notes", "1.0.0").grant("a").grant("b");
    let path = package_path(&fx, &spec);
    let grant_before = fx.installed_bytes();
    let out = fx.run(&["install", &path, "--allow-unsigned", "--yes", "--grant", "a"]);
    assert_eq!(out.exit_code, 1);
    assert!(mentions(&out, "install.review-required"));
    assert_eq!(fx.installed_bytes(), grant_before);
}

/// REQ: INS-005
#[test]
fn inst_005_allow_unsigned_reports_no_trusted_signer_a() {
    let fx = Fx::new();
    let spec = Spec::new("com.example.notes", "1.0.0");
    let out = fx.install(&spec);
    assert_eq!(out.exit_code, 0);
    let info = fx.run(&["info", "com.example.notes"]);
    assert!(info.stdout_jsonl.iter().any(|r| r.to_string().contains("trusted")));
    assert!(!info.stdout_jsonl.iter().any(|r| r.to_string().contains("\"signer\":\"trusted")));
}

/// REQ: INS-005
#[test]
fn inst_005_allow_unsigned_update_still_needs_signature_b() {
    let fx = Fx::new();
    let first = Spec::new("com.example.notes", "1.0.0");
    assert_eq!(fx.install(&first).exit_code, 0);
    let second = Spec::new("com.example.notes", "1.1.0");
    let path = package_path(&fx, &second);
    let out = fx.run(&["update", &path, "--yes"]);
    assert_ne!(out.exit_code, 0);
    assert_eq!(fx.active_version("com.example.notes").as_deref(), Some("1.0.0"));
}

/// REQ: INS-006
#[test]
fn inst_006_dry_run_success_writes_nothing_a() {
    let fx = Fx::new();
    let spec = Spec::new("com.example.notes", "1.0.0");
    let path = package_path(&fx, &spec);
    let before = (
        fx.snapshot_tree(&fx.root()),
        fx.snapshot_tree(&fx.state_dir()),
        fx.snapshot_tree(&fx.cache_dir()),
        fx.installed_bytes(),
    );
    let out = fx.run(&["install", &path, "--allow-unsigned", "--yes", "--dry-run"]);
    assert_eq!(out.exit_code, 0);
    let after = (
        fx.snapshot_tree(&fx.root()),
        fx.snapshot_tree(&fx.state_dir()),
        fx.snapshot_tree(&fx.cache_dir()),
        fx.installed_bytes(),
    );
    assert_eq!(before, after);
}

/// REQ: INS-006
#[test]
fn inst_006_dry_run_failure_exit_code_matches_real_install_b() {
    let fx = Fx::new();
    let spec = Spec::new("com.example.notes", "1.0.0");
    let path = package_path(&fx, &spec);
    let tampered = fx.write_file("tampered.iliumpkg", &[0u8; 16]);
    let real = fx.run(&["install", &tampered, "--allow-unsigned", "--yes"]);
    let dry = fx.run(&["install", &tampered, "--allow-unsigned", "--yes", "--dry-run"]);
    assert_eq!(dry.exit_code, real.exit_code);
    assert!(mentions(&dry, "package.hash-mismatch"));
    let _ = path;
}
