//! Rule statements of the guide-patterns-cookbook page as the manual states them (doc-lint tests).
use crate::support::{assert_page_line, assert_rule};

/// REQ: AUT-001
#[test]
fn doc_aut_001_core_must_not_call_the_clock_itself() {
    assert_rule(
        "AUT-001",
        "MUST NOT",
        &[
            "A core MUST NOT call the clock itself",
            "read the clock once per batch",
        ],
    );
}

/// REQ: AUT-002
#[test]
fn doc_aut_002_debounce_cancels_pending_timer_first() {
    assert_rule(
        "AUT-002",
        "MUST",
        &[
            "MUST cancel the pending timer before it arms a new one",
            "64 held timers",
        ],
    );
}

/// REQ: AUT-003
#[test]
fn doc_aut_003_handler_ignores_foreign_timer_ids() {
    assert_rule(
        "AUT-003",
        "MUST",
        &[
            "compare the id of each `timer-fired` event",
            "ignore every other id",
        ],
    );
}

/// REQ: AUT-004
#[test]
fn doc_aut_004_debounce_delay_at_least_50_ms() {
    assert_rule(
        "AUT-004",
        "MUST",
        &["50 ms or more", "refused with `invalid-argument`"],
    );
}

/// REQ: AUT-005 WARN
#[test]
fn doc_aut_005_own_send_rate_under_host_rate() {
    assert_rule(
        "AUT-005",
        "SHOULD NOT",
        &[
            "keep its own send rate under the host rate",
            "SHOULD NOT depend on `rate-limited`",
        ],
    );
}

/// REQ: AUT-006
#[test]
fn doc_aut_006_wait_retry_after_ms_after_rate_limited() {
    assert_rule(
        "AUT-006",
        "MUST",
        &["at least `retry-after-ms`", "`rate-limited`"],
    );
}

/// REQ: AUT-007
#[test]
fn doc_aut_007_throttle_uses_monotonic_millis() {
    assert_rule(
        "AUT-007",
        "MUST",
        &["`monotonic-millis`", "`saturating_sub`"],
    );
}

/// REQ: AUT-008
#[test]
fn doc_aut_008_rebuildable_data_not_in_durable_bucket() {
    assert_rule(
        "AUT-008",
        "MUST",
        &["`shared` bucket or to `/cache`", "16 MiB"],
    );
}

/// REQ: AUT-008
#[test]
fn doc_aut_008_rule_line_names_the_bucket_fill_consequence() {
    assert_page_line(
        "AUT-008",
        &["so that it does not fill the 16 MiB bucket"],
    );
}

/// REQ: AUT-009
#[test]
fn doc_aut_009_cached_value_carries_fetch_time() {
    assert_rule(
        "AUT-009",
        "MUST",
        &["`now-unix-millis`", "does not expire entries by age"],
    );
}

/// REQ: AUT-010 MAY
#[test]
fn doc_aut_010_only_lease_holder_refreshes() {
    assert_rule("AUT-010", "MAY", &["`lease` returned true"]);
}

/// REQ: AUT-011
#[test]
fn doc_aut_011_render_only_visible_window() {
    assert_rule(
        "AUT-011",
        "MUST NOT",
        &[
            "visible window plus a small margin",
            "4,096 nodes and 1,024 KiB",
        ],
    );
}

/// REQ: AUT-012
#[test]
fn doc_aut_012_window_shows_count_and_position() {
    assert_rule(
        "AUT-012",
        "MUST",
        &["`41 to 60 of 3,212`", "how many rows exist"],
    );
}

/// REQ: AUT-013
#[test]
fn doc_aut_013_permanent_errors_not_retried() {
    assert_rule(
        "AUT-013",
        "MUST NOT",
        &["`not-permitted`, `revoked`, `invalid-argument` or `unsupported`"],
    );
}

/// REQ: AUT-014
#[test]
fn doc_aut_014_retry_has_attempt_cap_and_growing_delay() {
    assert_rule(
        "AUT-014",
        "MUST",
        &[
            "maximum attempt count and a growing delay",
            "1 s, 2 s, 4 s, up to 60 s, with at most 5 attempts",
        ],
    );
}

/// REQ: AUT-015
#[test]
fn doc_aut_015_repeat_harmful_submission_carries_dedup_key() {
    assert_rule("AUT-015", "MUST", &["`dedup-key`", "an id and a revision"]);
}

/// REQ: AUT-016
#[test]
fn doc_aut_016_dedup_key_not_reused_within_10_minutes() {
    assert_rule(
        "AUT-016",
        "MUST NOT",
        &["within 10 minutes", "dropped as `duplicate`"],
    );
}

/// REQ: AUT-017
#[test]
fn doc_aut_017_restore_bytes_start_with_format_version() {
    assert_rule("AUT-017", "MUST", &["MUST fall back to defaults"]);
}

/// REQ: AUT-018
#[test]
fn doc_aut_018_state_outliving_trap_written_to_storage() {
    assert_rule(
        "AUT-018",
        "MUST",
        &[
            "written to storage when it changes",
            "not only in `deactivate`",
        ],
    );
}

/// REQ: AUT-019
#[test]
fn doc_aut_019_stale_completion_discarded_by_generation() {
    assert_rule(
        "AUT-019",
        "MUST",
        &["generation number", "discard a completion"],
    );
}

/// REQ: AUT-020
#[test]
fn doc_aut_020_batch_processed_in_order_before_deadline() {
    assert_rule(
        "AUT-020",
        "MUST",
        &[
            "every event of the batch in order",
            "(20 ms, 50 ms for tier 1)",
        ],
    );
}

/// REQ: AUT-021
#[test]
fn doc_aut_021_events_dropped_triggers_rebuild() {
    assert_rule("AUT-021", "MUST", &["`events-dropped`", "read call"]);
}

/// REQ: AUT-022
#[test]
fn doc_aut_022_slow_work_runs_as_job_or_request() {
    assert_rule(
        "AUT-022",
        "MUST",
        &["longer than 10 ms", "as a job or a request"],
    );
}

/// REQ: AUT-023
#[test]
fn doc_aut_023_client_request_has_explicit_timeout() {
    assert_rule(
        "AUT-023",
        "MUST",
        &["explicit timeout", "`timed-out` and `not-found`"],
    );
}

/// REQ: AUT-024
#[test]
fn doc_aut_024_changed_layout_raises_data_version_and_migrator() {
    assert_rule("AUT-024", "MUST", &["`data_version`", "`migrator` export"]);
}

/// REQ: AUT-025
#[test]
fn doc_aut_025_migrate_is_idempotent_and_writes_before_removing() {
    assert_rule(
        "AUT-025",
        "MUST",
        &["idempotent", "before it removes the old one"],
    );
}
