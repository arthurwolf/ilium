//! Manual page `reference-plugin-files`: disk usage, backup and restore archives, cross-platform
//! restore, network filesystems, case-folding path collisions, Windows path length, symlinks, the
//! relocated data directory, cache caps, purge records and wall-clock independence
//! (HST-651, 652, 653, 654, 655, 665, 666, 667, 668, 669, 672, 673, 675, 690).

use std::time::{Duration, Instant};

use super::common::{install_ok, pkg, rig, NOTES, OTHER};

/// REQ: HST-651
#[test]
fn hst_651_du_reports_bytes_per_category_within_5_s_for_100_plugins_and_sums_to_disk_size() {
    let rig = rig();
    rig.install_many(100);
    rig.write_known_bytes(NOTES, "data/known.bin", &[7u8; 1_234]);
    let started = Instant::now();
    let output = rig.cli(&["plugin", "du", "--json"]);
    assert!(
        started.elapsed() <= Duration::from_secs(5),
        "du for 100 plugins within 5 s"
    );
    let report = output.stdout_jsonl.last().expect("du report").clone();
    let reported_total: u64 = report["plugins"]
        .as_array()
        .expect("plugins array")
        .iter()
        .map(|row| row["bytes"].as_u64().expect("integer bytes"))
        .sum();
    assert_eq!(
        reported_total,
        rig.on_disk_size_of_plugin_roots(),
        "sum equals the on-disk size"
    );
    assert!(
        report["plugins"][0]["data"].is_u64(),
        "bytes per category as integers"
    );
}

/// REQ: HST-652
#[test]
fn hst_652_backup_is_consistent_excludes_cache_and_logs_and_writes_partial_then_renames() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let archive = rig.path_in_scratch("backup.tar.zst");
    let output = rig.cli(&["plugin", "backup", archive.to_str().expect("utf-8 path")]);
    assert_eq!(
        output.exit_code, 0,
        "backup exits 0 after the archive is verified readable"
    );
    assert!(archive.exists(), "renamed from <file>.partial on success");
    assert!(
        !rig.path_in_scratch("backup.tar.zst.partial").exists(),
        "partial file removed"
    );
    let entries = rig.archive_entries(&archive);
    assert!(
        entries.iter().all(|name| !name.contains("cache")
            && !name.contains("logs")
            && !name.contains("staging")),
        "cache, logs and staging excluded"
    );
    assert!(
        entries.iter().all(|name| !name.contains("secrets")),
        "secrets not included without --include-secrets"
    );
}

/// REQ: HST-653
#[test]
fn hst_653_restore_validates_hashes_before_changing_anything_and_refuses_a_running_server() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let archive = rig.make_backup_archive();
    let before = rig.snapshot_plugin_tree();
    rig.corrupt_archive_entry(&archive, "data/known.bin");
    let rejected = rig.cli(&["plugin", "restore", archive.to_str().expect("utf-8 path")]);
    assert_ne!(
        rejected.exit_code, 0,
        "a bad entry hash refuses the restore"
    );
    assert_eq!(
        rig.snapshot_plugin_tree(),
        before,
        "nothing changed before validation passed"
    );
    let server = rig.start_server("default");
    let running = rig.cli(&["plugin", "restore", archive.to_str().expect("utf-8 path")]);
    assert!(
        running.stderr.contains("restore.server-running"),
        "a running session server is refused"
    );
    server.shutdown();
}

/// REQ: HST-654
#[test]
fn hst_654_archive_manifest_declares_format_and_restore_rejects_unknown_or_newer_hosts() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let archive = rig.make_backup_archive();
    let manifest = rig.archive_json_entry(&archive, "backup.json");
    assert_eq!(manifest["format"], "ilium-backup/1", "format declared");
    for field in ["host_version", "created_at", "os"] {
        assert!(manifest.get(field).is_some(), "manifest has {field}");
    }
    assert!(
        manifest["created_at"]
            .as_str()
            .expect("time")
            .ends_with('Z'),
        "creation time UTC ISO 8601"
    );
    rig.rewrite_archive_format(&archive, "ilium-backup/9");
    let unknown = rig.cli(&["plugin", "restore", archive.to_str().expect("utf-8 path")]);
    assert!(
        unknown.stderr.contains("restore.format-unsupported"),
        "unknown major format refused"
    );
    rig.rewrite_archive_host_version(&archive, "99.0.0");
    let newer = rig.cli(&["plugin", "restore", archive.to_str().expect("utf-8 path")]);
    assert!(
        newer.stderr.contains("restore.host-newer"),
        "newer host archive refused without --force"
    );
}

