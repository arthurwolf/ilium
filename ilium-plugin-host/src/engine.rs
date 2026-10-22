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
use wasmtime::{Cache, CacheConfig, Config, Engine, InstanceAllocationStrategy, OptLevel, Store, StoreLimits, StoreLimitsBuilder, UpdateDeadline, WasmBacktraceDetails};

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
pub enum AllocatorKind { OnDemand, Pooling }

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
pub fn select_allocator(on_demand: (f64, f64), pooling: (f64, f64), added_rss_mib: f64) -> AllocatorKind {
    if (on_demand.0 > 150.0 || on_demand.1 > 5.0)
        && pooling.0 <= 150.0 && pooling.1 <= 5.0
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
    fn write(&mut self, bytes: &[u8]) { self.0.update(bytes); }
}

fn hex_sha256(hasher: Sha256) -> String {
    hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Includes Wasmtime's own artifact-compatibility hash, not a guessed CPU feature list.
pub fn config_fingerprint() -> String {
    static FINGERPRINT: OnceLock<String> = OnceLock::new();
    FINGERPRINT.get_or_init(|| {
        let mut hasher = Sha256Hasher(Sha256::new());
        WASMTIME_VERSION.hash(&mut hasher);
        match Engine::new(&runtime_config()) {
            Ok(engine) => engine.precompile_compatibility_hash().hash(&mut hasher),
            // The host cannot run with an invalid Config. This unique failure fingerprint
            // must never authenticate an artifact; PluginEngine::new returns the actual error.
            Err(error) => format!("engine-unavailable:{error}").hash(&mut hasher),
        }
        hex_sha256(hasher.0)
    }).clone()
}

pub fn compile_cache_dir(base: &Path) -> PathBuf {
    base.join("ilium").join("plugin-compile")
        .join(format!("{WASMTIME_VERSION}-{}", config_fingerprint()))
}

fn cache_dir_name(name: &str) -> bool {
    let Some((version, digest)) = name.split_once('-') else { return false; };
    version.split('.').count() == 3
        && version.split('.').all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        && digest.len() >= 3 && digest.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Prune only versioned directories in the dedicated cache namespace; preserve unknown files.
pub fn prune_stale_cache_dirs(base: &Path) -> std::io::Result<Vec<PathBuf>> {
    let ilium_dir = base.join("ilium");
    let root = ilium_dir.join("plugin-compile");
    if !root.exists() { return Ok(Vec::new()); }
    for directory in [&ilium_dir, &root] {
        if !std::fs::symlink_metadata(directory)?.file_type().is_dir() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput,
                "cache namespace must be a real directory"));
        }
    }
    let keep = format!("{WASMTIME_VERSION}-{}", config_fingerprint());
    let mut removed = Vec::new();
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == keep || !cache_dir_name(&name) || !entry.file_type()?.is_dir() { continue; }
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
        Self { world: Some(world), api_version, requested: Vec::new(), expected_world_text: None }
    }
    pub fn bare(api_version: (u16, u16, u16)) -> Self {
        Self { world: None, api_version, requested: Vec::new(), expected_world_text: None }
    }
    pub fn requesting(mut self, grants: &[&str]) -> Self {
        self.requested = grants.iter().map(|grant| (*grant).to_owned()).collect(); self
    }
    pub fn expecting_world_text(mut self, text: &str) -> Self {
        self.expected_world_text = Some(text.to_owned()); self
    }
}

fn diagnostic(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(Code::from_static(code), "", message)
}

fn validator_features() -> WasmFeatures {
    let mut features = WasmFeatures::default();
    for flag in [WasmFeatures::THREADS, WasmFeatures::RELAXED_SIMD,
        WasmFeatures::MULTI_MEMORY, WasmFeatures::MEMORY64, WasmFeatures::GC,
        WasmFeatures::CM_ASYNC] {
        features.set(flag, false);
    }
    features.set(WasmFeatures::COMPONENT_MODEL, true);
    features
}

