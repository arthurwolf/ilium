//! Services page (LIF-280).

use ilium_plugin_api::Side;

use crate::common::*;

/// REQ: LIF-280
#[test]
fn lif_280_a_provider_failure_does_not_count_for_its_consumers() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").provides_service("svc.notes"));
    install_ok(&rig, &pkg(OTHER, "1.0.0").requires_service("svc.notes"));
    let server = rig.start_server("s1");
    rig.advance(secs(5));
    server.fail(NOTES, Side::Server);
    rig.advance(secs(2));
    assert_eq!(server.failure_count(NOTES, Side::Server), 1);
    assert_eq!(server.failure_count(OTHER, Side::Server), 0);
}

/// REQ: LIF-280
#[test]
fn lif_280_three_provider_traps_do_not_disable_the_consumer() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").provides_service("svc.notes"));
    install_ok(&rig, &pkg(OTHER, "1.0.0").requires_service("svc.notes"));
    let server = rig.start_server("s1");
    rig.advance(secs(5));
    for _ in 0..3 {
        server.fail(NOTES, Side::Server);
        rig.advance(secs(6));
    }
    assert!(server.is_disabled_by_failure(NOTES));
    assert!(!server.is_disabled_by_failure(OTHER));
    assert_eq!(server.failure_count(OTHER, Side::Server), 0);
}

/// REQ: LIF-280
#[test]
fn lif_280_a_failed_call_into_a_trapped_provider_is_an_error_result_not_a_consumer_failure() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").provides_service("svc.notes"));
    install_ok(&rig, &pkg(OTHER, "1.0.0").requires_service("svc.notes"));
    let server = rig.start_server("s1");
    rig.advance(secs(5));
    server.fail(NOTES, Side::Server);
    let result = server.call_service(OTHER, "svc.notes", "ping");
    assert!(result.is_err());
    assert_eq!(server.failure_count(OTHER, Side::Server), 0);
}
