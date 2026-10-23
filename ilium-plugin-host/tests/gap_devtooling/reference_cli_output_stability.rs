//! Compatibility snapshot of CLI output and deprecated flags kept working (DEV-891, DEV-998).
//! Page: reference-cli-output-stability.

use crate::common::*;
use crate::{
    cli_build_with_changed_record_field, cli_with_deprecation_entry, deprecated_flag_invocation,
    env_at_contract_minor_offset, with_code,
};

/// REQ: DEV-891
#[test]
fn changed_json_record_shape_fails_compatibility_check_until_a_deprecation_entry_exists() {
    let root = scratch();
    let changed_env = cli_build_with_changed_record_field(root.path(), "artifact", "extra_field");
    let failed = run(&changed_env, &["compat-check", "--json"]);
    assert_ne!(
        failed.exit_code, 0,
        "a changed record shape must fail the compatibility check"
    );
    assert!(
        !with_code(&failed, "compat.record-changed").is_empty(),
        "the failure names the changed record: {:?}",
        failed.stdout_jsonl
    );
    let with_entry = cli_with_deprecation_entry(&changed_env, "DEV-863");
    let passed = run(&with_entry, &["compat-check", "--json"]);
    assert_eq!(
        passed.exit_code, 0,
        "a matching DEV-863 entry lets the check pass: {:?}",
        passed.stdout_jsonl
    );
}

/// REQ: DEV-998
#[test]
fn deprecated_cli_flag_keeps_working_for_two_minor_releases_of_the_contract() {
    let root = scratch();
    let (invocation, env_at_minor_zero) = deprecated_flag_invocation(root.path());
    let refs: Vec<&str> = invocation.iter().map(String::as_str).collect();
    let first = run(&env_at_minor_zero, &refs);
    assert_eq!(
        first.exit_code, 0,
        "the deprecated flag works now: {:?}",
        first.stdout_jsonl
    );
    assert!(
        !with_code(&first, "deprecated").is_empty(),
        "the flag is reported as deprecated"
    );
    for minor_offset in [1, 2] {
        let later_env = env_at_contract_minor_offset(&env_at_minor_zero, minor_offset);
        let later = run(&later_env, &refs);
        assert_eq!(
            later.exit_code, 0,
            "the deprecated flag still works {minor_offset} minor release(s) later: {:?}",
            later.stdout_jsonl
        );
    }
}
