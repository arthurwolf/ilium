//! The `.iliumpkg` archive: the stored ZIP subset of reference-package-format.md.

use ilium_plugin_api::manifest::{Diagnostic, FileEntry, Manifest};

/// One file in a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    /// Path inside the archive (reference-packaging.md PKG-110).
    pub path: String,
    /// File bytes.
    pub bytes: Vec<u8>,
}

/// Projection of the first signature in the `signature` document (PKG-200..208).
/// Full countersignatures and exact signed bytes remain in `PackageArchive::entries`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureDoc {
    /// Key id of the signer.
    pub key_id: String,
    /// Publisher id of the signer.
    pub publisher_id: String,
    /// Signing time, RFC 3339 UTC.
    pub signed_at: String,
    /// Base64url Ed25519 signature over the exact publisher-bound manifest message.
    pub signature: String,
    /// The signed file table.
    pub files: Vec<FileEntry>,
}

/// A parsed, structurally valid package (reference-packaging.md).
#[derive(Debug, Clone)]
pub struct PackageArchive {
    /// The manifest.
    pub manifest: Manifest,
    /// Every archive entry, including the exact manifest and signature bytes.
    pub entries: Vec<ArchiveEntry>,
    /// The signature, if the package is signed.
    pub signature: Option<SignatureDoc>,
}

/// Reads only stage-1 archive structure, paths and byte caps.
/// Does not parse the manifest or signature envelope, validate inventory/components,
/// verify signatures, or establish admission. The installer owns those later stages.
pub fn read_archive_entries(bytes: &[u8]) -> Result<Vec<ArchiveEntry>, Vec<Diagnostic>> {
    read_entries(bytes)
}

/// Reads package bytes; a structural fault is a `package.*` diagnostic (reference-packaging.md).
pub fn read_package(bytes: &[u8]) -> Result<PackageArchive, Vec<Diagnostic>> {
    let entries = read_entries(bytes)?;
    let raw = entries
        .iter()
        .find(|entry| entry.path == "manifest.json")
        .ok_or_else(|| {
            vec![failure(
                "package.manifest-missing",
                "manifest.json is required",
            )]
        })?;
    let text = std::str::from_utf8(&raw.bytes).map_err(|_| {
        vec![failure(
            "manifest.json-invalid",
            "manifest.json must be UTF-8",
        )]
    })?;
    let manifest = ilium_plugin_api::manifest::parse_manifest(text)?;
    // A malformed signature must never become an apparently unsigned package.
    // This checks document shape only; cryptographic verification and trust remain
    // separate admission stages. Exact bytes and every countersignature stay in entries.
    let signature = entries
        .iter()
        .find(|entry| entry.path == "signature")
        .map(|entry| {
            crate::signing::validate_signature_document(&entry.bytes)?;
            signature_projection(&entry.bytes, &manifest.files).ok_or_else(|| {
                failure(
                    "package.signature-format",
                    "Validated signature has no publisher projection",
                )
            })
        })
        .transpose()
        .map_err(|error| vec![error])?;
    Ok(PackageArchive {
        manifest,
        entries,
        signature,
    })
}

