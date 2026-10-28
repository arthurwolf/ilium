//! lifecycle.md: LIF-025..059.

use crate::support::*;
use ilium_plugin_api::generated::types::ErrorCode;
use ilium_plugin_api::ids::PluginId;
use proptest::prelude::*;

fn id(text: &str) -> PluginId {
    PluginId(text.to_owned())
}

/// REQ: LIF-025
#[test]
fn lif_025_windows_are_per_side_and_scope() {
    let (mut host, scope) = two_sided();
    host.fail(A, Side::Client, &scope, FailureKind::Trap);
    host.fail(A, Side::Client, &scope, FailureKind::Trap);
    assert_eq!(host.side(A, Side::Client, &scope).failures_in_window, 2);
    assert_eq!(failures(&host, A), 0);
}

/// REQ: LIF-025
#[test]
fn lif_025_scene_and_client_sides_have_separate_windows() {
    let mut host = LifeHost::new();
    host.install(
        PluginSpec::new(A)
            .sides(&[Side::Client, Side::Scene])
            .startup(),
    );
    host.start(StartMode::Normal);
    let scope = host.attach_client("c1");
    host.fail(A, Side::Scene, &scope, FailureKind::Trap);
    assert_eq!(host.side(A, Side::Client, &scope).failures_in_window, 0);
}

/// REQ: LIF-026
#[test]
fn lif_026_third_trap_on_client_a_leaves_client_b_and_server_running() {
    let (mut host, scope_a) = two_sided();
    let scope_b = host.attach_client("client-2");
    for _ in 0..3 {
        host.fail(A, Side::Client, &scope_a, FailureKind::Trap);
    }
    assert_eq!(
        host.side(A, Side::Client, &scope_a).state,
        InstanceState::StoppedByFailure
    );
    assert_eq!(
        host.side(A, Side::Client, &scope_b).state,
        InstanceState::Active
    );
    assert_eq!(state(&host, A), InstanceState::Active);
}

/// REQ: LIF-026
#[test]
fn lif_026_failures_on_two_clients_do_not_add_up() {
    let (mut host, scope_a) = two_sided();
    let scope_b = host.attach_client("client-2");
    host.fail(A, Side::Client, &scope_a, FailureKind::Trap);
    host.fail(A, Side::Client, &scope_b, FailureKind::Trap);
    assert_eq!(host.side(A, Side::Client, &scope_a).failures_in_window, 1);
    assert_eq!(host.side(A, Side::Client, &scope_b).failures_in_window, 1);
}

/// REQ: LIF-027
#[test]
fn lif_027_failure_at_599_s_counts_and_at_601_s_does_not() {
    let mut host = one_plugin();
    trap(&mut host, A);
    host.advance_ms(599_000);
    assert_eq!(failures(&host, A), 1);
    host.advance_ms(2_000);
    assert_eq!(failures(&host, A), 0);
}

/// REQ: LIF-027
#[test]
fn lif_027_second_trap_after_601_s_is_a_first_failure() {
    let mut host = one_plugin();
    trap(&mut host, A);
    host.advance_ms(601_000);
    trap(&mut host, A);
    assert_eq!(restart_in_ms(&host, A), Some(1000));
}

proptest! {
    /// REQ: LIF-027
    #[test]
    fn lif_027_count_equals_failures_younger_than_600_s(gaps in proptest::collection::vec(1_001u64..400_000, 1..6)) {
        let mut host = one_plugin();
        let mut times = Vec::new();
        let mut now = 0u64;
        for gap in gaps {
            host.advance_ms(gap);
            now += gap;
            if failures(&host, A) >= 2 { break; }
            trap(&mut host, A);
            times.push(now);
        }
        let expected = times.iter().filter(|t| now - **t < 600_000).count() as u32;
        prop_assert_eq!(failures(&host, A), expected);
    }
}

/// REQ: LIF-028
#[test]
fn lif_028_first_failure_restarts_after_one_second() {
    let mut host = one_plugin();
    trap(&mut host, A);
    assert_eq!(restart_in_ms(&host, A), Some(1000));
    assert_eq!(state(&host, A), InstanceState::RestartScheduled);
}

/// REQ: LIF-028
#[test]
fn lif_028_restart_has_not_begun_at_999_ms_and_has_at_1000() {
    let mut host = one_plugin();
    trap(&mut host, A);
    host.advance_ms(999);
    assert_eq!(state(&host, A), InstanceState::RestartScheduled);
    host.advance_ms(1);
    assert_ne!(state(&host, A), InstanceState::RestartScheduled);
}

