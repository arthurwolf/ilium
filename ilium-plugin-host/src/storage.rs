//! Durable per-plugin buckets. Runtime adapters must supply canonical scope identities,
//! resolved limits and the running manifest's data version; guest strings are never paths.

use ilium_platform::{file_lock::ExclusiveFileLock, secure_fs};
use ilium_plugin_api::{
    ids::{parse_plugin_id, PluginId},
    ErrorCode, PluginError, Tier,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
};

pub const VALUE_MAX_BYTES: usize = 1024 * 1024;
pub const BUCKET_MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_BUCKET_BYTES: usize = 256 * 1024 * 1024;
const BATCH_MAX_BYTES: usize = 4 * 1024 * 1024;
const MAGIC: &[u8; 8] = b"ILIUMKV1";

/// Host-owned identity, not a guest-supplied path. Project must be the canonical
/// project identity; session names are framed together with that project.
#[derive(Debug, Clone)]
pub struct StorageContext {
    pub project: Option<PathBuf>,
    pub session: Option<String>,
    pub data_version: u32,
}

impl Default for StorageContext {
    fn default() -> Self {
        Self {
            project: None,
            session: None,
            data_version: 1,
        }
    }
}

#[derive(Debug)]
pub struct PluginStorage {
    directory: PathBuf,
    context: StorageContext,
    bucket_bytes: usize,
    shared_bytes: usize,
}

#[derive(Default)]
struct Database {
    stamp: Option<u32>,
    sequence: u64,
    values: BTreeMap<String, (u64, Vec<u8>)>,
}

fn error(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.chars().take(256).collect(),
        retry_after_ms: None,
    }
}
fn invalid(message: &str) -> PluginError {
    error(ErrorCode::InvalidArgument, message)
}
fn io_error(failure: io::Error) -> PluginError {
    error(
        ErrorCode::Io,
        if failure.kind() == io::ErrorKind::StorageFull {
            "no-space"
        } else {
            "storage-io"
        },
    )
}
fn integrity() -> PluginError {
    error(ErrorCode::Io, "storage-integrity")
}
fn key_valid(key: &str) -> bool {
    !key.is_empty() && key.len() <= 256
}

fn private_directory(path: &Path) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "storage root must be absolute",
        ));
    }
    let mut ancestor = PathBuf::new();
    for component in path.components() {
        if matches!(component, Component::ParentDir | Component::CurDir) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "storage path traversal",
            ));
        }
        ancestor.push(component.as_os_str());
        match fs::symlink_metadata(&ancestor) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(io::Error::other("storage ancestor is not a directory"))
            }
            Ok(_) => {}
            Err(failure) if failure.kind() == io::ErrorKind::NotFound => {
                secure_fs::create_private_directory(&ancestor)?;
                secure_fs::sync_parent_directory(&ancestor)?
            }
            Err(failure) => return Err(failure),
        }
    }
    secure_fs::restrict_directory_to_owner(path)
}

fn regular_or_missing(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(io::Error::other("storage leaf is not a regular file"))
        }
        Ok(_) => Ok(()),
        Err(failure) if failure.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(failure) => Err(failure),
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    regular_or_missing(path)?;
    let temporary = path.with_extension("pending");
    regular_or_missing(&temporary)?;
    let mut file = secure_fs::private_open_options()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&temporary)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("storage temporary is not a regular file"));
    }
    secure_fs::restrict_open_file_to_owner(&file)?;
    file.set_len(0)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    secure_fs::replace_file_durably(&temporary, path)
}

fn encode(database: &Database) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&database.stamp.map_or(u64::MAX, u64::from).to_le_bytes());
    payload.extend_from_slice(&database.sequence.to_le_bytes());
    payload.extend_from_slice(&(database.values.len() as u64).to_le_bytes());
    for (key, (order, value)) in &database.values {
        payload.extend_from_slice(&(key.len() as u16).to_le_bytes());
        payload.extend_from_slice(&(value.len() as u32).to_le_bytes());
        payload.extend_from_slice(&order.to_le_bytes());
        payload.extend_from_slice(key.as_bytes());
        payload.extend_from_slice(value);
    }
    let mut bytes = Vec::with_capacity(40 + payload.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&Sha256::digest(&payload));
    bytes.extend_from_slice(&payload);
    bytes
}

