//! Directory packs are read once at mount into a stored ZIP, then served through
//! the same immutable `ZipSource` path as a selected archive. Per-tile reads never
//! touch the live folder, so a pack edited mid-route cannot change the definitions
//! a route already uses. The snapshot digest is the identity of what was read.
use super::archive::{crc32, ZipSource};
use super::budget::{ByteBudget, Cancel};
use super::error::{AssetError, Result};
use super::source::{AssetSource, LocalTree, SourceBytes, SourceLimits};

/// MS-DOS date 1980-01-01 and time 00:00. Fixed timestamps keep the snapshot
/// bytes, and therefore its digest, a pure function of the pack content.
const FIXED_DOS_DATE: u16 = 0x0021;
const FIXED_DOS_TIME: u16 = 0;
const UTF8_NAME_FLAG: u16 = 0x0800;
const STORED: u16 = 0;
const ZIP_VERSION: u16 = 20;

struct CentralRecord {
    name: Vec<u8>,
    crc: u32,
    size: u32,
    local_offset: u32,
}

fn checked_u32(value: usize, what: &str) -> Result<u32> {
    u32::try_from(value)
        .map_err(|_| AssetError::InvalidMetadata(format!("directory snapshot {what} too large")))
}

fn write_local_header(archive: &mut Vec<u8>, name: &[u8], crc: u32, size: u32) -> Result<()> {
    let name_length = u16::try_from(name.len())
        .map_err(|_| AssetError::InvalidMetadata("directory snapshot name too long".into()))?;
    archive.extend_from_slice(&0x0403_4b50_u32.to_le_bytes());
    archive.extend_from_slice(&ZIP_VERSION.to_le_bytes());
    archive.extend_from_slice(&UTF8_NAME_FLAG.to_le_bytes());
    archive.extend_from_slice(&STORED.to_le_bytes());
    archive.extend_from_slice(&FIXED_DOS_TIME.to_le_bytes());
    archive.extend_from_slice(&FIXED_DOS_DATE.to_le_bytes());
    archive.extend_from_slice(&crc.to_le_bytes());
    archive.extend_from_slice(&size.to_le_bytes());
    archive.extend_from_slice(&size.to_le_bytes());
    archive.extend_from_slice(&name_length.to_le_bytes());
    archive.extend_from_slice(&0_u16.to_le_bytes());
    archive.extend_from_slice(name);
    Ok(())
}

fn write_central_record(central: &mut Vec<u8>, record: &CentralRecord) -> Result<()> {
    let name_length = u16::try_from(record.name.len())
        .map_err(|_| AssetError::InvalidMetadata("directory snapshot name too long".into()))?;
    central.extend_from_slice(&0x0201_4b50_u32.to_le_bytes());
    central.extend_from_slice(&ZIP_VERSION.to_le_bytes());
    central.extend_from_slice(&ZIP_VERSION.to_le_bytes());
    central.extend_from_slice(&UTF8_NAME_FLAG.to_le_bytes());
    central.extend_from_slice(&STORED.to_le_bytes());
    central.extend_from_slice(&FIXED_DOS_TIME.to_le_bytes());
    central.extend_from_slice(&FIXED_DOS_DATE.to_le_bytes());
    central.extend_from_slice(&record.crc.to_le_bytes());
    central.extend_from_slice(&record.size.to_le_bytes());
    central.extend_from_slice(&record.size.to_le_bytes());
    central.extend_from_slice(&name_length.to_le_bytes());
    central.extend_from_slice(&0_u16.to_le_bytes()); // extra length
    central.extend_from_slice(&0_u16.to_le_bytes()); // comment length
    central.extend_from_slice(&0_u16.to_le_bytes()); // disk number start
    central.extend_from_slice(&0_u16.to_le_bytes()); // internal attributes
    central.extend_from_slice(&0_u32.to_le_bytes()); // external attributes
    central.extend_from_slice(&record.local_offset.to_le_bytes());
    central.extend_from_slice(&record.name);
    Ok(())
}

