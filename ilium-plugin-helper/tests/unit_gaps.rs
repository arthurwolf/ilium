//! Unit gap tests for `ilium-plugin-helper`: protocol types, serve dispatch and framing, sandbox
//! policy handling, forwarded-import name checks, and the helper configuration.
//!
//! Wire layout used by the serve tests: each frame is a `u32` little-endian length followed by
//! a body. The body follows bincode-1 fixed-width encoding (enum variant index as `u32`
//! little-endian, byte sequences as `u64` little-endian length plus bytes), as
//! reference-host-helper.md describes. The crate has no bincode dependency and no public
//! encoder, so the encoders below are local to this file.
//!
//! Sandboxed cases re-run this test binary as a child process, because `sandbox::apply` cannot
//! be undone in the process that calls it.

use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use ilium_plugin_helper::forward::forward_import;
use ilium_plugin_helper::protocol::{HelperReply, HelperRequest, Ready, HELPER_PROTOCOL_VERSION};
use ilium_plugin_helper::sandbox::{apply, SandboxPolicy};
use ilium_plugin_helper::{serve, HelperConfig, HelperError};
use serde::{de::DeserializeOwned, Serialize};

/// The per-message cap of the helper channel (reference-host-helper.md, HST-155).
const MESSAGE_CAP_BYTES: usize = 4 * 1024 * 1024;
/// Body bytes of a `Call` around its payload when the plugin id is `p` and the export is `e`:
/// variant tag 4, plugin 8 + 1, export 8 + 1, payload length 8.
const CALL_FIXED_OVERHEAD_BYTES: usize = 30;

const TAG_LOAD: u32 = 0;
const TAG_CALL: u32 = 1;
const TAG_UNLOAD: u32 = 2;
const TAG_SHUTDOWN: u32 = 3;

const REPLY_TAG_OK: u32 = 0;
const REPLY_TAG_FAILED: u32 = 1;
const REPLY_TAG_FORWARD_IMPORT: u32 = 2;

const SCENARIO_ENV: &str = "ILIUM_PLUGIN_HELPER_GAPS_SCENARIO";
const PORT_ENV: &str = "ILIUM_PLUGIN_HELPER_GAPS_PORT";
const CHILD_TEST_NAME: &str = "sandbox_child_scenario";
const CHILD_OK_MARKER: &str = "GAPS_CHILD_OK";

// ---------------------------------------------------------------------------------------------
// Local wire helpers
// ---------------------------------------------------------------------------------------------

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_byte_sequence(bytes: &mut Vec<u8>, value: &[u8]) {
    let length = u64::try_from(value.len()).expect("length fits in u64");
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(value);
}

fn frame(body: &[u8]) -> Vec<u8> {
    let length = u32::try_from(body.len()).expect("test body fits in u32");
    let mut bytes = length.to_le_bytes().to_vec();
    bytes.extend_from_slice(body);
    bytes
}

fn load_body(plugin: &str, component: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    push_u32(&mut body, TAG_LOAD);
    push_byte_sequence(&mut body, plugin.as_bytes());
    push_byte_sequence(&mut body, component);
    body
}

fn call_body(plugin: &str, export: &str, payload: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    push_u32(&mut body, TAG_CALL);
    push_byte_sequence(&mut body, plugin.as_bytes());
    push_byte_sequence(&mut body, export.as_bytes());
    push_byte_sequence(&mut body, payload);
    body
}

fn unload_body(plugin: &str) -> Vec<u8> {
    let mut body = Vec::new();
    push_u32(&mut body, TAG_UNLOAD);
    push_byte_sequence(&mut body, plugin.as_bytes());
    body
}

fn shutdown_body() -> Vec<u8> {
    let mut body = Vec::new();
    push_u32(&mut body, TAG_SHUTDOWN);
    body
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
}

/// Splits serve output into frame bodies.
fn split_frames(output: &[u8]) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    let mut position = 0usize;
    while position < output.len() {
        let length = usize::try_from(u32_at(output, position)).expect("frame length fits usize");
        let body_start = position + 4;
        let body_end = body_start + length;
        assert!(
            body_end <= output.len(),
            "serve output ends inside a frame body"
        );
        frames.push(output[body_start..body_end].to_vec());
        position = body_end;
    }
    frames
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> &'a [u8] {
        let slice = &self.bytes[self.position..self.position + count];
        self.position += count;
        slice
    }

    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take(4).try_into().expect("four bytes"))
    }

    fn byte_sequence(&mut self) -> Vec<u8> {
        let length = u64::from_le_bytes(self.take(8).try_into().expect("eight bytes"));
        let length = usize::try_from(length).expect("length fits usize");
        self.take(length).to_vec()
    }

    fn text(&mut self) -> String {
        String::from_utf8(self.byte_sequence()).expect("reply text is UTF-8")
    }
}

