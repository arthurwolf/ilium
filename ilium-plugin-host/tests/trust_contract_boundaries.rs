//! Actual trust records, key identity, locked persistence and crypto boundaries.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ilium_plugin_host::trust::{
    AddedBy, PublisherKey, Revocation, RevocationReason, Source, Status, TrustDocument, TrustEntry,
    TrustRepository, TrustSnapshot,
};
use sha2::{Digest, Sha256};
const NOW: &str = "2026-10-22 12:00:00";
fn key(seed: u8) -> PublisherKey {
    let key = ed25519_dalek::SigningKey::from_bytes(&[seed; 32]);
    let bytes = key.verifying_key().to_bytes();
    PublisherKey {
        format: "ilium-pub/1".into(),
        publisher_id: "example".into(),
        publisher: "Example Labs".into(),
        algorithm: "ed25519".into(),
        public_key: URL_SAFE_NO_PAD.encode(bytes),
        key_id: format!("{:x}", Sha256::digest(bytes)),
        created: NOW.into(),
    }
}
fn entry(key: &PublisherKey) -> TrustEntry {
    TrustEntry {
        key_id: key.key_id.clone(),
        publisher_id: key.publisher_id.clone(),
        publisher: key.publisher.clone(),
        public_key: key.public_key.clone(),
        added: NOW.into(),
        added_by: AddedBy::User,
        status: Status::Trusted,
        revoked: None,
    }
}
fn document(keys: Vec<TrustEntry>) -> TrustDocument {
    TrustDocument {
        keys,
        ..TrustDocument::default()
    }
}
fn revoked(mut entry: TrustEntry, reason: RevocationReason, at: &str) -> TrustEntry {
    entry.status = Status::Revoked;
    entry.revoked = Some(Revocation {
        reason,
        at: at.into(),
    });
    entry
}
#[test]
fn public_keys_require_the_actual_fingerprint_and_publisher_grammar() {
    let good = key(1);
    assert_eq!(
        PublisherKey::parse(&serde_json::to_vec(&good).unwrap()).unwrap(),
        good
    );
    let mut bad = good.clone();
    bad.key_id = "0".repeat(64);
    assert_eq!(
        PublisherKey::parse(&serde_json::to_vec(&bad).unwrap())
            .unwrap_err()
            .code
            .as_str(),
        "trust.key-malformed"
    );
    let mut reserved = good;
    reserved.publisher_id = "ilium".into();
    assert_eq!(
        PublisherKey::parse(&serde_json::to_vec(&reserved).unwrap())
            .unwrap_err()
            .code
            .as_str(),
        "trust.publisher-reserved"
    );
}
#[test]
fn either_store_revokes_and_a_user_entry_cannot_override_the_system() {
    let key = key(2);
    for (user, system) in [
        (
            document(vec![entry(&key)]),
            document(vec![revoked(
                entry(&key),
                RevocationReason::Compromised,
                NOW,
            )]),
        ),
        (
            document(vec![revoked(entry(&key), RevocationReason::Retired, NOW)]),
            document(vec![entry(&key)]),
        ),
    ] {
        let snapshot = TrustSnapshot::resolve(&user, &system, NOW).unwrap();
        assert_eq!(snapshot.keys()[0].source, Source::Both);
        assert!(snapshot.signing_store().keys.is_empty());
        assert_eq!(
            snapshot
                .require_publisher(&key.key_id, "example", "Example Labs")
                .unwrap_err()
                .code
                .as_str(),
            "trust.key-revoked"
        );
    }
}
#[test]
fn future_revocation_applies_at_its_exact_utc_boundary() {
    let key = key(3);
    let user = document(vec![revoked(
        entry(&key),
        RevocationReason::Retired,
        "2026-10-23 00:00:00",
    )]);
    assert_eq!(
        TrustSnapshot::resolve(&user, &TrustDocument::default(), NOW)
            .unwrap()
            .signing_store()
            .keys
            .len(),
        1
    );
    assert!(
        TrustSnapshot::resolve(&user, &TrustDocument::default(), "2026-10-23 00:00:00")
            .unwrap()
            .signing_store()
            .keys
            .is_empty()
    );
}
#[test]
fn publisher_binding_conflicts_and_nfc_names_are_checked() {
    let key = key(4);
    let mut first = entry(&key);
    first.publisher = "Café".into();
    let snapshot = TrustSnapshot::resolve(
        &document(vec![first.clone()]),
        &TrustDocument::default(),
        NOW,
    )
    .unwrap();
    assert!(snapshot
        .require_publisher(&key.key_id, "example", "Cafe\u{301}")
        .is_ok());
    assert_eq!(
        snapshot
            .require_publisher(&key.key_id, "example", "Other Labs")
            .unwrap_err()
            .code
            .as_str(),
        "manifest.publisher-mismatch"
    );
    let mut other = first.clone();
    other.publisher_id = "other".into();
    assert_eq!(
        TrustSnapshot::resolve(&document(vec![first]), &document(vec![other]), NOW)
            .unwrap_err()
            .code
            .as_str(),
        "trust.publisher-conflict"
    );
}
#[test]
fn locked_crud_preserves_other_keys_and_refuses_in_use_removal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("trust.json");
    let first = TrustRepository::new(path.clone(), TrustDocument::default()).unwrap();
    let second = TrustRepository::new(path.clone(), TrustDocument::default()).unwrap();
    let a = key(5);
    let b = key(6);
    first.add(&a, NOW, false).unwrap();
    assert_eq!(
        second.add(&b, NOW, false).unwrap_err().code.as_str(),
        "trust.additional-required"
    );
    second.add(&b, NOW, true).unwrap();
    assert_eq!(first.snapshot(NOW).unwrap().keys().len(), 2);
    assert_eq!(
        first
            .remove(&a.key_id, &["example.notes".into()], false)
            .unwrap_err()
            .code
            .as_str(),
        "trust.in-use"
    );
    let result = first
        .remove(&a.key_id, &["example.notes".into()], true)
        .unwrap();
    assert_eq!(result.disabled_untrusted, vec!["example.notes"]);
    assert_eq!(
        second.snapshot(NOW).unwrap().signing_store().keys[0].key_id,
        b.key_id
    );
    assert_eq!(
        PublisherKey::parse(&second.export_with_created(&b.key_id, &b.created).unwrap())
            .unwrap()
            .key_id,
        b.key_id
    );
    let stored = TrustDocument::parse(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(stored.keys[0].publisher, "Example Labs");
    assert_eq!(stored.keys[0].added_by, AddedBy::User);
}
#[test]
fn system_remove_is_refused_and_compromise_is_not_downgraded() {
    let dir = tempfile::tempdir().unwrap();
    let key = key(7);
    let repo =
        TrustRepository::new(dir.path().join("trust.json"), document(vec![entry(&key)])).unwrap();
    assert_eq!(
        repo.remove(&key.key_id, &[], true)
            .unwrap_err()
            .code
            .as_str(),
        "trust.system-entry"
    );
    let effects = repo
        .revoke(
            &key.key_id,
            RevocationReason::Compromised,
            NOW,
            &["example.notes".into()],
        )
        .unwrap();
    assert_eq!(effects.disabled_blocked, vec!["example.notes"]);
    assert_eq!(
        repo.revoke(&key.key_id, RevocationReason::Retired, NOW, &[])
            .unwrap_err()
            .code
            .as_str(),
        "trust.key-revoked"
    );
    assert_eq!(
        repo.add(&key, NOW, true).unwrap_err().code.as_str(),
        "trust.key-revoked"
    );
    assert!(repo.snapshot(NOW).unwrap().signing_store().keys.is_empty());
}
#[test]
fn invalid_existing_store_is_preserved_instead_of_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("trust.json");
    let original = b"{\"format\":\"future-format\",\"keys\":[]}";
    std::fs::write(&path, original).unwrap();
    let repo = TrustRepository::new(path.clone(), TrustDocument::default()).unwrap();
    assert_eq!(
        repo.add(&key(8), NOW, false).unwrap_err().code.as_str(),
        "trust.store-invalid"
    );
    assert_eq!(std::fs::read(path).unwrap(), original);
}
#[test]
fn known_key_does_not_make_unsigned_or_tampered_packages_trusted() {
    use ilium_plugin_host::{
        package::{read_package, write_package},
        signing::{self, SignedAt, SigningKey},
    };
    let digits: String = include_str!("package/golden/minimal_unsigned.hex")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let package: Vec<_> = (0..digits.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&digits[i..i + 2], 16).unwrap())
        .collect();
    let key = key(9);
    let snapshot =
        TrustSnapshot::resolve(&document(vec![entry(&key)]), &TrustDocument::default(), NOW)
            .unwrap();
    assert!(snapshot
        .verify_package(&read_package(&package).unwrap())
        .is_err());
    let signed = signing::sign(
        &package,
        &SigningKey::from_seed([9; 32]),
        SignedAt(NOW.into()),
    )
    .unwrap();
    assert!(snapshot
        .verify_package(&read_package(&signed).unwrap())
        .is_ok());
    let archive = read_package(&signed).unwrap();
    let files: Vec<_> = archive
        .entries
        .into_iter()
        .map(|mut e| {
            if e.path == "manifest.json" {
                let mut json: serde_json::Value = serde_json::from_slice(&e.bytes).unwrap();
                json["name"] = serde_json::json!("Tampered");
                e.bytes = serde_json::to_vec(&json).unwrap();
            }
            (e.path, e.bytes)
        })
        .collect();
    let tampered = read_package(&write_package(&files).unwrap()).unwrap();
    assert_eq!(
        snapshot
            .verify_package(&tampered)
            .unwrap_err()
            .code
            .as_str(),
        "manifest.signature-invalid"
    );
    assert_eq!(
        TrustSnapshot::default()
            .verify_package(&read_package(&signed).unwrap())
            .unwrap_err()
            .code
            .as_str(),
        "trust.key-unknown"
    );
    let revoked = TrustSnapshot::resolve(
        &document(vec![revoked(entry(&key), RevocationReason::Retired, NOW)]),
        &TrustDocument::default(),
        NOW,
    )
    .unwrap();
    assert_eq!(
        revoked
            .verify_package(&read_package(&signed).unwrap())
            .unwrap_err()
            .code
            .as_str(),
        "trust.key-revoked"
    );
}