/// REQ: HST-655
#[test]
fn hst_655_cross_os_restore_rewrites_no_data_bytes_and_refuses_unsupported_platform_plugins() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let archive = rig.make_backup_archive_from_os("linux");
    let data_before = rig.plugin_data_bytes(NOTES);
    let result = rig.cli(&[
        "plugin",
        "restore",
        archive.to_str().expect("utf-8 path"),
        "--merge",
    ]);
    assert_eq!(
        rig.plugin_data_bytes(NOTES),
        data_before,
        "no plugin data bytes rewritten"
    );
    assert!(
        rig.installed_json_separators_are_normalised(),
        "path separators normalised in installed.json"
    );
    let refused = result
        .stdout_jsonl
        .iter()
        .find(|row| row["plugin"] == NOTES)
        .expect("plugin listed in the restore result");
    assert_eq!(
        refused["state"], "refused-at-load",
        "unsupported T2/T3 plugin refused at load"
    );
    assert_eq!(
        refused["code"], "package.platform-unsupported",
        "with the platform code"
    );
}

/// REQ: HST-665
#[test]
fn hst_665_network_filesystem_is_logged_once_and_falls_back_to_owner_lock_or_read_only() {
    let rig = rig();
    rig.mount_as_network_filesystem("data");
    let server = rig.start_server("default");
    assert_eq!(
        server.log_lines_with_code("data.network-fs").len(),
        1,
        "logged once at start"
    );
    let lock = rig.lock_file_text(NOTES);
    assert!(
        lock.contains("owner") && lock.contains("boot_id"),
        "lock records owner id and boot id"
    );
    rig.make_lock_scheme_unavailable();
    let restart = rig.start_server("default");
    assert!(
        rig.is_read_only(&restart),
        "no guaranteed lock scheme starts in read-only mode"
    );
}

/// REQ: HST-666
#[test]
fn hst_666_ids_fold_to_lowercase_store_paths_and_case_colliding_packages_are_rejected() {
    let rig = rig();
    install_ok(&rig, &pkg("com.example.notes", "1.0.0"));
    let store = rig.store_path_for("com.example.notes");
    assert!(
        store
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-/".contains(c)),
        "store path uses [a-z0-9._-]"
    );
    let colliding = rig.write_package(
        &pkg("com.example.notes", "1.0.1")
            .with_file("Readme.md")
            .with_file("README.md"),
    );
    let output = rig.cli(&[
        "install",
        colliding.to_str().expect("utf-8 path"),
        "--yes",
        "--allow-unsigned",
    ]);
    assert_eq!(output.exit_code, 1, "a case-colliding package is refused");
    assert!(
        output.stderr.contains("package.path-collision"),
        "refusal names package.path-collision"
    );
}

/// REQ: HST-667
#[test]
fn hst_667_windows_uses_extended_length_paths_and_refuses_packages_with_long_relative_paths() {
    let rig = rig();
    rig.pretend_windows_host();
    assert!(
        rig.every_plugin_root_path_is_extended_length(),
        "every path under plugin roots has the \\\\?\\ prefix"
    );
    let long_name = "a".repeat(241);
    let package = rig.write_package(&pkg(NOTES, "1.0.0").with_file(&long_name));
    let output = rig.cli(&[
        "install",
        package.to_str().expect("utf-8 path"),
        "--yes",
        "--allow-unsigned",
    ]);
    assert!(
        output.stderr.contains("package.path-too-long"),
        "longer than 240 bytes refused, naming the file"
    );
    let fine = rig.write_package(&pkg(OTHER, "1.0.0").with_file(&"b".repeat(240)));
    assert_eq!(
        rig.cli(&[
            "install",
            fine.to_str().expect("utf-8 path"),
            "--yes",
            "--allow-unsigned"
        ])
        .exit_code,
        0,
        "240 bytes accepted"
    );
}

/// REQ: HST-668
#[test]
fn hst_668_symlinks_inside_plugin_roots_are_refused_and_logged_while_a_root_link_owned_by_user_is_allowed(
) {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_symlink_inside_data(NOTES, "data/link.bin");
    let error = rig
        .read_plugin_file(NOTES, "data/link.bin")
        .expect_err("symlink refused");
    assert_eq!(error.code.as_str(), "io");
    assert!(error.message.contains("symlink"), "message names symlink");
    assert_eq!(
        rig.log_lines_with_code("fs.symlink-refused").len(),
        1,
        "fs.symlink-refused logged with the path"
    );
    rig.point_plugins_root_at_other_disk_owned_by_user();
    assert!(
        rig.start_server("default").is_running(),
        "a root symlink owned by the current user is allowed"
    );
}

