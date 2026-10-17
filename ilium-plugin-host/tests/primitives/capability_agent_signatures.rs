//! agent.signatures detectors (capability-agent-signatures.md): detector order, poll budget,
//! deadlines, executor thread. Requirement ids HST-450..457 and the detector export ids.
//! All detector scheduling goes through gap accessors listed in API_GAPS.md.

use ilium_plugin_api::generated::types::ids::NodeId;
use ilium_plugin_api::{ErrorCode, ProcessKind, Side, Tier};
use ilium_plugin_host::testing::{FakeKernel, HostHarness};
use proptest::prelude::*;
use std::sync::Arc;

use crate::common::*;

const SIGNATURES: &[&str] = &["agent.signatures", "agent.read"];

/// Server-side host with detector plugins `ids` (each `(plugin id, tier)`), all granted.
fn detector_host(
    plugins: &[(&str, Tier)],
) -> (Arc<FakeKernel>, ilium_plugin_host::testing::HarnessHandle) {
    let kernel = Arc::new(FakeKernel::new(ProcessKind::Server));
    kernel.set_tree(standard_tree());
    let mut builder = HostHarness::new()
        .side(Side::Server)
        .grants(SIGNATURES)
        .kernel(kernel.clone());
    for (id, tier) in plugins {
        builder = builder.with_detector_plugin(id, *tier);
    }
    (
        kernel,
        builder.build().expect("harness builds detector plugins"),
    )
}

// ---------------------------------------------------------------- HST-450

/// REQ: HST-450
#[test]
fn hst_450_detector_call_runs_on_the_plugin_executor_thread() {
    let wasm = fixture_wasm("detector-thread-name");
    let (kernel, mut host) = detector_host(&[("test.plugin", Tier::T3)]);
    host.load_detector_wasm("test.plugin", &wasm);
    host.detect_cycle(&[PANE]);
    let names = host.detector_thread_names("test.plugin");
    assert!(!names.is_empty(), "no detector call observed");
    assert!(
        names.iter().all(|name| name.starts_with("plugin-executor")),
        "{names:?}"
    );
    drop(kernel);
}

/// REQ: HST-450
#[test]
fn hst_450_detector_call_never_runs_on_the_loop_thread() {
    let (_kernel, mut host) = detector_host(&[("test.plugin", Tier::T3)]);
    host.detect_cycle(&[PANE]);
    let loop_thread = host.detection_loop_thread_name();
    assert!(host
        .detector_thread_names("test.plugin")
        .iter()
        .all(|name| *name != loop_thread));
}

// ---------------------------------------------------------------- HST-451

/// REQ: HST-451
#[test]
fn hst_451_one_detector_call_per_pane_per_cycle() {
    let (_kernel, mut host) = detector_host(&[("test.plugin", Tier::T3)]);
    host.detect_cycle(&[PANE, EDITOR_PANE, BASIC_PANE]);
    assert_eq!(host.detector_call_count("test.plugin"), 3);
}

/// REQ: HST-451
#[test]
fn hst_451_a_tick_returns_before_a_blocked_detector_call_finishes() {
    let (_kernel, mut host) = detector_host(&[("test.plugin", Tier::T3)]);
    host.set_detector_cost_ms("test.plugin", 5_000);
    let started = std::time::Instant::now();
    host.detect_cycle(&[PANE]);
    assert!(
        started.elapsed() < std::time::Duration::from_millis(500),
        "the tick waited for the call"
    );
}

// ---------------------------------------------------------------- HST-452

/// REQ: HST-452
#[test]
fn hst_452_t1_detectors_are_asked_before_others() {
    let (_kernel, mut host) = detector_host(&[("a.third", Tier::T3), ("z.first", Tier::T1)]);
    host.detect_cycle(&[PANE]);
    assert_eq!(
        host.detector_ask_order(PANE),
        vec!["z.first".to_owned(), "a.third".to_owned()]
    );
}

