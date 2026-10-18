//! reference-resource-accounting.md: LIF-139..146.

use crate::support::*;
use ilium_plugin_api::generated::types::ErrorCode;
use ilium_plugin_api::limits::LimitKey;
use ilium_plugin_host::testing::life::LimitSource;
use proptest::prelude::*;

fn expected_retry(balance_ms: i64) -> u32 {
    let doubled = (-balance_ms * 2) as u64;
    let rounded = doubled.div_ceil(10) * 10;
    rounded.max(100) as u32
}

/// REQ: LIF-139
#[test]
fn lif_139_empty_bucket_refuses_with_quota_exceeded_and_retry_after() {
    let mut host = one_plugin();
    let error = host.bucket_refusal(A, -100);
    assert_eq!(error.code, ErrorCode::QuotaExceeded);
    assert_eq!(error.retry_after_ms, Some(200));
}

/// REQ: LIF-139
#[test]
fn lif_139_retry_after_has_a_floor_of_100_ms() {
    let mut host = one_plugin();
    assert_eq!(host.bucket_refusal(A, -1).retry_after_ms, Some(100));
    assert_eq!(host.bucket_refusal(A, -50).retry_after_ms, Some(100));
    assert_eq!(host.bucket_refusal(A, -51).retry_after_ms, Some(110));
}

proptest! {
    /// REQ: LIF-139
    #[test]
    fn lif_139_retry_after_follows_the_formula(balance in -5000i64..0) {
        let mut host = one_plugin();
        prop_assert_eq!(host.bucket_refusal(A, balance).retry_after_ms, Some(expected_retry(balance)));
    }
}

/// REQ: LIF-139
#[test]
fn lif_139_bucket_refills_500_ms_per_second_up_to_2000_ms() {
    let mut host = one_plugin();
    host.drain_bucket(A);
    host.advance_ms(1000);
    assert_eq!(host.bucket_balance_ms(A), 500);
    host.advance_ms(100_000);
    assert_eq!(host.bucket_balance_ms(A), 2000);
}

/// REQ: LIF-139
#[test]
fn lif_139_payload_costs_one_ms_per_64_kib() {
    let mut host = one_plugin();
    let before = host.bucket_balance_ms(A);
    host.submit_payload(A, 64 * 1024 * 4);
    assert!(before - host.bucket_balance_ms(A) >= 4);
}

/// REQ: LIF-140
#[test]
fn lif_140_hundred_quota_refusals_are_not_failures() {
    let mut host = one_plugin();
    for _ in 0..100 {
        host.bucket_refusal(A, -100);
    }
    assert_eq!(failures(&host, A), 0);
    assert_eq!(state(&host, A), InstanceState::Active);
}

/// REQ: LIF-141
#[test]
fn lif_141_unfit_reservation_stays_inactive_and_starts_when_memory_is_freed() {
    let mut host = LifeHost::new().with_pool_mib(40);
    host.install(PluginSpec::new(B).memory_mib(32).startup());
    host.install(PluginSpec::new(A).memory_mib(32).startup());
    host.start(StartMode::Normal);
    assert_eq!(state(&host, A), InstanceState::Inactive);
    assert_eq!(failures(&host, A), 0);
    host.cli(&["plugin", "disable", B]);
    assert_eq!(state(&host, A), InstanceState::Active);
}

/// REQ: LIF-141
#[test]
fn lif_141_a_reservation_that_exactly_fits_is_admitted() {
    let mut host = LifeHost::new().with_pool_mib(32);
    host.install(PluginSpec::new(A).memory_mib(32).startup());
    host.start(StartMode::Normal);
    assert_eq!(state(&host, A), InstanceState::Active);
}

/// REQ: LIF-141
#[test]
fn lif_141_a_reservation_one_mib_over_is_refused() {
    let mut host = LifeHost::new().with_pool_mib(31);
    host.install(PluginSpec::new(A).memory_mib(32).startup());
    host.start(StartMode::Normal);
    assert_eq!(state(&host, A), InstanceState::Inactive);
}

/// REQ: LIF-142
#[test]
fn lif_142_core_t1_is_admitted_before_a_third_party_startup_plugin() {
    let mut host = LifeHost::new().with_pool_mib(64);
    host.install(
        PluginSpec::new("aaa.third")
            .tier(Tier::T3)
            .memory_mib(32)
            .startup(),
    );
    host.install(
        PluginSpec::new("zzz.core")
            .tier(Tier::T1)
            .memory_mib(32)
            .startup(),
    );
    host.install(
        PluginSpec::new("yyy.core")
            .tier(Tier::T1)
            .memory_mib(32)
            .startup(),
    );
    host.start(StartMode::Normal);
    assert_eq!(state(&host, "zzz.core"), InstanceState::Active);
    assert_eq!(state(&host, "yyy.core"), InstanceState::Active);
    assert_eq!(state(&host, "aaa.third"), InstanceState::Inactive);
}

