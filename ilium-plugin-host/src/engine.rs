//! The Wasmtime embedding: engine configuration, compilation and the compiled-code cache
//! (reference-host-runtime.md). No engine type is public in the skeleton.

use std::path::PathBuf;

use ilium_plugin_api::ProcessKind;

use crate::error::HostError;

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
}
