//! reference-host-helper.md: the sandbox helper protocol, HST-150 to HST-171.
//!
//! The helper binary is driven over its dispatcher channel. Rules that only the host can
//! apply (counting, charging, caps, restart windows) are checked here through the manual text
//! (`doc_` tests) and, where the helper takes part, through its observable behaviour.

use crate::support::*;
use ilium_plugin_helper::protocol::HELPER_PROTOCOL_VERSION;
use std::time::{Duration, Instant};

const PAGE: &str = "reference-host-helper";

// ------------------------------------------------------------------ HST-150

/// REQ: HST-150
#[test]
fn hst_150_two_plugins_get_two_distinct_helper_processes() {
    let mut first = Helper::spawn();
    let mut second = Helper::spawn();
    assert_ne!(first.pid(), second.pid());
    assert_eq!(first.ready("plugin-a"), Ok(()));
    assert_eq!(second.ready("plugin-b"), Ok(()));
}

/// REQ: HST-150
#[test]
fn hst_150_second_plugin_in_the_same_helper_is_not_instantiated() {
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready("plugin-a"), Ok(()));
    let fingerprint = helper.hello.clone().expect("hello").fingerprint;
    assert!(helper.send(&msg_instantiate("plugin-b", &fingerprint, EMPTY_COMPONENT, &[])));
    assert!(helper.recv_tag(tag::READY, 1_500).is_none(), "a helper must not host two plugins");
}

/// REQ: HST-150
#[test]
fn hst_150_same_plugin_instantiated_twice_is_not_a_second_plugin_but_never_two_stores() {
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready("plugin-a"), Ok(()));
    let fingerprint = helper.hello.clone().expect("hello").fingerprint;
    helper.send(&msg_instantiate("plugin-a", &fingerprint, EMPTY_COMPONENT, &[]));
    // One Store at a time: at most one further Ready may come, never two.
    let mut ready = 0;
    while helper.recv_tag(tag::READY, 500).is_some() {
        ready += 1;
        if ready > 1 {
            break;
        }
    }
    assert!(ready <= 1);
}

/// REQ: HST-150
#[test]
fn doc_hst_150_manual_states_one_helper_per_plugin_side() {
    assert_page_has(PAGE, "One helper per plugin per side per hosting process");
    assert_page_has(PAGE, "MUST NOT host two plugins in one helper");
}

// ------------------------------------------------------------------ HST-151

/// REQ: HST-151
#[test]
fn hst_151_helper_binary_is_named_ilium_plugin_helper() {
    let name = std::path::Path::new(HELPER_BIN).file_name().expect("file name").to_string_lossy().into_owned();
    assert_eq!(name, "ilium-plugin-helper");
    assert!(std::path::Path::new(HELPER_BIN).is_file());
}

/// REQ: HST-151
#[test]
fn hst_151_matching_fingerprint_reaches_ready() {
    let mut helper = Helper::spawn();
    let hello = helper.expect_hello();
    assert!(!hello.fingerprint.is_empty());
    assert_eq!(helper.ready("plugin-a"), Ok(()));
}

/// REQ: HST-151
#[test]
fn hst_151_different_fingerprint_is_refused_and_never_reaches_ready() {
    let mut helper = Helper::spawn();
    let hello = helper.expect_hello();
    let wrong = format!("{}-different", hello.fingerprint);
    assert!(helper.send(&msg_instantiate("plugin-a", &wrong, EMPTY_COMPONENT, &[])));
    assert!(helper.recv_tag(tag::READY, 1_500).is_none());
}

/// REQ: HST-151
#[test]
fn hst_151_two_helpers_report_the_same_runtime_fingerprint() {
    let mut first = Helper::spawn();
    let mut second = Helper::spawn();
    assert_eq!(first.expect_hello().fingerprint, second.expect_hello().fingerprint);
}

// ------------------------------------------------------------------ HST-152

/// REQ: HST-152
#[test]
fn hst_152_helper_sends_only_hello_until_instantiate() {
    let mut helper = Helper::spawn();
    helper.expect_hello();
    assert!(helper.recv_tag(tag::READY, 400).is_none());
    assert!(helper.is_running());
}