fn decode(bytes: &[u8]) -> Result<Database, PluginError> {
    if bytes.len() < 64 || &bytes[..8] != MAGIC || Sha256::digest(&bytes[40..])[..] != bytes[8..40]
    {
        return Err(integrity());
    }
    let mut remaining = &bytes[40..];
    fn take<'a>(remaining: &mut &'a [u8], length: usize) -> Result<&'a [u8], PluginError> {
        if remaining.len() < length {
            return Err(integrity());
        }
        let (part, rest) = remaining.split_at(length);
        *remaining = rest;
        Ok(part)
    }
    fn number(remaining: &mut &[u8], length: usize) -> Result<u64, PluginError> {
        let mut buffer = [0; 8];
        buffer[..length].copy_from_slice(take(remaining, length)?);
        Ok(u64::from_le_bytes(buffer))
    }
    let stamp = match number(&mut remaining, 8)? {
        u64::MAX => None,
        value => Some(
            u32::try_from(value)
                .ok()
                .filter(|value| *value != 0)
                .ok_or_else(integrity)?,
        ),
    };
    let sequence = number(&mut remaining, 8)?;
    let count = number(&mut remaining, 8)?;
    if count > remaining.len() as u64 / 15 {
        return Err(integrity());
    }
    let mut database = Database {
        stamp,
        sequence,
        values: BTreeMap::new(),
    };
    let mut total = 0usize;
    for _ in 0..count {
        let key_len = number(&mut remaining, 2)? as usize;
        let value_len = number(&mut remaining, 4)? as usize;
        let order = number(&mut remaining, 8)?;
        if !(1..=256).contains(&key_len) || value_len > VALUE_MAX_BYTES || order > sequence {
            return Err(integrity());
        }
        let key = std::str::from_utf8(take(&mut remaining, key_len)?)
            .map_err(|_| integrity())?
            .to_owned();
        let value = take(&mut remaining, value_len)?.to_vec();
        total = total
            .checked_add(key_len + value_len)
            .ok_or_else(integrity)?;
        if total > MAX_BUCKET_BYTES || database.values.insert(key, (order, value)).is_some() {
            return Err(integrity());
        }
    }
    if !remaining.is_empty() {
        return Err(integrity());
    }
    Ok(database)
}

impl PluginStorage {
    /// Compatibility convenience: global/shared storage at the documented default data version 1.
    /// Project/session access requires `open_with_context`.
    pub fn open(root: &Path, plugin: PluginId) -> Result<Self, PluginError> {
        Self::open_with_context(
            root,
            plugin,
            StorageContext::default(),
            Tier::T3,
            BUCKET_MAX_BYTES,
            64 * 1024 * 1024,
        )
    }

    pub fn open_with_context(
        root: &Path,
        plugin: PluginId,
        context: StorageContext,
        tier: Tier,
        bucket_bytes: usize,
        shared_bytes: usize,
    ) -> Result<Self, PluginError> {
        parse_plugin_id(&plugin.0).map_err(|_| invalid("invalid-plugin-id"))?;
        let bucket_ceiling = if tier == Tier::T1 {
            MAX_BUCKET_BYTES
        } else {
            BUCKET_MAX_BYTES
        };
        let shared_ceiling = if tier == Tier::T1 {
            MAX_BUCKET_BYTES
        } else {
            128 * 1024 * 1024
        };
        if bucket_bytes > bucket_ceiling || shared_bytes > shared_ceiling {
            return Err(invalid("storage-limit-exceeds-tier"));
        }
        if context
            .project
            .as_ref()
            .is_some_and(|project| !project.is_absolute())
        {
            return Err(invalid("canonical-project-context-required"));
        }
        if context.data_version == 0 {
            return Err(invalid("invalid-data-version"));
        }
        let directory = root.join(&plugin.0);
        private_directory(&directory).map_err(io_error)?;
        Ok(Self {
            directory,
            context,
            bucket_bytes,
            shared_bytes,
        })
    }

