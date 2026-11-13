//! Private versioned decision stores (SEC-109, SEC-112..114, SEC-118..119).
//! Callers supply the user-state directory; these APIs never resolve project paths.

use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ilium_platform::{file_lock::ExclusiveFileLock, secure_fs};
use ilium_plugin_api::{
    grants::{check_grant, is_high_risk, Grant},
    side::Side,
    tier::Tier,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// Why a store could not be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// A newer format must be left untouched.
    #[error("unsupported store format version {0}")]
    UnsupportedVersion(u32),
    /// A filesystem operation failed.
    #[error("store I/O failed: {0}")]
    Io(String),
    /// The document or requested identity is invalid.
    #[error("invalid store: {0}")]
    Corrupt(String),
    /// Another writer changed the snapshot. Reopen explicitly rather than lose data.
    #[error("store changed concurrently; reopen before retrying")]
    Conflict,
}
fn io(error: std::io::Error) -> StoreError {
    StoreError::Io(error.to_string())
}
fn invalid(message: &str) -> StoreError {
    StoreError::Corrupt(message.into())
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn plugin_id(plugin: &str) -> Result<(), StoreError> {
    ilium_plugin_api::ids::parse_plugin_id(plugin)
        .map(|_| ())
        .map_err(|_| invalid("invalid plugin identity"))
}
fn fingerprint(key: &str) -> bool {
    key.len() == 64
        && key
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn tier(text: &str) -> Result<Tier, StoreError> {
    match text {
        "t1" | "T1" => Ok(Tier::T1),
        "t2" | "T2" => Ok(Tier::T2),
        "t3" | "T3" => Ok(Tier::T3),
        _ => Err(invalid("invalid tier")),
    }
}
fn identity(key: &str, text: &str) -> Result<Tier, StoreError> {
    let tier = tier(text)?;
    if !(fingerprint(key) || tier == Tier::T3 && key.is_empty()) {
        return Err(invalid("invalid publisher fingerprint"));
    }
    Ok(tier)
}
fn tier_text(tier: Tier) -> &'static str {
    match tier {
        Tier::T1 => "t1",
        Tier::T2 => "t2",
        Tier::T3 => "t3",
    }
}

// A duplicate key is ambiguous security state, including duplicate nested grant keys.
struct Strict(Value);
impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Strict;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("unambiguous JSON")
            }
            fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Strict, E> {
                Ok(Strict(value.into()))
            }
            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Strict, E> {
                Ok(Strict(value.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Strict, E> {
                Ok(Strict(value.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Strict, E> {
                serde_json::Number::from_f64(value)
                    .map(|n| Strict(n.into()))
                    .ok_or_else(|| E::custom("nonfinite number"))
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Strict, E> {
                Ok(Strict(value.into()))
            }
            fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Strict, E> {
                Ok(Strict(value.into()))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_none<E: serde::de::Error>(self) -> Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Strict, A::Error> {
                let mut values = Vec::new();
                while let Some(Strict(value)) = seq.next_element()? {
                    values.push(value);
                }
                Ok(Strict(values.into()))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Strict, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(serde::de::Error::custom("duplicate object key"));
                    }
                    let Strict(value) = map.next_value()?;
                    values.insert(key, value);
                }
                Ok(Strict(values.into()))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}
fn parse(bytes: &[u8]) -> Result<Value, StoreError> {
    serde_json::from_slice::<Strict>(bytes)
        .map(|value| value.0)
        .map_err(|error| StoreError::Corrupt(error.to_string()))
}

/// Startup notices must be delivered through the host's actual audit/UI context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreNotice {
    /// Original malformed bytes remain in this quarantine file.
    Unreadable(PathBuf),
    /// The supplied last-write audit hash differs from the file's actual hash.
    ExternalEdit,
    /// A persisted high-risk grant lacked a matching actual user-decision record.
    UnprovedHighGrant {
        plugin: String,
        grant: String,
        side: Side,
    },
}
/// Evidence extracted by the host from verified audit user-decision records.
/// Constructing this value is a trusted host operation, never a plugin input.
#[derive(Debug, Clone, Default)]
pub struct GrantEvidence {
    /// Actual hash recorded at the last server write, if available.
    pub last_write_sha256: Option<String>,
    /// Exact (plugin, key, lowercase tier, side, grant, epoch) current approvals.
    pub approvals: BTreeSet<(String, String, String, String, String, u64)>,
}
#[derive(Debug)]
struct Document {
    path: PathBuf,
    value: Value,
    sha256: String,
    notices: Vec<StoreNotice>,
}
impl Document {
    fn open(
        root: &Path,
        name: &str,
        empty: Value,
        validate: fn(&Value) -> Result<(), StoreError>,
    ) -> Result<Self, StoreError> {
        if !root.is_absolute()
            || root.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
        {
            return Err(invalid(
                "store root must be an absolute user-state directory",
            ));
        }
        secure_fs::create_private_directory(root).map_err(io)?;
        let path = root.join(name);
        let _lock = ExclusiveFileLock::acquire(&root.join("stores.lock")).map_err(io)?;
        let mut notices = Vec::new();
        let bytes = match read(&path) {
            Ok(bytes) => Some(bytes),
            Err(StoreError::Io(_)) if !path.try_exists().map_err(io)? => None,
            Err(error) => return Err(error),
        };
        let value = if let Some(bytes) = bytes {
            let parsed = parse(&bytes).and_then(|value| {
                let version = value
                    .get("version")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| invalid("missing store version"))?;
                if version > 1 {
                    return Err(StoreError::UnsupportedVersion(
                        u32::try_from(version).unwrap_or(u32::MAX),
                    ));
                }
                if version != 1 {
                    return Err(invalid("invalid store version"));
                }
                validate(&value)?;
                Ok(value)
            });
            match parsed {
                Ok(value) => {
                    secure_fs::restrict_file_to_owner(&path).map_err(io)?;
                    return Ok(Self {
                        path,
                        value,
                        sha256: hash(&bytes),
                        notices,
                    });
                }
                Err(error @ StoreError::UnsupportedVersion(_)) => return Err(error),
                Err(_) => {
                    let base = format!("{name}.corrupt-{}", timestamp(true)?);
                    let mut collision = 0_u64;
                    let quarantine = loop {
                        let quarantine = root.join(if collision == 0 {
                            base.clone()
                        } else {
                            format!("{base}.{collision}")
                        });
                        // Never overwrite an earlier recovery, even within the same second.
                        match fs::hard_link(&path, &quarantine) {
                            Ok(()) => break quarantine,
                            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                                collision = collision
                                    .checked_add(1)
                                    .ok_or_else(|| invalid("quarantine names exhausted"))?;
                            }
                            Err(error) => return Err(io(error)),
                        }
                    };
                    // Make the recovery name durable before unlinking its original name.
                    secure_fs::sync_parent_directory_if_supported(&quarantine).map_err(io)?;
                    fs::remove_file(&path).map_err(io)?;
                    secure_fs::sync_parent_directory_if_supported(&path).map_err(io)?;
                    notices.push(StoreNotice::Unreadable(quarantine));
                    empty
                }
            }
        } else {
            empty
        };
        let bytes = encode(&value)?;
        atomic_write(&path, &bytes)?;
        Ok(Self {
            path,
            value,
            sha256: hash(&bytes),
            notices,
        })
    }
    fn update(
        &mut self,
        change: impl FnOnce(&mut Value) -> Result<(), StoreError>,
    ) -> Result<(), StoreError> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| invalid("store has no parent"))?;
        let _lock = ExclusiveFileLock::acquire(&parent.join("stores.lock")).map_err(io)?;
        if hash(&read(&self.path)?) != self.sha256 {
            return Err(StoreError::Conflict);
        }
        let mut candidate = self.value.clone();
        change(&mut candidate)?;
        let bytes = encode(&candidate)?;
        atomic_write(&self.path, &bytes)?;
        self.sha256 = hash(&bytes);
        self.value = candidate;
        Ok(())
    }
    fn forget(&mut self, plugin: &str) -> Result<(), StoreError> {
        plugin_id(plugin)?;
        self.update(|value| {
            value["plugins"]
                .as_object_mut()
                .ok_or_else(|| invalid("plugins must be an object"))?
                .remove(plugin);
            Ok(())
        })
    }
}
fn read(path: &Path) -> Result<Vec<u8>, StoreError> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid("missing store directory"))?;
    let directory = secure_fs::NoFollowDirectory::open_root(parent).map_err(io)?;
    let mut file = directory
        .open_regular(
            path.file_name()
                .ok_or_else(|| invalid("missing store name"))?,
        )
        .map_err(io)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(io)?;
    Ok(bytes)
}

