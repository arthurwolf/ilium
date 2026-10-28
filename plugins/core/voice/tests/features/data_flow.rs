//! Red-phase tests for the voice data flow rules COR-080..COR-084: audio and transcript retention and
//! the local or remote processing boundary. The rig below drives the real plugin through the FakeHost,
//! so the tests fail until the plugin keeps these rules.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    AudioFramesEvent, ErrorCode, Event, Intent, PluginError, ViewTarget,
};
use serde_json::json;

use crate::common::{client_host, configure, f8_grab, key_pressed, key_released};

/// Where the transcription provider runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Locality {
    Local,
    Remote,
}

/// How a capture ended.
#[derive(Clone, Copy, Debug)]
enum Ending {
    Completed,
    Failed,
    Cancelled,
}

/// Observable result of one voice session run against the skeleton.
struct SessionObservation {
    audio_files_on_disk: usize,
    audio_buffer_len_after_end: usize,
    network_connections: usize,
    remote_marker_shown: bool,
    transcript_in_log: bool,
    transcript_after_end: String,
    leaked_to_other_plugins: bool,
    settings_names_provider: bool,
    fell_back_to_remote: bool,
}

/// The phrase the transcription job returns; it must never show up in a log, topic or foreign payload.
const PHRASE: &str = "close the pane called secret-marker-pane";

/// Setting key naming the transcription provider and whether it is `local` or `remote` (assumed key name:
/// the page names the provider in Settings but gives no key; see the manual page "Data and privacy").
const PROVIDER_KEY: &str = "transcription_provider";

fn strip_text(host: &mut FakeHost) -> String {
    host.render(ViewTarget::Panel("voice".to_string()), 60, 5)
        .text()
}

fn network_connections(host: &mut FakeHost) -> usize {
    host.http().calls().len() + host.http().streams().len()
}

/// Presses F8, feeds one audio frame, releases and returns the request id of the `transcribe` job.
fn speak(host: &mut FakeHost) -> Option<u64> {
    let grab = f8_grab(host);
    host.deliver_one(key_pressed(grab, false));
    host.settle();
    let capture = host.audio().captures().last()?.id;
    host.deliver_one(Event::AudioFrames(AudioFramesEvent {
        capture,
        samples: vec![1200, -1200, 900, -900],
    }));
    host.deliver_one(key_released(grab));
    host.settle();
    host.jobs().list().last().map(|job| job.request)
}

fn run_session(locality: Locality, ending: Ending) -> SessionObservation {
    let mut host = client_host();
    let provider = match locality {
        Locality::Local => "local",
        Locality::Remote => "remote",
    };
    configure(&mut host, PROVIDER_KEY, json!(provider));
    configure(&mut host, "input_mode", json!("push_to_talk"));
    configure(&mut host, "enabled", json!(true));
    let settings_text = host
        .render(ViewTarget::SettingsPage("voice-control".to_string()), 80, 40)
        .text();
    let settings_names_provider =
        settings_text.contains(provider) && settings_text.to_lowercase().contains("provider");
    let request = speak(&mut host);
    let marker_text = strip_text(&mut host);
    let remote_marker_shown = marker_text.to_lowercase().contains("remote");
    if let Some(request) = request {
        match ending {
            Ending::Completed => host.jobs().finish(request, Ok(PHRASE.as_bytes().to_vec())),
            Ending::Failed => host.jobs().finish(
                request,
                Err(PluginError::new(ErrorCode::Io, "provider failed")),
            ),
            Ending::Cancelled => host.jobs().request_cancel(),
        }
    }
    host.settle();
    let after_text = strip_text(&mut host);
    let transcript_after_end = if after_text.contains(PHRASE) {
        PHRASE.to_string()
    } else {
        String::new()
    };
    // The buffer is observable through the next job: a capture that carries no frame must hand the
    // transcription job an empty input when the earlier buffer was released.
    let jobs_before = host.jobs().list().len();
    let grab = f8_grab(&mut host);
    host.deliver_one(key_pressed(grab, false));
    host.settle();
    host.deliver_one(key_released(grab));
    host.settle();
    let audio_buffer_len_after_end = host
        .jobs()
        .list()
        .into_iter()
        .skip(jobs_before)
        .map(|job| job.input.len())
        .max()
        .unwrap_or(0);
    let transcript_in_log = host.logs().iter().any(|line| line.contains(PHRASE));
    let leaked_to_other_plugins = host
        .published_topics()
        .iter()
        .any(|(_, payload)| payload.contains(PHRASE))
        || host
            .service_calls()
            .iter()
            .any(|(_, _, payload)| payload.contains(PHRASE))
        || host
            .submitted()
            .iter()
            .any(|submission| match &submission.intent {
                Intent::InvokeCommand(spec) => spec.arguments_json.contains(PHRASE),
                other => format!("{other:?}").contains(PHRASE),
            });
    SessionObservation {
        audio_files_on_disk: host.fs().all_paths().len(),
        audio_buffer_len_after_end,
        network_connections: network_connections(&mut host),
        remote_marker_shown,
        transcript_in_log,
        transcript_after_end,
        leaked_to_other_plugins,
        settings_names_provider,
        fell_back_to_remote: matches!(locality, Locality::Local)
            && network_connections(&mut host) > 0,
    }
}

