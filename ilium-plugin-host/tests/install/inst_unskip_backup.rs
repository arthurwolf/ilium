//! Requirements INS-014, INS-015 and INS-016: `--unskip`, `--allow-downgrade` and `--no-backup`
//! (manual page reference-plugin-distribution-cli, install and update sections).

use ilium_plugin_host::cli::CliOutput;

use crate::support::*;

fn mentions(out: &CliOutput, text: &str) -> bool {
    out.stderr.contains(text) || out.stdout_jsonl.iter().any(|r| r.to_string().contains(text))
}

/// Rolls back from 1.3.0 to 1.2.0, which leaves 1.3.0 marked `skip`.
fn skipped_newer_version_fixture(fx: &Fx) {
    assert_eq!(fx.install(&Spec::new("com.example.notes", "1.2.0")).exit_code, 0);
    assert_eq!(fx.update_with(&Spec::new("com.example.notes", "1.3.0"), &[]).exit_code, 0);
    assert_eq!(fx.run(&["rollback", "com.example.notes"]).exit_code, 0);
}

/// REQ: INS-014
#[test]
fn inst_014_unskip_clears_mark_of_installed_version_a() {
    let fx = Fx::new();
    skipped_newer_version_fixture(&fx);
    let spec = Spec::new("com.example.notes", "1.3.0");
    let out = fx.install_with(&spec, &["--unskip"]);
    assert_eq!(out.exit_code, 0);
    let record = fx.record("com.example.notes").expect("record present");
    assert!(!record.to_string().contains("\"skip\":true"));
}

/// REQ: INS-014
#[test]
fn inst_014_unskip_changes_no_other_version_b() {
    let fx = Fx::new();
    skipped_newer_version_fixture(&fx);
    let before = fx.installed();
    let spec = Spec::new("com.example.notes", "1.3.0");
    let _ = fx.install_with(&spec, &["--unskip"]);
    let after = fx.installed();
    let before_1_2 = before.to_string().contains("1.2.0");
    assert!(before_1_2 && after.to_string().contains("1.2.0"));
    assert_eq!(fx.active_version("com.example.notes").as_deref(), Some("1.3.0"));
}

/// REQ: INS-015
#[test]
fn inst_015_allow_downgrade_keeps_skip_mark_a() {
    let fx = Fx::new();
    skipped_newer_version_fixture(&fx);
    let spec = Spec::new("com.example.notes", "1.2.0");
    let out = fx.install_with(&spec, &["--allow-downgrade"]);
    assert_eq!(out.exit_code, 0);
    assert!(fx.record("com.example.notes").expect("record present").to_string().contains("1.3.0"));
}

/// REQ: INS-015
#[test]
fn inst_015_allow_downgrade_leaves_skip_flag_on_newer_version_b() {
    let fx = Fx::new();
    skipped_newer_version_fixture(&fx);
    let spec = Spec::new("com.example.notes", "1.2.0");
    let _ = fx.install_with(&spec, &["--allow-downgrade"]);
    assert!(fx.installed().to_string().contains("\"skip\":true"));
}

/// REQ: INS-016
#[test]
fn inst_016_no_backup_fresh_install_exits_zero_a() {
    let fx = Fx::new();
    let spec = Spec::new("com.example.notes", "1.0.0");
    let out = fx.install_with(&spec, &["--no-backup"]);
    assert_eq!(out.exit_code, 0);
    assert_eq!(fx.active_version("com.example.notes").as_deref(), Some("1.0.0"));
}

/// REQ: INS-016
#[test]
fn inst_016_no_backup_fresh_install_emits_one_warning_b() {
    let fx = Fx::new();
    let spec = Spec::new("com.example.notes", "1.0.0");
    let out = fx.install_with(&spec, &["--no-backup"]);
    let warnings = out
        .stdout_jsonl
        .iter()
        .filter(|r| r.get("type").and_then(|t| t.as_str()) == Some("warning"))
        .count();
    assert_eq!(warnings, 1);
    assert!(mentions(&out, "no-backup") || warnings == 1);
}
