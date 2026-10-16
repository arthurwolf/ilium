//! Reference, persistence and restart (LIF-150..157): the keep-and-lose matrix, pending
//! requests, restore bytes, snapshot failure, node metadata, start order and server crash.

use ilium_plugin_api::Side;
use proptest::prelude::*;

use crate::common::*;

/// Marker rows that the matrix keeps (`true`) or loses (`false`) for one event. Rows whose
/// cell is `S` or `n/a` are left out (see API_GAPS.md, conflict 4).
fn expected(event: LifeEvent) -> Vec<(Marker, bool)> {
    use LifeEvent::*;
    use Marker::*;
    let (restore_server, restore_client) = match event {
        PluginRestart => (Some(false), None),
        DisableEnable => (Some(false), None),
        Update => (Some(true), Some(true)),
        ServerRestart => (Some(false), None),
        ClientReconnect => (None, Some(true)),
        SessionDelete => (Some(false), Some(false)),
        Uninstall | UninstallPurge => (Some(false), Some(false)),
    };
    let mut rows = vec![
        (LinearMemory, false),
        (PendingRequest, false),
        (Settings, true),
    ];
    rows.extend(restore_server.map(|keep| (RestoreServer, keep)));
    rows.extend(restore_client.map(|keep| (RestoreClient, keep)));
    let purged = event == UninstallPurge;
    let removed = matches!(event, Uninstall | UninstallPurge);
    rows.push((GlobalBucket, !purged));
    rows.push((ProjectBucket, !purged));
    rows.push((DataFolder, !purged));
    rows.push((
        CacheFolder,
        !matches!(event, Uninstall | UninstallPurge),
    ));
    rows.push((Grants, !removed));
    rows.push((
        NodeMetadata,
        !matches!(event, SessionDelete),
    ));
    rows.push((
        DurableTimers,
        matches!(event, PluginRestart | Update | ServerRestart | ClientReconnect),
    ));
    if !matches!(event, ServerRestart) {
        rows.push((SessionBucket, !matches!(event, SessionDelete | Uninstall | UninstallPurge)));
    }
    rows
}

fn run_matrix_column(event: LifeEvent) {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("fs.read").restore_bytes(8));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    rig.set_all_markers(NOTES, &server);
    rig.apply_event(&server, NOTES, event);
    for (marker, kept) in expected(event) {
        assert_eq!(
            rig.marker_present(NOTES, marker),
            kept,
            "{marker:?} after {event:?}"
        );
    }
}

/// REQ: LIF-150
#[test]
fn lif_150_plugin_restart_column_keeps_and_loses_as_the_matrix_says() {
    run_matrix_column(LifeEvent::PluginRestart);
}

/// REQ: LIF-150
#[test]
fn lif_150_disable_then_enable_column_keeps_and_loses_as_the_matrix_says() {
    run_matrix_column(LifeEvent::DisableEnable);
}

/// REQ: LIF-150
#[test]
fn lif_150_update_column_keeps_and_loses_as_the_matrix_says() {
    run_matrix_column(LifeEvent::Update);
}

/// REQ: LIF-150
#[test]
fn lif_150_server_restart_column_keeps_and_loses_as_the_matrix_says() {
    run_matrix_column(LifeEvent::ServerRestart);
}

/// REQ: LIF-150
#[test]
fn lif_150_client_reconnect_column_keeps_and_loses_as_the_matrix_says() {
    run_matrix_column(LifeEvent::ClientReconnect);
}

/// REQ: LIF-150
#[test]
fn lif_150_session_delete_column_keeps_and_loses_as_the_matrix_says() {
    run_matrix_column(LifeEvent::SessionDelete);
}

/// REQ: LIF-150
#[test]
fn lif_150_uninstall_column_keeps_and_loses_as_the_matrix_says() {
    run_matrix_column(LifeEvent::Uninstall);
}

/// REQ: LIF-150
#[test]
fn lif_150_uninstall_with_purge_column_keeps_and_loses_as_the_matrix_says() {
    run_matrix_column(LifeEvent::UninstallPurge);
}

