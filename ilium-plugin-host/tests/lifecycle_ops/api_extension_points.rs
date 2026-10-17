//! Extension points page (LIF-270, LIF-274).

use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_api::Side;

use crate::common::*;

const OWNER: &str = "com.example.owner";
const CONTRIBUTOR: &str = "com.example.contrib";

fn owner_and_contributor(rig: &OpsRig) {
    install_ok(
        rig,
        &pkg(OWNER, "1.0.0").extension_point("acme.points/source"),
    );
    install_ok(
        rig,
        &pkg(CONTRIBUTOR, "1.0.0").extension_contribution("acme.points/source"),
    );
}

/// REQ: LIF-270
#[test]
fn lif_270_extension_stopped_after_three_breaches_is_usable_again_after_a_contributor_restart() {
    let rig = rig();
    owner_and_contributor(&rig);
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    for _ in 0..3 {
        server.breach_extension(OWNER, "acme.points/source", CONTRIBUTOR);
    }
    assert!(server.extension_stopped(OWNER, "acme.points/source", CONTRIBUTOR));
    rig.cli(&["restart", CONTRIBUTOR]);
    rig.advance(secs(3));
    assert!(!server.extension_stopped(OWNER, "acme.points/source", CONTRIBUTOR));
    assert!(server
        .call_extension(OWNER, "acme.points/source", CONTRIBUTOR)
        .is_ok());
}

/// REQ: LIF-270
#[test]
fn lif_270_the_stop_is_not_persisted_across_a_server_restart() {
    let rig = rig();
    owner_and_contributor(&rig);
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    for _ in 0..3 {
        server.breach_extension(OWNER, "acme.points/source", CONTRIBUTOR);
    }
    server.crash();
    let fresh = rig.start_server("s1");
    rig.advance(secs(3));
    assert!(!fresh.extension_stopped(OWNER, "acme.points/source", CONTRIBUTOR));
    assert!(!rig.registry_bytes().windows(7).any(|w| w == b"stopped"));
}

/// REQ: LIF-270
#[test]
fn lif_270_two_breaches_do_not_stop_the_extension() {
    let rig = rig();
    owner_and_contributor(&rig);
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    for _ in 0..2 {
        server.breach_extension(OWNER, "acme.points/source", CONTRIBUTOR);
    }
    assert!(!server.extension_stopped(OWNER, "acme.points/source", CONTRIBUTOR));
}

/// REQ: LIF-270
#[test]
fn lif_270_reset_failures_also_ends_the_stop() {
    let rig = rig();
    owner_and_contributor(&rig);
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    for _ in 0..3 {
        server.breach_extension(OWNER, "acme.points/source", CONTRIBUTOR);
    }
    rig.cli(&["reset-failures", CONTRIBUTOR]);
    assert!(!server.extension_stopped(OWNER, "acme.points/source", CONTRIBUTOR));
}

/// REQ: LIF-274
#[test]
fn lif_274_inactive_contributors_of_an_active_owner_are_activated_without_a_call() {
    let rig = rig();
    owner_and_contributor(&rig);
    let server = rig.start_server("s1");
    rig.advance(secs(5));
    assert_eq!(server.state_of(OWNER, Side::Server), InstanceState::Active);
    assert_eq!(
        server.state_of(CONTRIBUTOR, Side::Server),
        InstanceState::Active
    );
}

/// REQ: LIF-274
#[test]
fn lif_274_contributors_are_activated_in_the_background_not_inside_the_owner_activation() {
    let rig = rig();
    owner_and_contributor(&rig);
    let server = rig.start_server_manual("s1");
    server.apply_pending();
    let trace = rig.trace();
    assert!(
        position_of(&trace, &format!("activate.end {OWNER}"))
            < position_of(&trace, &format!("activate.begin {CONTRIBUTOR}"))
    );
}

/// REQ: LIF-274
#[test]
fn lif_274_a_disabled_contributor_stays_inactive() {
    let rig = rig();
    owner_and_contributor(&rig);
    rig.cli(&["disable", CONTRIBUTOR]);
    let server = rig.start_server("s1");
    rig.advance(secs(5));
    assert_eq!(
        server.state_of(CONTRIBUTOR, Side::Server),
        InstanceState::Inactive
    );
}
