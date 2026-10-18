//! Progress-lane testing hooks on `FakeHost` (startup phase publishing, durable-write faults).
//!
//! Plugin-specific scaffolding kept out of the shared harness type. Skeleton bodies.

use ilium_plugin_sdk::testing::FakeHost;

/// One startup phase handle returned by `begin_startup_phase`; the skeleton passes it around as an identity.
pub type StartupPhase = u64;

/// Progress-lane hooks. Implemented for `FakeHost` with skeleton bodies.
pub trait ProgressHostExt {
    /// Opens a new startup phase; older phases are retired (progress "Startup marker").
    fn begin_startup_phase(&mut self) -> StartupPhase {
        unimplemented!("progress.begin-startup-phase")
    }
    /// Publishes a startup marker for `phase`: `(title, step, done, total)`.
    fn startup_publish(
        &mut self,
        _phase: StartupPhase,
        _title: &str,
        _step: &str,
        _done: u32,
        _total: u32,
    ) {
        unimplemented!("progress.startup-publish")
    }
    /// Finishes `phase`, clearing its marker.
    fn startup_finish(&mut self, _phase: StartupPhase) {
        unimplemented!("progress.startup-finish")
    }
    /// Cancels the startup sequence.
    fn cancel_startup(&mut self) {
        unimplemented!("progress.cancel-startup")
    }
    /// The current startup marker: `(title, step, done, total)`.
    fn startup_marker(&self) -> Option<(String, String, u32, u32)> {
        unimplemented!("progress.startup-marker")
    }
    /// Whether the startup callback is still admitted.
    fn startup_callback_admitted(&self) -> bool {
        unimplemented!("progress.startup-callback-admitted")
    }
    /// Blocks the next native startup write until `release_startup_write`.
    fn block_next_startup_write(&mut self) {
        unimplemented!("progress.block-next-startup-write")
    }
    /// Releases a blocked native startup write.
    fn release_startup_write(&mut self) {
        unimplemented!("progress.release-startup-write")
    }
    /// Fails the next durable write.
    fn fail_next_durable_write(&mut self) {
        unimplemented!("progress.fail-next-durable-write")
    }
    /// Sets the footer split orientation (`"vertical"` or `"horizontal"`).
    fn set_split_orientation(&mut self, _orientation: &str) {
        unimplemented!("progress.set-split-orientation")
    }
}

impl ProgressHostExt for FakeHost {}