/// REQ: HST-152
#[test]
fn hst_152_helper_serves_the_first_activation_after_a_long_wait() {
    let mut helper = Helper::spawn();
    helper.expect_hello();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(helper.ready("plugin-a"), Ok(()));
}

/// REQ: HST-152
#[test]
fn doc_hst_152_manual_states_lazy_start() {
    assert_page_has(PAGE, "| Start | Lazy, at the first activation of the plugin side |");
}

// ------------------------------------------------------------------ HST-153

/// REQ: HST-153
#[test]
fn hst_153_dispatcher_channel_works_over_an_inherited_socketpair() {
    let mut helper = Helper::spawn_socketpair();
    assert_eq!(helper.ready("plugin-a"), Ok(()));
    assert!(helper.heartbeat_echo(1, 2_000));
}

/// REQ: HST-153
#[test]
fn hst_153_helper_holds_no_socket_besides_the_inherited_channel() {
    let mut helper = Helper::spawn_socketpair();
    assert_eq!(helper.ready("plugin-a"), Ok(()));
    let sockets: Vec<(u32, String)> = helper.fd_links().into_iter().filter(|(_, t)| t.starts_with("socket:")).collect();
    assert!(!sockets.is_empty(), "channel socket missing");
    assert!(sockets.iter().all(|(fd, _)| *fd <= 1), "extra sockets: {sockets:?}");
    let inodes: std::collections::BTreeSet<&String> = sockets.iter().map(|(_, t)| t).collect();
    assert_eq!(inodes.len(), 1, "stdin and stdout must be the same channel");
}

/// REQ: HST-153
#[test]
fn hst_153_helper_over_pipes_has_no_socket_at_all() {
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready("plugin-a"), Ok(()));
    assert!(helper.fd_links().iter().all(|(_, t)| !t.starts_with("socket:")));
}

// ------------------------------------------------------------------ HST-154

/// REQ: HST-154
#[test]
fn hst_154_hello_arrives_within_5000_ms_with_the_protocol_version() {
    let mut helper = Helper::spawn();
    let started = Instant::now();
    let hello = helper.expect_hello();
    assert!(started.elapsed() < Duration::from_millis(5_000));
    assert_eq!(hello.protocol_version, HELPER_PROTOCOL_VERSION);
    assert!(!hello.fingerprint.is_empty());
}

/// REQ: HST-154
#[test]
fn hst_154_ready_arrives_within_5000_ms_after_instantiate() {
    let mut helper = Helper::spawn();
    helper.expect_hello();
    let started = Instant::now();
    assert_eq!(helper.ready("plugin-a"), Ok(()));
    assert!(started.elapsed() < Duration::from_millis(5_000));
}

/// REQ: HST-154
#[test]
fn hst_154_wrong_fingerprint_gives_no_ready_so_the_host_sees_spawn_failed() {
    let mut helper = Helper::spawn();
    helper.expect_hello();
    helper.send(&msg_instantiate("plugin-a", "not-the-fingerprint", EMPTY_COMPONENT, &[]));
    assert!(helper.recv_tag(tag::READY, 1_000).is_none());
}

/// REQ: HST-154
#[test]
fn doc_hst_154_manual_states_5000_ms_spawn_failed_and_first_failure_rule() {
    assert_page_has(PAGE, "`Ready` later than 5,000 ms after `Instantiate` as `helper.spawn-failed`");
    assert_page_has(PAGE, "count one failure only when the plugin had already started once");
}

// ------------------------------------------------------------------ HST-155

/// REQ: HST-155
#[test]
fn hst_155_heartbeat_round_trips_through_length_prefixed_frames() {
    let mut helper = Helper::spawn();
    helper.expect_hello();
    for counter in [0u64, 1, 255, 65_536, u64::MAX] {
        assert!(helper.heartbeat_echo(counter, 2_000), "counter {counter}");
    }
}

/// REQ: HST-155
#[test]
fn hst_155_unknown_message_kind_ends_the_helper() {
    let mut helper = Helper::spawn();
    helper.expect_hello();
    let body = Enc::new(tag::UNKNOWN).u64(1).done();
    assert!(helper.send(&body));
    assert!(helper.wait_exit(3_000).is_some(), "helper must end on an unknown kind");
}

/// REQ: HST-155
#[test]
fn hst_155_length_above_the_limit_ends_the_helper() {
    let mut helper = Helper::spawn();
    helper.expect_hello();
    helper.send_raw(&u32::MAX.to_le_bytes());
    assert!(helper.wait_exit(3_000).is_some());
}

