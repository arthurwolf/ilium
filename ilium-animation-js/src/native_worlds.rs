//! Native world/model/texture custody, separate from script pixels. Saved-world
//! roots are host-opened no-follow handles; source owners never cross the JS API.
use crate::error::{AnimationError, Result};
use ilium_ambient::{
    raster::PaintedOwner,
    resources::AmbientResources,
    scene::FrameReceiptId,
    voxel_landscape::{
        assets::{
            models::NormalizedModel,
            texture::{LinearRgba, Texture},
        },
        chunks::ColumnCache,
        generation, render,
    },
    Frame, Raster, Scene, VoxelLandscapeSettings,
};
use ilium_execution::{QuotaGroup, StorageAdmission};
use ilium_platform::{owned_worker::StopToken, secure_fs::NoFollowDirectory};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, Weak,
    },
    time::{Duration, SystemTime},
};
fn error(message: &str) -> AnimationError {
    AnimationError::Runtime(message.into())
}
static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(1);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldHandle {
    instance: u64,
    id: u64,
    epoch: u64,
}
impl WorldHandle {
    /// Projection only. Native request resolution uses the owning registry,
    /// never parses this string into authority.
    pub fn opaque_id(self) -> String {
        format!("world-{}-{}-{}", self.instance, self.id, self.epoch)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetHandle {
    instance: u64,
    id: u64,
    epoch: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceIdentity([u8; 32]);
impl SourceIdentity {
    pub fn from_host_digest(digest: [u8; 32]) -> Self {
        Self(digest)
    }
    pub fn hex(self) -> String {
        use std::fmt::Write;
        let mut output = String::with_capacity(64);
        for byte in self.0 {
            let _ = write!(&mut output, "{byte:02x}");
        }
        output
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedWorldSettings {
    pub seed: u32,
    pub minimum: [i32; 2],
    pub maximum: [i32; 2],
    pub rivers: bool,
    pub ravines: bool,
    pub scale: f32,
}
impl Default for GeneratedWorldSettings {
    fn default() -> Self {
        Self {
            seed: 42,
            minimum: [-2, -2],
            maximum: [3, 3],
            rivers: true,
            ravines: false,
            scale: 4.,
        }
    }
}
impl GeneratedWorldSettings {
    fn validate(&self) -> Result<()> {
        if !self.scale.is_finite()
            || !(0.5..=16.).contains(&self.scale)
            || (0..2).any(|axis| {
                self.minimum[axis] < -1000000
                    || self.maximum[axis] > 1000000
                    || self.maximum[axis] <= self.minimum[axis]
                    || i64::from(self.maximum[axis]) - i64::from(self.minimum[axis]) > 32
            })
        {
            return Err(error("generated world bounds exceed host policy"));
        }
        Ok(())
    }
}
/// Host selected root and exact native provenance revision; no path parameter.
pub struct SavedWorldGrant {
    root: Arc<NoFollowDirectory>,
    identity: SourceIdentity,
    epoch: u64,
    history_authorized: bool,
}
impl SavedWorldGrant {
    pub fn from_host(
        root: Arc<NoFollowDirectory>,
        identity: SourceIdentity,
        epoch: u64,
        history_authorized: bool,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(error("world grant epoch missing"));
        }
        Ok(Self {
            root,
            identity,
            epoch,
            history_authorized,
        })
    }
    pub fn root(&self) -> &NoFollowDirectory {
        &self.root
    }
    pub fn identity(&self) -> SourceIdentity {
        self.identity
    }
    pub fn history_authorized(&self) -> bool {
        self.history_authorized
    }
    pub(crate) fn epoch(&self) -> u64 {
        self.epoch
    }
}
/// Root factory must admit its complete native graph before preparing models,
/// retained palettes, reading this handle or spawning owned workers. The scene
/// owns actual join custody and existing native IssuedView/FrameOwners receipts.
/// Do not adapt path-discovering SavedScene::new as though it accepted a handle.
pub trait SavedWorldFactory {
    fn prepare(
        &mut self,
        grant: &SavedWorldGrant,
        resources: &AmbientResources,
    ) -> Result<HostWorldScene>;
}
pub struct HostWorldScene {
    scene: Box<dyn Scene>,
    identity: SourceIdentity,
    _admission: Arc<StorageAdmission>,
    root: Option<Arc<NoFollowDirectory>>,
    generated: Option<Arc<generation::PreparedWorld>>,
}
impl HostWorldScene {
    pub fn from_host(
        scene: Box<dyn Scene>,
        identity: SourceIdentity,
        admission: Arc<StorageAdmission>,
        root: Arc<NoFollowDirectory>,
    ) -> Self {
        Self {
            scene,
            identity,
            _admission: admission,
            root: Some(root),
            generated: None,
        }
    }
}
struct GeneratedScene {
    world: Arc<generation::PreparedWorld>,
    camera: [f64; 3],
    scale: f32,
    seed: u64,
}
impl Scene for GeneratedScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let size = [usize::from(frame.width) * 2, usize::from(frame.height) * 4];
        let canvas = render::draw_world(&self.world, self.camera, self.scale, size, self.seed);
        for (dot, color) in frame.raster.dots.iter_mut().zip(&canvas.colors) {
            *dot = (f32::from(color[0]) + f32::from(color[1]) + f32::from(color[2])) / (3. * 255.);
        }
        for y in 0..usize::from(frame.height) {
            for x in 0..usize::from(frame.width) {
                let mut sum = [0u32; 3];
                for dy in 0..4 {
                    for dx in 0..2 {
                        let rgb = canvas.colors[(y * 4 + dy) * size[0] + x * 2 + dx];
                        for channel in 0..3 {
                            sum[channel] += u32::from(rgb[channel]);
                        }
                    }
                }
                frame.cell_colors[y * usize::from(frame.width) + x] =
                    sum.map(|value| (value / 8) as u8);
            }
        }
    }
    fn uses_cell_colors(&self) -> bool {
        true
    }
}
struct WorldEntry {
    binding: Arc<Mutex<HostWorldScene>>,
    saved: bool,
    slots: [Weak<NativeWorldFrame>; 3],
    sequence: u64,
    last_clock: Option<(Duration, Duration)>,
}
pub struct WorldRenderRequest {
    pub width: u16,
    pub height: u16,
    pub time: Duration,
    pub wall: Duration,
    pub now: SystemTime,
    pub pre_rendered: bool,
}
/// The original raster, source token, native receipt and all owners are retained
/// in the Rust host. Serialize only intensity/colour copies to JS if requested.
pub struct NativeWorldFrame {
    handle: WorldHandle,
    receipt: FrameReceiptId,
    identity: SourceIdentity,
    raster: Raster,
    colors: Vec<[u8; 3]>,
    has_cell_colors: bool,
    pre_rendered: bool,
    emission_fence: Arc<Mutex<bool>>,
    quota: QuotaGroup,
    _source: Arc<StorageAdmission>,
    _scene_retention: Arc<Mutex<HostWorldScene>>,
    _root: Option<Arc<NoFollowDirectory>>,
    _admission: StorageAdmission,
}
impl NativeWorldFrame {
    /// Settles a proof minted by the trusted compositor before its ordered
    /// cancellation fence. Parent MUST order actual terminal emission, proof
    /// minting and revocation on its host compositor fence; a pixel buffer alone
    /// cannot prove emission. This API is not exposed as a script/IPC handler.
    pub fn settle_committed(&self, evidence: &TerminalPaintEvidence) -> Result<()> {
        if self.pre_rendered
            || evidence.handle != self.handle
            || evidence.receipt != self.receipt
            || !Arc::ptr_eq(&evidence.source, &self._scene_retention)
        {
            return Err(error("native terminal receipt mismatch"));
        }
        evidence
            .source
            .lock()
            .map_err(|_| error("native source custody poisoned"))?
            .scene
            .presented_frame(self.receipt, &evidence.owners);
        Ok(())
    }

    pub fn world_id(&self) -> String {
        self.handle.opaque_id()
    }
    pub fn dimensions(&self) -> (usize, usize) {
        (self.raster.width, self.raster.height)
    }
    pub fn raster(&self) -> &Raster {
        &self.raster
    }
    pub fn colors(&self) -> &[[u8; 3]] {
        &self.colors
    }
    pub fn has_cell_colors(&self) -> bool {
        self.has_cell_colors
    }
    pub fn source_identity(&self) -> SourceIdentity {
        self.identity
    }
    pub fn world_handle(&self) -> WorldHandle {
        self.handle
    }
    pub fn is_pre_rendered(&self) -> bool {
        self.pre_rendered
    }
    pub fn receipt(&self) -> FrameReceiptId {
        self.receipt
    }
}
/// Original native source-dot range. Tokens are process-unique and never
/// serialized to JavaScript. The admitted frame Arc survives until every
/// snapshot/output receipt that names the range has retired.
pub struct WorldDotBinding {
    frame: Arc<NativeWorldFrame>,
    first: u64,
    length: usize,
    _metadata: StorageAdmission,
}
impl std::fmt::Debug for WorldDotBinding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorldDotBinding")
            .field("world", &self.frame.handle)
            .field("receipt", &self.frame.receipt)
            .field("first", &self.first)
            .field("length", &self.length)
            .finish()
    }
}
impl WorldDotBinding {
    pub fn range_start(&self) -> u64 {
        self.first
    }
    pub fn frame(&self) -> &Arc<NativeWorldFrame> {
        &self.frame
    }
    pub fn token_for(
        &self,
        frame: &NativeWorldFrame,
        index: usize,
        owner: u32,
    ) -> Result<Option<u64>> {
        if !std::ptr::eq(frame, self.frame.as_ref())
            || index >= self.length
            || frame.raster.owner_ids[index] != owner
        {
            return Err(error("world dot token has no original source"));
        }
        if frame.raster.dots[index] == 0.0 {
            return Ok(None);
        }
        let index = u64::try_from(index).map_err(|_| error("world dot index range"))?;
        self.first
            .checked_add(index)
            .map(Some)
            .ok_or_else(|| error("world dot token range"))
    }
    /// Resolve only under retained native custody; repeated destination pixels
    /// may name this same source index and are deduplicated before history credit.
    pub fn owns_token_range(&self, token: crate::surface::SourceToken) -> bool {
        token
            .evidence_key()
            .checked_sub(self.first)
            .is_some_and(|offset| usize::try_from(offset).is_ok_and(|index| index < self.length))
    }
    pub fn source_index(&self, token: crate::surface::SourceToken) -> Option<usize> {
        let index = token.evidence_key().checked_sub(self.first)?;
        let index = usize::try_from(index).ok()?;
        (index < self.length && self.frame.raster.dots[index] > 0.0).then_some(index)
    }
    /// A retained binding can be credited only by an output authority charged
    /// to the same native quota root as the original world source.
    pub fn shares_root(&self, quota: &QuotaGroup) -> bool {
        self.frame.quota.shares_root(quota)
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct WorldPresentationOwner {
    pub token: String,
    pub dots: u32,
}
/// Aliases retain the same physical metadata and its exact original admission.
#[derive(Clone, Debug)]
pub struct WorldPresentationReceipt(pub(crate) Arc<WorldPresentationReceiptData>);
#[derive(Debug, Serialize)]
pub struct WorldPresentationReceiptData {
    pub frame_id: String,
    pub source_identity: String,
    pub composition_revision: u64,
    /// Number of surviving physical terminal dots, before source-index deduplication.
    pub emitted_dots: u32,
    pub owners: Vec<WorldPresentationOwner>,
    #[serde(skip)]
    pub(crate) owner_namespace: u64,
    #[serde(skip)]
    pub(crate) admission: StorageAdmission,
}
impl std::ops::Deref for WorldPresentationReceipt {
    type Target = WorldPresentationReceiptData;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Serialize for WorldPresentationReceipt {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        self.0.as_ref().serialize(serializer)
    }
}
/// A complete backend flush and the original broker proof precede construction.
/// This value retains native source/raster/history custody until the worker
/// processes the terminal acknowledgement. It never appears in script input.
pub struct WorldEmission {
    frame: Arc<NativeWorldFrame>,
    evidence: TerminalPaintEvidence,
    receipt: WorldPresentationReceipt,
}
impl std::fmt::Debug for WorldEmission {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorldEmission")
            .field("receipt", &self.receipt)
            .finish_non_exhaustive()
    }
}
impl WorldEmission {
    pub fn after_host_emission(
        binding: &Arc<WorldDotBinding>,
        unique_source_indices: &[usize],
        emitted_owner_dots: &BTreeMap<u32, u32>,
        composition_revision: u64,
    ) -> Result<Self> {
        Self::construct(
            binding,
            unique_source_indices,
            emitted_owner_dots,
            composition_revision,
            false,
        )
    }
    /// Called only after RetainedFrameAuthority verifies the exact terminal
    /// flush proof. A later service cancellation fences new output, not this
    /// already emitted frame's history receipt.
    pub(crate) fn after_proven_host_emission(
        binding: &Arc<WorldDotBinding>,
        unique_source_indices: &[usize],
        emitted_owner_dots: &BTreeMap<u32, u32>,
        composition_revision: u64,
    ) -> Result<Self> {
        Self::construct(
            binding,
            unique_source_indices,
            emitted_owner_dots,
            composition_revision,
            true,
        )
    }
    fn construct(
        binding: &Arc<WorldDotBinding>,
        unique_source_indices: &[usize],
        emitted_owner_dots: &BTreeMap<u32, u32>,
        composition_revision: u64,
        proven_flush: bool,
    ) -> Result<Self> {
        if composition_revision > 9_007_199_254_740_991
            || emitted_owner_dots.keys().filter(|&&id| id != 0).count() > 8192
        {
            return Err(error("world presentation receipt bound"));
        }
        let emitted_dots = emitted_owner_dots.values().try_fold(0_u32, |sum, value| {
            sum.checked_add(*value)
                .ok_or_else(|| error("world emitted dot count"))
        })?;
        if emitted_dots > (crate::surface::MAX_CELLS * 8) as u32 {
            return Err(error("world emitted dot count exceeds terminal frame"));
        }
        let frame = Arc::clone(binding.frame());
        if unique_source_indices.len() > frame.raster.dots.len() {
            return Err(error("world source witnesses exceed original frame"));
        }
        // Admit temporary map nodes before projecting original owner groups.
        // Zero is retained here for physical counts, never public attribution.
        let witness_bytes = unique_source_indices
            .len()
            .checked_mul(128)
            .and_then(|bytes| bytes.checked_add(1024))
            .ok_or_else(|| error("world source witness admission overflow"))?;
        let _witness_admission = frame
            .quota
            .reserve_external_storage(witness_bytes)
            .map_err(|failure| {
                AnimationError::Budget(format!("world source witnesses: {failure:?}"))
            })?;
        let mut witnessed_owner_dots = BTreeMap::<u32, u32>::new();
        for &index in unique_source_indices {
            let intensity = frame
                .raster
                .dots
                .get(index)
                .ok_or_else(|| error("world source witness index"))?;
            if *intensity <= 0.0 {
                return Err(error("world source witness names an unlit dot"));
            }
            *witnessed_owner_dots
                .entry(frame.raster.owner_ids[index])
                .or_default() += 1;
        }
        if witnessed_owner_dots.len() != emitted_owner_dots.len()
            || emitted_owner_dots.iter().any(|(owner, &physical)| {
                physical == 0
                    || witnessed_owner_dots
                        .get(owner)
                        .is_none_or(|&original| physical < original)
            })
        {
            return Err(error("world physical counts lack original owner witnesses"));
        }
        // This history witness uses each original source dot once; a scaled
        // source may legitimately contribute several distinct terminal dots.
        let evidence =
            TerminalPaintEvidence::construct(&frame, unique_source_indices, proven_flush)?;
        let owner_count = emitted_owner_dots.keys().filter(|&&id| id != 0).count();
        let metadata_bytes = owner_count
            .checked_mul(256)
            .and_then(|bytes| bytes.checked_add(4096))
            .ok_or_else(|| error("world informational receipt admission overflow"))?;
        let admission = frame
            .quota
            .reserve_external_storage(metadata_bytes)
            .map_err(|failure| {
                AnimationError::Budget(format!("world informational receipt: {failure:?}"))
            })?;
        let mut owners = Vec::with_capacity(owner_count);
        for (group, (_, &dots)) in emitted_owner_dots
            .iter()
            .filter(|(&id, _)| id != 0)
            .enumerate()
        {
            use std::fmt::Write;
            // At most 13+20+1+4 bytes; the admitted capacity never grows.
            let mut token = String::with_capacity(64);
            write!(
                &mut token,
                "source-owner-{}-{}",
                binding.range_start(),
                group + 1
            )
            .map_err(|_| error("world owner token formatting"))?;
            owners.push(WorldPresentationOwner { token, dots });
        }
        let receipt = WorldPresentationReceipt(Arc::new(WorldPresentationReceiptData {
            frame_id: format!("{}-{}", frame.world_id(), frame.receipt().sequence()),
            source_identity: frame.source_identity().hex(),
            composition_revision,
            emitted_dots,
            owners,
            owner_namespace: binding.range_start(),
            admission,
        }));
        Ok(Self {
            frame,
            evidence,
            receipt,
        })
    }
    #[allow(clippy::result_large_err)] // Rejection returns original admitted owners inline; do not allocate to retain custody.
    pub fn settle(self) -> std::result::Result<WorldPresentationReceipt, (Self, AnimationError)> {
        if let Err(error) = self.frame.settle_committed(&self.evidence) {
            return Err((self, error));
        }
        Ok(self.receipt)
    }
}

/// Mint ONLY after the host compositor successfully emits this retained frame.
/// The terminal's final surviving dot indices are mapped to native owner IDs;
/// caller-supplied script owner IDs and speculative render counts are absent.
pub struct TerminalPaintEvidence {
    handle: WorldHandle,
    receipt: FrameReceiptId,
    owners: Vec<PaintedOwner>,
    source: Arc<Mutex<HostWorldScene>>,
    _admission: StorageAdmission,
}
impl TerminalPaintEvidence {
    pub fn after_host_emission(
        frame: &NativeWorldFrame,
        painted_dot_indices: &[usize],
    ) -> Result<Self> {
        Self::construct(frame, painted_dot_indices, false)
    }
    fn construct(
        frame: &NativeWorldFrame,
        painted_dot_indices: &[usize],
        proven_flush: bool,
    ) -> Result<Self> {
        if !proven_flush {
            let closed = frame
                .emission_fence
                .lock()
                .map_err(|_| error("native emission fence poisoned"))?;
            if *closed {
                return Err(error("native emission fence closed"));
            }
        }
        if frame.pre_rendered {
            return Err(error(
                "pre-rendered frames cannot credit saved-world history",
            ));
        }
        if painted_dot_indices.len() > frame.raster.dots.len() {
            return Err(error("paint evidence exceeds native frame"));
        }
        let admission = frame
            .quota
            .reserve_external_storage(1024 + painted_dot_indices.len() * 96)
            .map_err(|failure| {
                AnimationError::Budget(format!("terminal evidence admission: {failure:?}"))
            })?;
        let mut seen = BTreeSet::new();
        let mut owners = BTreeMap::<u32, u32>::new();
        for &index in painted_dot_indices {
            if index >= frame.raster.dots.len() || !seen.insert(index) {
                return Err(error("invalid or duplicate painted native dot"));
            }
            let owner = frame.raster.owner_ids[index];
            if owner != 0 {
                if frame.raster.dots[index] <= 0. {
                    return Err(error("paint evidence attributes an unlit dot"));
                }
                *owners.entry(owner).or_default() += 1;
            }
        }
        Ok(Self {
            handle: frame.handle,
            receipt: frame.receipt,
            source: Arc::clone(&frame._scene_retention),
            _admission: admission,
            owners: owners
                .into_iter()
                .map(|(id, dots)| PaintedOwner { id, dots })
                .collect(),
        })
    }
}
/// Native supplied assets retain their ORIGINAL admitted graph. No duplicate
/// model/texture decoding, clocks or resource paths are delegated to scripts.
pub struct HostAssets {
    model: Arc<NormalizedModel>,
    texture: Arc<Texture>,
    quota: QuotaGroup,
    _admission: Arc<StorageAdmission>,
}
impl HostAssets {
    pub fn from_host(
        model: Arc<NormalizedModel>,
        texture: Arc<Texture>,
        quota: QuotaGroup,
        admission: Arc<StorageAdmission>,
    ) -> Result<Self> {
        let [width, height] = texture.image().dimensions();
        if model.quads.len() > 65536 || width > 4096 || height > 4096 {
            return Err(error("native model/texture exceeds adapter bounds"));
        }
        Ok(Self {
            model,
            texture,
            quota,
            _admission: admission,
        })
    }
}
#[derive(Debug, Serialize)]
pub struct ModelDescriptor {
    pub quad_count: usize,
    pub image_dimensions: [u32; 2],
}
pub struct WorldService {
    resources: AmbientResources,
    quota: QuotaGroup,
    instance: u64,
    epoch: u64,
    next_id: u64,
    worlds: BTreeMap<u64, WorldEntry>,
    assets: BTreeMap<u64, HostAssets>,
    _storage: StorageAdmission,
    cancelled: bool,
    emission_fence: Arc<Mutex<bool>>,
}
impl WorldService {
    pub fn new(resources: AmbientResources, quota: QuotaGroup, epoch: u64) -> Result<Self> {
        if epoch == 0 || !quota.shares_root(&resources.finite().quota_group()) {
            return Err(error("world service grant/quota mismatch"));
        }
        let storage = quota
            .reserve_external_storage(64 * 1024)
            .map_err(|failure| {
                AnimationError::Budget(format!("world registry admission: {failure:?}"))
            })?;
        let instance = NEXT_INSTANCE
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map_err(|_| error("world instance identity exhausted"))?;
        Ok(Self {
            resources,
            quota,
            instance,
            epoch,
            next_id: 1,
            worlds: BTreeMap::new(),
            assets: BTreeMap::new(),
            _storage: storage,
            cancelled: false,
            emission_fence: Arc::new(Mutex::new(false)),
        })
    }
    fn next(&mut self) -> Result<u64> {
        if self.cancelled {
            return Err(error("world service cancelled"));
        }
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .ok_or_else(|| error("world handle identity exhausted"))?;
        Ok(id)
    }
    fn insert(&mut self, binding: HostWorldScene, saved: bool) -> Result<WorldHandle> {
        if self.worlds.len() >= 8 {
            return Err(error("native world handle budget exhausted"));
        }
        let id = self.next()?;
        self.worlds.insert(
            id,
            WorldEntry {
                binding: Arc::new(Mutex::new(binding)),
                saved,
                slots: std::array::from_fn(|_| Weak::new()),
                sequence: 0,
                last_clock: None,
            },
        );
        Ok(WorldHandle {
            instance: self.instance,
            id,
            epoch: self.epoch,
        })
    }
    pub fn insert_generated(&mut self, settings: GeneratedWorldSettings) -> Result<WorldHandle> {
        self.insert_generated_with_stop(settings, &StopToken::default())
    }
    /// The original request cancellation signal aborts finite preparation.
    pub fn insert_generated_with_stop(
        &mut self,
        settings: GeneratedWorldSettings,
        stop: &StopToken,
    ) -> Result<WorldHandle> {
        settings.validate()?;
        if stop.is_stopped() {
            return Err(error("generated world preparation stopped"));
        }
        if self.cancelled || self.worlds.len() >= 8 {
            return Err(error("world service unavailable"));
        }
        // A conservative bounded preparation account includes terrain cache,
        // columns, feature geometry and transient map nodes, before generation.
        let admission = Arc::new(
            self.quota
                .reserve_external_storage(64 * 1024 * 1024)
                .map_err(|failure| {
                    AnimationError::Budget(format!("generated world admission: {failure:?}"))
                })?,
        );
        let native = VoxelLandscapeSettings {
            seed: settings.seed,
            rivers: settings.rivers,
            ravines: settings.ravines,
            caves: false,
            detail: 0,
            ..Default::default()
        };
        let mut cache = ColumnCache::new(16);
        let world = generation::prepare(
            generation::Region {
                minimum: settings.minimum,
                maximum: settings.maximum,
            },
            &native,
            &mut cache,
            || stop.is_stopped(),
        )
        .ok_or_else(|| error("native generated preparation rejected or stopped"))?;
        if stop.is_stopped() {
            return Err(error("generated world preparation stopped"));
        }
        let height = world
            .blocks
            .iter()
            .map(|block| block.position[2])
            .max()
            .unwrap_or(64);
        let camera = [
            f64::from(settings.minimum[0] + settings.maximum[0]) / 2.,
            f64::from(settings.minimum[1] + settings.maximum[1]) / 2.,
            f64::from(height),
        ];
        use sha2::{Digest, Sha256};
        let digest: [u8; 32] = Sha256::digest(serde_json::to_vec(&settings)?).into();
        let world = Arc::new(world);
        let binding = HostWorldScene {
            scene: Box::new(GeneratedScene {
                world: world.clone(),
                camera,
                scale: settings.scale,
                seed: u64::from(settings.seed),
            }),
            identity: SourceIdentity(digest),
            _admission: admission,
            root: None,
            generated: Some(world),
        };
        self.insert(binding, false)
    }
    pub fn insert_saved(
        &mut self,
        grant: SavedWorldGrant,
        factory: &mut dyn SavedWorldFactory,
    ) -> Result<WorldHandle> {
        if grant.epoch != self.epoch || self.cancelled || self.worlds.len() >= 8 {
            return Err(error("saved world grant unavailable"));
        }
        if !grant.history_authorized {
            return Err(AnimationError::PermissionDenied(
                "saved world requires explicit history authorization".into(),
            ));
        }
        let binding = factory.prepare(&grant, &self.resources)?;
        self.insert_prepared_saved(grant, binding)
    }
    /// A finite original native job has produced this actual scene graph. The
    /// actor rechecks activation and exact original child custody before insert.
    pub fn insert_prepared_saved(
        &mut self,
        grant: SavedWorldGrant,
        mut binding: HostWorldScene,
    ) -> Result<WorldHandle> {
        if grant.epoch != self.epoch || self.cancelled || self.worlds.len() >= 8 {
            return Err(error("prepared saved world grant unavailable"));
        }
        if !grant.history_authorized {
            return Err(AnimationError::PermissionDenied(
                "saved world requires explicit history authorization".into(),
            ));
        }
        if binding.identity != grant.identity
            || !binding
                .root
                .as_ref()
                .is_some_and(|root| Arc::ptr_eq(root, &grant.root))
        {
            return Err(error("saved scene provenance/root mismatch"));
        }
        binding.root = Some(grant.root);
        self.insert(binding, true)
    }
    pub fn register_assets(&mut self, assets: HostAssets) -> Result<AssetHandle> {
        if !self.quota.shares_root(&assets.quota) || self.assets.len() >= 64 {
            return Err(error("native assets quota/count mismatch"));
        }
        let id = self.next()?;
        self.assets.insert(id, assets);
        Ok(AssetHandle {
            instance: self.instance,
            id,
            epoch: self.epoch,
        })
    }
    fn asset(&self, handle: AssetHandle) -> Result<&HostAssets> {
        if self.cancelled || handle.instance != self.instance || handle.epoch != self.epoch {
            return Err(error("stale native asset handle"));
        }
        self.assets
            .get(&handle.id)
            .ok_or_else(|| error("native asset handle missing"))
    }
    pub fn describe_model(&self, handle: AssetHandle) -> Result<ModelDescriptor> {
        let asset = self.asset(handle)?;
        Ok(ModelDescriptor {
            quad_count: asset.model.quads.len(),
            image_dimensions: asset.texture.image().dimensions(),
        })
    }
    pub fn sample_texture(
        &self,
        handle: AssetHandle,
        uv: [f32; 2],
        time: Duration,
    ) -> Result<LinearRgba> {
        if uv
            .iter()
            .any(|value| !value.is_finite() || !(0. ..=1.).contains(value))
        {
            return Err(error("native texture UV outside bounds"));
        }
        self.asset(handle)?
            .texture
            .sample_color(uv, time)
            .ok_or_else(|| error("native texture is not color encoded"))
    }
    pub fn world_identity(&self, handle: WorldHandle) -> Result<SourceIdentity> {
        self.check(handle)?;
        let binding = &self
            .worlds
            .get(&handle.id)
            .ok_or_else(|| error("native world missing"))?
            .binding;
        let scene = binding
            .lock()
            .map_err(|_| error("native source custody poisoned"))?;
        Ok(scene.identity)
    }
    /// Project the registered source under its original storage account. The
    /// scene and query share generated occupancy; no terrain is regenerated.
    /// Encoded output owns a separate lease and survives closing the source.
    pub fn region(
        &mut self,
        handle: WorldHandle,
        spec: crate::world_region::RegionSpec,
        limits: crate::world_region::RegionLimits,
        encoding_work: usize,
        request_stop: &StopToken,
    ) -> Result<crate::world_region_encoding::EncodedRegion<StorageAdmission>> {
        self.check(handle)?;
        if request_stop.is_stopped() {
            return Err(error("native region request stopped"));
        }
        let entry = self
            .worlds
            .get(&handle.id)
            .ok_or_else(|| error("native world missing"))?;
        let mut binding = entry
            .binding
            .lock()
            .map_err(|_| error("native source custody poisoned"))?;
        let _identity_storage = self
            .quota
            .reserve_external_storage(64)
            .map_err(|reason| AnimationError::Budget(format!("region identity: {reason:?}")))?;
        let identity = binding.identity.hex();
        let encode = |projection, source_stop: Option<&StopToken>| {
            crate::world_region_encoding::encode_region(
                projection,
                &identity,
                spec.max_bytes,
                encoding_work,
                || request_stop.is_stopped() || source_stop.is_some_and(StopToken::is_stopped),
                |bytes| {
                    self.quota
                        .reserve_external_storage(bytes)
                        .map_err(|_| crate::world_region::RegionError::Admission)
                },
            )
            .map_err(|reason| AnimationError::Runtime(format!("native region: {reason:?}")))
        };
        if let Some(world) = &binding.generated {
            let projection = crate::world_region::collect_generated_region(
                world,
                &self.quota,
                spec,
                limits,
                || request_stop.is_stopped(),
            )
            .map_err(|reason| AnimationError::Runtime(format!("generated region: {reason:?}")))?;
            return encode(projection, None);
        }
        if !entry.saved || binding.root.is_none() {
            return Err(error("selected source has no original directory owner"));
        }
        if !matches!(
            binding.scene.readiness(),
            ilium_ambient::scene::SceneReadiness::Ready
        ) {
            return Err(error("selected source is not ready"));
        }
        let source = binding
            .scene
            .saved_world_source()
            .ok_or_else(|| error("selected source has no original decoded map"))?;
        if source.stop.is_stopped() || source.map.source().generation == 0 {
            return Err(error("selected source cancelled or invalid"));
        }
        let projection = crate::world_region_saved::collect_saved_region(
            &source.map.loaded().chunks,
            &self.quota,
            spec,
            limits,
            || request_stop.is_stopped() || source.stop.is_stopped(),
        )
        .map_err(|reason| AnimationError::Runtime(format!("selected region: {reason:?}")))?;
        encode(projection, Some(source.stop))
    }
    /// Preserve the selected-save-only public entry point.
    pub fn saved_region(
        &mut self,
        handle: WorldHandle,
        spec: crate::world_region::RegionSpec,
        limits: crate::world_region::RegionLimits,
        encoding_work: usize,
    ) -> Result<crate::world_region_encoding::EncodedRegion<StorageAdmission>> {
        self.check(handle)?;
        if !self.worlds.get(&handle.id).is_some_and(|entry| entry.saved) {
            return Err(error(
                "saved region requires an original selected saved source",
            ));
        }
        self.region(handle, spec, limits, encoding_work, &StopToken::default())
    }
    /// Resolve only the already owned source. Catalog completion is native
    /// state, never inferred from warning text or from a successful open ACK.
    pub fn world_readiness(
        &mut self,
        handle: WorldHandle,
    ) -> Result<ilium_ambient::scene::SceneReadiness> {
        self.check(handle)?;
        let entry = self
            .worlds
            .get(&handle.id)
            .ok_or_else(|| error("native world missing"))?;
        let mut source = entry
            .binding
            .lock()
            .map_err(|_| error("native source custody poisoned"))?;
        Ok(source.scene.readiness())
    }
    /// Idempotent native cleanup after an original cancelled operation or
    /// service retirement. Authenticate the sealed handle lineage even after
    /// issuance is closed; an earlier cancel may already have removed its slot.
    pub fn retire_world(&mut self, handle: WorldHandle) -> Result<()> {
        if handle.instance != self.instance || handle.epoch != self.epoch {
            return Err(error("foreign native world retirement handle"));
        }
        self.worlds.remove(&handle.id);
        Ok(())
    }
    pub fn close_world(&mut self, handle: WorldHandle) -> Result<()> {
        self.check(handle)?;
        self.worlds.remove(&handle.id);
        Ok(())
    }
    fn check(&self, handle: WorldHandle) -> Result<()> {
        if self.cancelled
            || handle.instance != self.instance
            || handle.epoch != self.epoch
            || !self.worlds.contains_key(&handle.id)
        {
            return Err(error("stale native world handle"));
        }
        Ok(())
    }
    pub fn render(
        &mut self,
        handle: WorldHandle,
        request: WorldRenderRequest,
    ) -> Result<Arc<NativeWorldFrame>> {
        self.check(handle)?;
        if request.width == 0 || request.height == 0 || request.width > 240 || request.height > 100
        {
            return Err(error("native world viewport outside bounds"));
        }
        let entry = self
            .worlds
            .get_mut(&handle.id)
            .ok_or_else(|| error("native world missing"))?;
        if request.pre_rendered && entry.saved {
            return Err(error("saved worlds require live source receipts"));
        }
        if !request.pre_rendered
            && entry
                .last_clock
                .is_some_and(|(time, wall)| request.time < time || request.wall < wall)
        {
            return Err(error("native world clock moved backwards"));
        }
        let slot = entry
            .slots
            .iter()
            .position(|lease| lease.strong_count() == 0)
            .ok_or_else(|| error("all native world receipt slots retained"))?;
        let mut binding = entry
            .binding
            .lock()
            .map_err(|_| error("native source custody poisoned"))?;
        match binding.scene.readiness() {
            ilium_ambient::scene::SceneReadiness::Preparing => {
                return Err(AnimationError::Preparing("world source"));
            }
            ilium_ambient::scene::SceneReadiness::Unavailable(reason) => {
                return Err(error(&reason));
            }
            ilium_ambient::scene::SceneReadiness::Ready => {}
        }
        let pixels = usize::from(request.width) * usize::from(request.height) * 8;
        let bytes = pixels * 64 + 16 * 1024 * 1024 + 4096;
        let admission = self
            .quota
            .reserve_external_storage(bytes)
            .map_err(|failure| {
                AnimationError::Budget(format!("native world frame admission: {failure:?}"))
            })?;
        let mut raster = Raster::default();
        raster.resize(
            usize::from(request.width) * 2,
            usize::from(request.height) * 4,
        );
        let mut colors = vec![[0; 3]; usize::from(request.width) * usize::from(request.height)];
        binding.scene.render(&mut Frame {
            raster: &mut raster,
            cell_colors: &mut colors,
            width: request.width,
            height: request.height,
            time: request.time,
            wall: request.wall,
            now: request.now,
        });
        if !binding.scene.has_prepared_frame() {
            return Err(AnimationError::Preparing("world frame"));
        }
        if raster.dots.len() != pixels
            || raster.owner_ids.len() != pixels
            || colors.len() != usize::from(request.width) * usize::from(request.height)
            || raster
                .dots
                .iter()
                .any(|dot| !dot.is_finite() || !(0. ..=1.).contains(dot))
        {
            return Err(error("native scene violated bounded frame shape"));
        }
        if binding.scene.receipt_bytes() > 16 * 1024 * 1024 {
            return Err(error("native source receipt exceeds admitted frame limit"));
        }
        entry.sequence = entry
            .sequence
            .checked_add(1)
            .ok_or_else(|| error("native receipt identity exhausted"))?;
        let receipt = FrameReceiptId::new(slot as u8, entry.sequence)
            .ok_or_else(|| error("native receipt slot invalid"))?;
        if !request.pre_rendered {
            binding.scene.seal_frame(receipt);
        }
        let frame = Arc::new(NativeWorldFrame {
            handle,
            receipt,
            identity: binding.identity,
            raster,
            colors,
            has_cell_colors: binding.scene.uses_cell_colors(),
            pre_rendered: request.pre_rendered,
            emission_fence: Arc::clone(&self.emission_fence),
            quota: self.quota.clone(),
            _source: Arc::clone(&binding._admission),
            _scene_retention: Arc::clone(&entry.binding),
            _root: binding.root.clone(),
            _admission: admission,
        });
        if !request.pre_rendered {
            entry.last_clock = Some((request.time, request.wall));
        }
        entry.slots[slot] = Arc::downgrade(&frame);
        Ok(frame)
    }
    /// Bind an issued frame to a process-unique token range before native draw
    /// preparation. The original frame/source and quota root remain retained.
    pub fn bind_draw_source(&self, frame: &Arc<NativeWorldFrame>) -> Result<Arc<WorldDotBinding>> {
        self.check(frame.handle)?;
        let entry = self
            .worlds
            .get(&frame.handle.id)
            .ok_or_else(|| error("world frame source missing"))?;
        if !Arc::ptr_eq(&entry.binding, &frame._scene_retention)
            || !self.quota.shares_root(&frame.quota)
            || (frame.pre_rendered && frame.raster.owner_ids.iter().any(|id| *id != 0))
        {
            return Err(error("world frame has no current source binding"));
        }
        let length = frame.raster.dots.len();
        if length == 0 {
            return Err(error("empty world dot range"));
        }
        let metadata = self.quota.reserve_external_storage(256).map_err(|reason| {
            AnimationError::Budget(format!("world dot binding admission: {reason:?}"))
        })?;
        let first = crate::surface::SourceToken::reserve_native_range(length)
            .map_err(|_| error("shared native source token space exhausted"))?;
        Ok(Arc::new(WorldDotBinding {
            frame: Arc::clone(frame),
            first,
            length,
            _metadata: metadata,
        }))
    }
    pub fn settle(
        &mut self,
        frame: &Arc<NativeWorldFrame>,
        evidence: TerminalPaintEvidence,
    ) -> Result<()> {
        if frame.handle.instance != self.instance || frame.handle.epoch != self.epoch {
            return Err(error("foreign native settlement owner"));
        }
        // Cancellation closes issuance; an earlier committed host emission
        // retains its source/receipt independently of this replaceable registry.
        frame.settle_committed(&evidence)
    }
    pub fn cancel(&mut self) {
        self.cancelled = true;
        *self
            .emission_fence
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = true;
        self.worlds.clear();
        self.assets.clear();
    }
}

impl Drop for WorldService {
    fn drop(&mut self) {
        self.cancel();
    }
}
