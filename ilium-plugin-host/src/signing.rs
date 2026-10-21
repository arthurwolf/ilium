//! Package signing, trust and verification (reference-packaging.md PKG-200..260).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ilium_plugin_api::manifest::Diagnostic;
use ilium_plugin_api::Tier;

use crate::package::PackageArchive;

/// An Ed25519 signing key (secret bytes).
#[derive(Clone)]
pub struct SigningKey(pub Vec<u8>);

impl std::fmt::Debug for SigningKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SigningKey(<redacted>)")
    }
}

impl SigningKey {
    /// A key from a 32-byte Ed25519 seed (reference-packaging.md PKG-200).
    pub fn from_seed(_seed: [u8; 32]) -> SigningKey {
        unimplemented!("host::signing::SigningKey::from_seed")
    }

    /// The key id of its public key (reference-packaging.md PKG-283).
    pub fn key_id(&self) -> String {
        unimplemented!("host::signing::SigningKey::key_id")
    }
}

/// An Ed25519 public key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicKey(pub Vec<u8>);

/// The signing time recorded in a signature (RFC 3339 UTC).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedAt(pub String);

/// One trusted publisher key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedKey {
    /// Key id.
    pub key_id: String,
    /// Publisher id.
    pub publisher_id: String,
    /// The public key.
    pub public_key: PublicKey,
}

/// The set of trusted keys (reference-packaging.md PKG-230).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustStore {
    /// Trusted keys.
    pub keys: Vec<TrustedKey>,
}

impl TrustStore {
    /// The trust store path: `ILIUM_TRUST_STORE` (absolute), else `$XDG_STATE_HOME/ilium/trust.json`,
    /// else `<home>/.local/state/ilium/trust.json` (reference-packaging.md PKG-221, PKG-267).
    pub fn default_path(_env: &BTreeMap<String, String>, _home: &Path) -> PathBuf {
        unimplemented!("host::signing::TrustStore::default_path")
    }

    /// Writes the store atomically with mode 0600 (reference-packaging.md PKG-267).
    pub fn save_atomic(&self, _path: &Path) -> std::io::Result<()> {
        unimplemented!("host::signing::TrustStore::save_atomic")
    }
}

/// The result of a successful verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    /// The tier the signature earns.
    pub tier: Tier,
    /// Publisher id.
    pub publisher_id: String,
    /// Key id.
    pub key_id: String,
}

/// Signs package bytes and returns the signed package (reference-packaging.md PKG-200).
pub fn sign(
    _package: &[u8],
    _key: &SigningKey,
    _signed_at: SignedAt,
) -> Result<Vec<u8>, Diagnostic> {
    unimplemented!("host::signing::sign")
}

/// Verifies a package against a trust store at a time (reference-packaging.md PKG-210).
pub fn verify(
    _package: &PackageArchive,
    _trust: &TrustStore,
    _now: SystemTime,
) -> Result<Verified, Diagnostic> {
    unimplemented!("host::signing::verify")
}

/// JSON canonicalisation (RFC 8785) of a value (reference-packaging.md PKG-205).
pub fn jcs(_value: &serde_json::Value) -> Result<String, Diagnostic> {
    unimplemented!("host::signing::jcs")
}

