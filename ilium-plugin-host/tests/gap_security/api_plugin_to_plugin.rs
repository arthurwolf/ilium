//! Plugin-to-plugin calls page (SEC-716).

use crate::support::*;

/// REQ: SEC-716
#[test]
fn sec_716_b_cannot_use_its_grant_for_a_request_that_originated_in_a_plugin_without_it() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    install_ok(&rig, &pkg(OTHER, "1.0.0").grant("fs.read"));
    let server = rig.start_server("s1");
    rig.advance(secs(2));
    let refused = server
        .call_on_behalf_of(NOTES, OTHER, "read-user-file")
        .expect_err("B must not read with its grant for A, which lacks fs.read");
    let text = format!("{refused:?}");
    assert!(
        text.contains("not-permitted"),
        "expected not-permitted, got {text}"
    );
    assert!(
        text.contains("confused-deputy"),
        "the detail must be confused-deputy, got {text}"
    );
}

/// REQ: SEC-716
#[test]
fn sec_716_the_same_chain_succeeds_when_the_origin_plugin_also_holds_the_grant() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("fs.read"));
    install_ok(&rig, &pkg(OTHER, "1.0.0").grant("fs.read"));
    let server = rig.start_server("s1");
    rig.advance(secs(2));
    server
        .call_on_behalf_of(NOTES, OTHER, "read-user-file")
        .expect("both plugins hold fs.read, so the chained call must be allowed");
}
