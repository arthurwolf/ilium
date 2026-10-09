//! Worker-only native source broker. No adapter fetches through ambient workers.
//! Caller owns execution admission, cancellation and broker authorization.
pub(crate) mod astronomy;
mod documents;
pub(crate) mod geography;
mod satellite;
mod series;
mod terrain;
pub mod types;
use crate::{
    error::{AnimationError, Result},
    http::{HttpOptions, HttpResponse},
};
use ilium_execution::{QuotaGroup, StorageAdmission};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
pub use types::*;

/// Implemented by the authority-bound native host, never by JavaScript.
/// Every request/stream hop and cached snapshot delivery must recheck grants.
pub(crate) trait BrokerSourceClient {
    fn authorize_demand(&self, demand: &SourceDemand) -> Result<()>;
    fn authorize_operation(&self, request: &SourceRequest) -> Result<()>;
    /// Process-wide cadence admission, shared across packages and handles.
    /// False defers this refresh without allocating or making a request.
    fn admit_refresh(
        &mut self,
        key: &str,
        monotonic_ms: u64,
        minimum_interval_ms: u64,
    ) -> Result<bool>;
    fn request(&mut self, options: &HttpOptions, stop: &AtomicBool) -> Result<HttpResponse>;
    fn stream_lines(
        &mut self,
        options: &HttpOptions,
        stop: &AtomicBool,
        max_line_bytes: usize,
        max_records: usize,
        callback: &mut dyn FnMut(&[u8]) -> Result<bool>,
    ) -> Result<HttpResponse>;
    /// Admit once and retain until the last process consumer, before lazy
    /// native OnceLock catalogues allocate their persistent baseline.
    fn admit_process_baseline(&mut self, key: &'static str, bytes: usize) -> Result<()>;
    /// Return a real admitted host image handle, not fabricated metadata.
    fn decode_image(&mut self, bytes: &[u8], max_pixels: usize, stop: &AtomicBool)
        -> Result<Value>;
    /// Return a real broker-admitted native heightfield. The Arc and its cache
    /// entry retain scratch/result charges while any source consumer holds it.
    fn terrain(
        &mut self,
        body: &str,
        seed: u32,
        stop: &AtomicBool,
    ) -> Result<Arc<ilium_ambient::animation_services::topography::Heightfield>>;
}

/// Native source bytes retain original storage admission through their last owner.
#[derive(Debug)]
pub struct SourceHttpResponse {
    status: u16,
    body: Vec<u8>,
    _admission: StorageAdmission,
}
impl SourceHttpResponse {
    #[cfg(all(
        feature = "v8-runtime",
        feature = "native-host",
        feature = "native-network"
    ))]
    pub(crate) fn copy_for_replay(&self, quota: &QuotaGroup, max_bytes: usize) -> Result<Self> {
        if self.body.len() > max_bytes || max_bytes == 0 || max_bytes > 32_000_000 {
            return types::fail("source replay wire limit");
        }
        let admission = source_storage(
            quota,
            self.body
                .len()
                .checked_add(64)
                .ok_or_else(|| AnimationError::Budget("source replay overflow".into()))?,
        )?;
        let mut body = Vec::new();
        body.try_reserve_exact(self.body.len())
            .map_err(|_| AnimationError::Budget("source replay allocation".into()))?;
        body.extend_from_slice(&self.body);
        Ok(Self {
            status: self.status,
            body,
            _admission: admission,
        })
    }
    pub fn read(
        status: u16,
        reader: &mut (impl std::io::Read + ?Sized),
        quota: &QuotaGroup,
        max_bytes: usize,
        stop: &AtomicBool,
    ) -> Result<Self> {
        if max_bytes == 0 || max_bytes > 32_000_000 {
            return Err(AnimationError::Runtime("source wire limit".into()));
        }
        let capacity = max_bytes
            .checked_add(1)
            .ok_or_else(|| AnimationError::Budget("source wire capacity".into()))?;
        let admission = quota
            .reserve_external_storage(capacity + 64)
            .map_err(|reason| {
                AnimationError::Budget(format!("source storage admission: {reason:?}"))
            })?;
        let mut body = Vec::new();
        body.try_reserve_exact(capacity)
            .map_err(|_| AnimationError::Budget("source wire allocation".into()))?;
        let mut block = [0u8; 8192];
        loop {
            if stop.load(Ordering::Acquire) {
                return Err(AnimationError::Runtime("cancelled".into()));
            }
            let available = (capacity - body.len()).min(block.len());
            if available == 0 {
                return Err(AnimationError::Runtime(
                    "provider response exceeded budget".into(),
                ));
            }
            let count = reader
                .read(&mut block[..available])
                .map_err(|error| AnimationError::Runtime(error.to_string()))?;
            if stop.load(Ordering::Acquire) {
                return Err(AnimationError::Runtime("cancelled".into()));
            }
            if count == 0 {
                break;
            }
            body.extend_from_slice(&block[..count]);
            if body.len() > max_bytes {
                return Err(AnimationError::Runtime(
                    "provider response exceeded budget".into(),
                ));
            }
        }
        Ok(Self {
            status,
            body,
            _admission: admission,
        })
    }
    // Readers borrow bytes while this owner retains their original quota.
    pub fn status(&self) -> u16 {
        self.status
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.body
    }
}

