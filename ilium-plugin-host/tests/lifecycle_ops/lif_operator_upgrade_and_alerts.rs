//! Operator upgrade and alert rules (LIF-950..956).
//!
//! These rows are operator procedures and an alert table in the manual. A test rig cannot drive
//! an operator or an alerting system, so each test reads the manual page that states the rule and
//! asserts its concrete content: the files to keep, the alert thresholds, the sampling interval,
//! the stale-alert span and the mute boundary. Whether a real administrator follows them is a
//! process check outside the host and is not claimed here.

use crate::common::*;

/// REQ: LIF-950
#[test]
fn lif_950_operator_keeps_a_copy_of_the_previous_binary_and_plugin_state_files() {
    let page = manual_page("guide-operator-upgrades");
    assert!(
        page.contains("MUST keep a copy of the previous Ilium binary and of the plugin state files (`installed.json`, the plugin buckets and the stamps)"),
        "the upgrade page must require a copy of the previous binary and of installed.json, buckets and stamps"
    );
}

/// REQ: LIF-950
#[test]
fn lif_950_both_copies_are_kept_until_the_post_upgrade_checks_pass_and_one_binary_is_the_minimum() {
    let page = manual_page("guide-operator-upgrades");
    assert!(
        page.contains("MUST keep both until the checks in the next section pass"),
        "both copies must be kept until the post-upgrade checks pass"
    );
    assert!(
        page.contains("Keeping one previous binary is the minimum this page relies on"),
        "one previous binary is stated as the minimum, not as an optional choice"
    );
    assert!(
        !page.contains("MAY discard the copy"),
        "the page must not allow discarding the copy before the checks pass"
    );
}

/// REQ: LIF-951
#[test]
fn lif_951_no_enable_of_a_format_newer_plugin_after_rollback_until_host_or_file_is_restored() {
    let page = manual_page("guide-operator-upgrades");
    assert!(
        page.contains("MUST NOT enable a plugin whose log shows `host.format-newer` until either the newer host has been started again"),
        "the rollback page must forbid enabling a format-newer plugin before the newer host runs again"
    );
}

/// REQ: LIF-951
#[test]
fn lif_951_the_restore_alternative_names_the_copy_kept_under_lif_950_and_the_reason() {
    let page = manual_page("guide-operator-upgrades");
    assert!(
        page.contains("or the file named in that log line has been restored from the copy kept under LIF-950, because the earlier host cannot read that file"),
        "the page must allow the restored copy as the only other way to unblock, with the reason"
    );
}

/// REQ: LIF-952
#[test]
fn lif_952_the_operator_alert_table_lists_every_signal_with_its_source() {
    let page = manual_page("reference-health-and-watchdog");
    for row in [
        "| Plugin failed |",
        "| Plugin degraded |",
        "| Near-misses |",
        "| Traps |",
        "| Event queue |",
        "| Quota |",
        "| ABI failure |",
    ] {
        assert!(page.contains(row), "the alert table must contain the row {row}");
    }
}

/// REQ: LIF-952
#[test]
fn lif_952_the_alert_thresholds_match_the_stated_numbers_and_spans() {
    let page = manual_page("reference-health-and-watchdog");
    assert!(
        page.contains("| 5 or more in 5 minutes |"),
        "near-miss alert fires at 5 or more in 5 minutes"
    );
    assert!(
        page.contains("768 or more of the 1,024 queued events in 1 minute"),
        "event queue alert fires at 768 or more of 1,024 queued events in 1 minute"
    );
    assert!(
        page.contains("1 or more `quota-exceeded` or `rate-limited` results in 5 minutes"),
        "quota alert fires on one or more quota results in 5 minutes"
    );
}

/// REQ: LIF-952
#[test]
fn lif_952_operator_must_not_rely_on_logs_and_must_fire_at_the_threshold_not_before() {
    let page = manual_page("reference-health-and-watchdog");
    assert!(
        page.contains("MUST NOT rely on reading logs for these signals"),
        "the alert rule must forbid log reading as the only signal"
    );
    assert!(
        page.contains("assert that each alert fires when its threshold is met and not before"),
        "the test note must require firing at the threshold and not earlier"
    );
}