/// Writes files as a package in the accepted ZIP subset (reference-packaging.md).
pub fn write_package(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, Diagnostic> {
    if files.len() > 4096 {
        return Err(failure(
            "package.entry-limit",
            "at most 4096 entries are allowed",
        ));
    }
    let mut files: Vec<_> = files.iter().collect();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut names = std::collections::BTreeSet::new();
    let mut folded = std::collections::BTreeSet::new();
    for (path, bytes) in &files {
        validate_path(path)?;
        check_names(path, &mut names, &mut folded)?;
        check_size(path, bytes.len())?;
    }
    if !names.contains("manifest.json") {
        return Err(failure(
            "package.manifest-missing",
            "manifest.json is required",
        ));
    }
    let total = files
        .iter()
        .try_fold(22usize, |total, (path, data)| {
            total
                .checked_add(76 + 2 * path.len())
                .and_then(|v| v.checked_add(data.len()))
        })
        .ok_or_else(|| failure("package.size-limit", "archive size overflow"))?;
    if total > PACKAGE_CAP {
        return Err(failure("package.size-limit", "package exceeds 300 MiB"));
    }
    let mut bytes = Vec::with_capacity(total);
    let mut metadata = Vec::with_capacity(files.len());
    for (path, data) in &files {
        let offset = bytes.len() as u32;
        let crc = crc32(data);
        push32(&mut bytes, 0x04034b50);
        for field in [20, 0x0800, 0, 0, 0x0021] {
            push16(&mut bytes, field);
        }
        for field in [crc, data.len() as u32, data.len() as u32] {
            push32(&mut bytes, field);
        }
        push16(&mut bytes, path.len() as u16);
        push16(&mut bytes, 0);
        bytes.extend_from_slice(path.as_bytes());
        bytes.extend_from_slice(data);
        metadata.push((offset, crc));
    }
    let central_offset = bytes.len() as u32;
    for ((path, data), (offset, crc)) in files.iter().zip(metadata) {
        push32(&mut bytes, 0x02014b50);
        for field in [0x0314, 20, 0x0800, 0, 0, 0x0021] {
            push16(&mut bytes, field);
        }
        for field in [crc, data.len() as u32, data.len() as u32] {
            push32(&mut bytes, field);
        }
        for field in [path.len() as u16, 0, 0, 0, 0] {
            push16(&mut bytes, field);
        }
        push32(&mut bytes, 0x81a40000);
        push32(&mut bytes, offset);
        bytes.extend_from_slice(path.as_bytes());
    }
    let central_size = bytes.len() as u32 - central_offset;
    push32(&mut bytes, 0x06054b50);
    for field in [0, 0, files.len() as u16, files.len() as u16] {
        push16(&mut bytes, field);
    }
    push32(&mut bytes, central_size);
    push32(&mut bytes, central_offset);
    push16(&mut bytes, 0);
    Ok(bytes)
}

const PACKAGE_CAP: usize = 300 * 1024 * 1024;
const ENTRY_CAP: usize = 64 * 1024 * 1024;

fn failure(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(
        ilium_plugin_api::error::Code::from_static(code),
        "",
        message,
    )
}
fn field16(bytes: &[u8], offset: usize) -> Result<u16, Diagnostic> {
    let value = bytes
        .get(offset..offset.saturating_add(2))
        .ok_or_else(|| failure("package.zip-invalid", "truncated ZIP field"))?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}
fn field32(bytes: &[u8], offset: usize) -> Result<u32, Diagnostic> {
    let value = bytes
        .get(offset..offset.saturating_add(4))
        .ok_or_else(|| failure("package.zip-invalid", "truncated ZIP field"))?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}
fn push16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
fn push32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (index, slot) in table.iter_mut().enumerate() {
        let mut value = index as u32;
        for _ in 0..8 {
            value = (value >> 1) ^ if value & 1 != 0 { 0xedb88320 } else { 0 };
        }
        *slot = value;
    }
    let mut crc = u32::MAX;
    for byte in bytes {
        crc = table[((crc ^ u32::from(*byte)) & 255) as usize] ^ (crc >> 8);
    }
    !crc
}

fn check_size(path: &str, length: usize) -> Result<(), Diagnostic> {
    let (limit, code) = match path {
        "manifest.json" => (1024 * 1024, "package.manifest-size"),
        "signature" => (16 * 1024, "package.signature-format"),
        _ => (ENTRY_CAP, "package.size-limit"),
    };
    if length > limit {
        return Err(failure(code, format!("{path} exceeds its byte limit")));
    }
    Ok(())
}