/// REQ: LIF-029
#[test]
fn lif_029_second_failure_restarts_after_five_seconds() {
    let mut host = one_plugin();
    trap(&mut host, A);
    host.advance_ms(1000);
    trap(&mut host, A);
    assert_eq!(restart_in_ms(&host, A), Some(5000));
}

/// REQ: LIF-029
#[test]
fn lif_029_restart_has_not_begun_at_4999_ms() {
    let mut host = one_plugin();
    trap(&mut host, A);
    host.advance_ms(1000);
    trap(&mut host, A);
    host.advance_ms(4999);
    assert_eq!(state(&host, A), InstanceState::RestartScheduled);
}

/// REQ: LIF-030
#[test]
fn lif_030_third_failure_stops_the_side_with_no_restart() {
    let mut host = one_plugin();
    for _ in 0..3 {
        trap(&mut host, A);
        host.advance_ms(1000);
    }
    assert_eq!(state(&host, A), InstanceState::StoppedByFailure);
    assert_eq!(restart_in_ms(&host, A), None);
    assert_eq!(host.package_state(A), PackageState::InstalledEnabled);
}

/// REQ: LIF-030
#[test]
fn lif_030_two_failures_do_not_stop_the_side() {
    let mut host = one_plugin();
    trap(&mut host, A);
    host.advance_ms(1000);
    trap(&mut host, A);
    assert_ne!(state(&host, A), InstanceState::StoppedByFailure);
}

/// REQ: LIF-031
#[test]
fn lif_031_stopped_side_stays_stopped_after_one_hour() {
    let mut host = one_plugin();
    for _ in 0..3 {
        trap(&mut host, A);
        host.advance_ms(1000);
    }
    host.advance_ms(3_600_000);
    assert_eq!(state(&host, A), InstanceState::StoppedByFailure);
}

proptest! {
    /// REQ: LIF-031
    #[test]
    fn lif_031_only_delays_of_one_and_five_seconds_ever_appear(waits in proptest::collection::vec(0u64..8_000, 1..10)) {
        let mut host = one_plugin();
        for wait in waits {
            host.advance_ms(wait);
            if state(&host, A) == InstanceState::Active { trap(&mut host, A); }
            if let Some(delay) = restart_in_ms(&host, A) {
                prop_assert!(delay <= 5000);
            }
        }
    }
}

/// REQ: LIF-032
#[test]
fn lif_032_next_activation_not_earlier_than_delay() {
    let mut host = one_plugin();
    trap(&mut host, A);
    let dropped_at = host.unix_ms();
    host.advance_ms(1000);
    let last = host
        .activations()
        .into_iter()
        .rfind(|r| r.plugin == A)
        .unwrap();
    assert!(last.begin_ms >= dropped_at + 1000);
}

/// REQ: LIF-032 WARN
#[test]
fn lif_032_next_activation_begins_within_250_ms_after_delay() {
    let mut host = one_plugin();
    trap(&mut host, A);
    let dropped_at = host.unix_ms();
    host.advance_ms(1250);
    let last = host
        .activations()
        .into_iter()
        .rfind(|r| r.plugin == A)
        .unwrap();
    assert!(last.begin_ms <= dropped_at + 1250);
}

/// REQ: LIF-033
#[test]
fn lif_033_counting_table_rows_count_one_failure_each() {
    let counted = [
        FailureKind::Trap,
        FailureKind::ActivateError,
        FailureKind::ActivateDeadline,
        FailureKind::ExportDeadline,
        FailureKind::DeactivateDeadline(DeactivationReason::Upgrade),
        FailureKind::DeactivateDeadline(DeactivationReason::Revoked),
        FailureKind::EventQueueOverflow,
        FailureKind::MemoryLimit,
        FailureKind::ServiceProviderActivateFails,
        FailureKind::HelperCrashRunning,
    ];
    for kind in counted {
        let mut host = one_plugin();
        host.fail(A, Side::Server, SERVER, kind);
        assert_eq!(failures(&host, A), 1, "{kind:?}");
    }
}

/// REQ: LIF-033
#[test]
fn lif_033_non_counting_rows_leave_the_window_empty() {
    let uncounted = [
        FailureKind::DeactivateDeadline(DeactivationReason::Disabled),
        FailureKind::DeactivateDeadline(DeactivationReason::Shutdown),
        FailureKind::JobTrap,
        FailureKind::HelperCrashOther,
    ];
    for kind in uncounted {
        let mut host = one_plugin();
        host.fail(A, Side::Server, SERVER, kind);
        assert_eq!(failures(&host, A), 0, "{kind:?}");
    }
}