/// REQ: LIF-142
#[test]
fn lif_142_third_party_fits_when_the_pool_has_room_for_all() {
    let mut host = LifeHost::new().with_pool_mib(96);
    host.install(PluginSpec::new("aaa.third").memory_mib(32).startup());
    host.install(
        PluginSpec::new("zzz.core")
            .tier(Tier::T1)
            .memory_mib(32)
            .startup(),
    );
    host.start(StartMode::Normal);
    assert_eq!(state(&host, "aaa.third"), InstanceState::Active);
}

/// REQ: LIF-143
#[test]
fn lif_143_ceiling_clamps_manifest_user_values() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new(A).memory_mib(200));
    assert_eq!(
        host.effective_limit(A, LimitKey::MemoryMib),
        (128, LimitSource::Ceiling)
    );
}

/// REQ: LIF-143
#[test]
fn lif_143_user_value_below_ceiling_wins_and_above_ceiling_clamps() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new(A).memory_mib(200));
    host.set_user_limit(Some(A), LimitKey::MemoryMib, 64);
    assert_eq!(
        host.effective_limit(A, LimitKey::MemoryMib),
        (64, LimitSource::User)
    );
    host.set_user_limit(Some(A), LimitKey::MemoryMib, 256);
    assert_eq!(host.effective_limit(A, LimitKey::MemoryMib).0, 128);
}

/// REQ: LIF-143
#[test]
fn lif_143_zero_means_unset_and_the_next_source_applies() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new(A).memory_mib(48));
    host.set_user_limit(Some(A), LimitKey::MemoryMib, 0);
    assert_eq!(
        host.effective_limit(A, LimitKey::MemoryMib),
        (48, LimitSource::Manifest)
    );
}

/// REQ: LIF-143
#[test]
fn lif_143_default_applies_when_nothing_else_is_set() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new(A));
    assert_eq!(
        host.effective_limit(A, LimitKey::MemoryMib),
        (32, LimitSource::Default)
    );
}

/// REQ: LIF-143
#[test]
fn lif_143_session_key_raised_above_its_current_value_is_clamped() {
    let defaults = ilium_plugin_host::session::SessionLimits::default();
    let raised = ilium_plugin_host::session::SessionLimits::from_user(&[(
        "session_memory_mib",
        defaults.session_memory_mib + 1,
    )]);
    assert_eq!(raised.session_memory_mib, defaults.session_memory_mib);
}

/// REQ: LIF-144
#[test]
fn lif_144_per_plugin_user_value_beats_the_global_user_value() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new(A));
    host.set_user_limit(None, LimitKey::MemoryMib, 96);
    host.set_user_limit(Some(A), LimitKey::MemoryMib, 48);
    assert_eq!(
        host.effective_limit(A, LimitKey::MemoryMib),
        (48, LimitSource::User)
    );
}

/// REQ: LIF-144
#[test]
fn lif_144_global_user_value_applies_to_plugins_without_their_own() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new(A));
    host.install(PluginSpec::new(B));
    host.set_user_limit(None, LimitKey::MemoryMib, 96);
    host.set_user_limit(Some(A), LimitKey::MemoryMib, 48);
    assert_eq!(host.effective_limit(B, LimitKey::MemoryMib).0, 96);
}

/// REQ: LIF-145
#[test]
fn lif_145_cgroup_memory_max_is_instance_memory_plus_32_mib() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new(A).tier(Tier::T2).memory_mib(32).startup());
    host.install(PluginSpec::new(B).tier(Tier::T2).memory_mib(64).startup());
    host.start(StartMode::Normal);
    assert_eq!(host.cgroup_memory_max_mib(), 32 + 64 + 32);
}

/// REQ: LIF-145
#[test]
fn lif_145_the_same_amount_is_charged_to_the_session_pool() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new(A).tier(Tier::T2).memory_mib(32).startup());
    host.start(StartMode::Normal);
    assert_eq!(host.session_pool_used_mib(), host.cgroup_memory_max_mib());
}

/// REQ: LIF-146
#[test]
fn lif_146_stats_print_use_limit_and_source_for_four_resources() {
    let mut host = one_plugin();
    let lines = records(&mut host, &["plugin", "stats", A], "plugin-stat");
    let resources: Vec<&str> = lines
        .iter()
        .map(|l| l["resource"].as_str().unwrap())
        .collect();
    for resource in ["memory", "execution", "storage", "disk"] {
        assert!(resources.contains(&resource), "{resource} in {resources:?}");
    }
    for line in &lines {
        for key in ["id", "side", "used", "limit", "source"] {
            assert!(line.get(key).is_some(), "{key} in {line}");
        }
    }
}

/// REQ: LIF-146
#[test]
fn lif_146_source_is_one_of_the_five_documented_values() {
    let mut host = one_plugin();
    let lines = records(&mut host, &["plugin", "stats", A], "plugin-stat");
    for line in lines {
        let source = line["source"].as_str().unwrap();
        assert!(
            ["default", "manifest", "user", "ceiling", "fixed"].contains(&source),
            "{source}"
        );
    }
}
