//! Bounded ZIP32 index and stored/raw-DEFLATE reader using existing flate2.
//! ZIP64, encryption, multi-volume and unsupported compression report errors.
//! Local headers, descriptors, overlaps, CRC and full DEFLATE consumption matter.
use super::budget::{zeroed_bytes, ByteBudget, Cancel, Reservation};
use super::error::{AssetError, Result};
use super::identity::{AssetPath, Digest256};
use super::source::{check_limit, AssetSource, MemberInfo, SourceBytes, SourceLimits};
use flate2::{Decompress, FlushDecompress, Status};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};
#[derive(Debug)]
struct ZipEntry {
    payload: Range<usize>,
    expanded: usize,
    crc: u32,
    method: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DuplicatePolicy {
    Reject,
    LastCentralEntry,
}
#[derive(Clone, Debug)]
pub struct DuplicateMember {
    pub path: AssetPath,
    pub previous_crc32: u32,
    pub replacement_crc32: u32,
    pub replacement_ordinal: u16,
}
#[derive(Debug)]
pub struct ZipSource {
    original: SourceBytes,
    index: BTreeMap<AssetPath, ZipEntry>,
    members: BTreeMap<AssetPath, MemberInfo>,
    limits: SourceLimits,
    reservation: Reservation,
    duplicate_members: Vec<DuplicateMember>,
}
fn bad(message: &str) -> AssetError {
    AssetError::InvalidMetadata(format!("ZIP: {message}"))
}
fn region(bytes: &[u8], start: usize, count: usize) -> Result<&[u8]> {
    bytes
        .get(
            start
                ..start
                    .checked_add(count)
                    .ok_or_else(|| bad("offset overflow"))?,
        )
        .ok_or_else(|| bad("truncated record"))
}
fn u16_at(bytes: &[u8], start: usize) -> Result<u16> {
    let b = region(bytes, start, 2)?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}
fn u32_at(bytes: &[u8], start: usize) -> Result<u32> {
    let b = region(bytes, start, 4)?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn extra_fields(bytes: &[u8]) -> Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        let kind = u16_at(bytes, offset)?;
        let length = u16_at(bytes, offset + 2)? as usize;
        region(bytes, offset + 4, length)?;
        if matches!(kind, 1 | 0x9901) {
            return Err(AssetError::Unsupported(
                "ZIP64 or AES ZIP extra field".into(),
            ));
        }
        offset += 4 + length;
    }
    Ok(())
}
/// CRC32 is ZIP integrity only; archive/member SHA-256 remains the custody hash.
pub(crate) fn crc32(bytes: &[u8], cancel: Cancel<'_>) -> Result<u32> {
    let mut value = !0_u32;
    for chunk in bytes.chunks(65536) {
        cancel.check()?;
        for byte in chunk {
            value ^= u32::from(*byte);
            for _ in 0..8 {
                value = (value >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(value & 1));
            }
        }
    }
    Ok(!value)
}
fn name_text(bytes: &[u8], utf8: bool) -> Result<String> {
    if utf8 {
        return std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| bad("invalid UTF-8 name"));
    }
    // ZIP's original unflagged filename encoding is CP437, not lossy UTF-8.
    let high: Vec<char> = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■ ".chars().collect();
    let mut output = String::new();
    for byte in bytes {
        if *byte < 128 {
            output.push(char::from(*byte));
            continue;
        }
        output.push(
            *high
                .get(usize::from(*byte - 128))
                .ok_or_else(|| bad("CP437 table index"))?,
        );
    }
    Ok(output)
}
impl ZipSource {
    pub fn open(
        original: SourceBytes,
        expected: Option<Digest256>,
        limits: SourceLimits,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        Self::open_with_duplicate_policy(
            original,
            expected,
            limits,
            budget,
            cancel,
            DuplicatePolicy::Reject,
        )
    }
    /// An explicit reviewed compatibility choice. Every duplicate is retained
    /// in `duplicate_members`; default open still rejects all duplicates.
    pub fn open_with_duplicate_policy(
        original: SourceBytes,
        expected: Option<Digest256>,
        limits: SourceLimits,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
        policy: DuplicatePolicy,
    ) -> Result<Self> {
        cancel.check()?;
        limits.validate()?;
        if !original.uses_budget(budget) {
            return Err(bad("archive and index use different byte accounts"));
        }
        check_limit(
            "archive bytes",
            original.bytes().len() as u64,
            limits.archive_bytes,
        )?;
        if let Some(expected) = expected {
            original.verify(expected)?;
        }
        let bytes = original.bytes();
        if bytes.len() < 22 {
            return Err(bad("missing end record"));
        }
        let low = bytes.len().saturating_sub(65557);
        let mut end = None;
        for offset in (low..=bytes.len() - 22).rev() {
            if bytes[offset..offset + 4] != [0x50, 0x4b, 0x05, 0x06] {
                continue;
            }
            if offset + 22 + usize::from(u16_at(bytes, offset + 20)?) == bytes.len() {
                end = Some(offset);
                break;
            }
        }
        let end = end.ok_or_else(|| bad("missing or trailing-data end record"))?;
        let count = u16_at(bytes, end + 10)?;
        let directory_size = u32_at(bytes, end + 12)?;
        let directory_offset = u32_at(bytes, end + 16)?;
        if count == u16::MAX || directory_size == u32::MAX || directory_offset == u32::MAX {
            return Err(AssetError::Unsupported("ZIP64 archive".into()));
        }
        if u16_at(bytes, end + 4)? != 0
            || u16_at(bytes, end + 6)? != 0
            || u16_at(bytes, end + 8)? != count
        {
            return Err(AssetError::Unsupported("multi-volume ZIP".into()));
        }
        check_limit("source entries", u64::from(count), limits.entries as u64)?;
        let mut cursor = directory_offset as usize;
        if cursor.checked_add(directory_size as usize) != Some(end) {
            return Err(bad("central directory extent"));
        }
        let reservation = budget.reserve(u64::from(count) * 4096 + 4096, cancel)?;
        let mut index = BTreeMap::<AssetPath, ZipEntry>::new();
        let mut members = BTreeMap::new();
        let mut all_names = BTreeSet::new();
        let mut duplicate_members = Vec::new();
        let mut spans = Vec::new();
        spans
            .try_reserve_exact(count as usize)
            .map_err(|_| AssetError::Allocation)?;
        let mut total = 0_u64;
        for ordinal in 0..count {
            cancel.check()?;
            if u32_at(bytes, cursor)? != 0x0201_4b50 {
                return Err(bad("central record signature"));
            }
            let version = u16_at(bytes, cursor + 6)?;
            let flags = u16_at(bytes, cursor + 8)?;
            let method = u16_at(bytes, cursor + 10)?;
            let crc = u32_at(bytes, cursor + 16)?;
            let packed = u32_at(bytes, cursor + 20)?;
            let expanded = u32_at(bytes, cursor + 24)?;
            let name_length = usize::from(u16_at(bytes, cursor + 28)?);
            let extra_length = usize::from(u16_at(bytes, cursor + 30)?);
            let comment_length = usize::from(u16_at(bytes, cursor + 32)?);
            let disk = u16_at(bytes, cursor + 34)?;
            let attributes = u32_at(bytes, cursor + 38)?;
            let local = u32_at(bytes, cursor + 42)?;
            if packed == u32::MAX || expanded == u32::MAX || local == u32::MAX || disk != 0 {
                return Err(AssetError::Unsupported(
                    "ZIP64 or multi-volume member".into(),
                ));
            }
            if flags & !0x080e != 0 || !matches!(method, 0 | 8) || version > 20 {
                return Err(AssetError::Unsupported(format!(
                    "ZIP flags={flags:#x}, method={method}, version={version}"
                )));
            }
            if method == 0 && (packed != expanded || flags & 6 != 0) {
                return Err(bad("stored member fields"));
            }
            check_limit("ZIP name bytes", name_length as u64, 1024)?;
            let raw_name = region(bytes, cursor + 46, name_length)?;
            let name = name_text(raw_name, flags & 0x800 != 0)?;
            let directory = name.ends_with('/');
            let path = AssetPath::parse(if directory {
                &name[..name.len() - 1]
            } else {
                &name
            })?;
            check_limit(
                "source depth",
                path.as_str().split('/').count() as u64,
                limits.depth as u64,
            )?;
            if !all_names.insert(path.clone()) {
                if directory || policy == DuplicatePolicy::Reject {
                    return Err(AssetError::Duplicate(path.as_str().into()));
                }
                let previous = index
                    .get(&path)
                    .ok_or_else(|| bad("duplicate collides with a directory"))?;
                duplicate_members.push(DuplicateMember {
                    path: path.clone(),
                    previous_crc32: previous.crc,
                    replacement_crc32: crc,
                    replacement_ordinal: ordinal,
                });
            }
            let unix_kind = (attributes >> 16) & 0xf000;
            if !matches!(unix_kind, 0 | 0x4000 | 0x8000) || (unix_kind == 0x4000 && !directory) {
                return Err(bad("symlink, special or contradictory member type"));
            }
            if directory && expanded != 0 {
                return Err(bad("directory contains file data"));
            }
            extra_fields(region(bytes, cursor + 46 + name_length, extra_length)?)?;
            cursor = cursor
                .checked_add(46 + name_length + extra_length + comment_length)
                .ok_or_else(|| bad("directory overflow"))?;
            if cursor > end {
                return Err(bad("central member overruns directory"));
            }
            let local = local as usize;
            if u32_at(bytes, local)? != 0x0403_4b50
                || u16_at(bytes, local + 4)? != version
                || u16_at(bytes, local + 6)? != flags
                || u16_at(bytes, local + 8)? != method
            {
                return Err(bad("central/local header disagreement"));
            }
            let local_name_length = usize::from(u16_at(bytes, local + 26)?);
            let local_extra_length = usize::from(u16_at(bytes, local + 28)?);
            if region(bytes, local + 30, local_name_length)? != raw_name {
                return Err(bad("central/local name disagreement"));
            }
            extra_fields(region(
                bytes,
                local + 30 + local_name_length,
                local_extra_length,
            )?)?;
            let payload_start = local
                .checked_add(30 + local_name_length + local_extra_length)
                .ok_or_else(|| bad("local overflow"))?;
            let payload_end = payload_start
                .checked_add(packed as usize)
                .ok_or_else(|| bad("payload overflow"))?;
            region(bytes, payload_start, packed as usize)?;
            let mut span_end = payload_end;
            if flags & 8 == 0 {
                if u32_at(bytes, local + 14)? != crc
                    || u32_at(bytes, local + 18)? != packed
                    || u32_at(bytes, local + 22)? != expanded
                {
                    return Err(bad("local CRC/length disagreement"));
                }
            } else {
                for (offset, target) in [(14, crc), (18, packed), (22, expanded)] {
                    let actual = u32_at(bytes, local + offset)?;
                    if actual != 0 && actual != target {
                        return Err(bad("nonzero descriptor-header disagreement"));
                    }
                }
                let unsigned = u32_at(bytes, payload_end)? == crc
                    && u32_at(bytes, payload_end + 4)? == packed
                    && u32_at(bytes, payload_end + 8)? == expanded;
                if unsigned {
                    span_end += 12;
                } else if u32_at(bytes, payload_end)? == 0x0807_4b50
                    && u32_at(bytes, payload_end + 4)? == crc
                    && u32_at(bytes, payload_end + 8)? == packed
                    && u32_at(bytes, payload_end + 12)? == expanded
                {
                    span_end += 16;
                } else {
                    return Err(bad("invalid data descriptor"));
                }
            }
            if span_end > directory_offset as usize {
                return Err(bad("member overlaps central directory"));
            }
            spans.push((local, span_end));
            check_limit("member bytes", u64::from(expanded), limits.member_bytes)?;
            total = total
                .checked_add(u64::from(expanded))
                .ok_or(AssetError::Allocation)?;
            check_limit("total source bytes", total, limits.total_member_bytes)?;
            if directory {
                continue;
            }
            members.insert(
                path.clone(),
                MemberInfo {
                    path: path.clone(),
                    bytes: u64::from(expanded),
                },
            );
            index.insert(
                path,
                ZipEntry {
                    payload: payload_start..payload_end,
                    expanded: expanded as usize,
                    crc,
                    method,
                },
            );
        }
        if cursor != end {
            return Err(bad("unaccounted central directory bytes"));
        }
        spans.sort_unstable();
        if spans.windows(2).any(|pair| pair[0].1 > pair[1].0) {
            return Err(bad("overlapping local records"));
        }
        for path in all_names {
            let mut prefix = String::new();
            let parts: Vec<_> = path.as_str().split('/').collect();
            for component in parts.iter().take(parts.len().saturating_sub(1)) {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(component);
                if members.contains_key(&AssetPath::parse(&prefix)?) {
                    return Err(bad("file/directory prefix collision"));
                }
            }
        }
        cancel.check()?;
        Ok(Self {
            original,
            index,
            members,
            limits,
            reservation,
            duplicate_members,
        })
    }
    pub fn verify_all(&self, budget: &ByteBudget, cancel: Cancel<'_>) -> Result<usize> {
        for path in self.members.keys() {
            cancel.check()?;
            if self
                .read(path, self.limits.member_bytes, budget, cancel)?
                .is_none()
            {
                return Err(bad("index changed"));
            }
        }
        Ok(self.members.len())
    }
}
impl ZipSource {
    pub fn duplicate_members(&self) -> &[DuplicateMember] {
        &self.duplicate_members
    }
}
impl AssetSource for ZipSource {
    fn members(&self) -> &BTreeMap<AssetPath, MemberInfo> {
        &self.members
    }
    fn source_digest(&self) -> Option<Digest256> {
        Some(self.original.digest())
    }
    fn read(
        &self,
        path: &AssetPath,
        cap: u64,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Option<SourceBytes>> {
        cancel.check()?;
        if !self.reservation.belongs_to(budget) {
            return Err(bad("read account differs from archive account"));
        }
        let Some(entry) = self.index.get(path) else {
            return Ok(None);
        };
        check_limit(
            "member read bytes",
            entry.expanded as u64,
            cap.min(self.limits.member_bytes),
        )?;
        let payload = &self.original.bytes()[entry.payload.clone()];
        let result = if entry.method == 0 {
            SourceBytes::from_slice(payload, cap, budget, cancel)?
        } else {
            let allocation = entry
                .expanded
                .checked_add(1)
                .ok_or(AssetError::Allocation)?;
            let reservation = budget.reserve(allocation as u64 + 512 * 1024, cancel)?;
            let mut output = zeroed_bytes(allocation)?;
            let mut decoder = Decompress::new(false);
            loop {
                cancel.check()?;
                let before_in = decoder.total_in() as usize;
                let before_out = decoder.total_out() as usize;
                let input_end = payload.len().min(before_in.saturating_add(65536));
                let output_end = output.len().min(before_out.saturating_add(65536));
                let status = decoder
                    .decompress(
                        &payload[before_in..input_end],
                        &mut output[before_out..output_end],
                        FlushDecompress::None,
                    )
                    .map_err(|e| bad(&super::error::summary(&e.to_string())))?;
                if decoder.total_out() > entry.expanded as u64 {
                    return Err(bad("DEFLATE exceeds declared length"));
                }
                if status == Status::StreamEnd {
                    break;
                }
                if decoder.total_in() as usize == before_in
                    && decoder.total_out() as usize == before_out
                {
                    return Err(bad("truncated or stalled DEFLATE stream"));
                }
            }
            if decoder.total_in() != payload.len() as u64
                || decoder.total_out() != entry.expanded as u64
            {
                return Err(bad("DEFLATE length or trailing data mismatch"));
            }
            output.truncate(entry.expanded);
            let digest = Digest256::of_checked(&output, cancel)?;
            SourceBytes {
                bytes: output,
                digest,
                reservation,
            }
        };
        if crc32(result.bytes(), cancel)? != entry.crc {
            return Err(bad("CRC32 mismatch"));
        }
        Ok(Some(result))
    }
}
#[cfg(test)]
pub(crate) fn synthetic_zip(entries: &[(&str, &[u8])], deflate: bool, descriptor: bool) -> Vec<u8> {
    use std::{io::Write, sync::atomic::AtomicBool};
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut bytes = Vec::new();
    let mut directory = Vec::new();
    for (name, plain) in entries {
        let payload = if deflate {
            let mut writer =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            writer.write_all(plain).unwrap();
            writer.finish().unwrap()
        } else {
            plain.to_vec()
        };
        let offset = bytes.len() as u32;
        let crc = crc32(plain, cancel).unwrap();
        let flags = 0x800u16 | if descriptor { 8 } else { 0 };
        let method = if deflate { 8u16 } else { 0 };
        let mut local = vec![0u8; 30];
        local[0..4].copy_from_slice(&0x04034b50u32.to_le_bytes());
        local[4..6].copy_from_slice(&20u16.to_le_bytes());
        local[6..8].copy_from_slice(&flags.to_le_bytes());
        local[8..10].copy_from_slice(&method.to_le_bytes());
        if !descriptor {
            local[14..18].copy_from_slice(&crc.to_le_bytes());
            local[18..22].copy_from_slice(&(payload.len() as u32).to_le_bytes());
            local[22..26].copy_from_slice(&(plain.len() as u32).to_le_bytes());
        }
        local[26..28].copy_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend(local);
        bytes.extend(name.as_bytes());
        bytes.extend(&payload);
        if descriptor {
            bytes.extend(0x08074b50u32.to_le_bytes());
            bytes.extend(crc.to_le_bytes());
            bytes.extend((payload.len() as u32).to_le_bytes());
            bytes.extend((plain.len() as u32).to_le_bytes());
        }
        let mut central = vec![0u8; 46];
        central[0..4].copy_from_slice(&0x02014b50u32.to_le_bytes());
        central[4..6].copy_from_slice(&20u16.to_le_bytes());
        central[6..8].copy_from_slice(&20u16.to_le_bytes());
        central[8..10].copy_from_slice(&flags.to_le_bytes());
        central[10..12].copy_from_slice(&method.to_le_bytes());
        central[16..20].copy_from_slice(&crc.to_le_bytes());
        central[20..24].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        central[24..28].copy_from_slice(&(plain.len() as u32).to_le_bytes());
        central[28..30].copy_from_slice(&(name.len() as u16).to_le_bytes());
        central[42..46].copy_from_slice(&offset.to_le_bytes());
        directory.extend(central);
        directory.extend(name.as_bytes());
    }
    let offset = bytes.len() as u32;
    let length = directory.len() as u32;
    bytes.extend(directory);
    let mut end = vec![0u8; 22];
    end[0..4].copy_from_slice(&0x06054b50u32.to_le_bytes());
    end[8..10].copy_from_slice(&(entries.len() as u16).to_le_bytes());
    end[10..12].copy_from_slice(&(entries.len() as u16).to_le_bytes());
    end[12..16].copy_from_slice(&length.to_le_bytes());
    end[16..20].copy_from_slice(&offset.to_le_bytes());
    bytes.extend(end);
    bytes
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    fn open(bytes: &[u8], budget: &ByteBudget, cancel: Cancel<'_>) -> Result<ZipSource> {
        ZipSource::open(
            SourceBytes::from_slice(bytes, 256 << 20, budget, cancel)?,
            Some(Digest256::of(bytes)),
            SourceLimits::default(),
            budget,
            cancel,
        )
    }
    #[test]
    fn stored_deflate_descriptors_empty_members_and_crc_match_original_bytes() {
        let budget = ByteBudget::new(64 << 20).unwrap();
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let large = vec![37u8; 190000];
        for deflate in [false, true] {
            for descriptor in [false, true] {
                let bytes = synthetic_zip(
                    &[("Pack/empty", b""), ("Pack/data", &large)],
                    deflate,
                    descriptor,
                );
                let archive = open(&bytes, &budget, cancel).unwrap();
                assert_eq!(archive.verify_all(&budget, cancel).unwrap(), 2);
                let value = archive
                    .read(
                        &AssetPath::parse("Pack/data").unwrap(),
                        1 << 20,
                        &budget,
                        cancel,
                    )
                    .unwrap()
                    .unwrap();
                assert_eq!(value.bytes(), large);
                assert_eq!(value.digest(), Digest256::of(&large));
                assert_eq!(archive.source_digest(), Some(Digest256::of(&bytes)));
            }
        }
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn traversal_duplicates_and_file_directory_collisions_fail_before_reading_pixels() {
        let budget = ByteBudget::new(4 << 20).unwrap();
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        for bytes in [
            synthetic_zip(&[("../escape", b"x")], false, false),
            synthetic_zip(&[("a", b"x"), ("a", b"y")], false, false),
            synthetic_zip(&[("a", b"x"), ("a/b", b"y")], false, false),
            synthetic_zip(&[("C:/x", b"x")], false, false),
        ] {
            assert!(open(&bytes, &budget, cancel).is_err());
        }
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn corrupt_payload_or_declared_expansion_is_not_an_absent_resource() {
        let budget = ByteBudget::new(4 << 20).unwrap();
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let mut bytes = synthetic_zip(&[("a", b"abc")], false, false);
        bytes[31] ^= 1;
        let archive = open(&bytes, &budget, cancel).unwrap();
        assert!(archive
            .read(&AssetPath::parse("a").unwrap(), 32, &budget, cancel)
            .is_err());
        assert!(archive
            .read(&AssetPath::parse("missing").unwrap(), 32, &budget, cancel)
            .unwrap()
            .is_none());
        let mut bytes = synthetic_zip(&[("a", b"abcabcabcabc")], true, false);
        let central = u32_at(&bytes, bytes.len() - 6).unwrap() as usize;
        bytes[22..26].copy_from_slice(&1u32.to_le_bytes());
        bytes[central + 24..central + 28].copy_from_slice(&1u32.to_le_bytes());
        let archive = open(&bytes, &budget, cancel).unwrap();
        assert!(archive
            .read(&AssetPath::parse("a").unwrap(), 32, &budget, cancel)
            .is_err());
    }
    #[test]
    fn central_local_disagreements_encryption_and_overlapping_payloads_are_rejected() {
        let budget = ByteBudget::new(4 << 20).unwrap();
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let original = synthetic_zip(&[("a", b"abc"), ("b", b"def")], false, false);
        let central = u32_at(&original, original.len() - 6).unwrap() as usize;
        let mut renamed = original.clone();
        renamed[30] = b'z';
        assert!(open(&renamed, &budget, cancel).is_err());
        let mut encrypted = original.clone();
        encrypted[central + 8] |= 1;
        assert!(matches!(
            open(&encrypted, &budget, cancel),
            Err(AssetError::Unsupported(_))
        ));
        let mut overlapping = original.clone();
        overlapping[central + 47 + 42..central + 47 + 46].copy_from_slice(&0u32.to_le_bytes());
        assert!(open(&overlapping, &budget, cancel).is_err());
        let mut zip64 = original;
        let end = zip64.len() - 22;
        zip64[end + 10..end + 12].copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(matches!(
            open(&zip64, &budget, cancel),
            Err(AssetError::Unsupported(_))
        ));
    }
    #[test]
    fn archive_size_count_budget_cancellation_and_foreign_accounts_are_enforced() {
        let budget = ByteBudget::new(4 << 20).unwrap();
        let other = ByteBudget::new(4 << 20).unwrap();
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let bytes = synthetic_zip(&[("a", b"abc")], true, true);
        let archive = open(&bytes, &budget, cancel).unwrap();
        assert!(archive
            .read(&AssetPath::parse("a").unwrap(), 2, &budget, cancel)
            .is_err());
        assert!(archive
            .read(&AssetPath::parse("a").unwrap(), 3, &other, cancel)
            .is_err());
        stop.store(true, std::sync::atomic::Ordering::Release);
        assert!(matches!(
            archive.verify_all(&budget, cancel),
            Err(AssetError::Cancelled)
        ));
    }
    #[test]
    fn cp437_is_not_lossy_utf8_or_an_incorrect_box_drawing_table() {
        assert_eq!(name_text(&[0x82, 0xd3], false).unwrap(), "é╙");
        assert!(name_text(&[0xff], true).is_err());
    }
}