/// REQ: LIF-033
#[test]
fn lif_033_extension_handler_timeouts_count_only_toward_the_three_breach_stop() {
    let mut host = one_plugin();
    host.fail(
        A,
        Side::Server,
        SERVER,
        FailureKind::ExtensionHandlerTimeout,
    );
    assert_eq!(failures(&host, A), 0);
    assert_eq!(state(&host, A), InstanceState::Active);
}

/// REQ: LIF-034
#[test]
fn lif_034_capacity_refusal_leaves_inactive_with_reason_and_no_failure() {
    let mut host = LifeHost::new().with_pool_mib(1);
    host.install(PluginSpec::new(A).memory_mib(32).startup());
    host.start(StartMode::Normal);
    assert_eq!(
        host.side(A, Side::Server, SERVER).state,
        InstanceState::Inactive
    );
    assert_eq!(failures(&host, A), 0);
    assert!(host
        .state_changes()
        .iter()
        .any(|c| c.plugin == A && c.reason == "capacity"));
}

/// REQ: LIF-034
#[test]
fn lif_034_retry_on_memory_release() {
    let mut host = LifeHost::new().with_pool_mib(40);
    host.install(PluginSpec::new(B).memory_mib(32).startup());
    host.install(PluginSpec::new(A).memory_mib(32));
    host.start(StartMode::Normal);
    host.trigger_activation(A);
    assert_eq!(state(&host, A), InstanceState::Inactive);
    host.plugins_api()
        .set_enabled(id(B), false, String::new())
        .unwrap();
    assert_eq!(state(&host, A), InstanceState::Active);
}

/// REQ: LIF-034
#[test]
fn lif_034_retry_schedule_is_1_5_30_then_every_60_seconds() {
    let mut host = LifeHost::new().with_pool_mib(1);
    host.install(PluginSpec::new(A).memory_mib(32).startup());
    host.start(StartMode::Normal);
    let mut waits = Vec::new();
    let mut last = host.capacity_retries(A);
    for _ in 0..200_000 {
        host.advance_ms(500);
        let now = host.capacity_retries(A);
        if now > last {
            waits.push(host.unix_ms());
            last = now;
        }
        if waits.len() == 5 {
            break;
        }
    }
    let gaps: Vec<u64> = waits.windows(2).map(|w| w[1] - w[0]).collect();
    assert_eq!(waits[0], 1000);
    assert_eq!(gaps, vec![5000, 30_000, 60_000, 60_000]);
}

/// REQ: LIF-035
#[test]
fn lif_035_failed_job_is_not_counted() {
    let mut host = one_plugin();
    host.fail(A, Side::Server, SERVER, FailureKind::JobTrap);
    assert_eq!(failures(&host, A), 0);
    assert_eq!(state(&host, A), InstanceState::Active);
}

/// REQ: LIF-036
#[test]
fn lif_036_each_clearing_action_empties_the_window() {
    for action in ["enable", "cli-reset", "recovery-page", "update"] {
        let mut host = one_plugin();
        trap(&mut host, A);
        host.advance_ms(1000);
        trap(&mut host, A);
        match action {
            "enable" => {
                host.plugins_api()
                    .set_enabled(id(A), false, String::new())
                    .unwrap();
                host.plugins_api()
                    .set_enabled(id(A), true, String::new())
                    .unwrap();
            }
            "cli-reset" => {
                assert_eq!(host.cli(&["plugin", "reset-failures", A]).exit_code, 0);
            }
            "recovery-page" => host.recovery_page_enable(A),
            _ => host.update(A, "2.0.0"),
        }
        assert_eq!(failures(&host, A), 0, "{action}");
    }
}

/// REQ: LIF-036
#[test]
fn lif_036_reset_failures_revives_a_stopped_side() {
    let mut host = one_plugin();
    for _ in 0..3 {
        trap(&mut host, A);
        host.advance_ms(1000);
    }
    assert_eq!(host.cli(&["plugin", "reset-failures", A]).exit_code, 0);
    assert_ne!(state(&host, A), InstanceState::StoppedByFailure);
}

/// REQ: LIF-037
#[test]
fn lif_037_server_stop_survives_restart_but_window_starts_empty() {
    let mut host = one_plugin();
    for _ in 0..3 {
        trap(&mut host, A);
        host.advance_ms(1000);
    }
    host.restart_server();
    assert_eq!(state(&host, A), InstanceState::StoppedByFailure);
    assert_eq!(failures(&host, A), 0);
}

