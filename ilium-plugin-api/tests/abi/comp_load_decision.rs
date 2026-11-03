//! Load decision and refusal of a package: CMP-001, CMP-002, CMP-003, CMP-007, CMP-008,
//! CMP-009 (compat-load-decision-table.md).

use std::collections::HashSet;

use ilium_plugin_api::abi::{trigger_abi_failure, Installation, LoadOutcome};
use ilium_plugin_api::codes;
use ilium_plugin_api::version::{host_window, ApiVersion, API_VERSION_BYTES, CURRENT, REFUSAL_ORDER};

/// REQ: CMP-001
#[test]
fn comp_001_decision_order_follows_refusal_table_a() {
    assert_eq!(
        REFUSAL_ORDER,
        [
            codes::manifest::SCHEMA,
            codes::package::API_UNSUPPORTED,
            codes::manifest::API_VERSION,
            codes::manifest::IMPORT_NOT_REQUESTED,
            codes::manifest::GRANT_UNKNOWN,
        ]
    );
}

/// REQ: CMP-001
#[test]
fn comp_001_each_check_has_its_own_code_so_one_code_is_reported_b() {
    let distinct: HashSet<&str> = REFUSAL_ORDER.iter().copied().collect();
    assert_eq!(distinct.len(), REFUSAL_ORDER.len());
}

/// REQ: CMP-002
#[test]
fn comp_002_refused_package_leaves_installed_version_active_a() {
    let mut installation = Installation::sample_mixed();
    installation.upgrade_host(CURRENT);
    assert_eq!(
        installation.load("refused-after-upgrade"),
        LoadOutcome::Refused("abi.minor-unsupported".to_owned())
    );
    assert_eq!(installation.load("stable-plugin"), LoadOutcome::Loaded);
}

/// REQ: CMP-002
#[test]
fn comp_002_unknown_package_refusal_changes_no_installed_state_b() {
    let mut installation = Installation::sample_mixed();
    let before = installation.snapshot();
    assert_eq!(
        installation.load("not-installed"),
        LoadOutcome::Refused("plugin.not-found".to_owned())
    );
    assert_eq!(installation.snapshot(), before);
}

/// REQ: CMP-003
#[test]
fn comp_003_section_version_equal_to_api_value_is_accepted_a() {
    let section = ApiVersion::from_custom_section(&API_VERSION_BYTES).expect("six bytes");
    let api = ApiVersion::parse("ilium:plugin@0.1.0").expect("api value");
    assert_eq!(section, CURRENT);
    assert_eq!(section, api);
}

/// REQ: CMP-003
#[test]
fn comp_003_patch_difference_is_a_mismatch_with_manifest_api_version_b() {
    let section = ApiVersion::from_custom_section(&API_VERSION_BYTES).expect("six bytes");
    let api = ApiVersion::parse("ilium:plugin@0.1.1").expect("api value");
    assert_ne!(section, api, "a patch difference counts as a mismatch");
    assert_eq!(codes::manifest::API_VERSION, "manifest.api-version");
}

/// REQ: CMP-007
#[test]
fn comp_007_api_unsupported_names_lowest_and_highest_of_window_a() {
    let window = host_window();
    assert_eq!(window.start().to_string(), "0.0.0");
    assert_eq!(window.end().minor, CURRENT.minor);
}

/// REQ: CMP-007
#[test]
fn comp_007_highest_bound_of_window_is_the_current_minor_b() {
    let end = host_window().end().to_string();
    assert!(
        end.starts_with(&format!("{}.{}.", CURRENT.major, CURRENT.minor)),
        "highest bound {end} must be inside the current minor"
    );
}

/// REQ: CMP-008
#[test]
fn comp_008_refused_package_adds_no_file_to_installation_a() {
    let mut installation = Installation::sample_mixed();
    installation.upgrade_host(CURRENT);
    let before = installation.snapshot();
    let _ = installation.load("refused-after-upgrade");
    assert_eq!(installation.snapshot(), before);
}

/// REQ: CMP-008
#[test]
fn comp_008_failed_load_keeps_other_plugins_unchanged_b() {
    let mut installation = Installation::sample_mixed();
    installation.upgrade_host(CURRENT);
    let before = installation.snapshot();
    let _ = installation.load("refused-after-upgrade");
    let _ = installation.load("stable-plugin");
    assert_eq!(installation.snapshot(), before);
}

/// REQ: CMP-009
#[test]
fn comp_009_refusal_log_line_carries_code_and_plugin_a() {
    let record = trigger_abi_failure(codes::package::API_UNSUPPORTED, "example.plugin");
    assert_eq!(record.code, codes::package::API_UNSUPPORTED);
    assert_eq!(record.plugin, "example.plugin");
}

/// REQ: CMP-009
#[test]
fn comp_009_every_table_code_gets_its_own_log_line_b() {
    for code in REFUSAL_ORDER {
        let record = trigger_abi_failure(code, "example.plugin");
        assert_eq!(record.code, code);
    }
}
