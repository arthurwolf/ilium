//! Red-phase tests for the voice data flow rules COR-080..COR-084: audio and transcript retention and
//! the local or remote processing boundary. The skeleton below stands in for the plugin behaviour that
//! is not built yet; every body is `unimplemented!()`.

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

fn run_session(_locality: Locality, _ending: Ending) -> SessionObservation {
    unimplemented!("voice data flow skeleton: COR-080..COR-084")
}

fn run_failing_local_provider() -> SessionObservation {
    unimplemented!("voice data flow skeleton: provider failure")
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
