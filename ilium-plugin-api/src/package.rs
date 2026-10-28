//! Deterministic plugin archives (reference-package-format.md; PKG-825).

use std::path::Path;

use crate::error::{ErrorCode, PluginError};
use crate::http::crc32;

/// DOS date of 1980-01-01.
const DOS_DATE: u16 = (1 << 5) | 1;
/// DOS time of 00:00:00.
const DOS_TIME: u16 = 0;
const UNIX_FILE_MODE: u32 = 0o100_644;

/// One entry of a read archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    name: String,
    date: u16,
    time: u16,
    external_attributes: u32,
    extra_len: usize,
}

impl ArchiveEntry {
    /// The path inside the archive.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The ZIP timestamp as `YYYY-MM-DD HH:MM:SS`; always `1980-01-01 00:00:00`.
    pub fn modified(&self) -> String {
        let year = 1980 + (self.date >> 9);
        let month = (self.date >> 5) & 0xf;
        let day = self.date & 0x1f;
        let hour = self.time >> 11;
        let minute = (self.time >> 5) & 0x3f;
        let second = (self.time & 0x1f) * 2;
        format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
    }

    /// The Unix mode; always `0o644`.
    pub fn mode(&self) -> u32 {
        (self.external_attributes >> 16) & 0o777
    }

    /// The length of the extra field; always 0.
    pub fn extra_field_len(&self) -> usize {
        self.extra_len
    }
}

fn zip_error(message: impl std::fmt::Display) -> PluginError {
    PluginError::coded("package.zip-invalid", ErrorCode::InvalidArgument, message)
}

fn collect_files(
    root: &Path,
    directory: &Path,
    out: &mut Vec<(String, std::path::PathBuf)>,
) -> Result<(), PluginError> {
    let entries = std::fs::read_dir(directory)
        .map_err(|error| zip_error(format!("cannot read {}: {error}", directory.display())))?;
    for entry in entries {
        let entry = entry.map_err(|error| zip_error(format!("cannot list a folder: {error}")))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| zip_error(format!("cannot stat {}: {error}", path.display())))?;
        if file_type.is_dir() {
            collect_files(root, &path, out)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| zip_error(error.to_string()))?
                .components()
                .map(|component| component.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            out.push((relative, path));
        }
    }
    Ok(())
}

fn u16_le(value: u16) -> [u8; 2] {
    value.to_le_bytes()
}

fn u32_le(value: u32) -> [u8; 4] {
    value.to_le_bytes()
}

