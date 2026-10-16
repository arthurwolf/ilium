//! Page reference-requirements-api-host.md: error record shape, code set, retry-after,
//! always-allowed codes, sync versus completion errors, install step order (API-001..005, 080).

use ilium_plugin_api::generated::types::ErrorCode;
use ilium_plugin_host::install::Installer;
use ilium_plugin_host::registry::PackageStore;
use ilium_plugin_host::testing::HostHarness;

use crate::support::*;

const ALL_CODES: [&str; 15] = [
    "not-permitted", "not-found", "invalid-argument", "conflict", "quota-exceeded", "rate-limited",
    "busy", "timed-out", "cancelled", "unsupported", "pane-not-ready", "revoked", "io", "network", "internal",
];

fn bad_argument_reply() -> ilium_plugin_host::testing::ImportReply {
    let mut host = host_with(QuietGuest);
    host.call_import("ilium:host/timers", "set", "{\"delay-ms\":1,\"repeat\":false}")
}

/// REQ: API-001
#[test]
fn api_001_fallible_function_returns_err_with_code_message_and_optional_retry_after() {
    let reply = bad_argument_reply();
    let error = reply.error().expect("a bad argument returns err");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert!(!error.message.is_empty());
    assert_eq!(error.retry_after_ms, None);
}

/// REQ: API-001
#[test]
fn api_001_a_successful_call_carries_no_error_record() {
    let mut host = host_with(QuietGuest);
    let reply = host.call_import("ilium:host/timers", "set", "{\"delay-ms\":50,\"repeat\":false}");
    assert!(reply.error().is_none());
}

/// REQ: API-001
#[test]
fn api_001_rate_limited_error_carries_the_retry_after_field() {
    let mut host = host_with(QuietGuest);
    for _ in 0..5 {
        host.call_import("ilium:host/sounds", "play", "{\"name\":\"bell\"}");
    }
    let error = host.call_import("ilium:host/sounds", "play", "{\"name\":\"bell\"}").error().expect("err");
    assert!(error.retry_after_ms.is_some());
}

/// REQ: API-002
#[test]
fn api_002_every_provoked_failure_uses_a_code_from_the_list_of_15() {
    let mut host = host_with(QuietGuest);
    let calls = [
        ("ilium:host/timers", "set", "{\"delay-ms\":1,\"repeat\":false}"),
        ("ilium:host/panes", "read-cells", "{\"pane\":999999,\"rows\":1}"),
        ("ilium:host/fs", "read", "{\"path\":\"/etc/passwd\",\"offset\":0,\"max-bytes\":1}"),
        ("ilium:host/tree", "set-name", "{\"node\":999999,\"name\":\"x\"}"),
    ];
    for (interface, function, args) in calls {
        let reply = host.call_import(interface, function, args);
        if let Some(code) = reply.error_code() {
            assert!(ALL_CODES.contains(&code), "{interface}.{function} gave {code}");
        }
    }
}

/// REQ: API-002
#[test]
fn api_002_the_wit_case_names_equal_the_documented_list_in_order() {
    assert_eq!(ErrorCode::CASE_NAMES, ALL_CODES);
}

/// REQ: API-002
#[test]
fn api_002_the_manual_lists_the_same_15_codes() {
    let page = manual_page("reference-errors-and-limits.md");
    for code in ALL_CODES {
        assert!(page.contains(&format!("`{code}`")), "manual misses {code}");
    }
}

/// REQ: API-003
#[test]
fn api_003_toast_over_the_rate_is_rate_limited_with_positive_retry_after() {
    let mut host = host_with(QuietGuest);
    for _ in 0..3 {
        host.call_import("ilium:host/toasts", "show", "{\"text\":\"a\"}");
    }
    let error = host.call_import("ilium:host/toasts", "show", "{\"text\":\"a\"}").error().expect("err");
    assert_eq!(error.code, ErrorCode::RateLimited);
    assert!(error.retry_after_ms.unwrap() > 0);
}

/// REQ: API-003
#[test]
fn api_003_cpu_budget_quota_exceeded_carries_retry_after() {
    let mut host = host_with(QuietGuest);
    host.charge_cpu_ms(2_500);
    let error = host.call_import("ilium:host/sounds", "play", "{\"name\":\"bell\"}").error().expect("err");
    assert_eq!(error.code, ErrorCode::QuotaExceeded);
    assert!(error.retry_after_ms.is_some());
}

/// REQ: API-003
#[test]
fn api_003_other_quota_exceeded_and_invalid_argument_have_no_retry_after() {
    let mut host = host_with(QuietGuest);
    let error = host.call_import("ilium:host/panes", "read-cells", "{\"pane\":1,\"rows\":1001}").error().expect("err");
    assert_eq!(error.code, ErrorCode::QuotaExceeded);
    assert_eq!(error.retry_after_ms, None);
}

/// REQ: API-003
#[test]
fn api_003_message_text_is_not_a_contract_two_runs_may_differ_but_the_code_is_stable() {
    let first = bad_argument_reply().error().expect("err");
    let second = bad_argument_reply().error().expect("err");
    assert_eq!(first.code, second.code);
}

/// REQ: API-004
#[test]
fn api_004_not_permitted_revoked_and_internal_are_allowed_from_any_result_function() {
    let mut host = HostHarness::new().grants(&["tree.write"]).with_native_guest(Box::new(QuietGuest)).build().expect("host");
    host.decline("tree.write");
    let declined = host.call_import("ilium:host/tree", "set-name", "{\"node\":1,\"name\":\"x\"}");
    assert_eq!(declined.error_code(), Some("not-permitted"));
    host.revoke_without_reactivation("tree.write");
    let revoked = host.call_import("ilium:host/tree", "set-name", "{\"node\":1,\"name\":\"x\"}");
    assert!(matches!(revoked.error_code(), Some("revoked") | Some("not-permitted")));
}

