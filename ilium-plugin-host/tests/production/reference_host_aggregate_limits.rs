//! Manual page `reference-host-aggregate-limits`: the machine-wide disk ceiling (HST-126, HST-127),
//! the machine-wide CPU bucket (HST-128, HST-953) and the rule that the ceilings never lower the
//! low-disk reserve (HST-954).

use ilium_plugin_host::host_capacity::{
    MachineCeilings, MachineWriteVerdict, FREE_SPACE_RESERVE_BYTES, MACHINE_DISK_CEILING_BYTES,
};

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
    let mib = 1024 * 1024;
    let mut machine = MachineCeilings::new(MACHINE_DISK_CEILING_BYTES - 10 * mib, u64::MAX / 2);
    assert_eq!(machine.write(10 * mib), MachineWriteVerdict::Accepted);
    assert_eq!(machine.write(1), MachineWriteVerdict::HostDiskCeiling);
    assert_eq!(machine.used_bytes(), MACHINE_DISK_CEILING_BYTES);
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
    let mut machine = MachineCeilings::new(0, u64::MAX / 2);
    assert_eq!(machine.charge_cpu(4_000, 0), Ok(()));
    let retry_after_ms = machine.charge_cpu(1_000, 0).expect_err("an empty bucket refuses");
    assert_eq!(retry_after_ms, 250, "1000 ms at 4000 ms per second");
    assert_eq!(machine.charge_cpu(1_000, 250), Ok(()), "refilled after the wait");
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
    let mib = 1024 * 1024;
    let mut machine = MachineCeilings::new(0, FREE_SPACE_RESERVE_BYTES + mib);
    assert_eq!(machine.write(2 * mib), MachineWriteVerdict::StorageFull);
    assert_eq!(machine.low_disk_steps_run(), 1, "the low-disk step ran");
    let mut at_ceiling = MachineCeilings::new(MACHINE_DISK_CEILING_BYTES, FREE_SPACE_RESERVE_BYTES - 1);
    assert_eq!(at_ceiling.write(1), MachineWriteVerdict::StorageFull);
    assert_eq!(at_ceiling.low_disk_steps_run(), 1, "the ceiling must not skip the step");
}
