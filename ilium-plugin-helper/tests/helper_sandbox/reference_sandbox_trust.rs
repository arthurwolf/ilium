//! reference-sandbox.md "Dispatcher protocol trust" (SEC-050 to SEC-054).

use crate::support::*;
use std::time::{Duration, Instant};

const PAGE: &str = "reference-sandbox";

// ------------------------------------------------------------------ SEC-050

/// REQ: SEC-050
#[test]
fn sec_050_helper_never_names_another_plugin_in_what_it_sends() {
    let wasm = require_fixture("hostile-suite");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("plugin-a", &wasm), Ok(()));
    assert!(helper.send(&msg_call_export(
        1,
        "send-as-other-plugin",
        b"plugin-b",
        2_000
    )));
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut seen = 0;
    while Instant::now() < deadline {
        match helper.recv(300) {
            Recv::Frame(frame) => {
                seen += 1;
                assert!(
                    !frame.windows(8).any(|w| w == b"plugin-b"),
                    "a frame names the other plugin"
                );
                if parse_call_result(&frame).is_some() {
                    break;
                }
            }
            Recv::Timeout => continue,
            Recv::Closed => break,
        }
    }
    assert!(seen > 0);
}

/// REQ: SEC-050
#[test]
fn sec_050_a_message_that_names_another_plugin_is_not_served() {
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready("plugin-a"), Ok(()));
    let fingerprint = helper.hello.clone().expect("hello").fingerprint;
    helper.send(&msg_instantiate(
        "plugin-b",
        &fingerprint,
        EMPTY_COMPONENT,
        &[],
    ));
    assert!(helper.recv_tag(tag::READY, 1_000).is_none());
}

/// REQ: SEC-050
#[test]
fn doc_sec_050_identity_comes_from_the_channel_and_foreign_ids_count_as_failures() {
    assert_page_has(
        PAGE,
        "The server MUST treat every message from a helper as hostile",
    );
    assert_page_has(
        PAGE,
        "A message that names another plugin id is dropped and counts as a failure",
    );
}

// ------------------------------------------------------------------ SEC-051

/// REQ: SEC-051
#[test]
fn sec_051_no_frame_from_the_helper_exceeds_4_mib() {
    let wasm = require_fixture("ui-stress");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("plugin-a", &wasm), Ok(()));
    assert!(helper.send(&msg_call_export(1, "bulk-10-mib", b"", 5_000)));
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        match helper.recv(500) {
            Recv::Frame(frame) => {
                assert!(frame.len() <= MAX_MESSAGE, "frame of {} bytes", frame.len());
                if parse_call_result(&frame).is_some() {
                    return;
                }
            }
            Recv::Timeout => continue,
            Recv::Closed => break,
        }
    }
    panic!("no CallResult for the bulk call");
}

/// REQ: SEC-051
#[test]
fn sec_051_helper_sends_at_most_5000_messages_per_second() {
    let wasm = require_fixture("hostile-suite");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("plugin-a", &wasm), Ok(()));
    assert!(helper.send(&msg_call_export(1, "flood-messages", b"", 3_000)));
    let window = Instant::now();
    let mut count = 0u32;
    while window.elapsed() < Duration::from_secs(1) {
        match helper.recv(50) {
            Recv::Frame(_) => count += 1,
            Recv::Timeout => continue,
            Recv::Closed => break,
        }
    }
    assert!(count > 0, "the flood fixture sent nothing");
    assert!(count <= 5_000, "{count} messages in one second");
}

/// REQ: SEC-051
#[test]
fn sec_051_inbound_message_over_4_mib_ends_the_helper() {
    let mut helper = Helper::spawn();
    let fingerprint = helper.expect_hello().fingerprint;
    helper.send(&instantiate_of_size(MAX_MESSAGE + 1, &fingerprint));
    assert!(helper.wait_exit(5_000).is_some());
}

/// REQ: SEC-051
#[test]
fn doc_sec_051_limits_are_4_mib_8_mib_terminal_view_and_5000_per_second() {
    assert_page_has(PAGE, "maximum message size of 4 MiB (8 MiB for `terminal-view` frames) and a rate of 5,000 messages per second per helper");
    assert_eq!(MAX_TERMINAL_VIEW_MESSAGE, 2 * MAX_MESSAGE);
}

// ------------------------------------------------------------------ SEC-052

/// REQ: SEC-052
#[test]
fn sec_052_bad_framing_ends_the_helper() {
    let mut helper = Helper::spawn();
    helper.expect_hello();
    helper.send_raw(&[0xFF, 0xFF, 0xFF, 0xFF]);
    assert!(helper.wait_exit(3_000).is_some());
}

/// REQ: SEC-052
#[test]
fn sec_052_invalid_utf8_in_a_string_field_ends_the_helper() {
    let mut helper = Helper::spawn();
    helper.expect_hello();
    let mut body = Enc::new(tag::SHUTDOWN).done();
    body.extend_from_slice(&2u64.to_le_bytes());
    body.extend_from_slice(&[0xFF, 0xFE]);
    helper.send(&body);
    assert!(helper.wait_exit(3_000).is_some());
}

/// REQ: SEC-052
#[test]
fn sec_052_schema_violation_ends_the_helper() {
    let mut helper = Helper::spawn();
    helper.expect_hello();
    helper.send(&Enc::new(tag::CALL_EXPORT).u8(1).done());
    assert!(helper.wait_exit(3_000).is_some());
}

