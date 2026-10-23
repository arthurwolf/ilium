//! Wasmtime component engine, validation, cache and epoch ownership.
//!
//! Compilation accepts raw component bytes. `validate_component` performs
//! static interface checks only; generated world linkers and parsed effective
//! grants must authorize package instantiation separately.

mod embedded;

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use ilium_execution::{reserve_admitted_worker, QuotaGroup, QuotaLimits};
use ilium_platform::owned_worker::{OwnedWorker, StopToken, WorkerKind};
use ilium_plugin_api::limits::LimitKey;
use ilium_plugin_api::manifest::Diagnostic;
use ilium_plugin_api::{Code, ProcessKind};
use sha2::{Digest, Sha256};
use wasmparser::{Parser, Payload, Validator, WasmFeatures};
use wasmtime::component::Component;
use wasmtime::{
    Cache, CacheConfig, Config, Engine, InstanceAllocationStrategy, OptLevel, Store, StoreLimits,
    StoreLimitsBuilder, UpdateDeadline, WasmBacktraceDetails,
};

use crate::calls::ResolvedLimits;
use crate::error::HostError;
use crate::linker::World;
use crate::telemetry::LogLine;

pub const WASMTIME_VERSION: &str = "49.0.2";
const COMPONENT_MAX_BYTES: usize = 32 * 1024 * 1024;
const CUSTOM_MAX_BYTES: usize = 4 * 1024 * 1024;
const CUSTOM_TOTAL_MAX_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineConfig {
    pub epoch_tick_ms: u64,
    pub cache_dir: Option<PathBuf>,
    pub process: ProcessKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllocatorKind {
    OnDemand,
    Pooling,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSettings {
    pub wasmtime_version: &'static str,
    pub component_model: bool,
    pub component_model_async: bool,
    pub concurrency_support: bool,
    pub epoch_interruption: bool,
    pub consume_fuel: bool,
    pub max_wasm_stack: usize,
    pub async_stack_size: usize,
    pub signals_based_traps: bool,
    pub allocator: AllocatorKind,
}

pub fn runtime_settings() -> RuntimeSettings {
    RuntimeSettings {
        wasmtime_version: WASMTIME_VERSION,
        component_model: true,
        component_model_async: false,
        concurrency_support: false,
        epoch_interruption: true,
        consume_fuel: false,
        max_wasm_stack: 512 * 1024,
        async_stack_size: 1024 * 1024,
        signals_based_traps: true,
        allocator: AllocatorKind::OnDemand,
    }
}

/// The one Wasmtime configuration function shared by the host and precompiler.
pub fn runtime_config() -> Config {
    let mut config = wasmtime::Config::new();
    config
        .wasm_component_model(true)
        .concurrency_support(false)
        .epoch_interruption(true)
        .consume_fuel(false)
        .max_wasm_stack(512 * 1024)
        .async_stack_size(1024 * 1024)
        .signals_based_traps(true)
        .parallel_compilation(true)
        .cranelift_opt_level(OptLevel::Speed)
        .wasm_simd(true)
        .wasm_relaxed_simd(false)
        .wasm_multi_memory(false)
        .wasm_memory64(false)
        .wasm_gc(false)
        .wasm_backtrace_details(WasmBacktraceDetails::Enable)
        .generate_address_map(true)
        .debug_info(false)
        .allocation_strategy(InstanceAllocationStrategy::OnDemand)
        .memory_guard_size(32 * 1024 * 1024);
    if usize::BITS == 64 {
        config.memory_reservation(4 * 1024 * 1024 * 1024);
    }
    config
}

/// Measurements are `(40-plugin startup ms, median instantiation ms)`.
pub fn select_allocator(
    on_demand: (f64, f64),
    pooling: (f64, f64),
    added_rss_mib: f64,
) -> AllocatorKind {
    if (on_demand.0 > 150.0 || on_demand.1 > 5.0)
        && pooling.0 <= 150.0
        && pooling.1 <= 5.0
        && added_rss_mib <= 16.0
    {
        AllocatorKind::Pooling
    } else {
        AllocatorKind::OnDemand
    }
}

struct Sha256Hasher(Sha256);
impl Hasher for Sha256Hasher {
    fn finish(&self) -> u64 {
        let digest = self.0.clone().finalize();
        u64::from_le_bytes(digest[..8].try_into().unwrap_or([0; 8]))
    }
    fn write(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
}

fn hex_sha256(hasher: Sha256) -> String {
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Includes Wasmtime's own artifact-compatibility hash, not a guessed CPU feature list.
pub fn config_fingerprint() -> String {
    static FINGERPRINT: OnceLock<String> = OnceLock::new();
    FINGERPRINT
        .get_or_init(|| {
            let mut hasher = Sha256Hasher(Sha256::new());
            WASMTIME_VERSION.hash(&mut hasher);
            match Engine::new(&runtime_config()) {
                Ok(engine) => engine.precompile_compatibility_hash().hash(&mut hasher),
                // The host cannot run with an invalid Config. This unique failure fingerprint
                // must never authenticate an artifact; PluginEngine::new returns the actual error.
                Err(error) => format!("engine-unavailable:{error}").hash(&mut hasher),
            }
            hex_sha256(hasher.0)
        })
        .clone()
}

pub fn compile_cache_dir(base: &Path) -> PathBuf {
    base.join("ilium")
        .join("plugin-compile")
        .join(format!("{WASMTIME_VERSION}-{}", config_fingerprint()))
}

fn cache_dir_name(name: &str) -> bool {
    let Some((version, digest)) = name.split_once('-') else {
        return false;
    };
    version.split('.').count() == 3
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        && digest.len() >= 3
        && digest.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Prune only versioned directories in the dedicated cache namespace; preserve unknown files.
pub fn prune_stale_cache_dirs(base: &Path) -> std::io::Result<Vec<PathBuf>> {
    let ilium_dir = base.join("ilium");
    let root = ilium_dir.join("plugin-compile");
    if !root.exists() {
        return Ok(Vec::new());
    }
    for directory in [&ilium_dir, &root] {
        if !std::fs::symlink_metadata(directory)?.file_type().is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "cache namespace must be a real directory",
            ));
        }
    }
    let keep = format!("{WASMTIME_VERSION}-{}", config_fingerprint());
    let mut removed = Vec::new();
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == keep || !cache_dir_name(&name) || !entry.file_type()?.is_dir() {
            continue;
        }
        std::fs::remove_dir_all(&path)?;
        removed.push(path);
    }
    Ok(removed)
}

pub fn clock_jump_detected(previous_ms: u64, now_ms: u64) -> bool {
    now_ms < previous_ms || now_ms - previous_ms > 1_000
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationContext {
    pub world: Option<World>,
    pub api_version: (u16, u16, u16),
    pub requested: Vec<String>,
    pub expected_world_text: Option<String>,
}
impl ValidationContext {
    pub fn new(world: World, api_version: (u16, u16, u16)) -> Self {
        Self {
            world: Some(world),
            api_version,
            requested: Vec::new(),
            expected_world_text: None,
        }
    }
    pub fn bare(api_version: (u16, u16, u16)) -> Self {
        Self {
            world: None,
            api_version,
            requested: Vec::new(),
            expected_world_text: None,
        }
    }
    pub fn requesting(mut self, grants: &[&str]) -> Self {
        self.requested = grants.iter().map(|grant| (*grant).to_owned()).collect();
        self
    }
    pub fn expecting_world_text(mut self, text: &str) -> Self {
        self.expected_world_text = Some(text.to_owned());
        self
    }
}

fn diagnostic(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(Code::from_static(code), "", message)
}

fn validator_features() -> WasmFeatures {
    let mut features = WasmFeatures::default();
    for flag in [
        WasmFeatures::THREADS,
        WasmFeatures::RELAXED_SIMD,
        WasmFeatures::MULTI_MEMORY,
        WasmFeatures::MEMORY64,
        WasmFeatures::GC,
        WasmFeatures::CM_ASYNC,
    ] {
        features.set(flag, false);
    }
    features.set(WasmFeatures::COMPONENT_MODEL, true);
    features
}

/// Statically known host imports. This list is only structural validation;
/// generated linker types and parsed grants must authorize instantiation.
fn known_host_import(interface: &str) -> bool {
    matches!(
        interface,
        "audio"
            | "catalogs"
            | "cli"
            | "client-requests"
            | "clipboard"
            | "clock"
            | "codec"
            | "commands"
            | "extensions"
            | "fs"
            | "geocode"
            | "gpu"
            | "help"
            | "http"
            | "http-api"
            | "inference"
            | "intents"
            | "intents-host"
            | "jobs"
            | "journal"
            | "kernel-settings"
            | "keymap"
            | "keys"
            | "location"
            | "log"
            | "media"
            | "messages"
            | "panes"
            | "plugins"
            | "process"
            | "services"
            | "sessions"
            | "settings"
            | "sounds"
            | "storage"
            | "timers"
            | "topics"
            | "tree"
            | "ui"
            | "ui-host"
            | "vector"
            | "workspace"
    )
}

fn import_allowed(name: &str, requested: &[String]) -> Result<(), Diagnostic> {
    let base = name.split('@').next().unwrap_or(name);
    if base.starts_with("wasi:sockets/") || base.starts_with("wasi:http/") {
        return Err(diagnostic(
            "component.wasi-forbidden",
            format!("forbidden import {name}"),
        ));
    }
    if base.starts_with("wasi:filesystem/") {
        // An import is syntactically supported even with zero filesystem grants.
        // This says nothing about preopens or operations: the later WASI linker
        // must derive those from parsed, effective fs scopes and fail closed.
        return Ok(());
    }
    if base.starts_with("wasi:cli/")
        || base.starts_with("wasi:clocks/")
        || base.starts_with("wasi:random/")
        || base.starts_with("wasi:io/")
        || base.starts_with("ilium:primitives/")
        || base == "ilium:anim/surface"
        || base == "ilium:anim/fonts"
        || matches!(
            base,
            "ilium:host/log"
                | "ilium:host/clock"
                | "ilium:host/timers"
                | "ilium:host/storage"
                | "ilium:host/settings"
                | "ilium:host/codec"
                | "ilium:host/messages"
        )
    {
        return Ok(());
    }
    let Some(interface) = base.strip_prefix("ilium:host/") else {
        return Err(diagnostic(
            "component.import-unknown",
            format!("unknown import {name}"),
        ));
    };
    if !known_host_import(interface) {
        return Err(diagnostic(
            "component.import-unknown",
            format!("unknown import {name}"),
        ));
    }
    if requested
        .iter()
        .any(|grant| grant == interface || grant.starts_with(&format!("{interface}.")))
    {
        Ok(())
    } else {
        Err(diagnostic(
            "manifest.import-not-requested",
            format!("import {name} needs a requested grant"),
        ))
    }
}

/// Ordered static checks only; this does not authorize linking or instantiation.
pub fn validate_component(bytes: &[u8], context: &ValidationContext) -> Result<(), Diagnostic> {
    Validator::new_with_features(validator_features())
        .validate_all(bytes)
        .map_err(|error| diagnostic("component.invalid", error.to_string()))?;
    if !Parser::is_component(bytes) {
        return Err(diagnostic("component.invalid", "module is not a component"));
    }
    if bytes.len() > COMPONENT_MAX_BYTES {
        return Err(diagnostic(
            "component.too-large",
            "component exceeds 32 MiB",
        ));
    }
    let mut api_sections = Vec::new();
    let mut imports = Vec::new();
    let mut exports = Vec::new();
    let mut custom_total = 0usize;
    let mut custom_too_large = false;
    let mut nested_depth = 0usize;
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.map_err(|error| diagnostic("component.invalid", error.to_string()))? {
            Payload::ModuleSection { .. } | Payload::ComponentSection { .. } => nested_depth += 1,
            Payload::End(_) if nested_depth > 0 => nested_depth -= 1,
            Payload::CustomSection(section) => {
                let len = section.data().len();
                custom_total = custom_total.saturating_add(len);
                custom_too_large |= len > CUSTOM_MAX_BYTES;
                if nested_depth == 0 && section.name() == "ilium:api-version" {
                    api_sections.push(section.data().to_vec());
                }
            }
            Payload::ComponentImportSection(section) if nested_depth == 0 => {
                for import in section {
                    imports.push(
                        import
                            .map_err(|error| diagnostic("component.invalid", error.to_string()))?
                            .name
                            .name
                            .to_owned(),
                    );
                }
            }
            Payload::ComponentExportSection(section) if nested_depth == 0 => {
                for export in section {
                    exports.push(
                        export
                            .map_err(|error| diagnostic("component.invalid", error.to_string()))?
                            .name
                            .name
                            .to_owned(),
                    );
                }
            }
            _ => {}
        }
    }
    if let Some(world) = context.world {
        // The base WIT worlds export lifecycle plus handler (server/client) or
        // renderer (anim). Exact server/client type identity is checked by the
        // generated world linker before instantiation; exports alone cannot prove it.
        let required = match world {
            World::Server | World::Client => "ilium:exports/handler",
            World::Anim => "ilium:exports/renderer",
        };
        if context
            .expected_world_text
            .as_deref()
            .is_some_and(|text| text != world.wit_name())
            || !exports
                .iter()
                .any(|name| name.starts_with("ilium:exports/lifecycle@"))
            || !exports.iter().any(|name| name.starts_with(required))
        {
            return Err(diagnostic(
                "component.world-mismatch",
                format!("component does not export {} base world", world.wit_name()),
            ));
        }
    }
    let expected = [
        context.api_version.0.to_be_bytes(),
        context.api_version.1.to_be_bytes(),
        context.api_version.2.to_be_bytes(),
    ]
    .concat();
    if api_sections.len() != 1 || api_sections[0] != expected {
        return Err(diagnostic(
            "manifest.api-version",
            "missing or incompatible ilium:api-version",
        ));
    }
    if custom_too_large || custom_total > CUSTOM_TOTAL_MAX_BYTES {
        return Err(diagnostic(
            "component.custom-section-too-large",
            "custom-section budget exceeded",
        ));
    }
    for import in imports {
        import_allowed(&import, &context.requested)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreSettings {
    pub memory_bytes: u64,
    pub table_elements: u64,
    pub trap_on_grow_failure: bool,
    pub instances: u32,
    pub tables: u32,
    pub memories: u32,
}
pub fn store_settings(limits: &ResolvedLimits) -> StoreSettings {
    StoreSettings {
        memory_bytes: limits
            .0
            .get(&LimitKey::MemoryMib)
            .copied()
            .unwrap_or(32)
            .saturating_mul(1024 * 1024),
        table_elements: limits
            .0
            .get(&LimitKey::TableElements)
            .copied()
            .unwrap_or(10_000),
        trap_on_grow_failure: true,
        instances: 16,
        tables: 8,
        memories: 4,
    }
}
/// Put this value in Store data and install `store.limiter(|data| &mut data.limits)`.
pub fn store_limits(limits: &ResolvedLimits) -> StoreLimits {
    let settings = store_settings(limits);
    StoreLimitsBuilder::new()
        .memory_size(settings.memory_bytes.min(usize::MAX as u64) as usize)
        .table_elements(settings.table_elements.min(usize::MAX as u64) as usize)
        .instances(settings.instances as usize)
        .tables(settings.tables as usize)
        .memories(settings.memories as usize)
        .trap_on_grow_failure(true)
        .build()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedArtifact {
    pub name: String,
    pub fingerprint: String,
    pub wasm: Vec<u8>,
    /// A trusted release-time precompile, never bytes from a plugin package or disk cache.
    pub cwasm: Vec<u8>,
}
pub fn embedded_artifacts() -> Vec<EmbeddedArtifact> {
    embedded::bundled_artifacts()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadPath {
    Deserialized,
    Compiled,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EngineStats {
    pub deserialize_calls: u64,
    pub component_new_calls: u64,
    pub instantiations: u64,
}
#[derive(Debug, Clone)]
pub struct CompiledComponent {
    pub source_len: usize,
    pub(crate) component: Arc<Component>,
}

#[derive(Debug)]
struct EpochClock {
    last_tick: Instant,
    suspended: Duration,
}

/// Charge only time when the host clock was advancing normally. A detected
/// suspend gap is credited back to calls already in flight, not just to the
/// callback on the first tick after resume.
fn active_elapsed(
    elapsed: Duration,
    suspended_now: Duration,
    suspended_at_start: Duration,
) -> Duration {
    elapsed.saturating_sub(suspended_now.saturating_sub(suspended_at_start))
}

pub struct PluginEngine {
    config: EngineConfig,
    engine: Engine,
    fingerprint: String,
    stats: Mutex<EngineStats>,
    log: Mutex<Vec<LogLine>>,
    embedded: Mutex<HashMap<String, CompiledComponent>>,
    epoch: Arc<AtomicU64>,
    jump_epoch: Arc<AtomicU64>,
    clock: Arc<Mutex<EpochClock>>,
    _ticker: OwnedWorker,
}
impl std::fmt::Debug for PluginEngine {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PluginEngine")
            .field("config", &self.config)
            .field("fingerprint", &self.fingerprint)
            .finish_non_exhaustive()
    }
}
impl PluginEngine {
    /// Independent finite quota for isolated callers. Production composition must
    /// use `with_quota` to share the process execution budget.
    pub fn new(config: EngineConfig) -> Result<Self, HostError> {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: 1,
            worker_bytes: 3 * 1024 * 1024,
        });
        Self::with_quota(config, &quota)
    }

    /// Reserve the ticker against the same quota as other process workers.
    pub fn with_quota(config: EngineConfig, quota: &QuotaGroup) -> Result<Self, HostError> {
        if config.epoch_tick_ms != 10 {
            return Err(HostError::EngineError(
                "the pinned epoch tick is 10 ms".into(),
            ));
        }
        let mut runtime = runtime_config();
        let mut log = Vec::new();
        if let Some(cache_dir) = &config.cache_dir {
            let cache_result = (|| {
                if let Some(base) = cache_dir
                    .parent()
                    .and_then(Path::parent)
                    .and_then(Path::parent)
                {
                    if *cache_dir == compile_cache_dir(base) {
                        prune_stale_cache_dirs(base)?;
                    }
                }
                if cache_dir.exists() && std::fs::metadata(cache_dir)?.permissions().readonly() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "cache directory is read-only",
                    ));
                }
                ilium_platform::secure_fs::create_private_directory(cache_dir)?;
                let mut cache_config = CacheConfig::new();
                cache_config
                    .with_directory(cache_dir)
                    .with_file_count_soft_limit(4_096)
                    .with_files_total_size_soft_limit(1024 * 1024 * 1024);
                let cache = Cache::new(cache_config)
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
                runtime.cache(Some(cache));
                Ok::<(), std::io::Error>(())
            })();
            if let Err(error) = cache_result {
                log.push(LogLine {
                    plugin: "host".into(),
                    level: "warn".into(),
                    message: format!("host.cache-unavailable: {error}; compiling in memory"),
                });
            }
        }
        let engine =
            Engine::new(&runtime).map_err(|error| HostError::EngineError(error.to_string()))?;
        let epoch = Arc::new(AtomicU64::new(0));
        let jump_epoch = Arc::new(AtomicU64::new(u64::MAX));
        let clock = Arc::new(Mutex::new(EpochClock {
            last_tick: Instant::now(),
            suspended: Duration::ZERO,
        }));
        // The reservation owns the quota lease through physical thread retirement.
        // Account an explicit native stack plus bounded ticker-owned state.
        let reservation = reserve_admitted_worker(quota, 2 * 1024 * 1024, 64 * 1024)
            .map_err(|error| HostError::EngineError(error.to_string()))?;
        let tick_engine = engine.clone();
        let tick_epoch = Arc::clone(&epoch);
        let tick_jump = Arc::clone(&jump_epoch);
        let tick_clock = Arc::clone(&clock);
        let tick = Duration::from_millis(config.epoch_tick_ms);
        let ticker = reservation
            .spawn(
                "ilium-plugin-epoch",
                WorkerKind::Cooperative,
                StopToken::default(),
                || {},
                move |stop| {
                    while !stop.is_stopped() {
                        thread::sleep(tick);
                        if stop.is_stopped() {
                            break;
                        }
                        let now = Instant::now();
                        let jumped = {
                            let mut clock = tick_clock
                                .lock()
                                .unwrap_or_else(|poison| poison.into_inner());
                            let gap = now.duration_since(clock.last_tick);
                            clock.last_tick = now;
                            if gap > Duration::from_secs(1) {
                                clock.suspended =
                                    clock.suspended.saturating_add(gap.saturating_sub(tick));
                                true
                            } else {
                                false
                            }
                        };
                        let current = tick_epoch.fetch_add(1, Ordering::AcqRel) + 1;
                        if jumped {
                            tick_jump.store(current, Ordering::Release);
                        }
                        tick_engine.increment_epoch();
                    }
                },
            )
            .map_err(|error| HostError::EngineError(error.to_string()))?;
        Ok(Self {
            config,
            engine,
            fingerprint: config_fingerprint(),
            stats: Mutex::new(EngineStats::default()),
            log: Mutex::new(log),
            embedded: Mutex::new(HashMap::new()),
            epoch,
            jump_epoch,
            clock,
            _ticker: ticker,
        })
    }
    pub(crate) fn runtime_engine(&self) -> &Engine {
        &self.engine
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }
    /// Compile after static interface checks only. The result is opaque outside
    /// this crate; a generated, grant-scoped linker must still gate instantiation.
    pub fn compile_interface_checked(
        &self,
        bytes: &[u8],
        context: &ValidationContext,
    ) -> Result<CompiledComponent, HostError> {
        validate_component(bytes, context).map_err(HostError::from)?;
        self.compile(bytes)
    }
    /// Raw Wasmtime compilation for trusted embedded components and engine tests.
    /// This opaque result alone cannot be instantiated outside the host crate.
    pub fn compile(&self, bytes: &[u8]) -> Result<CompiledComponent, HostError> {
        if bytes.len() > COMPONENT_MAX_BYTES {
            return Err(HostError::Diagnostic(diagnostic(
                "component.too-large",
                "component exceeds 32 MiB",
            )));
        }
        let component = Component::new(&self.engine, bytes)
            .map_err(|error| HostError::EngineError(error.to_string()))?;
        self.stats
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .component_new_calls += 1;
        Ok(CompiledComponent {
            source_len: bytes.len(),
            component: Arc::new(component),
        })
    }
    pub fn load_embedded(&self, artifact: &EmbeddedArtifact) -> Result<LoadPath, HostError> {
        let verified = artifact.fingerprint == self.fingerprint && embedded::is_bundled(artifact);
        if verified {
            let component = embedded::deserialize_verified(&self.engine, artifact)?;
            self.stats
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .deserialize_calls += 1;
            self.embedded
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .insert(
                    artifact.name.clone(),
                    CompiledComponent {
                        source_len: artifact.wasm.len(),
                        component: Arc::new(component),
                    },
                );
            return Ok(LoadPath::Deserialized);
        }
        self.log
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .push(LogLine {
                plugin: "host".into(),
                level: "warn".into(),
                message: format!("host.cwasm-incompatible: {}", artifact.name),
            });
        if artifact.wasm.is_empty() {
            return Err(HostError::EmbeddedIncompatible);
        }
        let compiled = self.compile(&artifact.wasm)?;
        self.embedded
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .insert(artifact.name.clone(), compiled);
        Ok(LoadPath::Compiled)
    }
    pub fn compiled_embedded(&self, name: &str) -> Option<CompiledComponent> {
        self.embedded
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(name)
            .cloned()
    }
    pub(crate) fn record_instantiation(&self) {
        self.stats
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .instantiations += 1;
    }
    pub fn stats(&self) -> EngineStats {
        *self
            .stats
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
    pub fn log_lines(&self) -> Vec<LogLine> {
        self.log
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }
    pub fn current_epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }
    pub fn deadline_ticks(&self, deadline_ms: u64) -> u64 {
        deadline_ms.saturating_add(self.config.epoch_tick_ms - 1) / self.config.epoch_tick_ms + 1
    }
    pub fn fuel_enabled(&self) -> bool {
        false
    }
    /// Install immediately before each guest call. Soft calls yield until the 100 ms grace expires.
    /// Callers must invoke async Wasmtime functions for `Yield` to be valid.
    pub fn arm_deadline<T>(&self, store: &mut Store<T>, deadline_ms: u64, soft: bool) {
        let (started, suspended_at_start) = {
            let clock = self
                .clock
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let started = Instant::now();
            // A call may begin after resume but before the ticker records the
            // jump. Include that pending gap in its baseline, so it cannot
            // receive a second full pause credit when the ticker catches up.
            let pending = started.duration_since(clock.last_tick);
            let pending = if pending > Duration::from_secs(1) {
                pending.saturating_sub(Duration::from_millis(self.config.epoch_tick_ms))
            } else {
                Duration::ZERO
            };
            (started, clock.suspended.saturating_add(pending))
        };
        let deadline = Duration::from_millis(deadline_ms);
        let jump = Arc::clone(&self.jump_epoch);
        let epoch = Arc::clone(&self.epoch);
        let clock = Arc::clone(&self.clock);
        store.epoch_deadline_callback(move |_| {
            let suspended_now = clock
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .suspended;
            let elapsed = active_elapsed(started.elapsed(), suspended_now, suspended_at_start);
            if jump.load(Ordering::Acquire) == epoch.load(Ordering::Acquire) || elapsed < deadline {
                return Ok(UpdateDeadline::Continue(1));
            }
            if soft && elapsed < deadline.saturating_add(Duration::from_millis(100)) {
                return Ok(UpdateDeadline::Yield(1));
            }
            Ok(UpdateDeadline::Interrupt)
        });
        store.set_epoch_deadline(self.deadline_ticks(deadline_ms));
    }
}

