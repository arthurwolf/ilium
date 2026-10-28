//! Core-plugin gap tests for `reference-config-and-data-migration`: a failed migration keeps the
//! stored bytes, and the next start retries it.

use ilium_plugin_host::testing::ops::{OpsRig, PackageSpec};

const NOTES: &str = "com.example.notes";
const SECTION: &str = "section.json";

fn install_ok(rig: &OpsRig, spec: &PackageSpec) {
    let path = rig.write_package(spec);
    let path_text = path.to_string_lossy().into_owned();
    let output = rig.cli(&["install", &path_text, "--yes", "--allow-unsigned"]);
    assert_eq!(
        output.exit_code, 0,
        "install failed: {:?}",
        output.stdout_jsonl
    );
}

/// REQ: COR-096
#[test]
fn cor_096_a_failed_migration_keeps_the_old_bytes_and_the_next_start_retries_it() {
    let rig = OpsRig::new();
    let old_bytes = br#"{"layout":"v1"}"#.to_vec();
    install_ok(&rig, &PackageSpec::new(NOTES, "1.0.0").data_version(1));
    let first = rig.start_server("s1");
    first.write_data_file(NOTES, SECTION, &old_bytes);
    first.kill_sigkill();

    install_ok(
        &rig,
        &PackageSpec::new(NOTES, "2.0.0")
            .data_version(2)
            .migrator_both(),
    );
    rig.fail_next_migration(NOTES);
    let failing = rig.start_server("s2");
    assert_eq!(
        failing.read_data_file(NOTES, SECTION).as_deref(),
        Some(old_bytes.as_slice()),
        "the old bytes must survive a failed migration"
    );
    assert_eq!(
        rig.bucket_stamp(NOTES),
        Some(1),
        "the stamp must stay at version 1"
    );
    failing.kill_sigkill();

    let retried = rig.start_server("s3");
    assert_eq!(
        rig.bucket_stamp(NOTES),
        Some(2),
        "the next start must run the migration again and stamp version 2"
    );
    assert!(
        rig.trace().iter().any(|line| line.contains("migrate")),
        "the next start must call the migration"
    );
    retried.shutdown();
}

/// REQ: COR-096
#[test]
fn cor_096_a_successful_migration_replaces_the_old_bytes_and_stamps_the_new_version() {
    let rig = OpsRig::new();
    let old_bytes = br#"{"layout":"v1"}"#.to_vec();
    install_ok(&rig, &PackageSpec::new(NOTES, "1.0.0").data_version(1));
    let first = rig.start_server("s1");
    first.write_data_file(NOTES, SECTION, &old_bytes);
    first.kill_sigkill();

    install_ok(
        &rig,
        &PackageSpec::new(NOTES, "2.0.0")
            .data_version(2)
            .migrator_both(),
    );
    let migrated = rig.start_server("s2");
    assert_eq!(
        rig.bucket_stamp(NOTES),
        Some(2),
        "a successful migration must stamp version 2"
    );
    assert_ne!(
        migrated.read_data_file(NOTES, SECTION).as_deref(),
        Some(old_bytes.as_slice()),
        "the stored bytes must be replaced by the migration result"
    );
    migrated.shutdown();
}
