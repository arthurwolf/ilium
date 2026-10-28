//! Benchmark and budget support (test-plan.md section 2.8, reference-host-budgets.md).
//!
//! Holds the run harness (iteration minimums, pinning, result record), the budget table and the
//! programs. B1 is measured against real host code; a program that is not built yet reports
//! `BenchError::ProgramFailed` instead of inventing numbers.

use std::collections::BTreeMap;
use std::hint::black_box;
use std::time::Instant;

use ilium_plugin_host::engine::RuntimeSettings;

/// The fewest warm-up iterations a run may use (reference-host-budgets.md HST-300).
pub const MIN_WARMUP_ITERATIONS: u32 = 100;

/// The fewest measured iterations a run may use (reference-host-budgets.md HST-300).
pub const MIN_MEASURED_ITERATIONS: u32 = 1000;

/// The benchmark ids whose budgets gate a release (reference-host-budgets.md HST-306).
pub const RELEASE_GATE_IDS: &[&str] = &[
    "B1", "B2", "B3", "B4", "B5", "B6", "B7", "B8", "B9", "B16", "B17", "B18", "B19", "B20", "B23",
];

/// How many iterations one benchmark run performs (reference-host-budgets.md HST-300).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunSpec {
    /// Warm-up iterations, at least `MIN_WARMUP_ITERATIONS`.
    pub warmup_iterations: u32,
    /// Measured iterations, at least `MIN_MEASURED_ITERATIONS`.
    pub measured_iterations: u32,
}

/// Why a benchmark run was refused or failed (reference-host-budgets.md HST-300, HST-307).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BenchError {
    /// The run spec is below the iteration minimums (HST-300).
    BelowMinimum(String),
    /// Benchmarks run only from a release build (HST-307).
    NotReleaseBuild,
    /// No program has that id (HST-305).
    UnknownProgram(String),
    /// The program failed.
    ProgramFailed(String),
}

/// One benchmark run's result record (reference-host-budgets.md HST-301).
#[derive(Debug, Clone, PartialEq)]
pub struct BenchResult {
    /// Median of the measured iterations, nanoseconds.
    pub median_ns: u64,
    /// 99th percentile of the measured iterations, nanoseconds.
    pub p99_ns: u64,
    /// Warm-up iterations performed.
    pub warmup_iterations: u32,
    /// Measured iterations performed.
    pub measured_iterations: u32,
    /// The core the run was pinned to, when pinning worked (HST-300).
    pub pinned_core: Option<usize>,
    /// The Wasmtime version.
    pub wasmtime_version: String,
    /// The pinned engine settings the run used (`engine::runtime_config()`).
    pub wasmtime_config: RuntimeSettings,
    /// The CPU model string.
    pub cpu_model: String,
    /// The number of cores.
    pub core_count: usize,
    /// The operating system.
    pub os: String,
    /// The source commit.
    pub commit: String,
    /// Comma-separated enabled target features (HST-308).
    pub target_features: String,
    /// Every metric the program recorded, by metric name (HST-303).
    pub metrics: BTreeMap<String, f64>,
}

impl BenchResult {
    /// The result as the JSON record `ilium plugin bench --json` prints (reference-host-budgets.md HST-301).
    pub fn to_json(&self) -> serde_json::Value {
        let config = &self.wasmtime_config;
        serde_json::json!({
            "median_ns": self.median_ns,
            "p99_ns": self.p99_ns,
            "warmup_iterations": self.warmup_iterations,
            "measured_iterations": self.measured_iterations,
            "pinned_core": self.pinned_core,
            "wasmtime_version": self.wasmtime_version,
            "wasmtime_config": {
                "wasmtime_version": config.wasmtime_version,
                "component_model": config.component_model,
                "component_model_async": config.component_model_async,
                "concurrency_support": config.concurrency_support,
                "epoch_interruption": config.epoch_interruption,
                "consume_fuel": config.consume_fuel,
                "max_wasm_stack": config.max_wasm_stack,
                "async_stack_size": config.async_stack_size,
                "signals_based_traps": config.signals_based_traps,
                "allocator": format!("{:?}", config.allocator),
            },
            "cpu_model": self.cpu_model,
            "core_count": self.core_count,
            "os": self.os,
            "commit": self.commit,
            "target_features": self.target_features,
            "metrics": self.metrics,
        })
    }
}

