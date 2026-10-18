//! Probe imports and budget probes used by `ilium-plugin-conformance` (reference-conformance-suite.md).

/// Burns a given amount of fuel-equivalent work in the guest (the `fuel-burn` probe import).
pub fn fuel_burn(_units: u64) {
    unimplemented!("host::conformance_hooks::fuel_burn")
}
