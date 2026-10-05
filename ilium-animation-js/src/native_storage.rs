//! Native bounded storage. The broker checks dispatch, native issue and delivery;
//! full package identity owns cache/state namespaces. No path or grant is script authority.
use crate::{
    error::{AnimationError, Result},
    permissions::{
        BindingKind, CallPhase, Capability, Channel, HostBinding, OperationNeed, OperationTicket,
        PackageIdentity, PermissionBroker, Right, Scope, Selection,
    },
};
use ilium_execution::{QuotaGroup, StorageAdmission};
use ilium_platform::animation_files::{
    validate_leaf, DirectoryEntry, FileIdentity, PinnedDirectory, PinnedFile, WriteMode,
};
use ilium_platform::secure_fs::NoFollowDirectory;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io,
    path::{Component, Path},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
const MAX_BYTES: usize = 8 * 1024 * 1024;
fn invalid(message: &str) -> AnimationError {
    AnimationError::Runtime(message.into())
}
fn auth(error: crate::permissions::PermissionError) -> AnimationError {
    AnimationError::PermissionDenied(error.to_string())
}
fn reserve(quota: &QuotaGroup, bytes: usize) -> Result<StorageAdmission> {
    quota
        .reserve_external_storage(bytes.max(1))
        .map_err(|error| AnimationError::Budget(format!("storage admission: {error:?}")))
}
#[derive(Debug, Clone, Default)]
pub struct StorageCancellation(Arc<AtomicBool>);
impl StorageCancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    fn check(&self) -> Result<()> {
        if self.0.load(Ordering::Acquire) {
            Err(invalid("native storage operation cancelled"))
        } else {
            Ok(())
        }
    }
}
pub struct RetainedBytes {
    bytes: Vec<u8>,
    _admission: StorageAdmission,
}
impl RetainedBytes {
    pub fn view(&self) -> &[u8] {
        &self.bytes
    }
    pub fn sha256(&self) -> [u8; 32] {
        Sha256::digest(&self.bytes).into()
    }
}
/// Arc cloning shares the exact immutable allocation; copying its bytes requires
/// a separate admission. There is deliberately no Vec clone/get-copy API.
pub struct StorageRead {
    ticket: OperationTicket,
    cancellation: StorageCancellation,
    value: Arc<RetainedBytes>,
}
impl StorageRead {
    pub fn deliver(self, broker: &mut PermissionBroker) -> Result<Arc<RetainedBytes>> {
        let Self {
            ticket,
            cancellation,
            value,
        } = self;
        if let Err(error) = cancellation.check() {
            let _ = broker.settle_without_delivery(&ticket);
            return Err(error);
        }
        match broker.deliver(&ticket, || value) {
            Ok(value) => Ok(value),
            Err(error) => {
                let _ = broker.settle_without_delivery(&ticket);
                Err(auth(error))
            }
        }
    }
}
pub struct RetainedListing {
    entries: Vec<DirectoryEntry>,
    _admission: StorageAdmission,
}
impl RetainedListing {
    pub fn view(&self) -> &[DirectoryEntry] {
        &self.entries
    }
}
pub struct StorageListing {
    ticket: OperationTicket,
    cancellation: StorageCancellation,
    value: RetainedListing,
}
impl StorageListing {
    pub fn deliver(self, broker: &mut PermissionBroker) -> Result<RetainedListing> {
        let Self {
            ticket,
            cancellation,
            value,
        } = self;
        if let Err(error) = cancellation.check() {
            let _ = broker.settle_without_delivery(&ticket);
            return Err(error);
        }
        match broker.deliver(&ticket, || value) {
            Ok(value) => Ok(value),
            Err(error) => {
                let _ = broker.settle_without_delivery(&ticket);
                Err(auth(error))
            }
        }
    }
}
#[derive(Clone)]
enum SelectedNative {
    File(Arc<PinnedFile>),
    Folder(Arc<PinnedDirectory>),
}
pub struct SelectedStorage {
    native: SelectedNative,
    binding: HostBinding,
    slot: String,
    writable: bool,
    quota: QuotaGroup,
    _admission: Arc<StorageAdmission>,
}
impl SelectedStorage {
    /// Only a trusted UI picker passes a user-entered host path here. Walk
    /// every component from a pinned root with nofollow opens; a guest never
    /// supplies a pathname to this constructor or the saved registry.
    pub fn pin_user_path(
        path: &Path,
        selection: Selection,
        slot: String,
        writable: bool,
        quota: QuotaGroup,
    ) -> Result<Self> {
        let text = path
            .to_str()
            .filter(|text| text.len() <= 4096)
            .ok_or_else(|| invalid("selected host path must be bounded UTF-8"))?;
        if !path.is_absolute() || text.chars().any(char::is_control) {
            return Err(invalid("selected host path must be absolute"));
        }
        let _walk_admission = reserve(&quota, 8192)?;
        let mut parts = Vec::new();
        for component in path.components() {
            match component {
                Component::RootDir => {}
                Component::Normal(part) => {
                    let part = part
                        .to_str()
                        .ok_or_else(|| invalid("selected path component encoding"))?;
                    validate_leaf(part)?;
                    if parts.len() >= 128 {
                        return Err(invalid("selected path depth"));
                    }
                    parts.push(part);
                }
                _ => return Err(invalid("selected host path traversal")),
            }
        }
        if parts.is_empty() {
            return Err(invalid("selected path cannot be filesystem root"));
        }
        let mut current = NoFollowDirectory::open_root(Path::new("/"))?;
        let directory_parts = if selection == Selection::File {
            &parts[..parts.len() - 1]
        } else {
            parts.as_slice()
        };
        for part in directory_parts {
            current = current.open_directory(std::ffi::OsStr::new(part))?;
        }
        match selection {
            Selection::Folder => Self::folder_from_host(
                Arc::new(PinnedDirectory::from_host(Arc::new(current))?),
                slot,
                writable,
                quota,
            ),
            Selection::File => {
                if writable {
                    return Err(invalid("selected file write requires parent-entry policy"));
                }
                let file = current.open_regular(std::ffi::OsStr::new(parts[parts.len() - 1]))?;
                Self::file_from_host(Arc::new(PinnedFile::from_host(file)?), slot, quota)
            }
        }
    }
    pub fn matches_right(&self, right: &Right) -> bool {
        let Scope::Disk { slot, selection } = &right.scope else {
            return false;
        };
        let correct_selection = matches!(
            (&self.native, selection),
            (SelectedNative::File(_), Selection::File)
                | (SelectedNative::Folder(_), Selection::Folder)
        );
        if slot != &self.slot || !correct_selection {
            return false;
        }
        match right.id {
            Capability::DiskRead => true,
            Capability::DiskWrite => self.writable,
            _ => false,
        }
    }
    /// Call only inside an actual committed finite IO job. The original
    /// operation ticket and completion ACK stay with its outer native owner.
    pub fn read_after_issue(
        &self,
        relative: &str,
        maximum: usize,
        cancellation: &StorageCancellation,
        quota: &QuotaGroup,
    ) -> Result<Arc<RetainedBytes>> {
        self.check_quota(quota)?;
        if relative.len() > 240 {
            return Err(invalid("selected relative path length"));
        }
        let _path_admission = reserve(quota, 8192)?;
        let file = match &self.native {
            SelectedNative::File(file) if relative.is_empty() => Arc::clone(file),
            SelectedNative::Folder(root) => {
                if !crate::package::valid_path(relative) {
                    return Err(invalid("selected relative path"));
                }
                let mut segments = relative.split('/').peekable();
                let mut folder = Arc::clone(root);
                let mut file = None;
                while let Some(segment) = segments.next() {
                    if segments.peek().is_some() {
                        folder = Arc::new(folder.child(segment, false)?);
                    } else {
                        file = Some(Arc::new(folder.open_file(segment)?));
                    }
                }
                file.ok_or_else(|| invalid("selected relative file"))?
            }
            _ => return Err(invalid("selected file relative path")),
        };
        bounded_read(&file, maximum, cancellation, quota)
    }
    pub fn list_after_issue(
        &self,
        relative: &str,
        maximum: usize,
        cancellation: &StorageCancellation,
        quota: &QuotaGroup,
    ) -> Result<RetainedListing> {
        self.check_quota(quota)?;
        if maximum == 0 || maximum > 1024 || relative.len() > 240 {
            return Err(invalid("selected directory listing bound"));
        }
        let SelectedNative::Folder(root) = &self.native else {
            return Err(invalid("selected file cannot be listed"));
        };
        let admission = reserve(quota, maximum * 512 + 65536)?;
        cancellation.check()?;
        let mut folder = Arc::clone(root);
        if !relative.is_empty() {
            if !crate::package::valid_path(relative) {
                return Err(invalid("selected relative directory"));
            }
            for segment in relative.split('/') {
                cancellation.check()?;
                folder = Arc::new(folder.child(segment, false)?);
            }
        }
        let entries = folder.list(maximum)?;
        cancellation.check()?;
        Ok(RetainedListing {
            entries,
            _admission: admission,
        })
    }
    pub fn write_after_issue(
        &self,
        relative: &str,
        bytes: &[u8],
        overwrite: bool,
        cancellation: &StorageCancellation,
        quota: &QuotaGroup,
    ) -> Result<[u8; 32]> {
        self.check_quota(quota)?;
        if !self.writable
            || relative.len() > 240
            || !crate::package::valid_path(relative)
            || bytes.len() > MAX_BYTES
        {
            return Err(invalid("selected write path, right or byte bound"));
        }
        let SelectedNative::Folder(root) = &self.native else {
            return Err(invalid("selected file entry cannot be atomically replaced"));
        };
        let _custody = reserve(quota, 32768)?;
        let mut segments = relative.split('/').peekable();
        let mut folder = Arc::clone(root);
        let mut leaf = None;
        while let Some(segment) = segments.next() {
            cancellation.check()?;
            if segments.peek().is_some() {
                folder = Arc::new(folder.child(segment, false)?);
            } else {
                leaf = Some(segment);
            }
        }
        let leaf = leaf.ok_or_else(|| invalid("selected write leaf"))?;
        let mode = if overwrite {
            WriteMode::ReplaceEntry
        } else {
            WriteMode::CreateNew
        };
        cancellation.check()?;
        let mut stage = folder.begin_atomic(leaf, mode)?;
        for chunk in bytes.chunks(16384) {
            cancellation.check()?;
            stage.write(chunk, MAX_BYTES)?;
        }
        stage.prepare_durable()?;
        cancellation.check()?;
        stage.publish_entry()?;
        stage.durable_ack()?;
        Ok(Sha256::digest(bytes).into())
    }
    pub fn folder_from_host(
        root: Arc<PinnedDirectory>,
        slot: String,
        writable: bool,
        quota: QuotaGroup,
    ) -> Result<Self> {
        let admission = Arc::new(reserve(&quota, 4096)?);
        let id = root.identity();
        let binding = HostBinding::new(
            BindingKind::Folder,
            format!("folder-{}-{}", id.device, id.inode),
        )
        .map_err(auth)?;
        Self::construct(
            SelectedNative::Folder(root),
            binding,
            slot,
            writable,
            quota,
            admission,
        )
    }
    pub fn file_from_host(file: Arc<PinnedFile>, slot: String, quota: QuotaGroup) -> Result<Self> {
        let admission = Arc::new(reserve(&quota, 4096)?);
        let id = file.identity();
        let binding = HostBinding::new(
            BindingKind::File,
            format!("file-{}-{}", id.device, id.inode),
        )
        .map_err(auth)?;
        Self::construct(
            SelectedNative::File(file),
            binding,
            slot,
            false,
            quota,
            admission,
        )
    }
    fn construct(
        native: SelectedNative,
        binding: HostBinding,
        slot: String,
        writable: bool,
        quota: QuotaGroup,
        admission: Arc<StorageAdmission>,
    ) -> Result<Self> {
        let resource = Self {
            native,
            binding,
            slot,
            writable,
            quota,
            _admission: admission,
        };
        resource.need(false)?;
        Ok(resource)
    }
    pub fn binding(&self) -> &HostBinding {
        &self.binding
    }
    fn check_quota(&self, quota: &QuotaGroup) -> Result<()> {
        if !self.quota.shares_root(quota) {
            return Err(invalid("selected storage quota mismatch"));
        }
        Ok(())
    }
    fn need(&self, write: bool) -> Result<OperationNeed> {
        if write && !self.writable {
            return Err(AnimationError::PermissionDenied(
                "selected resource is not writable".into(),
            ));
        }
        OperationNeed::new(
            Right {
                id: if write {
                    Capability::DiskWrite
                } else {
                    Capability::DiskRead
                },
                scope: Scope::Disk {
                    slot: self.slot.clone(),
                    selection: match self.native {
                        SelectedNative::File(_) => Selection::File,
                        SelectedNative::Folder(_) => Selection::Folder,
                    },
                },
            },
            Some(self.binding.clone()),
        )
        .map_err(auth)
    }
    pub fn operation_need(&self, write: bool) -> Result<OperationNeed> {
        self.need(write)
    }
    fn open(&self, leaf: Option<&str>) -> Result<Arc<PinnedFile>> {
        match (&self.native, leaf) {
            (SelectedNative::File(file), None) => Ok(Arc::clone(file)),
            (SelectedNative::Folder(root), Some(leaf)) => Ok(Arc::new(root.open_file(leaf)?)),
            _ => Err(invalid("selected file/folder shape mismatch")),
        }
    }
}
fn dispatch(
    broker: &mut PermissionBroker,
    channel: &Channel,
    demand: &str,
    need: OperationNeed,
) -> Result<OperationTicket> {
    broker
        .dispatch(channel, CallPhase::Async, demand, vec![need])
        .map_err(auth)
}
fn bounded_read(
    file: &PinnedFile,
    maximum: usize,
    cancellation: &StorageCancellation,
    quota: &QuotaGroup,
) -> Result<Arc<RetainedBytes>> {
    if maximum == 0 || maximum > MAX_BYTES {
        return Err(invalid("storage read size outside limits"));
    }
    cancellation.check()?;
    let admission = reserve(quota, maximum + 16384)?;
    if file.len()? > maximum as u64 {
        return Err(invalid("selected file exceeds read limit"));
    }
    let mut bytes = Vec::with_capacity(maximum);
    let mut chunk = [0u8; 16384];
    loop {
        cancellation.check()?;
        let available = (maximum + 1 - bytes.len()).min(chunk.len());
        let count = file.read_at(&mut chunk[..available], bytes.len() as u64)?;
        if count == 0 {
            break;
        }
        if count > maximum - bytes.len() {
            return Err(invalid("selected file grew beyond read limit"));
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    Ok(Arc::new(RetainedBytes {
        bytes,
        _admission: admission,
    }))
}
const PERSISTENT_MAGIC: &[u8; 8] = b"ILCACHE1";
const PERSISTENT_HEADER: usize = 52;
/// Disk I/O only. A native service operation must dispatch, commit and deliver
/// the original StatePersist ticket around the finite job using this owner.
pub struct PersistentCache {
    host_root: Arc<PinnedDirectory>,
    principal_directory: String,
    quota: QuotaGroup,
    _metadata: StorageAdmission,
}
impl PersistentCache {
    pub fn new(
        principal: &PackageIdentity,
        host_root: Arc<PinnedDirectory>,
        quota: QuotaGroup,
    ) -> Result<Self> {
        let metadata = reserve(&quota, 65536)?;
        let principal_directory = format!(
            "principal-{:x}",
            Sha256::digest(serde_json::to_vec(&(principal, "cache-session"))?)
        );
        Ok(Self {
            host_root,
            principal_directory,
            quota,
            _metadata: metadata,
        })
    }
    fn leaf(key: &str) -> Result<String> {
        validate_leaf(key)?;
        if key.len() > 128 {
            return Err(invalid("persistent cache key bound"));
        }
        Ok(format!("cache-{:x}.bin", Sha256::digest(key.as_bytes())))
    }
    fn now_seconds() -> Result<u64> {
        Ok(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| invalid("persistent clock precedes epoch"))?
            .as_secs())
    }
    pub fn get(
        &self,
        key: &str,
        maximum: usize,
        cancellation: &StorageCancellation,
    ) -> Result<Option<Arc<RetainedBytes>>> {
        if maximum == 0 || maximum > MAX_BYTES {
            return Err(invalid("persistent read bound"));
        }
        let admission = reserve(&self.quota, maximum + PERSISTENT_HEADER + 16384)?;
        let leaf = Self::leaf(key)?;
        cancellation.check()?;
        let directory = match self.host_root.child(&self.principal_directory, false) {
            Ok(directory) => directory,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let file = match directory.open_file(&leaf) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if file.len()? > (maximum + PERSISTENT_HEADER) as u64 {
            return Err(invalid("persistent file size bound"));
        }
        let size = usize::try_from(file.len()?).map_err(|_| invalid("persistent file size"))?;
        let mut bytes = Vec::with_capacity(size);
        let mut chunk = [0u8; 16384];
        while bytes.len() < size {
            cancellation.check()?;
            let available = chunk.len().min(size - bytes.len());
            let count = file.read_at(&mut chunk[..available], bytes.len() as u64)?;
            if count == 0 {
                return Err(invalid("persistent file changed while reading"));
            }
            bytes.extend_from_slice(&chunk[..count]);
        }
        if bytes.len() < PERSISTENT_HEADER || &bytes[..8] != PERSISTENT_MAGIC {
            return Err(invalid("persistent envelope header"));
        }
        let expiry = u64::from_le_bytes(
            bytes[8..16]
                .try_into()
                .map_err(|_| invalid("persistent TTL"))?,
        );
        let body_len = u32::from_le_bytes(
            bytes[16..20]
                .try_into()
                .map_err(|_| invalid("persistent length"))?,
        ) as usize;
        if body_len > MAX_BYTES || body_len > maximum || bytes.len() != PERSISTENT_HEADER + body_len
        {
            return Err(invalid("persistent envelope length/max_bytes"));
        }
        let expected: [u8; 32] = Sha256::digest(&bytes[PERSISTENT_HEADER..]).into();
        if bytes[20..52] != expected[..] {
            return Err(invalid("persistent digest changed"));
        }
        if expiry != 0 && expiry <= Self::now_seconds()? {
            return Ok(None);
        }
        bytes.copy_within(PERSISTENT_HEADER.., 0);
        bytes.truncate(body_len);
        Ok(Some(Arc::new(RetainedBytes {
            bytes,
            _admission: admission,
        })))
    }
    pub fn put(
        &self,
        key: &str,
        value: &[u8],
        ttl_seconds: Option<u64>,
        cancellation: &StorageCancellation,
    ) -> Result<[u8; 32]> {
        if value.len() > MAX_BYTES || ttl_seconds.is_some_and(|ttl| ttl == 0 || ttl > 31_536_000) {
            return Err(invalid("persistent value/TTL bound"));
        }
        let envelope_bytes = PERSISTENT_HEADER
            .checked_add(value.len())
            .ok_or_else(|| invalid("persistent envelope overflow"))?;
        let _admission = reserve(&self.quota, envelope_bytes + 128 * 512 + 65536)?;
        let leaf = Self::leaf(key)?;
        let expiry = ttl_seconds
            .map(|ttl| {
                Self::now_seconds()?
                    .checked_add(ttl)
                    .ok_or_else(|| invalid("persistent TTL overflow"))
            })
            .transpose()?
            .unwrap_or(0);
        let hash: [u8; 32] = Sha256::digest(value).into();
        let mut envelope = Vec::with_capacity(envelope_bytes);
        envelope.extend_from_slice(PERSISTENT_MAGIC);
        envelope.extend_from_slice(&expiry.to_le_bytes());
        envelope.extend_from_slice(&(value.len() as u32).to_le_bytes());
        envelope.extend_from_slice(&hash);
        envelope.extend_from_slice(value);
        cancellation.check()?;
        let directory = Arc::new(self.host_root.child(&self.principal_directory, true)?);
        let _lease = directory.try_exclusive_lease()?;
        let entries = directory.list(128)?;
        let now = Self::now_seconds()?;
        let mut removed_expired = false;
        for entry in &entries {
            if entry.is_directory
                || !entry.name.starts_with("cache-")
                || !entry.name.ends_with(".bin")
            {
                continue;
            }
            cancellation.check()?;
            let file = directory.open_file(&entry.name)?;
            let mut prefix = [0u8; 16];
            if file.read_at(&mut prefix, 0)? != prefix.len() || &prefix[..8] != PERSISTENT_MAGIC {
                return Err(invalid("persistent namespace envelope changed"));
            }
            let deadline = u64::from_le_bytes(
                prefix[8..16]
                    .try_into()
                    .map_err(|_| invalid("persistent inventory TTL"))?,
            );
            if deadline != 0 && deadline <= now {
                directory.remove_pinned_file(&entry.name, &file)?;
                removed_expired = true;
            }
        }
        if removed_expired {
            directory.sync()?;
        }
        let entries = if removed_expired {
            directory.list(128)?
        } else {
            entries
        };
        if entries.iter().any(|entry| entry.is_directory) {
            return Err(invalid("persistent namespace contains directories"));
        }
        let total = entries
            .iter()
            .try_fold(0u64, |acc, entry| acc.checked_add(entry.bytes))
            .ok_or_else(|| invalid("persistent namespace size overflow"))?;
        let prior = entries
            .iter()
            .find(|entry| entry.name == leaf)
            .map_or(0, |entry| entry.bytes);
        if entries.len() == 128 && prior == 0
            || total
                .checked_sub(prior)
                .and_then(|base| base.checked_add(envelope.len() as u64))
                .is_none_or(|size| size > (MAX_BYTES + 128 * PERSISTENT_HEADER) as u64)
        {
            return Err(invalid("persistent namespace byte/count bound"));
        }
        cancellation.check()?;
        let mut stage = directory.begin_atomic(&leaf, WriteMode::ReplaceEntry)?;
        for chunk in envelope.chunks(16384) {
            cancellation.check()?;
            stage.write(chunk, MAX_BYTES + PERSISTENT_HEADER)?;
        }
        stage.prepare_durable()?;
        cancellation.check()?;
        stage.publish_entry()?;
        stage.durable_ack()?;
        Ok(hash)
    }
    pub fn remove(&self, key: &str, cancellation: &StorageCancellation) -> Result<bool> {
        let _admission = reserve(&self.quota, 32768)?;
        let leaf = Self::leaf(key)?;
        cancellation.check()?;
        let directory = match self.host_root.child(&self.principal_directory, false) {
            Ok(directory) => directory,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        let _lease = directory.try_exclusive_lease()?;
        let file = match directory.open_file(&leaf) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        let mut header = [0u8; PERSISTENT_HEADER];
        let mut read = 0usize;
        while read < header.len() {
            cancellation.check()?;
            let count = file.read_at(&mut header[read..], read as u64)?;
            if count == 0 {
                return Err(invalid("persistent remove header changed"));
            }
            read += count;
        }
        if &header[..8] != PERSISTENT_MAGIC {
            return Err(invalid("persistent remove envelope"));
        }
        let expiry = u64::from_le_bytes(
            header[8..16]
                .try_into()
                .map_err(|_| invalid("persistent remove TTL"))?,
        );
        let live = expiry == 0 || expiry > Self::now_seconds()?;
        cancellation.check()?;
        directory.remove_pinned_file(&leaf, &file)?;
        directory.sync()?;
        Ok(live)
    }
}
/// One native operation's authorization/cancellation fence. Borrowing the broker
/// does not grant authority: dispatch, effect, and delivery still check tickets.
pub struct StorageOperation<'a> {
    pub broker: &'a mut PermissionBroker,
    pub channel: &'a Channel,
    pub demand: &'a str,
    pub cancellation: &'a StorageCancellation,
}
pub struct SelectedRead<'a> {
    pub leaf: Option<&'a str>,
    pub maximum: usize,
}
pub struct SelectedWrite<'a> {
    pub leaf: &'a str,
    pub bytes: &'a [u8],
    pub mode: WriteMode,
}
/// Cache/state keys share the principal-bound namespace but occupy separate files.
pub struct NamespaceKey<'a> {
    pub key: &'a str,
    pub state: bool,
}
pub fn read_selected(
    operation: StorageOperation<'_>,
    selected: &SelectedStorage,
    request: SelectedRead<'_>,
    quota: &QuotaGroup,
) -> Result<StorageRead> {
    let StorageOperation {
        broker,
        channel,
        demand,
        cancellation,
    } = operation;
    let SelectedRead { leaf, maximum } = request;
    selected.check_quota(quota)?;
    let _custody = reserve(quota, 4096)?;
    let ticket = dispatch(broker, channel, demand, selected.need(false)?)?;
    let result = (|| {
        cancellation.check()?;
        let file = broker
            .commit(&ticket, || selected.open(leaf))
            .map_err(auth)??;
        bounded_read(&file, maximum, cancellation, quota)
    })();
    match result {
        Ok(value) => Ok(StorageRead {
            ticket,
            cancellation: cancellation.clone(),
            value,
        }),
        Err(error) => {
            let _ = broker.settle_without_delivery(&ticket);
            Err(error)
        }
    }
}
pub fn list_selected(
    broker: &mut PermissionBroker,
    channel: &Channel,
    demand: &str,
    selected: &SelectedStorage,
    maximum: usize,
    cancellation: &StorageCancellation,
    quota: &QuotaGroup,
) -> Result<StorageListing> {
    if maximum == 0 || maximum > 1024 {
        return Err(invalid("directory entry bound"));
    }
    selected.check_quota(quota)?;
    let SelectedNative::Folder(root) = &selected.native else {
        return Err(invalid("selected file cannot be listed"));
    };
    let admission = reserve(quota, maximum * 512 + 65536)?;
    selected.check_quota(quota)?;
    let _custody = reserve(quota, 4096)?;
    let ticket = dispatch(broker, channel, demand, selected.need(false)?)?;
    let result = (|| {
        cancellation.check()?;
        broker.commit(&ticket, || ()).map_err(auth)?;
        let entries = root.list(maximum)?;
        cancellation.check()?;
        Ok(RetainedListing {
            entries,
            _admission: admission,
        })
    })();
    match result {
        Ok(value) => Ok(StorageListing {
            ticket,
            cancellation: cancellation.clone(),
            value,
        }),
        Err(error) => {
            let _ = broker.settle_without_delivery(&ticket);
            Err(error)
        }
    }
}
#[derive(Debug)]
pub struct WriteReceipt {
    pub bytes: usize,
    pub sha256: [u8; 32],
    pub identity: FileIdentity,
    pub durable: bool,
    pub metadata_preserved: bool,
    _admission: StorageAdmission,
}
/// Both temporary creation and final rename have their OWN fresh authorization
/// tickets. Bulk writes and file/directory fsync never run under the broker lock.
fn write_root(
    operation: StorageOperation<'_>,
    need: OperationNeed,
    root: &Arc<PinnedDirectory>,
    request: SelectedWrite<'_>,
    quota: &QuotaGroup,
) -> Result<WriteReceipt> {
    let StorageOperation {
        broker,
        channel,
        demand,
        cancellation,
    } = operation;
    let SelectedWrite { leaf, bytes, mode } = request;
    validate_leaf(leaf)?;
    if bytes.len() > MAX_BYTES {
        return Err(invalid("native write exceeds size limit"));
    }
    let custody = reserve(quota, 32768)?;
    cancellation.check()?;
    let preparation = dispatch(broker, channel, demand, need.clone())?;
    let result = (|| {
        let mut stage = broker
            .commit(&preparation, || root.begin_atomic(leaf, mode))
            .map_err(auth)??;
        for chunk in bytes.chunks(16384) {
            cancellation.check()?;
            stage.write(chunk, MAX_BYTES)?;
        }
        stage.prepare_durable()?;
        cancellation.check()?;
        Ok(stage)
    })();
    let stage = match result {
        Ok(stage) => stage,
        Err(error) => {
            let _ = broker.settle_without_delivery(&preparation);
            return Err(error);
        }
    };
    broker.settle_without_delivery(&preparation).map_err(auth)?;
    let publication = dispatch(broker, channel, demand, need)?;
    let mut stage = stage;
    let result = (|| {
        cancellation.check()?;
        let identity = broker
            .commit(&publication, || stage.publish_entry())
            .map_err(auth)??;
        stage.durable_ack()?;
        cancellation.check()?;
        let receipt = WriteReceipt {
            bytes: bytes.len(),
            sha256: Sha256::digest(bytes).into(),
            identity,
            durable: true,
            metadata_preserved: false,
            _admission: custody,
        };
        broker.deliver(&publication, || receipt).map_err(auth)
    })();
    if result.is_err() {
        let _ = broker.settle_without_delivery(&publication);
    }
    result
}
pub fn write_selected(
    operation: StorageOperation<'_>,
    selected: &SelectedStorage,
    request: SelectedWrite<'_>,
    quota: &QuotaGroup,
) -> Result<WriteReceipt> {
    selected.check_quota(quota)?;
    let SelectedNative::Folder(root) = &selected.native else {
        return Err(invalid(
            "atomic writes to a selected file need a safely bound parent-entry policy; unsupported",
        ));
    };
    write_root(operation, selected.need(true)?, root, request, quota)
}
pub struct MemoryNamespace {
    principal_digest: [u8; 32],
    quota: QuotaGroup,
    entries: BTreeMap<String, MemoryEntry>,
    bytes: usize,
    _admission: StorageAdmission,
}
struct MemoryEntry {
    value: Arc<RetainedBytes>,
    expires_at: Option<Instant>,
}
impl MemoryNamespace {
    pub fn new(principal: &PackageIdentity, quota: QuotaGroup) -> Result<Self> {
        let admission = reserve(&quota, 64 * 1024)?;
        let principal_digest = Sha256::digest(serde_json::to_vec(principal)?).into();
        Ok(Self {
            principal_digest,
            quota,
            entries: BTreeMap::new(),
            bytes: 0,
            _admission: admission,
        })
    }
    pub fn principal_digest(&self) -> [u8; 32] {
        self.principal_digest
    }
    pub fn put(&mut self, key: &str, bytes: &[u8]) -> Result<()> {
        self.put_with_ttl(key, bytes, None)
    }
    pub fn put_with_ttl(
        &mut self,
        key: &str,
        bytes: &[u8],
        ttl_seconds: Option<u64>,
    ) -> Result<()> {
        validate_leaf(key)?;
        if key.len() > 128 || ttl_seconds.is_some_and(|ttl| ttl == 0 || ttl > 31_536_000) {
            return Err(invalid("cache key/TTL bound"));
        }
        let expires_at = ttl_seconds
            .map(|ttl| {
                Instant::now()
                    .checked_add(Duration::from_secs(ttl))
                    .ok_or_else(|| invalid("cache TTL overflow"))
            })
            .transpose()?;
        self.prune_expired();
        if self.entries.len() >= 64 && !self.entries.contains_key(key) {
            return Err(invalid("cache key/count bound"));
        }
        let previous = self
            .entries
            .get(key)
            .map_or(0, |entry| entry.value.view().len());
        let total = self
            .bytes
            .checked_sub(previous)
            .and_then(|size| size.checked_add(bytes.len()))
            .ok_or_else(|| invalid("in-memory namespace size overflow"))?;
        if total > MAX_BYTES {
            return Err(invalid("in-memory namespace byte bound"));
        }
        let admission = reserve(&self.quota, bytes.len() + 512)?;
        let value = Arc::new(RetainedBytes {
            bytes: bytes.to_vec(),
            _admission: admission,
        });
        self.entries
            .insert(key.to_owned(), MemoryEntry { value, expires_at });
        self.bytes = total;
        Ok(())
    }
    pub fn get(&self, key: &str) -> Result<Option<Arc<RetainedBytes>>> {
        self.get_bounded(key, MAX_BYTES)
    }
    pub fn get_bounded(&self, key: &str, maximum: usize) -> Result<Option<Arc<RetainedBytes>>> {
        validate_leaf(key)?;
        if maximum == 0 || maximum > MAX_BYTES {
            return Err(invalid("cache get max_bytes"));
        }
        let Some(entry) = self.entries.get(key) else {
            return Ok(None);
        };
        if entry
            .expires_at
            .is_some_and(|deadline| deadline <= Instant::now())
        {
            return Ok(None);
        }
        if entry.value.view().len() > maximum {
            return Err(invalid("cache value exceeds caller max_bytes"));
        }
        Ok(Some(Arc::clone(&entry.value)))
    }
    pub fn remove(&mut self, key: &str) -> Result<bool> {
        validate_leaf(key)?;
        let existed = self.entries.remove(key);
        if let Some(entry) = existed {
            self.bytes = self.bytes.saturating_sub(entry.value.view().len());
            Ok(entry
                .expires_at
                .is_none_or(|deadline| deadline > Instant::now()))
        } else {
            Ok(false)
        }
    }
    fn prune_expired(&mut self) {
        let now = Instant::now();
        let mut released = 0usize;
        self.entries.retain(|_, entry| {
            if entry.expires_at.is_some_and(|deadline| deadline <= now) {
                released = released.saturating_add(entry.value.view().len());
                false
            } else {
                true
            }
        });
        self.bytes = self.bytes.saturating_sub(released);
    }
}
/// Host-owned persistence root. JS receives only cache/state keys; names are
/// hashed below the complete native principal, never its display ID/lineage alone.
/// This store never accesses the permissions ledger or remembered-denial file.
pub struct PersistentNamespace {
    principal_digest: [u8; 32],
    root: Arc<PinnedDirectory>,
    name: String,
    quota: QuotaGroup,
    principal: PackageIdentity,
    _admission: StorageAdmission,
}
impl PersistentNamespace {
    fn need(&self) -> Result<OperationNeed> {
        OperationNeed::new(
            Right {
                id: Capability::StatePersist,
                scope: Scope::Namespace {
                    name: self.name.clone(),
                },
            },
            None,
        )
        .map_err(auth)
    }
    pub fn open_from_host(
        broker: &mut PermissionBroker,
        channel: &Channel,
        demand: &str,
        principal: &PackageIdentity,
        host_root: Arc<PinnedDirectory>,
        name: String,
        quota: QuotaGroup,
    ) -> Result<Self> {
        if broker.identity() != principal {
            return Err(AnimationError::PermissionDenied(
                "persistent namespace principal mismatch".into(),
            ));
        }
        let _custody = reserve(&quota, 4096)?;
        let principal_digest: [u8; 32] = Sha256::digest(serde_json::to_vec(principal)?).into();
        validate_leaf(&name)?;
        if name.len() > 128 {
            return Err(invalid("persistent namespace name bound"));
        }
        let directory = format!(
            "principal-{:x}",
            Sha256::digest(serde_json::to_vec(&(principal, &name))?)
        );
        let need = OperationNeed::new(
            Right {
                id: Capability::StatePersist,
                scope: Scope::Namespace { name: name.clone() },
            },
            None,
        )
        .map_err(auth)?;
        let ticket = dispatch(broker, channel, demand, need)?;
        let result = (|| {
            let root = broker
                .commit(&ticket, || host_root.child(&directory, true))
                .map_err(auth)??;
            host_root.sync()?;
            broker.deliver(&ticket, || Arc::new(root)).map_err(auth)
        })();
        let root = match result {
            Ok(root) => root,
            Err(error) => {
                let _ = broker.settle_without_delivery(&ticket);
                return Err(error);
            }
        };
        Ok(Self {
            principal_digest,
            root,
            name,
            quota,
            principal: principal.clone(),
            _admission: _custody,
        })
    }
    pub fn principal_digest(&self) -> [u8; 32] {
        self.principal_digest
    }
    fn check_principal(&self, broker: &PermissionBroker) -> Result<()> {
        if broker.identity() != &self.principal {
            return Err(AnimationError::PermissionDenied(
                "persistent namespace principal mismatch".into(),
            ));
        }
        Ok(())
    }
    fn leaf(key: &str, state: bool) -> Result<String> {
        validate_leaf(key)?;
        if key.len() > 128 {
            return Err(invalid("persistent key bound"));
        }
        Ok(format!(
            "{}-{:x}.bin",
            if state { "state" } else { "cache" },
            Sha256::digest(key.as_bytes())
        ))
    }
    pub fn write(
        &self,
        operation: StorageOperation<'_>,
        key: NamespaceKey<'_>,
        bytes: &[u8],
    ) -> Result<WriteReceipt> {
        let StorageOperation {
            broker,
            channel,
            demand,
            cancellation,
        } = operation;
        let NamespaceKey { key, state } = key;
        self.check_principal(broker)?;
        cancellation.check()?;
        let leaf = Self::leaf(key, state)?;
        let _inventory_admission = reserve(&self.quota, 128 * 512 + 65536)?;
        let inventory = dispatch(broker, channel, demand, self.need()?)?;
        let checked = (|| {
            let lease = broker
                .commit(&inventory, || self.root.try_exclusive_lease())
                .map_err(auth)??;
            let entries = self.root.list(128)?;
            if entries.iter().any(|entry| entry.is_directory) {
                return Err(invalid("persistent namespace contains directories"));
            }
            let current = entries
                .iter()
                .try_fold(0u64, |total, entry| total.checked_add(entry.bytes))
                .ok_or_else(|| invalid("persistent namespace size overflow"))?;
            let existing = entries.iter().find(|entry| entry.name == leaf);
            let previous = existing.map_or(0, |entry| entry.bytes);
            let total = (current - previous)
                .checked_add(bytes.len() as u64)
                .ok_or_else(|| invalid("persistent byte count overflow"))?;
            if total > MAX_BYTES as u64 || entries.len() == 128 && existing.is_none() {
                return Err(invalid("persistent namespace byte/count bound"));
            }
            cancellation.check()?;
            Ok(lease)
        })();
        let _lease = match checked {
            Ok(lease) => lease,
            Err(error) => {
                let _ = broker.settle_without_delivery(&inventory);
                return Err(error);
            }
        };
        broker.settle_without_delivery(&inventory).map_err(auth)?;
        write_root(
            StorageOperation {
                broker,
                channel,
                demand,
                cancellation,
            },
            self.need()?,
            &self.root,
            SelectedWrite {
                leaf: &leaf,
                bytes,
                mode: WriteMode::ReplaceEntry,
            },
            &self.quota,
        )
    }
    pub fn read(
        &self,
        operation: StorageOperation<'_>,
        key: NamespaceKey<'_>,
        maximum: usize,
    ) -> Result<StorageRead> {
        let StorageOperation {
            broker,
            channel,
            demand,
            cancellation,
        } = operation;
        let NamespaceKey { key, state } = key;
        self.check_principal(broker)?;
        let leaf = Self::leaf(key, state)?;
        let ticket = dispatch(broker, channel, demand, self.need()?)?;
        let result = (|| {
            cancellation.check()?;
            let file = broker
                .commit(&ticket, || self.root.open_file(&leaf))
                .map_err(auth)??;
            bounded_read(&file, maximum, cancellation, &self.quota)
        })();
        match result {
            Ok(value) => Ok(StorageRead {
                ticket,
                cancellation: cancellation.clone(),
                value,
            }),
            Err(error) => {
                let _ = broker.settle_without_delivery(&ticket);
                Err(error)
            }
        }
    }
}