/// REQ: SEC-052
#[test]
fn sec_052_unknown_message_kind_ends_the_helper() {
    let mut helper = Helper::spawn();
    helper.expect_hello();
    helper.send(&Enc::new(tag::UNKNOWN).done());
    assert!(helper.wait_exit(3_000).is_some());
}

/// REQ: SEC-052
#[test]
fn sec_052_out_of_range_resource_handle_ends_the_helper() {
    let wasm = require_fixture("hostile-suite");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("plugin-a", &wasm), Ok(()));
    assert!(helper.send(&msg_call_export(1, "use-resource", b"", 2_000)));
    let frame = helper
        .recv_tag(tag::HOST_CALL, 3_000)
        .expect("fixture forwards a resource call");
    let call = parse_host_call(&frame).expect("decodes");
    // The reply names a resource handle far outside the table the host handed out.
    helper.send(&msg_host_reply(call.call_id, &u64::MAX.to_le_bytes(), None));
    // SEC-052: an out-of-range resource handle kills the helper.
    assert!(
        helper.wait_exit(3_000).is_some(),
        "helper must end on an out-of-range handle"
    );
}

/// REQ: SEC-052
#[test]
fn doc_sec_052_malformed_message_kills_the_helper_and_counts_as_a_failure() {
    assert_page_has(PAGE, "(bad framing, bad UTF-8, a schema violation, an unknown message kind, an out-of-range resource handle) kills the helper");
    assert_page_has(PAGE, "The server never tries to repair the message");
}

// ------------------------------------------------------------------ SEC-053

const WASMTIME_ROWS: [&str; 7] = [
    "| Linear-memory guard pages | On, 2 GiB guard region on 64-bit |",
    "| Spectre mitigations | On (bounds-check hardening for table and memory access) |",
    "| Epoch interruption | On, deadlines set per call; a missed deadline traps |",
    "| `wasi:sockets`, `wasi:http` imports | Refused at install, and checked again at load; a component that imports them is never instantiated |",
    "| `wasi:clocks`, `wasi:random` | Virtualised: clocks coarsened ([SEC-370]), random from the host CSPRNG |",
    "| Wasmtime version | Pinned exactly in the release, recorded in the build metadata,",
    "| Memory and table maximums | From the manifest `limits`, clamped to the tier ceiling |",
];

/// REQ: SEC-053
#[test]
fn doc_sec_053_wasmtime_configuration_rows() {
    for row in WASMTIME_ROWS {
        assert_page_has(PAGE, row);
    }
}

/// REQ: SEC-053
#[test]
fn sec_053_component_importing_wasi_sockets_is_not_instantiated() {
    let wasm = require_fixture("imports-wasi-sockets");
    let mut helper = Helper::spawn();
    assert_eq!(
        helper.ready_with("plugin-a", &wasm),
        Err("component.wasi-forbidden".to_string())
    );
}

/// REQ: SEC-053
#[test]
fn sec_053_component_importing_wasi_http_is_not_instantiated() {
    let wasm = require_fixture("imports-wasi-http");
    let mut helper = Helper::spawn();
    assert_eq!(
        helper.ready_with("plugin-a", &wasm),
        Err("component.wasi-forbidden".to_string())
    );
}

/// REQ: SEC-053
#[test]
fn sec_053_epoch_interruption_traps_a_missed_deadline() {
    let wasm = require_fixture("fuel-burn");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("plugin-a", &wasm), Ok(()));
    let (result, _) = run_call(&mut helper, 1, "spin", b"", 50, 2_000, |_| {
        (Vec::new(), None)
    });
    assert_eq!(
        result.expect("call result").trap.as_deref(),
        Some("host.deadline")
    );
}

/// REQ: SEC-053
#[test]
fn sec_053_out_of_bounds_memory_access_traps_and_does_not_end_the_helper() {
    let wasm = require_fixture("hostile-suite");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("plugin-a", &wasm), Ok(()));
    let (result, _) = run_call(
        &mut helper,
        1,
        "read-out-of-bounds",
        b"",
        2_000,
        3_000,
        |_| (Vec::new(), None),
    );
    assert!(result.expect("call result").trap.is_some());
    assert!(helper.heartbeat_echo(2, 2_000));
}

// ------------------------------------------------------------------ SEC-054

/// REQ: SEC-054
#[test]
fn doc_sec_054_cache_is_owner_only_keyed_and_integrity_checked() {
    assert_page_has(
        PAGE,
        "MUST NOT load a precompiled artefact that was written by a process it cannot verify",
    );
    assert_page_has(
        PAGE,
        "keyed by the SHA-256 of the component and the Wasmtime version",
    );
    assert_page_has(PAGE, "an HMAC under a per-install key held by the server");
    assert_page_has(PAGE, "No plugin scope can reach it");
}

/// REQ: SEC-054
#[test]
fn doc_sec_054_failed_check_recompiles_and_policy_key_disables_the_cache() {
    assert_page_has(PAGE, "When the check fails the module is recompiled");
    assert_page_has(PAGE, "`[packages] precompiled_cache = false`");
}

/// REQ: SEC-054
#[test]
fn sec_054_helper_instantiates_a_component_from_bytes_without_a_package_store_path() {
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready("plugin-a"), Ok(()));
    let open_paths: Vec<_> = helper
        .fd_links()
        .into_iter()
        .filter(|(fd, t)| *fd > 2 && t.starts_with('/'))
        .collect();
    assert!(open_paths.is_empty(), "{open_paths:?}");
}
