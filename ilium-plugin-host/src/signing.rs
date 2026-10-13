//! Package signing, trust and verification (reference-packaging.md PKG-200..260).

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
pub fn verify(package: &PackageArchive, trust: &TrustStore, now: SystemTime) -> Result<Verified, Diagnostic> {
    unimplemented!("host::signing::verify")
}

/// JSON canonicalisation (RFC 8785) of a value (reference-packaging.md PKG-205).
pub fn jcs(value: &serde_json::Value) -> Result<String, Diagnostic> {
    unimplemented!("host::signing::jcs")
}
