//! reference-host-aggregate-limits.md rows HST-510 and HST-511: installed and active ceilings.

use ilium_plugin_host::host_capacity::{effective_ceilings, CapacityHost};

/// REQ: HST-510
#[test]
fn hst_510_the_257th_install_is_refused_and_the_first_256_are_kept() {
    let mut host = CapacityHost::new(256, 128);
    for index in 0..256 {
        host.install(&format!("test.pkg-{index}"))
            .expect("within the installed ceiling");
    }
    assert!(host.install("test.pkg-256").is_err());
    assert_eq!(host.installed_count(), 256);
}

/// REQ: HST-510
#[test]
fn hst_510_the_129th_activation_is_refused_and_the_first_128_stay_active() {
    let mut host = CapacityHost::new(256, 128);
    for index in 0..129 {
        host.install(&format!("test.side-{index}"))
            .expect("installs within the ceiling");
    }
    for index in 0..128 {
        host.activate(&format!("test.side-{index}"))
            .expect("activations within the ceiling");
    }
    assert!(host.activate("test.side-128").is_err());
    assert_eq!(host.active_plugin_ids().len(), 128);
}

/// REQ: HST-510
#[test]
fn hst_510_policy_may_lower_the_ceilings_but_never_raise_them() {
    assert_eq!(effective_ceilings(10, 5), (10, 5));
    assert_eq!(effective_ceilings(1000, 1000), (256, 128));
}

/// REQ: HST-511
#[test]
fn hst_511_a_refused_activation_reports_host_capacity_and_keeps_the_active_set() {
    let mut host = CapacityHost::new(256, 2);
    for plugin_id in ["acme.a", "acme.b", "acme.c"] {
        host.install(plugin_id).expect("installs");
    }
    host.activate("acme.a").expect("first activation");
    host.activate("acme.b").expect("second activation");
    let refusal = host.activate("acme.c").expect_err("active ceiling reached");
    assert_eq!(refusal.reason, "host-capacity");
    assert_eq!(host.active_plugin_ids(), vec!["acme.a", "acme.b"]);
}

/// REQ: HST-511
#[test]
fn hst_511_a_plugin_left_inactive_is_listed_by_plugin_status() {
    let mut host = CapacityHost::new(256, 1);
    host.install("acme.a").expect("installs");
    host.install("acme.b").expect("installs");
    host.activate("acme.a").expect("activates");
    assert!(host.activate("acme.b").is_err());
    assert!(host.status_listing().contains("acme.b"));
}
