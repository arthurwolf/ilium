//! Conformance suite entry (reference-conformance-suite.md).

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::{Duration, Instant};

use ilium_plugin_sdk_core::types::DeactivationReason;

use crate::errors::HarnessError;
use crate::host::FakeHost;

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

/// Reason of a case that needs the subject to be the SDK or the host, not a plugin.
const REASON_OTHER_SUBJECT: &str = "subject-not-plugin";
/// Reason of a case that needs a compiled component or a deadline probe.
const REASON_NEEDS_COMPONENT: &str = "needs-component-mode";
/// Reason of a case whose side the plugin did not build.
const REASON_SIDE_NOT_BUILT: &str = "side-not-built";

/// The catalogue in order: area, number of cases.
const CATALOGUE: &[(&str, u8)] = &[
    ("LIFE", 5),
    ("EVT", 10),
    ("GRANT", 5),
    ("VIEW", 4),
    ("STORE", 3),
    ("MEM", 2),
    ("RESTART", 5),
    ("PKG", 5),
];

/// Cases that only run with the plugin as subject and need a built package or component.
const PACKAGE_CASES: &[&str] = &["CONF-PKG-01", "CONF-PKG-02"];

/// Cases whose stimulus is a deadline probe or compiled component, even for a plugin subject.
const COMPONENT_CASES: &[&str] = &["CONF-LIFE-04", "CONF-EVT-02", "CONF-VIEW-01"];

/// Cases of the submission area that a scene-only plugin cannot take part in.
fn is_submission_case(name: &str) -> bool {
    ["06", "07", "08", "09", "10"]
        .iter()
        .any(|number| name == format!("CONF-EVT-{number}"))
}

fn pass() -> Result<(), String> {
    Ok(())
}

/// CONF-LIFE-01: activate runs once, then events can be delivered.
fn life_01<P: ilium_plugin_sdk_core::NativeEntry + Default + 'static>() -> Result<(), String> {
    let mut host = FakeHost::new();
    let handle = host.activate::<P>();
    if handle.instance != 1 {
        return Err(format!("first instance was {}", handle.instance));
    }
    host.deliver(Vec::new());
    pass()
}

/// CONF-LIFE-02: activate stays under the default deadline of two seconds.
fn life_02<P: ilium_plugin_sdk_core::NativeEntry + Default + 'static>() -> Result<(), String> {
    let mut host = FakeHost::new();
    let started = Instant::now();
    host.activate::<P>();
    if started.elapsed() > Duration::from_millis(2000) {
        return Err("activate took longer than 2000 ms".to_string());
    }
    pass()
}

/// CONF-LIFE-05: every delivered deactivate reason reaches the plugin without a failure.
fn life_05<P: ilium_plugin_sdk_core::NativeEntry + Default + 'static>() -> Result<(), String> {
    for reason in [
        DeactivationReason::Shutdown,
        DeactivationReason::Disabled,
        DeactivationReason::Upgrade,
        DeactivationReason::Revoked,
        DeactivationReason::Granted,
    ] {
        let mut host = FakeHost::new();
        host.activate::<P>();
        host.deactivate(reason);
    }
    pass()
}

/// CONF-VIEW-02: rendering twice gives equal screens.
fn view_02<P: ilium_plugin_sdk_core::NativeEntry + Default + 'static>() -> Result<(), String> {
    let mut host = FakeHost::new();
    host.activate::<P>();
    let target = ilium_plugin_sdk_core::types::ViewTarget::Panel("main".to_string());
    let first = host.render(target.clone(), 80, 24);
    let second = host.render(target, 80, 24);
    if first != second {
        return Err("two renders gave different screens".to_string());
    }
    pass()
}

/// Runs one real check, turning a harness panic into a failure reason.
fn guarded(check: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    match catch_unwind(AssertUnwindSafe(check)) {
        Ok(result) => result,
        Err(payload) => {
            let text = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|text| (*text).to_string()))
                .unwrap_or_else(|| "the check panicked".to_string());
            Err(text)
        }
    }
}

/// Runs the whole conformance suite against a plugin type in the current mode.
pub fn run_suite<P: ilium_plugin_sdk_core::NativeEntry + Default + 'static>(
) -> Result<Vec<CaseResult>, HarnessError> {
    let mut results = Vec::new();
    for (area, count) in CATALOGUE {
        for number in 1..=*count {
            let name = format!("CONF-{area}-{number:02}");
            let outcome: Result<(), String> = if P::SIDE == "scene" && is_submission_case(&name) {
                Err(REASON_SIDE_NOT_BUILT.to_string())
            } else {
                match name.as_str() {
                    "CONF-LIFE-01" => guarded(life_01::<P>),
                    "CONF-LIFE-02" => guarded(life_02::<P>),
                    "CONF-LIFE-05" => guarded(life_05::<P>),
                    "CONF-VIEW-02" => guarded(view_02::<P>),
                    other if PACKAGE_CASES.contains(&other) || COMPONENT_CASES.contains(&other) => {
                        Err(REASON_NEEDS_COMPONENT.to_string())
                    }
                    _ => Err(REASON_OTHER_SUBJECT.to_string()),
                }
            };
            results.push(match outcome {
                Ok(()) => CaseResult { name, passed: true, reason: String::new() },
                Err(reason) => CaseResult { name, passed: false, reason },
            });
        }
    }
    Ok(results)
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
        for expected_name in [
            "CONF-LIFE-01",
            "CONF-EVT-06",
            "CONF-PKG-05",
            "CONF-RESTART-05",
        ] {
            assert!(results.iter().any(|result| result.name == expected_name));
        }
    }

    /// REQ: DEV-642
    #[test]
    fn results_follow_catalogue_order() {
        let results = run_suite::<ServerPlugin>().unwrap();
        assert_eq!(
            results.first().map(|result| result.name.as_str()),
            Some("CONF-LIFE-01")
        );
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
                assert!(
                    result.reason.is_empty(),
                    "{} passed with a reason",
                    result.name
                );
            } else {
                assert!(
                    !result.reason.is_empty(),
                    "{} failed or skipped without a reason",
                    result.name
                );
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
        assert!(results
            .iter()
            .all(|result| result.reason != "side-not-built"));
    }
}
