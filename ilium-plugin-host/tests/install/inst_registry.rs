//! Requirements INST-101, INST-102 and INST-103: registry search limits and yanked-version info
//! (manual pages reference-registry and reference-plugin-distribution-cli).

use ilium_plugin_host::cli::CliOutput;

use crate::support::*;

fn mentions(out: &CliOutput, text: &str) -> bool {
    out.stderr.contains(text) || out.stdout_jsonl.iter().any(|r| r.to_string().contains(text))
}

/// REQ: INST-101
#[test]
fn inst_101_second_install_of_same_file_keeps_store_a() {
    let fx = Fx::new();
    let spec = Spec::new("com.example.notes", "1.0.0");
    assert_eq!(fx.install(&spec).exit_code, 0);
    let store_before = fx.snapshot_tree(&fx.data_dir());
    let out = fx.install(&spec);
    assert_eq!(out.exit_code, 0);
    assert_eq!(fx.snapshot_tree(&fx.data_dir()), store_before);
}

/// REQ: INST-101
#[test]
fn inst_101_second_install_keeps_installed_json_bytes_b() {
    let fx = Fx::new();
    let spec = Spec::new("com.example.notes", "1.0.0");
    assert_eq!(fx.install(&spec).exit_code, 0);
    let installed_before = fx.installed_bytes();
    let _ = fx.install(&spec);
    assert_eq!(fx.installed_bytes(), installed_before);
}

/// REQ: INST-102
#[test]
fn inst_102_search_limit_zero_refused_before_network_a() {
    let fx = Fx::new();
    let out = fx.run(&["search", "notes", "--limit", "0"]);
    assert_eq!(out.exit_code, 2);
    assert!(mentions(&out, "usage.invalid-value"));
}

/// REQ: INST-102
#[test]
fn inst_102_search_limit_101_refused_before_network_b() {
    let fx = Fx::new();
    let out = fx.run(&["search", "notes", "--limit", "101"]);
    assert_eq!(out.exit_code, 2);
    assert!(fx.fetcher.calls().is_empty());
}

/// REQ: INST-103
#[test]
fn inst_103_info_of_yanked_registry_plugin_shows_yanked_a() {
    let fx = Fx::new();
    let out = fx.run(&["info", "com.example.notes", "--version", "1.2.1"]);
    assert_eq!(out.exit_code, 0);
    assert!(out.stdout_jsonl.iter().any(|r| r.get("plugin").and_then(|p| p.get("yanked")).is_some()));
}

/// REQ: INST-103
#[test]
fn inst_103_info_of_yanked_registry_plugin_field_is_true_b() {
    let fx = Fx::new();
    let out = fx.run(&["info", "com.example.notes", "--version", "1.2.1"]);
    assert!(out.stdout_jsonl.iter().any(|r| r.get("plugin").and_then(|p| p.get("yanked")) == Some(&serde_json::Value::Bool(true))));
}
