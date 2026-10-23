//! Rule statement AUT-950: the conformance suite runs on the release build before `pack`, and a
//! release has no `fail` verdict. The page and the requirement table are checked for the order and the report.
use crate::support::assert_rule;

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
    unimplemented!("ilium-plugin-testkit::conformance::run_suite")
}
