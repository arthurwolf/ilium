//! Runtime numbers of the call boundary: ABI-011..ABI-013, ABI-017, ABI-026, ABI-027, ABI-042,
//! ABI-044, ABI-045, ABI-048. Constant checks are pure data; limit and rule checks call stubs.

use ilium_plugin_api::abi::{
    batch_sizes, deadline_ticks, effectful_import_error, is_pure_export, is_sdk_correlation,
    restore_bytes_accepted, EFFECTFUL_IMPORT_MESSAGE, EPOCH_TICK_MS, MAX_EVENTS_PER_BATCH,
    PURE_EXPORTS, RESTORE_BYTES_MAX, RESULT_HOLD_MS, SDK_CORRELATION_FLOOR,
    SESSION_PENDING_REQUESTS,
};
use ilium_plugin_api::codes;
use ilium_plugin_api::limits::{ceiling, default_of, LimitKey};
use ilium_plugin_api::{ErrorCode, Tier};
use proptest::prelude::*;

/// REQ: ABI-011
#[test]
fn abi_011_sdk_floor_is_2_pow_63() {
    assert_eq!(SDK_CORRELATION_FLOOR, 9_223_372_036_854_775_808);
}

/// REQ: ABI-011
#[test]
fn abi_011_number_below_2_pow_63_belongs_to_the_plugin() {
    assert!(!is_sdk_correlation((1 << 63) - 1));
    assert!(!is_sdk_correlation(0));
}

/// REQ: ABI-011
#[test]
fn abi_011_number_from_2_pow_63_belongs_to_the_sdk() {
    assert!(is_sdk_correlation(1 << 63));
    assert!(is_sdk_correlation(u64::MAX));
}

/// REQ: ABI-012
#[test]
fn abi_012_result_hold_is_30_seconds() {
    assert_eq!(RESULT_HOLD_MS, 30_000);
}

/// REQ: ABI-013
#[test]
fn abi_013_pending_requests_default_is_64() {
    assert_eq!(default_of(LimitKey::PendingRequests), 64);
}

/// REQ: ABI-013
#[test]
fn abi_013_pending_requests_ceilings() {
    assert_eq!(ceiling(LimitKey::PendingRequests, Tier::T1), 256);
    assert_eq!(ceiling(LimitKey::PendingRequests, Tier::T2), 64);
    assert_eq!(ceiling(LimitKey::PendingRequests, Tier::T3), 64);
}

/// REQ: ABI-013
#[test]
fn abi_013_session_pending_requests_is_4096() {
    assert_eq!(SESSION_PENDING_REQUESTS, 4_096);
}

/// REQ: ABI-017
#[test]
fn abi_017_invalid_utf8_traps_with_the_abi_trap() {
    assert_eq!(codes::trap::ABI, "trap.abi");
    assert!(ilium_plugin_api::error::registry()
        .iter()
        .any(|info| info.code == codes::trap::ABI));
}

/// REQ: ABI-026
#[test]
fn abi_026_epoch_tick_is_10_ms() {
    assert_eq!(EPOCH_TICK_MS, 10);
}

/// REQ: ABI-027
#[test]
fn abi_027_memory_limit_default_and_ceilings() {
    assert_eq!(default_of(LimitKey::MemoryMib), 32);
    assert_eq!(ceiling(LimitKey::MemoryMib, Tier::T1), 256);
    assert_eq!(ceiling(LimitKey::MemoryMib, Tier::T2), 128);
}

/// REQ: ABI-027
#[test]
fn abi_027_memory_growth_trap_code_exists() {
    assert_eq!(codes::trap::MEMORY, "trap.memory");
}

/// REQ: ABI-042
#[test]
fn abi_042_deadline_20_ms_is_3_ticks() {
    assert_eq!(deadline_ticks(20), 3);
}

/// REQ: ABI-042
#[test]
fn abi_042_deadline_21_ms_is_4_ticks() {
    assert_eq!(deadline_ticks(21), 4);
}

/// REQ: ABI-042
#[test]
fn abi_042_deadline_1_ms_is_2_ticks() {
    assert_eq!(deadline_ticks(1), 2);
}

/// REQ: ABI-044
#[test]
fn abi_044_restore_bytes_up_to_1_mib_are_kept() {
    assert_eq!(RESTORE_BYTES_MAX, 1_048_576);
    assert!(restore_bytes_accepted(0));
    assert!(restore_bytes_accepted(1_048_576));
}

