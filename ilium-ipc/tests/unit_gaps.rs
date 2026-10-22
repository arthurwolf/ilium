//! Direct coverage for public `ilium-ipc` items that had no unit test: plugin wire records,
//! text-trigger target labels, voice-text rejections, pane environment names, request diagnostic
//! names, bounded decoding and frame writing.

use bincode::Options;
use ilium_core::NodeId;
use ilium_ipc::{
    decode_bounded_frame, encode_frame, pane_env, plugin, ClientRequest, FrameWriter, IpcError,
    MouseEventKind, MouseModifiers, PaneResizeCause, PromptSubmissionSource, ServerEvent,
    TextTriggerTarget, VoiceTextRejection, VoiceTextRejectionCode, MAX_FRAME_LEN,
};
use serde::de::DeserializeOwned;
use serde::{Serialize, Serializer};
use std::collections::HashSet;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::AsyncWrite;

fn run_async<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime builds")
        .block_on(future)
}

/// Serializes to exactly these bytes: bincode writes a tuple of `u8` with no length prefix.
struct RawBytes(Vec<u8>);

impl Serialize for RawBytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeTuple;
        let mut tuple = serializer.serialize_tuple(self.0.len())?;
        for byte in &self.0 {
            tuple.serialize_element(byte)?;
        }
        tuple.end()
    }
}

fn decode_raw_client_request(bytes: Vec<u8>) -> Result<ClientRequest, IpcError> {
    let frame = encode_frame(&RawBytes(bytes)).expect("raw bytes fit the frame limit");
    decode_bounded_frame::<ClientRequest>(&frame)
}

fn encode<T: Serialize + ?Sized>(value: &T) -> Vec<u8> {
    bincode::serialize(value).expect("plugin record serializes")
}

/// Fixed-width integers, and trailing bytes are an error, matching the frame decoder.
fn decode_strict<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, bincode::Error> {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .deserialize(bytes)
}

fn attach_request() -> ClientRequest {
    ClientRequest::Attach {
        session: "main".to_owned(),
        hello: plugin::PluginHello::default(),
    }
}

fn key_input(submission: Option<PromptSubmissionSource>) -> ClientRequest {
    ClientRequest::KeyInput {
        pane_id: NodeId(2),
        bytes: b"\r".to_vec(),
        submission,
    }
}

fn mouse_input() -> ClientRequest {
    ClientRequest::MouseInput {
        pane_id: NodeId(2),
        kind: MouseEventKind::Moved,
        column: 4,
        row: 5,
        modifiers: MouseModifiers {
            shift: false,
            alt: false,
            control: false,
        },
    }
}

/// Requests with their documented diagnostic names. Covers unit, struct and payload variants.
fn sampled_requests() -> Vec<(ClientRequest, &'static str)> {
    vec![
        (ClientRequest::Detach, "detach"),
        (ClientRequest::KillSession, "kill_session"),
        (ClientRequest::RestartServer, "restart_server"),
        (
            ClientRequest::RevertLastRestructure,
            "revert_last_restructure",
        ),
        (
            ClientRequest::RegisterVoiceTextReceiver,
            "register_voice_text_receiver",
        ),
        (
            ClientRequest::ClosePane { pane_id: NodeId(2) },
            "close_pane",
        ),
        (
            ClientRequest::ResizePane {
                pane_id: NodeId(2),
                rows: 24,
                cols: 80,
                cause: PaneResizeCause::HostTerminal,
            },
            "resize_pane",
        ),
        (key_input(None), "key_input"),
        (mouse_input(), "mouse_input"),
        (attach_request(), "attach"),
    ]
}

#[test]
fn bounded_decode_round_trips_a_client_request() {
    let request = ClientRequest::Detach;
    let frame = encode_frame(&request).expect("encode detach");
    let decoded = decode_bounded_frame::<ClientRequest>(&frame).expect("decode detach");
    assert_eq!(decoded, request);
}

#[test]
fn bounded_decode_round_trips_a_server_event() {
    let event = ServerEvent::Error {
        message: "boom".to_owned(),
    };
    let frame = encode_frame(&event).expect("encode server event");
    let decoded = decode_bounded_frame::<ServerEvent>(&frame).expect("decode server event");
    assert_eq!(decoded, event);
}

#[test]
fn bounded_decode_empty_payload_is_an_error_not_a_panic() {
    let result = decode_raw_client_request(Vec::new());
    assert!(
        matches!(result, Err(IpcError::Bincode(_))),
        "empty payload must be a bincode error"
    );
}

