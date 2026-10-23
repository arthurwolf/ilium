//! Manual page `reference-host-aggregate-limits`: the machine-wide disk ceiling (HST-126, HST-127),
//! the machine-wide CPU bucket (HST-128, HST-953) and the rule that the ceilings never lower the
//! low-disk reserve (HST-954).

use super::common::{assert_page_states, manual_page};

const PAGE: &str = "reference-host-aggregate-limits.md";

/// REQ: HST-126
#[test]
fn hst_126_the_limits_table_sets_the_machine_disk_ceiling_at_8192_mib() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "| Aggregate disk ceiling (`data` plus `cache`, all plugins) | 8,192 MiB (proposed, owner to confirm) | HST-126 |",
    );
}

/// REQ: HST-126
#[test]
fn hst_126_a_write_within_the_plugin_quota_but_past_the_machine_ceiling_is_refused() {
    unimplemented!("ilium_plugin_host::host_capacity::machine_disk_ceiling")
}

/// REQ: HST-127
#[test]
fn hst_127_a_write_past_the_machine_ceiling_fails_with_quota_exceeded_and_host_disk_ceiling() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "fail with `quota-exceeded` and the message `host-disk-ceiling`",
    );
}

/// REQ: HST-127
#[test]
fn hst_127_the_refused_write_evicts_no_other_plugin_and_the_host_does_not_retry_it() {
    let page = manual_page(PAGE);
    assert_page_states(&page, "MUST NOT evict data of another plugin to make room");
    assert_page_states(&page, "The host MUST NOT retry the write itself.");
}

/// REQ: HST-128
#[test]
fn hst_128_the_cpu_bucket_holds_and_refills_4000_ms_per_second() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "holds 4,000 ms of thread CPU time and refills at 4,000 ms of thread CPU time per second (proposed, owner to confirm)",
    );
}

/// REQ: HST-128
#[test]
fn hst_128_the_machine_bucket_applies_in_addition_to_the_per_side_group() {
    unimplemented!("ilium_plugin_host::host_capacity::machine_cpu_bucket")
}

/// REQ: HST-953
#[test]
fn hst_953_an_empty_cpu_bucket_returns_quota_exceeded_with_retry_after_ms() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "When the CPU bucket of HST-128 is empty, a new submission and every call that returns a `request-id` MUST return `quota-exceeded` with a `retry-after-ms`",
    );
}

/// REQ: HST-953
#[test]
fn hst_953_the_cpu_refusal_does_not_trap_the_plugin_or_count_as_a_failure() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "The refusal MUST NOT trap the plugin, MUST NOT count as a failure, and MUST NOT charge CPU time to any side beyond the cost of the call itself",
    );
}

/// REQ: HST-954
#[test]
fn hst_954_the_machine_ceilings_do_not_lower_the_free_space_reserve() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "The disk ceiling and the CPU ceiling MUST NOT lower the free-space reserve of [EXT-064]",
    );
}

/// REQ: HST-954
#[test]
fn hst_954_the_machine_ceilings_never_skip_the_low_disk_steps() {
    unimplemented!("ilium_plugin_host::host_capacity::machine_ceilings_with_low_disk_steps")
}
