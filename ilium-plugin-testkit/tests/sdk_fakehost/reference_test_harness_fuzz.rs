//! reference-test-harness.md "Property tests and fuzz hooks": DEV-520 to DEV-523.

use ilium_plugin_sdk_core::types::Event;
use ilium_plugin_testkit::FakeHost;
use ilium_plugin_testkit::fuzz::{arbitrary_event, fuzz_events, fuzz_run};
use proptest::prelude::*;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use crate::common::{Spy, spy, spy_reset, with_clean_env, with_env};

fn as_json(events: &[Event]) -> Vec<serde_json::Value> {
    events.iter().map(|event| serde_json::to_value(event).expect("event serializes")).collect()
}

/// A plugin that panics on its first event batch, which the harness reports as a trap.
#[derive(Default)]
pub struct Exploding;

impl ilium_plugin_sdk_core::NativeEntry for Exploding {
    const SIDE: &'static str = "server";
    const EXPORTS: &'static [&'static str] = &["lifecycle", "handler"];

    fn native_activate(
        &mut self,
        _context: ilium_plugin_sdk_core::types::Activation,
    ) -> Result<(), ilium_plugin_sdk_core::types::PluginError> {
        Ok(())
    }

    fn native_deactivate(
        &mut self,
        _reason: ilium_plugin_sdk_core::types::DeactivationReason,
    ) -> Option<Vec<u8>> {
        None
    }

    fn native_on_events(
        &mut self,
        _batch: Vec<Event>,
    ) -> Vec<ilium_plugin_sdk_core::types::Submission> {
        panic!("exploding plugin")
    }
}

// ---- DEV-520 ---------------------------------------------------------------------------------

/// REQ: DEV-520
#[test]
fn dev_520_fuzz_events_returns_exactly_count_events() {
    with_clean_env(|| {
        let host = FakeHost::new().grant("tree.read");
        assert_eq!(host.fuzz_events(1, 10).len(), 10);
    });
}

/// REQ: DEV-520
#[test]
fn dev_520_count_zero_gives_an_empty_batch() {
    with_clean_env(|| {
        let host = FakeHost::new();
        assert!(host.fuzz_events(1, 0).is_empty());
    });
}

/// REQ: DEV-520
#[test]
fn dev_520_the_same_seed_gives_the_same_batch() {
    with_clean_env(|| {
        let host = FakeHost::new().grants(&["tree.read", "agent.read"]);
        let first = as_json(&host.fuzz_events(77, 32));
        let second = as_json(&host.fuzz_events(77, 32));
        assert_eq!(first, second);
    });
}

/// REQ: DEV-520
#[test]
fn dev_520_different_seeds_give_different_batches() {
    with_clean_env(|| {
        let host = FakeHost::new().grants(&["tree.read", "agent.read"]);
        assert_ne!(as_json(&host.fuzz_events(1, 32)), as_json(&host.fuzz_events(2, 32)));
    });
}

/// REQ: DEV-520
#[test]
fn dev_520_two_hosts_with_the_same_grants_agree_on_a_seed() {
    with_clean_env(|| {
        let first = FakeHost::new().grant("tree.read").fuzz_events(5, 16);
        let second = FakeHost::new().grant("tree.read").fuzz_events(5, 16);
        assert_eq!(as_json(&first), as_json(&second));
    });
}

/// REQ: DEV-520
#[test]
fn dev_520_a_batch_survives_a_json_round_trip_so_every_event_is_valid() {
    with_clean_env(|| {
        let host = FakeHost::new().grant("tree.read");
        for event in host.fuzz_events(9, 64) {
            let text = serde_json::to_string(&event).expect("serialize");
            let back: Event = serde_json::from_str(&text).expect("an event the catalogue allows");
            assert_eq!(serde_json::to_string(&back).expect("serialize again"), text);
        }
    });
}

/// REQ: DEV-520
#[test]
fn dev_520_free_function_and_host_method_agree() {
    with_clean_env(|| {
        let host = FakeHost::new();
        assert_eq!(as_json(&fuzz_events(3, 8)), as_json(&host.fuzz_events(3, 8)));
    });
}

proptest! {
    // REQ: DEV-520
    #[test]
    fn dev_520_property_seed_and_count_fix_the_batch(seed in any::<u64>(), count in 0u32..64) {
        let host = FakeHost::new();
        let first = host.fuzz_events(seed, count);
        let second = host.fuzz_events(seed, count);
        prop_assert_eq!(first.len(), count as usize);
        prop_assert_eq!(as_json(&first), as_json(&second));
    }
}

// ---- DEV-521 ---------------------------------------------------------------------------------

/// REQ: DEV-521
#[test]
fn dev_521_fuzz_run_delivers_one_batch_per_round() {
    with_clean_env(|| {
        spy_reset();
        fuzz_run::<Spy>(4, 6).expect("a quiet plugin never fails");
        assert_eq!(spy().batches.len(), 6);
    });
}

/// REQ: DEV-521
#[test]
fn dev_521_zero_rounds_deliver_nothing_and_pass() {
    with_clean_env(|| {
        spy_reset();
        fuzz_run::<Spy>(4, 0).expect("zero rounds");
        assert!(spy().batches.is_empty());
    });
}

/// REQ: DEV-521
#[test]
fn dev_521_a_trap_fails_the_run_with_the_seed() {
    with_clean_env(|| {
        let failure = fuzz_run::<Exploding>(31, 5).expect_err("a panicking plugin traps");
        assert_eq!(failure.seed, 31);
        assert!(failure.round < 5);
        assert!(failure.code.starts_with("trap."), "code was {}", failure.code);
    });
}

