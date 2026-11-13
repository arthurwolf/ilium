//! reference-trust-and-supply-chain.md "Licence attribution" and reference-check-and-lint-rules.md
//! (notice lints): red-phase skeletons for third-party licence and NOTICE attribution.

fn generate_notices(_lock_text: &str) -> String {
    unimplemented!("THIRD-PARTY-NOTICES generator")
}

fn notice_diff(_lock_text: &str, _notices: &str) -> Vec<String> {
    unimplemented!("notice completeness check")
}

fn licence_policy_violations(_lock_text: &str, _allow: &[&str]) -> Vec<String> {
    unimplemented!("licence allow-list check")
}

fn version_verbose_notices_line(_notices_present: bool) -> String {
    unimplemented!("ilium --version --verbose notices line")
}

fn lint_notice_codes(_has_bundled: bool, _notice_file: Option<&str>) -> Vec<String> {
    unimplemented!("lint.notice-missing and lint.notice-incomplete")
}

const LOCK: &str = "[[package]]\nname = \"wasmtime\"\nversion = \"1.0.0\"\n";

/// REQ: SEC-420
#[test]
fn sec_420_notices_list_wasmtime_with_version_and_licence() {
    let notices = generate_notices(LOCK);
    assert!(notices.contains("wasmtime"));
    assert!(notices.contains("1.0.0"));
    assert!(notices.contains("Apache-2.0 WITH LLVM-exception"));
}

/// REQ: SEC-421
#[test]
fn sec_421_a_crate_missing_from_the_notices_is_reported() {
    assert_eq!(notice_diff(LOCK, ""), vec!["wasmtime".to_string()]);
}

/// REQ: SEC-422
#[test]
fn sec_422_a_licence_outside_the_allow_list_fails() {
    assert_eq!(
        licence_policy_violations(LOCK, &["MIT"]),
        vec!["wasmtime".to_string()]
    );
}

/// REQ: SEC-423
#[test]
fn sec_423_version_verbose_reports_missing_notices() {
    assert_eq!(version_verbose_notices_line(false), "notices: missing");
}

/// REQ: SEC-420
#[test]
fn sec_420_lint_flags_bundled_code_without_a_notice_file() {
    assert_eq!(
        lint_notice_codes(true, None),
        vec!["lint.notice-missing".to_string()]
    );
}

/// REQ: SEC-421
#[test]
fn sec_421_lint_flags_a_bundled_dependency_absent_from_the_notice_file() {
    assert_eq!(
        lint_notice_codes(true, Some("other")),
        vec!["lint.notice-incomplete".to_string()]
    );
}