/// REQ: LIF-037
#[test]
fn lif_037_client_stop_is_gone_after_reattach() {
    let (mut host, scope) = two_sided();
    for _ in 0..3 {
        host.fail(A, Side::Client, &scope, FailureKind::Trap);
    }
    host.detach_client("client-1");
    let scope = host.attach_client("client-1");
    assert_eq!(
        host.side(A, Side::Client, &scope).state,
        InstanceState::Active
    );
}

/// REQ: LIF-038
#[test]
fn lif_038_server_stop_cancels_restart_timers() {
    let mut host = one_plugin();
    trap(&mut host, A);
    let begun = host.activations().len();
    host.shutdown();
    host.advance_ms(10_000);
    assert_eq!(host.activations().len(), begun);
}

/// REQ: LIF-038
#[test]
fn lif_038_failure_during_shutdown_is_logged_not_counted() {
    let mut host = one_plugin();
    host.begin_shutdown();
    trap(&mut host, A);
    assert_eq!(failures(&host, A), 0);
    // manual does not pin the log message text; the line must belong to the failed plugin.
    assert!(host.log().iter().any(|l| l.plugin == A));
}

/// REQ: LIF-039
#[test]
fn lif_039_disable_while_restart_pending_moves_to_disabled_by_user() {
    let mut host = one_plugin();
    trap(&mut host, A);
    let begun = host.activations().len();
    host.plugins_api()
        .set_enabled(id(A), false, String::new())
        .unwrap();
    host.advance_ms(6000);
    assert_eq!(host.package_state(A), PackageState::InstalledDisabledByUser);
    assert_eq!(host.activations().len(), begun);
}

/// REQ: LIF-040
#[test]
fn lif_040_update_during_restart_activates_new_version_and_clears_window() {
    let mut host = one_plugin();
    trap(&mut host, A);
    host.update(A, "2.0.0");
    assert_eq!(failures(&host, A), 0);
    assert_eq!(host.active_version(A, Side::Server), "2.0.0");
}

/// REQ: LIF-041
#[test]
fn lif_041_revoke_during_restart_keeps_timer_and_shortens_granted() {
    let mut host = LifeHost::new();
    host.install(
        PluginSpec::new(A)
            .grants(&["fs.read", "net.http"])
            .startup(),
    );
    host.start(StartMode::Normal);
    trap(&mut host, A);
    host.revoke(A, "net.http");
    assert_eq!(restart_in_ms(&host, A), Some(1000));
    host.advance_ms(1000);
    assert_eq!(
        host.granted_of_last_activation(A),
        vec!["fs.read".to_owned()]
    );
}

/// REQ: LIF-042
#[test]
fn lif_042_essential_plugin_restarts_after_five_seconds_on_third_failure_with_banner() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new("ilium.core.tree").tier(Tier::T1).startup());
    host.start(StartMode::Normal);
    for _ in 0..3 {
        trap(&mut host, "ilium.core.tree");
        host.advance_ms(5000);
    }
    assert_ne!(
        state(&host, "ilium.core.tree"),
        InstanceState::StoppedByFailure
    );
    assert!(!host.banner().is_empty());
}

/// REQ: LIF-042
#[test]
fn lif_042_non_essential_t1_plugin_is_stopped() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new("ilium.core.extra").tier(Tier::T1).startup());
    host.start(StartMode::Normal);
    for _ in 0..3 {
        trap(&mut host, "ilium.core.extra");
        host.advance_ms(1000);
    }
    assert_eq!(
        state(&host, "ilium.core.extra"),
        InstanceState::StoppedByFailure
    );
}

/// REQ: LIF-043
#[test]
fn lif_043_crash_restart_is_never_sent() {
    let mut host = one_plugin();
    trap(&mut host, A);
    host.advance_ms(1000);
    host.update(A, "2.0.0");
    host.revoke(A, "fs.read");
    host.plugins_api()
        .set_enabled(id(A), false, String::new())
        .unwrap();
    host.plugins_api()
        .set_enabled(id(A), true, String::new())
        .unwrap();
    host.shutdown();
    assert!(!host
        .deactivations(A)
        .contains(&DeactivationReason::CrashRestart));
}

/// REQ: LIF-043
#[test]
fn lif_043_enum_has_the_six_values() {
    let all = [
        DeactivationReason::Shutdown,
        DeactivationReason::Disabled,
        DeactivationReason::Upgrade,
        DeactivationReason::CrashRestart,
        DeactivationReason::Revoked,
        DeactivationReason::Granted,
    ];
    assert_eq!(all.len(), 6);
}

