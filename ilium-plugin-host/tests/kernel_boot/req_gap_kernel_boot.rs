//! Gap-review requirements of the kernel-boot lane (host crate part): install free-space checks
//! (HST-000), helper SIGKILL classification (HST-046) and disk accounting of plugin data (HST-047).
//!
//! These tests target behaviour that is not implemented yet. Each one calls the closest public
//! entry point that exists today and asserts the required result, so it fails at runtime.

use ilium_plugin_api::ids::PluginId;
use ilium_plugin_api::{ErrorCode, Tier};
use ilium_plugin_host::helper_forensics::{record_helper_death, HelperDeath, HelperExit};
use ilium_plugin_host::host_capacity::{MachineCeilings, MachineWriteVerdict};
use ilium_plugin_host::storage::{PluginStorage, StorageContext};

const MIB: u64 = 1024 * 1024;
const PACKAGE_BYTES: u64 = 8 * MIB;
const MEMORY_LIMIT_BYTES: u64 = 256 * MIB;

fn sigkill_death(peak_memory_bytes: u64) -> HelperDeath {
    HelperDeath {
        plugin_id: "acme.notes".to_string(),
        exit: HelperExit::Signal(9),
        call_name: Some("on-save".to_string()),
        call_elapsed_ms: Some(40),
        helper_uptime_seconds: 12,
        peak_memory_bytes,
        log_lines: vec!["allocating buffer".to_string()],
    }
}

/// REQ: HST-000
#[test]
fn req_hst_000_install_space_on_the_cache_volume_is_accepted_when_free_a() {
    // Package size times 3 plus 64 MiB is the data-volume need; the cache volume needs the package
    // size plus 64 MiB. Ample free space on both volumes must let the install write proceed.
    let mut ceilings = MachineCeilings::new(0, 1024 * 1024 * MIB);
    let verdict = ceilings.write(PACKAGE_BYTES + 64 * MIB);
    assert_eq!(verdict, MachineWriteVerdict::Accepted);
}

/// REQ: HST-000
#[test]
fn req_hst_000_install_space_short_on_the_cache_volume_is_refused_b() {
    // A cache volume with no free space must refuse the write with the no-space verdict, so the
    // install stops before writing anything on either volume.
    let mut ceilings = MachineCeilings::new(0, 0);
    let verdict = ceilings.write(PACKAGE_BYTES + 64 * MIB);
    assert_eq!(
        verdict,
        MachineWriteVerdict::StorageFull,
        "install must refuse with no-space, naming the short volume"
    );
}

/// REQ: HST-046
#[test]
fn req_hst_046_sigkill_after_oom_counter_rise_is_trap_memory_a() {
    // The helper hit its memory limit and the cgroup `oom_kill` counter rose, so the death is
    // reported as `trap.memory`. The closest existing input is the peak memory at the limit.
    let record = record_helper_death(&sigkill_death(MEMORY_LIMIT_BYTES));
    assert_eq!(record.condition_id, "trap.memory");
    assert_eq!(record.terminating_signal, Some(9));
}

/// REQ: HST-046
#[test]
fn req_hst_046_sigkill_without_oom_record_is_helper_killed_b() {
    // A SIGKILL with no out-of-memory record is `helper.killed`, names signal 9 and is not a
    // memory trap.
    let record = record_helper_death(&sigkill_death(16 * MIB));
    assert_eq!(record.condition_id, "helper.killed");
    assert_eq!(record.terminating_signal, Some(9));
    assert_ne!(record.condition_id, "trap.memory");
}

/// REQ: HST-047
#[test]
fn req_hst_047_small_files_stop_at_the_plugin_disk_limit_a() {
    // Many one-byte values must hit the plugin's own disk limit with a quota refusal, so the
    // allocated size cannot pass `disk_mib` through small files.
    let root = tempfile::tempdir().expect("create a temporary storage root");
    let plugin = PluginId("acme.notes".to_string());
    let mut storage = PluginStorage::open_with_context(
        root.path(),
        plugin,
        StorageContext::default(),
        Tier::T3,
        4096,
        4096,
    )
    .expect("open plugin storage with a small disk limit");
    let refusal = (0..50_000u32).find_map(|index| {
        storage
            .put("data", &format!("k{index}"), vec![7u8])
            .err()
    });
    assert!(
        refusal
            .as_ref()
            .is_some_and(|error| error.code == ErrorCode::QuotaExceeded),
        "one-byte writes must end in quota-exceeded before the limit is passed"
    );
}

/// REQ: HST-047
#[test]
fn req_hst_047_one_byte_write_at_full_machine_usage_is_refused_b() {
    // When the machine-wide disk usage is already at its ceiling, even a one-byte write must be
    // refused with the host disk ceiling verdict instead of being accepted.
    let mut ceilings = MachineCeilings::new(u64::MAX - 1, 1024 * 1024 * MIB);
    let verdict = ceilings.write(1);
    assert_eq!(verdict, MachineWriteVerdict::HostDiskCeiling);
}