/// REQ: HST-155
#[test]
fn hst_155_zero_length_frame_ends_the_helper() {
    let mut helper = Helper::spawn();
    helper.expect_hello();
    helper.send_raw(&0u32.to_le_bytes());
    assert!(helper.wait_exit(3_000).is_some());
}

/// REQ: HST-155
#[test]
fn hst_155_message_of_4_mib_minus_one_is_accepted() {
    let mut helper = Helper::spawn();
    let fingerprint = helper.expect_hello().fingerprint;
    assert!(helper.send(&instantiate_of_size(MAX_MESSAGE - 1, &fingerprint)));
    assert!(helper.recv_tag(tag::READY, 8_000).is_some() || helper.recv_tag(tag::INSTANTIATE_FAILED, 100).is_some());
    assert!(helper.is_running());
}

/// REQ: HST-155
#[test]
fn hst_155_message_of_exactly_4_mib_is_accepted() {
    let mut helper = Helper::spawn();
    let fingerprint = helper.expect_hello().fingerprint;
    assert!(helper.send(&instantiate_of_size(MAX_MESSAGE, &fingerprint)));
    assert!(helper.recv_tag(tag::READY, 8_000).is_some() || helper.recv_tag(tag::INSTANTIATE_FAILED, 100).is_some());
    assert!(helper.is_running());
}

/// REQ: HST-155
#[test]
fn hst_155_message_of_4_mib_plus_one_ends_the_helper() {
    let mut helper = Helper::spawn();
    let fingerprint = helper.expect_hello().fingerprint;
    helper.send(&instantiate_of_size(MAX_MESSAGE + 1, &fingerprint));
    assert!(helper.wait_exit(5_000).is_some());
}

/// REQ: HST-155
#[test]
fn hst_155_truncated_body_followed_by_close_ends_the_helper() {
    let mut helper = Helper::spawn();
    helper.expect_hello();
    let mut bytes = 100u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&[0u8; 10]);
    helper.send_raw(&bytes);
    helper.close_input();
    assert!(helper.wait_exit(3_000).is_some());
}

/// REQ: HST-155
#[test]
fn hst_155_schema_violation_ends_the_helper() {
    let mut helper = Helper::spawn();
    helper.expect_hello();
    // A Heartbeat whose counter is cut to three bytes.
    let body = Enc::new(tag::HEARTBEAT).u8(1).u8(2).u8(3).done();
    helper.send(&body);
    assert!(helper.wait_exit(3_000).is_some());
}

// ------------------------------------------------------------------ HST-156

/// REQ: HST-156
#[test]
fn hst_156_instantiate_naming_another_plugin_is_dropped() {
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready("plugin-a"), Ok(()));
    let fingerprint = helper.hello.clone().expect("hello").fingerprint;
    helper.send(&msg_instantiate("plugin-other", &fingerprint, EMPTY_COMPONENT, &[]));
    assert!(helper.recv_tag(tag::READY, 1_000).is_none());
}

/// REQ: HST-156
#[test]
fn hst_156_helper_keeps_serving_its_own_plugin_after_a_foreign_message() {
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready("plugin-a"), Ok(()));
    let fingerprint = helper.hello.clone().expect("hello").fingerprint;
    helper.send(&msg_instantiate("plugin-other", &fingerprint, EMPTY_COMPONENT, &[]));
    assert!(helper.recv_tag(tag::READY, 300).is_none());
    assert!(helper.is_running(), "a dropped foreign message must not end the helper");
    assert!(helper.heartbeat_echo(7, 2_000), "helper must keep serving its own plugin");
}

/// REQ: HST-156
#[test]
fn doc_hst_156_manual_states_identity_comes_from_the_channel() {
    assert_page_has(PAGE, "derive the plugin identity from the channel and MUST drop any message that names another plugin");
}

// ------------------------------------------------------------------ HST-157

/// REQ: HST-157
#[test]
fn hst_157_host_reply_with_unknown_call_id_is_dropped_and_helper_survives() {
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready("plugin-a"), Ok(()));
    helper.send(&msg_host_reply(9_999, b"x", None));
    assert!(helper.heartbeat_echo(5, 2_000), "helper must keep serving after a stale reply");
}

