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
pub fn constant_time_eq(_left: &[u8], _right: &[u8]) -> bool {
    unimplemented!("host::sync::constant_time_eq")
}
