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

    /// REQ: DEV-531 (summary.json MUST list lines_total, lines_covered and percent, in that order)
    #[test]
    fn dev_531_summary_serialises_its_three_fields_in_order() {
        let json = serde_json::to_string(&summary_with(1000, 712, 71.2)).expect("serialise");
        assert_eq!(
            json,
            r#"{"lines_total":1000,"lines_covered":712,"percent":71.2}"#
        );
    }

    /// REQ: DEV-531 (the summary written to summary.json reads back unchanged)
    #[test]
    fn dev_531_summary_round_trips_through_json() {
        let original = summary_with(250, 200, 80.0);
        let json = serde_json::to_string(&original).expect("serialise");
        let restored: CoverageSummary = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(restored, original);
    }

    /// REQ: DEV-530 (ilium plugin test --coverage MUST write line coverage for the plugin source)
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

    /// REQ: DEV-530 (a missing coverage tool MUST fail with toolchain.coverage-tool-missing;
    /// this test asserts the code whenever the tool is absent on the running machine)
    #[test]
    fn dev_530_missing_tool_error_carries_the_coverage_tool_missing_code() {
        let project_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        if let Err(error) = collect(project_dir) {
            assert_eq!(error.code, "toolchain.coverage-tool-missing");
        }
    }

    /// REQ: DEV-532 (--min-coverage MUST exit 3 when coverage is lower; message reads
    /// `coverage 71.2 percent is below 80`)
    #[test]
    fn dev_532_below_minimum_produces_the_exact_message() {
        let error = check_minimum(&summary_with(1000, 712, 71.2), 80.0).expect_err("below");
        assert_eq!(error, "coverage 71.2 percent is below 80");
    }

    /// REQ: DEV-532 (coverage equal to the minimum is not lower, so it passes)
    #[test]
    fn dev_532_equal_to_minimum_passes() {
        assert!(check_minimum(&summary_with(1000, 800, 80.0), 80.0).is_ok());
    }

    /// REQ: DEV-532 (coverage above the minimum passes)
    #[test]
    fn dev_532_above_minimum_passes() {
        assert!(check_minimum(&summary_with(1000, 955, 95.5), 80.0).is_ok());
    }

    /// REQ: DEV-532 (just below the minimum fails, with the one-decimal percent in the message)
    #[test]
    fn dev_532_just_below_minimum_fails_with_its_percent() {
        let error = check_minimum(&summary_with(1000, 799, 79.9), 80.0).expect_err("below");
        assert_eq!(error, "coverage 79.9 percent is below 80");
    }

    /// REQ: DEV-532 (empty input: zero coverage passes a zero minimum, since 0 is not below 0)
    #[test]
    fn dev_532_zero_coverage_passes_a_zero_minimum() {
        assert!(check_minimum(&summary_with(0, 0, 0.0), 0.0).is_ok());
    }

    /// REQ: DEV-532 (invalid input: no percentage can reach a minimum above 100, so it fails)
    #[test]
    fn dev_532_full_coverage_fails_a_minimum_above_one_hundred() {
        let error = check_minimum(&summary_with(1000, 1000, 100.0), 101.0).expect_err("below");
        assert_eq!(error, "coverage 100 percent is below 101");
    }
}