#[test]
fn concurrent_user_transactions_do_not_lose_other_publishers_keys() {
    let dir = tempfile::tempdir().unwrap();
    let path = std::sync::Arc::new(dir.path().join("trust.json"));
    let start = std::sync::Arc::new(std::sync::Barrier::new(8));
    let threads: Vec<_> = (20..28)
        .map(|seed| {
            let path = path.clone();
            let start = start.clone();
            std::thread::spawn(move || {
                let repo = TrustRepository::new((*path).clone(), TrustDocument::default()).unwrap();
                start.wait();
                repo.add(&key(seed), NOW, true).unwrap();
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(
        TrustRepository::new((*path).clone(), TrustDocument::default())
            .unwrap()
            .snapshot(NOW)
            .unwrap()
            .keys()
            .len(),
        8
    );
}

#[test]
fn retired_keys_return_badges_without_claiming_runtime_shutdown() {
    let dir = tempfile::tempdir().unwrap();
    let repo =
        TrustRepository::new(dir.path().join("trust.json"), TrustDocument::default()).unwrap();
    let key = key(10);
    repo.add(&key, NOW, false).unwrap();
    let effect = repo
        .revoke(
            &key.key_id,
            RevocationReason::Retired,
            NOW,
            &["example.notes".into()],
        )
        .unwrap();
    assert_eq!(effect.retired_badge, vec!["example.notes"]);
    assert!(effect.disabled_blocked.is_empty());
    assert!(effect.disabled_untrusted.is_empty());
    assert_eq!(
        repo.export(&key.key_id).unwrap_err().code.as_str(),
        "trust.context-required"
    );
}

#[test]
fn strict_raw_records_reject_inconsistent_status_dates_and_unknown_fields() {
    let key = key(11);
    let mut inconsistent = entry(&key);
    inconsistent.status = Status::Revoked;
    assert!(document(vec![inconsistent]).validate().is_err());
    let mut date = entry(&key);
    date.added = "2026-02-30 12:00:00".into();
    assert!(document(vec![date]).validate().is_err());
    let mut json = serde_json::to_value(document(vec![entry(&key)])).unwrap();
    json["future_authority"] = serde_json::json!(true);
    assert!(TrustDocument::parse(&serde_json::to_vec(&json).unwrap()).is_err());
}

#[test]
fn real_clock_conversion_preserves_epoch_and_gregorian_leap_day() {
    use ilium_plugin_host::trust::utc_timestamp;
    use std::time::{Duration, UNIX_EPOCH};
    assert_eq!(utc_timestamp(UNIX_EPOCH).unwrap(), "1970-01-01 00:00:00");
    assert_eq!(
        utc_timestamp(UNIX_EPOCH + Duration::from_secs(951782400)).unwrap(),
        "2000-02-29 00:00:00"
    );
    assert_eq!(
        utc_timestamp(UNIX_EPOCH + Duration::from_secs(951868799)).unwrap(),
        "2000-02-29 23:59:59"
    );
}

#[test]
fn rejected_user_store_returns_warning_and_retains_only_valid_system_authority() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("trust.json");
    let original = b"{\"format\":\"unsupported\",\"keys\":[]}";
    std::fs::write(&path, original).unwrap();
    let system_key = key(12);
    let repo = TrustRepository::new(path.clone(), document(vec![entry(&system_key)])).unwrap();
    let loaded = repo
        .load_snapshot_at(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1792670400))
        .unwrap();
    assert_eq!(loaded.warnings.len(), 1);
    assert_eq!(
        loaded.snapshot.signing_store().keys[0].key_id,
        system_key.key_id
    );
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[test]
fn duplicate_trust_and_publisher_fields_are_rejected_without_last_value_wins() {
    let public = key(21);
    let raw = serde_json::to_string(&public).unwrap();
    let repeated_publisher = raw.replacen(
        "\"publisher\":\"Example Labs\"",
        "\"publisher\":\"Attacker\",\"publisher\":\"Example Labs\"",
        1,
    );
    assert_ne!(raw, repeated_publisher);
    assert_eq!(
        PublisherKey::parse(repeated_publisher.as_bytes())
            .unwrap_err()
            .code
            .as_str(),
        "trust.key-malformed"
    );
    let raw = serde_json::to_string(&document(vec![entry(&public)])).unwrap();
    let repeated_status = raw.replacen(
        "\"status\":\"trusted\"",
        "\"status\":\"revoked\",\"status\":\"trusted\"",
        1,
    );
    assert_ne!(raw, repeated_status);
    assert_eq!(
        TrustDocument::parse(repeated_status.as_bytes())
            .unwrap_err()
            .code
            .as_str(),
        "trust.store-invalid"
    );
    let repeated_keys = raw.replacen('{', "{\"keys\":[],", 1);
    assert_eq!(
        TrustDocument::parse(repeated_keys.as_bytes())
            .unwrap_err()
            .code
            .as_str(),
        "trust.store-invalid"
    );
}
