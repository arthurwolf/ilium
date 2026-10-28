//! Gap tests for the trust and supply-chain reference: publisher transfer and id recovery
//! (LIF-309..311).
//!
//! Assumptions: `present_transfer` takes the id, the receiving key and the signing keys;
//! `pinned_key` returns the publisher key the host pins for an id.

use crate::support::*;

/// REQ: LIF-309
#[test]
fn lif_309_transfer_changes_pinned_key_only_with_two_signatures() {
    assert_manual_says(
        "reference-trust-and-supply-chain",
        "A plugin id MUST move to another publisher only through a transfer statement",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").signed("key-current"));
    rig.trust_publisher("key-current");
    rig.trust_publisher("key-receiving");
    let one_signature = rig.present_transfer(NOTES, "key-receiving", &["key-current"]);
    assert_ne!(
        one_signature.exit_code, 0,
        "one signature must not transfer"
    );
    assert_eq!(rig.pinned_key(NOTES).as_deref(), Some("key-current"));
    let two_signatures =
        rig.present_transfer(NOTES, "key-receiving", &["key-current", "key-receiving"]);
    assert_eq!(two_signatures.exit_code, 0, "two signatures must transfer");
    assert_eq!(rig.pinned_key(NOTES).as_deref(), Some("key-receiving"));
}

/// REQ: LIF-310
#[test]
fn lif_310_transfer_keeps_grants_and_storage_then_waits_for_consent_before_update() {
    assert_manual_says(
        "reference-trust-and-supply-chain",
        "After a transfer the existing grants and the plugin data MUST stay unchanged",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").signed("key-current"));
    let server = rig.start_server("default");
    rig.write_storage(NOTES, "global", "draft", b"kept");
    let grants_before = rig.grants_json();
    rig.trust_publisher("key-current");
    rig.trust_publisher("key-receiving");
    rig.present_transfer(NOTES, "key-receiving", &["key-current", "key-receiving"]);
    assert_eq!(rig.grants_json(), grants_before, "grants stay unchanged");
    assert_eq!(
        rig.read_storage(NOTES, "global", "draft"),
        Some(b"kept".to_vec()),
        "plugin data stays unchanged"
    );
    assert_eq!(
        server
            .notices()
            .iter()
            .filter(|notice| notice.contains("fingerprint"))
            .count(),
        1,
        "the new key fingerprint is shown once"
    );
    update(&rig, &pkg(NOTES, "2.0.0").signed("key-receiving"));
    server.apply_pending();
    assert_eq!(
        server.active_version(NOTES).as_deref(),
        Some("1.0.0"),
        "the next update from the new key waits for consent"
    );
}

/// REQ: LIF-311
#[test]
fn lif_311_recovered_id_refuses_update_from_new_key_as_pin_mismatch() {
    assert_manual_says(
        "reference-trust-and-supply-chain",
        "A recovered id MUST NOT inherit the old pin",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").signed("key-old"));
    rig.registry_takedown(NOTES, "1.0.0");
    rig.recover_id(NOTES, "reviewed-recovery-request");
    let output = update(&rig, &pkg(NOTES, "3.0.0").signed("key-new"));
    assert_ne!(
        output.exit_code, 0,
        "the first package under the new key is refused"
    );
    assert!(
        output.stderr.to_lowercase().contains("pin mismatch"),
        "the refusal names the pin mismatch: {}",
        output.stderr
    );
    assert_eq!(list_row(&rig, NOTES)["version"], "1.0.0");
}

/// REQ: LIF-309
#[test]
fn lif_309_transfer_signed_only_by_the_receiving_key_keeps_the_current_pin() {
    assert_manual_says(
        "reference-trust-and-supply-chain",
        "and countersigned by the receiving key",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").signed("key-current"));
    rig.trust_publisher("key-current");
    rig.trust_publisher("key-receiving");
    let receiving_only = rig.present_transfer(NOTES, "key-receiving", &["key-receiving"]);
    assert_ne!(
        receiving_only.exit_code, 0,
        "a transfer without the current publisher signature is refused"
    );
    assert_eq!(rig.pinned_key(NOTES).as_deref(), Some("key-current"));
}

/// REQ: LIF-310
#[test]
fn lif_310_transfer_does_not_restart_the_running_instance() {
    assert_manual_says(
        "reference-trust-and-supply-chain",
        "the host MUST show the user the new publisher key fingerprint once",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").signed("key-current"));
    let server = rig.start_server("default");
    let activations_before = server.activations(NOTES).len();
    rig.trust_publisher("key-current");
    rig.trust_publisher("key-receiving");
    rig.present_transfer(NOTES, "key-receiving", &["key-current", "key-receiving"]);
    server.apply_pending();
    assert_eq!(
        server.activations(NOTES).len(),
        activations_before,
        "the transfer alone activates nothing new"
    );
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.0.0"));
}

/// REQ: LIF-311
#[test]
fn lif_311_reinstall_from_the_new_key_after_recovery_is_accepted_as_a_new_install() {
    assert_manual_says(
        "reference-trust-and-supply-chain",
        "every host MUST treat the first package under the new key as a new install",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").signed("key-old"));
    rig.registry_takedown(NOTES, "1.0.0");
    rig.recover_id(NOTES, "reviewed-recovery-request");
    let reinstall = install(&rig, &pkg(NOTES, "3.0.0").signed("key-new"));
    assert_eq!(
        reinstall.exit_code, 0,
        "a user reinstall under the new key is accepted"
    );
    assert_eq!(list_row(&rig, NOTES)["version"], "3.0.0");
}
