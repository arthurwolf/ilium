//! Support windows of host releases and major-version announcements (DEV-964, DEV-966, DEV-995).
//! Page: reference-abi-major-versions.

use crate::{
    major_announcement, manual_page, patch_release_exists_for, plus_twelve_months, release_surface,
    support_dates_of_minor, supported_minors_at,
};

/// REQ: DEV-964
#[test]
fn each_minor_has_twelve_months_of_security_fixes_with_end_date_in_the_support_matrix() {
    let page = manual_page("reference-abi-major-versions");
    assert!(
        page.contains("12 months"),
        "the support period is 12 months"
    );
    assert!(
        page.to_lowercase().contains("abi minor") && page.to_lowercase().contains("not"),
        "the ABI minor window is a loading rule and not the support period"
    );
    let matrix = manual_page("reference-toolchain-support-matrix").to_lowercase();
    assert!(
        matrix.contains("end of support"),
        "the support matrix publishes the end date"
    );

    let (release_date, end_date) = support_dates_of_minor("1.4");
    assert_eq!(
        end_date,
        plus_twelve_months(&release_date),
        "the end date is twelve months after the release date"
    );
}

/// REQ: DEV-966
#[test]
fn security_patch_ships_to_every_supported_minor_without_changing_any_interface() {
    let page = manual_page("reference-abi-major-versions");
    assert!(page.contains("patch"), "the page describes patch releases");
    let before = release_surface("1.4.0");
    let after = release_surface("1.4.1");
    assert_eq!(
        before, after,
        "a patch release does not change any WIT interface, limit, error code or manifest key"
    );
    for minor in supported_minors_at("1.4.1") {
        assert!(
            patch_release_exists_for(&minor),
            "every minor inside its support period gets the security patch: {minor}"
        );
    }
}

/// REQ: DEV-995
#[test]
fn major_release_is_announced_six_months_ahead_and_the_announced_list_does_not_grow() {
    let page = manual_page("reference-abi-major-versions");
    assert!(
        page.contains("6 months"),
        "the announcement period is six months"
    );
    let announcement = major_announcement("2.0.0");
    assert!(
        announcement.days_before_release >= 180,
        "announced at least 6 months before release: {} days",
        announcement.days_before_release
    );
    assert_eq!(
        announcement.removals_at_release, announcement.removals_announced,
        "the announced list of removals and limit changes does not grow"
    );
}

/// REQ: DEV-964
#[test]
fn a_minor_is_listed_as_supported_while_inside_its_twelve_month_window() {
    assert!(
        supported_minors_at("1.4.1").contains(&"1.4".to_string()),
        "minor 1.4 is inside its support period at 1.4.1"
    );
}

/// REQ: DEV-966
#[test]
fn the_patch_rule_is_stated_for_minors_inside_their_support_period() {
    let page = manual_page("reference-abi-major-versions");
    assert!(
        page.contains("inside its support period"),
        "the page states the patch rule for minors inside their support period"
    );
    assert!(
        patch_release_exists_for("1.4"),
        "a patch release exists for minor 1.4"
    );
}

/// REQ: DEV-995
#[test]
fn major_announcement_for_two_point_zero_is_published_before_its_release() {
    let announcement = major_announcement("2.0.0");
    assert!(
        announcement.days_before_release > 0,
        "the announcement precedes the release: {} days",
        announcement.days_before_release
    );
}
