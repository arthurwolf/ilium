//! Manual page `reference-offline-and-low-disk`: offline update checks, offline verify and install,
//! interrupted downloads, integrity checks, unreadable registry recovery, the recovery page, disk
//! pressure steps, the disk-low event, read-only data folders and writable recovery
//! (HST-657, 659, 660, 661, 662, 663, 664, 693, 694, 695).

use std::time::Duration;

use super::common::{install_ok, pkg, rig, NOTES};

/// REQ: HST-657
#[test]
fn hst_657_storage_integrity_check_is_bounded_and_does_not_block_other_plugins() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.grow_storage_database_mib(NOTES, 16);
    let started = std::time::Instant::now();
    rig.open_storage_database(NOTES);
    assert!(
        started.elapsed() <= Duration::from_secs(32),
        "bounded to 2 s per MiB-class database"
    );
    rig.hang_integrity_check(NOTES);
    let other_start = rig.start_other_plugin_while_checking(NOTES);
    assert!(
        other_start.is_ok(),
        "other plugins start while the check runs"
    );
    assert!(
        rig.database_moved_aside_after_check_timeout(NOTES),
        "not finished in 10 s is treated as corruption"
    );
}

/// REQ: HST-659
#[test]
fn hst_659_unreadable_installed_json_is_never_overwritten_and_rebuilds_disabled_candidates() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.corrupt_file(&rig.plugins_root().join("installed.json"), b"{ not json");
    rig.start_server("default");
    assert_eq!(
        rig.read_file_text(&rig.plugins_root().join("installed.json")),
        "{ not json",
        "the damaged file is not overwritten"
    );
    assert!(
        rig.corrupt_copy_exists("installed.json.corrupt-"),
        "damaged copy kept as installed.json.corrupt-<time>"
    );
    let candidate = rig.rebuilt_candidate_registry();
    assert!(
        candidate
            .iter()
            .any(|entry| entry.id == NOTES && !entry.enabled),
        "rebuilt plugins start disabled"
    );
    assert!(
        rig.recovery_page_offers_rebuild(),
        "Rebuild action offered on the recovery page"
    );
}

/// REQ: HST-660
#[test]
fn hst_660_recovery_page_is_reachable_from_cli_and_lists_damaged_items_with_exit_78() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.corrupt_file(&rig.plugins_root().join("installed.json"), b"{ not json");
    rig.disable_plugin_platform_entirely();
    let output = rig.cli(&["plugin", "recovery"]);
    assert_eq!(
        output.exit_code, 78,
        "unresolved items exit 78 from the CLI"
    );
    assert!(
        output.stdout_raw.contains("installed.json"),
        "each damaged item listed with its path"
    );
    for action in ["Rebuild", "Restore from backup", "Start without"] {
        assert!(
            output.stdout_raw.contains(action),
            "action {action} available"
        );
    }
}

/// REQ: HST-661
#[test]
fn hst_661_free_space_steps_evict_then_shrink_logs_then_block_installs_and_undo_with_hysteresis() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.set_free_bytes(500 * 1024 * 1024);
    rig.advance_clock(Duration::from_secs(30));
    assert_eq!(
        rig.log_lines_with_code("disk.pressure.evict-caches").len(),
        1,
        "step one logged once"
    );
    rig.set_free_bytes(120 * 1024 * 1024);
    rig.advance_clock(Duration::from_secs(30));
    assert_eq!(
        rig.log_rotation_limit_mib(),
        1,
        "log rotation shrunk to 1 MiB x 2 below 128 MiB"
    );
    rig.set_free_bytes(60 * 1024 * 1024);
    rig.advance_clock(Duration::from_secs(30));
    let package = rig.write_package(&pkg(NOTES, "1.0.1"));
    let blocked = rig.cli(&["install", package.to_str().expect("utf-8 path"), "--yes"]);
    assert_ne!(blocked.exit_code, 0, "installs are blocked below 64 MiB");
    assert!(
        blocked.stderr.contains("no-space"),
        "blocked install reports no-space"
    );
    rig.set_free_bytes(512 * 1024 * 1024 + 60 * 1024 * 1024);
    rig.advance_clock(Duration::from_secs(30));
    assert!(
        !rig.disk_pressure_step_active("evict-caches"),
        "step undone above threshold plus 10 percent"
    );
}

