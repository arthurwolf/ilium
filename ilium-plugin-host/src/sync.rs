//! The concurrency shim: `std` types normally, `loom` types under the cargo feature `loom`, so the
//! concurrent core can be model-checked (test plan section 2.7). A cargo feature, not
//! `--cfg loom`, because ni-build rejects config and RUSTFLAGS overrides.

#[cfg(feature = "loom")]
pub use loom::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "loom")]
pub use loom::sync::{Arc, Mutex};

#[cfg(not(feature = "loom"))]
pub use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(not(feature = "loom"))]
pub use std::sync::{Arc, Mutex};

/// Constant-time byte comparison for secrets (reference-threat-model.md SEC-376).
pub fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    use subtle::ConstantTimeEq;
    // Token lengths are public protocol metadata. For equal lengths, subtle
    // prevents content-dependent early returns and optimizer removal.
    bool::from(left.ct_eq(right))
}
