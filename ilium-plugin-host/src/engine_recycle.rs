//! Wasmtime compile-cache acceptance and Engine recycling (HST-513, HST-515). Skeleton: every
//! function with behaviour is `unimplemented!` until the implementation phase.

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
pub fn load_compiled_cache(_cache: &CompiledCache, _host_wasmtime_version: &str) -> CacheOutcome {
    unimplemented!("host::engine_recycle::load_compiled_cache")
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
pub struct EngineSupervisor;

impl EngineSupervisor {
    /// A supervisor with the given thresholds.
    pub fn new(_policy: RecyclePolicy) -> EngineSupervisor {
        unimplemented!("host::engine_recycle::EngineSupervisor::new")
    }

    /// Registers a plugin side that is active and must stay active across a recycle.
    pub fn register_active_plugin(&mut self, _plugin_id: &str) {
        unimplemented!("host::engine_recycle::EngineSupervisor::register_active_plugin")
    }

    /// True when the code memory exceeds its threshold or the uptime reaches its threshold.
    pub fn recycle_due(&self, _code_memory_bytes: u64, _uptime_seconds: u64) -> bool {
        unimplemented!("host::engine_recycle::EngineSupervisor::recycle_due")
    }

    /// Recycles the Engine. Refused while any call is in flight. Counts no plugin failure and
    /// logs `host.engine-recycled` (HST-515).
    pub fn recycle(&mut self, _calls_in_flight: u32) -> Result<RecycleReport, RecycleRefused> {
        unimplemented!("host::engine_recycle::EngineSupervisor::recycle")
    }

    /// Ids of the plugin sides that are still active.
    pub fn active_plugin_ids(&self) -> Vec<String> {
        unimplemented!("host::engine_recycle::EngineSupervisor::active_plugin_ids")
    }
}