/// REQ: HST-662
#[test]
fn hst_662_disk_low_event_reaches_only_host_event_plugins_once_per_minute_without_absolute_path() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.set_free_bytes(100 * 1024 * 1024);
    rig.advance_clock(Duration::from_secs(10));
    rig.set_free_bytes(90 * 1024 * 1024);
    rig.advance_clock(Duration::from_secs(10));
    let events = rig.disk_low_events_received(NOTES);
    assert_eq!(events.len(), 1, "at most once per 60 s");
    let event = &events[0];
    for field in ["free_mib", "volume", "step"] {
        assert!(event.get(field).is_some(), "event carries {field}");
    }
    assert!(
        !event
            .to_string()
            .contains(rig.data_dir(NOTES).to_str().expect("utf-8 path")),
        "no absolute volume path"
    );
    assert!(
        rig.footer_warning_text().contains("MiB"),
        "footer warning shows free space in MiB"
    );
}

/// REQ: HST-663
#[test]
fn hst_663_free_space_recovery_resumes_installs_and_logs_and_clears_warning_within_30_s() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.fill_disk();
    let package = rig.write_package(&pkg(NOTES, "1.0.1"));
    let blocked = rig.cli(&["install", package.to_str().expect("utf-8 path"), "--yes"]);
    assert_ne!(
        blocked.exit_code, 0,
        "install blocked while the disk is full"
    );
    rig.write_log_lines_while_full(NOTES, 25);
    rig.free_disk();
    rig.advance_clock(Duration::from_secs(30));
    assert!(
        !rig.footer_warning_visible(),
        "footer warning cleared within 30 s"
    );
    assert_eq!(
        rig.log_lines_with_code("log.dropped").len(),
        1,
        "one log.dropped line with the count"
    );
    assert!(
        rig.blocked_installs_resumed(),
        "blocked installs resumed above 64 MiB"
    );
}

/// REQ: HST-664
#[test]
fn hst_664_non_writable_data_folder_goes_read_only_and_recovers_without_restart() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    rig.make_data_read_only(NOTES);
    server.advance_clock(Duration::from_secs(30));
    let write = rig
        .plugin_storage_put(NOTES, "k", b"v")
        .expect_err("write while read-only refused");
    assert_eq!(write.code.as_str(), "io");
    assert_eq!(write.message, "read-only", "message read-only");
    rig.make_data_writable(NOTES);
    server.advance_clock(Duration::from_secs(30));
    assert_eq!(
        server.log_lines_with_code("data.writable-again").len(),
        1,
        "logged when writable again"
    );
    assert_eq!(
        rig.plugin_failure_count(NOTES),
        0,
        "read-only writes do not count as failures"
    );
}

/// REQ: HST-693
#[test]
fn hst_693_offline_update_check_fails_fast_with_network_backs_off_and_marks_stale_metadata() {
    let rig = rig();
    rig.block_network();
    let started = std::time::Instant::now();
    let output = rig.cli(&["plugin", "update", "--check"]);
    assert!(
        started.elapsed() <= Duration::from_secs(10),
        "fails within the 10 s connect timeout"
    );
    assert!(output.stderr.contains("network"), "fails with network");
    let server = rig.start_server("default");
    assert!(
        server.is_running(),
        "startup is not blocked by the failed check"
    );
    assert!(
        rig.retry_intervals_are_backoff_capped_at_6_hours(),
        "retry at most every 15 minutes, backing off to 6 hours"
    );
    assert!(
        rig.cached_registry_entries_marked_stale_after_14_days(),
        "cached metadata at most 14 days, marked stale"
    );
}

/// REQ: HST-694
#[test]
fn hst_694_verify_and_local_install_work_offline_and_stale_revocation_lists_warn() {
    let rig = rig();
    rig.block_network();
    rig.set_cached_revocation_age_days(3);
    let package = rig.write_package(&pkg(NOTES, "1.0.0"));
    let install = rig.cli(&["install", package.to_str().expect("utf-8 path"), "--yes"]);
    assert_eq!(install.exit_code, 0, "local install succeeds offline");
    rig.set_cached_revocation_age_days(8);
    let verify = rig.cli(&["plugin", "verify", NOTES]);
    assert!(
        verify.stderr.contains("revocation.stale"),
        "older than 7 days warns revocation.stale"
    );
    assert!(
        rig.offline_revocation_policy_applies_to_registry_only(),
        "policy key applies only to registry plugins"
    );
}

/// REQ: HST-695
#[test]
fn hst_695_interrupted_download_resumes_three_times_then_fails_and_old_version_stays_active() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.serve_download_interrupted_each_time(NOTES, "1.1.0");
    let output = rig.cli(&["plugin", "update", NOTES]);
    assert!(
        output.stderr.contains("network"),
        "fails with network after three resumes"
    );
    assert_eq!(
        rig.range_requests_made(NOTES),
        3,
        "up to 3 range-request resumes"
    );
    assert_eq!(
        rig.partial_download_marker(NOTES).expect("partial marked"),
        (
            "https://example.invalid/notes-1.1.0.ilium".to_owned(),
            rig.partial_bytes(NOTES)
        ),
        "partial carries its URL and byte count"
    );
    assert_eq!(
        rig.active_version(NOTES),
        Some("1.0.0".to_owned()),
        "old version active and unchanged"
    );
}