/// Statically known host imports. This list is only structural validation;
/// generated linker types and parsed grants must authorize instantiation.
fn known_host_import(interface: &str) -> bool {
    matches!(interface,
        "audio" | "catalogs" | "cli" | "client-requests" | "clipboard" | "clock" |
        "codec" | "commands" | "extensions" | "fs" | "geocode" | "gpu" | "help" |
        "http" | "http-api" | "inference" | "intents" | "intents-host" | "jobs" |
        "journal" | "kernel-settings" | "keymap" | "keys" | "location" | "log" |
        "media" | "messages" | "panes" | "plugins" | "process" | "services" |
        "sessions" | "settings" | "sounds" | "storage" | "timers" | "topics" |
        "tree" | "ui" | "ui-host" | "vector" | "workspace")
}

fn import_allowed(name: &str, requested: &[String]) -> Result<(), Diagnostic> {
    let base = name.split('@').next().unwrap_or(name);
    if base.starts_with("wasi:sockets/") || base.starts_with("wasi:http/") {
        return Err(diagnostic("component.wasi-forbidden", format!("forbidden import {name}")));
    }
    if base.starts_with("wasi:filesystem/") {
        // An import is syntactically supported even with zero filesystem grants.
        // This says nothing about preopens or operations: the later WASI linker
        // must derive those from parsed, effective fs scopes and fail closed.
        return Ok(());
    }
    if base.starts_with("wasi:cli/") || base.starts_with("wasi:clocks/") ||
        base.starts_with("wasi:random/") || base.starts_with("wasi:io/") ||
        base.starts_with("ilium:primitives/") ||
        base == "ilium:anim/surface" || base == "ilium:anim/fonts" ||
        matches!(base, "ilium:host/log" | "ilium:host/clock" | "ilium:host/timers" |
            "ilium:host/storage" | "ilium:host/settings" | "ilium:host/codec" | "ilium:host/messages") {
        return Ok(());
    }
    let Some(interface) = base.strip_prefix("ilium:host/") else {
        return Err(diagnostic("component.import-unknown", format!("unknown import {name}")));
    };
    if !known_host_import(interface) {
        return Err(diagnostic("component.import-unknown", format!("unknown import {name}")));
    }
    if requested.iter().any(|grant| grant == interface || grant.starts_with(&format!("{interface}."))) {
        Ok(())
    } else {
        Err(diagnostic("manifest.import-not-requested", format!("import {name} needs a requested grant")))
    }
}

