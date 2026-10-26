//! Durable package registries. Canonical installed state follows LIF-281/282;
//! grant bindings follow SEC-112 through SEC-114.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use ilium_platform::{file_lock::ExclusiveFileLock, secure_fs};
use ilium_plugin_api::{error::Code, grants::Grant, ids::PluginId, Diagnostic};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::error::HostError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageStore {
    /// Root directory of the version store, excluding the plugin id.
    pub root: PathBuf,
}
impl PackageStore {
    /// Resolves an admitted package identity without allowing either input to add path components.
    pub fn checked_version_dir(
        &self,
        plugin: &PluginId,
        version: &str,
    ) -> Result<PathBuf, HostError> {
        ilium_plugin_api::ids::parse_plugin_id(&plugin.0)?;
        if !valid_package_version(version) {
            return Err(error(
                "manifest.version-invalid",
                "package version must be a full SemVer in one path component",
            ));
        }
        Ok(self.root.join(&plugin.0).join(version))
    }

    /// Convenience path for already validated identities. Invalid inputs map to an
    /// isolated reserved namespace that cannot name an admitted plugin. Admission
    /// and filesystem mutations must use [`Self::checked_version_dir`] to retain errors.
    pub fn version_dir(&self, plugin: &PluginId, version: &str) -> PathBuf {
        self.checked_version_dir(plugin, version)
            .unwrap_or_else(|_| {
                let mut digest = Sha256::new();
                digest.update((plugin.0.len() as u64).to_be_bytes());
                digest.update(plugin.0.as_bytes());
                digest.update(version.as_bytes());
                self.root
                    .join(".invalid-package-path")
                    .join(format!("{:x}", digest.finalize()))
            })
    }
}

/// A projection of a complete installed record, not sufficient to admit a new package.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InstalledRecord {
    pub id: String,
    pub version: String,
    pub source: String,
    pub channel: String,
    pub pinned: bool,
}

#[derive(Debug)]
pub struct InstalledRegistry {
    path: PathBuf,
}
impl InstalledRegistry {
    pub fn open(path: &Path) -> Result<Self, HostError> {
        let registry = Self {
            path: path.to_owned(),
        };
        registry.read()?;
        Ok(registry)
    }
    fn read(&self) -> Result<Value, HostError> {
        let value = read_json(&self.path)?.unwrap_or_else(
            || json!({"format":"ilium-installed/1","plugins":[],"purge_tombstones":[]}),
        );
        validate_installed(&value)?;
        Ok(value)
    }
    /// Hold the canonical writer lock through admission, staging and notification.
    /// Reads occur after acquiring the lock, never from the caller's stale projection.
    pub fn transaction(&self) -> Result<InstalledTransaction, HostError> {
        let guard = lock(&self.path.with_file_name("installed.lock"))?;
        let before = read_bytes(&self.path)?;
        let document = self.read()?;
        Ok(InstalledTransaction {
            path: self.path.clone(),
            before,
            document,
            committed: false,
            _lock: guard,
        })
    }

    pub fn list(&self) -> Result<Vec<InstalledRecord>, HostError> {
        let value = self.read()?;
        Ok(value["plugins"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|row| InstalledRecord {
                id: row["id"].as_str().unwrap_or_default().into(),
                version: row["active_version"].as_str().unwrap_or_default().into(),
                source: row["source"].as_str().unwrap_or_default().into(),
                channel: row["channel"].as_str().unwrap_or_default().into(),
                pinned: row["pinned"].as_bool().unwrap_or_default(),
            })
            .collect())
    }
    pub fn put(&mut self, record: InstalledRecord) -> Result<(), HostError> {
        let _lock = lock(&self.path.with_file_name("installed.lock"))?;
        let mut value = self.read()?;
        let rows = value["plugins"].as_array_mut().ok_or_else(corrupt)?;
        let row = rows
            .iter_mut()
            .find(|row| row["id"] == record.id)
            .ok_or_else(|| {
                error(
                    "install.metadata-required",
                    "new installation requires complete admission metadata",
                )
            })?;
        // A version change requires fresh hashes and signer admission, unavailable in this projection.
        if row["active_version"] != record.version
            || row["source"] != record.source
            || row["channel"] != record.channel
        {
            return Err(error(
                "install.metadata-required",
                "changing package identity requires complete admission metadata",
            ));
        }
        row["pinned"] = json!(record.pinned);
        write_json(&self.path, &value)
    }
    /// Persists an admitted complete canonical row. The caller owns package/trust validation.
    pub fn put_complete_record(&mut self, record: Value) -> Result<(), HostError> {
        validate_row(&record)?;
        let _lock = lock(&self.path.with_file_name("installed.lock"))?;
        let mut value = self.read()?;
        let rows = value["plugins"].as_array_mut().ok_or_else(corrupt)?;
        rows.retain(|row| row["id"] != record["id"]);
        rows.push(record);
        rows.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        write_json(&self.path, &value)
    }
    pub fn remove(&mut self, plugin: &PluginId) -> Result<(), HostError> {
        let _lock = lock(&self.path.with_file_name("installed.lock"))?;
        let mut value = self.read()?;
        value["plugins"]
            .as_array_mut()
            .ok_or_else(corrupt)?
            .retain(|row| row["id"] != plugin.0);
        write_json(&self.path, &value)
    }
}

