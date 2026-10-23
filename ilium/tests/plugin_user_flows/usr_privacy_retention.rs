//! Privacy rules for stored plugin data and external services (USR-955 to USR-958). The retention
//! table and the privacy statements are in user-privacy.md; the geocode provider rule is a behaviour
//! rule that needs a capture proxy and stays an unimplemented! stub until the rig exists.

use crate::support::*;

fn states(page: &str, fact: &str) {
    assert!(page.contains(fact), "the page must state: {fact}");
}

/// REQ: USR-955
#[test]
fn usr_955_every_kind_of_stored_data_has_a_retention_row_with_its_deleting_command() {
    let page = manual_page("user-privacy");
    states(&page, "| What | Default retention | Deleted by |");
    states(&page, "| Plugin logs |");
    states(&page, "| Crash records |");
    states(&page, "| Pre-migration backups |");
    states(&page, "| Support-bundle staging files |");
    states(&page, "| `installed.json` extras that hold settings values |");
}

/// REQ: USR-955
#[test]
fn usr_955_a_kind_without_a_deleting_command_is_named_as_kept() {
    let page = manual_page("user-privacy");
    states(
        &page,
        "| Audit log | `audit.keep_days`, default 365 days ([SEC-224]) | `erase-data` does not delete it before the period ends.",
    );
    states(
        &page,
        "| Geocode results | 24 hours, keyed by query text ([USR-957]) | Not in the erase list of [USR-326] (owner to confirm) |",
    );
}

/// REQ: USR-956
#[test]
fn usr_956_the_registry_privacy_notice_covers_ip_address_and_user_agent_retention() {
    let page = manual_page("user-privacy");
    states(
        &page,
        "A registry you configure receives each request you make to it, as any web server does. Each request carries your IP address and the `User-Agent` header.",
    );
    states(
        &page,
        "[USR-956] A registry operator MUST publish a privacy notice that states how long its access logs keep the IP address and `User-Agent` of each request, and that period MUST NOT exceed 30 days",
    );
}

/// REQ: USR-956
#[test]
fn usr_956_ilium_states_no_access_log_period_of_its_own_for_a_registry() {
    let page = manual_page("user-privacy");
    let line = page
        .lines()
        .find(|line| line.contains("The registry operator decides how long its access logs keep them"))
        .expect("the registry paragraph must name the operator's log period");
    assert!(
        !line.contains("day"),
        "the paragraph must leave the period to the operator, not give a day count: {line}"
    );
}

/// REQ: USR-957
#[test]
fn usr_957_the_search_and_reverse_lookup_send_only_query_text_or_coordinates() {
    let page = manual_page("user-privacy");
    states(
        &page,
        "A search sends the query text, between 2 and 200 characters, to Photon, with Nominatim as fallback. A reverse lookup sends the coordinates of the point.",
    );
}

/// REQ: USR-957
#[test]
fn usr_957_a_cached_geocode_result_is_kept_for_24_hours_and_no_longer() {
    let page = manual_page("user-privacy");
    states(
        &page,
        "The host keeps each result for 24 hours, keyed by query text",
    );
}

/// REQ: USR-957
#[test]
fn usr_957_the_provider_request_carries_only_those_fields_and_the_cache_expires() {
    unimplemented!("ilium_plugin_host::testing::user_rig::UserRig::geocode_capture")
}

/// REQ: USR-958
#[test]
fn usr_958_a_written_request_is_answered_within_30_days_and_names_the_holding_party() {
    let page = manual_page("user-privacy");
    states(
        &page,
        "[USR-958] The Ilium project MUST answer a written request for access to or erasure of data it holds within 30 days (proposed, owner to confirm), and MUST name the party that holds any data it does not hold.",
    );
}

/// REQ: USR-958
#[test]
fn usr_958_data_held_by_a_registry_provider_or_host_is_asked_of_that_party() {
    let page = manual_page("user-privacy");
    states(
        &page,
        "For data that a registry, a provider or a host holds, ask that party. Ask the Ilium project about anything it holds.",
    );
}
