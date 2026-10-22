//! Deprecation rules of `manifest.json` checked by `ilium plugin check` (DEV-885, DEV-887). A
//! deprecated item names `since`, `remove_in` and `message`.

use ilium_plugin_host::cli::CliOutput;
use serde_json::{json, Value};

use crate::common::*;

/// Writes a `deprecated` object with the given versions into a manifest.
fn set_deprecation(manifest: &mut Value, since: &str, remove_in: &str) {
    manifest["deprecated"] = json!({
        "since": since,
        "remove_in": remove_in,
        "message": "Use the replacement command."
    });
}

/// True when the output text, the error output or any record mentions `text`.
fn mentions(output: &CliOutput, text: &str) -> bool {
    output.stdout_text.contains(text)
        || output.stderr.contains(text)
        || output.stdout_jsonl.iter().any(|record| record.to_string().contains(text))
}

/// REQ: DEV-885
#[test]
fn check_rejects_deprecation_window_of_one_minor_version() {
    let root = scratch();
    let env = env(root.path());
    let dir = project_dir(&env);
    project_with(&dir, |manifest| set_deprecation(manifest, "0.1.0", "0.2.0"));
    let output = run_in_project(&env, "check", &[]);
    assert!(
        mentions(&output, "lint.deprecation-too-short"),
        "one minor version between since and remove_in is too short"
    );
}

/// REQ: DEV-885
#[test]
fn check_accepts_deprecation_window_of_two_minor_versions() {
    let root = scratch();
    let env = env(root.path());
    let dir = project_dir(&env);
    project_with(&dir, |manifest| set_deprecation(manifest, "0.1.0", "0.3.0"));
    let output = run_in_project(&env, "check", &[]);
    assert!(
        !mentions(&output, "lint.deprecation-too-short"),
        "two minor versions between since and remove_in is allowed"
    );
}

/// REQ: DEV-887
#[test]
fn check_reports_error_when_removed_item_is_still_declared() {
    let root = scratch();
    let env = env(root.path());
    let dir = project_dir(&env);
    project_with(&dir, |manifest| {
        manifest["version"] = json!("0.3.0");
        set_deprecation(manifest, "0.1.0", "0.3.0");
    });
    let output = run_in_project(&env, "check", &[]);
    let finding = output
        .stdout_jsonl
        .iter()
        .find(|record| record.to_string().contains("lint.deprecated-removed"))
        .expect("a lint.deprecated-removed finding");
    assert_eq!(
        finding.get("severity").and_then(Value::as_str),
        Some("error"),
        "a removed item that is still declared is an error"
    );
}