/// Packs the source tree at `root` into archive bytes that depend only on the tree and `signed_at`.
pub fn pack_directory(root: &Path, signed_at: &str) -> Result<Vec<u8>, PluginError> {
    if signed_at.is_empty() {
        return Err(zip_error("signed_at must not be empty"));
    }
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut archive: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    for (name, path) in &files {
        let data = std::fs::read(path)
            .map_err(|error| zip_error(format!("cannot read {}: {error}", path.display())))?;
        let checksum = crc32(&data);
        let offset = archive.len() as u32;
        let size = data.len() as u32;
        archive.extend_from_slice(&u32_le(0x0403_4b50));
        archive.extend_from_slice(&u16_le(20)); // version needed
        archive.extend_from_slice(&u16_le(0x0800)); // UTF-8 names
        archive.extend_from_slice(&u16_le(0)); // stored
        archive.extend_from_slice(&u16_le(DOS_TIME));
        archive.extend_from_slice(&u16_le(DOS_DATE));
        archive.extend_from_slice(&u32_le(checksum));
        archive.extend_from_slice(&u32_le(size));
        archive.extend_from_slice(&u32_le(size));
        archive.extend_from_slice(&u16_le(name.len() as u16));
        archive.extend_from_slice(&u16_le(0)); // extra length
        archive.extend_from_slice(name.as_bytes());
        archive.extend_from_slice(&data);

        central.extend_from_slice(&u32_le(0x0201_4b50));
        central.extend_from_slice(&u16_le((3 << 8) | 20)); // made by Unix
        central.extend_from_slice(&u16_le(20));
        central.extend_from_slice(&u16_le(0x0800));
        central.extend_from_slice(&u16_le(0));
        central.extend_from_slice(&u16_le(DOS_TIME));
        central.extend_from_slice(&u16_le(DOS_DATE));
        central.extend_from_slice(&u32_le(checksum));
        central.extend_from_slice(&u32_le(size));
        central.extend_from_slice(&u32_le(size));
        central.extend_from_slice(&u16_le(name.len() as u16));
        central.extend_from_slice(&u16_le(0)); // extra length
        central.extend_from_slice(&u16_le(0)); // comment length
        central.extend_from_slice(&u16_le(0)); // disk
        central.extend_from_slice(&u16_le(0)); // internal attributes
        central.extend_from_slice(&u32_le(UNIX_FILE_MODE << 16));
        central.extend_from_slice(&u32_le(offset));
        central.extend_from_slice(name.as_bytes());
    }
    let central_offset = archive.len() as u32;
    let central_size = central.len() as u32;
    archive.extend_from_slice(&central);
    archive.extend_from_slice(&u32_le(0x0605_4b50));
    archive.extend_from_slice(&u16_le(0));
    archive.extend_from_slice(&u16_le(0));
    archive.extend_from_slice(&u16_le(files.len() as u16));
    archive.extend_from_slice(&u16_le(files.len() as u16));
    archive.extend_from_slice(&u32_le(central_size));
    archive.extend_from_slice(&u32_le(central_offset));
    archive.extend_from_slice(&u16_le(0)); // no comment
    Ok(archive)
}

fn read_u16(bytes: &[u8], at: usize) -> Result<u16, PluginError> {
    bytes
        .get(at..at + 2)
        .map(|slice| u16::from_le_bytes([slice[0], slice[1]]))
        .ok_or_else(|| zip_error("truncated archive"))
}

fn read_u32(bytes: &[u8], at: usize) -> Result<u32, PluginError> {
    bytes
        .get(at..at + 4)
        .map(|slice| u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
        .ok_or_else(|| zip_error("truncated archive"))
}

/// Reads the entries of archive bytes.
pub fn read_archive(bytes: &[u8]) -> Result<Vec<ArchiveEntry>, PluginError> {
    if bytes.len() < 22 {
        return Err(zip_error("archive shorter than an end record"));
    }
    let end = (0..=bytes.len() - 22)
        .rev()
        .find(|&at| bytes[at..at + 4] == [0x50, 0x4b, 0x05, 0x06])
        .ok_or_else(|| zip_error("no end-of-central-directory record"))?;
    let count = read_u16(bytes, end + 10)? as usize;
    let mut cursor = read_u32(bytes, end + 16)? as usize;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        if read_u32(bytes, cursor)? != 0x0201_4b50 {
            return Err(zip_error("bad central directory header"));
        }
        let time = read_u16(bytes, cursor + 12)?;
        let date = read_u16(bytes, cursor + 14)?;
        let name_len = read_u16(bytes, cursor + 28)? as usize;
        let extra_len = read_u16(bytes, cursor + 30)? as usize;
        let comment_len = read_u16(bytes, cursor + 32)? as usize;
        let external_attributes = read_u32(bytes, cursor + 38)?;
        let name_bytes = bytes
            .get(cursor + 46..cursor + 46 + name_len)
            .ok_or_else(|| zip_error("truncated entry name"))?;
        let name = String::from_utf8(name_bytes.to_vec())
            .map_err(|_| zip_error("entry name is not UTF-8"))?;
        entries.push(ArchiveEntry {
            name,
            date,
            time,
            external_attributes,
            extra_len,
        });
        cursor += 46 + name_len + extra_len + comment_len;
    }
    Ok(entries)
}
