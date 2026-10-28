//! Manual page `reference-persistence-and-restart`: per-bucket quotas and the session bucket limit,
//! snapshot state caps, and placeholder restore for uninstalled plugins (HST-674, 691, 692).

use super::common::{install_ok, pkg, rig, NOTES};

/// REQ: HST-674
#[test]
fn hst_674_bucket_quota_applies_per_bucket_and_the_1001st_session_bucket_is_refused() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    for index in 0..1_000 {
        rig.create_session_bucket(NOTES, &format!("session-{index}"))
            .expect("first 1,000 buckets accepted");
    }
    let refused = rig
        .create_session_bucket(NOTES, "session-1000")
        .expect_err("the 1,001st bucket is refused");
    assert_eq!(refused.code.as_str(), "quota-exceeded");
    rig.close_session(NOTES, "session-0");
    assert!(
        rig.session_bucket_exists(NOTES, "session-0"),
        "closing a session keeps its bucket"
    );
}

/// REQ: HST-691
#[test]
fn hst_691_five_mib_state_blob_is_refused_and_an_unreadable_new_snapshot_keeps_the_previous_one() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    server.snapshot_now();
    let previous = rig.snapshot_file_bytes("default");
    rig.plugin_set_state(NOTES, &vec![0u8; 5 * 1024 * 1024]);
    server.snapshot_now();
    assert_eq!(
        server.log_lines_with_code("snapshot.state-too-large").len(),
        1,
        "5 MiB blob refused with snapshot.state-too-large"
    );
    rig.make_next_snapshot_unreadable();
    server.snapshot_now();
    assert_eq!(
        rig.snapshot_file_bytes("default"),
        previous,
        "previous snapshot kept when the new one is unreadable"
    );
}

/// REQ: HST-692
#[test]
fn hst_692_state_of_uninstalled_plugin_stays_in_the_snapshot_and_returns_on_install() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    rig.plugin_set_state(NOTES, b"remembered-state");
    server.snapshot_now();
    rig.uninstall(NOTES);
    let restored = rig.start_server("default");
    assert!(
        restored.placeholder_text().contains(NOTES),
        "placeholder names the plugin id"
    );
    assert!(
        rig.snapshot_file_contains_state(NOTES, b"remembered-state"),
        "state kept unmodified in the snapshot file"
    );
    let snapshot_before = rig.snapshot_file_bytes("default");
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    assert_eq!(
        rig.plugin_state_after_install(NOTES),
        b"remembered-state".to_vec(),
        "state returns after install"
    );
    assert_eq!(
        rig.snapshot_file_bytes("default"),
        snapshot_before,
        "no new snapshot written on that path"
    );
}

/// REQ: HST-674
#[test]
fn hst_674_a_closed_session_bucket_is_pruned_once_it_is_older_than_the_limit() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.create_session_bucket(NOTES, "session-old")
        .expect("session bucket created");
    rig.close_session(NOTES, "session-old");
    rig.advance_clock(std::time::Duration::from_secs(400 * 24 * 3600));
    let prune = rig.cli(&["plugin", "data", "prune", "--older-than", "30"]);
    assert_eq!(prune.exit_code, 0, "prune succeeds");
    assert!(
        !rig.session_bucket_exists(NOTES, "session-old"),
        "a session bucket unused for 400 days is removed by prune"
    );
}

/// REQ: HST-691
#[test]
fn hst_691_a_100_kib_state_blob_under_the_cap_is_kept_in_the_snapshot() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    let state = vec![7u8; 100 * 1024];
    rig.plugin_set_state(NOTES, &state);
    server.snapshot_now();
    assert_eq!(
        server.log_lines_with_code("snapshot.state-too-large").len(),
        0,
        "a 100 KiB blob is under the per-plugin cap and is not refused"
    );
    assert!(
        rig.snapshot_file_contains_state(NOTES, &state),
        "the 100 KiB state is written into the snapshot"
    );
}

/// REQ: HST-692
#[test]
fn hst_692_remembered_state_survives_repeated_snapshots_while_the_plugin_is_uninstalled() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    rig.plugin_set_state(NOTES, b"remembered-state");
    server.snapshot_now();
    rig.uninstall(NOTES);
    let restored = rig.start_server("default");
    restored.snapshot_now();
    restored.snapshot_now();
    assert!(
        rig.snapshot_file_contains_state(NOTES, b"remembered-state"),
        "state of an uninstalled plugin is not dropped by later snapshots"
    );
}
