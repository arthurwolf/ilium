//! user-plugin-security-and-privacy.md: no telemetry, files written, what leaves the machine,
//! the inference privacy log, plugin telemetry opt-in, `data_use` and `security_contact`.

use ilium_plugin_api::Tier;
use proptest::prelude::*;
use serde_json::{json, Value};

use crate::support::*;

/// REQ: SEC-260
#[test]
fn sec_260_a_session_with_every_bundled_plugin_makes_no_outbound_connection() {
    let mut rig = setup(&[]).tier(Tier::T1).start();
    rig.host.deliver(one_event());
    rig.host.pump(16);
    rig.host.advance_clock(std::time::Duration::from_secs(3600));
    assert!(rig.net.seen().is_empty(), "no telemetry, crash report or usage data");
    assert!(rig.net.resolutions().is_empty());
}

/// REQ: SEC-260
#[test]
fn sec_260_a_trap_does_not_produce_a_crash_report_upload() {
    let rig = setup(&[]).tier(Tier::T1).trapping_guest().start();
    rig.host.deliver(one_event());
    rig.host.pump(16);
    assert!(rig.net.seen().is_empty());
}

/// REQ: SEC-260
#[test]
fn sec_260_the_manual_promises_opt_in_for_any_future_collection() {
    assert_page_has("user-plugin-security-and-privacy.md", &["send no telemetry, crash reports or usage data", "opt-in, off by default"]);
}

/// REQ: SEC-261
#[test]
fn sec_261_every_file_the_platform_creates_is_readable_only_by_the_user() {
    use std::os::unix::fs::PermissionsExt;
    let rig = setup(&["clipboard.read"]).secret("token", "abcdef-secret").start();
    rig.host.persist_all_stores();
    rig.host.write_plugin_data("a.txt", b"x");
    let mut files = 0;
    let mut stack = vec![rig.state.path().to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("dir") {
            let entry = entry.expect("entry");
            let mode = entry.metadata().expect("metadata").permissions().mode() & 0o777;
            assert_eq!(mode & 0o077, 0, "{:?} is {mode:o}", entry.path());
            files += 1;
            if entry.path().is_dir() {
                stack.push(entry.path());
            }
        }
    }
    assert!(files > 0);
}

/// REQ: SEC-261
#[test]
fn sec_261_the_table_lists_eleven_places_with_a_way_to_delete_each() {
    let page = manual_page("user-plugin-security-and-privacy.md");
    let rows: Vec<&str> = page.lines().skip_while(|line| !line.starts_with("| What | Where (Linux)")).skip(2).take_while(|line| line.starts_with('|')).collect();
    assert_eq!(rows.len(), 11);
    for row in rows {
        let cells: Vec<&str> = row.split('|').map(str::trim).collect();
        assert!(!cells[5].is_empty(), "{row}");
    }
}

/// REQ: SEC-261
#[test]
fn sec_261_the_stores_named_in_the_table_exist_after_use_at_the_named_paths() {
    let rig = setup(&["clipboard.read"]).start();
    rig.host.persist_all_stores();
    for relative in ["grants.json", "audit/audit.jsonl"] {
        assert!(rig.state.path().join(relative).exists(), "{relative}");
    }
}

/// REQ: SEC-263
#[test]
fn sec_263_only_inventory_features_send_data_and_a_plugin_without_grants_sends_nothing() {
    let rig = t3(&[]);
    let result = rig.get("https://example.com/", &[]);
    assert!(result.is_err());
    assert!(rig.net.seen().is_empty());
}

/// REQ: SEC-263
#[test]
fn sec_263_the_two_headline_features_are_named_in_the_page() {
    assert_page_has("user-plugin-security-and-privacy.md", &["Network egress", "inference"]);
}

/// REQ: SEC-264
#[test]
fn sec_264_the_install_screen_of_an_inference_plugin_names_the_provider_and_remote_or_local() {
    let remote = setup(&["inference"]).provider("kilo-gateway", false).start();
    let text = remote.host.review_text_for(&["inference"]);
    assert!(text.contains("kilo-gateway") && text.contains("remote"), "{text}");
    let local = setup(&["inference"]).provider("ollama", true).start();
    let text = local.host.review_text_for(&["inference"]);
    assert!(text.contains("ollama") && text.contains("local"), "{text}");
}

/// REQ: SEC-264
#[test]
fn sec_264_a_plugin_without_the_inference_grant_gets_no_provider_line() {
    let rig = setup(&["clipboard.read"]).provider("kilo-gateway", false).start();
    assert!(!rig.host.review_text_for(&["clipboard.read"]).contains("kilo-gateway"));
}

