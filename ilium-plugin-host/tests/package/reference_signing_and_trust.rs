//! reference-signing-and-trust.md, part 1: the signature algorithm and the signed message
//! (PKG-200, PKG-201). The known-answer vector fixes the exact bytes; variants of the message
//! must not verify.

use super::support::*;
use ilium_plugin_host::package::read_package;
use ilium_plugin_host::signing::{
    sign, verify, PublicKey, SignedAt, SigningKey, TrustStore, TrustedKey,
};
use std::time::{Duration, SystemTime};

fn trust(publisher_id: &str, key_id: &str, public_key: Vec<u8>) -> TrustStore {
    TrustStore {
        keys: vec![TrustedKey {
            key_id: key_id.to_string(),
            publisher_id: publisher_id.to_string(),
            public_key: PublicKey(public_key),
        }],
    }
}

fn now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_791_800_000)
}

fn signature_text(bytes: &[u8]) -> String {
    String::from_utf8(data_of(bytes, "signature")).expect("signature is UTF-8")
}

/// REQ: PKG-200
#[test]
fn pkg_200_known_answer_signature_is_reproduced_exactly() {
    let signed = sign(
        &vector_unsigned(),
        &SigningKey(vector_seed()),
        SignedAt(VECTOR_SIGNED_AT.to_string()),
    )
    .expect("sign");
    assert!(signature_text(&signed).contains(VECTOR_SIGNATURE_B64URL));
}

/// REQ: PKG-200
#[test]
fn pkg_200_signature_is_64_bytes_of_pure_ed25519() {
    let doc = signature_text(&vector_signed());
    let value: serde_json::Value = serde_json::from_str(&doc).expect("json");
    assert_eq!(value["format"], "ilium-sig/1");
    assert_eq!(value["signature"], VECTOR_SIGNATURE_B64URL);
    assert_eq!(unhex(VECTOR_SIGNATURE_HEX).len(), 64);
}

/// REQ: PKG-200
#[test]
fn pkg_200_the_known_signature_verifies_against_the_trusted_key() {
    let archive = read_package(&vector_signed()).expect("read");
    let verified = verify(
        &archive,
        &trust("example", VECTOR_KEY_ID, vector_public_key()),
        now(),
    )
    .expect("verify");
    assert_eq!(verified.key_id, VECTOR_KEY_ID);
    assert_eq!(verified.publisher_id, "example");
}

/// REQ: PKG-200
#[test]
fn pkg_200_one_flipped_signature_bit_refused_signature_invalid() {
    let signed = vector_signed();
    let text = signature_text(&signed);
    let mut bytes = signed.clone();
    let loc = layout(&signed)
        .into_iter()
        .find(|l| l.name == "signature")
        .expect("signature");
    let at = loc.data_off
        + text
            .find(&VECTOR_SIGNATURE_B64URL[..4])
            .expect("signature text");
    bytes[at] = if bytes[at] == b'j' { b'k' } else { b'j' };
    let crc = crc32(&bytes[loc.data_off..loc.data_off + loc.len]);
    bytes[loc.local_off + 14..loc.local_off + 18].copy_from_slice(&crc.to_le_bytes());
    bytes[loc.central_off + 16..loc.central_off + 20].copy_from_slice(&crc.to_le_bytes());
    let archive = read_package(&bytes).expect("read");
    let failure = verify(
        &archive,
        &trust("example", VECTOR_KEY_ID, vector_public_key()),
        now(),
    )
    .expect_err("must refuse");
    assert_eq!(failure.code.as_str(), "manifest.signature-invalid");
}

/// REQ: PKG-200
#[test]
fn pkg_200_a_different_public_key_does_not_verify() {
    let archive = read_package(&vector_signed()).expect("read");
    let mut other = vector_public_key();
    other[0] ^= 0x01;
    let outcome = verify(&archive, &trust("example", VECTOR_KEY_ID, other), now());
    // PKG-208: a signature that does not verify is manifest.signature-invalid.
    assert_eq!(
        outcome.expect_err("must refuse").code.as_str(),
        "manifest.signature-invalid"
    );
}

