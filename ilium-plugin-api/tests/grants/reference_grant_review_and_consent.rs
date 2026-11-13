//! SEC-1xx rows of reference-requirements-security-policy.md that the pure contract crate can
//! answer: grant fitting for the tier at install step 8 and the pipeline table of the page.
//! The dialog, store and CLI rows are in the host half of this lane.

use ilium_plugin_api::grants::{check_grant, Grant};
use ilium_plugin_api::{Side, Tier};

use crate::support::{manual_page, reports};

fn grant(text: &str) -> Grant {
    Grant::parse(text).expect("sessions.cross is a known grant")
}

/// REQ: SEC-124
#[test]
fn sec_124_sessions_cross_in_a_t3_package_is_grant_tier() {
    let result = check_grant(&grant("sessions.cross"), Tier::T3, Side::Server);
    assert_eq!(
        result.expect_err("T3 is refused").code.as_str(),
        "manifest.grant-tier"
    );
}

/// REQ: SEC-124
#[test]
fn sec_124_install_check_reports_grant_tier_for_t3_sessions_cross() {
    assert!(reports(
        Tier::T3,
        Side::Server,
        &["sessions.cross"],
        "manifest.grant-tier"
    ));
}

/// REQ: SEC-124
#[test]
fn sec_124_sessions_cross_is_allowed_for_t1_and_t2() {
    for tier in [Tier::T1, Tier::T2] {
        assert!(
            check_grant(&grant("sessions.cross"), tier, Side::Server).is_ok(),
            "{tier}"
        );
        assert!(
            !reports(
                tier,
                Side::Server,
                &["sessions.cross"],
                "manifest.grant-tier"
            ),
            "{tier}"
        );
    }
}

/// REQ: SEC-124
#[test]
fn sec_124_other_grants_stay_allowed_in_t3() {
    assert!(check_grant(&grant("terminal.history"), Tier::T3, Side::Server).is_ok());
    assert!(!reports(
        Tier::T3,
        Side::Server,
        &["terminal.history"],
        "manifest.grant-tier"
    ));
}

/// REQ: SEC-124
#[test]
fn doc_sec_124_page_states_the_step_and_the_code() {
    let page = manual_page("reference-grant-review-and-consent.md");
    assert!(page.contains(
        "`sessions.cross` in a T3 package is refused at step 8 with `manifest.grant-tier`"
    ));
}

/// REQ: SEC-100
#[test]
fn doc_sec_100_pipeline_has_eleven_steps_in_the_documented_order() {
    let page = manual_page("reference-grant-review-and-consent.md");
    let table = page
        .split("| Step | Check | Failure code |")
        .nth(1)
        .expect("pipeline table");
    let steps: Vec<u32> = table
        .lines()
        .filter(|line| line.starts_with("| ") && !line.starts_with("| ---"))
        .take_while(|line| {
            line.split('|')
                .nth(1)
                .is_some_and(|cell| cell.trim().parse::<u32>().is_ok())
        })
        .map(|line| {
            line.split('|')
                .nth(1)
                .expect("step cell")
                .trim()
                .parse()
                .expect("step number")
        })
        .collect();
    assert_eq!(steps, (1..=11).collect::<Vec<u32>>());
}

/// REQ: SEC-100
#[test]
fn doc_sec_100_step_eight_lists_the_grant_codes() {
    let page = manual_page("reference-grant-review-and-consent.md");
    let row = page
        .lines()
        .find(|line| line.starts_with("| 8 |"))
        .expect("step 8 row");
    for code in [
        "manifest.grant-tier",
        "manifest.grant-argument",
        "manifest.grant-argument-missing",
        "manifest.grant-wildcard",
        "manifest.grant-scope",
        "manifest.grant-dependency",
        "manifest.limits-tier",
        "manifest.secret-scope",
        "manifest.replaces-tier",
    ] {
        assert!(row.contains(code), "step 8 lists {code}");
    }
}
