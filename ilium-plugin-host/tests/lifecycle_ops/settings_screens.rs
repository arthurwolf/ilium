//! Settings screens page (LIF-272).

use ilium_plugin_api::Side;

use crate::common::*;

fn failed_three_times() -> (OpsRig, ServerHandle) {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    for _ in 0..3 {
        server.fail(NOTES, Side::Server);
        rig.advance(secs(6));
    }
    (rig, server)
}

/// REQ: LIF-272
#[test]
fn lif_272_enable_from_the_recovery_page_clears_the_failure_window_and_activates() {
    let (rig, server) = failed_three_times();
    server.press_recovery_enable(NOTES);
    rig.advance(secs(3));
    assert_eq!(server.failure_count(NOTES, Side::Server), 0);
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.0.0"));
}

/// REQ: LIF-272
#[test]
fn lif_272_reset_failures_clears_the_window_without_activating() {
    let (rig, server) = failed_three_times();
    server.press_reset_failures(NOTES);
    rig.advance(secs(3));
    assert_eq!(server.failure_count(NOTES, Side::Server), 0);
    assert_eq!(server.active_version(NOTES), None);
}

/// REQ: LIF-272
#[test]
fn lif_272_after_reset_failures_one_more_failure_counts_as_the_first() {
    let (rig, server) = failed_three_times();
    server.press_reset_failures(NOTES);
    server.press_recovery_enable(NOTES);
    rig.advance(secs(3));
    server.fail(NOTES, Side::Server);
    assert_eq!(server.failure_count(NOTES, Side::Server), 1);
}