/// REQ: HST-452
#[test]
fn hst_452_equal_tiers_are_asked_by_plugin_id() {
    let (_kernel, mut host) = detector_host(&[
        ("b.two", Tier::T3),
        ("a.one", Tier::T3),
        ("c.three", Tier::T3),
    ]);
    host.detect_cycle(&[PANE]);
    assert_eq!(
        host.detector_ask_order(PANE),
        vec!["a.one", "b.two", "c.three"]
    );
}

/// REQ: HST-452, WIT:ilium:exports/detector.identify
#[test]
fn hst_452_first_signature_id_wins_and_stops_the_search() {
    let (_kernel, mut host) = detector_host(&[("a.one", Tier::T3), ("b.two", Tier::T3)]);
    host.set_detector_identity("a.one", Some("sig-a"));
    host.set_detector_identity("b.two", Some("sig-b"));
    host.detect_cycle(&[PANE]);
    assert_eq!(host.identified_signature(PANE).as_deref(), Some("sig-a"));
    assert_eq!(host.detector_ask_order(PANE), vec!["a.one"]);
}

/// REQ: HST-452, WIT:ilium:exports/detector.identify
#[test]
fn hst_452_no_detector_identifies_leaves_the_pane_unidentified() {
    let (_kernel, mut host) = detector_host(&[("a.one", Tier::T3)]);
    host.set_detector_identity("a.one", None);
    host.detect_cycle(&[PANE]);
    assert_eq!(host.identified_signature(PANE), None);
}

/// REQ: WIT:ilium:exports/detector.classify
#[test]
fn wit_detector_classify_result_becomes_the_pane_activity() {
    let (kernel, mut host) = detector_host(&[("a.one", Tier::T3)]);
    kernel.set_tree(vec![agent_node(
        PANE,
        "sig-a",
        ilium_plugin_api::generated::types::tree::AgentActivity::Idle,
    )]);
    host.set_detector_classification(
        "a.one",
        ilium_plugin_api::generated::types::tree::AgentActivity::Working,
    );
    host.detect_cycle(&[PANE]);
    assert_eq!(
        host.pane_activity(PANE),
        Some(ilium_plugin_api::generated::types::tree::AgentActivity::Working)
    );
}

/// REQ: WIT:ilium:exports/detector.classify
#[test]
fn wit_detector_classify_unchanged_result_emits_no_event() {
    let (kernel, mut host) = detector_host(&[("a.one", Tier::T3)]);
    kernel.set_tree(vec![agent_node(
        PANE,
        "sig-a",
        ilium_plugin_api::generated::types::tree::AgentActivity::Idle,
    )]);
    host.set_detector_classification(
        "a.one",
        ilium_plugin_api::generated::types::tree::AgentActivity::Idle,
    );
    host.detect_cycle(&[PANE]);
    assert_eq!(host.activity_events_emitted(), 0);
}

// ---------------------------------------------------------------- HST-453

fn many_panes(count: u64) -> Vec<NodeId> {
    (0..count).map(|index| NodeId(100 + index)).collect()
}

/// REQ: HST-453
#[test]
fn hst_453_cycle_spends_at_most_50_ms_of_detector_time() {
    let (_kernel, mut host) = detector_host(&[("test.plugin", Tier::T3)]);
    host.set_detector_cost_ms("test.plugin", 10);
    let report = host.detect_cycle(&many_panes(30));
    assert!(report.spent_ms <= 50, "{} ms", report.spent_ms);
}

/// REQ: HST-453
#[test]
fn hst_453_a_cycle_that_fits_the_budget_serves_every_pane() {
    let (_kernel, mut host) = detector_host(&[("test.plugin", Tier::T3)]);
    host.set_detector_cost_ms("test.plugin", 1);
    assert_eq!(host.detect_cycle(&many_panes(30)).served.len(), 30);
}

/// REQ: HST-453
#[test]
fn hst_453_the_start_pane_rotates_between_cycles() {
    let (_kernel, mut host) = detector_host(&[("test.plugin", Tier::T3)]);
    host.set_detector_cost_ms("test.plugin", 10);
    let panes = many_panes(30);
    let first = host.detect_cycle(&panes).served.first().copied();
    let second = host.detect_cycle(&panes).served.first().copied();
    assert_ne!(first, second);
}