/// REQ: HST-157
#[test]
fn hst_157_host_reply_for_a_finished_call_is_dropped() {
    let wasm = require_fixture("conformance-probe");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("plugin-a", &wasm), Ok(()));
    let (result, _) = run_call(&mut helper, 1, "ping", b"", 1_000, 3_000, |_| (Vec::new(), None));
    assert!(result.expect("call result").trap.is_none());
    helper.send(&msg_host_reply(1, b"late", None));
    assert!(helper.heartbeat_echo(6, 2_000));
}

/// REQ: HST-157
#[test]
fn hst_157_cancel_for_unknown_call_id_does_not_end_the_helper() {
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready("plugin-a"), Ok(()));
    helper.send(&msg_cancel(424_242));
    assert!(helper.heartbeat_echo(7, 2_000));
}

/// REQ: HST-157
#[test]
fn doc_hst_157_manual_names_the_stale_call_log_code() {
    assert_page_has(PAGE, "logged `helper.stale-call`");
}

// ------------------------------------------------------------------ HST-158

/// REQ: HST-158
#[test]
fn hst_158_helper_holds_only_stdio_and_the_channel() {
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready("plugin-a"), Ok(()));
    let extra: Vec<_> = helper.fd_links().into_iter().filter(|(fd, _)| *fd > 2).collect();
    assert!(extra.is_empty(), "unexpected descriptors: {extra:?}");
}

/// REQ: HST-158
#[test]
fn hst_158_helper_maps_no_shared_memory() {
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready("plugin-a"), Ok(()));
    let maps = std::fs::read_to_string(format!("/proc/{}/maps", helper.pid())).expect("helper maps file readable");
    assert!(!maps.is_empty(), "maps file must list the helper's own mappings");
    assert!(!maps.contains("/dev/shm"));
    assert!(!maps.contains("memfd:"));
    assert!(!maps.contains("/SYSV"));
}

/// REQ: HST-158
#[test]
fn hst_158_bulk_result_of_10_mib_arrives_in_messages_of_at_most_4_mib() {
    let wasm = require_fixture("ui-stress");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("plugin-a", &wasm), Ok(()));
    assert!(helper.send(&msg_call_export(1, "bulk-10-mib", b"", 5_000)));
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut frames = 0;
    while Instant::now() < deadline {
        match helper.recv(500) {
            Recv::Frame(frame) => {
                assert!(frame.len() <= MAX_MESSAGE);
                frames += 1;
                if parse_call_result(&frame).is_some() {
                    break;
                }
            }
            Recv::Timeout => continue,
            Recv::Closed => break,
        }
    }
    assert!(frames >= 3, "10 MiB need at least three chunks, saw {frames}");
}

// ------------------------------------------------------------------ HST-159

const PURE_IMPORTS: [&str; 5] = ["codec", "vector", "clock.now", "random", "log"];
const FORWARDED_IMPORTS: [&str; 10] =
    ["storage", "fs", "http", "process", "ui", "messages", "timers", "jobs", "inference", "clipboard"];

/// REQ: HST-159
#[test]
fn hst_159_pure_compute_imports_run_without_a_round_trip() {
    let wasm = require_fixture("grant-matrix");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("plugin-a", &wasm), Ok(()));
    for name in PURE_IMPORTS {
        let export = format!("import-{name}");
        let (result, imports) = run_call(&mut helper, 1, &export, b"", 2_000, 3_000, |_| (Vec::new(), None));
        assert!(result.is_some(), "{name}: no result");
        assert!(imports.is_empty(), "{name} was forwarded: {imports:?}");
    }
}

/// REQ: HST-159
#[test]
fn hst_159_every_other_import_is_forwarded_to_the_host() {
    let wasm = require_fixture("grant-matrix");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("plugin-a", &wasm), Ok(()));
    for name in FORWARDED_IMPORTS {
        let export = format!("import-{name}");
        let (result, imports) = run_call(&mut helper, 1, &export, b"", 2_000, 3_000, |_| (Vec::new(), None));
        assert!(result.is_some(), "{name}: no result");
        assert!(imports.iter().any(|i| i.contains(name)), "{name} not forwarded: {imports:?}");
    }
}