/// REQ: HST-657
#[test]
fn hst_657_a_database_that_fails_the_integrity_check_is_emptied_on_start() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.write_storage(NOTES, "bucket", "key", b"value");
    rig.corrupt_file(&rig.storage_db_path(NOTES), b"garbage bytes, not a database");
    rig.start_server("default");
    assert!(
        rig.read_storage(NOTES, "bucket", "key").is_none(),
        "a database failing the integrity check is handled as corruption and its bucket is emptied"
    );
}

/// REQ: HST-659
#[test]
fn hst_659_a_damaged_installed_json_is_not_overwritten_by_a_second_start() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.corrupt_file(&rig.plugins_root().join("installed.json"), b"{ not json");
    rig.start_server("default");
    rig.start_server("default");
    assert_eq!(
        rig.read_file_text(&rig.plugins_root().join("installed.json")),
        "{ not json",
        "repeated starts keep the damaged file untouched until the user rebuilds"
    );
}

/// REQ: HST-660
#[test]
fn hst_660_recovery_command_exits_zero_when_nothing_is_damaged() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let output = rig.cli(&["plugin", "recovery"]);
    assert_eq!(
        output.exit_code, 0,
        "no unresolved items means exit 0, not 78"
    );
}

/// REQ: HST-661
#[test]
fn hst_661_no_disk_pressure_step_is_logged_while_free_space_is_above_512_mib() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.set_free_bytes(600 * 1024 * 1024);
    rig.advance_clock(Duration::from_secs(30));
    assert_eq!(
        rig.log_lines_with_code("disk.pressure.evict-caches").len(),
        0,
        "no eviction step while free space is above 512 MiB"
    );
}

/// REQ: HST-662
#[test]
fn hst_662_plugin_without_the_events_host_grant_receives_no_disk_low_event() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.set_free_bytes(100 * 1024 * 1024);
    rig.advance_clock(Duration::from_secs(10));
    assert!(
        rig.disk_low_events_received(NOTES).is_empty(),
        "a plugin that does not hold events:host gets no host.disk-low event"
    );
}

/// REQ: HST-663
#[test]
fn hst_663_a_write_refused_with_no_space_succeeds_on_retry_after_space_returns() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.fill_disk();
    let refused = rig.plugin_storage_put(NOTES, "k", b"v");
    assert!(refused.is_err(), "write refused while the disk is full");
    rig.free_disk();
    let retried = rig.plugin_storage_put(NOTES, "k", b"v");
    assert!(
        retried.is_ok(),
        "the same write succeeds on retry without a host restart"
    );
}

/// REQ: HST-664
#[test]
fn hst_664_writes_acknowledged_before_read_only_mode_are_still_readable_afterwards() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let _server = rig.start_server("default");
    rig.write_storage(NOTES, "bucket", "key", b"value");
    rig.make_data_read_only(NOTES);
    rig.make_data_writable(NOTES);
    assert_eq!(
        rig.read_storage(NOTES, "bucket", "key"),
        Some(b"value".to_vec()),
        "a write acknowledged before read-only mode is not lost"
    );
}

/// REQ: HST-693
#[test]
fn hst_693_a_failed_update_check_is_not_retried_again_straight_away() {
    let rig = rig();
    rig.block_network();
    let _first = rig.cli(&["plugin", "update", "--check"]);
    let attempts_after_first = rig.network_attempts();
    let _second = rig.cli(&["plugin", "update", "--check"]);
    assert_eq!(
        rig.network_attempts(),
        attempts_after_first,
        "no second network attempt inside the 15-minute retry interval"
    );
}

/// REQ: HST-694
#[test]
fn hst_694_a_revocation_list_at_most_7_days_old_gives_no_stale_warning_offline() {
    let rig = rig();
    rig.block_network();
    rig.set_cached_revocation_age_days(3);
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let verify = rig.cli(&["plugin", "verify", NOTES]);
    assert!(
        !verify.stderr.contains("revocation.stale"),
        "a 3-day-old revocation list produces no revocation.stale warning"
    );
}

/// REQ: HST-695
#[test]
fn hst_695_a_partial_download_is_removed_after_24_hours() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.serve_download_interrupted_each_time(NOTES, "1.1.0");
    let _output = rig.cli(&["plugin", "update", NOTES]);
    assert!(
        rig.partial_bytes(NOTES) > 0,
        "an interrupted download leaves a partial file"
    );
    rig.advance_clock(Duration::from_secs(24 * 3600 + 1));
    assert!(
        rig.partial_download_marker(NOTES).is_none(),
        "the partial file is removed once it is older than 24 hours"
    );
}