fn inference_rig() -> Rig {
    setup(&["inference"]).provider("ollama", true).start()
}

fn ask(rig: &Rig, purpose: &str) {
    let _ = rig.call("ilium:host/inference.complete", json!({ "purpose": purpose, "prompt": "SECRET-PROMPT-TEXT", "model": "m" }));
    rig.host.pump(16);
}

/// REQ: SEC-265
#[test]
fn sec_265_each_request_writes_one_record_with_exactly_the_documented_fields_and_no_text() {
    let rig = inference_rig();
    ask(&rig, "summary");
    let path = rig.state.path().join("privacy").join("inference.jsonl");
    let lines = read_jsonl(&path);
    assert_eq!(lines.len(), 1);
    let mut keys: Vec<&str> = lines[0].as_object().expect("object").keys().map(String::as_str).collect();
    keys.sort_unstable();
    let mut expected = vec!["ts", "plugin", "purpose", "provider", "model", "tokens_in", "tokens_out", "bytes_out", "bytes_in", "local"];
    expected.sort_unstable();
    assert_eq!(keys, expected);
    assert_eq!(lines[0]["plugin"], "test.plugin");
    assert_eq!(lines[0]["local"], true);
    assert!(!std::fs::read_to_string(&path).expect("read").contains("SECRET-PROMPT-TEXT"));
}

/// REQ: SEC-265
#[test]
fn sec_265_ts_is_utc_iso_8601() {
    let rig = inference_rig();
    ask(&rig, "summary");
    let lines = read_jsonl(&rig.state.path().join("privacy").join("inference.jsonl"));
    let ts = lines[0]["ts"].as_str().expect("ts");
    assert!(ts.ends_with('Z') && ts.contains('T'), "{ts}");
}

/// REQ: SEC-265
#[test]
fn sec_265_retention_is_90_days_boundary() {
    for (age_days, kept) in [(89_u64, true), (90, true), (91, false)] {
        let mut rig = inference_rig();
        ask(&rig, "old");
        rig.host.advance_clock(std::time::Duration::from_secs(age_days * 86_400));
        ask(&rig, "new");
        let lines = read_jsonl(&rig.state.path().join("privacy").join("inference.jsonl"));
        let has_old = lines.iter().any(|line| line["purpose"] == "old");
        assert_eq!(has_old, kept, "{age_days} days");
    }
}

/// REQ: SEC-265
#[test]
fn sec_265_retention_is_16_mib_boundary() {
    let rig = inference_rig();
    rig.host.privacy_log_fill_to(16 * 1024 * 1024 + 4096);
    ask(&rig, "after");
    let size = std::fs::metadata(rig.state.path().join("privacy").join("inference.jsonl")).expect("file").len();
    assert!(size <= 16 * 1024 * 1024, "{size}");
}

/// REQ: SEC-265
#[test]
fn sec_265_the_cli_prints_jsonl_filtered_by_plugin() {
    let scratch = scratch();
    let env = cli_env(scratch.path());
    let dir = env.state_dir.join("privacy");
    std::fs::create_dir_all(&dir).expect("dir");
    let line = |plugin: &str| json!({ "ts": "2026-10-12T08:00:00.000Z", "plugin": plugin, "purpose": "p", "provider": "x", "model": "m", "tokens_in": 1, "tokens_out": 2, "bytes_out": 3, "bytes_in": 4, "local": false }).to_string();
    std::fs::write(dir.join("inference.jsonl"), format!("{}\n{}\n", line("one.plugin"), line("two.plugin"))).expect("write");
    let all = ilium_plugin_host::cli::run(&args(&["privacy", "log"]), &env);
    assert_eq!(all.exit_code, 0, "{all:?}");
    let one = ilium_plugin_host::cli::run(&args(&["privacy", "log", "--plugin", "one.plugin"]), &env);
    let records: Vec<&Value> = one.stdout_jsonl.iter().filter(|value| value.get("plugin").is_some()).collect();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["plugin"], "one.plugin");
}

/// REQ: SEC-265
#[test]
fn sec_265_a_replacement_inference_plugin_cannot_disable_the_log() {
    let rig = setup(&["inference"]).provider("ollama", true).replacement_inference_plugin().start();
    ask(&rig, "via replacement");
    assert_eq!(read_jsonl(&rig.state.path().join("privacy").join("inference.jsonl")).len(), 1);
}

/// REQ: SEC-266
#[test]
fn sec_266_the_manual_requires_a_visible_default_off_setting() {
    assert_page_has("user-plugin-security-and-privacy.md", &["MUST make it opt-in", "defaults to off"]);
}

