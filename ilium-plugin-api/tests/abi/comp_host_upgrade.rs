//! Host upgrade and rollback, operator triage and data version: COMP-004, COMP-005, COMP-015,
//! COMP-016, COMP-018, COMP-019 (compat-load-decision-table.md, compat-host-upgrade-runbook.md).

use ilium_plugin_api::abi::{refusal_listing, Installation, InstalledPlugin, LoadOutcome};
use ilium_plugin_api::codes;
use ilium_plugin_api::version::{ApiVersion, CURRENT};

const REFUSED: &str = "refused-after-upgrade";
const STABLE: &str = "stable-plugin";

fn older_host() -> ApiVersion {
    ApiVersion {
        major: CURRENT.major,
        minor: CURRENT.minor.saturating_sub(1),
        patch: 0,
    }
}

/// REQ: COMP-004
#[test]
fn comp_004_refusing_upgrade_leaves_files_grants_settings_and_data_a() {
    let mut installation = Installation::sample_mixed();
    let before = installation.snapshot();
    installation.upgrade_host(CURRENT);
    assert_eq!(
        installation.load(REFUSED),
        LoadOutcome::Refused("abi.minor-unsupported".to_owned())
    );
    assert_eq!(installation.snapshot(), before);
}

/// REQ: COMP-004
#[test]
fn comp_004_refused_plugin_is_not_uninstalled_b() {
    let mut installation = Installation::sample_mixed();
    installation.upgrade_host(CURRENT);
    assert_ne!(
        installation.load(REFUSED),
        LoadOutcome::Refused("plugin.not-found".to_owned()),
        "a refused plugin is still installed"
    );
}

/// REQ: COMP-005
#[test]
fn comp_005_refused_plugin_loads_again_after_host_rollback_a() {
    let mut installation = Installation::sample_mixed();
    assert_eq!(installation.load(REFUSED), LoadOutcome::Loaded);
    installation.upgrade_host(CURRENT);
    assert!(matches!(installation.load(REFUSED), LoadOutcome::Refused(_)));
    installation.rollback_host();
    assert_eq!(installation.load(REFUSED), LoadOutcome::Loaded);
}

/// REQ: COMP-005
#[test]
fn comp_005_reload_after_rollback_needs_no_file_change_b() {
    let mut installation = Installation::sample_mixed();
    let before = installation.snapshot();
    installation.upgrade_host(CURRENT);
    installation.rollback_host();
    assert_eq!(installation.snapshot(), before);
}

/// REQ: COMP-015
#[test]
fn comp_015_pre_upgrade_list_and_status_are_saved_a() {
    let installation = Installation::sample_mixed();
    let saved: Vec<(&str, LoadOutcome)> = [STABLE, REFUSED]
        .into_iter()
        .map(|name| (name, installation.clone().load(name)))
        .collect();
    assert_eq!(
        saved,
        vec![(STABLE, LoadOutcome::Loaded), (REFUSED, LoadOutcome::Loaded)]
    );
}

/// REQ: COMP-015
#[test]
fn comp_015_post_upgrade_status_can_be_compared_with_saved_b() {
    let saved = Installation::sample_mixed().load(REFUSED);
    let mut upgraded = Installation::sample_mixed();
    upgraded.upgrade_host(CURRENT);
    assert_ne!(upgraded.load(REFUSED), saved);
}

/// REQ: COMP-016
#[test]
fn comp_016_triage_classifies_refused_plugin_by_its_code_a() {
    let installed = vec![
        InstalledPlugin {
            name: REFUSED.to_owned(),
            declared_api: older_host(),
            refusal: Some("abi.minor-unsupported".to_owned()),
        },
        InstalledPlugin {
            name: STABLE.to_owned(),
            declared_api: older_host(),
            refusal: None,
        },
    ];
    let rows = refusal_listing(&installed);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].plugin, REFUSED);
    assert_eq!(rows[0].code, "abi.minor-unsupported");
}

/// REQ: COMP-016
#[test]
fn comp_016_triage_keeps_declared_api_of_refused_plugin_b() {
    let installed = vec![InstalledPlugin {
        name: REFUSED.to_owned(),
        declared_api: older_host(),
        refusal: Some("abi.minor-unsupported".to_owned()),
    }];
    let rows = refusal_listing(&installed);
    assert_eq!(rows[0].declared_api, older_host());
}

/// REQ: COMP-018
#[test]
fn comp_018_host_rollback_keeps_files_grants_and_settings_a() {
    let mut installation = Installation::sample_mixed();
    let before = installation.snapshot();
    installation.upgrade_host(CURRENT);
    installation.rollback_host();
    assert_eq!(installation.snapshot(), before);
}

/// REQ: COMP-018
#[test]
fn comp_018_accepted_plugin_still_loads_after_rollback_b() {
    let mut installation = Installation::sample_mixed();
    installation.upgrade_host(CURRENT);
    installation.rollback_host();
    assert_eq!(installation.load(STABLE), LoadOutcome::Loaded);
    assert_eq!(installation.load(REFUSED), LoadOutcome::Loaded);
}

/// REQ: COMP-019
#[test]
fn comp_019_newer_data_refusal_code_is_migration_data_newer_a() {
    assert_eq!(codes::migration::DATA_NEWER, "migration.data-newer");
}

/// REQ: COMP-019
#[test]
fn comp_019_newer_data_refusal_code_is_in_the_code_table_b() {
    assert!(codes::ALL.contains(&codes::migration::DATA_NEWER));
}
