//! Rule statements AUT-951 to AUT-955: registry retirement notice, status page, maintenance notice,
//! the route for Ilium-caused failures, and the README statement for `http:` grants.
use crate::support::{assert_page_line, assert_rule, table_row};

/// REQ: AUT-951
#[test]
fn aut_951_retirement_announced_at_least_90_days_before_it_takes_effect() {
    assert_rule(
        "AUT-951",
        "MUST",
        &["MUST be announced at least 90 days before it takes effect"],
    );
}

/// REQ: AUT-951
#[test]
fn aut_951_retirement_covers_registry_url_mirror_and_api_version() {
    assert_rule(
        "AUT-951",
        "MUST",
        &["a registry URL, a mirror or an API version"],
    );
}

/// REQ: AUT-952
#[test]
fn aut_952_operator_names_status_page_and_updates_it_within_one_hour() {
    assert_rule(
        "AUT-952",
        "MUST",
        &["MUST name a status page", "within 1 hour"],
    );
}

/// REQ: AUT-952
#[test]
fn aut_952_status_page_update_clock_starts_at_incident_affecting_installs_or_updates() {
    assert_rule(
        "AUT-952",
        "MUST",
        &["of the start of an incident that affects installs or updates"],
    );
}

/// REQ: AUT-953
#[test]
fn aut_953_planned_maintenance_announced_at_least_48_hours_before_start() {
    assert_rule(
        "AUT-953",
        "MUST",
        &["MUST be announced at least 48 hours before it starts"],
    );
}

/// REQ: AUT-953
#[test]
fn aut_953_maintenance_announcement_names_the_operations_that_stop() {
    assert_rule(
        "AUT-953",
        "MUST",
        &["the announcement MUST say which operations stop during it"],
    );
}

/// REQ: AUT-954
#[test]
fn aut_954_ilium_caused_failure_is_reported_to_ilium_and_readme_names_the_route() {
    assert_rule(
        "AUT-954",
        "MUST",
        &[
            "tell the user to report it to the Ilium project",
            "README MUST name",
        ],
    );
}

/// REQ: AUT-954
#[test]
fn aut_954_ilium_caused_failure_is_not_redirected_to_the_plugin_author() {
    assert_page_line(
        "AUT-954",
        &["caused the failure", "not to you"],
    );
}

/// REQ: AUT-955
#[test]
fn aut_955_readme_states_data_sent_to_each_http_grant_host_and_privacy_contact_before_first_release() {
    let row = table_row("AUT-955");
    for needle in [
        "`http:` grant",
        "state in the README which data the plugin sends to each granted host",
        "name a contact for privacy and erasure requests there",
        "before the first release",
    ] {
        assert!(row.contains(needle), "AUT-955 row lacks {needle:?}: {row}");
    }
}

/// REQ: AUT-955
#[test]
fn aut_955_publisher_not_ilium_is_responsible_for_the_sent_data() {
    let row = table_row("AUT-955");
    assert!(
        row.contains("The publisher is responsible for that data, not Ilium."),
        "AUT-955 row lacks the publisher responsibility sentence: {row}"
    );
}
