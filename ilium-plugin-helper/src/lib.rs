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

#[cfg(test)]
mod spec_tests {
    use super::*;
    use crate::sandbox::SandboxPolicy;

    fn helper_config() -> HelperConfig {
        HelperConfig {
            epoch_tick_ms: 10,
            apply_sandbox: false,
        }
    }

    /// A writer whose host channel is already closed; every write fails.
    struct ClosedChannelWriter;

    impl Write for ClosedChannelWriter {
        fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "host channel closed",
            ))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// REQ: reference-host-helper.md "Messages" (Shutdown: the helper exits with code 0) and the
    /// `serve` doc comment (serves until the host closes the channel).
    #[test]
    fn serve_returns_ok_when_host_closes_channel_without_requests() {
        let mut output: Vec<u8> = Vec::new();
        let result = serve(std::io::empty(), &mut output, helper_config());
        assert!(result.is_ok(), "clean channel close must end serve with Ok, got {result:?}");
    }

    /// REQ: reference-host-helper.md "Starting a helper" step 4 (the helper sends Hello first,
    /// before it waits for any Instantiate or CallExport).
    #[test]
    fn serve_writes_hello_before_it_reads_requests() {
        let mut output: Vec<u8> = Vec::new();
        serve(std::io::empty(), &mut output, helper_config())
            .expect("serve must return after the host closes the channel");
        assert!(!output.is_empty(), "the helper must announce itself with Hello");
    }

    /// REQ: HST-155 (a message with a bad length ends the helper as a failure). The length
    /// prefix 0xFFFF_FFFF is far above the 4 MiB message cap.
    #[test]
    fn serve_rejects_frame_length_above_message_cap() {
        let input = [0xFFu8, 0xFF, 0xFF, 0xFF];
        let mut output: Vec<u8> = Vec::new();
        let result = serve(&input[..], &mut output, helper_config());
        assert!(
            matches!(result, Err(HelperError::Protocol(_))),
            "oversized length must be a protocol violation, got {result:?}"
        );
    }

    /// REQ: HST-155 (a truncated message ends the helper and counts as a failure). Two bytes
    /// cannot hold a complete length prefix, so the helper must not wait or return Ok.
    #[test]
    fn serve_fails_on_truncated_frame() {
        let input = [0x10u8, 0x00];
        let mut output: Vec<u8> = Vec::new();
        let result = serve(&input[..], &mut output, helper_config());
        assert!(result.is_err(), "a truncated frame must end serve with an error");
    }

    /// REQ: HelperError::Channel ("Reading or writing the host channel failed") with
    /// reference-host-helper.md "Starting a helper" step 4 (the Hello write is the first channel write).
    #[test]
    fn serve_reports_channel_failure_when_hello_cannot_be_written() {
        let result = serve(std::io::empty(), ClosedChannelWriter, helper_config());
        assert!(
            matches!(result, Err(HelperError::Channel(_))),
            "a failed channel write must surface as Channel, got {result:?}"
        );
    }

    /// REQ: SEC-022 (the helper's network namespace has no interface) and SEC-032 (children of
    /// T2 and T3 plugins have no network). A default policy must grant no network and no paths.
    #[test]
    fn default_sandbox_policy_grants_no_network_and_no_paths() {
        let policy = SandboxPolicy::default();
        assert!(!policy.allow_network, "the default policy must deny network access");
        assert!(policy.read_paths.is_empty(), "the default policy must grant no read paths");
        assert!(policy.write_paths.is_empty(), "the default policy must grant no write paths");
    }
}
