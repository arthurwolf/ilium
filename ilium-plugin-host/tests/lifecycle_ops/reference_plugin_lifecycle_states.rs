//! Reference, plugin lifecycle states (LIF-283..285): package states P13-P16, waiting-dependency
//! I16-I18 and the failure-window scope.

use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_api::Side;

use crate::common::*;

fn signed_install(rig: &OpsRig) {
    rig.trust_publisher("key-a");
    install_ok(rig, &pkg(NOTES, "1.0.0").signed("key-a").grant("fs.read"));
    rig.plant_data(NOTES, 1);
    rig.plant_settings(NOTES, "theme", "dark");
}

fn assert_state_kept(rig: &OpsRig, server: &ServerHandle, state: &str) {
    assert_eq!(registry_entry(rig, NOTES)["state"], state);
    assert_eq!(server.active_version(NOTES), None, "must not run");
    assert!(rig.data_dir(NOTES).exists());
    assert_eq!(rig.setting(NOTES, "theme").as_deref(), Some("dark"));
    assert!(!rig.grants_json()[NOTES].is_null());
}

/// REQ: LIF-283
#[test]
fn lif_283_removing_the_signing_key_with_force_gives_disabled_untrusted_and_survives_restart() {
    let rig = rig();
    signed_install(&rig);
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    let output = rig.cli(&["trust", "remove", "key-a", "--force"]);
    assert_eq!(output.exit_code, 0);
    rig.advance(secs(3));
    assert_state_kept(&rig, &server, "disabled-untrusted");
    server.crash();
    let fresh = rig.start_server("s1");
    rig.advance(secs(3));
    assert_state_kept(&rig, &fresh, "disabled-untrusted");
}

/// REQ: LIF-283
#[test]
fn lif_283_trusting_the_key_again_leaves_disabled_untrusted_and_runs_the_plugin() {
    let rig = rig();
    signed_install(&rig);
    let server = rig.start_server("s1");
    rig.cli(&["trust", "remove", "key-a", "--force"]);
    rig.trust_publisher("key-a");
    rig.advance(secs(3));
    assert_eq!(registry_entry(&rig, NOTES)["state"], "installed-enabled");
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.0.0"));
}

/// REQ: LIF-283
#[test]
fn lif_283_enable_accept_risk_leaves_disabled_untrusted() {
    let rig = rig();
    signed_install(&rig);
    let server = rig.start_server("s1");
    rig.cli(&["trust", "remove", "key-a", "--force"]);
    let output = rig.cli(&["enable", NOTES, "--accept-risk"]);
    assert_eq!(output.exit_code, 0);
    rig.advance(secs(3));
    assert_eq!(registry_entry(&rig, NOTES)["state"], "installed-enabled");
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.0.0"));
}

/// REQ: LIF-283
#[test]
fn lif_283_plain_enable_does_not_leave_disabled_untrusted() {
    let rig = rig();
    signed_install(&rig);
    let _server = rig.start_server("s1");
    rig.cli(&["trust", "remove", "key-a", "--force"]);
    rig.cli(&["enable", NOTES]);
    assert_eq!(registry_entry(&rig, NOTES)["state"], "disabled-untrusted");
}

/// REQ: LIF-283
#[test]
fn lif_283_a_key_revoked_as_compromised_gives_disabled_blocked_and_survives_restart() {
    let rig = rig();
    signed_install(&rig);
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    rig.revoke_key("key-a", "compromised");
    rig.advance(secs(3));
    assert_state_kept(&rig, &server, "disabled-blocked");
    server.crash();
    let fresh = rig.start_server("s1");
    rig.advance(secs(3));
    assert_state_kept(&rig, &fresh, "disabled-blocked");
}

/// REQ: LIF-283
#[test]
fn lif_283_enable_accept_risk_leaves_disabled_blocked() {
    let rig = rig();
    signed_install(&rig);
    let server = rig.start_server("s1");
    rig.revoke_key("key-a", "compromised");
    rig.cli(&["enable", NOTES, "--accept-risk"]);
    rig.advance(secs(3));
    assert_eq!(registry_entry(&rig, NOTES)["state"], "installed-enabled");
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.0.0"));
}