#[test]
fn bounded_decode_truncated_payload_returns_a_bincode_error() {
    let mut bytes = bincode::serialize(&attach_request()).expect("serialize attach");
    bytes.pop();
    let result = decode_raw_client_request(bytes);
    assert!(
        matches!(result, Err(IpcError::Bincode(_))),
        "truncated payload must be a bincode error"
    );
}

#[test]
fn bounded_decode_refuses_trailing_bytes_after_a_valid_value() {
    let mut bytes = bincode::serialize(&attach_request()).expect("serialize attach");
    bytes.push(0);
    let result = decode_raw_client_request(bytes);
    assert!(
        matches!(result, Err(IpcError::Bincode(_))),
        "trailing bytes must be refused"
    );
}

/// `Attach` = variant 0, empty session, protocol 1, empty contract, then a `installed` sequence
/// whose length prefix claims `u64::MAX` entries with no bytes behind it.
#[test]
fn bounded_decode_refuses_oversized_sequence_length_before_allocating() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    bytes.extend_from_slice(&u64::MAX.to_le_bytes());
    let result = decode_raw_client_request(bytes);
    assert!(
        matches!(result, Err(IpcError::DecodeResourceLimit { .. })),
        "oversized sequence must hit the resource limit, got {result:?}"
    );
}

#[test]
fn frame_writer_write_then_read_round_trips_a_value() {
    let value = vec!["first".to_owned(), "second".to_owned()];
    let mut buffer = Vec::new();
    run_async(async {
        FrameWriter::new(&mut buffer)
            .write(&value)
            .await
            .expect("write frame");
    });
    let decoded = run_async(async {
        ilium_ipc::read_frame::<Vec<String>, _>(&mut buffer.as_slice())
            .await
            .expect("read frame")
    });
    assert_eq!(decoded, value);
}

#[test]
fn frame_writer_zero_length_value_writes_only_the_schema_tagged_prefix() {
    let mut buffer = Vec::new();
    run_async(async {
        FrameWriter::new(&mut buffer)
            .write(&())
            .await
            .expect("write empty value");
    });
    assert_eq!(buffer, 0xC000_0000_u32.to_le_bytes().to_vec());
}

#[test]
fn frame_writer_oversized_payload_is_refused_as_frame_too_large() {
    let oversized = vec![0_u8; MAX_FRAME_LEN as usize];
    let mut buffer = Vec::new();
    let result = run_async(async { FrameWriter::new(&mut buffer).write(&oversized).await });
    assert!(
        matches!(
            result,
            Err(IpcError::FrameTooLarge { actual, max })
                if actual > MAX_FRAME_LEN as usize && max == MAX_FRAME_LEN
        ),
        "oversized value must be refused as FrameTooLarge"
    );
    assert!(buffer.is_empty(), "a refused frame must write no bytes");
}

/// A writer whose every write fails with a broken pipe, as a closed peer would.
struct BrokenPipeWriter;

impl AsyncWrite for BrokenPipeWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        _buffer: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Poll::Ready(Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe)))
    }

    fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[test]
fn frame_writer_propagates_a_broken_pipe_as_an_io_error() {
    let result = run_async(async { FrameWriter::new(BrokenPipeWriter).write(&7_u32).await });
    assert!(
        matches!(
            &result,
            Err(IpcError::Io(error)) if error.kind() == std::io::ErrorKind::BrokenPipe
        ),
        "a broken pipe must surface as IpcError::Io"
    );
}

#[test]
fn diagnostic_names_of_sampled_variants_are_pinned() {
    for (request, expected) in sampled_requests() {
        assert_eq!(request.diagnostic_name(), expected, "name of {request:?}");
    }
}

#[test]
fn diagnostic_names_are_snake_case_and_unique_across_sampled_variants() {
    let names: Vec<&str> = sampled_requests()
        .iter()
        .map(|(request, _)| request.diagnostic_name())
        .collect();
    for name in &names {
        assert!(!name.is_empty(), "diagnostic names must not be empty");
        assert!(
            name.chars()
                .all(|character| character.is_ascii_lowercase() || character == '_'),
            "{name} is not snake_case"
        );
    }
    let unique: HashSet<&str> = names.iter().copied().collect();
    assert_eq!(
        unique.len(),
        names.len(),
        "two variants share a diagnostic name"
    );
}

