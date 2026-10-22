//! reference-host-runtime.md and reference-host-soak-and-load-testing.md rows HST-513 and
//! HST-515: compile-cache acceptance across Wasmtime versions and Engine recycling.

use ilium_plugin_host::engine_recycle::{
    load_compiled_cache, CacheOutcome, CompiledCache, EngineSupervisor, RecyclePolicy,
    RecycleRefused,
};

const MIB: u64 = 1024 * 1024;
const SEVEN_DAYS_SECONDS: u64 = 7 * 24 * 60 * 60;

fn production_policy() -> RecyclePolicy {
    RecyclePolicy {
        max_code_memory_bytes: 512 * MIB,
        max_uptime_seconds: SEVEN_DAYS_SECONDS,
    }
}

/// REQ: HST-513
#[test]
fn hst_513_a_cache_built_by_a_newer_wasmtime_is_rejected_and_rebuilt() {
    let cache = CompiledCache {
        wasmtime_version: "31.0.0".to_string(),
        payload: vec![0u8; 16],
    };
    assert_eq!(
        load_compiled_cache(&cache, "30.0.0"),
        CacheOutcome::RejectedAndRebuilt
    );
}

/// REQ: HST-513
#[test]
fn hst_513_a_cache_built_by_the_running_wasmtime_is_loaded() {
    let cache = CompiledCache {
        wasmtime_version: "30.0.0".to_string(),
        payload: vec![0u8; 16],
    };
    assert_eq!(load_compiled_cache(&cache, "30.0.0"), CacheOutcome::Loaded);
}

/// REQ: HST-515
#[test]
fn hst_515_code_memory_above_512_mib_makes_a_recycle_due() {
    let supervisor = EngineSupervisor::new(production_policy());
    assert!(supervisor.recycle_due(513 * MIB, 0));
    assert!(!supervisor.recycle_due(511 * MIB, 0));
}

/// REQ: HST-515
#[test]
fn hst_515_seven_days_of_uptime_makes_a_recycle_due_whatever_the_memory() {
    let supervisor = EngineSupervisor::new(production_policy());
    assert!(supervisor.recycle_due(0, SEVEN_DAYS_SECONDS));
    assert!(!supervisor.recycle_due(0, SEVEN_DAYS_SECONDS - 1));
}

/// REQ: HST-515
#[test]
fn hst_515_recycle_is_refused_while_a_call_is_in_flight() {
    let mut supervisor = EngineSupervisor::new(production_policy());
    let refusal = supervisor.recycle(1).expect_err("a call is in flight");
    assert_eq!(refusal, RecycleRefused::CallInFlight);
}

/// REQ: HST-515
#[test]
fn hst_515_recycle_keeps_plugins_active_counts_no_failure_and_logs_the_event() {
    let mut supervisor = EngineSupervisor::new(production_policy());
    supervisor.register_active_plugin("acme.notes");
    let report = supervisor.recycle(0).expect("no call in flight");
    assert_eq!(report.plugin_failure_delta, 0);
    assert_eq!(report.logged_event, "host.engine-recycled");
    assert_eq!(supervisor.active_plugin_ids(), vec!["acme.notes"]);
}
