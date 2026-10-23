//! Gap tests for the operator upgrade guide: staged rollouts and rollout commands (LIF-306..308).
//!
//! Assumptions: a rollout is a policy entry `rollout = { update, stage_percent }` under the
//! `[plugins."<id>"]` table; a host's stage membership comes from its identity; the list shows
//! `held` for a host outside the stage. The guide names the commands, not these shapes.

use crate::support::*;

/// REQ: LIF-306
#[test]
fn lif_306_stage_applies_update_only_on_hosts_inside_the_share() {
    assert_manual_says(
        "guide-operator-upgrades",
        "A policy MUST be able to assign an update to a rollout stage",
    );
    let fleet: Vec<OpsRig> = (0..10).map(|_| rig()).collect();
    for (index, host) in fleet.iter().enumerate() {
        host.set_host_identity(index as u32);
        install_ok(host, &pkg(NOTES, "1.0.0"));
        host.write_policy(&rollout_policy(NOTES, "2.0.0", 10));
        host.reload_policy();
        update(host, &pkg(NOTES, "2.0.0"));
    }
    let applied = fleet
        .iter()
        .filter(|host| list_row(host, NOTES)["version"] == "2.0.0")
        .count();
    assert_eq!(
        applied, 1,
        "a 10 percent stage over 10 hosts applies on one host"
    );
    for host in &fleet {
        let row = list_row(host, NOTES);
        if row["version"] != "2.0.0" {
            assert_eq!(row["state"], "held", "hosts outside the stage report held");
        }
    }
}

/// REQ: LIF-307
#[test]
fn lif_307_failure_in_first_stage_halts_widening_and_logs_one_halt() {
    assert_manual_says(
        "guide-operator-upgrades",
        "A rollout MUST halt on its own when the plugin fails inside the failure window",
    );
    let fleet: Vec<OpsRig> = (0..10).map(|_| rig()).collect();
    let mut servers = Vec::new();
    for (index, host) in fleet.iter().enumerate() {
        host.set_host_identity(index as u32);
        install_ok(host, &pkg(NOTES, "1.0.0"));
        host.write_policy(&rollout_policy(NOTES, "2.0.0", 10));
        host.reload_policy();
        servers.push(host.start_server("default"));
    }
    servers[0].fail_activation_of_version("2.0.0");
    update(&fleet[0], &pkg(NOTES, "2.0.0"));
    servers[0].fail(NOTES, Side::Server);
    assert_eq!(log_count(&servers[0], "rollout.halt"), 1);
    for host in &fleet[1..] {
        assert_eq!(
            list_row(host, NOTES)["state"],
            "held",
            "later stages stay held after the halt"
        );
    }
}

/// REQ: LIF-308
#[test]
fn lif_308_resume_and_cancel_change_stage_and_write_one_audit_event_each() {
    assert_manual_says(
        "guide-operator-upgrades",
        "An operator MUST be able to resume a halted rollout or cancel it",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.write_policy(&rollout_policy(NOTES, "2.0.0", 10));
    rig.reload_policy();
    let before = rig.audit_events().len();
    let resumed = rig.cli(&["plugin", "rollout", "resume", NOTES]);
    assert_eq!(
        resumed.exit_code, 0,
        "resume must succeed on a halted rollout"
    );
    assert_eq!(
        rig.audit_events().len(),
        before + 1,
        "resume writes one audit event"
    );
    assert_eq!(rig.rollout_state(NOTES).as_deref(), Some("widening"));
    let cancelled = rig.cli(&["plugin", "rollout", "cancel", NOTES]);
    assert_eq!(cancelled.exit_code, 0, "cancel must succeed");
    assert_eq!(
        rig.audit_events().len(),
        before + 2,
        "cancel writes one audit event"
    );
    assert_eq!(rig.rollout_state(NOTES).as_deref(), Some("cancelled"));
}