/// REQ: LIF-283
#[test]
fn lif_283_an_update_signed_by_another_trusted_key_of_the_publisher_leaves_disabled_blocked() {
    let rig = rig();
    signed_install(&rig);
    rig.trust_publisher("key-b");
    let _server = rig.start_server("s1");
    rig.revoke_key("key-a", "compromised");
    let output = update(&rig, &pkg(NOTES, "1.1.0").signed("key-b").same_publisher_as("key-a"));
    assert_eq!(output.exit_code, 0);
    assert_eq!(registry_entry(&rig, NOTES)["state"], "installed-enabled");
}

/// REQ: LIF-283
#[test]
fn lif_283_a_takedown_of_the_installed_version_gives_disabled_blocked() {
    let rig = rig();
    signed_install(&rig);
    let _server = rig.start_server("s1");
    rig.registry_takedown(NOTES, "1.0.0");
    assert_eq!(registry_entry(&rig, NOTES)["state"], "disabled-blocked");
}

/// REQ: LIF-283
#[test]
fn lif_283_leaving_either_state_clears_the_failure_windows() {
    let rig = rig();
    signed_install(&rig);
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    server.fail(NOTES, Side::Server);
    server.fail(NOTES, Side::Server);
    rig.cli(&["trust", "remove", "key-a", "--force"]);
    rig.cli(&["enable", NOTES, "--accept-risk"]);
    rig.advance(secs(3));
    assert_eq!(server.failure_count(NOTES, Side::Server), 0);
}

fn consumer_waiting_rig(wait_ms: u32) -> (OpsRig, ServerHandle) {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").provides_service("svc.notes"));
    install_ok(
        &rig,
        &pkg(OTHER, "1.0.0").requires_service("svc.notes").wait_ms(wait_ms),
    );
    rig.cli(&["disable", NOTES]);
    let server = rig.start_server("s1");
    rig.advance(secs(1));
    (rig, server)
}

/// REQ: LIF-284
#[test]
fn lif_284_consumer_with_a_disabled_provider_waits_without_counting_a_failure() {
    let (rig, server) = consumer_waiting_rig(5_000);
    rig.advance(secs(1));
    assert_eq!(server.state_of(OTHER, Side::Server), InstanceState::WaitingDependency);
    assert_eq!(server.failure_count(OTHER, Side::Server), 0);
}

/// REQ: LIF-284
#[test]
fn lif_284_enabling_the_provider_moves_the_consumer_to_activating() {
    let (rig, server) = consumer_waiting_rig(5_000);
    rig.advance(secs(1));
    rig.cli(&["enable", NOTES]);
    rig.advance(ms(500));
    let state = server.state_of(OTHER, Side::Server);
    assert!(matches!(state, InstanceState::Activating | InstanceState::Active), "{state:?}");
}

/// REQ: LIF-284
#[test]
fn lif_284_provider_that_stays_off_longer_than_wait_ms_leaves_the_consumer_inactive() {
    let (rig, server) = consumer_waiting_rig(2_000);
    rig.advance(secs(3));
    assert_eq!(server.state_of(OTHER, Side::Server), InstanceState::Inactive);
    assert_eq!(server.inactive_reason(OTHER).as_deref(), Some("dependency.unresolved"));
    assert_eq!(server.failure_count(OTHER, Side::Server), 0);
}

/// REQ: LIF-284
#[test]
fn lif_284_the_wait_is_not_over_just_before_wait_ms() {
    let (rig, server) = consumer_waiting_rig(2_000);
    rig.advance(ms(1_900));
    assert_eq!(server.state_of(OTHER, Side::Server), InstanceState::WaitingDependency);
}

