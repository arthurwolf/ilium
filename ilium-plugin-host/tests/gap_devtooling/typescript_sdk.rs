//! TypeScript SDK parity with the Rust SDK and bundle size (DEV-927, DEV-928). Pages:
//! typescript-sdk, rust-sdk.

use std::collections::BTreeSet;
use std::time::Duration;

use crate::common::*;
use crate::{
    add_oversize_bundle_payload, cold_instantiate_ms, manual_page, rows_with, utf8, with_code,
};

/// The SDK function names a reference page lists: backtick-quoted names ending in `()`.
fn sdk_function_names(page: &str) -> BTreeSet<String> {
    page.split('`')
        .skip(1)
        .step_by(2)
        .filter(|token| token.ends_with("()"))
        .map(|token| token.trim_end_matches("()").to_owned())
        .collect()
}

/// REQ: DEV-927
#[test]
fn rust_and_typescript_sdk_pages_list_the_same_function_names_with_a_parity_case_each() {
    let rust_names = sdk_function_names(&manual_page("rust-sdk"));
    let typescript_names = sdk_function_names(&manual_page("typescript-sdk"));
    assert!(
        !rust_names.is_empty(),
        "the Rust SDK page lists its functions"
    );
    assert_eq!(
        rust_names, typescript_names,
        "both SDK pages list the same function names"
    );

    let root = scratch();
    let cli_env = env(root.path());
    let listed = run(&cli_env, &["conformance", "--list", "--json"]);
    assert_eq!(listed.exit_code, 0, "{:?}", listed.stdout_jsonl);
    let case_names: Vec<String> = rows_with(&listed, "case")
        .iter()
        .map(|row| row["case"].as_str().expect("case name").to_owned())
        .collect();
    for name in &rust_names {
        assert!(
            case_names.iter().any(|case| case.contains(name.as_str())),
            "the conformance suite has a parity case for {name}"
        );
    }
}

/// REQ: DEV-928
#[test]
fn minimal_typescript_bundle_is_at_most_1_mib_and_cold_instantiation_within_150_ms() {
    let root = scratch();
    let cli_env = env(root.path());
    let project = project_dir(&cli_env);
    let made = run(
        &cli_env,
        &[
            "new",
            utf8(&project),
            "--id",
            "example.agent-tools",
            "--lang",
            "typescript",
            "--template",
            "server",
            "--json",
        ],
    );
    assert_eq!(made.exit_code, 0, "{:?}", made.stdout_jsonl);
    let built = run_in_project(&cli_env, "build", &[]);
    assert_eq!(built.exit_code, 0, "{:?}", built.stdout_jsonl);
    let bundle_name = tree_of(&project.join("dist"))
        .into_iter()
        .find(|path| path.ends_with(".js"))
        .expect("the TypeScript build writes a JavaScript bundle");
    let bundle = project.join("dist").join(&bundle_name);
    let size = std::fs::metadata(&bundle).expect("bundle metadata").len();
    assert!(
        size <= 1024 * 1024,
        "the minimal bundle is at most 1 MiB: {size} bytes"
    );
    let cold = cold_instantiate_ms(&bundle);
    assert!(
        cold <= Duration::from_millis(150),
        "cold instantiation takes {cold:?}"
    );
}

/// REQ: DEV-928
#[test]
fn build_warns_build_bundle_large_above_5_mib() {
    let root = scratch();
    let cli_env = env(root.path());
    let project = project_dir(&cli_env);
    let made = run(
        &cli_env,
        &[
            "new",
            utf8(&project),
            "--id",
            "example.agent-tools",
            "--lang",
            "typescript",
            "--template",
            "server",
            "--json",
        ],
    );
    assert_eq!(made.exit_code, 0, "{:?}", made.stdout_jsonl);
    add_oversize_bundle_payload(&project, 6);
    let built = run_in_project(&cli_env, "build", &[]);
    let warnings = with_code(&built, "build.bundle-large");
    assert!(
        !warnings.is_empty(),
        "a bundle over 5 MiB prints build.bundle-large: {:?}",
        built.stdout_jsonl
    );
    assert_eq!(warnings[0]["severity"], "warn");
}

/// REQ: DEV-927
#[test]
fn conformance_suite_has_no_failing_parity_case_for_either_sdk() {
    let root = scratch();
    let cli_env = env(root.path());
    let suite = run(&cli_env, &["conformance", "--json"]);
    assert_eq!(suite.exit_code, 0, "{:?}", suite.stdout_jsonl);
    let cases = rows_with(&suite, "case");
    assert!(!cases.is_empty(), "the suite runs its parity cases");
    let failed = cases
        .iter()
        .filter(|row| row["status"] == "fail")
        .count();
    assert_eq!(failed, 0, "{:?}", suite.stdout_jsonl);
}
