//! reference-kernel-and-boot.md rows that are unit tests of the host crate (COR-023).

use ilium_plugin_api::grants::{check_grant, Grant};
use ilium_plugin_api::{Side, Tier};

/// REQ: COR-023
#[test]
fn cor_023_extensions_contribute_under_scene_fails_with_grant_side() {
    let grant = Grant::parse("extensions.contribute:ilium.core.notifications/before-notify").expect("parses");
    let diagnostic = check_grant(&grant, Tier::T1, Side::Scene).expect_err("refused on the scene side");
    assert_eq!(diagnostic.code, "manifest.grant-side");
}

/// REQ: COR-023
#[test]
fn cor_023_extensions_provide_under_scene_fails_with_grant_side() {
    let grant = Grant::parse("extensions.provide:acme.point").expect("parses");
    let diagnostic = check_grant(&grant, Tier::T1, Side::Scene).expect_err("refused");
    assert_eq!(diagnostic.code, "manifest.grant-side");
}

/// REQ: COR-023
#[test]
fn cor_023_extensions_contribute_is_allowed_on_server_and_client() {
    let grant = Grant::parse("extensions.contribute:ilium.core.notifications/before-notify").expect("parses");
    for side in [Side::Server, Side::Client] {
        assert!(check_grant(&grant, Tier::T1, side).is_ok(), "{side:?}");
    }
}