fn decode_reply(body: &[u8]) -> HelperReply {
    let mut cursor = Cursor {
        bytes: body,
        position: 0,
    };
    match cursor.u32() {
        REPLY_TAG_OK => HelperReply::Ok {
            payload: cursor.byte_sequence(),
        },
        REPLY_TAG_FAILED => HelperReply::Failed {
            code: cursor.text(),
            message: cursor.text(),
        },
        REPLY_TAG_FORWARD_IMPORT => HelperReply::ForwardImport {
            interface: cursor.text(),
            function: cursor.text(),
            payload: cursor.byte_sequence(),
        },
        other => panic!("unknown reply variant tag {other}"),
    }
}

/// Reads serve output: the first frame is `Ready` (8 bytes), the rest are replies.
fn ready_and_replies(output: &[u8]) -> (Ready, Vec<HelperReply>) {
    let frames = split_frames(output);
    let (first, rest) = frames
        .split_first()
        .expect("serve must write a Ready frame first");
    assert_eq!(first.len(), 8, "Ready body is two u32 fields");
    let ready = Ready {
        protocol_version: u32_at(first, 0),
        helper_process_id: u32_at(first, 4),
    };
    let replies = rest.iter().map(|body| decode_reply(body)).collect();
    (ready, replies)
}

fn default_config() -> HelperConfig {
    HelperConfig {
        epoch_tick_ms: 10,
        apply_sandbox: false,
    }
}

/// Runs `serve` in this process over the given request bytes and returns its result and output.
fn serve_input(input: &[u8]) -> (Result<(), HelperError>, Vec<u8>) {
    let mut output: Vec<u8> = Vec::new();
    let result = serve(input, &mut output, default_config());
    (result, output)
}

fn assert_serde_round_trip<T>(value: &T, label: &str)
where
    T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let encoded = serde_json::to_string(value).expect("value encodes");
    let decoded: T = serde_json::from_str(&encoded).expect("encoded value decodes");
    assert_eq!(&decoded, value, "{label} changed across a serde round trip");
}

// ---------------------------------------------------------------------------------------------
// protocol.rs: Ready
// ---------------------------------------------------------------------------------------------

#[test]
fn ready_serde_round_trip_preserves_both_fields() {
    let ready = Ready {
        protocol_version: HELPER_PROTOCOL_VERSION,
        helper_process_id: 4242,
    };
    assert_serde_round_trip(&ready, "Ready");
    let encoded = serde_json::to_string(&ready).expect("Ready encodes");
    let decoded: Ready = serde_json::from_str(&encoded).expect("Ready decodes");
    assert_eq!(
        decoded.protocol_version, 1,
        "protocol version must survive the round trip"
    );
    assert_eq!(
        decoded.helper_process_id, 4242,
        "process id must survive the round trip"
    );
}

#[test]
fn ready_frame_carries_protocol_version_one() {
    let (_result, output) = serve_input(&[]);
    let (ready, _replies) = ready_and_replies(&output);
    assert_eq!(
        ready.protocol_version, HELPER_PROTOCOL_VERSION,
        "the helper must announce its protocol version"
    );
    assert_eq!(
        ready.protocol_version, 1,
        "the documented framing version is 1"
    );
}

#[test]
fn ready_frame_carries_nonzero_helper_process_id_of_this_process() {
    // serve runs in this process, so the helper's process id is this test process's id.
    let (_result, output) = serve_input(&[]);
    let (ready, _replies) = ready_and_replies(&output);
    assert_ne!(
        ready.helper_process_id, 0,
        "the helper process id must be nonzero"
    );
    assert_eq!(
        ready.helper_process_id,
        std::process::id(),
        "the helper process id must be the id of the spawned helper process"
    );
}

