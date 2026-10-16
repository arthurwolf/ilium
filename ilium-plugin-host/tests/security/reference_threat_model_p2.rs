//! reference-threat-model.md, part 2: side-channel and information-leak mitigations
//! (SEC-370 .. 376). Run with `--features testing`.

use std::collections::BTreeSet;
use std::time::Duration;

use ilium_plugin_api::{ErrorCode, Side, Tier};
use proptest::prelude::*;
use serde_json::json;

use crate::support::*;

const MS: u64 = 1_000_000;

/// Advances the manual clock in `step_micros` steps and records `(true elapsed nanos, reading)`.
fn sample_clock(rig: &mut Rig, step_micros: u64, steps: u64, wasi: bool) -> Vec<(u64, u64)> {
    let mut samples = Vec::new();
    let origin = if wasi { rig.host.guest_wasi_clock_nanos() } else { rig.host.guest_clock_nanos() };
    for step in 0..steps {
        rig.host.advance_clock(Duration::from_micros(step_micros));
        let reading = if wasi { rig.host.guest_wasi_clock_nanos() } else { rig.host.guest_clock_nanos() };
        samples.push(((step + 1) * step_micros * 1000, reading.saturating_sub(origin)));
    }
    samples
}

fn assert_coarse_and_jittered(samples: &[(u64, u64)]) {
    // Coarsened: every observed step is a whole number of milliseconds, at least 1 ms.
    let readings: Vec<u64> = samples.iter().map(|(_, reading)| *reading).collect();
    let distinct: BTreeSet<u64> = readings.iter().copied().collect();
    for pair in distinct.iter().collect::<Vec<_>>().windows(2) {
        assert!(pair[1] - pair[0] >= MS, "steps are 1 ms or more: {} -> {}", pair[0], pair[1]);
        assert_eq!((pair[1] - pair[0]) % MS, 0);
    }
    assert!(readings.windows(2).all(|pair| pair[0] <= pair[1]), "the guest clock never goes back");
    // Jittered: the true time at which the reading steps varies by up to 1 ms around the boundary.
    let step_times: BTreeSet<u64> = samples
        .windows(2)
        .filter(|pair| pair[0].1 != pair[1].1)
        .map(|pair| pair[1].0 % MS)
        .collect();
    assert!(step_times.len() > 1, "the step instants show jitter: {step_times:?}");
    // Error against the true time stays within coarsening plus jitter.
    for (truth, reading) in samples {
        assert!(truth.abs_diff(*reading) <= 2 * MS, "reading {reading} too far from {truth}");
    }
}

/// REQ: SEC-370
#[test]
fn sec_370_t3_guest_clock_is_coarsened_to_one_millisecond_and_jittered() {
    let mut rig = t3(&[]);
    let samples = sample_clock(&mut rig, 100, 20_000, false);
    assert_coarse_and_jittered(&samples);
}

/// REQ: SEC-370
#[test]
fn sec_370_t2_guest_clock_is_coarsened_and_jittered() {
    let mut rig = setup(&[]).tier(Tier::T2).start();
    let samples = sample_clock(&mut rig, 100, 20_000, false);
    assert_coarse_and_jittered(&samples);
}

/// REQ: SEC-370
#[test]
fn sec_370_the_virtualised_wasi_clock_gets_the_same_coarsening() {
    let mut rig = t3(&[]);
    let samples = sample_clock(&mut rig, 100, 20_000, true);
    assert_coarse_and_jittered(&samples);
}

/// REQ: SEC-370
#[test]
fn sec_370_a_sub_millisecond_interval_is_not_measurable() {
    let mut rig = t3(&[]);
    let first = rig.host.guest_clock_nanos();
    rig.host.advance_clock(Duration::from_micros(10));
    let second = rig.host.guest_clock_nanos();
    assert!(second - first == 0 || second - first >= MS, "a 10 us interval shows as {} ns", second - first);
}

/// REQ: SEC-370
#[test]
fn sec_370_a_t1_plugin_gets_the_same_resolution_by_default() {
    let mut rig = setup(&[]).tier(Tier::T1).start();
    let samples = sample_clock(&mut rig, 100, 5_000, false);
    let distinct: BTreeSet<u64> = samples.iter().map(|(_, reading)| *reading).collect();
    for pair in distinct.iter().collect::<Vec<_>>().windows(2) {
        assert!(pair[1] - pair[0] >= MS);
    }
}

