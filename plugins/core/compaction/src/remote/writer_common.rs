//! The rewrite shared by both agent writers, computed in memory: stale-transcript check, appended-record
//! verification and the new file contents. The host owns the file: it takes the backup, writes the returned bytes
//! to a temporary file next to the transcript and renames it over the original after re-checking that the original
//! still has the snapshot the rewrite was computed from (manual page, "Job runner").

use std::path::Path;

use chrono::{DateTime, Duration, SecondsFormat};

use crate::remote::error::CompactionError;
use crate::remote::transcript_io::{FileSnapshot, TailShape};

pub(crate) const MAX_APPENDED_VERIFY_BYTES: usize = 16 * 1024 * 1024;
type AppendedVerifier<'a> = dyn Fn(&[u8], u64, &[u8]) -> Result<(), String> + 'a;

/// ISO-8601 UTC with milliseconds, the timestamp shape both agents write. `now_millis` is the run's wall clock
/// handed in by the host; the plugin never reads a clock itself.
pub(crate) fn iso_timestamp(now_millis: i64, offset_millis: i64) -> String {
    DateTime::from_timestamp_millis(now_millis)
        .map(|moment| moment + Duration::milliseconds(offset_millis))
        .map(|moment| moment.to_rfc3339_opts(SecondsFormat::Millis, true))
        .unwrap_or_else(|| "1970-01-01T00:00:00.000Z".to_string())
}

/// A small deterministic generator of record ids (the plugin has no randomness source; the host passes a seed).
pub(crate) struct IdGenerator {
    state: u64,
}

impl IdGenerator {
    pub(crate) fn new(seed: u64) -> Self {
        Self {
            state: seed ^ 0x9E37_79B9_7F4A_7C15,
        }
    }

    fn next_u64(&mut self) -> u64 {
        // SplitMix64: full-period, good avalanche, no allocation.
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut mixed = self.state;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        mixed ^ (mixed >> 31)
    }

    /// A version-4 style UUID in the canonical 8-4-4-4-12 text form.
    pub(crate) fn next_uuid(&mut self) -> String {
        let high = self.next_u64();
        let low = self.next_u64();
        let time_low = (high >> 32) as u32;
        let time_mid = (high >> 16) as u16;
        let time_high = ((high as u16) & 0x0FFF) | 0x4000;
        let clock = (((low >> 48) as u16) & 0x3FFF) | 0x8000;
        let node = low & 0x0000_FFFF_FFFF_FFFF;
        format!("{time_low:08x}-{time_mid:04x}-{time_high:04x}-{clock:04x}-{node:012x}")
    }
}

/// One existing backup file next to the transcript, as the host lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupEntry {
    pub name: String,
    pub modified_millis: i64,
}

fn backup_prefix(file_name: &str) -> String {
    format!("{file_name}.pre-compaction-")
}

/// The file name of the backup the host takes before it overwrites `file_name`: `<name>.pre-compaction-<stamp>.bak`
/// with a counter suffix when a backup of the same second exists already (`existing` lists the names in the folder).
pub fn backup_file_name(file_name: &str, now_millis: i64, existing: &[String]) -> String {
    let stamp = DateTime::from_timestamp_millis(now_millis)
        .map(|moment| moment.format("%Y%m%d%H%M%S").to_string())
        .unwrap_or_else(|| "19700101000000".to_string());
    let prefix = backup_prefix(file_name);
    let first = format!("{prefix}{stamp}.bak");
    if !existing.contains(&first) {
        return first;
    }
    (1..)
        .map(|counter| format!("{prefix}{stamp}-{counter}.bak"))
        .find(|candidate| !existing.contains(candidate))
        .unwrap_or(first)
}

/// The oldest backups of `file_name` beyond `keep` (at least one is always kept), newest first by modification
/// time and then by name, exactly as the kernel pruned them.
pub fn backups_to_prune(file_name: &str, existing: &[BackupEntry], keep: usize) -> Vec<String> {
    let prefix = backup_prefix(file_name);
    let mut backups: Vec<&BackupEntry> = existing
        .iter()
        .filter(|entry| entry.name.starts_with(&prefix) && entry.name.ends_with(".bak"))
        .collect();
    backups.sort_by(|left, right| {
        (right.modified_millis, &right.name).cmp(&(left.modified_millis, &left.name))
    });
    backups
        .into_iter()
        .skip(keep.max(1))
        .map(|entry| entry.name.clone())
        .collect()
}

pub(crate) fn serialize_lines(records: &[serde_json::Value]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for record in records {
        // A `Value` always serializes; fall back to `null` rather than panic.
        let line = serde_json::to_vec(record).unwrap_or_else(|_| b"null".to_vec());
        bytes.extend_from_slice(&line);
        bytes.push(b'\n');
    }
    bytes
}

