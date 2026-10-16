//! Extending core plugins page (LIF-273).

use ilium_plugin_api::Side;

use crate::common::*;

/// REQ: LIF-273
#[test]
fn lif_273_a_replacement_disabled_after_three_crashes_re_enables_the_core_plugin_with_a_notice() {
    let rig = rig();
    let server = rig.start_server("s1");
    rig.install_replacement_of_core(&pkg("com.example.replace", "1.0.0"), "ilium.core.tree");
    rig.advance(secs(3));
    for _ in 0..3 {
        server.fail("com.example.replace", Side::Server);
        rig.advance(secs(6));
    }
    assert!(server.is_enabled("ilium.core.tree"));
    let notices = server.notices();
    assert!(notices.iter().any(|text| text.contains("ilium.core.tree") && text.contains("com.example.replace")), "{notices:?}");
}

/// REQ: LIF-273
#[test]
fn lif_273_the_notice_is_visible_exactly_once_per_disable() {
    let rig = rig();
    let server = rig.start_server("s1");
    rig.install_replacement_of_core(&pkg("com.example.replace", "1.0.0"), "ilium.core.tree");
    rig.advance(secs(3));
    for _ in 0..3 {
        server.fail("com.example.replace", Side::Server);
        rig.advance(secs(6));
    }
    let count = server.notices().iter().filter(|text| text.contains("ilium.core.tree")).count();
    assert_eq!(count, 1);
}

/// REQ: LIF-273
#[test]
fn lif_273_a_replacement_that_is_manually_disabled_also_re_enables_the_core_plugin() {
    let rig = rig();
    let server = rig.start_server("s1");
    rig.install_replacement_of_core(&pkg("com.example.replace", "1.0.0"), "ilium.core.tree");
    rig.advance(secs(3));
    server.disable("com.example.replace");
    assert!(server.is_enabled("ilium.core.tree"));
}
