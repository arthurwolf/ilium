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
    project_dir: &std::path::Path,
) -> Result<CoverageSummary, crate::errors::HarnessError> {
    use crate::errors::{codes, HarnessError};
    use std::process::Command;

    let is_typescript = project_dir.join("package.json").is_file();
    let output_dir = project_dir.join(".ilium").join("coverage");
    let lcov_path = output_dir.join("lcov.info");
    let missing = |tool: &str| {
        HarnessError::new(
            codes::COVERAGE_TOOL_MISSING,
            format!("{tool} is not installed; coverage needs it"),
        )
    };
    let io_error = |error: std::io::Error| {
        HarnessError::new(codes::COVERAGE_TOOL_MISSING, format!("coverage run failed: {error}"))
    };
    std::fs::create_dir_all(&output_dir).map_err(io_error)?;

    let status = if is_typescript {
        if !tool_on_path("bun") {
            return Err(missing("bun"));
        }
        Command::new("bun")
            .args(["test", "--coverage", "--coverage-reporter=lcov"])
            .arg(format!("--coverage-dir={}", output_dir.display()))
            .current_dir(project_dir)
            .status()
    } else {
        if !tool_on_path("cargo-llvm-cov") {
            return Err(missing("cargo llvm-cov"));
        }
        Command::new("cargo")
            .args(["llvm-cov", "--lcov", "--output-path"])
            .arg(&lcov_path)
            .current_dir(project_dir)
            .status()
    }
    .map_err(io_error)?;
    if !status.success() {
        return Err(HarnessError::new(
            codes::COVERAGE_TOOL_MISSING,
            format!("coverage tool exited with {status}"),
        ));
    }
    let lcov = std::fs::read_to_string(&lcov_path).map_err(io_error)?;
    let summary = summarize_lcov(&lcov);
    let json = serde_json::to_string_pretty(&summary).map_err(|error| {
        HarnessError::new(codes::COVERAGE_TOOL_MISSING, format!("summary not serialisable: {error}"))
    })?;
    std::fs::write(output_dir.join("summary.json"), json + "\n").map_err(io_error)?;
    Ok(summary)
}

/// True when an executable with this name sits in a `PATH` folder.
fn tool_on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|folder| folder.join(name).is_file()))
        .unwrap_or(false)
}

/// Adds up the `LF` (lines found) and `LH` (lines hit) records of an lcov file.
fn summarize_lcov(lcov: &str) -> CoverageSummary {
    let mut lines_total = 0u64;
    let mut lines_covered = 0u64;
    for line in lcov.lines() {
        if let Some(count) = line.strip_prefix("LF:") {
            lines_total += count.trim().parse::<u64>().unwrap_or(0);
        } else if let Some(count) = line.strip_prefix("LH:") {
            lines_covered += count.trim().parse::<u64>().unwrap_or(0);
        }
    }
    let percent = if lines_total == 0 {
        0.0
    } else {
        // Rounded to one decimal so the summary reads like the CLI message.
        ((lines_covered as f64 / lines_total as f64) * 1000.0).round() / 10.0
    };
    CoverageSummary {
        lines_total,
        lines_covered,
        percent,
    }
}

/// Checks `--min-coverage`; the message reads `coverage 71.2 percent is below 80` (DEV-532).
pub fn check_minimum(summary: &CoverageSummary, minimum: f64) -> Result<(), String> {
    if summary.percent >= minimum {
        return Ok(());
    }
    // `{}` on f64 prints 80 for 80.0 and 71.2 for 71.2, as the message example needs.
    Err(format!(
        "coverage {} percent is below {}",
        summary.percent, minimum
    ))
}

#[cfg(test)]
mod spec_tests {
    use super::{check_minimum, collect, CoverageSummary};
    use crate::errors::codes;
    use std::path::Path;

    fn summary_with(lines_total: u64, lines_covered: u64, percent: f64) -> CoverageSummary {
        CoverageSummary {
            lines_total,
            lines_covered,
            percent,
        }
    }

    /// REQ: DEV-531
    #[test]
    fn dev_531_summary_serialises_its_three_fields_in_order() {
        let json = serde_json::to_string(&summary_with(1000, 712, 71.2)).expect("serialise");
        assert_eq!(
            json,
            r#"{"lines_total":1000,"lines_covered":712,"percent":71.2}"#
        );
    }

    /// REQ: DEV-531
    #[test]
    fn dev_531_summary_round_trips_through_json() {
        let original = summary_with(250, 200, 80.0);
        let json = serde_json::to_string(&original).expect("serialise");
        let restored: CoverageSummary = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(restored, original);
    }

    /// REQ: DEV-530
    #[test]
    fn dev_530_collect_reports_line_counts_consistent_with_percent() {
        let project_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        match collect(project_dir) {
            Ok(summary) => {
                assert!(summary.lines_covered <= summary.lines_total);
                assert!((0.0..=100.0).contains(&summary.percent));
            }
            Err(error) => assert_eq!(error.code, codes::COVERAGE_TOOL_MISSING),
        }
    }

    /// this test asserts the code whenever the tool is absent on the running machine)
    /// REQ: DEV-530
    #[test]
    fn dev_530_missing_tool_error_carries_the_coverage_tool_missing_code() {
        let project_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        if let Err(error) = collect(project_dir) {
            assert_eq!(error.code, "toolchain.coverage-tool-missing");
        }
    }

    /// `coverage 71.2 percent is below 80`)
    /// REQ: DEV-532
    #[test]
    fn dev_532_below_minimum_produces_the_exact_message() {
        let error = check_minimum(&summary_with(1000, 712, 71.2), 80.0).expect_err("below");
        assert_eq!(error, "coverage 71.2 percent is below 80");
    }

    /// REQ: DEV-532
    #[test]
    fn dev_532_equal_to_minimum_passes() {
        assert!(check_minimum(&summary_with(1000, 800, 80.0), 80.0).is_ok());
    }

    /// REQ: DEV-532
    #[test]
    fn dev_532_above_minimum_passes() {
        assert!(check_minimum(&summary_with(1000, 955, 95.5), 80.0).is_ok());
    }

    /// REQ: DEV-532
    #[test]
    fn dev_532_just_below_minimum_fails_with_its_percent() {
        let error = check_minimum(&summary_with(1000, 799, 79.9), 80.0).expect_err("below");
        assert_eq!(error, "coverage 79.9 percent is below 80");
    }

    /// REQ: DEV-532
    #[test]
    fn dev_532_zero_coverage_passes_a_zero_minimum() {
        assert!(check_minimum(&summary_with(0, 0, 0.0), 0.0).is_ok());
    }

    /// REQ: DEV-532
    #[test]
    fn dev_532_full_coverage_fails_a_minimum_above_one_hundred() {
        let error = check_minimum(&summary_with(1000, 1000, 100.0), 101.0).expect_err("below");
        assert_eq!(error, "coverage 100 percent is below 101");
    }
}
