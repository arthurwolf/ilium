//! F-050: the voice session. F8 is grabbed through keys.intercept, a 16 kHz mono capture runs while the key is
//! held, `audio-frames` are buffered and a `transcribe` job runs on release; `voice.toggle` flips `enabled`.

use ilium_plugin_sdk::types::{AudioFramesEvent, Event, ErrorCode, PluginError};
use proptest::prelude::*;
use serde_json::json;

use crate::common::{
    client_host, command_invoked, configure, enabled_client, f8_grab, invoked_commands, key_pressed, key_released,
};

fn frames(capture: u64, samples: Vec<i16>) -> Event {
    Event::AudioFrames(AudioFramesEvent { capture, samples })
}

fn le_bytes(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|sample| sample.to_le_bytes()).collect()
}

/// Enabled push-to-talk host with the key pressed; returns the host, the grab and the open capture id.
fn recording() -> (ilium_plugin_sdk::testing::FakeHost, u64, u64) {
    let mut host = enabled_client("push_to_talk");
    let grab = f8_grab(&mut host);
    host.deliver_one(key_pressed(grab, false));
    host.settle();
    let capture = host.audio().captures().first().expect("capture opened").id;
    (host, grab, capture)
}

/// REQ: F-050
#[test]
fn session_050_enabled_voice_grabs_f8_until_release() {
    let mut host = enabled_client("push_to_talk");
    let grabs = host.keys().grabs();
    assert_eq!(grabs.len(), 1);
    assert_eq!(grabs[0].chords, ["F8"]);
    assert!(grabs[0].until_release);
    assert_eq!(grabs[0].pane, None);
    assert!(!grabs[0].released);
}

/// REQ: F-050
#[test]
fn session_050_disabled_voice_holds_no_grab() {
    let mut host = client_host();
    assert!(host.keys().grabs().is_empty());
}

/// REQ: F-050
#[test]
fn session_050_enabling_twice_keeps_exactly_one_grab() {
    let mut host = enabled_client("push_to_talk");
    configure(&mut host, "enabled", json!(true));
    let live = host.keys().grabs().into_iter().filter(|grab| !grab.released).count();
    assert_eq!(live, 1);
}

/// REQ: F-050
#[test]
fn session_050_turning_voice_off_releases_the_grab() {
    let mut host = enabled_client("push_to_talk");
    configure(&mut host, "enabled", json!(false));
    assert!(host.keys().grabs().iter().all(|grab| grab.released));
}

/// REQ: F-050
#[test]
fn session_050_turning_voice_on_again_grabs_f8_again() {
    let mut host = enabled_client("push_to_talk");
    configure(&mut host, "enabled", json!(false));
    configure(&mut host, "enabled", json!(true));
    assert_eq!(host.keys().grabs().iter().filter(|grab| !grab.released).count(), 1);
}

/// REQ: F-050
#[test]
fn session_050_pressing_f8_opens_a_16_khz_mono_capture_on_the_default_device() {
    let (mut host, _grab, _capture) = recording();
    let captures = host.audio().captures();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].sample_rate_hz, 16_000);
    assert_eq!(captures[0].channels, 1);
    assert_eq!(captures[0].device, None);
    assert!(captures[0].open);
}

/// REQ: F-050
#[test]
fn session_050_a_repeated_key_press_does_not_open_a_second_capture() {
    let (mut host, grab, _capture) = recording();
    host.deliver_one(key_pressed(grab, true));
    host.settle();
    assert_eq!(host.audio().captures().len(), 1);
}

/// REQ: F-050
#[test]
fn session_050_a_press_of_a_foreign_grab_is_ignored() {
    let mut host = enabled_client("push_to_talk");
    let grab = f8_grab(&mut host);
    host.deliver_one(key_pressed(grab + 1000, false));
    host.settle();
    assert!(host.audio().captures().is_empty());
}

/// REQ: F-050
#[test]
fn session_050_releasing_f8_closes_the_capture() {
    let (mut host, grab, _capture) = recording();
    host.deliver_one(key_released(grab));
    host.settle();
    assert!(host.audio().captures().iter().all(|capture| !capture.open));
}