fn encode(value: &Value) -> Result<Vec<u8>, StoreError> {
    let mut bytes =
        serde_json::to_vec(value).map_err(|error| StoreError::Corrupt(error.to_string()))?;
    bytes.push(b'\n');
    Ok(bytes)
}
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .ok_or_else(|| invalid("missing store directory"))?;
    let (temporary, mut file) = loop {
        let temporary = parent.join(format!(
            ".store-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match secure_fs::private_open_options()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io(error)),
        }
    };
    let result = (|| {
        secure_fs::restrict_open_file_to_owner(&file).map_err(io)?;
        file.write_all(bytes).map_err(io)?;
        file.sync_all().map_err(io)?;
        drop(file);
        secure_fs::replace_file_durably(&temporary, path).map_err(io)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn plugins(value: &Value) -> Result<&serde_json::Map<String, Value>, StoreError> {
    value
        .get("plugins")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("plugins must be an object"))
}
fn validate_grants(value: &Value) -> Result<(), StoreError> {
    for (plugin, row) in plugins(value)? {
        plugin_id(plugin)?;
        let key = row["key"].as_str().ok_or_else(|| invalid("missing key"))?;
        let tier = identity(
            key,
            row["tier"]
                .as_str()
                .ok_or_else(|| invalid("missing tier"))?,
        )?;
        if row["epoch"].as_u64().is_none() || row["installed"].as_str().is_none() {
            return Err(invalid("missing epoch or installed timestamp"));
        }
        for (side, grants) in row["grants"]
            .as_object()
            .ok_or_else(|| invalid("grants must be per-side"))?
        {
            let side: Side = side.parse().map_err(|_| invalid("invalid grant side"))?;
            let mut seen = BTreeSet::new();
            let grants = grants
                .as_array()
                .ok_or_else(|| invalid("grants must be an array"))?;
            if grants.len() > 256 {
                return Err(invalid("more than 256 grants for one side"));
            }
            for grant in grants {
                let grant = Grant::parse(
                    grant
                        .as_str()
                        .ok_or_else(|| invalid("grant must be a string"))?,
                )
                .map_err(|_| invalid("invalid grant"))?;
                validate_grant(&grant, tier, side)?;
                if !seen.insert(grant) {
                    return Err(invalid("duplicate grant"));
                }
            }
        }
    }
    Ok(())
}
fn validate_grant(grant: &Grant, tier: Tier, side: Side) -> Result<(), StoreError> {
    Grant::parse(grant.as_str()).map_err(|_| invalid("invalid grant"))?;
    check_grant(grant, tier, side).map_err(|_| invalid("grant unavailable for tier or side"))
}

/// Snapshot of per-side grants. `set`/`load` deliberately select the server side.
#[derive(Debug)]
pub struct GrantStore {
    document: Document,
    effective: Value,
}
impl GrantStore {
    /// Opens without user-decision evidence: persisted high-risk grants fail closed.
    pub fn open(root: &Path) -> Result<Self, StoreError> {
        Self::open_with_evidence(root, &GrantEvidence::default())
    }
    /// Opens with actual verified audit evidence supplied by the host (SEC-113).
    pub fn open_with_evidence(root: &Path, evidence: &GrantEvidence) -> Result<Self, StoreError> {
        let mut document = Document::open(
            root,
            "grants.json",
            json!({"version":1,"plugins":{}}),
            validate_grants,
        )?;
        if evidence
            .last_write_sha256
            .as_ref()
            .is_some_and(|expected| expected != &document.sha256)
        {
            document.notices.push(StoreNotice::ExternalEdit);
        }
        let mut effective = document.value.clone();
        for (plugin, row) in effective["plugins"]
            .as_object_mut()
            .ok_or_else(|| invalid("plugins must be an object"))?
        {
            let epoch = row["epoch"]
                .as_u64()
                .ok_or_else(|| invalid("invalid epoch"))?;
            let key = row["key"].as_str().unwrap_or_default().to_owned();
            let tier = row["tier"]
                .as_str()
                .unwrap_or_default()
                .to_ascii_lowercase();
            for (side, grants) in row["grants"]
                .as_object_mut()
                .ok_or_else(|| invalid("grants must be per-side"))?
            {
                let side_value: Side = side.parse().map_err(|_| invalid("invalid side"))?;
                grants
                    .as_array_mut()
                    .ok_or_else(|| invalid("invalid grants"))?
                    .retain(|value| {
                        let text = value.as_str().unwrap_or_default();
                        let high = is_high_risk(text);
                        let admitted = !high
                            || evidence.approvals.contains(&(
                                plugin.clone(),
                                key.clone(),
                                tier.clone(),
                                side.clone(),
                                text.into(),
                                epoch,
                            ));
                        if !admitted {
                            document.notices.push(StoreNotice::UnprovedHighGrant {
                                plugin: plugin.clone(),
                                grant: text.into(),
                                side: side_value,
                            });
                        }
                        admitted
                    });
            }
        }
        Ok(Self {
            document,
            effective,
        })
    }
    /// Begin a host install decision transaction. Lock order: installed.lock,
    /// then stores.lock. The snapshot is checked before any stage is published.
    pub fn install_transaction(&mut self) -> Result<InstallGrantTransaction<'_>, StoreError> {
        let parent = self
            .document
            .path
            .parent()
            .ok_or_else(|| invalid("store has no parent"))?;
        let lock = ExclusiveFileLock::acquire(&parent.join("stores.lock")).map_err(io)?;
        let before_bytes = read(&self.document.path)?;
        if hash(&before_bytes) != self.document.sha256 {
            return Err(StoreError::Conflict);
        }
        let before_effective = self.effective.clone();
        Ok(InstallGrantTransaction {
            store: self,
            before_bytes,
            before_effective,
            changed: false,
            _lock: lock,
        })
    }

    /// Records an actual host-approved server decision. Never call with plugin-supplied approvals.
    pub fn set(
        &mut self,
        plugin: &str,
        key: &str,
        tier: &str,
        grants: &[Grant],
    ) -> Result<(), StoreError> {
        self.set_for_side(plugin, key, tier, Side::Server, grants)
    }
    /// Records a side's actual decision, preserving other sides only for identical key/tier.
    /// The host must reset remembered consents before changing identity (SEC-109).
    pub fn set_for_side(
        &mut self,
        plugin: &str,
        key: &str,
        tier_name: &str,
        side: Side,
        grants: &[Grant],
    ) -> Result<(), StoreError> {
        plugin_id(plugin)?;
        let tier = identity(key, tier_name)?;
        if grants.len() > 256 {
            return Err(invalid("more than 256 grants for one side"));
        }
        let mut unique = BTreeSet::new();
        for grant in grants {
            validate_grant(grant, tier, side)?;
            if !unique.insert(grant.as_str()) {
                return Err(invalid("duplicate grant"));
            }
        }
        let installed = timestamp(false)?;
        // Start with the effective snapshot: unproved high-risk disk entries cannot be resurrected by an unrelated write.
        let effective = self.effective.clone();
        self.document.update(|value| {
            value["plugins"] = effective["plugins"].clone();
            let rows = value["plugins"]
                .as_object_mut()
                .ok_or_else(|| invalid("invalid plugins"))?;
            let old = rows.get(plugin);
            let epoch = old
                .and_then(|row| row["epoch"].as_u64())
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| invalid("grant epoch exhausted"))?;
            let same = old.is_some_and(|row| row["key"] == key && row["tier"] == tier_text(tier));
            let mut row = old.cloned().unwrap_or_else(|| json!({}));
            if !same {
                row["grants"] = json!({});
                row["installed"] = json!(installed);
            }
            row["epoch"] = json!(epoch);
            row["key"] = json!(key);
            row["tier"] = json!(tier_text(tier));
            row["grants"][side.as_str()] =
                json!(grants.iter().map(Grant::as_str).collect::<Vec<_>>());
            rows.insert(plugin.into(), row);
            Ok(())
        })?;
        self.effective = self.document.value.clone();
        Ok(())
    }
    /// Loads server grants only when the full identity matches.
    pub fn load(&self, plugin: &str, key: &str, tier: &str) -> Result<Vec<Grant>, StoreError> {
        self.load_for_side(plugin, key, tier, Side::Server)
    }
    /// Loads only the requested execution side.
    pub fn load_for_side(
        &self,
        plugin: &str,
        key: &str,
        tier_name: &str,
        side: Side,
    ) -> Result<Vec<Grant>, StoreError> {
        plugin_id(plugin)?;
        let tier = identity(key, tier_name)?;
        let row = &self.effective["plugins"][plugin];
        if row["key"] != key || row["tier"] != tier_text(tier) {
            return Ok(Vec::new());
        }
        row["grants"][side.as_str()]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .map(|value| {
                        Grant::parse(value.as_str().unwrap_or_default())
                            .map_err(|_| invalid("invalid stored grant"))
                    })
                    .collect()
            })
            .unwrap_or_else(|| Ok(Vec::new()))
    }
    /// The persisted file path.
    pub fn path(&self) -> &Path {
        &self.document.path
    }
    /// Actual committed bytes' SHA-256, for the host's write-audit receipt.
    pub fn sha256(&self) -> &str {
        &self.document.sha256
    }
    /// Startup notices; opening alone cannot fabricate an audit actor/session.
    pub fn notices(&self) -> &[StoreNotice] {
        &self.document.notices
    }
    /// Revokes an exact grant across all sides and advances the persisted epoch.
    pub fn revoke(&mut self, plugin: &str, grant: &str) -> Result<(), StoreError> {
        plugin_id(plugin)?;
        Grant::parse(grant).map_err(|_| invalid("invalid grant"))?;
        self.document.update(|value| {
            let Some(row) = value["plugins"]
                .as_object_mut()
                .and_then(|rows| rows.get_mut(plugin))
            else {
                return Ok(());
            };
            let mut changed = false;
            for values in row["grants"]
                .as_object_mut()
                .ok_or_else(|| invalid("invalid grants"))?
                .values_mut()
            {
                let values = values
                    .as_array_mut()
                    .ok_or_else(|| invalid("invalid grants"))?;
                let before = values.len();
                values.retain(|value| value != grant);
                changed |= before != values.len();
            }
            if changed {
                row["epoch"] = json!(row["epoch"]
                    .as_u64()
                    .ok_or_else(|| invalid("invalid epoch"))?
                    .checked_add(1)
                    .ok_or_else(|| invalid("grant epoch exhausted"))?);
            }
            Ok(())
        })?;
        // Keep startup filtering; never replace it with unproved disk grants.
        if let Some(sides) = self
            .effective
            .get_mut("plugins")
            .and_then(|plugins| plugins.get_mut(plugin))
            .and_then(|row| row.get_mut("grants"))
            .and_then(Value::as_object_mut)
        {
            for values in sides.values_mut() {
                if let Some(values) = values.as_array_mut() {
                    values.retain(|value| value != grant);
                }
            }
        }
        if let Some(row) = self.effective["plugins"]
            .as_object_mut()
            .and_then(|rows| rows.get_mut(plugin))
        {
            row["epoch"] = self.document.value["plugins"][plugin]["epoch"].clone();
        }
        Ok(())
    }
    /// Forgets grant decisions; the host must also forget consents and invalidate running contributions.
    pub fn forget(&mut self, plugin: &str) -> Result<(), StoreError> {
        self.document.forget(plugin)?;
        self.effective["plugins"]
            .as_object_mut()
            .ok_or_else(|| invalid("invalid plugins"))?
            .remove(plugin);
        Ok(())
    }
}