/// REQ: HST-669
#[test]
fn hst_669_data_dir_override_is_validated_and_refuses_bad_roots_without_moving_data() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let target = rig.path_in_scratch("elsewhere");
    std::fs::create_dir_all(&target).expect("target created");
    rig.set_env(
        "ILIUM_PLUGIN_DATA_DIR",
        target.to_str().expect("utf-8 path"),
    );
    let bad = rig.start_server("default");
    assert!(
        !bad.is_running(),
        "a target not mode 0700 refuses to start plugins"
    );
    assert_eq!(
        bad.log_lines_with_code("data.root-invalid").len(),
        1,
        "refusal logged as data.root-invalid"
    );
    std::fs::set_permissions(&target, std::os::unix::fs::PermissionsExt::from_mode(0o700))
        .expect("mode set");
    rig.set_env(
        "ILIUM_PLUGIN_DATA_DIR",
        target.to_str().expect("utf-8 path"),
    );
    let server = rig.start_server("default");
    assert!(server.is_running(), "valid 0700 target accepted");
    assert!(
        !rig.existing_data_moved_automatically(),
        "changing the path never moves existing data"
    );
}

/// REQ: HST-672
#[test]
fn hst_672_compile_cache_is_capped_at_1024_mib_evicts_lru_not_in_use_and_writes_atomically() {
    let rig = rig();
    assert_eq!(
        rig.compile_cache_cap_mib(),
        1024,
        "plugins.compile_cache_mib default 1024"
    );
    rig.fill_compile_cache_with_in_use_entry();
    assert!(
        rig.compile_cache_bytes() <= 1024 * 1024 * 1024,
        "LRU eviction keeps the cap"
    );
    assert!(
        rig.in_use_entry_survived(),
        "an entry in use is never evicted"
    );
    rig.crash_during_cache_write();
    assert!(
        rig.truncated_cache_entry_fails_sha256_and_is_rebuilt(),
        "truncated entry fails its SHA-256 and is rebuilt"
    );
}

/// REQ: HST-673
#[test]
fn hst_673_plugin_cache_quota_fails_writes_with_cache_full_and_evicts_whole_files_oldest_first() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.set_config_value("cache_mib", "64");
    let result = rig.write_plugin_cache(NOTES, 65 * 1024 * 1024);
    let error = result.expect_err("write over cache_mib refused");
    assert_eq!(error.code.as_str(), "quota-exceeded");
    assert!(error.message.contains("cache-full"), "message cache-full");
    rig.make_cache_volume_low_on_free_space(256 * 1024 * 1024);
    rig.write_plugin_cache(NOTES, 1024)
        .expect("small write accepted");
    assert!(
        rig.cache_evict_count() > 0,
        "cache.evict logged with the count of whole files evicted"
    );
}

/// REQ: HST-675
#[test]
fn hst_675_purged_json_entries_are_kept_365_days_and_applied_on_next_project_open() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.purge_plugin_in_project(NOTES);
    assert_eq!(
        rig.purged_json_retention_days(),
        365,
        "entries kept for 365 days"
    );
    rig.open_project_again();
    assert!(
        !rig.project_bucket_exists(NOTES),
        "the bucket and settings are deleted on the next open"
    );
    assert_eq!(
        rig.log_lines_with_code("purge.applied").len(),
        1,
        "purge.applied logged"
    );
    rig.leave_orphan_project_data("com.example.gone");
    let doctor = rig.cli(&["plugin", "doctor", "--json"]);
    assert!(
        doctor.stdout_raw.contains("orphaned"),
        "orphaned project data listed by doctor with its size"
    );
    assert!(
        rig.orphan_data_still_present("com.example.gone"),
        "orphaned data is never deleted automatically"
    );
}

/// REQ: HST-690
#[test]
fn hst_690_wall_clock_moving_backwards_never_shortens_retention_or_lock_staleness() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.create_backup_file_aged_days("old-backup.tar.zst", 20);
    rig.set_wall_clock_back_days(30);
    rig.advance_retention_sweep();
    assert!(
        rig.path_in_scratch("old-backup.tar.zst").exists(),
        "backup not deleted earlier than its stated age"
    );
    rig.use_monotonic_for_lock_staleness(NOTES);
    assert!(
        !rig.lock_stale_by_wall_clock_only(),
        "lock staleness uses a monotonic clock"
    );
}