#[test]
fn ready_protocol_version_differs_from_zero_and_max_so_a_mismatch_is_detectable() {
    assert_ne!(
        HELPER_PROTOCOL_VERSION, 0,
        "version 0 must never be a valid framing version"
    );
    assert_ne!(
        HELPER_PROTOCOL_VERSION,
        u32::MAX,
        "the u32::MAX version must never be a valid framing version"
    );
    let zero = Ready {
        protocol_version: 0,
        helper_process_id: 1,
    };
    let current = Ready {
        protocol_version: HELPER_PROTOCOL_VERSION,
        helper_process_id: 1,
    };
    assert_ne!(
        zero, current,
        "a version 0 Ready must not compare equal to the current one"
    );
}

// ---------------------------------------------------------------------------------------------
// protocol.rs: HelperRequest::Load
// ---------------------------------------------------------------------------------------------

#[test]
fn load_serde_round_trip_with_component() {
    let request = HelperRequest::Load {
        plugin: "ilium.example".to_owned(),
        component: vec![0x00, 0x61, 0x73, 0x6d, 0xff],
    };
    assert_serde_round_trip(&request, "HelperRequest::Load");
}

#[test]
fn load_serde_round_trip_with_empty_component_is_accepted_as_given() {
    let request = HelperRequest::Load {
        plugin: "ilium.example".to_owned(),
        component: Vec::new(),
    };
    assert_serde_round_trip(&request, "HelperRequest::Load with an empty component");
}

#[test]
fn load_serve_empty_plugin_id_is_protocol_violation() {
    let input = frame(&load_body("", &[1, 2, 3]));
    let (result, _output) = serve_input(&input);
    assert!(
        matches!(result, Err(HelperError::Protocol(_))),
        "an empty plugin id must end serve as a protocol violation, got {result:?}"
    );
}

#[test]
fn load_serve_component_over_message_cap_is_rejected() {
    let component = vec![0u8; MESSAGE_CAP_BYTES];
    let body = load_body("p", &component);
    assert!(
        body.len() > MESSAGE_CAP_BYTES,
        "the test body must exceed the cap"
    );
    let (result, _output) = serve_input(&frame(&body));
    assert!(
        matches!(result, Err(HelperError::Protocol(_))),
        "a component larger than the message cap must be a protocol violation, got {result:?}"
    );
}

#[test]
fn load_serve_empty_component_gets_one_reply_and_no_protocol_error() {
    let input = frame(&load_body("p", &[]));
    let (result, output) = serve_input(&input);
    assert!(
        result.is_ok(),
        "a zero-length component must not end serve, got {result:?}"
    );
    let (_ready, replies) = ready_and_replies(&output);
    assert_eq!(replies.len(), 1, "a Load must produce exactly one reply");
}

#[test]
fn load_serve_produces_exactly_one_reply() {
    let input = frame(&load_body("p", &[1, 2, 3]));
    let (result, output) = serve_input(&input);
    assert!(
        result.is_ok(),
        "a Load followed by EOF must end serve with Ok, got {result:?}"
    );
    let (_ready, replies) = ready_and_replies(&output);
    assert_eq!(
        replies.len(),
        1,
        "each Load must produce exactly one HelperReply"
    );
}

// ---------------------------------------------------------------------------------------------
// protocol.rs: HelperRequest::Call
// ---------------------------------------------------------------------------------------------

#[test]
fn call_serde_round_trip_with_payload() {
    let request = HelperRequest::Call {
        plugin: "ilium.example".to_owned(),
        export: "activate".to_owned(),
        payload: vec![7, 0, 255],
    };
    assert_serde_round_trip(&request, "HelperRequest::Call");
}

#[test]
fn call_serde_round_trip_with_empty_payload() {
    let request = HelperRequest::Call {
        plugin: "ilium.example".to_owned(),
        export: "activate".to_owned(),
        payload: Vec::new(),
    };
    assert_serde_round_trip(&request, "HelperRequest::Call with an empty payload");
}

#[test]
fn call_serve_empty_export_name_is_protocol_violation() {
    let input = frame(&call_body("p", "", &[]));
    let (result, _output) = serve_input(&input);
    assert!(
        matches!(result, Err(HelperError::Protocol(_))),
        "an empty export name must end serve as a protocol violation, got {result:?}"
    );
}

