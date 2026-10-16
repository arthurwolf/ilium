//! reference-plugin-cli.md: `install` (DEV-140..142).

use ilium_plugin_host::cli::MemorySession;

use crate::common::*;

fn s(path: &std::path::Path) -> &str {
    path.to_str().expect("utf8")
}

/// REQ: DEV-140
#[test]
fn dev_140_a_damaged_package_fails_with_a_pipeline_code_before_any_plugin_code_runs() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let (_, package) = packed_project(&env, root.path());
    let bytes = std::fs::read(&package).expect("read");
    std::fs::write(&package, &bytes[..bytes.len() / 3]).expect("write");
    let output = run(&env, &["install", s(&package), "--allow-unsigned", "--yes"]);
    assert!(!recs(&output, "error").is_empty());
    assert!(code(&output).expect("code").starts_with("package."), "{:?}", code(&output));
    assert_eq!(session.started_plugin_count(), 0);
    assert_ne!(output.exit_code, 0);
}

/// REQ: DEV-140
#[test]
fn dev_140_every_failure_is_printed_as_its_own_error_record() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let (_, package) = packed_project(&env, root.path());
    std::fs::write(&package, b"not a package at all").expect("write");
    let output = run(&env, &["install", s(&package), "--allow-unsigned", "--yes"]);
    assert!(!recs(&output, "error").is_empty());
    assert_eq!(types(&output).last().map(String::as_str), Some("result"));
}

/// REQ: DEV-140
#[test]
fn dev_140_a_missing_file_does_not_start_anything() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let missing = root.path().join("missing.iliumpkg");
    let output = run(&env, &["install", s(&missing), "--yes"]);
    assert_ne!(output.exit_code, 0);
    assert_eq!(session.started_plugin_count(), 0);
}

/// REQ: DEV-140
#[test]
fn dev_140_a_valid_signed_package_installs_with_exit_zero() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let (_, package) = packed_project(&env, root.path());
    let (key, public) = make_key(&env, root.path(), "acme");
    run(&env, &["sign", s(&package), "--key", s(&key)]);
    run(&env, &["trust", "add", s(&public)]);
    assert_eq!(run(&env, &["install", s(&package), "--yes"]).exit_code, 0);
}

/// REQ: DEV-141
#[test]
fn dev_141_yes_grants_requested_non_high_risk_grants() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let (_, package) = packed_project(&env, root.path());
    let output = run(&env, &["install", s(&package), "--allow-unsigned", "--yes"]);
    assert_eq!(output.exit_code, 0);
    assert!(session.review_dialog_count() == 0, "no dialog with --yes");
    assert!(!session.granted("acme.demo").is_empty());
    assert!(session.granted("acme.demo").iter().all(|grant| !session.is_high_risk(grant)));
}

/// REQ: DEV-141
#[test]
fn dev_141_yes_does_not_grant_a_high_risk_grant_that_is_not_named() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let (_, package) = packed_project_with_high_risk(&env, root.path());
    run(&env, &["install", s(&package), "--allow-unsigned", "--yes"]);
    assert!(!session.granted("acme.demo").iter().any(|grant| session.is_high_risk(grant)));
}

/// REQ: DEV-141
#[test]
fn dev_141_a_named_high_risk_grant_is_granted() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let (_, package) = packed_project_with_high_risk(&env, root.path());
    run(&env, &["install", s(&package), "--allow-unsigned", "--yes", "--grant", HIGH_RISK_GRANT]);
    assert!(session.granted("acme.demo").iter().any(|grant| grant == HIGH_RISK_GRANT));
}

/// REQ: DEV-141
#[test]
fn dev_141_without_yes_an_attached_client_shows_the_review_dialog() {
    let root = scratch();
    let session = MemorySession::new();
    session.set_client_attached(true);
    let env = env_session(root.path(), &session);
    let (_, package) = packed_project(&env, root.path());
    run(&env, &["install", s(&package), "--allow-unsigned"]);
    assert_eq!(session.review_dialog_count(), 1);
}

/// REQ: DEV-141
#[test]
fn dev_141_naming_a_grant_the_package_does_not_request_grants_nothing_extra() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let (_, package) = packed_project(&env, root.path());
    run(&env, &["install", s(&package), "--allow-unsigned", "--yes", "--grant", HIGH_RISK_GRANT]);
    assert!(!session.granted("acme.demo").iter().any(|grant| grant == HIGH_RISK_GRANT));
}

/// REQ: DEV-142
#[test]
fn dev_142_no_client_and_no_yes_fails_review_required() {
    let root = scratch();
    let session = MemorySession::new();
    session.set_client_attached(false);
    let env = env_session(root.path(), &session);
    let (_, package) = packed_project(&env, root.path());
    let output = run(&env, &["install", s(&package), "--allow-unsigned"]);
    assert_eq!(code(&output).as_deref(), Some("install.review-required"));
    assert_eq!(session.started_plugin_count(), 0);
    assert_eq!(session.review_dialog_count(), 0);
}

/// REQ: DEV-142
#[test]
fn dev_142_yes_removes_the_need_for_a_client() {
    let root = scratch();
    let session = MemorySession::new();
    session.set_client_attached(false);
    let env = env_session(root.path(), &session);
    let (_, package) = packed_project(&env, root.path());
    let output = run(&env, &["install", s(&package), "--allow-unsigned", "--yes"]);
    assert_ne!(code(&output).as_deref(), Some("install.review-required"));
}