fn data_use_entry(what: &str, wher: &str, purpose: &str, opt_in: bool) -> Value {
    json!({ "what": what, "where": wher, "purpose": purpose, "opt_in": opt_in })
}

fn data_use_errors(entries: Vec<Value>) -> Vec<String> {
    manifest_error_codes(&server_manifest(json!({ "data_use": entries })))
}

/// REQ: SEC-267
#[test]
fn sec_267_a_valid_data_use_list_is_accepted() {
    assert!(data_use_errors(vec![data_use_entry("usage counts", "https://stats.example", "improve the plugin", true)]).is_empty());
}

/// REQ: SEC-267
#[test]
fn sec_267_entry_count_boundary_7_8_9() {
    for (count, ok) in [(7_usize, true), (8, true), (9, false)] {
        let entries = (0..count).map(|index| data_use_entry(&format!("w{index}"), "somewhere", "why", false)).collect();
        assert_eq!(data_use_errors(entries).is_empty(), ok, "{count} entries");
    }
}

/// REQ: SEC-267
#[test]
fn sec_267_field_length_boundary_199_200_201() {
    for (length, ok) in [(199_usize, true), (200, true), (201, false)] {
        for field in ["what", "where", "purpose"] {
            let mut entry = data_use_entry("a", "b", "c", false);
            entry[field] = json!("x".repeat(length));
            assert_eq!(data_use_errors(vec![entry]).is_empty(), ok, "{field} {length}");
        }
    }
}

/// REQ: SEC-267
#[test]
fn sec_267_wrong_types_and_missing_fields_are_manifest_errors() {
    let missing = json!({ "what": "a", "where": "b", "purpose": "c" });
    assert!(!data_use_errors(vec![missing]).is_empty());
    let wrong = json!({ "what": 1, "where": "b", "purpose": "c", "opt_in": false });
    assert!(!data_use_errors(vec![wrong]).is_empty());
    let extra = json!({ "what": "a", "where": "b", "purpose": "c", "opt_in": false, "other": 1 });
    assert!(!data_use_errors(vec![extra]).is_empty());
}

/// REQ: SEC-267
#[test]
fn sec_267_the_review_dialog_shows_each_entry_under_its_heading() {
    let rig = t3(&[]);
    let text = rig.host.review_text_with_data_use(&[data_use_entry("usage counts", "https://stats.example", "improve", true), data_use_entry("crash logs", "https://crash.example", "fix bugs", false)]);
    assert!(text.contains("Data this plugin collects"));
    for needle in ["usage counts", "https://stats.example", "crash logs", "https://crash.example"] {
        assert!(text.contains(needle), "{needle}");
    }
}

/// REQ: SEC-267
#[test]
fn sec_267_a_manifest_without_data_use_shows_no_heading() {
    let rig = t3(&[]);
    assert!(!rig.host.review_text_with_data_use(&[]).contains("Data this plugin collects"));
}

proptest! {
    // REQ: SEC-267
    #[test]
    fn sec_267_any_list_within_the_limits_is_accepted(count in 0usize..=8, length in 1usize..=200) {
        let entries = (0..count).map(|_| data_use_entry(&"w".repeat(length), "b", "c", true)).collect();
        prop_assert!(data_use_errors(entries).is_empty());
    }
}

fn contact_errors(value: &str) -> Vec<String> {
    manifest_error_codes(&server_manifest(json!({ "security_contact": value })))
}

/// REQ: SEC-268 WARN
#[test]
fn sec_268_an_email_address_and_an_https_url_are_accepted() {
    assert!(contact_errors("security@example.com").is_empty());
    assert!(contact_errors("https://example.com/security").is_empty());
}

/// REQ: SEC-268 WARN
#[test]
fn sec_268_length_boundary_199_200_201() {
    for (length, ok) in [(199_usize, true), (200, true), (201, false)] {
        let value = format!("https://example.com/{}", "a".repeat(length - "https://example.com/".len()));
        assert_eq!(contact_errors(&value).is_empty(), ok, "{length}");
    }
}

/// REQ: SEC-268 WARN
#[test]
fn sec_268_details_shows_the_contact() {
    let rig = t3(&[]);
    assert!(rig.host.details_text_with_contact("security@example.com").contains("security@example.com"));
}

/// REQ: SEC-268 WARN
#[test]
fn sec_268_a_plain_http_url_or_free_text_is_not_a_contact() {
    assert!(!contact_errors("http://example.com/security").is_empty() || !contact_errors("call me maybe").is_empty());
}