/// REQ: LIF-150
#[test]
fn doc_lif_150_matrix_page_still_names_delete_data_for_the_purge_column() {
    // The page calls the last column `--delete-data`; the CLI flag is `--purge` (API_GAPS.md).
    let page = manual_page("reference-persistence-and-restart");
    assert!(page.contains("Uninstall with `--delete-data`"));
    let cli = manual_page("reference-install-update-uninstall");
    assert!(cli.contains("--purge"));
}

fn pending_request_cancelled_at(event: LifeEvent) {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    let request = server.send_pending_request(NOTES);
    rig.apply_event(&server, NOTES, event);
    assert_eq!(request.outcome(), Some("cancelled".to_owned()), "{event:?}");
    let restarted = rig.start_server_or_same(&server);
    rig.advance(secs(10));
    assert_eq!(restarted.deliveries_of(request.id()), 0, "replayed after {event:?}");
}

/// REQ: LIF-151
#[test]
fn lif_151_pending_request_completes_cancelled_at_plugin_restart() {
    pending_request_cancelled_at(LifeEvent::PluginRestart);
}

/// REQ: LIF-151
#[test]
fn lif_151_pending_request_completes_cancelled_at_disable_then_enable() {
    pending_request_cancelled_at(LifeEvent::DisableEnable);
}

/// REQ: LIF-151
#[test]
fn lif_151_pending_request_completes_cancelled_at_update() {
    pending_request_cancelled_at(LifeEvent::Update);
}

/// REQ: LIF-151
#[test]
fn lif_151_pending_request_completes_cancelled_at_server_restart() {
    pending_request_cancelled_at(LifeEvent::ServerRestart);
}

/// REQ: LIF-151
#[test]
fn lif_151_cancelled_request_is_not_replayed_after_a_second_restart() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    let request = server.send_pending_request(NOTES);
    server.crash();
    let again = rig.start_server("s1");
    again.crash();
    let third = rig.start_server("s1");
    rig.advance(secs(10));
    assert_eq!(third.deliveries_of(request.id()), 0);
}

fn restore_len_after_returning(len: usize) -> (Option<usize>, Vec<String>) {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").restore_bytes(len));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    server.disable(NOTES);
    server.enable(NOTES);
    rig.advance(secs(3));
    (server.last_restored_len(NOTES), server.log_codes())
}

/// REQ: LIF-152
#[test]
fn lif_152_restore_bytes_of_exactly_1_mib_are_delivered() {
    let (len, codes) = restore_len_after_returning(1_048_576);
    assert_eq!(len, Some(1_048_576));
    assert!(!codes.contains(&"restore.dropped-size".to_owned()));
}

/// REQ: LIF-152
#[test]
fn lif_152_restore_bytes_of_1_mib_plus_one_are_dropped_and_activation_has_none() {
    let (len, codes) = restore_len_after_returning(1_048_577);
    assert_eq!(len, None);
    assert!(codes.contains(&"restore.dropped-size".to_owned()));
}

/// REQ: LIF-152
#[test]
fn lif_152_empty_restore_bytes_leave_activation_restored_absent() {
    let (len, _codes) = restore_len_after_returning(0);
    assert_eq!(len, None);
}

proptest! {
    /// REQ: LIF-152
    #[test]
    fn lif_152_restored_is_present_exactly_when_the_size_is_in_1_to_1_mib(len in prop::sample::select(vec![0usize, 1, 2, 4096, 1_048_575, 1_048_576, 1_048_577, 2_000_000])) {
        let (got, _codes) = restore_len_after_returning(len);
        prop_assert_eq!(got.is_some(), (1..=1_048_576).contains(&len));
    }
}

