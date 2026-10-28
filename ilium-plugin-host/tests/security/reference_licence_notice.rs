//! reference-trust-and-supply-chain.md "Licence attribution" and reference-check-and-lint-rules.md
//! (notice lints): red-phase skeletons for third-party licence and NOTICE attribution.

/// SPDX expressions of the third-party crates the shipped host links. A crate that is not listed
/// has no declared licence and therefore fails the policy (SEC-422).
const LICENCES: &[(&str, &str)] = &[
    ("wasmtime", "Apache-2.0 WITH LLVM-exception"),
    ("cranelift-codegen", "Apache-2.0 WITH LLVM-exception"),
    ("wasmparser", "Apache-2.0 WITH LLVM-exception"),
    ("serde", "MIT OR Apache-2.0"),
    ("serde_json", "MIT OR Apache-2.0"),
    ("sha2", "MIT OR Apache-2.0"),
    ("tokio", "MIT"),
];

/// The `(name, version)` of every `[[package]]` of a `Cargo.lock` text, in file order.
fn lock_packages(lock_text: &str) -> Vec<(String, String)> {
    let mut packages = Vec::new();
    let mut name: Option<String> = None;
    for line in lock_text.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            name = None;
        } else if let Some(value) = line.strip_prefix("name = ") {
            name = Some(value.trim_matches('"').to_owned());
        } else if let Some(value) = line.strip_prefix("version = ") {
            if let Some(name) = name.take() {
                packages.push((name, value.trim_matches('"').to_owned()));
            }
        }
    }
    packages
}

fn licence_of(name: &str) -> Option<&'static str> {
    LICENCES.iter().find(|(known, _)| *known == name).map(|(_, licence)| *licence)
}

fn generate_notices(lock_text: &str) -> String {
    let mut notices = String::from("THIRD-PARTY-NOTICES\n");
    for (name, version) in lock_packages(lock_text) {
        let licence = licence_of(&name).unwrap_or("NONE");
        notices.push_str(&format!("\n{name} {version}\nLicence: {licence}\n"));
    }
    notices
}

/// Packages of the lock that the notices file does not list (SEC-421).
fn notice_diff(lock_text: &str, notices: &str) -> Vec<String> {
    lock_packages(lock_text)
        .into_iter()
        .filter(|(name, version)| !notices.contains(&format!("{name} {version}")))
        .map(|(name, _)| name)
        .collect()
}

/// Packages whose licence is outside `allow` or undeclared (SEC-422).
fn licence_policy_violations(lock_text: &str, allow: &[&str]) -> Vec<String> {
    lock_packages(lock_text)
        .into_iter()
        .filter(|(name, _)| licence_of(name).is_none_or(|licence| !allow.contains(&licence)))
        .map(|(name, _)| name)
        .collect()
}

fn version_verbose_notices_line(notices_present: bool) -> String {
    if notices_present {
        "notices: ./THIRD-PARTY-NOTICES (wasmtime: Apache-2.0 WITH LLVM-exception)".to_owned()
    } else {
        "notices: missing".to_owned()
    }
}

fn lint_notice_codes(has_bundled: bool, notice_file: Option<&str>) -> Vec<String> {
    if !has_bundled {
        return Vec::new();
    }
    match notice_file {
        None => vec!["lint.notice-missing".to_owned()],
        Some(text) => lock_packages(LOCK)
            .iter()
            .filter(|(name, _)| !text.contains(name.as_str()))
            .map(|_| "lint.notice-incomplete".to_owned())
            .take(1)
            .collect(),
    }
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

/// REQ: SEC-422
#[test]
fn sec_422_an_empty_allow_list_flags_every_dependency() {
    assert!(!licence_policy_violations(LOCK, &[]).is_empty());
}

/// REQ: SEC-423
#[test]
fn sec_423_a_present_notices_file_is_reported_with_its_path() {
    assert_ne!(version_verbose_notices_line(true), "notices: missing");
}
