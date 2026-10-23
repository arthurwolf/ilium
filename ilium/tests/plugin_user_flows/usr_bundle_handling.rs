//! Administrator handling of a support bundle (USR-960 to USR-962). The runbook guide-admin-
//! failure-runbook.md states who may receive a bundle, how long it is kept and when it is read.

use crate::support::*;

fn states(page: &str, fact: &str) {
    assert!(page.contains(fact), "the runbook must state: {fact}");
}

/// REQ: USR-960
#[test]
fn usr_960_a_bundle_goes_outside_only_to_the_security_contact_or_project_contact() {
    let page = manual_page("guide-admin-failure-runbook");
    states(
        &page,
        "Send it outside your organisation only to the plugin's `security_contact` or to the Ilium project contact in the escalation path, and only over a channel your organisation approves.",
    );
}

/// REQ: USR-960
#[test]
fn usr_960_a_bundle_is_treated_as_sensitive_data() {
    let page = manual_page("guide-admin-failure-runbook");
    states(&page, "Treat every bundle as sensitive.");
}

/// REQ: USR-961
#[test]
fn usr_961_a_bundle_is_kept_no_longer_than_the_open_decision_retention() {
    let page = manual_page("guide-admin-failure-runbook");
    states(
        &page,
        "Keep it no longer than the retention in the Open decisions table (proposed, owner to confirm), and delete every copy at the end of that period, including copies in tickets and mail.",
    );
}

/// REQ: USR-961
#[test]
fn usr_961_the_open_decisions_row_gives_the_90_day_period_after_the_incident_closes() {
    let page = manual_page("guide-admin-failure-runbook");
    states(
        &page,
        "| Retention of a support bundle kept as evidence | 90 days after the incident is closed (proposed, owner to confirm).",
    );
}

/// REQ: USR-962
#[test]
fn usr_962_log_txt_is_read_before_any_transfer_and_the_bundle_is_refused_if_unshareable() {
    let page = manual_page("guide-admin-failure-runbook");
    states(
        &page,
        "Before any transfer, read `log.txt` and remove or refuse the bundle if it holds data you may not share.",
    );
}

/// REQ: USR-962
#[test]
fn usr_962_log_txt_may_hold_personal_data_that_redaction_does_not_remove() {
    let page = manual_page("guide-admin-failure-runbook");
    states(
        &page,
        "`log.txt` may contain names, messages or other personal data",
    );
}