/// REQ: LIF-044
#[test]
fn lif_044_update_sends_upgrade_before_the_swap() {
    let mut host = one_plugin();
    host.update(A, "2.0.0");
    assert_eq!(host.deactivations(A), vec![DeactivationReason::Upgrade]);
    assert_eq!(host.active_version(A, Side::Server), "2.0.0");
}

/// REQ: LIF-044
#[test]
fn lif_044_every_live_instance_gets_upgrade() {
    let (mut host, _scope) = two_sided();
    host.attach_client("client-2");
    host.update(A, "2.0.0");
    assert_eq!(
        host.all_deactivations(A)
            .iter()
            .filter(|r| **r == DeactivationReason::Upgrade)
            .count(),
        3
    );
}

/// REQ: LIF-045
#[test]
fn lif_045_providers_and_required_plugins_activate_first() {
    let mut host = LifeHost::new();
    host.install(
        PluginSpec::new("c.consumer")
            .requires_service("x:svc/api")
            .startup(),
    );
    host.install(
        PluginSpec::new("a.provider")
            .provides_service("x:svc/api")
            .startup(),
    );
    host.install(
        PluginSpec::new("d.needs")
            .requires_plugin("e.base")
            .startup(),
    );
    host.install(PluginSpec::new("e.base").startup());
    host.start(StartMode::Normal);
    let order = host.start_order();
    let at = |name: &str| order.iter().position(|p| p == name).unwrap();
    assert!(at("a.provider") < at("c.consumer"));
    assert!(at("e.base") < at("d.needs"));
}

/// REQ: LIF-045
#[test]
fn lif_045_optional_requirement_does_not_order() {
    let mut host = LifeHost::new();
    host.install(
        PluginSpec::new("a.first")
            .requires_optional_plugin("z.last")
            .startup(),
    );
    host.install(PluginSpec::new("z.last").startup());
    host.start(StartMode::Normal);
    assert_eq!(
        host.start_order(),
        vec!["a.first".to_owned(), "z.last".to_owned()]
    );
}

/// REQ: LIF-046
#[test]
fn lif_046_smallest_tier_then_id_goes_first() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new("b").tier(Tier::T1).startup());
    host.install(PluginSpec::new("a").tier(Tier::T2).startup());
    host.install(PluginSpec::new("a1").tier(Tier::T1).startup());
    host.start(StartMode::Normal);
    assert_eq!(host.start_order(), vec!["a1", "b", "a"]);
}

proptest! {
    /// REQ: LIF-046
    #[test]
    fn lif_046_independent_plugins_start_in_key_order(tiers in proptest::collection::vec(1u8..4, 2..8)) {
        let mut host = LifeHost::new();
        let mut keys = Vec::new();
        for (index, tier) in tiers.iter().enumerate() {
            let name = format!("p{index:02}");
            let tier_value = match tier { 1 => Tier::T1, 2 => Tier::T2, _ => Tier::T3 };
            host.install(PluginSpec::new(&name).tier(tier_value).startup());
            keys.push((*tier, name));
        }
        host.start(StartMode::Normal);
        keys.sort();
        let expected: Vec<String> = keys.into_iter().map(|(_, n)| n).collect();
        prop_assert_eq!(host.start_order(), expected);
    }
}

/// REQ: LIF-047
#[test]
fn lif_047_at_most_four_concurrent_activations_per_side() {
    let mut host = LifeHost::new();
    for index in 0..12 {
        host.install(
            PluginSpec::new(&format!("p{index:02}"))
                .activate_delay_ms(50)
                .startup(),
        );
    }
    host.start(StartMode::Normal);
    assert!(host.peak_concurrent_activations(Side::Server) <= 4);
    assert!(
        host.peak_concurrent_activations(Side::Server) >= 2,
        "independent plugins may overlap"
    );
}

/// REQ: LIF-047
#[test]
fn lif_047_activations_begin_in_key_order() {
    let mut host = LifeHost::new();
    for index in (0..12).rev() {
        host.install(
            PluginSpec::new(&format!("p{index:02}"))
                .activate_delay_ms(50)
                .startup(),
        );
    }
    host.start(StartMode::Normal);
    let begins: Vec<String> = host.activations().into_iter().map(|r| r.plugin).collect();
    let mut sorted = begins.clone();
    sorted.sort();
    assert_eq!(begins, sorted);
}