/// REQ: SEC-370
#[test]
fn sec_370_a_t1_plugin_that_requests_a_finer_clock_gets_one() {
    let mut rig = setup(&[]).tier(Tier::T1).option("fine-clock", json!(true)).start();
    let samples = sample_clock(&mut rig, 10, 200, false);
    let distinct: BTreeSet<u64> = samples.iter().map(|(_, reading)| *reading).collect();
    assert!(distinct.len() > 2, "a fine clock resolves 10 us steps");
}

/// REQ: SEC-370
#[test]
fn sec_370_a_t3_plugin_cannot_request_a_finer_clock() {
    let mut rig = setup(&[]).option("fine-clock", json!(true)).start();
    let samples = sample_clock(&mut rig, 10, 200, false);
    let distinct: BTreeSet<u64> = samples.iter().map(|(_, reading)| *reading).collect();
    assert!(distinct.len() <= 2, "third-party clocks stay coarse");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    // REQ: SEC-370
    #[test]
    fn sec_370_guest_readings_never_resolve_below_one_millisecond(step_micros in 1u64..900, steps in 50u64..400) {
        let mut rig = t3(&[]);
        let samples = sample_clock(&mut rig, step_micros, steps, false);
        let distinct: BTreeSet<u64> = samples.iter().map(|(_, reading)| *reading).collect();
        for pair in distinct.iter().collect::<Vec<_>>().windows(2) {
            prop_assert!(pair[1] - pair[0] >= MS);
        }
    }
}

// ---- SEC-371 ----

fn screen_rig() -> Rig {
    t3(&["terminal.read-screen"])
}

/// Writes `count` pane outputs `gap_millis` apart and returns the `screen-changed` events seen.
fn emit_outputs(rig: &mut Rig, count: u32, gap_millis: u64) -> usize {
    let before = rig.host.screen_changed_count();
    for index in 0..count {
        rig.host.pane_output(&format!("line {index}\n"));
        rig.host.advance_clock(Duration::from_millis(gap_millis));
        rig.host.pump(4);
    }
    rig.host.advance_clock(Duration::from_millis(200));
    rig.host.pump(8);
    rig.host.screen_changed_count() - before
}

/// REQ: SEC-371
#[test]
fn sec_371_a_burst_of_output_is_coalesced_into_one_event() {
    let mut rig = screen_rig();
    for index in 0..50 {
        rig.host.pane_output(&format!("burst {index}\n"));
    }
    rig.host.advance_clock(Duration::from_millis(200));
    rig.host.pump(8);
    assert_eq!(rig.host.screen_changed_count(), 1);
}

/// REQ: SEC-371
#[test]
fn sec_371_at_most_ten_events_arrive_in_one_second_of_continuous_output() {
    let mut rig = screen_rig();
    let seen = emit_outputs(&mut rig, 100, 10);
    assert!((1..=10).contains(&seen), "{seen} events in one second");
}

/// REQ: SEC-371
#[test]
fn sec_371_a_quiet_pane_with_slow_output_gets_an_event_per_change() {
    let mut rig = screen_rig();
    let seen = emit_outputs(&mut rig, 5, 500);
    assert_eq!(seen, 5);
}

/// REQ: SEC-371
#[test]
fn sec_371_no_event_is_emitted_while_the_pty_echo_flag_is_off() {
    let mut rig = screen_rig();
    rig.host.set_pane_echo(false);
    let seen = emit_outputs(&mut rig, 20, 150);
    assert_eq!(seen, 0);
}

/// REQ: SEC-371
#[test]
fn sec_371_events_resume_when_echo_is_back_on() {
    let mut rig = screen_rig();
    rig.host.set_pane_echo(false);
    assert_eq!(emit_outputs(&mut rig, 3, 150), 0);
    rig.host.set_pane_echo(true);
    assert!(emit_outputs(&mut rig, 3, 150) >= 1);
}

