//! Demand-acquired generated-world owner for one accepted native activation.
//! Script ids are lookups into retained WorldService/NativeDrawHost state, not
//! grants. Selected saved-world reads require the separate original DiskRead
//! operation and handle-based loader; no pathname-derived substitute exists.
use crate::{
    engine::{CompletionState, EngineLimits, HostRequest, ServiceValue},
    error::{AnimationError, Result},
    manifest::AnimationMode,
    native_asset_host::NativeAssetHost,
    native_draw_host::NativeDrawHost,
    native_saved_factory::{NativeSavedFactory, NativeSavedSource},
    native_storage::{ProtectedWorldHistory, SelectedStorage, StorageCancellation},
    native_world_region::copy_region_response_with_stop,
    native_worlds::{
        GeneratedWorldSettings, HostWorldScene, NativeWorldFrame, SavedWorldFactory,
        SavedWorldGrant, SourceIdentity, WorldHandle, WorldRenderRequest, WorldService,
    },
    plan_authorization::operation_demand,
    runtime::{
        HelperRetirementEvidence, PackageInstance, ServiceGroupSettlement, ServiceOperation,
    },
    world_region::{parse_region_request, RegionLimits},
};
use ilium_ambient::{
    minecraft::{catalog, nbt, region},
    resources::AmbientResources,
    scene::SceneEnv,
    VoxelLandscapeSettings,
};
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, QuotaGroup, Receipt, Retention,
    StorageAdmission,
};
use ilium_platform::{
    animation_files::{FileIdentity, PinnedDirectory},
    owned_worker::StopToken,
    secure_fs::NoFollowDirectory,
};
use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io,
    sync::Arc,
    time::{Duration, SystemTime},
};

const MAX_WORLD_HANDLES: usize = 8;
const MAX_FRAME_HANDLES: usize = 64;
const MAX_PENDING_LISTS: usize = 2;
const MAX_LIST_ENTRIES: usize = 64;
const MAX_LEVEL_BYTES: usize = 2 * 1024 * 1024;
const WORLD_LIST_WORK_BYTES: usize = 32 * 1024 * 1024;
const MAX_REGION_CELLS: usize = 65_536;
const REGION_WORK_PER_CELL: usize = 256;
const MIN_REGION_PALETTE_METADATA_BYTES: usize = 70;
/// Count every pending output at its requested maximum until the original
/// operation has been delivered or settled. Existing rows for the same grant
/// may be replaced atomically only after that delivery ACK.
fn selected_list_capacity(
    retained: usize,
    replaced: usize,
    pending_maxima: usize,
    requested: usize,
) -> bool {
    retained
        .checked_sub(replaced)
        .and_then(|current| current.checked_add(pending_maxima))
        .and_then(|current| current.checked_add(requested))
        .is_some_and(|total| total <= MAX_LIST_ENTRIES * MAX_PENDING_LISTS)
}
fn invalid(reason: &str) -> AnimationError {
    AnimationError::Runtime(format!("native worlds: {reason}"))
}

#[derive(Clone, Copy, Debug)]
struct RegionHostPolicy {
    limits: RegionLimits,
    receiving_bytes: usize,
}

fn region_host_policy(engine: &EngineLimits) -> Result<RegionHostPolicy> {
    let receiving_bytes = engine
        .json_bytes
        .checked_add(engine.backing_bytes)
        .ok_or_else(|| AnimationError::Budget("world region receiving-byte overflow".into()))?;
    let cells = (engine.backing_bytes / 2).min(MAX_REGION_CELLS);
    if cells == 0 {
        return Err(AnimationError::Budget(
            "world region backing store cannot hold one cell".into(),
        ));
    }
    let palette_from_json = (engine.json_bytes / MIN_REGION_PALETTE_METADATA_BYTES).max(1);
    let palette = cells.min(usize::from(u16::MAX) + 1).min(palette_from_json);
    let work = cells
        .checked_mul(REGION_WORK_PER_CELL)
        .ok_or_else(|| AnimationError::Budget("world region collector-work overflow".into()))?;
    Ok(RegionHostPolicy {
        limits: RegionLimits {
            cells,
            palette,
            work,
        },
        receiving_bytes,
    })
}
struct ActiveWorld {
    handle: WorldHandle,
    descriptor: Value,
    stop: Option<StopToken>,
}
struct ActiveFrame {
    world_id: String,
    descriptor: Value,
    _source: Arc<NativeWorldFrame>,
}
#[derive(Serialize)]
struct ListedWorldEntry {
    id: String,
    name: String,
    identity: String,
}
#[derive(Clone)]
struct ListedWorldChild {
    root: Arc<NoFollowDirectory>,
    parent_identity: FileIdentity,
    child_identity: FileIdentity,
    identity: String,
}
struct ListedWorlds {
    entries: Vec<ListedWorldEntry>,
    children: BTreeMap<String, ListedWorldChild>,
    _storage: StorageAdmission,
}
struct ListedWorldBinding {
    child: ListedWorldChild,
    selected: Arc<SelectedStorage>,
}
struct SelectedWorldListJob {
    root: Arc<NoFollowDirectory>,
    maximum: usize,
    epoch: u64,
    stop: StopToken,
    storage: StorageAdmission,
}
impl Job for SelectedWorldListJob {
    type Output = ListedWorlds;
    type Error = AnimationError;
    fn run(self, context: JobContext) -> Result<Self::Output> {
        if self.stop.is_stopped() || context.stop_requested() {
            return Err(invalid("selected world list stopped"));
        }
        // Every open descends from the exact selected directory descriptor.
        // No saved path, canonicalization, or path-derived permission is used.
        let root = PinnedDirectory::from_host(self.root)
            .map_err(|_| invalid("selected world root pin failed"))?;
        let root_identity = root.identity();
        let entries = root
            .list(self.maximum)
            .map_err(|_| invalid("selected world scan failed or exceeded limit"))?;
        let mut output = Vec::with_capacity(entries.len());
        let mut children = BTreeMap::new();
        let mut scratch = Vec::new();
        for entry in entries {
            if self.stop.is_stopped() || context.stop_requested() {
                return Err(invalid("selected world list stopped"));
            }
            if entry.name.len() > 128 {
                return Err(invalid("selected world directory name bound"));
            }
            if !entry.is_directory {
                continue;
            }
            let child = root
                .child(&entry.name, false)
                .map_err(|_| invalid("selected world child changed"))?;
            let file = match child.open_file("level.dat") {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(_) => return Err(invalid("selected world level.dat denied or changed")),
            };
            let length = usize::try_from(
                file.len()
                    .map_err(|_| invalid("selected world metadata length"))?,
            )
            .map_err(|_| invalid("selected world metadata length range"))?;
            if length == 0 || length > MAX_LEVEL_BYTES {
                return Err(invalid("selected world metadata exceeds limit"));
            }
            let modified = file
                .modified()
                .map_err(|_| invalid("selected world metadata age"))?;
            scratch.clear();
            scratch.resize(length, 0);
            let mut offset = 0usize;
            while offset < length {
                if self.stop.is_stopped() || context.stop_requested() {
                    return Err(invalid("selected world list stopped"));
                }
                let next = (offset + 8192).min(length);
                let count = file
                    .read_at(&mut scratch[offset..next], offset as u64)
                    .map_err(|_| invalid("selected world metadata read"))?;
                if count == 0 {
                    return Err(invalid("selected world metadata short read"));
                }
                offset += count;
            }
            if file
                .len()
                .map_err(|_| invalid("selected world metadata restat"))?
                != length as u64
                || file
                    .modified()
                    .map_err(|_| invalid("selected world metadata restat"))?
                    != modified
            {
                return Err(invalid("selected world metadata changed during scan"));
            }
            let cancelled = || self.stop.is_stopped() || context.stop_requested();
            let limits = nbt::Limits {
                max_bytes: MAX_LEVEL_BYTES,
                max_depth: 32,
                max_string_bytes: 8192,
                max_collection_len: 16384,
                max_nodes: 4096,
                max_elements: 16384,
            };
            let decoded = region::decompress(
                region::Compression::Gzip,
                &scratch,
                limits.max_bytes,
                &cancelled,
            )
            .map_err(|_| invalid("selected world level.dat decode"))?;
            let document = nbt::parse_checked(&decoded, limits, &cancelled)
                .map_err(|_| invalid("selected world NBT metadata"))?;
            let metadata = catalog::metadata(&document)
                .map_err(|_| invalid("selected world metadata/version unsupported"))?;
            let name = metadata
                .name
                .to_utf8()
                .map_err(|_| invalid("selected world name is not Unicode"))?;
            if name.is_empty() || name.len() > 256 {
                return Err(invalid("selected world name bound"));
            }
            let child_identity = child.identity();
            let mut digest = Sha256::new();
            digest.update(b"ilium-selected-world-metadata-v1");
            for number in [
                root_identity.device,
                root_identity.inode,
                child_identity.device,
                child_identity.inode,
                self.epoch,
            ] {
                digest.update(number.to_be_bytes());
            }
            digest.update(&scratch);
            let identity = format!("{:x}", digest.finalize());
            let original = child.original_root();
            children.insert(
                entry.name.clone(),
                ListedWorldChild {
                    root: original,
                    parent_identity: root_identity,
                    child_identity,
                    identity: identity.clone(),
                },
            );
            output.push(ListedWorldEntry {
                id: entry.name,
                name,
                identity,
            });
        }
        Ok(ListedWorlds {
            entries: output,
            children,
            _storage: self.storage,
        })
    }
}
/// Supplied by the original ambient host, retaining its actual shared history
/// runtime. Settings and any resource-pack access require independent host
/// authority; selected world rights cannot authorize native pack paths.
#[derive(Clone)]
pub struct NativeSavedContext {
    pub settings: VoxelLandscapeSettings,
    pub environment: SceneEnv,
}
struct SavedWorldOpenJob {
    selected: Arc<SelectedStorage>,
    archive: Arc<SelectedStorage>,
    history: Arc<ProtectedWorldHistory>,
    child: ListedWorldChild,
    child_id: String,
    epoch: u64,
    context: NativeSavedContext,
    quota: QuotaGroup,
    stop: StopToken,
    cancellation: StorageCancellation,
    storage: StorageAdmission,
}
struct PreparedSavedWorld {
    grant: SavedWorldGrant,
    scene: HostWorldScene,
    _storage: StorageAdmission,
}
fn listed_source_identity(value: &str) -> Result<SourceIdentity> {
    if value.len() != 64 {
        return Err(invalid("listed source identity length"));
    }
    let mut digest = [0u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let hex = |byte: u8| -> Result<u8> {
            match byte {
                b'0'..=b'9' => Ok(byte - b'0'),
                b'a'..=b'f' => Ok(byte - b'a' + 10),
                _ => Err(invalid("listed source identity encoding")),
            }
        };
        digest[index] = (hex(pair[0])? << 4) | hex(pair[1])?;
    }
    Ok(SourceIdentity::from_host_digest(digest))
}
impl Job for SavedWorldOpenJob {
    type Output = PreparedSavedWorld;
    type Error = AnimationError;
    fn run(self, context: JobContext) -> Result<Self::Output> {
        if self.stop.is_stopped() || context.stop_requested() {
            return Err(invalid("saved open cancelled before source reads"));
        }
        let parent_label = self.selected.original_folder_label(&self.quota)?.to_owned();
        let parent = Arc::new(PinnedDirectory::from_host(
            self.selected.original_world_folder(&self.quota)?,
        )?);
        if parent.identity() != self.child.parent_identity {
            return Err(invalid("original selected parent changed"));
        }
        let original_child = PinnedDirectory::from_host(Arc::clone(&self.child.root))?;
        let fresh_child = parent.child(&self.child_id, false)?;
        if original_child.identity() != self.child.child_identity
            || fresh_child.identity() != self.child.child_identity
        {
            return Err(invalid("original listed child entry changed"));
        }
        let grant = SavedWorldGrant::from_host(
            Arc::clone(&self.child.root),
            listed_source_identity(&self.child.identity)?,
            self.epoch,
            true,
        )?;
        let native_jar = self.archive.original_archive_file(&self.quota)?;
        let history_root = self
            .history
            .root_after_issue(&self.cancellation, &self.quota)?;
        let history_storage = self.history.report_label().to_owned();
        if self.stop.is_stopped() || context.stop_requested() {
            return Err(invalid("saved open cancelled after namespace preparation"));
        }
        let resources = self.context.environment.resources.clone();
        let mut factory = NativeSavedFactory::from_host(NativeSavedSource {
            selected_label: parent_label.join(&self.child_id),
            parent_label,
            parent,
            child: Arc::clone(&self.child.root),
            child_identity: self.child.child_identity,
            epoch: self.epoch,
            native_jar,
            history_storage,
            history_root,
            runtime: Arc::clone(&self.context.environment.saved_runtime),
            settings: self.context.settings,
            environment: self.context.environment,
            stop: self.stop.clone(),
        });
        let scene = factory.prepare(&grant, &resources)?;
        if self.stop.is_stopped() || context.stop_requested() {
            return Err(invalid("saved open cancelled after real scene admission"));
        }
        Ok(PreparedSavedWorld {
            grant,
            scene,
            _storage: self.storage,
        })
    }
}
struct PendingSavedOpen {
    request: HostRequest,
    operations: Vec<ServiceOperation>,
    settlement: Option<Arc<ServiceGroupSettlement>>,
    receipt: Option<Receipt<SavedWorldOpenJob>>,
    retention: Option<Retention>,
    prepared: Option<PreparedSavedWorld>,
    failure: Option<String>,
    installed: Option<String>,
    result: Option<ServiceValue>,
    stop: StopToken,
    cancellation: StorageCancellation,
    uncertain: bool,
    _sources: (
        Arc<SelectedStorage>,
        Arc<SelectedStorage>,
        Arc<ProtectedWorldHistory>,
    ),
}
struct PendingWorldList {
    request: HostRequest,
    grant_id: String,
    reserved_entries: usize,
    operation: ServiceOperation,
    receipt: Option<Receipt<SelectedWorldListJob>>,
    retention: Option<Retention>,
    _result: Option<ServiceValue>,
    listing: Option<ListedWorlds>,
    _original_resource: Arc<SelectedStorage>,
    stop: StopToken,
    _uncertain: bool,
}

