//! Operator runbook outputs: COMP-017, COMP-020, COMP-021 (compat-host-upgrade-runbook.md).

use ilium_plugin_api::abi::{
    refusal_listing, render_refusal_listing, Installation, InstalledPlugin,
};
use ilium_plugin_api::version::{ApiVersion, CURRENT};

fn older_host() -> ApiVersion {
    ApiVersion {
        major: CURRENT.major,
        minor: CURRENT.minor.saturating_sub(1),
        patch: 0,
    }
}

fn refused(name: &str) -> InstalledPlugin {
    InstalledPlugin {
        name: name.to_owned(),
        declared_api: older_host(),
        refusal: Some("abi.minor-unsupported".to_owned()),
    }
}

fn accepted(name: &str) -> InstalledPlugin {
    InstalledPlugin {
        name: name.to_owned(),
        declared_api: CURRENT,
        refusal: None,
    }
}

/// REQ: COMP-017
#[test]
fn comp_017_refused_plugin_under_triage_appears_in_bulk_report_a() {
    let text = render_refusal_listing(&[refused("refused-after-upgrade"), accepted("ok-plugin")]);
    assert!(text.starts_with("refused-after-upgrade  abi.minor-unsupported"));
}

/// REQ: COMP-017
#[test]
fn comp_017_plugin_accepted_by_host_drops_out_of_refusal_listing_b() {
    let rows = refusal_listing(&[accepted("ok-plugin")]);
    assert!(rows.is_empty(), "an accepted installed version is no longer refused");
}

/// REQ: COMP-020
#[test]
fn comp_020_bulk_report_has_one_row_per_refused_plugin_in_one_output_a() {
    let text = render_refusal_listing(&[
        refused("refused-one"),
        accepted("ok-plugin"),
        refused("refused-two"),
    ]);
    let rows: Vec<&str> = text.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(rows.len(), 2);
}

/// REQ: COMP-020
#[test]
fn comp_020_each_row_carries_refusal_code_and_declared_api_b() {
    let text = render_refusal_listing(&[refused("refused-one")]);
    assert_eq!(
        text,
        format!(
            "refused-one  abi.minor-unsupported  declared api {}\n",
            older_host()
        )
    );
}

/// REQ: COMP-021
#[test]
fn comp_021_saved_installation_snapshot_is_an_in_memory_capture_a() {
    let installation = Installation::sample_mixed();
    assert_eq!(installation.snapshot(), installation.snapshot());
}

/// REQ: COMP-021
#[test]
fn comp_021_refusal_listing_is_plain_text_without_a_store_path_b() {
    let text = render_refusal_listing(&[refused("refused-one")]);
    assert!(!text.contains('/'), "the listing is text output, not a file in the store");
}