#[cfg(test)]
mod tests {
    use super::{sign, verify, PublicKey, SignedAt, SigningKey, TrustStore, TrustedKey, Verified};
    use crate::package::{read_package, write_package};
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    use ed25519_dalek::{Signer, SigningKey as DalekSigningKey};
    use ilium_plugin_api::Tier;
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};

    /// Seed of the manual test vector (reference-signing-and-trust.md, "Test vector").
    const SEED_HEX: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
    const PUBLIC_KEY_BASE64: &str = "A6EHv_POEL4dcN0Y50vAmWfk1jCbpQ1fHdyGZBJVMbg";
    const KEY_ID: &str = "56475aa75463474c0285df5dbf2bcab73da651358839e9b77481b2eab107708c";
    const SIGNATURE_BASE64: &str =
        "jv5zcDEiiDljfN-iy3nZLtCgScmbTMJoOv0M3X9r0D9Tm4dvdqE43gpEEAW_cqVod6iYQAjeIR8E0-vFwEplBw";
    const SIGNED_AT: &str = "2026-10-12 10:00:00";
    const SIGNATURE_PREFIX: &[u8] = b"ilium-plugin-signature-v1";
    const MANIFEST_SHA256: &str = "350b5e47c5781e72a9bb7b3d6630335fca0390402d0f625a48857a234f99c99d";
    const SIGNED_PACKAGE_SHA256: &str = "d2238076eea48c58ceb4416dbb50438c6ffee23028051d7e9a0cb1f9ae7fc408";
    const SIGNED_PACKAGE_LENGTH: usize = 1114;
    const COMPONENT_HEX: &str = "0061736d0d000100001811696c69756d3a6170692d76657273696f6e000000010000";
    /// The 658-byte unsigned package of reference-package-format.md, "Test vector: a minimal package".
    const UNSIGNED_HEX: &str = "