/// Fixed metadata backed by the actual admitted native image allocation.
/// An opaque script token still requires host binding/authorization on delivery.
#[derive(Debug, Clone)]
pub struct NativeSourceImage {
    handle: crate::native_media::ImageHandle,
    pixels: Arc<crate::native_media::Admitted<crate::native_media::ImagePixels>>,
}
impl NativeSourceImage {
    pub fn from_native(
        media: &crate::native_media::NativeMedia,
        handle: crate::native_media::ImageHandle,
    ) -> Result<Self> {
        Ok(Self {
            handle,
            pixels: media.snapshot(handle)?,
        })
    }
    pub fn native_handle(&self) -> crate::native_media::ImageHandle {
        self.handle
    }
    /// Original allocation identity only; source/demand/current authorization
    /// stays with the native owner, never a copied handle or matching quota.
    pub fn shares_root(&self, quota: &QuotaGroup) -> bool {
        self.pixels.shares_root(quota)
    }
    /// The native root may retain the actual admitted allocation while binding
    /// its own opaque image token. This is not an ID lookup or a new grant.
    pub fn admitted_pixels(
        &self,
    ) -> &Arc<crate::native_media::Admitted<crate::native_media::ImagePixels>> {
        &self.pixels
    }
    /// Stable allocation identity while this admitted image remains retained.
    /// Used only to deduplicate native aliases; never exposed to packages.
    pub(crate) fn allocation_key(&self) -> usize {
        Arc::as_ptr(&self.pixels) as usize
    }
    /// Whether two wrappers retain the exact same original admitted pixels.
    pub(crate) fn same_allocation(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.pixels, &other.pixels)
    }
    fn metadata(&self, slot: usize) -> Value {
        let pixels = self.pixels.view();
        serde_json::json!({"native_image_slot":slot,"width":pixels.width,"height":pixels.height})
    }
}
/// Embedded native terrain keeps its original admission through every Arc.
/// Only fixed native body IDs/seeds enter this loader; no network acquisition.
#[derive(Debug)]
pub struct NativeSourceHeightfield {
    field: Arc<ilium_ambient::animation_services::topography::Heightfield>,
    _admission: StorageAdmission,
}
impl NativeSourceHeightfield {
    pub fn load_native(
        world: ilium_ambient::animation_services::topography::WorldId,
        seed: u32,
        quota: &QuotaGroup,
        stop: &AtomicBool,
    ) -> Result<Arc<Self>> {
        cancelled(stop)?;
        let admission = source_storage(quota, 8 * 1024 * 1024 + 8192)?;
        // Fixed embedded PNG grids <=2048x1024; generated grids1024x512.
        // Scratch covers decode + luma conversion + simultaneous elevation Vec.
        let _scratch = source_storage(quota, 32 * 1024 * 1024)?;
        let field = ilium_ambient::animation_services::topography::load_world(world, seed, stop)
            .ok_or_else(|| AnimationError::Runtime("terrain cancelled".into()))?
            .map_err(AnimationError::Runtime)?;
        if field.meters.capacity() > 2048 * 1024 || field.name.capacity() > 4096 {
            return types::fail("native terrain retained limit");
        }
        cancelled(stop)?;
        Ok(Arc::new(Self {
            field: Arc::new(field),
            _admission: admission,
        }))
    }
    pub fn view(&self) -> &ilium_ambient::animation_services::topography::Heightfield {
        &self.field
    }
}
/// Implemented by the native host. No generic Value can enter the source
/// decoder. Pass the supplied ORIGINAL quota to response/media admission.
/// DNS/grant checks and cancellation stay owned by the host transport adapter.
pub trait NativeSourceClient {
    /// Only an actor replay client may report a typed pending native transport.
    /// Ordinary provider errors retain the existing failure/backoff behavior.
    fn suspended(&self) -> Result<bool> {
        Ok(false)
    }
    fn authorize_demand(&self, demand: &SourceDemand) -> Result<()>;
    fn authorize_operation(&self, request: &SourceRequest) -> Result<()>;
    fn admit_refresh(
        &mut self,
        key: &str,
        monotonic_ms: u64,
        minimum_interval_ms: u64,
    ) -> Result<bool>;
    fn request_bytes(
        &mut self,
        options: &HttpOptions,
        quota: &QuotaGroup,
        stop: &AtomicBool,
    ) -> Result<SourceHttpResponse>;
    /// Deliver at most max_records bounded lines; status only, no heap metadata.
    fn stream_lines(
        &mut self,
        options: &HttpOptions,
        stop: &AtomicBool,
        max_line_bytes: usize,
        max_records: usize,
        callback: &mut dyn FnMut(&[u8]) -> Result<bool>,
    ) -> Result<u16>;
    fn admit_process_baseline(&mut self, key: &'static str, bytes: usize) -> Result<()>;
    fn decode_image(
        &mut self,
        bytes: &[u8],
        max_pixels: usize,
        quota: &QuotaGroup,
        stop: &AtomicBool,
    ) -> Result<NativeSourceImage>;
    fn terrain(
        &mut self,
        body: &str,
        seed: u32,
        quota: &QuotaGroup,
        stop: &AtomicBool,
    ) -> Result<Arc<NativeSourceHeightfield>>;
}
struct ProviderAdapter<C> {
    native: C,
    quota: QuotaGroup,
    images: Vec<NativeSourceImage>,
    terrain: Option<Arc<NativeSourceHeightfield>>,
}
impl<C: NativeSourceClient> BrokerSourceClient for ProviderAdapter<C> {
    fn authorize_demand(&self, demand: &SourceDemand) -> Result<()> {
        self.native.authorize_demand(demand)
    }
    fn authorize_operation(&self, request: &SourceRequest) -> Result<()> {
        self.native.authorize_operation(request)
    }
    fn admit_refresh(&mut self, key: &str, now: u64, interval: u64) -> Result<bool> {
        self.native.admit_refresh(key, now, interval)
    }
    fn request(&mut self, options: &HttpOptions, stop: &AtomicBool) -> Result<HttpResponse> {
        let response = self.native.request_bytes(options, &self.quota, stop)?;
        if response.body.len() > options.max_bytes
            || response.body.capacity() > options.max_bytes + 1
        {
            return types::fail("native source wire response");
        }
        let body = if options.response == "bytes" {
            // Exact bounded expansion into the legacy provider representation;
            // weather's 384 MiB peak includes 8 MiB * size_of::<Value>().
            let mut values = Vec::new();
            values
                .try_reserve_exact(response.body.len())
                .map_err(|_| AnimationError::Budget("source byte expansion".into()))?;
            values.extend(response.body.iter().map(|byte| Value::from(*byte)));
            Value::Array(values)
        } else {
            let text = std::str::from_utf8(&response.body)
                .map_err(|_| AnimationError::Runtime("source response UTF-8".into()))?;
            if matches!(text.trim_start().as_bytes().first(), Some(b'{' | b'[')) {
                preflight_json(
                    &response.body,
                    options.url == ilium_ambient::live_data::openseafeed::ENDPOINT,
                )?;
            }
            if options
                .url
                .starts_with("https://overpass-api.de/api/interpreter")
            {
                preflight_osm(&response.body)?;
            }
            Value::String(text.to_owned())
        };
        Ok(HttpResponse {
            status: response.status,
            body,
            headers: BTreeMap::new(),
            final_url: String::new(),
        })
    }
    fn stream_lines(
        &mut self,
        options: &HttpOptions,
        stop: &AtomicBool,
        max_line_bytes: usize,
        max_records: usize,
        callback: &mut dyn FnMut(&[u8]) -> Result<bool>,
    ) -> Result<HttpResponse> {
        let mut records = 0usize;
        let mut total = 0usize;
        let status =
            self.native
                .stream_lines(options, stop, max_line_bytes, max_records, &mut |line| {
                    cancelled(stop)?;
                    records = records
                        .checked_add(1)
                        .ok_or_else(|| AnimationError::Budget("source stream records".into()))?;
                    total = total
                        .checked_add(line.len())
                        .ok_or_else(|| AnimationError::Budget("source stream bytes".into()))?;
                    if records > max_records
                        || line.len() > max_line_bytes
                        || total > options.max_bytes
                    {
                        return types::fail("source stream budget");
                    }
                    let data = line.strip_prefix(b"data:").unwrap_or(line);
                    if matches!(
                        data.iter()
                            .copied()
                            .find(|byte| !byte.is_ascii_whitespace()),
                        Some(b'{' | b'[')
                    ) {
                        preflight_json(data, false)?;
                    }
                    callback(line)
                })?;
        Ok(HttpResponse {
            status,
            headers: BTreeMap::new(),
            body: Value::Null,
            final_url: String::new(),
        })
    }
    fn admit_process_baseline(&mut self, key: &'static str, bytes: usize) -> Result<()> {
        self.native.admit_process_baseline(key, bytes)
    }
    fn decode_image(
        &mut self,
        bytes: &[u8],
        max_pixels: usize,
        stop: &AtomicBool,
    ) -> Result<Value> {
        if self.images.len() >= 216 {
            return types::fail("source image handle limit");
        }
        let image = self
            .native
            .decode_image(bytes, max_pixels, &self.quota, stop)?;
        let pixels = image.pixels.view();
        let count = (pixels.width as usize)
            .checked_mul(pixels.height as usize)
            .ok_or_else(|| AnimationError::Budget("source image dimensions".into()))?;
        if count == 0 || count > max_pixels || count.checked_mul(4) != Some(pixels.rgba.len()) {
            return types::fail("source decoded image dimensions");
        }
        let metadata = image.metadata(self.images.len());
        self.images.push(image);
        Ok(metadata)
    }
    fn terrain(
        &mut self,
        body: &str,
        seed: u32,
        stop: &AtomicBool,
    ) -> Result<Arc<ilium_ambient::animation_services::topography::Heightfield>> {
        let terrain = self.native.terrain(body, seed, &self.quota, stop)?;
        let field = Arc::clone(&terrain.field);
        self.terrain = Some(terrain);
        Ok(field)
    }
}

/// Allocation-free tree cardinality scan (serde's bounded string scratch is
/// covered by the held wire/parse peak). Reject before native Value allocation.
fn preflight_json(bytes: &[u8], fleet: bool) -> Result<()> {
    use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
    struct Count {
        nodes: usize,
        strings: usize,
        fleet: bool,
    }
    struct Seed<'a> {
        count: &'a mut Count,
        depth: usize,
    }
    impl<'de> DeserializeSeed<'de> for Seed<'_> {
        type Value = ();
        fn deserialize<D: serde::Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> std::result::Result<(), D::Error> {
            self.count.nodes += 1;
            if self.depth > 32
                || self.count.nodes
                    > if self.count.fleet {
                        4 * 1024 * 1024
                    } else {
                        16_384
                    }
            {
                return Err(serde::de::Error::custom("source JSON node/depth limit"));
            }
            let before = self.count.nodes;
            let depth = self.depth;
            let count = &mut *self.count;
            deserializer.deserialize_any(Seed { count, depth })?;
            if depth >= 2 && count.nodes - before > 16_384 {
                return Err(serde::de::Error::custom("source JSON subtree limit"));
            }
            Ok(())
        }
    }
    impl<'de> Visitor<'de> for Seed<'_> {
        type Value = ();
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("bounded source JSON")
        }
        fn visit_bool<E: serde::de::Error>(self, _: bool) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_i64<E: serde::de::Error>(self, _: i64) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_u64<E: serde::de::Error>(self, _: u64) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_f64<E: serde::de::Error>(self, _: f64) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_str<E: serde::de::Error>(self, value: &str) -> std::result::Result<(), E> {
            self.count.strings += value.len();
            if value.len() > 8192
                || self.count.strings
                    > if self.count.fleet {
                        32_000_000
                    } else {
                        512 * 1024
                    }
            {
                return Err(E::custom("source JSON string limit"));
            }
            Ok(())
        }
        fn visit_string<E: serde::de::Error>(self, value: String) -> std::result::Result<(), E> {
            self.visit_str(&value)
        }
        fn visit_seq<A: SeqAccess<'de>>(
            self,
            mut sequence: A,
        ) -> std::result::Result<(), A::Error> {
            while sequence
                .next_element_seed(Seed {
                    count: self.count,
                    depth: self.depth + 1,
                })?
                .is_some()
            {}
            Ok(())
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
            while map
                .next_key_seed(Seed {
                    count: self.count,
                    depth: self.depth + 1,
                })?
                .is_some()
            {
                map.next_value_seed(Seed {
                    count: self.count,
                    depth: self.depth + 1,
                })?;
            }
            Ok(())
        }
    }
    let mut count = Count {
        nodes: 0,
        strings: 0,
        fleet,
    };
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    Seed {
        count: &mut count,
        depth: 0,
    }
    .deserialize(&mut decoder)
    .map_err(|error| AnimationError::Budget(format!("source JSON preflight: {error}")))?;
    decoder.end()?;
    Ok(())
}
// Native OSM relation members may reuse the same way geometry many times.
// A raw JSON node cap alone does NOT bound this reference expansion. Count
// worst-case six layers, split-path duplication and point features BEFORE
// native geometry_paths/parse_map materialize any expanded paths.
fn preflight_osm(bytes: &[u8]) -> Result<()> {
    let value: Value = serde_json::from_slice(bytes)?; // ordinary16k-node preflight already passed
    let elements = value["elements"]
        .as_array()
        .ok_or_else(|| AnimationError::Runtime("OSM elements".into()))?;
    if elements.len() > 1024 {
        return Err(AnimationError::Budget(
            "OSM source element projection".into(),
        ));
    }
    let ways: BTreeMap<u64, usize> = elements
        .iter()
        .filter(|element| element["type"] == "way")
        .filter_map(|element| {
            Some((
                element["id"].as_u64()?,
                element["geometry"].as_array().map_or(0, Vec::len),
            ))
        })
        .collect();
    let mut expanded = elements.len() * 6;
    for element in elements {
        let points = element["geometry"].as_array().map_or(0, Vec::len);
        expanded = expanded
            .checked_add(points * 12)
            .ok_or_else(|| AnimationError::Budget("OSM point projection".into()))?;
        if let Some(members) = element["members"].as_array() {
            for member in members {
                let supplied = member["geometry"].as_array().map_or(0, Vec::len);
                let points = if supplied == 0 {
                    member["ref"]
                        .as_u64()
                        .and_then(|id| ways.get(&id).copied())
                        .unwrap_or(0)
                } else {
                    supplied
                };
                expanded = expanded
                    .checked_add(points * 12)
                    .ok_or_else(|| AnimationError::Budget("OSM relation projection".into()))?;
                if expanded > 8192 {
                    return Err(AnimationError::Budget("OSM reference expansion".into()));
                }
            }
        }
        if expanded > 8192 {
            return Err(AnimationError::Budget("OSM point projection".into()));
        }
    }
    Ok(())
}
fn operation_retained(request: &SourceRequest) -> Result<usize> {
    const MIB: usize = 1024 * 1024;
    let units = match request {
        SourceRequest::AstronomyCatalogue { max_stars, .. } => max_stars.checked_mul(16_384),
        SourceRequest::GeographyElevation { width, height, .. } => width
            .checked_mul(*height)
            .and_then(|pixels| pixels.checked_mul(128)),
        _ => Some(31 * MIB),
    }
    .ok_or_else(|| AnimationError::Budget("source operation result size".into()))?;
    if units > 256 * MIB {
        return Err(AnimationError::Budget(
            "source operation result envelope".into(),
        ));
    }
    units
        .checked_add(MIB)
        .ok_or_else(|| AnimationError::Budget("source operation result size".into()))
}
fn operation_peak(request: &SourceRequest) -> Result<usize> {
    const MIB: usize = 1024 * 1024;
    Ok(match request {
        SourceRequest::WikipediaArticle { .. } => ilium_wikipedia::ARTICLE_PARSE_PEAK_BYTES,
        SourceRequest::OsmTile { format, .. } if format == "raster" => 128 * MIB,
        SourceRequest::GeographyProject { .. } | SourceRequest::AstronomyObserve { .. } => MIB,
        _ => 96 * MIB,
    })
}