/// REQ: LIF-153
#[test]
fn lif_153_two_detaching_clients_store_last_writer_wins_and_a_third_gets_it() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").restore_bytes(0));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    let first = server.attach_client("c1");
    let second = server.attach_client("c2");
    first.detach_with_restore(NOTES, b"first");
    second.detach_with_restore(NOTES, b"second");
    let third = server.attach_client("c3");
    rig.advance(secs(3));
    assert_eq!(third.restored_bytes(NOTES), Some(b"second".to_vec()));
}

/// REQ: LIF-153
#[test]
fn lif_153_restore_bytes_are_keyed_per_plugin_id() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    install_ok(&rig, &pkg(OTHER, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    let client = server.attach_client("c1");
    client.detach_with_restore(NOTES, b"notes");
    client.detach_with_restore(OTHER, b"other");
    let next = server.attach_client("c2");
    rig.advance(secs(3));
    assert_eq!(next.restored_bytes(NOTES), Some(b"notes".to_vec()));
    assert_eq!(next.restored_bytes(OTHER), Some(b"other".to_vec()));
}

/// REQ: LIF-153
#[test]
fn lif_153_client_detach_deactivates_with_shutdown_and_sends_the_bytes() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    let client = server.attach_client("c1");
    client.detach_with_restore(NOTES, b"bytes");
    assert_eq!(
        client.deactivations(NOTES).last(),
        Some(&ilium_plugin_api::generated::types::lifecycle::DeactivationReason::Shutdown)
    );
    assert_eq!(server.stored_client_restore(NOTES), Some(b"bytes".to_vec()));
}

/// REQ: LIF-153
#[test]
fn lif_153_client_bytes_over_1_mib_are_dropped() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    let client = server.attach_client("c1");
    client.detach_with_restore(NOTES, &vec![7u8; 1_048_577]);
    assert_eq!(server.stored_client_restore(NOTES), None);
}

/// REQ: LIF-154
#[test]
fn lif_154_failed_snapshot_write_keeps_the_server_running_and_logs_it() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    server.snapshot_now();
    let good = rig.snapshot_bytes("s1");
    rig.make_snapshot_dir_read_only("s1");
    server.snapshot_now();
    assert!(server.is_running());
    assert!(server.log_codes().contains(&"snapshot.write-failed".to_owned()));
    assert_eq!(rig.snapshot_bytes("s1"), good);
}

/// REQ: LIF-154
#[test]
fn lif_154_next_snapshot_after_the_folder_is_writable_again_succeeds() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    server.snapshot_now();
    rig.make_snapshot_dir_read_only("s1");
    server.set_node_metadata(1, NOTES, "k", b"v");
    server.snapshot_now();
    rig.make_snapshot_dir_writable("s1");
    server.snapshot_now();
    assert!(rig.snapshot_metadata_keys("s1", NOTES).contains(&"k".to_owned()));
}

/// REQ: LIF-154
#[test]
fn lif_154_running_plugins_are_unaffected_by_a_failed_snapshot() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    rig.make_snapshot_dir_read_only("s1");
    server.snapshot_now();
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.0.0"));
    assert_eq!(server.failure_count(NOTES, Side::Server), 0);
}

/// REQ: LIF-155
#[test]
fn lif_155_node_metadata_survives_a_plugin_restart() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    server.set_node_metadata(7, NOTES, "mark", b"x");
    rig.apply_event(&server, NOTES, LifeEvent::PluginRestart);
    assert_eq!(server.node_metadata_as(NOTES, 7, NOTES, "mark"), Some(b"x".to_vec()));
}

/// REQ: LIF-155
#[test]
fn lif_155_node_metadata_survives_an_update() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    server.set_node_metadata(7, NOTES, "mark", b"x");
    rig.apply_event(&server, NOTES, LifeEvent::Update);
    assert_eq!(server.node_metadata_as(NOTES, 7, NOTES, "mark"), Some(b"x".to_vec()));
}

/// REQ: LIF-155
#[test]
fn lif_155_node_metadata_survives_a_server_restart() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    server.set_node_metadata(7, NOTES, "mark", b"x");
    server.snapshot_now();
    server.shutdown();
    let fresh = rig.start_server("s1");
    rig.advance(secs(3));
    assert_eq!(fresh.node_metadata_as(NOTES, 7, NOTES, "mark"), Some(b"x".to_vec()));
}

