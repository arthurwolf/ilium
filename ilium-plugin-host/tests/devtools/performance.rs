//! performance.md "Targets for the tools" (DEV-850..853). Benchmarks on the build host; each
//! measures the best of several runs against the stated target. Bounded loops only.

use std::time::{Duration, Instant};

use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_host::testing::HostHarness;

use crate::common::*;

const RUNS: usize = 5;

fn best_of(mut work: impl FnMut()) -> Duration {
    (0..RUNS)
        .map(|_| {
            let start = Instant::now();
            work();
            start.elapsed()
        })
        .min()
        .expect("runs")
}

fn batch() -> Vec<Event> {
    (0..64).map(|_| Event::PaneFocused(None)).collect()
}

fn one_batch(harness: &ilium_plugin_host::testing::HarnessHandle) {
    harness.deliver(batch());
    harness.pump(16);
}

fn component_harness() -> ilium_plugin_host::testing::HarnessHandle {
    HostHarness::new()
        .with_wat(&server_component_text(&[]))
        .build()
        .expect("build harness")
}

/// REQ: DEV-850
#[test]
fn dev_850_tracing_adds_at_most_5_percent_to_an_event_batch() {
    let plain = component_harness();
    let traced = HostHarness::new()
        .with_wat(&server_component_text(&[]))
        .tracing(true)
        .build()
        .expect("build traced harness");
    let base = best_of(|| one_batch(&plain));
    let with_trace = best_of(|| one_batch(&traced));
    assert!(
        with_trace.as_secs_f64() <= base.as_secs_f64() * 1.05,
        "{with_trace:?} vs {base:?}"
    );
}

/// REQ: DEV-850
#[test]
fn dev_850_profiling_changes_call_time_by_at_most_5_percent() {
    let plain = component_harness();
    let profiled = HostHarness::new()
        .with_wat(&server_component_text(&[]))
        .profiling(true)
        .build()
        .expect("build profiled harness");
    let base = best_of(|| one_batch(&plain)).as_secs_f64();
    let with_profile = best_of(|| one_batch(&profiled)).as_secs_f64();
    assert!(
        (with_profile - base).abs() <= base * 0.05,
        "{with_profile} vs {base}"
    );
}

/// REQ: DEV-851 WARN
#[test]
fn dev_851_native_mode_event_batch_finishes_under_10_ms() {
    let harness = HostHarness::new()
        .with_native_guest(Box::new(ilium_plugin_host::testing::NullGuest))
        .build()
        .expect("build native harness");
    let elapsed = best_of(|| one_batch(&harness));
    assert!(elapsed < Duration::from_millis(10), "{elapsed:?}");
}

/// REQ: DEV-851 WARN
#[test]
fn dev_851_component_mode_event_batch_finishes_under_100_ms_after_load() {
    let harness = component_harness();
    one_batch(&harness); // one-time load is excluded
    let elapsed = best_of(|| one_batch(&harness));
    assert!(elapsed < Duration::from_millis(100), "{elapsed:?}");
}

/// REQ: DEV-852 WARN
#[test]
fn dev_852_check_of_a_10_mib_package_finishes_under_2_s() {
    let root = scratch();
    let env = env(root.path());
    let file = write_package_file(
        root.path(),
        "big.ilp",
        &unsigned_package(&server_component_of_size(10 * 1024 * 1024 - 4096)),
    );
    let elapsed = best_of(|| {
        run(&env, &["check", &file, "--json"]);
    });
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
}

/// REQ: DEV-852 WARN
#[test]
fn dev_852_lint_of_a_project_finishes_under_5_s() {
    let (_root, env) = lint_env_with(|_| {});
    let elapsed = best_of(|| {
        lint(&env, &[]);
    });
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
}

/// REQ: DEV-853 WARN
#[test]
fn dev_853_dev_reload_budget_is_1_s_after_the_build() {
    assert_eq!(
        ilium_plugin_host::cli::dev::RELOAD_BUDGET,
        Duration::from_secs(1)
    );
}

/// REQ: DEV-853 WARN
#[test]
fn dev_853_change_debounce_is_300_ms() {
    assert_eq!(
        ilium_plugin_host::cli::dev::DEBOUNCE,
        Duration::from_millis(300)
    );
}

/// REQ: DEV-853 WARN
#[test]
fn dev_853_two_changes_inside_the_debounce_trigger_one_rebuild() {
    let mut debouncer = ilium_plugin_host::cli::dev::Debouncer::new(Duration::from_millis(300));
    debouncer.change(0);
    debouncer.change(200);
    assert_eq!(debouncer.due(499), 0);
    assert_eq!(debouncer.due(500), 1);
}
