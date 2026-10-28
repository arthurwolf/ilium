//! Trust and supply chain page (SEC-725, SEC-726, SEC-727).

use crate::support::*;

/// REQ: SEC-725
#[test]
fn sec_725_update_from_a_second_registry_with_the_same_plugin_id_is_refused_as_source_changed() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.cli(&[
        "registry",
        "add",
        "private",
        "https://registry.example.test/private",
    ]);
    rig.serve_registry_plugin("private", NOTES, "1.1.0");
    let refused = rig.cli(&["update", NOTES]);
    assert_ne!(
        refused.exit_code, 0,
        "an update from a different registry must be refused"
    );
    assert!(
        error_codes(&refused)
            .iter()
            .any(|code| code == "registry.source-changed"),
        "expected registry.source-changed, got {:?}",
        error_codes(&refused)
    );
    let switched = rig.cli(&["update", NOTES, "--switch-registry"]);
    assert_eq!(
        switched.exit_code, 0,
        "--switch-registry must allow the update"
    );
}

/// REQ: SEC-726
#[test]
fn sec_726_upload_with_a_denied_import_is_held_in_pending_review_and_not_listed() {
    let rig = rig();
    let package =
        rig.write_package(&pkg(NOTES, "1.0.0").content_marker("denied-import:wasi:sockets"));
    let package_text = package.to_string_lossy().into_owned();
    let output = rig.cli(&[
        "registry",
        "publish",
        &package_text,
        "--account",
        "acme-publisher",
        "--totp",
        "123456",
    ]);
    assert_eq!(
        output.exit_code, 0,
        "the upload itself must be accepted for review"
    );
    let pending = rig.registry_metadata_states(NOTES);
    assert!(
        pending
            .iter()
            .any(|(version, state)| version == "1.0.0" && state == "pending-review"),
        "a version that trips static analysis must be held, got {pending:?}"
    );
    assert!(
        !rig.registry_metadata_visible_versions(NOTES)
            .contains(&"1.0.0".to_string()),
        "clients must not receive a pending-review version in metadata"
    );
}

/// REQ: SEC-726
#[test]
fn sec_726_pending_review_hold_ends_after_72_hours_at_most() {
    let rig = rig();
    rig.plant_pending_review_version(NOTES, "1.0.0");
    rig.advance(secs(72 * 3_600 + 1));
    assert!(
        rig.registry_metadata_visible_versions(NOTES)
            .contains(&"1.0.0".to_string()),
        "a pending-review version must not stay hidden longer than 72 hours"
    );
}

/// REQ: SEC-727
#[test]
fn sec_727_release_builds_are_byte_identical_across_two_machines_for_one_tag() {
    let page = manual_page("reference-trust-and-supply-chain");
    assert!(
        page.contains("byte-identical") && page.contains("CycloneDX") && page.contains("SLSA"),
        "the supply chain page must state the reproducibility, SBOM and provenance rules"
    );
    let rig = rig();
    let (first, second) = rig.build_release_twice("v0.0.0-gap-check");
    assert_eq!(
        first, second,
        "two builds from the same tag must produce byte-identical artifacts"
    );
    let artifacts = rig.release_artifacts("v0.0.0-gap-check");
    for expected in ["sbom.cdx.json", "provenance.intoto.jsonl"] {
        assert!(
            artifacts.iter().any(|name| name == expected),
            "the release job must publish {expected} beside the artifact, got {artifacts:?}"
        );
    }
}

/// REQ: SEC-725
#[test]
fn sec_725_update_from_the_original_registry_is_not_refused_as_source_changed() {
    assert_manual_says(
        "reference-trust-and-supply-chain",
        "the host MUST use the registry the plugin was installed from",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.serve_registry_plugin("default", NOTES, "1.1.0");
    let output = rig.cli(&["update", NOTES]);
    assert_eq!(
        output.exit_code, 0,
        "an update from the installing registry needs no switch"
    );
    assert!(
        !error_codes(&output)
            .iter()
            .any(|code| code == "registry.source-changed"),
        "the installing registry is not reported as changed"
    );
}

/// REQ: SEC-726
#[test]
fn sec_726_clean_upload_is_listed_at_once_without_a_review_hold() {
    assert_manual_says(
        "reference-trust-and-supply-chain",
        "MUST hold a version that matches a rule in state `pending-review`",
    );
    let rig = rig();
    let package = rig.write_package(&pkg(NOTES, "1.0.0"));
    let package_text = package.to_string_lossy().into_owned();
    let output = rig.cli(&[
        "registry",
        "publish",
        &package_text,
        "--account",
        "acme-publisher",
        "--totp",
        "123456",
    ]);
    assert_eq!(output.exit_code, 0, "the clean upload is accepted");
    assert!(
        rig.registry_metadata_visible_versions(NOTES)
            .contains(&"1.0.0".to_string()),
        "a clean version is visible to clients without a review hold"
    );
}

/// REQ: SEC-727
#[test]
fn sec_727_release_job_publishes_at_least_three_artifacts_for_a_tag() {
    assert_manual_says(
        "reference-trust-and-supply-chain",
        "the release job MUST publish the SBOM (CycloneDX) and the SLSA provenance attestation",
    );
    let rig = rig();
    let (first, _second) = rig.build_release_twice("v0.0.0-gap-check-2");
    assert!(!first.is_empty(), "the release build produces bytes");
    assert!(
        rig.release_artifacts("v0.0.0-gap-check-2").len() >= 3,
        "the release job publishes the artifact, its SBOM and its provenance"
    );
}