fn validate_path(path: &str) -> Result<(), Diagnostic> {
    if path.ends_with('/') {
        return Err(failure(
            "package.entry-directory",
            format!("directory entry {path:?}"),
        ));
    }
    let parts: Vec<_> = path.split('/').collect();
    if path.len() > 255
        || parts.len() > 16
        || parts.iter().any(|part| {
            part.is_empty()
                || part.starts_with('.')
                || part.ends_with('.')
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
                || reserved_component(part)
        })
    {
        return Err(failure(
            "package.path-invalid",
            format!("invalid archive path {path:?}"),
        ));
    }
    let allowed = match parts.as_slice() {
        [name] => matches!(
            *name,
            "manifest.json"
                | "server.wasm"
                | "client.wasm"
                | "scene.wasm"
                | "signature"
                | "README.md"
                | "CHANGELOG.md"
                | "LICENSE"
                | "NOTICE"
                | "sbom.json"
                | "build-info.json"
                | "sbom.spdx.json"
                | "provenance.json"
        ),
        ["assets" | "src", _, ..] => true,
        ["help", name] => name.ends_with(".md"),
        ["sounds", name] => name.ends_with(".wav") || name.ends_with(".ogg"),
        ["screenshots", name] => name.ends_with(".png"),
        _ => false,
    };
    if !allowed {
        return Err(failure(
            "package.entry-unexpected",
            format!("unexpected archive entry {path}"),
        ));
    }
    Ok(())
}
fn reserved_component(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|number| number.len() == 1 && matches!(number.as_bytes()[0], b'1'..=b'9'))
}
fn check_names(
    path: &str,
    names: &mut std::collections::BTreeSet<String>,
    folded: &mut std::collections::BTreeSet<String>,
) -> Result<(), Diagnostic> {
    if !names.insert(path.to_owned()) {
        return Err(failure(
            "package.path-duplicate",
            format!("duplicate entry {path}"),
        ));
    }
    if !folded.insert(path.to_ascii_lowercase()) {
        return Err(failure(
            "package.path-case-collision",
            format!("case-colliding entry {path}"),
        ));
    }
    for other in names.iter() {
        let other_folded = other.to_ascii_lowercase();
        let path_folded = path.to_ascii_lowercase();
        if other != path
            && (path_folded
                .strip_prefix(&other_folded)
                .is_some_and(|tail| tail.starts_with('/'))
                || other_folded
                    .strip_prefix(&path_folded)
                    .is_some_and(|tail| tail.starts_with('/')))
        {
            return Err(failure(
                "package.path-invalid",
                format!("file/folder collision between {other} and {path}"),
            ));
        }
    }
    Ok(())
}

#[derive(Debug)]
struct CentralEntry {
    path: String,
    header: usize,
    local: usize,
    length: usize,
    crc: u32,
}