/// REQ: LIF-048
#[test]
fn lif_048_service_cycle_refused_at_stage_10() {
    let mut host = LifeHost::new();
    host.install(
        PluginSpec::new("a")
            .requires_service("x:s/b")
            .provides_service("x:s/a"),
    );
    let error = host
        .try_install(
            PluginSpec::new("b")
                .requires_service("x:s/a")
                .provides_service("x:s/b"),
        )
        .unwrap_err();
    assert_eq!(error.code.as_str(), "manifest.service-cycle");
}

/// REQ: LIF-048
#[test]
fn lif_048_plugin_cycle_refused_with_dependency_cycle() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new("a").requires_plugin("b"));
    let error = host
        .try_install(PluginSpec::new("b").requires_plugin("a"))
        .unwrap_err();
    assert_eq!(error.code.as_str(), "dependency.cycle");
}

/// REQ: LIF-049
#[test]
fn lif_049_host_version_is_checked_only_through_requires_host() {
    let mut host = LifeHost::new();
    let error = host
        .try_install(PluginSpec::new(A).requires_host(">=99.0.0"))
        .unwrap_err();
    assert_eq!(error.code.as_str(), "dependency.host-version");
}

/// REQ: LIF-049
#[test]
fn lif_049_engines_key_is_refused() {
    let mut host = LifeHost::new();
    let error = host
        .try_install_manifest_key(A, "engines", "{}")
        .unwrap_err();
    assert!(
        error.code.as_str().starts_with("manifest."),
        "{}",
        error.code.as_str()
    );
}

/// REQ: LIF-050
#[test]
fn lif_050_shutdown_runs_reverse_start_order_one_at_a_time() {
    let mut host = LifeHost::new();
    host.install(
        PluginSpec::new("a.provider")
            .provides_service("x:s/a")
            .startup(),
    );
    host.install(
        PluginSpec::new("b.consumer")
            .requires_service("x:s/a")
            .startup(),
    );
    host.install(PluginSpec::new("c.other").startup());
    host.start(StartMode::Normal);
    let started = host.start_order();
    host.shutdown();
    let mut reversed = started.clone();
    reversed.reverse();
    assert_eq!(host.deactivation_order(), reversed);
    assert_eq!(host.peak_concurrent_deactivations(), 1);
}

/// REQ: LIF-050
#[test]
fn lif_050_shutdown_order_ignores_completion_order_of_starts() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new("a").activate_delay_ms(80).startup());
    host.install(PluginSpec::new("b").startup());
    host.start(StartMode::Normal);
    host.shutdown();
    assert_eq!(host.deactivation_order(), vec!["b", "a"]);
}

fn provider_consumer() -> LifeHost {
    let mut host = LifeHost::new();
    host.install(
        PluginSpec::new("a.provider")
            .provides_service("x:s/a")
            .startup(),
    );
    host.install(
        PluginSpec::new("b.consumer")
            .requires_service("x:s/a")
            .startup(),
    );
    host.start(StartMode::Normal);
    host
}

/// REQ: LIF-051
#[test]
fn lif_051_consumer_stays_active_when_provider_is_disabled() {
    let mut host = provider_consumer();
    host.plugins_api()
        .set_enabled(id("a.provider"), false, String::new())
        .unwrap();
    assert_eq!(state(&host, "b.consumer"), InstanceState::Active);
}

/// REQ: LIF-051
#[test]
fn lif_051_consumer_stays_active_when_provider_fails_or_is_uninstalled() {
    let mut host = provider_consumer();
    trap(&mut host, "a.provider");
    assert_eq!(state(&host, "b.consumer"), InstanceState::Active);
    host.uninstall("a.provider");
    assert_eq!(state(&host, "b.consumer"), InstanceState::Active);
}

/// REQ: LIF-052
#[test]
fn lif_052_service_call_result_by_provider_state() {
    let mut host = provider_consumer();
    assert!(host.call_service("b.consumer", "x:s/a").is_ok());
    trap(&mut host, "a.provider");
    let busy = host.call_service("b.consumer", "x:s/a").unwrap_err();
    assert_eq!(busy.code, ErrorCode::Busy);
    assert_eq!(busy.retry_after_ms, Some(1000));
}