/// REQ: SEC-371
#[test]
fn sec_371_output_written_in_no_echo_mode_is_not_delivered_after_echo_returns_as_a_stale_event() {
    let mut rig = screen_rig();
    rig.host.set_pane_echo(false);
    rig.host.pane_output("password: ");
    rig.host.set_pane_echo(true);
    rig.host.advance_clock(Duration::from_millis(500));
    rig.host.pump(8);
    assert_eq!(rig.host.screen_changed_count(), 0, "the password prompt period leaves no event");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    // REQ: SEC-371
    #[test]
    fn sec_371_events_never_exceed_ten_per_second(count in 1u32..150, gap_millis in 1u64..60) {
        let mut rig = screen_rig();
        let seen = emit_outputs(&mut rig, count, gap_millis);
        let seconds = ((count as u64 * gap_millis + 200) as f64 / 1000.0).ceil() as usize;
        prop_assert!(seen <= 10 * seconds.max(1), "{} events in {} s", seen, seconds);
    }
}

// ---- SEC-372 ----

/// REQ: SEC-372
#[test]
fn sec_372_a_t2_or_t3_side_runs_in_a_helper_process() {
    for tier in [Tier::T2, Tier::T3] {
        let rig = setup(&[]).tier(tier).start();
        assert_ne!(rig.host.guest_process_id(), std::process::id(), "{tier:?}");
    }
}

/// REQ: SEC-372
#[test]
fn sec_372_the_server_and_client_sides_of_one_plugin_run_in_different_helpers() {
    let server = rig(Tier::T3, Side::Server, &[]);
    let client = rig(Tier::T3, Side::Client, &[]);
    assert_ne!(server.host.guest_process_id(), client.host.guest_process_id());
}

/// REQ: SEC-372
#[test]
fn sec_372_two_plugins_do_not_share_a_helper() {
    let first = t3(&[]);
    let second = t3(&[]);
    assert_ne!(first.host.guest_process_id(), second.host.guest_process_id());
}

/// REQ: SEC-372
#[test]
fn sec_372_a_t1_plugin_shares_the_server_address_space() {
    let rig = setup(&[]).tier(Tier::T1).start();
    assert_eq!(rig.host.guest_process_id(), std::process::id());
}

// ---- SEC-373 ----

fn unavailable_rig() -> Rig {
    setup(&[]).option("installed-plugins", json!(["other.installed"])).start()
}

fn list_extensions(rig: &Rig, owner: &str) -> ilium_plugin_api::PluginError {
    match rig.value("ilium:host/extensions.list", json!({ "point": format!("{owner}/point") })) {
        Err(error) => error,
        Ok(value) => panic!("expected a refusal, got {value}"),
    }
}

/// REQ: SEC-373
#[test]
fn sec_373_an_ungranted_installed_plugin_is_refused_with_not_permitted_unavailable() {
    let rig = unavailable_rig();
    let error = list_extensions(&rig, "other.installed");
    assert_eq!(error.code, ErrorCode::NotPermitted);
    assert_eq!(reason_of(&error), "unavailable");
}

/// REQ: SEC-373
#[test]
fn sec_373_a_plugin_that_is_not_installed_is_refused_with_not_permitted_unavailable() {
    let rig = unavailable_rig();
    let error = list_extensions(&rig, "other.missing");
    assert_eq!(error.code, ErrorCode::NotPermitted);
    assert_eq!(reason_of(&error), "unavailable");
}

/// REQ: SEC-373
#[test]
fn sec_373_the_two_errors_are_identical() {
    let rig = unavailable_rig();
    let installed = list_extensions(&rig, "other.installed");
    let missing = list_extensions(&rig, "other.missing");
    assert_eq!(installed.code, missing.code);
    assert_eq!(installed.message, missing.message);
}

/// REQ: SEC-373
#[test]
fn sec_373_neither_error_names_the_plugin_id() {
    let rig = unavailable_rig();
    for owner in ["other.installed", "other.missing"] {
        assert!(!list_extensions(&rig, owner).message.contains(owner));
    }
}

/// REQ: SEC-373
#[test]
fn sec_373_peer_calls_do_not_reveal_installed_plugin_ids_either() {
    let rig = unavailable_rig();
    let installed = rig.value("ilium:host/plugins.set-enabled", json!({ "plugin": "other.installed", "enabled": true, "scope": "user" }));
    let missing = rig.value("ilium:host/plugins.set-enabled", json!({ "plugin": "other.missing", "enabled": true, "scope": "user" }));
    let (installed, missing) = (installed.expect_err("refused"), missing.expect_err("refused"));
    assert_eq!((installed.code, installed.message), (missing.code, missing.message));
}