/// REQ: HST-159
#[test]
fn hst_159_helper_never_decides_a_grant_the_host_refusal_reaches_the_guest() {
    let wasm = require_fixture("grant-matrix");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("plugin-a", &wasm), Ok(()));
    let (result, imports) = run_call(&mut helper, 1, "import-storage", b"", 2_000, 3_000, |_| {
        (Vec::new(), Some("not-permitted".to_string()))
    });
    assert!(!imports.is_empty());
    let result = result.expect("call result");
    assert!(String::from_utf8_lossy(&result.result).contains("not-permitted") || result.trap.is_some());
}

// ------------------------------------------------------------------ HST-160

/// REQ: HST-160, B18
#[test]
#[ignore = "budget: run on the reference machine"]
fn hst_160_forwarded_import_round_trip_median_0_2_ms_p99_0_5_ms() {
    const MEDIAN_BUDGET_NANOS: u128 = 200_000;
    const P99_BUDGET_NANOS: u128 = 500_000;
    let wasm = require_fixture("conformance-probe");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("plugin-a", &wasm), Ok(()));
    let mut samples: Vec<u128> = Vec::new();
    let mut last = Instant::now();
    let (result, _) = run_call(&mut helper, 1, "forward-1000", b"", 5_000, 10_000, |_| {
        samples.push(last.elapsed().as_nanos());
        last = Instant::now();
        (Vec::new(), None)
    });
    assert!(result.is_some());
    assert!(samples.len() >= 1_000);
    samples.sort_unstable();
    assert!(samples[samples.len() / 2] <= MEDIAN_BUDGET_NANOS);
    assert!(samples[samples.len() * 99 / 100] <= P99_BUDGET_NANOS);
}

/// REQ: HST-160
#[test]
fn doc_hst_160_manual_states_the_round_trip_budget() {
    assert_page_has(PAGE, "0.2 ms median and 0.5 ms at the 99th percentile");
    assert_page_has(PAGE, "at most 10 ms in round trips at the 99th percentile");
}

// ------------------------------------------------------------------ HST-161

/// REQ: HST-161
#[test]
fn hst_161_slow_host_call_inside_a_call_makes_it_miss_the_deadline() {
    let wasm = require_fixture("conformance-probe");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("plugin-a", &wasm), Ok(()));
    let started = Instant::now();
    let (result, _) = run_call(&mut helper, 1, "forward-1", b"", 100, 3_000, |_| {
        std::thread::sleep(Duration::from_millis(250));
        (Vec::new(), None)
    });
    let result = result.expect("call result");
    assert_eq!(result.trap.as_deref(), Some("host.deadline"));
    assert!(started.elapsed() < Duration::from_millis(100 + 500));
}

/// REQ: HST-161
#[test]
fn hst_161_fast_host_call_inside_the_deadline_returns_normally() {
    let wasm = require_fixture("conformance-probe");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("plugin-a", &wasm), Ok(()));
    let (result, imports) = run_call(&mut helper, 1, "forward-1", b"", 1_000, 3_000, |_| (Vec::new(), None));
    assert_eq!(imports.len(), 1);
    assert!(result.expect("call result").trap.is_none());
}

/// REQ: HST-161
#[test]
fn hst_161_round_trips_add_up_inside_one_deadline() {
    let wasm = require_fixture("conformance-probe");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("plugin-a", &wasm), Ok(()));
    // Five calls of 30 ms each exceed a 100 ms deadline together although each is short.
    let (result, _) = run_call(&mut helper, 1, "forward-5", b"", 100, 3_000, |_| {
        std::thread::sleep(Duration::from_millis(30));
        (Vec::new(), None)
    });
    assert_eq!(result.expect("call result").trap.as_deref(), Some("host.deadline"));
}

// ------------------------------------------------------------------ HST-162

/// REQ: HST-162
#[test]
fn hst_162_runaway_guest_traps_in_the_helper_with_host_deadline() {
    let wasm = require_fixture("fuel-burn");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("plugin-a", &wasm), Ok(()));
    let (result, _) = run_call(&mut helper, 1, "spin", b"", 50, 2_000, |_| (Vec::new(), None));
    assert_eq!(result.expect("call result").trap.as_deref(), Some("host.deadline"));
}

