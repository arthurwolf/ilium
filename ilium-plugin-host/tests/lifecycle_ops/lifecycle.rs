//! Lifecycle page: dev reload (LIF-220..229), stop effects (LIF-271) and the bundled "Not now"
//! counter (LIF-275).

use ilium_plugin_api::generated::types::lifecycle::DeactivationReason;
use ilium_plugin_api::Side;

use crate::common::*;

fn dev_rig() -> (OpsRig, ServerHandle) {
    let rig = rig();
    let server = rig.start_server("s1");
    (rig, server)
}

/// REQ: LIF-220
#[test]
fn lif_220_dev_with_no_server_exits_2_with_dev_no_server() {
    let rig = rig();
    let output = rig.dev_cli(&["--cwd", "/nonexistent-project"]);
    assert_eq!(output.exit_code, 2);
    assert_eq!(error_codes(&output), vec!["dev.no-server".to_owned()]);
}

/// REQ: LIF-220
#[test]
fn lif_220_dev_attaches_to_the_session_of_cwd_by_default() {
    let (rig, server) = dev_rig();
    let session = rig.dev_start(&pkg(NOTES, "1.0.0"), &[]);
    assert_eq!(session.attached_session(), "s1");
    assert!(server.has_dev_package(NOTES));
}

/// REQ: LIF-220
#[test]
fn lif_220_dev_with_session_flag_attaches_to_the_named_session() {
    let rig = rig();
    let _a = rig.start_server("a");
    let b = rig.start_server("b");
    let session = rig.dev_start(&pkg(NOTES, "1.0.0"), &["--session", "b"]);
    assert_eq!(session.attached_session(), "b");
    assert!(b.has_dev_package(NOTES));
}

/// REQ: LIF-220
#[test]
fn lif_220_start_server_flag_starts_a_server_when_none_runs() {
    let rig = rig();
    let session = rig.dev_start(&pkg(NOTES, "1.0.0"), &["--start-server"]);
    assert!(session.exit_code().is_none());
    assert!(rig.server_running("s1"));
}

/// REQ: LIF-221
#[test]
fn lif_221_dev_package_shadows_the_installed_one_for_that_session_only() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let a = rig.start_server("a");
    let b = rig.start_server("b");
    rig.advance(secs(3));
    let _dev = rig.dev_start(&pkg(NOTES, "2.0.0"), &["--session", "a"]);
    rig.advance(secs(3));
    assert_eq!(a.active_version(NOTES).as_deref(), Some("2.0.0"));
    assert_eq!(b.active_version(NOTES).as_deref(), Some("1.0.0"));
    assert_eq!(registry_entry(&rig, NOTES)["active_version"], "1.0.0");
}

/// REQ: LIF-221
#[test]
fn lif_221_ending_dev_makes_the_installed_version_active_again() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    let dev = rig.dev_start(&pkg(NOTES, "2.0.0"), &[]);
    rig.advance(secs(3));
    dev.end();
    rig.advance(secs(3));
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.0.0"));
    assert!(!server.has_dev_package(NOTES));
}

/// REQ: LIF-222
#[test]
fn lif_222_two_changes_100_ms_apart_start_one_build_300_ms_after_the_last() {
    let (rig, _server) = dev_rig();
    let dev = rig.dev_start(&pkg(NOTES, "1.0.0"), &[]);
    let builds_before = dev.build_count();
    dev.touch_source();
    rig.advance(ms(100));
    dev.touch_source();
    rig.advance(ms(299));
    assert_eq!(dev.build_count(), builds_before);
    rig.advance(ms(2));
    assert_eq!(dev.build_count(), builds_before + 1);
}

/// REQ: LIF-222
#[test]
fn lif_222_changes_300_ms_apart_start_two_builds() {
    let (rig, _server) = dev_rig();
    let dev = rig.dev_start(&pkg(NOTES, "1.0.0"), &[]);
    let builds_before = dev.build_count();
    dev.touch_source();
    rig.advance(ms(301));
    dev.touch_source();
    rig.advance(ms(301));
    assert_eq!(dev.build_count(), builds_before + 2);
}

/// REQ: LIF-223
#[test]
fn lif_223_reload_steps_run_in_the_documented_order_with_two_clients() {
    let (rig, server) = dev_rig();
    let dev = rig.dev_start(&pkg(NOTES, "1.0.0").restore_bytes(4), &[]);
    let _first = server.attach_client("c1");
    let _second = server.attach_client("c2");
    rig.advance(secs(3));
    dev.reload(&pkg(NOTES, "1.0.1").restore_bytes(4));
    let log = dev.reload_log();
    let order = [
        "build",
        "validate-manifest",
        "compare-grants",
        "deactivate:server:upgrade",
        "deactivate:client:c1:upgrade",
        "deactivate:client:c2:upgrade",
        "swap",
        "activate:server:restored",
        "activate:client:c1:restored",
        "activate:client:c2:restored",
    ];
    let positions: Vec<usize> = order.iter().map(|step| position_of(&log, step)).collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "{log:?}"
    );
}