/// One all-side install decision; the lock protects snapshot, replacement and
/// explicit rollback. Callers must persist actual approval audit evidence for the
/// returned epoch; a package cannot create that evidence itself.
pub struct InstallGrantTransaction<'a> {
    store: &'a mut GrantStore,
    before_bytes: Vec<u8>,
    before_effective: Value,
    changed: bool,
    _lock: ExclusiveFileLock,
}
impl InstallGrantTransaction<'_> {
    pub fn replace_all_sides(
        &mut self,
        plugin: &str,
        key: &str,
        tier_name: &str,
        grants: &ilium_plugin_api::manifest::Capabilities,
    ) -> Result<u64, StoreError> {
        plugin_id(plugin)?;
        let tier = identity(key, tier_name)?;
        for (side, entries) in [
            (Side::Server, &grants.server),
            (Side::Client, &grants.client),
            (Side::Scene, &grants.scene),
        ] {
            if entries.len() > 256 {
                return Err(invalid("more than 256 grants for one side"));
            }
            let mut unique = BTreeSet::new();
            for grant in entries {
                validate_grant(grant, tier, side)?;
                if !unique.insert(grant.as_str()) {
                    return Err(invalid("duplicate grant"));
                }
            }
        }
        let mut value = self.store.effective.clone();
        let old = &value["plugins"][plugin];
        let epoch = old["epoch"]
            .as_u64()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| invalid("grant epoch exhausted"))?;
        let installed = if old["key"] == key && old["tier"] == tier_text(tier) {
            old["installed"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or(timestamp(false)?)
        } else {
            timestamp(false)?
        };
        value["plugins"][plugin] = json!({"key":key,"tier":tier_text(tier),"epoch":epoch,"installed":installed,
            "grants":{"server":grants.server.iter().map(Grant::as_str).collect::<Vec<_>>(),
            "client":grants.client.iter().map(Grant::as_str).collect::<Vec<_>>(),
            "scene":grants.scene.iter().map(Grant::as_str).collect::<Vec<_>>()}});
        validate_grants(&value)?;
        let bytes = encode(&value)?;
        self.changed = true;
        atomic_write(&self.store.document.path, &bytes)?;
        self.store.document.sha256 = hash(&bytes);
        self.store.document.value = value.clone();
        self.store.effective = value;
        Ok(epoch)
    }
    pub fn rollback(&mut self) -> Result<(), StoreError> {
        if !self.changed {
            return Ok(());
        }
        atomic_write(&self.store.document.path, &self.before_bytes)?;
        self.store.document.value = parse(&self.before_bytes)?;
        self.store.document.sha256 = hash(&self.before_bytes);
        self.store.effective = self.before_effective.clone();
        self.changed = false;
        Ok(())
    }
    pub fn sha256(&self) -> &str {
        self.store.sha256()
    }
}