/// Parses only records written by the current rewrite. The retained prefix was already parsed before planning, so
/// reparsing it would duplicate the largest allocation without strengthening these new-record checks. `rewritten`
/// is the complete new transcript; everything from `appended_offset` must be exactly `expected_bytes`.
pub(crate) fn read_appended_records(
    rewritten: &[u8],
    appended_offset: u64,
    expected_records: usize,
    expected_bytes: &[u8],
) -> Result<Vec<serde_json::Value>, String> {
    if expected_bytes.len() > MAX_APPENDED_VERIFY_BYTES {
        return Err(format!(
            "appended records exceed the {} MiB verification limit",
            MAX_APPENDED_VERIFY_BYTES / (1024 * 1024)
        ));
    }
    let offset = usize::try_from(appended_offset)
        .map_err(|_| "the appended offset does not fit the address space".to_string())?;
    let suffix = rewritten.get(offset..).ok_or_else(|| {
        "could not read appended records back: the file is shorter than expected".to_string()
    })?;
    if suffix.len() > expected_bytes.len() {
        return Err("the rewritten transcript has unexpected trailing bytes".into());
    }
    if suffix != expected_bytes {
        return Err("the rewritten appended bytes differ from the prepared records".into());
    }
    let mut records = Vec::with_capacity(expected_records);
    for line in expected_bytes.split(|byte| *byte == b'\n') {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let record = serde_json::from_slice(line)
            .map_err(|error| format!("an appended record is invalid JSON: {error}"))?;
        records.push(record);
    }
    if records.len() != expected_records {
        return Err(format!(
            "expected {expected_records} appended records, found {}",
            records.len()
        ));
    }
    Ok(records)
}