/// REQ: F-050
#[test]
fn session_050_a_release_of_a_foreign_grab_keeps_the_capture_open() {
    let (mut host, grab, _capture) = recording();
    host.deliver_one(key_released(grab + 1000));
    host.settle();
    assert!(host.audio().captures()[0].open);
}

/// REQ: F-050
#[test]
fn session_050_a_release_without_a_press_starts_no_job() {
    let mut host = enabled_client("push_to_talk");
    let grab = f8_grab(&mut host);
    host.deliver_one(key_released(grab));
    host.settle();
    assert!(host.jobs().list().is_empty());
}

/// REQ: F-050
#[test]
fn session_050_release_starts_one_transcribe_job_with_the_little_endian_samples() {
    let (mut host, grab, capture) = recording();
    host.deliver_one(frames(capture, vec![1, -2, 300]));
    host.deliver_one(frames(capture, vec![i16::MAX, i16::MIN]));
    host.deliver_one(key_released(grab));
    host.settle();
    let jobs = host.jobs().list();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].kind, "transcribe");
    assert_eq!(jobs[0].input, le_bytes(&[1, -2, 300, i16::MAX, i16::MIN]));
}

/// REQ: F-050
#[test]
fn session_050_frames_of_another_capture_are_not_buffered() {
    let (mut host, grab, capture) = recording();
    host.deliver_one(frames(capture + 99, vec![7, 7, 7]));
    host.deliver_one(frames(capture, vec![5]));
    host.deliver_one(key_released(grab));
    host.settle();
    assert_eq!(host.jobs().list()[0].input, le_bytes(&[5]));
}

/// REQ: F-050
#[test]
fn session_050_frames_after_the_release_do_not_reach_the_job() {
    let (mut host, grab, capture) = recording();
    host.deliver_one(frames(capture, vec![1]));
    host.deliver_one(key_released(grab));
    host.deliver_one(frames(capture, vec![2]));
    host.settle();
    assert_eq!(host.jobs().list()[0].input, le_bytes(&[1]));
}

/// REQ: F-050
#[test]
fn session_050_a_second_press_starts_a_fresh_buffer() {
    let (mut host, grab, capture) = recording();
    host.deliver_one(frames(capture, vec![1, 2]));
    host.deliver_one(key_released(grab));
    host.deliver_one(key_pressed(grab, false));
    host.settle();
    let second = host.audio().captures().last().expect("second capture").id;
    host.deliver_one(frames(second, vec![9]));
    host.deliver_one(key_released(grab));
    host.settle();
    let jobs = host.jobs().list();
    assert_eq!(jobs.len(), 2);
    assert_eq!(jobs[1].input, le_bytes(&[9]));
}

/// REQ: F-050
#[test]
fn session_050_a_failed_transcription_is_logged_as_a_warning() {
    let (mut host, grab, capture) = recording();
    host.deliver_one(frames(capture, vec![1]));
    host.deliver_one(key_released(grab));
    host.settle();
    let request = host.jobs().list()[0].request;
    host.jobs().finish(
        request,
        Err(PluginError { code: ErrorCode::Internal, message: "no model".to_string(), retry_after_ms: None }),
    );
    host.settle();
    assert!(host.logs().iter().any(|line| line.level == "warn" && line.message.starts_with("transcription failed")));
}

/// REQ: F-050
#[test]
fn session_050_a_failed_transcription_submits_no_command() {
    let (mut host, grab, capture) = recording();
    host.deliver_one(frames(capture, vec![1]));
    host.deliver_one(key_released(grab));
    host.settle();
    let request = host.jobs().list()[0].request;
    host.jobs().finish(
        request,
        Err(PluginError { code: ErrorCode::Internal, message: "no model".to_string(), retry_after_ms: None }),
    );
    host.settle();
    assert!(invoked_commands(&host.submitted()).is_empty());
}

/// REQ: F-050
#[test]
fn session_050_toggle_command_turns_voice_on_and_saves_enabled() {
    let mut host = client_host();
    host.deliver_one(command_invoked("voice.toggle"));
    host.settle();
    assert_eq!(host.kernel_settings("voice-control").get("enabled"), Some(json!(true)));
}