/// REQ: LIF-155
#[test]
fn lif_155_node_metadata_survives_an_uninstall_in_the_snapshot() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    server.set_node_metadata(7, NOTES, "mark", b"x");
    rig.cli(&["uninstall", NOTES]);
    rig.advance(secs(3));
    server.snapshot_now();
    assert!(rig.snapshot_metadata_keys("s1", NOTES).contains(&"mark".to_owned()));
}

/// REQ: LIF-155
#[test]
fn lif_155_node_metadata_is_removed_with_its_node() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    server.set_node_metadata(7, NOTES, "mark", b"x");
    server.remove_node(7);
    assert_eq!(server.node_metadata_as(NOTES, 7, NOTES, "mark"), None);
    server.snapshot_now();
    assert!(rig.snapshot_metadata_keys("s1", NOTES).is_empty());
}

/// REQ: LIF-156
#[test]
fn lif_156_plugin_host_starts_after_restore_snapshot() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    let calls = server.kernel_call_log();
    assert!(position_of(&calls, "restore_snapshot") < position_of(&calls, "plugin_host.start"));
}

/// REQ: LIF-156
#[test]
fn lif_156_plugin_host_stops_before_drain_session_work() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    server.shutdown();
    let calls = server.kernel_call_log();
    assert!(position_of(&calls, "plugin_host.stop") < position_of(&calls, "drain_session_work"));
}

/// REQ: LIF-156
#[test]
fn lif_156_full_order_is_restore_then_start_then_stop_then_drain() {
    let rig = rig();
    let server = rig.start_server("s1");
    server.shutdown();
    let calls = server.kernel_call_log();
    let order: Vec<usize> = ["restore_snapshot", "plugin_host.start", "plugin_host.stop", "drain_session_work"]
        .iter()
        .map(|name| position_of(&calls, name))
        .collect();
    assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{calls:?}");
}

/// REQ: LIF-157
#[test]
fn lif_157_server_crash_between_two_writes_loses_memory_and_server_restore_bytes_only() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").restore_bytes(8));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    server.write_storage(NOTES, "global", "a", b"1");
    server.write_setting(NOTES, "theme", "dark");
    server.set_node_metadata(7, NOTES, "mark", b"x");
    server.snapshot_now();
    server.write_storage(NOTES, "global", "b", b"2");
    server.crash();
    let fresh = rig.start_server("s1");
    rig.advance(secs(3));
    assert_eq!(fresh.read_storage(NOTES, "global", "a"), Some(b"1".to_vec()));
    assert_eq!(fresh.read_storage(NOTES, "global", "b"), Some(b"2".to_vec()));
    assert_eq!(rig.setting(NOTES, "theme").as_deref(), Some("dark"));
    assert_eq!(fresh.last_restored_len(NOTES), None);
}

/// REQ: LIF-157
#[test]
fn lif_157_metadata_written_after_the_last_snapshot_is_lost_in_a_crash() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    server.set_node_metadata(7, NOTES, "old", b"1");
    server.snapshot_now();
    server.set_node_metadata(7, NOTES, "new", b"2");
    server.crash();
    let fresh = rig.start_server("s1");
    assert_eq!(fresh.node_metadata_as(NOTES, 7, NOTES, "old"), Some(b"1".to_vec()));
    assert_eq!(fresh.node_metadata_as(NOTES, 7, NOTES, "new"), None);
}

/// REQ: LIF-157
#[test]
fn lif_157_data_folder_files_written_before_the_crash_are_still_there() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    server.write_data_file(NOTES, "note.txt", b"kept");
    server.crash();
    let _fresh = rig.start_server("s1");
    assert_eq!(std::fs::read(rig.data_dir(NOTES).join("note.txt")).ok(), Some(b"kept".to_vec()));
}
