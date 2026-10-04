//! Blocking, bounded saved-window loading for an owned preparation worker.
use super::{chunk, coverage::Coverage, region};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

const MAX_CHUNKS: usize = 128;
const MAX_SECTIONS: usize = 32;
const MAX_STORAGE_CHARGE: usize = 128 << 20;
// The projected route source is a separately admitted, bounded read. Catalog
// windows retain their original 128-chunk and 32-MiB defaults.
pub const MAX_PROJECTED_CHUNKS: usize = 512;
pub const MAX_PROJECTED_STORAGE_CHARGE: usize = 192 << 20;

/// Chunk-coordinate request around a Minecraft [x, z] block anchor. This only
/// selects coordinates; load_window must independently qualify saved coverage.
pub fn requested_square(center: [i32; 2], radius_chunks: u8) -> Result<BTreeSet<[i32; 2]>, Error> {
    let side = usize::from(radius_chunks) * 2 + 1;
    if side * side > MAX_CHUNKS {
        return Err(Error::Limit);
    }
    let center = center.map(|coordinate| coordinate.div_euclid(16));
    let radius = i32::from(radius_chunks);
    let mut requested = BTreeSet::new();
    for z in center[1] - radius..=center[1] + radius {
        for x in center[0] - radius..=center[0] + radius {
            let position = [x, z];
            if !addressable(position) {
                return Err(Error::InvalidCoordinates);
            }
            requested.insert(position);
        }
    }
    Ok(requested)
}

