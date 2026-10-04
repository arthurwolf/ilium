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
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
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
    if file.len()? > maximum as u64 {
        return Err(invalid("selected file exceeds read limit"));
    }
    let admission = reserve(quota, maximum + 16384)?;
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
    entries: BTreeMap<String, Arc<RetainedBytes>>,
    bytes: usize,
    _admission: StorageAdmission,
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
        validate_leaf(key)?;
        if key.len() > 128 || self.entries.len() >= 64 && !self.entries.contains_key(key) {
            return Err(invalid("cache key/count bound"));
        }
        let previous = self.entries.get(key).map_or(0, |entry| entry.view().len());
        let total = self.bytes - previous + bytes.len();
        if total > MAX_BYTES {
            return Err(invalid("in-memory namespace byte bound"));
        }
        let admission = reserve(&self.quota, bytes.len() + 512)?;
        let value = Arc::new(RetainedBytes {
            bytes: bytes.to_vec(),
            _admission: admission,
        });
        self.entries.insert(key.to_owned(), value);
        self.bytes = total;
        Ok(())
    }
    pub fn get(&self, key: &str) -> Result<Option<Arc<RetainedBytes>>> {
        validate_leaf(key)?;
        Ok(self.entries.get(key).map(Arc::clone))
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
