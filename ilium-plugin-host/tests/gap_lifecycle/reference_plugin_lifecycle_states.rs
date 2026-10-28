//! Gap tests for the plugin lifecycle states reference: retired state, the retirement transition,
//! disable causes, takedowns, held uninstalls, scoped enablement, timers across disable and the
//! subscriber queue (LIF-700, 701, 703, 705, 706, 718, 719, 720, 724).
//!
//! Assumptions: `plant_end_of_life` records an end-of-life date (YYYY-MM-DD) for an id;
//! `jump_wall_clock` moves only the wall clock (signed whole seconds); `state_change_events`
//! lists `plugin-state-changed` events as text; `hold_uninstall` keeps an uninstall in the
//! `uninstall-pending` state; `start_long_call` runs a call that does not return until released.

use ilium_plugin_api::lifecycle::DeactivationReason;

use crate::support::*;

/// REQ: LIF-700
#[test]
fn lif_700_end_of_life_date_leaves_plugin_retired_after_restart_not_user_disabled() {
    assert_manual_says(
        "reference-plugin-lifecycle-states",
        "The package-state table MUST contain a state `retired`",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_end_of_life(NOTES, "2026-01-01");
    let server = rig.start_server("default");
    server.shutdown();
    let _restarted = rig.start_server("default");
    let row = list_row(&rig, NOTES);
    assert_eq!(row["state"], "retired");
    assert_eq!(row["retired_reason"], "end-of-life");
    assert_ne!(row["state"], "installed-disabled-by-user");
}

/// REQ: LIF-701
#[test]
fn lif_701_retirement_sends_one_deactivate_emits_event_and_keeps_data() {
    assert_manual_says(
        "reference-plugin-lifecycle-states",
        "The package-state transition table MUST have a row P17",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.write_storage(NOTES, "global", "draft", b"kept");
    let server = rig.start_server("default");
    rig.plant_end_of_life(NOTES, "2026-01-01");
    rig.jump_wall_clock(40 * 86_400);
    server.apply_pending();
    assert_eq!(
        server.deactivations(NOTES).len(),
        1,
        "deactivate is sent once"
    );
    assert!(
        rig.state_change_events(NOTES)
            .iter()
            .any(|event| event.contains("retired")),
        "plugin-state-changed carries reason retired"
    );
    assert_eq!(log_count(&server, "plugin.retired"), 1);
    assert_eq!(
        rig.read_storage(NOTES, "global", "draft"),
        Some(b"kept".to_vec())
    );
}

/// REQ: LIF-703
#[test]
fn lif_703_enabling_a_retired_plugin_needs_accept_risk_and_plugin_set_enabled_is_refused() {
    assert_manual_says(
        "reference-plugin-lifecycle-states",
        "Enabling a `retired` plugin MUST need `ilium plugin enable <id> --accept-risk`",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_end_of_life(NOTES, "2026-01-01");
    let server = rig.start_server("default");
    server.apply_pending();
    let refused = rig.cli(&["plugin", "enable", NOTES]);
    assert_ne!(
        refused.exit_code, 0,
        "enable without --accept-risk is refused"
    );
    let accepted = rig.cli(&["plugin", "enable", NOTES, "--accept-risk"]);
    assert_eq!(
        accepted.exit_code, 0,
        "enable with --accept-risk is accepted"
    );
    let from_plugin = server.call_plugin(NOTES, "plugins.set-enabled");
    let error = from_plugin.expect_err("a plugin may not call plugins.set-enabled");
    assert!(
        format!("{error:?}").contains("not-permitted"),
        "plugins.set-enabled returns not-permitted: {error:?}"
    );
    let updated = update(&rig, &pkg(NOTES, "1.1.0"));
    assert_eq!(
        updated.exit_code, 0,
        "the update to a non-deprecated version is accepted"
    );
    assert_ne!(
        list_row(&rig, NOTES)["state"],
        "retired",
        "an update to a version that is not deprecated clears retired"
    );
}

/// REQ: LIF-705
#[test]
fn lif_705_key_revocation_and_takedown_send_one_deactivate_disabled_each() {
    assert_manual_says(
        "reference-plugin-lifecycle-states",
        "For transitions P6, P13, P15 and P17 (policy, untrusted, blocked, retired)",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").signed("key-revoked"));
    let server = rig.start_server("default");
    rig.trust_publisher("key-revoked");
    rig.revoke_key("key-revoked", "compromised");
    server.apply_pending();
    assert_eq!(
        server.deactivations(NOTES),
        vec![DeactivationReason::Disabled],
        "the untrusted transition sends exactly one deactivate(disabled)"
    );
    let takedown_rig = crate::support::rig();
    install_ok(&takedown_rig, &pkg(OTHER, "1.0.0"));
    let takedown_server = takedown_rig.start_server("default");
    takedown_rig.registry_takedown(OTHER, "1.0.0");
    takedown_server.apply_pending();
    assert_eq!(
        takedown_server.deactivations(OTHER),
        vec![DeactivationReason::Disabled],
        "the blocked transition sends exactly one deactivate(disabled), never a new reason"
    );
}

/// REQ: LIF-705
#[test]
fn lif_705_policy_and_retirement_both_send_one_deactivate_disabled_with_distinct_log_cause() {
    assert_manual_says(
        "reference-plugin-lifecycle-states",
        "the host MUST send `deactivate(disabled)` to each live instance",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    rig.write_policy(&kill_policy(NOTES, ">=1.0.0, <2.0.0"));
    rig.reload_policy();
    assert_eq!(
        server.deactivations(NOTES),
        vec![DeactivationReason::Disabled],
        "the policy transition sends exactly one deactivate(disabled)"
    );
    assert!(
        rig.state_change_events(NOTES)
            .iter()
            .any(|event| event.contains("policy")),
        "the distinct cause is recorded in plugin-state-changed.reason"
    );
}

/// REQ: LIF-706
#[test]
fn lif_706_takedown_mid_call_cancels_the_call_and_blocks_at_the_next_safe_point() {
    assert_manual_says(
        "reference-plugin-lifecycle-states",
        "A registry takedown or key revocation that arrives while a plugin is mid-call",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    let pending = rig.start_long_call(NOTES, "slow-export");
    rig.registry_takedown(NOTES, "1.0.0");
    server.apply_pending();
    assert_eq!(
        server.inactive_reason(NOTES).as_deref(),
        Some("blocked"),
        "the plugin moves to disabled-blocked"
    );
    assert_eq!(
        pending.outcome().as_deref(),
        Some("cancelled"),
        "the running call is cancelled to its caller"
    );
}

/// REQ: LIF-718
#[test]
fn lif_718_every_lifecycle_call_during_held_uninstall_returns_uninstalling_and_changes_nothing() {
    assert_manual_says(
        "reference-plugin-lifecycle-states",
        "While a plugin is `uninstall-pending`, `enable`, `disable`, `update`, `rollback` and `install`",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    install_ok(&rig, &pkg(NOTES, "1.1.0"));
    rig.hold_uninstall(NOTES);
    let before = rig.registry_json();
    for args in [
        vec!["plugin", "enable", NOTES],
        vec!["plugin", "disable", NOTES],
        vec!["plugin", "rollback", NOTES],
    ] {
        let output = rig.cli(&args);
        assert_eq!(output.exit_code, 4, "{args:?} returns exit 4");
        assert!(error_codes(&output).contains(&"install.uninstalling".to_string()));
    }
    let update_output = update(&rig, &pkg(NOTES, "1.2.0"));
    assert_eq!(
        update_output.exit_code, 4,
        "update returns exit 4 during uninstall"
    );
    assert_eq!(
        rig.registry_json(),
        before,
        "no lifecycle call changes any state"
    );
}

/// REQ: LIF-719
#[test]
fn lif_719_timer_due_during_disabled_period_fires_once_after_enable() {
    assert_manual_says(
        "reference-plugin-lifecycle-states",
        "On disable the host MUST stop accepting new calls at once",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    server.disable(NOTES);
    rig.plant_durable_timer(NOTES, 1_800_000_000_000);
    rig.advance(days(3));
    assert_eq!(
        rig.timer_firings(NOTES),
        0,
        "no durable timers run while disabled"
    );
    server.enable(NOTES);
    server.apply_pending();
    assert_eq!(rig.timer_firings(NOTES), 1, "one catch-up firing on enable");
}

/// REQ: LIF-720
#[test]
fn lif_720_scoped_set_enabled_changes_only_its_overlay_and_resolves_narrowest_first() {
    assert_manual_says(
        "reference-plugin-lifecycle-states",
        "`set-enabled` with scope `project` or `session` MUST only change the overlay for that scope",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let global_state = installed_json(&rig)["plugins"][NOTES]["state"].clone();
    rig.set_enabled_levels(NOTES, "s1", Some(true), Some(false), Some(true));
    let info = rig.cli(&["plugin", "info", NOTES, "--json", "--session", "s1"]);
    assert_eq!(
        info.stdout_jsonl[0]["enabled"], true,
        "the session overlay wins over the project overlay"
    );
    assert_eq!(
        installed_json(&rig)["plugins"][NOTES]["state"],
        global_state,
        "the global package state is unchanged"
    );
}

/// REQ: LIF-724
#[test]
fn lif_724_subscriber_overflow_drops_oldest_and_delivers_one_resync() {
    assert_manual_says(
        "reference-plugin-lifecycle-states",
        "A subscriber of `plugin-state-changed` MUST have a queue of at most 256 events",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    let subscriber = server.attach_client("subscriber");
    subscriber.block_delivery();
    for _ in 0..150 {
        server.disable(NOTES);
        server.enable(NOTES);
    }
    subscriber.resume_delivery();
    let messages = server.received_messages();
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.contains("plugin-state-resync"))
            .count(),
        1,
        "one resync event after overflow"
    );
    assert!(
        messages.len() <= 257,
        "the queue never holds more than 256 events plus the resync"
    );
}

/// REQ: LIF-700
#[test]
fn lif_700_retired_record_in_installed_json_carries_retired_at_and_reason() {
    assert_manual_says(
        "reference-plugin-lifecycle-states",
        "with `retired_at` and `retired_reason` of `end-of-life` or `abandoned`",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_end_of_life(NOTES, "2026-01-01");
    let _server = rig.start_server("default");
    let record = installed_json(&rig)["plugins"][NOTES].clone();
    assert_eq!(record["state"], "retired", "installed.json stores retired");
    assert!(
        !record["retired_at"].is_null(),
        "retired_at is recorded in installed.json"
    );
    assert_eq!(record["retired_reason"], "end-of-life");
}

/// REQ: LIF-701
#[test]
fn lif_701_retirement_is_applied_once_and_later_passes_do_not_repeat_it() {
    assert_manual_says(
        "reference-plugin-lifecycle-states",
        "It MUST send `deactivate` to each live instance once",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    rig.plant_end_of_life(NOTES, "2026-01-01");
    rig.jump_wall_clock(40 * 86_400);
    server.apply_pending();
    rig.jump_wall_clock(10 * 86_400);
    server.apply_pending();
    assert_eq!(
        server.deactivations(NOTES).len(),
        1,
        "a later pass does not deactivate the retired plugin again"
    );
    assert_eq!(log_count(&server, "plugin.retired"), 1);
}

/// REQ: LIF-703
#[test]
fn lif_703_enable_of_a_retired_plugin_without_accept_risk_is_refused_with_plugin_retired() {
    assert_manual_says(
        "reference-plugin-lifecycle-states",
        "and MUST return `not-permitted` with `plugin.retired` for `plugins.set-enabled` from a plugin",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_end_of_life(NOTES, "2026-01-01");
    let server = rig.start_server("default");
    server.apply_pending();
    let refused = rig.cli(&["plugin", "enable", NOTES]);
    assert!(
        error_codes(&refused).contains(&"plugin.retired".to_string()),
        "enable of a retired plugin names plugin.retired"
    );
    assert_eq!(
        list_row(&rig, NOTES)["state"],
        "retired",
        "the refused enable leaves the plugin retired"
    );
}

/// REQ: LIF-706
#[test]
fn lif_706_takedown_of_a_plugin_that_ignores_stop_is_blocked_within_the_deadline() {
    assert_manual_says(
        "reference-plugin-lifecycle-states",
        "and MUST NOT wait longer than the deactivate deadline",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").deactivate_hangs());
    let server = rig.start_server("default");
    let started = rig.virtual_elapsed();
    rig.registry_takedown(NOTES, "1.0.0");
    server.apply_pending();
    assert_eq!(
        server.inactive_reason(NOTES).as_deref(),
        Some("blocked"),
        "a plugin that never returns from deactivate is still blocked"
    );
    assert!(
        rig.virtual_elapsed() - started < secs(10),
        "the takedown does not wait past the deactivate deadline"
    );
}

/// REQ: LIF-718
#[test]
fn lif_718_install_of_the_same_id_during_held_uninstall_returns_uninstalling() {
    assert_manual_says(
        "reference-plugin-lifecycle-states",
        "`enable`, `disable`, `update`, `rollback` and `install` of the same id MUST return `install.uninstalling` (exit 4)",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.hold_uninstall(NOTES);
    let output = install(&rig, &pkg(NOTES, "1.2.0"));
    assert_eq!(output.exit_code, 4, "install during uninstall exits 4");
    assert!(error_codes(&output).contains(&"install.uninstalling".to_string()));
    assert_eq!(list_row(&rig, NOTES)["state"], "uninstall-pending");
}

/// REQ: LIF-719
#[test]
fn lif_719_new_calls_are_refused_while_disabled() {
    assert_manual_says(
        "reference-plugin-lifecycle-states",
        "the host MUST stop accepting new calls at once",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    server.disable(NOTES);
    let refused = server.call_plugin(NOTES, "plugins.list");
    assert!(
        refused.is_err(),
        "a call to a disabled plugin is refused at once"
    );
}

/// REQ: LIF-720
#[test]
fn lif_720_project_overlay_resolves_when_no_session_overlay_is_set() {
    assert_manual_says(
        "reference-plugin-lifecycle-states",
        "and the effective enabled value MUST resolve narrowest-first (session, project, global)",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.set_enabled_levels(NOTES, "s2", None, Some(false), Some(true));
    let info = rig.cli(&["plugin", "info", NOTES, "--json", "--session", "s2"]);
    assert_eq!(
        info.stdout_jsonl[0]["enabled"], false,
        "with no session overlay the project overlay wins over the global value"
    );
}

/// REQ: LIF-724
#[test]
fn lif_724_queue_under_the_limit_delivers_events_in_order_with_no_resync() {
    assert_manual_says(
        "reference-plugin-lifecycle-states",
        "Ordering per plugin id MUST be preserved.",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    let subscriber = server.attach_client("subscriber");
    subscriber.block_delivery();
    server.disable(NOTES);
    server.enable(NOTES);
    subscriber.resume_delivery();
    let messages = server.received_messages();
    assert!(
        !messages.iter().any(|m| m.contains("plugin-state-resync")),
        "no resync is sent below the queue limit"
    );
    let events: Vec<&String> = messages
        .iter()
        .filter(|m| m.contains("plugin-state-changed") && m.contains(NOTES))
        .collect();
    assert_eq!(events.len(), 2, "both changes are delivered");
    assert!(events[0].contains("disabled"), "the disable comes first");
    assert!(events[1].contains("enabled"), "the enable comes second");
}