#[derive(Debug, Clone, Copy)]
pub struct SourceClock {
    pub monotonic_ms: u64,
    pub epoch_ms: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceHandle(pub u64);
struct Slot {
    demand: SourceDemand,
    interval_ms: u64,
    next_due_ms: u64,
    failures: u32,
    revision: u64,
    snapshot: Option<Arc<AdmittedSourceSnapshot>>,
    _admission: StorageAdmission,
}
pub struct SourceDispatcher<C: NativeSourceClient> {
    client: ProviderAdapter<C>,
    slots: BTreeMap<SourceHandle, Slot>,
    next_id: u64,
    quota: QuotaGroup,
    _metadata: StorageAdmission,
}
impl<C: NativeSourceClient> SourceDispatcher<C> {
    /// Supply the existing original-root quota; never create a private bank.
    pub fn new(client: C, quota: QuotaGroup) -> Result<Self> {
        // Sparse BTreeMap leaves may survive removal of the last slot. Keep
        // the full bounded tree/slot envelope charged through dispatcher drop.
        let metadata_bytes = std::mem::size_of::<Slot>()
            .checked_mul(32 * 24)
            .and_then(|bytes| bytes.checked_add(16384))
            .ok_or_else(|| AnimationError::Budget("source registry size".into()))?;
        let metadata = source_storage(&quota, metadata_bytes)?;
        Ok(Self {
            client: ProviderAdapter {
                native: client,
                quota: quota.clone(),
                images: Vec::new(),
                terrain: None,
            },
            slots: BTreeMap::new(),
            next_id: 1,
            quota,
            _metadata: metadata,
        })
    }
    pub fn open(&mut self, demand: SourceDemand, clock: SourceClock) -> Result<SourceHandle> {
        let interval_ms = validate_demand(&demand)?;
        self.client.authorize_demand(&demand)?;
        if self.slots.len() >= 32 {
            return types::fail("source handle budget exhausted");
        }
        // Hold the accepted demand/map node through close, including String
        // capacities. Caller still owns its pre-acceptance input allocation.
        let demand_admission = source_storage(&self.quota, demand_bytes(&demand)?)?;
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| AnimationError::Runtime("source handle space exhausted".into()))?;
        let handle = SourceHandle(id);
        self.slots.insert(
            handle,
            Slot {
                demand,
                interval_ms,
                next_due_ms: clock.monotonic_ms,
                failures: 0,
                revision: 0,
                snapshot: None,
                _admission: demand_admission,
            },
        );
        Ok(handle)
    }
    #[cfg(all(
        feature = "v8-runtime",
        feature = "native-host",
        feature = "native-network"
    ))]
    pub(crate) fn actor_client(&self) -> &C {
        &self.client.native
    }
    pub fn close(&mut self, handle: SourceHandle) -> Result<()> {
        self.slots
            .remove(&handle)
            .map(|_| ())
            .ok_or_else(|| AnimationError::Runtime("unknown source handle".into()))
    }
    pub fn close_all(&mut self) {
        self.slots.clear();
    }
    pub fn latest(&self, handle: SourceHandle) -> Result<Option<Arc<AdmittedSourceSnapshot>>> {
        let slot = self
            .slots
            .get(&handle)
            .ok_or_else(|| AnimationError::Runtime("unknown source handle".into()))?;
        self.client.authorize_demand(&slot.demand)?;
        Ok(slot.snapshot.clone())
    }
    /// Caller schedules this at next_due_ms in its owned actor. This method
    /// never starts threads, sleeps, or silently polls an inactive demand.
    pub fn poll(
        &mut self,
        handle: SourceHandle,
        clock: SourceClock,
        stop: &AtomicBool,
    ) -> Result<Option<Arc<AdmittedSourceSnapshot>>> {
        cancelled(stop)?;
        let slot = self
            .slots
            .get_mut(&handle)
            .ok_or_else(|| AnimationError::Runtime("unknown source handle".into()))?;
        self.client.authorize_demand(&slot.demand)?;
        if clock.monotonic_ms < slot.next_due_ms {
            return Ok(slot.snapshot.clone());
        }
        let previous_due_ms = slot.next_due_ms;
        slot.next_due_ms = clock.monotonic_ms.saturating_add(slot.interval_ms);
        let key = match &slot.demand {
            SourceDemand::Series(options) => match options.provider {
                SeriesProvider::Usgs => "usgs".into(),
                _ => format!("series:{:?}", options.provider),
            },
            SourceDemand::Earthquakes(_) => "usgs".into(),
            SourceDemand::Aircraft(_) => "opensky".into(),
            SourceDemand::Boats(options) => {
                match options.provider.unwrap_or(BoatProvider::Openseafeed) {
                    BoatProvider::Openseafeed => "openseafeed",
                    BoatProvider::Digitraffic => "digitraffic",
                }
                .into()
            }
            SourceDemand::Chess(_) => "lichess-tv".into(),
            SourceDemand::Weather(_) => "weather".into(),
        };
        if !self
            .client
            .admit_refresh(&key, clock.monotonic_ms, slot.interval_ms)?
        {
            return Ok(slot.snapshot.clone());
        }
        let revision = slot
            .revision
            .checked_add(1)
            .ok_or_else(|| AnimationError::Runtime("source revision exhausted".into()))?;
        let budget = source_budget(&slot.demand)?;
        // Reserve retained output BEFORE provider/JSON/codec work. This guard
        // also covers a new error snapshot if provider work fails. Existing
        // cached/escaped payloads keep their own separate original debits.
        let mut retained_admission = None;
        self.client.images.clear();
        self.client.terrain = None;
        let result = (|| -> Result<SourceSnapshot> {
            retained_admission = Some(source_storage(&self.quota, budget.retained_bytes)?);
            let _peak = source_storage(&self.quota, budget.peak_bytes)?;
            let mut snapshot = match &slot.demand {
                SourceDemand::Series(options) => {
                    series::fetch(&mut self.client, options, clock.epoch_ms, revision, stop)
                        .map(SourceSnapshot::Series)
                }
                SourceDemand::Earthquakes(options) => geography::earthquakes(
                    &mut self.client,
                    options,
                    clock.epoch_ms,
                    revision,
                    stop,
                )
                .map(SourceSnapshot::Geographic),
                SourceDemand::Aircraft(options) => {
                    geography::aircraft(&mut self.client, options, clock.epoch_ms, revision, stop)
                        .map(SourceSnapshot::Geographic)
                }
                SourceDemand::Boats(options) => {
                    geography::boats(&mut self.client, options, clock.epoch_ms, revision, stop)
                        .map(SourceSnapshot::Geographic)
                }
                SourceDemand::Chess(options) => {
                    documents::chess(&mut self.client, options, clock.epoch_ms, revision, stop)
                        .map(SourceSnapshot::Chess)
                }
                SourceDemand::Weather(options) => {
                    astronomy::weather(&mut self.client, options, clock.epoch_ms, revision, stop)
                        .map(SourceSnapshot::Weather)
                }
            }?;
            snapshot.compact();
            if snapshot.owned_bytes()? > budget.retained_bytes {
                return Err(AnimationError::Budget(
                    "source retained result envelope".into(),
                ));
            }
            Ok(snapshot)
        })();
        cancelled(stop)?;
        self.client.authorize_demand(&slot.demand)?;
        if self.client.native.suspended()? {
            // Preserve the SAME native slot/revision/cache/cadence through actor suspension.
            // The actor will resume this clock and its recorded transport prefix.
            slot.next_due_ms = previous_due_ms;
            return Err(AnimationError::Runtime(
                "native source transport suspended".into(),
            ));
        }
        match result {
            Ok(snapshot) => {
                let admission = retained_admission.take().ok_or_else(|| {
                    AnimationError::Budget("source result admission missing".into())
                })?;
                slot.snapshot = Some(Arc::new(AdmittedSourceSnapshot::new(
                    snapshot,
                    admission,
                    std::mem::take(&mut self.client.images),
                    stop,
                )?));
                slot.revision = revision;
                slot.failures = 0;
                Ok(slot.snapshot.clone())
            }
            Err(error) => {
                self.client.images.clear();
                self.client.terrain = None;
                slot.failures = slot.failures.saturating_add(1);
                let backoff = slot
                    .interval_ms
                    .saturating_mul(1u64 << slot.failures.min(5))
                    .min(3_600_000);
                slot.next_due_ms = clock
                    .monotonic_ms
                    .saturating_add(backoff.max(slot.interval_ms));
                if let (Some(previous), Some(admission)) =
                    (&slot.snapshot, retained_admission.take())
                {
                    // Full payload copy is paid BEFORE clone; escaped old Arcs
                    // remain immutable and retain their original debit.
                    let mut retained = previous.view().clone();
                    if let Some(metadata) = retained.metadata_mut() {
                        metadata.status = "error".into();
                        metadata.error = Some(error_brief(&error));
                        metadata.age_ms = metadata
                            .observed_at_ms
                            .map(|observed| clock.epoch_ms.saturating_sub(observed).max(0) as u64);
                    }
                    if retained.owned_bytes()? <= budget.retained_bytes {
                        slot.snapshot = Some(Arc::new(AdmittedSourceSnapshot::new(
                            retained,
                            admission,
                            previous.native_images().to_vec(),
                            stop,
                        )?));
                    }
                }
                Err(error)
            }
        }
    }
    pub fn next_due_ms(&self, handle: SourceHandle) -> Option<u64> {
        self.slots.get(&handle).map(|slot| slot.next_due_ms)
    }
    pub fn dispatch(
        &mut self,
        request: SourceRequest,
        clock: SourceClock,
        stop: &AtomicBool,
    ) -> Result<Arc<AdmittedSourceValue>> {
        self.dispatch_with_format(request, clock, stop, false)
    }
    #[cfg(all(
        feature = "v8-runtime",
        feature = "native-host",
        feature = "native-network"
    ))]
    pub fn dispatch_native(
        &mut self,
        request: SourceRequest,
        clock: SourceClock,
        stop: &AtomicBool,
    ) -> Result<Arc<AdmittedSourceValue>> {
        self.dispatch_with_format(request, clock, stop, true)
    }
    fn dispatch_with_format(
        &mut self,
        request: SourceRequest,
        _clock: SourceClock,
        stop: &AtomicBool,
        binary_elevation: bool,
    ) -> Result<Arc<AdmittedSourceValue>> {
        cancelled(stop)?;
        self.client.authorize_operation(&request)?;
        let admission = source_storage(&self.quota, operation_retained(&request)?)?;
        let _peak = source_storage(&self.quota, operation_peak(&request)?)?;
        self.client.images.clear();
        self.client.terrain = None;
        let mut binary_f32 = None;
        let result = match &request {
            SourceRequest::GeographyCoastlines {
                body,
                bounds,
                max_points,
                seed,
            } => terrain::coastlines(&mut self.client, body, *bounds, *max_points, *seed, stop),
            SourceRequest::GeographyElevation {
                body,
                bounds,
                width,
                height,
                seed,
            } => {
                if binary_elevation {
                    terrain::elevation_binary(
                        &mut self.client,
                        body,
                        *bounds,
                        *width,
                        *height,
                        seed.unwrap_or(0),
                        stop,
                    )
                    .map(|product| {
                        binary_f32 = Some(product.samples);
                        product.metadata
                    })
                } else {
                    terrain::elevation(
                        &mut self.client,
                        body,
                        *bounds,
                        *width,
                        *height,
                        seed.unwrap_or(0),
                        stop,
                    )
                }
            }
            SourceRequest::GeographyProject {
                latitude,
                longitude,
                projection,
            } => geography::project(*latitude, *longitude, projection),
            SourceRequest::ChessDiscover { max_games } => {
                documents::discover_chess(&mut self.client, *max_games, stop)
            }
            SourceRequest::WikipediaSearch { query, max_results } => {
                documents::wiki_search(&mut self.client, query, *max_results, stop)
            }
            SourceRequest::WikipediaArticle {
                title,
                max_bytes,
                max_images,
            } => documents::wiki_article(&mut self.client, title, *max_bytes, *max_images, stop),
            SourceRequest::OsmGeocode { query, max_results } => {
                geography::geocode(&mut self.client, query, *max_results, stop)
            }
            SourceRequest::OsmTile { x, y, zoom, format } => {
                geography::osm_tile(&mut self.client, *x, *y, *zoom, format, stop)
            }
            SourceRequest::AstronomyCatalogue { name, max_stars } => {
                astronomy::catalogue(&mut self.client, name, *max_stars)
            }
            SourceRequest::AstronomyObserve {
                epoch_ms,
                latitude,
                longitude,
            } => astronomy::observe(*epoch_ms, *latitude, *longitude),
        };
        let output = match result {
            Ok(value) => value,
            Err(error) => {
                self.client.images.clear();
                self.client.terrain = None;
                return Err(error);
            }
        };
        cancelled(stop)?;
        self.client.authorize_operation(&request)?;
        let binary_bytes = binary_f32
            .as_ref()
            .map(|values: &Vec<f32>| {
                values
                    .capacity()
                    .checked_mul(std::mem::size_of::<f32>())
                    .ok_or_else(|| AnimationError::Budget("source f32 capacity".into()))
            })
            .transpose()?
            .unwrap_or(0);
        let retained_bytes = operation_retained(&request)?;
        if json_owned_bytes(&output)?
            .checked_add(binary_bytes)
            .is_none_or(|bytes| bytes > retained_bytes)
        {
            return types::fail("source output budget");
        }
        Ok(Arc::new(AdmittedSourceValue::new(
            output,
            admission,
            std::mem::take(&mut self.client.images),
            binary_f32,
        )))
    }
}

