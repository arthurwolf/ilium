//! Strict RAM-only archive loading. Listing validates bytes but never evaluates modules.
use crate::{
    error::{AnimationError, Result},
    manifest::Manifest,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read, Seek, SeekFrom},
};

#[derive(Debug, Clone, Copy)]
pub struct PackageLimits {
    pub archive_bytes: u64,
    pub expanded_bytes: u64,
    pub file_bytes: u64,
    pub entries: usize,
    pub compression_ratio: u64,
}
impl Default for PackageLimits {
    fn default() -> Self {
        Self {
            archive_bytes: 32 * 1024 * 1024,
            expanded_bytes: 64 * 1024 * 1024,
            file_bytes: 16 * 1024 * 1024,
            entries: 256,
            compression_ratio: 100,
        }
    }
}
#[derive(Debug, Clone)]
pub struct Package {
    manifest: Manifest,
    files: BTreeMap<String, Vec<u8>>,
    /// Exact compressed archive identity; ZIP repacking changes this digest.
    digest: String,
    archive_hash: [u8; 32],
    archive_length: usize,
}
/// Portable names with no ambiguity across Windows and Unix import resolvers.
pub fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 240
        && !path.contains(['\\', ':', '\0'])
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !reserved_name(part)
                && !part.ends_with(['.', ' '])
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
}
fn reserved_name(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
    matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || (stem.len() == 4
        && (stem.starts_with("COM") || stem.starts_with("LPT"))
        && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}
/// Descriptor inspection never decompresses modules/assets and confers no integrity/trust.
/// Activation must subsequently call Package::from_bytes on the exact chosen bytes.
pub fn inspect_manifest(bytes: &[u8], limits: PackageLimits) -> Result<Manifest> {
    inspect_manifest_reader(Cursor::new(bytes), bytes.len() as u64, limits)
}
/// Generic caller-owned bounded archive reader; this module performs no filesystem I/O.
pub fn inspect_manifest_reader<R: Read + Seek>(
    mut reader: R,
    archive_bytes: u64,
    limits: PackageLimits,
) -> Result<Manifest> {
    if archive_bytes > limits.archive_bytes {
        return Err(AnimationError::Budget("archive bytes".into()));
    }
    let end = archive_bytes
        .checked_sub(22)
        .ok_or_else(|| AnimationError::InvalidPackage("truncated ZIP".into()))?;
    let mut footer = [0_u8; 22];
    reader.seek(SeekFrom::Start(end))?;
    reader.read_exact(&mut footer)?;
    let count = directory_count(&footer, end, limits)?;
    reader.seek(SeekFrom::Start(0))?;
    let mut archive = zip::ZipArchive::new(reader)?;
    if archive.len() != count {
        return Err(AnimationError::InvalidPackage(
            "duplicate ZIP entries".into(),
        ));
    }
    let file = archive.by_name("manifest.json")?;
    if file.size() > 256 * 1024
        || file.size() > limits.file_bytes
        || file.size()
            > file
                .compressed_size()
                .max(1)
                .saturating_mul(limits.compression_ratio)
    {
        return Err(AnimationError::Budget("manifest bytes".into()));
    }
    if file.encrypted() || file.is_symlink() || !file.is_file() {
        return Err(AnimationError::InvalidPackage(
            "unsafe manifest entry".into(),
        ));
    }
    let expected = file.size();
    let mut data = Vec::new();
    file.take(256 * 1024 + 1).read_to_end(&mut data)?;
    if data.len() as u64 != expected {
        return Err(AnimationError::Integrity("manifest size".into()));
    }
    let manifest: Manifest = serde_json::from_slice(&data)?;
    manifest.validate()?;
    Ok(manifest)
}
/// ZIP32 bounded preflight precedes library metadata allocation. zip's IndexMap
/// collapses duplicate names, so its len must also equal the original EOCD count.
fn directory_count(footer: &[u8], end: u64, limits: PackageLimits) -> Result<usize> {
    let word = |offset: usize| u16::from_le_bytes([footer[offset], footer[offset + 1]]);
    let dword = |offset: usize| {
        u32::from_le_bytes([
            footer[offset],
            footer[offset + 1],
            footer[offset + 2],
            footer[offset + 3],
        ])
    };
    let count = word(10) as usize;
    if dword(0) != 0x0605_4b50
        || word(4) != 0
        || word(6) != 0
        || word(8) != word(10)
        || word(20) != 0
        || count == u16::MAX as usize
        || dword(12) as u64 + dword(16) as u64 != end
    {
        return Err(AnimationError::InvalidPackage(
            "noncanonical ZIP32 directory".into(),
        ));
    }
    if dword(12) as u64 > (count as u64).saturating_mul(512) {
        return Err(AnimationError::Budget("ZIP metadata bytes".into()));
    }
    if count > limits.entries {
        return Err(AnimationError::Budget("entry count".into()));
    }
    Ok(count)
}
impl Package {
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    pub fn files(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.files
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    /// Native trust uses the loader-bound raw archive digest, not manifest claims.
    pub(crate) fn archive_hash(&self) -> [u8; 32] {
        self.archive_hash
    }
    /// Reject length first so an unrelated unbounded input is never hashed.
    pub(crate) fn archive_matches(&self, bytes: &[u8]) -> bool {
        bytes.len() == self.archive_length
            && <[u8; 32]>::from(Sha256::digest(bytes)) == self.archive_hash
    }
    pub fn module_source(&self, path: &str) -> Result<&str> {
        if !valid_path(path)
            || !(path == self.manifest.entry
                || (path.starts_with("modules/") && path.ends_with(".mjs")))
        {
            return Err(AnimationError::InvalidPackage("module path".into()));
        }
        let bytes = self
            .files
            .get(path)
            .ok_or_else(|| AnimationError::Integrity("missing module".into()))?;
        std::str::from_utf8(bytes)
            .map_err(|_| AnimationError::InvalidPackage("module is not UTF8".into()))
    }

    pub fn from_bytes(bytes: &[u8], limits: PackageLimits) -> Result<Self> {
        if bytes.len() as u64 > limits.archive_bytes {
            return Err(AnimationError::Budget("archive bytes".into()));
        }
        let end = bytes
            .len()
            .checked_sub(22)
            .ok_or_else(|| AnimationError::InvalidPackage("truncated ZIP".into()))?;
        let entry_count = directory_count(&bytes[end..], end as u64, limits)?;
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
        if archive.len() != entry_count {
            return Err(AnimationError::InvalidPackage(
                "duplicate ZIP entries".into(),
            ));
        }

        if archive.len() > limits.entries {
            return Err(AnimationError::Budget("entry count".into()));
        }
        let mut files = BTreeMap::new();
        let mut folded = BTreeSet::new();
        let mut expanded = 0_u64;
        for index in 0..archive.len() {
            let mut file = archive.by_index(index)?;
            let path = file.name().to_owned();
            if !valid_path(&path)
                || file.name_raw() != path.as_bytes()
                || file.enclosed_name().is_none()
                || !file.is_file()
                || file.is_symlink()
                || file.encrypted()
            {
                return Err(AnimationError::InvalidPackage(format!(
                    "unsafe entry {path:?}"
                )));
            }
            if !folded.insert(path.to_ascii_lowercase()) {
                return Err(AnimationError::InvalidPackage(
                    "duplicate/case-colliding entry".into(),
                ));
            }
            if !matches!(
                file.compression(),
                zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated
            ) {
                return Err(AnimationError::InvalidPackage(
                    "unsupported compression".into(),
                ));
            }
            expanded = expanded
                .checked_add(file.size())
                .ok_or_else(|| AnimationError::Budget("expanded overflow".into()))?;
            if expanded > limits.expanded_bytes
                || file.size() > limits.file_bytes
                || file.size()
                    > file
                        .compressed_size()
                        .max(1)
                        .saturating_mul(limits.compression_ratio)
            {
                return Err(AnimationError::Budget(format!("expanded entry {path}")));
            }
            let declared_size = file.size();
            let mut data = Vec::new();
            file.by_ref()
                .take(limits.file_bytes.saturating_add(1))
                .read_to_end(&mut data)?;
            if data.len() as u64 != declared_size {
                return Err(AnimationError::Integrity(format!("size {path}")));
            }
            files.insert(path, data);
        }
        let metadata = files
            .remove("manifest.json")
            .ok_or_else(|| AnimationError::InvalidPackage("missing manifest".into()))?;
        if metadata.len() > 256 * 1024 {
            return Err(AnimationError::Budget("manifest bytes".into()));
        }
        let manifest: Manifest = serde_json::from_slice(&metadata)?;
        manifest.validate()?;
        let mut inventory = BTreeSet::new();
        for item in &manifest.files {
            if !valid_path(&item.path)
                || item.path == "manifest.json"
                || !inventory.insert(item.path.clone())
            {
                return Err(AnimationError::InvalidPackage(
                    "unsafe/duplicate inventory".into(),
                ));
            }
            let data = files
                .get(&item.path)
                .ok_or_else(|| AnimationError::Integrity(format!("missing {}", item.path)))?;
            if data.len() as u64 != item.bytes
                || format!("{:x}", Sha256::digest(data)) != item.sha256
            {
                return Err(AnimationError::Integrity(item.path.clone()));
            }
            if item.path != manifest.entry
                && !item.path.starts_with("assets/")
                && !(item.path.starts_with("modules/") && item.path.ends_with(".mjs"))
            {
                return Err(AnimationError::InvalidPackage(format!(
                    "unrecognized entry {}",
                    item.path
                )));
            }
        }
        if inventory.len() != files.len() || !inventory.contains(&manifest.entry) {
            return Err(AnimationError::Integrity(
                "unlisted file or missing entry".into(),
            ));
        }
        let expected_assets: Vec<_> = manifest
            .files
            .iter()
            .filter(|item| item.path.starts_with("assets/"))
            .collect();
        let actual_assets: BTreeMap<_, _> = manifest
            .assets
            .iter()
            .map(|item| (&item.path, item))
            .collect();
        if actual_assets.len() != manifest.assets.len()
            || expected_assets.len() != actual_assets.len()
            || expected_assets
                .iter()
                .any(|item| actual_assets.get(&item.path).copied() != Some(*item))
        {
            return Err(AnimationError::Integrity("asset inventory mismatch".into()));
        }
        for (name, data) in &files {
            if name.ends_with(".mjs") && std::str::from_utf8(data).is_err() {
                return Err(AnimationError::InvalidPackage("module is not UTF8".into()));
            }
        }
        let archive_digest = Sha256::digest(bytes);
        let digest = format!("{archive_digest:x}");
        Ok(Self {
            manifest,
            files,
            digest,
            archive_hash: archive_digest.into(),
            archive_length: bytes.len(),
        })
    }
    pub fn entry_source(&self) -> Result<&str> {
        let bytes = self
            .files
            .get(&self.manifest.entry)
            .ok_or_else(|| AnimationError::Integrity("missing entry".into()))?;
        std::str::from_utf8(bytes)
            .map_err(|_| AnimationError::InvalidPackage("entry is not UTF8".into()))
    }
    pub fn asset(&self, path: &str) -> Result<&[u8]> {
        if !path.starts_with("assets/") || !valid_path(path) {
            return Err(AnimationError::InvalidPackage("asset path".into()));
        }
        self.files
            .get(path)
            .map(Vec::as_slice)
            .ok_or_else(|| AnimationError::InvalidPackage("missing asset".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (path, data) in entries {
            writer
                .start_file(
                    *path,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Stored),
                )
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }
    fn metadata(source: &[u8]) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"api_version":1,"id":"test","name":"Test","version":"1.0.0","entry":"entry.mjs","modes":["live"],"files":[{"path":"entry.mjs","bytes":source.len(),"sha256":format!("{:x}",Sha256::digest(source))}]})).unwrap()
    }
    #[test]
    fn accepts_verified_ram_package() {
        let source = b"export function plan() {}";
        let meta = metadata(source);
        let package = Package::from_bytes(
            &archive(&[("manifest.json", &meta), ("entry.mjs", source)]),
            PackageLimits::default(),
        )
        .unwrap();
        assert_eq!(
            package.entry_source().unwrap(),
            std::str::from_utf8(source).unwrap()
        );
    }
    #[test]
    fn loaded_package_owns_module_bytes_after_archive_buffer_changes() {
        let source = b"export const value = 7;";
        let asset = b"asset";
        let meta = serde_json::to_vec(&serde_json::json!({
            "api_version": 1,
            "id": "test",
            "name": "Test",
            "version": "1.0.0",
            "entry": "entry.mjs",
            "modes": ["live"],
            "files": [
                {"path": "entry.mjs", "bytes": source.len(), "sha256": format!("{:x}", Sha256::digest(source))},
                {"path": "assets/marker.bin", "bytes": asset.len(), "sha256": format!("{:x}", Sha256::digest(asset))}
            ],
            "assets": [
                {"path": "assets/marker.bin", "bytes": asset.len(), "sha256": format!("{:x}", Sha256::digest(asset))}
            ]
        }))
        .unwrap();
        let mut archive = archive(&[
            ("manifest.json", &meta),
            ("entry.mjs", source),
            ("assets/marker.bin", asset),
        ]);
        let package = Package::from_bytes(&archive, PackageLimits::default()).unwrap();
        archive.fill(0);
        assert_eq!(package.entry_source().unwrap().as_bytes(), source);
        assert_eq!(package.asset("assets/marker.bin").unwrap(), b"asset");
    }
    #[test]
    fn rejects_tampered_module() {
        let meta = metadata(b"good");
        assert!(matches!(
            Package::from_bytes(
                &archive(&[("manifest.json", &meta), ("entry.mjs", b"evil")]),
                PackageLimits::default()
            ),
            Err(AnimationError::Integrity(_))
        ));
    }
    #[test]
    fn rejects_unlisted_assets() {
        let meta = metadata(b"a");
        assert!(Package::from_bytes(
            &archive(&[
                ("manifest.json", &meta),
                ("entry.mjs", b"a"),
                ("assets/hidden", b"x")
            ]),
            PackageLimits::default()
        )
        .is_err());
    }
    #[test]
    fn rejects_traversal_and_portable_ambiguity() {
        for path in [
            "../entry.mjs",
            "/entry.mjs",
            "assets/../entry.mjs",
            "assets\\a",
            "assets/a:",
            "assets/a.",
            "assets/CON.txt",
            "assets/lpt1.png",
            "modules/NUL.mjs",
        ] {
            assert!(!valid_path(path));
            assert!(
                Package::from_bytes(&archive(&[(path, b"a")]), PackageLimits::default()).is_err()
            );
        }
    }
    #[test]
    fn bounds_archive_before_parsing() {
        let limits = PackageLimits {
            archive_bytes: 1,
            ..PackageLimits::default()
        };
        assert!(matches!(
            Package::from_bytes(b"xx", limits),
            Err(AnimationError::Budget(_))
        ));
    }
    #[test]
    fn bounds_expanded_file() {
        let limits = PackageLimits {
            file_bytes: 2,
            ..PackageLimits::default()
        };
        assert!(matches!(
            Package::from_bytes(&archive(&[("entry.mjs", b"abc")]), limits),
            Err(AnimationError::Budget(_))
        ));
    }
    #[test]
    fn rejects_case_collision() {
        assert!(Package::from_bytes(
            &archive(&[("entry.mjs", b"a"), ("ENTRY.mjs", b"a")]),
            PackageLimits::default()
        )
        .is_err());
    }
    #[test]
    fn rejects_exact_duplicate_names_before_zip_deduplication() {
        let meta = metadata(b"a");
        let mut bytes = archive(&[
            ("manifest.json", &meta),
            ("entry.mjs", b"a"),
            ("other.mjs", b"a"),
        ]);
        for index in 0..bytes.len().saturating_sub(9) {
            if &bytes[index..index + 9] == b"other.mjs" {
                bytes[index..index + 9].copy_from_slice(b"entry.mjs");
            }
        }
        assert!(matches!(
            Package::from_bytes(&bytes, PackageLimits::default()),
            Err(AnimationError::InvalidPackage(_))
        ));
    }
    #[test]
    fn rejects_symlink_and_encrypted_flags() {
        let meta = metadata(b"a");
        let bytes = archive(&[("manifest.json", &meta), ("entry.mjs", b"a")]);
        let central = bytes
            .windows(4)
            .position(|part| part == b"PK\x01\x02")
            .unwrap();
        let mut symlink = bytes.clone();
        symlink[central + 38..central + 42].copy_from_slice(&0xa1ff0000_u32.to_le_bytes());
        assert!(Package::from_bytes(&symlink, PackageLimits::default()).is_err());
        let mut encrypted = bytes;
        encrypted[6] |= 1;
        encrypted[central + 8] |= 1;
        assert!(Package::from_bytes(&encrypted, PackageLimits::default()).is_err());
    }
    #[test]
    fn rejects_advertised_entry_count_before_metadata_allocation() {
        let bytes = archive(&[("entry.mjs", b"a")]);
        let limits = PackageLimits {
            entries: 0,
            ..PackageLimits::default()
        };
        assert!(matches!(
            Package::from_bytes(&bytes, limits),
            Err(AnimationError::Budget(_))
        ));
    }
    #[test]
    fn descriptor_does_not_read_module_contents_or_claim_integrity() {
        let meta = metadata(b"module-data");
        let mut bytes = archive(&[("manifest.json", &meta), ("entry.mjs", b"module-data")]);
        let start = bytes
            .windows(11)
            .position(|part| part == b"module-data")
            .unwrap();
        bytes[start] ^= 1;
        assert_eq!(
            inspect_manifest(&bytes, PackageLimits::default())
                .unwrap()
                .id,
            "test"
        );
        assert!(Package::from_bytes(&bytes, PackageLimits::default()).is_err());
    }
}
