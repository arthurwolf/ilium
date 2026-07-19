//! Errors owned by the voice integration boundary.

#[derive(Debug, thiserror::Error)]
pub enum VoiceError {
    #[error("invalid voice configuration: {0}")]
    InvalidConfiguration(String),
    #[error("no {direction} audio device is available")]
    MissingAudioDevice { direction: &'static str },
    #[error("audio device {name:?} was not found for {direction}")]
    NamedAudioDeviceNotFound {
        direction: &'static str,
        name: String,
    },
    #[error("failed to enumerate {direction} audio devices: {source}")]
    EnumerateAudioDevices {
        direction: &'static str,
        source: cpal::Error,
    },
    #[error("failed to read {direction} audio configuration: {source}")]
    AudioConfiguration {
        direction: &'static str,
        source: cpal::Error,
    },
    #[error("failed to build {direction} audio stream: {source}")]
    BuildAudioStream {
        direction: &'static str,
        source: cpal::Error,
    },
    #[error("unsupported {direction} audio sample format: {format}")]
    UnsupportedSampleFormat {
        direction: &'static str,
        format: cpal::SampleFormat,
    },
    #[error("failed to start {direction} audio stream: {source}")]
    StartAudioStream {
        direction: &'static str,
        source: cpal::Error,
    },
    #[error("failed to connect to the OpenAI Realtime API: {0}")]
    Connect(String),
    #[error("timed out while connecting to the OpenAI Realtime API")]
    ConnectionTimeout,
    #[error("the OpenAI API key could not be used as an authorization header")]
    InvalidApiKeyHeader,
    #[error("Realtime transport failed: {0}")]
    Transport(#[source] tokio_tungstenite::tungstenite::Error),
    #[error("Realtime protocol payload was invalid: {0}")]
    Protocol(#[from] serde_json::Error),
    #[error("OpenAI rejected the Realtime session configuration: {0}")]
    SessionConfigurationRejected(String),
    #[error("OpenAI did not acknowledge the Realtime session configuration in time")]
    SessionConfigurationTimeout,
    #[error("the voice service command channel closed")]
    CommandChannelClosed,
    #[error("the OpenAI Realtime session ended unexpectedly")]
    SessionEnded,
}
