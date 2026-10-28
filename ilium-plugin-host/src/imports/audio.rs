//! Host implementation of `ilium:host/audio` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostAudio` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::audio::{AudioDevice, HostAudio};
use std::cell::RefCell;
use std::collections::HashSet;

use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::RequestId;

/// Lowest sample rate any audio call accepts, in hertz.
const MIN_SAMPLE_RATE: u32 = 8_000;
/// Highest sample rate any audio call accepts, in hertz.
const MAX_SAMPLE_RATE: u32 = 192_000;
/// Most capture streams a plugin may hold open (default quota).
const MAX_OPEN_STREAMS: usize = 2;
/// Longest playback one `play` call accepts, in seconds.
const MAX_PLAY_SECONDS: u64 = 30;

/// Open capture streams and the id counters.
#[derive(Debug)]
struct AudioState {
    next_capture: u64,
    next_request: u64,
    open: HashSet<u64>,
}

thread_local! {
    static STATE: RefCell<AudioState> = RefCell::new(AudioState { next_capture: 1, next_request: 1, open: HashSet::new() });
}

/// Builds a refusal with the given code and a plain-text message.
fn refuse(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_owned(),
        retry_after_ms: None,
    }
}

/// Checks sample rate and channel count shared by every audio call.
fn validate_format(sample_rate: u32, channels: u8) -> Result<(), PluginError> {
    if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&sample_rate) {
        return Err(refuse(
            ErrorCode::InvalidArgument,
            "sample rate must be 8000 to 192000 Hz",
        ));
    }
    match channels {
        0 => Err(refuse(ErrorCode::InvalidArgument, "channels must be 1 or 2")),
        1 | 2 => Ok(()),
        _ => Err(refuse(ErrorCode::Unsupported, "only mono and stereo are supported")),
    }
}

/// Registers a new open stream, enforcing the stream quota.
fn open_stream() -> Result<u64, PluginError> {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.open.len() >= MAX_OPEN_STREAMS {
            return Err(refuse(ErrorCode::QuotaExceeded, "too many open capture streams"));
        }
        let id = state.next_capture;
        state.next_capture += 1;
        state.open.insert(id);
        Ok(id)
    })
}

/// Host side of the `audio` interface.
#[derive(Debug)]
pub struct AudioImport;

impl HostAudio for AudioImport {
    fn devices(&mut self) -> Vec<AudioDevice> {
        // Listing needs the `audio` grant; without a grant or a backend the list is empty.
        Vec::new()
    }

    fn open_capture(
        &mut self,
        device: Option<String>,
        sample_rate: u32,
        channels: u8,
    ) -> Result<u64, PluginError> {
        validate_format(sample_rate, channels)?;
        // `devices()` lists nothing here, so any named device is not in the list.
        if device.is_some() {
            return Err(refuse(ErrorCode::NotFound, "device is not in devices()"));
        }
        open_stream()
    }

    fn open_loopback(&mut self, sample_rate: u32, channels: u8) -> Result<u64, PluginError> {
        validate_format(sample_rate, channels)?;
        open_stream()
    }

    fn close_capture(&mut self, capture: u64) {
        STATE.with(|state| {
            state.borrow_mut().open.remove(&capture);
        });
    }

    fn play(
        &mut self,
        samples: Vec<i16>,
        sample_rate: u32,
        channels: u8,
    ) -> Result<RequestId, PluginError> {
        validate_format(sample_rate, channels)?;
        let sample_count = samples.len() as u64;
        if sample_count % u64::from(channels) != 0 {
            return Err(refuse(
                ErrorCode::InvalidArgument,
                "sample count must be a multiple of the channel count",
            ));
        }
        let frames = sample_count / u64::from(channels);
        if frames > MAX_PLAY_SECONDS * u64::from(sample_rate) {
            return Err(refuse(ErrorCode::InvalidArgument, "playback is longer than 30 s"));
        }
        let id = STATE.with(|state| {
            let mut state = state.borrow_mut();
            let id = state.next_request;
            state.next_request += 1;
            id
        });
        Ok(RequestId(id))
    }
}

