//! Benchmark and budget support (test-plan.md section 2.8, reference-host-budgets.md).
//!
//! Skeleton: the `perf-budgets` test lane fills in the programs and the budget tests.

use std::collections::BTreeMap;

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
        unimplemented!("bench::BenchResult::to_json")
    }
}

/// Runs the program `id` with an explicit spec (reference-host-budgets.md HST-300).
pub fn run_with(_id: &str, _spec: &RunSpec) -> Result<BenchResult, BenchError> {
    unimplemented!("bench::run_with")
}

/// Runs the program `id` with the default spec (reference-host-budgets.md HST-300, HST-307).
pub fn run_benchmark(_id: &str) -> Result<BenchResult, BenchError> {
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
pub fn judge(_id: &str, _metric: &str, _value: f64) -> Verdict {
    unimplemented!("bench::judge")
}

/// The process exit code for a set of verdicts: non-zero on any `Fail` or `PlanStop` (HST-302).
pub fn exit_code_for(_verdicts: &[Verdict]) -> i32 {
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
pub fn pooling_allowed(_evidence: &PoolingEvidence) -> bool {
    unimplemented!("bench::pooling_allowed")
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
            wasmtime_config: ilium_plugin_host::engine::runtime_config(),
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