struct UncertainWorldCompletion {
    request: HostRequest,
    _result: ServiceValue,
}
/// Constructed only for a real worlds.open request. It uses the existing scene
/// actor and quota root, with no uncharged worker, timer or service bank.
pub struct NativeWorldHost {
    service: WorldService,
    client: Client,
    epoch: u64,
    pending_lists: BTreeMap<u64, PendingWorldList>,
    pending_saved: BTreeMap<u64, PendingSavedOpen>,
    uncertain_completion: Option<UncertainWorldCompletion>,
    helper_retirement: Option<HelperRetirementEvidence>,
    saved_context: Option<NativeSavedContext>,
    saved_world_active: Option<String>,
    listed_worlds: BTreeMap<(String, String), ListedWorldBinding>,
    active_worlds: BTreeMap<String, ActiveWorld>,
    active_frames: BTreeMap<String, ActiveFrame>,
    terminal_snapshots: BTreeMap<String, Value>,
    quota: QuotaGroup,
    limits: EngineLimits,
    mode: AnimationMode,
    closed: bool,
    _metadata: StorageAdmission,
}
impl NativeWorldHost {
    pub fn new(
        resources: AmbientResources,
        quota: QuotaGroup,
        epoch: u64,
        limits: EngineLimits,
        mode: AnimationMode,
    ) -> Result<Self> {
        if mode != AnimationMode::Live {
            return Err(AnimationError::PermissionDenied(
                "world service requires live mode".into(),
            ));
        }
        let metadata = quota
            .reserve_external_storage(128 * 1024)
            .map_err(|error| AnimationError::Budget(format!("world registry: {error:?}")))?;
        let client = resources.finite().clone();
        let service = WorldService::new(resources, quota.clone(), epoch)?;
        Ok(Self {
            service,
            client,
            epoch,
            pending_lists: BTreeMap::new(),
            pending_saved: BTreeMap::new(),
            uncertain_completion: None,
            helper_retirement: None,
            saved_context: None,
            saved_world_active: None,
            listed_worlds: BTreeMap::new(),
            active_worlds: BTreeMap::new(),
            active_frames: BTreeMap::new(),
            terminal_snapshots: BTreeMap::new(),
            quota,
            limits,
            mode,
            closed: false,
            _metadata: metadata,
        })
    }
    fn bind_helper_owner(&mut self, instance: &PackageInstance) -> Result<()> {
        if !instance.shares_root(&self.quota) {
            return Err(AnimationError::PermissionDenied(
                "world helper owner uses a foreign quota root".into(),
            ));
        }
        let candidate = instance.helper_retirement_evidence();
        match self.helper_retirement.as_ref() {
            Some(original) if !original.same_owner(&candidate) => Err(
                AnimationError::PermissionDenied("foreign world helper owner".into()),
            ),
            Some(_) => Ok(()),
            None => {
                self.helper_retirement = Some(candidate);
                Ok(())
            }
        }
    }
    /// Attach the actual retained ambient host context, never a test environment
    /// or a freshly invented SavedRuntime. No preparation occurs at this point.
    pub fn set_saved_context(&mut self, context: NativeSavedContext) -> Result<()> {
        if self.closed
            || self.saved_context.is_some()
            || !self.pending_saved.is_empty()
            || !context
                .environment
                .resources
                .finite()
                .quota_group()
                .shares_root(&self.quota)
        {
            return Err(invalid("saved host context activation/root mismatch"));
        }
        self.saved_context = Some(context);
        Ok(())
    }
    fn fields<'a>(request: &'a HostRequest, allowed: &[&str]) -> Result<&'a Map<String, Value>> {
        if !request.payload.arrays().is_empty() || !request.payload.planes().is_empty() {
            return Err(invalid("world options must be JSON only"));
        }
        let fields = request
            .payload
            .metadata()
            .as_object()
            .ok_or_else(|| invalid("world options object"))?;
        if fields.len() > allowed.len() || fields.keys().any(|key| !allowed.contains(&key.as_str()))
        {
            return Err(invalid("unknown world option"));
        }
        Ok(fields)
    }

    fn prepare_region_response(&mut self, request: &HostRequest) -> Result<ServiceValue> {
        if request.method != "worlds.region"
            || request.is_cancelled()
            || !request.payload.shares_root(&self.quota)
        {
            return Err(AnimationError::PermissionDenied(
                "world region request owner/lifetime".into(),
            ));
        }
        let fields = Self::fields(
            request,
            &[
                "world",
                "x",
                "y",
                "z",
                "width",
                "height",
                "depth",
                "max_bytes",
            ],
        )?;
        let policy = region_host_policy(&self.limits)?;
        let parsed = parse_region_request(fields, policy.limits, policy.receiving_bytes)
            .map_err(|reason| invalid(&format!("world region request: {reason:?}")))?;
        let handle = self
            .active_worlds
            .get(parsed.world_id)
            .map(|world| world.handle)
            .ok_or_else(|| invalid("unknown original world for region"))?;
        let stop = request.stop_token();
        let encoded = self.service.region(
            handle,
            parsed.spec,
            policy.limits,
            parsed.spec.max_bytes,
            &stop,
        )?;
        if request.is_cancelled() {
            return Err(invalid("world region cancelled after native encoding"));
        }
        let result =
            copy_region_response_with_stop(&encoded, &self.limits, self.quota.clone(), &stop)?;
        if request.is_cancelled() {
            return Err(invalid("world region cancelled after completion copy"));
        }
        Ok(result)
    }

    fn quarantine_uncertain_completion(
        slot: &mut Option<UncertainWorldCompletion>,
        request: &HostRequest,
        result: ServiceValue,
    ) -> Result<()> {
        if slot.is_some() {
            return Err(invalid("multiple uncertain synchronous world completions"));
        }
        request.stop_token().stop();
        *slot = Some(UncertainWorldCompletion {
            request: request.clone(),
            _result: result,
        });
        Ok(())
    }

    fn reap_uncertain_completion(
        slot: &mut Option<UncertainWorldCompletion>,
        physically_retired: bool,
    ) {
        if !physically_retired {
            return;
        }
        slot.take();
    }

    fn complete_service_value(
        &mut self,
        instance: &mut PackageInstance,
        request: &HostRequest,
        result: ServiceValue,
    ) -> Result<CompletionState> {
        if self.uncertain_completion.is_some() {
            return Err(invalid("uncertain world completion still retained"));
        }
        instance.check_native_world_request(request)?;
        self.bind_helper_owner(instance)?;
        if !result.shares_root(&self.quota) {
            return Err(AnimationError::PermissionDenied(
                "foreign world completion root".into(),
            ));
        }
        let retained = result.clone();
        match instance.complete_native_world(request, result) {
            Ok(state) => Ok(state),
            Err(error) => {
                Self::quarantine_uncertain_completion(
                    &mut self.uncertain_completion,
                    request,
                    retained,
                )?;
                self.closed = true;
                Err(error)
            }
        }
    }
    fn handle_id<'a>(fields: &'a Map<String, Value>, key: &str, kind: &str) -> Result<&'a str> {
        let value = fields
            .get(key)
            .ok_or_else(|| invalid("world handle missing"))?;
        let projection = value
            .as_object()
            .filter(|projection| {
                projection.len() == 2
                    && projection.get("kind").and_then(Value::as_str) == Some(kind)
            })
            .ok_or_else(|| invalid("world handle projection"))?;
        projection
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && id.len() <= 128)
            .ok_or_else(|| invalid("world handle ID"))
    }
    fn direct_id<'a>(fields: &'a Map<String, Value>, kind: &str) -> Result<&'a str> {
        if fields.len() != 2 || fields.get("kind").and_then(Value::as_str) != Some(kind) {
            return Err(invalid("world close handle projection"));
        }
        fields
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && id.len() <= 128)
            .ok_or_else(|| invalid("world close handle ID"))
    }
    fn complete(
        &mut self,
        instance: &mut PackageInstance,
        request: &HostRequest,
        value: Value,
    ) -> Result<CompletionState> {
        instance.check_native_world_request(request)?;
        let result = ServiceValue::copy_from_host(
            &value,
            &[],
            &BTreeMap::new(),
            &self.limits,
            self.quota.clone(),
        )?;
        self.complete_service_value(instance, request, result)
    }

    fn region(&mut self, instance: &mut PackageInstance, request: &HostRequest) -> Result<()> {
        self.region_inner(instance, request, |_| {})
    }

    #[cfg(test)]
    fn region_with_before_publication_hook_for_test(
        &mut self,
        instance: &mut PackageInstance,
        request: &HostRequest,
        before_publication: impl FnOnce(&ServiceValue),
    ) -> Result<()> {
        self.region_inner(instance, request, before_publication)
    }

    fn region_inner<F>(
        &mut self,
        instance: &mut PackageInstance,
        request: &HostRequest,
        before_publication: F,
    ) -> Result<()>
    where
        F: FnOnce(&ServiceValue),
    {
        instance.check_native_world_request(request)?;
        let result = match self.prepare_region_response(request) {
            Ok(result) => result,
            Err(error) if request.is_cancelled() => return Err(error),
            Err(error) => {
                return self.refuse(instance, request, "world_region_failed", &error.to_string());
            }
        };
        before_publication(&result);
        if request.is_cancelled() {
            return Err(invalid("world region cancelled before helper publication"));
        }
        match self.complete_service_value(instance, request, result) {
            Ok(CompletionState::Delivered) => Ok(()),
            Ok(_) => {
                self.closed = true;
                Err(invalid("world region completion was not delivered"))
            }
            Err(error) => Err(error),
        }
    }
    fn refuse(
        &mut self,
        instance: &mut PackageInstance,
        request: &HostRequest,
        code: &str,
        message: &str,
    ) -> Result<()> {
        let result = json!({"ok":false,"error":{"code":code,"message":message}});
        if self.complete(instance, request, result)? != CompletionState::Delivered {
            self.closed = true;
            return Err(invalid("world refusal ACK uncertain"));
        }
        Ok(())
    }
    /// A selected list is real finite DiskRead work. The root is the original
    /// selected descriptor, and the broker ticket is its original request ID.
    fn list_selected(
        &mut self,
        instance: &mut PackageInstance,
        request: HostRequest,
        assets: &NativeAssetHost,
    ) -> Result<()> {
        let fields = Self::fields(&request, &["grant", "max_entries"])?;
        if fields.len() != 2 || self.pending_lists.len() >= MAX_PENDING_LISTS {
            return self.refuse(
                instance,
                &request,
                "world_list_limit",
                "World list admission is full.",
            );
        }
        let maximum = fields
            .get("max_entries")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .filter(|value| (1..=MAX_LIST_ENTRIES).contains(value))
            .ok_or_else(|| invalid("world list entry bound"))?;
        let grant = fields
            .get("grant")
            .ok_or_else(|| invalid("world list grant"))?;
        let grant_id = grant
            .as_object()
            .and_then(|value| value.get("id"))
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("world grant ID"))?
            .to_owned();
        let old_count = self
            .listed_worlds
            .keys()
            .filter(|(id, _)| id == &grant_id)
            .count();
        let pending_reservations: usize = self
            .pending_lists
            .values()
            .map(|pending| pending.reserved_entries)
            .sum();
        if !selected_list_capacity(
            self.listed_worlds.len(),
            old_count,
            pending_reservations,
            maximum,
        ) {
            return self.refuse(
                instance,
                &request,
                "world_list_limit",
                "Retained selected-world descriptor capacity is full.",
            );
        }
        let (request_id, resource) = match assets.selected_world_folder(grant) {
            Ok(value) => value,
            Err(_) => {
                return self.refuse(
                    instance,
                    &request,
                    "world_grant_denied",
                    "The original selected world folder is unavailable.",
                )
            }
        };
        let root = match resource.original_world_folder(&self.quota) {
            Ok(root) => root,
            Err(_) => {
                return self.refuse(
                    instance,
                    &request,
                    "world_grant_denied",
                    "The original selected world folder was revoked.",
                )
            }
        };
        let storage = match self
            .quota
            .reserve_external_storage(WORLD_LIST_WORK_BYTES + MAX_LIST_ENTRIES * 1024)
        {
            Ok(storage) => storage,
            Err(_) => {
                return self.refuse(
                    instance,
                    &request,
                    "world_list_budget",
                    "World list scratch admission was refused.",
                )
            }
        };
        let reservation = match self.client.try_reserve(
            Lane::Io,
            JobCost {
                input_bytes: 64 * 1024,
                result_bytes: 128 * 1024,
            },
        ) {
            Ok(reservation) => reservation,
            Err(_) => {
                return self.refuse(
                    instance,
                    &request,
                    "world_list_budget",
                    "World list IO admission was refused.",
                )
            }
        };
        if self.pending_lists.contains_key(&request.id) {
            return Err(invalid("duplicate world list request ID"));
        }
        let need = match resource.operation_need(false) {
            Ok(need) => need,
            Err(_) => {
                return self.refuse(
                    instance,
                    &request,
                    "world_grant_denied",
                    "The selected world read is unavailable.",
                )
            }
        };
        let operation = match instance.dispatch_service(
            request.clone(),
            &operation_demand(&request_id),
            vec![need],
        ) {
            Ok(operation) => operation,
            Err(_) => {
                return self.refuse(
                    instance,
                    &request,
                    "world_read_denied",
                    "The original selected DiskRead demand was denied.",
                )
            }
        };
        let stop = request.stop_token();
        let job = SelectedWorldListJob {
            root,
            maximum,
            epoch: self.epoch,
            stop: stop.clone(),
            storage,
        };
        let id = request.id;
        self.pending_lists.insert(
            id,
            PendingWorldList {
                request,
                grant_id,
                reserved_entries: maximum,
                operation,
                receipt: None,
                retention: None,
                _result: None,
                listing: None,
                _original_resource: resource,
                stop,
                _uncertain: false,
            },
        );
        let pending = self
            .pending_lists
            .get(&id)
            .ok_or_else(|| invalid("world list ticket custody"))?;
        match instance.commit_service(&pending.operation, || reservation.submit(job)) {
            Ok(Ok(receipt)) => {
                self.pending_lists
                    .get_mut(&id)
                    .ok_or_else(|| invalid("world list receipt custody"))?
                    .receipt = Some(receipt);
                Ok(())
            }
            Ok(Err(rejected)) => {
                drop(rejected);
                let pending = self
                    .pending_lists
                    .remove(&id)
                    .ok_or_else(|| invalid("world list rejected custody"))?;
                instance.settle_service(&pending.operation)?;
                self.refuse(
                    instance,
                    &pending.request,
                    "world_list_budget",
                    "World list IO was refused.",
                )
            }
            Err(error) => {
                let pending = self
                    .pending_lists
                    .remove(&id)
                    .ok_or_else(|| invalid("world list unissued custody"))?;
                instance.settle_service(&pending.operation)?;
                self.refuse(
                    instance,
                    &pending.request,
                    "world_list_unissued",
                    &error.to_string(),
                )
            }
        }
    }
    /// Called only on the existing finite native actor wake. A lost receipt or
    /// uncertain helper delivery stays retained; there is no speculative retry.
    pub fn on_completion_wake(&mut self, instance: &mut PackageInstance) -> Result<()> {
        self.bind_helper_owner(instance)?;
        let ids: Vec<_> = self.pending_lists.keys().copied().collect();
        for id in ids {
            let mut pending = self
                .pending_lists
                .remove(&id)
                .ok_or_else(|| invalid("world list pending owner missing"))?;
            let poll = match pending.receipt.as_mut() {
                Some(receipt) => receipt.try_take(),
                None => {
                    self.pending_lists.insert(id, pending);
                    continue;
                }
            };
            match poll {
                JobPoll::Pending => {
                    self.pending_lists.insert(id, pending);
                    continue;
                }
                JobPoll::Lost | JobPoll::Taken => {
                    pending._uncertain = true;
                    pending.stop.stop();
                    self.closed = true;
                    self.pending_lists.insert(id, pending);
                    continue;
                }
                JobPoll::Ready(outcome) => {
                    let (outcome, retention) = outcome.into_parts();
                    pending.receipt = None;
                    pending.retention = Some(retention);
                    match outcome {
                        JobOutcome::Finished(Ok(listing)) => pending.listing = Some(listing),
                        JobOutcome::Finished(Err(_)) => {}
                        _ => {}
                    }
                }
            }
            if self.closed || pending.request.is_cancelled() {
                match instance.settle_service(&pending.operation) {
                    Ok(()) => continue,
                    Err(error) => {
                        pending._uncertain = true;
                        self.closed = true;
                        self.pending_lists.insert(id, pending);
                        return Err(error);
                    }
                }
            }
            let value = match &pending.listing {
                Some(listing) => json!({"ok":true,"value":listing.entries}),
                None => json!({"ok":false,"error":{"code":"world_list_read_failed",
                    "message":"The original selected world list did not finish."}}),
            };
            let result = match ServiceValue::copy_from_host(
                &value,
                &[],
                &BTreeMap::new(),
                &self.limits,
                self.quota.clone(),
            ) {
                Ok(result) => result,
                Err(error) => {
                    pending._uncertain = true;
                    pending.stop.stop();
                    self.closed = true;
                    self.pending_lists.insert(id, pending);
                    return Err(error);
                }
            };
            pending._result = Some(result.clone());
            match instance.complete_authorized(&pending.operation, result) {
                Ok(CompletionState::Delivered) => {
                    // Publish native child descriptors only after the original
                    // DiskRead result ACK. Guest IDs remain lookup keys, not grants.
                    if let Some(listing) = pending.listing.take() {
                        self.listed_worlds
                            .retain(|(grant, _), _| grant != &pending.grant_id);
                        for (id, child) in listing.children {
                            self.listed_worlds.insert(
                                (pending.grant_id.clone(), id),
                                ListedWorldBinding {
                                    child,
                                    selected: Arc::clone(&pending._original_resource),
                                },
                            );
                        }
                    }
                }
                Ok(_) => {
                    pending._uncertain = true;
                    pending.stop.stop();
                    self.closed = true;
                    self.pending_lists.insert(id, pending);
                }
                Err(error) => {
                    pending._uncertain = true;
                    pending.stop.stop();
                    self.closed = true;
                    self.pending_lists.insert(id, pending);
                    return Err(error);
                }
            }
        }
        self.complete_saved_wake(instance)
    }

    pub fn on_helper_retired(&mut self) -> Result<()> {
        if !self.finite_work_drained() {
            return Err(invalid("world finite work has not drained"));
        }
        let evidence = self
            .helper_retirement
            .as_ref()
            .ok_or_else(|| invalid("world original helper owner is unbound"))?;
        if !evidence.is_physically_retired() {
            return Err(invalid(
                "original world helper physical retirement is unproven",
            ));
        }
        Self::reap_uncertain_completion(&mut self.uncertain_completion, true);
        Ok(())
    }
    fn quarantine_saved_pending(&mut self, mut pending: PendingSavedOpen) {
        pending.uncertain = true;
        pending.stop.stop();
        pending.cancellation.cancel();
        self.closed = true;
        self.pending_saved.insert(pending.request.id, pending);
    }
    /// Native setup has completed before publication; the admitted scene owns
    /// its separate preparation worker and final-history drain through real join.
    fn complete_saved_wake(&mut self, instance: &mut PackageInstance) -> Result<()> {
        let ids: Vec<_> = self.pending_saved.keys().copied().collect();
        for id in ids {
            let mut pending = self
                .pending_saved
                .remove(&id)
                .ok_or_else(|| invalid("saved completion custody missing"))?;
            if pending.uncertain {
                if pending.receipt.is_none() && instance.is_physically_retired() {
                    self.settle_saved_pending(instance, pending)?;
                } else {
                    self.pending_saved.insert(id, pending);
                }
                continue;
            }
            let poll = match pending.receipt.as_mut() {
                Some(receipt) => receipt.try_take(),
                None => {
                    self.pending_saved.insert(id, pending);
                    continue;
                }
            };
            match poll {
                JobPoll::Pending => {
                    self.pending_saved.insert(id, pending);
                    continue;
                }
                JobPoll::Lost | JobPoll::Taken => {
                    self.quarantine_saved_pending(pending);
                    continue;
                }
                JobPoll::Ready(outcome) => {
                    let (outcome, retention) = outcome.into_parts();
                    pending.receipt = None;
                    pending.retention = Some(retention);
                    match outcome {
                        JobOutcome::Finished(Ok(prepared)) => pending.prepared = Some(prepared),
                        JobOutcome::Finished(Err(error)) => {
                            pending.failure = Some(error.to_string().chars().take(1024).collect())
                        }
                        _ => {
                            pending.failure =
                                Some("The original saved-world IO job did not finish.".into())
                        }
                    }
                }
            }
            if self.closed || pending.request.is_cancelled() {
                pending.stop.stop();
                pending.cancellation.cancel();
                self.settle_saved_pending(instance, pending)?;
                continue;
            }
            if let Some(prepared) = pending.prepared.take() {
                let original_identity = prepared.grant.identity();
                match instance.with_resource_registry_authority(|| {
                    self.service
                        .insert_prepared_saved(prepared.grant, prepared.scene)
                }) {
                    Ok(Ok(handle)) => {
                        let world_id = handle.opaque_id();
                        // insert_prepared_saved checked this exact identity against
                        // the actual native scene before returning the handle.
                        let descriptor = json!({"id":world_id,"kind":"worlds","revision":1,
                            "status":{"state":"preparing"},"identity":original_identity.hex()});
                        self.active_worlds.insert(
                            world_id.clone(),
                            ActiveWorld {
                                handle,
                                descriptor,
                                stop: Some(pending.stop.clone()),
                            },
                        );
                        self.saved_world_active = Some(world_id.clone());
                        pending.installed = Some(world_id);
                    }
                    Ok(Err(error)) | Err(error) => {
                        pending.failure = Some(error.to_string().chars().take(1024).collect())
                    }
                }
            }
            let value = match pending
                .installed
                .as_ref()
                .and_then(|id| self.active_worlds.get(id))
            {
                Some(world) => json!({"ok":true,"value":world.descriptor}),
                None => json!({"ok":false,"error":{"code":"saved_open_failed",
                    "message":pending.failure.as_deref().unwrap_or("The original saved world was not prepared.")}}),
            };
            let result = match ServiceValue::copy_from_host(
                &value,
                &[],
                &BTreeMap::new(),
                &self.limits,
                self.quota.clone(),
            ) {
                Ok(result) => result,
                Err(error) => {
                    self.quarantine_saved_pending(pending);
                    return Err(error);
                }
            };
            pending.result = Some(result.clone());
            let settlement = match pending.settlement.as_ref() {
                Some(settlement) => Arc::clone(settlement),
                None => {
                    self.quarantine_saved_pending(pending);
                    return Err(invalid("saved original settlement receipt missing"));
                }
            };
            let operations: Vec<_> = pending.operations.iter().collect();
            match instance.complete_authorized_group(&operations, result, &settlement) {
                Ok(CompletionState::Delivered) => {}
                Ok(CompletionState::Cancelled) => {
                    pending.stop.stop();
                    pending.cancellation.cancel();
                    self.settle_saved_pending(instance, pending)?;
                }
                Ok(_) => self.quarantine_saved_pending(pending),
                Err(error) => {
                    self.quarantine_saved_pending(pending);
                    return Err(error);
                }
            }
        }
        Ok(())
    }
    /// Dispose only known-unissued work or a genuinely completed original job.
    /// Partial dispatch keeps each remaining original ticket if cleanup fails.
    fn settle_saved_pending(
        &mut self,
        instance: &mut PackageInstance,
        mut pending: PendingSavedOpen,
    ) -> Result<()> {
        let settle = match &pending.settlement {
            Some(receipt) => {
                let operations: Vec<_> = pending.operations.iter().collect();
                instance.settle_service_group(&operations, receipt)
            }
            None => {
                let mut failure = None;
                while let Some(operation) = pending.operations.last() {
                    if let Err(error) = instance.settle_service(operation) {
                        failure = Some(error);
                        break;
                    }
                    pending.operations.pop();
                }
                failure.map_or(Ok(()), Err)
            }
        };
        if let Err(error) = settle {
            pending.uncertain = true;
            pending.stop.stop();
            pending.cancellation.cancel();
            self.closed = true;
            self.pending_saved.insert(pending.request.id, pending);
            return Err(error);
        }
        if let Some(id) = pending.installed.take() {
            if let Some(world) = self.active_worlds.remove(&id) {
                self.service.retire_world(world.handle)?;
            }
            if self.saved_world_active.as_deref() == Some(id.as_str()) {
                self.saved_world_active = None;
            }
        }
        Ok(())
    }
    fn refuse_saved_unissued(
        &mut self,
        instance: &mut PackageInstance,
        id: u64,
        code: &str,
        message: &str,
    ) -> Result<()> {
        let pending = self
            .pending_saved
            .remove(&id)
            .ok_or_else(|| invalid("unissued saved-open custody missing"))?;
        let request = pending.request.clone();
        self.settle_saved_pending(instance, pending)?;
        self.refuse(instance, &request, code, message)
    }
    #[allow(clippy::result_large_err)] // Rejection returns original admitted owners inline; do not allocate to retain custody.
    fn open_saved(
        &mut self,
        instance: &mut PackageInstance,
        request: HostRequest,
        assets: &mut NativeAssetHost,
    ) -> Result<()> {
        let fields = Self::fields(&request, &["id", "grant", "archive"])?;
        if fields.len() != 3
            || self.saved_world_active.is_some()
            || !self.pending_saved.is_empty()
            || self.active_worlds.len() >= MAX_WORLD_HANDLES
            || self.active_worlds.len() + self.active_frames.len() + self.terminal_snapshots.len()
                >= 64
        {
            return self.refuse(
                instance,
                &request,
                "saved_world_busy",
                "The original saved-world owner is busy or the open options are incomplete.",
            );
        }
        let child_id = fields
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && id.len() <= 128)
            .ok_or_else(|| invalid("saved child ID"))?
            .to_owned();
        let grant = fields
            .get("grant")
            .ok_or_else(|| invalid("saved folder grant"))?;
        let grant_id = Self::handle_id(fields, "grant", "asset")?.to_owned();
        let archive_grant = fields
            .get("archive")
            .ok_or_else(|| invalid("separate native archive grant"))?;
        let (folder_request, selected) = assets.selected_world_folder(grant)?;
        let (archive_request, archive) = assets.selected_native_archive(archive_grant)?;
        let binding = self
            .listed_worlds
            .get(&(grant_id, child_id.clone()))
            .ok_or_else(|| invalid("original saved child has not been listed"))?;
        if !Arc::ptr_eq(&binding.selected, &selected) {
            return Err(invalid("listed original selected resource changed"));
        }
        let child = binding.child.clone();
        if self.saved_context.is_none() {
            return self.refuse(
                instance,
                &request,
                "saved_host_context_missing",
                "The original ambient host context is not bound to this activation.",
            );
        }
        let work = self
            .quota
            .reserve_external_storage(WORLD_LIST_WORK_BYTES)
            .map_err(|error| AnimationError::Budget(format!("saved open work: {error:?}")))?;
        let reservation = self
            .client
            .try_reserve(
                Lane::Io,
                JobCost {
                    input_bytes: 64 * 1024,
                    result_bytes: 128 * 1024,
                },
            )
            .map_err(|error| AnimationError::Budget(format!("saved open IO: {error:?}")))?;
        let context = self
            .saved_context
            .as_ref()
            .ok_or_else(|| invalid("saved host context disappeared"))?
            .clone();
        let (history_request, history) = assets.selected_world_history()?;
        let needs = [
            (folder_request, selected.operation_need(false)?),
            (archive_request, archive.operation_need(false)?),
            (history_request, history.operation_need()?),
        ];
        let stop = request.stop_token();
        let cancellation = StorageCancellation::default();
        let id = request.id;
        self.pending_saved.insert(
            id,
            PendingSavedOpen {
                request: request.clone(),
                operations: Vec::with_capacity(3),
                settlement: None,
                receipt: None,
                retention: None,
                prepared: None,
                failure: None,
                installed: None,
                result: None,
                stop: stop.clone(),
                cancellation: cancellation.clone(),
                uncertain: false,
                _sources: (
                    Arc::clone(&selected),
                    Arc::clone(&archive),
                    Arc::clone(&history),
                ),
            },
        );
        for (demand, need) in needs {
            match instance.dispatch_service(request.clone(), &operation_demand(&demand), vec![need])
            {
                Ok(operation) => self
                    .pending_saved
                    .get_mut(&id)
                    .ok_or_else(|| invalid("saved dispatch custody"))?
                    .operations
                    .push(operation),
                Err(error) => {
                    return self.refuse_saved_unissued(
                        instance,
                        id,
                        "saved_permission_denied",
                        &error.to_string(),
                    )
                }
            }
        }
        let pending = self
            .pending_saved
            .get(&id)
            .ok_or_else(|| invalid("saved original group missing"))?;
        let references: Vec<_> = pending.operations.iter().collect();
        let settlement = match instance.prepare_service_group_settlement(&references) {
            Ok(receipt) => receipt,
            Err(error) => {
                return self.refuse_saved_unissued(
                    instance,
                    id,
                    "saved_group_unissued",
                    &error.to_string(),
                )
            }
        };
        self.pending_saved
            .get_mut(&id)
            .ok_or_else(|| invalid("saved settlement custody"))?
            .settlement = Some(settlement);
        let job = SavedWorldOpenJob {
            selected,
            archive,
            history,
            child,
            child_id,
            epoch: self.epoch,
            context,
            quota: self.quota.clone(),
            stop,
            cancellation,
            storage: work,
        };
        let pending = self
            .pending_saved
            .get(&id)
            .ok_or_else(|| invalid("saved issue custody"))?;
        let references: Vec<_> = pending.operations.iter().collect();
        match instance.commit_service_group(&references, || reservation.submit(job)) {
            Ok(Ok(receipt)) => {
                self.pending_saved
                    .get_mut(&id)
                    .ok_or_else(|| invalid("saved original receipt custody"))?
                    .receipt = Some(receipt);
                Ok(())
            }
            Ok(Err(rejected)) => {
                drop(rejected); // Native bank returned the original job as known unstarted.
                self.refuse_saved_unissued(
                    instance,
                    id,
                    "saved_io_unissued",
                    "The original saved-world IO bank refused admission.",
                )
            }
            Err(error) => {
                self.refuse_saved_unissued(instance, id, "saved_group_unissued", &error.to_string())
            }
        }
    }
    fn open_generated(
        &mut self,
        instance: &mut PackageInstance,
        request: &HostRequest,
    ) -> Result<()> {
        let fields = Self::fields(request, &["id", "seed", "grant"])?;
        if fields.is_empty()
            || fields.len() > 2
            || fields.get("id").and_then(Value::as_str) != Some("generated")
            || fields.contains_key("grant")
            || self.active_worlds.len() >= MAX_WORLD_HANDLES
            || self.active_worlds.len() + self.active_frames.len() + self.terminal_snapshots.len()
                >= 64
        {
            return Err(invalid("only bounded generated world open is available"));
        }
        let seed = fields
            .get("seed")
            .map(|value| {
                value
                    .as_u64()
                    .and_then(|seed| u32::try_from(seed).ok())
                    .ok_or_else(|| invalid("generated seed"))
            })
            .transpose()?
            .unwrap_or(42);
        let settings = GeneratedWorldSettings {
            seed,
            ..GeneratedWorldSettings::default()
        };
        let stop = request.stop_token();
        let handle = self.service.insert_generated_with_stop(settings, &stop)?;
        if request.is_cancelled() {
            self.service.close_world(handle)?;
            return Err(invalid("world open cancelled after preparation"));
        }
        let id = handle.opaque_id();
        let descriptor = json!({"id":id,"kind":"worlds","revision":1,
            "status":{"state":"ready"},"identity":self.service.world_identity(handle)?.hex()});
        self.active_worlds.insert(
            id.clone(),
            ActiveWorld {
                handle,
                descriptor: descriptor.clone(),
                stop: None,
            },
        );
        match self.complete(instance, request, json!({"ok":true,"value":descriptor})) {
            Ok(CompletionState::Delivered) => Ok(()),
            Ok(_) => {
                self.closed = true;
                Err(invalid(
                    "world open ACK uncertain; original source retained",
                ))
            }
            Err(error) => {
                self.closed = true;
                Err(error)
            }
        }
    }
    fn frame(
        &mut self,
        instance: &mut PackageInstance,
        request: &HostRequest,
        draw: &mut NativeDrawHost,
    ) -> Result<()> {
        let fields = Self::fields(request, &["world", "width", "height", "time", "wall"])?;
        if fields.len() != 5
            || self.active_frames.len() >= MAX_FRAME_HANDLES
            || self.active_worlds.len() + self.active_frames.len() + self.terminal_snapshots.len()
                >= 64
        {
            return Err(invalid("world frame shape or handle bound"));
        }
        let world_id = Self::handle_id(fields, "world", "worlds")?.to_owned();
        let world = self
            .active_worlds
            .get(&world_id)
            .ok_or_else(|| invalid("unknown original world"))?;
        let width = fields
            .get("width")
            .and_then(Value::as_u64)
            .and_then(|value| u16::try_from(value).ok())
            .filter(|value| *value > 0 && *value <= 240)
            .ok_or_else(|| invalid("world frame width"))?;
        let height = fields
            .get("height")
            .and_then(Value::as_u64)
            .and_then(|value| u16::try_from(value).ok())
            .filter(|value| *value > 0 && *value <= 100)
            .ok_or_else(|| invalid("world frame height"))?;
        let clock = |field: &str| -> Result<Duration> {
            let value = fields
                .get(field)
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite() && (0.0..=1e9).contains(value))
                .ok_or_else(|| invalid("world frame clock"))?;
            Duration::try_from_secs_f64(value).map_err(|_| invalid("world frame clock range"))
        };
        let time = clock("time")?;
        let wall = clock("wall")?;
        // The frame wall clock is elapsed animation time; civil time comes only
        // from the native host and is never reconstructed from guest seconds.
        let now = SystemTime::now();
        let source = match self.service.render(
            world.handle,
            WorldRenderRequest {
                width,
                height,
                time,
                wall,
                now,
                pre_rendered: false,
            },
        ) {
            Ok(source) => source,
            Err(AnimationError::Preparing(_)) => {
                return self.refuse(
                    instance,
                    request,
                    "world_frame_preparing",
                    "The original native world is still preparing this viewport.",
                )
            }
            Err(error) => {
                return self.refuse(instance, request, "world_frame_failed", &error.to_string())
            }
        };
        if request.is_cancelled() {
            return Err(invalid("world frame cancelled after render"));
        }
        let descriptor =
            draw.retain_world_frame(instance, &self.service, &source, &request.stop_token())?;
        let id = descriptor
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("native frame descriptor missing ID"))?
            .to_owned();
        if self.active_frames.contains_key(&id) {
            return Err(invalid("duplicate native frame ID"));
        }
        self.active_frames.insert(
            id.clone(),
            ActiveFrame {
                world_id,
                descriptor: descriptor.clone(),
                _source: source,
            },
        );
        match self.complete(instance, request, json!({"ok":true,"value":descriptor})) {
            Ok(CompletionState::Delivered) => Ok(()),
            Ok(_) => {
                self.closed = true;
                Err(invalid("world frame ACK uncertain; source retained"))
            }
            Err(error) => {
                self.closed = true;
                Err(error)
            }
        }
    }
    fn close_world(&mut self, instance: &mut PackageInstance, request: &HostRequest) -> Result<()> {
        let fields = Self::fields(request, &["id", "kind"])?;
        let id = Self::direct_id(fields, "worlds")?.to_owned();
        let world = self
            .active_worlds
            .get(&id)
            .ok_or_else(|| invalid("unknown original world"))?;
        if self
            .active_frames
            .values()
            .any(|frame| frame.world_id == id)
        {
            return Err(invalid("close retained world frames before world"));
        }
        if self.terminal_snapshots.len() >= 64 {
            return Err(invalid("world terminal seed capacity"));
        }
        let mut descriptor = world.descriptor.clone();
        descriptor["revision"] = json!(2);
        descriptor["status"] = json!({"state":"closed"});
        let handle = world.handle;
        match self.complete(
            instance,
            request,
            json!({"ok":true,"value":descriptor.clone()}),
        ) {
            Ok(CompletionState::Delivered) => {
                if let Some(stop) = self
                    .active_worlds
                    .get(&id)
                    .and_then(|world| world.stop.as_ref())
                {
                    stop.stop();
                }
                self.service.close_world(handle)?;
                self.active_worlds.remove(&id);
                if self.saved_world_active.as_deref() == Some(id.as_str()) {
                    self.saved_world_active = None;
                }
                self.terminal_snapshots.insert(id, descriptor);
                Ok(())
            }
            Ok(_) => {
                self.closed = true;
                Err(invalid("world close ACK uncertain; owner retained"))
            }
            Err(error) => {
                self.closed = true;
                Err(error)
            }
        }
    }
    fn close_frame(
        &mut self,
        instance: &mut PackageInstance,
        request: &HostRequest,
        draw: &mut NativeDrawHost,
    ) -> Result<()> {
        let fields = Self::fields(request, &["id", "kind"])?;
        let id = Self::direct_id(fields, "worlds.frame")?.to_owned();
        let frame = self
            .active_frames
            .get(&id)
            .ok_or_else(|| invalid("unknown original world frame"))?;
        if self.terminal_snapshots.len() >= 64 {
            return Err(invalid("world terminal seed capacity"));
        }
        let mut descriptor = frame.descriptor.clone();
        descriptor["revision"] = json!(2);
        descriptor["status"] = json!({"state":"closed"});
        match self.complete(
            instance,
            request,
            json!({"ok":true,"value":descriptor.clone()}),
        ) {
            Ok(CompletionState::Delivered) => {
                draw.release_world_frame(&id)?;
                self.active_frames.remove(&id);
                self.terminal_snapshots.insert(id, descriptor);
                Ok(())
            }
            Ok(_) => {
                self.closed = true;
                Err(invalid("world frame close ACK uncertain; owner retained"))
            }
            Err(error) => {
                self.closed = true;
                Err(error)
            }
        }
    }
    pub fn dispatch(
        &mut self,
        instance: &mut PackageInstance,
        request: HostRequest,
        draw: &mut NativeDrawHost,
        assets: Option<&mut NativeAssetHost>,
    ) -> Result<Option<HostRequest>> {
        if !request.method.starts_with("worlds.") {
            return Ok(Some(request));
        }
        if self.closed || self.mode != AnimationMode::Live {
            return Err(invalid("world owner retiring"));
        }
        instance.check_native_world_request(&request)?;
        self.bind_helper_owner(instance)?;
        match request.method.as_str() {
            "worlds.open" => {
                let options = Self::fields(&request, &["id", "seed", "grant", "archive"])?;
                if options.get("id").and_then(Value::as_str) == Some("generated")
                    && !options.contains_key("grant")
                    && !options.contains_key("archive")
                {
                    self.open_generated(instance, &request)?;
                } else {
                    self.open_saved(
                        instance,
                        request.clone(),
                        assets.ok_or_else(|| invalid("original selected asset owner missing"))?,
                    )?;
                }
            }
            "worlds.frame" => self.frame(instance, &request, draw)?,
            "worlds.list" => self.list_selected(
                instance,
                request.clone(),
                assets.ok_or_else(|| invalid("original selected asset owner missing"))?,
            )?,
            "worlds.close" => self.close_world(instance, &request)?,
            "worlds.frame.close" => self.close_frame(instance, &request, draw)?,
            "worlds.region" => self.region(instance, &request)?,
            "worlds.model" => {
                let result = json!({"ok":false,"error":{"code":"world_model_unavailable","message":"No native model and texture bank is bound to this world."}});
                if self.complete(instance, &request, result)? != CompletionState::Delivered {
                    self.closed = true;
                    return Err(invalid("world refusal ACK uncertain"));
                }
            }
            _ => return Err(invalid("world method inventory")),
        }
        Ok(None)
    }
    pub fn snapshots(&mut self, instance: &PackageInstance) -> Result<ServiceValue> {
        instance.native_world_authority()?;
        self.bind_helper_owner(instance)?;
        for world in self.active_worlds.values_mut() {
            let status = match self.service.world_readiness(world.handle)? {
                ilium_ambient::scene::SceneReadiness::Preparing => json!({"state":"preparing"}),
                ilium_ambient::scene::SceneReadiness::Ready => json!({"state":"ready"}),
                ilium_ambient::scene::SceneReadiness::Unavailable(reason) => {
                    json!({"state":"error","error":{"code":"world_source_unavailable","message":reason}})
                }
            };
            if world.descriptor.get("status") != Some(&status) {
                let revision = world
                    .descriptor
                    .get("revision")
                    .and_then(Value::as_u64)
                    .and_then(|revision| revision.checked_add(1))
                    .ok_or_else(|| invalid("world status revision exhausted"))?;
                world.descriptor["status"] = status;
                world.descriptor["revision"] = json!(revision);
            }
        }
        let values: Vec<Value> = self
            .active_worlds
            .values()
            .map(|world| world.descriptor.clone())
            .chain(
                self.active_frames
                    .values()
                    .map(|frame| frame.descriptor.clone()),
            )
            .chain(self.terminal_snapshots.values().cloned())
            .collect();
        if values.len() > 64 {
            return Err(invalid("world seed snapshot bound"));
        }
        ServiceValue::copy_from_host(
            &Value::Array(values),
            &[],
            &BTreeMap::new(),
            &self.limits,
            self.quota.clone(),
        )
    }
    pub fn acknowledge_seeded_snapshots(&mut self) {
        self.terminal_snapshots.clear();
    }
    pub fn revoke(&mut self) {
        self.closed = true;
        if let Some(pending) = &self.uncertain_completion {
            pending.request.stop_token().stop();
        }
        for world in self.active_worlds.values() {
            if let Some(stop) = &world.stop {
                stop.stop();
            }
        }
        self.service.cancel();
        for pending in self.pending_lists.values() {
            pending.stop.stop();
        }
        for pending in self.pending_saved.values() {
            pending.stop.stop();
            pending.cancellation.cancel();
        }
        // Prepared frames and their exact source Arcs remain in the draw host
        // until the caller's original terminal/helper owners have retired.
    }
    pub fn finite_work_drained(&self) -> bool {
        self.closed && self.pending_lists.is_empty() && self.pending_saved.is_empty()
    }
    pub fn is_drained(&self) -> bool {
        self.finite_work_drained() && self.uncertain_completion.is_none()
    }
}

