//! The Wasmtime embedding: engine configuration, compilation and the compiled-code cache
//! (reference-host-runtime.md HST-050..088). No engine type is public in the skeleton.
//!
//! Skeleton: every function with behaviour is `unimplemented!` until the implementation phase.
//! The host crate does not yet depend on `wasmtime`; `RuntimeSettings` stands in for the
//! `wasmtime::Config` that `runtime_config()` builds (reference-host-runtime.md HST-057).

use std::path::{Path, PathBuf};

use ilium_plugin_api::manifest::Diagnostic;
use ilium_plugin_api::ProcessKind;

use crate::calls::ResolvedLimits;
use crate::error::HostError;
use crate::linker::World;
use crate::telemetry::LogLine;

/// The pinned Wasmtime version (reference-host-runtime.md HST-050).
pub const WASMTIME_VERSION: &str = "49.0.2";

/// Engine settings (reference-host-runtime.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineConfig {
    /// Epoch tick length in milliseconds, the deadline granularity (reference-host-runtime.md).
    pub epoch_tick_ms: u64,
    /// Directory of the compiled-code cache, if any (reference-host-runtime.md).
    pub cache_dir: Option<PathBuf>,
    /// The process that embeds the engine (reference-host-architecture.md).
    pub process: ProcessKind,
}

/// Which instance allocator the engine uses (reference-host-runtime.md HST-066).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllocatorKind {
    /// Wasmtime's on-demand allocator.
    OnDemand,
    /// Wasmtime's pooling allocator.
    Pooling,
}

/// The pinned engine settings behind `runtime_config()` (reference-host-runtime.md HST-050..066).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSettings {
    /// `WASMTIME_VERSION`.
    pub wasmtime_version: &'static str,
    /// Component model on (HST-053).
    pub component_model: bool,
    /// Async component model off (HST-054).
    pub component_model_async: bool,
    /// Concurrency support off (HST-056).
    pub concurrency_support: bool,
    /// Epoch interruption on (HST-060).
    pub epoch_interruption: bool,
    /// Fuel off (HST-061).
    pub consume_fuel: bool,
    /// Wasm stack size in bytes (HST-062).
    pub max_wasm_stack: usize,
    /// Async stack size in bytes (HST-062).
    pub async_stack_size: usize,
    /// Signals-based traps (HST-064).
    pub signals_based_traps: bool,
    /// The chosen allocator (HST-066).
    pub allocator: AllocatorKind,
}

/// The one engine configuration every host process shares (reference-host-runtime.md HST-057).
pub fn runtime_settings() -> RuntimeSettings {
    unimplemented!("host::engine::runtime_settings")
}

/// The `Config` function the host, the helper and the precompiler link (reference-host-runtime.md
/// HST-057, HST-151); the same value as `runtime_settings()` until `wasmtime` is a dependency.
pub fn runtime_config() -> RuntimeSettings {
    unimplemented!("host::engine::runtime_config")
}

/// Picks the allocator from measured `(p99 instantiate µs, rss MiB)` pairs (reference-host-runtime.md HST-066).
pub fn select_allocator(on_demand: (f64, f64), pooling: (f64, f64), added_rss_mib: f64) -> AllocatorKind {
    unimplemented!("host::engine::select_allocator")
}

/// Fingerprint of the engine configuration (reference-host-runtime.md HST-057).
pub fn config_fingerprint() -> String {
    unimplemented!("host::engine::config_fingerprint")
}

/// The compiled-code cache directory under `base`: `<version>-<fingerprint>` (HST-070).
pub fn compile_cache_dir(base: &Path) -> PathBuf {
    unimplemented!("host::engine::compile_cache_dir")
}

/// Removes cache directories of other versions or fingerprints; returns what it removed (HST-071).
pub fn prune_stale_cache_dirs(base: &Path) -> std::io::Result<Vec<PathBuf>> {
    unimplemented!("host::engine::prune_stale_cache_dirs")
}

/// Whether the monotonic clock jumped more than the tolerated amount (reference-host-runtime.md HST-085).
pub fn clock_jump_detected(previous_ms: u64, now_ms: u64) -> bool {
    unimplemented!("host::engine::clock_jump_detected")
}

/// What a component is validated against (reference-host-runtime.md HST-055, HST-080..083).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationContext {
    /// The world the component must export, if known.
    pub world: Option<World>,
    /// The host's API version.
    pub api_version: (u16, u16, u16),
    /// Grants the manifest requests.
    pub requested: Vec<String>,
    /// Text the world name must contain, if any.
    pub expected_world_text: Option<String>,
}

impl ValidationContext {
    /// A context for a world at an API version.
    pub fn new(world: World, api_version: (u16, u16, u16)) -> ValidationContext {
        ValidationContext { world: Some(world), api_version, requested: Vec::new(), expected_world_text: None }
    }

    /// A context without a world, at an API version.
    pub fn bare(api_version: (u16, u16, u16)) -> ValidationContext {
        ValidationContext { world: None, api_version, requested: Vec::new(), expected_world_text: None }
    }

    /// With these requested grants.
    pub fn requesting(mut self, grants: &[&str]) -> ValidationContext {
        self.requested = grants.iter().map(|grant| (*grant).to_owned()).collect();
        self
    }