504b03041400000800000000210072c5884392010000920100000d0000006d61
6e69666573742e6a736f6e7b0a202022617069223a2022696c69756d3a706c75
67696e40302e312e30222c0a2020226964223a20226578616d706c652e6d696e
696d616c222c0a2020226e616d65223a20224d696e696d616c222c0a20202276
657273696f6e223a2022302e312e30222c0a2020227075626c6973686572223a
20224578616d706c65204c616273222c0a202022736572766572223a207b0a20
20202022656e747279223a20227365727665722e7761736d222c0a2020202022
776f726c64223a2022696c69756d3a7365727665722f6261736540302e312e30
222c0a20202020226578706f727473223a205b5d0a20207d2c0a20202266696c
6573223a205b0a202020207b0a2020202020202270617468223a202273657276
65722e7761736d222c0a202020202020226c656e677468223a2033342c0a2020
2020202022736861323536223a20223032386662386162336531353435373035
3937663363633039616435393738326162343464396136383733663138333434
393064613632356566336336636438220a202020207d0a20205d0a7d0a504b03
041400000800000000210017cea50d22000000220000000b0000007365727665
722e7761736d0061736d0d000100001811696c69756d3a6170692d7665727369
6f6e000000010000504b010214031400000800000000210072c5884392010000
920100000d0000000000000000000000a481000000006d616e69666573742e6a
736f6e504b010214031400000800000000210017cea50d22000000220000000b
0000000000000000000000a481bd0100007365727665722e7761736d504b0506
000000000200020074000000080200000000
";

    // ---------------------------------------------------------------------------------------------
    // Helpers
    // ---------------------------------------------------------------------------------------------

    fn hex_bytes(text: &str) -> Vec<u8> {
        let digits: Vec<u8> = text.bytes().filter(|byte| !byte.is_ascii_whitespace()).collect();
        digits
            .chunks(2)
            .map(|pair| {
                let pair_text = std::str::from_utf8(pair).expect("hex digits are ASCII");
                u8::from_str_radix(pair_text, 16).expect("a valid hex byte")
            })
            .collect()
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn position_of(haystack: &[u8], needle: &[u8]) -> usize {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
            .expect("the needle is present in the haystack")
    }

    fn seed() -> [u8; 32] {
        hex_bytes(SEED_HEX)
            .try_into()
            .expect("the seed is 32 bytes")
    }

    fn unsigned_package() -> Vec<u8> {
        hex_bytes(UNSIGNED_HEX)
    }

    /// The 402 manifest bytes exactly as stored in the manual vector (PKG-202: never re-serialised).
    fn manifest_bytes() -> Vec<u8> {
        let package = unsigned_package();
        let start = position_of(&package, b"{\n  \"api\"");
        let manifest = package[start..start + 402].to_vec();
        assert_eq!(
            sha256_hex(&manifest),
            MANIFEST_SHA256,
            "the manifest bytes taken from the manual vector are the test input"
        );
        manifest
    }

    fn component_bytes() -> Vec<u8> {
        hex_bytes(COMPONENT_HEX)
    }

    fn public_key_of(seed: [u8; 32]) -> Vec<u8> {
        DalekSigningKey::from_bytes(&seed)
            .verifying_key()
            .to_bytes()
            .to_vec()
    }

    fn key_id_of(public_key: &[u8]) -> String {
        sha256_hex(public_key)
    }

    /// Base64url Ed25519 signature over `M` = prefix, 0x00, publisher id, 0x00, SHA-256 of the manifest
    /// bytes (PKG-201), made with the given seed. Built independently of the code under test.
    fn signature_over(manifest: &[u8], seed: [u8; 32], prefix: &[u8], publisher_id: &str) -> String {
        let mut message = prefix.to_vec();
        message.push(0);
        message.extend_from_slice(publisher_id.as_bytes());
        message.push(0);
        message.extend_from_slice(Sha256::digest(manifest).as_slice());
        let signature = DalekSigningKey::from_bytes(&seed).sign(&message);
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    }

    fn raw_signature() -> Vec<u8> {
        URL_SAFE_NO_PAD
            .decode(SIGNATURE_BASE64)
            .expect("the vector signature is base64url")
    }

    fn entry(key_id: &str, publisher_id: &str, signature: &str) -> (String, String, String) {
        (
            key_id.to_string(),
            publisher_id.to_string(),
            signature.to_string(),
        )
    }

    fn valid_entry(publisher_id: &str) -> (String, String, String) {
        entry(
            KEY_ID,
            publisher_id,
            &signature_over(&manifest_bytes(), seed(), SIGNATURE_PREFIX, publisher_id),
        )
    }

    fn entries_of(count: usize) -> Vec<(String, String, String)> {
        (1..=count)
            .map(|index| entry(&format!("{index:064x}"), "example", SIGNATURE_BASE64))
            .collect()
    }

    /// The `signature` entry document in the format of PKG-204 to PKG-206.
    fn signature_document(entries: &[(String, String, String)]) -> Vec<u8> {
        let signatures: Vec<serde_json::Value> = entries
            .iter()
            .map(|(key_id, publisher_id, signature)| {
                serde_json::json!({
                    "algorithm": "ed25519",
                    "key_id": key_id,
                    "publisher_id": publisher_id,
                    "signature": signature,
                    "signed_at": SIGNED_AT,
                })
            })
            .collect();
        serde_json::to_vec(&serde_json::json!({
            "format": "ilium-sig/1",
            "signatures": signatures,
        }))
        .expect("the signature document serialises")
    }

    fn padded_document(length: usize) -> Vec<u8> {
        let mut document = signature_document(&[valid_entry("example")]);
        document.resize(length, b' ');
        document
    }

    fn package_with(manifest: &[u8], document: &[u8]) -> Vec<u8> {
        write_package(&[
            ("manifest.json".to_string(), manifest.to_vec()),
            ("server.wasm".to_string(), component_bytes()),
            ("signature".to_string(), document.to_vec()),
        ])
        .expect("the test package is writable")
    }

    fn package_with_document(document: &[u8]) -> Vec<u8> {
        package_with(&manifest_bytes(), document)
    }

    fn signed_package() -> Vec<u8> {
        sign(
            &unsigned_package(),
            &SigningKey::from_seed(seed()),
            SignedAt(SIGNED_AT.to_string()),
        )
        .expect("the manual vector signs")
    }

    fn trust_store_for(publisher_id: &str, seed: [u8; 32]) -> TrustStore {
        let public_key = public_key_of(seed);
        TrustStore {
            keys: vec![TrustedKey {
                key_id: key_id_of(&public_key),
                publisher_id: publisher_id.to_string(),
                public_key: PublicKey(public_key),
            }],
        }
    }

    fn example_trust() -> TrustStore {
        trust_store_for("example", seed())
    }

    fn now() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)
    }

    /// The codes a package is refused with: read-level codes, or the verify-level code.
    fn refusal_codes(package: &[u8], trust: &TrustStore) -> Vec<String> {
        match read_package(package) {
            Err(diagnostics) => diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str().to_string())
                .collect(),
            Ok(archive) => vec![verify(&archive, trust, now())
                .expect_err("the package must be refused")
                .code
                .as_str()
                .to_string()],
        }
    }

    fn assert_refused_with(package: &[u8], trust: &TrustStore, expected_code: &str) {
        let codes = refusal_codes(package, trust);
        assert!(
            codes.iter().any(|code| code == expected_code),
            "expected a refusal with {expected_code}, got {codes:?}"
        );
    }

    fn environment_of(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    // ---------------------------------------------------------------------------------------------
    // Keys
    // ---------------------------------------------------------------------------------------------

    /// REQ: PKG-214
    #[test]
    fn from_seed_derives_the_key_id_of_the_manual_vector() {
        assert_eq!(SigningKey::from_seed(seed()).key_id(), KEY_ID);
    }

    /// REQ: PKG-214
    #[test]
    fn from_seed_with_all_zero_seed_gives_a_64_character_lowercase_key_id() {
        let key_id = SigningKey::from_seed([0u8; 32]).key_id();
        assert_eq!(key_id.len(), 64);
        assert!(key_id
            .chars()
            .all(|character| character.is_ascii_digit() || ('a'..='f').contains(&character)));
        assert_ne!(key_id, KEY_ID);
    }

    /// REQ: PKG-213
    #[test]
    fn debug_output_of_a_signing_key_never_shows_the_seed() {
        let rendered = format!("{:?}", SigningKey::from_seed(seed()));
        assert_eq!(rendered, "SigningKey(<redacted>)");
        assert!(!rendered.contains(SEED_HEX));
        assert!(!rendered.contains("000102"));
    }

    // ---------------------------------------------------------------------------------------------
    // Signing
    // ---------------------------------------------------------------------------------------------

    /// REQ: PKG-200
    #[test]
    fn sign_reproduces_the_known_answer_signature_of_the_manual_vector() {
        let archive = read_package(&signed_package()).expect("the signed package is structurally valid");
        let signature = archive
            .signature
            .expect("the signed package has a signature entry");
        assert_eq!(signature.signature, SIGNATURE_BASE64);
        assert_eq!(signature.key_id, KEY_ID);
        assert_eq!(signature.publisher_id, "example");
        assert_eq!(signature.signed_at, SIGNED_AT);
    }

    /// REQ: PKG-650
    #[test]
    fn sign_output_matches_the_manual_signed_package_length_and_hash() {
        let package = signed_package();
        assert_eq!(package.len(), SIGNED_PACKAGE_LENGTH);
        assert_eq!(sha256_hex(&package), SIGNED_PACKAGE_SHA256);
    }

    /// REQ: PKG-650
    #[test]
    fn sign_twice_with_the_same_inputs_gives_identical_bytes() {
        assert_eq!(signed_package(), signed_package());
    }

    /// REQ: PKG-206
    #[test]
    fn sign_records_signed_at_without_changing_the_signature() {
        let package = sign(
            &unsigned_package(),
            &SigningKey::from_seed(seed()),
            SignedAt("2027-01-02 03:04:05".to_string()),
        )
        .expect("the manual vector signs with another time");
        let signature = read_package(&package)
            .expect("the signed package is structurally valid")
            .signature
            .expect("the signed package has a signature entry");
        assert_eq!(signature.signed_at, "2027-01-02 03:04:05");
        assert_eq!(signature.signature, SIGNATURE_BASE64);
    }

    /// REQ: PKG-202
    #[test]
    fn sign_hashes_the_manifest_bytes_exactly_as_stored() {
        let manifest = String::from_utf8(manifest_bytes())
            .expect("the manifest is UTF-8")
            .replacen("{\n", "{\n\n", 1)
            .into_bytes();
        let unsigned = write_package(&[
            ("manifest.json".to_string(), manifest.clone()),
            ("server.wasm".to_string(), component_bytes()),
        ])
        .expect("the unsigned package is writable");
        let package = sign(
            &unsigned,
            &SigningKey::from_seed(seed()),
            SignedAt(SIGNED_AT.to_string()),
        )
        .expect("the package signs");
        let signature = read_package(&package)
            .expect("the signed package is structurally valid")
            .signature
            .expect("the signed package has a signature entry");
        assert_eq!(
            signature.signature,
            signature_over(&manifest, seed(), SIGNATURE_PREFIX, "example")
        );
    }

    /// REQ: PKG-200
    #[test]
    fn sign_refuses_empty_input_because_there_is_no_manifest_to_sign() {
        assert!(sign(
            &[],
            &SigningKey::from_seed(seed()),
            SignedAt(SIGNED_AT.to_string()),
        )
        .is_err());
    }

    /// REQ: PKG-651
    #[test]
    fn sign_uses_the_given_signed_at_text_verbatim() {
        let package = sign(
            &unsigned_package(),
            &SigningKey::from_seed(seed()),
            SignedAt("2026-12-31 23:59:59".to_string()),
        )
        .expect("the package signs");
        let signature = read_package(&package)
            .expect("the signed package is structurally valid")
            .signature
            .expect("the signed package has a signature entry");
        assert_eq!(signature.signed_at, "2026-12-31 23:59:59");
    }

    // ---------------------------------------------------------------------------------------------
    // Verification
    // ---------------------------------------------------------------------------------------------

    /// REQ: PKG-228
    #[test]
    fn verify_a_valid_signature_from_a_trusted_key_gives_tier_t2() {
        let archive = read_package(&signed_package()).expect("the signed package is structurally valid");
        let verified = verify(&archive, &example_trust(), now()).expect("a trusted signature verifies");
        assert_eq!(
            verified,
            Verified {
                tier: Tier::T2,
                publisher_id: "example".to_string(),
                key_id: KEY_ID.to_string(),
            }
        );
    }

    /// REQ: PKG-206
    #[test]
    fn verify_does_not_depend_on_the_clock() {
        let archive = read_package(&signed_package()).expect("the signed package is structurally valid");
        let at_epoch = verify(&archive, &example_trust(), SystemTime::UNIX_EPOCH)
            .expect("the epoch is not a decision input");
        let far_future = verify(
            &archive,
            &example_trust(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(4_000_000_000),
        )
        .expect("a far-future clock is not a decision input");
        assert_eq!(at_epoch, far_future);
    }

    /// REQ: PKG-207
    #[test]
    fn verify_accepts_when_one_of_two_valid_signatures_is_from_a_trusted_key() {
        let other_seed = [0x42u8; 32];
        let document = signature_document(&[
            entry(
                &key_id_of(&public_key_of(other_seed)),
                "example",
                &signature_over(&manifest_bytes(), other_seed, SIGNATURE_PREFIX, "example"),
            ),
            valid_entry("example"),
        ]);
        let archive = read_package(&package_with_document(&document))
            .expect("two signature objects are structurally valid");
        let verified = verify(&archive, &example_trust(), now())
            .expect("the trusted signature decides the package");
        assert_eq!(verified.tier, Tier::T2);
        assert_eq!(verified.key_id, KEY_ID);
    }

    /// REQ: PKG-255
    #[test]
    fn verify_refuses_a_key_missing_from_the_empty_trust_store_with_key_unknown() {
        assert_refused_with(&signed_package(), &TrustStore::default(), "trust.key-unknown");
    }

    /// REQ: PKG-248
    #[test]
    fn verify_refuses_a_package_whose_manifest_has_one_changed_byte() {
        let manifest = String::from_utf8(manifest_bytes())
            .expect("the manifest is UTF-8")
            .replacen("\"Minimal\"", "\"Minimax\"", 1)
            .into_bytes();
        let document = signature_document(&[valid_entry("example")]);
        assert_refused_with(
            &package_with(&manifest, &document),
            &example_trust(),
            "manifest.signature-invalid",
        );
    }

    /// REQ: PKG-250
    #[test]
    fn verify_refuses_a_signature_with_one_changed_byte() {
        let mut raw = raw_signature();
        raw[0] ^= 0x01;
        let document = signature_document(&[entry(KEY_ID, "example", &URL_SAFE_NO_PAD.encode(&raw))]);
        assert_refused_with(
            &package_with_document(&document),
            &example_trust(),
            "manifest.signature-invalid",
        );
    }

    /// REQ: PKG-203
    #[test]
    fn verify_refuses_a_non_canonical_scalar_with_signature_invalid() {
        let mut raw = raw_signature();
        raw[63] = 0xff;
        let document = signature_document(&[entry(KEY_ID, "example", &URL_SAFE_NO_PAD.encode(&raw))]);
        assert_refused_with(
            &package_with_document(&document),
            &example_trust(),
            "manifest.signature-invalid",
        );
    }

    /// REQ: PKG-251
    #[test]
    fn verify_refuses_a_signature_made_under_another_domain_prefix() {
        let document = signature_document(&[entry(
            KEY_ID,
            "example",
            &signature_over(
                &manifest_bytes(),
                seed(),
                b"ilium-plugin-signature-v2",
                "example",
            ),
        )]);
        assert_refused_with(
            &package_with_document(&document),
            &example_trust(),
            "manifest.signature-invalid",
        );
    }

    /// REQ: PKG-252
    #[test]
    fn verify_refuses_a_signature_made_for_another_publisher_id() {
        let document = signature_document(&[entry(
            KEY_ID,
            "example",
            &signature_over(&manifest_bytes(), seed(), SIGNATURE_PREFIX, "other"),
        )]);
        assert_refused_with(
            &package_with_document(&document),
            &example_trust(),
            "manifest.signature-invalid",
        );
    }

    /// REQ: PKG-253
    #[test]
    fn verify_refuses_a_plugin_id_outside_the_signed_publisher_with_id_publisher() {
        let document = signature_document(&[entry(
            KEY_ID,
            "other",
            &signature_over(&manifest_bytes(), seed(), SIGNATURE_PREFIX, "other"),
        )]);
        assert_refused_with(
            &package_with_document(&document),
            &trust_store_for("other", seed()),
            "manifest.id-publisher",
        );
    }

    /// REQ: PKG-208
    #[test]
    fn verify_refuses_a_key_id_that_verifies_no_signature_with_signature_invalid() {
        let zero_key_id = "0".repeat(64);
        let document = signature_document(&[entry(&zero_key_id, "example", SIGNATURE_BASE64)]);
        assert_refused_with(
            &package_with_document(&document),
            &example_trust(),
            "manifest.signature-invalid",
        );
    }

    // ---------------------------------------------------------------------------------------------
    // Signature document format
    // ---------------------------------------------------------------------------------------------

    /// REQ: PKG-254
    #[test]
    fn read_refuses_a_format_other_than_ilium_sig_1() {
        let document = String::from_utf8(signature_document(&[valid_entry("example")]))
            .expect("the document is UTF-8")
            .replace("ilium-sig/1", "ilium-sig/2")
            .into_bytes();
        assert_refused_with(
            &package_with_document(&document),
            &example_trust(),
            "package.signature-format",
        );
    }

    /// REQ: PKG-204
    #[test]
    fn read_refuses_a_document_with_an_extra_top_level_key() {
        let mut value: serde_json::Value = serde_json::from_slice(&signature_document(&[valid_entry("example")]))
            .expect("the document is JSON");
        value["note"] = serde_json::json!("extra");
        let document = serde_json::to_vec(&value).expect("the document serialises");
        assert_refused_with(
            &package_with_document(&document),
            &example_trust(),
            "package.signature-format",
        );
    }

    /// REQ: PKG-204
    #[test]
    fn read_refuses_a_document_without_format() {
        let mut value: serde_json::Value = serde_json::from_slice(&signature_document(&[valid_entry("example")]))
            .expect("the document is JSON");
        value
            .as_object_mut()
            .expect("the document is an object")
            .remove("format");
        let document = serde_json::to_vec(&value).expect("the document serialises");
        assert_refused_with(
            &package_with_document(&document),
            &example_trust(),
            "package.signature-format",
        );
    }

    /// REQ: PKG-205
    #[test]
    fn read_refuses_a_document_with_zero_signature_objects() {
        let document = signature_document(&[]);
        assert_refused_with(
            &package_with_document(&document),
            &example_trust(),
            "package.signature-format",
        );
    }

    /// REQ: PKG-205
    #[test]
    fn read_refuses_a_document_with_five_signature_objects() {
        let document = signature_document(&entries_of(5));
        assert_refused_with(
            &package_with_document(&document),
            &example_trust(),
            "package.signature-format",
        );
    }

    /// REQ: PKG-205
    #[test]
    fn read_accepts_one_and_four_signature_objects_at_the_boundaries() {
        read_package(&package_with_document(&signature_document(&entries_of(1))))
            .expect("one signature object is within the limit");
        read_package(&package_with_document(&signature_document(&entries_of(4))))
            .expect("four signature objects are within the limit");
    }

    /// REQ: PKG-205
    #[test]
    fn read_refuses_a_signature_object_without_signed_at() {
        let document = serde_json::to_vec(&serde_json::json!({
            "format": "ilium-sig/1",
            "signatures": [{
                "algorithm": "ed25519",
                "key_id": KEY_ID,
                "publisher_id": "example",
                "signature": SIGNATURE_BASE64,
            }],
        }))
        .expect("the document serialises");
        assert_refused_with(
            &package_with_document(&document),
            &example_trust(),
            "package.signature-format",
        );
    }

    /// REQ: PKG-206
    #[test]
    fn read_refuses_an_uppercase_key_id() {
        let document = signature_document(&[entry(&KEY_ID.to_uppercase(), "example", SIGNATURE_BASE64)]);
        assert_refused_with(
            &package_with_document(&document),
            &example_trust(),
            "package.signature-format",
        );
    }

    /// REQ: PKG-206
    #[test]
    fn read_refuses_a_signature_one_character_short_of_86() {
        let document = signature_document(&[entry(KEY_ID, "example", &SIGNATURE_BASE64[..85])]);
        assert_refused_with(
            &package_with_document(&document),
            &example_trust(),
            "package.signature-format",
        );
    }

    /// REQ: PKG-208
    #[test]
    fn read_accepts_a_signature_entry_of_exactly_16_kib() {
        read_package(&package_with_document(&padded_document(16_384)))
            .expect("an entry of exactly 16 KiB is within the limit");
    }

    /// REQ: PKG-208
    #[test]
    fn read_refuses_a_signature_entry_one_byte_over_16_kib() {
        assert_refused_with(
            &package_with_document(&padded_document(16_385)),
            &example_trust(),
            "package.signature-format",
        );
    }

    /// REQ: PKG-208
    #[test]
    fn read_refuses_the_same_key_listed_twice() {
        let document = signature_document(&[valid_entry("example"), valid_entry("example")]);
        assert_refused_with(
            &package_with_document(&document),
            &example_trust(),
            "package.signature-format",
        );
    }

    // ---------------------------------------------------------------------------------------------
    // Trust store location and persistence
    // ---------------------------------------------------------------------------------------------

    /// REQ: PKG-221
    #[test]
    fn default_path_falls_back_to_the_local_state_folder_of_the_home() {
        assert_eq!(
            TrustStore::default_path(&BTreeMap::new(), Path::new("/home/tester")),
            PathBuf::from("/home/tester/.local/state/ilium/trust.json")
        );
    }

    /// REQ: PKG-267
    #[test]
    fn default_path_uses_xdg_state_home_when_it_is_set() {
        let environment = environment_of(&[("XDG_STATE_HOME", "/srv/state")]);
        assert_eq!(
            TrustStore::default_path(&environment, Path::new("/home/tester")),
            PathBuf::from("/srv/state/ilium/trust.json")
        );
    }

    /// REQ: PKG-267
    #[test]
    fn default_path_lets_ilium_trust_store_replace_the_xdg_location() {
        let environment = environment_of(&[
            ("XDG_STATE_HOME", "/srv/state"),
            ("ILIUM_TRUST_STORE", "/opt/ci/trust.json"),
        ]);
        assert_eq!(
            TrustStore::default_path(&environment, Path::new("/home/tester")),
            PathBuf::from("/opt/ci/trust.json")
        );
    }

    /// REQ: PKG-221
    #[test]
    fn save_atomic_writes_the_ilium_trust_format_with_mode_0600() {
        let directory = tempfile::tempdir().expect("a temporary folder");
        let path = directory.path().join("trust.json");
        example_trust()
            .save_atomic(&path)
            .expect("the store is written");
        let value: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&path).expect("the store file exists"),
        )
        .expect("the store is JSON");
        assert_eq!(value["format"], "ilium-trust/1");
        assert_eq!(value["keys"][0]["key_id"], KEY_ID);
        assert_eq!(value["keys"][0]["publisher_id"], "example");
        assert_eq!(value["keys"][0]["public_key"], PUBLIC_KEY_BASE64);
        let mode = std::fs::metadata(&path)
            .expect("the store file has metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    /// REQ: PKG-221
    #[test]
    fn save_atomic_writes_an_empty_key_list_for_the_empty_store() {
        let directory = tempfile::tempdir().expect("a temporary folder");
        let path = directory.path().join("trust.json");
        TrustStore::default()
            .save_atomic(&path)
            .expect("the empty store is written");
        let value: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&path).expect("the store file exists"),
        )
        .expect("the store is JSON");
        assert_eq!(value["keys"].as_array().expect("keys is an array").len(), 0);
    }

    /// REQ: PKG-221
    #[test]
    fn save_atomic_replaces_the_file_and_leaves_no_temporary_file_behind() {
        let directory = tempfile::tempdir().expect("a temporary folder");
        let path = directory.path().join("trust.json");
        TrustStore::default()
            .save_atomic(&path)
            .expect("the first write succeeds");
        example_trust()
            .save_atomic(&path)
            .expect("the second write replaces the first");
        let names: Vec<String> = std::fs::read_dir(directory.path())
            .expect("the folder is readable")
            .map(|entry| {
                entry
                    .expect("the entry is readable")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, vec!["trust.json".to_string()]);
        let value: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&path).expect("the store file exists"),
        )
        .expect("the store is JSON");
        assert_eq!(value["keys"].as_array().expect("keys is an array").len(), 1);
    }

    /// REQ: PKG-221
    #[test]
    fn save_atomic_fails_when_the_folder_does_not_exist() {
        let directory = tempfile::tempdir().expect("a temporary folder");
        let path = directory.path().join("missing").join("trust.json");
        assert!(example_trust().save_atomic(&path).is_err());
    }
}