#[cfg(test)]
mod selected_capacity_tests {
    use super::selected_list_capacity;
    #[test]
    fn two_pending_grants_cannot_publish_more_than_original_registry_capacity() {
        // One delivered 64-entry grant and one pending 64-entry replacement.
        assert!(selected_list_capacity(64, 64, 0, 64));
        // A second distinct grant must be refused while the first result is pending;
        // otherwise two ACKs would leave 192 retained children.
        assert!(!selected_list_capacity(64, 0, 64, 64));
        // The replacement may publish 64 under its ACK, leaving capacity for
        // one subsequent distinct grant once its pending reservation retires.
        assert!(selected_list_capacity(64, 0, 0, 64));
        assert!(!selected_list_capacity(128, 0, 0, 1));
    }
    #[test]
    fn selected_source_identity_requires_the_exact_original_lowercase_digest() {
        let bytes: [u8; 32] = std::array::from_fn(|index| index as u8);
        let expected = super::SourceIdentity::from_host_digest(bytes);
        let encoded = expected.hex();
        assert_eq!(super::listed_source_identity(&encoded).unwrap(), expected);
        for invalid in [
            "0".repeat(63),
            "0".repeat(65),
            "g".repeat(64),
            "A".repeat(64),
            "é".repeat(32),
        ] {
            assert!(super::listed_source_identity(&invalid).is_err());
        }
    }
}