/// One installed-registry transaction. Dropping leaves the last durable commit
/// intact; rollback is explicit so notification failures cannot undo a live install.
#[derive(Debug)]
pub struct InstalledTransaction {
    path: PathBuf,
    before: Option<Vec<u8>>,
    document: Value,
    committed: bool,
    _lock: ExclusiveFileLock,
}
impl InstalledTransaction {
    pub fn document(&self) -> &Value {
        &self.document
    }
    pub fn record(&self, id: &str) -> Option<&Value> {
        self.document["plugins"]
            .as_array()?
            .iter()
            .find(|row| row["id"] == id)
    }
    pub fn put_complete_record(&mut self, record: Value) -> Result<(), HostError> {
        validate_row(&record)?;
        let rows = self.document["plugins"]
            .as_array_mut()
            .ok_or_else(corrupt)?;
        rows.retain(|row| row["id"] != record["id"]);
        rows.push(record);
        rows.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        validate_installed(&self.document)
    }
    pub fn commit(&mut self) -> Result<(), HostError> {
        validate_installed(&self.document)?;
        // A durable replace can report a directory-sync failure after rename.
        // Mark rollback ownership before writing so that case is recoverable too.
        self.committed = true;
        write_json(&self.path, &self.document)?;
        Ok(())
    }
    /// Restore exactly the registry bytes observed under this lock. This is for
    /// pre-notification transaction failure, never an activation/data rollback.
    pub fn rollback(&mut self) -> Result<(), HostError> {
        if !self.committed {
            return Ok(());
        }
        match &self.before {
            Some(bytes) => write_bytes(&self.path, bytes)?,
            None => {
                match std::fs::remove_file(&self.path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(failure) => return Err(error("install.io", failure.to_string())),
                }
                secure_fs::sync_parent_directory(&self.path)
                    .map_err(|e| error("install.io", e.to_string()))?;
            }
        }
        self.committed = false;
        Ok(())
    }
}

#[derive(Debug)]
pub struct GrantStore {
    path: PathBuf,
    document: Value,
    _lock: ExclusiveFileLock,
}
impl GrantStore {
    pub fn open(path: &Path) -> Result<Self, HostError> {
        let guard = lock(&path.with_extension("lock"))?;
        let document = read_json(path)?.unwrap_or_else(|| json!({"version":1,"plugins":{}}));
        if document["version"].as_u64() != Some(1) {
            return Err(error("store.version", "unsupported grant store version"));
        }
        if !document["plugins"].is_object() {
            return Err(HostError::GrantsUnreadable);
        }
        // No audit-decision source exists in this adapter. Reading cannot establish consent.
        Ok(Self {
            path: path.to_owned(),
            document,
            _lock: guard,
        })
    }
    pub fn granted(&self, plugin: &PluginId) -> Result<Vec<Grant>, HostError> {
        let Some(row) = self.document["plugins"].get(&plugin.0) else {
            return Ok(Vec::new());
        };
        let mut grants = Vec::new();
        for side in ["server", "client"] {
            let entries = row["grants"][side]
                .as_array()
                .ok_or(HostError::GrantsUnreadable)?;
            for entry in entries {
                let grant = Grant::parse(entry.as_str().ok_or(HostError::GrantsUnreadable)?)
                    .map_err(|_| HostError::GrantsUnreadable)?;
                // SEC-113: external high-risk entries cannot stand in for an audited decision.
                if !ilium_plugin_api::grants::is_high_risk(grant.as_str()) {
                    grants.push(grant);
                }
            }
        }
        grants.sort();
        grants.dedup();
        Ok(grants)
    }
    pub fn set_granted(&mut self, plugin: &PluginId, grants: &[Grant]) -> Result<(), HostError> {
        let existing = self.granted(plugin)?;
        if grants.iter().any(|grant| !existing.contains(grant)) {
            return Err(error(
                "grants.binding-required",
                "adding grants requires side, signer, tier and audited consent bindings",
            ));
        }
        let mut next = self.document.clone();
        if let Some(row) = next["plugins"].get_mut(&plugin.0) {
            for side in ["server", "client"] {
                let entries = row["grants"][side]
                    .as_array_mut()
                    .ok_or(HostError::GrantsUnreadable)?;
                entries.retain(|entry| {
                    grants
                        .iter()
                        .any(|grant| entry.as_str() == Some(grant.as_str()))
                });
            }
            let epoch = row["epoch"]
                .as_u64()
                .ok_or(HostError::GrantsUnreadable)?
                .checked_add(1)
                .ok_or(HostError::GrantsUnreadable)?;
            row["epoch"] = json!(epoch);
        }
        write_json(&self.path, &next)?;
        self.document = next;
        Ok(())
    }
}

