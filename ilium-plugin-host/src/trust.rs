//! Raw publisher trust records and locked user-store transactions (PKG-214..247).
//! System records are supplied by the platform's administrator-owned-file loader.
//! Signature verification, trust decisions, and installed-plugin lifecycle remain
//! separate: CRUD never asserts that a plugin was stopped or installed.

use crate::signing::{PublicKey, TrustStore, TrustedKey};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::VerifyingKey;
use ilium_platform::{file_lock::ExclusiveFileLock, secure_fs};
use ilium_plugin_api::{manifest::Diagnostic, Code};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use unicode_normalization::UnicodeNormalization;

type Result<T> = std::result::Result<T, Diagnostic>;
fn error(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(Code::from_static(code), "", message)
}
fn io_error(error: std::io::Error) -> Diagnostic {
    self::error("trust.store-invalid", error.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AddedBy {
    User,
    Admin,
    Registry,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Trusted,
    Revoked,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RevocationReason {
    Compromised,
    Superseded,
    Retired,
}
impl std::fmt::Display for RevocationReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Compromised => "compromised",
            Self::Superseded => "superseded",
            Self::Retired => "retired",
        })
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revocation {
    pub reason: RevocationReason,
    pub at: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustEntry {
    pub key_id: String,
    pub publisher_id: String,
    pub publisher: String,
    pub public_key: String,
    pub added: String,
    pub added_by: AddedBy,
    pub status: Status,
    pub revoked: Option<Revocation>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustDocument {
    pub format: String,
    pub keys: Vec<TrustEntry>,
}
impl Default for TrustDocument {
    fn default() -> Self {
        Self {
            format: "ilium-trust/1".into(),
            keys: Vec::new(),
        }
    }
}

/// The public-file form, distinct from a signature or a trust decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublisherKey {
    pub format: String,
    pub publisher_id: String,
    pub publisher: String,
    pub algorithm: String,
    pub public_key: String,
    pub key_id: String,
    pub created: String,
}
impl PublisherKey {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let key: Self = serde_json::from_slice(bytes)
            .map_err(|e| error("trust.key-malformed", e.to_string()))?;
        key.validate()?;
        Ok(key)
    }
    fn validate(&self) -> Result<()> {
        if self.format != "ilium-pub/1" || self.algorithm != "ed25519" || !timestamp(&self.created)
        {
            return Err(error("trust.key-malformed", "invalid public-key document"));
        }
        validate_identity(
            &self.key_id,
            &self.publisher_id,
            &self.publisher,
            &self.public_key,
        )
    }
}
impl TrustDocument {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let document: Self = serde_json::from_slice(bytes)
            .map_err(|e| error("trust.store-invalid", e.to_string()))?;
        document.validate()?;
        Ok(document)
    }
    pub fn validate(&self) -> Result<()> {
        if self.format != "ilium-trust/1" {
            return Err(error("trust.store-invalid", "unknown trust-store format"));
        }
        let mut ids = BTreeMap::new();
        for entry in &self.keys {
            validate_identity(
                &entry.key_id,
                &entry.publisher_id,
                &entry.publisher,
                &entry.public_key,
            )?;
            if !timestamp(&entry.added)
                || entry.revoked.as_ref().is_some_and(|r| !timestamp(&r.at))
                || (entry.status == Status::Revoked) != entry.revoked.is_some()
            {
                return Err(error(
                    "trust.store-invalid",
                    "inconsistent status or invalid UTC timestamp",
                ));
            }
            if let Some(previous) = ids.insert(&entry.key_id, &entry.publisher_id) {
                return Err(error(
                    if previous != &entry.publisher_id {
                        "trust.publisher-conflict"
                    } else {
                        "trust.store-invalid"
                    },
                    "duplicate key identity",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    User,
    System,
    Both,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedKey {
    pub entry: TrustEntry,
    pub source: Source,
    /// The strongest currently effective revocation from either store.
    pub revocation: Option<Revocation>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustSnapshot {
    keys: Vec<ResolvedKey>,
}
impl TrustSnapshot {
    pub fn keys(&self) -> &[ResolvedKey] {
        &self.keys
    }

    pub fn resolve(user: &TrustDocument, system: &TrustDocument, now: &str) -> Result<Self> {
        user.validate()?;
        system.validate()?;
        if !timestamp(now) {
            return Err(error(
                "trust.store-invalid",
                "invalid current UTC timestamp",
            ));
        }
        let mut merged: BTreeMap<String, ResolvedKey> = BTreeMap::new();
        for (source, document) in [(Source::System, system), (Source::User, user)] {
            for entry in &document.keys {
                let revocation = entry
                    .revoked
                    .as_ref()
                    .filter(|r| r.at.as_str() <= now)
                    .cloned();
                if let Some(existing) = merged.get_mut(&entry.key_id) {
                    if existing.entry.publisher_id != entry.publisher_id {
                        return Err(error(
                            "trust.publisher-conflict",
                            "key is bound to conflicting publisher ids",
                        ));
                    }
                    if existing.entry.public_key != entry.public_key
                        || existing.entry.publisher.nfc().collect::<String>()
                            != entry.publisher.nfc().collect::<String>()
                    {
                        return Err(error(
                            "trust.store-invalid",
                            "conflicting metadata for one key",
                        ));
                    }
                    existing.source = Source::Both;
                    if let Some(revoked) = revocation {
                        if existing
                            .revocation
                            .as_ref()
                            .is_none_or(|r| r.reason != RevocationReason::Compromised)
                        {
                            existing.revocation = Some(revoked);
                        }
                    }
                } else {
                    merged.insert(
                        entry.key_id.clone(),
                        ResolvedKey {
                            entry: entry.clone(),
                            source,
                            revocation,
                        },
                    );
                }
            }
        }
        Ok(Self {
            keys: merged.into_values().collect(),
        })
    }
    /// Projection for the crypto verifier; raw metadata and revoked keys stay here.
    pub fn signing_store(&self) -> TrustStore {
        TrustStore {
            keys: self
                .keys
                .iter()
                .filter(|key| key.revocation.is_none())
                .filter_map(|key| {
                    URL_SAFE_NO_PAD
                        .decode(&key.entry.public_key)
                        .ok()
                        .map(|bytes| TrustedKey {
                            key_id: key.entry.key_id.clone(),
                            publisher_id: key.entry.publisher_id.clone(),
                            public_key: PublicKey(bytes),
                        })
                })
                .collect(),
        }
    }
    /// Checks an already verified signature's identity, never a free-text name alone.
    pub fn require_publisher(
        &self,
        key_id: &str,
        publisher_id: &str,
        display_name: &str,
    ) -> Result<&ResolvedKey> {
        let key = self
            .keys
            .iter()
            .find(|key| key.entry.key_id == key_id)
            .ok_or_else(|| error("trust.key-unknown", format!("key {key_id} is not trusted")))?;
        if let Some(revoked) = &key.revocation {
            return Err(error(
                "trust.key-revoked",
                format!("key {key_id} was revoked ({})", revoked.reason),
            ));
        }
        if key.entry.publisher_id != publisher_id {
            return Err(error(
                "trust.publisher-conflict",
                "signature publisher does not own this key",
            ));
        }
        if key.entry.publisher.nfc().collect::<String>() != display_name.nfc().collect::<String>() {
            return Err(error(
                "manifest.publisher-mismatch",
                "publisher display name does not match the key record",
            ));
        }
        Ok(key)
    }
    /// Verifies exact package bytes and rejects a currently revoked signer even
    /// if another trusted signature would otherwise satisfy the crypto verifier.
    pub fn verify_package(
        &self,
        package: &crate::package::PackageArchive,
    ) -> Result<crate::signing::Verified> {
        let signature = package
            .entries
            .iter()
            .find(|entry| entry.path == "signature")
            .ok_or_else(|| {
                error(
                    "trust.key-unknown",
                    "a trust check requires a signed package",
                )
            })?;
        crate::signing::validate_signature_document(&signature.bytes)?;
        let document: serde_json::Value = serde_json::from_slice(&signature.bytes)
            .map_err(|e| error("package.signature-format", e.to_string()))?;
        if let Some(signatures) = document["signatures"].as_array() {
            for signature in signatures {
                if self.keys.iter().any(|key| {
                    Some(key.entry.key_id.as_str()) == signature["key_id"].as_str()
                        && key.revocation.is_some()
                }) {
                    return Err(error(
                        "trust.key-revoked",
                        "package has a revoked signing key",
                    ));
                }
            }
        }
        let first = &document["signatures"][0];
        self.require_publisher(
            first["key_id"].as_str().unwrap_or_default(),
            first["publisher_id"].as_str().unwrap_or_default(),
            &package.manifest.publisher,
        )?;
        crate::signing::verify(package, &self.signing_store(), std::time::SystemTime::now())
    }
}

/// Caller supplies the complete installed-plugin inventory for this key. The
/// resulting lifecycle effects must be applied by the actual runtime owner.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrustChange {
    pub disabled_untrusted: Vec<String>,
    pub disabled_blocked: Vec<String>,
    pub retired_badge: Vec<String>,
}

/// A rejected user file contributes no keys; its warning must be shown by the
/// host. Mutation paths stay strict and never replace the rejected file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustLoad {
    pub snapshot: TrustSnapshot,
    pub warnings: Vec<Diagnostic>,
}

#[derive(Debug)]
pub struct TrustRepository {
    user_path: PathBuf,
    system: TrustDocument,
}
impl TrustRepository {
    /// `system` must be loaded from an administrator-owned file by the platform.
    /// This owner never writes it. Paths are explicit; no ambient HOME mutation.
    pub fn new(user_path: PathBuf, system: TrustDocument) -> Result<Self> {
        system.validate()?;
        if !user_path.is_absolute() || user_path.file_name().is_none() {
            return Err(error(
                "trust.store-invalid",
                "user trust path must be an absolute file path",
            ));
        }
        Ok(Self { user_path, system })
    }
    pub fn snapshot(&self, now: &str) -> Result<TrustSnapshot> {
        TrustSnapshot::resolve(&read_user(&self.user_path)?, &self.system, now)
    }
    /// Uses an actual host clock reading, including scheduled revocation boundaries.
    pub fn snapshot_at(&self, now: std::time::SystemTime) -> Result<TrustSnapshot> {
        self.snapshot(&utc_timestamp(now)?)
    }
    /// PKG-221 read path: reject the whole invalid user file, retain valid system
    /// authority, and return the warning explicitly for the host to display.
    pub fn load_snapshot_at(&self, now: std::time::SystemTime) -> Result<TrustLoad> {
        let now = utc_timestamp(now)?;
        let (user, warnings) = match read_user(&self.user_path) {
            Ok(user) => (user, Vec::new()),
            Err(warning) => (TrustDocument::default(), vec![warning]),
        };
        Ok(TrustLoad {
            snapshot: TrustSnapshot::resolve(&user, &self.system, &now)?,
            warnings,
        })
    }

    /// Must follow an explicit trust decision. A second publisher key requires
    /// the separately authenticated additional-key/rotation approval.
    pub fn add(&self, key: &PublisherKey, added: &str, additional_approved: bool) -> Result<()> {
        key.validate()?;
        if !timestamp(added) {
            return Err(error("trust.store-invalid", "invalid added time"));
        }
        self.transaction(|user| {
            let effective = TrustSnapshot::resolve(user, &self.system, added)?;
            if let Some(existing) = effective
                .keys
                .iter()
                .find(|entry| entry.entry.key_id == key.key_id)
            {
                if existing.entry.publisher_id != key.publisher_id {
                    return Err(error(
                        "trust.publisher-conflict",
                        "key belongs to another publisher",
                    ));
                }
                if existing.revocation.is_some()
                    || user
                        .keys
                        .iter()
                        .chain(&self.system.keys)
                        .any(|entry| entry.key_id == key.key_id && entry.revoked.is_some())
                {
                    return Err(error(
                        "trust.key-revoked",
                        "cannot overwrite a revocation with trust",
                    ));
                }
                effective.require_publisher(&key.key_id, &key.publisher_id, &key.publisher)?;
                return Ok(());
            }
            if !additional_approved
                && effective
                    .keys
                    .iter()
                    .any(|entry| entry.entry.publisher_id == key.publisher_id)
            {
                return Err(error(
                    "trust.additional-required",
                    "another key already exists for this publisher",
                ));
            }
            user.keys.push(TrustEntry {
                key_id: key.key_id.clone(),
                publisher_id: key.publisher_id.clone(),
                publisher: key.publisher.clone(),
                public_key: key.public_key.clone(),
                added: added.into(),
                added_by: AddedBy::User,
                status: Status::Trusted,
                revoked: None,
            });
            Ok(())
        })
    }
    pub fn remove(
        &self,
        key_id: &str,
        installed_plugins: &[String],
        force: bool,
    ) -> Result<TrustChange> {
        self.transaction(|user| {
            if self.system.keys.iter().any(|key| key.key_id == key_id) {
                return Err(error(
                    "trust.system-entry",
                    "cannot remove a system trust entry",
                ));
            }
            if !user.keys.iter().any(|key| key.key_id == key_id) {
                return Err(error("trust.key-unknown", "unknown user key"));
            }
            if !force && !installed_plugins.is_empty() {
                return Err(error(
                    "trust.in-use",
                    format!(
                        "key signs installed plugins: {}",
                        installed_plugins.join(", ")
                    ),
                ));
            }
            user.keys.retain(|key| key.key_id != key_id);
            Ok(TrustChange {
                disabled_untrusted: installed_plugins.to_vec(),
                ..TrustChange::default()
            })
        })
    }
    pub fn revoke(
        &self,
        key_id: &str,
        reason: RevocationReason,
        at: &str,
        installed_plugins: &[String],
    ) -> Result<TrustChange> {
        if !timestamp(at) {
            return Err(error("trust.store-invalid", "invalid revocation time"));
        }
        self.transaction(|user| {
            if !user.keys.iter().any(|key| key.key_id == key_id) {
                let mut key = self
                    .system
                    .keys
                    .iter()
                    .find(|key| key.key_id == key_id)
                    .cloned()
                    .ok_or_else(|| error("trust.key-unknown", "cannot revoke an unknown key"))?;
                key.added_by = AddedBy::User;
                user.keys.push(key);
            }
            let key = user
                .keys
                .iter_mut()
                .find(|key| key.key_id == key_id)
                .ok_or_else(|| error("trust.key-unknown", "unknown key"))?;
            // A weaker second request must never erase an existing compromise.
            if key
                .revoked
                .as_ref()
                .is_some_and(|r| r.reason == RevocationReason::Compromised)
                && reason != RevocationReason::Compromised
            {
                return Err(error(
                    "trust.key-revoked",
                    "cannot weaken a compromised-key revocation",
                ));
            }
            let effective_at = key
                .revoked
                .as_ref()
                .map(|r| r.at.as_str())
                .unwrap_or(at)
                .min(at)
                .to_owned();
            key.status = Status::Revoked;
            key.revoked = Some(Revocation {
                reason,
                at: effective_at,
            });
            Ok(if reason == RevocationReason::Compromised {
                TrustChange {
                    disabled_blocked: installed_plugins.to_vec(),
                    ..TrustChange::default()
                }
            } else {
                TrustChange {
                    retired_badge: installed_plugins.to_vec(),
                    ..TrustChange::default()
                }
            })
        })
    }
    /// The specified raw-store schema does not retain the public file's creation
    /// timestamp. Never fabricate it from `added`; the caller must supply it.
    pub fn export(&self, _key_id: &str) -> Result<Vec<u8>> {
        Err(error(
            "trust.context-required",
            "public-key creation time is not retained in trust records",
        ))
    }
    pub fn export_with_created(&self, key_id: &str, created: &str) -> Result<Vec<u8>> {
        if !timestamp(created) {
            return Err(error(
                "trust.key-malformed",
                "invalid key creation timestamp",
            ));
        }
        let user = read_user(&self.user_path)?;
        TrustSnapshot::resolve(&user, &self.system, "9999-12-31 23:59:59")?;
        let key = self
            .system
            .keys
            .iter()
            .chain(&user.keys)
            .find(|key| key.key_id == key_id)
            .ok_or_else(|| error("trust.key-unknown", "unknown key"))?;
        let public = PublisherKey {
            format: "ilium-pub/1".into(),
            algorithm: "ed25519".into(),
            key_id: key.key_id.clone(),
            publisher_id: key.publisher_id.clone(),
            publisher: key.publisher.clone(),
            public_key: key.public_key.clone(),
            created: created.into(),
        };
        let mut bytes = serde_json::to_vec_pretty(&public)
            .map_err(|e| error("trust.store-invalid", e.to_string()))?;
        bytes.push(b'\n');
        Ok(bytes)
    }
    fn transaction<T>(&self, change: impl FnOnce(&mut TrustDocument) -> Result<T>) -> Result<T> {
        let parent = self
            .user_path
            .parent()
            .ok_or_else(|| error("trust.store-invalid", "missing parent"))?;
        secure_fs::create_private_directory(parent).map_err(io_error)?;
        let mut lock_path = self.user_path.as_os_str().to_os_string();
        lock_path.push(".lock");
        let _lock = ExclusiveFileLock::acquire(Path::new(&lock_path)).map_err(io_error)?;
        // Reload only after taking the stable sibling lock: concurrent writers
        // cannot replace each other's snapshots. Invalid existing data is never overwritten.
        let mut user = read_user(&self.user_path)?;
        TrustSnapshot::resolve(&user, &self.system, "9999-12-31 23:59:59")?;
        let result = change(&mut user)?;
        user.validate()?;
        write_atomic(&self.user_path, &user)?;
        Ok(result)
    }
}

fn read_user(path: &Path) -> Result<TrustDocument> {
    let Some(parent) = path.parent() else {
        return Err(error("trust.store-invalid", "missing parent"));
    };
    let directory = match secure_fs::NoFollowDirectory::open_root(parent) {
        Ok(directory) => directory,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(TrustDocument::default()),
        Err(e) => return Err(io_error(e)),
    };
    let mut file = match directory.open_regular(
        path.file_name()
            .ok_or_else(|| error("trust.store-invalid", "missing filename"))?,
    ) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(TrustDocument::default()),
        Err(e) => return Err(io_error(e)),
    };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(io_error)?;
    TrustDocument::parse(&bytes)
}
fn write_atomic(path: &Path, document: &TrustDocument) -> Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .ok_or_else(|| error("trust.store-invalid", "missing parent"))?;
    let (temporary, mut file) = loop {
        let temporary = parent.join(format!(
            ".ilium-trust-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        if temporary == path {
            continue;
        }
        match secure_fs::private_open_options()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(io_error(e)),
        }
    };
    let result = (|| {
        let mut bytes = serde_json::to_vec_pretty(document)
            .map_err(|e| error("trust.store-invalid", e.to_string()))?;
        bytes.push(b'\n');
        file.write_all(&bytes).map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        drop(file);
        secure_fs::replace_file_durably(&temporary, path).map_err(io_error)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}
fn validate_identity(
    key_id: &str,
    publisher_id: &str,
    publisher: &str,
    public_key: &str,
) -> Result<()> {
    if matches!(publisher_id, "ilium" | "core" | "local" | "test") {
        return Err(error("trust.publisher-reserved", "reserved publisher id"));
    }
    let id_valid = !publisher_id.is_empty()
        && publisher_id.len() <= 64
        && publisher_id.split('.').count() <= 4
        && publisher_id.split('.').all(|part| {
            part.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
                && part.split('-').all(|word| {
                    !word.is_empty()
                        && word
                            .bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                })
        });
    let display_valid = (1..=64).contains(&publisher.chars().count())
        && !publisher
            .chars()
            .any(|c| c.is_control() || matches!(c,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}'));
    let bytes: [u8; 32] = URL_SAFE_NO_PAD
        .decode(public_key)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| {
            error(
                "trust.key-malformed",
                "public key must be unpadded base64url encoding of 32 bytes",
            )
        })?;
    let valid_point = VerifyingKey::from_bytes(&bytes)
        .ok()
        .is_some_and(|key| !key.is_weak() && key.to_edwards().compress().to_bytes() == bytes);
    if !id_valid
        || !display_valid
        || !valid_point
        || format!("{:x}", Sha256::digest(bytes)) != key_id
    {
        return Err(error(
            "trust.key-malformed",
            "invalid key, fingerprint or publisher identity",
        ));
    }
    Ok(())
}
fn timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 19
        || [4, 7, 10, 13, 16].into_iter().any(|i| {
            bytes[i]
                != match i {
                    4 | 7 => b'-',
                    10 => b' ',
                    _ => b':',
                }
        })
        || bytes
            .iter()
            .enumerate()
            .any(|(i, b)| ![4, 7, 10, 13, 16].contains(&i) && !b.is_ascii_digit())
    {
        return false;
    }
    let number = |start, end| {
        value
            .get(start..end)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0)
    };
    let (year, month, day) = (number(0, 4), number(5, 7), number(8, 10));
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        _ => 0,
    };
    year > 0
        && day > 0
        && day <= days
        && number(11, 13) < 24
        && number(14, 16) < 60
        && number(17, 19) < 60
}

/// Formats a supplied host time in the trust contract's UTC form. No ambient clock
/// is read here, so scheduled revocations can be tested at the exact boundary.
pub fn utc_timestamp(now: std::time::SystemTime) -> Result<String> {
    let seconds = now
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| {
            error(
                "trust.store-invalid",
                "host time predates supported Unix epoch",
            )
        })?
        .as_secs();
    let mut days = seconds / 86400;
    let mut year = 1970u32;
    loop {
        let leap =
            year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
        let length = if leap { 366 } else { 365 };
        if days < length {
            break;
        }
        days -= length;
        year += 1;
        if year > 9999 {
            return Err(error(
                "trust.store-invalid",
                "host time exceeds trust timestamp range",
            ));
        }
    }
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let lengths = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 0;
    while month < 11 && days >= lengths[month] {
        days -= lengths[month];
        month += 1;
    }
    let clock = seconds % 86400;
    Ok(format!(
        "{year:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        month + 1,
        days + 1,
        clock / 3600,
        (clock % 3600) / 60,
        clock % 60
    ))
}

/// What the host did with a restored `trust.json` whose keyed MAC could not be verified (HST-959).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoredStoreOutcome {
    /// The kept copy, `trust.json.tampered-<YYYYMMDD-HHmmss>`.
    pub kept_as: String,
    /// `trust.store-tampered` for T2 and T3 plugins; T1 plugins are never refused by it.
    pub refused_code: Option<&'static str>,
    /// The instruction shown to the user.
    pub message: String,
}

/// Handles a restored trust store on a machine where the MAC cannot be verified. `stamp` is the
/// UTC time as `YYYYMMDD-HHmmss`.
pub fn restored_store_without_mac(tier: ilium_plugin_api::Tier, stamp: &str) -> RestoredStoreOutcome {
    let refused_code = match tier {
        ilium_plugin_api::Tier::T1 => None,
        _ => Some("trust.store-tampered"),
    };
    RestoredStoreOutcome {
        kept_as: format!("trust.json.tampered-{stamp}"),
        refused_code,
        message: "Re-establish each publisher with `ilium plugin trust add <publisher.pub>`".to_owned(),
    }
}
