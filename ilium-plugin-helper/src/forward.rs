//! Imports forwarded from the helper to the host (reference-host-architecture.md).

use crate::HelperError;

/// Sends one import call to the host and waits for its answer.
pub fn forward_import(
    interface: &str,
    function: &str,
    payload: &[u8],
) -> Result<Vec<u8>, HelperError> {
    unimplemented!("helper::forward::forward_import")
}