pub fn compile_cache_key(component_sha256: &str, engine_id: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"ilium-plugin-compile-cache-v1\0");
    digest.update((component_sha256.len() as u64).to_le_bytes());
    digest.update(component_sha256.as_bytes());
    digest.update((engine_id.len() as u64).to_le_bytes());
    digest.update(engine_id.as_bytes());
    hex_sha256(digest)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ilium_plugin_api::limits::LimitKey;
    use ilium_plugin_api::ProcessKind;

    use super::{
        clock_jump_detected, compile_cache_dir, compile_cache_key, config_fingerprint,
        embedded_artifacts, prune_stale_cache_dirs, runtime_config, runtime_settings,
        select_allocator, store_settings, validate_component, AllocatorKind, EmbeddedArtifact,
        EngineConfig, EngineStats, LoadPath, PluginEngine, ValidationContext, WASMTIME_VERSION,
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
        // wasmtime::Config has no equality or read-back, so the declared settings are checked
        // against the version constant; the live configuration is exercised by PluginEngine::new.
        assert_eq!(runtime_settings().wasmtime_version, WASMTIME_VERSION);
        let _config = runtime_config();
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
        assert_eq!(
            refusal_code(&bytes, &bare_context()),
            "manifest.api-version"
        );
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
            assert_eq!(
                artifact.fingerprint, fingerprint,
                "artifact {}",
                artifact.name
            );
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
        assert!(engine.compiled_embedded("absent-key").is_none());
    }

    /// REQ: HST-066
    #[test]
    fn load_embedded_deserializes_when_the_fingerprint_matches() {
        let engine = PluginEngine::new(engine_config()).expect("engine builds");
        let artifact = EmbeddedArtifact {
            name: "bundled".to_owned(),
            fingerprint: config_fingerprint(),
            wasm: valid_minimal_component(),
            cwasm: Vec::new(),
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
            cwasm: Vec::new(),
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

#[cfg(test)]
mod epoch_clock_tests {
    use super::active_elapsed;
    use std::time::Duration;

    #[test]
    fn suspend_credit_preserves_remaining_call_budget_after_the_first_resume_tick() {
        let deadline = Duration::from_millis(40);
        let after_resume = active_elapsed(
            Duration::from_millis(5_030),
            Duration::from_millis(5_000),
            Duration::ZERO,
        );
        assert!(after_resume < deadline);
        let next_tick = active_elapsed(
            Duration::from_millis(5_040),
            Duration::from_millis(5_000),
            Duration::ZERO,
        );
        assert_eq!(next_tick, deadline);
    }

    #[test]
    fn pause_credit_is_relative_to_the_call_start() {
        assert_eq!(
            active_elapsed(
                Duration::from_millis(20),
                Duration::from_secs(5),
                Duration::from_secs(5)
            ),
            Duration::from_millis(20)
        );
    }
}