/// REQ: LIF-223
#[test]
fn lif_223_deactivations_during_a_reload_use_the_upgrade_reason() {
    let (rig, server) = dev_rig();
    let dev = rig.dev_start(&pkg(NOTES, "1.0.0"), &[]);
    rig.advance(secs(3));
    dev.reload(&pkg(NOTES, "1.0.1"));
    assert_eq!(
        server.deactivations(NOTES).last(),
        Some(&DeactivationReason::Upgrade)
    );
}

/// REQ: LIF-224
#[test]
fn lif_224_a_new_grant_produces_a_review_prompt_in_the_dev_terminal() {
    let (rig, _server) = dev_rig();
    let dev = rig.dev_start(&pkg(NOTES, "1.0.0"), &[]);
    dev.reload(&pkg(NOTES, "1.0.1").high_risk_grant("process.spawn"));
    assert_eq!(dev.prompts(), vec!["process.spawn".to_owned()]);
}

/// REQ: LIF-224
#[test]
fn lif_224_a_grant_removed_from_the_manifest_is_revoked_without_a_prompt() {
    let (rig, server) = dev_rig();
    let dev = rig.dev_start(&pkg(NOTES, "1.0.0").grant("fs.read"), &[]);
    rig.advance(secs(3));
    dev.reload(&pkg(NOTES, "1.0.1"));
    assert!(dev.prompts().is_empty());
    assert!(!server.granted(NOTES).contains(&"fs.read".to_owned()));
}

/// REQ: LIF-224
#[test]
fn lif_224_grant_flag_accepts_the_named_grant_without_a_prompt() {
    let (rig, server) = dev_rig();
    let dev = rig.dev_start(&pkg(NOTES, "1.0.0"), &["--grant", "process.spawn"]);
    dev.reload(&pkg(NOTES, "1.0.1").high_risk_grant("process.spawn"));
    assert!(dev.prompts().is_empty());
    rig.advance(secs(3));
    assert!(server.granted(NOTES).contains(&"process.spawn".to_owned()));
}

/// REQ: LIF-224
#[test]
fn lif_224_a_declined_high_risk_grant_is_not_granted_and_the_reload_continues() {
    let (rig, server) = dev_rig();
    let dev = rig.dev_start(&pkg(NOTES, "1.0.0").grant("fs.read"), &[]);
    dev.answer_prompts("n");
    dev.reload(
        &pkg(NOTES, "1.0.1")
            .grant("fs.read")
            .high_risk_grant("process.spawn"),
    );
    rig.advance(secs(3));
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.0.1"));
    assert!(server.granted(NOTES).contains(&"fs.read".to_owned()));
    assert!(!server.granted(NOTES).contains(&"process.spawn".to_owned()));
}

/// REQ: LIF-224
#[test]
fn lif_224_a_grant_flag_for_a_different_name_does_not_accept_the_new_grant() {
    let (rig, server) = dev_rig();
    let dev = rig.dev_start(&pkg(NOTES, "1.0.0"), &["--grant", "fs.write"]);
    dev.answer_prompts("n");
    dev.reload(&pkg(NOTES, "1.0.1").high_risk_grant("process.spawn"));
    rig.advance(secs(3));
    assert!(!server.granted(NOTES).contains(&"process.spawn".to_owned()));
}

/// REQ: LIF-225
#[test]
fn lif_225_added_contribution_is_registered_at_the_swap_and_extensions_changed_follows() {
    let (rig, server) = dev_rig();
    let dev = rig.dev_start(&pkg(NOTES, "1.0.0"), &[]);
    rig.advance(secs(3));
    let events_before = server.extension_events().len();
    dev.reload(&pkg(NOTES, "1.0.1").extension_contribution("acme.points/source"));
    rig.advance(secs(1));
    let events = server.extension_events();
    assert!(events.len() > events_before);
    assert!(events.last().expect("event").contains("acme.points/source"));
    assert!(server
        .contributions(NOTES)
        .contains(&"acme.points/source".to_owned()));
}

/// REQ: LIF-225
#[test]
fn lif_225_a_changed_activation_trigger_is_re_registered_at_the_swap() {
    let (rig, server) = dev_rig();
    let dev = rig.dev_start(&pkg(NOTES, "1.0.0").activation_trigger("on-startup"), &[]);
    rig.advance(secs(3));
    dev.reload(&pkg(NOTES, "1.0.1").activation_trigger("on-command:acme.run"));
    assert_eq!(
        server.triggers(NOTES),
        vec!["on-command:acme.run".to_owned()]
    );
}