/// User flags in a supplied user config file. Package trust state remains authoritative.
#[derive(Debug)]
pub struct EnableFlags {
    path: PathBuf,
    document: toml::Value,
}
impl EnableFlags {
    pub fn open(path: &Path) -> Result<Self, HostError> {
        let document = match read_bytes(path)? {
            Some(bytes) => std::str::from_utf8(&bytes)
                .map_err(|_| error("config.invalid", "config is not UTF-8"))?
                .parse::<toml::Value>()
                .map_err(|e| error("config.invalid", e.to_string()))?,
            None => toml::Value::Table(Default::default()),
        };
        if let Some(plugins) = document.get("plugins") {
            let table = plugins
                .as_table()
                .ok_or_else(|| error("config.invalid", "plugins must be a table"))?;
            if table
                .values()
                .any(|row| row.get("enabled").is_some_and(|enabled| !enabled.is_bool()))
            {
                return Err(error("config.invalid", "enabled must be boolean"));
            }
        }
        Ok(Self {
            path: path.to_owned(),
            document,
        })
    }
    pub fn is_enabled(&self, plugin: &PluginId) -> bool {
        self.document
            .get("plugins")
            .and_then(|p| p.get(&plugin.0))
            .and_then(|p| p.get("enabled"))
            .and_then(toml::Value::as_bool)
            .unwrap_or(true)
    }
    pub fn set_enabled(&mut self, plugin: &PluginId, enabled: bool) -> Result<(), HostError> {
        let _lock = lock(&self.path.with_extension("lock"))?;
        let mut next = Self::open(&self.path)?.document;
        let root = next
            .as_table_mut()
            .ok_or_else(|| error("config.invalid", "config must be a table"))?;
        let plugins = root
            .entry("plugins")
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .ok_or_else(|| error("config.invalid", "plugins must be a table"))?;
        let row = plugins
            .entry(plugin.0.clone())
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .ok_or_else(|| error("config.invalid", "plugin config must be a table"))?;
        row.insert("enabled".into(), toml::Value::Boolean(enabled));
        let bytes =
            toml::to_string_pretty(&next).map_err(|e| error("config.invalid", e.to_string()))?;
        write_bytes(&self.path, bytes.as_bytes())?;
        self.document = next;
        Ok(())
    }
}

pub fn downloads_dir(cache: &Path) -> PathBuf {
    cache.join("ilium/plugins/downloads")
}
pub fn registry_cache_dir(cache: &Path, url: &str) -> PathBuf {
    cache
        .join("ilium/plugins/registry")
        .join(format!("{:x}", Sha256::digest(url.as_bytes())))
}