#[cfg(test)] // Qualify the newly chosen dispatcher policy independently from source/world authority.
mod region_policy_tests {
    // These values are new host policy and were not part of the earlier pure-parser qualification.
    use super::*; // Reach the exact production region_host_policy function.

    #[test] // Default EngineLimits should remain finite despite a 16MiB backing store.
    fn default_region_policy_caps_cells_palette_and_collector_work() {
        // Freeze the explicit new policy values for review.
        let engine = EngineLimits::default(); // Use the runtime's actual default JSON/backing ceilings.
        let policy = region_host_policy(&engine).unwrap(); // Derive only pure limits.
        assert_eq!(
            policy.receiving_bytes,
            engine.json_bytes + engine.backing_bytes
        ); // Bind max_bytes to existing receiving dimensions.
        assert_eq!(policy.limits.cells, 65_536); // Prevent the default 16MiB backing store from authorizing an eight-million-cell scan.
        assert_eq!(
            policy.limits.palette,
            engine.json_bytes / MIN_REGION_PALETTE_METADATA_BYTES
        ); // Bound distinct states by the configured metadata receiver.
        assert_eq!(policy.limits.work, 65_536 * REGION_WORK_PER_CELL); // Keep the absolute collector budget at 16,777,216 work units.
    } // End default-policy regression.

    #[test] // Narrowed engine limits must narrow region policy rather than retaining defaults.
    fn narrowed_region_policy_tracks_actual_json_and_backing_limits() {
        // Prove policy remains per-instance.
        let engine = EngineLimits {
            // Start from a valid ordinary engine shape with narrowed receiving storage.
            backing_bytes: 1000, // Permit at most 500 U16 indices before the hard cell cap.
            json_bytes: 140, // Permit at most two minimum-sized semantic palette entries by host policy.
            ..EngineLimits::default()  // Preserve unrelated execution deadlines and heap limits.
        }; // Finish narrowed receiving limits.
        let policy = region_host_policy(&engine).unwrap(); // Derive the production limits.
        assert_eq!(policy.receiving_bytes, 1140); // Use checked JSON+backing total for parser max_bytes.
        assert_eq!(policy.limits.cells, 500); // Derive binary cell capacity from actual backing bytes.
        assert_eq!(policy.limits.palette, 2); // Derive palette capacity from actual JSON bytes.
        assert_eq!(policy.limits.work, 500 * REGION_WORK_PER_CELL); // Scale collector work with the accepted volume.
    } // End narrowed-policy regression.

