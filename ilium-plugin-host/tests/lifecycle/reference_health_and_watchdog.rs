//! reference-health-and-watchdog.md: LIF-122..138.

use crate::support::*;
use ilium_plugin_api::generated::host::plugins::HostPlugins;
use ilium_plugin_api::generated::types::requests::Outcome;
use ilium_plugin_api::ids::PluginId;
use ilium_plugin_host::engine::PluginEngine;
use proptest::prelude::*;

fn id(text: &str) -> PluginId {
    PluginId(text.to_owned())
}

/// REQ: LIF-122
#[test]
fn lif_122_each_health_state_is_reported_for_its_condition() {
    let mut host = one_plugin();
    assert_eq!(host.health(A), HealthState::Healthy);
    host.near_miss(A, 5);
    assert_eq!(host.health(A), HealthState::Degraded);
    host.advance_ms(301_000);
    trap(&mut host, A);
    assert_eq!(host.health(A), HealthState::Restarting);
    host.advance_ms(1000);
    trap(&mut host, A);
    host.advance_ms(5000);
    trap(&mut host, A);
    assert_eq!(host.health(A), HealthState::Failed);
    host.plugins_api()
        .set_enabled(id(A), false, String::new())
        .unwrap();
    assert_eq!(host.health(A), HealthState::Disabled);
}

/// REQ: LIF-122
#[test]
fn lif_122_refused_beats_every_other_row_and_the_plugins_api_agrees() {
    let mut host = LifeHost::new();
    host.install_refused(PluginSpec::new(A), "api.unsupported");
    assert_eq!(host.health(A), HealthState::Refused);
    let info = host
        .plugins_api()
        .list()
        .into_iter()
        .find(|i| i.id.0 == A)
        .unwrap();
    assert_eq!(info.health, HealthState::Refused);
    let status = records(&mut host, &["plugin", "status"], "plugin-status");
    assert_eq!(status[0]["health"], "refused");
}

/// REQ: LIF-122
#[test]
fn lif_122_lazy_plugin_never_activated_is_dormant() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new(A));
    host.start(StartMode::Normal);
    assert_eq!(host.health(A), HealthState::Dormant);
}

/// REQ: LIF-123
#[test]
fn lif_123_four_near_misses_stay_healthy_five_degrade() {
    let mut host = one_plugin();
    host.near_miss(A, 4);
    assert_eq!(host.health(A), HealthState::Healthy);
    host.near_miss(A, 1);
    assert_eq!(host.health(A), HealthState::Degraded);
}

/// REQ: LIF-123
#[test]
fn lif_123_degraded_returns_to_healthy_after_five_minutes() {
    let mut host = one_plugin();
    host.near_miss(A, 5);
    host.advance_ms(299_000);
    assert_eq!(host.health(A), HealthState::Degraded);
    host.advance_ms(2_000);
    assert_eq!(host.health(A), HealthState::Healthy);
}

/// REQ: LIF-123
#[test]
fn lif_123_queue_threshold_is_above_768_of_1024_for_a_minute() {
    let mut host = one_plugin();
    host.fill_event_queue(A, 750);
    host.advance_ms(61_000);
    assert_eq!(host.health(A), HealthState::Healthy);
    host.fill_event_queue(A, 751);
    assert_eq!(host.health(A), HealthState::Degraded);
}

/// REQ: LIF-123
#[test]
fn lif_123_rate_limited_and_quota_exceeded_results_degrade() {
    let mut host = one_plugin();
    host.rate_limited(A);
    assert_eq!(host.health(A), HealthState::Degraded);
    host.advance_ms(301_000);
    host.quota_exceeded(A);
    assert_eq!(host.health(A), HealthState::Degraded);
}

/// REQ: LIF-124
#[test]
fn lif_124_disabled_plugin_is_not_degraded() {
    let mut host = one_plugin();
    host.near_miss(A, 5);
    host.plugins_api()
        .set_enabled(id(A), false, String::new())
        .unwrap();
    assert_eq!(host.health(A), HealthState::Disabled);
}

/// REQ: LIF-124
#[test]
fn lif_124_stopped_plugin_is_failed_not_degraded() {
    let mut host = one_plugin();
    host.near_miss(A, 5);
    for _ in 0..3 {
        trap(&mut host, A);
        host.advance_ms(1000);
    }
    assert_eq!(host.health(A), HealthState::Failed);
}

/// REQ: LIF-125
#[test]
fn lif_125_loop_is_interrupted_within_deadline_plus_20_ms() {
    let mut host = one_plugin();
    let result = host.run_call(A, 20, 10_000, 0, 0);
    assert!(result.trapped);
    assert!(
        (20..=40).contains(&result.elapsed_ms),
        "{}",
        result.elapsed_ms
    );
}

