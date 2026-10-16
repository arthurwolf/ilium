//! testing.md "Host conformance checklist": the fake host and the real host must agree (LIF-279).
//! The same stimulus runs against `FakeHost` (testkit) and against the host under test
//! (`HostHarness` with `RestartPolicy`), and the observable stories must be equal.

use std::time::Duration;

use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::lifecycle::{RestartDecision, RestartPolicy};
use ilium_plugin_testkit::{FakeHost, Mode};
use proptest::prelude::*;

use crate::common::{
    deliver_and_pump, harness_with, manifest_path, trap, ScriptedGuest,
};
use crate::gap;

fn fake_delays(at_times_ms: &[u64]) -> (Vec<u32>, bool) {
    let mut host = FakeHost::for_manifest(manifest_path("minimal.json")).expect("manifest loads");
    for at in at_times_ms {
        host.set_clock(*at);
        host.inject_trap("trap.deadline");
    }
    (host.restarts().iter().map(|restart| restart.delay_ms).collect(), host.is_disabled())
}

fn real_delays(at_times_ms: &[u64]) -> (Vec<u32>, bool) {
    let mut policy = RestartPolicy::default();
    let mut delays = Vec::new();
    let mut disabled = false;
    for at in at_times_ms {
        match policy.on_failure(*at) {
            RestartDecision::RestartAfterMs(delay) => delays.push(delay as u32),
            RestartDecision::Disable => {
                disabled = true;
                break;
            }
        }
    }
    (delays, disabled)
}

/// REQ: LIF-279
#[test]
fn lif_279_fake_host_and_real_rule_agree_on_the_three_failure_story() {
    let times = [0, 10_000, 20_000];
    assert_eq!(fake_delays(&times), real_delays(&times));
    assert_eq!(real_delays(&times), (vec![1_000, 5_000], true));
}

/// REQ: LIF-279
#[test]
fn lif_279_fake_host_and_real_rule_agree_when_old_failures_expire() {
    let times = [0, 6 * 60_000, 11 * 60_000];
    assert_eq!(fake_delays(&times), real_delays(&times));
}

proptest! {
    /// REQ: LIF-279
    #[test]
    fn lif_279_fake_host_and_real_rule_agree_for_any_failure_times(gaps in proptest::collection::vec(0u64..400_000, 1..8)) {
        let mut now = 0u64;
        let times: Vec<u64> = gaps.iter().map(|gap| { now += gap; now }).collect();
        prop_assert_eq!(fake_delays(&times), real_delays(&times));
    }
}

/// REQ: LIF-279
#[test]
fn lif_279_the_real_host_harness_walks_the_same_states_after_a_trap() {
    let (guest, _recorder) = ScriptedGuest::answering(Box::new(|_| Err(trap("trap.deadline"))));
    let mut handle = harness_with(guest, &[]);
    deliver_and_pump(&handle, vec![Event::ThemeChanged]);
    assert_eq!(gap::state_name(handle.state()), "restart-scheduled");
    handle.advance_clock(Duration::from_millis(1_000));
    let _ = handle.pump(8);
    assert_eq!(handle.state(), InstanceState::Active);
}

/// REQ: LIF-279
#[test]
fn lif_279_the_real_host_harness_stops_after_the_third_trap() {
    let (guest, _recorder) = ScriptedGuest::answering(Box::new(|_| Err(trap("trap.deadline"))));
    let mut handle = harness_with(guest, &[]);
    for wait_ms in [1_000u64, 5_000, 0] {
        deliver_and_pump(&handle, vec![Event::ThemeChanged]);
        handle.advance_clock(Duration::from_millis(wait_ms));
        let _ = handle.pump(8);
    }
    assert_eq!(handle.state(), InstanceState::StoppedByFailure);
}

/// REQ: LIF-279
#[test]
fn lif_279_fake_host_offers_every_method_of_the_testing_table() {
    let mut host = FakeHost::for_manifest(manifest_path("minimal.json")).expect("manifest loads");
    host.grant_now("sound.play").expect("declared");
    host.revoke("sound.play");
    host.grants_changed(&["sound.play"]);
    host.advance_clock(1);
    host.set_clock(2);
    let _ = host.clock().now();
    host.inject_trap("trap.deadline");
    let _ = host.restarts();
    let _ = host.is_disabled();
    let _ = host.failures();
    let _ = host.diagnostics();
    let _ = host.lifecycle_state();
    let _ = host.restored();
    let _ = host.logs();
    let _ = host.outcomes();
    let _ = host.submitted();
    let _ = host.pending_requests();
    let _ = host.snapshot();
}

/// REQ: LIF-279
#[test]
fn lif_279_fake_host_in_component_mode_reports_the_same_mode_it_was_given() {
    let host = FakeHost::new().mode(Mode::Component).build();
    assert_eq!(host.current_mode(), Mode::Component);
    let host = FakeHost::new().mode(Mode::Native).build();
    assert_eq!(host.current_mode(), Mode::Native);
}