    #[test] // A backing store too small for one U16 must not silently relax typed-array requirements.
    fn region_policy_refuses_backing_store_smaller_than_one_u16() {
        // Exercise the minimum service ABI shape.
        let engine = EngineLimits {
            // Construct the smallest nonzero backing ceiling accepted by EngineLimits shape.
            backing_bytes: 1, // This cannot represent one complete U16 region cell.
            ..EngineLimits::default()  // Preserve all other existing runtime limits.
        }; // Finish the intentionally unusable region receiver.
        assert!(region_host_policy(&engine).is_err()); // Make region service unavailable instead of inventing byte packing.
    } // End minimum-backing regression.
} // End host-policy tests.

#[cfg(test)] // Exercise real WorldService/QuotaGroup source handling without claiming helper transport authority.
mod region_dispatch_tests {
    // Cover production result preparation, forged/closed sources, cancellation and raw-result custody.
    use super::*; // Reach private ActiveWorld and production NativeWorldHost helpers.
    use crate::engine::{
        // Construct genuine immutable HostRequest/ServiceValue objects.
        ArraySpec, // Describe the retained binary completion in the uncertainty-custody case.
        ServiceAuthority, // Supply valid native request activation coordinates.
        ServiceBudget, // Exercise the real retained-request occupancy fence.
        ServicePhase, // Preserve the actual asynchronous region request phase.
        TypedArrayKind, // Describe the U16 completion plane.
    }; // End engine test imports.
    use ilium_execution::{
        // Use the actual finite execution and storage ledgers.
        ClientLimits,    // Construct the AmbientResources finite execution client.
        Execution,       // Keep generated-source execution ownership alive through each test.
        ExecutionConfig, // Supply exact finite lane configuration.
        LaneConfig,      // Configure the one CPU preparation lane and disabled unused lanes.
        QuotaLimits,     // Construct one original quota root.
    }; // End execution imports.

    fn quota() -> QuotaGroup {
        // Supply enough finite storage for the existing 64MiB generated-world admission.
        QuotaGroup::new(QuotaLimits {
            // Use one real original execution ledger.
            clients: 2,                      // Admit the AmbientResources finite client.
            jobs: 4, // Permit the generated preparation path and finite execution bookkeeping.
            service_jobs: 0, // worlds.region itself creates no service job or replacement bank.
            input_bytes: 8 * 1024 * 1024, // Bound finite native inputs.
            result_bytes: 8 * 1024 * 1024, // Bound finite native job results.
            worker_threads: 4, // Admit the one CPU lane plus bounded execution infrastructure.
            worker_bytes: 512 * 1024 * 1024, // Cover generated world, host registry, request/result and test execution admissions.
        }) // Finish the finite original root.
    } // End quota fixture.

    fn world_host() -> (Execution, NativeWorldHost, QuotaGroup) {
        // Construct the actual WorldService owner used by production.
        let quota = quota(); // Establish the sole original quota root.
        let zero = LaneConfig {
            // Disable lanes which these synchronous generated-region tests do not use.
            threads: 0,                   // Spawn no worker.
            queue_slots: 0,               // Admit no queued work.
            priority: None,               // No thread priority exists for a disabled lane.
            resident_bytes_per_thread: 0, // Debit no nonexistent resident worker memory.
        }; // End disabled lane definition.
        let execution = Execution::start(
            // Start the real bounded execution service.
            quota.clone(), // Share the exact original ledger with WorldService.
            ExecutionConfig {
                // Configure one finite CPU worker only.
                cpu: LaneConfig {
                    // Generated preparation uses the existing CPU execution path where required.
                    threads: 1,                             // Admit one worker.
                    queue_slots: 1,                         // Permit one bounded queued job.
                    priority: None,                         // Preserve ordinary test priority.
                    resident_bytes_per_thread: 1024 * 1024, // Charge finite worker residency.
                }, // End CPU lane.
                io: zero, // Region preparation opens no selected path in this generated fixture.
                service: zero, // Region preparation creates no separate service lane work.
            }, // End execution configuration.
        ) // End real execution startup.
        .unwrap(); // The fixture quota deliberately satisfies all admissions.
        let resources = AmbientResources::new(
            // Bind WorldService to an actual finite execution client.
            execution // Borrow the live execution owner.
                .client(ClientLimits {
                    // Give AmbientResources a finite client view.
                    jobs: 2,                   // Bound outstanding native jobs.
                    service_jobs: 0,           // No service jobs are used here.
                    input_bytes: 1024 * 1024,  // Bound client input accounting.
                    result_bytes: 1024 * 1024, // Bound client result accounting.
                }) // End client limits.
                .unwrap(), // The original root has enough client capacity.
        ); // End ambient resource construction.
        let host = NativeWorldHost::new(
            // Construct the actual production world owner.
            resources,               // Transfer the finite native resources.
            quota.clone(),           // Preserve exact root identity.
            71,                      // Supply one nonzero native epoch.
            EngineLimits::default(), // Use the ordinary receiving limits.
            AnimationMode::Live,     // World APIs are live-only by contract.
        ) // End host construction.
        .unwrap(); // The fixture satisfies all metadata admissions.
        (execution, host, quota) // Keep execution alive beside host and return the original root for assertions.
    } // End host fixture.

    fn request_payload(world_id: &str, max_bytes: usize) -> Value {
        // Build the exact current eight-field SDK request.
        json!({ // Preserve public X-east/Y-up/Z-south coordinates.
            "world":{"id":world_id,"kind":"worlds"}, // Guest projection remains a lookup key, never native authority.
            "x":-1, // Stay inside the default generated X window.
            "y":0, // Query the lowest representable generated height.
            "z":-2, // Stay inside the default generated Z window.
            "width":2, // Query two X cells.
            "height":1, // Query one Y layer.
            "depth":2, // Query two Z cells.
            "max_bytes":max_bytes, // Bound the complete metadata plus U16 response.
        }) // Finish exact request metadata.
    } // End public request fixture.

    fn request(quota: &QuotaGroup, metadata: Value) -> HostRequest {
        // Construct the same admitted immutable request shape produced by helper ingress.
        let limits = EngineLimits::default(); // Use ordinary pending request limits.
        let budget = ServiceBudget::new(&limits); // Retain aggregate request occupancy through the final payload alias.
        let payload = ServiceValue::copy_request_from_host(
            // Copy request metadata under the original parent root.
            &metadata,        // Borrow the exact JSON request tree.
            &[],              // worlds.region has no binary input arrays.
            &BTreeMap::new(), // worlds.region has no binary input planes.
            &limits,          // Enforce the same service metadata limits as helper ingress.
            quota.clone(),    // Debit only this supplied original root.
            &budget,          // Retain one real pending request lease.
        ) // End immutable request-payload construction.
        .unwrap(); // Fixture metadata is intentionally bounded.
        HostRequest::from_transport(
            // Use the production retained request constructor.
            1,                      // Supply one nonzero helper correlation ID.
            "worlds.region".into(), // Exercise only the target service method.
            60_000,                 // Keep the test request live during synchronous assertions.
            "a".repeat(64),         // Supply a syntactically valid immutable package digest.
            ServiceAuthority {
                // Supply valid nonzero native activation coordinates.
                instance_id: 1,         // Bind one synthetic test activation identity.
                plan_generation: 1,     // Bind one accepted plan generation.
                authorization_epoch: 1, // Bind one authorization epoch.
            }, // End native activation stamp.
            ServicePhase::Async, // Region is an ongoing live service call in these direct source tests.
            payload,             // Transfer the genuinely admitted payload.
        ) // End immutable HostRequest construction.
        .unwrap() // All production request-shape invariants are satisfied.
    } // End request fixture.

    fn install_generated(host: &mut NativeWorldHost) -> (String, WorldHandle, String) {
        // Register one genuine generated source and its guest projection.
        let handle = host // Use the exact WorldService already owned by NativeWorldHost.
            .service // Reach the private native registry.
            .insert_generated(GeneratedWorldSettings::default()) // Prepare the existing default generated occupancy once.
            .unwrap(); // The fixture has enough original-root storage.
        let id = handle.opaque_id(); // Publish only the existing opaque guest lookup projection.
        let identity = host.service.world_identity(handle).unwrap().hex(); // Read the authenticated original native source identity.
        let descriptor = json!({ // Match the ordinary generated worlds.open descriptor.
            "id":id, // Preserve the opaque guest lookup ID.
            "kind":"worlds", // Preserve the SDK handle kind.
            "revision":1, // Publish initial descriptor revision.
            "status":{"state":"ready"}, // Generated preparation is complete before insertion returns.
            "identity":identity, // Expose source identity only as metadata.
        }); // End descriptor fixture.
        host.active_worlds.insert(
            // Publish the exact sealed handle in the same private registry used by production lookup.
            id.clone(), // Key only by the guest projection string.
            ActiveWorld {
                // Preserve native authority separately from JSON.
                handle,     // Store the sealed instance/epoch/id handle.
                descriptor, // Retain the ordinary script-facing descriptor.
                stop: None, // Generated sources use the per-request stop only after preparation.
            }, // End active generated source.
        ); // End host registry insertion.
        (id, handle, identity) // Return projections only for assertions and request construction.
    } // End generated source installation.

    #[test] // Reach the actual parser, host registry, WorldService collector, encoder and response constructor.
    fn registered_generated_region_prepares_native_u16_result_without_frame_or_history_authority() {
        // Cover R1 and R14 before helper transport.
        let (_execution, mut host, quota) = world_host(); // Keep the real finite execution owner alive.
        let (id, _handle, identity) = install_generated(&mut host); // Prepare and register one genuine original generated source.
        assert!(host.active_frames.is_empty()); // No rendered-frame authority exists before the raw query.
        let request = request(&quota, request_payload(&id, 4096)); // Construct an exact admitted SDK request.
        let result = host.prepare_region_response(&request).unwrap(); // Run the complete production raw-result preparation.
        assert_eq!(result.metadata()["ok"], true); // Preserve the encoded success envelope.
        assert_eq!(result.metadata()["value"]["identity"], identity); // Bind metadata to the original registered source identity.
        assert_eq!(
            result.metadata()["value"]["palette"][0]["name"],
            "ilium:generated/basalt"
        ); // Read actual registered generated material identity.
        assert_eq!(result.arrays().len(), 1); // Publish exactly one binary region plane.
        assert_eq!(result.arrays()[0].kind, TypedArrayKind::U16); // Brand only native-order bytes as U16.
        let indices: Vec<u16> = result // Decode according to the actual host ABI rather than assuming little endian in the test.
            .planes()["b0"] // Borrow the independently admitted completion plane.
            .chunks_exact(2) // Visit complete U16 words only.
            .map(|word| u16::from_ne_bytes([word[0], word[1]])) // Interpret bytes using the same native order negotiated by helper transport.
            .collect(); // Materialize only tiny assertion data outside production ownership.
        assert_eq!(indices, vec![0, 0, 0, 0]); // Preserve the previously qualified four-cell generated basalt fixture.
        assert!(host.active_frames.is_empty()); // Raw region projection must not mint NativeWorldFrame custody.
        assert!(host.terminal_snapshots.is_empty()); // Raw region projection must not create terminal/presentation history descriptors.
    } // End registered generated raw-result regression.

    #[test] // Forged IDs, stale native handles and cancelled requests must fail without query-result debit drift.
    fn forged_closed_and_cancelled_region_sources_refuse_before_escaping_result_storage() {
        // Cover R2, R3 and R4.
        let (_execution, mut host, quota) = world_host(); // Construct one genuine original native registry.
        let (id, handle, _identity) = install_generated(&mut host); // Register one source which the valid control request could use.
        let forged = request(&quota, request_payload("forged-world-id", 4096)); // Supply a guest string with no registry entry.
        let before_forged = quota.snapshot().worker_bytes; // Capture request-plus-world debit before native lookup.
        assert!(host.prepare_region_response(&forged).is_err()); // A copied string must never reconstruct WorldHandle authority.
        assert_eq!(quota.snapshot().worker_bytes, before_forged); // Forged lookup must not acquire projection/encoding/result storage.
        let cancelled = request(&quota, request_payload(&id, 4096)); // Construct an otherwise valid original-root request.
        cancelled.stop_token().stop(); // Stop its real retained cancellation token before source access.
        let before_cancelled = quota.snapshot().worker_bytes; // Capture all preexisting owners after cancellation.
        assert!(host.prepare_region_response(&cancelled).is_err()); // Refuse before native region collection.
        assert_eq!(quota.snapshot().worker_bytes, before_cancelled); // Cancellation must create no extra projection/result debit.
        host.service.close_world(handle).unwrap(); // Remove the actual sealed native handle while intentionally leaving the guest host row stale for this regression.
        let closed = request(&quota, request_payload(&id, 4096)); // Reuse the former guest lookup string after native closure.
        let before_closed = quota.snapshot().worker_bytes; // Capture the post-source-close baseline.
        assert!(host.prepare_region_response(&closed).is_err()); // WorldService::check must reject the stale sealed handle.
        assert_eq!(quota.snapshot().worker_bytes, before_closed); // Closed-handle refusal must not escape a new region result.
    } // End forged/closed/cancelled source regression.

    #[test] // Request storage from an equal-limit foreign root must not reach the registered source.
    fn foreign_request_quota_cannot_query_original_world_registry() {
        // Extend R2 to actual quota identity rather than string identity only.
        let (_execution, mut host, quota) = world_host(); // Create the genuine original root and source registry.
        let (id, _handle, _identity) = install_generated(&mut host); // Register one valid source.
        let foreign = QuotaGroup::new(QuotaLimits {
            // Construct another ledger with deliberately similar finite limits.
            clients: 2,                      // Match shape without sharing identity.
            jobs: 4,                         // Match shape without sharing identity.
            service_jobs: 0,                 // Match shape without sharing identity.
            input_bytes: 8 * 1024 * 1024,    // Match shape without sharing identity.
            result_bytes: 8 * 1024 * 1024,   // Match shape without sharing identity.
            worker_threads: 4,               // Match shape without sharing identity.
            worker_bytes: 512 * 1024 * 1024, // Match shape without sharing identity.
        }); // End foreign equal-limit root.
        let foreign_request = request(&foreign, request_payload(&id, 4096)); // Carry syntactically valid source metadata under the wrong ledger.
        let original_before = quota.snapshot().worker_bytes; // Observe the real source root before attempted query.
        let foreign_before = foreign.snapshot().worker_bytes; // Observe the foreign request root before refusal.
        assert!(host.prepare_region_response(&foreign_request).is_err()); // Root identity must fail before source lookup.
        assert_eq!(quota.snapshot().worker_bytes, original_before); // The real source root must see no new projection admission.
        assert_eq!(foreign.snapshot().worker_bytes, foreign_before); // The foreign request keeps only its preexisting immutable payload debit.
    } // End foreign-root request regression.

    fn retained_completion(quota: &QuotaGroup) -> ServiceValue {
        // Build one genuine independently admitted binary completion for custody testing.
        let arrays = [ArraySpec {
            // Declare exactly one service plane.
            name: "b0".into(), // Use the canonical service binary name emitted by copy_region_response.
            kind: TypedArrayKind::U16, // Preserve native typed-array semantics.
            elements: 2,       // Encode two local indices.
        }]; // End binary inventory.
        let planes = BTreeMap::from([("b0".to_owned(), vec![0_u8, 0_u8, 1_u8, 0_u8])]); // Supply a finite little fixture; pointer identity matters more than numeric interpretation here.
        ServiceValue::copy_from_host(
            // Create the same immutable Arc-backed completion type used by production.
            &json!({"ok":true,"value":{"blocks":{"$ilium_binary":"b0"}}}), // Reference the declared plane through the actual service marker.
            &arrays,                  // Supply its exact U16 descriptor.
            &planes, // Copy bytes into one independently admitted result allocation.
            &EngineLimits::default(), // Apply real service limits.
            quota.clone(), // Charge only the supplied original root.
        ) // End completion construction.
        .unwrap() // Fixture bytes satisfy the service graph.
    } // End retained completion fixture.

