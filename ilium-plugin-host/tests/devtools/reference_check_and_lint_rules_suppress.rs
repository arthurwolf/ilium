//! reference-check-and-lint-rules.md "Severity, deny and exit codes" and "Suppression"
//! (DEV-225, 226, 230..233). Channels: lint JSONL records and exit codes.

use ilium_plugin_host::cli::CliOutput;
use serde_json::json;

use crate::common::*;

const DESCRIPTION_KEY: &str = "/description";

/// A project whose only warning is `lint.description-missing`, with `lint.toml` text `toml`.
fn warned(toml: Option<&str>) -> (tempfile::TempDir, ilium_plugin_host::cli::CliEnv) {
    let (root, env) = lint_env_with(|manifest| manifest["description"] = json!(""));
    if let Some(text) = toml {
        write_text(&project_dir(&env), "lint.toml", text);
    }
    (root, env)
}

fn deny(env: &ilium_plugin_host::cli::CliEnv) -> CliOutput {
    lint(env, &["--deny", "warnings"])
}

/// REQ: DEV-225
#[test]
fn dev_225_plain_lint_exits_0_with_only_warn_and_info_findings() {
    let (_root, env) = warned(None);
    let output = lint(&env, &[]);
    assert_eq!(
        severity_of(&output, "lint", "lint.description-missing").as_deref(),
        Some("warn")
    );
    assert_eq!(output.exit_code, 0);
}

/// REQ: DEV-225
#[test]
fn dev_225_plain_lint_exits_3_when_an_error_finding_exists() {
    let (_root, env) = warned(Some("[[suppress]]\nrule = \"lint.description-missing\"\n"));
    let output = lint(&env, &[]);
    assert_eq!(
        severity_of(&output, "lint", "lint.suppression-reason-missing").as_deref(),
        Some("error")
    );
    assert_eq!(output.exit_code, 3);
}

/// REQ: DEV-225
#[test]
fn dev_225_result_record_carries_the_same_exit_code() {
    let (_root, env) = warned(Some("[[suppress]]\nrule = \"lint.description-missing\"\n"));
    let output = lint(&env, &[]);
    let result = output.stdout_jsonl.last().expect("result record");
    assert_eq!(result["exit_code"], 3);
    assert_eq!(result["ok"], false);
}

/// REQ: DEV-226
#[test]
fn dev_226_deny_warnings_exits_3_for_an_unsuppressed_warning() {
    let (_root, env) = warned(None);
    assert_eq!(deny(&env).exit_code, 3);
}

/// REQ: DEV-226
#[test]
fn dev_226_deny_warnings_exits_0_when_only_info_findings_exist() {
    let (_root, env) = lint_env_with(|_| {});
    std::fs::remove_file(project_dir(&env).join("README.md")).expect("remove readme");
    let output = deny(&env);
    assert_eq!(
        severity_of(&output, "lint", "lint.readme-missing").as_deref(),
        Some("info")
    );
    assert_eq!(output.exit_code, 0);
}

/// REQ: DEV-226
#[test]
fn dev_226_deny_warnings_exits_0_when_the_only_warning_is_suppressed() {
    let (_root, env) = warned(Some("[[suppress]]\nrule = \"lint.description-missing\"\nreason = \"the registry page writes the text\"\n"));
    assert_eq!(deny(&env).exit_code, 0);
}

/// REQ: DEV-226
#[test]
fn dev_226_deny_warnings_exits_3_for_an_error_finding_too() {
    let (_root, env) = warned(Some("[[suppress]]\nrule = \"lint.description-missing\"\n"));
    assert_eq!(deny(&env).exit_code, 3);
}

/// REQ: DEV-230
#[test]
fn dev_230_lint_toml_suppresses_a_rule() {
    let (_root, env) = warned(Some(
        "[[suppress]]\nrule = \"lint.description-missing\"\nreason = \"text comes later\"\n",
    ));
    assert_eq!(
        is_suppressed(&lint(&env, &[]), "lint.description-missing"),
        Some(true)
    );
}

/// REQ: DEV-230
#[test]
fn dev_230_manifest_suppression_key_is_an_unknown_manifest_key() {
    let (_root, env) = lint_env_with(|manifest| {
        manifest["suppress"] = json!([{ "rule": "lint.description-missing", "reason": "x" }]);
    });
    let output = run_in_project(&env, "check", &[]);
    assert!(has_rule(&output, "check", "manifest.unknown-key"));
}

/// REQ: DEV-230
#[test]
fn dev_230_a_suppression_key_in_the_manifest_does_not_suppress_anything() {
    let (_root, env) = lint_env_with(|manifest| {
        manifest["description"] = json!("");
        manifest["suppress"] = json!([{ "rule": "lint.description-missing", "reason": "x" }]);
    });
    assert_eq!(
        is_suppressed(&lint(&env, &[]), "lint.description-missing"),
        Some(false)
    );
}

/// REQ: DEV-231
#[test]
fn dev_231_table_without_reason_produces_suppression_reason_missing() {
    let (_root, env) = warned(Some("[[suppress]]\nrule = \"lint.description-missing\"\n"));
    assert!(has_rule(
        &lint(&env, &[]),
        "lint",
        "lint.suppression-reason-missing"
    ));
}

/// REQ: DEV-231
#[test]
fn dev_231_table_with_rule_and_reason_has_no_reason_finding() {
    let (_root, env) = warned(Some(
        "[[suppress]]\nrule = \"lint.description-missing\"\nreason = \"text comes later\"\n",
    ));
    assert!(!has_rule(
        &lint(&env, &[]),
        "lint",
        "lint.suppression-reason-missing"
    ));
}