/// REQ: ABI-044
#[test]
fn abi_044_restore_bytes_over_1_mib_are_dropped() {
    assert!(!restore_bytes_accepted(1_048_577));
}

/// REQ: ABI-045
#[test]
fn abi_045_batch_holds_at_most_256_events() {
    assert_eq!(MAX_EVENTS_PER_BATCH, 256);
}

/// REQ: ABI-045
#[test]
fn abi_045_600_queued_events_are_three_batches() {
    assert_eq!(batch_sizes(600), vec![256, 256, 88]);
}

/// REQ: ABI-045
#[test]
fn abi_045_empty_queue_is_no_batch() {
    assert_eq!(batch_sizes(0), Vec::<usize>::new());
}

/// REQ: ABI-045
#[test]
fn abi_045_event_queue_default_and_ceilings() {
    assert_eq!(default_of(LimitKey::EventQueue), 1_024);
    assert_eq!(ceiling(LimitKey::EventQueue, Tier::T1), 4_096);
    assert_eq!(ceiling(LimitKey::EventQueue, Tier::T2), 1_024);
    assert_eq!(ceiling(LimitKey::EventQueue, Tier::T3), 1_024);
}

/// REQ: ABI-048
#[test]
fn abi_048_pure_exports_are_render_annotate_and_settings_validate() {
    assert_eq!(
        PURE_EXPORTS,
        ["views.render", "annotate", "settings-validate"]
    );
    for export in PURE_EXPORTS {
        assert!(is_pure_export(export), "{export}");
    }
    assert!(!is_pure_export("on-events"));
}

/// REQ: ABI-048
#[test]
fn abi_048_effectful_import_returns_not_permitted_with_the_fixed_message() {
    let error = effectful_import_error();
    assert_eq!(error.code, ErrorCode::NotPermitted);
    assert_eq!(error.message, EFFECTFUL_IMPORT_MESSAGE);
    assert_eq!(EFFECTFUL_IMPORT_MESSAGE, "effectful import in a pure call");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    // REQ: ABI-011
    #[test]
    fn abi_011_split_is_at_2_pow_63(correlation in any::<u64>()) {
        prop_assert_eq!(is_sdk_correlation(correlation), correlation >= SDK_CORRELATION_FLOOR);
    }

    // REQ: ABI-042
    #[test]
    fn abi_042_ticks_are_ceil_d_over_10_plus_1(deadline in 0u64..1_000_000) {
        prop_assert_eq!(deadline_ticks(deadline), deadline.div_ceil(10) + 1);
    }

    // REQ: ABI-026, ABI-042
    #[test]
    fn abi_042_spinning_guest_traps_inside_d_to_d_plus_20(deadline in 1u64..100_000) {
        // The first tick lands up to one period after the call starts, so the trap lands
        // between (ticks - 1) and ticks periods after the start.
        let ticks = deadline_ticks(deadline);
        let earliest = (ticks - 1) * EPOCH_TICK_MS;
        let latest = ticks * EPOCH_TICK_MS;
        prop_assert!(earliest >= deadline, "earliest {} < d {}", earliest, deadline);
        prop_assert!(latest <= deadline + 20, "latest {} > d + 20", latest);
    }

    // REQ: ABI-045
    #[test]
    fn abi_045_batches_cover_the_queue_in_order(queued in 0usize..10_000) {
        let sizes = batch_sizes(queued);
        prop_assert_eq!(sizes.iter().sum::<usize>(), queued);
        prop_assert!(sizes.iter().all(|size| (1..=256).contains(size)));
        prop_assert!(sizes.iter().rev().skip(1).all(|size| *size == 256));
    }

    // REQ: ABI-044
    #[test]
    fn abi_044_restore_rule_is_1_mib(length in 0usize..4_000_000) {
        prop_assert_eq!(restore_bytes_accepted(length), length <= 1_048_576);
    }
}

/// REQ: ABI-017
#[test]
fn abi_017_invalid_utf8_trap_is_not_retryable_and_is_documented_in_errors_and_limits() {
    let info = ilium_plugin_api::error::registry()
        .iter()
        .find(|info| info.code == codes::trap::ABI)
        .expect("trap.abi is in the error registry");
    assert!(!info.retryable, "a trap on invalid UTF-8 repeats on retry");
    assert_eq!(info.page, "reference-errors-and-limits.md");
}
