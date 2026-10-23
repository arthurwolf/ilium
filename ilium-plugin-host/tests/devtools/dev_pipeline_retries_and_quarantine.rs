//! Retry, quarantine and flaky-test rules DEV-956, DEV-960, DEV-961 and DEV-963
//! (reference-test-harness.md, "Flaky tests and retries"). The documentation tests read the page;
//! the pipeline and runner behaviour is not in the repository yet and stays unimplemented.

use crate::dev_pipeline_manual::{manual_page, repository_file};

/// REQ: DEV-956
#[test]
fn dev_956_the_page_forbids_release_retries_and_allows_one_pull_request_retry() {
    let page = manual_page("reference-test-harness.md");
    assert!(
        page.contains("A release pipeline MUST NOT retry a failing plugin test."),
        "a release pipeline never retries a failing plugin test"
    );
    assert!(
        page.contains("A pull-request pipeline MAY retry it at most 1 time"),
        "a pull-request pipeline may retry at most one time"
    );
}

/// REQ: DEV-956
#[test]
fn dev_956_every_retry_attempt_starts_fresh_with_the_first_attempts_seed() {
    unimplemented!("ilium-plugin-host::cli::test::RetryPolicy::attempt_seed_and_fresh_instance");
}

/// REQ: DEV-960
#[test]
fn dev_960_the_page_limits_a_quarantine_expiry_to_14_days_after_it_is_added() {
    let page = manual_page("reference-test-harness.md");
    assert!(
        page.contains("A test MAY be quarantined only by a quarantine entry that names an owner and an expiry date"),
        "quarantine needs an owner and an expiry date"
    );
    assert!(
        page.contains("no more than 14 days after the day the entry is added"),
        "the expiry date is at most 14 days after the entry is added"
    );
}

/// REQ: DEV-960
#[test]
fn dev_960_a_quarantine_entry_expiring_after_14_days_is_refused() {
    unimplemented!("ilium-plugin-host::cli::test::Quarantine::validate_expiry_window");
}

/// REQ: DEV-961
#[test]
fn dev_961_conformance_cases_are_not_quarantinable_and_a_fail_verdict_exits_3() {
    let harness = manual_page("reference-test-harness.md");
    assert!(
        harness.contains("MUST NOT be quarantined"),
        "a conformance case must not be quarantined"
    );
    let conformance = repository_file("docs/plugins-manual/pages/reference-conformance-suite.md");
    assert!(
        conformance.contains("A `fail` verdict makes the exit code 3."),
        "a fail verdict of the conformance suite exits with code 3"
    );
}

/// REQ: DEV-961
#[test]
fn dev_961_a_quarantine_entry_for_a_conformance_case_is_refused() {
    unimplemented!("ilium-plugin-conformance::quarantine::refuse_conformance_case");
}

/// REQ: DEV-963
#[test]
fn dev_963_a_test_that_passes_on_a_retry_is_listed_as_flaky_with_its_attempt_count() {
    unimplemented!("ilium-plugin-host::cli::test::RunSummary::flaky_entries");
}

/// REQ: DEV-963
#[test]
fn dev_963_a_retried_test_that_recovers_is_never_reported_as_a_plain_pass() {
    unimplemented!("ilium-plugin-host::cli::test::RunSummary::plain_pass_after_retry");
}