#[test]
fn high_frequency_classification_covers_presentation_and_unsubmitted_input() {
    let high_frequency = [
        ClientRequest::ResizePane {
            pane_id: NodeId(2),
            rows: 24,
            cols: 80,
            cause: PaneResizeCause::HostTerminal,
        },
        key_input(None),
        ClientRequest::UserKeyInput {
            pane_id: NodeId(2),
            bytes: b"a".to_vec(),
            submission: None,
            prompt_epoch: None,
        },
        mouse_input(),
        ClientRequest::SetPaneFocus {
            pane_id: NodeId(2),
            focused: true,
        },
        ClientRequest::RecordNodeActivity { node_id: NodeId(2) },
        ClientRequest::SetVisiblePanes {
            pane_ids: vec![NodeId(2)],
        },
        ClientRequest::DiscardTerminalDelivery {
            pane_ids: vec![NodeId(2)],
        },
    ];
    for request in &high_frequency {
        assert!(
            request.is_high_frequency_diagnostic(),
            "{request:?} must log at debug level"
        );
    }
}

#[test]
fn structural_and_submitted_requests_are_not_high_frequency() {
    let structural = [
        ClientRequest::Detach,
        ClientRequest::ClosePane { pane_id: NodeId(2) },
        ClientRequest::SetNodeExpanded {
            node_id: NodeId(2),
            expanded: true,
        },
        ClientRequest::RegisterVoiceTextReceiver,
        key_input(Some(PromptSubmissionSource::Keyboard)),
        ClientRequest::UserKeyInput {
            pane_id: NodeId(2),
            bytes: b"a".to_vec(),
            submission: Some(PromptSubmissionSource::Keyboard),
            prompt_epoch: Some("epoch-1".to_owned()),
        },
    ];
    for request in &structural {
        assert!(
            !request.is_high_frequency_diagnostic(),
            "{request:?} must stay on the info-level trail"
        );
    }
}

#[test]
fn text_trigger_target_labels_are_exact() {
    assert_eq!(TextTriggerTarget::Agents.label(), "Agents");
    assert_eq!(TextTriggerTarget::Terminals.label(), "Terminals");
    assert_eq!(TextTriggerTarget::Both.label(), "Both");
}

#[test]
fn text_trigger_target_labels_are_distinct() {
    let labels: Vec<&str> = TextTriggerTarget::ALL
        .iter()
        .map(|target| target.label())
        .collect();
    let unique: HashSet<&str> = labels.iter().copied().collect();
    assert_eq!(
        unique.len(),
        labels.len(),
        "labels {labels:?} must be distinct"
    );
}

#[derive(Serialize)]
struct TargetHolder {
    target: TextTriggerTarget,
}

/// The saved settings name is the snake_case serde name; the label is its capitalised form.
#[test]
fn text_trigger_target_label_matches_the_saved_serde_name() {
    for target in TextTriggerTarget::ALL {
        let saved = toml::to_string(&TargetHolder { target }).expect("serialize target");
        let saved_name = saved
            .trim()
            .trim_start_matches("target = \"")
            .trim_end_matches('"');
        assert_eq!(
            saved_name,
            target.label().to_ascii_lowercase(),
            "saved name must equal the label, ignoring case"
        );
    }
}

fn declared_index(target: TextTriggerTarget) -> usize {
    match target {
        TextTriggerTarget::Agents => 0,
        TextTriggerTarget::Terminals => 1,
        TextTriggerTarget::Both => 2,
    }
}

#[test]
fn text_trigger_target_all_lists_each_variant_once_in_declared_order() {
    assert_eq!(TextTriggerTarget::ALL.len(), 3);
    for (position, target) in TextTriggerTarget::ALL.iter().enumerate() {
        assert_eq!(
            declared_index(*target),
            position,
            "ALL[{position}] must be the variant declared at that position"
        );
    }
}

#[test]
fn voice_text_rejection_new_preserves_code_and_message() {
    let rejection =
        VoiceTextRejection::new(VoiceTextRejectionCode::VoiceOff, "voice control is off");
    assert_eq!(rejection.code, VoiceTextRejectionCode::VoiceOff);
    assert_eq!(rejection.message, "voice control is off");
}

#[test]
fn voice_text_rejection_new_accepts_str_and_owned_string_alike() {
    let from_str = VoiceTextRejection::new(VoiceTextRejectionCode::NoVoiceClient, "none");
    let from_owned =
        VoiceTextRejection::new(VoiceTextRejectionCode::NoVoiceClient, String::from("none"));
    assert_eq!(from_str, from_owned);
}

