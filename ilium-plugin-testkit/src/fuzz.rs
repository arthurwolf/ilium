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
pub fn fuzz_run<P: ilium_plugin_sdk_core::NativeEntry + Default>(_seed: u64, _rounds: u32) -> Result<(), FuzzFailure> {
    unimplemented!("DEV-521")
}