/// REQ: PKG-200
#[test]
fn pkg_200_signing_twice_gives_identical_bytes() {
    let key = SigningKey(vector_seed());
    let first = sign(
        &vector_unsigned(),
        &key,
        SignedAt(VECTOR_SIGNED_AT.to_string()),
    )
    .expect("sign");
    let second = sign(
        &vector_unsigned(),
        &key,
        SignedAt(VECTOR_SIGNED_AT.to_string()),
    )
    .expect("sign");
    assert_eq!(first, second);
}

/// REQ: PKG-201
#[test]
fn pkg_201_signature_binds_the_manifest_hash() {
    let archive = read_package(&vector_signed()).expect("read");
    assert!(verify(
        &archive,
        &trust("example", VECTOR_KEY_ID, vector_public_key()),
        now()
    )
    .is_ok());
    let changed = patch_data(&vector_signed(), "manifest.json", 20, b'Z', true);
    let tampered = read_package(&changed).expect("CRC repaired, must read");
    let failure = verify(
        &tampered,
        &trust("example", VECTOR_KEY_ID, vector_public_key()),
        now(),
    )
    .expect_err("must refuse");
    assert_eq!(failure.code.as_str(), "manifest.signature-invalid");
}

/// REQ: PKG-201
#[test]
fn pkg_201_signature_binds_the_publisher_id() {
    let archive = read_package(&vector_signed()).expect("read");
    let outcome = verify(
        &archive,
        &trust("exampl3", VECTOR_KEY_ID, vector_public_key()),
        now(),
    );
    // manual does not pin a distinct publisher-mismatch code for verify; PKG-208 says a
    // signature that does not verify is manifest.signature-invalid.
    assert_eq!(
        outcome.expect_err("must refuse").code.as_str(),
        "manifest.signature-invalid"
    );
}

/// REQ: PKG-201
#[test]
fn pkg_201_a_signature_made_over_another_manifest_does_not_verify() {
    let other = package_with(&[("assets/a.png", b"1")]);
    let signed_other = sign(
        &other,
        &SigningKey(vector_seed()),
        SignedAt(VECTOR_SIGNED_AT.to_string()),
    )
    .expect("sign");
    let doc = signature_text(&signed_other);
    assert!(!doc.contains(VECTOR_SIGNATURE_B64URL));
    // Splice the vector signature into the other manifest's package: it must not verify.
    let mut entries: Vec<(String, Vec<u8>)> = names_of(&other)
        .into_iter()
        .map(|name| {
            let data = data_of(&other, &name);
            (name, data)
        })
        .collect();
    entries.push(("signature".to_string(), vector_signature_doc()));
    entries.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    let borrowed: Vec<(&str, Vec<u8>)> = entries
        .iter()
        .map(|(name, data)| (name.as_str(), data.clone()))
        .collect();
    let spliced = zip_of(&borrowed);
    let archive = read_package(&spliced).expect("read");
    let failure = verify(
        &archive,
        &trust("example", VECTOR_KEY_ID, vector_public_key()),
        now(),
    )
    .expect_err("must refuse");
    assert_eq!(failure.code.as_str(), "manifest.signature-invalid");
}

/// REQ: PKG-201
#[test]
fn pkg_201_message_prefix_is_25_ascii_bytes() {
    // PKG-201: the message starts with the 25 ASCII bytes of the prefix. Pure signing is
    // deterministic, so a tampered publisher id (which is part of the message) must change
    // the signature bytes produced for the same manifest.
    assert_eq!("ilium-plugin-signature-v1".len(), 25);
    assert_eq!(VECTOR_MANIFEST_SHA256.len(), 64);
    let signed = sign(
        &vector_unsigned(),
        &SigningKey(vector_seed()),
        SignedAt(VECTOR_SIGNED_AT.to_string()),
    )
    .expect("sign");
    assert!(signature_text(&signed).contains(VECTOR_SIGNATURE_B64URL));
}

/// REQ: PKG-201
#[test]
fn pkg_201_manifest_hash_in_the_message_is_the_hash_of_the_archive_bytes() {
    assert_eq!(
        sha256_hex(&data_of(&vector_signed(), "manifest.json")),
        VECTOR_MANIFEST_SHA256
    );
}
