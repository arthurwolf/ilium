//! Requirements SEC-966 .. SEC-968 (reference-security-response.md): the security-fix window of each
//! host minor, the start-time advisory check with its warn line and `ilium version --json` field, and
//! the status bar badge for a High or Critical advisory.

use serde_json::json;

use crate::support::*;

/// REQ: SEC-966
#[test]
fn sec_966_each_host_minor_gets_security_fixes_for_at_least_twelve_months_after_release() {
    let page = manual_page("reference-security-response.md");
    assert!(
        page.contains("Each host minor receives security fixes for the window of [DEV-964]: at least 12 months (proposed, owner to confirm) after its release date."),
        "the page must state the 12-month minimum window after release"
    );
}

/// REQ: SEC-966
#[test]
fn sec_966_the_latest_minor_and_the_one_before_keep_fixes_even_after_their_end_date() {
    let page = manual_page("reference-security-response.md");
    assert!(
        page.contains("The latest minor and the one before it receive security fixes in every case, even after their end date (proposed, owner to confirm)."),
        "the page must keep the latest two minors supported past their end date"
    );
}

/// REQ: SEC-966
#[test]
fn sec_966_a_minor_past_its_end_date_reports_end_of_life_and_keeps_loading_plugins() {
    let page = manual_page("reference-security-response.md");
    assert!(
        page.contains("A minor past its end date reports `support: \"end-of-life\"`"),
        "the page must name the end-of-life support state"
    );
    assert!(
        page.contains("keeps loading its installed plugins unchanged."),
        "the page must keep installed plugins loading past the end date"
    );
}

/// REQ: SEC-966
#[test]
fn sec_966_a_host_past_its_end_date_still_loads_installed_plugins_unchanged() {
    unimplemented!("ilium_plugin_host::host_info::end_of_life_minor_loads_installed_plugins")
}

/// REQ: SEC-967
#[test]
fn sec_967_an_affected_host_writes_one_host_advisory_affected_warn_line_at_start() {
    unimplemented!("ilium_plugin_host::host_info::advisory_check_writes_one_warn_line_at_start")
}

/// REQ: SEC-967
#[test]
fn sec_967_ilium_version_json_lists_the_affected_advisory_ids_and_a_fixed_host_lists_none() {
    unimplemented!("ilium_plugin_host::cli::run version --json advisories field")
}

/// REQ: SEC-968
#[test]
fn sec_968_a_high_advisory_shows_the_update_badge_next_to_the_safe_badge() {
    let rig = setup(&[])
        .option(
            "runtime-advisory",
            json!({ "id": "ILIUM-2026-0001", "severity": "High", "fixed": "1.0.0" }),
        )
        .option("host-version", json!("0.9.0"))
        .start();
    let bar = rig.host.status_bar_text();
    assert!(
        bar.contains("Update: ILIUM-2026-0001"),
        "the status bar must show the advisory id: {bar}"
    );
    assert!(bar.contains("SAFE"), "the badge is shown next to SAFE: {bar}");
}

/// REQ: SEC-968
#[test]
fn sec_968_the_badge_disappears_only_when_the_host_reaches_the_fixed_version() {
    let rig = setup(&[])
        .option(
            "runtime-advisory",
            json!({ "id": "ILIUM-2026-0001", "severity": "High", "fixed": "1.0.0" }),
        )
        .option("host-version", json!("1.0.0"))
        .start();
    assert!(
        !rig.host.status_bar_text().contains("Update:"),
        "a host at the fixed version must show no update badge"
    );
}

/// REQ: SEC-968
#[test]
fn sec_968_the_advisory_page_states_the_badge_and_its_disappearance_rule() {
    let page = manual_page("reference-security-response.md");
    assert!(
        page.contains("the status bar MUST show `Update: <advisory id>` (proposed, owner to confirm) next to the `SAFE` badge of [SEC-329]"),
        "the page must state the badge text and its place next to SAFE"
    );
    assert!(
        page.contains("The badge MUST disappear only when the host reaches the `fixed` version of every such advisory."),
        "the page must state that the badge disappears only at the fixed version"
    );
}
