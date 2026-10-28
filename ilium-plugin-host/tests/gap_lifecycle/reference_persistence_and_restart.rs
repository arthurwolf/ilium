//! Gap tests for the persistence and restart reference: monotonic timers and backup age pruning
//! (LIF-317, LIF-318).
//!
//! Assumptions: `jump_wall_clock` moves only the wall clock (signed whole seconds), `suspend_for`
//! advances the monotonic clock as a suspend would, `timer_firings` counts fires of a durable
//! timer, `backup_names` lists a plugin's pre-update backups, and `prune_backups` runs the
//! prune step that a server start performs.

use crate::support::*;

/// REQ: LIF-317
#[test]
fn lif_317_wall_clock_jump_and_suspend_fire_a_due_timer_once_and_never_early() {
    assert_manual_says(
        "reference-persistence-and-restart",
        "Durable timers, update deadlines and the failure window MUST run on the monotonic clock",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    let due_unix_ms: u64 = 1_800_000_000_000;
    rig.plant_durable_timer(NOTES, due_unix_ms);
    rig.jump_wall_clock(60 * 86_400);
    assert_eq!(
        rig.timer_firings(NOTES),
        0,
        "a wall-clock jump forward fires no timer early"
    );
    rig.jump_wall_clock(-60 * 86_400);
    rig.suspend_for(days(2));
    rig.advance(days(2));
    server.apply_pending();
    assert_eq!(
        rig.timer_firings(NOTES),
        1,
        "a timer that came due during a suspend fires exactly once after resume"
    );
}

/// REQ: LIF-318
#[test]
fn lif_318_backup_is_kept_until_retention_passes_despite_wall_clock_jumps() {
    assert_manual_says(
        "reference-persistence-and-restart",
        "A backward wall-clock jump MUST NOT make the host prune a backup younger than its retention",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_backup(NOTES, "1.0.0.bak");
    rig.age_backups(NOTES, days(3));
    rig.jump_wall_clock(30 * 86_400);
    rig.jump_wall_clock(-30 * 86_400);
    rig.prune_backups();
    assert!(
        rig.backup_names(NOTES)
            .iter()
            .any(|name| name == "1.0.0.bak"),
        "a backup younger than its retention is kept after a wall-clock move"
    );
}

/// REQ: LIF-317
#[test]
fn lif_317_monotonic_advance_fires_a_past_due_timer_once_and_wall_jumps_do_not_refire_it() {
    assert_manual_says(
        "reference-persistence-and-restart",
        "A timer that came due during a suspend MUST fire once after resume, not once per missed period.",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    rig.plant_durable_timer(NOTES, 1_800_000_000_000);
    rig.advance(days(2));
    server.apply_pending();
    assert_eq!(
        rig.timer_firings(NOTES),
        1,
        "the monotonic advance fires the due timer once"
    );
    rig.jump_wall_clock(-60 * 86_400);
    server.apply_pending();
    assert_eq!(
        rig.timer_firings(NOTES),
        1,
        "a backward wall-clock jump does not fire the timer a second time"
    );
}

/// REQ: LIF-318
#[test]
fn lif_318_backup_older_than_retention_is_pruned_on_monotonic_age_alone() {
    assert_manual_says(
        "reference-persistence-and-restart",
        "Backup age pruning MUST use the age recorded as monotonic uptime plus the boot id",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_backup(NOTES, "1.0.0.bak");
    rig.age_backups(NOTES, days(365));
    rig.prune_backups();
    assert!(
        !rig.backup_names(NOTES)
            .iter()
            .any(|name| name == "1.0.0.bak"),
        "a backup far past its retention is pruned"
    );
}