/// REQ: LIF-225
#[test]
fn lif_225_a_reload_without_contribution_changes_sends_no_extensions_changed() {
    let (rig, server) = dev_rig();
    let dev = rig.dev_start(&pkg(NOTES, "1.0.0"), &[]);
    rig.advance(secs(3));
    let before = server.extension_events().len();
    dev.reload(&pkg(NOTES, "1.0.1"));
    assert_eq!(server.extension_events().len(), before);
}

/// REQ: LIF-226
#[test]
fn lif_226_dev_mode_counts_failures_but_never_stops_the_plugin() {
    let (rig, server) = dev_rig();
    let _dev = rig.dev_start(&pkg(NOTES, "1.0.0"), &[]);
    rig.advance(secs(3));
    for _ in 0..4 {
        server.fail(NOTES, Side::Server);
        rig.advance(secs(6));
    }
    assert!(!server.is_disabled_by_failure(NOTES));
    assert_eq!(server.failure_count(NOTES, Side::Server), 4);
    assert_eq!(
        server.state_of(NOTES, Side::Server),
        ilium_plugin_api::generated::host::plugins::InstanceState::Active
    );
}

/// REQ: LIF-226
#[test]
fn lif_226_the_third_and_later_failures_restart_after_5_seconds() {
    let (rig, server) = dev_rig();
    let _dev = rig.dev_start(&pkg(NOTES, "1.0.0"), &[]);
    rig.advance(secs(3));
    for _ in 0..2 {
        server.fail(NOTES, Side::Server);
        rig.advance(secs(6));
    }
    server.fail(NOTES, Side::Server);
    assert_eq!(server.restart_delay_ms(NOTES, Side::Server), Some(5_000));
    server.fail(NOTES, Side::Server);
    assert_eq!(server.restart_delay_ms(NOTES, Side::Server), Some(5_000));
}

/// REQ: LIF-226
#[test]
fn lif_226_a_successful_reload_clears_the_failure_window() {
    let (rig, server) = dev_rig();
    let dev = rig.dev_start(&pkg(NOTES, "1.0.0"), &[]);
    rig.advance(secs(3));
    for _ in 0..3 {
        server.fail(NOTES, Side::Server);
        rig.advance(secs(6));
    }
    dev.reload(&pkg(NOTES, "1.0.1"));
    assert_eq!(server.failure_count(NOTES, Side::Server), 0);
}

/// REQ: LIF-227
#[test]
fn lif_227_build_error_keeps_the_running_instance_and_prints_one_build_error_line() {
    let (rig, server) = dev_rig();
    let dev = rig.dev_start(&pkg(NOTES, "1.0.0"), &[]);
    rig.advance(secs(3));
    let deactivations = server.deactivations(NOTES).len();
    dev.reload_failing_build();
    let errors: Vec<_> = dev
        .output_lines()
        .into_iter()
        .filter(|l| l["type"] == "build-error")
        .collect();
    assert_eq!(errors.len(), 1);
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.0.0"));
    assert_eq!(server.deactivations(NOTES).len(), deactivations);
}

/// REQ: LIF-227
#[test]
fn lif_227_manifest_error_keeps_the_running_instance_and_sends_no_deactivate() {
    let (rig, server) = dev_rig();
    let dev = rig.dev_start(&pkg(NOTES, "1.0.0"), &[]);
    rig.advance(secs(3));
    let deactivations = server.deactivations(NOTES).len();
    dev.reload(&pkg(NOTES, "1.0.1").bad_manifest());
    let errors: Vec<_> = dev
        .output_lines()
        .into_iter()
        .filter(|l| l["type"] == "build-error")
        .collect();
    assert_eq!(errors.len(), 1);
    assert_eq!(server.deactivations(NOTES).len(), deactivations);
}

/// REQ: LIF-228
#[test]
fn lif_228_restore_bytes_pass_to_the_new_build_when_data_version_is_equal() {
    let (rig, server) = dev_rig();
    let dev = rig.dev_start(&pkg(NOTES, "1.0.0").data_version(1).restore_bytes(8), &[]);
    rig.advance(secs(3));
    dev.reload(&pkg(NOTES, "1.0.1").data_version(1).restore_bytes(8));
    assert_eq!(server.last_restored_len(NOTES), Some(8));
}