#[cfg(test)]
mod tests {
    use super::AudioImport;
    use ilium_plugin_api::generated::host::audio::HostAudio;
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};

    /// Returns the error code of a failed call; panics when the call succeeded.
    fn error_code_of<T>(result: Result<T, PluginError>) -> ErrorCode {
        match result {
            Ok(_) => panic!("expected an error result, got Ok"),
            Err(error) => error.code,
        }
    }

    /// Panics with the error code when the call failed; passes the success value through.
    fn expect_ok<T>(result: Result<T, PluginError>) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("expected Ok, got error {:?}", error.code),
        }
    }

    /// Source: api-audio.md "Grants" and capability-audio.md "Devices" (no requirement ID).
    #[test]
    fn devices_without_audio_grant_returns_empty_list() {
        let mut audio = AudioImport;
        assert!(audio.devices().is_empty());
    }

    /// Source: api-audio.md "Surface" (open_capture returns a capture id) and "Edge cases" table.
    #[test]
    fn open_capture_default_device_mono_16khz_returns_capture_id() {
        let mut audio = AudioImport;
        let capture = expect_ok(audio.open_capture(None, 16_000, 1));
        assert_ne!(capture, 0, "a capture id must identify the opened stream");
    }

    /// Source: api-audio.md "Sample format" (stereo is two channels) and "Edge cases" table.
    #[test]
    fn open_capture_default_device_stereo_48khz_is_accepted() {
        let mut audio = AudioImport;
        expect_ok(audio.open_capture(None, 48_000, 2));
    }

    /// Source: api-audio.md "Sample format": any rate from 8,000 to 192,000 Hz is accepted (lower boundary).
    #[test]
    fn open_capture_minimum_rate_8000_hz_is_accepted() {
        let mut audio = AudioImport;
        expect_ok(audio.open_capture(None, 8_000, 1));
    }

    /// Source: api-audio.md "Sample format": any rate from 8,000 to 192,000 Hz is accepted (upper boundary).
    #[test]
    fn open_capture_maximum_rate_192000_hz_is_accepted() {
        let mut audio = AudioImport;
        expect_ok(audio.open_capture(None, 192_000, 2));
    }

    /// Source: api-audio.md "Edge cases" table: rate outside 8,000 to 192,000 Hz gives invalid-argument.
    #[test]
    fn open_capture_rate_one_below_minimum_is_invalid_argument() {
        let mut audio = AudioImport;
        assert_eq!(
            error_code_of(audio.open_capture(None, 7_999, 1)),
            ErrorCode::InvalidArgument
        );
    }

    /// Source: api-audio.md "Edge cases" table: rate outside 8,000 to 192,000 Hz gives invalid-argument.
    #[test]
    fn open_capture_rate_one_above_maximum_is_invalid_argument() {
        let mut audio = AudioImport;
        assert_eq!(
            error_code_of(audio.open_capture(None, 192_001, 1)),
            ErrorCode::InvalidArgument
        );
    }

    /// Source: api-audio.md "Edge cases" table: zero channels gives invalid-argument.
    #[test]
    fn open_capture_zero_channels_is_invalid_argument() {
        let mut audio = AudioImport;
        assert_eq!(
            error_code_of(audio.open_capture(None, 16_000, 0)),
            ErrorCode::InvalidArgument
        );
    }

    /// Source: api-audio.md "Edge cases" table: more than two channels gives unsupported.
    #[test]
    fn open_capture_three_channels_is_unsupported() {
        let mut audio = AudioImport;
        assert_eq!(
            error_code_of(audio.open_capture(None, 16_000, 3)),
            ErrorCode::Unsupported
        );
    }

    /// Source: api-audio.md "Edge cases" table: a device id not in devices() gives not-found.
    #[test]
    fn open_capture_unknown_device_id_is_not_found() {
        let mut audio = AudioImport;
        assert_eq!(
            error_code_of(audio.open_capture(Some("no-such-device".to_string()), 16_000, 1)),
            ErrorCode::NotFound
        );
    }

    /// Source: api-audio.md "Edge cases" table: too many open streams gives quota-exceeded (default 2).
    #[test]
    fn open_capture_third_concurrent_stream_is_quota_exceeded() {
        let mut audio = AudioImport;
        expect_ok(audio.open_capture(None, 16_000, 1));
        expect_ok(audio.open_capture(None, 16_000, 1));
        assert_eq!(
            error_code_of(audio.open_capture(None, 16_000, 1)),
            ErrorCode::QuotaExceeded
        );
    }

    /// Source: api-audio.md "Surface" (open_loopback returns a capture id).
    #[test]
    fn open_loopback_valid_rate_and_channels_returns_capture_id() {
        let mut audio = AudioImport;
        let capture = expect_ok(audio.open_loopback(48_000, 2));
        assert_ne!(
            capture, 0,
            "a loopback capture id must identify the opened stream"
        );
    }

    /// Source: api-audio.md "Edge cases" table: rate outside 8,000 to 192,000 Hz gives invalid-argument (lower boundary).
    #[test]
    fn open_loopback_rate_one_below_minimum_is_invalid_argument() {
        let mut audio = AudioImport;
        assert_eq!(
            error_code_of(audio.open_loopback(7_999, 1)),
            ErrorCode::InvalidArgument
        );
    }

    /// Source: api-audio.md "Edge cases" table: rate outside 8,000 to 192,000 Hz gives invalid-argument (upper boundary).
    #[test]
    fn open_loopback_rate_one_above_maximum_is_invalid_argument() {
        let mut audio = AudioImport;
        assert_eq!(
            error_code_of(audio.open_loopback(192_001, 1)),
            ErrorCode::InvalidArgument
        );
    }

    /// Source: api-audio.md "Edge cases" table: zero channels gives invalid-argument.
    #[test]
    fn open_loopback_zero_channels_is_invalid_argument() {
        let mut audio = AudioImport;
        assert_eq!(
            error_code_of(audio.open_loopback(48_000, 0)),
            ErrorCode::InvalidArgument
        );
    }

    /// Source: api-audio.md "Edge cases" table: more than two channels gives unsupported.
    #[test]
    fn open_loopback_three_channels_is_unsupported() {
        let mut audio = AudioImport;
        assert_eq!(
            error_code_of(audio.open_loopback(48_000, 3)),
            ErrorCode::Unsupported
        );
    }

    /// Source: api-audio.md "Edge cases" table: close-capture of an unknown or closed id does nothing.
    #[test]
    fn close_capture_unknown_id_does_nothing() {
        let mut audio = AudioImport;
        audio.close_capture(0);
        audio.close_capture(u64::MAX);
    }

    /// Source: api-audio.md "Surface" (close-capture releases the device) and "Edge cases" table (stream quota).
    #[test]
    fn close_capture_releases_stream_slot_for_a_new_capture() {
        let mut audio = AudioImport;
        let first = expect_ok(audio.open_capture(None, 16_000, 1));
        expect_ok(audio.open_capture(None, 16_000, 1));
        audio.close_capture(first);
        expect_ok(audio.open_capture(None, 16_000, 1));
    }

    /// Source: api-audio.md "Surface" (play returns on request id) and "Sample format" (interleaved s16, stereo).
    #[test]
    fn play_stereo_interleaved_samples_returns_request_id() {
        let mut audio = AudioImport;
        expect_ok(audio.play(vec![0i16, 0, 1_000, -1_000], 16_000, 2));
    }

    /// Source: api-audio.md "Sample format": one event (20 ms) at 16 kHz mono holds 320 samples.
    #[test]
    fn play_one_chunk_mono_samples_returns_request_id() {
        let mut audio = AudioImport;
        expect_ok(audio.play(vec![0i16; 320], 16_000, 1));
    }

    /// Source: api-audio.md "Edge cases" table: empty input is a sample count of 0, a multiple of any channel count.
    #[test]
    fn play_empty_samples_is_accepted() {
        let mut audio = AudioImport;
        expect_ok(audio.play(Vec::new(), 16_000, 1));
    }

    /// Source: api-audio.md "Edge cases" table: sample count not a multiple of channels gives invalid-argument.
    #[test]
    fn play_sample_count_not_multiple_of_channels_is_invalid_argument() {
        let mut audio = AudioImport;
        assert_eq!(
            error_code_of(audio.play(vec![0i16; 3], 16_000, 2)),
            ErrorCode::InvalidArgument
        );
    }

    /// Source: api-audio.md "Edge cases" table: playback up to 30 s is accepted (boundary: 480,000 samples at 16 kHz mono).
    #[test]
    fn play_exactly_thirty_seconds_is_accepted() {
        let mut audio = AudioImport;
        expect_ok(audio.play(vec![0i16; 480_000], 16_000, 1));
    }

    /// Source: api-audio.md "Edge cases" table: longer than 30 s gives invalid-argument (480,001 samples at 16 kHz mono).
    #[test]
    fn play_just_over_thirty_seconds_is_invalid_argument() {
        let mut audio = AudioImport;
        assert_eq!(
            error_code_of(audio.play(vec![0i16; 480_001], 16_000, 1)),
            ErrorCode::InvalidArgument
        );
    }

    /// Source: api-audio.md "Sample format": any rate from 8,000 to 192,000 Hz is accepted; out-of-range rate for play is inferred invalid-argument.
    #[test]
    fn play_rate_below_minimum_is_invalid_argument() {
        let mut audio = AudioImport;
        assert_eq!(
            error_code_of(audio.play(vec![0i16; 320], 7_999, 1)),
            ErrorCode::InvalidArgument
        );
    }
}
