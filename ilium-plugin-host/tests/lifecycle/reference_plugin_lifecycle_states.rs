//! reference-plugin-lifecycle-states.md: LIF-001..020.

use crate::support::*;
use ilium_plugin_api::generated::host::plugins::HostPlugins;
use ilium_plugin_api::generated::types::ErrorCode;
use ilium_plugin_api::ids::PluginId;
use ilium_plugin_api::lifecycle::{next_instance_state, InstanceInput};
use proptest::prelude::*;

fn id(text: &str) -> PluginId {
    PluginId(text.to_owned())
}

/// REQ: LIF-001
#[test]
fn lif_001_every_listed_plugin_has_exactly_one_package_state() {
    let mut host = one_plugin();
    host.install(PluginSpec::new(B));
    host.plugins_api()
        .set_enabled(id(B), false, String::new())
        .unwrap();
    let list = records(&mut host, &["plugin", "list", "--json"], "plugin");
    assert_eq!(list.len(), 2);
    for record in &list {
        assert!(
            record["state"].is_string(),
            "one state per record: {record}"
        );
    }
}

/// REQ: LIF-001
#[test]
fn lif_001_state_stays_single_valued_through_disable_and_enable() {
    let mut host = one_plugin();
    for enabled in [false, true, false] {
        host.plugins_api()
            .set_enabled(id(A), enabled, String::new())
            .unwrap();
        let list = records(&mut host, &["plugin", "list", "--json"], "plugin");
        assert_eq!(list.iter().filter(|r| r["id"] == A).count(), 1);
    }
}

/// REQ: LIF-002
#[test]
fn lif_002_client_stop_keeps_package_enabled_and_server_running() {
    let (mut host, scope) = two_sided();
    for _ in 0..3 {
        host.fail(A, Side::Client, &scope, FailureKind::Trap);
    }
    assert_eq!(
        host.side(A, Side::Client, &scope).state,
        InstanceState::StoppedByFailure
    );
    assert_eq!(host.package_state(A), PackageState::InstalledEnabled);
    assert_eq!(state(&host, A), InstanceState::Active);
}

/// REQ: LIF-002
#[test]
fn lif_002_server_stop_does_not_stop_client() {
    let (mut host, scope) = two_sided();
    for _ in 0..3 {
        trap(&mut host, A);
    }
    assert_eq!(state(&host, A), InstanceState::StoppedByFailure);
    assert_eq!(
        host.side(A, Side::Client, &scope).state,
        InstanceState::Active
    );
    assert_eq!(host.package_state(A), PackageState::InstalledEnabled);
}

/// REQ: LIF-003
#[test]
fn lif_003_refused_at_load_is_never_instantiated_and_shows_code() {
    let mut host = LifeHost::new();
    host.install_refused(PluginSpec::new(A), "api.unsupported");
    host.start(StartMode::Normal);
    assert_eq!(host.package_state(A), PackageState::RefusedAtLoad);
    assert!(host.activations().iter().all(|r| r.plugin != A));
    let list = records(&mut host, &["plugin", "list", "--json"], "plugin");
    assert_eq!(list[0]["disabled_reason"], "api.unsupported");
}

/// REQ: LIF-003
#[test]
fn lif_003_healthy_package_is_instantiated() {
    let host = one_plugin();
    assert!(host.activations().iter().any(|r| r.plugin == A));
}

/// REQ: LIF-004
#[test]
fn lif_004_safe_mode_is_not_persisted() {
    let mut host = one_plugin();
    let before = host.installed_json();
    host.shutdown();
    host.start_safe(SafeMode::AllOptional, None, None);
    host.shutdown();
    assert_eq!(host.installed_json(), before);
    host.start(StartMode::Normal);
    assert_eq!(host.package_state(A), PackageState::InstalledEnabled);
}

/// REQ: LIF-004
#[test]
fn lif_004_policy_block_is_not_written_to_installed_json() {
    let mut host = LifeHost::new();
    host.install_policy_blocked(PluginSpec::new(A));
    host.start(StartMode::Normal);
    assert_eq!(host.package_state(A), PackageState::DisabledByPolicy);
    let text = host.installed_json().to_string();
    assert!(!text.contains("disabled-by-policy"), "{text}");
}

/// REQ: LIF-005
#[test]
fn lif_005_observed_transitions_are_all_in_the_table() {
    let mut host = one_plugin();
    trap(&mut host, A);
    host.advance_ms(1000);
    trap(&mut host, A);
    host.advance_ms(5000);
    host.plugins_api()
        .set_enabled(id(A), false, String::new())
        .unwrap();
    host.plugins_api()
        .set_enabled(id(A), true, String::new())
        .unwrap();
    for change in host
        .state_changes()
        .iter()
        .filter(|c| c.side == Side::Server)
    {
        assert_ne!(change.from, change.to, "a change moves between states");
    }
}