#[test]
fn call_serve_for_never_loaded_plugin_returns_namespaced_failed_reply() {
    let input = frame(&call_body("p", "e", &[]));
    let (result, output) = serve_input(&input);
    assert!(
        result.is_ok(),
        "a Call for an unloaded plugin must not end serve, got {result:?}"
    );
    let (_ready, replies) = ready_and_replies(&output);
    assert_eq!(replies.len(), 1, "the Call must produce exactly one reply");
    match &replies[0] {
        HelperReply::Failed { code, message } => {
            assert!(!code.is_empty(), "the failure code must not be empty");
            assert!(
                code.contains('.'),
                "the failure code must be namespaced, got {code}"
            );
            assert!(!message.is_empty(), "the failure must carry a message");
        }
        other => panic!("a Call for an unloaded plugin must reply Failed, got {other:?}"),
    }
}

#[test]
fn call_serve_payload_at_exact_message_cap_is_served() {
    let payload = vec![0u8; MESSAGE_CAP_BYTES - CALL_FIXED_OVERHEAD_BYTES];
    let body = call_body("p", "e", &payload);
    assert_eq!(
        body.len(),
        MESSAGE_CAP_BYTES,
        "the test body must be exactly the cap"
    );
    let (result, output) = serve_input(&frame(&body));
    assert!(
        result.is_ok(),
        "a body exactly at the cap must be served, got {result:?}"
    );
    let (_ready, replies) = ready_and_replies(&output);
    assert_eq!(
        replies.len(),
        1,
        "a capped Call must produce exactly one reply"
    );
    assert!(
        matches!(replies.first(), Some(HelperReply::Failed { .. })),
        "the capped Call targets an unloaded plugin, so it must reply Failed"
    );
}

#[test]
fn call_serve_payload_one_byte_over_message_cap_is_rejected() {
    let payload = vec![0u8; MESSAGE_CAP_BYTES - CALL_FIXED_OVERHEAD_BYTES + 1];
    let body = call_body("p", "e", &payload);
    assert_eq!(
        body.len(),
        MESSAGE_CAP_BYTES + 1,
        "the test body must be one byte over the cap"
    );
    let (result, _output) = serve_input(&frame(&body));
    assert!(
        matches!(result, Err(HelperError::Protocol(_))),
        "a body one byte over the cap must be a protocol violation, got {result:?}"
    );
}

#[test]
fn call_serve_empty_payload_gets_exactly_one_reply() {
    let input = frame(&call_body("p", "e", &[]));
    let (_result, output) = serve_input(&input);
    let (_ready, replies) = ready_and_replies(&output);
    assert_eq!(
        replies.len(),
        1,
        "a Call with an empty payload must produce one reply"
    );
}

// ---------------------------------------------------------------------------------------------
// protocol.rs: HelperRequest::Unload and HelperRequest::Shutdown
// ---------------------------------------------------------------------------------------------

#[test]
fn unload_serde_round_trip() {
    let request = HelperRequest::Unload {
        plugin: "ilium.example".to_owned(),
    };
    assert_serde_round_trip(&request, "HelperRequest::Unload");
}

#[test]
fn unload_serve_of_unknown_plugin_is_a_defined_outcome() {
    let input = frame(&unload_body("never-loaded"));
    let (result, output) = serve_input(&input);
    assert!(
        result.is_ok(),
        "unloading an unknown plugin must not end serve with an error, got {result:?}"
    );
    let (_ready, replies) = ready_and_replies(&output);
    assert_eq!(replies.len(), 1, "an Unload must produce exactly one reply");
}

#[test]
fn shutdown_serde_round_trip() {
    assert_serde_round_trip(&HelperRequest::Shutdown, "HelperRequest::Shutdown");
}

#[test]
fn shutdown_serve_ends_with_ok_after_flushing_the_earlier_reply() {
    let mut input = frame(&call_body("p", "e", &[]));
    input.extend(frame(&shutdown_body()));
    let (result, output) = serve_input(&input);
    assert!(
        result.is_ok(),
        "Shutdown must end serve with Ok(()), got {result:?}"
    );
    let (_ready, replies) = ready_and_replies(&output);
    assert_eq!(
        replies.len(),
        1,
        "the reply to the request before Shutdown must be flushed"
    );
    assert!(
        matches!(replies.first(), Some(HelperReply::Failed { .. })),
        "the flushed reply must be the reply to the earlier Call"
    );
}