/// Reads every member of `tree` once and returns the whole pack as a stored ZIP
/// behind `ZipSource`. The snapshot archive obeys the same 256 MiB archive cap as
/// a selected ZIP, so an oversize folder fails the mount with a limit error
/// rather than being read partially. Any unreadable member also fails the mount:
/// a half-read pack must never serve definitions.
pub(crate) fn snapshot_directory(
    tree: &LocalTree,
    budget: &ByteBudget,
    cancel: Cancel<'_>,
) -> Result<(ZipSource, super::identity::Digest256)> {
    let limits = SourceLimits::default();
    let mut archive = Vec::new();
    let mut central_records = Vec::new();
    for path in tree.members().keys() {
        cancel.check()?;
        let member: SourceBytes = tree.read(path, u64::MAX, budget, cancel)?.ok_or_else(|| {
            AssetError::InvalidMetadata(format!(
                "directory snapshot member vanished: {}",
                path.as_str()
            ))
        })?;
        let bytes = member.bytes();
        let size = checked_u32(bytes.len(), "member")?;
        let crc = crc32(bytes, cancel)?;
        let name = path.as_str().as_bytes().to_vec();
        let local_offset = checked_u32(archive.len(), "archive offset")?;
        write_local_header(&mut archive, &name, crc, size)?;
        archive.extend_from_slice(bytes);
        central_records.push(CentralRecord {
            name,
            crc,
            size,
            local_offset,
        });
    }
    let central_offset = checked_u32(archive.len(), "central directory offset")?;
    for record in &central_records {
        write_central_record(&mut archive, record)?;
    }
    let central_size = checked_u32(archive.len(), "central directory")? - central_offset;
    let entry_count = u16::try_from(central_records.len())
        .map_err(|_| AssetError::InvalidMetadata("directory snapshot has too many files".into()))?;
    archive.extend_from_slice(&0x0605_4b50_u32.to_le_bytes());
    archive.extend_from_slice(&0_u16.to_le_bytes()); // disk number
    archive.extend_from_slice(&0_u16.to_le_bytes()); // central directory disk
    archive.extend_from_slice(&entry_count.to_le_bytes());
    archive.extend_from_slice(&entry_count.to_le_bytes());
    archive.extend_from_slice(&central_size.to_le_bytes());
    archive.extend_from_slice(&central_offset.to_le_bytes());
    archive.extend_from_slice(&0_u16.to_le_bytes()); // comment length
    let original = SourceBytes::from_slice(&archive, limits.archive_bytes, budget, cancel)?;
    drop(archive);
    let digest = original.digest();
    let source = ZipSource::open(original, None, limits, budget, cancel)?;
    Ok((source, digest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenes::voxel_landscape::assets::identity::AssetPath;
    use std::sync::atomic::AtomicBool;

    fn write_pack(directory: &std::path::Path, files: &[(&str, &[u8])]) {
        for (name, bytes) in files {
            let path = directory.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
    }

    #[test]
    fn snapshot_round_trips_every_member_and_survives_source_edits() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("pack");
        std::fs::create_dir(&directory).unwrap();
        write_pack(
            &directory,
            &[
                ("assets/minecraft/models/stone.json", b"{\"a\":1}"),
                ("pack.mcmeta", b"{}"),
            ],
        );
        let budget = ByteBudget::new(64 << 20).unwrap();
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let tree = LocalTree::open(
            &directory,
            Default::default(),
            SourceLimits::default(),
            &budget,
            cancel,
        )
        .unwrap();
        let (snapshot, digest) = snapshot_directory(&tree, &budget, cancel).unwrap();
        // The live folder changes after the snapshot; the snapshot must not.
        std::fs::write(directory.join("pack.mcmeta"), b"CHANGED").unwrap();
        let path = AssetPath::parse("pack.mcmeta").unwrap();
        let read = snapshot
            .read(&path, u64::MAX, &budget, cancel)
            .unwrap()
            .unwrap();
        assert_eq!(read.bytes(), b"{}");
        assert_eq!(snapshot.source_digest(), Some(digest));
        assert_eq!(snapshot.members().len(), 2);
        drop(read);
        drop(snapshot);
        drop(tree);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn snapshot_digest_is_a_pure_function_of_pack_content() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        for directory in [&first, &second] {
            std::fs::create_dir(directory).unwrap();
            write_pack(directory, &[("pack.mcmeta", b"{}")]);
        }
        let budget = ByteBudget::new(64 << 20).unwrap();
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let open = |path: &std::path::Path| {
            LocalTree::open(
                path,
                Default::default(),
                SourceLimits::default(),
                &budget,
                cancel,
            )
            .unwrap()
        };
        let (_, first_digest) = snapshot_directory(&open(&first), &budget, cancel).unwrap();
        let (_, second_digest) = snapshot_directory(&open(&second), &budget, cancel).unwrap();
        assert_eq!(first_digest, second_digest);
    }
}