    fn identity(&self, bucket: &str) -> Result<String, PluginError> {
        let mut hasher = Sha256::new();
        let mut field = |bytes: &[u8]| {
            hasher.update((bytes.len() as u64).to_le_bytes());
            hasher.update(bytes);
        };
        match bucket {
            "global" | "shared" => return Ok(bucket.to_owned()),
            "project" | "session" => {
                let project = self
                    .context
                    .project
                    .as_ref()
                    .ok_or_else(|| invalid("project-context-required"))?;
                field(project.as_os_str().as_encoded_bytes());
                if bucket == "session" {
                    field(
                        self.context
                            .session
                            .as_ref()
                            .ok_or_else(|| invalid("session-context-required"))?
                            .as_bytes(),
                    );
                }
            }
            _ => return Err(invalid("invalid-bucket")),
        }
        Ok(format!("{bucket}-{:x}", hasher.finalize()))
    }

    fn locked(&self, identity: &str) -> Result<ExclusiveFileLock, PluginError> {
        private_directory(&self.directory).map_err(io_error)?;
        let path = self.directory.join(format!("{identity}.lock"));
        regular_or_missing(&path).map_err(io_error)?;
        ExclusiveFileLock::acquire(&path).map_err(io_error)
    }

    fn load(&self, identity: &str) -> Result<Database, PluginError> {
        let directory =
            secure_fs::NoFollowDirectory::open_root(&self.directory).map_err(io_error)?;
        let mut file = match directory.open_regular(std::ffi::OsStr::new(&format!("{identity}.db")))
        {
            Ok(file) => file,
            Err(failure) if failure.kind() == io::ErrorKind::NotFound => {
                return Ok(Database::default())
            }
            Err(failure) => return Err(io_error(failure)),
        };
        secure_fs::restrict_open_file_to_owner(&file).map_err(io_error)?;
        // Every serialized entry adds 14 bytes to a non-empty key. This bound
        // preserves the full quota even for one-byte keys with empty values.
        let maximum = (MAX_BUCKET_BYTES as u64) * 15 + 64;
        if file.metadata().map_err(io_error)?.len() > maximum {
            return Err(integrity());
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take(maximum + 1)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        if bytes.len() as u64 > maximum {
            return Err(integrity());
        }
        decode(&bytes)
    }

    fn save(&self, identity: &str, database: &Database) -> Result<(), PluginError> {
        atomic_write(
            &self.directory.join(format!("{identity}.db")),
            &encode(database),
        )
        .map_err(io_error)
    }

    fn check_version(&self, bucket: &str, database: &Database) -> Result<(), PluginError> {
        if bucket != "shared"
            && database
                .stamp
                .is_some_and(|stamp| stamp > self.context.data_version)
        {
            return Err(error(ErrorCode::Conflict, "stale-data-version"));
        }
        Ok(())
    }

    pub fn get(&self, bucket: &str, key: &str) -> Result<Option<Vec<u8>>, PluginError> {
        if !key_valid(key) {
            return Err(invalid("invalid-key"));
        }
        let identity = self.identity(bucket)?;
        let _lock = self.locked(&identity)?;
        Ok(self
            .load(&identity)?
            .values
            .remove(key)
            .map(|(_, value)| value))
    }

    pub fn keys(&self, bucket: &str, prefix: &str) -> Result<Vec<String>, PluginError> {
        let identity = self.identity(bucket)?;
        let _lock = self.locked(&identity)?;
        Ok(self
            .load(&identity)?
            .values
            .into_keys()
            .filter(|key| key.starts_with(prefix))
            .collect())
    }

    pub fn put(&mut self, bucket: &str, key: &str, value: Vec<u8>) -> Result<(), PluginError> {
        if value.len() > VALUE_MAX_BYTES {
            return Err(invalid("value-too-large"));
        }
        self.write_batch(bucket, &[(key.to_owned(), Some(value))])
    }

    pub fn remove(&mut self, bucket: &str, key: &str) -> Result<(), PluginError> {
        self.write_batch(bucket, &[(key.to_owned(), None)])
    }

    /// Batches preserve the 1 MiB per-value cap as well as the 4 MiB aggregate
    /// boundary and the 256-entry limit (LIF-117).
    pub fn write_batch(
        &mut self,
        bucket: &str,
        entries: &[(String, Option<Vec<u8>>)],
    ) -> Result<(), PluginError> {
        let bytes = entries.iter().try_fold(0usize, |total, (key, value)| {
            total
                .checked_add(key.len())?
                .checked_add(value.as_ref().map_or(0, Vec::len))
        });
        if entries.len() > 256
            || bytes.is_none_or(|bytes| bytes > BATCH_MAX_BYTES)
            || entries.iter().any(|(key, value)| {
                !key_valid(key)
                    || value
                        .as_ref()
                        .is_some_and(|value| value.len() > VALUE_MAX_BYTES)
            })
        {
            return Err(invalid("invalid-batch"));
        }
        let identity = self.identity(bucket)?;
        let _lock = self.locked(&identity)?;
        let mut database = self.load(&identity)?;
        self.check_version(bucket, &database)?;
        if bucket != "shared"
            && database
                .stamp
                .is_some_and(|stamp| stamp < self.context.data_version)
        {
            return Err(error(ErrorCode::Conflict, "migration-required"));
        }
        for (key, value) in entries {
            if let Some(value) = value {
                database.sequence = database
                    .sequence
                    .checked_add(1)
                    .ok_or_else(|| error(ErrorCode::Internal, "storage-sequence-exhausted"))?;
                database
                    .values
                    .insert(key.clone(), (database.sequence, value.clone()));
            } else {
                database.values.remove(key);
            }
        }
        let mut size: usize = database
            .values
            .iter()
            .map(|(key, (_, value))| key.len() + value.len())
            .sum();
        let quota = if bucket == "shared" {
            self.shared_bytes
        } else {
            self.bucket_bytes
        };
        if bucket != "shared" && size > quota {
            return Err(error(ErrorCode::QuotaExceeded, "bucket-quota-exceeded"));
        }
        while size > quota {
            let oldest = database
                .values
                .iter()
                .min_by_key(|(_, (order, _))| *order)
                .map(|(key, _)| key.clone());
            if let Some(key) = oldest {
                if let Some((_, value)) = database.values.remove(&key) {
                    size -= key.len() + value.len();
                }
            } else {
                break;
            }
        }
        if bucket != "shared" && database.stamp.is_none() {
            database.stamp = Some(self.context.data_version);
        }
        self.save(&identity, &database)
    }

    /// A shared-cache refresh lease uses actual host wall time, never a guest
    /// supplied timestamp. All callers must use the same wall-clock domain.
    /// Clock rollback conservatively extends an existing lease.
    pub fn lease_at(
        &self,
        key: &str,
        ttl_millis: u32,
        now: std::time::SystemTime,
    ) -> Result<bool, PluginError> {
        if !key_valid(key) {
            return Err(invalid("invalid-key"));
        }
        if !(1..=3_600_000).contains(&ttl_millis) {
            return Err(invalid("invalid-lease-ttl"));
        }
        let now = u64::try_from(
            now.duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| invalid("invalid-lease-clock"))?
                .as_millis(),
        )
        .map_err(|_| invalid("invalid-lease-clock"))?;
        let until = now
            .checked_add(u64::from(ttl_millis))
            .ok_or_else(|| invalid("invalid-lease-clock"))?;
        let _lock = self.locked("shared")?;
        let path = self.directory.join("shared.leases");
        let directory =
            secure_fs::NoFollowDirectory::open_root(&self.directory).map_err(io_error)?;
        let mut leases: BTreeMap<String, u64> =
            match directory.open_regular(std::ffi::OsStr::new("shared.leases")) {
                Ok(mut file) => {
                    secure_fs::restrict_open_file_to_owner(&file).map_err(io_error)?;
                    let bound = (self.shared_bytes as u64)
                        .saturating_mul(8)
                        .saturating_add(2);
                    if file.metadata().map_err(io_error)?.len() > bound {
                        return Err(integrity());
                    }
                    let mut bytes = Vec::new();
                    (&mut file)
                        .take(bound + 1)
                        .read_to_end(&mut bytes)
                        .map_err(io_error)?;
                    if bytes.len() as u64 > bound {
                        return Err(integrity());
                    }
                    serde_json::from_slice(&bytes).map_err(|_| integrity())?
                }
                Err(failure) if failure.kind() == io::ErrorKind::NotFound => BTreeMap::new(),
                Err(failure) => return Err(io_error(failure)),
            };
        if leases.keys().any(|key| !key_valid(key)) {
            return Err(integrity());
        }
        leases.retain(|_, expiry| *expiry > now);
        if leases.contains_key(key) {
            return Ok(false);
        }
        let bytes = leases.keys().map(|key| key.len() + 8).sum::<usize>();
        if bytes.saturating_add(key.len() + 8) > self.shared_bytes {
            return Ok(false);
        }
        leases.insert(key.to_owned(), until);
        atomic_write(
            &path,
            &serde_json::to_vec(&leases).map_err(|_| integrity())?,
        )
        .map_err(io_error)?;
        Ok(true)
    }

