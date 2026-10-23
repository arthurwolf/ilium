//! Gap tests for the lifecycle page: wall-clock evaluation of end-of-life and abandonment,
//! administrator pins, failure windows, safe mode and essential core restarts (LIF-702, LIF-704,
//! LIF-721, LIF-723, LIF-729).
//!
//! Assumptions: `jump_wall_clock` moves only the wall clock (signed whole seconds); `pin` and
//! `unpin` run `ilium plugin pin|unpin <id>`; `start_server_safe_mode` starts a server in the
//! named safe mode; `plant_end_of_life` records an end-of-life date for an id.

use crate::support::*;

/// REQ: LIF-702
#[test]
fn lif_702_backward_wall_clock_jump_does_not_reenable_a_retired_plugin() {
    assert_manual_says(
        "lifecycle",
        "a backward wall-clock jump MUST NOT re-enable a retired plugin",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_end_of_life(NOTES, "2026-01-01");
    let server = rig.start_server("default");
    server.shutdown();
    let _restarted = rig.start_server("default");
    assert_eq!(list_row(&rig, NOTES)["state"], "retired");
    rig.jump_wall_clock(-40 * 86_400);
    let _again = rig.start_server("default");
    assert_eq!(
        list_row(&rig, NOTES)["state"],
        "retired",
        "set the clock back 40 days after retirement and the state stays retired"
    );
}

/// REQ: LIF-704
#[test]
fn lif_704_pin_skips_abandonment_disable_and_unpin_restarts_the_30_day_countdown() {
    assert_manual_says(
        "lifecycle",
        "When an administrator pins a plugin (PKG-711) the host MUST skip the abandonment disable",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    rig.plant_abandonment_notice(NOTES, days(0));
    rig.cli(&["plugin", "pin", NOTES]);
    rig.jump_wall_clock(31 * 86_400);
    server.apply_pending();
    assert!(
        server.is_enabled(NOTES),
        "a pinned plugin stays enabled after 31 days"
    );
    assert_eq!(
        log_count(&server, "plugin.abandon-pinned"),
        1,
        "logged once per start"
    );
    rig.cli(&["plugin", "unpin", NOTES]);
    rig.jump_wall_clock(29 * 86_400);
    server.apply_pending();
    assert!(
        server.is_enabled(NOTES),
        "a fresh 30 day warning has not passed after 29 days"
    );
    rig.jump_wall_clock(2 * 86_400);
    server.apply_pending();
    assert!(
        !server.is_enabled(NOTES),
        "the fresh countdown disables the plugin after 30 days"
    );
}

/// REQ: LIF-721
#[test]
fn lif_721_failure_windows_restart_empty_and_host_crash_is_not_counted() {
    assert_manual_says("lifecycle", "Failure windows MUST be held in memory only");
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    server.fail(NOTES, Side::Server);
    server.fail(NOTES, Side::Server);
    server.shutdown();
    let restarted = rig.start_server("default");
    restarted.fail(NOTES, Side::Server);
    assert_eq!(
        restarted.restart_delay_ms(NOTES, Side::Server),
        Some(1_000),
        "one failure after a restart gives a 1 s restart delay, not 5 s"
    );
    assert_eq!(restarted.failure_count(NOTES, Side::Server), 1);
}

/// REQ: LIF-723
#[test]
fn lif_723_safe_mode_holds_enable_and_logs_the_starting_plugin_as_crash_suspect() {
    assert_manual_says(
        "lifecycle",
        "In `third-party` safe mode, `ilium plugin enable <id>` MUST be accepted into the registry",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").activate_traps());
    for _ in 0..3 {
        let crashed = rig.start_server_safe_mode("default", "third-party");
        crashed.crash();
    }
    let enabled = rig.cli(&["plugin", "enable", NOTES]);
    assert_eq!(enabled.exit_code, 0, "enable is accepted into the registry");
    let server = rig.start_server_safe_mode("default", "third-party");
    assert!(
        server.active_version(NOTES).is_none(),
        "no activation until safe mode is left"
    );
    assert_eq!(list_row(&rig, NOTES)["disabled_reason"], "safe-mode");
    assert!(
        server
            .log_codes()
            .contains(&format!("crash.suspect {NOTES}")),
        "the plugin that was starting at each crash mark is logged as crash.suspect"
    );
}

/// REQ: LIF-729
#[test]
fn lif_729_essential_core_plugin_restarts_after_5_s_and_shows_notice_after_10_failures() {
    assert_manual_says(
        "lifecycle",
        "An essential core plugin's third and later failures MUST restart after 5 s on every failure",
    );
    let rig = rig();
    rig.install_bundled(&pkg(CORE, "1.0.0"));
    let server = rig.start_server("default");
    for _ in 0..12 {
        server.fail(CORE, Side::Server);
        rig.advance(secs(10));
    }
    assert_eq!(
        server.restart_delay_ms(CORE, Side::Server),
        Some(5_000),
        "the third and later failures restart after 5 s"
    );
    assert_eq!(
        log_count(&server, "failure.core-restart"),
        10,
        "one log per restart"
    );
    assert!(
        !server.notices().is_empty(),
        "a persistent notice is shown after 10 failures in one window"
    );
}