pub fn validate_demand(demand: &SourceDemand) -> Result<u64> {
    match demand {
        SourceDemand::Series(options) => {
            if !(1..=4096).contains(&options.max_samples)
                || options.interval_ms == 0
                || !(1..=525600).contains(&options.window_minutes)
            {
                return types::fail("invalid series demand");
            }
            let source = series::source(options)?;
            Ok(options.interval_ms.max(source.minimum_poll_seconds * 1000))
        }
        SourceDemand::Earthquakes(options)
        | SourceDemand::Aircraft(options)
        | SourceDemand::Boats(options) => {
            options.validate()?;
            if !matches!(demand, SourceDemand::Boats(_)) && options.provider.is_some() {
                return types::fail("boat provider on non-boat demand");
            }
            let floor = match demand {
                SourceDemand::Aircraft(_) => 900_000,
                SourceDemand::Boats(options)
                    if options.provider == Some(BoatProvider::Digitraffic) =>
                {
                    30_000
                }
                SourceDemand::Boats(_) => 60_000,
                _ => 60_000,
            };
            Ok(((1000.0 / options.max_hz).ceil() as u64).max(floor))
        }
        SourceDemand::Chess(options) => {
            if options.game_id.is_empty()
                || options.game_id.len() > 32
                || !options
                    .game_id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric())
                || !options.max_hz.is_finite()
                || !(0.01..=20.0).contains(&options.max_hz)
            {
                return types::fail("invalid chess demand");
            }
            Ok(((1000.0 / options.max_hz).ceil() as u64).max(1000))
        }
        SourceDemand::Weather(options) => {
            options.validate()?;
            let floor = if options
                .layers
                .iter()
                .all(|layer| matches!(layer.as_str(), "night_lights_daily" | "black_marble"))
            {
                3_600_000
            } else {
                600_000
            };
            Ok(((1000.0 / options.geographic.max_hz).ceil() as u64).max(floor))
        }
    }
}
pub(crate) fn cancelled(stop: &AtomicBool) -> Result<()> {
    if stop.load(Ordering::Acquire) {
        types::fail("cancelled")
    } else {
        Ok(())
    }
}
pub(crate) fn validate_pixels(width: usize, height: usize) -> Result<()> {
    if width == 0
        || height == 0
        || width
            .checked_mul(height)
            .is_none_or(|pixels| pixels > 1024 * 1024)
    {
        return types::fail("image dimensions exceed source budget");
    }
    Ok(())
}
pub(crate) fn http_options(url: String, max_bytes: usize, response: &str) -> HttpOptions {
    HttpOptions {
        url,
        method: "GET".into(),
        headers: BTreeMap::new(),
        body: None,
        response: response.into(),
        max_bytes,
        timeout_ms: 15000,
        credential: None,
    }
}
pub(crate) fn bytes<C: BrokerSourceClient>(
    client: &mut C,
    options: HttpOptions,
    stop: &AtomicBool,
) -> Result<Vec<u8>> {
    cancelled(stop)?;
    options.validate()?;
    let response = client.request(&options, stop)?;
    if !(200..=299).contains(&response.status) {
        return types::fail(&format!("provider HTTP {}", response.status));
    }
    let data = match response.body {
        Value::String(text) => text.into_bytes(),
        Value::Array(values) => values
            .into_iter()
            .map(|value| {
                value
                    .as_u64()
                    .and_then(|value| u8::try_from(value).ok())
                    .ok_or_else(|| AnimationError::Runtime("source byte response type".into()))
            })
            .collect::<Result<Vec<_>>>()?,
        value => serde_json::to_vec(&value)?,
    };
    if data.len() > options.max_bytes {
        return types::fail("provider response exceeded budget");
    }
    cancelled(stop)?;
    Ok(data)
}