/// REQ: LIF-953
#[test]
fn lif_953_the_operator_samples_metrics_and_health_at_most_one_minute_apart() {
    let page = manual_page("reference-health-and-watchdog");
    assert!(
        page.contains("An administrator MUST sample `ilium plugin metrics` and `ilium plugin health` at an interval of at most 1 minute"),
        "the sampling interval must be at most 1 minute"
    );
}

/// REQ: LIF-953
#[test]
fn lif_953_the_one_minute_interval_is_the_shortest_span_of_the_degraded_table() {
    let page = manual_page("reference-health-and-watchdog");
    assert!(
        page.contains("which is the shortest time span in the degraded table"),
        "the one minute interval is tied to the shortest span of the degraded table"
    );
    assert!(
        page.contains("in 1 minute"),
        "a one minute span exists in the degraded table, so one minute is the shortest span"
    );
}

/// REQ: LIF-954
#[test]
fn lif_954_an_open_alert_stays_open_across_a_restart_until_a_healthy_sample() {
    let page = manual_page("reference-health-and-watchdog");
    assert!(
        page.contains("Counters reset at a server restart"),
        "the page must state that counters reset at a restart"
    );
    assert!(
        page.contains("an alert MUST NOT resolve because a counter fell"),
        "a fall in a counter must not resolve an alert"
    );
    assert!(
        page.contains("an alert that is open MUST stay open until a sample taken after the restart reports `healthy`"),
        "an open alert stays open until a healthy sample after the restart"
    );
}

/// REQ: LIF-954
#[test]
fn lif_954_the_metrics_reset_at_restart_is_stated_by_lif_136() {
    let page = manual_page("reference-requirements-lifecycle-ops");
    assert!(
        page.contains("A server restart MUST reset the metrics."),
        "LIF-136, which LIF-954 relies on, must state the metrics reset at restart"
    );
}

/// REQ: LIF-955
#[test]
fn lif_955_an_alert_older_than_three_minutes_shows_no_data_and_is_a_monitoring_failure() {
    let page = manual_page("reference-health-and-watchdog");
    assert!(
        page.contains("An alert whose newest sample is older than 3 minutes"),
        "the stale span is 3 minutes"
    );
    assert!(
        page.contains("A stale alert MUST show `no data`, MUST NOT show as clear, and MUST be treated as a failure of the monitoring."),
        "a stale alert must show no data, never clear, and count as a monitoring failure"
    );
}

/// REQ: LIF-955
#[test]
fn lif_955_the_three_minute_stale_span_is_exactly_three_sampling_intervals() {
    let page = manual_page("reference-health-and-watchdog");
    assert!(
        page.contains("which is three sampling intervals of LIF-953, is stale"),
        "the stale span is three sampling intervals of LIF-953"
    );
    assert!(
        page.contains("An administrator MUST sample `ilium plugin metrics` and `ilium plugin health` at an interval of at most 1 minute"),
        "three intervals of one minute give the three minute span"
    );
}

/// REQ: LIF-956
#[test]
fn lif_956_mute_does_not_silence_the_operator_alert() {
    let page = manual_page("reference-health-and-watchdog");
    assert!(
        page.contains("An operator alert MUST NOT be silenced by `ilium plugin mute <id>`, because that command mutes only the user notification of [HST-641]."),
        "the operator alert must not be silenced by the mute command"
    );
}

/// REQ: LIF-956
#[test]
fn lif_956_mute_is_scoped_to_the_user_notification_of_hst_641() {
    let page = manual_page("reference-health-and-watchdog");
    assert!(
        page.contains("MUST NOT raise one for a plugin the user has muted with `ilium plugin mute <id>`"),
        "HST-641 must scope the mute to the user notification only"
    );
}