/// REQ: LIF-284
#[test]
fn lif_284_retries_run_after_1_5_and_30_seconds_then_every_60_seconds() {
    let (rig, server) = consumer_waiting_rig(100);
    rig.advance(secs(200));
    let retries = server.dependency_retry_times(OTHER);
    let seconds: Vec<u64> = retries.iter().map(|t| t.as_secs()).collect();
    assert!(seconds.starts_with(&[1, 6, 36]), "{seconds:?}");
    assert!(seconds[3..].windows(2).all(|pair| pair[1] - pair[0] == 60), "{seconds:?}");
}

/// REQ: LIF-284
#[test]
fn lif_284_a_provider_becoming_active_later_activates_the_inactive_consumer() {
    let (rig, server) = consumer_waiting_rig(100);
    rig.advance(secs(3));
    rig.cli(&["enable", NOTES]);
    rig.advance(secs(2));
    assert_eq!(server.state_of(OTHER, Side::Server), InstanceState::Active);
}

/// REQ: LIF-285
#[test]
fn lif_285_changing_the_active_version_empties_the_failure_window() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    for _ in 0..2 {
        server.fail(NOTES, Side::Server);
        rig.advance(secs(6));
    }
    update(&rig, &pkg(NOTES, "1.1.0"));
    rig.advance(secs(3));
    server.fail(NOTES, Side::Server);
    assert_eq!(server.failure_count(NOTES, Side::Server), 1);
}

/// REQ: LIF-285
#[test]
fn lif_285_dev_reload_empties_the_failure_window() {
    let rig = rig();
    let server = rig.start_server("s1");
    let dev = rig.dev_start(&pkg(NOTES, "1.0.0"), &[]);
    rig.advance(secs(3));
    for _ in 0..2 {
        server.fail(NOTES, Side::Server);
        rig.advance(secs(6));
    }
    dev.reload(&pkg(NOTES, "1.0.1"));
    server.fail(NOTES, Side::Server);
    assert_eq!(server.failure_count(NOTES, Side::Server), 1);
}

/// REQ: LIF-285
#[test]
fn lif_285_manual_enable_empties_the_failure_window() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    for _ in 0..2 {
        server.fail(NOTES, Side::Server);
        rig.advance(secs(6));
    }
    rig.cli(&["enable", NOTES]);
    rig.advance(secs(3));
    server.fail(NOTES, Side::Server);
    assert_eq!(server.failure_count(NOTES, Side::Server), 1);
}

/// REQ: LIF-285
#[test]
fn lif_285_first_failure_of_an_update_candidate_leaves_the_window_empty() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    update(&rig, &pkg(NOTES, "1.1.0").activate_error());
    rig.advance(secs(3));
    assert_eq!(server.failure_count(NOTES, Side::Server), 0);
}

/// REQ: LIF-285
#[test]
fn lif_285_manual_restart_neither_counts_nor_empties_the_window() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    server.fail(NOTES, Side::Server);
    rig.advance(secs(6));
    let before = server.failure_count(NOTES, Side::Server);
    rig.cli(&["restart", NOTES]);
    rig.advance(secs(3));
    assert_eq!(server.failure_count(NOTES, Side::Server), before);
}

/// REQ: LIF-285
#[test]
fn lif_285_the_window_is_per_side() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").client_side());
    let server = rig.start_server("s1");
    let _client = server.attach_client("c1");
    rig.advance(secs(3));
    server.fail(NOTES, Side::Server);
    assert_eq!(server.failure_count(NOTES, Side::Server), 1);
    assert_eq!(server.failure_count(NOTES, Side::Client), 0);
}

/// REQ: LIF-285
#[test]
fn lif_285_the_window_is_per_session() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let a = rig.start_server("a");
    let b = rig.start_server("b");
    rig.advance(secs(3));
    a.fail(NOTES, Side::Server);
    assert_eq!(a.failure_count(NOTES, Side::Server), 1);
    assert_eq!(b.failure_count(NOTES, Side::Server), 0);
}