/// REQ: HST-162
#[test]
fn hst_162_trap_arrives_before_the_host_kill_500_ms_after_the_deadline() {
    let wasm = require_fixture("fuel-burn");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("plugin-a", &wasm), Ok(()));
    let started = Instant::now();
    let (result, _) = run_call(&mut helper, 1, "spin", b"", 50, 2_000, |_| (Vec::new(), None));
    assert!(result.is_some());
    assert!(started.elapsed() < Duration::from_millis(50 + 500));
}

/// REQ: HST-162
#[test]
fn hst_162_trap_does_not_arrive_before_the_deadline() {
    let wasm = require_fixture("fuel-burn");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("plugin-a", &wasm), Ok(()));
    let started = Instant::now();
    let (result, _) = run_call(&mut helper, 1, "spin", b"", 200, 2_000, |_| (Vec::new(), None));
    assert_eq!(result.expect("call result").trap.as_deref(), Some("host.deadline"));
    assert!(started.elapsed() >= Duration::from_millis(200));
}

/// REQ: HST-162
#[test]
fn doc_hst_162_manual_states_the_500_ms_kill_and_one_failure() {
    assert_page_has(PAGE, "No `CallResult` arrives within 500 ms after the deadline | The host sends SIGKILL");
    assert_page_has(PAGE, "One failure is counted");
}

// ------------------------------------------------------------------ HST-163

/// REQ: HST-163
#[test]
fn hst_163_helper_answers_heartbeats_while_idle() {
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready("plugin-a"), Ok(()));
    for counter in 1..=3u64 {
        assert!(helper.heartbeat_echo(counter, 1_000));
    }
}

/// REQ: HST-163
#[test]
fn hst_163_helper_answers_heartbeats_on_its_control_thread_during_a_spinning_call() {
    let wasm = require_fixture("fuel-burn");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("plugin-a", &wasm), Ok(()));
    assert!(helper.send(&msg_call_export(1, "spin", b"", 3_000)));
    std::thread::sleep(Duration::from_millis(100));
    assert!(helper.heartbeat_echo(11, 900), "heartbeat unanswered while the guest spins");
}

/// REQ: HST-163
#[test]
fn doc_hst_163_manual_states_three_missed_heartbeats_ping_every_second() {
    assert_page_has(PAGE, "kill a helper after three missed heartbeats (a ping every 1 s)");
}

// ------------------------------------------------------------------ HST-164 to HST-167 (host rules)

/// REQ: HST-164
#[test]
fn doc_hst_164_manual_states_the_charge_memory_mib_plus_32() {
    assert_page_has(PAGE, "| Helper | `memory_mib + 32` MiB |");
    assert_page_has(PAGE, "charge `memory_mib + 32` MiB per helper to `session_memory_mib` at spawn and release it at exit");
}

/// REQ: HST-164
#[test]
fn doc_hst_164_session_total_limit_is_1024() {
    assert_page_has(PAGE, "`session_memory_mib` 1,024");
}

/// REQ: HST-165
#[test]
fn doc_hst_165_manual_states_refusal_code_no_failure_and_host_capacity() {
    assert_page_has(PAGE, "refused with `session.memory-exceeded`, MUST NOT count as a failure");
    assert_page_has(PAGE, "leave the side `inactive` with the reason `host-capacity`");
    assert_page_has(PAGE, "| `session.memory-exceeded` | Charge would pass `session_memory_mib` | no |");
}

/// REQ: HST-166
#[test]
fn doc_hst_166_manual_states_session_helpers_cap_default_16_ceiling_32() {
    assert_page_has(PAGE, "default 16, ceiling 32");
    assert_page_has(PAGE, "`helper.too-many`, without counting a failure");
    assert_page_has(PAGE, "| `helper.too-many` | `session_helpers` reached | no |");
}

/// REQ: HST-167 WARN
#[test]
fn doc_hst_167_manual_states_idle_shutdown_after_60_s() {
    assert_page_has(PAGE, "idle for 60 s");
    assert_page_has(PAGE, "start it again at the next need");
}

/// REQ: HST-167 WARN
#[test]
fn hst_167_shutdown_message_ends_an_idle_helper_with_exit_code_0() {
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready("plugin-a"), Ok(()));
    assert!(helper.send(&msg_shutdown("idle")));
    let status = helper.wait_exit(3_000).expect("helper exits on Shutdown");
    assert_eq!(status.code(), Some(0));
}