#[test]
fn voice_text_rejection_allows_an_empty_message() {
    let rejection = VoiceTextRejection::new(VoiceTextRejectionCode::ClientUnresponsive, "");
    assert!(rejection.message.is_empty());
    assert_eq!(rejection.code, VoiceTextRejectionCode::ClientUnresponsive);
}

#[test]
fn pane_env_names_match_the_documented_variables() {
    assert_eq!(pane_env::PANE_ID, "ILIUM_PANE_ID");
    assert_eq!(pane_env::SESSION_NAME, "ILIUM_SESSION_NAME");
    assert_eq!(pane_env::SESSION_SOCKET, "ILIUM_SESSION_SOCKET");
    assert_eq!(pane_env::WORKTREE, "ILIUM_WORKTREE");
}

#[test]
fn pane_env_names_are_pairwise_distinct() {
    let names = [
        pane_env::PANE_ID,
        pane_env::SESSION_NAME,
        pane_env::SESSION_SOCKET,
        pane_env::WORKTREE,
    ];
    let unique: HashSet<&str> = names.iter().copied().collect();
    assert_eq!(unique.len(), names.len());
}

#[test]
fn plugin_protocol_version_is_one_at_the_crate_root_and_the_plugin_path() {
    assert_eq!(
        ilium_ipc::PLUGIN_PROTOCOL_VERSION,
        plugin::PLUGIN_PROTOCOL_VERSION
    );
    assert_eq!(plugin::PLUGIN_PROTOCOL_VERSION, 1);
}

#[test]
fn plugin_state_changed_round_trips_fields_in_declared_order() {
    let state = plugin::PluginStateChanged {
        plugin: "ilium.weather".to_owned(),
        side: "server".to_owned(),
        state: "active".to_owned(),
        instance: 7,
    };
    let bytes = encode(&state);
    let expected = [
        encode("ilium.weather"),
        encode("server"),
        encode("active"),
        encode(&7_u64),
    ]
    .concat();
    assert_eq!(bytes, expected, "fields must encode in declared order");
    let decoded: plugin::PluginStateChanged = decode_strict(&bytes).expect("decode state");
    assert_eq!(decoded, state);
}

#[test]
fn plugin_state_changed_round_trips_empty_strings_and_max_instance() {
    let empty = plugin::PluginStateChanged {
        plugin: String::new(),
        side: String::new(),
        state: String::new(),
        instance: 0,
    };
    let decoded: plugin::PluginStateChanged = decode_strict(&encode(&empty)).expect("decode empty");
    assert_eq!(decoded, empty);

    let maximum = plugin::PluginStateChanged {
        instance: u64::MAX,
        ..empty
    };
    let decoded: plugin::PluginStateChanged =
        decode_strict(&encode(&maximum)).expect("decode maximum instance");
    assert_eq!(decoded.instance, u64::MAX);
}

#[test]
fn plugin_state_changed_truncated_bytes_return_an_error() {
    let state = plugin::PluginStateChanged {
        plugin: "p".to_owned(),
        side: "client".to_owned(),
        state: "idle".to_owned(),
        instance: 1,
    };
    let mut bytes = encode(&state);
    bytes.pop();
    assert!(decode_strict::<plugin::PluginStateChanged>(&bytes).is_err());
}

#[test]
fn plugin_contribution_delta_round_trips_an_empty_delta() {
    let delta = plugin::PluginContributionDelta {
        plugin: String::new(),
        extension_point: String::new(),
        added_json: Vec::new(),
        removed: Vec::new(),
        revision: 0,
    };
    let decoded: plugin::PluginContributionDelta =
        decode_strict(&encode(&delta)).expect("decode empty delta");
    assert_eq!(decoded, delta);
}

#[test]
fn plugin_contribution_delta_refuses_trailing_bytes() {
    let delta = plugin::PluginContributionDelta {
        plugin: "p".to_owned(),
        extension_point: "commands".to_owned(),
        added_json: vec!["{}".to_owned()],
        removed: vec!["old".to_owned()],
        revision: 3,
    };
    let mut bytes = encode(&delta);
    bytes.push(0);
    assert!(decode_strict::<plugin::PluginContributionDelta>(&bytes).is_err());
}

