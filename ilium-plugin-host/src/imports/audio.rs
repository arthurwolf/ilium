//! Host implementation of `ilium:host/audio` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostAudio` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::audio::{AudioDevice, HostAudio};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::RequestId;

/// Host side of the `audio` interface.
#[derive(Debug)]
pub struct AudioImport;

impl HostAudio for AudioImport {
    fn devices(&mut self) -> Vec<AudioDevice> {
        unimplemented!("host::imports::audio::devices")
    }

    fn open_capture(
        &mut self,
        _device: Option<String>,
        _sample_rate: u32,
        _channels: u8,
    ) -> Result<u64, PluginError> {
        unimplemented!("host::imports::audio::open_capture")
    }

    fn open_loopback(&mut self, _sample_rate: u32, _channels: u8) -> Result<u64, PluginError> {
        unimplemented!("host::imports::audio::open_loopback")
    }

    fn close_capture(&mut self, _capture: u64) {
        unimplemented!("host::imports::audio::close_capture")
    }

    fn play(
        &mut self,
        _samples: Vec<i16>,
        _sample_rate: u32,
        _channels: u8,
    ) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::audio::play")
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
        assert_ne!(capture, 0, "a loopback capture id must identify the opened stream");
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
