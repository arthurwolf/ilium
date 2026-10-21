//! Conformance suite entry (reference-conformance-suite.md).

use crate::errors::HarnessError;

/// One conformance case result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseResult {
    /// Stable test name.
    pub name: String,
    /// True when the case passed.
    pub passed: bool,
    /// Failure reason, empty for a pass.
    pub reason: String,
}

/// Runs the whole conformance suite against a plugin type in the current mode.
pub fn run_suite<P: ilium_plugin_sdk_core::NativeEntry + Default>(
) -> Result<Vec<CaseResult>, HarnessError> {
    unimplemented!("conformance.suite")
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use ilium_plugin_sdk_core::types::{
        Activation, DeactivationReason, Event, PluginError, Submission,
    };

    /// Server-side plugin the suite runs against. The bodies return neutral values
    /// because the suite, not the plugin, drives the cases.
    #[derive(Default)]
    struct ServerPlugin;

    impl ilium_plugin_sdk_core::NativeEntry for ServerPlugin {
        const SIDE: &'static str = "server";
        const EXPORTS: &'static [&'static str] = &[];
        fn native_activate(&mut self, _context: Activation) -> Result<(), PluginError> {
            Ok(())
        }
        fn native_deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
            None
        }
        fn native_on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
            Vec::new()
        }
    }

    /// Scene-only plugin: it has no server or client side.
    #[derive(Default)]
    struct SceneOnlyPlugin;

    impl ilium_plugin_sdk_core::NativeEntry for SceneOnlyPlugin {
        const SIDE: &'static str = "scene";
        const EXPORTS: &'static [&'static str] = &[];
        fn native_activate(&mut self, _context: Activation) -> Result<(), PluginError> {
            Ok(())
        }
        fn native_deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
            None
        }
        fn native_on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
            Vec::new()
        }
    }

    /// Catalogue areas in catalogue order (reference-conformance-suite.md, "Case catalogue").
    const CATALOGUE_AREAS: [&str; 8] = [
        "LIFE", "EVT", "GRANT", "VIEW", "STORE", "MEM", "RESTART", "PKG",
    ];
    /// Number of cases in the catalogue minimum (DEV-610): 5+10+5+4+3+2+5+5.
    const CATALOGUE_MINIMUM: usize = 39;

    fn area_index(case_name: &str) -> Option<usize> {
        let area = case_name.split('-').nth(1)?;
        CATALOGUE_AREAS.iter().position(|known| *known == area)
    }

    fn is_stable_case_id(case_name: &str) -> bool {
        let parts: Vec<&str> = case_name.split('-').collect();
        parts.len() == 3
            && parts[0] == "CONF"
            && CATALOGUE_AREAS.contains(&parts[1])
            && parts[2].len() == 2
            && parts[2].chars().all(|character| character.is_ascii_digit())
    }

    /// REQ: DEV-610
    #[test]
    fn suite_contains_every_catalogue_case() {
        let results = run_suite::<ServerPlugin>().unwrap();
        assert!(results.len() >= CATALOGUE_MINIMUM);
        for expected_name in ["CONF-LIFE-01", "CONF-EVT-06", "CONF-PKG-05", "CONF-RESTART-05"] {
            assert!(results.iter().any(|result| result.name == expected_name));
        }
    }

    /// REQ: DEV-642
    #[test]
    fn results_follow_catalogue_order() {
        let results = run_suite::<ServerPlugin>().unwrap();
        assert_eq!(results.first().map(|result| result.name.as_str()), Some("CONF-LIFE-01"));
        let indices: Vec<usize> = results
            .iter()
            .map(|result| area_index(&result.name).expect("every case id names a catalogue area"))
            .collect();
        assert!(indices.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    /// REQ: DEV-604
    #[test]
    fn case_ids_are_unique_and_well_formed() {
        let results = run_suite::<ServerPlugin>().unwrap();
        let mut seen_names: Vec<&str> = results.iter().map(|result| result.name.as_str()).collect();
        assert!(seen_names.iter().all(|name| is_stable_case_id(name)));
        seen_names.sort_unstable();
        let count_before_dedup = seen_names.len();
        seen_names.dedup();
        assert_eq!(seen_names.len(), count_before_dedup);
    }

    /// REQ: DEV-605
    #[test]
    fn repeated_runs_produce_equal_results() {
        let first_run = run_suite::<ServerPlugin>().unwrap();
        let second_run = run_suite::<ServerPlugin>().unwrap();
        assert_eq!(first_run, second_run);
    }

    /// REQ: DEV-640
    #[test]
    fn non_passing_cases_always_carry_a_reason() {
        let results = run_suite::<ServerPlugin>().unwrap();
        for result in &results {
            if result.passed {
                assert!(result.reason.is_empty(), "{} passed with a reason", result.name);
            } else {
                assert!(!result.reason.is_empty(), "{} failed or skipped without a reason", result.name);
            }
        }
    }

    /// REQ: DEV-613
    #[test]
    fn scene_only_plugin_skips_submission_cases_as_side_not_built() {
        let results = run_suite::<SceneOnlyPlugin>().unwrap();
        let submission_case = results
            .iter()
            .find(|result| result.name == "CONF-EVT-06")
            .expect("the catalogue contains CONF-EVT-06");
        assert!(!submission_case.passed);
        assert_eq!(submission_case.reason, "side-not-built");
    }

    /// REQ: DEV-613
    #[test]
    fn server_plugin_never_skips_for_missing_side() {
        let results = run_suite::<ServerPlugin>().unwrap();
        assert!(results.iter().all(|result| result.reason != "side-not-built"));
    }
}
