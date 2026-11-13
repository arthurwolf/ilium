//! Errors of the pure voice modules (the transport and audio-device variants of the kernel `VoiceError` are
//! host-side concerns and have no counterpart here).

use thiserror::Error;

#[derive(Debug, Error)]
pub enum VoiceError {
    #[error("invalid voice configuration: {0}")]
    InvalidConfiguration(String),
    #[error("failed to prepare {direction} audio device catalogue: {reason}")]
    AudioDeviceCatalogue {
        direction: &'static str,
        reason: &'static str,
    },
    #[error("Realtime protocol payload was invalid: {0}")]
    Protocol(#[from] serde_json::Error),
    #[error("Realtime provider event has {bytes} bytes; the limit is {limit}")]
    ProviderEventTooLarge { bytes: usize, limit: usize },
    #[error("failed to connect to the OpenAI Realtime API: {0}")]
    Connect(String),
}
