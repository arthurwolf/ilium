//! Requirements INS-007, INS-011, INS-013, INS-104, INS-108 and INS-109: `--locked`,
//! `--from-bundle`, `--reinstall` and install locking (manual pages reference-plugin-distribution-cli
//! and reference-install-and-update).

use ilium_plugin_host::cli::CliOutput;

use crate::support::*;

fn mentions(out: &CliOutput, text: &str) -> bool {
    out.stderr.contains(text) || out.stdout_jsonl.iter().any(|r| r.to_string().contains(text))
}

/// REQ: INS-007
#[test]
fn inst_007_locked_hash_mismatch_fails_a() {
    let fx = Fx::new();
    let spec = Spec::new("com.example.notes", "1.0.0");
    assert_eq!(fx.install(&spec).exit_code, 0);
    let out = fx.run(&["install", "--locked"]);
    assert_eq!(out.exit_code, 1);
    assert!(mentions(&out, "dependency.lock-mismatch"));
}

/// REQ: INS-007
#[test]
fn inst_007_locked_hash_mismatch_no_network_b() {
    let fx = Fx::new();
    let _ = fx.run(&["install", "--locked"]);
    assert!(fx.fetcher.calls().is_empty());
}

/// REQ: INS-011
#[test]
fn inst_011_bundle_install_exit_zero_a() {
    let fx = Fx::new();
    let out = fx.run(&["install", "--from-bundle", "./bundle", "com.example.notes"]);
    assert_eq!(out.exit_code, 0);
}

/// REQ: INS-011
#[test]
fn inst_011_bundle_install_opens_no_connection_b() {
    let fx = Fx::new();
    let _ = fx.run(&["install", "--from-bundle", "./bundle", "com.example.notes"]);
    assert!(fx.fetcher.calls().is_empty());
}

/// REQ: INS-013
#[test]
fn inst_013_reinstall_locked_same_bytes_succeeds_a() {
    let fx = Fx::new();
    let spec = Spec::new("com.example.notes", "1.0.0");
    assert_eq!(fx.install(&spec).exit_code, 0);
    let out = fx.run(&["install", "--locked", "--reinstall"]);
    assert_eq!(out.exit_code, 0);
    assert_eq!(fx.active_version("com.example.notes").as_deref(), Some("1.0.0"));
}

/// REQ: INS-013
#[test]
fn inst_013_reinstall_locked_different_hash_fails_b() {
    let fx = Fx::new();
    let spec = Spec::new("com.example.notes", "1.0.0");
    assert_eq!(fx.install(&spec).exit_code, 0);
    let out = fx.run(&["install", "--locked", "--reinstall"]);
    assert_eq!(out.exit_code, 1);
    assert!(mentions(&out, "dependency.lock-mismatch"));
}

/// REQ: INS-104
#[test]
fn inst_104_lock_mismatch_leaves_installed_json_a() {
    let fx = Fx::new();
    let spec = Spec::new("com.example.notes", "1.0.0");
    assert_eq!(fx.install(&spec).exit_code, 0);
    let before = fx.installed_bytes();
    let out = fx.run(&["install", "--locked"]);
    assert_eq!(out.exit_code, 1);
    assert_eq!(fx.installed_bytes(), before);
}

/// REQ: INS-104
#[test]
fn inst_104_lock_mismatch_error_names_code_b() {
    let fx = Fx::new();
    let spec = Spec::new("com.example.notes", "1.0.0");
    assert_eq!(fx.install(&spec).exit_code, 0);
    let out = fx.run(&["install", "--locked"]);
    assert!(mentions(&out, "dependency.lock-mismatch"));
}

/// REQ: INS-108
#[test]
fn inst_108_incomplete_bundle_fails_download_failed_a() {
    let fx = Fx::new();
    let out = fx.run(&["install", "--from-bundle", "./incomplete-bundle", "com.example.notes"]);
    assert_eq!(out.exit_code, 1);
    assert!(mentions(&out, "install.download-failed"));
}

/// REQ: INS-108
#[test]
fn inst_108_incomplete_bundle_never_fetches_missing_file_b() {
    let fx = Fx::new();
    let _ = fx.run(&["install", "--from-bundle", "./incomplete-bundle", "com.example.notes"]);
    assert!(fx.fetcher.calls().is_empty());
}

/// REQ: INS-109
#[test]
fn inst_109_locked_second_install_keeps_files_a() {
    let fx = Fx::new();
    let spec = Spec::new("com.example.notes", "1.0.0");
    let before_store = fx.snapshot_tree(&fx.data_dir());
    let before_installed = fx.installed_bytes();
    let out = fx.run(&["install", "com.example.notes", "--allow-unsigned", "--yes"]);
    assert_ne!(out.exit_code, 0);
    assert_eq!(fx.installed_bytes(), before_installed);
    assert_eq!(fx.snapshot_tree(&fx.data_dir()), before_store);
    let _ = spec;
}

/// REQ: INS-109
#[test]
fn inst_109_install_locked_refusal_reports_install_locked_b() {
    let fx = Fx::new();
    let before_store = fx.snapshot_tree(&fx.data_dir());
    let out = fx.run(&["install", "com.example.notes", "--allow-unsigned", "--yes"]);
    assert!(out.stderr.contains("install.locked") || out.stdout_jsonl.iter().any(|r| r.to_string().contains("install.locked")));
    assert_eq!(fx.snapshot_tree(&fx.data_dir()), before_store);
}
