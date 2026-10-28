//! Manual page `reference-health-and-watchdog`: health transitions, the health command exit codes,
//! the stalled-loop watchdog, degraded notifications, shutdown flushing and crash-safe journal
//! recovery (HST-615, 617, 618, 641, 648, 679, 680).

use std::time::Duration;

use super::common::{install_ok, pkg, rig, NOTES, OTHER};

/// REQ: HST-615
#[test]
fn hst_615_five_flips_within_10_seconds_write_one_health_changed_line_with_final_state() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    for index in 0..5 {
        let state = if index % 2 == 0 {
            "degraded"
        } else {
            "healthy"
        };
        rig.force_health(NOTES, state);
        rig.advance_clock(Duration::from_secs(2));
    }
    rig.force_health(NOTES, "failed");
    let lines = rig.log_lines_with_code("host.health.changed");
    assert_eq!(
        lines.len(),
        1,
        "at most one transition per plugin side per 10 seconds"
    );
    assert_eq!(
        lines[0]["new"], "failed",
        "the latest state is applied after the window"
    );
    assert!(
        lines[0].get("old").is_some() && lines[0].get("trigger").is_some(),
        "line names old state and trigger row"
    );
}

/// REQ: HST-617
#[test]
fn hst_617_health_command_exit_codes_and_one_json_line_per_plugin_when_piped() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    install_ok(&rig, &pkg(OTHER, "1.0.0"));
    rig.force_health(NOTES, "healthy");
    rig.force_health(OTHER, "healthy");
    assert_eq!(
        rig.cli(&["plugin", "health"]).exit_code,
        0,
        "no plugin failed or degraded exits 0"
    );
    rig.force_health(OTHER, "degraded");
    assert_eq!(
        rig.cli(&["plugin", "health"]).exit_code,
        1,
        "a degraded plugin exits 1"
    );
    rig.force_health(OTHER, "failed");
    let output = rig.cli(&["plugin", "health"]);
    assert_eq!(output.exit_code, 2, "a failed plugin exits 2");
    assert_eq!(
        output.stdout_jsonl.len(),
        2,
        "one JSON line per plugin when output is piped"
    );
}

/// REQ: HST-618
#[test]
fn hst_618_stalled_loop_is_logged_after_2_seconds_and_names_the_blocking_call() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    rig.install_slow_sync_host_call_fixture(NOTES, Duration::from_secs(3));
    rig.trigger_slow_host_call(NOTES);
    rig.advance_clock(Duration::from_millis(2_100));
    let lines = server.log_lines_with_code("host.loop-stalled");
    assert_eq!(lines.len(), 1, "host.loop-stalled written once");
    let named = lines[0]["call"]
        .as_str()
        .expect("the line names the running call");
    assert!(
        named.contains("slow"),
        "names the slow synchronous host call: {named}"
    );
}

/// REQ: HST-641
#[test]
fn hst_641_degraded_twice_in_an_hour_notifies_once_and_muted_plugins_never_notify() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    install_ok(&rig, &pkg(OTHER, "1.0.0"));
    rig.force_health(NOTES, "degraded");
    rig.force_health(NOTES, "healthy");
    rig.force_health(NOTES, "degraded");
    assert_eq!(
        rig.notifications_for(NOTES),
        1,
        "one notification per plugin per 24 hours"
    );
    rig.cli(&["plugin", "mute", OTHER]);
    rig.force_health(OTHER, "failed");
    assert_eq!(
        rig.notifications_for(OTHER),
        0,
        "muted plugin raises no notification"
    );
}

/// REQ: HST-648
#[test]
fn hst_648_shutdown_interrupts_a_plugin_ignoring_stop_and_flushes_within_2_seconds() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    rig.make_plugin_ignore_stop(NOTES);
    let started = std::time::Instant::now();
    server.shutdown();
    let flush = started.elapsed();
    assert!(
        flush <= Duration::from_secs(2),
        "flush finishes within 2 seconds, took {flush:?}"
    );
    assert_eq!(
        server.log_lines_with_code("host.shutdown.forced").len(),
        1,
        "forced interruption logged"
    );
    assert!(
        rig.plugin_interrupted_at(NOTES) <= Duration::from_secs(1),
        "interrupted at 1 second"
    );
}