fn read_entries(bytes: &[u8]) -> Result<Vec<ArchiveEntry>, Vec<Diagnostic>> {
    read_entries_inner(bytes)
        .map_err(|error| vec![error])
        .and_then(|(entries, errors)| {
            if errors.is_empty() {
                Ok(entries)
            } else {
                Err(errors)
            }
        })
}
fn read_entries_inner(bytes: &[u8]) -> Result<(Vec<ArchiveEntry>, Vec<Diagnostic>), Diagnostic> {
    if bytes.len() > PACKAGE_CAP {
        return Err(failure("package.size-limit", "package exceeds 300 MiB"));
    }
    let end = bytes
        .len()
        .checked_sub(22)
        .ok_or_else(|| failure("package.zip-invalid", "missing ZIP end record"))?;
    if bytes.get(end..end + 4) != Some(b"PK\x05\x06") {
        // This does not accept an alternate end record: it only diagnoses forbidden suffix bytes.
        if let Some(offset) = bytes.windows(4).rposition(|window| window == b"PK\x05\x06") {
            if bytes.len().saturating_sub(offset) >= 22 {
                return Err(failure(
                    if field16(bytes, offset + 20)? != 0 {
                        "package.zip-comment"
                    } else {
                        "package.zip-trailing"
                    },
                    "bytes follow the ZIP end record",
                ));
            }
        }
        return Err(failure(
            "package.zip-invalid",
            "ZIP end record is not in the final 22 bytes",
        ));
    }
    if field16(bytes, end + 20)? != 0 {
        return Err(failure("package.zip-comment", "ZIP comments are forbidden"));
    }
    if end >= 20 && bytes.get(end - 20..end - 16) == Some(b"PK\x06\x07") {
        return Err(failure("package.zip64", "ZIP64 locator is forbidden"));
    }
    let count = usize::from(field16(bytes, end + 10)?);
    let central_size = field32(bytes, end + 12)?;
    let central_offset = field32(bytes, end + 16)?;
    if central_size == u32::MAX || central_offset == u32::MAX || count == usize::from(u16::MAX) {
        return Err(failure("package.zip64", "ZIP64 sentinel is forbidden"));
    }
    if count > 4096 {
        return Err(failure(
            "package.entry-limit",
            "at most 4096 entries are allowed",
        ));
    }
    if field16(bytes, end + 4)? != 0
        || field16(bytes, end + 6)? != 0
        || usize::from(field16(bytes, end + 8)?) != count
    {
        return Err(failure(
            "package.zip-invalid",
            "inconsistent disk or entry counts",
        ));
    }
    let central_offset = central_offset as usize;
    let central_size = central_size as usize;
    if central_offset.checked_add(central_size) != Some(end) {
        if bytes
            .get(central_offset.saturating_add(central_size)..)
            .is_some_and(|tail| tail.starts_with(b"PK\x06\x06") || tail.starts_with(b"PK\x06\x07"))
        {
            return Err(failure("package.zip64", "ZIP64 records are forbidden"));
        }
        return Err(failure(
            "package.zip-invalid",
            "inconsistent central directory bounds",
        ));
    }
    let mut cursor = central_offset;
    let mut metadata = Vec::with_capacity(count);
    let mut errors = Vec::new();
    let mut total_uncompressed = 0u64;
    let mut names = std::collections::BTreeSet::new();
    let mut folded = std::collections::BTreeSet::new();
    for _ in 0..count {
        if cursor
            .checked_add(46)
            .is_none_or(|end_header| end_header > end)
            || field32(bytes, cursor)? != 0x02014b50
        {
            return Err(failure(
                "package.zip-invalid",
                "truncated central directory",
            ));
        }
        let name_length = usize::from(field16(bytes, cursor + 28)?);
        let extra = usize::from(field16(bytes, cursor + 30)?);
        let comment = usize::from(field16(bytes, cursor + 32)?);
        let next = cursor + 46 + name_length + extra + comment;
        if next > end {
            return Err(failure(
                "package.zip-invalid",
                "central directory entry extends past end",
            ));
        }
        let path = std::str::from_utf8(&bytes[cursor + 46..cursor + 46 + name_length])
            .map_err(|_| failure("package.path-invalid", "entry name is not UTF-8"))?
            .to_owned();
        let length = field32(bytes, cursor + 24)?;
        let compressed = field32(bytes, cursor + 20)?;
        let local = field32(bytes, cursor + 42)?;
        if [length, compressed, local].contains(&u32::MAX) {
            return Err(failure("package.zip64", "ZIP64 sentinel is forbidden"));
        }
        total_uncompressed += u64::from(length);
        let mut entry_errors = Vec::new();
        if u64::from(length) > u64::from(compressed) * 200 {
            entry_errors.push(failure(
                "package.zip-invalid",
                "entry compression ratio exceeds 200 to 1",
            ));
        }
        if total_uncompressed > 512 * 1024 * 1024 {
            entry_errors.push(failure(
                "package.zip-invalid",
                "total uncompressed size exceeds 512 MiB",
            ));
        }
        check_header(bytes, cursor, true, &mut entry_errors)?;
        for result in [
            validate_path(&path),
            check_names(&path, &mut names, &mut folded),
            check_size(&path, length as usize),
        ] {
            if let Err(error) = result {
                entry_errors.push(error);
            }
        }
        if metadata
            .last()
            .is_some_and(|previous: &CentralEntry| previous.path > path)
        {
            entry_errors.push(failure(
                "package.entry-order",
                format!("entry {path} is out of byte order"),
            ));
        }
        if length != compressed {
            entry_errors.push(failure(
                "package.header-mismatch",
                "stored entry sizes differ",
            ));
        }
        errors.extend(entry_errors);
        metadata.push(CentralEntry {
            path,
            header: cursor,
            local: local as usize,
            length: length as usize,
            crc: field32(bytes, cursor + 16)?,
        });
        cursor = next;
    }
    if cursor != end {
        return Err(failure(
            "package.zip-invalid",
            "central directory size or entry count disagrees",
        ));
    }
    if !names.contains("manifest.json") {
        errors.push(failure(
            "package.manifest-missing",
            "manifest.json is required",
        ));
    }
    if metadata.first().is_some_and(|entry| entry.local != 0) && bytes.starts_with(b"PK\x03\x04") {
        return Err(failure(
            "package.header-mismatch",
            "central directory does not begin with the first local entry",
        ));
    }
    // Central caps precede all entry data access and allocation.
    if !errors.is_empty() {
        return Ok((Vec::new(), bounded_errors(errors)));
    }
    let mut local_cursor = 0;
    let mut entries = Vec::with_capacity(count);
    for entry in metadata {
        if entry.local != local_cursor {
            let is_header =
                bytes.get(entry.local..entry.local.saturating_add(4)) == Some(b"PK\x03\x04");
            let expected_is_header =
                bytes.get(local_cursor..local_cursor.saturating_add(4)) == Some(b"PK\x03\x04");
            return Err(failure(
                if is_header && !expected_is_header && entry.local > local_cursor {
                    "package.zip-trailing"
                } else {
                    "package.header-mismatch"
                },
                "local entries must be adjacent and follow central order",
            ));
        }
        if local_cursor
            .checked_add(30)
            .is_none_or(|value| value > central_offset)
            || field32(bytes, local_cursor)? != 0x04034b50
        {
            return Err(failure(
                "package.header-mismatch",
                "local header is missing",
            ));
        }
        let name_length = usize::from(field16(bytes, local_cursor + 26)?);
        let extra = usize::from(field16(bytes, local_cursor + 28)?);
        let data_start = local_cursor + 30 + name_length + extra;
        let data_end = data_start
            .checked_add(entry.length)
            .ok_or_else(|| failure("package.zip-invalid", "entry size overflow"))?;
        let mut entry_errors = Vec::new();
        check_header(bytes, local_cursor, false, &mut entry_errors)?;
        for offset in [18, 22] {
            if field32(bytes, local_cursor + offset)? == u32::MAX {
                return Err(failure("package.zip64", "local ZIP64 size sentinel"));
            }
        }
        if field32(bytes, local_cursor + 14)? != entry.crc
            || field32(bytes, local_cursor + 18)? as usize != entry.length
            || field32(bytes, local_cursor + 22)? as usize != entry.length
            || bytes.get(local_cursor + 30..local_cursor + 30 + name_length)
                != Some(entry.path.as_bytes())
            || field16(bytes, local_cursor + 6)? != field16(bytes, entry.header + 8)?
            || field16(bytes, local_cursor + 8)? != field16(bytes, entry.header + 10)?
        {
            entry_errors.push(failure(
                "package.header-mismatch",
                format!("local/central header mismatch for {}", entry.path),
            ));
        }
        if !entry_errors.is_empty() {
            errors.extend(entry_errors);
            break;
        }
        if data_end > central_offset {
            return Err(failure(
                "package.zip-invalid",
                "entry data overlaps the central directory",
            ));
        }
        let data = &bytes[data_start..data_end];
        if crc32(data) != entry.crc {
            errors.push(failure(
                "package.crc-mismatch",
                format!("CRC-32 mismatch for {}", entry.path),
            ));
        }
        entries.push(ArchiveEntry {
            path: entry.path,
            bytes: data.to_vec(),
        });
        local_cursor = data_end;
    }
    if errors.is_empty() && local_cursor != central_offset {
        errors.push(failure(
            "package.zip-trailing",
            "bytes between local entries and central directory",
        ));
    }
    Ok((entries, bounded_errors(errors)))
}