/// REQ: HST-453
#[test]
fn hst_453_unserved_panes_keep_their_last_activity() {
    let (kernel, mut host) = detector_host(&[("test.plugin", Tier::T3)]);
    host.set_detector_cost_ms("test.plugin", 20);
    host.set_detector_classification(
        "test.plugin",
        ilium_plugin_api::generated::types::tree::AgentActivity::Working,
    );
    let panes = many_panes(10);
    let report = host.detect_cycle(&panes);
    for pane in panes.iter().filter(|pane| !report.served.contains(pane)) {
        assert_eq!(host.pane_activity(*pane), kernel.initial_activity(*pane));
    }
    assert!(
        report.served.len() < panes.len(),
        "the budget must leave some panes unserved"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    // REQ: HST-453
    #[test]
    fn hst_453_spent_time_never_exceeds_the_budget(cost in 1u64..40, count in 1u64..40) {
        let (_kernel, mut host) = detector_host(&[("test.plugin", Tier::T3)]);
        host.set_detector_cost_ms("test.plugin", cost);
        prop_assert!(host.detect_cycle(&many_panes(count)).spent_ms <= 50);
    }
}

// ---------------------------------------------------------------- HST-454

/// REQ: HST-454
#[test]
fn hst_454_missed_deadline_keeps_the_previous_classification_and_emits_nothing() {
    let (kernel, mut host) = detector_host(&[("test.plugin", Tier::T3)]);
    kernel.set_tree(vec![agent_node(
        PANE,
        "sig",
        ilium_plugin_api::generated::types::tree::AgentActivity::Idle,
    )]);
    host.set_detector_classification(
        "test.plugin",
        ilium_plugin_api::generated::types::tree::AgentActivity::Working,
    );
    host.set_detector_cost_ms("test.plugin", 21);
    host.detect_cycle(&[PANE]);
    assert_eq!(
        host.pane_activity(PANE),
        Some(ilium_plugin_api::generated::types::tree::AgentActivity::Idle)
    );
    assert_eq!(host.activity_events_emitted(), 0);
}

/// REQ: HST-454
#[test]
fn hst_454_a_call_at_the_deadline_counts_and_one_over_is_a_breach() {
    let (_kernel, mut host) = detector_host(&[("test.plugin", Tier::T3)]);
    host.set_detector_cost_ms("test.plugin", 20);
    host.detect_cycle(&[PANE]);
    assert_eq!(host.detector_breaches("test.plugin"), 0);
    host.set_detector_cost_ms("test.plugin", 21);
    host.detect_cycle(&[PANE]);
    assert_eq!(host.detector_breaches("test.plugin"), 1);
}

/// REQ: HST-454
#[test]
fn hst_454_the_third_breach_in_a_row_traps_the_side() {
    let (_kernel, mut host) = detector_host(&[("test.plugin", Tier::T3)]);
    host.set_detector_cost_ms("test.plugin", 30);
    host.detect_cycle(&[PANE]);
    host.detect_cycle(&[PANE]);
    assert!(host.failures().is_empty(), "two breaches must not trap");
    host.detect_cycle(&[PANE]);
    assert_eq!(host.failures().len(), 1);
}

/// REQ: HST-454
#[test]
fn hst_454_a_good_call_resets_the_breach_count() {
    let (_kernel, mut host) = detector_host(&[("test.plugin", Tier::T3)]);
    host.set_detector_cost_ms("test.plugin", 30);
    host.detect_cycle(&[PANE]);
    host.detect_cycle(&[PANE]);
    host.set_detector_cost_ms("test.plugin", 1);
    host.detect_cycle(&[PANE]);
    host.set_detector_cost_ms("test.plugin", 30);
    host.detect_cycle(&[PANE]);
    host.detect_cycle(&[PANE]);
    assert!(host.failures().is_empty());
}

// ---------------------------------------------------------------- HST-455

/// REQ: HST-455
#[test]
fn hst_455_a_slow_detector_does_not_delay_another_plugins_detector() {
    let (_kernel, mut host) = detector_host(&[("a.slow", Tier::T3), ("b.fast", Tier::T3)]);
    host.set_detector_cost_ms("a.slow", 5_000);
    host.set_detector_cost_ms("b.fast", 0);
    let started = std::time::Instant::now();
    host.detect_cycle(&[PANE, EDITOR_PANE]);
    assert!(host.detector_call_count("b.fast") >= 1);
    assert!(started.elapsed() < std::time::Duration::from_millis(500));
}

/// REQ: HST-455
#[test]
fn hst_455_a_slow_detector_does_not_delay_the_detection_loop() {
    let (_kernel, mut host) = detector_host(&[("a.slow", Tier::T3)]);
    host.set_detector_cost_ms("a.slow", 5_000);
    let first = host.detect_cycle(&[PANE]);
    let second = host.detect_cycle(&[PANE]);
    assert!(
        first.tick_ms < 50 && second.tick_ms < 50,
        "{} / {}",
        first.tick_ms,
        second.tick_ms
    );
}

// ---------------------------------------------------------------- HST-456

/// REQ: HST-456
#[test]
fn hst_456_default_deadline_is_20_ms() {
    let (_kernel, host) = detector_host(&[("test.plugin", Tier::T3)]);
    assert_eq!(host.detector_deadline_ms("test.plugin", None), 20);
}

/// REQ: HST-456
#[test]
fn hst_456_t1_ceiling_is_50_ms_and_t2_t3_ceiling_is_20_ms() {
    let (_kernel, host) =
        detector_host(&[("t1.p", Tier::T1), ("t2.p", Tier::T2), ("t3.p", Tier::T3)]);
    assert_eq!(host.detector_deadline_ms("t1.p", Some(50)), 50);
    assert_eq!(host.detector_deadline_ms("t1.p", Some(51)), 50);
    assert_eq!(host.detector_deadline_ms("t2.p", Some(21)), 20);
    assert_eq!(host.detector_deadline_ms("t3.p", Some(21)), 20);
    assert_eq!(host.detector_deadline_ms("t3.p", Some(19)), 19);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    // REQ: HST-456
    #[test]
    fn hst_456_deadline_never_exceeds_the_tier_ceiling(requested in 1u64..200) {
        let (_kernel, host) = detector_host(&[("t1.p", Tier::T1), ("t3.p", Tier::T3)]);
        prop_assert!(host.detector_deadline_ms("t1.p", Some(requested)) <= 50);
        prop_assert!(host.detector_deadline_ms("t3.p", Some(requested)) <= 20);
    }
}

// ---------------------------------------------------------------- HST-457

/// REQ: HST-457
#[test]
fn hst_457_classify_30_panes_costs_under_half_a_millisecond_per_pane_p99() {
    let (_kernel, mut host) = detector_host(&[("test.plugin", Tier::T3)]);
    host.set_detector_cost_ms("test.plugin", 0);
    let panes = many_panes(30);
    let mut per_pane_micros: Vec<u128> = Vec::new();
    for _ in 0..200 {
        let started = std::time::Instant::now();
        host.detect_cycle(&panes);
        per_pane_micros.push(started.elapsed().as_micros() / 30);
    }
    per_pane_micros.sort_unstable();
    let p99 = per_pane_micros[per_pane_micros.len() * 99 / 100];
    assert!(p99 < 500, "p99 {p99} us per pane");
}

/// REQ: WIT:ilium:exports/detector.classify
#[test]
fn wit_detector_classify_without_grant_is_never_called() {
    let kernel = Arc::new(FakeKernel::new(ProcessKind::Server));
    kernel.set_tree(standard_tree());
    let mut host = HostHarness::new()
        .side(Side::Server)
        .grants(&[])
        .kernel(kernel)
        .with_detector_plugin("test.plugin", Tier::T3)
        .build()
        .expect("harness builds");
    host.detect_cycle(&[PANE]);
    assert_eq!(host.detector_call_count("test.plugin"), 0);
    let _ = ErrorCode::NotPermitted;
}