proptest! {
    /// REQ: LIF-125
    #[test]
    fn lif_125_interrupt_time_stays_in_d_to_d_plus_20(deadline in 10u64..200) {
        let mut host = one_plugin();
        let result = host.run_call(A, deadline, 100_000, 0, 0);
        prop_assert!(result.trapped);
        prop_assert!(result.elapsed_ms >= deadline && result.elapsed_ms <= deadline + 20);
    }
}

/// REQ: LIF-126
#[test]
fn lif_126_call_at_ninety_percent_of_deadline_completes() {
    let mut host = one_plugin();
    let result = host.run_call(A, 100, 90, 0, 0);
    assert!(!result.trapped);
}

/// REQ: LIF-126
#[test]
fn lif_126_call_just_under_the_deadline_completes() {
    let mut host = one_plugin();
    assert!(!host.run_call(A, 50, 49, 0, 0).trapped);
}

/// REQ: LIF-127
#[test]
fn lif_127_fuel_is_off() {
    let engine = PluginEngine::new(host_engine_config()).expect("engine builds");
    assert!(!engine.fuel_enabled());
}

/// REQ: LIF-128
#[test]
fn lif_128_tick_is_ten_ms_in_every_process() {
    use ilium_plugin_api::ProcessKind;
    for process in [ProcessKind::Server, ProcessKind::Client] {
        let mut config = host_engine_config();
        config.process = process;
        let engine = PluginEngine::new(config).expect("engine builds");
        assert_eq!(engine.config().epoch_tick_ms, 10);
    }
}

proptest! {
    /// REQ: LIF-128
    #[test]
    fn lif_128_deadline_is_ceil_d_over_10_plus_one_ticks(deadline in 1u64..10_000) {
        let engine = PluginEngine::new(host_engine_config()).expect("engine builds");
        prop_assert_eq!(engine.deadline_ticks(deadline), (deadline + 9) / 10 + 1);
    }
}

fn host_engine_config() -> ilium_plugin_host::engine::EngineConfig {
    ilium_plugin_host::engine::EngineConfig {
        epoch_tick_ms: 10,
        cache_dir: None,
        process: ilium_plugin_api::ProcessKind::Server,
    }
}

/// REQ: LIF-129
#[test]
fn lif_129_synchronous_host_call_time_counts_toward_the_deadline() {
    let mut host = one_plugin();
    let result = host.run_call(A, 50, 5, 200, 0);
    assert!(result.trapped);
}

/// REQ: LIF-129
#[test]
fn lif_129_async_wait_does_not_count() {
    let mut host = one_plugin();
    let result = host.run_call(A, 50, 5, 0, 500);
    assert!(!result.trapped);
}

/// REQ: LIF-130
#[test]
fn lif_130_pending_requests_of_a_dropped_instance_complete_cancelled() {
    let mut host = one_plugin();
    host.pending_requests(A, 3);
    trap(&mut host, A);
    let outcomes = host.request_outcomes(A);
    assert_eq!(outcomes.len(), 3);
    assert!(outcomes.iter().all(|o| *o == Outcome::Cancelled));
    assert_eq!(host.export_calls_after_drop(A), 0);
}

/// REQ: LIF-130
#[test]
fn lif_130_deactivated_instance_cancels_its_requests_too() {
    let mut host = one_plugin();
    host.pending_requests(A, 2);
    host.plugins_api()
        .set_enabled(id(A), false, String::new())
        .unwrap();
    assert!(host
        .request_outcomes(A)
        .iter()
        .all(|o| *o == Outcome::Cancelled));
}

/// REQ: LIF-131
#[test]
fn lif_131_unanswered_request_completes_timed_out() {
    let mut host = one_plugin();
    host.request_never_answered(A, 1000);
    host.advance_ms(1000);
    let outcomes = host.request_outcomes(A);
    assert!(
        matches!(&outcomes[0], Outcome::Failed(e) | Outcome::Refused(e) if e.code == ilium_plugin_api::generated::types::ErrorCode::TimedOut)
    );
}

/// REQ: LIF-131
#[test]
fn lif_131_request_is_still_pending_just_before_its_timeout() {
    let mut host = one_plugin();
    host.request_never_answered(A, 1000);
    host.advance_ms(999);
    assert!(host.request_outcomes(A).is_empty());
}

/// REQ: LIF-132
#[test]
fn lif_132_helper_killed_after_three_missed_heartbeats() {
    let mut host = one_plugin();
    assert_eq!(host.helper_hang(3, 10_000), Some(3000));
}