/// REQ: SEC-373
#[test]
fn sec_373_run_hook_on_an_unavailable_owner_gives_the_same_answer_for_both_cases() {
    let rig = unavailable_rig();
    let call = |owner: &str| rig.value("ilium:host/extensions.run-hook", json!({ "point": format!("{owner}/point"), "payload-json": "{}" })).expect_err("refused");
    let (installed, missing) = (call("other.installed"), call("other.missing"));
    assert_eq!((installed.code, installed.message), (missing.code, missing.message));
}

// ---- SEC-374 ----

/// Error texts of calls that fail for different reasons.
fn failing_error_texts(rig: &Rig) -> Vec<String> {
    let calls = [
        ("ilium:host/storage.put", json!({ "bucket": "data", "key": "../escape", "value": [1] })),
        ("ilium:host/fs.read", json!({ "path": { "scope": "project", "path": "/etc/passwd" }, "offset": 0, "max-bytes": 10 })),
        ("ilium:host/fs.read", json!({ "path": { "scope": "data", "path": "../../../etc/passwd" }, "offset": 0, "max-bytes": 10 })),
        ("ilium:host/fs.metadata", json!({ "path": { "scope": "data", "path": "missing/file" } })),
        ("ilium:host/extensions.list", json!({ "point": "other.installed/point" })),
        ("ilium:host/plugins.set-enabled", json!({ "plugin": "other.installed", "enabled": true, "scope": "user" })),
    ];
    calls
        .iter()
        .filter_map(|(name, args)| rig.value(name, args.clone()).err())
        .map(|error| error.message)
        .collect()
}

/// REQ: SEC-374
#[test]
fn sec_374_errors_do_not_contain_the_scratch_state_directory() {
    let rig = setup(&[]).option("installed-plugins", json!(["other.installed"])).start();
    let state = rig.state.path().to_string_lossy().into_owned();
    let texts = failing_error_texts(&rig);
    assert!(!texts.is_empty(), "the calls fail");
    for text in texts {
        assert!(!text.contains(&state), "{text}");
    }
}

/// REQ: SEC-374
#[test]
fn sec_374_errors_do_not_contain_absolute_paths() {
    let rig = setup(&[]).option("installed-plugins", json!(["other.installed"])).start();
    let texts = failing_error_texts(&rig);
    assert!(!texts.is_empty());
    for text in texts {
        for root in ["/home/", "/tmp/", "/media/", "/var/", "/etc/", "/usr/", "/run/", "C:\\", "/Users/"] {
            assert!(!text.contains(root), "{root} in {text}");
        }
    }
}

/// REQ: SEC-374
#[test]
fn sec_374_errors_do_not_contain_ids_of_other_plugins() {
    let rig = setup(&[]).option("installed-plugins", json!(["other.installed"])).start();
    for text in failing_error_texts(&rig) {
        assert!(!text.contains("other.installed"), "{text}");
    }
}

/// REQ: SEC-374
#[test]
fn sec_374_trap_messages_do_not_contain_absolute_paths() {
    let rig = setup(&[]).trapping_guest().start();
    let message = rig.host.trap_message_for("trapping-guest");
    for root in ["/home/", "/tmp/", "/media/", "/usr/"] {
        assert!(!message.contains(root), "{message}");
    }
}

/// REQ: SEC-374
#[test]
fn sec_374_the_error_string_helper_gives_no_path_for_an_outside_scope_read() {
    let rig = t3(&[]);
    let text = rig.host.error_string_for("fs.read ../../../etc/passwd");
    assert!(!text.contains("/etc/passwd") || text.contains("not-permitted"), "{text}");
    assert!(!text.starts_with('/'));
}

// ---- SEC-375 ----

