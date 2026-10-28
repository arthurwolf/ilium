//! Manual page `reference-errors-and-limits`: the machine-wide failure stop after five failures in
//! 24 hours (HST-950), the stored failure times across restarts (HST-951) and their reset (HST-952).

use ilium_plugin_host::lifecycle::FailureLedger;

use super::common::{assert_page_states, manual_page};

const PAGE: &str = "reference-errors-and-limits.md";

/// REQ: HST-950
#[test]
fn hst_950_the_fifth_failure_in_24_hours_stops_the_side_with_stopped_by_failure() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "The host MUST stop a plugin side with the state `stopped-by-failure` when it has counted 5 failures (proposed, owner to confirm) within a sliding window of 24 hours",
    );
    assert_page_states(
        &page,
        "Plugin <name> stopped after 5 failures in 24 hours",
    );
}

/// REQ: HST-950
#[test]
fn hst_950_four_failures_in_24_hours_do_not_stop_the_side() {
    let hour = 3600;
    let mut ledger = FailureLedger::default();
    for failure in 0..4 {
        ledger.record(failure * hour);
    }
    assert!(!ledger.must_stop(5 * hour), "four failures do not stop the side");
    ledger.record(6 * hour);
    assert!(ledger.must_stop(7 * hour), "the fifth failure stops it");
    // Failures that age out of 24 hours stop counting.
    assert!(!ledger.must_stop(25 * hour + 1), "aged-out failures do not count");
    ledger.clear();
    assert_eq!(ledger.in_window(7 * hour), 0);
}

/// REQ: HST-951
#[test]
fn hst_951_each_counted_failure_time_is_written_to_installed_json_atomically() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "The host MUST write the wall-clock time of each counted failure into that side's entry in `installed.json`",
    );
}

/// REQ: HST-951
#[test]
fn hst_951_a_server_restart_neither_clears_failure_times_nor_counts_as_a_failure() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "A server restart MUST NOT clear them and MUST NOT count as a failure.",
    );
}

/// REQ: HST-952
#[test]
fn hst_952_enabling_resetting_or_updating_clears_the_24_hour_failure_times() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "Enabling the plugin, `ilium plugin reset-failures <id>` and installing an update MUST clear the 24-hour times of the affected side together with its 10-minute window",
    );
}

/// REQ: HST-952
#[test]
fn hst_952_failure_times_older_than_24_hours_do_not_count_toward_the_stop() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "Times that age out of the 24-hour window MUST NOT count toward [HST-950].",
    );
}