/// REQ: LIF-132
#[test]
fn lif_132_helper_killed_500_ms_past_a_deadline_when_that_comes_first() {
    let mut host = one_plugin();
    assert_eq!(host.helper_hang(10, 500), Some(500));
    assert_eq!(host.helper_hang(2, 10_000), None);
}

/// REQ: LIF-133
#[test]
fn lif_133_helper_crash_counts_one_failure_for_the_running_plugin_only() {
    let mut host = LifeHost::new();
    for name in [A, B, C] {
        host.install(PluginSpec::new(name).tier(Tier::T2).startup());
    }
    host.start(StartMode::Normal);
    host.helper_crash(A, &[A, B, C]);
    assert_eq!(failures(&host, A), 1);
    assert_eq!(failures(&host, B), 0);
    assert_eq!(failures(&host, C), 0);
    assert_eq!(state(&host, B), InstanceState::Active);
}

/// REQ: LIF-134
#[test]
fn lif_134_five_helper_restarts_in_ten_minutes_are_allowed_the_sixth_stops() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new(A).tier(Tier::T2).startup());
    host.start(StartMode::Normal);
    for _ in 0..5 {
        host.helper_crash(B, &[A]);
    }
    assert_ne!(state(&host, A), InstanceState::StoppedByFailure);
    host.helper_crash(B, &[A]);
    assert_eq!(state(&host, A), InstanceState::StoppedByFailure);
    assert_eq!(
        host.side(A, Side::Server, SERVER)
            .last_error_code
            .as_deref(),
        Some("helper.unstable")
    );
}

/// REQ: LIF-134
#[test]
fn lif_134_restarts_older_than_ten_minutes_do_not_count() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new(A).tier(Tier::T2).startup());
    host.start(StartMode::Normal);
    for _ in 0..5 {
        host.helper_crash(B, &[A]);
    }
    host.advance_ms(601_000);
    host.helper_crash(B, &[A]);
    assert_ne!(state(&host, A), InstanceState::StoppedByFailure);
    assert!(host.helper_restarts() <= 5);
}

/// REQ: LIF-135
#[test]
fn lif_135_buckets_older_than_24_hours_are_discarded() {
    let mut host = one_plugin();
    host.near_miss(A, 1);
    for _ in 0..25 {
        host.advance_ms(3_600_000);
        host.near_miss(A, 1);
    }
    assert!(host.metric_buckets(A, Side::Server) <= 24);
}

/// REQ: LIF-135
#[test]
fn lif_135_hourly_buckets_accumulate_within_a_day() {
    let mut host = one_plugin();
    for _ in 0..5 {
        host.advance_ms(3_600_000);
        host.near_miss(A, 1);
    }
    assert!(host.metric_buckets(A, Side::Server) >= 5);
}

/// REQ: LIF-136
#[test]
fn lif_136_server_restart_resets_metrics() {
    let mut host = one_plugin();
    host.near_miss(A, 3);
    host.restart_server();
    assert_eq!(host.metric_buckets(A, Side::Server), 0);
}

/// REQ: LIF-137
#[test]
fn lif_137_status_prints_one_plugin_status_line_per_plugin() {
    let mut host = one_plugin();
    host.install(PluginSpec::new(B));
    let lines = records(&mut host, &["plugin", "status"], "plugin-status");
    assert_eq!(lines.len(), 2);
    assert!(lines
        .iter()
        .all(|l| l["health"].is_string() && l["id"].is_string()));
}

/// REQ: LIF-137
#[test]
fn lif_137_status_exit_code_is_zero_and_stderr_is_quiet() {
    let mut host = one_plugin();
    let out = host.cli(&["plugin", "status"]);
    assert_eq!(out.exit_code, 0);
    assert!(out.stderr.is_empty());
}

/// REQ: LIF-138
#[test]
fn lif_138_report_has_a_plugins_section_listing_unhealthy_plugins() {
    let mut host = one_plugin();
    host.near_miss(A, 5);
    let report = host.diagnostics_report();
    let section = report.split("Plugins").nth(1).expect("a Plugins section");
    assert!(section.contains(A));
}

/// REQ: LIF-138
#[test]
fn lif_138_healthy_plugins_are_left_out_of_the_section() {
    let mut host = one_plugin();
    host.install(PluginSpec::new(B).startup());
    host.near_miss(A, 5);
    let report = host.diagnostics_report();
    let section = report.split("Plugins").nth(1).expect("a Plugins section");
    assert!(!section.contains(B));
}
