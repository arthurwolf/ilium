//! Gap tests for the install and update reference: auto-update rules (LIF-727).
//!
//! Assumptions: `enable_auto_update` turns on automatic updates for a host; `trigger_auto_update`
//! runs one check at a safe point; `mark_package_state` sets `skip`, `pinned` or `retired` on an
//! installed id, as the operator commands do.

use crate::support::*;

/// REQ: LIF-727
#[test]
fn lif_727_auto_update_skips_each_excluded_case_and_logs_one_line_per_applied_update() {
    assert_manual_says(
        "reference-install-and-update",
        "Auto-update MUST apply only at a safe point",
    );
    let excluded_cases: Vec<(&str, PackageSpec)> = vec![
        ("adds a grant", pkg(NOTES, "1.1.0").grant("net.http")),
        (
            "changes the publisher key",
            pkg(NOTES, "1.1.0").signed("key-other"),
        ),
        ("crosses a major version", pkg(NOTES, "2.0.0")),
    ];
    for (case, spec) in excluded_cases {
        let rig = rig();
        install_ok(&rig, &pkg(NOTES, "1.0.0"));
        rig.enable_auto_update();
        rig.set_registry_candidate(&spec);
        rig.trigger_auto_update();
        assert_eq!(
            list_row(&rig, NOTES)["version"],
            "1.0.0",
            "auto-update must not apply a case that {case}"
        );
    }

    for state in ["skip", "pinned", "retired"] {
        let rig = rig();
        install_ok(&rig, &pkg(NOTES, "1.0.0"));
        rig.enable_auto_update();
        rig.mark_package_state(NOTES, state);
        rig.set_registry_candidate(&pkg(NOTES, "1.0.1"));
        rig.trigger_auto_update();
        assert_eq!(
            list_row(&rig, NOTES)["version"],
            "1.0.0",
            "auto-update skips a plugin in state {state}"
        );
    }

    let allowed = rig();
    install_ok(&allowed, &pkg(NOTES, "1.0.0"));
    allowed.enable_auto_update();
    allowed.set_registry_candidate(&pkg(NOTES, "1.0.1"));
    let server = allowed.start_server("default");
    allowed.trigger_auto_update();
    server.apply_pending();
    assert_eq!(
        list_row(&allowed, NOTES)["version"],
        "1.0.1",
        "a plain patch applies"
    );
    assert_eq!(
        log_count(&server, "update.auto"),
        1,
        "one update.auto line per applied update"
    );
}

/// REQ: LIF-727
#[test]
fn lif_727_a_plain_patch_waits_for_the_safe_point_before_it_applies() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.enable_auto_update();
    rig.set_registry_candidate(&pkg(NOTES, "1.0.1"));
    let server = rig.start_server("default");
    rig.trigger_auto_update();
    assert_eq!(
        list_row(&rig, NOTES)["version"],
        "1.0.0",
        "the patch is not applied before the safe point runs"
    );
    server.apply_pending();
    assert_eq!(
        list_row(&rig, NOTES)["version"],
        "1.0.1",
        "the patch applies at the safe point"
    );
}
