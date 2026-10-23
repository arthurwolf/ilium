//! Package signing, trust and verification (reference-packaging.md PKG-200..260).

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signature, Signer, VerifyingKey};
use ilium_plugin_api::Code;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ilium_plugin_api::Tier;
use ilium_plugin_api::manifest::Diagnostic;

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
    pub fn from_seed(seed: [u8; 32]) -> SigningKey {
        SigningKey(seed.to_vec())
    }

    /// The key id of its public key (reference-packaging.md PKG-283).
    pub fn key_id(&self) -> String {
        signing_key(self)
            .map(|key| fingerprint(key.verifying_key().as_bytes()))
            .unwrap_or_default()
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

/// An already-resolved effective set of trusted, nonrevoked publisher keys.
/// This projection does not represent raw-store provenance, revocation or display names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustStore {
    /// Trusted keys.
    pub keys: Vec<TrustedKey>,
}

impl TrustStore {
    /// The trust store path: `ILIUM_TRUST_STORE` (absolute), else `$XDG_STATE_HOME/ilium/trust.json`,
    /// else `<home>/.local/state/ilium/trust.json` (reference-packaging.md PKG-221, PKG-267).
    pub fn default_path(env: &BTreeMap<String, String>, home: &Path) -> PathBuf {
        if let Some(path) = env
            .get("ILIUM_TRUST_STORE")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
        {
            return path;
        }
        if let Some(path) = env
            .get("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
        {
            return path.join("ilium/trust.json");
        }
        home.join(".local/state/ilium/trust.json")
    }

    /// Writes the effective key projection atomically with owner-only permissions.
    /// Raw-store owners must preserve their display/provenance/revocation metadata
    /// separately; this projection cannot substitute for a full trust-policy store.
    pub fn save_atomic(&self, path: &Path) -> std::io::Result<()> {
        use ilium_platform::secure_fs;
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_FILE: AtomicU64 = AtomicU64::new(0);
        let invalid = || {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid trust key identity or conflicting publisher",
            )
        };
        let mut ids = BTreeMap::new();
        for key in &self.keys {
            let bytes: [u8; 32] = key
                .public_key
                .0
                .as_slice()
                .try_into()
                .map_err(|_| invalid())?;
            if valid_public_key(&bytes).is_none()
                || fingerprint(&bytes) != key.key_id
                || !publisher_id_valid(&key.publisher_id)
                || matches!(
                    key.publisher_id.as_str(),
                    "ilium" | "core" | "local" | "test"
                )
            {
                return Err(invalid());
            }
            if ids
                .insert(&key.key_id, &key.publisher_id)
                .is_some_and(|previous| previous != &key.publisher_id)
            {
                return Err(invalid());
            }
        }
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .ok_or_else(invalid)?;
        secure_fs::create_private_directory(parent)?;
        let keys: Vec<_> = self.keys.iter().map(|key| serde_json::json!({"key_id":key.key_id,"publisher_id":key.publisher_id,"public_key":URL_SAFE_NO_PAD.encode(&key.public_key.0)})).collect();
        let mut bytes =
            serde_json::to_vec_pretty(&serde_json::json!({"format":"ilium-trust/1","keys":keys}))?;
        bytes.push(b'\n');
        let (temporary, mut file) = loop {
            let sequence = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
            let temporary = parent.join(format!(
                ".ilium-trust-{}-{sequence}.tmp",
                std::process::id()
            ));
            match secure_fs::private_open_options()
                .write(true)
                .create_new(true)
                .open(&temporary)
            {
                Ok(file) => break (temporary, file),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        };
        let result = (|| {
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            secure_fs::replace_file_durably(&temporary, path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
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

/// Signs using the plugin id's first namespace segment as publisher id.
/// Key-file callers must use [`sign_for_publisher`] with the key's explicit identity.
pub fn sign(package: &[u8], key: &SigningKey, signed_at: SignedAt) -> Result<Vec<u8>, Diagnostic> {
    let archive = crate::package::read_package(package).map_err(first_package_error)?;
    let publisher = archive.manifest.id.split('.').next().unwrap_or_default();
    sign_for_publisher(package, key, publisher, signed_at)
}

/// Signs exact manifest bytes for an explicit publisher identity, without modifying
/// any manifest or payload bytes. The returned archive is deterministically ordered.
pub fn sign_for_publisher(
    package: &[u8],
    key: &SigningKey,
    publisher: &str,
    signed_at: SignedAt,
) -> Result<Vec<u8>, Diagnostic> {
    let archive = crate::package::read_package(package).map_err(first_package_error)?;
    check_publisher(&archive.manifest.id, publisher)?;
    if !timestamp_valid(&signed_at.0) {
        return Err(failure(
            "package.signature-format",
            "signed_at must be a valid YYYY-MM-DD HH:mm:ss UTC date",
        ));
    }
    let manifest = raw_entry(&archive, "manifest.json").ok_or_else(|| {
        failure(
            "manifest.signature-invalid",
            "Exact stored manifest bytes are unavailable",
        )
    })?;
    let key = signing_key(key)?;
    let signature = key.sign(&message(manifest, publisher));
    let envelope = SignatureEnvelope {
        format: "ilium-sig/1".into(),
        signatures: vec![SignatureEntry {
            algorithm: "ed25519".into(),
            key_id: fingerprint(key.verifying_key().as_bytes()),
            publisher_id: publisher.into(),
            signature: URL_SAFE_NO_PAD.encode(signature.to_bytes()),
            signed_at: signed_at.0,
        }],
    };
    let mut signature_bytes = serde_json::to_vec_pretty(&envelope)
        .map_err(|_| failure("package.signature-format", "Signature serialization failed"))?;
    signature_bytes.push(b'\n');
    let mut files: Vec<_> = archive
        .entries
        .into_iter()
        .filter(|entry| entry.path != "signature")
        .map(|entry| (entry.path, entry.bytes))
        .collect();
    files.push(("signature".into(), signature_bytes));
    files.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    crate::package::write_package(&files)
}

/// Verifies cryptography against an already-resolved effective trusted-key set.
/// This does not establish publisher display-name agreement, revocation freshness,
/// rotation acceptance or bundled-release provenance: installation must check those.
pub fn verify(
    package: &PackageArchive,
    trust: &TrustStore,
    now: SystemTime,
) -> Result<Verified, Diagnostic> {
    verify_with_public_keys(package, trust, &[], now)
}

/// Additional public keys allow verification of every countersignature without
/// implicitly trusting those keys. Unknown key bytes fail closed; only a key in
/// `trust` can earn T2. Time is informational for package signatures.
pub fn verify_with_public_keys(
    package: &PackageArchive,
    trust: &TrustStore,
    known_keys: &[TrustedKey],
    _now: SystemTime,
) -> Result<Verified, Diagnostic> {
    let Some(signature_bytes) = raw_entry(package, "signature") else {
        if package.signature.is_some() {
            return Err(failure(
                "manifest.signature-invalid",
                "Exact signature document bytes are unavailable",
            ));
        }
        return Ok(Verified {
            tier: Tier::T3,
            publisher_id: String::new(),
            key_id: String::new(),
        });
    };
    let envelope = parse_signature_document(signature_bytes)?;
    let manifest = raw_entry(package, "manifest.json").ok_or_else(|| {
        failure(
            "manifest.signature-invalid",
            "Exact stored manifest bytes are unavailable",
        )
    })?;
    let mut seen = std::collections::BTreeSet::new();
    let mut verified = None;
    let publisher_id = envelope.signatures[0].publisher_id.clone();
    for (index, entry) in envelope.signatures.into_iter().enumerate() {
        if entry.algorithm != "ed25519"
            || entry.key_id.len() != 64
            || !entry
                .key_id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || !publisher_id_valid(&entry.publisher_id)
            || !timestamp_valid(&entry.signed_at)
            || !seen.insert(entry.key_id.clone())
            || entry.signature.len() != 86
        {
            return Err(failure(
                "package.signature-format",
                "Malformed signature fields or duplicate key",
            ));
        }
        if index == 0 {
            check_publisher(&package.manifest.id, &entry.publisher_id)?;
        }
        let signature = URL_SAFE_NO_PAD.decode(&entry.signature).map_err(|_| {
            failure(
                "package.signature-format",
                "Signature is not unpadded base64url",
            )
        })?;
        if URL_SAFE_NO_PAD.encode(&signature) != entry.signature {
            return Err(failure(
                "package.signature-format",
                "Noncanonical signature encoding",
            ));
        }
        let signature = Signature::from_slice(&signature).map_err(|_| {
            failure(
                "package.signature-format",
                "Signature must contain 64 bytes",
            )
        })?;
        let candidates: Vec<_> = trust
            .keys
            .iter()
            .chain(known_keys)
            .filter(|key| key.key_id == entry.key_id && key.publisher_id == entry.publisher_id)
            .collect();
        if candidates.is_empty() {
            return Err(failure(
                "manifest.signature-invalid",
                "The signing key is not available in the effective trust set or supplied public keys",
            ));
        }
        let verifies = |candidate: &TrustedKey| {
            let Ok(bytes) = <&[u8; 32]>::try_from(candidate.public_key.0.as_slice()) else {
                return false;
            };
            if fingerprint(bytes) != candidate.key_id {
                return false;
            }
            valid_public_key(bytes).is_some_and(|key| {
                key.verify_strict(&message(manifest, &entry.publisher_id), &signature)
                    .is_ok()
            })
        };
        if !candidates.iter().any(|candidate| verifies(candidate)) {
            return Err(failure(
                "manifest.signature-invalid",
                "Signature does not match exact manifest bytes and publisher identity",
            ));
        }
        if trust.keys.iter().any(|key| {
            key.key_id == entry.key_id && key.publisher_id == entry.publisher_id && verifies(key)
        }) && verified.is_none()
        {
            verified = Some(Verified {
                tier: Tier::T2,
                publisher_id: publisher_id.clone(),
                key_id: entry.key_id,
            });
        }
    }
    verified.ok_or_else(|| {
        failure(
            "trust.key-unknown",
            "No verified signature belongs to an effective trusted key",
        )
    })
}

/// Checks the complete signature envelope shape without granting trust or claiming
/// cryptographic verification. Archive admission uses this before projecting fields.
pub fn validate_signature_document(bytes: &[u8]) -> Result<(), Diagnostic> {
    parse_signature_document(bytes).map(|_| ())
}

fn parse_signature_document(signature_bytes: &[u8]) -> Result<SignatureEnvelope, Diagnostic> {
    if signature_bytes.len() > 16 * 1024 {
        return Err(failure(
            "package.signature-format",
            "Signature document exceeds 16 KiB",
        ));
    }
    let envelope: SignatureEnvelope = serde_json::from_slice(signature_bytes)
        .map_err(|_| failure("package.signature-format", "Malformed signature document"))?;
    if envelope.format != "ilium-sig/1" || !(1..=4).contains(&envelope.signatures.len()) {
        return Err(failure(
            "package.signature-format",
            "Unsupported signature format or signature count",
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for entry in &envelope.signatures {
        let decoded = URL_SAFE_NO_PAD.decode(&entry.signature).ok();
        if entry.algorithm != "ed25519"
            || entry.key_id.len() != 64
            || !entry.key_id.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || !publisher_id_valid(&entry.publisher_id)
            || !timestamp_valid(&entry.signed_at)
            || !seen.insert(&entry.key_id)
            || entry.signature.len() != 86
            || !decoded.is_some_and(|bytes| bytes.len() == 64 && URL_SAFE_NO_PAD.encode(bytes) == entry.signature)
        {
            return Err(failure("package.signature-format", "Malformed signature fields or duplicate key"));
        }
    }
    Ok(envelope)
}

fn first_package_error(errors: Vec<Diagnostic>) -> Diagnostic {
    errors
        .into_iter()
        .next()
        .unwrap_or_else(|| failure("package.zip-invalid", "Package parsing failed"))
}
fn raw_entry<'a>(package: &'a PackageArchive, name: &str) -> Option<&'a [u8]> {
    package
        .entries
        .iter()
        .find(|entry| entry.path == name)
        .map(|entry| entry.bytes.as_slice())
}
fn check_publisher(id: &str, publisher: &str) -> Result<(), Diagnostic> {
    if id.starts_with("ilium.") {
        return Err(failure(
            "manifest.id-reserved",
            "The ilium namespace belongs to the release",
        ));
    }
    if !publisher_id_valid(publisher) || matches!(publisher, "ilium" | "core" | "local" | "test") {
        return Err(failure(
            "trust.publisher-reserved",
            "Invalid or reserved publisher identity",
        ));
    }
    if !id
        .strip_prefix(publisher)
        .is_some_and(|suffix| suffix.starts_with('.'))
    {
        return Err(failure(
            "manifest.id-publisher",
            "Plugin id must start with publisher id followed by a dot",
        ));
    }
    Ok(())
}

/// JSON canonicalisation (RFC 8785) of a value (reference-packaging.md PKG-205).
pub fn jcs(value: &serde_json::Value) -> Result<String, Diagnostic> {
    let bytes = serde_json_canonicalizer::to_vec(value)
        .map_err(|_| failure("package.manifest-json", "JSON cannot be canonicalized"))?;
    String::from_utf8(bytes)
        .map_err(|_| failure("package.manifest-json", "Canonical JSON is not UTF-8"))
}

fn failure(code: &'static str, message: &str) -> Diagnostic {
    Diagnostic::error(Code::from_static(code), "", message)
}
fn fingerprint(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn signing_key(key: &SigningKey) -> Result<ed25519_dalek::SigningKey, Diagnostic> {
    let seed: [u8; 32] = key.0.as_slice().try_into().map_err(|_| {
        failure(
            "trust.key-malformed",
            "An Ed25519 seed must contain 32 bytes",
        )
    })?;
    Ok(ed25519_dalek::SigningKey::from_bytes(&seed))
}
fn valid_public_key(bytes: &[u8; 32]) -> Option<VerifyingKey> {
    let key = VerifyingKey::from_bytes(bytes).ok()?;
    (!key.is_weak() && key.to_edwards().compress().to_bytes() == *bytes).then_some(key)
}
fn publisher_id_valid(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.split('.').count() <= 4
        && id.split('.').all(|segment| {
            segment
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_lowercase)
                && segment.split('-').all(|part| {
                    !part.is_empty()
                        && part
                            .bytes()
                            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
                })
        })
}
fn message(manifest: &[u8], publisher: &str) -> Vec<u8> {
    let mut message = b"ilium-plugin-signature-v1\0".to_vec();
    message.extend_from_slice(publisher.as_bytes());
    message.push(0);
    message.extend_from_slice(&Sha256::digest(manifest));
    message
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignatureEnvelope {
    format: String,
    signatures: Vec<SignatureEntry>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignatureEntry {
    algorithm: String,
    key_id: String,
    publisher_id: String,
    signature: String,
    signed_at: String,
}

fn timestamp_valid(time: &str) -> bool {
    let bytes = time.as_bytes();
    if bytes.len() != 19
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b' '
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return false;
    }
    if bytes
        .iter()
        .enumerate()
        .any(|(index, byte)| ![4, 7, 10, 13, 16].contains(&index) && !byte.is_ascii_digit())
    {
        return false;
    }
    let number = |start: usize, end: usize| time[start..end].parse::<u32>().unwrap_or(u32::MAX);
    let (year, month, day) = (number(0, 4), number(5, 7), number(8, 10));
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    year > 0
        && day > 0
        && day <= days
        && number(11, 13) < 24
        && number(14, 16) < 60
        && number(17, 19) < 60
}
