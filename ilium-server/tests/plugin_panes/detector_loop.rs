//! Detection loop integration (capability-agent-signatures.md): HST-450, HST-451, HST-455.

use std::time::{Duration, Instant};

use crate::common::*;

/// REQ: HST-450
#[test]
fn hst_450_server_detector_call_thread_is_the_plugin_executor_not_the_loop() {
    let kernel = world(&["agent.signatures"]);
    kernel.run_detection_tick_with_fixture_detector("detector-thread-name");
    let names = kernel.detector_thread_names();
    assert!(!names.is_empty(), "fixture.missing or no call observed");
    assert!(names
        .iter()
        .all(|name| name.starts_with("plugin-executor")
            && *name != kernel.detection_loop_thread_name()));
}

/// REQ: HST-451
#[test]
fn hst_451_the_loop_tick_does_not_wait_for_a_detector_call() {
    let kernel = world(&["agent.signatures"]);
    kernel.set_detector_block_ms(5_000);
    let started = Instant::now();
    kernel.run_detection_tick();
    assert!(
        started.elapsed() < Duration::from_millis(250),
        "the tick waited for the detector call"
    );
}

/// REQ: HST-451
#[test]
fn hst_451_one_detector_call_per_pane_in_a_tick() {
    let kernel = world(&["agent.signatures"]);
    kernel.run_detection_tick();
    assert_eq!(kernel.detector_calls_in_last_tick(), kernel.pane_count());
}

/// REQ: HST-455
#[test]
fn hst_455_a_slow_detector_does_not_delay_another_plugins_detector() {
    let kernel = world(&["agent.signatures"]);
    kernel.set_detector_block_ms_for("slow.plugin", 5_000);
    kernel.run_detection_tick();
    assert!(kernel.wait_for_detector_call("fast.plugin", Duration::from_millis(500)));
}

/// REQ: HST-455
#[test]
fn hst_455_a_slow_detector_does_not_delay_the_next_loop_tick() {
    let kernel = world(&["agent.signatures"]);
    kernel.set_detector_block_ms_for("slow.plugin", 5_000);
    kernel.run_detection_tick();
    let started = Instant::now();
    kernel.run_detection_tick();
    assert!(started.elapsed() < Duration::from_millis(250));
}
