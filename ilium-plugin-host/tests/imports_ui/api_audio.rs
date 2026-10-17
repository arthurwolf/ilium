//! `ilium:host/audio` (api-audio.md): devices, capture, loopback, play.

use ilium_plugin_api::generated::host::audio::{AudioDevice, HostAudio};
use ilium_plugin_api::ErrorCode;

use crate::common::*;

const GRANTS: &[&str] = &[
    "audio.devices",
    "audio.capture",
    "audio.loopback",
    "audio.playback",
];

fn device(id: &str, is_input: bool) -> AudioDevice {
    AudioDevice {
        id: id.to_owned(),
        name: format!("name-{id}"),
        is_input,
        is_default: false,
    }
}

/// REQ: WIT:ilium:host/audio.devices
#[test]
fn wit_audio_devices_lists_the_scripted_devices() {
    let mut rig = rig(GRANTS);
    rig.kernel
        .set_audio_devices(vec![device("mic", true), device("out", false)]);
    let devices = rig.host.audio().devices();
    assert_eq!(devices.len(), 2);
    assert_eq!(devices[0].id, "mic");
    assert!(devices[0].is_input && !devices[1].is_input);
}

/// REQ: WIT:ilium:host/audio.devices
#[test]
fn wit_audio_devices_without_the_grant_is_empty() {
    let mut rig = declined_rig(&["audio.devices"]);
    rig.kernel.set_audio_devices(vec![device("mic", true)]);
    assert!(rig.host.audio().devices().is_empty());
}

/// REQ: WIT:ilium:host/audio.open-capture
#[test]
fn wit_audio_open_capture_returns_a_capture_id() {
    let mut rig = rig(GRANTS);
    rig.kernel.set_audio_devices(vec![device("mic", true)]);
    let id = rig
        .host
        .audio()
        .open_capture(Some("mic".to_owned()), 16_000, 1)
        .expect("opens");
    assert!(rig.kernel.open_captures().contains(&id));
}

/// REQ: WIT:ilium:host/audio.open-capture
#[test]
fn wit_audio_open_capture_default_device_with_none() {
    let mut rig = rig(GRANTS);
    rig.kernel.set_audio_devices(vec![device("mic", true)]);
    assert!(rig.host.audio().open_capture(None, 8_000, 2).is_ok());
}