/// One remembered consent decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentDecision {
    /// Canonical kind and target, separated by a colon (or `geocode`).
    pub grant: String,
    /// Whether the actual user answer was allow.
    pub allowed: bool,
}
fn consent_target(text: &str) -> Result<(&str, Option<&str>), StoreError> {
    if text == "geocode" {
        return Ok((text, None));
    }
    let (kind, target) = text
        .split_once(':')
        .ok_or_else(|| invalid("consent requires a kind and canonical target"))?;
    if !matches!(kind, "open-external" | "http-hosts")
        || target.is_empty()
        || target.chars().any(char::is_control)
    {
        return Err(invalid("invalid consent target"));
    }
    Ok((kind, Some(target)))
}
fn decision(value: &Value) -> Result<(bool, u64), StoreError> {
    let text = value
        .as_str()
        .ok_or_else(|| invalid("invalid consent decision"))?;
    if text == "allow" {
        return Ok((true, 0));
    }
    let count = text
        .strip_prefix("deny:")
        .and_then(|text| text.parse::<u64>().ok())
        .filter(|count| *count > 0)
        .ok_or_else(|| invalid("invalid denial count"))?;
    Ok((false, count))
}
fn validate_consents(value: &Value) -> Result<(), StoreError> {
    for (plugin, row) in plugins(value)? {
        plugin_id(plugin)?;
        for (kind, targets) in row
            .as_object()
            .ok_or_else(|| invalid("invalid consent row"))?
        {
            if kind == "geocode" {
                decision(targets)?;
                continue;
            }
            if !matches!(kind.as_str(), "open-external" | "http-hosts") {
                return Err(invalid("unknown consent kind"));
            }
            for (target, value) in targets
                .as_object()
                .ok_or_else(|| invalid("invalid target map"))?
            {
                consent_target(&format!("{kind}:{target}"))?;
                decision(value)?;
            }
        }
    }
    Ok(())
}
/// Actual persisted consent answers; timeout/pending state belongs to `ConsentGate`.
#[derive(Debug)]
pub struct ConsentStore {
    document: Document,
}
impl ConsentStore {
    /// Opens the private snapshot; subsequent external edits are not ingested.
    pub fn open(root: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            document: Document::open(
                root,
                "consents.json",
                json!({"version":1,"plugins":{}}),
                validate_consents,
            )?,
        })
    }
    /// Records an actual answer. The caller resolves canonical origins/hosts before calling.
    pub fn record(&mut self, plugin: &str, grant: &str, allowed: bool) -> Result<(), StoreError> {
        plugin_id(plugin)?;
        let (kind, target) = consent_target(grant)?;
        self.document.update(|value| {
            let rows = value["plugins"]
                .as_object_mut()
                .ok_or_else(|| invalid("invalid plugins"))?;
            let row = rows.entry(plugin).or_insert_with(|| json!({}));
            let slot = if let Some(target) = target {
                if row[kind].is_null() {
                    row[kind] = json!({});
                }
                &mut row[kind][target]
            } else {
                &mut row[kind]
            };
            let count = if slot.is_null() { 0 } else { decision(slot)?.1 };
            *slot = if allowed {
                json!("allow")
            } else {
                json!(format!(
                    "deny:{}",
                    count
                        .checked_add(1)
                        .ok_or_else(|| invalid("denial count exhausted"))?
                ))
            };
            Ok(())
        })
    }
    /// Reads persistent matching-target decline counts, including across restarts.
    pub fn decline_count(&self, plugin: &str, grant: &str) -> Result<u64, StoreError> {
        plugin_id(plugin)?;
        let (kind, target) = consent_target(grant)?;
        let row = &self.document.value["plugins"][plugin][kind];
        let value = target.map_or(row, |target| &row[target]);
        if value.is_null() {
            Ok(0)
        } else {
            decision(value).map(|value| value.1)
        }
    }
    /// Explicit matching-target reset (SEC-110).
    pub fn reset(&mut self, plugin: &str, grant: &str) -> Result<(), StoreError> {
        plugin_id(plugin)?;
        let (kind, target) = consent_target(grant)?;
        self.document.update(|value| {
            let Some(row) = value["plugins"]
                .as_object_mut()
                .and_then(|rows| rows.get_mut(plugin))
            else {
                return Ok(());
            };
            if let Some(target) = target {
                if let Some(map) = row[kind].as_object_mut() {
                    map.remove(target);
                }
            } else if let Some(map) = row.as_object_mut() {
                map.remove(kind);
            }
            Ok(())
        })
    }
    /// Forgets this plugin's actual answers only.
    pub fn forget(&mut self, plugin: &str) -> Result<(), StoreError> {
        self.document.forget(plugin)
    }
    /// Snapshot answers; invalid plugin identities return no permissions.
    pub fn decisions(&self, plugin: &str) -> Vec<ConsentDecision> {
        if plugin_id(plugin).is_err() {
            return Vec::new();
        }
        let mut result = Vec::new();
        if let Some(row) = self.document.value["plugins"][plugin].as_object() {
            for (kind, targets) in row {
                if kind == "geocode" {
                    result.push(ConsentDecision {
                        grant: kind.clone(),
                        allowed: targets == "allow",
                    });
                } else if let Some(targets) = targets.as_object() {
                    for (target, value) in targets {
                        result.push(ConsentDecision {
                            grant: format!("{kind}:{target}"),
                            allowed: value == "allow",
                        });
                    }
                }
            }
        }
        result
    }
    /// Actual write receipt hash for the host audit pipeline.
    pub fn sha256(&self) -> &str {
        &self.document.sha256
    }
    /// Recovery notices for the actual host audit/UI context.
    pub fn notices(&self) -> &[StoreNotice] {
        &self.document.notices
    }
}
fn validate_declines(value: &Value) -> Result<(), StoreError> {
    let mut seen = BTreeSet::new();
    for key in value["declined"]
        .as_array()
        .ok_or_else(|| invalid("invalid declined keys"))?
    {
        let key = key
            .as_str()
            .filter(|key| fingerprint(key))
            .ok_or_else(|| invalid("invalid declined fingerprint"))?;
        if !seen.insert(key) {
            return Err(invalid("duplicate declined fingerprint"));
        }
    }
    Ok(())
}
/// Refusal decisions plus a snapshot from the authoritative raw trust repository.
/// This adapter never writes or projects raw publisher metadata itself.
#[derive(Debug)]
pub struct TrustStore {
    document: Document,
    trusted: crate::trust::TrustSnapshot,
}
impl TrustStore {
    /// Opens user trust. System-key admission uses `trust::TrustRepository` with
    /// the actual administrator-loaded document instead of this convenience API.
    pub fn open(root: &Path) -> Result<Self, StoreError> {
        let repository = crate::trust::TrustRepository::new(
            root.join("trust.json"),
            crate::trust::TrustDocument::default(),
        )
        .map_err(|error| StoreError::Corrupt(format!("{error:?}")))?;
        let trusted = repository
            .snapshot_at(SystemTime::now())
            .map_err(|error| StoreError::Corrupt(format!("{error:?}")))?;
        // A refusal cannot be represented as a fabricated trusted publisher row.
        let document = Document::open(
            root,
            "trust-decisions.json",
            json!({"version":1,"declined":[]}),
            validate_declines,
        )?;
        Ok(Self { document, trusted })
    }
    /// Remembers an actual user refusal in private decision metadata, preserving
    /// the authoritative trust document. Admission must consult this refusal.
    pub fn decline(&mut self, key: &str) -> Result<(), StoreError> {
        if !fingerprint(key) {
            return Err(invalid("invalid publisher fingerprint"));
        }
        self.document.update(|value| {
            let keys = value["declined"]
                .as_array_mut()
                .ok_or_else(|| invalid("invalid declined keys"))?;
            if !keys.iter().any(|value| value == key) {
                keys.push(json!(key));
            }
            Ok(())
        })
    }
    /// `trusted`, `trust.declined`, or `unknown`. Active revocation fails closed.
    /// This is a startup snapshot; install admission must use current trust policy.
    pub fn state(&self, key: &str) -> String {
        if !fingerprint(key) {
            return "unknown".into();
        }
        if self.document.value["declined"]
            .as_array()
            .is_some_and(|keys| keys.iter().any(|value| value == key))
        {
            return "trust.declined".into();
        }
        if self
            .trusted
            .keys()
            .iter()
            .any(|resolved| resolved.entry.key_id == key && resolved.revocation.is_none())
        {
            return "trusted".into();
        }
        "unknown".into()
    }
    /// Actual refusal metadata write hash for the host audit pipeline.
    pub fn sha256(&self) -> &str {
        &self.document.sha256
    }
    /// Recovery notices for actual host audit/UI context.
    pub fn notices(&self) -> &[StoreNotice] {
        &self.document.notices
    }
}
/// One actual audit JSON line, read back without reconstructing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    /// Event name.
    pub kind: String,
    /// Actual plugin identity, if present.
    pub plugin: Option<String>,
    /// Original JSON line.
    pub line: String,
}
/// Read-only snapshot of all retained audit segments. Decision-evidence consumers
/// must first use the production audit verifier; this API preserves original lines.
#[derive(Debug)]
pub struct AuditLog {
    entries: Vec<AuditEntry>,
}
impl AuditLog {
    /// Reads actual retained segments under the production writer's shared lock.
    /// Missing logs are empty; no synthetic store-edit event is inserted.
    pub fn open(root: &Path) -> Result<Self, StoreError> {
        let directory = root.join("audit");
        if !directory.try_exists().map_err(io)? {
            return Ok(Self {
                entries: Vec::new(),
            });
        }
        let _lock = ExclusiveFileLock::acquire(&directory.join("audit.lock")).map_err(io)?;
        let mut paths = Vec::new();
        for entry in fs::read_dir(&directory).map_err(io)? {
            let entry = entry.map_err(io)?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name == "audit.jsonl" {
                paths.push((u64::MAX, entry.path()));
                continue;
            }
            if let Some(first) = name
                .strip_prefix("audit-")
                .and_then(|name| name.strip_suffix(".jsonl"))
                .and_then(|text| text.parse::<u64>().ok())
            {
                paths.push((first, entry.path()));
            }
        }
        paths.sort_by_key(|(first, _)| *first);
        let mut entries = Vec::new();
        let mut previous: Option<u64> = None;
        for (_, path) in paths {
            let bytes = read(&path)?;
            if !bytes.is_empty() && bytes.last() != Some(&b'\n') {
                return Err(invalid("incomplete audit line"));
            }
            let text = std::str::from_utf8(&bytes).map_err(|_| invalid("audit is not UTF-8"))?;
            for line in text.lines() {
                let value = parse(line.as_bytes())?;
                let sequence = value["seq"]
                    .as_u64()
                    .ok_or_else(|| invalid("missing audit sequence"))?;
                if previous.is_some_and(|previous| Some(sequence) != previous.checked_add(1)) {
                    return Err(invalid("audit sequence gap or duplicate"));
                }
                previous = Some(sequence);
                let kind = value["event"]
                    .as_str()
                    .ok_or_else(|| invalid("missing audit event"))?
                    .into();
                let plugin = value
                    .get("plugin_id")
                    .map(|value| {
                        value
                            .as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| invalid("invalid audit plugin"))
                    })
                    .transpose()?;
                if let Some(plugin) = &plugin {
                    plugin_id(plugin)?;
                }
                entries.push(AuditEntry {
                    kind,
                    plugin,
                    line: line.into(),
                });
            }
        }
        Ok(Self { entries })
    }
    /// Actual retained entries, oldest first.
    pub fn entries(&self) -> Vec<AuditEntry> {
        self.entries.clone()
    }
}

fn timestamp(compact: bool) -> Result<String, StoreError> {
    let utc = crate::trust::utc_timestamp(SystemTime::now())
        .map_err(|error| StoreError::Corrupt(format!("{error:?}")))?;
    if compact {
        Ok(utc.replace('-', "").replace(' ', "-").replace(':', ""))
    } else {
        Ok(format!("{}Z", utc.replace(' ', "T")))
    }
}