struct SourceBudget {
    peak_bytes: usize,
    retained_bytes: usize,
}
fn source_storage(quota: &QuotaGroup, bytes: usize) -> Result<StorageAdmission> {
    quota
        .reserve_external_storage(bytes)
        .map_err(|reason| AnimationError::Budget(format!("source storage admission: {reason:?}")))
}
fn source_budget(demand: &SourceDemand) -> Result<SourceBudget> {
    const MIB: usize = 1024 * 1024;
    // Cardinality preflight bounds full-feed Value allocation BEFORE native
    // parsing: ordinary16k nodes/512KiB strings; fleet4M nodes with each row
    // subtree<=16k and streaming native decode. Byte-array expansion is exact
    // encoded_len * size_of::<Value>(); weather limits it to an8MiB tile.
    // Logical envelopes include projected vector compaction/copies; codec
    // scratch/pixels and process caches retain their OWN original debits.
    // These are declared capacities, not allocator/RSS containment.
    let (peak_bytes, retained_bytes) = match demand {
        SourceDemand::Series(options) => (
            if options.provider == SeriesProvider::Wikipedia {
                16 * MIB
            } else {
                96 * MIB
            },
            options
                .max_samples
                .checked_mul(std::mem::size_of::<SeriesPoint>())
                .and_then(|bytes| bytes.checked_add(1024 * 1024))
                .ok_or_else(|| AnimationError::Budget("series storage budget".into()))?,
        ),
        SourceDemand::Earthquakes(options)
        | SourceDemand::Aircraft(options)
        | SourceDemand::Boats(options) => (
            if matches!(demand, SourceDemand::Boats(geo) if geo.provider.unwrap_or(BoatProvider::Openseafeed) == BoatProvider::Openseafeed)
            {
                384 * MIB
            } else {
                96 * MIB
            },
            options
                .max_entities
                .checked_mul(std::mem::size_of::<GeoEntity>() + 8192)
                .and_then(|bytes| bytes.checked_add(MIB))
                .ok_or_else(|| AnimationError::Budget("geographic storage budget".into()))?,
        ),
        SourceDemand::Chess(_) => (8 * MIB, MIB),
        SourceDemand::Weather(_) => (384 * MIB, 32 * MIB),
    };
    Ok(SourceBudget {
        peak_bytes,
        retained_bytes,
    })
}
fn demand_bytes(demand: &SourceDemand) -> Result<usize> {
    let mut bytes = std::mem::size_of::<SourceDemand>() + 1024;
    let geo = |value: &GeoOptions| -> Option<usize> {
        value
            .fields
            .capacity()
            .checked_mul(std::mem::size_of::<String>())?
            .checked_add(
                value
                    .fields
                    .iter()
                    .try_fold(0usize, |sum, text| sum.checked_add(text.capacity()))?,
            )?
            .checked_add(value.credential.as_ref().map_or(0, String::capacity))
    };
    let extra = match demand {
        SourceDemand::Series(value) => value.source_id.as_ref().map_or(0, String::capacity),
        SourceDemand::Earthquakes(value)
        | SourceDemand::Aircraft(value)
        | SourceDemand::Boats(value) => {
            geo(value).ok_or_else(|| AnimationError::Budget("source demand size".into()))?
        }
        SourceDemand::Chess(value) => value.game_id.capacity(),
        SourceDemand::Weather(value) => geo(&value.geographic)
            .and_then(|size| {
                size.checked_add(
                    value
                        .layers
                        .capacity()
                        .checked_mul(std::mem::size_of::<String>())?,
                )
            })
            .and_then(|size| {
                size.checked_add(
                    value
                        .layers
                        .iter()
                        .try_fold(0usize, |sum, text| sum.checked_add(text.capacity()))?,
                )
            })
            .ok_or_else(|| AnimationError::Budget("weather demand size".into()))?,
    };
    bytes = bytes
        .checked_add(extra)
        .ok_or_else(|| AnimationError::Budget("source demand size".into()))?;
    if bytes > 64 * 1024 {
        return Err(AnimationError::Budget("source demand capacity".into()));
    }
    Ok(bytes)
}
fn error_brief(error: &AnimationError) -> String {
    use std::fmt::Write;
    struct Brief {
        text: String,
        remaining: usize,
    }
    impl std::fmt::Write for Brief {
        fn write_str(&mut self, value: &str) -> std::fmt::Result {
            for character in value.chars().take(self.remaining) {
                self.text.push(character);
                self.remaining -= 1;
            }
            if self.remaining == 0 {
                Err(std::fmt::Error)
            } else {
                Ok(())
            }
        }
    }
    let mut brief = Brief {
        text: String::with_capacity(960),
        remaining: 240,
    };
    let _ = write!(&mut brief, "{error}");
    brief.text
}