#[test]
fn requests_after_shutdown_are_not_served() {
    let mut input = frame(&shutdown_body());
    input.extend(frame(&call_body("p", "e", &[])));
    let (result, output) = serve_input(&input);
    assert!(
        result.is_ok(),
        "Shutdown must end serve with Ok(()), got {result:?}"
    );
    let (_ready, replies) = ready_and_replies(&output);
    assert!(
        replies.is_empty(),
        "a request after Shutdown must not be served, got {replies:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// protocol.rs: HelperReply
// ---------------------------------------------------------------------------------------------

#[test]
fn reply_ok_serde_round_trip_with_empty_payload() {
    let reply = HelperReply::Ok {
        payload: Vec::new(),
    };
    assert_serde_round_trip(&reply, "HelperReply::Ok with an empty payload");
}

#[test]
fn reply_ok_serde_round_trip_keeps_payload_bytes_unchanged() {
    let original = vec![0x00u8, 0xFF, 0x80, 0x7F, 0x01, 0x00];
    let reply = HelperReply::Ok {
        payload: original.clone(),
    };
    assert_serde_round_trip(&reply, "HelperReply::Ok with binary payload");
    let encoded = serde_json::to_string(&reply).expect("reply encodes");
    let decoded: HelperReply = serde_json::from_str(&encoded).expect("reply decodes");
    match decoded {
        HelperReply::Ok { payload } => {
            assert_eq!(
                payload, original,
                "the encoded result must come back byte for byte"
            )
        }
        other => panic!("expected Ok, got {other:?}"),
    }
}

#[test]
fn reply_failed_serde_round_trip_preserves_namespaced_code() {
    let reply = HelperReply::Failed {
        code: "trap.deadline".to_owned(),
        message: "the call exceeded its deadline".to_owned(),
    };
    assert_serde_round_trip(&reply, "HelperReply::Failed");
    let encoded = serde_json::to_string(&reply).expect("reply encodes");
    let decoded: HelperReply = serde_json::from_str(&encoded).expect("reply decodes");
    match decoded {
        HelperReply::Failed { code, .. } => {
            assert_eq!(
                code, "trap.deadline",
                "the namespaced code must be preserved verbatim"
            )
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn reply_failed_serde_round_trip_keeps_empty_code_and_message_as_given() {
    let reply = HelperReply::Failed {
        code: String::new(),
        message: String::new(),
    };
    assert_serde_round_trip(&reply, "HelperReply::Failed with an empty code and message");
}

#[test]
fn reply_forward_import_serde_round_trip() {
    let reply = HelperReply::ForwardImport {
        interface: "ilium:storage".to_owned(),
        function: "get".to_owned(),
        payload: b"settings/theme".to_vec(),
    };
    assert_serde_round_trip(&reply, "HelperReply::ForwardImport");
}

#[test]
fn forward_import_empty_interface_name_is_protocol_violation() {
    let outcome = forward_import("", "get", b"");
    assert!(
        matches!(outcome, Err(HelperError::Protocol(_))),
        "an empty interface name must be a protocol violation, got {outcome:?}"
    );
}

#[test]
fn forward_import_empty_function_name_is_protocol_violation() {
    let outcome = forward_import("ilium:storage", "", b"");
    assert!(
        matches!(outcome, Err(HelperError::Protocol(_))),
        "an empty function name must be a protocol violation, got {outcome:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// lib.rs: serve framing and end-of-channel handling
// ---------------------------------------------------------------------------------------------

#[test]
fn serve_returns_ok_on_eof_between_frames() {
    let input = frame(&unload_body("never-loaded"));
    let (result, output) = serve_input(&input);
    assert!(
        result.is_ok(),
        "EOF between frames must end serve with Ok, got {result:?}"
    );
    let (_ready, replies) = ready_and_replies(&output);
    assert_eq!(
        replies.len(),
        1,
        "the complete frame before EOF must still be answered"
    );
}

#[test]
fn serve_fails_with_channel_or_protocol_error_on_eof_mid_frame() {
    // The length prefix announces 16 body bytes, but only 3 follow before EOF.
    let input = [16u8, 0, 0, 0, 1, 2, 3];
    let (result, _output) = serve_input(&input);
    assert!(
        matches!(
            result,
            Err(HelperError::Channel(_)) | Err(HelperError::Protocol(_))
        ),
        "EOF in the middle of a frame must be a Channel or Protocol error, got {result:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// lib.rs: HelperConfig
// ---------------------------------------------------------------------------------------------

#[test]
fn epoch_tick_zero_is_rejected_or_defined_without_panic() {
    let mut output: Vec<u8> = Vec::new();
    let config = HelperConfig {
        epoch_tick_ms: 0,
        apply_sandbox: false,
    };
    let result = serve(std::io::empty(), &mut output, config);
    assert!(
        matches!(result, Ok(()) | Err(HelperError::Protocol(_))),
        "epoch_tick_ms 0 must be a defined setting or a Protocol error, got {result:?}"
    );
}

#[test]
fn epoch_tick_u64_max_does_not_overflow_serve() {
    let mut output: Vec<u8> = Vec::new();
    let config = HelperConfig {
        epoch_tick_ms: u64::MAX,
        apply_sandbox: false,
    };
    let result = serve(std::io::empty(), &mut output, config);
    assert!(
        result.is_ok(),
        "epoch_tick_ms u64::MAX must not overflow serve, got {result:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// Sandboxed cases: each scenario runs in a child process of this test binary.
// ---------------------------------------------------------------------------------------------

/// Writer that records whether `no_new_privs` was set when the first byte was written.
#[derive(Default)]
struct RecordingWriter {
    buffer: Vec<u8>,
    no_new_privs_at_first_write: Option<bool>,
}

impl Write for RecordingWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.no_new_privs_at_first_write.is_none() {
            self.no_new_privs_at_first_write = Some(no_new_privs_enabled());
        }
        self.buffer.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn no_new_privs_enabled() -> bool {
    let status = std::fs::read_to_string("/proc/self/status").expect("read process status");
    status
        .lines()
        .find(|line| line.starts_with("NoNewPrivs:"))
        .and_then(|line| line.split_whitespace().nth(1))
        == Some("1")
}

fn port_from_environment() -> u16 {
    std::env::var(PORT_ENV)
        .expect("the parent passes the loopback listener port")
        .parse()
        .expect("the port is a u16")
}

/// Runs one sandboxed scenario in a child process and returns its stdout.
fn run_scenario(scenario: &str, extra_env: &[(&str, String)]) -> String {
    let test_binary = std::env::current_exe().expect("current test binary path");
    let mut command = std::process::Command::new(test_binary);
    command
        .args([
            "--exact",
            CHILD_TEST_NAME,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(SCENARIO_ENV, scenario);
    for (name, value) in extra_env {
        command.env(name, value);
    }
    let output = command
        .output()
        .expect("spawn the sandboxed child test process");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success() && stdout.contains(CHILD_OK_MARKER),
        "sandboxed scenario {scenario} failed (status {}); stdout was: {stdout}",
        output.status
    );
    stdout
}

/// Entry point of the child processes. Without the scenario variable it does nothing.
#[test]
fn sandbox_child_scenario() {
    let Ok(scenario) = std::env::var(SCENARIO_ENV) else {
        return;
    };
    match scenario.as_str() {
        "serve_sandboxed" => child_serve_sandboxed(),
        "allow_network" => child_allow_network(),
        "deny_network" => child_deny_network(),
        "relative_path" => child_relative_path(),
        "empty_path" => child_empty_path(),
        "missing_path" => child_missing_path(),
        "symlink_path" => child_symlink_path(),
        "duplicate_paths" => child_duplicate_paths(),
        other => panic!("unknown sandbox scenario {other}"),
    }
    println!("{CHILD_OK_MARKER}");
}

fn child_serve_sandboxed() {
    let mut writer = RecordingWriter::default();
    let config = HelperConfig {
        epoch_tick_ms: 10,
        apply_sandbox: true,
    };
    let result = serve(std::io::empty(), &mut writer, config);
    result.expect("serve with the sandbox enabled must succeed");
    assert_eq!(
        writer.no_new_privs_at_first_write,
        Some(true),
        "serve must apply the sandbox (no_new_privs set) before it writes the Ready frame"
    );
    assert!(
        !writer.buffer.is_empty(),
        "serve must write Ready after applying the sandbox"
    );
}

fn child_allow_network() {
    let policy = SandboxPolicy {
        allow_network: true,
        ..SandboxPolicy::default()
    };
    apply(&policy).expect("apply with network allowed must succeed");
    let loopback = SocketAddr::from(([127, 0, 0, 1], port_from_environment()));
    TcpStream::connect_timeout(&loopback, Duration::from_secs(5))
        .expect("an outbound socket must connect after apply with network allowed");
}

fn child_deny_network() {
    apply(&SandboxPolicy::default()).expect("apply with the default policy must succeed");
    let loopback = SocketAddr::from(([127, 0, 0, 1], port_from_environment()));
    TcpStream::connect_timeout(&loopback, Duration::from_secs(5))
        .expect("loopback must stay usable with network denied");
    // 192.0.2.0/24 is TEST-NET-1, documentation space that is never a loopback address.
    let outside = SocketAddr::from(([192, 0, 2, 1], 9));
    assert!(
        TcpStream::connect_timeout(&outside, Duration::from_secs(2)).is_err(),
        "a connect to a non-loopback address must be blocked when network is denied"
    );
}

fn child_relative_path() {
    let policy = SandboxPolicy {
        read_paths: vec![PathBuf::from("relative/dir")],
        ..SandboxPolicy::default()
    };
    match apply(&policy) {
        Err(HelperError::Sandbox(message)) => assert!(
            message.contains("relative/dir"),
            "the error must name the relative path, got {message}"
        ),
        other => panic!("a relative read path must be a named Sandbox error, got {other:?}"),
    }
}

fn child_empty_path() {
    let policy = SandboxPolicy {
        read_paths: vec![PathBuf::new()],
        ..SandboxPolicy::default()
    };
    match apply(&policy) {
        Err(HelperError::Sandbox(message)) => assert!(
            !message.trim().is_empty(),
            "an empty path entry must produce a named Sandbox error"
        ),
        other => panic!("an empty path entry must be a named Sandbox error, got {other:?}"),
    }
}

fn child_missing_path() {
    let missing = "/ilium-plugin-helper-gaps-missing-directory";
    let policy = SandboxPolicy {
        read_paths: vec![PathBuf::from(missing)],
        ..SandboxPolicy::default()
    };
    match apply(&policy) {
        Err(HelperError::Sandbox(message)) => assert!(
            message.contains(missing),
            "the error must name the nonexistent path, got {message}"
        ),
        other => panic!("a nonexistent path must be a Sandbox error, got {other:?}"),
    }
}

fn child_symlink_path() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("helper-gaps-symlink");
    let target = root.join("target");
    std::fs::create_dir_all(&target).expect("create the symlink target directory");
    std::fs::write(target.join("data.txt"), "granted").expect("write the target file");
    let link = root.join("link");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&target, &link).expect("create the symlink");

    let policy = SandboxPolicy {
        read_paths: vec![link.clone()],
        ..SandboxPolicy::default()
    };
    apply(&policy).expect("apply with a symlinked read path must succeed");
    let through_link =
        std::fs::read_to_string(link.join("data.txt")).expect("read through the granted symlink");
    assert_eq!(
        through_link, "granted",
        "the symlink must grant access to its target"
    );
    let through_target =
        std::fs::read_to_string(target.join("data.txt")).expect("read the resolved target");
    assert_eq!(
        through_target, "granted",
        "the resolved target must be readable"
    );
}

fn child_duplicate_paths() {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join("helper-gaps-duplicates");
    std::fs::create_dir_all(&directory).expect("create the duplicate-path directory");
    let policy = SandboxPolicy {
        read_paths: vec![directory.clone(), directory.clone()],
        write_paths: vec![directory.clone(), directory.clone()],
        allow_network: false,
    };
    apply(&policy).expect("duplicate entries in both lists must be harmless");
}

#[test]
fn serve_with_sandbox_enabled_applies_it_before_writing_ready() {
    run_scenario("serve_sandboxed", &[]);
}

#[test]
fn apply_with_network_allowed_keeps_outbound_sockets_usable() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback listener");
    let port = listener.local_addr().expect("listener address").port();
    run_scenario("allow_network", &[(PORT_ENV, port.to_string())]);
}

#[test]
fn apply_with_network_denied_blocks_non_loopback_connect_but_keeps_loopback() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback listener");
    let port = listener.local_addr().expect("listener address").port();
    run_scenario("deny_network", &[(PORT_ENV, port.to_string())]);
}

#[test]
fn apply_rejects_relative_read_path_with_named_sandbox_error() {
    run_scenario("relative_path", &[]);
}

#[test]
fn apply_rejects_empty_path_entry_with_named_sandbox_error() {
    run_scenario("empty_path", &[]);
}

#[test]
fn apply_nonexistent_path_returns_sandbox_error_naming_the_path() {
    run_scenario("missing_path", &[]);
}

#[test]
fn apply_grants_symlinked_read_path_through_its_target() {
    run_scenario("symlink_path", &[]);
}

#[test]
fn apply_accepts_duplicate_entries_in_both_lists() {
    run_scenario("duplicate_paths", &[]);
}