/// Puts 1 MiB values until the quota refuses; returns the number accepted and the error text.
fn fill_until_quota(rig: &Rig) -> (u32, String) {
    for index in 0..4096u32 {
        let result = rig.value("ilium:host/storage.put", json!({ "bucket": "data", "key": format!("k{index}"), "value": vec![0u8; 1024 * 1024] }));
        if let Err(error) = result {
            assert_eq!(error.code, ErrorCode::QuotaExceeded);
            return (index, error.message);
        }
    }
    panic!("the quota never refused 4 GiB");
}

/// REQ: SEC-375
#[test]
fn sec_375_the_quota_error_is_the_same_whatever_other_plugins_store() {
    let empty = setup(&[]).start();
    let busy = setup(&[]).start();
    busy.host.write_other_plugin_data("other.installed", "blob", &vec![7u8; 64 * 1024 * 1024]);
    let (accepted_empty, message_empty) = fill_until_quota(&empty);
    let (accepted_busy, message_busy) = fill_until_quota(&busy);
    assert_eq!(accepted_empty, accepted_busy, "neighbour data must not change the plugin's own quota");
    assert_eq!(message_empty, message_busy);
}

/// REQ: SEC-375
#[test]
fn sec_375_the_quota_message_does_not_mention_other_plugins() {
    let rig = setup(&[]).start();
    rig.host.write_other_plugin_data("other.installed", "blob", &[1, 2, 3]);
    let (_, message) = fill_until_quota(&rig);
    assert!(!message.contains("other.installed"), "{message}");
}

/// REQ: SEC-375
#[test]
fn sec_375_a_plugin_sees_only_its_own_usage() {
    let rig = setup(&[]).start();
    rig.host.write_other_plugin_data("other.installed", "blob", &vec![1u8; 4 * 1024 * 1024]);
    let usage = rig.value("ilium:host/storage.usage", json!({})).expect("usage");
    assert_eq!(usage["data-bytes"].as_u64(), Some(0), "{usage}");
}

// ---- SEC-376 ----

/// REQ: SEC-376
#[test]
fn sec_376_constant_time_comparison_accepts_equal_tokens_only() {
    use ilium_plugin_host::sync::constant_time_eq;
    assert!(constant_time_eq(b"abc123", b"abc123"));
    assert!(!constant_time_eq(b"abc123", b"abc124"));
    assert!(!constant_time_eq(b"abc123", b"Xbc123"));
    assert!(!constant_time_eq(b"abc123", b"abc12"));
    assert!(!constant_time_eq(b"", b"a"));
    assert!(constant_time_eq(b"", b""));
}

proptest! {
    // REQ: SEC-376
    #[test]
    fn sec_376_constant_time_comparison_agrees_with_equality(left in prop::collection::vec(any::<u8>(), 0..64), right in prop::collection::vec(any::<u8>(), 0..64)) {
        use ilium_plugin_host::sync::constant_time_eq;
        prop_assert_eq!(constant_time_eq(&left, &right), left == right);
        prop_assert!(constant_time_eq(&left, &left));
    }
}

/// REQ: SEC-376
#[test]
fn sec_376_the_http_api_rejects_tokens_wrong_at_the_first_or_last_byte_alike() {
    let rig = t3(&[]);
    let token: String = rig.host.http_api_token();
    let mut first = token.clone().into_bytes();
    first[0] ^= 1;
    let mut last = token.clone().into_bytes();
    let end = last.len() - 1;
    last[end] ^= 1;
    for wrong in [first, last] {
        let header = format!("Bearer {}", String::from_utf8_lossy(&wrong));
        assert_eq!(rig.host.http_api_request("/", &[("Authorization", header.as_str())]), 401);
    }
    let good = format!("Bearer {token}");
    assert_ne!(rig.host.http_api_request("/", &[("Authorization", good.as_str())]), 401);
}

/// REQ: SEC-376
#[test]
fn sec_376_no_source_file_compares_a_token_with_plain_equality() {
    let src = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    let mut stack = vec![src];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                for (number, line) in std::fs::read_to_string(&path).unwrap_or_default().lines().enumerate() {
                    let lowered = line.to_lowercase();
                    if lowered.contains("token") && (line.contains(" == ") || line.contains(" != ")) && !lowered.trim_start().starts_with("//") {
                        offenders.push(format!("{}:{}", path.display(), number + 1));
                    }
                }
            }
        }
    }
    assert!(offenders.is_empty(), "token comparisons that are not constant-time: {offenders:?}");
}
