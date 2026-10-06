//! Saved allocations are candidate coordinates, never renderable coverage.
use super::region;
use ilium_platform::animation_files::PinnedDirectory;
use std::collections::BTreeSet;
use std::path::Path;

/// Hard ceiling for one save's allocated chunk coordinates.
pub const MAX_ALLOCATED_CHUNKS: usize = 262_144;

#[derive(Debug, Default)]
pub struct AllocationIndex {
    pub chunks: BTreeSet<[i32; 2]>,
    /// Rejected regions stay outside the allocation set. Retain enough detail
    /// for a visible partial-map status without unbounded error history.
    pub issues: Vec<RegionIssue>,
    pub rejected_regions: usize,
}

#[derive(Debug)]
pub struct RegionIssue {
    pub position: [i32; 2],
    pub message: String,
}

pub fn allocated_chunks(
    directory: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<AllocationIndex, region::Error> {
    if cancelled() {
        return Err(region::Error::Cancelled);
    }
    let mut regions = BTreeSet::new();
    for (number, entry) in std::fs::read_dir(directory)?.enumerate() {
        if cancelled() {
            return Err(region::Error::Cancelled);
        }
        if number >= 16384 {
            return Err(region::Error::Limit("region directory entries"));
        }
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let mut parts = name.split('.');
        let (Some("r"), Some(x), Some(z), Some("mca"), None) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            continue;
        };
        let (Ok(x), Ok(z)) = (x.parse::<i32>(), z.parse::<i32>()) else {
            continue;
        };
        if !entry.file_type()?.is_file() {
            continue;
        }
        // Require the canonical spelling used by read_index; alternate strings
        // such as r.+1.0.mca must not redirect a read to r.1.0.mca.
        if name != format!("r.{x}.{z}.mca") {
            continue;
        }
        if x.checked_mul(32).is_none() || z.checked_mul(32).is_none() {
            return Err(region::Error::Invalid(
                "region coordinates exceed chunk domain",
            ));
        }
        regions.insert([x, z]);
        if regions.len() > 4096 {
            return Err(region::Error::Limit("region file count"));
        }
    }
    let mut result = AllocationIndex::default();
    for position in regions {
        let index = match region::read_index(directory, position, cancelled) {
            Ok(index) => index,
            Err(region::Error::Cancelled) => return Err(region::Error::Cancelled),
            Err(error) => {
                result.rejected_regions += 1;
                if result.issues.len() < 64 {
                    result.issues.push(RegionIssue {
                        position,
                        message: error.to_string(),
                    });
                }
                continue;
            }
        };
        let origin = position.map(|coordinate| coordinate * 32);
        for (slot, entry) in index.entries.iter().enumerate() {
            if entry.is_none() {
                continue;
            }
            // The checked region multiplication above also leaves 31 cells
            // before i32::MAX, since the origin is a multiple of 32.
            result.chunks.insert([
                origin[0] + (slot % 32) as i32,
                origin[1] + (slot / 32) as i32,
            ]);
            if result.chunks.len() > MAX_ALLOCATED_CHUNKS {
                return Err(region::Error::Limit("allocated chunk count"));
            }
        }
    }
    Ok(result)
}

