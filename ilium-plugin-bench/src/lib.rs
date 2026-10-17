//! Benchmark and budget support (test-plan.md section 2.8, reference-host-budgets.md).
//!
//! Skeleton: the `perf-budgets` test lane fills in the programs and the budget tests.

#![allow(unused_variables, dead_code)]

use std::collections::BTreeMap;

use ilium_plugin_host::engine::RuntimeSettings;

/// The fewest warm-up iterations a run may use (reference-host-budgets.md HST-300).
pub const MIN_WARMUP_ITERATIONS: u32 = 100;

/// The fewest measured iterations a run may use (reference-host-budgets.md HST-300).
pub const MIN_MEASURED_ITERATIONS: u32 = 1000;

/// The benchmark ids whose budgets gate a release (reference-host-budgets.md HST-306).
pub const RELEASE_GATE_IDS: &[&str] = &[
    "B1", "B2", "B3", "B4", "B5", "B6", "B7", "B8", "B9", "B16", "B17", "B18", "B19", "B20",
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
        unimplemented!("bench::BenchResult::to_json")
    }
}

/// Runs the program `id` with an explicit spec (reference-host-budgets.md HST-300).
pub fn run_with(id: &str, spec: &RunSpec) -> Result<BenchResult, BenchError> {
    unimplemented!("bench::run_with")
}

/// Runs the program `id` with the default spec (reference-host-budgets.md HST-300, HST-307).
pub fn run_benchmark(id: &str) -> Result<BenchResult, BenchError> {
    unimplemented!("bench::run_benchmark")
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
    unimplemented!("bench::judge")
}

/// The process exit code for a set of verdicts: non-zero on any `Fail` or `PlanStop` (HST-302).
pub fn exit_code_for(verdicts: &[Verdict]) -> i32 {
    unimplemented!("bench::exit_code_for")
}

/// Whether this binary is a release build; benchmarks refuse to run otherwise (HST-307).
pub fn is_release_build() -> bool {
    unimplemented!("bench::is_release_build")
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
    unimplemented!("bench::pooling_allowed")
}
