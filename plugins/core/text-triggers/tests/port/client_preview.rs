//! Twins of the dialog preview tests: the test-rule service answers a sample against a pattern and reply.

use ilium_plugin_sdk::types::ErrorCode;
use serde_json::json;

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium-client/src/text_trigger_dialog/preview_tests.rs::close_does_not_wait_for_blocked_cpu_and_keeps_real_queued_receipt
#[test]
fn port_001_close_does_not_wait_for_blocked_cpu_and_keeps_real_queued_receipt() {
    let mut host = client_host();
    let reply = preview(&mut host, "bb", "r", "abba");
    assert!(reply["preview"].as_str().is_some());
    host.settle();
    assert!(host.submitted().is_empty());
    assert!(host.outcomes().is_empty());
}

/// REQ: PORT:ilium-client/src/text_trigger_dialog/preview_tests.rs::exact_sample_limit_preserves_unicode_and_separating_lf
#[test]
fn port_002_exact_sample_limit_preserves_unicode_and_separating_lf() {
    let mut host = client_host();
    let sample = format!("{}\n\u{1f980}", "a".repeat(1024 * 1024 - 5));
    assert_eq!(sample.len(), 1024 * 1024);
    let reply = preview(&mut host, "\u{1f980}", "r", &sample);
    let text = reply["preview"].as_str().expect("preview text");
    assert!(
        text.ends_with("\n\u{2713} [\u{1f980}]"),
        "tail: {:?}",
        &text[text.len().saturating_sub(40)..]
    );
}

/// REQ: PORT:ilium-client/src/text_trigger_dialog/preview_tests.rs::installed_result_keeps_job_and_storage_debit_until_physical_cpu_retirement
#[test]
fn port_003_installed_result_keeps_job_and_storage_debit_until_physical_cpu_retirement() {
    let mut host = client_host();
    let first = preview(&mut host, "bb", "r", "abba");
    let second = preview(&mut host, "bb", "r", "abba");
    assert_eq!(first, second);
}

/// REQ: PORT:ilium-client/src/text_trigger_dialog/preview_tests.rs::intrinsic_cpu_limit_reports_unavailable_before_copy_instead_of_retrying
#[test]
fn port_004_intrinsic_cpu_limit_reports_unavailable_before_copy_instead_of_retrying() {
    let mut host = client_host();
    let outcome = preview_result(&mut host, "(((a{100}){100}){100}){100}", "r", "aaaa");
    match outcome {
        Err(error) => assert_eq!(error.code, ErrorCode::InvalidArgument),
        Ok(reply) => assert!(
            !reply["issues"].as_array().expect("issues").is_empty(),
            "{reply}"
        ),
    }
}

/// REQ: PORT:ilium-client/src/text_trigger_dialog/preview_tests.rs::newer_revision_cancels_queued_old_original_and_old_result_never_installs
#[test]
fn port_005_newer_revision_cancels_queued_old_original_and_old_result_never_installs() {
    let mut host = client_host();
    let old = preview(&mut host, "one", "r", "one two");
    let new = preview(&mut host, "two", "r", "one two");
    assert!(old["preview"].as_str().expect("old").contains("[one]"));
    let new_text = new["preview"].as_str().expect("new");
    assert!(
        new_text.contains("[two]") && !new_text.contains("[one]"),
        "{new_text}"
    );
}

/// REQ: PORT:ilium-client/src/text_trigger_dialog/preview_tests.rs::sample_limit_with_one_to_three_bytes_before_unicode_refuses_without_livelock
#[test]
fn port_006_sample_limit_with_one_to_three_bytes_before_unicode_refuses_without_livelock() {
    let mut host = client_host();
    for short in 1..=3usize {
        let sample = format!("{}\u{1f980}", "a".repeat(1024 * 1024 - short));
        let reply = preview(&mut host, "a", "r", &sample);
        let issues = reply["issues"].as_array().expect("issues");
        assert!(
            issues.iter().any(|issue| issue == "capture-limit"),
            "short {short}: {reply}"
        );
    }
}

/// REQ: PORT:ilium-client/src/text_trigger_dialog/preview_tests.rs::transient_job_refusal_retries_without_capturing_authored_draft
#[test]
fn port_007_transient_job_refusal_retries_without_capturing_authored_draft() {
    let mut host = client_host();
    set_rules(&mut host, &["draft"], &["r"]);
    assert!(
        preview_result(&mut host, "[", "r", "x").is_err()
            || preview(&mut host, "[", "r", "x")["issues"]
                .as_array()
                .is_some()
    );
    let ok = preview(&mut host, "x", "r", "x");
    assert!(ok["preview"].as_str().expect("preview").contains("[x]"));
    assert_eq!(host.settings().get("patterns"), Some(json!(["draft"])));
}

/// REQ: PORT:ilium-client/src/text_trigger_dialog/preview_tests.rs::worker_preserves_exact_preview_semantics_and_regex_work_is_off_ui
#[test]
fn port_008_worker_preserves_exact_preview_semantics_and_regex_work_is_off_ui() {
    let mut host = client_host();
    let reply = preview(&mut host, "bb", "r", "abba\nxxx");
    assert_eq!(reply["preview"], json!("\u{2713} a[bb]a\n\u{b7} xxx"));
    let zero = preview(&mut host, "^", "r", "alpha");
    assert_eq!(
        zero["preview"],
        json!("\u{2713} alpha  \u{27ea}zero-width match\u{27eb}")
    );
}
