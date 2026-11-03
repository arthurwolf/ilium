//! publish-registry-operator-guide.md "Metadata documents, expiry and accounts" (PUBL-026..PUBL-032).
//!
//! Metadata documents are read from the standard fixture registry, which follows the manual's
//! signed-metadata layout (`index.json`, `snapshot.json`, `timestamp.json`, `root.json`, ...).
//! Operator status and checks are driven through the `registry` command of the sandbox.

use crate::common::*;

fn scan_for(dir: &std::path::Path, needle: &str, hits: &mut Vec<String>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan_for(&path, needle, hits);
            } else if let Ok(text) = std::fs::read_to_string(&path) {
                if text.contains(needle) {
                    hits.push(path.display().to_string());
                }
            }
        }
    }
}

// ---- PUBL-026 every table file served at its path with the right type ----------------------

/// REQ: PUBL-026
#[test]
fn publ_026_each_table_file_is_served_at_its_path_a() {
    let sandbox = Sandbox::standard();
    let served = sandbox.net.paths(BASE);
    for name in ["root.json", "timestamp.json", "snapshot.json", "index.json", "revocations.json"] {
        assert!(served.iter().any(|path| path.ends_with(name)), "{name} is not served: {served:?}");
    }
    assert!(BASE.starts_with("https://"), "the registry must be served over HTTPS");
}

/// REQ: PUBL-026
#[test]
fn publ_026_signed_type_matches_the_file_role_b() {
    let tree = RegistryBuilder::standard().build();
    for (file, role) in [
        ("index.json", "index"),
        ("snapshot.json", "snapshot"),
        ("timestamp.json", "timestamp"),
        ("root.json", "root"),
        ("revocations.json", "revocations"),
    ] {
        let signed = signed_of(&tree[file]);
        assert_eq!(signed["type"], role, "{file} must carry signed.type {role}");
    }
}

// ---- PUBL-027 timestamp lifetime 24 hours and re-signed every 12 hours --------------------

/// REQ: PUBL-027
#[test]
fn publ_027_timestamp_expires_24_hours_after_signing_a() {
    let tree = RegistryBuilder::standard().build();
    let signed = signed_of(&tree["timestamp.json"]);
    assert_eq!(signed["expires"].as_str(), Some(at(86_400).as_str()));
}

/// REQ: PUBL-027
#[test]
fn publ_027_registry_resigns_timestamp_every_twelve_hours_b() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run_at(now_plus(12 * 3600), &["registry", "status", "--json"]);
    assert!(
        output_contains(&output, "timestamp_resigned"),
        "status must report the timestamp re-signing: {output:?}"
    );
}

// ---- PUBL-028 snapshot expires no sooner than the timestamp, 7 days ----------------------

/// REQ: PUBL-028
#[test]
fn publ_028_snapshot_expiry_is_not_before_timestamp_expiry_a() {
    let tree = RegistryBuilder::standard().build();
    let snapshot_expires = signed_of(&tree["snapshot.json"])["expires"].as_str().unwrap_or_default().to_owned();
    let timestamp_expires = signed_of(&tree["timestamp.json"])["expires"].as_str().unwrap_or_default().to_owned();
    assert!(snapshot_expires >= timestamp_expires, "{snapshot_expires} < {timestamp_expires}");
}

/// REQ: PUBL-028
#[test]
fn publ_028_snapshot_expires_seven_days_after_signing_b() {
    let tree = RegistryBuilder::standard().build();
    let signed = signed_of(&tree["snapshot.json"]);
    assert_eq!(signed["expires"].as_str(), Some(at(7 * 86_400).as_str()));
}

// ---- PUBL-029 root lifetime and rotation 30 days before expiry ----------------------------

/// REQ: PUBL-029
#[test]
fn publ_029_root_expires_at_least_365_days_after_signing_a() {
    let tree = RegistryBuilder::standard().build();
    let expires = signed_of(&tree["root.json"])["expires"].as_str().unwrap_or_default().to_owned();
    assert!(expires >= at(365 * 86_400), "root.json expires too early: {expires}");
}

/// REQ: PUBL-029
#[test]
fn publ_029_check_fails_when_root_has_under_30_days_left_b() {
    let sandbox = Sandbox::standard();
    let days_335 = now_plus(335 * 86_400);
    let output = sandbox.run_at(days_335, &["registry", "check"]);
    assert_ne!(output.exit_code, 0, "under 30 days of root left must fail the check: {output:?}");
}

// ---- PUBL-030 machine-readable expiry status and operator alerts --------------------------

/// REQ: PUBL-030
#[test]
fn publ_030_status_exposes_timestamp_and_root_expiry_a() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run(&["registry", "status", "--json"]);
    let record = result_record(&output).expect("status prints a result record");
    assert!(record["timestamp_expires"].is_string(), "{output:?}");
    assert!(record["root_expires"].is_string(), "{output:?}");
}

/// REQ: PUBL-030
#[test]
fn publ_030_operator_alerted_under_six_hours_to_timestamp_expiry_b() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run_at(now_plus(19 * 3600), &["registry", "status", "--json"]);
    assert!(!records(&output, "alert").is_empty(), "an alert must fire under 6 hours: {output:?}");
}

// ---- PUBL-031 no token, Authorization header or second factor in logs --------------------

/// REQ: PUBL-031
#[test]
fn publ_031_publish_token_absent_from_registry_logs_a() {
    let sandbox = Sandbox::standard();
    let file = sandbox.project_file("tok.iliumpkg", &signed_package("com.acme.tokenlog", "1.0.0"));
    let token = "ilium-pat-LOG-LEAK-CHECK-0031";
    let _ = sandbox.run_with_vars(
        &[("ILIUM_PUBLISH_TOKEN", token)],
        &["publish", file.to_str().expect("utf-8 path"), "--registry", BASE],
    );
    let mut hits = Vec::new();
    scan_for(&sandbox.dir.path().join("data"), token, &mut hits);
    assert!(hits.is_empty(), "the token appears in logs: {hits:?}");
}

/// REQ: PUBL-031
#[test]
fn publ_031_authorization_header_and_second_factor_absent_from_logs_b() {
    let sandbox = Sandbox::standard();
    let _ = sandbox.run(&["registry", "login", "--second-factor", "123456"]);
    let mut hits = Vec::new();
    scan_for(&sandbox.dir.path().join("data"), "Authorization:", &mut hits);
    scan_for(&sandbox.dir.path().join("data"), "123456", &mut hits);
    assert!(hits.is_empty(), "authorization or second factor logged: {hits:?}");
}

// ---- PUBL-032 two maintainers with second factors ----------------------------------------

/// REQ: PUBL-032
#[test]
fn publ_032_publisher_with_two_maintainers_has_no_warning_a() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run(&["registry", "publisher", PUBLISHER_ID]);
    assert_ok(&output);
    assert!(!output_contains(&output, "No second maintainer"), "{output:?}");
}

/// REQ: PUBL-032
#[test]
fn publ_032_single_maintainer_publisher_shows_warning_b() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run(&["registry", "publisher", "com.solo"]);
    assert!(
        output_contains(&output, "No second maintainer"),
        "a publisher with one maintainer must show the warning: {output:?}"
    );
}
