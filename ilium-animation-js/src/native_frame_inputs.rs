//! Native-selected cached frame inputs; no device, network or worker creation.
//! Provider methods borrow host-owned observations. Acquisition remains with
//! genuine native service owners; this interface alone grants no capability.
use crate::{
    engine::{ArraySpec, EngineLimits, TypedArrayKind},
    error::{AnimationError, Result},
    helper::HelperAuthority,
    native_audio::RetainedAudioSnapshot,
    plan::{AudioDemand, InputDemands},
    runtime::PackageInstance,
    surface::Shape,
};
use ilium_ambient::OccupancyMask;
use ilium_execution::{QuotaGroup, StorageAdmission};
use serde_json::{json, Map, Value};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
pub struct PointerObservation {
    pub normalized: [f32; 2],
    pub buttons: Option<u16>,
}
#[derive(Clone)]
pub struct OcclusionObservation {
    pub mask: Arc<OccupancyMask>,
    pub viewport_revision: u64,
    pub occupancy_revision: u64,
}
#[derive(Clone)]
pub struct ClockObservation {
    pub epoch_ms: Option<i64>,
    pub timezone: Option<String>,
}
#[derive(Clone, Copy)]
pub struct LocationObservation {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_m: Option<f64>,
    pub accuracy_m: Option<f64>,
}
/// Cached reads only: do not start captures, perform IO or synchronously wait
/// for an execution-bank job. The actual native owners enforce selected grants.
pub trait CachedInputProvider {
    fn pointer(&mut self) -> Result<Option<PointerObservation>>;
    fn occlusion(&mut self) -> Result<Option<OcclusionObservation>>;
    fn clock(&mut self, civil: bool) -> Result<Option<ClockObservation>>;
    fn location(&mut self) -> Result<Option<LocationObservation>>;
    fn audio(&mut self, demand: &AudioDemand) -> Result<Option<Arc<RetainedAudioSnapshot>>>;
}
struct Cache<T> {
    next: Option<Instant>,
    captured: Option<Instant>,
    revision: u64,
    value: Option<T>,
}
impl<T> Default for Cache<T> {
    fn default() -> Self {
        Self {
            next: None,
            captured: None,
            revision: 0,
            value: None,
        }
    }
}
fn invalid(message: &str) -> AnimationError {
    AnimationError::Runtime(format!("native frame inputs: {message}"))
}
impl<T> Cache<T> {
    fn refresh(
        &mut self,
        now: Instant,
        rate: f64,
        gather: impl FnOnce() -> Result<Option<T>>,
    ) -> Result<()> {
        if self.next.is_some_and(|next| now < next) {
            return Ok(());
        }
        if !rate.is_finite() || rate <= 0. || rate > 120. {
            return Err(invalid("input cadence"));
        }
        let interval = Duration::try_from_secs_f64(1. / rate)
            .map_err(|_| invalid("input cadence overflow"))?;
        let next = now
            .checked_add(interval)
            .ok_or_else(|| invalid("input deadline overflow"))?;
        let revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("input revision exhausted"))?;
        let value = gather()?;
        self.value = value;
        self.captured = Some(now);
        self.next = Some(next);
        self.revision = revision;
        Ok(())
    }
    fn metadata(&self, now: Instant) -> Value {
        let age = self.captured.map_or(0, |stamp| {
            now.saturating_duration_since(stamp)
                .as_millis()
                .min(u128::from(u64::MAX)) as u64
        });
        json!({"revision":self.revision,"available":true,"status":"ready","age_ms":age})
    }
}
/// A producer packet retains its own immutable original-root allocation.
/// Metadata has inputs and input_specs; planes are input_0, input_1, ... .
/// Caller merges them into its native frame seed, then uses the protected inert
/// seed/ACK boundary. This value is data, never a grant or presentation receipt.
pub struct FrameInputPacket {
    pub value: FrameInputValue,
}
/// Native frame seeds have input_N planes, not the asynchronous service bN
/// marker graph. Keep their original producer admissions with every alias.
#[derive(Clone)]
pub struct FrameInputValue(Arc<FrameInputStorage>);
struct FrameInputStorage {
    metadata: Value,
    arrays: Vec<ArraySpec>,
    planes: BTreeMap<String, Vec<u8>>,
    quota: QuotaGroup,
    _planes: Vec<StorageAdmission>,
    _metadata: StorageAdmission,
}
impl FrameInputValue {
    pub fn metadata(&self) -> &Value {
        &self.0.metadata
    }
    pub fn arrays(&self) -> &[ArraySpec] {
        &self.0.arrays
    }
    pub fn planes(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.0.planes
    }
    pub fn shares_root(&self, quota: &QuotaGroup) -> bool {
        self.0.quota.shares_root(quota)
    }
}
struct MetadataByteCount {
    bytes: usize,
    limit: usize,
}
impl std::io::Write for MetadataByteCount {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .filter(|total| *total <= self.limit)
            .ok_or_else(|| std::io::Error::other("input metadata byte limit"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
struct Builder {
    inputs: Map<String, Value>,
    specs: Vec<Value>,
    arrays: Vec<ArraySpec>,
    planes: BTreeMap<String, Vec<u8>>,
    quota: QuotaGroup,
    limits: EngineLimits,
    scratch: Vec<StorageAdmission>,
    _metadata: StorageAdmission,
}
impl Builder {
    fn new(quota: QuotaGroup, limits: EngineLimits) -> Result<Self> {
        let metadata = quota
            .reserve_external_storage(128 * 1024)
            .map_err(|e| AnimationError::Budget(format!("input packet metadata: {e:?}")))?;
        Ok(Self {
            inputs: Map::new(),
            specs: Vec::new(),
            arrays: Vec::new(),
            planes: BTreeMap::new(),
            quota,
            limits,
            scratch: Vec::new(),
            _metadata: metadata,
        })
    }
    fn plane(
        &mut self,
        path: &str,
        kind: TypedArrayKind,
        elements: usize,
        fill: impl FnOnce(&mut Vec<u8>) -> Result<()>,
    ) -> Result<()> {
        if self.arrays.len() >= 32 {
            return Err(invalid("input plane count"));
        }
        let width = match kind {
            TypedArrayKind::U8 => 1,
            TypedArrayKind::F32 => 4,
            _ => return Err(invalid("input plane kind")),
        };
        let size = elements
            .checked_mul(width)
            .ok_or_else(|| invalid("input plane size"))?;
        let total = self
            .planes
            .values()
            .try_fold(size, |n, p| n.checked_add(p.len()))
            .ok_or_else(|| invalid("input aggregate size"))?;
        if total > self.limits.frame_bytes {
            return Err(AnimationError::Budget("input plane bytes".into()));
        }
        let storage = self
            .quota
            .reserve_external_storage(size.saturating_add(64))
            .map_err(|e| AnimationError::Budget(format!("input plane scratch: {e:?}")))?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(size)
            .map_err(|_| AnimationError::Budget("input plane allocation".into()))?;
        fill(&mut bytes)?;
        if bytes.len() != size {
            return Err(invalid("input plane producer size"));
        }
        let name = format!("input_{}", self.arrays.len());
        self.specs.push(json!({"path":path,"kind":match kind {TypedArrayKind::U8=>"u8",_=>"f32"},"elements":elements}));
        self.arrays.push(ArraySpec {
            name: name.clone(),
            kind,
            elements,
        });
        self.planes.insert(name, bytes);
        self.scratch.push(storage);
        Ok(())
    }
    fn floats(&mut self, path: &str, data: &[f32], maximum: usize) -> Result<()> {
        if data.len() > maximum || data.iter().any(|v| !v.is_finite()) {
            return Err(invalid("audio product shape or nonfinite sample"));
        }
        self.plane(path, TypedArrayKind::F32, data.len(), |bytes| {
            for value in data {
                bytes.extend_from_slice(&value.to_ne_bytes());
            }
            Ok(())
        })
    }
    fn finish(self) -> Result<FrameInputPacket> {
        let metadata = json!({"inputs":self.inputs,"input_specs":self.specs});
        let mut count = MetadataByteCount {
            bytes: 0,
            limit: self.limits.json_bytes.min(128 * 1024),
        };
        // Count encoding under existing metadata admission without allocating
        // another serialized tree or weakening a narrowed instance limit.
        serde_json::to_writer(&mut count, &metadata)
            .map_err(|_| AnimationError::Budget("input metadata byte limit".into()))?;
        let value = FrameInputValue(Arc::new(FrameInputStorage {
            metadata,
            arrays: self.arrays,
            planes: self.planes,
            quota: self.quota,
            _planes: self.scratch,
            _metadata: self._metadata,
        }));
        Ok(FrameInputPacket { value }) // Original producer guards follow the final immutable packet alias.
    }
}
/// Bound once to the SAME accepted instance/epoch and actual pruned plan.
/// One native actor owns refresh; no timers, subscriptions or thread here.
pub struct NativeFrameInputs {
    wanted: InputDemands,
    authority: HelperAuthority,
    quota: QuotaGroup,
    limits: EngineLimits,
    pointer: Cache<PointerObservation>,
    occlusion: Cache<OcclusionObservation>,
    clock: Cache<ClockObservation>,
    location: Cache<LocationObservation>,
    audio: Cache<Arc<RetainedAudioSnapshot>>,
    _metadata: StorageAdmission,
}
impl NativeFrameInputs {
    pub fn new(instance: &mut PackageInstance, quota: QuotaGroup) -> Result<Self> {
        if !instance.shares_root(&quota) {
            return Err(invalid("foreign input root"));
        }
        let storage = quota
            .reserve_external_storage(128 * 1024)
            .map_err(|e| AnimationError::Budget(format!("input owner: {e:?}")))?;
        let authority = instance
            .frame_authority()
            .ok_or_else(|| invalid("input activation unavailable"))?;
        let wanted = instance.plan().inputs.clone();
        let limits = instance.engine_limits().clone();
        instance.check_live_input_authority(&authority)?;
        Ok(Self {
            wanted,
            authority,
            quota,
            limits,
            pointer: Cache::default(),
            occlusion: Cache::default(),
            clock: Cache::default(),
            location: Cache::default(),
            audio: Cache::default(),
            _metadata: storage,
        })
    }
    pub fn demands(&self) -> &InputDemands {
        &self.wanted
    }
    pub fn prepare(
        &mut self,
        instance: &mut PackageInstance,
        shape: Shape,
        viewport_revision: u64,
        now: Instant,
        provider: &mut impl CachedInputProvider,
    ) -> Result<FrameInputPacket> {
        // Fresh native activation before touching cached protected data. Native
        // acquisition owners and final protected seed both independently recheck.
        instance.check_live_input_authority(&self.authority)?;
        if viewport_revision > 9_007_199_254_740_991 {
            return Err(invalid("viewport revision JS precision"));
        }
        let layout = shape.layout().map_err(|e| invalid(&e.to_string()))?;
        if let Some(d) = &self.wanted.pointer {
            self.pointer.refresh(now, d.max_hz, || provider.pointer())?;
        }
        if let Some(d) = &self.wanted.occlusion {
            self.occlusion
                .refresh(now, d.max_hz, || provider.occlusion())?;
        }
        // Monotonic-only clock demand uses context.time; it does not acquire
        // civil host data or construct a partial SDK ClockSnapshot.
        if let Some(d) = self.wanted.clock.as_ref().filter(|d| d.civil) {
            self.clock.refresh(now, d.max_hz, || provider.clock(true))?;
        }
        if let Some(d) = &self.wanted.location {
            self.location
                .refresh(now, d.max_hz, || provider.location())?;
        }
        if let Some(d) = &self.wanted.audio {
            let quota = &self.quota;
            self.audio.refresh(now, d.max_hz, || {
                let snapshot = provider.audio(d)?;
                if snapshot
                    .as_ref()
                    .is_some_and(|value| !value.shares_root(quota))
                {
                    return Err(invalid("foreign retained audio root"));
                }
                Ok(snapshot)
            })?;
        }
        let mut out = Builder::new(self.quota.clone(), self.limits.clone())?;
        if let Some(point) = self.pointer.value {
            if point
                .normalized
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
            {
                return Err(invalid("pointer coordinates"));
            }
            let mut value = self.pointer.metadata(now);
            value["x"] = json!(f64::from(point.normalized[0]) * f64::from(shape.cell_width) * 2.);
            value["y"] = json!(f64::from(point.normalized[1]) * f64::from(shape.cell_height) * 4.);
            value["inside"] = json!(true);
            if let Some(buttons) = point.buttons {
                value["buttons"] = json!(buttons);
            }
            out.inputs.insert("pointer".into(), value);
        }
        if let (Some(d), Some(observation)) = (&self.wanted.occlusion, &self.occlusion.value) {
            let mask = &observation.mask;
            // A cached mask from previous geometry is withheld until its actual
            // native owner refreshes. It never gets relabelled as current space.
            if observation.viewport_revision == viewport_revision
                && u32::from(mask.width()) == shape.cell_width
                && u32::from(mask.height()) == shape.cell_height
            {
                let mut value = self.occlusion.metadata(now);
                value["cell_width"] = json!(shape.cell_width);
                value["cell_height"] = json!(shape.cell_height);
                value["pixel_width"] = json!(shape.cell_width * 2);
                value["pixel_height"] = json!(shape.cell_height * 4);
                value["viewport_revision"] = json!(observation.viewport_revision);
                value["revision"] = json!(observation.occupancy_revision);
                value["frame_space_id"] = json!(format!(
                    "{}:{}:{}x{}",
                    self.authority.instance_id,
                    self.authority.plan_generation,
                    shape.cell_width,
                    shape.cell_height
                ));
                if d.cells {
                    out.plane(
                        "occlusion.cells",
                        TypedArrayKind::U8,
                        layout.cells,
                        |bytes| {
                            for y in 0..mask.height() {
                                for x in 0..mask.width() {
                                    bytes.push(u8::from(
                                        mask.is_occupied(i32::from(x), i32::from(y)),
                                    ));
                                }
                            }
                            Ok(())
                        },
                    )?;
                }
                if d.pixels {
                    out.plane(
                        "occlusion.pixels",
                        TypedArrayKind::U8,
                        layout.dots,
                        |bytes| {
                            for y in 0..usize::from(mask.height()) * 4 {
                                for x in 0..usize::from(mask.width()) * 2 {
                                    bytes.push(u8::from(
                                        mask.is_occupied((x / 2) as i32, (y / 4) as i32),
                                    ));
                                }
                            }
                            Ok(())
                        },
                    )?;
                }
                out.inputs.insert("occlusion".into(), value);
            }
        }
        if let Some(clock) = &self.clock.value {
            let civil = self.wanted.clock.as_ref().is_some_and(|d| d.civil);
            let mut value = self.clock.metadata(now);
            if civil {
                let epoch = clock
                    .epoch_ms
                    .ok_or_else(|| invalid("civil clock epoch unavailable"))?;
                if epoch.unsigned_abs() > 9_007_199_254_740_991 {
                    return Err(invalid("civil clock JS precision"));
                }
                let timezone = clock
                    .timezone
                    .as_deref()
                    .ok_or_else(|| invalid("civil timezone unavailable"))?;
                if timezone.len() > 128 || timezone.chars().any(char::is_control) {
                    return Err(invalid("timezone metadata"));
                }
                value["epoch_ms"] = json!(epoch);
                value["timezone"] = json!(timezone);
            }
            out.inputs.insert("clock".into(), value);
        }
        if let Some(location) = self.location.value {
            if !location.latitude.is_finite()
                || !(-90. ..=90.).contains(&location.latitude)
                || !location.longitude.is_finite()
                || !(-180. ..=180.).contains(&location.longitude)
                || location.altitude_m.is_some_and(|v| !v.is_finite())
                || location
                    .accuracy_m
                    .is_some_and(|v| !v.is_finite() || v < 0.)
            {
                return Err(invalid("native observer location"));
            }
            let mut value = self.location.metadata(now);
            value["latitude"] = json!(location.latitude);
            value["longitude"] = json!(location.longitude);
            if let Some(v) = location.altitude_m {
                value["altitude_m"] = json!(v);
            }
            if let Some(v) = location.accuracy_m {
                value["accuracy_m"] = json!(v);
            }
            out.inputs.insert("location".into(), value);
        }
        if let (Some(d), Some(retained)) = (&self.wanted.audio, &self.audio.value) {
            let source = retained.view();
            let mut value = self.audio.metadata(now);
            value["captured_at_ms"] = json!(source.captured_at_ms);
            for product in &d.products {
                match product.as_str() {
                    "level" => {
                        if let Some(v) = source.level {
                            if !v.is_finite() {
                                return Err(invalid("audio level"));
                            }
                            value["level"] = json!(v);
                        }
                        if let Some(v) = source.rms {
                            if !v.is_finite() {
                                return Err(invalid("audio rms"));
                            }
                            value["rms"] = json!(v);
                        }
                    }
                    "waveform" => {
                        if let Some(data) = &source.waveform {
                            out.floats("audio.waveform", data, d.waveform_samples.unwrap_or(256))?;
                        }
                    }
                    "envelope" => {
                        if let Some(data) = &source.envelope {
                            out.floats("audio.envelope", data, 1024)?;
                        }
                    }
                    "bands" => {
                        if let Some(data) = &source.bands {
                            out.floats("audio.bands", data, d.band_count.unwrap_or(32))?;
                            if let Some(count) = source.band_count {
                                value["band_count"] = json!(count);
                            }
                        }
                    }
                    "history" => {
                        if let Some(data) = &source.history {
                            let maximum = d
                                .band_count
                                .unwrap_or(32)
                                .checked_mul(d.history_frames.unwrap_or(32))
                                .ok_or_else(|| invalid("audio history shape"))?;
                            out.floats("audio.history", data, maximum)?;
                            if let Some(count) = source.band_count {
                                value["band_count"] = json!(count);
                            }
                            if let Some(count) = source.history_frames {
                                value["history_frames"] = json!(count);
                            }
                        }
                    }
                    _ => return Err(invalid("unknown accepted audio product")),
                }
            }
            out.inputs.insert("audio".into(), value);
        }
        out.finish()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cadence_does_not_gather_early_and_failed_gather_can_retry() {
        let now = Instant::now();
        let mut cache = Cache::<u8>::default();
        let mut calls = 0;
        cache
            .refresh(now, 10., || {
                calls += 1;
                Ok(Some(7))
            })
            .unwrap();
        cache
            .refresh(now + Duration::from_millis(99), 10., || {
                calls += 1;
                Ok(Some(8))
            })
            .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(cache.value, Some(7));
        assert!(cache
            .refresh(now + Duration::from_millis(100), 10., || Err(invalid(
                "cached provider failure"
            )))
            .is_err());
        assert_eq!(cache.value, Some(7));
        assert_eq!(cache.revision, 1);
        cache
            .refresh(now + Duration::from_millis(100), 10., || {
                calls += 1;
                Ok(None)
            })
            .unwrap();
        assert_eq!(calls, 2);
        assert!(cache.value.is_none());
    }
    #[test]
    fn pathological_rate_refuses_before_gather() {
        let mut cache = Cache::<u8>::default();
        for rate in [0., f64::NAN, 121., f64::MIN_POSITIVE] {
            assert!(cache
                .refresh(Instant::now(), rate, || panic!(
                    "no acquisition before valid cadence"
                ))
                .is_err());
        }
    }

    fn quota(bytes: usize) -> QuotaGroup {
        QuotaGroup::new(ilium_execution::QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: 0,
            worker_bytes: bytes,
        })
    }
    #[test]
    fn failed_plane_admission_does_not_enter_native_fill() {
        let root = quota(128 * 1024);
        let mut builder = Builder::new(root.clone(), EngineLimits::default()).unwrap();
        assert!(builder
            .plane("audio.waveform", TypedArrayKind::F32, 1, |_| panic!(
                "native fill cannot run before original admission"
            ))
            .is_err());
        assert!(builder.arrays.is_empty());
        assert!(builder.planes.is_empty());
        drop(builder);
        assert_eq!(root.snapshot().worker_bytes, 0);
    }
    #[test]
    fn packet_binary_copy_keeps_original_debit_through_last_alias() {
        let root = quota(4 * 1024 * 1024);
        let mut builder = Builder::new(root.clone(), EngineLimits::default()).unwrap();
        builder.floats("audio.waveform", &[0.25, -0.5], 2).unwrap();
        let packet = builder.finish().unwrap();
        assert!(packet.value.shares_root(&root));
        assert_eq!(packet.value.arrays().len(), 1);
        assert_eq!(
            packet.value.planes()["input_0"],
            [0.25f32.to_ne_bytes(), (-0.5f32).to_ne_bytes()].concat()
        );
        let alias = packet.value.clone();
        let retained = root.snapshot().worker_bytes;
        assert!(retained > 0);
        drop(packet);
        assert_eq!(root.snapshot().worker_bytes, retained);
        drop(alias);
        assert_eq!(root.snapshot().worker_bytes, 0);
    }
    #[test]
    fn frame_packet_keeps_two_native_seed_planes_and_narrow_metadata_bound() {
        let root = quota(4 * 1024 * 1024);
        let mut builder = Builder::new(root.clone(), EngineLimits::default()).unwrap();
        builder.floats("audio.waveform", &[0.5], 1).unwrap();
        builder
            .plane("occlusion.cells", TypedArrayKind::U8, 2, |bytes| {
                bytes.extend_from_slice(&[0, 1]);
                Ok(())
            })
            .unwrap();
        let packet = builder.finish().unwrap();
        assert_eq!(packet.value.arrays()[0].name, "input_0");
        assert_eq!(packet.value.arrays()[1].name, "input_1");
        assert_eq!(packet.value.planes()["input_1"], [0, 1]);
        assert_eq!(
            packet.value.metadata()["input_specs"][1]["path"],
            "occlusion.cells"
        );
        assert!(!packet.value.shares_root(&quota(4 * 1024 * 1024)));
        drop(packet);
        assert_eq!(root.snapshot().worker_bytes, 0);
        let limits = EngineLimits {
            json_bytes: 1,
            ..EngineLimits::default()
        };
        let builder = Builder::new(root.clone(), limits).unwrap();
        assert!(
            matches!(builder.finish(), Err(AnimationError::Budget(message)) if message == "input metadata byte limit")
        );
        assert_eq!(root.snapshot().worker_bytes, 0);
    }
    #[test]
    fn invalid_samples_and_oversized_planes_leave_inventory_empty() {
        let root = quota(4 * 1024 * 1024);
        let limits = EngineLimits {
            frame_bytes: 8,
            ..EngineLimits::default()
        };
        let mut builder = Builder::new(root, limits).unwrap();
        assert!(builder.floats("audio.waveform", &[f32::NAN], 1).is_err());
        assert!(builder.floats("audio.waveform", &[0., 1., 2.], 2).is_err());
        assert!(builder
            .plane("occlusion.cells", TypedArrayKind::U8, 9, |_| panic!(
                "oversized plane cannot enter fill"
            ))
            .is_err());
        assert!(builder.arrays.is_empty());
        assert!(builder.planes.is_empty());
    }
}
