//! Coverage summary records (reference-test-harness.md "Coverage").

/// Contents of `.ilium/coverage/summary.json`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CoverageSummary {
    /// Lines in the plugin source.
    pub lines_total: u64,
    /// Lines the tests ran.
    pub lines_covered: u64,
    /// `lines_covered` over `lines_total`, in percent.
    pub percent: f64,
}

/// Runs the language coverage tool and writes `lcov.info` and `summary.json`; fails with `toolchain.coverage-tool-missing`.
pub fn collect(
    _project_dir: &std::path::Path,
) -> Result<CoverageSummary, crate::errors::HarnessError> {
    unimplemented!("DEV-530")
}

/// Checks `--min-coverage`; the message reads `coverage 71.2 percent is below 80` (DEV-532).
pub fn check_minimum(_summary: &CoverageSummary, _minimum: f64) -> Result<(), String> {
    unimplemented!("DEV-532")
}
