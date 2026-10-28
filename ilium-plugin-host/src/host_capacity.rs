//! Aggregate ceilings of the session server: installed plugins and active plugin sides
//! (reference-host-aggregate-limits.md, HST-510, HST-511). Skeleton: every function with
//! behaviour is plain in-memory bookkeeping.

/// A refused install or activation. `reason` is the stable word, `host-capacity` when the
/// active ceiling is reached (HST-511).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapacityRefusal {
    pub reason: String,
}

/// The session server's plugin ledger with its aggregate ceilings (HST-510, HST-511).
#[derive(Debug, Default)]
pub struct CapacityHost {
    installed_ceiling: usize,
    active_ceiling: usize,
    installed: Vec<String>,
    active: Vec<String>,
}

/// Hard ceiling of installed plugins (HST-510).
const MAX_INSTALLED: usize = 256;

/// Hard ceiling of active plugin sides (HST-510).
const MAX_ACTIVE: usize = 128;

fn refusal(reason: &str) -> CapacityRefusal {
    CapacityRefusal {
        reason: reason.to_string(),
    }
}

impl CapacityHost {
    /// A ledger with the installed and active ceilings the policy resolved to.
    pub fn new(installed_ceiling: usize, active_ceiling: usize) -> CapacityHost {
        CapacityHost {
            installed_ceiling,
            active_ceiling,
            installed: Vec::new(),
            active: Vec::new(),
        }
    }

    /// Records an installed plugin; refused at the installed ceiling (HST-510).
    pub fn install(&mut self, plugin_id: &str) -> Result<(), CapacityRefusal> {
        if self.installed.iter().any(|known| known == plugin_id) {
            return Ok(());
        }
        if self.installed.len() >= self.installed_ceiling {
            return Err(refusal("host-capacity-installed"));
        }
        self.installed.push(plugin_id.to_string());
        Ok(())
    }

    /// Activates an installed plugin side; refused with `host-capacity` at the active ceiling
    /// and never evicts an active plugin to make room (HST-511).
    pub fn activate(&mut self, plugin_id: &str) -> Result<(), CapacityRefusal> {
        if !self.installed.iter().any(|known| known == plugin_id) {
            return Err(refusal("not-installed"));
        }
        if self.active.iter().any(|known| known == plugin_id) {
            return Ok(());
        }
        if self.active.len() >= self.active_ceiling {
            return Err(refusal("host-capacity"));
        }
        self.active.push(plugin_id.to_string());
        Ok(())
    }

    /// Number of installed plugins.
    pub fn installed_count(&self) -> usize {
        self.installed.len()
    }

    /// Ids of the currently active plugins, in activation order.
    pub fn active_plugin_ids(&self) -> Vec<String> {
        self.active.clone()
    }

    /// The text `ilium plugin status` prints, including plugins left inactive (HST-511).
    pub fn status_listing(&self) -> String {
        let mut listing = String::new();
        for plugin_id in &self.installed {
            let state = if self.active.contains(plugin_id) {
                "active"
            } else {
                "inactive (host-capacity)"
            };
            listing.push_str(&format!("{plugin_id}  {state}\n"));
        }
        listing
    }
}

/// The ceilings a policy resolves to: an administrator may lower each value but never raise it
/// above 256 installed plugins and 128 active plugin sides (HST-510).
pub fn effective_ceilings(requested_installed: usize, requested_active: usize) -> (usize, usize) {
    (
        requested_installed.min(MAX_INSTALLED),
        requested_active.min(MAX_ACTIVE),
    )
}

/// Machine-wide disk ceiling for `data` plus `cache` of all plugins, in bytes (HST-126).
pub const MACHINE_DISK_CEILING_BYTES: u64 = 8_192 * 1024 * 1024;

/// Free-space reserve the host keeps for its own journal, audit log and snapshots (EXT-064).
pub const FREE_SPACE_RESERVE_BYTES: u64 = 16 * 1024 * 1024;

/// Machine-wide CPU bucket capacity and refill per second, in milliseconds (HST-128).
pub const MACHINE_CPU_BUCKET_MS: u64 = 4_000;

/// What the machine-wide ceilings decided about one plugin write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineWriteVerdict {
    Accepted,
    /// `quota-exceeded` with the message `host-disk-ceiling` (HST-127).
    HostDiskCeiling,
    /// Free space is under the reserve: `storage.full`, and the low-disk steps still run (HST-954).
    StorageFull,
}

/// Machine-wide disk and CPU ceilings, applied in addition to per-plugin quotas.
#[derive(Debug)]
pub struct MachineCeilings {
    used_bytes: u64,
    free_bytes: u64,
    cpu_tokens_ms: u64,
    cpu_last_ms: u64,
    low_disk_steps_run: u32,
}

impl MachineCeilings {
    pub fn new(used_bytes: u64, free_bytes: u64) -> MachineCeilings {
        MachineCeilings {
            used_bytes,
            free_bytes,
            cpu_tokens_ms: MACHINE_CPU_BUCKET_MS,
            cpu_last_ms: 0,
            low_disk_steps_run: 0,
        }
    }

    /// Decides one write of `bytes` that already fits the plugin's own quota. The low-disk steps
    /// run whenever free space is under the reserve, whatever the ceiling says (HST-954).
    pub fn write(&mut self, bytes: u64) -> MachineWriteVerdict {
        if self.free_bytes < FREE_SPACE_RESERVE_BYTES.saturating_add(bytes) {
            self.low_disk_steps_run += 1;
            return MachineWriteVerdict::StorageFull;
        }
        if self.used_bytes.saturating_add(bytes) > MACHINE_DISK_CEILING_BYTES {
            return MachineWriteVerdict::HostDiskCeiling;
        }
        self.used_bytes += bytes;
        self.free_bytes -= bytes;
        MachineWriteVerdict::Accepted
    }

    pub fn used_bytes(&self) -> u64 {
        self.used_bytes
    }

    pub fn low_disk_steps_run(&self) -> u32 {
        self.low_disk_steps_run
    }

    /// Charges `cost_ms` of thread CPU time at `now_ms`. Returns `Err(retry_after_ms)` when the
    /// bucket cannot cover it (`quota-exceeded` with `retry-after-ms`, HST-953).
    pub fn charge_cpu(&mut self, cost_ms: u64, now_ms: u64) -> Result<(), u64> {
        let elapsed = now_ms.saturating_sub(self.cpu_last_ms);
        self.cpu_last_ms = now_ms;
        let refill = elapsed.saturating_mul(MACHINE_CPU_BUCKET_MS) / 1_000;
        self.cpu_tokens_ms = self.cpu_tokens_ms.saturating_add(refill).min(MACHINE_CPU_BUCKET_MS);
        if self.cpu_tokens_ms >= cost_ms {
            self.cpu_tokens_ms -= cost_ms;
            return Ok(());
        }
        let missing = cost_ms - self.cpu_tokens_ms;
        Err((missing * 1_000).div_ceil(MACHINE_CPU_BUCKET_MS).max(1))
    }
}

/// Extra free space `ilium plugin backup` keeps on top of the archive size (HST-132).
pub const BACKUP_RESERVE_BYTES: u64 = 512 * 1024 * 1024;

/// The `backup.no-space` precheck: refuse before any write when free space is under the backed-up
/// bytes plus 512 MiB (HST-132).
pub fn backup_space_precheck(backed_up_bytes: u64, free_bytes: u64) -> Result<(), &'static str> {
    if free_bytes < backed_up_bytes.saturating_add(BACKUP_RESERVE_BYTES) {
        Err("backup.no-space")
    } else {
        Ok(())
    }
}