    /// Expecting the world name to contain this text.
    pub fn expecting_world_text(mut self, text: &str) -> ValidationContext {
        self.expected_world_text = Some(text.to_owned());
        self
    }
}

/// Validates component bytes before compilation (reference-host-runtime.md HST-055, HST-080..083).
pub fn validate_component(bytes: &[u8], context: &ValidationContext) -> Result<(), Diagnostic> {
    unimplemented!("host::engine::validate_component")
}

/// Per-store settings derived from the resolved limits (reference-host-runtime.md HST-067).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreSettings {
    /// Linear memory cap in bytes.
    pub memory_bytes: u64,
    /// Table element cap.
    pub table_elements: u64,
    /// Whether a failed grow traps instead of returning -1.
    pub trap_on_grow_failure: bool,
    /// Instance cap.
    pub instances: u32,
    /// Table cap.
    pub tables: u32,
    /// Memory cap.
    pub memories: u32,
}

/// The store settings for resolved limits (reference-host-runtime.md HST-067).
pub fn store_settings(limits: &ResolvedLimits) -> StoreSettings {
    unimplemented!("host::engine::store_settings")
}

/// One precompiled core-plugin artifact shipped in the binary (reference-host-runtime.md HST-057).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedArtifact {
    /// The artifact name.
    pub name: String,
    /// The config fingerprint it was compiled with.
    pub fingerprint: String,
    /// The component bytes to compile when the `.cwasm` is stale.
    pub wasm: Vec<u8>,
}

/// The embedded artifacts of this build (reference-host-runtime.md HST-057).
pub fn embedded_artifacts() -> Vec<EmbeddedArtifact> {
    unimplemented!("host::engine::embedded_artifacts")
}

/// How an embedded artifact was loaded (reference-host-runtime.md HST-058).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadPath {
    /// The `.cwasm` matched and was deserialized.
    Deserialized,
    /// The `.cwasm` was stale; the component was compiled.
    Compiled,
}

/// Counters of the engine (reference-host-runtime.md HST-058, HST-072).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EngineStats {
    /// `deserialize` calls.
    pub deserialize_calls: u64,
    /// `Component::new` calls.
    pub component_new_calls: u64,
    /// Instantiations.
    pub instantiations: u64,
}

/// Key of a compiled component in the cache: hash of bytes, engine version and settings.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey(pub String);

/// A compiled component, ready to instantiate (reference-host-runtime.md).
#[derive(Debug, Clone)]
pub struct CompiledComponent {
    /// Length of the source component in bytes.
    pub source_len: usize,
}

/// The shared engine (reference-host-runtime.md).
#[derive(Debug)]
pub struct PluginEngine {
    config: EngineConfig,
}

impl PluginEngine {
    /// Builds the engine with epoch interruption and fuel off (reference-host-runtime.md).
    pub fn new(config: EngineConfig) -> Result<PluginEngine, HostError> {
        unimplemented!("host::engine::PluginEngine::new")
    }

    /// The configuration this engine was built with.
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Compiles component bytes (reference-host-runtime.md).
    pub fn compile(&self, bytes: &[u8]) -> Result<CompiledComponent, HostError> {
        unimplemented!("host::engine::PluginEngine::compile")
    }

    /// Looks a precompiled component up in the cache (reference-host-runtime.md).
    pub fn deserialize_cached(&self, key: &CacheKey) -> Result<Option<CompiledComponent>, HostError> {
        unimplemented!("host::engine::PluginEngine::deserialize_cached")
    }

    /// Loads an embedded artifact, compiling when its `.cwasm` is stale (reference-host-runtime.md HST-058).
    pub fn load_embedded(&self, artifact: &EmbeddedArtifact) -> Result<LoadPath, HostError> {
        unimplemented!("host::engine::PluginEngine::load_embedded")
    }

    /// The engine counters (reference-host-runtime.md HST-058, HST-072).
    pub fn stats(&self) -> EngineStats {
        unimplemented!("host::engine::PluginEngine::stats")
    }

    /// The log lines the engine wrote, oldest first (reference-host-runtime.md HST-072).
    pub fn log_lines(&self) -> Vec<LogLine> {
        unimplemented!("host::engine::PluginEngine::log_lines")
    }

    /// The current epoch (reference-host-runtime.md HST-060).
    pub fn current_epoch(&self) -> u64 {
        unimplemented!("host::engine::PluginEngine::current_epoch")
    }

    /// Epoch ticks a deadline of `deadline_ms` spans, rounded up plus one (reference-host-runtime.md HST-060).
    pub fn deadline_ticks(&self, deadline_ms: u64) -> u64 {
        unimplemented!("host::engine::PluginEngine::deadline_ticks")
    }

    /// Whether fuel metering is on; always false (reference-host-runtime.md HST-061).
    pub fn fuel_enabled(&self) -> bool {
        unimplemented!("host::engine::PluginEngine::fuel_enabled")
    }
}

/// The compile-cache key of a component: its SHA-256 plus the engine identity (wasmtime version,
/// CPU features and config), so a changed engine never serves a stale artifact
/// (reference-install-and-update.md PKG-268).
pub fn compile_cache_key(component_sha256: &str, engine_id: &str) -> String {
    unimplemented!("host::engine::compile_cache_key")
}
