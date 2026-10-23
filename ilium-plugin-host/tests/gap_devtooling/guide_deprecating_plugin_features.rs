//! Deprecation notices, the deprecation list, deprecation message rules and expiry (DEV-886,
//! DEV-969, DEV-970, DEV-971, DEV-972). Page: guide-deprecating-plugin-features.

use serde_json::{json, Value};

use crate::common::*;
use crate::{
    deprecated_command_env, deprecated_fixture_env, deprecated_item_env, deprecated_item_session,
    package_declaring_expired_item, rows_with, with_code,
};

/// The records of a run that are not deprecation notices.
fn non_notice_records(output: &ilium_plugin_host::cli::CliOutput) -> Vec<Value> {
    output
        .stdout_jsonl
        .iter()
        .filter(|record| record["code"] != "deprecated")
        .cloned()
        .collect()
}

/// REQ: DEV-886
#[test]
fn deprecated_command_shows_notice_once_per_session_and_keeps_its_results() {
    let root = scratch();
    let env = deprecated_command_env(
        root.path(),
        "legacy-list",
        "Use `ilium plugin list` instead.",
    );
    let first = run(&env, &["legacy-list", "--json"]);
    let second = run(&env, &["legacy-list", "--json"]);
    assert_eq!(
        first.exit_code, second.exit_code,
        "deprecation must not change the exit code"
    );
    let first_notices = with_code(&first, "deprecated");
    assert_eq!(
        first_notices.len(),
        1,
        "the message is shown at first use in the session"
    );
    assert_eq!(
        first_notices[0]["message"],
        "Use `ilium plugin list` instead."
    );
    assert!(
        with_code(&second, "deprecated").is_empty(),
        "the notice is not repeated in the same session"
    );
    assert_eq!(
        non_notice_records(&first),
        non_notice_records(&second),
        "deprecation does not change the results"
    );
}

/// REQ: DEV-969
#[test]
fn deprecations_list_names_each_deprecated_item_kind_and_exits_zero_when_empty() {
    let root = scratch();
    let env = deprecated_fixture_env(root.path());
    let listed = run(&env, &["deprecations", "--json"]);
    assert_eq!(listed.exit_code, 0, "{:?}", listed.stdout_jsonl);
    let rows = rows_with(&listed, "kind");
    for kind in ["host-feature", "sdk-item", "plugin-item"] {
        let row = rows
            .iter()
            .find(|row| row["kind"] == kind)
            .unwrap_or_else(|| panic!("a row of kind {kind}"));
        for field in ["name", "since", "remove_in", "owner"] {
            assert!(row.get(field).is_some(), "{kind} row has {field}: {row}");
        }
    }
    let empty_root = scratch();
    let empty_env = crate::common::env(empty_root.path());
    let none = run(&empty_env, &["deprecations", "--json"]);
    assert_eq!(none.exit_code, 0, "an empty list exits 0");
    assert!(
        rows_with(&none, "kind").is_empty(),
        "no rows when nothing is deprecated"
    );
}

/// REQ: DEV-970
#[test]
fn deprecated_message_over_280_bytes_or_with_control_character_is_refused() {
    let long_message = "a".repeat(281);
    for message in [long_message.as_str(), "bad\u{0007}message"] {
        let (_root, env) = lint_env_with(|manifest| {
            manifest["deprecated"] =
                json!({ "message": message, "since": "1.0.0", "remove_in": "2.0.0" });
        });
        let out = run_in_project(&env, "check", &[]);
        assert_ne!(
            out.exit_code, 0,
            "an invalid deprecated message must fail: {message:?}"
        );
        assert!(
            !with_code(&out, "manifest.deprecated-invalid").is_empty(),
            "refused with manifest.deprecated-invalid: {:?}",
            out.stdout_jsonl
        );
    }
}

/// REQ: DEV-971
#[test]
fn deprecated_item_warn_lines_are_limited_to_one_per_minute_with_suppressed_count() {
    let root = scratch();
    let env = deprecated_item_env(root.path());
    let session = deprecated_item_session(&env);
    for _ in 0..100 {
        session.call_deprecated_item();
    }
    let first_minute = session.warn_log_lines();
    assert_eq!(
        first_minute.len(),
        1,
        "one warn line within the first minute"
    );
    session.advance_clock_ms(60_000);
    session.call_deprecated_item();
    let next_minute = session.warn_log_lines();
    assert_eq!(next_minute.len(), 2, "one more warn line after a minute");
    assert_eq!(
        next_minute[1]["suppressed"], 99,
        "the next line counts the 99 suppressed lines"
    );
}

/// REQ: DEV-972
#[test]
fn package_still_declaring_item_at_remove_in_version_is_refused_naming_the_item() {
    let root = scratch();
    let env = env(root.path());
    let package_path = package_declaring_expired_item(&env, "legacy-tool", "2.0.0");
    let out = run(&env, &["install", package_path.as_str(), "--json"]);
    assert_ne!(out.exit_code, 0, "a package past remove_in must not load");
    assert!(
        !with_code(&out, "manifest.deprecated-expired").is_empty(),
        "refused with manifest.deprecated-expired: {:?}",
        out.stdout_jsonl
    );
    assert!(
        out.stdout_raw.contains("legacy-tool"),
        "the refusal names the item"
    );
}