proptest! {
    /// REQ: LIF-005
    #[test]
    fn lif_005_random_triggers_only_make_table_transitions(steps in proptest::collection::vec(0u8..6, 1..40)) {
        let mut host = one_plugin();
        for step in steps {
            match step {
                0 => trap(&mut host, A),
                1 => host.advance_ms(1000),
                2 => host.advance_ms(5000),
                3 => { let _ = host.plugins_api().set_enabled(id(A), false, String::new()); }
                4 => { let _ = host.plugins_api().set_enabled(id(A), true, String::new()); }
                _ => host.reset_failures(A),
            }
        }
        let mut previous = None;
        for change in host.state_changes().into_iter().filter(|c| c.side == Side::Server) {
            if let Some(last) = previous {
                prop_assert_eq!(change.from, last);
            }
            previous = Some(change.to);
        }
    }
}

/// REQ: LIF-005
#[test]
fn lif_005_pure_table_rejects_stopped_activate_ok_illegal_transition() {
    assert!(next_instance_state(InstanceState::Stopped, &InstanceInput::ActivateOk).is_err());
}

/// REQ: LIF-006
#[test]
fn lif_006_event_to_activating_instance_is_queued() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new(A).activate_delay_ms(100));
    host.start(StartMode::Normal);
    host.deliver_event(A, Side::Server, SERVER, 1);
    assert_eq!(host.queued_events(A, Side::Server), 1);
}

/// REQ: LIF-006
#[test]
fn lif_006_event_to_stopped_instance_is_dropped_without_export_call() {
    let mut host = one_plugin();
    host.shutdown();
    host.deliver_event(A, Side::Server, SERVER, 1);
    assert_eq!(host.dropped_events(A, Side::Server), 1);
    assert_eq!(host.export_calls(A, Side::Server), 0);
}

/// REQ: LIF-007
#[test]
fn lif_007_enable_refused_at_load_not_permitted() {
    let mut host = LifeHost::new();
    host.install_refused(PluginSpec::new(A), "api.unsupported");
    host.start(StartMode::Normal);
    let error = host
        .plugins_api()
        .set_enabled(id(A), true, String::new())
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotPermitted);
    assert!(
        error.message.contains("api.unsupported"),
        "{}",
        error.message
    );
    assert_eq!(host.package_state(A), PackageState::RefusedAtLoad);
}

/// REQ: LIF-007
#[test]
fn lif_007_enable_disabled_by_policy_not_permitted_and_no_change() {
    let mut host = LifeHost::new();
    host.install_policy_blocked(PluginSpec::new(A));
    host.start(StartMode::Normal);
    let events = host.state_changes().len();
    let error = host
        .plugins_api()
        .set_enabled(id(A), true, String::new())
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotPermitted);
    assert_eq!(host.package_state(A), PackageState::DisabledByPolicy);
    assert_eq!(host.state_changes().len(), events);
}

/// REQ: LIF-008
#[test]
fn lif_008_server_stop_by_failure_survives_restart() {
    let mut host = one_plugin();
    for _ in 0..3 {
        trap(&mut host, A);
        host.advance_ms(1000);
    }
    assert_eq!(
        host.installed_json()["plugins"][A]["failure_stopped"],
        serde_json::json!(["server"])
    );
    host.restart_server();
    assert_eq!(state(&host, A), InstanceState::StoppedByFailure);
}

/// REQ: LIF-008
#[test]
fn lif_008_client_stop_is_not_stored() {
    let (mut host, scope) = two_sided();
    for _ in 0..3 {
        host.fail(A, Side::Client, &scope, FailureKind::Trap);
    }
    assert!(!host.installed_json().to_string().contains("client"));
}

/// REQ: LIF-009
#[test]
fn lif_009_enable_clears_window_so_next_failure_restarts_after_one_second() {
    let mut host = one_plugin();
    trap(&mut host, A);
    host.advance_ms(1000);
    trap(&mut host, A);
    host.plugins_api()
        .set_enabled(id(A), false, String::new())
        .unwrap();
    host.plugins_api()
        .set_enabled(id(A), true, String::new())
        .unwrap();
    trap(&mut host, A);
    assert_eq!(restart_in_ms(&host, A), Some(1000));
}

/// REQ: LIF-009
#[test]
fn lif_009_update_clears_window() {
    let mut host = one_plugin();
    trap(&mut host, A);
    host.update(A, "2.0.0");
    assert_eq!(failures(&host, A), 0);
}

/// REQ: LIF-010
#[test]
fn lif_010_first_activate_failure_rolls_back() {
    let mut host = one_plugin();
    host.update_candidate_fails(A, "2.0.0");
    assert_eq!(host.active_version(A, Side::Server), "1.0.0");
    assert_ne!(host.package_state(A), PackageState::UpdateStaged);
}