// ------------------------------------------------------------------ HST-168, HST-169

/// REQ: HST-168
#[test]
fn hst_168_killed_helper_closes_the_channel_during_a_call() {
    let wasm = require_fixture("fuel-burn");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("plugin-a", &wasm), Ok(()));
    assert!(helper.send(&msg_call_export(1, "spin", b"", 10_000)));
    std::thread::sleep(Duration::from_millis(100));
    helper.signal("KILL");
    assert!(matches!(helper.recv(3_000), Recv::Closed), "channel must close when the helper dies");
}

/// REQ: HST-168
#[test]
fn hst_168_killed_idle_helper_also_closes_the_channel_without_a_call() {
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready("plugin-a"), Ok(()));
    helper.signal("KILL");
    assert!(matches!(helper.recv(3_000), Recv::Closed));
}

/// REQ: HST-168
#[test]
fn doc_hst_168_manual_distinguishes_crash_during_call_and_idle() {
    assert_page_has(PAGE, "| Crash during a call | Channel closed | One failure;");
    assert_page_has(PAGE, "| Crash while idle | Channel closed | No failure;");
}

/// REQ: HST-169
#[test]
fn hst_169_cancel_ends_a_running_call_with_a_call_result() {
    let wasm = require_fixture("fuel-burn");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("plugin-a", &wasm), Ok(()));
    assert!(helper.send(&msg_call_export(1, "spin", b"", 10_000)));
    std::thread::sleep(Duration::from_millis(50));
    assert!(helper.send(&msg_cancel(1)));
    let frame = helper.recv_tag(tag::CALL_RESULT, 2_000).expect("cancelled call still ends in CallResult");
    assert_eq!(parse_call_result(&frame).expect("decodes").call_id, 1);
}

/// REQ: HST-169
#[test]
fn hst_169_pending_call_has_no_answer_after_the_helper_is_killed() {
    let wasm = require_fixture("fuel-burn");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("plugin-a", &wasm), Ok(()));
    assert!(helper.send(&msg_call_export(1, "spin", b"", 10_000)));
    helper.signal("KILL");
    assert!(helper.recv_tag(tag::CALL_RESULT, 1_500).is_none());
}

/// REQ: HST-169
#[test]
fn doc_hst_169_manual_states_pending_requests_complete_as_cancelled() {
    assert_page_has(PAGE, "pending requests complete as `cancelled`");
}

// ------------------------------------------------------------------ HST-170, HST-171

/// REQ: HST-170
#[test]
fn doc_hst_170_manual_states_5_starts_in_10_minutes_then_helper_unstable() {
    assert_page_has(PAGE, "at most 5 times in 10 minutes");
    assert_page_has(PAGE, "The sixth start moves the server side of the T2 and T3 plugins of the session to `stopped-by-failure` with the reason `helper.unstable`");
}

/// REQ: HST-170
#[test]
fn hst_170_each_start_of_the_helper_binary_is_independent_and_fresh() {
    // Five restarts in a row all reach Ready; the limit is a host rule, not a helper one.
    for round in 0..5 {
        let mut helper = Helper::spawn();
        assert_eq!(helper.ready("plugin-a"), Ok(()), "start {round}");
    }
}

/// REQ: HST-171
#[test]
fn hst_171_helper_exits_on_sigterm_within_2_s() {
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready("plugin-a"), Ok(()));
    helper.signal("TERM");
    assert!(helper.wait_exit(2_000).is_some(), "SIGTERM must end the helper before SIGKILL at 2 s");
}

/// REQ: HST-171
#[test]
fn hst_171_helper_exits_on_sigterm_even_while_the_guest_spins() {
    let wasm = require_fixture("fuel-burn");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("plugin-a", &wasm), Ok(()));
    assert!(helper.send(&msg_call_export(1, "spin", b"", 10_000)));
    std::thread::sleep(Duration::from_millis(100));
    helper.signal("TERM");
    assert!(helper.wait_exit(2_000).is_some());
}

/// REQ: HST-171
#[test]
fn doc_hst_171_manual_states_sigterm_then_sigkill_after_2_s() {
    assert_page_has(PAGE, "the host sends SIGTERM and then SIGKILL after 2 s");
}
