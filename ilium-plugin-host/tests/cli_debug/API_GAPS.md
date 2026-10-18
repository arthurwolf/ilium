# API gaps: cli_debug (log, trace, dev, plugin CLI) tests

No new dev-dependencies. Everything below is missing from the skeleton.

- ilium-plugin-host `cli::CliEnv::new(data_dir: PathBuf, state_dir: PathBuf, config_dir: PathBuf, cwd: PathBuf, now: SystemTime, is_ci: bool, fetcher: Arc<dyn Fetcher>) -> CliEnv` plus `with_session(Arc<dyn SessionBackend>)`, `with_probe(Arc<dyn ToolProbe>)`, `with_var(&str, &str)`; DEV-650..830. Why: the struct literal cannot carry a session or a probe.
- ilium-plugin-host `cli::{SessionBackend, ToolProbe, SessionPlugin{dev, running}, ProfileCall{export, duration_ms}}`: traits and structs that a fake session implements; DEV-720..830, HST-361.
- ilium-plugin-host `cli::{component_has_name_section(&[u8]) -> bool, source_map_path(Side) -> PathBuf, release_tag_matches(&str, &str) -> bool, sha256_sidecar_line(&[u8], &str) -> String}`; DEV-715, 716, 741, 742.
- ilium-plugin-host `telemetry::{TraceKind, TraceRecord, TraceRing, TRACE_RING_CAPACITY, redact_detail, check_unredacted}`: ring, redaction, `trace.unredacted-denied`; DEV-670..676, 823.
- ilium-plugin-host `telemetry::{InspectNode, render_inspector, MemoryView, render_memory_view, format_binary, Profiler, TimingRecord}`; DEV-680..702, 830.
- ilium-plugin-host `telemetry::{DevLoop, DevBackend, RebuildOutcome, DevReport, DevInstall}`; DEV-710..713.
- ilium-plugin-host `telemetry::{PluginLog, LogConfig, LogLevel, LogSide, format_log_line, truncate_message, resolve_threshold, restart_log, disabled_log, plugin_log_dir, format_local_time, mark_uninstalled, mark_reinstalled, purge_expired_logs, LOG_MESSAGE_MAX_BYTES, LOG_ROTATE_BYTES}`; DEV-650..667.
- ilium-plugin-host `cli::CliOutput.stdout_text: String` (human text of stdout); DEV-660..667.
- Serialize on `HourBucket`, `Span` and `TraceRecord`; HST-361, DEV-720.
- Telemetry types used by cli_debug (`SideTelemetry`, `HourBucket`, `COUNTER_NAMES`): see telemetry/API_GAPS.md.
- ilium-plugin-host `telemetry::Profiler::overhead_fraction(&self) -> f64`; DEV-693. Why: the 5 percent bound needs a measurable overhead.
- ilium-plugin-host `telemetry::Profiler::{new() -> Profiler, record(&mut self, &str, Duration), timing(&self, &str) -> TimingRecord}` with `TimingRecord{calls: u64, total_ms: f64, p50_ms: Option<f64>, p95_ms: Option<f64>, max_ms: Option<f64>, over_budget: u64}`; DEV-690..692.
- ilium-plugin-host `telemetry::{TraceRing::begin_instance(&mut self) -> u64, push(&mut self, u64, TraceKind, Value) -> u64, records(&self) -> Vec<TraceRecord>}`, `TraceRecord{seq, time_ms, kind, instance, detail}`; DEV-670..676.
- ilium-plugin-host `telemetry::{MemoryView{side, linear_bytes, peak_bytes, limit_bytes, storage_bytes, instances, restarts}, render_inspector(&str, &InspectNode, usize, usize) -> String, render_memory_view(&str, &MemoryView) -> String}`; DEV-681, 682, 700..702.
- ilium-plugin-host `cli::cli_run("test --replay <file>")` must accept a replay file; DEV-725. Why: replay format is only named in the manual.