fn check_header(
    bytes: &[u8],
    offset: usize,
    central: bool,
    errors: &mut Vec<Diagnostic>,
) -> Result<(), Diagnostic> {
    let shift = if central { 2 } else { 0 };
    let flags = field16(bytes, offset + 6 + shift)?;
    if flags & 1 != 0 {
        errors.push(failure(
            "package.zip-encrypted",
            "encrypted entries are forbidden",
        ));
    }
    if flags & 8 != 0 {
        errors.push(failure(
            "package.zip-descriptor",
            "data descriptors are forbidden",
        ));
    }
    if flags & 0x0800 == 0 || flags & !(0x0800 | 1 | 8) != 0 {
        errors.push(failure(
            "package.zip-invalid",
            "unsupported general-purpose flags",
        ));
    }
    if field16(bytes, offset + 8 + shift)? != 0 {
        errors.push(failure(
            "package.zip-compressed",
            "only stored entries are allowed",
        ));
    }
    if field16(bytes, offset + 4 + shift)? != 20
        || field16(bytes, offset + 10 + shift)? != 0
        || field16(bytes, offset + 12 + shift)? != 0x0021
    {
        errors.push(failure(
            "package.zip-invalid",
            "noncanonical ZIP version or timestamp",
        ));
    }
    if field16(bytes, offset + if central { 30 } else { 28 })? != 0 {
        errors.push(failure(
            "package.zip-extra",
            "ZIP extra fields are forbidden",
        ));
    }
    if central {
        if field16(bytes, offset + 32)? != 0 {
            errors.push(failure(
                "package.zip-comment",
                "entry comments are forbidden",
            ));
        }
        if field32(bytes, offset + 38)? != 0x81a40000 {
            errors.push(failure(
                "package.entry-symlink",
                "entry must be a regular Unix 0644 file",
            ));
        }
        if field16(bytes, offset + 4)? != 0x0314
            || field16(bytes, offset + 34)? != 0
            || field16(bytes, offset + 36)? != 0
        {
            errors.push(failure(
                "package.zip-invalid",
                "noncanonical creator, disk or internal attributes",
            ));
        }
    }
    Ok(())
}

fn bounded_errors(mut errors: Vec<Diagnostic>) -> Vec<Diagnostic> {
    if errors.len() > 100 {
        let omitted = errors.len() - 100;
        errors.truncate(100);
        let mut warning = failure(
            "package.errors-truncated",
            format!("{omitted} additional archive errors omitted"),
        );
        warning.severity = ilium_plugin_api::Severity::Warning;
        errors.push(warning);
    }
    errors
}

fn signature_projection(bytes: &[u8], files: &[FileEntry]) -> Option<SignatureDoc> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let signature = value.get("signatures")?.as_array()?.first()?;
    Some(SignatureDoc {
        key_id: signature.get("key_id")?.as_str()?.to_owned(),
        publisher_id: signature.get("publisher_id")?.as_str()?.to_owned(),
        signed_at: signature.get("signed_at")?.as_str()?.to_owned(),
        signature: signature.get("signature")?.as_str()?.to_owned(),
        files: files.to_vec(),
    })
}