/// Runs the program `id` with an explicit spec (reference-host-budgets.md HST-300).
pub fn run_with(id: &str, spec: &RunSpec) -> Result<BenchResult, BenchError> {
    if spec.warmup_iterations < MIN_WARMUP_ITERATIONS
        || spec.measured_iterations < MIN_MEASURED_ITERATIONS
    {
        return Err(BenchError::BelowMinimum(format!(
            "{} warm-up and {} measured iterations; at least {MIN_WARMUP_ITERATIONS} and {MIN_MEASURED_ITERATIONS} are required",
            spec.warmup_iterations, spec.measured_iterations
        )));
    }
    if !KNOWN_PROGRAM_IDS.contains(&id) {
        return Err(BenchError::UnknownProgram(id.to_string()));
    }
    // One pinned core for the whole run, where the platform allows it.
    let pinned_core = ilium_platform::thread_priority::pin_current_thread_to_first_allowed_core();
    let (median_ns, p99_ns, metrics) = run_program(id, spec)?;
    Ok(BenchResult {
        median_ns,
        p99_ns,
        warmup_iterations: spec.warmup_iterations,
        measured_iterations: spec.measured_iterations,
        pinned_core,
        wasmtime_version: ilium_plugin_host::engine::WASMTIME_VERSION.to_string(),
        wasmtime_config: ilium_plugin_host::engine::runtime_settings(),
        cpu_model: cpu_model(),
        core_count: std::thread::available_parallelism().map_or(1, |count| count.get()),
        os: std::env::consts::OS.to_string(),
        commit: source_commit(),
        target_features: target_features(),
        metrics,
    })
}

/// Every program id B1..B24 (HST-305).
const KNOWN_PROGRAM_IDS: &[&str] = &[
    "B1", "B2", "B3", "B4", "B5", "B6", "B7", "B8", "B9", "B10", "B11", "B12", "B13", "B14", "B15",
    "B16", "B17", "B18", "B19", "B20", "B21", "B22", "B23", "B24",
];

/// Runs program `id` and returns its median, 99th percentile and metrics.
fn run_program(id: &str, spec: &RunSpec) -> Result<(u64, u64, BTreeMap<String, f64>), BenchError> {
    match id {
        "B1" => Ok(host_call_program(spec)),
        other => Err(BenchError::ProgramFailed(format!(
            "the {other} program is not built in this crate yet"
        ))),
    }
}

/// B1: an empty host call, a host call with a 64-byte string and a 400 KB `list<u8>` lowered into a
/// host buffer (reference-host-budgets.md B1). The calls are real host functions.
fn host_call_program(spec: &RunSpec) -> (u64, u64, BTreeMap<String, f64>) {
    let line = "x".repeat(64);
    let empty_call = measure(spec, || {
        black_box(ilium_plugin_host::engine::runtime_settings());
    });
    let string_call = measure(spec, || {
        black_box(ilium_plugin_host::telemetry::cut_wasi_line(black_box(&line)));
    });
    let rows = vec![0xA5u8; 400 * 1024];
    let mut host_buffer = vec![0u8; rows.len()];
    let write_rows = measure(spec, || {
        host_buffer.copy_from_slice(black_box(&rows));
        black_box(&host_buffer);
    });
    let mut metrics = BTreeMap::new();
    metrics.insert("median_ns".to_string(), empty_call.0 as f64);
    metrics.insert("p99_ns".to_string(), empty_call.1 as f64);
    metrics.insert("string_64_median_ns".to_string(), string_call.0 as f64);
    metrics.insert("write_rows_400kb_ns".to_string(), write_rows.0 as f64);
    (empty_call.0, empty_call.1, metrics)
}

/// Runs `work` for the warm-up, then times each measured iteration; returns (median, p99) in ns.
fn measure(spec: &RunSpec, mut work: impl FnMut()) -> (u64, u64) {
    for _ in 0..spec.warmup_iterations {
        work();
    }
    let mut samples: Vec<u64> = (0..spec.measured_iterations)
        .map(|_| {
            let started = Instant::now();
            work();
            started.elapsed().as_nanos().max(1) as u64
        })
        .collect();
    samples.sort_unstable();
    let last = samples.len() - 1;
    let median = samples[last / 2];
    let p99 = samples[(last * 99).div_ceil(100)];
    (median, p99)
}

fn cpu_model() -> String {
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|line| line.starts_with("model name"))
                .and_then(|line| line.split_once(':'))
                .map(|(_, model)| model.trim().to_string())
        })
        .filter(|model| !model.is_empty())
        .unwrap_or_else(|| format!("unknown {} cpu", std::env::consts::ARCH))
}

