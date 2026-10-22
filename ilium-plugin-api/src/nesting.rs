//! Nesting-depth bound of decoded WIT values (reference-errors-and-limits.md "Value nesting
//! depth limits"; ABI-061, ABI-062). Skel-api addition: the manual states the bound and the
//! refusal code but no function, so the tests have a pure surface to call.

use crate::error::Code;

/// Deepest nesting a decoded WIT value may have, in levels, counting nested JSON strings and
/// recursive widget trees (ABI-061).
pub const MAX_NESTING_DEPTH: u32 = 32;

/// ABI-061, ABI-062: `Ok` for a value nested at most [`MAX_NESTING_DEPTH`] levels; a deeper value
/// is refused with the stable code `abi.depth-exceeded` and neither traps nor panics.
pub fn check_nesting_depth(_depth: u32) -> Result<(), Code> {
    unimplemented!("api::nesting::check_nesting_depth")
}