/// Builds the rewritten transcript: the first `shape.keep_len` bytes of `current` (a torn last line is dropped),
/// a newline when the kept part lacks one, then `appended`. Refuses when `current` is no longer the transcript
/// `snapshot` was taken from and when the persisted appended bytes fail `verify`.
pub(crate) fn rewrite_bytes(
    path: &Path,
    current: &[u8],
    snapshot: FileSnapshot,
    shape: TailShape,
    appended: &[u8],
    verify: &AppendedVerifier<'_>,
) -> Result<Vec<u8>, CompactionError> {
    if FileSnapshot::of(current) != snapshot {
        return Err(CompactionError::TranscriptChanged {
            path: path.to_path_buf(),
        });
    }
    if appended.len() > MAX_APPENDED_VERIFY_BYTES {
        return Err(CompactionError::Verification(format!(
            "appended records exceed the {} MiB verification limit",
            MAX_APPENDED_VERIFY_BYTES / (1024 * 1024)
        )));
    }
    let appended_offset = shape.keep_len + usize::from(shape.needs_newline);
    let mut rewritten = Vec::with_capacity(appended_offset + appended.len());
    rewritten.extend_from_slice(&current[..shape.keep_len.min(current.len())]);
    if shape.needs_newline {
        rewritten.push(b'\n');
    }
    rewritten.extend_from_slice(appended);
    verify(&rewritten, appended_offset as u64, appended).map_err(CompactionError::Verification)?;
    Ok(rewritten)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::transcript_io::tail_shape;

    fn snapshot_of(bytes: &[u8]) -> (FileSnapshot, TailShape) {
        (FileSnapshot::of(bytes), tail_shape(bytes))
    }

    fn accept(_: &[u8], _: u64, _: &[u8]) -> Result<(), String> {
        Ok(())
    }

    /// REQ: PORT:ilium-remote-compaction/src/writer_common.rs::rewrite_appends_backs_up_and_prunes
    #[test]
    fn rewrite_appends_backs_up_and_prunes() {
        // The host owns backups and pruning now; the rewrite itself must append every round and keep the original
        // bytes intact for the host's backup copy.
        let path = Path::new("s.jsonl");
        let mut current = b"{\"a\":1}\n".to_vec();
        for round in 0..4 {
            let original = current.clone();
            let (snapshot, shape) = snapshot_of(&current);
            current = rewrite_bytes(path, &original, snapshot, shape, b"{\"n\":1}\n", &accept)
                .unwrap_or_else(|error| panic!("round {round}: {error}"));
            assert!(current.starts_with(&original));
        }
        let text = String::from_utf8(current).expect("utf-8");
        assert_eq!(text.lines().count(), 5);
    }

    /// REQ: PORT:ilium-remote-compaction/src/writer_common.rs::a_torn_last_line_is_dropped_and_a_missing_newline_is_added
    #[test]
    fn a_torn_last_line_is_dropped_and_a_missing_newline_is_added() {
        let path = Path::new("torn.jsonl");
        let torn = b"{\"a\":1}\n{\"b\":".to_vec();
        let (snapshot, shape) = snapshot_of(&torn);
        let rewritten =
            rewrite_bytes(path, &torn, snapshot, shape, b"{\"n\":1}\n", &accept).expect("rewrite");
        assert_eq!(rewritten, b"{\"a\":1}\n{\"n\":1}\n");

        let bare = b"{\"a\":1}".to_vec();
        let (snapshot, shape) = snapshot_of(&bare);
        let rewritten =
            rewrite_bytes(path, &bare, snapshot, shape, b"{\"n\":1}\n", &accept).expect("rewrite");
        assert_eq!(rewritten, b"{\"a\":1}\n{\"n\":1}\n");
    }

    /// REQ: PORT:ilium-remote-compaction/src/writer_common.rs::a_changed_file_is_refused_and_nothing_is_left_behind
    #[test]
    fn a_changed_file_is_refused_and_nothing_is_left_behind() {
        let path = Path::new("s.jsonl");
        let original = b"{\"a\":1}\n".to_vec();
        let (snapshot, shape) = snapshot_of(&original);
        let changed = b"{\"a\":1}\n{\"late\":true}\n".to_vec();
        let error = rewrite_bytes(path, &changed, snapshot, shape, b"{}\n", &accept)
            .err()
            .expect("refused");
        assert!(matches!(error, CompactionError::TranscriptChanged { .. }));
    }

    /// REQ: PORT:ilium-remote-compaction/src/writer_common.rs::a_failed_verification_leaves_the_original_and_no_files
    #[test]
    fn a_failed_verification_leaves_the_original_and_no_files() {
        let path = Path::new("s.jsonl");
        let original = b"{\"a\":1}\n".to_vec();
        let (snapshot, shape) = snapshot_of(&original);
        let error = rewrite_bytes(path, &original, snapshot, shape, b"{}\n", &|_, _, _| {
            Err("broken".into())
        })
        .err()
        .expect("refused");
        assert!(matches!(error, CompactionError::Verification(_)));
        assert_eq!(original, b"{\"a\":1}\n");
    }

    /// REQ: PORT:ilium-remote-compaction/src/writer_common.rs::appended_readback_checks_only_the_new_suffix
    #[test]
    fn appended_readback_checks_only_the_new_suffix() {
        let mut file = vec![b'x'; 4 * 1024 * 1024];
        let offset = file.len() as u64;
        let appended = b"{\"type\":\"compacted\"}\n";
        file.extend_from_slice(appended);

        let records =
            read_appended_records(&file, offset, 1, appended).expect("read back appended suffix");
        assert_eq!(records[0]["type"], "compacted");
        assert!(read_appended_records(&file, offset, 1, b"{}\n").is_err());
    }

    /// REQ: PORT:ilium-remote-compaction/src/writer_common.rs::appended_verification_refuses_oversized_payloads
    #[test]
    fn appended_verification_refuses_oversized_payloads() {
        let path = Path::new("s.jsonl");
        let original = b"{}\n".to_vec();
        let (snapshot, shape) = snapshot_of(&original);
        let appended = vec![b' '; MAX_APPENDED_VERIFY_BYTES + 1];
        let error = rewrite_bytes(path, &original, snapshot, shape, &appended, &accept)
            .err()
            .expect("refused");
        assert!(matches!(error, CompactionError::Verification(_)));
    }

    #[test]
    fn backup_names_are_stamped_and_never_collide() {
        let name = "s.jsonl";
        let first = backup_file_name(name, 1_700_000_000_000, &[]);
        assert_eq!(first, "s.jsonl.pre-compaction-20231114221320.bak");
        let second = backup_file_name(name, 1_700_000_000_000, std::slice::from_ref(&first));
        assert_eq!(second, "s.jsonl.pre-compaction-20231114221320-1.bak");
    }

    #[test]
    fn pruning_keeps_the_newest_backups_and_at_least_one() {
        let entries: Vec<BackupEntry> = (1..=4)
            .map(|index| BackupEntry {
                name: format!("s.jsonl.pre-compaction-2020010100000{index}.bak"),
                modified_millis: i64::from(index),
            })
            .chain([BackupEntry {
                name: "other.jsonl.pre-compaction-20200101000009.bak".into(),
                modified_millis: 0,
            }])
            .collect();
        let pruned = backups_to_prune("s.jsonl", &entries, 2);
        assert_eq!(
            pruned,
            [
                "s.jsonl.pre-compaction-20200101000002.bak",
                "s.jsonl.pre-compaction-20200101000001.bak"
            ]
        );
        assert_eq!(backups_to_prune("s.jsonl", &entries, 0).len(), 3);
    }

    #[test]
    fn record_ids_are_deterministic_unique_and_uuid_shaped() {
        let mut first = IdGenerator::new(7);
        let mut second = IdGenerator::new(7);
        let ids: Vec<String> = (0..50).map(|_| first.next_uuid()).collect();
        for id in &ids {
            assert_eq!(id, &second.next_uuid());
            assert_eq!(id.len(), 36);
            assert_eq!(id.matches('-').count(), 4);
            assert_eq!(&id[14..15], "4");
        }
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len());
    }

    #[test]
    fn timestamps_are_utc_milliseconds_and_offsets_increase() {
        assert_eq!(
            iso_timestamp(1_700_000_000_000, 0),
            "2023-11-14T22:13:20.000Z"
        );
        assert_eq!(
            iso_timestamp(1_700_000_000_000, 5),
            "2023-11-14T22:13:20.005Z"
        );
    }
}