/// A local provider whose transcription job fails: the plugin must stay local and say so.
fn run_failing_local_provider() -> SessionObservation {
    run_session(Locality::Local, Ending::Failed)
}

/// REQ: COR-080
#[test]
fn audio_never_reaches_disk_and_buffer_is_released() {
    for ending in [Ending::Completed, Ending::Failed, Ending::Cancelled] {
        let seen = run_session(Locality::Local, ending);
        assert_eq!(seen.audio_files_on_disk, 0, "{ending:?}");
        assert_eq!(seen.audio_buffer_len_after_end, 0, "{ending:?}");
    }
}

/// REQ: COR-081
#[test]
fn local_provider_opens_no_connection_and_remote_shows_marker() {
    let local = run_session(Locality::Local, Ending::Completed);
    assert_eq!(local.network_connections, 0);
    assert!(!local.remote_marker_shown);
    let remote = run_session(Locality::Remote, Ending::Completed);
    assert_eq!(remote.network_connections, 1);
    assert!(remote.remote_marker_shown);
}

/// REQ: COR-082
#[test]
fn settings_name_provider_and_failure_never_falls_back_to_remote() {
    let seen = run_failing_local_provider();
    assert!(seen.settings_names_provider);
    assert!(!seen.fell_back_to_remote);
}

/// REQ: COR-083
#[test]
fn transcript_is_session_only_and_not_logged() {
    let seen = run_session(Locality::Local, Ending::Completed);
    assert!(!seen.transcript_in_log);
    assert!(seen.transcript_after_end.is_empty());
}

/// REQ: COR-084
#[test]
fn audio_and_transcript_are_not_exposed_to_other_plugins() {
    let seen = run_session(Locality::Remote, Ending::Completed);
    assert!(!seen.leaked_to_other_plugins);
}

/// REQ: COR-080
#[test]
fn audio_is_not_kept_on_disk_or_in_memory_after_a_failing_local_provider() {
    let seen = run_failing_local_provider();
    assert_eq!(seen.audio_files_on_disk, 0);
    assert_eq!(seen.audio_buffer_len_after_end, 0);
}

/// REQ: COR-081
#[test]
fn local_provider_opens_no_connection_for_any_ending() {
    for ending in [Ending::Completed, Ending::Failed, Ending::Cancelled] {
        let seen = run_session(Locality::Local, ending);
        assert_eq!(seen.network_connections, 0, "{ending:?}");
        assert!(!seen.remote_marker_shown, "{ending:?}");
    }
}

/// REQ: COR-082
#[test]
fn failing_local_provider_opens_no_remote_connection_and_shows_no_remote_marker() {
    let seen = run_failing_local_provider();
    assert_eq!(seen.network_connections, 0);
    assert!(!seen.remote_marker_shown);
}

/// REQ: COR-083
#[test]
fn transcript_is_empty_after_every_ending_and_never_logged() {
    for ending in [Ending::Completed, Ending::Failed, Ending::Cancelled] {
        let seen = run_session(Locality::Local, ending);
        assert!(seen.transcript_after_end.is_empty(), "{ending:?}");
        assert!(!seen.transcript_in_log, "{ending:?}");
    }
}

/// REQ: COR-084
#[test]
fn audio_and_transcript_are_not_exposed_for_either_locality() {
    for locality in [Locality::Local, Locality::Remote] {
        let seen = run_session(locality, Ending::Completed);
        assert!(!seen.leaked_to_other_plugins, "{locality:?}");
    }
}