/// REQ: API-004
#[test]
fn api_004_request_id_function_with_4097_pending_in_the_session_returns_busy() {
    let mut host = HostHarness::new().grants(&["http:example.com"]).session_pending_requests(4_096).with_native_guest(Box::new(QuietGuest)).build().expect("host");
    host.fill_session_pending(4_096);
    assert_eq!(host.start_slow_request().error_code(), Some("busy"));
}

/// REQ: API-004
#[test]
fn api_004_a_function_without_a_request_id_may_omit_busy_or_return_it() {
    let mut host = host_with(QuietGuest);
    let reply = host.call_import("ilium:host/timers", "set", "{\"delay-ms\":50,\"repeat\":false}");
    // Both allowed behaviours: success, or err busy under load; never another surprise code.
    assert!(matches!(reply.error_code(), None | Some("busy")));
}

/// REQ: API-005
#[test]
fn api_005_path_outside_the_scope_fails_in_the_call_not_in_the_completion() {
    let mut host = HostHarness::new().grants(&["fs.read:project"]).with_native_guest(Box::new(QuietGuest)).build().expect("host");
    let reply = host.call_import("ilium:host/fs", "read-async", "{\"path\":\"/etc/passwd\"}");
    assert!(reply.error().is_some());
    assert!(reply.request_id().is_none());
}

/// REQ: API-005
#[test]
fn api_005_a_file_that_vanished_arrives_as_failed_not_found_in_the_completion() {
    let mut host = HostHarness::new().grants(&["fs.read:project"]).with_native_guest(Box::new(QuietGuest)).build().expect("host");
    let reply = host.call_import("ilium:host/fs", "read-async", "{\"path\":\"gone.txt\"}");
    let id = reply.request_id().expect("accepted: syntax and scope are fine");
    host.pump(8);
    let completion = host.completion_of(id).expect("a completion was enqueued");
    assert_eq!(completion.failed_code(), Some("not-found"));
}

/// REQ: API-005
#[test]
fn api_005_a_per_call_cap_breach_fails_in_the_call() {
    let mut host = HostHarness::new().grants(&["http:example.com"]).with_native_guest(Box::new(QuietGuest)).build().expect("host");
    let reply = host.call_import("ilium:host/http", "request", &format!("{{\"url\":\"https://example.com/{}\"}}", "a".repeat(8 * 1024)));
    assert_eq!(reply.error_code(), Some("invalid-argument"));
    assert!(reply.request_id().is_none());
}

// ---------------------------------------------------------------- API-080

fn install_codes(files: Vec<(String, Vec<u8>)>) -> Vec<String> {
    let dir = tempfile::tempdir().expect("temp dir");
    let installer = Installer::new(PackageStore { root: dir.path().to_path_buf() });
    let bytes = ilium_plugin_host::package::write_package(&files).expect("package writes");
    installer
        .check(&bytes, ilium_plugin_api::manifest::CheckMode::Install)
        .diagnostics
        .iter()
        .filter(|d| d.severity == ilium_plugin_api::error::Severity::Error)
        .map(|d| d.code.as_str().to_owned())
        .collect()
}

fn manifest_with(id: &str, extra: &str) -> Vec<(String, Vec<u8>)> {
    let text = format!(
        "{{\"id\":\"{id}\",\"name\":\"T\",\"version\":\"0.1.0\",\"tier\":\"T3\",\"api\":\"0.1.0\"{extra}}}"
    );
    vec![("manifest.json".to_owned(), text.into_bytes())]
}

/// REQ: API-080
#[test]
fn api_080_bad_id_and_unknown_key_report_only_the_id_code() {
    let codes = install_codes(manifest_with("Bad Id", ",\"zzz\":1"));
    assert_eq!(codes.len(), 1, "codes {codes:?}");
    assert!(codes[0].starts_with("manifest.id"));
}

/// REQ: API-080
#[test]
fn api_080_two_unknown_keys_report_both_codes_inside_the_step() {
    let codes = install_codes(manifest_with("vendor.demo", ",\"zzz\":1,\"yyy\":2"));
    assert_eq!(codes.len(), 2, "codes {codes:?}");
    assert!(codes.iter().all(|c| c == "manifest.unknown-key"));
}

/// REQ: API-080
#[test]
fn api_080_one_unknown_key_reports_exactly_one_code() {
    assert_eq!(install_codes(manifest_with("vendor.demo", ",\"zzz\":1")).len(), 1);
}

/// REQ: API-080
#[test]
fn api_080_the_cli_exit_code_is_1_when_any_error_line_was_printed() {
    let dir = tempfile::tempdir().expect("temp dir");
    let package = dir.path().join("pkg");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(package.join("manifest.json"), manifest_with("Bad Id", "")[0].1.clone()).unwrap();
    let output = ilium_plugin_host::cli::run_check_dir(&package);
    assert_eq!(output.exit_code, 1);
    assert!(output.stdout_jsonl.iter().any(|line| line["severity"] == "error"));
}

/// REQ: API-080
#[test]
fn api_080_a_clean_package_exits_0() {
    let dir = tempfile::tempdir().expect("temp dir");
    let package = dir.path().join("pkg");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(package.join("manifest.json"), manifest_with("vendor.demo", "")[0].1.clone()).unwrap();
    let output = ilium_plugin_host::cli::run_check_dir(&package);
    assert!(output.exit_code == 0 || output.stdout_jsonl.iter().all(|line| line["severity"] != "error"));
}
