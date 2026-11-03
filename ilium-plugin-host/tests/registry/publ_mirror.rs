//! publish-mirror-runbook.md "Mirror operation" (PUBL-017..PUBL-025).
//!
//! Mirror sync is driven through `mirror` on a sandbox whose source registry is the standard
//! fixture; the mirror tree is read from the sandbox folder `mirror`.

use crate::common::*;

fn mirror_tree(sandbox: &Sandbox) -> Tree {
    sandbox.snapshot_of("mirror")
}

fn temp_files_under(dir: &std::path::Path, found: &mut Vec<String>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                temp_files_under(&path, found);
            } else if path.extension().is_some_and(|ext| ext == "part" || ext == "tmp") {
                found.push(path.display().to_string());
            }
        }
    }
}

// ---- PUBL-017 sync interval is at most half the shortest expiry ---------------------------

/// REQ: PUBL-017
#[test]
fn publ_017_sync_interval_longer_than_half_expiry_is_refused_a() {
    let sandbox = Sandbox::standard();
    // The timestamp expires after 24 hours, so a 13 hour interval is too slow.
    sandbox.set_registry_setting("sync_interval_seconds", toml::Value::Integer(13 * 3600));
    let output = sandbox.run(&["mirror", BASE]);
    assert_ne!(output.exit_code, 0, "a sync interval over half the expiry must be refused: {output:?}");
}

/// REQ: PUBL-017
#[test]
fn publ_017_twelve_hour_interval_is_accepted_b() {
    let sandbox = Sandbox::standard();
    sandbox.set_registry_setting("sync_interval_seconds", toml::Value::Integer(12 * 3600));
    let output = sandbox.run(&["mirror", BASE]);
    assert_ok(&output);
}

// ---- PUBL-018 alert before the timestamp expires -----------------------------------------

/// REQ: PUBL-018
#[test]
fn publ_018_alert_fires_six_hours_before_timestamp_expiry_a() {
    let sandbox = Sandbox::standard();
    assert_ok(&sandbox.run(&["mirror", BASE]));
    // Run the check 19 hours after the fixture time: under 6 hours remain on a 24 hour timestamp.
    let output = sandbox.run_at(now_plus(19 * 3600), &["mirror", BASE]);
    assert!(
        !records(&output, "alert").is_empty(),
        "an alert must fire when timestamp.json expires in under 6 hours: {output:?}"
    );
}

/// REQ: PUBL-018
#[test]
fn publ_018_no_alert_while_expiry_is_far_b() {
    let sandbox = Sandbox::standard();
    assert_ok(&sandbox.run(&["mirror", BASE]));
    let output = sandbox.run(&["mirror", BASE]);
    assert!(records(&output, "alert").is_empty(), "no alert while far from expiry: {output:?}");
}

// ---- PUBL-019 failed syncs keep old files and replace atomically --------------------------

/// REQ: PUBL-019
#[test]
fn publ_019_failed_sync_leaves_previous_files_in_place_a() {
    let sandbox = Sandbox::standard();
    assert_ok(&sandbox.run(&["mirror", BASE]));
    let before = mirror_tree(&sandbox);
    sandbox.net.set_down(true);
    let output = sandbox.run(&["mirror", BASE]);
    sandbox.net.set_down(false);
    assert_ne!(output.exit_code, 0, "{output:?}");
    assert_eq!(before, mirror_tree(&sandbox), "a failed sync must keep the previous files");
}

/// REQ: PUBL-019
#[test]
fn publ_019_no_partial_files_remain_after_a_killed_sync_b() {
    let sandbox = Sandbox::standard();
    assert_ok(&sandbox.run(&["mirror", BASE]));
    sandbox.net.set_down(true);
    let _ = sandbox.run(&["mirror", BASE]);
    sandbox.net.set_down(false);
    let mut found = Vec::new();
    temp_files_under(&sandbox.dir.path().join("data"), &mut found);
    assert!(found.is_empty(), "partial files left behind: {found:?}");
}

// ---- PUBL-020 full mirrors are byte-identical to the source --------------------------------

/// REQ: PUBL-020
#[test]
fn publ_020_full_mirror_files_match_source_hashes_a() {
    let sandbox = Sandbox::standard();
    assert_ok(&sandbox.run(&["mirror", BASE]));
    let source = sandbox.snapshot();
    let mirrored = mirror_tree(&sandbox);
    for (name, bytes) in &source {
        let copy = mirrored.get(name).unwrap_or_else(|| panic!("mirror misses {name}"));
        assert_eq!(sha256_hex(bytes), sha256_hex(copy), "{name} changed on the mirror");
    }
}

/// REQ: PUBL-020
#[test]
fn publ_020_full_mirror_has_no_extra_or_missing_files_b() {
    let sandbox = Sandbox::standard();
    assert_ok(&sandbox.run(&["mirror", BASE]));
    let source_names: Vec<String> = sandbox.snapshot().keys().cloned().collect();
    let mirror_names: Vec<String> = mirror_tree(&sandbox).keys().cloned().collect();
    assert_eq!(source_names, mirror_names);
}

// ---- PUBL-021 partial mirrors use their own keys ------------------------------------------

/// REQ: PUBL-021
#[test]
fn publ_021_partial_mirror_publishes_its_own_root_a() {
    let sandbox = Sandbox::standard();
    assert_ok(&sandbox.run(&["mirror", BASE, "--partial"]));
    let source_root = sandbox.snapshot()["root.json"].clone();
    let mirror_root = mirror_tree(&sandbox)["root.json"].clone();
    assert!(!mirror_root.is_empty(), "a partial mirror must publish its own root.json");
    assert_ne!(source_root, mirror_root, "the partial mirror must not reuse the source root");
}

