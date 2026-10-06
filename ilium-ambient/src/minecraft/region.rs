//! Read-only Java Anvil adapter for trusted, preferably quiescent local saves.
//! Two equal observations detect changes; they are NOT an atomic filesystem
//! snapshot or a hostile-path/no-follow security boundary. Kernel I/O can block.
use super::nbt::{self, Compound, Document, Tag};
use flate2::{Decompress, FlushDecompress, Status, bufread::GzDecoder};
use ilium_platform::animation_files::{FileIdentity, PinnedDirectory, PinnedFile};
use std::{
    fs::{self, File, Metadata},
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::SystemTime,
};

pub const MIN_DATA_VERSION: i32 = 2834;
/// Deliberate qualification ceiling, not the newest Minecraft version.
pub const MAX_DATA_VERSION: i32 = 3218;
const SECTOR: u64 = 4096;
const HEADER: usize = 8192;
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_compressed_bytes: usize,
    pub nbt: nbt::Limits,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_compressed_bytes: 8 << 20,
            nbt: nbt::Limits::default(),
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("region I/O: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Nbt(#[from] nbt::Error),
    #[error("invalid Anvil data: {0}")]
    Invalid(&'static str),
    #[error("region or external payload changed during reading")]
    Changed,
    #[error("read cancelled")]
    Cancelled,
    #[error("resource limit: {0}")]
    Limit(&'static str),
    #[error("unsupported region compression ID {0}")]
    UnsupportedCompression(u8),
    #[error("DataVersion {0} is outside admitted range 2834..=3218")]
    DataVersion(i32),
    #[error("chunk coordinate mismatch: expected {expected:?}, got {actual:?}")]
    Coordinates {
        expected: [i32; 2],
        actual: [i32; 2],
    },
}
type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compression {
    Gzip,
    Zlib,
    Raw,
}
fn compression(marker: u8) -> Result<Compression> {
    match marker & 127 {
        1 => Ok(Compression::Gzip),
        2 => Ok(Compression::Zlib),
        3 => Ok(Compression::Raw),
        other => Err(Error::UnsupportedCompression(other)),
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Level,
    Root,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
    pub position: [i32; 2],
    pub data_version: i32,
    pub layout: Layout,
}
#[derive(Debug)]
pub struct ChunkNbt {
    pub identity: Identity,
    pub timestamp: u32,
    pub compression: Compression,
    pub external: bool,
    pub document: Document,
}
pub fn region_of(chunk: [i32; 2]) -> [i32; 2] {
    chunk.map(|v| v.div_euclid(32))
}
pub fn slot(chunk: [i32; 2]) -> usize {
    (chunk[0].rem_euclid(32) + 32 * chunk[1].rem_euclid(32)) as usize
}
fn integer(c: &Compound, key: &str) -> Result<i32> {
    match nbt::get(c, key) {
        Some(Tag::Int(n)) => Ok(*n),
        _ => Err(Error::Invalid("missing or non-Int identity field")),
    }
}
/// Shared with chunk.rs: version-gate BEFORE interpreting either schema.
pub fn chunk_body(doc: &Document) -> Result<(i32, Layout, &Compound)> {
    let version = integer(&doc.root, "DataVersion")?;
    if !(MIN_DATA_VERSION..=MAX_DATA_VERSION).contains(&version) {
        return Err(Error::DataVersion(version));
    }
    match nbt::get(&doc.root, "Level") {
        Some(Tag::Compound(c)) => {
            if ["xPos", "zPos", "sections", "Sections"]
                .iter()
                .any(|key| nbt::get(&doc.root, key).is_some())
            {
                return Err(Error::Invalid("ambiguous root and Level schemas"));
            }
            Ok((version, Layout::Level, c))
        }
        Some(_) => Err(Error::Invalid("Level is not a compound")),
        None => Ok((version, Layout::Root, &doc.root)),
    }
}
pub fn verify_identity(doc: &Document, expected: [i32; 2]) -> Result<Identity> {
    let (data_version, layout, c) = chunk_body(doc)?;
    let actual = [integer(c, "xPos")?, integer(c, "zPos")?];
    if expected != actual {
        return Err(Error::Coordinates { expected, actual });
    }
    Ok(Identity {
        position: actual,
        data_version,
        layout,
    })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub sector: u32,
    pub sectors: u8,
    pub timestamp: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Index {
    pub region: [i32; 2],
    pub entries: [Option<Entry>; 1024],
}
fn word(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}
impl Index {
    pub fn parse(header: &[u8; HEADER], file_len: u64, region: [i32; 2]) -> Result<Self> {
        if file_len < HEADER as u64 || !file_len.is_multiple_of(SECTOR) {
            return Err(Error::Invalid("short or non-sector-aligned region file"));
        }
        let mut entries = [None; 1024];
        let mut spans = Vec::with_capacity(1024);
        for i in 0..1024 {
            let loc = word(&header[i * 4..i * 4 + 4]);
            if loc == 0 {
                continue;
            }
            let sector = loc >> 8;
            let sectors = (loc & 255) as u8;
            let end = u64::from(sector) + u64::from(sectors);
            if sector < 2 || sectors == 0 || end * SECTOR > file_len {
                return Err(Error::Invalid("location intersects header or exceeds file"));
            }
            spans.push((u64::from(sector), end));
            entries[i] = Some(Entry {
                sector,
                sectors,
                timestamp: word(&header[4096 + i * 4..4100 + i * 4]),
            });
        }
        spans.sort_unstable();
        if spans.windows(2).any(|p| p[0].1 > p[1].0) {
            return Err(Error::Invalid("overlapping chunk sectors"));
        }
        Ok(Self { region, entries })
    }
}
fn check(cancel: &dyn Fn() -> bool) -> Result<()> {
    if cancel() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}
#[derive(Debug, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: SystemTime,
    created: Option<SystemTime>,
}
fn stamp(m: Metadata) -> Result<Stamp> {
    if !m.file_type().is_file() {
        return Err(Error::Invalid("not a regular file"));
    }
    Ok(Stamp {
        len: m.len(),
        modified: m.modified()?,
        created: m.created().ok(),
    })
}
fn open_regular(path: &Path) -> Result<(File, Stamp)> {
    // Reject a final-component symlink observed here. This is not race-proof.
    let before = stamp(fs::symlink_metadata(path)?)?;
    let f = super::io::open_regular(path)?;
    if stamp(f.metadata()?)? != before {
        return Err(Error::Changed);
    }
    Ok((f, before))
}
fn unchanged(f: &File, path: &Path, before: &Stamp) -> Result<()> {
    if &stamp(f.metadata()?)? != before || &stamp(fs::symlink_metadata(path)?)? != before {
        return Err(Error::Changed);
    }
    Ok(())
}
fn read_bytes(f: &mut File, n: usize, cancel: &dyn Fn() -> bool) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    data.try_reserve_exact(n)
        .map_err(|_| Error::Limit("allocation failed"))?;
    data.resize(n, 0);
    for piece in data.chunks_mut(8192) {
        check(cancel)?;
        f.read_exact(piece)?;
    }
    Ok(data)
}
#[derive(Debug, PartialEq, Eq)]
struct Stored {
    marker: u8,
    data: Vec<u8>,
    external_stamp: Option<Stamp>,
}
#[derive(Debug, PartialEq, Eq)]
struct Capture {
    header: [u8; HEADER],
    stamp: Stamp,
    index: Index,
    stored: Option<Stored>,
}
fn capture(
    path: &Path,
    region: [i32; 2],
    chunk: Option<[i32; 2]>,
    limits: Limits,
    cancel: &dyn Fn() -> bool,
) -> Result<Capture> {
    check(cancel)?;
    let (mut f, before) = open_regular(path)?;
    let mut header = [0; HEADER];
    f.read_exact(&mut header)?;
    let index = Index::parse(&header, before.len, region)?;
    let mut stored = None;
    if let Some(pos) = chunk {
        if region_of(pos) != region {
            return Err(Error::Invalid("chunk outside region"));
        }
        if let Some(entry) = index.entries[slot(pos)] {
            f.seek(SeekFrom::Start(u64::from(entry.sector) * SECTOR))?;
            let mut prefix = [0; 5];
            f.read_exact(&mut prefix)?;
            let len = u64::from(word(&prefix[..4]));
            if len < 1 || len + 4 > u64::from(entry.sectors) * SECTOR {
                return Err(Error::Invalid("chunk length outside allocated sectors"));
            }
            let marker = prefix[4];
            compression(marker)?; // Never sniff or silently reinterpret an unsupported ID.
            let (data, external_stamp) = if marker & 128 != 0 {
                if len != 1 || entry.sectors != 1 {
                    return Err(Error::Invalid(
                        "external chunk stub must occupy one sector, length 1",
                    ));
                }
                let parent = path
                    .parent()
                    .ok_or(Error::Invalid("region has no parent"))?;
                let external = parent.join(format!("c.{}.{}.mcc", pos[0], pos[1]));
                let (mut e, stamp) = open_regular(&external)?;
                if stamp.len == 0 || stamp.len > limits.max_compressed_bytes as u64 {
                    return Err(Error::Limit("external compressed bytes"));
                }
                let data = read_bytes(&mut e, stamp.len as usize, cancel)?;
                unchanged(&e, &external, &stamp)?;
                (data, Some(stamp))
            } else {
                if len - 1 > limits.max_compressed_bytes as u64 {
                    return Err(Error::Limit("internal compressed bytes"));
                }
                (read_bytes(&mut f, (len - 1) as usize, cancel)?, None)
            };
            stored = Some(Stored {
                marker,
                data,
                external_stamp,
            });
        }
    }
    // Recheck the whole header, including timestamps and unrelated allocations.
    f.seek(SeekFrom::Start(0))?;
    let mut again = [0; HEADER];
    f.read_exact(&mut again)?;
    if header != again {
        return Err(Error::Changed);
    }
    unchanged(&f, path, &before)?;
    check(cancel)?;
    Ok(Capture {
        header,
        stamp: before,
        index,
        stored,
    })
}
fn stable(
    directory: &Path,
    region: [i32; 2],
    chunk: Option<[i32; 2]>,
    limits: Limits,
    cancel: &dyn Fn() -> bool,
) -> Result<Capture> {
    check(cancel)?;
    let path: PathBuf =
        fs::canonicalize(directory)?.join(format!("r.{}.{}.mca", region[0], region[1]));
    let a = capture(&path, region, chunk, limits, cancel)?;
    // Reopen by path: do not keep serving an old handle after a rename.
    let b = capture(&path, region, chunk, limits, cancel)?;
    if a != b {
        return Err(Error::Changed);
    }
    Ok(a)
}
/// Header coverage is only an index, not proof of supported/complete terrain.
pub fn read_index(directory: &Path, region: [i32; 2], cancel: &dyn Fn() -> bool) -> Result<Index> {
    Ok(stable(directory, region, None, Limits::default(), cancel)?.index)
}
/// None ONLY means the location entry is zero. Missing .mcc, malformed data,
/// unsupported versions/compression, and coordinate mismatches remain errors.
pub fn read_chunk(
    directory: &Path,
    expected: [i32; 2],
    limits: Limits,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<ChunkNbt>> {
    let snapshot = stable(
        directory,
        region_of(expected),
        Some(expected),
        limits,
        cancel,
    )?;
    let Some(stored) = snapshot.stored else {
        return Ok(None);
    };
    let kind = compression(stored.marker)?;
    let bytes = decompress(kind, &stored.data, limits.nbt.max_bytes, cancel)?;
    let document = nbt::parse_checked(&bytes, limits.nbt, cancel)?;
    check(cancel)?;
    let identity = verify_identity(&document, expected)?;
    let entry = snapshot.index.entries[slot(expected)]
        .ok_or(Error::Invalid("payload without index entry"))?;
    Ok(Some(ChunkNbt {
        identity,
        timestamp: entry.timestamp,
        compression: kind,
        external: stored.marker & 128 != 0,
        document,
    }))
}
// The selected world reader uses only host-opened no-follow descriptors. It is
// called on an admitted finite I/O worker after the original DiskRead ticket was
// committed. File names are fixed Anvil leaves, never guest path authority.
#[derive(PartialEq, Eq)]
struct PinnedCapture {
    header: [u8; HEADER],
    file_identity: FileIdentity,
    file_len: u64,
    modified: SystemTime,
    external: Option<(FileIdentity, u64, SystemTime)>,
    index: Index,
    stored: Option<Stored>,
}
fn pinned_read_exact(
    file: &PinnedFile,
    output: &mut [u8],
    offset: u64,
    cancel: &dyn Fn() -> bool,
) -> Result<()> {
    let mut filled = 0usize;
    while filled < output.len() {
        check(cancel)?;
        let at = offset
            .checked_add(filled as u64)
            .ok_or(Error::Limit("read offset"))?;
        let count = file.read_at(&mut output[filled..], at)?;
        if count == 0 {
            return Err(Error::Changed);
        }
        filled += count;
    }
    Ok(())
}
fn pinned_capture(
    directory: &PinnedDirectory,
    expected: [i32; 2],
    limits: Limits,
    cancel: &dyn Fn() -> bool,
) -> Result<PinnedCapture> {
    check(cancel)?;
    let region = region_of(expected);
    let name = format!("r.{}.{}.mca", region[0], region[1]);
    let file = directory.open_file(&name)?;
    let identity = file.identity();
    let file_len = file.len()?;
    let modified = file.modified()?;
    let mut header = [0u8; HEADER];
    pinned_read_exact(&file, &mut header, 0, cancel)?;
    let index = Index::parse(&header, file_len, region)?;
    let mut external_stamp = None;
    let stored = if let Some(entry) = index.entries[slot(expected)] {
        let sector = u64::from(entry.sector)
            .checked_mul(SECTOR)
            .ok_or(Error::Limit("sector offset"))?;
        let mut prefix = [0u8; 5];
        pinned_read_exact(&file, &mut prefix, sector, cancel)?;
        let length = u64::from(word(&prefix[..4]));
        if length < 1 || length + 4 > u64::from(entry.sectors) * SECTOR {
            return Err(Error::Invalid("chunk length outside allocated sectors"));
        }
        let marker = prefix[4];
        compression(marker)?;
        let data = if marker & 128 != 0 {
            if length != 1 || entry.sectors != 1 {
                return Err(Error::Invalid(
                    "external chunk stub must occupy one sector, length 1",
                ));
            }
            let name = format!("c.{}.{}.mcc", expected[0], expected[1]);
            let external = directory.open_file(&name)?;
            let length = external.len()?;
            if length == 0 || length > limits.max_compressed_bytes as u64 {
                return Err(Error::Limit("external compressed bytes"));
            }
            let age = external.modified()?;
            let mut bytes = vec![0u8; length as usize];
            pinned_read_exact(&external, &mut bytes, 0, cancel)?;
            if external.len()? != length || external.modified()? != age {
                return Err(Error::Changed);
            }
            external_stamp = Some((external.identity(), length, age));
            bytes
        } else {
            if length - 1 > limits.max_compressed_bytes as u64 {
                return Err(Error::Limit("internal compressed bytes"));
            }
            let mut bytes = vec![0u8; (length - 1) as usize];
            pinned_read_exact(&file, &mut bytes, sector + 5, cancel)?;
            bytes
        };
        Some(Stored {
            marker,
            data,
            external_stamp: None,
        })
    } else {
        None
    };
    let mut again = [0u8; HEADER];
    pinned_read_exact(&file, &mut again, 0, cancel)?;
    if again != header
        || file.identity() != identity
        || file.len()? != file_len
        || file.modified()? != modified
    {
        return Err(Error::Changed);
    }
    check(cancel)?;
    Ok(PinnedCapture {
        header,
        file_identity: identity,
        file_len,
        modified,
        external: external_stamp,
        index,
        stored,
    })
}
/// Header-only handle-relative allocation observation for a selected region.
/// Two independent opens detect entry swaps and ordinary concurrent writes.
pub fn read_index_pinned(
    directory: &PinnedDirectory,
    region: [i32; 2],
    cancel: &dyn Fn() -> bool,
) -> Result<Index> {
    let capture = || -> Result<(FileIdentity, u64, SystemTime, [u8; HEADER], Index)> {
        check(cancel)?;
        let name = format!("r.{}.{}.mca", region[0], region[1]);
        let file = directory.open_file(&name)?;
        let identity = file.identity();
        let length = file.len()?;
        let modified = file.modified()?;
        let mut header = [0u8; HEADER];
        pinned_read_exact(&file, &mut header, 0, cancel)?;
        let index = Index::parse(&header, length, region)?;
        let mut again = [0u8; HEADER];
        pinned_read_exact(&file, &mut again, 0, cancel)?;
        if header != again || file.len()? != length || file.modified()? != modified {
            return Err(Error::Changed);
        }
        Ok((identity, length, modified, header, index))
    };
    let first = capture()?;
    if first != capture()? {
        return Err(Error::Changed);
    }
    check(cancel)?;
    Ok(first.4)
}

/// Read one qualified chunk from the original selected region descriptor.
/// Two independent opens detect entry swaps and ordinary concurrent writes;
/// no filename, symlink or stale guest identity can replace the selected root.
pub fn read_chunk_pinned(
    region_directory: &PinnedDirectory,
    expected: [i32; 2],
    limits: Limits,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<ChunkNbt>> {
    let first = pinned_capture(region_directory, expected, limits, cancel)?;
    let second = pinned_capture(region_directory, expected, limits, cancel)?;
    if first != second {
        return Err(Error::Changed);
    }
    let Some(stored) = first.stored else {
        return Ok(None);
    };
    let kind = compression(stored.marker)?;
    let bytes = decompress(kind, &stored.data, limits.nbt.max_bytes, cancel)?;
    let document = nbt::parse_checked(&bytes, limits.nbt, cancel)?;
    check(cancel)?;
    let identity = verify_identity(&document, expected)?;
    let timestamp = first.index.entries[slot(expected)]
        .ok_or(Error::Invalid("payload without index entry"))?
        .timestamp;
    Ok(Some(ChunkNbt {
        identity,
        timestamp,
        compression: kind,
        external: stored.marker & 128 != 0,
        document,
    }))
}

fn append(out: &mut Vec<u8>, bytes: &[u8], limit: usize) -> Result<()> {
    if bytes.len() > limit.saturating_sub(out.len()) {
        return Err(Error::Limit("uncompressed bytes"));
    }
    out.try_reserve(bytes.len())
        .map_err(|_| Error::Limit("allocation failed"))?;
    out.extend_from_slice(bytes);
    Ok(())
}
/// Exactly one complete stream, including checksum/trailer; no trailing bytes.
/// The caller also bounds compressed input; read_chunk does so before allocation.
pub fn decompress(
    kind: Compression,
    data: &[u8],
    limit: usize,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<u8>> {
    check(cancel)?;
    let mut out = Vec::new();
    let mut buf = [0; 8192];
    match kind {
        Compression::Raw => append(&mut out, data, limit)?,
        Compression::Gzip => {
            let mut d = GzDecoder::new(data);
            loop {
                check(cancel)?;
                let cap = limit.saturating_sub(out.len()).min(buf.len() - 1) + 1;
                let n = d.read(&mut buf[..cap])?;
                if n == 0 {
                    break;
                }
                append(&mut out, &buf[..n], limit)?;
            }
            if !d.into_inner().is_empty() {
                return Err(Error::Invalid("trailing gzip member/bytes"));
            }
        }
        Compression::Zlib => {
            // Read::read_to_end alone is not proof that zlib reached StreamEnd.
            let mut d = Decompress::new(true);
            loop {
                check(cancel)?;
                let (before_in, before_out) = (d.total_in(), d.total_out());
                let cap = limit.saturating_sub(out.len()).min(buf.len() - 1) + 1;
                let status = d
                    .decompress(
                        &data[before_in as usize..],
                        &mut buf[..cap],
                        FlushDecompress::None,
                    )
                    .map_err(|_| Error::Invalid("invalid zlib stream"))?;
                let n = (d.total_out() - before_out) as usize;
                append(&mut out, &buf[..n], limit)?;
                if status == Status::StreamEnd {
                    if d.total_in() != data.len() as u64 {
                        return Err(Error::Invalid("trailing zlib bytes"));
                    }
                    break;
                }
                if d.total_in() == before_in && n == 0 {
                    return Err(Error::Invalid("truncated or stalled zlib stream"));
                }
            }
        }
    }
    check(cancel)?;
    Ok(out)
}