    #[test] // Prove P1 retains the exact ServiceValue allocation rather than source graph or a second binary copy.
    fn uncertain_completion_keeps_exact_result_until_physical_retirement_evidence() {
        // Cover the deterministic custody portion of R11/R12.
        let quota = QuotaGroup::new(QuotaLimits {
            // Create a small isolated ownership ledger.
            clients: 1,                     // No actual client worker is needed.
            jobs: 1,                        // No actual job is needed.
            service_jobs: 1,                // Preserve finite service shape.
            input_bytes: 1024 * 1024,       // Admit one request.
            result_bytes: 1024 * 1024,      // Admit one result.
            worker_threads: 1,              // No worker is started.
            worker_bytes: 16 * 1024 * 1024, // Admit immutable request/result copies.
        }); // End isolated custody root.
        let request = request(&quota, request_payload("retained-world", 4096)); // Build one real immutable HostRequest allocation.
        let request_only = quota.snapshot().worker_bytes; // Record its retained payload debit.
        let result = retained_completion(&quota); // Add one independently admitted immutable result.
        let result_pointer = result.planes()["b0"].as_ptr(); // Capture exact allocation identity before ownership transfer.
        let request_and_result = quota.snapshot().worker_bytes; // Observe both original-root owners.
        assert!(request_and_result > request_only); // Prove result custody adds a real debit.
        let mut slot = None; // Start with no uncertain synchronous completion.
        NativeWorldHost::quarantine_uncertain_completion(
            // Execute the exact production ownership transition after completion error.
            &mut slot, // Transfer into the bounded quarantine owner.
            &request,  // Retain the exact original request allocation.
            result,    // Transfer the exact result alias without another plane copy.
        ) // End quarantine transition.
        .unwrap(); // First uncertainty must install successfully.
        assert!(request.is_cancelled()); // Quarantine fences continuation without asserting helper process exit.
        assert_eq!(slot.as_ref().unwrap().request.id, request.id); // Preserve exact request correlation.
        assert_eq!(
            slot.as_ref().unwrap()._result.planes()["b0"].as_ptr(),
            result_pointer
        ); // Prove exact ServiceValue allocation retention.
        assert_eq!(quota.snapshot().worker_bytes, request_and_result); // Cancellation must release neither immutable owner.
        NativeWorldHost::reap_uncertain_completion(&mut slot, false); // Model logical retirement with no physical helper proof.
        assert!(slot.is_some()); // Preserve completion custody.
        assert_eq!(quota.snapshot().worker_bytes, request_and_result); // Preserve its original-root debit.
        NativeWorldHost::reap_uncertain_completion(&mut slot, true); // Supply only the terminal physical-retirement condition.
        assert!(slot.is_none()); // Release the completion and its request clone.
        assert_eq!(quota.snapshot().worker_bytes, request_only); // The caller's original request alias is the only remaining owner.
        drop(request); // Release the final request alias.
        assert_eq!(quota.snapshot().worker_bytes, 0); // Return every isolated fixture admission to baseline.
    } // End deterministic uncertainty-custody regression.
} // End production raw-region preparation tests.

#[cfg(test)]
mod actual_region_uncertain_delivery_tests {
    use super::*;
    use crate::{
        engine::CreateState,
        helper::{isolation_qualification as fixture, HelperLimits},
        permissions::Ceiling,
        runtime::{InstancePreparation, PackageInstance},
        trust::TrustVerifier,
    };
    use ilium_execution::{ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaLimits};
    use std::{cell::Cell, collections::BTreeMap, path::Path};

    fn quota() -> QuotaGroup {
        QuotaGroup::new(QuotaLimits {
            clients: 4,
            jobs: 8,
            service_jobs: 0,
            input_bytes: 16 * 1024 * 1024,
            result_bytes: 16 * 1024 * 1024,
            worker_threads: 40,
            worker_bytes: 2 * 1024 * 1024 * 1024,
        })
    }

    fn world_host(quota: &QuotaGroup) -> (Execution, NativeWorldHost) {
        let zero = LaneConfig {
            threads: 0,
            queue_slots: 0,
            priority: None,
            resident_bytes_per_thread: 0,
        };
        let execution = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: LaneConfig {
                    threads: 1,
                    queue_slots: 1,
                    priority: None,
                    resident_bytes_per_thread: 1024 * 1024,
                },
                io: zero,
                service: zero,
            },
        )
        .unwrap();
        let resources = AmbientResources::new(
            execution
                .client(ClientLimits {
                    jobs: 4,
                    service_jobs: 0,
                    input_bytes: 4 * 1024 * 1024,
                    result_bytes: 4 * 1024 * 1024,
                })
                .unwrap(),
        );
        let host = NativeWorldHost::new(
            resources,
            quota.clone(),
            71,
            EngineLimits::default(),
            AnimationMode::Live,
        )
        .unwrap();
        (execution, host)
    }

    fn package_source() -> &'static str {
        r#"
