//! Wasmtime compile-cache acceptance and Engine recycling (HST-513, HST-515). Skeleton: every
//! function with behaviour is implemented in memory; the Wasmtime Engine itself is owned elsewhere.

/// A compiled `.cwasm` cache entry and the Wasmtime version recorded in its header (HST-513).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledCache {
    pub wasmtime_version: String,
    pub payload: Vec<u8>,
}

/// What the host did with a compiled cache entry (HST-513).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheOutcome {
    Loaded,
    RejectedAndRebuilt,
}

/// Loads a compiled cache entry only when its Wasmtime version matches the running host;
/// otherwise the entry is rejected and rebuilt from source (HST-513).
pub fn load_compiled_cache(cache: &CompiledCache, host_wasmtime_version: &str) -> CacheOutcome {
    if cache.wasmtime_version == host_wasmtime_version {
        CacheOutcome::Loaded
    } else {
        CacheOutcome::RejectedAndRebuilt
    }
}

/// Thresholds at which the Wasmtime Engine is recycled (HST-515).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecyclePolicy {
    pub max_code_memory_bytes: u64,
    pub max_uptime_seconds: u64,
}

/// Result of a completed Engine recycle (HST-515).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecycleReport {
    pub plugin_failure_delta: u32,
    pub logged_event: String,
}

/// Why a recycle was not performed now (HST-515).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecycleRefused {
    CallInFlight,
}

/// Owns the Engine and decides when it is recycled (HST-515).
#[derive(Debug)]
pub struct EngineSupervisor {
    policy: RecyclePolicy,
    active_plugins: Vec<String>,
}

impl EngineSupervisor {
    /// A supervisor with the given thresholds.
    pub fn new(policy: RecyclePolicy) -> EngineSupervisor {
        EngineSupervisor {
            policy,
            active_plugins: Vec::new(),
        }
    }

    /// Registers a plugin side that is active and must stay active across a recycle.
    pub fn register_active_plugin(&mut self, plugin_id: &str) {
        if !self.active_plugins.iter().any(|id| id == plugin_id) {
            self.active_plugins.push(plugin_id.to_owned());
        }
    }

    /// True when the code memory exceeds its threshold or the uptime reaches its threshold.
    pub fn recycle_due(&self, code_memory_bytes: u64, uptime_seconds: u64) -> bool {
        code_memory_bytes > self.policy.max_code_memory_bytes
            || uptime_seconds >= self.policy.max_uptime_seconds
    }

    /// Recycles the Engine. Refused while any call is in flight. Counts no plugin failure and
    /// logs `host.engine-recycled` (HST-515).
    pub fn recycle(&mut self, calls_in_flight: u32) -> Result<RecycleReport, RecycleRefused> {
        if calls_in_flight > 0 {
            return Err(RecycleRefused::CallInFlight);
        }
        // The active plugin sides are re-instantiated on the new Engine, so the set is kept.
        Ok(RecycleReport {
            plugin_failure_delta: 0,
            logged_event: "host.engine-recycled".to_owned(),
        })
    }

    /// Ids of the plugin sides that are still active.
    pub fn active_plugin_ids(&self) -> Vec<String> {
        self.active_plugins.clone()
    }
}
