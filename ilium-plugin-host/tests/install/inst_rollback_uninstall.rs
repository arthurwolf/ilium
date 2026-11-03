//! Requirements INST-105, INST-106, INST-107 and INST-110: rollback, fleet update and uninstall
//! (manual pages reference-plugin-distribution-cli and reference-install-and-update).

use ilium_plugin_host::cli::CliOutput;

use crate::support::*;

fn mentions(out: &CliOutput, text: &str) -> bool {
    out.stderr.contains(text) || out.stdout_jsonl.iter().any(|r| r.to_string().contains(text))
}

/// REQ: INST-105
#[test]
fn inst_105_rollback_without_previous_fails_exit_five_a() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("com.example.notes", "1.0.0")).exit_code, 0);
    let out = fx.run(&["rollback", "com.example.notes"]);
    assert_eq!(out.exit_code, 5);
    assert!(mentions(&out, "install.no-previous"));
}

/// REQ: INST-105
#[test]
fn inst_105_rollback_without_previous_keeps_installed_json_b() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("com.example.notes", "1.0.0")).exit_code, 0);
    let before = fx.installed_bytes();
    let _ = fx.run(&["rollback", "com.example.notes"]);
    assert_eq!(fx.installed_bytes(), before);
}

/// REQ: INST-106
#[test]
fn inst_106_rollback_keeps_newer_version_in_store_a() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("com.example.notes", "1.2.0")).exit_code, 0);
    assert_eq!(fx.update_with(&Spec::new("com.example.notes", "1.3.0"), &[]).exit_code, 0);
    assert_eq!(fx.run(&["rollback", "com.example.notes"]).exit_code, 0);
    assert!(fx.store_dir("com.example.notes", "1.3.0").exists());
}

/// REQ: INST-106
#[test]
fn inst_106_rollback_store_listing_still_has_1_3_0_b() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("com.example.notes", "1.2.0")).exit_code, 0);
    assert_eq!(fx.update_with(&Spec::new("com.example.notes", "1.3.0"), &[]).exit_code, 0);
    let _ = fx.run(&["rollback", "com.example.notes"]);
    let listing = fx.run(&["list", "--store"]);
    assert!(mentions(&listing, "1.3.0"));
}

/// REQ: INST-107
#[test]
fn inst_107_update_all_keeps_successful_plugin_new_version_a() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("com.example.alpha", "1.0.0")).exit_code, 0);
    assert_eq!(fx.install(&Spec::new("com.example.beta", "1.0.0")).exit_code, 0);
    let out = fx.run(&["update", "--all", "--yes"]);
    assert_eq!(out.exit_code, 1);
    assert_eq!(fx.active_version("com.example.alpha").as_deref(), Some("1.1.0"));
}

/// REQ: INST-107
#[test]
fn inst_107_update_all_reports_failed_signature_check_b() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("com.example.alpha", "1.0.0")).exit_code, 0);
    assert_eq!(fx.install(&Spec::new("com.example.beta", "1.0.0")).exit_code, 0);
    let out = fx.run(&["update", "--all", "--yes"]);
    assert!(mentions(&out, "signature"));
}

/// REQ: INST-110
#[test]
fn inst_110_uninstall_keeps_data_folder_a() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("com.example.notes", "1.0.0")).exit_code, 0);
    let file = fx.data_dir().join("com.example.notes").join("user-note.txt");
    std::fs::create_dir_all(file.parent().expect("parent")).expect("create data dir");
    std::fs::write(&file, b"keep me").expect("write data file");
    let before = fx.snapshot_tree(&fx.data_dir());
    assert_eq!(fx.run(&["uninstall", "com.example.notes"]).exit_code, 0);
    assert_eq!(fx.snapshot_tree(&fx.data_dir()), before);
}

/// REQ: INST-110
#[test]
fn inst_110_reinstall_after_uninstall_finds_data_file_b() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("com.example.notes", "1.0.0")).exit_code, 0);
    let file = fx.data_dir().join("com.example.notes").join("user-note.txt");
    std::fs::create_dir_all(file.parent().expect("parent")).expect("create data dir");
    std::fs::write(&file, b"keep me").expect("write data file");
    assert_eq!(fx.run(&["uninstall", "com.example.notes"]).exit_code, 0);
    assert_eq!(fx.install(&Spec::new("com.example.notes", "1.0.0")).exit_code, 0);
    assert_eq!(std::fs::read(&file).expect("data file still present"), b"keep me".to_vec());
}
