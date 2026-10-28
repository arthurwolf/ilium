//! Twins of ilium-client/src/cost_tracker/preparation.rs: the bounded CPU/IO preparation of the cost overlay, the
//! acknowledged-row bookkeeping and the history-cache generations.
//!
//! The originals drive a tracker with blocked workers and a real filesystem. The twins call one scenario op per
//! original, `prep.<name-with-dashes>`, with the scenario parameters and assert the key observable values the
//! original asserted (see ../API_GAPS_ports.md).
use serde_json::{json, Value};

use crate::common::client;

fn scenario(name: &str, request: Value) -> Value {
    client(&format!("prep.{name}"), request)
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::acknowledged_rows_keep_exact_card_title_and_storage_after_new_source_and_shutdown
#[test]
fn port_cost_tracker_preparation_acknowledged_rows_keep_exact_card_title_and_storage_after_new_source_and_shutdown(
) {
    let reply = scenario(
        "acknowledged-rows-keep-exact-card-title-and-storage-after-new-source-and-shutdown",
        json!({ "fixture": "actual-cost-ack-fixture", "acknowledged_title": "title actually emitted", "stale_pointer_row": 99 }),
    );
    assert!(
        reply["card"]["text"]
            .as_str()
            .unwrap_or("")
            .contains("$1.00"),
        "{reply}"
    );
    assert_eq!(
        reply["card"]["overlay_revision"],
        reply["tracker"]["overlay_revision"]
    );
    assert_eq!(reply["newer_card_differs_from_old"], json!(true));
    assert_eq!(reply["first_ready_card_requests_first_paint"], json!(true));
    assert_eq!(
        reply["card_anchor_row"], reply["acknowledged_title_row"],
        "card anchors to the acknowledged title row, not the stale pointer row 99"
    );
    assert_eq!(
        reply["values"]["cost_title"],
        json!("title actually emitted")
    );
    assert_eq!(
        reply["same_source_title_row_requests_redraw"],
        json!(false),
        "no ACK redraw loop"
    );
    assert_eq!(
        reply["changed_cpu_source_schedules_replacement_paint"],
        json!(true)
    );
    assert_eq!(reply["new_source_settles_after_one_ack_paint"], json!(true));
    assert_eq!(
        reply["old_values"]["cost_title"],
        json!("title actually emitted")
    );
    assert_eq!(
        reply["finite_job_envelopes_pinned_by_cards"],
        json!(0),
        "cards do not pin finite job envelopes"
    );
    assert_eq!(
        reply["retained_ack_rows_keep_storage_after_shutdown"],
        json!(true)
    );
    assert_eq!(reply["last_ack_row_releases_card_allocation"], json!(true));
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::blocked_cpu_keeps_ui_nonblocking_fences_revisions_and_does_not_starve_time
#[test]
fn port_cost_tracker_preparation_blocked_cpu_keeps_ui_nonblocking_fences_revisions_and_does_not_starve_time(
) {
    let reply = scenario(
        "blocked-cpu-keeps-ui-nonblocking-fences-revisions-and-does-not-starve-time",
        json!({ "block_cpu": true, "time_offsets_ms": [1, 2, 3], "deadline_secs": 5 }),
    );
    assert_eq!(reply["tick_returned_changed"], json!(false));
    assert!(
        reply["tick_elapsed_ms"]
            .as_u64()
            .is_some_and(|elapsed| elapsed < 100),
        "the UI thread never blocks on the CPU lane"
    );
    assert_eq!(
        reply["cpu_jobs_started"],
        json!(1),
        "time updates cannot multiply CPU jobs"
    );
    assert_eq!(
        reply["wrong_metric_published"],
        json!(false),
        "wrong metric cannot publish"
    );
    assert_eq!(reply["metric_after_settle"], json!("quota"));
    assert_eq!(reply["amount_after_settle"], json!(1.0));
    assert_eq!(reply["remaining_jobs"], json!(0));
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::cache_change_discards_queued_io_under_fixed_bands
#[test]
fn port_cost_tracker_preparation_cache_change_discards_queued_io_under_fixed_bands() {
    let reply = scenario(
        "cache-change-discards-queued-io-under-fixed-bands",
        json!({ "ready": false, "ineligible_tick": "fixed_bands" }),
    );
    assert_eq!(
        reply["old_receipt_installed"],
        json!(false),
        "a receipt of the previous cache path is never installed"
    );
    assert_eq!(reply["history_entries"], json!(0));
    assert_eq!(reply["shutdown_complete"], json!(true));
    assert_eq!(reply["remaining_workers"], json!(0));
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::cache_change_discards_queued_io_when_display_is_disabled
#[test]
fn port_cost_tracker_preparation_cache_change_discards_queued_io_when_display_is_disabled() {
    let reply = scenario(
        "cache-change-discards-queued-io-when-display-is-disabled",
        json!({ "ready": false, "ineligible_tick": "disabled" }),
    );
    assert_eq!(
        reply["old_receipt_installed"],
        json!(false),
        "a receipt of the previous cache path is never installed"
    );
    assert_eq!(reply["history_entries"], json!(0));
    assert_eq!(reply["shutdown_complete"], json!(true));
    assert_eq!(reply["remaining_workers"], json!(0));
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::cache_change_discards_queued_io_without_home
#[test]
fn port_cost_tracker_preparation_cache_change_discards_queued_io_without_home() {
    let reply = scenario(
        "cache-change-discards-queued-io-without-home",
        json!({ "ready": false, "ineligible_tick": "no_home" }),
    );
    assert_eq!(
        reply["old_receipt_installed"],
        json!(false),
        "a receipt of the previous cache path is never installed"
    );
    assert_eq!(reply["history_entries"], json!(0));
    assert_eq!(reply["shutdown_complete"], json!(true));
    assert_eq!(reply["remaining_workers"], json!(0));
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::cache_change_discards_ready_io_under_fixed_bands
#[test]
fn port_cost_tracker_preparation_cache_change_discards_ready_io_under_fixed_bands() {
    let reply = scenario(
        "cache-change-discards-ready-io-under-fixed-bands",
        json!({ "ready": true, "ineligible_tick": "fixed_bands" }),
    );
    assert_eq!(
        reply["old_receipt_installed"],
        json!(false),
        "a receipt of the previous cache path is never installed"
    );
    assert_eq!(reply["history_entries"], json!(0));
    assert_eq!(reply["shutdown_complete"], json!(true));
    assert_eq!(reply["remaining_workers"], json!(0));
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::cache_change_discards_ready_io_when_display_is_disabled
#[test]
fn port_cost_tracker_preparation_cache_change_discards_ready_io_when_display_is_disabled() {
    let reply = scenario(
        "cache-change-discards-ready-io-when-display-is-disabled",
        json!({ "ready": true, "ineligible_tick": "disabled" }),
    );
    assert_eq!(
        reply["old_receipt_installed"],
        json!(false),
        "a receipt of the previous cache path is never installed"
    );
    assert_eq!(reply["history_entries"], json!(0));
    assert_eq!(reply["shutdown_complete"], json!(true));
    assert_eq!(reply["remaining_workers"], json!(0));
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::cache_change_discards_ready_io_without_home
#[test]
fn port_cost_tracker_preparation_cache_change_discards_ready_io_without_home() {
    let reply = scenario(
        "cache-change-discards-ready-io-without-home",
        json!({ "ready": true, "ineligible_tick": "no_home" }),
    );
    assert_eq!(
        reply["old_receipt_installed"],
        json!(false),
        "a receipt of the previous cache path is never installed"
    );
    assert_eq!(reply["history_entries"], json!(0));
    assert_eq!(reply["shutdown_complete"], json!(true));
    assert_eq!(reply["remaining_workers"], json!(0));
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::cache_change_releases_installed_generation_after_independent_last_reader
#[test]
fn port_cost_tracker_preparation_cache_change_releases_installed_generation_after_independent_last_reader(
) {
    let reply = scenario(
        "cache-change-releases-installed-generation-after-independent-last-reader",
        json!({ "entries": 1, "independent_reader": true, "new_cache_path": "new-cache.json" }),
    );
    assert_eq!(reply["snapshot_entries"], json!(1));
    assert_eq!(reply["entries_after_path_change"], json!(0));
    assert_eq!(reply["reader_entries_after_path_change"], json!(1));
    assert_eq!(reply["reader_pointer_unchanged"], json!(true));
    assert_eq!(reply["shutdown_complete"], json!(true));
    assert_eq!(reply["retirement_live"], json!(0));
    assert_eq!(reply["remaining_workers"], json!(0));
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::changing_history_cache_path_fences_an_already_captured_cpu_generation
#[test]
fn port_cost_tracker_preparation_changing_history_cache_path_fences_an_already_captured_cpu_generation(
) {
    let reply = scenario(
        "changing-history-cache-path-fences-an-already-captured-cpu-generation",
        json!({ "new_cache_path": "/worker879/new-history-cache" }),
    );
    assert_eq!(reply["identity_matches_before_change"], json!(true));
    assert_eq!(reply["identity_matches_after_change"], json!(false));
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::completed_history_receipt_drains_before_a_new_cpu_admission_at_original_limits
#[test]
fn port_cost_tracker_preparation_completed_history_receipt_drains_before_a_new_cpu_admission_at_original_limits(
) {
    let reply = scenario(
        "completed-history-receipt-drains-before-a-new-cpu-admission-at-original-limits",
        json!({ "transcript_lines": ["{\"type\":\"user\",\"message\":{\"content\":\"hi\"}}", "{\"type\":\"cost-state\",\"sessionId\":\"only\",\"totalCostUSD\":1.0}", "{\"type\":\"assistant\",\"message\":{\"content\":[]}}"], "input_byte_limit_mib": 512 }),
    );
    assert_eq!(
        reply["input_bytes_held_while_receipt_pending"],
        json!(512 * 1024 * 1024)
    );
    assert_eq!(reply["history_entries"], json!(1));
    assert_eq!(
        reply["receipt_released_before_cpu_admission"],
        json!(true),
        "coordinator must release the original 64 MiB receipt before CPU admission"
    );
    assert_eq!(reply["engine_history_entries"], json!(1));
    assert_eq!(reply["history_entries_after_path_change"], json!(0));
    assert_eq!(reply["shutdown_complete"], json!(true));
    assert_eq!(reply["remaining_workers"], json!(0));
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::ordinary_cancelled_rescan_preserves_the_previous_complete_generation
#[test]
fn port_cost_tracker_preparation_ordinary_cancelled_rescan_preserves_the_previous_complete_generation(
) {
    let reply = scenario(
        "ordinary-cancelled-rescan-preserves-the-previous-complete-generation",
        json!({ "first_scan_entries": 1, "cancel_second_scan": true }),
    );
    assert_eq!(reply["first_entries"], json!(1));
    assert_eq!(reply["has_error_after_cancel"], json!(true));
    assert_eq!(reply["has_result_after_cancel"], json!(true));
    assert_eq!(reply["entries_after_cancel"], json!(1));
    assert_eq!(reply["entries_pointer_unchanged"], json!(true));
    assert_eq!(reply["shutdown_complete"], json!(true));
    assert_eq!(reply["remaining_workers"], json!(0));
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::oversized_price_capacity_refuses_before_copying_and_keeps_old_overlay
#[test]
fn port_cost_tracker_preparation_oversized_price_capacity_refuses_before_copying_and_keeps_old_overlay(
) {
    let reply = scenario(
        "oversized-price-capacity-refuses-before-copying-and-keeps-old-overlay",
        json!({ "model_name_bytes": "oversized", "capacity": "exceeded" }),
    );
    assert_eq!(reply["tick_changed"], json!(false));
    assert_eq!(reply["active_job"], Value::Null);
    assert_eq!(reply["overlay_unchanged"], json!(true));
    assert!(
        reply["history_error"]
            .as_str()
            .unwrap_or("")
            .contains("settings exceed"),
        "{reply}"
    );
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::acknowledged_rows_keep_exact_card_title_and_storage_after_new_source_and_shutdown
#[test]
fn port_cost_tracker_preparation_card_anchor_never_follows_the_stale_pointer_row() {
    let reply = scenario(
        "acknowledged-rows-keep-exact-card-title-and-storage-after-new-source-and-shutdown",
        json!({ "fixture": "actual-cost-ack-fixture", "acknowledged_title": "title actually emitted", "stale_pointer_row": 99 }),
    );
    assert_ne!(reply["card_anchor_row"], json!(99), "{reply}");
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::blocked_cpu_keeps_ui_nonblocking_fences_revisions_and_does_not_starve_time
#[test]
fn port_cost_tracker_preparation_every_time_update_is_applied_once_while_cpu_is_blocked() {
    let reply = scenario(
        "blocked-cpu-keeps-ui-nonblocking-fences-revisions-and-does-not-starve-time",
        json!({ "block_cpu": true, "time_offsets_ms": [1, 2, 3], "deadline_secs": 5 }),
    );
    assert_eq!(reply["time_updates_applied"], json!(3), "{reply}");
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::cache_change_discards_queued_io_under_fixed_bands
#[test]
fn port_cost_tracker_preparation_queued_fixed_bands_io_holds_no_quota_bytes_after_discard() {
    let reply = scenario(
        "cache-change-discards-queued-io-under-fixed-bands",
        json!({ "ready": false, "ineligible_tick": "fixed_bands" }),
    );
    assert_eq!(reply["quota_input_bytes_after_discard"], json!(0), "{reply}");
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::cache_change_discards_queued_io_when_display_is_disabled
#[test]
fn port_cost_tracker_preparation_queued_disabled_display_io_holds_no_quota_bytes_after_discard() {
    let reply = scenario(
        "cache-change-discards-queued-io-when-display-is-disabled",
        json!({ "ready": false, "ineligible_tick": "disabled" }),
    );
    assert_eq!(reply["quota_input_bytes_after_discard"], json!(0), "{reply}");
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::cache_change_discards_queued_io_without_home
#[test]
fn port_cost_tracker_preparation_queued_no_home_io_holds_no_quota_bytes_after_discard() {
    let reply = scenario(
        "cache-change-discards-queued-io-without-home",
        json!({ "ready": false, "ineligible_tick": "no_home" }),
    );
    assert_eq!(reply["quota_input_bytes_after_discard"], json!(0), "{reply}");
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::cache_change_discards_ready_io_under_fixed_bands
#[test]
fn port_cost_tracker_preparation_ready_fixed_bands_io_holds_no_quota_bytes_after_discard() {
    let reply = scenario(
        "cache-change-discards-ready-io-under-fixed-bands",
        json!({ "ready": true, "ineligible_tick": "fixed_bands" }),
    );
    assert_eq!(reply["quota_input_bytes_after_discard"], json!(0), "{reply}");
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::cache_change_discards_ready_io_when_display_is_disabled
#[test]
fn port_cost_tracker_preparation_ready_disabled_display_io_holds_no_quota_bytes_after_discard() {
    let reply = scenario(
        "cache-change-discards-ready-io-when-display-is-disabled",
        json!({ "ready": true, "ineligible_tick": "disabled" }),
    );
    assert_eq!(reply["quota_input_bytes_after_discard"], json!(0), "{reply}");
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::cache_change_discards_ready_io_without_home
#[test]
fn port_cost_tracker_preparation_ready_no_home_io_holds_no_quota_bytes_after_discard() {
    let reply = scenario(
        "cache-change-discards-ready-io-without-home",
        json!({ "ready": true, "ineligible_tick": "no_home" }),
    );
    assert_eq!(reply["quota_input_bytes_after_discard"], json!(0), "{reply}");
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::cache_change_releases_installed_generation_after_independent_last_reader
#[test]
fn port_cost_tracker_preparation_generation_is_released_once_the_independent_reader_drops() {
    let reply = scenario(
        "cache-change-releases-installed-generation-after-independent-last-reader",
        json!({ "entries": 1, "independent_reader": true, "new_cache_path": "new-cache.json" }),
    );
    assert_eq!(reply["reader_entries_after_reader_drop"], json!(0), "{reply}");
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::changing_history_cache_path_fences_an_already_captured_cpu_generation
#[test]
fn port_cost_tracker_preparation_captured_cpu_generation_is_not_admitted_after_path_change() {
    let reply = scenario(
        "changing-history-cache-path-fences-an-already-captured-cpu-generation",
        json!({ "new_cache_path": "/worker879/new-history-cache" }),
    );
    assert_eq!(reply["cpu_generation_admitted_after_change"], json!(false), "{reply}");
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::completed_history_receipt_drains_before_a_new_cpu_admission_at_original_limits
#[test]
fn port_cost_tracker_preparation_pending_receipt_is_charged_the_original_64_mib_limit() {
    let reply = scenario(
        "completed-history-receipt-drains-before-a-new-cpu-admission-at-original-limits",
        json!({ "transcript_lines": ["{\"type\":\"user\",\"message\":{\"content\":\"hi\"}}", "{\"type\":\"cost-state\",\"sessionId\":\"only\",\"totalCostUSD\":1.0}", "{\"type\":\"assistant\",\"message\":{\"content\":[]}}"], "input_byte_limit_mib": 512 }),
    );
    assert_eq!(reply["receipt_limit_bytes"], json!(64 * 1024 * 1024), "{reply}");
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::ordinary_cancelled_rescan_preserves_the_previous_complete_generation
#[test]
fn port_cost_tracker_preparation_cancelled_rescan_reports_a_cancellation_error() {
    let reply = scenario(
        "ordinary-cancelled-rescan-preserves-the-previous-complete-generation",
        json!({ "first_scan_entries": 1, "cancel_second_scan": true }),
    );
    assert!(
        reply["error_after_cancel"]
            .as_str()
            .is_some_and(|error| error.contains("cancel")),
        "{reply}"
    );
}

/// REQ: PORT:ilium-client/src/cost_tracker/preparation.rs::oversized_price_capacity_refuses_before_copying_and_keeps_old_overlay
#[test]
fn port_cost_tracker_preparation_oversized_price_table_copies_nothing() {
    let reply = scenario(
        "oversized-price-capacity-refuses-before-copying-and-keeps-old-overlay",
        json!({ "model_name_bytes": "oversized", "capacity": "exceeded" }),
    );
    assert_eq!(reply["price_entries_copied"], json!(0), "{reply}");
}