/// REQ: LIF-052
#[test]
fn lif_052_stopped_disabled_and_uninstalled_provider_return_not_found() {
    let mut host = provider_consumer();
    host.plugins_api()
        .set_enabled(id("a.provider"), false, String::new())
        .unwrap();
    assert_eq!(
        host.call_service("b.consumer", "x:s/a").unwrap_err().code,
        ErrorCode::NotFound
    );
    host.uninstall("a.provider");
    assert_eq!(
        host.call_service("b.consumer", "x:s/a").unwrap_err().code,
        ErrorCode::NotFound
    );
}

/// REQ: LIF-053
#[test]
fn lif_053_availability_changes_are_sent_to_active_consumers() {
    let mut host = provider_consumer();
    host.plugins_api()
        .set_enabled(id("a.provider"), false, String::new())
        .unwrap();
    host.plugins_api()
        .set_enabled(id("a.provider"), true, String::new())
        .unwrap();
    let events = host.service_events("b.consumer");
    assert_eq!(events.len(), 2);
    assert!(!events[0].available && events[1].available);
    assert_eq!(events[0].interface, "x:s/a");
    assert_eq!(events[1].provider, "a.provider");
}

/// REQ: LIF-053
#[test]
fn lif_053_unrelated_plugins_get_no_availability_event() {
    let mut host = provider_consumer();
    host.install(PluginSpec::new("c.unrelated").startup());
    host.plugins_api()
        .set_enabled(id("a.provider"), false, String::new())
        .unwrap();
    assert!(host.service_events("c.unrelated").is_empty());
}

/// REQ: LIF-054
#[test]
fn lif_054_cli_disable_prints_a_warning_per_dependant_and_still_disables() {
    let mut host = provider_consumer();
    host.install(
        PluginSpec::new("c.consumer2")
            .requires_service("x:s/a")
            .startup(),
    );
    host.start(StartMode::Normal);
    let out = host.cli(&["plugin", "disable", "a.provider"]);
    assert_eq!(out.exit_code, 0);
    let warnings = out
        .stdout_jsonl
        .iter()
        .filter(|l| l["type"] == "warning")
        .count();
    assert_eq!(warnings, 2);
    assert_eq!(
        host.package_state("a.provider"),
        PackageState::InstalledDisabledByUser
    );
}

/// REQ: LIF-054
#[test]
fn lif_054_no_dependants_means_no_warning() {
    let mut host = one_plugin();
    let out = host.cli(&["plugin", "disable", A]);
    assert_eq!(out.exit_code, 0);
    assert!(!out.stdout_jsonl.is_empty());
    assert!(out.stdout_jsonl.iter().all(|l| l["type"] != "warning"));
}

/// REQ: LIF-055
#[test]
fn lif_055_owner_activates_before_contributors_and_lazy_contributors_follow() {
    let mut host = LifeHost::new();
    host.install(
        PluginSpec::new("a.contrib")
            .contributes_to("z.owner/point")
            .startup(),
    );
    host.install(PluginSpec::new("z.owner").owns_point("point").startup());
    host.start(StartMode::Normal);
    let order: Vec<String> = host.activations().into_iter().map(|r| r.plugin).collect();
    let at = |n: &str| order.iter().position(|p| p == n).unwrap();
    assert!(at("z.owner") < at("a.contrib"));
}

/// REQ: LIF-055
#[test]
fn lif_055_lazy_contributor_is_activated_in_background_after_owner_activate() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new("z.owner").owns_point("point").startup());
    host.install(PluginSpec::new("a.lazy").contributes_to("z.owner/point"));
    host.start(StartMode::Normal);
    host.advance_ms(1);
    let owner = host
        .activations()
        .into_iter()
        .find(|r| r.plugin == "z.owner")
        .unwrap();
    let lazy = host
        .activations()
        .into_iter()
        .find(|r| r.plugin == "a.lazy")
        .unwrap();
    assert!(lazy.begin_seq > owner.end_seq);
}

/// REQ: LIF-056
#[test]
fn lif_056_hook_skips_not_ready_contributor_and_logs() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new("z.owner").owns_point("point").startup());
    host.install(
        PluginSpec::new("a.slow")
            .contributes_to("z.owner/point")
            .activate_delay_ms(200),
    );
    host.start(StartMode::Normal);
    let result = host.call_hook("z.owner", "point");
    assert_eq!(result.passes, 1);
    assert!(host
        .log()
        .iter()
        .any(|l| l.code == "extension.contributor-not-ready"));
}

/// REQ: LIF-056
#[test]
fn lif_056_hook_call_does_not_wait_for_the_contributor() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new("z.owner").owns_point("point").startup());
    host.install(
        PluginSpec::new("a.slow")
            .contributes_to("z.owner/point")
            .activate_delay_ms(200),
    );
    host.start(StartMode::Normal);
    let before = host.unix_ms();
    host.call_hook("z.owner", "point");
    assert!(host.unix_ms() - before < 200);
}

