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
pub fn select_allocator(
    _on_demand: (f64, f64),
    _pooling: (f64, f64),
    _added_rss_mib: f64,
) -> AllocatorKind {
    unimplemented!("host::engine::select_allocator")
}

/// Fingerprint of the engine configuration (reference-host-runtime.md HST-057).
pub fn config_fingerprint() -> String {
    unimplemented!("host::engine::config_fingerprint")
}

/// The compiled-code cache directory under `base`: `<version>-<fingerprint>` (HST-070).
pub fn compile_cache_dir(_base: &Path) -> PathBuf {
    unimplemented!("host::engine::compile_cache_dir")
}

/// Removes cache directories of other versions or fingerprints; returns what it removed (HST-071).
pub fn prune_stale_cache_dirs(_base: &Path) -> std::io::Result<Vec<PathBuf>> {
    unimplemented!("host::engine::prune_stale_cache_dirs")
}

/// Whether the monotonic clock jumped more than the tolerated amount (reference-host-runtime.md HST-085).
pub fn clock_jump_detected(_previous_ms: u64, _now_ms: u64) -> bool {
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
        ValidationContext {
            world: Some(world),
            api_version,
            requested: Vec::new(),
            expected_world_text: None,
        }
    }

    /// A context without a world, at an API version.
    pub fn bare(api_version: (u16, u16, u16)) -> ValidationContext {
        ValidationContext {
            world: None,
            api_version,
            requested: Vec::new(),
            expected_world_text: None,
        }
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
pub fn validate_component(_bytes: &[u8], _context: &ValidationContext) -> Result<(), Diagnostic> {
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
pub fn store_settings(_limits: &ResolvedLimits) -> StoreSettings {
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
    pub fn new(_config: EngineConfig) -> Result<PluginEngine, HostError> {
        unimplemented!("host::engine::PluginEngine::new")
    }

    /// The configuration this engine was built with.
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Compiles component bytes (reference-host-runtime.md).
    pub fn compile(&self, _bytes: &[u8]) -> Result<CompiledComponent, HostError> {
        unimplemented!("host::engine::PluginEngine::compile")
    }

    /// Looks a precompiled component up in the cache (reference-host-runtime.md).
    pub fn deserialize_cached(
        &self,
        _key: &CacheKey,
    ) -> Result<Option<CompiledComponent>, HostError> {
        unimplemented!("host::engine::PluginEngine::deserialize_cached")
    }

    /// Loads an embedded artifact, compiling when its `.cwasm` is stale (reference-host-runtime.md HST-058).
    pub fn load_embedded(&self, _artifact: &EmbeddedArtifact) -> Result<LoadPath, HostError> {
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
    pub fn deadline_ticks(&self, _deadline_ms: u64) -> u64 {
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
pub fn compile_cache_key(_component_sha256: &str, _engine_id: &str) -> String {
    unimplemented!("host::engine::compile_cache_key")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ilium_plugin_api::limits::LimitKey;
    use ilium_plugin_api::ProcessKind;

    use super::{
        clock_jump_detected, compile_cache_dir, compile_cache_key, config_fingerprint,
        embedded_artifacts, prune_stale_cache_dirs, runtime_config, runtime_settings,
        select_allocator, store_settings, validate_component, AllocatorKind, CacheKey,
        EmbeddedArtifact, EngineConfig, EngineStats, LoadPath, PluginEngine, ValidationContext,
        WASMTIME_VERSION,
    };
    use crate::calls::ResolvedLimits;
    use crate::linker::World;

    const MIB: usize = 1024 * 1024;
    const WASM_COMPONENT_HEADER: [u8; 8] = [0x00, 0x61, 0x73, 0x6d, 0x0d, 0x00, 0x01, 0x00];
    const API_VERSION_PAYLOAD: [u8; 6] = [0x00, 0x00, 0x00, 0x01, 0x00, 0x00];

    fn leb128_u32(mut value: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        loop {
            let low_seven_bits = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                bytes.push(low_seven_bits);
                return bytes;
            }
            bytes.push(low_seven_bits | 0x80);
        }
    }

    /// One component-model custom section: id 0, size, name, payload.
    fn custom_section(name: &str, payload: &[u8]) -> Vec<u8> {
        let mut content = leb128_u32(name.len() as u32);
        content.extend_from_slice(name.as_bytes());
        content.extend_from_slice(payload);
        let mut section = vec![0x00];
        section.extend(leb128_u32(content.len() as u32));
        section.extend(content);
        section
    }

    fn component_with_sections(sections: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = WASM_COMPONENT_HEADER.to_vec();
        for section in sections {
            bytes.extend_from_slice(section);
        }
        bytes
    }

    fn api_version_section(payload: &[u8]) -> Vec<u8> {
        custom_section("ilium:api-version", payload)
    }

    /// A component with no world, no imports and a correct `ilium:api-version` section.
    fn valid_minimal_component() -> Vec<u8> {
        component_with_sections(&[api_version_section(&API_VERSION_PAYLOAD)])
    }

    fn bare_context() -> ValidationContext {
        ValidationContext::bare((0, 1, 0))
    }

    fn refusal_code(bytes: &[u8], context: &ValidationContext) -> String {
        validate_component(bytes, context)
            .expect_err("the component must be refused")
            .code
            .as_str()
            .to_owned()
    }

    fn engine_config() -> EngineConfig {
        EngineConfig {
            epoch_tick_ms: 10,
            cache_dir: None,
            process: ProcessKind::Server,
        }
    }

    /// REQ: HST-050
    #[test]
    fn runtime_settings_pins_the_wasmtime_version() {
        let settings = runtime_settings();
        assert_eq!(settings.wasmtime_version, "49.0.2");
        assert_eq!(WASMTIME_VERSION, "49.0.2");
    }

    /// REQ: HST-054
    #[test]
    fn runtime_settings_keeps_component_model_async_and_concurrency_off() {
        let settings = runtime_settings();
        assert!(settings.component_model);
        assert!(!settings.component_model_async);
        assert!(!settings.concurrency_support);
    }

    /// REQ: HST-058
    #[test]
    fn runtime_settings_enables_epoch_interruption_and_disables_fuel() {
        let settings = runtime_settings();
        assert!(settings.epoch_interruption);
        assert!(!settings.consume_fuel);
    }

    /// REQ: HST-059
    #[test]
    fn runtime_settings_sets_the_documented_stack_sizes() {
        let settings = runtime_settings();
        assert_eq!(settings.max_wasm_stack, 512 * 1024);
        assert_eq!(settings.async_stack_size, MIB);
    }

    /// REQ: HST-060
    #[test]
    fn runtime_settings_enables_signals_based_traps() {
        assert!(runtime_settings().signals_based_traps);
    }

    /// REQ: HST-063
    #[test]
    fn runtime_settings_default_to_the_on_demand_allocator() {
        assert_eq!(runtime_settings().allocator, AllocatorKind::OnDemand);
    }

    /// REQ: HST-087
    #[test]
    fn runtime_config_is_the_same_value_as_runtime_settings() {
        assert_eq!(runtime_config(), runtime_settings());
    }

    /// REQ: HST-064
    #[test]
    fn select_allocator_picks_pooling_when_on_demand_misses_the_budget_and_pooling_fits() {
        let selected = select_allocator((6000.0, 100.0), (1000.0, 120.0), 10.0);
        assert_eq!(selected, AllocatorKind::Pooling);
    }

    /// REQ: HST-064
    #[test]
    fn select_allocator_keeps_on_demand_when_on_demand_meets_the_budget() {
        let selected = select_allocator((4000.0, 100.0), (1000.0, 120.0), 10.0);
        assert_eq!(selected, AllocatorKind::OnDemand);
    }

    /// REQ: HST-064
    #[test]
    fn select_allocator_keeps_on_demand_when_pooling_misses_the_instantiate_budget() {
        let selected = select_allocator((6000.0, 100.0), (6000.0, 120.0), 10.0);
        assert_eq!(selected, AllocatorKind::OnDemand);
    }

    /// REQ: HST-064
    #[test]
    fn select_allocator_accepts_exactly_16_mib_added_rss_and_refuses_more() {
        assert_eq!(
            select_allocator((6000.0, 100.0), (1000.0, 120.0), 16.0),
            AllocatorKind::Pooling
        );
        assert_eq!(
            select_allocator((6000.0, 100.0), (1000.0, 120.0), 16.5),
            AllocatorKind::OnDemand
        );
    }

    /// REQ: HST-057
    #[test]
    fn config_fingerprint_is_non_empty_and_stable() {
        let first = config_fingerprint();
        assert!(!first.is_empty());
        assert_eq!(first, config_fingerprint());
    }

    /// REQ: HST-061
    #[test]
    fn compile_cache_dir_is_named_by_wasmtime_version_and_fingerprint() {
        let base = std::path::Path::new("/cache/base");
        let directory = compile_cache_dir(base);
        assert_eq!(directory.parent(), Some(base));
        let expected_name = format!("{WASMTIME_VERSION}-{}", config_fingerprint());
        assert_eq!(
            directory.file_name().and_then(|name| name.to_str()),
            Some(expected_name.as_str())
        );
    }

    /// REQ: HST-061
    #[test]
    fn prune_stale_cache_dirs_removes_other_keys_and_keeps_the_current_one() {
        let base = tempfile::tempdir().expect("temporary base directory");
        let current = compile_cache_dir(base.path());
        let other_version = base.path().join(format!("48.0.0-{}", config_fingerprint()));
        let other_fingerprint = base.path().join(format!("{WASMTIME_VERSION}-stale"));
        for directory in [&current, &other_version, &other_fingerprint] {
            std::fs::create_dir_all(directory).expect("create cache directory");
        }

        let mut removed = prune_stale_cache_dirs(base.path()).expect("prune succeeds");
        removed.sort();
        let mut expected = vec![other_version.clone(), other_fingerprint.clone()];
        expected.sort();

        assert_eq!(removed, expected);
        assert!(current.exists());
        assert!(!other_version.exists());
        assert!(!other_fingerprint.exists());
    }

    /// REQ: HST-061
    #[test]
    fn prune_stale_cache_dirs_on_an_empty_base_removes_nothing() {
        let base = tempfile::tempdir().expect("temporary base directory");
        let removed = prune_stale_cache_dirs(base.path()).expect("prune succeeds");
        assert!(removed.is_empty());
    }

    /// REQ: PKG-268
    #[test]
    fn compile_cache_key_is_deterministic_for_the_same_inputs() {
        let first = compile_cache_key("abc123", "49.0.2-fingerprint");
        assert_eq!(first, compile_cache_key("abc123", "49.0.2-fingerprint"));
    }

    /// REQ: PKG-268
    #[test]
    fn compile_cache_key_changes_with_the_component_hash_or_the_engine_id() {
        let first = compile_cache_key("abc123", "49.0.2-fingerprint");
        assert_ne!(first, compile_cache_key("def456", "49.0.2-fingerprint"));
        assert_ne!(first, compile_cache_key("abc123", "48.0.0-fingerprint"));
    }

    /// REQ: HST-082
    #[test]
    fn clock_jump_is_not_detected_for_exactly_one_second_or_less() {
        assert!(!clock_jump_detected(1000, 2000));
        assert!(!clock_jump_detected(0, 999));
    }

    /// REQ: HST-082
    #[test]
    fn clock_jump_is_detected_beyond_one_second() {
        assert!(clock_jump_detected(0, 1001));
        assert!(clock_jump_detected(5000, 60_000));
    }

    /// REQ: HST-082
    #[test]
    fn clock_jump_is_not_detected_when_no_time_has_passed() {
        assert!(!clock_jump_detected(42, 42));
    }

    /// REQ: HST-072
    #[test]
    fn validation_context_builders_record_world_grants_and_world_text() {
        let context = ValidationContext::new(World::Server, (0, 1, 0))
            .requesting(&["storage"])
            .expecting_world_text("server");
        assert_eq!(context.world, Some(World::Server));
        assert_eq!(context.api_version, (0, 1, 0));
        assert_eq!(context.requested, vec!["storage".to_owned()]);
        assert_eq!(context.expected_world_text.as_deref(), Some("server"));
    }

    /// REQ: HST-084
    #[test]
    fn bare_validation_context_has_no_world_and_no_grants() {
        let context = ValidationContext::bare((0, 1, 0));
        assert_eq!(context.world, None);
        assert!(context.requested.is_empty());
        assert_eq!(context.expected_world_text, None);
    }

    /// REQ: HST-085
    #[test]
    fn validate_component_accepts_a_minimal_component_with_a_matching_api_section() {
        validate_component(&valid_minimal_component(), &bare_context())
            .expect("a valid component is accepted");
    }

    /// REQ: HST-084
    #[test]
    fn validate_component_refuses_empty_bytes_as_invalid() {
        assert_eq!(
            refusal_code(&[], &bare_context()),
            "package.component-invalid"
        );
    }

    /// REQ: HST-084
    #[test]
    fn validate_component_refuses_non_wasm_bytes_as_invalid() {
        assert_eq!(
            refusal_code(b"not a component", &bare_context()),
            "package.component-invalid"
        );
    }

    /// REQ: HST-084
    #[test]
    fn validate_component_refuses_a_missing_api_section() {
        let bytes = component_with_sections(&[custom_section("other", &[1, 2, 3])]);
        assert_eq!(
            refusal_code(&bytes, &bare_context()),
            "package.api-section-missing"
        );
    }

    /// REQ: HST-085
    #[test]
    fn validate_component_refuses_an_api_section_with_another_version() {
        let bytes = component_with_sections(&[api_version_section(&[0, 0, 0, 2, 0, 0])]);
        assert_eq!(refusal_code(&bytes, &bare_context()), "manifest.api-version");
    }

    /// REQ: HST-085
    #[test]
    fn validate_component_refuses_an_api_section_that_is_not_six_bytes() {
        let bytes = component_with_sections(&[api_version_section(&[0, 0, 0, 1, 0, 0, 0])]);
        assert!(validate_component(&bytes, &bare_context()).is_err());
    }

    /// REQ: HST-086
    #[test]
    fn validate_component_refuses_a_component_over_32_mib() {
        let payload = vec![0u8; 32 * MIB];
        let bytes = component_with_sections(&[custom_section("padding", &payload)]);
        assert!(bytes.len() > 32 * MIB);
        assert_eq!(refusal_code(&bytes, &bare_context()), "component.too-large");
    }

    /// REQ: HST-086
    #[test]
    fn validate_component_refuses_a_custom_section_over_4_mib() {
        let oversized = vec![0u8; 4 * MIB + 1000];
        let bytes = component_with_sections(&[
            api_version_section(&API_VERSION_PAYLOAD),
            custom_section("padding", &oversized),
        ]);
        assert_eq!(
            refusal_code(&bytes, &bare_context()),
            "component.custom-section-too-large"
        );
    }

    /// REQ: HST-086
    #[test]
    fn validate_component_refuses_custom_sections_together_over_8_mib() {
        let each = vec![0u8; 3 * MIB];
        let bytes = component_with_sections(&[
            api_version_section(&API_VERSION_PAYLOAD),
            custom_section("first", &each),
            custom_section("second", &each),
            custom_section("third", &each),
        ]);
        assert_eq!(
            refusal_code(&bytes, &bare_context()),
            "component.custom-section-too-large"
        );
    }

    /// REQ: HST-086
    #[test]
    fn validate_component_accepts_custom_sections_within_both_limits() {
        let each = vec![0u8; 3 * MIB];
        let bytes = component_with_sections(&[
            api_version_section(&API_VERSION_PAYLOAD),
            custom_section("first", &each),
            custom_section("second", &each),
        ]);
        validate_component(&bytes, &bare_context()).expect("6 MiB of custom sections is accepted");
    }

    /// REQ: HST-084
    #[test]
    fn validate_component_order_puts_the_size_check_before_the_api_section_check() {
        let payload = vec![0u8; 32 * MIB];
        let bytes = component_with_sections(&[custom_section("padding", &payload)]);
        assert_eq!(refusal_code(&bytes, &bare_context()), "component.too-large");
    }

    /// REQ: HST-067
    #[test]
    fn store_settings_convert_memory_and_table_limits_and_trap_on_grow_failure() {
        let limits = ResolvedLimits(BTreeMap::from([
            (LimitKey::MemoryMib, 256),
            (LimitKey::TableElements, 50_000),
        ]));
        let settings = store_settings(&limits);
        assert_eq!(settings.memory_bytes, 256 * MIB as u64);
        assert_eq!(settings.table_elements, 50_000);
        assert!(settings.trap_on_grow_failure);
    }

    /// REQ: HST-067
    #[test]
    fn store_settings_cap_instances_tables_and_memories_at_engine_limits() {
        let limits = ResolvedLimits(BTreeMap::from([(LimitKey::MemoryMib, 256)]));
        let settings = store_settings(&limits);
        assert_eq!(settings.instances, 16);
        assert_eq!(settings.tables, 8);
        assert_eq!(settings.memories, 4);
    }

    /// REQ: HST-067
    #[test]
    fn store_settings_for_zero_memory_allow_no_bytes() {
        let limits = ResolvedLimits(BTreeMap::from([(LimitKey::MemoryMib, 0)]));
        assert_eq!(store_settings(&limits).memory_bytes, 0);
    }

    /// REQ: HST-057
    #[test]
    fn embedded_artifacts_carry_the_current_config_fingerprint_and_bytes() {
        let fingerprint = config_fingerprint();
        for artifact in embedded_artifacts() {
            assert_eq!(artifact.fingerprint, fingerprint, "artifact {}", artifact.name);
            assert!(!artifact.wasm.is_empty(), "artifact {}", artifact.name);
        }
    }

    /// REQ: HST-079
    #[test]
    fn engine_keeps_the_config_it_was_built_with_and_uses_a_10_ms_tick() {
        let config = engine_config();
        let engine = PluginEngine::new(config.clone()).expect("engine builds");
        assert_eq!(engine.config(), &config);
        assert_eq!(engine.config().epoch_tick_ms, 10);
    }

    /// REQ: HST-058
    #[test]
    fn engine_reports_fuel_disabled() {
        let engine = PluginEngine::new(engine_config()).expect("engine builds");
        assert!(!engine.fuel_enabled());
    }

    /// REQ: HST-081
    #[test]
    fn deadline_ticks_round_up_and_add_one() {
        let engine = PluginEngine::new(engine_config()).expect("engine builds");
        assert_eq!(engine.deadline_ticks(0), 1);
        assert_eq!(engine.deadline_ticks(1), 2);
        assert_eq!(engine.deadline_ticks(10), 2);
        assert_eq!(engine.deadline_ticks(11), 3);
        assert_eq!(engine.deadline_ticks(20), 3);
    }

    /// REQ: HST-067
    #[test]
    fn compile_accepts_a_valid_component_and_records_its_length() {
        let engine = PluginEngine::new(engine_config()).expect("engine builds");
        let bytes = valid_minimal_component();
        let compiled = engine.compile(&bytes).expect("valid component compiles");
        assert_eq!(compiled.source_len, bytes.len());
    }

    /// REQ: HST-067
    #[test]
    fn compile_refuses_empty_bytes() {
        let engine = PluginEngine::new(engine_config()).expect("engine builds");
        assert!(engine.compile(&[]).is_err());
    }

    /// REQ: HST-065
    #[test]
    fn deserialize_cached_returns_none_for_an_unknown_key() {
        let engine = PluginEngine::new(engine_config()).expect("engine builds");
        let found = engine
            .deserialize_cached(&CacheKey("absent-key".to_owned()))
            .expect("lookup succeeds");
        assert!(found.is_none());
    }

    /// REQ: HST-066
    #[test]
    fn load_embedded_deserializes_when_the_fingerprint_matches() {
        let engine = PluginEngine::new(engine_config()).expect("engine builds");
        let artifact = EmbeddedArtifact {
            name: "bundled".to_owned(),
            fingerprint: config_fingerprint(),
            wasm: valid_minimal_component(),
        };
        let path = engine.load_embedded(&artifact).expect("load succeeds");
        assert_eq!(path, LoadPath::Deserialized);
        assert_eq!(engine.stats().deserialize_calls, 1);
    }

    /// REQ: HST-088
    #[test]
    fn load_embedded_compiles_and_logs_when_the_fingerprint_differs() {
        let engine = PluginEngine::new(engine_config()).expect("engine builds");
        let artifact = EmbeddedArtifact {
            name: "bundled".to_owned(),
            fingerprint: "stale-fingerprint".to_owned(),
            wasm: valid_minimal_component(),
        };
        let path = engine.load_embedded(&artifact).expect("load succeeds");
        assert_eq!(path, LoadPath::Compiled);

        let stats = engine.stats();
        assert_eq!(stats.deserialize_calls, 0);
        assert_eq!(stats.component_new_calls, 1);
        assert!(engine
            .log_lines()
            .iter()
            .any(|line| line.message.contains("host.cwasm-incompatible")));
    }

    /// REQ: HST-058
    #[test]
    fn a_new_engine_starts_with_zeroed_counters() {
        let engine = PluginEngine::new(engine_config()).expect("engine builds");
        assert_eq!(engine.stats(), EngineStats::default());
    }
}