/// REQ: DEV-231
#[test]
fn dev_231_key_path_limits_the_suppression_to_the_same_key_path() {
    let toml = format!("[[suppress]]\nrule = \"lint.description-missing\"\nkey_path = \"{DESCRIPTION_KEY}\"\nreason = \"text comes later\"\n");
    let (_root, env) = warned(Some(&toml));
    assert_eq!(
        is_suppressed(&lint(&env, &[]), "lint.description-missing"),
        Some(true)
    );
}

/// REQ: DEV-231
#[test]
fn dev_231_a_different_key_path_does_not_suppress_and_is_reported_unused() {
    let (_root, env) = warned(Some("[[suppress]]\nrule = \"lint.description-missing\"\nkey_path = \"/name\"\nreason = \"text comes later\"\n"));
    let output = lint(&env, &[]);
    assert_eq!(
        is_suppressed(&output, "lint.description-missing"),
        Some(false)
    );
    assert_eq!(
        severity_of(&output, "lint", "lint.suppression-unused").as_deref(),
        Some("warn")
    );
}

/// REQ: DEV-231
#[test]
fn dev_231_without_key_path_every_finding_of_the_rule_is_suppressed() {
    let (_root, env) = lint_env_with(|manifest| {
        manifest["description"] = json!("");
        manifest["contributes"]["commands"] = json!([{ "id": "open", "title": "Open notes", "icon": "nope-one" }, { "id": "close", "title": "Close notes", "icon": "nope-two" }]);
    });
    write_text(
        &project_dir(&env),
        "lint.toml",
        "[[suppress]]\nrule = \"lint.icon-name-unknown\"\nreason = \"icons come from a theme\"\n",
    );
    let output = lint(&env, &[]);
    let icons: Vec<_> = records_of(&output, "lint")
        .into_iter()
        .filter(|record| record["rule"] == "lint.icon-name-unknown")
        .collect();
    assert_eq!(icons.len(), 2);
    assert!(icons.iter().all(|record| record["suppressed"] == true));
}

/// REQ: DEV-231
#[test]
fn dev_231_suppressing_an_unknown_lint_rule_produces_suppression_unknown() {
    let (_root, env) = warned(Some(
        "[[suppress]]\nrule = \"lint.no-such-rule\"\nreason = \"typo\"\n",
    ));
    assert_eq!(
        severity_of(&lint(&env, &[]), "lint", "lint.suppression-unknown").as_deref(),
        Some("error")
    );
}

/// REQ: DEV-232
#[test]
fn dev_232_suppressing_a_manifest_code_produces_suppression_unknown() {
    let (_root, env) = warned(Some(
        "[[suppress]]\nrule = \"manifest.schema\"\nreason = \"no\"\n",
    ));
    let output = lint(&env, &[]);
    assert!(has_rule(&output, "lint", "lint.suppression-unknown"));
    assert_eq!(output.exit_code, 3);
}

/// REQ: DEV-232
#[test]
fn dev_232_suppressing_a_component_code_produces_suppression_unknown() {
    let (_root, env) = warned(Some(
        "[[suppress]]\nrule = \"component.size-warning\"\nreason = \"no\"\n",
    ));
    assert!(has_rule(
        &lint(&env, &[]),
        "lint",
        "lint.suppression-unknown"
    ));
}

/// REQ: DEV-232
#[test]
fn dev_232_suppressing_a_signature_code_produces_suppression_unknown() {
    let (_root, env) = warned(Some(
        "[[suppress]]\nrule = \"signature.absent\"\nreason = \"no\"\n",
    ));
    assert!(has_rule(
        &lint(&env, &[]),
        "lint",
        "lint.suppression-unknown"
    ));
}

/// REQ: DEV-232
#[test]
fn dev_232_check_ignores_lint_toml_and_still_reports_the_finding() {
    let (_root, env) = lint_env_with(|_| {});
    write_text(
        &project_dir(&env),
        "lint.toml",
        "[[suppress]]\nrule = \"component.missing\"\nreason = \"no\"\n",
    );
    let output = run_in_project(&env, "check", &[]);
    let record = finding(&output, "check", "component.missing").expect("finding");
    assert_eq!(record["severity"], "error");
    assert_eq!(output.exit_code, 3);
}

/// REQ: DEV-233
#[test]
fn dev_233_suppressed_finding_is_still_printed_with_suppressed_true() {
    let (_root, env) = warned(Some(
        "[[suppress]]\nrule = \"lint.description-missing\"\nreason = \"text comes later\"\n",
    ));
    let output = lint(&env, &[]);
    let record = finding(&output, "lint", "lint.description-missing").expect("still printed");
    assert_eq!(record["suppressed"], true);
    assert_eq!(record["key_path"], DESCRIPTION_KEY);
}

/// REQ: DEV-233
#[test]
fn dev_233_unsuppressed_finding_has_suppressed_false() {
    let (_root, env) = warned(None);
    assert_eq!(
        is_suppressed(&lint(&env, &[]), "lint.description-missing"),
        Some(false)
    );
}

/// REQ: DEV-233
#[test]
fn dev_233_suppressed_findings_do_not_count_toward_the_exit_code_or_the_warning_total() {
    let (_root, env) = warned(Some(
        "[[suppress]]\nrule = \"lint.description-missing\"\nreason = \"text comes later\"\n",
    ));
    let output = deny(&env);
    assert_eq!(output.exit_code, 0);
    let summary = records_of(&output, "summary");
    assert_eq!(summary[0]["warnings"], 0);
}
