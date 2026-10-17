//! Twins of ilium-client/src/recap/custody.rs: redaction before text leaves Ilium and the denied paths.

use crate::support::*;

fn evidence_of(screen: &[&str]) -> String {
    let (mut host, _, _) = start_shell_run(screen);
    job_evidence(&mut host)
}

/// REQ: PORT:ilium-client/src/recap/custody.rs::redacts_token_value_suffix_without_hiding_the_key_name
#[test]
fn port_custody_redacts_token_value_suffix_without_hiding_the_key_name() {
    let evidence = evidence_of(&["export TOKEN_VALUE=supersecretvalue123"]);
    assert!(
        evidence.contains("export TOKEN_VALUE=[redacted]"),
        "{evidence}"
    );
    assert!(!evidence.contains("supersecretvalue123"));
}

/// REQ: PORT:ilium-client/src/recap/custody.rs::redacts_key_values_but_keeps_key_names
#[test]
fn port_custody_redacts_key_values_but_keeps_key_names() {
    let evidence =
        evidence_of(&["export ANTHROPIC_API_KEY=abcd1234efgh5678 and api_key: \"zzzz9999\""]);
    assert!(!evidence.contains("abcd1234efgh5678"));
    assert!(!evidence.contains("zzzz9999"));
    assert!(evidence.contains("ANTHROPIC_API_KEY"));
    assert!(evidence.matches("[redacted]").count() >= 2, "{evidence}");
}

/// REQ: PORT:ilium-client/src/recap/custody.rs::redacts_token_shapes_and_private_key_blocks
#[test]
fn port_custody_redacts_token_shapes_and_private_key_blocks() {
    let evidence = evidence_of(&[
        "token sk-abcdefghijklmnopqrstu and ghp_abcdefghijklmnopqrstuvwxyz0123 -----BEGIN OPENSSH PRIVATE KEY-----",
        "AAAA",
        "-----END OPENSSH PRIVATE KEY----- Authorization: Bearer abcdefghijklmnop12345",
    ]);
    assert!(!evidence.contains("abcdefghijklmnopqrstu"));
    assert!(!evidence.contains("ghp_abcdef"));
    assert!(!evidence.contains("AAAA"));
    assert!(!evidence.contains("abcdefghijklmnop12345"));
    assert!(evidence.contains("[redacted]"));
}

/// REQ: PORT:ilium-client/src/recap/custody.rs::leaves_ordinary_prose_untouched
#[test]
fn port_custody_leaves_ordinary_prose_untouched() {
    let text = "Ran cargo test; 12 tests passed in src/lib.rs.";
    let evidence = evidence_of(&[text]);
    assert!(evidence.contains(text));
    assert!(!evidence.contains("[redacted]"));
}

fn pointer_outcome(path: &str) -> serde_json::Value {
    let screen = format!("wrote {path}");
    short_after_answer(
        &[screen.as_str()],
        &[(path, "x")],
        pointer_answer(path, "note"),
    )
}

/// REQ: PORT:ilium-client/src/recap/custody.rs::denies_credential_stores_and_key_files
#[test]
fn port_custody_denies_credential_stores_and_key_files() {
    for path in [
        ".ssh/config",
        ".env.local",
        "deploy/id_ed25519",
        "certs/server.pem",
        "config/api-secrets.yaml",
        ".claude/projects/x/session.jsonl",
    ] {
        let short = pointer_outcome(path);
        assert_eq!(short["pointers"], serde_json::json!([]), "{path}");
        assert_eq!(short["rejected_pointers"], 1, "{path}");
    }
}

/// REQ: PORT:ilium-client/src/recap/custody.rs::allows_ordinary_project_files
#[test]
fn port_custody_allows_ordinary_project_files() {
    for path in ["src/lib.rs", "docs/TODO.md"] {
        let short = pointer_outcome(path);
        assert_eq!(
            short["pointers"].as_array().map(Vec::len),
            Some(1),
            "{path}"
        );
        assert_eq!(short["rejected_pointers"], 0, "{path}");
    }
}