/// REQ: F-050
#[test]
fn session_050_toggle_command_twice_turns_voice_off_again() {
    let mut host = client_host();
    host.deliver_one(command_invoked("voice.toggle"));
    host.deliver_one(command_invoked("voice.toggle"));
    host.settle();
    assert_eq!(host.kernel_settings("voice-control").get("enabled"), Some(json!(false)));
}

/// REQ: F-050
#[test]
fn session_050_semantic_vad_f8_press_toggles_voice_and_release_does_nothing() {
    let mut host = enabled_client("semantic_vad");
    let grab = f8_grab(&mut host);
    host.deliver_one(key_pressed(grab, false));
    host.settle();
    assert_eq!(host.kernel_settings("voice-control").get("enabled"), Some(json!(false)));
    host.deliver_one(key_released(grab));
    host.settle();
    assert_eq!(host.kernel_settings("voice-control").get("enabled"), Some(json!(false)));
    assert!(host.jobs().list().is_empty());
}

/// REQ: F-050
#[test]
fn session_050_push_to_talk_toggle_from_off_turns_voice_on_and_a_press_then_records() {
    let mut host = client_host();
    configure(&mut host, "input_mode", json!("push_to_talk"));
    host.deliver_one(command_invoked("voice.toggle"));
    host.settle();
    assert_eq!(host.kernel_settings("voice-control").get("enabled"), Some(json!(true)));
    let grab = f8_grab(&mut host);
    host.deliver_one(key_pressed(grab, false));
    host.settle();
    assert_eq!(host.audio().captures().len(), 1);
}

/// REQ: F-050
#[test]
fn session_050_push_to_talk_f8_press_does_not_toggle_voice_off() {
    let (mut host, grab, _capture) = recording();
    host.deliver_one(key_pressed(grab, false));
    host.settle();
    assert_eq!(host.kernel_settings("voice-control").get("enabled"), Some(json!(true)));
    assert_eq!(host.audio().captures().len(), 1);
    assert!(host.audio().captures()[0].open);
}

/// REQ: F-050
#[test]
fn session_050_deactivation_closes_an_open_capture_and_releases_the_grab() {
    let (mut host, _grab, _capture) = recording();
    host.deactivate(ilium_plugin_sdk::types::DeactivationReason::Shutdown);
    assert!(host.audio().captures().iter().all(|capture| !capture.open));
    assert!(host.keys().grabs().iter().all(|grab| grab.released));
}

/// REQ: F-050
#[test]
fn session_050_a_phrase_heard_runs_the_matched_command_through_an_invoke_command_intent() {
    let (mut host, grab, capture) = recording();
    host.contribute_extension(
        "ilium.core.voice/voice-intents",
        "open-board",
        50,
        &json!({ "phrases": ["open the board"], "command": "open-board", "confirm": false }).to_string(),
    );
    host.deliver_one(frames(capture, vec![1]));
    host.deliver_one(key_released(grab));
    host.settle();
    let request = host.jobs().list()[0].request;
    host.jobs().finish(request, Ok(b"open the board".to_vec()));
    host.settle();
    assert_eq!(invoked_commands(&host.submitted()), ["open-board"]);
}

/// REQ: F-050
#[test]
fn session_050_an_unknown_phrase_runs_no_command() {
    let (mut host, grab, capture) = recording();
    host.deliver_one(frames(capture, vec![1]));
    host.deliver_one(key_released(grab));
    host.settle();
    let request = host.jobs().list()[0].request;
    host.jobs().finish(request, Ok(b"zzz qqq unmatched".to_vec()));
    host.settle();
    assert!(invoked_commands(&host.submitted()).is_empty());
}

proptest! {
    // REQ: F-050
    #[test]
    fn session_050_property_the_job_input_is_the_little_endian_concatenation_of_every_frame(
        batches in proptest::collection::vec(proptest::collection::vec(any::<i16>(), 0..8), 0..6)
    ) {
        let (mut host, grab, capture) = recording();
        let mut all = Vec::new();
        for batch in &batches {
            all.extend_from_slice(batch);
            host.deliver_one(frames(capture, batch.clone()));
        }
        host.deliver_one(key_released(grab));
        host.settle();
        let jobs = host.jobs().list();
        prop_assert_eq!(jobs.len(), 1);
        prop_assert_eq!(&jobs[0].input, &le_bytes(&all));
    }
}
