//! Manual page `reference-debugging-tools`: log file caps and eviction, and the 14-day session log
//! folder rule and purge (HST-670, 671).

use super::common::{install_ok, pkg, rig, NOTES};

/// REQ: HST-670
#[test]
fn hst_670_total_log_cap_deletes_oldest_rotated_file_of_largest_log_and_counts_evictions() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.fill_plugin_logs_past_cap(256 * 1024 * 1024);
    let active_before = rig.active_log_bytes(NOTES);
    rig.trigger_log_cap_eviction();
    assert!(
        rig.total_plugin_log_bytes() <= 256 * 1024 * 1024,
        "total log size within plugins.log_total_mib"
    );
    assert_eq!(
        rig.active_log_bytes(NOTES),
        active_before,
        "the active file is kept"
    );
    rig.trigger_log_cap_eviction();
    assert_eq!(
        rig.log_code_count("log.cap-evict"),
        2,
        "two log.cap-evict records after two evictions"
    );
}

/// REQ: HST-671
#[test]
fn hst_671_session_log_folder_older_than_14_days_is_deleted_at_start_and_purge_clears_plugin_logs()
{
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.create_session_log_folder("stale-session", 15);
    rig.create_session_log_folder("running-session", 15);
    let server = rig.start_server("running-session");
    assert!(
        !rig.session_log_folder_exists("stale-session"),
        "folder with newest file 15 days old deleted at start"
    );
    assert!(
        rig.session_log_folder_exists("running-session"),
        "folder of a running session kept"
    );
    assert!(server.is_running(), "server running");
    rig.cli(&["plugin", "uninstall", NOTES, "--purge", "--yes"]);
    assert!(
        rig.plugin_log_files_in_every_session_folder(NOTES)
            .is_empty(),
        "purge removes the plugin's log files everywhere"
    );
}

/// REQ: HST-670
#[test]
fn hst_670_eviction_deletes_nothing_while_logs_are_under_the_cap() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.emit_plugin_log(NOTES, "info", "small line");
    let before = rig.plugin_log_files(NOTES);
    rig.trigger_log_cap_eviction();
    assert_eq!(
        rig.plugin_log_files(NOTES),
        before,
        "no log file is deleted while under the cap"
    );
    assert_eq!(
        rig.log_code_count("log.cap-evict"),
        0,
        "no eviction record while under the cap"
    );
}

/// REQ: HST-671
#[test]
fn hst_671_a_recent_session_log_folder_is_kept_at_start() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.create_session_log_folder("recent-session", 1);
    let server = rig.start_server("default");
    assert!(
        rig.session_log_folder_exists("recent-session"),
        "a folder whose newest file is 1 day old is kept"
    );
    server.shutdown();
}