/// The commit of the source, from the `ILIUM_BENCH_COMMIT` variable or `git`; `unknown` otherwise.
fn source_commit() -> String {
    if let Ok(commit) = std::env::var("ILIUM_BENCH_COMMIT") {
        if !commit.trim().is_empty() {
            return commit.trim().to_string();
        }
    }
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Comma-separated CPU features enabled at compile time (HST-308).
fn target_features() -> String {
    let mut features: Vec<&str> = Vec::new();
    macro_rules! feature {
        ($name:literal) => {
            if cfg!(target_feature = $name) {
                features.push($name);
            }
        };
    }
    feature!("sse");
    feature!("sse2");
    feature!("sse3");
    feature!("ssse3");
    feature!("sse4.1");
    feature!("sse4.2");
    feature!("avx");
    feature!("avx2");
    feature!("fma");
    feature!("neon");
    feature!("simd128");
    features.join(",")
}

/// Runs the program `id` with the default spec (reference-host-budgets.md HST-300, HST-307).
pub fn run_benchmark(id: &str) -> Result<BenchResult, BenchError> {
    if !is_release_build() {
        return Err(BenchError::NotReleaseBuild);
    }
    run_with(
        id,
        &RunSpec {
            warmup_iterations: MIN_WARMUP_ITERATIONS,
            measured_iterations: MIN_MEASURED_ITERATIONS,
        },
    )
}

/// The budget verdict for one metric (reference-host-budgets.md HST-302..304).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Within budget.
    Pass,
    /// Over budget.
    Fail,
    /// More than twice the budget on B1, B3 or B4: stop the plan (HST-304).
    PlanStop,
    /// Recorded only, no budget (HST-303).
    Recorded,
}

/// Judges `value` of `metric` of program `id` against its budget (reference-host-budgets.md HST-302..304).
pub fn judge(id: &str, metric: &str, value: f64) -> Verdict {
    let Some(budget) = budget_for(id, metric) else {
        // An unknown id or metric has no budget to meet, so it can never pass.
        return Verdict::Fail;
    };
    let limit = match budget {
        Budget::Recorded => return Verdict::Recorded,
        Budget::AtMost(limit) | Budget::Under(limit) => limit,
    };
    if value.is_nan() {
        return Verdict::Fail;
    }
    if matches!(id, "B1" | "B3" | "B4") && value > limit * 2.0 {
        return Verdict::PlanStop;
    }
    let met = match budget {
        Budget::Under(limit) => value < limit,
        _ => value <= limit,
    };
    if met {
        Verdict::Pass
    } else {
        Verdict::Fail
    }
}

/// How one metric is judged.
#[derive(Clone, Copy)]
enum Budget {
    /// Passes at or below the limit.
    AtMost(f64),
    /// Passes strictly below the limit ("under" in the table where the page sets an exclusive bound).
    Under(f64),
    /// No target: recorded only.
    Recorded,
}

/// The budget table of reference-host-budgets.md, by benchmark id and metric name.
fn budget_for(id: &str, metric: &str) -> Option<Budget> {
    use Budget::{AtMost, Recorded, Under};
    Some(match (id, metric) {
        ("B1", "median_ns") => AtMost(1_000.0),
        ("B1", "write_rows_400kb_ns") => AtMost(200_000.0),
        ("B2", "wall_ms") => AtMost(5.0),
        ("B2", "rss_mib") => AtMost(4.0),
        ("B2", "js_wall_ms") => Recorded,
        ("B3", "p99_ratio") => AtMost(1.3),
        ("B3", "js_vs_v8_ratio") => AtMost(1.1),
        ("B3", "js_vs_v8_baseline_absent_ms") => Recorded,
        ("B4", "extra_ms") => AtMost(1.0),
        ("B5", "p99_ms") => AtMost(2.0),
        ("B6", "host_start_ms") => AtMost(150.0),
        ("B6", "added_rss_mib") => AtMost(120.0),
        ("B6", "binary_size_delta_mib") => Recorded,
        ("B7", "overhead_percent") => AtMost(5.0),
        ("B8", "per_pane_ms") => AtMost(0.5),
        ("B9", "start_ms") => AtMost(5.0),
        ("B9", "p99_ratio_idle") => AtMost(1.2),
        ("B10", "chain_ms") => AtMost(1.0),
        ("B11", "first_frame_ms") => AtMost(5.0),
        ("B12", "p99_ratio_idle") => AtMost(1.5),
        ("B13", "painted_ms") => AtMost(8.0),
        ("B14", "server_cpu_percent") => AtMost(5.0),
        ("B14", "mirror_lag_p99_ms") => AtMost(5.0),
        ("B15", "p99_ms") => AtMost(3.0),
        ("B16", "t1_p99_ms") => AtMost(1.0),
        ("B16", "t2_p99_ms") => AtMost(1.5),
        ("B17", "p99_ms") => AtMost(5.0),
        ("B18", "median_ms") => AtMost(0.2),
        ("B18", "p99_ms") => AtMost(0.5),
        ("B19", "p99_ms") => AtMost(1.0),
        ("B20", "p99_ms") => AtMost(5.0),
        ("B21", _) => Recorded,
        ("B22", "added_rss_mib") => Under(120.0),
        ("B23", "cold_ms") => AtMost(60.0),
        ("B24", "cpu_percent") => AtMost(0.5),
        ("B24", "wakeups_per_s") => AtMost(100.0),
        _ => return None,
    })
}