#[test]
fn plugin_view_tree_round_trips_an_empty_tree() {
    let tree = plugin::PluginViewTree {
        plugin: String::new(),
        view: String::new(),
        tree_json: String::new(),
        revision: 0,
    };
    let decoded: plugin::PluginViewTree = decode_strict(&encode(&tree)).expect("decode tree");
    assert_eq!(decoded, tree);
}

#[test]
fn plugin_view_tree_round_trips_nested_json_content() {
    let tree = plugin::PluginViewTree {
        plugin: "ilium.notes".to_owned(),
        view: "panel".to_owned(),
        tree_json: r#"{"kind":"column","children":[{"kind":"text","value":"hi"}]}"#.to_owned(),
        revision: 12,
    };
    let decoded: plugin::PluginViewTree = decode_strict(&encode(&tree)).expect("decode tree");
    assert_eq!(decoded, tree);
}

/// `PluginViewTree` has no child count; this crafts a `tree_json` length prefix that claims more
/// bytes than the input holds and checks that decoding fails cleanly.
#[test]
fn plugin_view_tree_length_prefix_beyond_the_input_fails_cleanly() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    bytes.extend_from_slice(&u64::MAX.to_le_bytes());
    assert!(decode_strict::<plugin::PluginViewTree>(&bytes).is_err());
}

#[test]
fn plugin_submission_round_trips_empty_and_non_empty_payloads() {
    let empty = plugin::PluginSubmission {
        plugin: "ilium.notes".to_owned(),
        instance: 1,
        submissions_json: Vec::new(),
    };
    let decoded: plugin::PluginSubmission =
        decode_strict(&encode(&empty)).expect("decode empty submission");
    assert_eq!(decoded, empty);

    let filled = plugin::PluginSubmission {
        plugin: "ilium.notes".to_owned(),
        instance: u64::MAX,
        submissions_json: vec!["{\"a\":1}".to_owned(), "{\"b\":2}".to_owned()],
    };
    let decoded: plugin::PluginSubmission =
        decode_strict(&encode(&filled)).expect("decode filled submission");
    assert_eq!(decoded, filled);
}

#[test]
fn plugin_submission_malformed_bytes_return_an_error() {
    assert!(decode_strict::<plugin::PluginSubmission>(&[0xFF; 3]).is_err());
}

#[test]
fn plugin_outcome_round_trips_success_and_failure_records() {
    let success = plugin::PluginOutcome {
        plugin: "ilium.notes".to_owned(),
        request: 9,
        outcome_json: "{\"ok\":true}".to_owned(),
    };
    let failure = plugin::PluginOutcome {
        plugin: "ilium.notes".to_owned(),
        request: 10,
        outcome_json: "{\"error\":\"denied\"}".to_owned(),
    };
    for outcome in [success, failure] {
        let decoded: plugin::PluginOutcome =
            decode_strict(&encode(&outcome)).expect("decode outcome");
        assert_eq!(decoded, outcome);
    }
}

#[test]
fn plugin_outcome_empty_outcome_json_round_trips() {
    let outcome = plugin::PluginOutcome {
        plugin: String::new(),
        request: 0,
        outcome_json: String::new(),
    };
    let decoded: plugin::PluginOutcome = decode_strict(&encode(&outcome)).expect("decode outcome");
    assert_eq!(decoded, outcome);
}

#[test]
fn plugin_outcome_malformed_bytes_return_an_error() {
    assert!(decode_strict::<plugin::PluginOutcome>(&[0xFF; 3]).is_err());
}

#[test]
fn plugin_cli_request_round_trips_zero_arguments() {
    let request = plugin::PluginCliRequest { args: Vec::new() };
    let decoded: plugin::PluginCliRequest =
        decode_strict(&encode(&request)).expect("decode no arguments");
    assert_eq!(decoded, request);
}

#[test]
fn plugin_cli_request_preserves_argument_order_and_empty_arguments() {
    let request = plugin::PluginCliRequest {
        args: vec![
            "install".to_owned(),
            String::new(),
            "--force".to_owned(),
            "ilium.notes".to_owned(),
        ],
    };
    let decoded: plugin::PluginCliRequest =
        decode_strict(&encode(&request)).expect("decode arguments");
    assert_eq!(decoded.args, request.args);
}

#[test]
fn plugin_cli_request_malformed_bytes_return_an_error() {
    assert!(decode_strict::<plugin::PluginCliRequest>(&[0xFF; 3]).is_err());
}