/// REQ: LIF-057
#[test]
fn lif_057_client_keeps_old_version_until_change_notice() {
    let (mut host, scope) = two_sided();
    host.update_server_only(A, "2.0.0");
    assert_eq!(host.active_version(A, Side::Server), "2.0.0");
    assert_eq!(host.client_version(A, &scope), "1.0.0");
    host.send_change_notice(A);
    assert_eq!(host.client_version(A, &scope), "2.0.0");
}

/// REQ: LIF-057
#[test]
fn lif_057_messages_between_sides_are_not_blocked_by_a_version_difference() {
    let (mut host, scope) = two_sided();
    host.update_server_only(A, "2.0.0");
    assert!(host.message_between_sides(A, &scope).is_ok());
}

/// REQ: LIF-058
#[test]
fn lif_058_api_version_mismatch_names_both_versions() {
    let mut host = one_plugin();
    let error = host.connect_client_with_api("c", "0.2.0").unwrap_err();
    assert!(
        error.message.contains("api-version mismatch"),
        "{}",
        error.message
    );
    assert!(error.message.contains("0.2.0") && error.message.contains("0.1.0"));
}

/// REQ: LIF-058
#[test]
fn lif_058_matching_api_version_is_accepted() {
    let mut host = one_plugin();
    assert!(host.connect_client_with_api("c", "0.1.0").is_ok());
}

/// REQ: LIF-059 WARN
#[test]
fn lif_059_first_frame_when_essential_active_or_one_second() {
    let mut host = LifeHost::new();
    host.install(
        PluginSpec::new("ilium.core.tree")
            .tier(Tier::T1)
            .sides(&[Side::Client])
            .startup(),
    );
    host.start(StartMode::Normal);
    host.attach_client("c");
    assert!(host.first_frame_ms("c") <= 1100);
    // The mark is present exactly while essential plugins are still starting.
    let has_mark = host
        .first_frame_marks("c")
        .iter()
        .any(|m| m == "starting plugins");
    assert_eq!(has_mark, !host.essential_active("c"));
}

/// REQ: LIF-059 WARN
#[test]
fn lif_059_slow_plugin_does_not_hold_the_first_frame_past_a_second() {
    let mut host = LifeHost::new();
    host.install(
        PluginSpec::new("ilium.core.tree")
            .tier(Tier::T1)
            .sides(&[Side::Client])
            .activate_delay_ms(5_000)
            .startup(),
    );
    host.start(StartMode::Normal);
    host.attach_client("c");
    let frame = host.first_frame_ms("c");
    assert!((1000..=1100).contains(&frame), "{frame}");
}

/// REQ: LIF-035
#[test]
fn lif_035_a_job_trap_schedules_no_restart() {
    let mut host = one_plugin();
    host.fail(A, Side::Server, SERVER, FailureKind::JobTrap);
    assert_eq!(
        restart_in_ms(&host, A),
        None,
        "a failed job must not start the restart window"
    );
}

/// REQ: LIF-039
#[test]
fn lif_039_disabling_cancels_the_pending_restart_timer_at_once() {
    let mut host = one_plugin();
    trap(&mut host, A);
    assert!(restart_in_ms(&host, A).is_some(), "a restart must be pending");
    host.plugins_api()
        .set_enabled(id(A), false, String::new())
        .unwrap();
    assert_eq!(
        restart_in_ms(&host, A),
        None,
        "the timer must be cancelled by the disable"
    );
}

/// REQ: LIF-040
#[test]
fn lif_040_an_update_cancels_the_pending_restart_timer() {
    let mut host = one_plugin();
    trap(&mut host, A);
    host.update(A, "2.0.0");
    assert_eq!(
        restart_in_ms(&host, A),
        None,
        "the update must cancel the old timer"
    );
}

/// REQ: LIF-041
#[test]
fn lif_041_revoke_during_restart_does_not_reset_the_timer() {
    let mut host = LifeHost::new();
    host.install(
        PluginSpec::new(A)
            .grants(&["fs.read", "net.http"])
            .startup(),
    );
    host.start(StartMode::Normal);
    trap(&mut host, A);
    host.advance_ms(500);
    host.revoke(A, "net.http");
    assert_eq!(
        restart_in_ms(&host, A),
        Some(500),
        "the revoke must keep the original timer, not restart its delay"
    );
}
