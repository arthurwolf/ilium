//! Native procedural clip chunks. The host supplies a pinned private directory;
//! this module never opens package modules/assets or an arbitrary guest path.
//! Source-owned frames require a separate retained evidence adapter and cannot
//! be restored through this procedural-only store.
use crate::error::{AnimationError, Result};
use crate::surface::{NativeText, PackedSurface, Shape};
use ilium_execution::{QuotaGroup, StorageAdmission};
use ilium_platform::animation_files::{DirectoryMutationLease, PinnedDirectory, WriteMode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    io::{self, Cursor, Read, Write},
    sync::{Arc, Mutex},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const MAGIC: &[u8; 8] = b"ILMCH001";
// The platform directory inventory caps at 1024 entries; sixteen frames per
// chunk cover the replay contract's 14,400-frame ceiling within that bound.
const FRAMES_PER_CHUNK: usize = 16;
const CHUNK_RAW_MAX: usize = 32 * 1024 * 1024;
const CHUNK_COMPRESSED_MAX: usize = CHUNK_RAW_MAX + CHUNK_RAW_MAX / 8 + 4096;
const INDEX_MAX: usize = 1024 * 1024;
const MAX_DIRECTORY_ENTRIES: usize = 1024;
// One entry in a completed clip is its index; leave room for it.
const MAX_CHUNKS: usize = MAX_DIRECTORY_ENTRIES - 1;
const WINDOW_CHUNKS: usize = 2;

fn invalid(message: &str) -> AnimationError {
    AnimationError::Runtime(format!("clip chunk: {message}"))
}
fn charge(quota: &QuotaGroup, bytes: usize) -> Result<StorageAdmission> {
    quota
        .reserve_external_storage(bytes)
        .map_err(|error| AnimationError::Budget(format!("clip chunk storage: {error:?}")))
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn valid_key(key: &str) -> bool {
    key.len() == 64
        && key
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn exact_file(directory: &PinnedDirectory, name: &str, maximum: usize) -> Result<Vec<u8>> {
    let file = directory.open_file(name)?;
    let length = usize::try_from(file.len()?).map_err(|_| invalid("file length overflow"))?;
    if length == 0 || length > maximum {
        return Err(AnimationError::Budget("clip chunk file length".into()));
    }
    let mut bytes = vec![0; length];
    let mut read = 0usize;
    while read < length {
        let count = file.read_at(&mut bytes[read..], read as u64)?;
        if count == 0 {
            return Err(
                io::Error::new(io::ErrorKind::UnexpectedEof, "clip chunk shortened").into(),
            );
        }
        read += count;
    }
    if file.len()? != length as u64 {
        return Err(invalid("clip chunk length changed"));
    }
    Ok(bytes)
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ProceduralFrameRef<'a> {
    masks: &'a [u8],
    rgb: &'a [Option<[u8; 3]>],
    text: &'a [NativeText],
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProceduralFrameOwned {
    masks: Vec<u8>,
    rgb: Vec<Option<[u8; 3]>>,
    text: Vec<NativeText>,
}
struct BoundedCounter(usize);
impl Write for BoundedCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|n| *n <= CHUNK_RAW_MAX - 14)
            .ok_or_else(|| io::Error::other("procedural frame byte limit"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn validate_procedural(shape: Shape, frame: &PackedSurface, text: &[NativeText]) -> Result<()> {
    let layout = shape
        .layout()
        .map_err(|error| invalid(&error.to_string()))?;
    if frame.masks.len() != layout.cells
        || frame.rgb.len() != layout.cells
        || frame.owners.len() != layout.dots
        || frame.owners.iter().any(Option::is_some)
    {
        return Err(AnimationError::PermissionDenied(
            "procedural clip contains absent cells or protected source owners".into(),
        ));
    }
    let mut text_bytes = 0usize;
    let mut occupied = BTreeSet::new();
    if text.len() > 64 {
        return Err(invalid("procedural text span limit"));
    }
    for span in text {
        text_bytes = text_bytes
            .checked_add(span.text.len())
            .ok_or_else(|| invalid("procedural text byte overflow"))?;
        if text_bytes > 16_384
            || !matches!(span.width, 1 | 2)
            || span.text.len() > 256
            || span.text.chars().any(char::is_control)
            || span.text.graphemes(true).count() != 1
            || UnicodeWidthStr::width(span.text.as_str()) != usize::from(span.width)
            || span
                .x
                .checked_add(u32::from(span.width))
                .is_none_or(|end| end > shape.cell_width)
            || span.y >= shape.cell_height
        {
            return Err(invalid("procedural text geometry, width or byte limit"));
        }
        for x in span.x..span.x + u32::from(span.width) {
            if !occupied.insert((span.y, x)) {
                return Err(invalid("overlapping procedural text cells"));
            }
        }
    }
    Ok(())
}

/// A disk frame's decoded allocations retain a charge from the original root.
pub struct ProceduralFrame {
    pub packed: PackedSurface,
    pub text: Vec<NativeText>,
    _storage: StorageAdmission,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChunkRecord {
    name: String,
    first: usize,
    count: usize,
    compressed_bytes: usize,
    raw_bytes: usize,
    sha256: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompleteIndex {
    version: u32,
    key: String,
    frame_count: usize,
    source_owned: bool,
    complete: bool,
    chunks: Vec<ChunkRecord>,
}
impl CompleteIndex {
    fn validate(&self, expected_key: &str) -> Result<()> {
        if self.version != 1
            || self.key != expected_key
            || !self.complete
            || self.source_owned
            || self.frame_count == 0
            || self.frame_count > 14_400
            || self.chunks.is_empty()
            || self.chunks.len() > MAX_CHUNKS
        {
            return Err(invalid("index identity, completion or source policy"));
        }
        let mut next = 0usize;
        for chunk in &self.chunks {
            if chunk.first != next
                || chunk.count == 0
                || chunk.count > FRAMES_PER_CHUNK
                || chunk.raw_bytes < MAGIC.len() + 2 + chunk.count * 5
                || chunk.raw_bytes > CHUNK_RAW_MAX
                || chunk.compressed_bytes == 0
                || chunk.compressed_bytes > CHUNK_COMPRESSED_MAX
                || !valid_key(&chunk.sha256)
                || chunk.name != format!("chunk-{}-{}.zip", chunk.first, chunk.sha256)
            {
                return Err(invalid("noncontiguous or malformed chunk index"));
            }
            next = next
                .checked_add(chunk.count)
                .ok_or_else(|| invalid("frame count overflow"))?;
        }
        if next != self.frame_count {
            return Err(invalid("index frame count mismatch"));
        }
        Ok(())
    }
}

// Called only while the pinned root's exclusive namespace lease is held.
// A live reader holds a shared lease on its child, so no retained playback
// window can lose backing chunks. Unknown/corrupt entries are quarantined.
fn reclaim_clip(
    root: &PinnedDirectory,
    quota: &QuotaGroup,
    key: &str,
    completed_allowed: bool,
) -> Result<Option<usize>> {
    let directory = match root.child(key, false) {
        Ok(directory) => directory,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let _lease = match directory.try_exclusive_lease() {
        Ok(lease) => lease,
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let _inventory = charge(quota, INDEX_MAX + 2 * 1024 * 1024)?;
    let entries = directory.list(MAX_DIRECTORY_ENTRIES)?;
    let has_index = entries.iter().any(|entry| entry.name == "index.json");
    if has_index && !completed_allowed {
        return Ok(None);
    }
    let expected: BTreeSet<String> = if has_index {
        let bytes = exact_file(&directory, "index.json", INDEX_MAX)?;
        let index: CompleteIndex = serde_json::from_slice(&bytes)?;
        index.validate(key)?;
        index
            .chunks
            .into_iter()
            .map(|chunk| chunk.name)
            .chain(std::iter::once("index.json".to_owned()))
            .collect()
    } else {
        BTreeSet::new()
    };
    let mut freed = 0usize;
    for entry in &entries {
        if entry.is_directory {
            return Err(invalid("clip reclamation found nested directory"));
        }
        let recognized = if has_index {
            expected.contains(&entry.name)
        } else {
            (entry.name.starts_with("chunk-") && entry.name.ends_with(".zip"))
                || entry.name.starts_with(".ilium-stage-")
        };
        if !recognized {
            return Err(invalid("clip reclamation found foreign entry"));
        }
        freed = freed
            .checked_add(
                usize::try_from(entry.bytes).map_err(|_| invalid("clip reclamation file size"))?,
            )
            .ok_or_else(|| invalid("clip reclamation size overflow"))?;
    }
    if has_index && expected.len() != entries.len() {
        return Err(invalid("clip reclamation index inventory mismatch"));
    }
    // Index removal first withdraws completion. Any later deletion failure
    // leaves an unavailable partial directory, never a false completed clip.
    if has_index {
        let pinned = directory.open_file("index.json")?;
        directory.remove_pinned_file("index.json", &pinned)?;
    }
    for entry in &entries {
        if entry.name == "index.json" {
            continue;
        }
        let pinned = directory.open_file(&entry.name)?;
        directory.remove_pinned_file(&entry.name, &pinned)?;
    }
    directory.sync()?;
    let identity = directory.identity();
    // Keep the child exclusive lease through parent unlink. Otherwise a new
    // playback reader could acquire the child between lease release and unlink.
    root.remove_empty_child(key, identity)?;
    Ok(Some(freed))
}
// Partial directories go first. Completed clips are ordered by the native
// index inode's last admitted use, oldest first. A live reader's shared child
// lease still wins over every eviction attempt.
fn ranked_clips(root: &PinnedDirectory) -> Result<Vec<(String, bool)>> {
    let mut ranked = Vec::new();
    for entry in root.list(MAX_DIRECTORY_ENTRIES)? {
        if !entry.is_directory || !valid_key(&entry.name) {
            return Err(invalid("unexpected clip root entry"));
        }
        let directory = root.child(&entry.name, false)?;
        let used = match directory.open_file("index.json") {
            Ok(index) => Some(index.modified()?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        ranked.push((entry.name, used));
    }
    ranked.sort_by(|left, right| left.1.cmp(&right.1).then(left.0.cmp(&right.0)));
    Ok(ranked
        .into_iter()
        .map(|(name, used)| (name, used.is_some()))
        .collect())
}
fn reclaim_budget(
    root: &PinnedDirectory,
    quota: &QuotaGroup,
    current: &str,
    disk_used: &mut usize,
    required: usize,
    maximum: usize,
) -> Result<()> {
    if disk_used
        .checked_add(required)
        .is_some_and(|total| total <= maximum)
    {
        return Ok(());
    }
    for (name, completed) in ranked_clips(root)? {
        if name == current {
            continue;
        }
        if let Some(freed) = reclaim_clip(root, quota, &name, completed)? {
            *disk_used = disk_used
                .checked_sub(freed)
                .ok_or_else(|| invalid("clip disk accounting underflow"))?;
            if disk_used
                .checked_add(required)
                .is_some_and(|total| total <= maximum)
            {
                return Ok(());
            }
        }
    }
    Err(AnimationError::Budget(
        "clip root disk budget retained by live readers or foreign entries".into(),
    ))
}

/// The host creates/pins the private cache root and supplies its ORIGINAL quota.
pub struct ClipChunkStore {
    root: Arc<PinnedDirectory>,
    quota: QuotaGroup,
    max_disk_bytes: usize,
}
impl ClipChunkStore {
    pub fn shares_root(&self, quota: &QuotaGroup) -> bool {
        self.quota.shares_root(quota)
    }
    pub fn new(
        root: Arc<PinnedDirectory>,
        quota: QuotaGroup,
        max_disk_bytes: usize,
    ) -> Result<Self> {
        if !(CHUNK_COMPRESSED_MAX + INDEX_MAX..=2 * 1024 * 1024 * 1024).contains(&max_disk_bytes) {
            return Err(AnimationError::Budget("clip disk limit".into()));
        }
        Ok(Self {
            root,
            quota,
            max_disk_bytes,
        })
    }
    /// Reject source-owned frames until a real retained/cold reauthentication
    /// adapter exists. The caller may only pass encoded procedural frame bytes.
    pub fn begin_procedural(
        &self,
        key: &str,
        frame_count: usize,
        has_source_owners: bool,
    ) -> Result<ClipChunkWriter> {
        if !valid_key(key) || frame_count == 0 || frame_count > 14_400 {
            return Err(invalid("writer key or finite frame count"));
        }
        if has_source_owners {
            return Err(AnimationError::PermissionDenied(
                "cold protected replay evidence is unavailable".into(),
            ));
        }
        // One pinned root-wide mutation lease serializes independent clip keys.
        // Account every old complete or abandoned byte before admitting another
        // chunk; a per-key limit alone would permit unbounded cache growth.
        let root_lease = self.root.try_exclusive_lease()?;
        let _inventory_storage = charge(&self.quota, 2 * 1024 * 1024)?;
        let mut root_entries = self.root.list(MAX_DIRECTORY_ENTRIES)?;
        if root_entries.len() == MAX_DIRECTORY_ENTRIES
            && !root_entries.iter().any(|entry| entry.name == key)
        {
            let mut removed = false;
            for (name, completed) in ranked_clips(&self.root)? {
                if reclaim_clip(&self.root, &self.quota, &name, completed)?.is_some() {
                    removed = true;
                    break;
                }
            }
            if !removed {
                return Err(AnimationError::Budget("clip directory inventory".into()));
            }
            root_entries = self.root.list(MAX_DIRECTORY_ENTRIES)?;
        }
        let mut disk_used = 0usize;
        for entry in &root_entries {
            if !entry.is_directory || !valid_key(&entry.name) {
                return Err(invalid("unexpected cache root entry"));
            }
            let existing = self.root.child(&entry.name, false)?;
            for member in existing.list(MAX_DIRECTORY_ENTRIES)? {
                if member.is_directory {
                    return Err(invalid("unexpected clip subdirectory"));
                }
                disk_used = disk_used
                    .checked_add(usize::try_from(member.bytes).map_err(|_| invalid("disk size"))?)
                    .ok_or_else(|| invalid("disk size overflow"))?;
            }
        }
        if root_entries.iter().any(|entry| entry.name == key) {
            if let Some(freed) = reclaim_clip(&self.root, &self.quota, key, false)? {
                disk_used = disk_used
                    .checked_sub(freed)
                    .ok_or_else(|| invalid("clip partial cleanup accounting"))?;
            }
        }
        reclaim_budget(
            &self.root,
            &self.quota,
            key,
            &mut disk_used,
            INDEX_MAX,
            self.max_disk_bytes,
        )?;
        let directory = Arc::new(self.root.child(key, true)?);
        let lease = directory.try_exclusive_lease()?;
        match directory.open_file("index.json") {
            Ok(_) => return Err(invalid("completed clip already exists")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(ClipChunkWriter {
            directory,
            root: Arc::clone(&self.root),
            _root_lease: root_lease,
            _lease: lease,
            quota: self.quota.clone(),
            key: key.to_owned(),
            expected: frame_count,
            written: 0,
            frames: Vec::with_capacity(FRAMES_PER_CHUNK),
            charges: Vec::with_capacity(FRAMES_PER_CHUNK),
            raw_bytes: 0,
            chunks: Vec::new(),
            disk_used,
            max_disk_bytes: self.max_disk_bytes,
            _index_storage: charge(&self.quota, INDEX_MAX)?,
        })
    }
    /// A missing/partial index is unavailable; no chunk is inferred complete
    /// from a filename. This path admits only source-free procedural clips.
    pub fn open_procedural(&self, key: &str) -> Result<ClipChunkReader> {
        if !valid_key(key) {
            return Err(invalid("reader key"));
        }
        let directory = Arc::new(self.root.child(key, false)?);
        let lease = directory.try_shared_lease()?;
        let index_storage = charge(&self.quota, INDEX_MAX)?;
        let index_bytes = exact_file(&directory, "index.json", INDEX_MAX)?;
        let index: CompleteIndex = serde_json::from_slice(&index_bytes)?;
        index.validate(key)?;
        // This shared child lease excludes eviction until the reader drops.
        // Update the exact completed index's age only after validation.
        directory.open_file("index.json")?.mark_used()?;
        Ok(ClipChunkReader {
            directory,
            _lease: lease,
            quota: self.quota.clone(),
            index,
            window: Mutex::new(Vec::with_capacity(WINDOW_CHUNKS)),
            _index_storage: index_storage,
        })
    }
}

pub struct ClipChunkWriter {
    directory: Arc<PinnedDirectory>,
    root: Arc<PinnedDirectory>,
    _root_lease: DirectoryMutationLease,
    _lease: DirectoryMutationLease,
    quota: QuotaGroup,
    key: String,
    expected: usize,
    written: usize,
    frames: Vec<Vec<u8>>,
    charges: Vec<StorageAdmission>,
    raw_bytes: usize,
    chunks: Vec<ChunkRecord>,
    disk_used: usize,
    max_disk_bytes: usize,
    _index_storage: StorageAdmission,
}
impl ClipChunkWriter {
    /// Only full source-free frames enter the persistent procedural stream.
    /// The native caller already produced/validated the unoccluded packed frame.
    pub fn push_packed(
        &mut self,
        shape: Shape,
        packed: &PackedSurface,
        text: &[NativeText],
    ) -> Result<()> {
        validate_procedural(shape, packed, text)?;
        let wire = ProceduralFrameRef {
            masks: &packed.masks,
            rgb: &packed.rgb,
            text,
        };
        let mut count = BoundedCounter(0);
        serde_json::to_writer(&mut count, &wire)?;
        let _encoding = charge(&self.quota, count.0 + 4096)?;
        let mut bytes = Vec::with_capacity(count.0);
        serde_json::to_writer(&mut bytes, &wire)?;
        if bytes.len() != count.0 {
            return Err(invalid("procedural frame changed during encoding"));
        }
        self.push_frame(&bytes)
    }
    pub fn push_frame(&mut self, bytes: &[u8]) -> Result<()> {
        if self.written >= self.expected || bytes.is_empty() || bytes.len() > CHUNK_RAW_MAX - 15 {
            return Err(invalid("frame outside finite chunk schedule"));
        }
        let addition = bytes
            .len()
            .checked_add(4)
            .ok_or_else(|| invalid("frame size"))?;
        if self.frames.len() == FRAMES_PER_CHUNK
            || self.raw_bytes + addition + MAGIC.len() + 2 > CHUNK_RAW_MAX
        {
            self.flush_chunk()?;
        }
        let storage = charge(&self.quota, addition + 4096)?;
        self.frames.push(bytes.to_vec());
        self.charges.push(storage);
        self.raw_bytes += addition;
        self.written += 1;
        Ok(())
    }
    fn flush_chunk(&mut self) -> Result<()> {
        if self.frames.is_empty() {
            return Ok(());
        }
        if self.chunks.len() >= MAX_CHUNKS {
            return Err(AnimationError::Budget("clip chunk count".into()));
        }
        let raw_length = MAGIC.len() + 2 + self.raw_bytes;
        let _raw_storage = charge(&self.quota, raw_length)?;
        let mut raw = Vec::with_capacity(raw_length);
        raw.extend_from_slice(MAGIC);
        raw.extend_from_slice(&(self.frames.len() as u16).to_le_bytes());
        for frame in &self.frames {
            raw.extend_from_slice(&(frame.len() as u32).to_le_bytes());
            raw.extend_from_slice(frame);
        }
        let _zip_storage = charge(&self.quota, CHUNK_COMPRESSED_MAX)?;
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer.start_file(
            "frames.bin",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )?;
        writer.write_all(&raw)?;
        let compressed = writer.finish()?.into_inner();
        if compressed.len() > CHUNK_COMPRESSED_MAX {
            return Err(AnimationError::Budget("compressed clip chunk".into()));
        }
        let first = self.written - self.frames.len();
        let sha256 = digest(&compressed);
        let name = format!("chunk-{first}-{sha256}.zip");
        reclaim_budget(
            &self.root,
            &self.quota,
            &self.key,
            &mut self.disk_used,
            compressed.len() + INDEX_MAX,
            self.max_disk_bytes,
        )?;
        let next_used = self
            .disk_used
            .checked_add(compressed.len())
            .ok_or_else(|| invalid("disk budget overflow"))?;
        let mut file = self
            .directory
            .begin_atomic(&name, WriteMode::ReplaceEntry)?;
        file.write(&compressed, CHUNK_COMPRESSED_MAX)?;
        file.prepare_durable()?;
        file.publish_entry()?;
        file.durable_ack()?;
        self.chunks.push(ChunkRecord {
            name,
            first,
            count: self.frames.len(),
            compressed_bytes: compressed.len(),
            raw_bytes: raw_length,
            sha256,
        });
        self.disk_used = next_used;
        self.frames.clear();
        self.charges.clear();
        self.raw_bytes = 0;
        Ok(())
    }
    pub fn finish(mut self) -> Result<()> {
        if self.written != self.expected {
            return Err(invalid("partial clip cannot publish index"));
        }
        self.flush_chunk()?;
        let index = CompleteIndex {
            version: 1,
            key: self.key.clone(),
            frame_count: self.expected,
            source_owned: false,
            complete: true,
            chunks: std::mem::take(&mut self.chunks),
        };
        index.validate(&self.key)?;
        let bytes = serde_json::to_vec(&index)?;
        if bytes.len() > INDEX_MAX || self.disk_used + bytes.len() > self.max_disk_bytes {
            return Err(AnimationError::Budget("clip index bytes".into()));
        }
        let mut file = self
            .directory
            .begin_atomic("index.json", WriteMode::CreateNew)?;
        file.write(&bytes, INDEX_MAX)?;
        file.prepare_durable()?;
        file.publish_entry()?;
        file.durable_ack()?;
        Ok(())
    }
}

struct DecodedChunk {
    raw: Vec<u8>,
    ranges: Vec<std::ops::Range<usize>>,
    _storage: StorageAdmission,
}
pub struct FrameChunkLease {
    chunk: Arc<DecodedChunk>,
    offset: usize,
}
impl FrameChunkLease {
    pub fn bytes(&self) -> &[u8] {
        &self.chunk.raw[self.chunk.ranges[self.offset].clone()]
    }
}
pub struct ClipChunkReader {
    directory: Arc<PinnedDirectory>,
    _lease: DirectoryMutationLease,
    quota: QuotaGroup,
    index: CompleteIndex,
    window: Mutex<Vec<(usize, Arc<DecodedChunk>)>>,
    _index_storage: StorageAdmission,
}
impl ClipChunkReader {
    pub fn frame_count(&self) -> usize {
        self.index.frame_count
    }
    /// Decode a source-free frame after bounded positional read, ZIP integrity,
    /// key identity, and original-root storage admission. No SourceToken is
    /// reconstructed from disk bytes.
    pub fn procedural_frame(&self, frame_index: usize, shape: Shape) -> Result<ProceduralFrame> {
        let layout = shape
            .layout()
            .map_err(|error| invalid(&error.to_string()))?;
        let loan = self.frame(frame_index)?;
        let bytes = loan.bytes();
        // Typed Vec capacities and text objects may exceed their JSON byte
        // count during deserialization; retain a conservative second copy.
        let charge_bytes = bytes
            .len()
            .checked_mul(2)
            .and_then(|n| {
                n.checked_add(
                    layout.dots * std::mem::size_of::<Option<crate::surface::SourceToken>>(),
                )
            })
            .and_then(|n| n.checked_add(8192))
            .ok_or_else(|| invalid("decoded frame charge"))?;
        let storage = charge(&self.quota, charge_bytes)?;
        let wire: ProceduralFrameOwned = serde_json::from_slice(bytes)?;
        let packed = PackedSurface {
            masks: wire.masks,
            rgb: wire.rgb,
            owners: vec![None; layout.dots],
        };
        validate_procedural(shape, &packed, &wire.text)?;
        Ok(ProceduralFrame {
            packed,
            text: wire.text,
            _storage: storage,
        })
    }
    pub fn frame(&self, frame_index: usize) -> Result<FrameChunkLease> {
        if frame_index >= self.index.frame_count {
            return Err(invalid("frame index outside clip"));
        }
        let chunk_index = self
            .index
            .chunks
            .iter()
            .position(|record| {
                frame_index >= record.first && frame_index < record.first + record.count
            })
            .ok_or_else(|| invalid("indexed frame missing"))?;
        {
            let mut window = self.window.lock().map_err(|_| invalid("window poisoned"))?;
            if let Some(position) = window.iter().position(|(index, _)| *index == chunk_index) {
                let entry = window.remove(position);
                let chunk = Arc::clone(&entry.1);
                window.push(entry);
                return Ok(FrameChunkLease {
                    chunk,
                    offset: frame_index - self.index.chunks[chunk_index].first,
                });
            }
        }
        // Disk and decompression run without the playback-window mutex.
        let chunk = Arc::new(self.load_chunk(chunk_index)?);
        let mut window = self.window.lock().map_err(|_| invalid("window poisoned"))?;
        if let Some(position) = window.iter().position(|(index, _)| *index == chunk_index) {
            let entry = window.remove(position);
            let existing = Arc::clone(&entry.1);
            window.push(entry);
            return Ok(FrameChunkLease {
                chunk: existing,
                offset: frame_index - self.index.chunks[chunk_index].first,
            });
        }
        if window.len() == WINDOW_CHUNKS {
            window.remove(0);
        }
        window.push((chunk_index, Arc::clone(&chunk)));
        Ok(FrameChunkLease {
            chunk,
            offset: frame_index - self.index.chunks[chunk_index].first,
        })
    }
    fn load_chunk(&self, chunk_index: usize) -> Result<DecodedChunk> {
        let record = &self.index.chunks[chunk_index];
        let _compressed_storage = charge(&self.quota, record.compressed_bytes)?;
        // The indexed compressed length is the amount charged above. Refuse a
        // longer pinned file before allocating, even if it is under the
        // format-wide maximum.
        let bytes = exact_file(&self.directory, &record.name, record.compressed_bytes)?;
        if bytes.len() != record.compressed_bytes || digest(&bytes) != record.sha256 {
            return Err(AnimationError::Integrity(
                "clip chunk digest mismatch".into(),
            ));
        }
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
        if archive.len() != 1 {
            return Err(invalid("chunk archive member count"));
        }
        let mut member = archive.by_name("frames.bin")?;
        if member.size() != record.raw_bytes as u64 || member.size() > CHUNK_RAW_MAX as u64 {
            return Err(invalid("chunk raw length"));
        }
        let _raw_storage = charge(&self.quota, record.raw_bytes)?;
        let mut raw = Vec::with_capacity(record.raw_bytes);
        member
            .by_ref()
            .take(record.raw_bytes as u64 + 1)
            .read_to_end(&mut raw)?;
        if raw.len() != record.raw_bytes {
            return Err(invalid("chunk inflated length"));
        }
        // The retained charge starts before the temporary inflation charge
        // leaves scope. Ranges borrow the single retained raw allocation.
        let storage = charge(&self.quota, record.raw_bytes + 8192)?;
        let ranges = decode_frames(&raw, record.count)?;
        Ok(DecodedChunk {
            raw,
            ranges,
            _storage: storage,
        })
    }
}

fn decode_frames(raw: &[u8], expected: usize) -> Result<Vec<std::ops::Range<usize>>> {
    if raw.len() < MAGIC.len() + 2 || &raw[..MAGIC.len()] != MAGIC {
        return Err(invalid("chunk magic"));
    }
    let count = u16::from_le_bytes([raw[8], raw[9]]) as usize;
    if count != expected || count == 0 || count > FRAMES_PER_CHUNK {
        return Err(invalid("chunk frame count"));
    }
    let mut offset = 10usize;
    let mut frames = Vec::with_capacity(count);
    for _ in 0..count {
        let end = offset
            .checked_add(4)
            .ok_or_else(|| invalid("frame length offset"))?;
        let length_bytes: [u8; 4] = raw
            .get(offset..end)
            .ok_or_else(|| invalid("short frame length"))?
            .try_into()
            .map_err(|_| invalid("frame length"))?;
        let length = u32::from_le_bytes(length_bytes) as usize;
        offset = end;
        let end = offset
            .checked_add(length)
            .ok_or_else(|| invalid("frame length overflow"))?;
        if length == 0 || end > raw.len() {
            return Err(invalid("short or empty frame"));
        }
        frames.push(offset..end);
        offset = end;
    }
    if offset != raw.len() {
        return Err(invalid("trailing chunk bytes"));
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::{ColourSpace, Format, Mode, Update};
    use ilium_platform::secure_fs::NoFollowDirectory;
    #[test]
    fn raw_chunk_decoder_rejects_missing_and_trailing_frames() {
        let mut raw = MAGIC.to_vec();
        raw.extend_from_slice(&1u16.to_le_bytes());
        raw.extend_from_slice(&3u32.to_le_bytes());
        raw.extend_from_slice(b"ABC");
        assert_eq!(decode_frames(&raw, 1).unwrap(), vec![14..17]);
        assert!(decode_frames(&raw, 2).is_err());
        raw.push(0);
        assert!(decode_frames(&raw, 1).is_err());
        raw.pop();
        raw.pop();
        assert!(decode_frames(&raw, 1).is_err());
    }
    #[test]
    fn root_budget_counts_other_completed_and_abandoned_clip_bytes() {
        let fixture = tempfile::tempdir().unwrap();
        let root = Arc::new(
            PinnedDirectory::from_host(Arc::new(
                NoFollowDirectory::open_root(fixture.path()).unwrap(),
            ))
            .unwrap(),
        );
        let quota = QuotaGroup::new(ilium_execution::QuotaLimits {
            clients: 1,
            jobs: 1,
            service_jobs: 0,
            input_bytes: 1024,
            result_bytes: 1024,
            worker_threads: 1,
            worker_bytes: 512 * 1024 * 1024,
        });
        let limit = 128 * 1024 * 1024;
        let store = ClipChunkStore::new(root, quota, limit).unwrap();
        let other = fixture.path().join("b".repeat(64));
        std::fs::create_dir(&other).unwrap();
        std::fs::File::create(other.join("chunk-0-abandoned.zip"))
            .unwrap()
            .set_len(limit as u64)
            .unwrap();
        let writer = store.begin_procedural(&"a".repeat(64), 1, false).unwrap();
        assert!(
            !other.exists(),
            "abandoned partial bytes are reclaimed before admission"
        );
        drop(writer);
    }
    #[test]
    fn completed_cache_evicts_least_recently_used_clip() {
        let fixture = tempfile::tempdir().unwrap();
        let root = Arc::new(
            PinnedDirectory::from_host(Arc::new(
                NoFollowDirectory::open_root(fixture.path()).unwrap(),
            ))
            .unwrap(),
        );
        let quota = QuotaGroup::new(ilium_execution::QuotaLimits {
            clients: 1,
            jobs: 1,
            service_jobs: 0,
            input_bytes: 1024,
            result_bytes: 1024,
            worker_threads: 1,
            worker_bytes: 512 * 1024 * 1024,
        });
        let limit = 128 * 1024 * 1024;
        let store = ClipChunkStore::new(root.clone(), quota, limit).unwrap();
        let shape = Shape {
            cell_width: 1,
            cell_height: 1,
            mode: Mode::Cells,
            format: Format::Mask8,
            update: Update::Replace,
            cell_rgb: false,
            colour_space: ColourSpace::Srgb,
        };
        let frame = PackedSurface {
            masks: vec![1],
            rgb: vec![None],
            owners: vec![None; 8],
        };
        let oldest = "a".repeat(64);
        let recent = "b".repeat(64);
        for key in [&oldest, &recent] {
            let mut writer = store.begin_procedural(key, 1, false).unwrap();
            writer.push_packed(shape, &frame, &[]).unwrap();
            writer.finish().unwrap();
        }
        let old_index = std::fs::OpenOptions::new()
            .write(true)
            .open(fixture.path().join(&oldest).join("index.json"))
            .unwrap();
        old_index
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000)),
            )
            .unwrap();
        drop(old_index);
        drop(store.open_procedural(&recent).unwrap()); // Actual admitted use updates index age.
        let old_chunk = root
            .child(&oldest, false)
            .unwrap()
            .list(1024)
            .unwrap()
            .into_iter()
            .find(|entry| entry.name.starts_with("chunk-"))
            .unwrap()
            .name;
        std::fs::OpenOptions::new()
            .write(true)
            .open(fixture.path().join(&oldest).join(old_chunk))
            .unwrap()
            .set_len((limit - INDEX_MAX) as u64)
            .unwrap();
        let next = "c".repeat(64);
        let writer = store.begin_procedural(&next, 1, false).unwrap();
        assert!(!fixture.path().join(&oldest).exists());
        assert!(fixture.path().join(&recent).join("index.json").is_file());
        drop(writer);
    }

    #[test]
    fn native_chunk_index_is_atomic_exclusive_and_integrity_checked() {
        let fixture = tempfile::tempdir().unwrap();
        let root = Arc::new(
            PinnedDirectory::from_host(Arc::new(
                NoFollowDirectory::open_root(fixture.path()).unwrap(),
            ))
            .unwrap(),
        );
        let quota = QuotaGroup::new(ilium_execution::QuotaLimits {
            clients: 1,
            jobs: 1,
            service_jobs: 0,
            input_bytes: 1024,
            result_bytes: 1024,
            worker_threads: 1,
            worker_bytes: 512 * 1024 * 1024,
        });
        let store = ClipChunkStore::new(root.clone(), quota.clone(), 128 * 1024 * 1024).unwrap();
        let key = "a".repeat(64);
        let shape = Shape {
            cell_width: 1,
            cell_height: 1,
            mode: Mode::Cells,
            format: Format::Mask8,
            update: Update::Replace,
            cell_rgb: false,
            colour_space: ColourSpace::Srgb,
        };
        let frame = PackedSurface {
            masks: vec![0x81],
            rgb: vec![None],
            owners: vec![None; 8],
        };
        let mut abandoned = store.begin_procedural(&key, 2, false).unwrap();
        assert!(
            store.begin_procedural(&key, 2, false).is_err(),
            "writer lease excludes concurrent mutation"
        );
        abandoned.push_packed(shape, &frame, &[]).unwrap();
        drop(abandoned);
        assert!(
            store.open_procedural(&key).is_err(),
            "partial clip has no completed index"
        );
        let mut writer = store.begin_procedural(&key, 2, false).unwrap();
        writer.push_packed(shape, &frame, &[]).unwrap();
        writer.push_packed(shape, &frame, &[]).unwrap();
        writer.finish().unwrap();
        let reader = store.open_procedural(&key).unwrap();
        assert_eq!(reader.frame_count(), 2);
        assert_eq!(
            reader.procedural_frame(0, shape).unwrap().packed.masks,
            vec![0x81]
        );
        // Inflating the already pinned chunk models a resident cache at its
        // budget. Its shared playback lease prevents eviction, even when a
        // different clip asks for space. Releasing that lease permits exact
        // completed-index withdrawal and a new writer.
        let listed = root.child(&key, false).unwrap().list(1024).unwrap();
        let pinned_name = listed
            .iter()
            .find(|entry| entry.name.starts_with("chunk-"))
            .unwrap()
            .name
            .clone();
        std::fs::OpenOptions::new()
            .write(true)
            .open(fixture.path().join(&key).join(&pinned_name))
            .unwrap()
            .set_len((128 * 1024 * 1024) as u64)
            .unwrap();
        let alternate = "b".repeat(64);
        assert!(
            matches!(
                store.begin_procedural(&alternate, 1, false),
                Err(AnimationError::Budget(_))
            ),
            "live playback pins its clip"
        );
        drop(reader);
        let admitted = store.begin_procedural(&alternate, 1, false).unwrap();
        assert!(!fixture.path().join(&key).exists());
        drop(admitted);
        // Recreate the original clip to retain corruption/read assertions.
        let mut writer = store.begin_procedural(&key, 1, false).unwrap();
        writer.push_packed(shape, &frame, &[]).unwrap();
        writer.finish().unwrap();
        let directory = Arc::new(root.child(&key, false).unwrap());
        let chunk_name = directory
            .list(1024)
            .unwrap()
            .into_iter()
            .find(|entry| entry.name.starts_with("chunk-"))
            .unwrap()
            .name;
        let mut corrupt = directory
            .begin_atomic(&chunk_name, WriteMode::ReplaceEntry)
            .unwrap();
        corrupt.write(b"corrupt", CHUNK_COMPRESSED_MAX).unwrap();
        corrupt.prepare_durable().unwrap();
        corrupt.publish_entry().unwrap();
        corrupt.durable_ack().unwrap();
        assert!(matches!(
            store
                .open_procedural(&key)
                .unwrap()
                .procedural_frame(0, shape),
            Err(AnimationError::Integrity(_))
        ));
        std::fs::remove_file(fixture.path().join(&key).join(&chunk_name)).unwrap();
        assert!(matches!(
            store
                .open_procedural(&key)
                .unwrap()
                .procedural_frame(0, shape),
            Err(AnimationError::Io(_))
        ));
    }

    #[test]
    fn oversized_pinned_chunk_is_refused_at_its_indexed_charge_boundary() {
        let fixture = tempfile::tempdir().unwrap();
        let root = Arc::new(
            PinnedDirectory::from_host(Arc::new(
                NoFollowDirectory::open_root(fixture.path()).unwrap(),
            ))
            .unwrap(),
        );
        let quota = QuotaGroup::new(ilium_execution::QuotaLimits {
            clients: 1,
            jobs: 1,
            service_jobs: 0,
            input_bytes: 1024,
            result_bytes: 1024,
            worker_threads: 1,
            worker_bytes: 512 * 1024 * 1024,
        });
        let store = ClipChunkStore::new(root.clone(), quota.clone(), 128 * 1024 * 1024).unwrap();
        let key = "d".repeat(64);
        let shape = Shape {
            cell_width: 1,
            cell_height: 1,
            mode: Mode::Cells,
            format: Format::Mask8,
            update: Update::Replace,
            cell_rgb: false,
            colour_space: ColourSpace::Srgb,
        };
        let frame = PackedSurface {
            masks: vec![0x81],
            rgb: vec![None],
            owners: vec![None; 8],
        };
        let mut writer = store.begin_procedural(&key, 1, false).unwrap();
        writer.push_packed(shape, &frame, &[]).unwrap();
        writer.finish().unwrap();
        let directory = root.child(&key, false).unwrap();
        let chunk_name = directory
            .list(1024)
            .unwrap()
            .into_iter()
            .find(|entry| entry.name.starts_with("chunk-"))
            .unwrap()
            .name;
        let indexed_length = directory.open_file(&chunk_name).unwrap().len().unwrap();
        assert!(indexed_length < CHUNK_COMPRESSED_MAX as u64);
        std::fs::OpenOptions::new()
            .write(true)
            .open(fixture.path().join(&key).join(&chunk_name))
            .unwrap()
            .set_len(indexed_length + 1)
            .unwrap();
        let reader = store.open_procedural(&key).unwrap();
        let admitted_before_load = quota.snapshot().worker_bytes;
        assert!(matches!(reader.procedural_frame(0, shape),
            Err(AnimationError::Budget(ref reason)) if reason == "clip chunk file length"));
        assert_eq!(quota.snapshot().worker_bytes, admitted_before_load);
    }

    #[test]
    fn source_free_wide_text_round_trips_through_the_real_bounded_chunk() {
        let fixture = tempfile::tempdir().unwrap();
        let root = Arc::new(
            PinnedDirectory::from_host(Arc::new(
                NoFollowDirectory::open_root(fixture.path()).unwrap(),
            ))
            .unwrap(),
        );
        let quota = QuotaGroup::new(ilium_execution::QuotaLimits {
            clients: 1,
            jobs: 1,
            service_jobs: 0,
            input_bytes: 1024,
            result_bytes: 1024,
            worker_threads: 1,
            worker_bytes: 512 * 1024 * 1024,
        });
        let store = ClipChunkStore::new(root, quota, 128 * 1024 * 1024).unwrap();
        let key = "e".repeat(64);
        let shape = Shape {
            cell_width: 2,
            cell_height: 1,
            mode: Mode::Cells,
            format: Format::Mask8,
            update: Update::Replace,
            cell_rgb: false,
            colour_space: ColourSpace::Srgb,
        };
        let packed = PackedSurface {
            masks: vec![0, 0],
            rgb: vec![None, None],
            owners: vec![None; 16],
        };
        let text = vec![NativeText {
            x: 0,
            y: 0,
            text: "界".into(),
            width: 2,
            style: crate::surface::TextStyle {
                rgb: Some([255, 0, 0]),
                background: Some([0, 64, 0]),
                bold: true,
                italic: true,
                underline: true,
            },
        }];
        assert!(store.begin_procedural(&key, 1, true).is_err());
        let mut writer = store.begin_procedural(&key, 1, false).unwrap();
        writer.push_packed(shape, &packed, &text).unwrap();
        writer.finish().unwrap();
        let reader = store.open_procedural(&key).unwrap();
        let loaded = reader.procedural_frame(0, shape).unwrap();
        assert_eq!(loaded.text, text);
        assert_eq!(loaded.packed.masks, packed.masks);
        let malformed = vec![NativeText {
            x: 1,
            ..text[0].clone()
        }];
        assert!(validate_procedural(shape, &packed, &malformed).is_err());
        let mut protected = packed;
        protected.owners[0] = Some(crate::surface::SourceToken::from_native(1).unwrap());
        assert!(validate_procedural(shape, &protected, &text).is_err());
    }
}