/// REQ: LIF-010
#[test]
fn lif_010_failure_after_first_activation_does_not_roll_back() {
    let mut host = one_plugin();
    host.update(A, "2.0.0");
    trap(&mut host, A);
    assert_eq!(host.active_version(A, Side::Server), "2.0.0");
}

/// REQ: LIF-011
#[test]
fn lif_011_pool_refusal_is_not_a_failure() {
    let mut host = LifeHost::new().with_pool_mib(40);
    host.install(PluginSpec::new(B).memory_mib(8).startup());
    host.install(PluginSpec::new(A).memory_mib(32).startup());
    host.start(StartMode::Normal);
    let side = host.side(A, Side::Server, SERVER);
    assert_eq!(side.state, InstanceState::Inactive);
    assert_eq!(side.failures_in_window, 0);
}

/// REQ: LIF-011
#[test]
fn lif_011_pool_refusal_logs_host_capacity_reason() {
    let mut host = LifeHost::new().with_pool_mib(1);
    host.install(PluginSpec::new(A).memory_mib(32).startup());
    host.start(StartMode::Normal);
    assert!(host
        .state_changes()
        .iter()
        .any(|c| c.plugin == A && c.reason == "capacity"));
}

/// REQ: LIF-012
#[test]
fn lif_012_trap_drops_instance_without_deactivate() {
    let mut host = one_plugin();
    trap(&mut host, A);
    assert!(host.deactivations(A).is_empty());
    assert_eq!(state(&host, A), InstanceState::RestartScheduled);
}

/// REQ: LIF-012
#[test]
fn lif_012_activate_error_drops_instance_without_deactivate() {
    let mut host = one_plugin();
    host.fail(A, Side::Server, SERVER, FailureKind::ActivateError);
    assert!(host.deactivations(A).is_empty());
}

/// REQ: LIF-013
#[test]
fn lif_013_disable_during_scheduled_restart_cancels_timer() {
    let mut host = one_plugin();
    trap(&mut host, A);
    let begun = host.activations().len();
    host.plugins_api()
        .set_enabled(id(A), false, String::new())
        .unwrap();
    host.advance_ms(10_000);
    assert_eq!(host.activations().len(), begun);
}

/// REQ: LIF-013
#[test]
fn lif_013_uninstall_during_scheduled_restart_cancels_timer() {
    let mut host = one_plugin();
    trap(&mut host, A);
    let begun = host.activations().len();
    host.uninstall(A);
    host.advance_ms(10_000);
    assert_eq!(host.activations().len(), begun);
}

/// REQ: LIF-014
#[test]
fn lif_014_disable_sends_one_deactivate_disabled() {
    let mut host = one_plugin();
    host.plugins_api()
        .set_enabled(id(A), false, String::new())
        .unwrap();
    assert_eq!(host.deactivations(A), vec![DeactivationReason::Disabled]);
}

/// REQ: LIF-014
#[test]
fn lif_014_update_and_shutdown_send_one_each_with_their_reason() {
    let mut host = one_plugin();
    host.update(A, "2.0.0");
    host.shutdown();
    assert_eq!(
        host.deactivations(A),
        vec![DeactivationReason::Upgrade, DeactivationReason::Shutdown]
    );
}

/// REQ: LIF-015
#[test]
fn lif_015_clean_detach_sends_shutdown_only() {
    let (mut host, _scope) = two_sided();
    host.detach_client("client-1");
    assert_eq!(
        host.client_deactivations(A, "client-1"),
        vec![DeactivationReason::Shutdown]
    );
}

/// REQ: LIF-015
#[test]
fn lif_015_killed_client_gets_no_call() {
    let (mut host, _scope) = two_sided();
    host.kill_client("client-1");
    assert!(host.client_deactivations(A, "client-1").is_empty());
}

/// REQ: LIF-016
#[test]
fn lif_016_one_event_per_change_in_order() {
    let mut host = one_plugin();
    host.plugins_api()
        .set_enabled(id(A), false, String::new())
        .unwrap();
    host.plugins_api()
        .set_enabled(id(A), true, String::new())
        .unwrap();
    trap(&mut host, A);
    let changes = host.state_changes();
    let mut chain = changes
        .iter()
        .filter(|c| c.plugin == A && c.side == Side::Server);
    let mut last = chain.next().unwrap().to;
    for change in chain {
        assert_eq!(change.from, last);
        last = change.to;
    }
    assert!(changes
        .windows(2)
        .all(|pair| pair[0].at_unix_ms <= pair[1].at_unix_ms));
}