/// The process exit code for a set of verdicts: non-zero on any `Fail` or `PlanStop` (HST-302).
pub fn exit_code_for(verdicts: &[Verdict]) -> i32 {
    verdicts
        .iter()
        .map(|verdict| match verdict {
            Verdict::Pass | Verdict::Recorded => 0,
            Verdict::Fail => 1,
            // A plan stop is the more serious miss, so it has its own code.
            Verdict::PlanStop => 2,
        })
        .max()
        .unwrap_or(0)
}

/// Whether this binary is a release build; benchmarks refuse to run otherwise (HST-307).
pub fn is_release_build() -> bool {
    !cfg!(debug_assertions)
}

/// The measurements that decide whether instance pooling may be enabled (reference-host-budgets.md HST-312).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PoolingEvidence {
    /// B6 host start with on-demand allocation, milliseconds.
    pub on_demand_host_start_ms: f64,
    /// B2 wall time with on-demand allocation, milliseconds.
    pub on_demand_b2_wall_ms: f64,
    /// B6 host start with pooling, milliseconds.
    pub pooling_host_start_ms: f64,
    /// B2 wall time with pooling, milliseconds.
    pub pooling_b2_wall_ms: f64,
    /// Extra resident memory pooling costs, MiB.
    pub pooling_extra_rss_mib: f64,
}