/// REQ: HST-679
#[test]
fn hst_679_kill_between_update_and_purge_steps_recovers_once_and_no_plugin_runs_first() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.begin_update_swap(NOTES, &pkg(NOTES, "1.1.0"));
    rig.kill_host_between_steps();
    rig.begin_purge(NOTES);
    rig.kill_host_between_steps();
    let server = rig.start_server("default");
    assert_eq!(
        rig.plugin_export_calls_before_recovery(),
        0,
        "no plugin ran before recovery ended"
    );
    let recovered = server.log_lines_with_code("recovery.completed");
    assert_eq!(recovered.len(), 1, "one recovery.completed line");
    assert!(
        recovered[0].get("count").is_some(),
        "the line carries the count"
    );
    assert_eq!(
        rig.active_version(NOTES),
        Some("1.0.0".to_owned()),
        "final state is consistent"
    );
}

/// REQ: HST-680
#[test]
fn hst_680_kill_point_matrix_leaves_a_consistent_state_on_next_start() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    for point in rig.kill_points() {
        let scenario = rig.fresh_scenario_for(&point);
        scenario.kill_at(&point);
        let server = scenario.restart();
        assert!(
            server.is_running(),
            "restart after kill at {point} succeeds"
        );
        assert!(
            rig.state_is_consistent(NOTES),
            "consistent state after kill at {point}"
        );
        assert!(
            rig.fault_writer_checked(&point),
            "fault-injection writer checked state at {point}"
        );
    }
}

/// REQ: HST-615
#[test]
fn hst_615_one_health_change_writes_one_line_with_the_new_state() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.force_health(NOTES, "failed");
    let lines = rig.log_lines_with_code("host.health.changed");
    assert_eq!(lines.len(), 1, "one transition, one line");
    assert!(
        lines[0].to_string().contains("failed"),
        "the line carries the new state: {}",
        lines[0]
    );
}

/// REQ: HST-617
#[test]
fn hst_617_health_exits_0_and_prints_one_line_per_plugin_when_all_are_healthy() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    install_ok(&rig, &pkg(OTHER, "1.0.0"));
    rig.force_health(NOTES, "healthy");
    rig.force_health(OTHER, "healthy");
    let output = rig.cli(&["plugin", "health"]);
    assert_eq!(output.exit_code, 0, "{:?}", output.stdout_jsonl);
    assert_eq!(
        output.stdout_jsonl.len(),
        2,
        "one JSON line per installed plugin"
    );
}

/// REQ: HST-618
#[test]
fn hst_618_one_second_without_a_loop_tick_writes_no_loop_stalled_line() {
    let rig = rig();
    let server = rig.start_server("default");
    rig.advance_clock(Duration::from_secs(1));
    assert!(
        rig.log_lines_with_code("host.loop-stalled").is_empty(),
        "one second without a tick is under the 2 second limit"
    );
    server.shutdown();
}

/// REQ: HST-641
#[test]
fn hst_641_a_plugin_that_turns_degraded_once_raises_exactly_one_notification() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.force_health(NOTES, "degraded");
    assert_eq!(
        rig.notifications_for(NOTES),
        1,
        "one notification for the first degraded state"
    );
}

/// REQ: HST-648
#[test]
fn hst_648_a_plugin_side_that_stops_in_time_is_not_reported_as_forced() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    server.shutdown();
    assert!(
        rig.log_lines_with_code("host.shutdown.forced").is_empty(),
        "no forced stop for a plugin side that stops in time"
    );
}

/// REQ: HST-679
#[test]
fn hst_679_recovery_after_an_interrupted_update_writes_one_counted_recovery_line() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.begin_update_swap(NOTES, &pkg(NOTES, "1.1.0"));
    rig.kill_host_between_steps();
    let server = rig.start_server("default");
    let recovered = rig.log_lines_with_code("recovery.completed");
    assert_eq!(recovered.len(), 1, "one recovery line");
    assert!(
        recovered[0].to_string().contains("count"),
        "the recovery line carries a count: {}",
        recovered[0]
    );
    server.shutdown();
}

/// REQ: HST-680
#[test]
fn hst_680_the_kill_point_matrix_covers_all_seven_listed_points() {
    let rig = rig();
    assert_eq!(
        rig.kill_points().into_iter().count(),
        7,
        "install staging, rename into store, update swap, migrate, storage batch, snapshot write and grants.json write"
    );
}
