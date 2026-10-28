//! Rule statement AUT-950: the conformance suite runs on the release build before `pack`, and a
//! release has no `fail` verdict. The page and the requirement table are checked for the order and the report.
use crate::support::{assert_rule, InOrder};
use ilium_plugin_sdk::testing::conformance::run_suite;
use ilium_plugin_sdk::types::{Activation, DeactivationReason, Event, PluginError, Submission};
use ilium_plugin_sdk::NativeEntry;

/// Reasons of a case the native run does not take part in; they are not `fail` verdicts.
const NOT_RUN_REASONS: [&str; 3] = ["subject-not-plugin", "needs-component-mode", "side-not-built"];

/// The cases that ended in a `fail` verdict: they ran and did not pass.
fn fail_verdicts(
    results: &[ilium_plugin_sdk::testing::conformance::CaseResult],
) -> Vec<String> {
    results
        .iter()
        .filter(|case| !case.passed && !NOT_RUN_REASONS.contains(&case.reason.as_str()))
        .map(|case| format!("{}: {}", case.name, case.reason))
        .collect()
}

/// A plugin whose activation always fails, so the suite must report a `fail` verdict for it.
#[derive(Default)]
struct RefusesToActivate;

impl NativeEntry for RefusesToActivate {
    const SIDE: &'static str = "server";
    const EXPORTS: &'static [&'static str] = &["lifecycle", "handler"];
    fn native_activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Err(PluginError::new(
            ilium_plugin_sdk::types::ErrorCode::Internal,
            "refuses to activate",
        ))
    }
    fn native_deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        None
    }
    fn native_on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

/// REQ: AUT-950
#[test]
fn aut_950_conformance_runs_on_release_build_before_pack_and_report_is_kept() {
    assert_rule(
        "AUT-950",
        "MUST",
        &[
            "after `build --release` and before `pack`",
            "The conformance report MUST be kept with the release",
        ],
    );
}

/// REQ: AUT-950
#[test]
fn aut_950_release_run_has_no_fail_verdict() {
    let results = run_suite::<InOrder>().expect("the suite runs");
    assert!(!results.is_empty(), "the suite must report cases");
    assert!(
        results.iter().any(|case| case.passed),
        "at least the lifecycle cases run natively"
    );
    assert_eq!(fail_verdicts(&results), Vec::<String>::new());
    // The same check must see a fail verdict when a plugin really breaks a case.
    let broken = run_suite::<RefusesToActivate>().expect("the suite runs");
    assert!(
        !fail_verdicts(&broken).is_empty(),
        "a plugin that refuses to activate must produce a fail verdict"
    );
}
