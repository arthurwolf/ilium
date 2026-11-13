//! Probe imports and budget probes used by `ilium-plugin-conformance` (reference-conformance-suite.md).

/// Busy-waits for the given number of wall-clock milliseconds in the guest (the `busy-wait` probe import; deadlines are epoch-based and fuel is off).
pub fn busy_wait_ms(_millis: u64) {
    unimplemented!("host::conformance_hooks::busy_wait_ms")
}