/// Whether pooling is allowed given the evidence (reference-host-budgets.md HST-312).
pub fn pooling_allowed(evidence: &PoolingEvidence) -> bool {
    const HOST_START_BUDGET_MS: f64 = 150.0;
    const B2_WALL_BUDGET_MS: f64 = 5.0;
    const MAX_EXTRA_RSS_MIB: f64 = 16.0;
    let on_demand_misses = evidence.on_demand_host_start_ms > HOST_START_BUDGET_MS
        || evidence.on_demand_b2_wall_ms > B2_WALL_BUDGET_MS;
    let pooling_meets_both = evidence.pooling_host_start_ms <= HOST_START_BUDGET_MS
        && evidence.pooling_b2_wall_ms <= B2_WALL_BUDGET_MS;
    on_demand_misses && pooling_meets_both && evidence.pooling_extra_rss_mib <= MAX_EXTRA_RSS_MIB
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    /// REQ: HST-300
    #[test]
    fn iteration_minimums_match_the_spec() {
        assert_eq!(MIN_WARMUP_ITERATIONS, 100);
        assert_eq!(MIN_MEASURED_ITERATIONS, 1000);
    }

    /// REQ: HST-306
    #[test]
    fn release_gate_includes_required_benchmarks() {
        let mut required: Vec<String> = (1..=9).map(|n| format!("B{n}")).collect();
        required.extend((16..=20).map(|n| format!("B{n}")));
        required.push("B23".to_string());
        for id in &required {
            assert!(
                RELEASE_GATE_IDS.contains(&id.as_str()),
                "release gate misses {id}"
            );
        }
        assert_eq!(RELEASE_GATE_IDS.len(), required.len());
    }

    /// REQ: HST-300
    #[test]
    fn run_with_refuses_iterations_below_minimum() {
        let zero = RunSpec {
            warmup_iterations: 0,
            measured_iterations: 0,
        };
        assert!(matches!(
            run_with("B1", &zero),
            Err(BenchError::BelowMinimum(_))
        ));
        let short_warmup = RunSpec {
            warmup_iterations: 99,
            measured_iterations: 1000,
        };
        assert!(matches!(
            run_with("B1", &short_warmup),
            Err(BenchError::BelowMinimum(_))
        ));
        let short_measured = RunSpec {
            warmup_iterations: 100,
            measured_iterations: 999,
        };
        assert!(matches!(
            run_with("B1", &short_measured),
            Err(BenchError::BelowMinimum(_))
        ));
    }

    /// REQ: HST-305
    #[test]
    fn run_with_rejects_unknown_program_id() {
        let spec = RunSpec {
            warmup_iterations: 100,
            measured_iterations: 1000,
        };
        assert_eq!(
            run_with("B99", &spec),
            Err(BenchError::UnknownProgram("B99".to_string()))
        );
    }

    /// REQ: HST-303
    #[test]
    fn run_benchmark_refuses_non_release_build() {
        if !is_release_build() {
            assert_eq!(run_benchmark("B1"), Err(BenchError::NotReleaseBuild));
        }
    }

    /// REQ: HST-301
    #[test]
    fn to_json_carries_every_required_field() {
        let mut metrics = BTreeMap::new();
        metrics.insert("median_ns".to_string(), 512.0);
        let result = BenchResult {
            median_ns: 512,
            p99_ns: 900,
            warmup_iterations: 100,
            measured_iterations: 1000,
            pinned_core: Some(2),
            wasmtime_version: "0.0.0-test".to_string(),
            wasmtime_config: ilium_plugin_host::engine::runtime_settings(),
            cpu_model: "test cpu".to_string(),
            core_count: 8,
            os: "linux".to_string(),
            commit: "abc1234".to_string(),
            target_features: "sse2".to_string(),
            metrics,
        };
        let json = result.to_json();
        assert_eq!(json.get("median_ns").and_then(|v| v.as_u64()), Some(512));
        assert_eq!(json.get("p99_ns").and_then(|v| v.as_u64()), Some(900));
        assert_eq!(
            json.get("warmup_iterations").and_then(|v| v.as_u64()),
            Some(100)
        );
        assert_eq!(
            json.get("measured_iterations").and_then(|v| v.as_u64()),
            Some(1000)
        );
        assert_eq!(
            json.get("wasmtime_version").and_then(|v| v.as_str()),
            Some("0.0.0-test")
        );
        assert_eq!(
            json.get("cpu_model").and_then(|v| v.as_str()),
            Some("test cpu")
        );
        assert_eq!(json.get("core_count").and_then(|v| v.as_u64()), Some(8));
        assert_eq!(json.get("os").and_then(|v| v.as_str()), Some("linux"));
        assert_eq!(json.get("commit").and_then(|v| v.as_str()), Some("abc1234"));
        assert!(json.get("wasmtime_config").is_some());
    }

    /// REQ: HST-302, HST-304
    #[test]
    fn judge_b1_pass_fail_and_plan_stop() {
        assert_eq!(judge("B1", "median_ns", 900.0), Verdict::Pass);
        assert_eq!(judge("B1", "median_ns", 1100.0), Verdict::Fail);
        assert_eq!(judge("B1", "median_ns", 2001.0), Verdict::PlanStop);
    }

    /// REQ: HST-302
    #[test]
    fn judge_record_only_benchmark_is_recorded() {
        assert_eq!(judge("B21", "spawn_to_ready_ms", 12.0), Verdict::Recorded);
        assert_eq!(judge("B21", "spawn_to_ready_ms", 1.0e6), Verdict::Recorded);
    }

    /// REQ: HST-302
    #[test]
    fn exit_code_is_non_zero_on_fail_or_plan_stop() {
        assert_eq!(exit_code_for(&[]), 0);
        assert_eq!(exit_code_for(&[Verdict::Pass, Verdict::Recorded]), 0);
        assert_ne!(exit_code_for(&[Verdict::Pass, Verdict::Fail]), 0);
        assert_ne!(exit_code_for(&[Verdict::PlanStop]), 0);
    }

    /// REQ: HST-312
    #[test]
    fn pooling_allowed_only_when_rule_holds() {
        let enabling = PoolingEvidence {
            on_demand_host_start_ms: 200.0,
            on_demand_b2_wall_ms: 4.0,
            pooling_host_start_ms: 120.0,
            pooling_b2_wall_ms: 3.0,
            pooling_extra_rss_mib: 10.0,
        };
        assert!(pooling_allowed(&enabling));

        let too_much_rss = PoolingEvidence {
            pooling_extra_rss_mib: 17.0,
            ..enabling
        };
        assert!(!pooling_allowed(&too_much_rss));

        let on_demand_already_meets = PoolingEvidence {
            on_demand_host_start_ms: 100.0,
            on_demand_b2_wall_ms: 4.0,
            ..enabling
        };
        assert!(!pooling_allowed(&on_demand_already_meets));
    }
}
