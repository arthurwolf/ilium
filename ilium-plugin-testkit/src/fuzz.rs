//! Property-test and fuzz hooks (reference-test-harness.md "Property tests and fuzz hooks").

use ilium_plugin_sdk_core::types::Event;
use rand_chacha::ChaCha8Rng;

/// A failed fuzz run: the seed and round reproduce it (DEV-521).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("fuzz failure: seed {seed}, round {round}, code {code}")]
pub struct FuzzFailure {
    /// Seed of the run.
    pub seed: u64,
    /// Round that failed.
    pub round: u32,
    /// Trap code or harness code.
    pub code: String,
}

/// One valid random event drawn from the catalogue (DEV-522).
pub fn arbitrary_event(_rng: &mut ChaCha8Rng) -> Event {
    unimplemented!("DEV-522")
}

/// A valid random event batch; the same seed gives the same batch (DEV-520).
pub fn fuzz_events(_seed: u64, _count: u32) -> Vec<Event> {
    unimplemented!("DEV-520")
}

/// Delivers `rounds` random batches to a fresh host of plugin type `P` (DEV-521).
pub fn fuzz_run<P: ilium_plugin_sdk_core::NativeEntry + Default>(
    _seed: u64,
    _rounds: u32,
) -> Result<(), FuzzFailure> {
    unimplemented!("DEV-521")
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use rand::SeedableRng;

    /// Plugin that never traps; the quiet-run cases use it.
    #[derive(Default)]
    struct QuietPlugin;

    impl ilium_plugin_sdk_core::NativeEntry for QuietPlugin {
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
            Vec::new()
        }
    }

    /// Plugin whose event handler always panics; the failure cases use it.
    #[derive(Default)]
    struct TrappingPlugin;

    impl ilium_plugin_sdk_core::NativeEntry for TrappingPlugin {
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
            panic!("trapping plugin")
        }
    }

    /// REQ: DEV-520
    #[test]
    fn dev_520_fuzz_events_returns_exactly_count_events() {
        assert_eq!(fuzz_events(1, 10).len(), 10);
    }

    /// REQ: DEV-520
    #[test]
    fn dev_520_same_seed_gives_the_same_batch() {
        let first = serde_json::to_value(fuzz_events(77, 16)).expect("batch serializes");
        let second = serde_json::to_value(fuzz_events(77, 16)).expect("batch serializes");
        assert_eq!(first, second);
    }

    /// REQ: DEV-520
    #[test]
    fn dev_520_zero_count_gives_an_empty_batch() {
        assert!(fuzz_events(1, 0).is_empty());
    }

    /// REQ: DEV-522
    #[test]
    fn dev_522_arbitrary_event_is_a_valid_event_that_round_trips() {
        let mut rng = ChaCha8Rng::seed_from_u64(9);
        for _ in 0..32 {
            let event = arbitrary_event(&mut rng);
            let value = serde_json::to_value(&event).expect("event serializes");
            let back: Event = serde_json::from_value(value).expect("event deserializes");
            assert_eq!(back, event);
        }
    }

    /// REQ: DEV-521
    #[test]
    fn dev_521_fuzz_run_passes_when_the_plugin_never_traps() {
        fuzz_run::<QuietPlugin>(4, 6).expect("a quiet plugin never fails");
    }

    /// REQ: DEV-521
    #[test]
    fn dev_521_zero_rounds_pass_without_delivery() {
        fuzz_run::<QuietPlugin>(4, 0).expect("zero rounds never fail");
    }

    /// REQ: DEV-521
    #[test]
    fn dev_521_a_trap_fails_the_run_with_seed_and_round_in_the_message() {
        let failure = fuzz_run::<TrappingPlugin>(31, 5).expect_err("a trapping plugin fails");
        assert_eq!(failure.seed, 31);
        assert!(failure.round < 5, "round was {}", failure.round);
        let text = failure.to_string();
        assert!(text.contains("31"), "message was {text}");
        assert!(
            text.contains(&failure.round.to_string()),
            "message was {text}"
        );
    }

    /// REQ: DEV-521
    #[test]
    fn dev_521_the_same_seed_reproduces_the_same_failure() {
        let first = fuzz_run::<TrappingPlugin>(31, 5).expect_err("trap");
        let second = fuzz_run::<TrappingPlugin>(31, 5).expect_err("trap");
        assert_eq!(first, second);
    }
}