/// REQ: WIT:ilium:host/audio.open-capture
#[test]
fn wit_audio_open_capture_zero_channels_invalid_argument() {
    let mut rig = rig(GRANTS);
    assert_eq!(
        code_of(rig.host.audio().open_capture(None, 16_000, 0)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/audio.open-capture
#[test]
fn wit_audio_open_capture_three_channels_unsupported() {
    let mut rig = rig(GRANTS);
    assert_eq!(
        code_of(rig.host.audio().open_capture(None, 16_000, 3)),
        ErrorCode::Unsupported
    );
}

/// REQ: WIT:ilium:host/audio.open-capture
#[test]
fn wit_audio_open_capture_sample_rate_boundaries_invalid_argument() {
    let mut rig = rig(GRANTS);
    assert_eq!(
        code_of(rig.host.audio().open_capture(None, 7_999, 1)),
        ErrorCode::InvalidArgument
    );
    let low = rig
        .host
        .audio()
        .open_capture(None, 8_000, 1)
        .expect("8000 is the lowest rate");
    rig.host.audio().close_capture(low);
    assert!(rig.host.audio().open_capture(None, 48_000, 1).is_ok());
}

/// REQ: WIT:ilium:host/audio.open-capture
#[test]
fn wit_audio_open_capture_rate_above_the_ceiling_invalid_argument() {
    let mut rig = rig(GRANTS);
    assert_eq!(
        code_of(rig.host.audio().open_capture(None, 192_001, 1)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/audio.open-capture
#[test]
fn wit_audio_open_capture_third_capture_quota_exceeded() {
    let mut rig = rig(GRANTS);
    rig.host
        .audio()
        .open_capture(None, 16_000, 1)
        .expect("first");
    rig.host
        .audio()
        .open_capture(None, 16_000, 1)
        .expect("second");
    assert_eq!(
        code_of(rig.host.audio().open_capture(None, 16_000, 1)),
        ErrorCode::QuotaExceeded
    );
}

/// REQ: WIT:ilium:host/audio.open-capture
#[test]
fn wit_audio_open_capture_unknown_device_not_found() {
    let mut rig = rig(GRANTS);
    assert_eq!(
        code_of(
            rig.host
                .audio()
                .open_capture(Some("nope".to_owned()), 16_000, 1)
        ),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/audio.open-capture
#[test]
fn wit_audio_open_capture_declined_grant_not_permitted() {
    let mut rig = declined_rig(&["audio.capture"]);
    assert_eq!(
        code_of(rig.host.audio().open_capture(None, 16_000, 1)),
        ErrorCode::NotPermitted
    );
}

/// REQ: WIT:ilium:host/audio.open-loopback
#[test]
fn wit_audio_open_loopback_returns_a_capture_id() {
    let mut rig = rig(GRANTS);
    let id = rig.host.audio().open_loopback(16_000, 2).expect("opens");
    assert!(rig.kernel.open_captures().contains(&id));
}

/// REQ: WIT:ilium:host/audio.open-loopback
#[test]
fn wit_audio_open_loopback_no_monitor_source_unsupported() {
    let mut rig = rig(GRANTS);
    rig.kernel.set_loopback_available(false);
    assert_eq!(
        code_of(rig.host.audio().open_loopback(16_000, 2)),
        ErrorCode::Unsupported
    );
}

/// REQ: WIT:ilium:host/audio.open-loopback
#[test]
fn wit_audio_open_loopback_zero_channels_invalid_argument() {
    let mut rig = rig(GRANTS);
    assert_eq!(
        code_of(rig.host.audio().open_loopback(16_000, 0)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/audio.open-loopback
#[test]
fn wit_audio_open_loopback_declined_grant_not_permitted() {
    let mut rig = declined_rig(&["audio.loopback"]);
    assert_eq!(
        code_of(rig.host.audio().open_loopback(16_000, 2)),
        ErrorCode::NotPermitted
    );
}

/// REQ: WIT:ilium:host/audio.close-capture
#[test]
fn wit_audio_close_capture_releases_the_slot() {
    let mut rig = rig(GRANTS);
    let first = rig
        .host
        .audio()
        .open_capture(None, 16_000, 1)
        .expect("first");
    rig.host
        .audio()
        .open_capture(None, 16_000, 1)
        .expect("second");
    rig.host.audio().close_capture(first);
    assert!(rig.host.audio().open_capture(None, 16_000, 1).is_ok());
}

/// REQ: WIT:ilium:host/audio.close-capture
#[test]
fn wit_audio_close_capture_unknown_id_is_a_no_op() {
    let mut rig = rig(GRANTS);
    rig.host.audio().close_capture(424_242);
    assert!(rig.host.failures().is_empty());
}

/// REQ: WIT:ilium:host/audio.close-capture
#[test]
fn wit_audio_close_capture_twice_is_a_no_op() {
    let mut rig = rig(GRANTS);
    let id = rig
        .host
        .audio()
        .open_capture(None, 16_000, 1)
        .expect("opens");
    rig.host.audio().close_capture(id);
    rig.host.audio().close_capture(id);
    assert!(rig.kernel.open_captures().is_empty());
}

/// REQ: WIT:ilium:host/audio.play
#[test]
fn wit_audio_play_queues_the_samples() {
    let mut rig = rig(GRANTS);
    let request = rig
        .host
        .audio()
        .play(vec![0i16; 1_600], 16_000, 1)
        .expect("accepted");
    assert!(rig.host.request_completed(request));
    assert_eq!(rig.kernel.played_audio().len(), 1);
}

/// REQ: WIT:ilium:host/audio.play
#[test]
fn wit_audio_play_samples_not_a_multiple_of_channels_invalid_argument() {
    let mut rig = rig(GRANTS);
    assert_eq!(
        code_of(rig.host.audio().play(vec![0i16; 3], 16_000, 2)),
        ErrorCode::InvalidArgument
    );
    assert!(rig.host.audio().play(vec![0i16; 4], 16_000, 2).is_ok());
}

/// REQ: WIT:ilium:host/audio.play
#[test]
fn wit_audio_play_thirty_seconds_boundary_invalid_argument() {
    let mut rig = rig(GRANTS);
    let thirty_seconds = 8_000 * 30;
    assert!(rig
        .host
        .audio()
        .play(vec![0i16; thirty_seconds], 8_000, 1)
        .is_ok());
    assert_eq!(
        code_of(
            rig.host
                .audio()
                .play(vec![0i16; thirty_seconds + 1], 8_000, 1)
        ),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/audio.play
#[test]
fn wit_audio_play_over_sixteen_mib_invalid_argument() {
    let mut rig = rig(GRANTS);
    let limit = 16 * MIB / 2;
    assert_eq!(
        code_of(rig.host.audio().play(vec![0i16; limit + 1], 48_000, 2)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/audio.play
#[test]
fn wit_audio_play_three_channels_unsupported() {
    let mut rig = rig(GRANTS);
    assert_eq!(
        code_of(rig.host.audio().play(vec![0i16; 3], 16_000, 3)),
        ErrorCode::Unsupported
    );
}

/// REQ: WIT:ilium:host/audio.play
#[test]
fn wit_audio_play_declined_grant_not_permitted() {
    let mut rig = declined_rig(&["audio.playback"]);
    assert_eq!(
        code_of(rig.host.audio().play(vec![0i16; 2], 16_000, 1)),
        ErrorCode::NotPermitted
    );
}