/// REQ: LIF-228
#[test]
fn lif_228_restore_bytes_are_dropped_when_data_version_changes() {
    let (rig, server) = dev_rig();
    let dev = rig.dev_start(&pkg(NOTES, "1.0.0").data_version(1).restore_bytes(8), &[]);
    rig.advance(secs(3));
    dev.reload(
        &pkg(NOTES, "1.0.1")
            .data_version(2)
            .restore_bytes(8)
            .migrator_both(),
    );
    assert_eq!(server.last_restored_len(NOTES), None);
    assert!(server
        .log_codes()
        .contains(&"restore.dropped-data-version".to_owned()));
}

/// REQ: LIF-229
#[test]
fn lif_229_dev_process_renews_the_lease_every_2_seconds() {
    let (rig, _server) = dev_rig();
    let dev = rig.dev_start(&pkg(NOTES, "1.0.0"), &[]);
    rig.advance(secs(10));
    let renewals = dev.lease_renewals();
    assert!((4..=6).contains(&renewals), "renewals: {renewals}");
}

/// REQ: LIF-229
#[test]
fn lif_229_killed_dev_process_loses_the_lease_and_the_installed_version_returns_within_10_seconds()
{
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    let dev = rig.dev_start(&pkg(NOTES, "2.0.0"), &[]);
    rig.advance(secs(3));
    dev.kill_without_cleanup();
    rig.advance(secs(10));
    rig.advance(ms(100));
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.0.0"));
    assert!(!server.has_dev_package(NOTES));
}

/// REQ: LIF-229
#[test]
fn lif_229_lease_not_yet_expired_after_9_seconds_keeps_the_dev_package() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    let dev = rig.dev_start(&pkg(NOTES, "2.0.0"), &[]);
    rig.advance(secs(3));
    dev.pause_renewals();
    rig.advance(secs(9));
    assert!(server.has_dev_package(NOTES));
}

/// REQ: LIF-229
#[test]
fn lif_229_closed_connection_removes_the_dev_package_at_once() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    let dev = rig.dev_start(&pkg(NOTES, "2.0.0"), &[]);
    rig.advance(secs(3));
    dev.close_connection();
    rig.advance(ms(100));
    assert!(!server.has_dev_package(NOTES));
}

/// REQ: LIF-271
#[test]
fn lif_271_a_trap_releases_the_input_lock_stream_and_child_process() {
    let rig = rig();
    install_ok(
        &rig,
        &pkg(NOTES, "1.0.0").grant("process.spawn").grant("net.http"),
    );
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    let held = server.hold_effects(NOTES);
    assert!(held.input_lock && held.stream_open && held.child_running);
    server.fail(NOTES, Side::Server);
    rig.advance(secs(1));
    let after = server.effects_of(NOTES);
    assert!(!after.input_lock, "input lock still held");
    assert!(!after.stream_open, "stream still open");
    assert!(!after.child_running, "child still running");
}

/// REQ: LIF-271
#[test]
fn lif_271_effects_of_the_dead_instance_are_released_too_not_only_the_new_one() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("process.spawn"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    let held = server.hold_effects(NOTES);
    server.fail(NOTES, Side::Server);
    rig.advance(secs(6));
    assert!(!server.effects_of_instance(held.instance).any());
}

/// REQ: LIF-271
#[test]
fn lif_271_a_stop_by_disable_releases_the_same_effects() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("process.spawn"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    server.hold_effects(NOTES);
    server.disable(NOTES);
    rig.advance(secs(2));
    assert!(!server.effects_of(NOTES).any());
}

/// REQ: LIF-275
#[test]
fn lif_275_an_update_resets_the_not_now_counter_of_a_bundled_plugin() {
    let rig = rig();
    let server = rig.start_server("s1");
    rig.install_bundled(&pkg("ilium.bundled.demo", "1.0.0").first_use_prompt("net.http"));
    rig.advance(secs(3));
    server.answer_first_use("ilium.bundled.demo", "net.http", "not-now");
    server.answer_first_use("ilium.bundled.demo", "net.http", "not-now");
    assert_eq!(server.not_now_count("ilium.bundled.demo", "net.http"), 2);
    rig.update_bundled(&pkg("ilium.bundled.demo", "1.1.0").first_use_prompt("net.http"));
    rig.advance(secs(3));
    assert_eq!(server.not_now_count("ilium.bundled.demo", "net.http"), 0);
    server.trigger_first_use("ilium.bundled.demo", "net.http");
    assert_eq!(server.prompts_for("ilium.bundled.demo", "net.http"), 3);
}

/// REQ: LIF-275
#[test]
fn lif_275_an_update_of_a_non_bundled_plugin_keeps_the_counter() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").first_use_prompt("net.http"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    server.answer_first_use(NOTES, "net.http", "not-now");
    update(&rig, &pkg(NOTES, "1.1.0").first_use_prompt("net.http"));
    rig.advance(secs(3));
    assert_eq!(server.not_now_count(NOTES, "net.http"), 1);
}