fn error(code: &'static str, message: impl Into<String>) -> HostError {
    Diagnostic::error(Code::from_static(code), "", message).into()
}
fn corrupt() -> HostError {
    error(
        "install.state-corrupt",
        "installed registry does not match ilium-installed/1",
    )
}
fn lock(path: &Path) -> Result<ExclusiveFileLock, HostError> {
    let started = Instant::now();
    loop {
        if let Some(guard) =
            ExclusiveFileLock::try_acquire(path).map_err(|e| error("install.io", e.to_string()))?
        {
            return Ok(guard);
        }
        if started.elapsed() >= Duration::from_secs(30) {
            return Err(error("install.locked", "registry writer lock timed out"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>, HostError> {
    let mut file = match secure_fs::open_regular_file(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(error("install.io", e.to_string())),
    };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|e| error("install.io", e.to_string()))?;
    Ok(Some(bytes))
}
fn read_json(path: &Path) -> Result<Option<Value>, HostError> {
    read_bytes(path)?
        .map(|bytes| serde_json::from_slice(&bytes).map_err(|_| corrupt()))
        .transpose()
}
fn write_json(path: &Path, value: &Value) -> Result<(), HostError> {
    let mut bytes =
        serde_json::to_vec_pretty(value).map_err(|e| error("install.io", e.to_string()))?;
    bytes.push(b'\n');
    write_bytes(path, &bytes)
}
fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), HostError> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    secure_fs::create_private_directory(parent).map_err(|e| error("install.io", e.to_string()))?;
    let temporary = path.with_file_name(format!(
        ".ilium-write-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = secure_fs::private_open_options()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        secure_fs::replace_file_durably(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(|e: std::io::Error| error("install.io", e.to_string()))
}
fn hex(value: &Value) -> bool {
    value.as_str().is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
fn utc(value: &Value) -> bool {
    value.as_str().is_some_and(|s| {
        s.len() == 20
            && s.bytes().enumerate().all(|(i, b)| match i {
                4 | 7 => b == b'-',
                10 => b == b'T',
                13 | 16 => b == b':',
                19 => b == b'Z',
                _ => b.is_ascii_digit(),
            })
    })
}
fn exact(value: &Value, keys: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|o| o.len() == keys.len() && keys.iter().all(|k| o.contains_key(*k)))
}
fn member(value: &Value, choices: &[&str]) -> bool {
    value.as_str().is_some_and(|s| choices.contains(&s))
}
fn valid_package_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 64
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
        && semver::Version::parse(version).is_ok_and(|version| {
            [version.major, version.minor, version.patch]
                .into_iter()
                .all(|part| part <= i32::MAX as u64)
        })
}
fn version_value(value: &Value) -> bool {
    value.as_str().is_some_and(valid_package_version)
}
fn version_array(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|versions| versions.iter().all(version_value))
}
fn validate_row(row: &Value) -> Result<(), HostError> {
    let keys = [
        "id",
        "active_version",
        "previous_version",
        "state",
        "source",
        "source_url",
        "channel",
        "publisher_id",
        "key_id",
        "package_sha256",
        "manifest_sha256",
        "pinned",
        "skip",
        "declined",
        "failure_stopped",
        "installed_at",
        "update",
    ];
    let valid = exact(row, &keys)
        && row["id"]
            .as_str()
            .is_some_and(|id| ilium_plugin_api::ids::parse_plugin_id(id).is_ok())
        && version_value(&row["active_version"])
        && (row["previous_version"].is_null() || version_value(&row["previous_version"]))
        && ["previous_version", "source_url", "publisher_id"]
            .iter()
            .all(|k| row[*k].is_null() || row[*k].is_string())
        && member(
            &row["state"],
            &[
                "installed-enabled",
                "installed-disabled-by-user",
                "disabled-untrusted",
                "disabled-blocked",
                "uninstall-pending",
            ],
        )
        && member(
            &row["source"],
            &["bundled", "file", "url", "registry", "dev"],
        )
        && member(&row["channel"], &["stable", "beta", "dev", "file"])
        && (row["key_id"].is_null() || hex(&row["key_id"]))
        && hex(&row["package_sha256"])
        && hex(&row["manifest_sha256"])
        && row["pinned"].is_boolean()
        && version_array(&row["skip"])
        && version_array(&row["declined"])
        && row["failure_stopped"]
            .as_array()
            .is_some_and(|a| a.iter().all(|v| v == "server"))
        && utc(&row["installed_at"])
        && (row["update"].is_null()
            || (exact(
                &row["update"],
                &["candidate", "state", "attempts", "started_at"],
            ) && version_value(&row["update"]["candidate"])
                && member(&row["update"]["state"], &["staged", "migrating", "trial"])
                && row["update"]["attempts"].as_u64().is_some_and(|n| n <= 2)
                && utc(&row["update"]["started_at"])));
    if valid {
        Ok(())
    } else {
        Err(corrupt())
    }
}
fn validate_installed(value: &Value) -> Result<(), HostError> {
    if !exact(value, &["format", "plugins", "purge_tombstones"])
        || value["format"] != "ilium-installed/1"
        || !value["purge_tombstones"].as_array().is_some_and(|ids| {
            ids.iter().all(|id| {
                id.as_str()
                    .is_some_and(|id| ilium_plugin_api::ids::parse_plugin_id(id).is_ok())
            })
        })
    {
        return Err(corrupt());
    }
    let rows = value["plugins"].as_array().ok_or_else(corrupt)?;
    let mut ids = std::collections::BTreeSet::new();
    for row in rows {
        validate_row(row)?;
        if !ids.insert(row["id"].as_str().unwrap_or_default()) {
            return Err(corrupt());
        }
    }
    Ok(())
}
