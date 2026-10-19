# API gaps found by the host-runtime lane

One line per missing item: crate, signature, requirement ids, reason. Tests call these as the manual implies; they do not compile until the skeleton adds them.

## ilium-plugin-host::engine
- `pub const WASMTIME_VERSION: &str` (HST-050) pinned version constant.
- `pub fn runtime_settings() -> RuntimeSettings` with pub fields `wasmtime_version, component_model, component_model_async, concurrency_support, epoch_interruption, consume_fuel, max_wasm_stack, async_stack_size, signals_based_traps, allocator: AllocatorKind` (HST-050..060, 063) Config introspection.
- `pub enum AllocatorKind { OnDemand, Pooling }`; `pub fn select_allocator(on_demand: (f64, f64), pooling: (f64, f64), added_rss_mib: f64) -> AllocatorKind` (HST-063, 064).
- `pub fn config_fingerprint() -> String` (HST-057, 053).
- `pub struct EngineConfig { pub epoch_tick_ms: u64, pub cache_dir: Option<PathBuf>, pub process: ProcessKind }`; `PluginEngine::{new(EngineConfig)->Result<_,_>, compile(&[u8]), load_embedded(&EmbeddedArtifact)->Result<LoadPath,_>, stats()->EngineStats{deserialize_calls,component_new_calls,instantiations}, log_lines(), current_epoch(), config()}` (HST-056..088).
- `pub enum LoadPath { Deserialized, Compiled }`; `pub struct EmbeddedArtifact { pub name, pub fingerprint: String, pub wasm: Vec<u8> }`; `pub fn embedded_artifacts() -> Vec<EmbeddedArtifact>` (HST-053, 066, 088).
- `pub fn compile_cache_dir(base: &Path) -> PathBuf`; `pub fn prune_stale_cache_dirs(base: &Path) -> io::Result<Vec<PathBuf>>` (HST-061, 062).
- `pub fn validate_component(bytes: &[u8], ctx: &ValidationContext) -> Result<(), Diagnostic>`; `ValidationContext::{new(World,(u16,u16,u16)), bare(..), requesting(&[&str]), expecting_world_text(&str)}` (HST-072, 077, 084..086).
- `pub fn clock_jump_detected(previous_ms: u64, now_ms: u64) -> bool` (HST-082).
## ilium-plugin-host::calls
- `pub fn deadline_ticks(deadline_ms: u64) -> u64`; `pub struct ResolvedLimits(pub BTreeMap<LimitKey,u64>)`; `pub fn store_settings(&ResolvedLimits) -> StoreSettings{memory_bytes,table_elements,trap_on_grow_failure,instances,tables,memories}` (HST-074, 081).
## ilium-plugin-host::linker
- `pub enum World { Server, Client }`; `pub fn linker_cache_key(Side, Tier, &[Grant]) -> String`; `pub fn build_linker(World, &[Grant]) -> Result<LinkerHandle{world},_>` (HST-070).
## ilium-plugin-host::telemetry
- `pub fn cut_wasi_line(&str) -> String` (HST-078).
## ilium-plugin-host::session
- `pub struct SessionLimits { pub session_memory_mib, session_helpers, session_processes, session_http_streams, session_decorations, session_pending_requests, session_key_grabs, session_children }` with `Default` and `from_user(&[(&str,u64)])` (HST-406, API-070).
## ilium-plugin-host::install / cli
- `pub fn install_with_limits(dir:&Path, Tier, &Limits) -> InstallReport{installed, log}` (HST-404); `Installer::{new, install(&[u8], &dyn Consent), check(&[u8], CheckMode)}`; `pub fn cli::run_check_dir(&Path) -> CliOutput` (API-080); `CliEnv` fields as used in `hst_050_doctor_*`; `error.code` on install errors.
## ilium-plugin-host::testing (HostHarness / HarnessHandle)
- Builder: `.tier(Tier)`, `.grants(&[&str])`, `.limits(Limits)`, `.kernel(Arc<FakeKernel>)`, `.both_sides()`, `.two_plugins()`, `.activate_error(&str)`, `.session_memory_mib(u64)`, `.session_helpers(u64)`, `.session_pending_requests(u64)`, `.with_wasm_bytes(&[u8])` (many ids).
- Handle: `state()`, `state_of(Side)`, `failures()`, `failure_count()`, `failure_count_of(Side)`, `fail_side(Side)`, `restart_delay_ms()`, `enable()`, `disable()`, `grant(&str)`, `revoke(&str)`, `revoke_without_reactivation(&str)`, `decline(&str)`, `upgrade_in_place()`, `shutdown()`, `fail_job()`, `set_guest(Box<dyn NativeGuest>)`, `set_deactivate_result(..)`, `restore_bytes()`, `set_render_result(..)`, `render_view(&str)`, `fail_scene_frame()`, `call_import(iface, func, json_args) -> ImportReply`, `call_import_as(usize, ..)`, `open_stream_as(usize)`, `start_slow_request()`, `complete_oldest_request()`, `completion_of(id)`, `pending_requests()`, `fill_session_pending(n)`, `charge_cpu_ms(u64)`, `quota_balance_ms()`, `queued_events()`, `queued_timer_ticks()`, `queue_timer_tick(id)`, `store_data()`, `grant_generation()`, `instance_pre_generation()`, `tick_observations()`, `tick_decision_names()`, `inject_clock_jump(Duration)`, `activate_second_plugin_with_memory(u64)`, `activate_second_plugin_with_helper()`, `second_plugin_state()/reason()/failure_count()`, `free_session_memory_mib(u64)`, `kernel()`, `logs()` items with `plugin, level, message`.
- `ImportReply::{error() -> Option<PluginError>, error_code() -> Option<&str>, trap_code() -> Option<String>, retry_after_ms(), request_id(), timer_id()}`; `CompletionView::failed_code()`.
- ilium-plugin-host dev-dependency: sha2 = "0.10" (HST-070: assert linker_cache_key equals the SHA-256 hex of the sorted grant names plus side and tier)