    pub fn lease(&self, key: &str, ttl_millis: u32) -> Result<bool, PluginError> {
        self.lease_at(key, ttl_millis, std::time::SystemTime::now())
    }

    /// Clears only the rebuildable shared cache, for example after a data
    /// version change. Persistent buckets and refresh leases are separate.
    pub fn clear_shared(&mut self) -> Result<(), PluginError> {
        let _lock = self.locked("shared")?;
        self.save("shared", &Database::default())
    }

    /// Explicit recovery seam: the lifecycle adapter must deliver the returned
    /// notice to the plugin and write the required host error log. Corrupt bytes
    /// are renamed intact; ordinary I/O/permission failures are never erased.
    pub fn recover_corrupt_bucket(
        &mut self,
        bucket: &str,
        now: std::time::SystemTime,
    ) -> Result<PathBuf, PluginError> {
        let identity = self.identity(bucket)?;
        let _lock = self.locked(&identity)?;
        match self.load(&identity) {
            Err(failure)
                if failure.code == ErrorCode::Io && failure.message == "storage-integrity" => {}
            Err(failure) => return Err(failure),
            Ok(_) => return Err(invalid("storage-is-not-corrupt")),
        }
        let timestamp = now
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| invalid("invalid-recovery-clock"))?
            .as_nanos();
        let original = self.directory.join(format!("{identity}.db"));
        let saved = self
            .directory
            .join(format!("{identity}.db.corrupt-{timestamp}"));
        // Never overwrite a previous recovery artifact, including a link.
        match fs::symlink_metadata(&saved) {
            Err(failure) if failure.kind() == io::ErrorKind::NotFound => {}
            Err(failure) => return Err(io_error(failure)),
            Ok(_) => return Err(error(ErrorCode::Conflict, "recovery-name-exists")),
        }
        regular_or_missing(&original).map_err(io_error)?;
        secure_fs::replace_file_durably(&original, &saved).map_err(io_error)?;
        self.save(&identity, &Database::default())?;
        Ok(saved)
    }

    /// Host acknowledgement after a successful migration or the explicitly
    /// documented no-migrator path. Only the supplied bucket is stamped: a
    /// project/session not in this activation must remain lazily migratable.
    pub fn stamp_bucket_after_migration(
        &self,
        bucket: &str,
        target: u32,
    ) -> Result<(), PluginError> {
        if target == 0 || bucket == "shared" {
            return Err(invalid("invalid-versioned-bucket"));
        }
        let identity = self.identity(bucket)?;
        let _lock = self.locked(&identity)?;
        let mut database = self.load(&identity)?;
        if database.stamp.is_some_and(|stamp| stamp > target) {
            return Err(error(ErrorCode::Conflict, "migration.data-newer"));
        }
        // Missing/unstamped buckets receive their actual instance stamp only
        // at first write (LIF-102), not an invented migration history.
        if database.stamp.is_none() {
            return Ok(());
        }
        database.stamp = Some(target);
        self.save(&identity, &database)
    }

    /// No guest export can be invoked without a runtime adapter. This method
    /// verifies the existing global stamp; upgrades require `migrate_bucket_with`.
    pub fn migrate(&mut self, target_version: u32) -> Result<(), PluginError> {
        let identity = self.identity("global")?;
        let _lock = self.locked(&identity)?;
        let database = self.load(&identity)?;
        match database.stamp {
            Some(version) if version > target_version => {
                Err(error(ErrorCode::Conflict, "migration.data-newer"))
            }
            Some(version) if version == target_version => Ok(()),
            _ => Err(error(ErrorCode::Internal, "migration-context-required")),
        }
    }

    /// Transactional bucket transformation. The real runtime owns backup,
    /// deadline, migration instance and cross-bucket/data-directory recovery.
    /// The callback must not reenter this bucket (its process lock is held).
    pub fn migrate_bucket_with<F>(
        &mut self,
        bucket: &str,
        target: u32,
        transform: F,
    ) -> Result<(), PluginError>
    where
        F: FnOnce(u32, u32, &mut BTreeMap<String, Vec<u8>>) -> Result<(), PluginError>,
    {
        if bucket == "shared" {
            return Err(invalid("shared-is-not-versioned"));
        }
        let identity = self.identity(bucket)?;
        let _lock = self.locked(&identity)?;
        let mut database = self.load(&identity)?;
        let Some(from) = database.stamp else {
            return Err(error(ErrorCode::Conflict, "unstamped-data"));
        };
        if from > target {
            return Err(error(ErrorCode::Conflict, "migration.data-newer"));
        }
        if from == target {
            return Ok(());
        }
        let mut values = database
            .values
            .iter()
            .map(|(key, (_, value))| (key.clone(), value.clone()))
            .collect();
        transform(from, target, &mut values)?;
        if values
            .iter()
            .any(|(key, value)| !key_valid(key) || value.len() > VALUE_MAX_BYTES)
        {
            return Err(invalid("invalid-migrated-value"));
        }
        if values
            .iter()
            .map(|(key, value)| key.len() + value.len())
            .sum::<usize>()
            > self.bucket_bytes
        {
            return Err(error(ErrorCode::QuotaExceeded, "bucket-quota-exceeded"));
        }
        database.values = values
            .into_iter()
            .map(|(key, value)| (key, (0, value)))
            .collect();
        database.stamp = Some(target);
        self.save(&identity, &database)
    }
}

/// Host-only global-bucket stamp acknowledgement. Project/session buckets must
/// be acknowledged separately through their actual `StorageContext` when they
/// activate (LIF-104/114). The lifecycle owner must hold its plugin migration
/// lock and must have completed migration or the no-migrator path first.
pub fn write_bucket_stamp(storage_root: &Path, plugin: &str, data_version: u64) -> io::Result<()> {
    let version = u32::try_from(data_version)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid data version"))?;
    if version == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid data version",
        ));
    }
    let plugin = parse_plugin_id(plugin)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid plugin id"))?;
    let storage = PluginStorage::open(storage_root, plugin)
        .map_err(|failure| io::Error::other(failure.message))?;
    storage
        .stamp_bucket_after_migration("global", version)
        .map_err(|failure| io::Error::other(failure.message))
}
