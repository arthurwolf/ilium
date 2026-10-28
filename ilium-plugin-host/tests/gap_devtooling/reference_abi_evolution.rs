//! ABI evolution rules: additive changes, deprecation windows, removals and the host's
//! compatibility gate (DEV-967, DEV-968, DEV-975, DEV-977, DEV-978). Page: reference-abi-evolution.

use crate::common::*;
use crate::{
    abi_compat_gate, abi_removal_of, abi_surface, deprecated_item_build, host_load_attempt,
    manual_page, new_rust_project, plugin_built_against, set_manifest_abi, with_code,
};

/// REQ: DEV-967
#[test]
fn additive_abi_change_adds_a_new_minor_and_old_plugins_still_load_unchanged() {
    let page = manual_page("reference-abi-evolution");
    assert!(
        page.to_lowercase().contains("additive"),
        "the page names additive changes"
    );
    let before = abi_surface("1.4");
    let after = abi_surface("1.5");
    assert!(
        before.iter().all(|item| after.contains(item)),
        "every 1.4 interface item still exists in 1.5"
    );
    assert!(after.len() > before.len(), "1.5 adds at least one item");
    let old_plugin_loads = plugin_built_against("1.4").loads_on_host("1.5");
    assert!(
        old_plugin_loads,
        "a plugin built against 1.4 loads unchanged on a 1.5 host"
    );
}

/// REQ: DEV-968
#[test]
fn removal_is_preceded_by_a_deprecation_window_of_at_least_one_minor_and_a_warning() {
    let page = manual_page("reference-abi-evolution");
    assert!(
        page.to_lowercase().contains("deprecat"),
        "the page describes deprecation"
    );
    let removal = abi_removal_of("export-old-thing");
    assert!(
        removal.deprecated_in_minor < removal.removed_in_minor,
        "the item is deprecated in an earlier minor than the removal"
    );
    assert!(
        removal.removed_in_minor - removal.deprecated_in_minor >= 1,
        "the deprecation window spans at least one minor"
    );
    let (_root, env, _dir) = new_rust_project();
    let deprecated_run = deprecated_item_build(&env, "export-old-thing");
    assert!(
        !with_code(&deprecated_run, "abi.deprecated").is_empty(),
        "a deprecated item prints an abi.deprecated warning"
    );
}

/// REQ: DEV-975
#[test]
fn host_refuses_plugin_built_for_an_unknown_future_minor_with_abi_too_new() {
    let (_root, env, dir) = new_rust_project();
    set_manifest_abi(&dir, "9.9");
    let out = run_in_project(&env, "check", &[]);
    assert_eq!(
        error_code(&out).as_deref(),
        Some("abi.too-new"),
        "{:?}",
        out.stdout_jsonl
    );
    let loaded = host_load_attempt(&dir, "9.9");
    assert!(!loaded.started, "the host does not start the plugin");
    assert_eq!(loaded.error_code.as_deref(), Some("abi.too-new"));
}

/// REQ: DEV-977
#[test]
fn breaking_change_is_refused_by_the_gate_unless_the_major_version_changes() {
    let report = abi_compat_gate("1.4", "1.5-with-removed-export");
    assert!(
        !report.passes,
        "a removed export in the same major fails the gate"
    );
    assert_eq!(report.failing_rule.as_deref(), Some("abi.breaking-change"));
    let major = abi_compat_gate("1.4", "2.0-with-removed-export");
    assert!(
        major.passes,
        "the same removal in a new major passes the gate"
    );
}

/// REQ: DEV-978
#[test]
fn compat_gate_names_every_removed_or_changed_export_in_its_report() {
    let report = abi_compat_gate("1.4", "1.5-with-removed-export");
    let named = report.named_items();
    for item in report.changed_items() {
        assert!(
            named.iter().any(|name| name == &item),
            "the report names the changed export {item}"
        );
    }
    assert!(
        !report.changed_items().is_empty(),
        "the fixture changes at least one export"
    );
}

/// REQ: DEV-967
#[test]
fn two_patch_releases_of_one_minor_export_the_same_wit_surface() {
    assert_eq!(
        crate::release_surface("1.4.0"),
        crate::release_surface("1.4.2"),
        "patch releases of minor 1.4 export identical WIT packages"
    );
}

/// REQ: DEV-968
#[test]
fn ilium_version_json_lists_the_wit_hashes_of_the_shipped_packages() {
    let (_root, env, _dir) = new_rust_project();
    let version = run(&env, &["version", "--json"]);
    assert_eq!(version.exit_code, 0, "{:?}", version.stdout_jsonl);
    assert!(
        version
            .stdout_jsonl
            .iter()
            .any(|record| record.get("wit_hashes").is_some()),
        "ilium version --json lists wit_hashes: {:?}",
        version.stdout_jsonl
    );
}

/// REQ: DEV-975
#[test]
fn host_starts_a_plugin_built_for_a_supported_earlier_minor() {
    let (_root, _env, dir) = new_rust_project();
    set_manifest_abi(&dir, "1.4");
    let loaded = host_load_attempt(&dir, "1.4");
    assert!(
        loaded.started,
        "a plugin built for minor 1.4 starts on the 1.4 host"
    );
    assert_eq!(loaded.error_code, None);
}

/// REQ: DEV-977
#[test]
fn additive_change_within_the_same_major_passes_the_compatibility_gate() {
    let report = abi_compat_gate("1.4", "1.5");
    assert!(
        report.passes,
        "an additive minor passes the gate: {:?}",
        report.failing_rule
    );
}

/// REQ: DEV-978
#[test]
fn compat_gate_report_of_an_unchanged_release_lists_no_changed_items() {
    let report = abi_compat_gate("1.4", "1.4");
    assert!(
        report.changed_items().is_empty(),
        "an unchanged release changes no export"
    );
    assert!(report.passes, "an unchanged release passes the gate");
}