/// REQ: LIF-016
#[test]
fn lif_016_reasons_come_from_the_documented_set() {
    let mut host = one_plugin();
    trap(&mut host, A);
    host.plugins_api()
        .set_enabled(id(A), false, String::new())
        .unwrap();
    let allowed = [
        "user",
        "failure",
        "policy",
        "update",
        "uninstall",
        "trigger",
        "capacity",
        "server-start",
        "server-stop",
        "detach",
        "grant",
    ];
    assert!(host
        .state_changes()
        .iter()
        .all(|c| allowed.contains(&c.reason.as_str())));
}

/// REQ: LIF-017
#[test]
fn lif_017_list_has_one_record_per_plugin_with_nineteen_fields_in_order() {
    let mut host = one_plugin();
    host.install(PluginSpec::new(B));
    host.plugins_api()
        .set_enabled(id(B), false, String::new())
        .unwrap();
    let list = host.plugins_api().list();
    assert_eq!(list.len(), 2);
    let json = serde_json::to_value(&list[0]).unwrap();
    let keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys.len(), 19);
    assert_eq!(keys[0], "id");
}

/// REQ: LIF-017
#[test]
fn lif_017_disabled_record_reports_user_reason() {
    let mut host = one_plugin();
    host.plugins_api()
        .set_enabled(id(A), false, String::new())
        .unwrap();
    let info = host
        .plugins_api()
        .list()
        .into_iter()
        .find(|i| i.id.0 == A)
        .unwrap();
    assert_eq!(info.disabled_reason.as_deref(), Some("user"));
}

/// REQ: LIF-018
#[test]
fn lif_018_enabled_matches_package_state_rule() {
    let cases = [
        (PackageState::InstalledEnabled, true),
        (PackageState::UpdateStaged, true),
        (PackageState::InstalledDisabledByUser, false),
        (PackageState::DisabledUntrusted, false),
        (PackageState::DisabledBlocked, false),
        (PackageState::DisabledByPolicy, false),
        (PackageState::RefusedAtLoad, false),
        (PackageState::UninstallPending, false),
    ];
    for (package, enabled) in cases {
        let mut host = LifeHost::new();
        host.install(PluginSpec::new(A));
        host.force_package_state(A, package);
        let info = host
            .plugins_api()
            .list()
            .into_iter()
            .find(|i| i.id.0 == A)
            .unwrap();
        assert_eq!(info.enabled, enabled, "{package:?}");
    }
}

/// REQ: LIF-018
#[test]
fn lif_018_failed_is_true_when_any_side_is_stopped_by_failure() {
    let mut host = one_plugin();
    for _ in 0..3 {
        trap(&mut host, A);
        host.advance_ms(1000);
    }
    let info = host
        .plugins_api()
        .list()
        .into_iter()
        .find(|i| i.id.0 == A)
        .unwrap();
    assert!(info.failed && info.enabled);
}

/// REQ: LIF-019
#[test]
fn lif_019_set_enabled_is_idempotent_without_event() {
    let mut host = one_plugin();
    let before = host.state_changes().len();
    host.plugins_api()
        .set_enabled(id(A), true, String::new())
        .unwrap();
    assert_eq!(host.state_changes().len(), before);
    host.plugins_api()
        .set_enabled(id(A), false, String::new())
        .unwrap();
    let mid = host.state_changes().len();
    host.plugins_api()
        .set_enabled(id(A), false, String::new())
        .unwrap();
    assert_eq!(host.state_changes().len(), mid);
}

/// REQ: LIF-019
#[test]
fn lif_019_noop_changes_nothing_in_the_record() {
    let mut host = one_plugin();
    let before = host.plugins_api().list();
    host.plugins_api()
        .set_enabled(id(A), true, String::new())
        .unwrap();
    assert_eq!(host.plugins_api().list(), before);
}

/// REQ: LIF-020
#[test]
fn lif_020_disable_and_uninstall_run_one_after_the_other() {
    let mut host = one_plugin();
    host.race_disable_and_uninstall(A);
    let changes = host.state_changes();
    let order: Vec<_> = changes
        .iter()
        .filter(|c| c.plugin == A)
        .map(|c| (c.from, c.to))
        .collect();
    for pair in order.windows(2) {
        assert_eq!(pair[0].1, pair[1].0, "serialised: {order:?}");
    }
}

/// REQ: LIF-020
#[test]
fn lif_020_other_plugins_are_not_blocked_by_a_slow_transition() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new(A).deactivate_delay_ms(200).startup());
    host.install(PluginSpec::new(B).startup());
    host.start(StartMode::Normal);
    let slow = host.begin_disable(A);
    host.plugins_api()
        .set_enabled(id(B), false, String::new())
        .unwrap();
    assert!(
        !slow.finished(),
        "B finished while A was still deactivating"
    );
}
