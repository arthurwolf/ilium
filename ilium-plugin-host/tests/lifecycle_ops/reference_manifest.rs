//! Manifest reference (LIF-277, LIF-278): data_version range and migrator exports.

use proptest::prelude::*;

use crate::common::*;

/// REQ: LIF-277
#[test]
fn lif_277_negative_data_version_is_refused_manifest_data_version() {
    let rig = rig();
    let output = install(&rig, &pkg(NOTES, "1.0.0").raw_data_version("-1"));
    assert_eq!(output.exit_code, 1);
    assert!(error_codes(&output).contains(&"manifest.data-version".to_owned()));
}

/// REQ: LIF-277
#[test]
fn lif_277_data_version_above_u32_max_is_refused_manifest_data_version() {
    let rig = rig();
    let output = install(&rig, &pkg(NOTES, "1.0.0").raw_data_version("4294967296"));
    assert!(error_codes(&output).contains(&"manifest.data-version".to_owned()));
}

/// REQ: LIF-277
#[test]
fn lif_277_data_version_zero_and_u32_max_are_accepted() {
    for value in ["0", "4294967295"] {
        let rig = rig();
        let output = install(
            &rig,
            &pkg(NOTES, "1.0.0").raw_data_version(value).migrator_both(),
        );
        assert!(
            !error_codes(&output).contains(&"manifest.data-version".to_owned()),
            "{value}"
        );
    }
}

/// REQ: LIF-277
#[test]
fn lif_277_fractional_or_string_data_version_is_refused() {
    for value in ["1.5", "\"2\""] {
        let rig = rig();
        let output = install(&rig, &pkg(NOTES, "1.0.0").raw_data_version(value));
        assert!(
            error_codes(&output).contains(&"manifest.data-version".to_owned()),
            "{value}"
        );
    }
}

/// REQ: LIF-277
#[test]
fn lif_277_stored_version_above_the_manifest_value_is_handled_as_a_downgrade() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").data_version(3).migrator_both());
    rig.plant_data(NOTES, 3);
    let path = rig.write_package(&pkg(NOTES, "0.9.0").data_version(2).migrator_both());
    let text = path.to_string_lossy().into_owned();
    let output = rig.cli(&[
        "install",
        &text,
        "--yes",
        "--allow-unsigned",
        "--allow-downgrade",
    ]);
    assert_eq!(output.exit_code, 1);
    assert_eq!(
        error_codes(&output),
        vec!["migration.data-newer".to_owned()]
    );
}

proptest! {
    /// REQ: LIF-277
    #[test]
    fn lif_277_data_version_is_valid_exactly_for_integers_in_1_to_u32_max(value in -10i64..=4_294_967_300i64) {
        let rig = rig();
        let spec = pkg(NOTES, "1.0.0").raw_data_version(&value.to_string()).migrator_both();
        let output = install(&rig, &spec);
        let refused = error_codes(&output).contains(&"manifest.data-version".to_owned());
        prop_assert_eq!(refused, !(1..=4_294_967_295i64).contains(&value));
    }
}

/// REQ: LIF-278
#[test]
fn lif_278_data_version_2_without_migrator_in_one_component_exports_is_refused() {
    let rig = rig();
    let output = install(
        &rig,
        &pkg(NOTES, "1.0.0")
            .client_side()
            .data_version(2)
            .migrator_server_only(),
    );
    assert_eq!(output.exit_code, 1);
    assert!(error_codes(&output).contains(&"manifest.exports-mismatch".to_owned()));
}

/// REQ: LIF-278
#[test]
fn lif_278_component_that_does_not_export_migrator_although_listed_is_refused() {
    let rig = rig();
    let output = install(
        &rig,
        &pkg(NOTES, "1.0.0")
            .data_version(2)
            .migrator_listed_but_not_exported(),
    );
    assert!(error_codes(&output).contains(&"manifest.exports-mismatch".to_owned()));
}

/// REQ: LIF-278
#[test]
fn lif_278_data_version_2_with_migrator_in_both_components_is_accepted() {
    let rig = rig();
    let output = install(
        &rig,
        &pkg(NOTES, "1.0.0")
            .client_side()
            .data_version(2)
            .migrator_both(),
    );
    assert!(!error_codes(&output).contains(&"manifest.exports-mismatch".to_owned()));
}

/// REQ: LIF-278
#[test]
fn lif_278_data_version_1_needs_no_migrator() {
    let rig = rig();
    let output = install(&rig, &pkg(NOTES, "1.0.0").data_version(1));
    assert!(!error_codes(&output).contains(&"manifest.exports-mismatch".to_owned()));
}

/// REQ: LIF-278
#[test]
fn lif_278_mismatch_is_reported_at_stage_6_and_leaves_nothing_installed() {
    let rig = rig();
    let output = install(
        &rig,
        &pkg(NOTES, "1.0.0")
            .data_version(2)
            .migrator_server_only()
            .client_side(),
    );
    let line = output
        .stdout_jsonl
        .iter()
        .find(|line| line["code"] == "manifest.exports-mismatch")
        .expect("exports-mismatch line");
    assert_eq!(line["stage"], 6);
    assert!(registry_ids(&rig).is_empty());
}
