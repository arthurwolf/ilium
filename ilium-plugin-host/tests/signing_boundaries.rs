use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ilium_plugin_host::package::{read_package, write_package};
use ilium_plugin_host::signing::{
    jcs, sign, verify, PublicKey, SignedAt, SigningKey, TrustStore, TrustedKey,
};
use serde_json::{json, Value};
use std::time::SystemTime;

const KEY_ID: &str = "56475aa75463474c0285df5dbf2bcab73da651358839e9b77481b2eab107708c";
fn unhex(text: &str) -> Vec<u8> {
    text.split_whitespace()
        .collect::<String>()
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
fn unsigned() -> Vec<u8> {
    unhex(include_str!("package/golden/minimal_unsigned.hex"))
}
fn signed() -> Vec<u8> {
    unhex(include_str!("package/golden/minimal_signed.hex"))
}
fn trust() -> TrustStore {
    TrustStore {
        keys: vec![TrustedKey {
            key_id: KEY_ID.into(),
            publisher_id: "example".into(),
            public_key: PublicKey(unhex(
                "03a107bff3ce10be1d70dd18e74bc09967e4d6309ba50d5f1ddc8664125531b8",
            )),
        }],
    }
}
fn altered_signature(change: impl FnOnce(&mut Value)) -> Vec<u8> {
    let archive = read_package(&signed()).unwrap();
    let mut files: Vec<_> = archive
        .entries
        .into_iter()
        .map(|entry| (entry.path, entry.bytes))
        .collect();
    let entry = files
        .iter_mut()
        .find(|(name, _)| name == "signature")
        .unwrap();
    let mut document: Value = serde_json::from_slice(&entry.1).unwrap();
    change(&mut document);
    entry.1 = serde_json::to_vec_pretty(&document).unwrap();
    entry.1.push(b'\n');
    write_package(&files).unwrap()
}
fn verification_code(bytes: &[u8]) -> String {
    match read_package(bytes) {
        Err(errors) => errors.first().unwrap().code.as_str().to_owned(),
        Ok(archive) => verify(&archive, &trust(), SystemTime::UNIX_EPOCH)
            .unwrap_err()
            .code
            .as_str()
            .to_owned(),
    }
}
#[test]
fn unknown_signature_key_is_signature_invalid() {
    let bytes = altered_signature(|document| {
        document["signatures"][0]["key_id"] = json!("0".repeat(64));
    });
    assert_eq!(verification_code(&bytes), "manifest.signature-invalid");
}
#[test]
fn seed_derives_documented_public_key_fingerprint_and_hides_secret_debug() {
    let key = SigningKey::from_seed(std::array::from_fn(|index| index as u8));
    assert_eq!(key.key_id(), KEY_ID);
    assert_eq!(format!("{key:?}"), "SigningKey(<redacted>)");
}
#[test]
fn exact_known_answer_package_is_reproduced() {
    let key = SigningKey::from_seed(std::array::from_fn(|index| index as u8));
    let bytes = sign(&unsigned(), &key, SignedAt("2026-10-12 10:00:00".into())).unwrap();
    assert_eq!(bytes, signed());
    let verified = verify(
        &read_package(&bytes).unwrap(),
        &trust(),
        SystemTime::UNIX_EPOCH,
    )
    .unwrap();
    assert_eq!(verified.key_id, KEY_ID);
    assert_eq!(verified.publisher_id, "example");
}
#[test]
fn noncanonical_scalar_is_rejected_by_real_crypto() {
    let bytes = altered_signature(|document| {
        let signature = document["signatures"][0]["signature"].as_str().unwrap();
        let mut signature = URL_SAFE_NO_PAD.decode(signature).unwrap();
        signature[32..].fill(255);
        document["signatures"][0]["signature"] = json!(URL_SAFE_NO_PAD.encode(signature));
    });
    assert_eq!(
        verify(
            &read_package(&bytes).unwrap(),
            &trust(),
            SystemTime::UNIX_EPOCH
        )
        .unwrap_err()
        .code
        .as_str(),
        "manifest.signature-invalid"
    );
}
#[test]
fn malformed_documents_duplicate_keys_and_padding_are_not_crypto_successes() {
    for change in [0, 1, 2] {
        let bytes = altered_signature(|document| match change {
            0 => {
                document["signatures"][0]["extra"] = json!(true);
            }
            1 => {
                let entry = document["signatures"][0].clone();
                document["signatures"].as_array_mut().unwrap().push(entry);
            }
            _ => {
                let text = document["signatures"][0]["signature"]
                    .as_str()
                    .unwrap()
                    .to_owned();
                document["signatures"][0]["signature"] = json!(format!("{text}="));
            }
        });
        assert_eq!(verification_code(&bytes), "package.signature-format");
    }
}
#[test]
fn absent_trust_and_mismatched_public_key_fail_closed() {
    let archive = read_package(&signed()).unwrap();
    assert_eq!(
        verify(&archive, &TrustStore::default(), SystemTime::UNIX_EPOCH)
            .unwrap_err()
            .code
            .as_str(),
        "manifest.signature-invalid"
    );
    let mut wrong = trust();
    wrong.keys[0].public_key.0 = vec![0; 32];
    assert_eq!(
        verify(&archive, &wrong, SystemTime::UNIX_EPOCH)
            .unwrap_err()
            .code
            .as_str(),
        "manifest.signature-invalid"
    );
}
#[test]
fn informational_time_does_not_change_trust_but_invalid_calendar_is_refused() {
    let bytes = altered_signature(|document| {
        document["signatures"][0]["signed_at"] = json!("9999-12-31 23:59:59")
    });
    assert!(verify(
        &read_package(&bytes).unwrap(),
        &trust(),
        SystemTime::UNIX_EPOCH
    )
    .is_ok());
    let bytes = altered_signature(|document| {
        document["signatures"][0]["signed_at"] = json!("2025-02-29 00:00:00")
    });
    assert_eq!(verification_code(&bytes), "package.signature-format");
}
#[test]
fn jcs_known_number_string_and_utf16_key_vectors() {
    assert_eq!(
        jcs(
            &serde_json::from_str::<Value>("[333333333.33333329,1e30,4.50,2e-3,1e-27,-0.0]")
                .unwrap()
        )
        .unwrap(),
        "[333333333.3333333,1e+30,4.5,0.002,1e-27,0]"
    );
    assert_eq!(
        jcs(&json!("\u{000f}\n\"\\/é")).unwrap(),
        "\"\\u000f\\n\\\"\\\\/é\""
    );
    assert_eq!(
        jcs(&json!({"\u{e000}":1,"\u{10000}":2,"a":3})).unwrap(),
        "{\"a\":3,\"𐀀\":2,\"\":1}"
    );
}
#[test]
fn explicit_trust_path_resolution_never_uses_relative_overrides() {
    let home = std::path::Path::new("/example-home");
    let mut env = std::collections::BTreeMap::new();
    assert_eq!(
        TrustStore::default_path(&env, home),
        home.join(".local/state/ilium/trust.json")
    );
    env.insert("XDG_STATE_HOME".into(), "/state".into());
    assert_eq!(
        TrustStore::default_path(&env, home),
        std::path::PathBuf::from("/state/ilium/trust.json")
    );
    env.insert("ILIUM_TRUST_STORE".into(), "relative.json".into());
    assert_eq!(
        TrustStore::default_path(&env, home),
        std::path::PathBuf::from("/state/ilium/trust.json")
    );
    env.insert("ILIUM_TRUST_STORE".into(), "/explicit.json".into());
    assert_eq!(
        TrustStore::default_path(&env, home),
        std::path::PathBuf::from("/explicit.json")
    );
}
#[test]
fn explicit_store_save_is_atomic_and_rejects_bad_keys_before_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("trust.json");
    trust().save_atomic(&path).unwrap();
    let before = std::fs::read(&path).unwrap();
    let document: Value = serde_json::from_slice(&before).unwrap();
    assert_eq!(document["keys"][0]["key_id"], KEY_ID);
    let mut invalid = trust();
    invalid.keys[0].key_id = "0".repeat(64);
    assert!(invalid.save_atomic(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn verification_hashes_exact_stored_manifest_instead_of_parsed_reserialization() {
    let mut archive = read_package(&signed()).unwrap();
    let manifest = archive
        .entries
        .iter_mut()
        .find(|entry| entry.path == "manifest.json")
        .unwrap();
    manifest.bytes.push(b' ');
    assert_eq!(
        verify(&archive, &trust(), SystemTime::UNIX_EPOCH)
            .unwrap_err()
            .code
            .as_str(),
        "manifest.signature-invalid"
    );
}
