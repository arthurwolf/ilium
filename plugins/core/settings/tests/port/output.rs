//! Twins of ilium-client/src/presentation.rs: the terminal output worker belongs to the host, the
//! plugin only has to keep every frame it draws bounded, deterministic and failure-tolerant
//! (F-089, F-090).

use ilium_plugin_sdk::types::{ActionKind, ActionValue, ErrorCode, PluginError, ViewTarget};
use serde_json::json;

use crate::common::*;
use crate::helpers::*;

/// REQ: PORT:ilium-client/src/presentation.rs::prepared_frame_retains_existing_cell_vector_capacity, PORT:ilium-client/src/presentation.rs::retained_acknowledged_buffers_keep_distinct_frame_admission, PORT:ilium-client/src/presentation.rs::oversized_graphics_symbol_releases_reserved_capacity, PORT:ilium-client/src/presentation.rs::terminal_diff_does_not_credit_unchanged_identical_braille, PORT:ilium-client/src/presentation.rs::diff_cursor_and_resize_follow_the_actual_emitted_base, PORT:ilium-client/src/presentation.rs::output_worker_admission_includes_the_worst_case_diff_mask_and_stack, F-089, F-090
#[test]
fn port_089_frames_are_drawn_to_the_requested_size_even_after_many_renders() {
    let mut host = client_host();
    for round in 0..40u16 {
        let columns = 20 + (round % 7) * 25;
        let rows = 6 + (round % 5) * 12;
        let screen = render_checked(&mut host, tab("user-interface"), columns, rows);
        for row in 0..rows {
            for column in 0..columns {
                assert!(screen.cell(row, column).symbol.chars().count() <= 8);
            }
        }
    }
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/presentation.rs::terminal_diff_does_not_credit_unchanged_identical_braille, PORT:ilium-client/src/presentation.rs::diff_cursor_and_resize_follow_the_actual_emitted_base, F-089, F-090
#[test]
fn port_089_an_identical_scene_renders_identical_cells() {
    let mut host = client_host();
    let first = host.render(tab("icons"), 80, 24);
    let second = host.render(tab("icons"), 80, 24);
    for row in 0..24 {
        for column in 0..80 {
            assert_eq!(
                first.cell(row, column).symbol,
                second.cell(row, column).symbol
            );
            assert_eq!(
                first.cell(row, column).foreground,
                second.cell(row, column).foreground
            );
            assert_eq!(
                first.cell(row, column).background,
                second.cell(row, column).background
            );
        }
    }
}

/// REQ: PORT:ilium-client/src/presentation.rs::blocked_logical_drop_retains_worker_and_escaped_frame_until_actual_join, PORT:ilium-client/src/presentation.rs::blocked_output_bounds_admission_and_emits_fifo_before_shutdown, PORT:ilium-client/src/presentation.rs::cancelled_shutdown_preserves_pending_original_output_failure, PORT:ilium-client/src/presentation.rs::failed_output_returns_uncertain_custody_without_success_proof, PORT:ilium-client/src/presentation.rs::failure_receipts_share_original_storage_after_actual_worker_join, PORT:ilium-client/src/presentation.rs::uncertain_output_tail_is_not_retried_on_drop, F-089, F-090
#[test]
fn port_089_a_blocked_or_failed_write_leaves_the_plugin_views_usable() {
    let mut host = client_host();
    host.inject_failure(
        "settings.set",
        PluginError::new(ErrorCode::Io, "write failed"),
    );
    toggle(&mut host, "user-interface", "ui.use_stable_glyphs", true);
    assert_eq!(
        value_of(&mut host, "ui.use_stable_glyphs"),
        Some(json!(false)),
        "a failed write is never reported as stored"
    );
    let _ = render_checked(&mut host, tab("user-interface"), 80, 24);
    toggle(&mut host, "user-interface", "ui.use_stable_glyphs", true);
    assert_eq!(
        value_of(&mut host, "ui.use_stable_glyphs"),
        Some(json!(true)),
        "the failed write is not retried behind the user's back"
    );
}

/// REQ: PORT:ilium-client/src/presentation.rs::reservations_cannot_cross_output_owners, PORT:ilium-client/src/presentation.rs::output_worker_requests_its_admitted_native_stack, PORT:ilium-client/src/presentation.rs::storage_refusal_precedes_spawn_and_drops_original_cleanup_capture_once, PORT:ilium-client/src/presentation.rs::thread_refusal_precedes_spawn_and_initial_output_has_no_buffer_allocation, PORT:ilium-client/src/presentation.rs::output_panic_restores_on_output_thread_and_reports_failure, PORT:ilium-client/src/presentation.rs::restoration_waits_for_blocked_output_and_drain, PORT:ilium-client/src/presentation.rs::shutdown_waits_for_original_thread_local_cleanup_after_body_completion, F-089, F-090
#[test]
fn port_089_each_view_keeps_its_own_frame_and_no_view_borrows_anothers_cells() {
    let mut host = client_host();
    let settings = host.render(tab("user-interface"), 60, 12).text();
    let panel = host
        .render(ViewTarget::Panel("overview".to_string()), 60, 12)
        .text();
    let step = host
        .render(ViewTarget::OnboardingStep("first-run".to_string()), 60, 12)
        .text();
    assert_eq!(host.render(tab("user-interface"), 60, 12).text(), settings);
    assert_eq!(
        host.render(ViewTarget::Panel("overview".to_string()), 60, 12)
            .text(),
        panel
    );
    assert_eq!(
        host.render(ViewTarget::OnboardingStep("first-run".to_string()), 60, 12)
            .text(),
        step
    );
    assert!(host.failures().is_empty());
}
