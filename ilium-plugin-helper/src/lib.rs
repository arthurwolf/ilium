//! The sandbox helper of the plugin platform (reference-host-architecture.md "Helper process").
//!
//! T2 and T3 plugins run in this process, never in the server or client. The host spawns it
//! through `ilium-platform`, speaks the framed [`protocol`] with it, and the helper forwards
//! imports that need the application back to the host ([`forward`]).
//!
//! Skeleton state: data types are complete, behaviour is `unimplemented!("helper::<path>")`.

use std::io::{Read, Write};

pub mod forward;
pub mod protocol;
pub mod sandbox;

/// Failures of the helper (reference-host-architecture.md).
#[derive(Debug, thiserror::Error)]
pub enum HelperError {
    /// Reading or writing the host channel failed.
    #[error("helper channel failed: {0}")]
    Channel(String),
    /// A frame could not be decoded.
    #[error("helper protocol violation: {0}")]
    Protocol(String),
    /// The OS sandbox could not be established.
    #[error("helper sandbox failed: {0}")]
    Sandbox(String),
}

/// Settings the host passes to the helper at spawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperConfig {
    /// Epoch tick in milliseconds, as for the in-process engine.
    pub epoch_tick_ms: u64,
    /// Whether to apply the OS sandbox before serving.
    pub apply_sandbox: bool,
}

/// Serves requests from `input` and answers on `output` until the host closes the channel.
pub fn serve(
    _input: impl Read,
    _output: impl Write,
    _config: HelperConfig,
) -> Result<(), HelperError> {
    unimplemented!("helper::serve")
}