/// The selected-root catalog uses the original no-follow directory. The
/// caller holds its committed DiskRead operation and complete scene admission.
/// Allocation is still a candidate index, never renderable coverage.
pub fn allocated_chunks_pinned(
    directory: &PinnedDirectory,
    cancelled: &dyn Fn() -> bool,
) -> Result<AllocationIndex, region::Error> {
    if cancelled() {
        return Err(region::Error::Cancelled);
    }
    let mut regions = BTreeSet::new();
    for entry in directory.list_saved_catalog(16_384)? {
        if cancelled() {
            return Err(region::Error::Cancelled);
        }
        if entry.is_directory {
            continue;
        }
        let name = entry.name;
        let mut parts = name.split('.');
        let (Some("r"), Some(x), Some(z), Some("mca"), None) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            continue;
        };
        let (Ok(x), Ok(z)) = (x.parse::<i32>(), z.parse::<i32>()) else {
            continue;
        };
        if name != format!("r.{x}.{z}.mca") {
            continue;
        }
        if x.checked_mul(32).is_none() || z.checked_mul(32).is_none() {
            return Err(region::Error::Invalid(
                "region coordinates exceed chunk domain",
            ));
        }
        regions.insert([x, z]);
        if regions.len() > 4096 {
            return Err(region::Error::Limit("region file count"));
        }
    }
    let mut result = AllocationIndex::default();
    for position in regions {
        if cancelled() {
            return Err(region::Error::Cancelled);
        }
        let index = match region::read_index_pinned(directory, position, cancelled) {
            Ok(index) => index,
            Err(region::Error::Cancelled) => return Err(region::Error::Cancelled),
            Err(error) => {
                result.rejected_regions += 1;
                if result.issues.len() < 64 {
                    result.issues.push(RegionIssue {
                        position,
                        message: error.to_string(),
                    });
                }
                continue;
            }
        };
        let origin = position.map(|coordinate| coordinate * 32);
        for (slot, entry) in index.entries.iter().enumerate() {
            if entry.is_none() {
                continue;
            }
            result.chunks.insert([
                origin[0] + (slot % 32) as i32,
                origin[1] + (slot / 32) as i32,
            ]);
            if result.chunks.len() > MAX_ALLOCATED_CHUNKS {
                return Err(region::Error::Limit("allocated chunk count"));
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(directory: &Path, coordinates: [i32; 2], slots: &[usize]) {
        let mut bytes = vec![0; (2 + slots.len()) * 4096];
        for (index, slot) in slots.iter().enumerate() {
            bytes[slot * 4..slot * 4 + 4]
                .copy_from_slice(&(((index as u32 + 2) << 8) | 1).to_be_bytes());
        }
        std::fs::write(
            directory.join(format!("r.{}.{}.mca", coordinates[0], coordinates[1])),
            bytes,
        )
        .unwrap();
    }

    #[test]
    fn indexes_negative_coordinates_and_never_decodes_or_generates_payloads() {
        let directory = tempfile::tempdir().unwrap();
        file(directory.path(), [-2, 1], &[0, 31, 32, 1023]);
        file(directory.path(), [0, -1], &[0]);
        std::fs::write(directory.path().join("unrelated.mca"), b"untouched").unwrap();
        let chunks = allocated_chunks(directory.path(), &|| false).unwrap();
        assert!(chunks.issues.is_empty());
        assert_eq!(
            chunks.chunks,
            [[-64, 32], [-33, 32], [-64, 33], [-33, 63], [0, -32]].into()
        );
        // A header allocation says nothing about the deliberately invalid body.
        assert!(
            region::read_chunk(directory.path(), [-64, 32], Default::default(), &|| false).is_err()
        );
        assert_eq!(
            std::fs::read(directory.path().join("unrelated.mca")).unwrap(),
            b"untouched"
        );
    }

    #[test]
    fn surfaces_corruption_and_cancellation_instead_of_empty_success() {
        let directory = tempfile::tempdir().unwrap();
        file(directory.path(), [0, 0], &[0]);
        assert!(matches!(
            allocated_chunks(directory.path(), &|| true),
            Err(region::Error::Cancelled)
        ));
        std::fs::write(directory.path().join("r.1.0.mca"), b"bad").unwrap();
        let partial = allocated_chunks(directory.path(), &|| false).unwrap();
        assert_eq!(partial.chunks, [[0, 0]].into());
        assert_eq!(partial.rejected_regions, 1);
        assert_eq!(partial.issues[0].position, [1, 0]);
        assert!(!partial.issues[0].message.is_empty());
    }

    #[test]
    fn rejects_region_coordinates_that_cannot_represent_chunk_coordinates() {
        let directory = tempfile::tempdir().unwrap();
        file(directory.path(), [i32::MAX, 0], &[0]);
        assert!(allocated_chunks(directory.path(), &|| false).is_err());
    }

    #[test]
    fn ignores_noncanonical_names_and_directories_but_empty_cancel_is_explicit() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("r.+1.0.mca"), b"bad").unwrap();
        std::fs::write(directory.path().join("r.01.0.mca"), b"bad").unwrap();
        std::fs::create_dir(directory.path().join("r.1.0.mca")).unwrap();
        assert!(
            allocated_chunks(directory.path(), &|| false)
                .unwrap()
                .chunks
                .is_empty()
        );
        let empty = tempfile::tempdir().unwrap();
        assert!(matches!(
            allocated_chunks(empty.path(), &|| true),
            Err(region::Error::Cancelled)
        ));
    }

    #[test]
    fn partial_error_history_is_bounded_without_losing_rejection_count() {
        let directory = tempfile::tempdir().unwrap();
        for x in 0..80 {
            std::fs::write(directory.path().join(format!("r.{x}.0.mca")), b"bad").unwrap();
        }
        let result = allocated_chunks(directory.path(), &|| false).unwrap();
        assert!(result.chunks.is_empty());
        assert_eq!(result.rejected_regions, 80);
        assert_eq!(result.issues.len(), 64);
        assert_eq!(result.issues[0].position, [0, 0]);
        assert_eq!(result.issues[63].position, [63, 0]);
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn pinned_allocation_reads_original_directory_after_its_path_is_replaced() {
        use ilium_platform::{animation_files::PinnedDirectory, secure_fs::NoFollowDirectory};
        use std::sync::Arc;
        let temporary = tempfile::tempdir().unwrap();
        let selected = temporary.path().join("region");
        std::fs::create_dir(&selected).unwrap();
        file(&selected, [-2, 1], &[0, 31, 32, 1023]);
        let original =
            PinnedDirectory::from_host(Arc::new(NoFollowDirectory::open_root(&selected).unwrap()))
                .unwrap();
        std::fs::rename(&selected, temporary.path().join("retired-region")).unwrap();
        std::fs::create_dir(&selected).unwrap();
        file(&selected, [1, 2], &[0]);
        let retained = allocated_chunks_pinned(&original, &|| false).unwrap();
        assert!(retained.issues.is_empty());
        assert_eq!(
            retained.chunks,
            [[-64, 32], [-33, 32], [-64, 33], [-33, 63]].into(),
            "a retained original directory must never reopen its replaced path label"
        );
        let replacement = allocated_chunks(&selected, &|| false).unwrap();
        assert_eq!(replacement.chunks, [[32, 64]].into());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn pinned_allocation_keeps_bounded_partial_errors_and_explicit_cancellation() {
        use ilium_platform::{animation_files::PinnedDirectory, secure_fs::NoFollowDirectory};
        use std::sync::Arc;
        let temporary = tempfile::tempdir().unwrap();
        file(temporary.path(), [0, 0], &[0]);
        for x in 1..=80 {
            std::fs::write(temporary.path().join(format!("r.{x}.0.mca")), b"bad").unwrap();
        }
        let original = PinnedDirectory::from_host(Arc::new(
            NoFollowDirectory::open_root(temporary.path()).unwrap(),
        ))
        .unwrap();
        assert!(matches!(
            allocated_chunks_pinned(&original, &|| true),
            Err(region::Error::Cancelled)
        ));
        let result = allocated_chunks_pinned(&original, &|| false).unwrap();
        assert_eq!(result.chunks, [[0, 0]].into());
        assert_eq!(result.rejected_regions, 80);
        assert_eq!(result.issues.len(), 64);
        assert_eq!(result.issues[0].position, [1, 0]);
        assert_eq!(result.issues[63].position, [64, 0]);
    }
}