/// REQ: DEV-521
#[test]
fn dev_521_the_failure_message_names_seed_and_round() {
    with_clean_env(|| {
        let failure = fuzz_run::<Exploding>(31, 5).expect_err("trap");
        let text = failure.to_string();
        assert!(text.contains("31"), "message was {text}");
        assert!(text.contains(&failure.round.to_string()), "message was {text}");
    });
}

/// REQ: DEV-521
#[test]
fn dev_521_the_same_seed_fails_in_the_same_round() {
    with_clean_env(|| {
        let first = fuzz_run::<Exploding>(31, 5).expect_err("trap");
        let second = fuzz_run::<Exploding>(31, 5).expect_err("trap");
        assert_eq!(first, second);
    });
}

/// REQ: DEV-521
#[test]
fn dev_521_an_expected_trap_does_not_fail_the_run_through_the_host_method() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new();
        host.activate::<Spy>();
        host.expect_trap("trap.deadline");
        host.inject_trap("trap.deadline");
        let result = host.fuzz_run(2, 1);
        assert_eq!(result.as_ref().err().map(|failure| format!("{failure:?}")), None, "an expected trap is not a fuzz failure");
    });
}

/// REQ: DEV-521
#[test]
fn dev_521_a_trap_that_nobody_expected_fails_the_host_method() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new();
        host.activate::<Spy>();
        host.inject_trap("trap.memory");
        let failure = host.fuzz_run(2, 3).expect_err("unexpected trap");
        assert_eq!(failure.seed, 2);
        assert_eq!(failure.code, "trap.memory");
    });
}

// ---- DEV-522 ---------------------------------------------------------------------------------

/// REQ: DEV-522
#[test]
fn dev_522_arbitrary_event_is_a_function_of_the_rng_state() {
    let mut first = ChaCha8Rng::seed_from_u64(11);
    let mut second = ChaCha8Rng::seed_from_u64(11);
    for _ in 0..16 {
        let one = serde_json::to_value(arbitrary_event(&mut first)).expect("serialize");
        let two = serde_json::to_value(arbitrary_event(&mut second)).expect("serialize");
        assert_eq!(one, two);
    }
}

/// REQ: DEV-522
#[test]
fn dev_522_arbitrary_events_are_valid_and_round_trip() {
    let mut rng = ChaCha8Rng::seed_from_u64(12);
    for _ in 0..64 {
        let event = arbitrary_event(&mut rng);
        let text = serde_json::to_string(&event).expect("serialize");
        let back: Event = serde_json::from_str(&text).expect("valid event");
        assert_eq!(serde_json::to_string(&back).expect("serialize"), text);
    }
}

/// REQ: DEV-522
#[test]
fn dev_522_a_stream_of_events_is_not_constant() {
    let mut rng = ChaCha8Rng::seed_from_u64(13);
    let events: Vec<String> = (0..32)
        .map(|_| serde_json::to_string(&arbitrary_event(&mut rng)).expect("serialize"))
        .collect();
    assert!(events.iter().any(|text| *text != events[0]));
}

proptest! {
    // REQ: DEV-522
    #[test]
    fn dev_522_property_every_arbitrary_event_round_trips(seed in any::<u64>()) {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let event = arbitrary_event(&mut rng);
        let text = serde_json::to_string(&event).expect("serialize");
        let back: Event = serde_json::from_str(&text).expect("valid event");
        prop_assert_eq!(serde_json::to_string(&back).expect("serialize"), text);
    }
}

// ---- DEV-523 ---------------------------------------------------------------------------------

/// REQ: DEV-523
#[test]
fn dev_523_the_environment_seed_is_the_host_seed() {
    with_env(&[("ILIUM_PLUGIN_TEST_SEED", "42")], || {
        assert_eq!(FakeHost::new().current_seed(), 42);
    });
}

/// REQ: DEV-523
#[test]
fn dev_523_a_run_with_the_environment_seed_equals_a_run_with_the_explicit_seed() {
    let from_env = with_env(&[("ILIUM_PLUGIN_TEST_SEED", "42")], || {
        let host = FakeHost::new();
        as_json(&host.fuzz_events(host.current_seed(), 20))
    });
    let explicit = with_clean_env(|| as_json(&FakeHost::new().seed(42).fuzz_events(42, 20)));
    assert_eq!(from_env, explicit);
}

/// REQ: DEV-523
#[test]
fn dev_523_another_environment_seed_gives_another_batch() {
    let first = with_env(&[("ILIUM_PLUGIN_TEST_SEED", "42")], || {
        let host = FakeHost::new();
        as_json(&host.fuzz_events(host.current_seed(), 20))
    });
    let second = with_env(&[("ILIUM_PLUGIN_TEST_SEED", "43")], || {
        let host = FakeHost::new();
        as_json(&host.fuzz_events(host.current_seed(), 20))
    });
    assert_ne!(first, second);
}

/// REQ: DEV-523
#[test]
fn dev_523_two_runs_with_the_environment_seed_fail_the_same_way() {
    let run = || {
        with_env(&[("ILIUM_PLUGIN_TEST_SEED", "9")], || {
            let seed = FakeHost::new().current_seed();
            fuzz_run::<Exploding>(seed, 4).expect_err("trap")
        })
    };
    assert_eq!(run(), run());
}
