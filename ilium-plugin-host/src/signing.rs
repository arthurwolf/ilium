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
    pub fn from_seed(seed: [u8; 32]) -> SigningKey {
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
    pub fn default_path(env: &BTreeMap<String, String>, home: &Path) -> PathBuf {
        unimplemented!("host::signing::TrustStore::default_path")
    }

    /// Writes the store atomically with mode 0600 (reference-packaging.md PKG-267).
    pub fn save_atomic(&self, path: &Path) -> std::io::Result<()> {
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
pub fn sign(package: &[u8], key: &SigningKey, signed_at: SignedAt) -> Result<Vec<u8>, Diagnostic> {
    unimplemented!("host::signing::sign")
}

/// Verifies a package against a trust store at a time (reference-packaging.md PKG-210).
pub fn verify(
    package: &PackageArchive,
    trust: &TrustStore,
    now: SystemTime,
) -> Result<Verified, Diagnostic> {
    unimplemented!("host::signing::verify")
}

/// JSON canonicalisation (RFC 8785) of a value (reference-packaging.md PKG-205).
pub fn jcs(value: &serde_json::Value) -> Result<String, Diagnostic> {
    unimplemented!("host::signing::jcs")
}