export function plan(){
    return {format:'gray32',fps:30,inputs:{}};
}
export async function create(){
    const opened=await __ilium_dispatch('worlds.open',{id:'generated',seed:42});
    if(!opened.ok)throw Error('r11_generated_open_failed');
    const world={id:opened.value.id,kind:'worlds'};
    const region=await __ilium_dispatch('worlds.region',{world,x:-1,y:0,z:-2,width:2,height:1,depth:2,max_bytes:4096});
    if(!region.ok)throw Error('r11_region_failed');
    if(!(region.value.blocks instanceof Uint16Array))throw Error('r11_region_not_u16');
    if(region.value.blocks.length!==4)throw Error('r11_region_length');
    return {render(){},dispose(){}};
}
"#
    }

    fn package_instance(quota: &QuotaGroup) -> PackageInstance {
        package_instance_for_source(quota, package_source())
    }

    fn package_instance_for_source(quota: &QuotaGroup, source: &str) -> PackageInstance {
        let bytes = fixture::archive(source);
        let executable =
            std::env::var("ILIUM_ANIMATION_HELPER").expect("actual helper absolute path required");
        let verifier = TrustVerifier::from_release_inventory(vec![]).unwrap();
        let settings = json!({});
        let environment = json!({});
        let verified = PackageInstance::verify(InstancePreparation {
            archive: &bytes,
            verifier: &verifier,
            helper_executable: Path::new(&executable),
            trusted_bootstrap: crate::TRUSTED_BOOTSTRAP,
            settings: &settings,
            mode: AnimationMode::Live,
            environment: &environment,
            host_policy: Ceiling {
                permissions: vec![],
            },
            instance_id: 73,
            limits: HelperLimits::default(),
            quota: quota.clone(),
        })
        .unwrap();
        let (mut instance, review) = verified.prepare_without_rights().unwrap();
        let pending = instance.begin_resolution(review, BTreeMap::new()).unwrap();
        let resolution = instance.finish_resolution(pending).unwrap();
        assert_eq!(resolution.accepted_creation(), Some(CreateState::Pending));
        instance
    }

    #[test]
    #[ignore = "requires actual ILIUM_ANIMATION_HELPER and delegated Linux cgroup/bwrap isolation"]
    fn actual_helper_complete_service_ack_loss_retains_region_until_physical_retirement() {
        let quota = quota();
        let (mut execution, mut host) = world_host(&quota);
        let mut instance = package_instance(&quota);

        let mut requests = instance.requests().unwrap();
        assert_eq!(requests.len(), 1);
        let open_request = requests.remove(0);
        assert_eq!(open_request.method, "worlds.open");
        instance.check_native_world_request(&open_request).unwrap();
        host.open_generated(&mut instance, &open_request).unwrap();

        assert_eq!(instance.pump().unwrap(), CreateState::Pending);
        let mut requests = instance.requests().unwrap();
        assert_eq!(requests.len(), 1);
        let region_request = requests.remove(0);
        assert_eq!(region_request.method, "worlds.region");

        instance
            .arm_complete_service_ack_failure_for_test()
            .unwrap();

        let completion_error = host
            .region(&mut instance, &region_request)
            .expect_err("R11 requires the real post-write acknowledgement failure");
        let completion_message = completion_error.to_string();
        assert!(
            completion_message
                .contains("test interrupted CompleteService acknowledgement after packet release"),
            "{completion_message}",
        );
        assert!(
            completion_message
                .contains("test interrupted physical retirement before child shutdown"),
            "{completion_message}",
        );

        let transport = instance.helper_transport_test_snapshot();
        let sequence = transport
            .target_sequence
            .expect("CompleteService sequence was not registered");
        assert_eq!(transport.packet_released_sequence, Some(sequence));
        assert_eq!(transport.acknowledgement_failure_sequence, Some(sequence));
        assert_eq!(transport.write_succeeded, Some(true));
        assert_eq!(transport.retirement_interruptions, 1);
        assert!(transport.session_closed);
        assert!(!transport.physically_retired);
        assert!(!instance.is_physically_retired());

        let retained = host
            .uncertain_completion
            .as_ref()
            .expect("uncertain region result was not retained");
        assert_eq!(retained.request.id, region_request.id);
        assert_eq!(retained.request.authority, region_request.authority);
        assert_eq!(
            retained.request.package_digest,
            region_request.package_digest
        );
        assert!(retained.request.is_cancelled());
        assert!(region_request.is_cancelled());
        let transport_plane_pointer = transport
            .service_plane_pointer
            .expect("CompleteService packet carried no b0 ServiceValue plane");
        assert_eq!(
            retained._result.planes()["b0"].as_ptr() as usize,
            transport_plane_pointer,
        );

        let retained_pointer = retained._result.planes()["b0"].as_ptr() as usize;
        host.revoke();
        assert!(host.finite_work_drained());
        assert!(host.on_helper_retired().is_err());
        assert_eq!(
            host.uncertain_completion.as_ref().unwrap()._result.planes()["b0"].as_ptr() as usize,
            retained_pointer,
        );

        // A physically retired helper with the same visible package, instance
        // coordinates and quota root is not retirement evidence for this host.
        let mut foreign = package_instance(&quota);
        foreign.retire_helper().unwrap();
        assert!(foreign.is_physically_retired());
        assert!(!instance.is_physically_retired());
        let retained_after_foreign_exit = quota.snapshot().worker_bytes;
        assert!(host.on_helper_retired().is_err());
        assert_eq!(
            host.uncertain_completion.as_ref().unwrap()._result.planes()["b0"].as_ptr() as usize,
            retained_pointer,
        );
        assert_eq!(quota.snapshot().worker_bytes, retained_after_foreign_exit);

        instance.retire_helper().unwrap();
        assert!(instance.is_physically_retired());
        let retired_transport = instance.helper_transport_test_snapshot();
        assert!(retired_transport.physically_retired);
        assert_eq!(retired_transport.retirement_interruptions, 1);
        assert!(host.uncertain_completion.is_some());

        let before_reap = quota.snapshot().worker_bytes;
        host.on_helper_retired().unwrap();
        assert!(host.uncertain_completion.is_none());
        assert!(host.is_drained());
        let after_reap = quota.snapshot().worker_bytes;
        assert!(after_reap < before_reap);

        instance.revoke_activation().unwrap();
        drop(open_request);
        drop(region_request);
        drop(host);
        drop(foreign);
        drop(instance);
        execution.request_shutdown(ilium_execution::ShutdownMode::Cancel);
        let joined = execution
            .join_until_background(std::time::Instant::now() + std::time::Duration::from_secs(10))
            .unwrap();
        assert!(joined.shutdown_complete);
        assert_eq!(joined.remaining_workers, 0);
        drop(execution);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    #[ignore = "requires actual ILIUM_ANIMATION_HELPER and delegated Linux cgroup/bwrap isolation"]
    fn actual_helper_region_cancelled_after_result_copy_never_publishes_complete_service() {
        let quota = quota();
        let (mut execution, mut host) = world_host(&quota);
        let mut instance = package_instance(&quota);

        let mut requests = instance.requests().unwrap();
        assert_eq!(requests.len(), 1);
        let open_request = requests.remove(0);
        assert_eq!(open_request.method, "worlds.open");
        instance.check_native_world_request(&open_request).unwrap();
        host.open_generated(&mut instance, &open_request).unwrap();
        drop(open_request);

        assert_eq!(instance.pump().unwrap(), CreateState::Pending);
        let mut requests = instance.requests().unwrap();
        assert_eq!(requests.len(), 1);
        let region_request = requests.remove(0);
        assert_eq!(region_request.method, "worlds.region");

        let sequence_before = instance.helper_sequence_for_test();
        assert!(instance.helper_has_pending_service_request_for_test(region_request.id));
        let storage_before = quota.snapshot().worker_bytes;
        let stop = region_request.stop_token();
        let result_seen = Cell::new(false);
        let result_pointer = Cell::new(None::<usize>);
        let result_binary_bytes = Cell::new(0_usize);
        let storage_with_result = Cell::new(0_usize);

        let error = host
            .region_with_before_publication_hook_for_test(
                &mut instance,
                &region_request,
                |result| {
                    assert!(result.shares_root(&quota));
                    assert_eq!(result.metadata()["ok"], true);
                    assert_eq!(result.arrays().len(), 1);
                    assert_eq!(result.binary_bytes(), 8);
                    result_pointer.set(Some(result.planes()["b0"].as_ptr() as usize));
                    result_binary_bytes.set(result.binary_bytes());
                    storage_with_result.set(quota.snapshot().worker_bytes);
                    assert!(storage_with_result.get() > storage_before);
                    result_seen.set(true);
                    stop.stop();
                },
            )
            .expect_err("R7 requires cancellation before helper publication");

        assert!(error
            .to_string()
            .contains("world region cancelled before helper publication"));
        assert!(result_seen.get());
        assert!(result_pointer.get().is_some());
        assert_eq!(result_binary_bytes.get(), 8);
        assert!(region_request.is_cancelled());

        assert_eq!(instance.helper_sequence_for_test(), sequence_before,);
        assert!(instance.helper_has_pending_service_request_for_test(region_request.id));
        assert!(host.uncertain_completion.is_none());
        assert!(!host.closed);
        assert_eq!(quota.snapshot().worker_bytes, storage_before);

        host.revoke();
        instance.retire_helper().unwrap();
        assert!(instance.is_physically_retired());
        host.on_helper_retired().unwrap();
        instance.revoke_activation().unwrap();
        drop(region_request);
        drop(host);
        drop(instance);
        // Drop only signals shutdown; observe the original worker's physical exit.
        execution.request_shutdown(ilium_execution::ShutdownMode::Cancel);
        let joined = execution
            .join_until_background(std::time::Instant::now() + std::time::Duration::from_secs(10))
            .unwrap();
        assert!(joined.shutdown_complete);
        assert_eq!(joined.remaining_workers, 0);
        drop(execution);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    #[ignore = "requires actual ILIUM_ANIMATION_HELPER and delegated Linux cgroup/bwrap isolation"]
    fn actual_helper_region_success_resolves_typed_plane_and_retires_original_owners() {
        let quota = quota();
        let (mut execution, mut host) = world_host(&quota);
        let mut instance = package_instance(&quota);
        let mut requests = instance.requests().unwrap();
        assert_eq!(requests.len(), 1);
        let open_request = requests.remove(0);
        assert_eq!(open_request.method, "worlds.open");
        instance.check_native_world_request(&open_request).unwrap();
        host.open_generated(&mut instance, &open_request).unwrap();
        drop(open_request);
        assert_eq!(instance.pump().unwrap(), CreateState::Pending);
        let mut requests = instance.requests().unwrap();
        assert_eq!(requests.len(), 1);
        let region_request = requests.remove(0);
        assert_eq!(region_request.method, "worlds.region");
        let sequence_before = instance.helper_sequence_for_test();
        assert!(instance.helper_has_pending_service_request_for_test(region_request.id));
        host.region(&mut instance, &region_request).unwrap();
        assert!(instance.helper_sequence_for_test() > sequence_before);
        assert!(host.uncertain_completion.is_none());
        assert!(!host.closed);
        assert!(!region_request.is_cancelled());
        // Ready requires the real JavaScript checks for a four-element Uint16Array to pass.
        assert_eq!(instance.pump().unwrap(), CreateState::Ready);
        assert!(!instance.helper_has_pending_service_request_for_test(region_request.id));
        assert!(instance.requests().unwrap().is_empty());
        host.revoke();
        instance.retire_helper().unwrap();
        assert!(instance.is_physically_retired());
        host.on_helper_retired().unwrap();
        instance.revoke_activation().unwrap();
        drop(region_request);
        drop(host);
        drop(instance);
        execution.request_shutdown(ilium_execution::ShutdownMode::Cancel);
        let joined = execution
            .join_until_background(std::time::Instant::now() + std::time::Duration::from_secs(10))
            .unwrap();
        assert!(joined.shutdown_complete);
        assert_eq!(joined.remaining_workers, 0);
        drop(execution);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    #[ignore = "requires actual ILIUM_ANIMATION_HELPER and delegated Linux cgroup/bwrap isolation"]
    fn actual_helper_world_frame_sdk_preserves_original_resource_and_close() {
        let quota = quota();
        let (mut execution, mut host) = world_host(&quota);
        let mut draw = NativeDrawHost::new(
            quota.clone(),
            crate::native_media::MediaLimits::default(),
            crate::native_draw::DrawLimits::default(),
        )
        .unwrap();
        let source = r#"
export function plan(){return {format:'gray32',fps:30,inputs:{}};}
export async function create(){
    const host=__ilium_host;
    const opened=await host.worlds.open({id:'generated',seed:42});
    if(!opened.ok)throw Error('world_frame_open_failed');
    const framed=await host.worlds.frame({world:opened.value,width:2,height:1,time:0,wall:0});
    if(!framed.ok)throw Error('world_frame_failed');
    if(framed.value.world_id!==opened.value.id)throw Error('world_frame_original_world');
    if(framed.value.source_identity!==opened.value.identity)throw Error('world_frame_original_source');
    if(framed.value.width!==4||framed.value.height!==4)throw Error('world_frame_dot_dimensions');
    if(framed.value.status().state!=='ready')throw Error('world_frame_status');
    framed.value.close();
    return {render(){},dispose(){}};
}
"#;
        let mut instance = package_instance_for_source(&quota, source);
        let mut requests = instance.requests().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "worlds.open");
        assert!(host
            .dispatch(&mut instance, requests.remove(0), &mut draw, None)
            .unwrap()
            .is_none());
        assert_eq!(instance.pump().unwrap(), CreateState::Pending);
        let mut requests = instance.requests().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "worlds.frame");
        assert!(host
            .dispatch(&mut instance, requests.remove(0), &mut draw, None)
            .unwrap()
            .is_none());
        assert_eq!(host.active_frames.len(), 1);
        let frame_id = host.active_frames.keys().next().unwrap().clone();
        assert_eq!(instance.pump().unwrap(), CreateState::Ready);
        let mut requests = instance.requests().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "worlds.frame.close");
        assert!(host
            .dispatch(&mut instance, requests.remove(0), &mut draw, None)
            .unwrap()
            .is_none());
        assert_eq!(instance.pump().unwrap(), CreateState::Ready);
        assert!(instance.requests().unwrap().is_empty());
        assert!(host.active_frames.is_empty());
        assert_eq!(
            host.terminal_snapshots[&frame_id]["status"]["state"],
            "closed"
        );
        assert!(draw.release_world_frame(&frame_id).is_err());
        assert!(host.uncertain_completion.is_none());
        host.revoke();
        instance.retire_helper().unwrap();
        assert!(instance.is_physically_retired());
        host.on_helper_retired().unwrap();
        instance.revoke_activation().unwrap();
        drop(draw);
        drop(host);
        drop(instance);
        execution.request_shutdown(ilium_execution::ShutdownMode::Cancel);
        let joined = execution
            .join_until_background(std::time::Instant::now() + std::time::Duration::from_secs(10))
            .unwrap();
        assert!(joined.shutdown_complete);
        assert_eq!(joined.remaining_workers, 0);
        drop(execution);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    #[ignore = "requires actual ILIUM_ANIMATION_HELPER and delegated Linux cgroup/bwrap isolation"]
    fn actual_helper_presentation_blit_uses_original_native_world_frame() {
        let quota = quota();
        let (mut execution, mut host) = world_host(&quota);
        let mut draw = NativeDrawHost::new(
            quota.clone(),
            crate::native_media::MediaLimits::default(),
            crate::native_draw::DrawLimits::default(),
        )
        .unwrap();
        let source = r#"
export function plan(){return {format:'gray32',fps:30,inputs:{}};}
export async function create(){
    const host=__ilium_host;
    const opened=await host.worlds.open({id:'generated',seed:42});
    if(!opened.ok)throw Error('world_frame_open_failed');
    const framed=await host.worlds.frame({world:opened.value,width:2,height:1,time:0,wall:0});
    if(!framed.ok)throw Error('world_frame_failed');
    if(framed.value.world_id!==opened.value.id)throw Error('world_frame_original_world');
    if(framed.value.source_identity!==opened.value.identity)throw Error('world_frame_original_source');
    if(framed.value.width!==4||framed.value.height!==4)throw Error('world_frame_dot_dimensions');
    if(framed.value.status().state!=='ready')throw Error('world_frame_status');
    let accepted=false;
    void host.tasks.yield().then(result=>{
        if(!result.ok)throw Error('world_blit_close_turn_failed');
        if(!accepted)throw Error('world_blit_closed_before_native_acceptance');
        framed.value.close();
    });
    return {render(context,frame){
        const result=host.presentation.blit_source({frame,source:opened.value,world_frame:framed.value,rectangle:{unit:'cells',x:0,y:0,width:2,height:1}});
        if(!result.ok)throw Error('native_world_blit_failed');
        frame.after_accept(()=>{accepted=true;});
        frame.present();
    },dispose(){}};
}
"#;
        let mut instance = package_instance_for_source(&quota, source);
        let mut requests = instance.requests().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "worlds.open");
        assert!(host
            .dispatch(&mut instance, requests.remove(0), &mut draw, None)
            .unwrap()
            .is_none());
        assert_eq!(instance.pump().unwrap(), CreateState::Pending);
        let mut requests = instance.requests().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "worlds.frame");
        assert!(host
            .dispatch(&mut instance, requests.remove(0), &mut draw, None)
            .unwrap()
            .is_none());
        assert_eq!(host.active_frames.len(), 1);
        let frame_id = host.active_frames.keys().next().unwrap().clone();
        assert_eq!(instance.pump().unwrap(), CreateState::Ready);
        let mut task_host = crate::native_task_host::NativeTaskHost::new(
            quota.clone(),
            instance.engine_limits().clone(),
        )
        .unwrap();
        let mut close_turn_requests = instance.requests().unwrap();
        assert_eq!(close_turn_requests.len(), 1);
        assert_eq!(close_turn_requests[0].method, "tasks.yield");
        assert!(task_host
            .dispatch(&mut instance, close_turn_requests.remove(0))
            .unwrap()
            .is_none());
        assert!(task_host.next_due().is_some());
        let expected_pixels = host.active_frames[&frame_id]._source.raster().dots.clone();
        let mut native_bindings = draw.world_bindings();
        assert_eq!(native_bindings.len(), 1);
        let native_binding = native_bindings.remove(0);
        assert!(Arc::ptr_eq(
            native_binding.frame(),
            &host.active_frames[&frame_id]._source
        ));
        assert!(native_binding.shares_root(&quota));
        assert!(host.active_frames[&frame_id]
            ._source
            .raster()
            .owner_ids
            .iter()
            .all(|id| *id == 0));
        assert!(instance.requests().unwrap().is_empty());
        use crate::engine::{ArraySpec, TypedArrayKind};
        use crate::surface::{
            ColourSpace, Data, Format, FrameMeta, Mode, Planes, Shape, Surface, Update,
        };
        let shape = Shape {
            cell_width: 2,
            cell_height: 1,
            mode: Mode::Pixels,
            format: Format::Gray32,
            update: Update::Replace,
            cell_rgb: false,
            colour_space: ColourSpace::Srgb,
        };
        let layout = shape.layout().unwrap();
        let surface_admission = quota
            .reserve_external_storage(
                layout.canonical_bytes * 2 + layout.handoff_bytes * 2 + layout.dots * 64 + 65536,
            )
            .unwrap();
        let mut surface = Surface::new(73, 1, shape).unwrap();
        let seed = surface.begin(1).unwrap();
        let Data::F32(seed_pixels) = seed.data else {
            panic!("original gray32 seed");
        };
        let seed_spec = [ArraySpec {
            name: "work_data".into(),
            kind: TypedArrayKind::F32,
            elements: layout.elements,
        }];
        instance.seed_frame(&json!({"frame":{"key":seed.key,"shape":shape,"reset":seed.reset,"invalid_rects":seed.invalid_rects,"input_specs":[]},"services":[]}), &seed_spec, &BTreeMap::from([("work_data".into(), seed_pixels.into_iter().flat_map(f32::to_ne_bytes).collect())])).unwrap();
        let specs = [
            ("work_data", TypedArrayKind::F32),
            ("data", TypedArrayKind::F32),
            ("work_touch", TypedArrayKind::U8),
            ("touch", TypedArrayKind::U8),
            ("work_order", TypedArrayKind::U32),
            ("order", TypedArrayKind::U32),
        ]
        .into_iter()
        .map(|(name, kind)| ArraySpec {
            name: name.into(),
            kind,
            elements: layout.samples,
        })
        .collect::<Vec<_>>();
        let retained = instance.render(&json!({"time":0,"wall":0,"delta":0.025,"wall_delta":0.025,"inputs":{},"_ilium_frame":{"key":seed.key,"shape":shape}}), &specs).unwrap();
        assert!(
            instance.requests().unwrap().is_empty(),
            "blit must not call a service during render"
        );
        let (mut output, returned_admission) = retained.into_parts();
        let metadata = FrameMeta::parse(&serde_json::to_vec(&output.metadata).unwrap()).unwrap();
        assert_eq!(metadata.commands.len(), 1);
        assert!(metadata.error.is_none());
        let float_plane = |bytes: Vec<u8>| {
            bytes
                .chunks_exact(4)
                .map(|part| f32::from_ne_bytes(part.try_into().unwrap()))
                .collect()
        };
        let order = output
            .planes
            .remove("order")
            .unwrap()
            .chunks_exact(4)
            .map(|part| u32::from_ne_bytes(part.try_into().unwrap()))
            .collect();
        let planes = Planes {
            data: Data::F32(float_plane(output.planes.remove("data").unwrap())),
            touch: output.planes.remove("touch").unwrap(),
            order,
            cell_rgb: None,
            colour_touch: None,
            colour_order: None,
        };
        let outcome = draw
            .finish(
                &mut instance,
                &mut surface,
                metadata,
                planes,
                &StopToken::default(),
                |snapshot, _| {
                    let Data::F32(pixels) = snapshot.data() else {
                        panic!("native gray32 output");
                    };
                    assert_eq!(
                        pixels, &expected_pixels,
                        "exact original prepared world raster"
                    );
                    assert_eq!(snapshot.owners().len(), expected_pixels.len());
                    for (index, owner) in snapshot.owners().iter().enumerate() {
                        if expected_pixels[index] == 0.0 {
                            assert!(
                                owner.is_none(),
                                "unlit source dot must have no provenance token"
                            );
                        } else {
                            let token = owner
                                .expect("lit original source dot must retain native provenance");
                            assert!(native_binding.owns_token_range(token));
                            assert_eq!(native_binding.source_index(token), Some(index));
                            assert_eq!(
                                native_binding.frame().raster().owner_ids[index],
                                0,
                                "provenance must not invent a saved-history owner"
                            );
                        }
                    }
                    Ok(())
                },
            )
            .unwrap();
        assert!(outcome.accepted);
        instance.accept_frame(true).unwrap();
        assert!(
            instance.requests().unwrap().is_empty(),
            "logical acknowledgement must issue no service request"
        );
        let due = task_host.next_due().unwrap();
        assert!(task_host.on_due(&mut instance, due).unwrap());
        assert_eq!(instance.pump().unwrap(), CreateState::Ready);
        assert!(task_host.next_due().is_none());
        drop(returned_admission);
        drop(output); // Release the original six returned-plane admissions before checking the zero ledger.
        drop(surface);
        drop(surface_admission);
        let mut requests = instance.requests().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "worlds.frame.close");
        assert!(host
            .dispatch(&mut instance, requests.remove(0), &mut draw, None)
            .unwrap()
            .is_none());
        assert_eq!(instance.pump().unwrap(), CreateState::Ready);
        assert!(instance.requests().unwrap().is_empty());
        assert!(host.active_frames.is_empty());
        assert_eq!(
            host.terminal_snapshots[&frame_id]["status"]["state"],
            "closed"
        );
        assert!(draw.release_world_frame(&frame_id).is_err());
        assert!(host.uncertain_completion.is_none());
        host.revoke();
        instance.retire_helper().unwrap();
        assert!(instance.is_physically_retired());
        host.on_helper_retired().unwrap();
        instance.revoke_activation().unwrap();
        drop(native_binding);
        drop(native_bindings);
        task_host.revoke();
        assert!(task_host.is_drained());
        drop(task_host);
        drop(draw);
        drop(host);
        drop(instance);
        execution.request_shutdown(ilium_execution::ShutdownMode::Cancel);
        let joined = execution
            .join_until_background(std::time::Instant::now() + std::time::Duration::from_secs(10))
            .unwrap();
        assert!(joined.shutdown_complete);
        assert_eq!(joined.remaining_workers, 0);
        drop(execution);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    fn actual_helper_blit_format(
        format: crate::surface::Format,
        mode: crate::surface::Mode,
        wire_format: &str,
    ) {
        use crate::surface::{Format, Mode};
        let quota = quota();
        let (mut execution, mut host) = world_host(&quota);
        let mut draw = NativeDrawHost::new(
            quota.clone(),
            crate::native_media::MediaLimits::default(),
            crate::native_draw::DrawLimits::default(),
        )
        .unwrap();
        let source_template = r#"
    export function plan(){return {format:'gray32',fps:30,inputs:{}};}
    export async function create(){
        const host=__ilium_host;
        const opened=await host.worlds.open({id:'generated',seed:42});
        if(!opened.ok)throw Error('world_frame_open_failed');
        const framed=await host.worlds.frame({world:opened.value,width:2,height:1,time:0,wall:0});
        if(!framed.ok)throw Error('world_frame_failed');
        if(framed.value.world_id!==opened.value.id)throw Error('world_frame_original_world');
        if(framed.value.source_identity!==opened.value.identity)throw Error('world_frame_original_source');
        if(framed.value.width!==4||framed.value.height!==4)throw Error('world_frame_dot_dimensions');
        if(framed.value.status().state!=='ready')throw Error('world_frame_status');
        let accepted=false;
        void host.tasks.yield().then(result=>{
            if(!result.ok)throw Error('world_blit_close_turn_failed');
            if(!accepted)throw Error('world_blit_closed_before_native_acceptance');
            framed.value.close();
        });
        return {render(context,frame){
            const result=host.presentation.blit_source({frame,source:opened.value,world_frame:framed.value,rectangle:{unit:'cells',x:0,y:0,width:2,height:1}});
            if(!result.ok)throw Error('native_world_blit_failed');
            frame.after_accept(()=>{accepted=true;});
            frame.present();
        },dispose(){}};
    }
    "#;
        let mode_name = if mode == Mode::Cells {
            "cells"
        } else {
            "pixels"
        };
        let source = source_template.replace(
            "format:'gray32'",
            &format!("output:{{mode:'{mode_name}',format:'{wire_format}',update:'replace'}}"),
        );
        let mut instance = package_instance_for_source(&quota, &source);
        let mut requests = instance.requests().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "worlds.open");
        assert!(host
            .dispatch(&mut instance, requests.remove(0), &mut draw, None)
            .unwrap()
            .is_none());
        assert_eq!(instance.pump().unwrap(), CreateState::Pending);
        let mut requests = instance.requests().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "worlds.frame");
        assert!(host
            .dispatch(&mut instance, requests.remove(0), &mut draw, None)
            .unwrap()
            .is_none());
        assert_eq!(host.active_frames.len(), 1);
        let frame_id = host.active_frames.keys().next().unwrap().clone();
        assert_eq!(instance.pump().unwrap(), CreateState::Ready);
        let mut task_host = crate::native_task_host::NativeTaskHost::new(
            quota.clone(),
            instance.engine_limits().clone(),
        )
        .unwrap();
        let mut close_turn_requests = instance.requests().unwrap();
        assert_eq!(close_turn_requests.len(), 1);
        assert_eq!(close_turn_requests[0].method, "tasks.yield");
        assert!(task_host
            .dispatch(&mut instance, close_turn_requests.remove(0))
            .unwrap()
            .is_none());
        assert!(task_host.next_due().is_some());
        let expected_pixels = host.active_frames[&frame_id]._source.raster().dots.clone();
        let mut native_bindings = draw.world_bindings();
        assert_eq!(native_bindings.len(), 1);
        let native_binding = native_bindings.remove(0);
        assert!(Arc::ptr_eq(
            native_binding.frame(),
            &host.active_frames[&frame_id]._source
        ));
        assert!(native_binding.shares_root(&quota));
        let original_colors = host.active_frames[&frame_id]._source.colors().to_vec();
        let has_colors = host.active_frames[&frame_id]._source.has_cell_colors();
        assert!(host.active_frames[&frame_id]
            ._source
            .raster()
            .owner_ids
            .iter()
            .all(|id| *id == 0));
        assert!(instance.requests().unwrap().is_empty());
        use crate::engine::{ArraySpec, TypedArrayKind};
        use crate::surface::{ColourSpace, Data, FrameMeta, Planes, Shape, Surface, Update};
        let shape = Shape {
            cell_width: 2,
            cell_height: 1,
            mode,
            format,
            update: Update::Replace,
            cell_rgb: false,
            colour_space: ColourSpace::Srgb,
        };
        let layout = shape.layout().unwrap();
        let surface_admission = quota
            .reserve_external_storage(
                layout.canonical_bytes * 2 + layout.handoff_bytes * 2 + layout.dots * 64 + 65536,
            )
            .unwrap();
        let mut surface = Surface::new(73, 1, shape).unwrap();
        let seed = surface.begin(1).unwrap();
        let (seed_pixels, data_kind) = match seed.data {
            Data::F32(values) => (
                values
                    .into_iter()
                    .flat_map(f32::to_ne_bytes)
                    .collect::<Vec<_>>(),
                TypedArrayKind::F32,
            ),
            Data::U8(values) => (values, TypedArrayKind::U8),
        };
        let seed_spec = [ArraySpec {
            name: "work_data".into(),
            kind: data_kind,
            elements: layout.elements,
        }];
        instance.seed_frame(&json!({"frame":{"key":seed.key,"shape":shape,"reset":seed.reset,"invalid_rects":seed.invalid_rects,"input_specs":[]},"services":[]}), &seed_spec, &BTreeMap::from([("work_data".into(), seed_pixels)])).unwrap();
        let specs = [
            ("work_data", data_kind),
            ("data", data_kind),
            ("work_touch", TypedArrayKind::U8),
            ("touch", TypedArrayKind::U8),
            ("work_order", TypedArrayKind::U32),
            ("order", TypedArrayKind::U32),
        ]
        .into_iter()
        .map(|(name, kind)| ArraySpec {
            name: name.into(),
            kind,
            elements: if name == "data" || name == "work_data" {
                layout.elements
            } else {
                layout.samples
            },
        })
        .collect::<Vec<_>>();
        let retained = instance.render(&json!({"time":0,"wall":0,"delta":0.025,"wall_delta":0.025,"inputs":{},"_ilium_frame":{"key":seed.key,"shape":shape}}), &specs).unwrap();
        assert!(
            instance.requests().unwrap().is_empty(),
            "blit must not call a service during render"
        );
        let (mut output, returned_admission) = retained.into_parts();
        let metadata = FrameMeta::parse(&serde_json::to_vec(&output.metadata).unwrap()).unwrap();
        assert_eq!(metadata.commands.len(), 1);
        assert!(metadata.error.is_none());
        let float_plane = |bytes: Vec<u8>| {
            bytes
                .chunks_exact(4)
                .map(|part| f32::from_ne_bytes(part.try_into().unwrap()))
                .collect()
        };
        let order = output
            .planes
            .remove("order")
            .unwrap()
            .chunks_exact(4)
            .map(|part| u32::from_ne_bytes(part.try_into().unwrap()))
            .collect();
        let planes = Planes {
            data: if format == Format::Gray32 {
                Data::F32(float_plane(output.planes.remove("data").unwrap()))
            } else {
                Data::U8(output.planes.remove("data").unwrap())
            },
            touch: output.planes.remove("touch").unwrap(),
            order,
            cell_rgb: None,
            colour_touch: None,
            colour_order: None,
        };
        let outcome = draw
            .finish(
                &mut instance,
                &mut surface,
                metadata,
                planes,
                &StopToken::default(),
                |snapshot, _| {
                    match (format, snapshot.data()) {
                        (Format::Gray32, Data::F32(pixels)) => assert_eq!(pixels, &expected_pixels),
                        (Format::Mask8, Data::U8(pixels)) => {
                            // Published Unicode Braille positions, independent of the native renderer's table.
                            let bits = [[1u8, 8], [2, 16], [4, 32], [64, 128]];
                            let mut expected = vec![0u8; 2];
                            for (index, intensity) in expected_pixels.iter().enumerate() {
                                let x = index % 4;
                                let y = index / 4;
                                if *intensity >= 0.5 {
                                    expected[x / 2] |= bits[y][x % 2];
                                }
                            }
                            assert_eq!(pixels, &expected);
                        }
                        (Format::Mono1, Data::U8(pixels)) => {
                            let mut expected = vec![0u8; 4];
                            for (index, intensity) in expected_pixels.iter().enumerate() {
                                if *intensity >= 0.5 {
                                    expected[index / 4] |= 1 << (7 - index % 4);
                                }
                            }
                            assert_eq!(pixels, &expected);
                        }
                        (Format::Mono8, Data::U8(pixels)) => assert_eq!(
                            pixels,
                            &expected_pixels
                                .iter()
                                .map(|value| u8::from(*value >= 0.5))
                                .collect::<Vec<_>>()
                        ),
                        (Format::Gray8, Data::U8(pixels)) => assert_eq!(
                            pixels,
                            &expected_pixels
                                .iter()
                                .map(|value| (value * 255.).round() as u8)
                                .collect::<Vec<_>>()
                        ),
                        (Format::Rgb8 | Format::Rgba8, Data::U8(pixels)) => {
                            let channels = if format == Format::Rgba8 { 4 } else { 3 };
                            assert_eq!(pixels.len(), expected_pixels.len() * channels);
                            for (index, intensity) in expected_pixels.iter().enumerate() {
                                let color = if has_colors {
                                    original_colors[(index / 4) / 4 * 2 + (index % 4) / 2]
                                } else {
                                    [255, 255, 255]
                                };
                                for (channel, original) in color.into_iter().enumerate() {
                                    // Independent f64 IEC sRGB conversion; native f32 rounding may differ by one byte.
                                    let component = f64::from(original) / 255.;
                                    let linear = if component <= 0.04045 {
                                        component / 12.92
                                    } else {
                                        ((component + 0.055) / 1.055).powf(2.4)
                                    };
                                    let level = linear
                                        * if channels == 3 {
                                            f64::from(*intensity)
                                        } else {
                                            1.
                                        };
                                    let encoded = if level <= 0.0031308 {
                                        12.92 * level
                                    } else {
                                        1.055 * level.powf(1. / 2.4) - 0.055
                                    };
                                    let expected = (encoded.clamp(0., 1.) * 255.).round() as u8;
                                    assert!(
                                        pixels[index * channels + channel].abs_diff(expected) <= 1
                                    );
                                }
                                if channels == 4 {
                                    assert_eq!(
                                        pixels[index * channels + 3],
                                        (intensity * 255.).round() as u8
                                    );
                                }
                            }
                        }
                        _ => panic!("native format and typed-plane mismatch"),
                    }
                    assert_eq!(snapshot.owners().len(), expected_pixels.len());
                    for (index, owner) in snapshot.owners().iter().enumerate() {
                        let visible = if format == Format::Mask8 {
                            expected_pixels[index] >= 0.5
                        } else {
                            snapshot.states()[index] == 2
                        };
                        if !visible {
                            assert!(
                                owner.is_none(),
                                "quantized-away source dot must have no surviving provenance"
                            );
                        } else {
                            let token = owner.expect(
                                "visible original source dot must retain native provenance",
                            );
                            assert!(native_binding.owns_token_range(token));
                            assert_eq!(native_binding.source_index(token), Some(index));
                            assert_eq!(
                                native_binding.frame().raster().owner_ids[index],
                                0,
                                "generated provenance must not invent a saved-history owner"
                            );
                        }
                    }
                    Ok(())
                },
            )
            .unwrap();
        assert!(outcome.accepted);
        instance.accept_frame(true).unwrap();
        assert!(
            instance.requests().unwrap().is_empty(),
            "logical acknowledgement must issue no service request"
        );
        let due = task_host.next_due().unwrap();
        assert!(task_host.on_due(&mut instance, due).unwrap());
        assert_eq!(instance.pump().unwrap(), CreateState::Ready);
        assert!(task_host.next_due().is_none());
        drop(returned_admission);
        drop(output); // Returned plane owner must retire before the zero-ledger assertion.
        drop(surface);
        drop(surface_admission);
        let mut requests = instance.requests().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "worlds.frame.close");
        assert!(host
            .dispatch(&mut instance, requests.remove(0), &mut draw, None)
            .unwrap()
            .is_none());
        assert_eq!(instance.pump().unwrap(), CreateState::Ready);
        assert!(instance.requests().unwrap().is_empty());
        assert!(host.active_frames.is_empty());
        assert_eq!(
            host.terminal_snapshots[&frame_id]["status"]["state"],
            "closed"
        );
        assert!(draw.release_world_frame(&frame_id).is_err());
        assert!(host.uncertain_completion.is_none());
        host.revoke();
        instance.retire_helper().unwrap();
        assert!(instance.is_physically_retired());
        host.on_helper_retired().unwrap();
        instance.revoke_activation().unwrap();
        drop(native_binding);
        drop(native_bindings);
        task_host.revoke();
        assert!(task_host.is_drained());
        drop(task_host);
        drop(draw);
        drop(host);
        drop(instance);
        execution.request_shutdown(ilium_execution::ShutdownMode::Cancel);
        let joined = execution
            .join_until_background(std::time::Instant::now() + std::time::Duration::from_secs(10))
            .unwrap();
        assert!(joined.shutdown_complete);
        assert_eq!(joined.remaining_workers, 0);
        drop(execution);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    #[ignore = "requires actual ILIUM_ANIMATION_HELPER and delegated Linux cgroup/bwrap isolation"]
    fn actual_helper_world_blit_mask8_original_raster() {
        actual_helper_blit_format(
            crate::surface::Format::Mask8,
            crate::surface::Mode::Cells,
            "mask8",
        );
    }
    #[test]
    #[ignore = "requires actual ILIUM_ANIMATION_HELPER and delegated Linux cgroup/bwrap isolation"]
    fn actual_helper_world_blit_mono1_original_raster() {
        actual_helper_blit_format(
            crate::surface::Format::Mono1,
            crate::surface::Mode::Pixels,
            "mono1",
        );
    }
    #[test]
    #[ignore = "requires actual ILIUM_ANIMATION_HELPER and delegated Linux cgroup/bwrap isolation"]
    fn actual_helper_world_blit_mono8_original_raster() {
        actual_helper_blit_format(
            crate::surface::Format::Mono8,
            crate::surface::Mode::Pixels,
            "mono8",
        );
    }
    #[test]
    #[ignore = "requires actual ILIUM_ANIMATION_HELPER and delegated Linux cgroup/bwrap isolation"]
    fn actual_helper_world_blit_gray8_original_raster() {
        actual_helper_blit_format(
            crate::surface::Format::Gray8,
            crate::surface::Mode::Pixels,
            "gray8",
        );
    }
    #[test]
    #[ignore = "requires actual ILIUM_ANIMATION_HELPER and delegated Linux cgroup/bwrap isolation"]
    fn actual_helper_world_blit_gray32_original_raster() {
        actual_helper_blit_format(
            crate::surface::Format::Gray32,
            crate::surface::Mode::Pixels,
            "gray32",
        );
    }
    #[test]
    #[ignore = "requires actual ILIUM_ANIMATION_HELPER and delegated Linux cgroup/bwrap isolation"]
    fn actual_helper_world_blit_rgb8_original_raster() {
        actual_helper_blit_format(
            crate::surface::Format::Rgb8,
            crate::surface::Mode::Pixels,
            "rgb8",
        );
    }
    #[test]
    #[ignore = "requires actual ILIUM_ANIMATION_HELPER and delegated Linux cgroup/bwrap isolation"]
    fn actual_helper_world_blit_rgba8_original_raster() {
        actual_helper_blit_format(
            crate::surface::Format::Rgba8,
            crate::surface::Mode::Pixels,
            "rgba8",
        );
    }
}
