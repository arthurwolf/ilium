//! Lane `registry-publish`: dormant namespace policy (reference-registry.md, Namespaces).
//! Red phase: the policy functions are not implemented yet.

use super::common::page_text;

const DORMANT_AFTER_DAYS: u64 = 730;
const RECLAIM_NOTICE_DAYS: u64 = 90;

/// Whether a namespace is dormant at `now_day` (days since epoch).
fn namespace_is_dormant(
    last_publish_day: u64,
    last_install_day: u64,
    maintained_statement_day: Option<u64>,
    now_day: u64,
) -> bool {
    // A maintained statement counts as activity: it restarts the 730-day clock.
    let last_activity = last_publish_day
        .max(last_install_day)
        .max(maintained_statement_day.unwrap_or(0));
    now_day.saturating_sub(last_activity) >= DORMANT_AFTER_DAYS
}

/// Whether a dormant namespace may be reassigned.
fn namespace_may_be_reclaimed(
    dormant_days: u64,
    days_since_notice: Option<u64>,
    owner_replied: bool,
    versions_installed: bool,
) -> bool {
    dormant_days >= DORMANT_AFTER_DAYS
        && days_since_notice.is_some_and(|days| days >= RECLAIM_NOTICE_DAYS)
        && !owner_replied
        && !versions_installed
}

/// The client notice for a dormant namespace.
fn dormant_notice(last_publish: &str) -> String {
    format!(
        "This is a dormant namespace: its last publish was on {last_publish}. Installed versions keep working."
    )
}

/// REQ: PKG-655
#[test]
fn dormancy_needs_730_days_without_publish_or_install() {
    let _ = page_text("reference-registry.md");
    assert!(namespace_is_dormant(0, 0, None, DORMANT_AFTER_DAYS + 400));
    assert!(!namespace_is_dormant(0, 0, None, 100));
    assert!(!namespace_is_dormant(0, 800, None, 800 + 100));
}

/// REQ: PKG-655
#[test]
fn maintained_statement_defers_dormancy() {
    assert!(!namespace_is_dormant(0, 0, Some(900), 1_000));
    assert!(namespace_is_dormant(
        0,
        0,
        Some(10),
        10 + DORMANT_AFTER_DAYS + 400
    ));
}

/// REQ: PKG-656
#[test]
fn reclaim_requires_notice_wait_and_no_installs() {
    assert!(namespace_may_be_reclaimed(
        DORMANT_AFTER_DAYS,
        Some(RECLAIM_NOTICE_DAYS),
        false,
        false
    ));
    assert!(!namespace_may_be_reclaimed(
        DORMANT_AFTER_DAYS,
        None,
        false,
        false
    ));
    assert!(!namespace_may_be_reclaimed(
        DORMANT_AFTER_DAYS,
        Some(30),
        false,
        false
    ));
    assert!(!namespace_may_be_reclaimed(
        DORMANT_AFTER_DAYS,
        Some(100),
        true,
        false
    ));
    assert!(!namespace_may_be_reclaimed(
        DORMANT_AFTER_DAYS,
        Some(100),
        false,
        true
    ));
    assert!(!namespace_may_be_reclaimed(100, Some(100), false, false));
}

/// REQ: PKG-657
#[test]
fn client_shows_dormant_notice_with_last_publish_date() {
    let notice = dormant_notice("2027-03-01");
    assert!(notice.contains("dormant namespace") && notice.contains("2027-03-01"));
}

/// REQ: PKG-656
#[test]
fn reclaim_is_refused_one_day_before_the_dormancy_threshold() {
    assert!(!namespace_may_be_reclaimed(
        DORMANT_AFTER_DAYS - 1,
        Some(RECLAIM_NOTICE_DAYS),
        false,
        false
    ));
}

/// REQ: PKG-657
#[test]
fn dormant_notice_never_says_the_namespace_is_blocked() {
    let notice = dormant_notice("2027-03-01");
    assert!(!notice.to_lowercase().contains("block"), "{notice}");
}