/// Ordered static checks only; this does not authorize linking or instantiation.
pub fn validate_component(bytes: &[u8], context: &ValidationContext) -> Result<(), Diagnostic> {
    Validator::new_with_features(validator_features()).validate_all(bytes)
        .map_err(|error| diagnostic("component.invalid", error.to_string()))?;
    if !Parser::is_component(bytes) {
        return Err(diagnostic("component.invalid", "module is not a component"));
    }
    if bytes.len() > COMPONENT_MAX_BYTES {
        return Err(diagnostic("component.too-large", "component exceeds 32 MiB"));
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
                if nested_depth == 0 && section.name() == "ilium:api-version" { api_sections.push(section.data().to_vec()); }
            }
            Payload::ComponentImportSection(section) if nested_depth == 0 => {
                for import in section {
                    imports.push(import.map_err(|error| diagnostic("component.invalid", error.to_string()))?.name.name.to_owned());
                }
            }
            Payload::ComponentExportSection(section) if nested_depth == 0 => {
                for export in section {
                    exports.push(export.map_err(|error| diagnostic("component.invalid", error.to_string()))?.name.name.to_owned());
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
        if context.expected_world_text.as_deref().is_some_and(|text| text != world.wit_name()) ||
            !exports.iter().any(|name| name.starts_with("ilium:exports/lifecycle@")) ||
            !exports.iter().any(|name| name.starts_with(required)) {
            return Err(diagnostic("component.world-mismatch", format!("component does not export {} base world", world.wit_name())));
        }
    }
    let expected = [context.api_version.0.to_be_bytes(), context.api_version.1.to_be_bytes(), context.api_version.2.to_be_bytes()].concat();
    if api_sections.len() != 1 || api_sections[0] != expected {
        return Err(diagnostic("manifest.api-version", "missing or incompatible ilium:api-version"));
    }
    if custom_too_large || custom_total > CUSTOM_TOTAL_MAX_BYTES {
        return Err(diagnostic("component.custom-section-too-large", "custom-section budget exceeded"));
    }
    for import in imports { import_allowed(&import, &context.requested)?; }
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
        memory_bytes: limits.0.get(&LimitKey::MemoryMib).copied().unwrap_or(32).saturating_mul(1024 * 1024),
        table_elements: limits.0.get(&LimitKey::TableElements).copied().unwrap_or(10_000),
        trap_on_grow_failure: true, instances: 16, tables: 8, memories: 4,
    }
}
/// Put this value in Store data and install `store.limiter(|data| &mut data.limits)`.
pub fn store_limits(limits: &ResolvedLimits) -> StoreLimits {
    let settings = store_settings(limits);
    StoreLimitsBuilder::new()
        .memory_size(settings.memory_bytes.min(usize::MAX as u64) as usize)
        .table_elements(settings.table_elements.min(usize::MAX as u64) as usize)
        .instances(settings.instances as usize).tables(settings.tables as usize)
        .memories(settings.memories as usize).trap_on_grow_failure(true).build()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedArtifact {
    pub name: String,
    pub fingerprint: String,
    pub wasm: Vec<u8>,
    /// A trusted release-time precompile, never bytes from a plugin package or disk cache.
    pub cwasm: Vec<u8>,
}
pub fn embedded_artifacts() -> Vec<EmbeddedArtifact> { embedded::bundled_artifacts() }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadPath { Deserialized, Compiled }
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
fn active_elapsed(elapsed: Duration, suspended_now: Duration, suspended_at_start: Duration) -> Duration {
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
        formatter.debug_struct("PluginEngine")
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
            clients: 0, jobs: 0, service_jobs: 0, input_bytes: 0, result_bytes: 0,
            worker_threads: 1, worker_bytes: 3 * 1024 * 1024,
        });
        Self::with_quota(config, &quota)
    }

    /// Reserve the ticker against the same quota as other process workers.
    pub fn with_quota(config: EngineConfig, quota: &QuotaGroup) -> Result<Self, HostError> {
        if config.epoch_tick_ms != 10 { return Err(HostError::EngineError("the pinned epoch tick is 10 ms".into())); }
        let mut runtime = runtime_config();
        let mut log = Vec::new();
        if let Some(cache_dir) = &config.cache_dir {
            let cache_result = (|| {
                if let Some(base) = cache_dir.parent().and_then(Path::parent).and_then(Path::parent) {
                    if *cache_dir == compile_cache_dir(base) { prune_stale_cache_dirs(base)?; }
                }
                if cache_dir.exists() && std::fs::metadata(cache_dir)?.permissions().readonly() {
                    return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "cache directory is read-only"));
                }
                ilium_platform::secure_fs::create_private_directory(cache_dir)?;
                let mut cache_config = CacheConfig::new();
                cache_config.with_directory(cache_dir)
                    .with_file_count_soft_limit(4_096)
                    .with_files_total_size_soft_limit(1024 * 1024 * 1024);
                let cache = Cache::new(cache_config).map_err(|error| std::io::Error::other(error.to_string()))?;
                runtime.cache(Some(cache));
                Ok::<(), std::io::Error>(())
            })();
            if let Err(error) = cache_result {
                log.push(LogLine { plugin: "host".into(), level: "warn".into(),
                    message: format!("host.cache-unavailable: {error}; compiling in memory") });
            }
        }
        let engine = Engine::new(&runtime).map_err(|error| HostError::EngineError(error.to_string()))?;
        let epoch = Arc::new(AtomicU64::new(0));
        let jump_epoch = Arc::new(AtomicU64::new(u64::MAX));
        let clock = Arc::new(Mutex::new(EpochClock { last_tick: Instant::now(), suspended: Duration::ZERO }));
        // The reservation owns the quota lease through physical thread retirement.
        // Account an explicit native stack plus bounded ticker-owned state.
        let reservation = reserve_admitted_worker(quota, 2 * 1024 * 1024, 64 * 1024)
            .map_err(|error| HostError::EngineError(error.to_string()))?;
        let tick_engine = engine.clone();
        let tick_epoch = Arc::clone(&epoch);
        let tick_jump = Arc::clone(&jump_epoch);
        let tick_clock = Arc::clone(&clock);
        let tick = Duration::from_millis(config.epoch_tick_ms);
        let ticker = reservation.spawn(
            "ilium-plugin-epoch", WorkerKind::Cooperative, StopToken::default(), || {},
            move |stop| {
            while !stop.is_stopped() {
                thread::sleep(tick);
                if stop.is_stopped() { break; }
                let now = Instant::now();
                let jumped = {
                    let mut clock = tick_clock.lock().unwrap_or_else(|poison| poison.into_inner());
                    let gap = now.duration_since(clock.last_tick);
                    clock.last_tick = now;
                    if gap > Duration::from_secs(1) {
                        clock.suspended = clock.suspended.saturating_add(gap.saturating_sub(tick));
                        true
                    } else { false }
                };
                let current = tick_epoch.fetch_add(1, Ordering::AcqRel) + 1;
                if jumped { tick_jump.store(current, Ordering::Release); }
                tick_engine.increment_epoch();
            }
        }).map_err(|error| HostError::EngineError(error.to_string()))?;
        Ok(Self { config, engine, fingerprint: config_fingerprint(), stats: Mutex::new(EngineStats::default()),
            log: Mutex::new(log), embedded: Mutex::new(HashMap::new()), epoch, jump_epoch,
            clock, _ticker: ticker })
    }
    pub(crate) fn runtime_engine(&self) -> &Engine { &self.engine }

    pub fn config(&self) -> &EngineConfig { &self.config }
    /// Compile after static interface checks only. The result is opaque outside
    /// this crate; a generated, grant-scoped linker must still gate instantiation.
    pub fn compile_interface_checked(&self, bytes: &[u8], context: &ValidationContext) -> Result<CompiledComponent, HostError> {
        validate_component(bytes, context).map_err(HostError::from)?;
        self.compile(bytes)
    }
    /// Raw Wasmtime compilation for trusted embedded components and engine tests.
    /// This opaque result alone cannot be instantiated outside the host crate.
    pub fn compile(&self, bytes: &[u8]) -> Result<CompiledComponent, HostError> {
        if bytes.len() > COMPONENT_MAX_BYTES {
            return Err(HostError::Diagnostic(diagnostic("component.too-large", "component exceeds 32 MiB")));
        }
        let component = Component::new(&self.engine, bytes)
            .map_err(|error| HostError::EngineError(error.to_string()))?;
        self.stats.lock().unwrap_or_else(|poison| poison.into_inner()).component_new_calls += 1;
        Ok(CompiledComponent { source_len: bytes.len(), component: Arc::new(component) })
    }
    pub fn load_embedded(&self, artifact: &EmbeddedArtifact) -> Result<LoadPath, HostError> {
        let verified = artifact.fingerprint == self.fingerprint && embedded::is_bundled(artifact);
        if verified {
            let component = embedded::deserialize_verified(&self.engine, artifact)?;
            self.stats.lock().unwrap_or_else(|poison| poison.into_inner()).deserialize_calls += 1;
            self.embedded.lock().unwrap_or_else(|poison| poison.into_inner()).insert(
                artifact.name.clone(), CompiledComponent { source_len: artifact.wasm.len(), component: Arc::new(component) });
            return Ok(LoadPath::Deserialized);
        }
        self.log.lock().unwrap_or_else(|poison| poison.into_inner()).push(LogLine {
            plugin: "host".into(), level: "warn".into(),
            message: format!("host.cwasm-incompatible: {}", artifact.name),
        });
        if artifact.wasm.is_empty() { return Err(HostError::EmbeddedIncompatible); }
        let compiled = self.compile(&artifact.wasm)?;
        self.embedded.lock().unwrap_or_else(|poison| poison.into_inner()).insert(artifact.name.clone(), compiled);
        Ok(LoadPath::Compiled)
    }
    pub fn compiled_embedded(&self, name: &str) -> Option<CompiledComponent> {
        self.embedded.lock().unwrap_or_else(|poison| poison.into_inner()).get(name).cloned()
    }
    pub(crate) fn record_instantiation(&self) { self.stats.lock().unwrap_or_else(|poison| poison.into_inner()).instantiations += 1; }
    pub fn stats(&self) -> EngineStats { *self.stats.lock().unwrap_or_else(|poison| poison.into_inner()) }
    pub fn log_lines(&self) -> Vec<LogLine> { self.log.lock().unwrap_or_else(|poison| poison.into_inner()).clone() }
    pub fn current_epoch(&self) -> u64 { self.epoch.load(Ordering::Acquire) }
    pub fn deadline_ticks(&self, deadline_ms: u64) -> u64 {
        deadline_ms.saturating_add(self.config.epoch_tick_ms - 1) / self.config.epoch_tick_ms + 1
    }
    pub fn fuel_enabled(&self) -> bool { false }
    /// Install immediately before each guest call. Soft calls yield until the 100 ms grace expires.
    /// Callers must invoke async Wasmtime functions for `Yield` to be valid.
    pub fn arm_deadline<T>(&self, store: &mut Store<T>, deadline_ms: u64, soft: bool) {
        let (started, suspended_at_start) = {
            let clock = self.clock.lock().unwrap_or_else(|poison| poison.into_inner());
            let started = Instant::now();
            // A call may begin after resume but before the ticker records the
            // jump. Include that pending gap in its baseline, so it cannot
            // receive a second full pause credit when the ticker catches up.
            let pending = started.duration_since(clock.last_tick);
            let pending = if pending > Duration::from_secs(1) { pending.saturating_sub(Duration::from_millis(self.config.epoch_tick_ms)) } else { Duration::ZERO };
            (started, clock.suspended.saturating_add(pending))
        };
        let deadline = Duration::from_millis(deadline_ms);
        let jump = Arc::clone(&self.jump_epoch);
        let epoch = Arc::clone(&self.epoch);
        let clock = Arc::clone(&self.clock);
        store.epoch_deadline_callback(move |_| {
            let suspended_now = clock.lock().unwrap_or_else(|poison| poison.into_inner()).suspended;
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
mod epoch_clock_tests {
    use super::active_elapsed;
    use std::time::Duration;

    #[test]
    fn suspend_credit_preserves_remaining_call_budget_after_the_first_resume_tick() {
        let deadline = Duration::from_millis(40);
        let after_resume = active_elapsed(Duration::from_millis(5_030), Duration::from_millis(5_000), Duration::ZERO);
        assert!(after_resume < deadline);
        let next_tick = active_elapsed(Duration::from_millis(5_040), Duration::from_millis(5_000), Duration::ZERO);
        assert_eq!(next_tick, deadline);
    }

    #[test]
    fn pause_credit_is_relative_to_the_call_start() {
        assert_eq!(active_elapsed(Duration::from_millis(20), Duration::from_secs(5), Duration::from_secs(5)), Duration::from_millis(20));
    }
}

