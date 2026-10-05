//! Original-instance bundle/cache service owner. The bundle is the already
//! verified in-memory Package; no guest path names an OS file. Selected and
//! persistent resources require separately ticketed native owners.
use crate::{
    engine::{ArraySpec, CompletionState, HostRequest, ServiceValue, TypedArrayKind},
    error::{AnimationError, Result},
    native_storage::{
        MemoryNamespace, PersistentCache, RetainedBytes, RetainedListing, SelectedStorage,
        StorageCancellation,
    },
    package::valid_path,
    permissions::{Capability, OperationNeed, Right, Scope},
    plan_authorization::operation_demand,
    runtime::{PackageInstance, ServiceOperation},
};
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, QuotaGroup, Receipt, Retention,
    StorageAdmission,
};
use ilium_platform::animation_files::PinnedDirectory;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc};

const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_PENDING: usize = 32;
const MAX_ENTRIES: usize = 1024;
const REGISTRY_BYTES: usize = 64 * 1024;

fn invalid(message: &'static str) -> AnimationError {
    AnimationError::Runtime(format!("native asset: {message}"))
}

#[cfg(test)]
#[path = "native_asset_qualification_tests.rs"]
mod qualification_tests;
fn options<'a>(request: &'a HostRequest, allowed: &[&str]) -> Result<&'a Map<String, Value>> {
    let value = request
        .payload
        .metadata()
        .as_object()
        .ok_or_else(|| invalid("options record"))?;
    if value.len() > allowed.len() || value.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(invalid("unknown option"));
    }
    Ok(value)
}
fn no_planes(request: &HostRequest) -> Result<()> {
    if !request.payload.arrays().is_empty() || !request.payload.planes().is_empty() {
        return Err(invalid("unexpected binary plane"));
    }
    Ok(())
}
fn field<'a>(options: &'a Map<String, Value>, name: &str) -> Result<&'a str> {
    options
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("missing string field"))
}
fn integer(options: &Map<String, Value>, name: &str, maximum: usize) -> Result<usize> {
    options
        .get(name)
        .and_then(Value::as_u64)
        .and_then(|number| usize::try_from(number).ok())
        .filter(|number| (1..=maximum).contains(number))
        .ok_or_else(|| invalid("integer bound"))
}
fn input<'a>(request: &'a HostRequest, options: &Map<String, Value>) -> Result<&'a [u8]> {
    let marker = options
        .get("bytes")
        .and_then(Value::as_object)
        .filter(|value| value.len() == 1)
        .and_then(|value| value.get("$ilium_binary"))
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("binary marker"))?;
    let arrays = request.payload.arrays();
    if arrays.len() != 1
        || arrays[0].name != marker
        || arrays[0].kind != TypedArrayKind::U8
        || arrays[0].elements > MAX_BYTES
        || request.payload.planes().len() != 1
    {
        return Err(invalid("binary inventory"));
    }
    request
        .payload
        .planes()
        .get(marker)
        .filter(|bytes| bytes.len() == arrays[0].elements)
        .map(Vec::as_slice)
        .ok_or_else(|| invalid("binary plane"))
}
fn bundle_grant(options: &Map<String, Value>, bundle_id: &str) -> Result<()> {
    let grant = options
        .get("grant")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("asset handle projection"))?;
    if grant.len() != 2
        || grant.get("id").and_then(Value::as_str) != Some(bundle_id)
        || grant.get("kind").and_then(Value::as_str) != Some("asset")
    {
        return Err(AnimationError::PermissionDenied(
            "asset handle has no original bundle owner".into(),
        ));
    }
    Ok(())
}
fn binary_value(
    value: Value,
    bytes: Option<&[u8]>,
    instance: &PackageInstance,
    quota: &QuotaGroup,
) -> Result<ServiceValue> {
    let mut planes = BTreeMap::new();
    let mut arrays = Vec::new();
    if let Some(bytes) = bytes {
        if bytes.len() > MAX_BYTES {
            return Err(AnimationError::Budget("asset result byte bound".into()));
        }
        // The temporary plane has a distinct admission from the immutable
        // package/cache allocation and V8's later backing-store copy.
        let _scratch = quota
            .reserve_external_storage(bytes.len().saturating_add(8192))
            .map_err(|error| AnimationError::Budget(format!("asset result scratch: {error:?}")))?;
        planes.insert("b0".to_owned(), bytes.to_vec());
        arrays.push(ArraySpec {
            name: "b0".into(),
            kind: TypedArrayKind::U8,
            elements: bytes.len(),
        });
        return ServiceValue::copy_from_host(
            &json!({"ok": true, "value": value}),
            &arrays,
            &planes,
            instance.engine_limits(),
            quota.clone(),
        );
    }
    ServiceValue::copy_from_host(
        &json!({"ok": true, "value": value}),
        &arrays,
        &planes,
        instance.engine_limits(),
        quota.clone(),
    )
}
fn listing(
    instance: &PackageInstance,
    subdirectory: &str,
    maximum: usize,
    quota: &QuotaGroup,
) -> Result<Value> {
    let _scratch = quota
        .reserve_external_storage(maximum * 512 + 65536)
        .map_err(|error| AnimationError::Budget(format!("bundle listing scratch: {error:?}")))?;
    if !subdirectory.is_empty() && !valid_path(subdirectory) {
        return Err(invalid("bundle relative directory"));
    }
    let prefix = if subdirectory.is_empty() {
        "assets/".to_owned()
    } else {
        format!("{subdirectory}/")
    };
    if !prefix.starts_with("assets/") {
        return Err(invalid("bundle directory must remain in assets"));
    }
    let mut names = BTreeMap::<String, (u64, bool)>::new();
    for asset in &instance.package().manifest().assets {
        let Some(relative) = asset.path.strip_prefix(&prefix) else {
            continue;
        };
        let Some((head, tail)) = relative.split_once('/') else {
            names.insert(relative.to_owned(), (asset.bytes, false));
            continue;
        };
        if !tail.is_empty() {
            names.insert(head.to_owned(), (0, true));
        }
        if names.len() > maximum || names.len() > MAX_ENTRIES {
            return Err(AnimationError::Budget("bundle listing entry bound".into()));
        }
    }
    if names.len() > maximum {
        return Err(AnimationError::Budget("bundle listing entry bound".into()));
    }
    Ok(Value::Array(
        names
            .into_iter()
            .map(|(name, (bytes, directory))| {
                json!({"name":name,"bytes":bytes,"directory":directory})
            })
            .collect(),
    ))
}
struct Pending {
    request: HostRequest,
    value: ServiceValue,
}
enum SelectedOutput {
    Read(Arc<RetainedBytes>),
    List(RetainedListing),
    Write([u8; 32]),
}
enum SelectedCall {
    Read {
        relative: String,
        maximum: usize,
    },
    List {
        relative: String,
        maximum: usize,
    },
    Write {
        relative: String,
        bytes: Vec<u8>,
        overwrite: bool,
    },
}
struct SelectedJob {
    resource: Arc<SelectedStorage>,
    call: SelectedCall,
    cancellation: StorageCancellation,
    quota: QuotaGroup,
}
impl Job for SelectedJob {
    type Output = SelectedOutput;
    type Error = String;
    fn run(self, context: JobContext) -> std::result::Result<Self::Output, String> {
        if context.stop_token().is_stopped() {
            return Err("selected asset I/O stopped before effect".into());
        }
        let output = match self.call {
            SelectedCall::Read { relative, maximum } => SelectedOutput::Read(
                self.resource
                    .read_after_issue(&relative, maximum, &self.cancellation, &self.quota)
                    .map_err(|error| error.to_string())?,
            ),
            SelectedCall::List { relative, maximum } => SelectedOutput::List(
                self.resource
                    .list_after_issue(&relative, maximum, &self.cancellation, &self.quota)
                    .map_err(|error| error.to_string())?,
            ),
            SelectedCall::Write {
                relative,
                bytes,
                overwrite,
            } => SelectedOutput::Write(
                self.resource
                    .write_after_issue(
                        &relative,
                        &bytes,
                        overwrite,
                        &self.cancellation,
                        &self.quota,
                    )
                    .map_err(|error| error.to_string())?,
            ),
        };
        if context.stop_token().is_stopped() {
            return Err("selected asset I/O stopped after effect".into());
        }
        Ok(output)
    }
}
struct SelectedRecord {
    operation: ServiceOperation,
    receipt: Option<Receipt<SelectedJob>>,
    output: Option<std::result::Result<SelectedOutput, String>>,
    retention: Option<Retention>,
    value: Option<ServiceValue>,
    failure: Option<ServiceValue>,
}
enum PersistentCall {
    Get {
        key: String,
        maximum: usize,
    },
    Put {
        key: String,
        bytes: Vec<u8>,
        ttl: Option<u64>,
    },
    Remove {
        key: String,
    },
}
enum PersistentOutput {
    Get(Option<Arc<RetainedBytes>>),
    Put([u8; 32]),
    Remove(bool),
}
struct PersistentJob {
    store: Arc<PersistentCache>,
    call: PersistentCall,
    cancellation: StorageCancellation,
}
impl Job for PersistentJob {
    type Output = PersistentOutput;
    type Error = String;
    fn run(self, context: JobContext) -> std::result::Result<Self::Output, String> {
        if context.stop_token().is_stopped() {
            return Err("persistent cache job stopped".into());
        }
        let output = match self.call {
            PersistentCall::Get { key, maximum } => PersistentOutput::Get(
                self.store
                    .get(&key, maximum, &self.cancellation)
                    .map_err(|error| error.to_string())?,
            ),
            PersistentCall::Put { key, bytes, ttl } => PersistentOutput::Put(
                self.store
                    .put(&key, &bytes, ttl, &self.cancellation)
                    .map_err(|error| error.to_string())?,
            ),
            PersistentCall::Remove { key } => PersistentOutput::Remove(
                self.store
                    .remove(&key, &self.cancellation)
                    .map_err(|error| error.to_string())?,
            ),
        };
        if context.stop_token().is_stopped() {
            return Err("persistent cache job stopped after effect".into());
        }
        Ok(output)
    }
}
struct PersistentRecord {
    operation: ServiceOperation,
    receipt: Option<Receipt<PersistentJob>>,
    output: Option<std::result::Result<PersistentOutput, String>>,
    retention: Option<Retention>,
    value: Option<ServiceValue>,
    positive: Option<ServiceValue>,
    negative: Option<ServiceValue>,
    failure: Option<ServiceValue>,
}
/// A projection is lookup data. The returned object comes from this
/// original native asset owner, never from the guest's copied handle fields.
pub(crate) enum VideoAssetSelection<'a> {
    Bundle(&'a [u8]),
    Selected {
        request_id: String,
        resource: Arc<SelectedStorage>,
    },
}
pub struct NativeAssetHost {
    quota: QuotaGroup,
    client: Client,
    cache: MemoryNamespace,
    bundle_id: String,
    selected: BTreeMap<String, (String, Arc<SelectedStorage>)>,
    selected_pending: BTreeMap<u64, SelectedRecord>,
    persistent: Arc<PersistentCache>,
    state_request_id: Option<String>,
    persistent_pending: BTreeMap<u64, PersistentRecord>,
    cancellation: StorageCancellation,
    pending: BTreeMap<u64, Pending>,
    closed: bool,
    _metadata: StorageAdmission,
}
impl NativeAssetHost {
    pub fn new(
        instance: &PackageInstance,
        client: Client,
        quota: QuotaGroup,
        state_root: Arc<PinnedDirectory>,
    ) -> Result<Self> {
        if !instance.shares_root(&quota) || !client.quota_group().shares_root(&quota) {
            return Err(AnimationError::PermissionDenied(
                "asset host quota differs from original instance".into(),
            ));
        }
        let principal = instance.storage_principal()?;
        let state_request_id = instance.state_persist_request_id()?;
        let metadata = quota
            .reserve_external_storage(REGISTRY_BYTES)
            .map_err(|error| AnimationError::Budget(format!("asset registry: {error:?}")))?;
        if instance.selected_storage().len() > 64 {
            return Err(AnimationError::Budget("selected asset count".into()));
        }
        let mut selected = BTreeMap::new();
        for (request_id, resource) in instance.selected_storage() {
            selected.insert(
                PackageInstance::selected_asset_id(request_id),
                (request_id.clone(), Arc::clone(resource)),
            );
        }
        Ok(Self {
            client,
            cache: MemoryNamespace::new(&principal, quota.clone())?,
            bundle_id: instance.bundle_asset_id(),
            selected,
            selected_pending: BTreeMap::new(),
            persistent: Arc::new(PersistentCache::new(&principal, state_root, quota.clone())?),
            state_request_id,
            persistent_pending: BTreeMap::new(),
            cancellation: StorageCancellation::default(),
            quota,
            pending: BTreeMap::new(),
            closed: false,
            _metadata: metadata,
        })
    }
    pub fn revoke(&mut self) {
        // ACK uncertainty retains exact result/admission until physical helper
        // retirement; cache mutation is never reissued by retrying an RPC.
        self.closed = true;
        self.cancellation.cancel();
        for record in self.selected_pending.values() {
            if let Some(receipt) = &record.receipt {
                receipt.cancel();
            }
        }
        for record in self.persistent_pending.values() {
            if let Some(receipt) = &record.receipt {
                receipt.cancel();
            }
        }
    }
    pub fn release_terminal_after_helper_retirement(&mut self) {
        if self.closed {
            self.pending.clear();
            // The helper's physical exit proves only its own copy/ACK boundary.
            // A selected finite worker may still own a pinned directory, staged
            // write, or committed publication. Retain its original Receipt
            // through a genuine Ready/NotStarted outcome; Lost stays quarantined.
            self.selected_pending
                .retain(|_, record| record.receipt.is_some());
            self.persistent_pending
                .retain(|_, record| record.receipt.is_some());
        }
    }
    pub fn is_drained(&self) -> bool {
        self.closed
            && self.pending.is_empty()
            && self.selected_pending.is_empty()
            && self.persistent_pending.is_empty()
    }
    /// Resolve an SDK Video asset against the already constructed bundle and
    /// selected-resource registries. This method performs no disk read and
    /// grants no right; selected Video I/O still needs its exact broker ticket.
    pub(crate) fn video_asset<'a>(
        &self,
        instance: &'a PackageInstance,
        asset: &Value,
        relative_path: &str,
    ) -> Result<VideoAssetSelection<'a>> {
        if self.closed || !instance.shares_root(&self.quota) {
            return Err(AnimationError::PermissionDenied(
                "video asset owner retired or foreign".into(),
            ));
        }
        let projected = asset
            .as_object()
            .filter(|value| {
                value.len() == 2 && value.get("kind").and_then(Value::as_str) == Some("asset")
            })
            .ok_or_else(|| invalid("video asset handle projection"))?;
        let id = projected
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("video asset handle ID"))?;
        if id == self.bundle_id {
            if !relative_path.starts_with("assets/") || !valid_path(relative_path) {
                return Err(invalid("video bundle path"));
            }
            return Ok(VideoAssetSelection::Bundle(
                instance.package().asset(relative_path)?,
            ));
        }
        if !relative_path.is_empty() && !valid_path(relative_path) {
            return Err(invalid("video selected relative path"));
        }
        let (request_id, resource) = self.selected.get(id).ok_or_else(|| {
            AnimationError::PermissionDenied("unknown original selected video asset".into())
        })?;
        Ok(VideoAssetSelection::Selected {
            request_id: request_id.clone(),
            resource: Arc::clone(resource),
        })
    }

    fn selected_handle(
        &self,
        request: &HostRequest,
    ) -> Result<Option<(String, Arc<SelectedStorage>)>> {
        if !matches!(
            request.method.as_str(),
            "assets.read" | "assets.list" | "assets.write"
        ) {
            return Ok(None);
        }
        let fields = request
            .payload
            .metadata()
            .as_object()
            .ok_or_else(|| invalid("asset options"))?;
        let grant = fields
            .get("grant")
            .and_then(Value::as_object)
            .filter(|grant| {
                grant.len() == 2 && grant.get("kind").and_then(Value::as_str) == Some("asset")
            })
            .ok_or_else(|| invalid("asset handle"))?;
        let id = grant
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("asset handle ID"))?;
        if id == self.bundle_id {
            return Ok(None);
        }
        self.selected
            .get(id)
            .map(|(request_id, selected)| (request_id.clone(), Arc::clone(selected)))
            .map(Some)
            .ok_or_else(|| {
                AnimationError::PermissionDenied("unknown original selected asset handle".into())
            })
    }
    fn dispatch_selected(
        &mut self,
        instance: &mut PackageInstance,
        request: HostRequest,
        request_id: String,
        resource: Arc<SelectedStorage>,
    ) -> Result<Option<HostRequest>> {
        let writing = request.method == "assets.write";
        if !writing {
            no_planes(&request)?;
        }
        let mut input_bytes: usize = 16 * 1024;
        let (success, failure) = if writing {
            let fields = options(&request, &["grant", "relative_path", "bytes", "overwrite"])?;
            let bytes = input(&request, fields)?;
            input_bytes = input_bytes
                .checked_add(bytes.len())
                .ok_or_else(|| invalid("write input bound"))?;
            let hash = Sha256::digest(bytes);
            let success = binary_value(
                json!({"sha256":format!("{hash:x}")}),
                None,
                instance,
                &self.quota,
            )?;
            let failure = ServiceValue::copy_from_host(
                &json!({"ok":false,"error":{"code":"storage_failed","message":"Native selected write failed or durable outcome is uncertain."}}),
                &[],
                &BTreeMap::new(),
                instance.engine_limits(),
                self.quota.clone(),
            )?;
            (Some(success), Some(failure))
        } else {
            (None, None)
        };
        let reservation = self
            .client
            .try_reserve(
                Lane::Io,
                JobCost {
                    input_bytes,
                    result_bytes: MAX_BYTES + 256 * 1024,
                },
            )
            .map_err(|error| {
                AnimationError::Budget(format!("selected asset job admission: {error:?}"))
            })?;
        let call = match request.method.as_str() {
            "assets.read" => {
                let fields = options(&request, &["grant", "relative_path", "max_bytes"])?;
                let relative = field(fields, "relative_path")?;
                if !relative.is_empty() && !valid_path(relative) {
                    return Err(invalid("selected relative path"));
                }
                SelectedCall::Read {
                    relative: relative.to_owned(),
                    maximum: integer(fields, "max_bytes", MAX_BYTES)?,
                }
            }
            "assets.list" => {
                let fields = options(&request, &["grant", "relative_path", "max_entries"])?;
                let relative = match fields.get("relative_path") {
                    Some(value) => value
                        .as_str()
                        .ok_or_else(|| invalid("selected list path"))?,
                    None => "",
                };
                if !relative.is_empty() && !valid_path(relative) {
                    return Err(invalid("selected relative directory"));
                }
                SelectedCall::List {
                    relative: relative.to_owned(),
                    maximum: integer(fields, "max_entries", MAX_ENTRIES)?,
                }
            }
            "assets.write" => {
                let fields = options(&request, &["grant", "relative_path", "bytes", "overwrite"])?;
                let relative = field(fields, "relative_path")?;
                if !valid_path(relative) {
                    return Err(invalid("selected write relative path"));
                }
                let overwrite = fields
                    .get("overwrite")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| invalid("selected overwrite flag"))?;
                SelectedCall::Write {
                    relative: relative.to_owned(),
                    bytes: input(&request, fields)?.to_vec(),
                    overwrite,
                }
            }
            _ => return Err(invalid("selected asset call")),
        };
        let need = resource.operation_need(writing)?;
        let operation =
            instance.dispatch_service(request, &operation_demand(&request_id), vec![need])?;
        let id = operation.request().id;
        self.selected_pending.insert(
            id,
            SelectedRecord {
                operation,
                receipt: None,
                output: None,
                retention: None,
                value: success,
                failure,
            },
        );
        let job = SelectedJob {
            resource,
            call,
            cancellation: self.cancellation.clone(),
            quota: self.quota.clone(),
        };
        let record = self
            .selected_pending
            .get_mut(&id)
            .ok_or_else(|| invalid("selected custody missing"))?;
        match instance.commit_service(&record.operation, || reservation.submit(job)) {
            Ok(Ok(receipt)) => {
                record.receipt = Some(receipt);
                Ok(None)
            }
            Ok(Err(error)) => {
                self.closed = true;
                Err(AnimationError::Budget(format!(
                    "selected asset issue refused: {:?}",
                    error.reason
                )))
            }
            Err(error) => {
                self.closed = true;
                Err(error)
            }
        }
    }
    fn dispatch_persistent(
        &mut self,
        instance: &mut PackageInstance,
        request: HostRequest,
        state_request_id: String,
    ) -> Result<Option<HostRequest>> {
        let writing = request.method == "cache.put";
        if !writing {
            no_planes(&request)?;
        }
        let mut input_bytes: usize = 16 * 1024;
        let mut success = None;
        let mut positive = None;
        let mut negative = None;
        let mut failure = None;
        if writing {
            let fields = options(&request, &["key", "bytes", "persistent", "ttl_seconds"])?;
            if fields.get("persistent") != Some(&Value::Bool(true)) {
                return Err(invalid("persistent cache flag"));
            }
            let bytes = input(&request, fields)?;
            input_bytes = input_bytes
                .checked_add(bytes.len())
                .ok_or_else(|| invalid("persistent input bound"))?;
            success = Some(binary_value(
                json!({"sha256":format!("{:x}",Sha256::digest(bytes))}),
                None,
                instance,
                &self.quota,
            )?);
        } else if request.method == "cache.remove" {
            positive = Some(binary_value(json!(true), None, instance, &self.quota)?);
            negative = Some(binary_value(json!(false), None, instance, &self.quota)?);
        }
        if writing || request.method == "cache.remove" {
            failure = Some(ServiceValue::copy_from_host(
                &json!({"ok":false,"error":{"code":"storage_failed","message":"Native persistent cache effect failed or durability is uncertain."}}),
                &[],
                &BTreeMap::new(),
                instance.engine_limits(),
                self.quota.clone(),
            )?);
        }
        let reservation = self
            .client
            .try_reserve(
                Lane::Io,
                JobCost {
                    input_bytes,
                    result_bytes: MAX_BYTES + 256 * 1024,
                },
            )
            .map_err(|error| {
                AnimationError::Budget(format!("persistent cache job admission: {error:?}"))
            })?;
        let call = match request.method.as_str() {
            "cache.get" => {
                let fields = options(&request, &["key", "max_bytes"])?;
                PersistentCall::Get {
                    key: field(fields, "key")?.to_owned(),
                    maximum: integer(fields, "max_bytes", MAX_BYTES)?,
                }
            }
            "cache.put" => {
                let fields = options(&request, &["key", "bytes", "persistent", "ttl_seconds"])?;
                let ttl = fields
                    .get("ttl_seconds")
                    .map(|value| value.as_u64().ok_or_else(|| invalid("persistent TTL")))
                    .transpose()?;
                PersistentCall::Put {
                    key: field(fields, "key")?.to_owned(),
                    bytes: input(&request, fields)?.to_vec(),
                    ttl,
                }
            }
            "cache.remove" => {
                let fields = options(&request, &["key"])?;
                PersistentCall::Remove {
                    key: field(fields, "key")?.to_owned(),
                }
            }
            _ => return Err(invalid("persistent method")),
        };
        let need = OperationNeed::new(
            Right {
                id: Capability::StatePersist,
                scope: Scope::Namespace {
                    name: "session".into(),
                },
            },
            None,
        )
        .map_err(|error| AnimationError::PermissionDenied(error.to_string()))?;
        let operation =
            instance.dispatch_service(request, &operation_demand(&state_request_id), vec![need])?;
        let id = operation.request().id;
        self.persistent_pending.insert(
            id,
            PersistentRecord {
                operation,
                receipt: None,
                output: None,
                retention: None,
                value: success,
                positive,
                negative,
                failure,
            },
        );
        let job = PersistentJob {
            store: Arc::clone(&self.persistent),
            call,
            cancellation: self.cancellation.clone(),
        };
        let record = self
            .persistent_pending
            .get_mut(&id)
            .ok_or_else(|| invalid("persistent custody missing"))?;
        match instance.commit_service(&record.operation, || reservation.submit(job)) {
            Ok(Ok(receipt)) => {
                record.receipt = Some(receipt);
                Ok(None)
            }
            Ok(Err(error)) => {
                self.closed = true;
                Err(AnimationError::Budget(format!(
                    "persistent cache issue refused: {:?}",
                    error.reason
                )))
            }
            Err(error) => {
                self.closed = true;
                Err(error)
            }
        }
    }
    fn apply_selected_poll(&mut self, id: u64, poll: JobPoll<SelectedJob>) -> Result<()> {
        let record = self
            .selected_pending
            .get_mut(&id)
            .ok_or_else(|| invalid("selected receipt owner missing"))?;
        match poll {
            JobPoll::Pending => {}
            JobPoll::Ready(outcome) => {
                record.receipt = None;
                let (outcome, retention) = outcome.into_parts();
                record.retention = Some(retention);
                record.output = Some(match outcome {
                    JobOutcome::Finished(result) => result,
                    JobOutcome::NotStarted { .. } => Err("selected asset job did not start".into()),
                    JobOutcome::Panicked => Err("selected asset job panicked".into()),
                });
            }
            JobPoll::Lost | JobPoll::Taken => {
                self.closed = true;
                return Err(invalid(
                    "selected asset receipt lost; physical exit unknown",
                ));
            }
        }
        Ok(())
    }
    pub fn on_completion_wake(&mut self, instance: &mut PackageInstance) -> Result<()> {
        let ids: Vec<u64> = self.selected_pending.keys().copied().collect();
        for id in ids {
            let poll = self
                .selected_pending
                .get_mut(&id)
                .and_then(|record| record.receipt.as_mut().map(Receipt::try_take));
            if let Some(poll) = poll {
                self.apply_selected_poll(id, poll)?;
            }
            let Some(record) = self.selected_pending.get_mut(&id) else {
                continue;
            };
            if self.closed || record.output.is_none() {
                continue;
            }
            if record
                .output
                .as_ref()
                .is_some_and(std::result::Result::is_err)
            {
                if let Some(failure) = record.failure.take() {
                    record.value = Some(failure);
                }
            }
            if let Some(Ok(SelectedOutput::Write(hash))) = record.output.as_ref() {
                let original = record.operation.request();
                let fields = options(original, &["grant", "relative_path", "bytes", "overwrite"])?;
                let expected: [u8; 32] = Sha256::digest(input(original, fields)?).into();
                if *hash != expected {
                    self.closed = true;
                    return Err(invalid(
                        "selected write receipt differs from original bytes",
                    ));
                }
            }
            if record.value.is_none() {
                let value = match record
                    .output
                    .as_ref()
                    .ok_or_else(|| invalid("selected result missing"))?
                {
                    Ok(SelectedOutput::Read(bytes)) => {
                        let relative = record
                            .operation
                            .request()
                            .payload
                            .metadata()
                            .get("relative_path")
                            .and_then(Value::as_str)
                            .ok_or_else(|| invalid("selected result path"))?;
                        let name = relative.rsplit('/').next().unwrap_or("");
                        binary_value(
                            json!({"bytes":{"$ilium_binary":"b0"},
                            "sha256":format!("{:x}", Sha256::digest(bytes.view())), "name":name}),
                            Some(bytes.view()),
                            instance,
                            &self.quota,
                        )?
                    }
                    Ok(SelectedOutput::List(listing)) => {
                        let _scratch = self
                            .quota
                            .reserve_external_storage(listing.view().len() * 512 + 65536)
                            .map_err(|error| {
                                AnimationError::Budget(format!(
                                    "selected listing projection: {error:?}"
                                ))
                            })?;
                        let entries: Vec<Value> = listing.view().iter().map(|entry|
                            json!({"name":entry.name,"bytes":entry.bytes,"directory":entry.is_directory})).collect();
                        binary_value(Value::Array(entries), None, instance, &self.quota)?
                    }
                    Ok(SelectedOutput::Write(_)) => {
                        return Err(invalid("selected write result admission missing"))
                    }
                    Err(_) => ServiceValue::copy_from_host(
                        &json!({"ok":false,"error":{"code":"storage_failed","message":"Native selected asset I/O failed."}}),
                        &[],
                        &BTreeMap::new(),
                        instance.engine_limits(),
                        self.quota.clone(),
                    )?,
                };
                record.value = Some(value);
            }
            let value = record
                .value
                .as_ref()
                .ok_or_else(|| invalid("selected value missing"))?
                .clone();
            match instance.complete_authorized(&record.operation, value) {
                Ok(CompletionState::Delivered) => {
                    self.selected_pending.remove(&id);
                }
                Ok(_) => {
                    self.closed = true;
                }
                Err(error) => {
                    self.closed = true;
                    return Err(error);
                }
            }
        }
        let ids: Vec<u64> = self.persistent_pending.keys().copied().collect();
        for id in ids {
            let Some(record) = self.persistent_pending.get_mut(&id) else {
                continue;
            };
            if let Some(receipt) = record.receipt.as_mut() {
                let outcome = match receipt.try_take() {
                    JobPoll::Pending => continue,
                    JobPoll::Ready(outcome) => outcome,
                    JobPoll::Lost | JobPoll::Taken => {
                        self.closed = true;
                        return Err(invalid(
                            "persistent cache receipt lost; physical exit unknown",
                        ));
                    }
                };
                record.receipt = None;
                let (outcome, retention) = outcome.into_parts();
                record.retention = Some(retention);
                record.output = Some(match outcome {
                    JobOutcome::Finished(result) => result,
                    JobOutcome::NotStarted { .. } => {
                        Err("persistent cache job did not start".into())
                    }
                    JobOutcome::Panicked => {
                        Err("persistent cache job panicked; effect uncertain".into())
                    }
                });
            }
            if self.closed || record.output.is_none() {
                continue;
            }
            if record
                .output
                .as_ref()
                .is_some_and(std::result::Result::is_err)
            {
                if let Some(failure) = record.failure.take() {
                    record.value = Some(failure);
                }
            }
            if record.value.is_none() {
                let value = match record
                    .output
                    .as_ref()
                    .ok_or_else(|| invalid("persistent result missing"))?
                {
                    Ok(PersistentOutput::Get(Some(bytes))) => binary_value(
                        json!({"$ilium_binary":"b0"}),
                        Some(bytes.view()),
                        instance,
                        &self.quota,
                    )?,
                    Ok(PersistentOutput::Get(None)) => {
                        binary_value(Value::Null, None, instance, &self.quota)?
                    }
                    Ok(PersistentOutput::Remove(found)) => {
                        let key = record
                            .operation
                            .request()
                            .payload
                            .metadata()
                            .get("key")
                            .and_then(Value::as_str)
                            .ok_or_else(|| invalid("persistent remove key"))?;
                        let memory_removed = self.cache.remove(key)?;
                        if *found || memory_removed {
                            record
                                .positive
                                .take()
                                .ok_or_else(|| invalid("persistent positive admission missing"))?
                        } else {
                            record
                                .negative
                                .take()
                                .ok_or_else(|| invalid("persistent negative admission missing"))?
                        }
                    }
                    Ok(PersistentOutput::Put(hash)) => {
                        let original = record.operation.request();
                        let fields =
                            options(original, &["key", "bytes", "persistent", "ttl_seconds"])?;
                        let expected: [u8; 32] = Sha256::digest(input(original, fields)?).into();
                        if *hash != expected {
                            self.closed = true;
                            return Err(invalid(
                                "persistent write receipt differs from original bytes",
                            ));
                        }
                        return Err(invalid("persistent write success admission missing"));
                    }
                    Err(_) => ServiceValue::copy_from_host(
                        &json!({"ok":false,"error":{"code":"storage_failed","message":"Native persistent cache I/O failed."}}),
                        &[],
                        &BTreeMap::new(),
                        instance.engine_limits(),
                        self.quota.clone(),
                    )?,
                };
                record.value = Some(value);
            }
            if let Some(Ok(PersistentOutput::Put(hash))) = record.output.as_ref() {
                let original = record.operation.request();
                let fields = options(original, &["key", "bytes", "persistent", "ttl_seconds"])?;
                let expected: [u8; 32] = Sha256::digest(input(original, fields)?).into();
                if *hash != expected {
                    self.closed = true;
                    return Err(invalid(
                        "persistent write receipt differs from original bytes",
                    ));
                }
            }
            let value = record
                .value
                .as_ref()
                .ok_or_else(|| invalid("persistent value missing"))?
                .clone();
            match instance.complete_authorized(&record.operation, value) {
                Ok(CompletionState::Delivered) => {
                    self.persistent_pending.remove(&id);
                }
                Ok(_) => {
                    self.closed = true;
                }
                Err(error) => {
                    self.closed = true;
                    return Err(error);
                }
            }
        }
        Ok(())
    }
    pub fn dispatch(
        &mut self,
        instance: &mut PackageInstance,
        request: HostRequest,
    ) -> Result<Option<HostRequest>> {
        if !matches!(
            request.method.as_str(),
            "assets.read"
                | "assets.list"
                | "assets.write"
                | "cache.get"
                | "cache.put"
                | "cache.remove"
        ) {
            return Ok(Some(request));
        }
        if self.closed
            || !request.payload.shares_root(&self.quota)
            || self.pending.len() + self.selected_pending.len() + self.persistent_pending.len()
                >= MAX_PENDING
            || self.pending.contains_key(&request.id)
            || self.selected_pending.contains_key(&request.id)
            || self.persistent_pending.contains_key(&request.id)
        {
            return Err(AnimationError::Budget(
                "asset owner closed, foreign, or full".into(),
            ));
        }
        if let Some((request_id, resource)) = self.selected_handle(&request)? {
            return self.dispatch_selected(instance, request, request_id, resource);
        }
        if let Some(state_request_id) = self.state_request_id.clone() {
            let persistent = match request.method.as_str() {
                "cache.put" => {
                    request.payload.metadata().get("persistent") == Some(&Value::Bool(true))
                }
                "cache.remove" => true,
                "cache.get" => {
                    let fields = options(&request, &["key", "max_bytes"])?;
                    let key = field(fields, "key")?;
                    let maximum = integer(fields, "max_bytes", MAX_BYTES)?;
                    matches!(self.cache.get_bounded(key, maximum), Ok(None))
                }
                _ => false,
            };
            if persistent {
                return self.dispatch_persistent(instance, request, state_request_id);
            }
        }
        instance.check_baseline_storage(&request)?;
        let result = self.handle(instance, &request);
        let value = match result {
            Ok(value) => value,
            Err(error) => {
                let code = match error {
                    AnimationError::Budget(_) => "budget_exceeded",
                    AnimationError::PermissionDenied(_) => "permission_denied",
                    _ => "storage_failed",
                };
                ServiceValue::copy_from_host(
                    &json!({"ok":false,"error":{"code":code,"message":"Native storage request was refused."}}),
                    &[],
                    &BTreeMap::new(),
                    instance.engine_limits(),
                    self.quota.clone(),
                )?
            }
        };
        let id = request.id;
        self.pending.insert(id, Pending { request, value });
        let pending = self
            .pending
            .get(&id)
            .ok_or_else(|| invalid("pending custody"))?;
        match instance.complete_baseline_storage(&pending.request, pending.value.clone()) {
            Ok(CompletionState::Delivered) => {
                self.pending.remove(&id);
                Ok(None)
            }
            Ok(_) => {
                self.closed = true;
                Ok(None)
            }
            Err(error) => {
                self.closed = true;
                Err(error)
            }
        }
    }
    fn handle(
        &mut self,
        instance: &PackageInstance,
        request: &HostRequest,
    ) -> Result<ServiceValue> {
        match request.method.as_str() {
            "assets.read" => {
                no_planes(request)?;
                let fields = options(request, &["grant", "relative_path", "max_bytes"])?;
                bundle_grant(fields, &self.bundle_id)?;
                let relative = field(fields, "relative_path")?;
                if !valid_path(relative) {
                    return Err(invalid("bundle relative path"));
                }
                let maximum = integer(fields, "max_bytes", MAX_BYTES)?;
                if !relative.starts_with("assets/") {
                    return Err(invalid("bundle path must remain in assets"));
                }
                let bytes = instance.package().asset(relative)?;
                if bytes.len() > maximum {
                    return Err(AnimationError::Budget(
                        "bundle read exceeds max_bytes".into(),
                    ));
                }
                let name = relative
                    .rsplit('/')
                    .next()
                    .ok_or_else(|| invalid("asset name"))?;
                let hash = Sha256::digest(bytes);
                binary_value(
                    json!({"bytes":{"$ilium_binary":"b0"},"sha256":format!("{hash:x}"),"name":name}),
                    Some(bytes),
                    instance,
                    &self.quota,
                )
            }
            "assets.list" => {
                no_planes(request)?;
                let fields = options(request, &["grant", "relative_path", "max_entries"])?;
                bundle_grant(fields, &self.bundle_id)?;
                let relative = match fields.get("relative_path") {
                    Some(value) => value
                        .as_str()
                        .ok_or_else(|| invalid("list relative path"))?,
                    None => "",
                };
                let maximum = integer(fields, "max_entries", MAX_ENTRIES)?;
                binary_value(
                    listing(instance, relative, maximum, &self.quota)?,
                    None,
                    instance,
                    &self.quota,
                )
            }
            "assets.write" => Err(AnimationError::PermissionDenied(
                "verified package bundle is read-only".into(),
            )),
            "cache.get" => {
                no_planes(request)?;
                let fields = options(request, &["key", "max_bytes"])?;
                let key = field(fields, "key")?;
                let maximum = integer(fields, "max_bytes", MAX_BYTES)?;
                let value = self.cache.get_bounded(key, maximum)?;
                binary_value(
                    if value.is_some() {
                        json!({"$ilium_binary":"b0"})
                    } else {
                        Value::Null
                    },
                    value.as_ref().map(|value| value.view()),
                    instance,
                    &self.quota,
                )
            }
            "cache.put" => {
                let fields = options(request, &["key", "bytes", "persistent", "ttl_seconds"])?;
                let key = field(fields, "key")?;
                if !matches!(fields.get("persistent"), None | Some(Value::Bool(false))) {
                    return Err(AnimationError::PermissionDenied(
                        "persistent cache requires a selected native state namespace".into(),
                    ));
                }
                let ttl = fields
                    .get("ttl_seconds")
                    .map(|value| value.as_u64().ok_or_else(|| invalid("cache TTL")))
                    .transpose()?;
                let bytes = input(request, fields)?;
                let hash = Sha256::digest(bytes);
                let success = binary_value(
                    json!({"sha256":format!("{hash:x}")}),
                    None,
                    instance,
                    &self.quota,
                )?;
                self.cache.put_with_ttl(key, bytes, ttl)?;
                Ok(success)
            }
            "cache.remove" => {
                no_planes(request)?;
                let fields = options(request, &["key"])?;
                let key = field(fields, "key")?;
                let present = binary_value(json!(true), None, instance, &self.quota)?;
                let absent = binary_value(json!(false), None, instance, &self.quota)?;
                let removed = self.cache.remove(key)?;
                Ok(if removed { present } else { absent })
            }
            _ => Err(invalid("unhandled asset method")),
        }
    }
}