fn addressable(position: [i32; 2]) -> bool {
    position.into_iter().all(|coordinate| {
        coordinate
            .checked_mul(16)
            .and_then(|minimum| minimum.checked_add(15))
            .is_some()
    })
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_chunks: usize,
    pub min_section: i8,
    pub max_section: i8,
    /// Byte-denominated retained-storage charge, not allocator/RSS bytes.
    /// Excludes transient NBT/decompression and external caller-held snapshots.
    pub max_storage_charge: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_chunks: 128,
            min_section: -4,
            max_section: 19,
            max_storage_charge: 32 << 20,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("saved window exceeds chunk limit")]
    Limit,
    #[error("invalid saved-window section range")]
    InvalidSections,
    #[error("chunk coordinate cannot be addressed by signed block coordinates")]
    InvalidCoordinates,
    #[error("saved-window loading cancelled")]
    Cancelled,
    #[error("saved window exceeds retained-storage charge limit")]
    StorageLimit,
}

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error(transparent)]
    Region(#[from] region::Error),
    #[error(transparent)]
    Chunk(#[from] chunk::Error),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rejection {
    Absent,
    ProtoChunk,
    IncompleteSections,
    CoordinateMismatch,
    Read(String),
}
#[derive(Debug)]
pub struct Issue {
    pub position: [i32; 2],
    pub reason: Rejection,
}
#[derive(Debug, Default)]
pub struct LoadedWindow {
    pub chunks: BTreeMap<[i32; 2], Arc<chunk::DecodedChunk>>,
    pub coverage: Coverage,
    pub rejected_chunks: usize,
    pub issues: Vec<Issue>,
    /// Conservative retained charge computed by the loader before publication.
    pub(crate) retained_storage_charge: usize,
}

/// Runs blocking filesystem/decompression work; call only from the preparation
/// worker. A rejected coordinate remains missing, never generated or air.
pub fn load_window(
    region_directory: &Path,
    requested: &BTreeSet<[i32; 2]>,
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
) -> Result<LoadedWindow, Error> {
    load_window_with_ceiling(
        region_directory,
        requested,
        limits,
        [MAX_CHUNKS, MAX_STORAGE_CHARGE],
        cancelled,
    )
}

/// A route's complete inverse-projected source, including the native biome
/// and adjacent-state support ring. The caller must hold a reservation for the
/// full projected storage ceiling while this function runs and must require
/// `loaded.coverage.chunks == requested` before accepting any output. Missing,
/// rejected, or over-limit chunks cannot be turned into synthetic air.
pub fn load_projected_window(
    region_directory: &Path,
    requested: &BTreeSet<[i32; 2]>,
    cancelled: &dyn Fn() -> bool,
) -> Result<LoadedWindow, Error> {
    load_window_with_ceiling(
        region_directory,
        requested,
        Limits {
            max_chunks: MAX_PROJECTED_CHUNKS,
            max_storage_charge: MAX_PROJECTED_STORAGE_CHARGE,
            ..Limits::default()
        },
        [MAX_PROJECTED_CHUNKS, MAX_PROJECTED_STORAGE_CHARGE],
        cancelled,
    )
}

fn load_window_with_ceiling(
    region_directory: &Path,
    requested: &BTreeSet<[i32; 2]>,
    limits: Limits,
    ceiling: [usize; 2],
    cancelled: &dyn Fn() -> bool,
) -> Result<LoadedWindow, Error> {
    // The aggregate chunk count and per-chunk decoded collection limits bound
    // retained output independently of the much larger allocation inventory.
    let decode_limits = chunk::Limits {
        max_sections: MAX_SECTIONS,
        max_palette_entries: 8192,
        max_properties: 8192,
        max_text_units: 262144,
    };
    load_with_ceiling(requested, limits, ceiling, cancelled, |position| {
        let Some(stored) = region::read_chunk(
            region_directory,
            position,
            region::Limits::default(),
            cancelled,
        )?
        else {
            return Ok(None);
        };
        Ok(Some(chunk::decode(
            &stored.document,
            position,
            decode_limits,
            cancelled,
        )?))
    })
}

#[cfg(test)]
pub(crate) fn load_with(
    requested: &BTreeSet<[i32; 2]>,
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
    read: impl FnMut([i32; 2]) -> Result<Option<chunk::DecodedChunk>, ReadError>,
) -> Result<LoadedWindow, Error> {
    load_with_ceiling(
        requested,
        limits,
        [MAX_CHUNKS, MAX_STORAGE_CHARGE],
        cancelled,
        read,
    )
}

fn load_with_ceiling(
    requested: &BTreeSet<[i32; 2]>,
    limits: Limits,
    ceiling: [usize; 2],
    cancelled: &dyn Fn() -> bool,
    mut read: impl FnMut([i32; 2]) -> Result<Option<chunk::DecodedChunk>, ReadError>,
) -> Result<LoadedWindow, Error> {
    if cancelled() {
        return Err(Error::Cancelled);
    }
    if limits.max_storage_charge == 0 || limits.max_storage_charge > ceiling[1] {
        return Err(Error::StorageLimit);
    }
    if limits.max_chunks == 0
        || limits.max_chunks > ceiling[0]
        || requested.len() > limits.max_chunks
    {
        return Err(Error::Limit);
    }
    let section_count = i16::from(limits.max_section) - i16::from(limits.min_section) + 1;
    if !(1..=MAX_SECTIONS as i16).contains(&section_count) {
        return Err(Error::InvalidSections);
    }
    if requested.iter().any(|position| !addressable(*position)) {
        return Err(Error::InvalidCoordinates);
    }
    let mut loaded = LoadedWindow::default();
    // Reserve the independently bounded diagnostic vector/string allowance,
    // including 256 four-byte Unicode characters per retained read issue.
    let mut storage_charge =
        std::mem::size_of::<LoadedWindow>() + 64 * (std::mem::size_of::<Issue>() + 4 * 256);
    if storage_charge > limits.max_storage_charge {
        return Err(Error::StorageLimit);
    }
    for &position in requested {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let result = read(position);
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let reason = match result {
            Ok(Some(decoded)) => {
                if decoded.identity.position != position {
                    Rejection::CoordinateMismatch
                } else if !decoded.is_full() {
                    Rejection::ProtoChunk
                } else if !decoded.has_full_coverage(limits.min_section, limits.max_section) {
                    Rejection::IncompleteSections
                } else {
                    // Retain no partial window on exhaustion. One decoded chunk
                    // and its NBT transient can exist before this admission gate;
                    // their existing per-chunk limits remain independent bounds.
                    let charge = match decoded.storage_charge(cancelled) {
                        Ok(charge) => charge,
                        Err(chunk::Error::Cancelled) => return Err(Error::Cancelled),
                        Err(_) => return Err(Error::StorageLimit),
                    };
                    // Arc counters plus conservative tree/set bookkeeping per
                    // coordinate. This is a logical charge, not heap measurement.
                    let overhead = 16
                        * (std::mem::size_of::<[i32; 2]>()
                            + std::mem::size_of::<Arc<chunk::DecodedChunk>>()
                            + 2 * std::mem::size_of::<usize>());
                    storage_charge = storage_charge
                        .checked_add(charge)
                        .and_then(|bytes| bytes.checked_add(overhead))
                        .filter(|&bytes| bytes <= limits.max_storage_charge)
                        .ok_or(Error::StorageLimit)?;
                    loaded.chunks.insert(position, Arc::new(decoded));
                    loaded.coverage.chunks.insert(position);
                    continue;
                }
            }
            Ok(None) => Rejection::Absent,
            Err(
                ReadError::Region(region::Error::Cancelled)
                | ReadError::Chunk(
                    chunk::Error::Cancelled | chunk::Error::Region(region::Error::Cancelled),
                ),
            ) => {
                return Err(Error::Cancelled);
            }
            Err(
                ReadError::Region(region::Error::Nbt(error))
                | ReadError::Chunk(chunk::Error::Region(region::Error::Nbt(error))),
            ) if error.reason == "cancelled" => {
                // Region parsing preserves the NBT parser's cancellation error.
                // A one-shot callback need not still be true after the read.
                return Err(Error::Cancelled);
            }
            Err(error) => Rejection::Read(error.to_string().chars().take(256).collect()),
        };
        loaded.rejected_chunks += 1;
        if loaded.issues.len() < 64 {
            loaded.issues.push(Issue { position, reason });
        }
    }
    loaded.retained_storage_charge = storage_charge;
    Ok(loaded)
}

#[cfg(test)]
#[path = "loader_tests.rs"]
mod tests;
