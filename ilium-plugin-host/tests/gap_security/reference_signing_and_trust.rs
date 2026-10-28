//! Signing and trust page (SEC-721, SEC-722).

use crate::support::*;

/// REQ: SEC-721
#[test]
fn sec_721_trust_store_modified_between_reads_refuses_t2_and_t3_and_keeps_a_tampered_copy() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").signed("key-a"));
    let server = rig.start_server("s1");
    rig.advance(secs(1));
    rig.tamper_trust_store_between_reads();
    rig.advance(secs(2));
    let refusal = server
        .refusal(NOTES)
        .expect("a plugin loaded after a trust store tamper must be refused");
    assert!(
        refusal.contains("trust.store-tampered"),
        "expected trust.store-tampered, got {refusal}"
    );
    assert!(
        rig.tampered_copies().iter().any(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().starts_with(".tampered-"))
                .unwrap_or(false)
        }),
        "the modified trust store must be preserved as .tampered-<timestamp>"
    );
    assert!(
        server
            .notices()
            .iter()
            .any(|notice| notice.contains("trust store")),
        "the user must be told about the tampering"
    );
}

/// REQ: SEC-722
#[test]
fn sec_722_signing_key_valid_for_more_than_two_years_is_refused_at_generation() {
    let rig = rig();
    let output = rig.cli(&[
        "keys",
        "new",
        "--algorithm",
        "ed25519",
        "--validity-days",
        "900",
    ]);
    assert_ne!(
        output.exit_code, 0,
        "an Ed25519 signing key with more than 2 years of validity must be refused"
    );
}

/// REQ: SEC-722
#[test]
fn sec_722_publish_with_an_expired_key_is_refused_and_the_cli_warns_from_60_days_before_expiry() {
    let rig = rig();
    let created = rig.cli(&[
        "keys",
        "new",
        "--algorithm",
        "ed25519",
        "--validity-days",
        "730",
    ]);
    assert_eq!(created.exit_code, 0, "a 2-year key must be created");
    rig.advance(secs((730 - 60) * 86_400));
    let warned = rig.cli(&["keys", "status"]);
    assert!(
        warned
            .stdout_jsonl
            .iter()
            .any(|line| line["type"] == "warning" && line["code"] == "key.expiring"),
        "the CLI must warn 60 days before expiry"
    );
    rig.advance(secs(61 * 86_400));
    let publish = rig.cli(&["registry", "publish", NOTES, "--sign-with", "current"]);
    assert_ne!(
        publish.exit_code, 0,
        "the registry must refuse a publish with an expired key"
    );
    assert!(
        error_codes(&publish)
            .iter()
            .any(|code| code == "key.expired"),
        "expected key.expired, got {:?}",
        error_codes(&publish)
    );
}

/// REQ: SEC-721
#[test]
fn sec_721_untouched_trust_store_loads_the_plugin_and_keeps_no_tampered_copy() {
    assert_manual_says(
        "reference-signing-and-trust",
        "The host MUST detect modification of the trust store and the grant file between reads",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").signed("key-a"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    assert!(
        rig.tampered_copies().is_empty(),
        "an unmodified trust store leaves no .tampered- copy"
    );
    assert_eq!(
        server.active_version(NOTES).as_deref(),
        Some("1.0.0"),
        "the signed plugin loads when the trust store is intact"
    );
}