/// REQ: PUBL-021
#[test]
fn publ_021_partial_mirror_resigns_index_and_snapshot_b() {
    let sandbox = Sandbox::standard();
    assert_ok(&sandbox.run(&["mirror", BASE, "--partial"]));
    let source = sandbox.snapshot();
    let mirrored = mirror_tree(&sandbox);
    assert_ne!(source["index.json"], mirrored["index.json"], "index.json must be re-signed");
    assert_ne!(source["snapshot.json"], mirrored["snapshot.json"], "snapshot.json must be re-signed");
}

// ---- PUBL-022 takedowns reach mirrors within 6 hours --------------------------------------

/// REQ: PUBL-022
#[test]
fn publ_022_takedown_is_applied_to_mirror_within_six_hours_a() {
    let sandbox = Sandbox::standard();
    assert_ok(&sandbox.run(&["mirror", BASE]));
    assert_ok(&sandbox.run(&["takedown", "com.acme.notes@0.3.0", "--reason", "malware"]));
    let output = sandbox.run_at(now_plus(6 * 3600 - 60), &["mirror", BASE]);
    assert_ok(&output);
    let revocations = signed_of(&mirror_tree(&sandbox)["revocations.json"]).to_string();
    assert!(revocations.contains("com.acme.notes"), "the mirror must carry the takedown");
}

/// REQ: PUBL-022
#[test]
fn publ_022_late_takedown_is_reported_as_breach_b() {
    let sandbox = Sandbox::standard();
    assert_ok(&sandbox.run(&["mirror", BASE]));
    assert_ok(&sandbox.run(&["takedown", "com.acme.notes@0.3.0", "--reason", "malware"]));
    let output = sandbox.run_at(now_plus(7 * 3600), &["mirror", BASE]);
    assert!(
        !records(&output, "alert").is_empty() || !warning_codes(&output).is_empty(),
        "a takedown not applied within 6 hours must raise an alert: {output:?}"
    );
}

// ---- PUBL-023 mirrors never remove blobs on their own ------------------------------------

/// REQ: PUBL-023
#[test]
fn publ_023_mirror_keeps_blob_removed_at_source_without_signed_change_a() {
    let sandbox = Sandbox::standard();
    assert_ok(&sandbox.run(&["mirror", BASE]));
    let blob = RegistryBuilder::standard().blob_path("com.acme.notes", "0.3.0");
    sandbox.net.remove_file(&format!("{BASE}{blob}"));
    assert_ok(&sandbox.run(&["mirror", BASE]));
    assert!(
        mirror_tree(&sandbox).keys().any(|name| name.ends_with(&blob)),
        "a full mirror must keep a blob unless a takedown or signed change removes it"
    );
}

/// REQ: PUBL-023
#[test]
fn publ_023_removal_follows_signed_takedown_b() {
    let sandbox = Sandbox::standard();
    assert_ok(&sandbox.run(&["mirror", BASE]));
    assert_ok(&sandbox.run(&["takedown", "com.acme.notes@0.3.0", "--reason", "malware"]));
    assert_ok(&sandbox.run_at(now_plus(3600), &["mirror", BASE]));
    let blob = RegistryBuilder::standard().blob_path("com.acme.notes", "0.3.0");
    assert!(
        !mirror_tree(&sandbox).keys().any(|name| name.ends_with(&blob)),
        "after a signed takedown the mirror removes the blob"
    );
}

// ---- PUBL-024 free space of twice the source blobs ----------------------------------------

/// REQ: PUBL-024
#[test]
fn publ_024_sync_refused_when_free_space_is_below_twice_blobs_a() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run_with_vars(
        &[("ILIUM_TEST_FREE_BYTES", "1024")],
        &["mirror", BASE],
    );
    assert_ne!(output.exit_code, 0, "a sync must refuse with too little free space: {output:?}");
}

/// REQ: PUBL-024
#[test]
fn publ_024_sync_runs_with_twice_blob_size_free_b() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run_with_vars(
        &[("ILIUM_TEST_FREE_BYTES", "18446744073709551615")],
        &["mirror", BASE],
    );
    assert_ok(&output);
}

// ---- PUBL-025 mirror on its own filesystem ------------------------------------------------

/// REQ: PUBL-025
#[test]
fn publ_025_mirror_inside_build_cache_root_is_refused_a() {
    let sandbox = Sandbox::standard();
    let cache = sandbox.dir.path().join("build-cache");
    std::fs::create_dir_all(&cache).expect("create build cache folder");
    let output = sandbox.run(&["mirror", BASE, "--dir", cache.to_str().expect("utf-8 path"), "--cache-root", cache.to_str().expect("utf-8 path")]);
    assert_ne!(output.exit_code, 0, "a mirror on the build cache disk must be refused: {output:?}");
}

/// REQ: PUBL-025
#[test]
fn publ_025_mirror_on_own_filesystem_is_accepted_b() {
    let sandbox = Sandbox::standard();
    let mirror_dir = sandbox.dir.path().join("mirror-own-disk");
    let output = sandbox.run(&["mirror", BASE, "--dir", mirror_dir.to_str().expect("utf-8 path")]);
    assert_ok(&output);
}
