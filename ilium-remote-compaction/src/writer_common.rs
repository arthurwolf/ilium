//! The crash-safe rewrite shared by both agent writers: stale-file check,
//! backup, temp file, verification, atomic rename and backup pruning.

use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use chrono::{Duration, SecondsFormat, Utc};

use crate::error::CompactionError;
use crate::transcript_io::{read_transcript_bytes, FileSnapshot, TailShape};

const MAX_APPENDED_VERIFY_BYTES: usize = 16 * 1024 * 1024;
type AppendedVerifier<'a> = dyn Fn(&Path, u64, &[u8]) -> Result<(), String> + 'a;

/// What a successful rewrite produced.
pub(crate) struct Committed {
    pub(crate) backup_path: PathBuf,
    pub(crate) pruned_backups: usize,
}

/// ISO-8601 UTC with milliseconds, the timestamp shape both agents write.
pub(crate) fn iso_timestamp(offset_millis: i64) -> String {
    (Utc::now() + Duration::milliseconds(offset_millis))
        .to_rfc3339_opts(SecondsFormat::Millis, true)
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

fn write_error(path: &Path) -> impl FnOnce(std::io::Error) -> CompactionError + '_ {
    move |error| CompactionError::Write {
        path: path.to_path_buf(),
        error,
    }
}

fn backup_prefix(file_name: &str) -> String {
    format!("{file_name}.pre-compaction-")
}

fn unique_backup_path(directory: &Path, file_name: &str) -> PathBuf {
    let stamp = Utc::now().format("%Y%m%d%H%M%S");
    let prefix = backup_prefix(file_name);
    let first = directory.join(format!("{prefix}{stamp}.bak"));
    if !first.exists() {
        return first;
    }
    (1..)
        .map(|counter| directory.join(format!("{prefix}{stamp}-{counter}.bak")))
        .find(|candidate| !candidate.exists())
        .unwrap_or(first)
}

/// Creates `path` (which must not exist), gives it `permissions` before any
/// content is written, writes `bytes` and flushes them to disk.
fn write_new_file(
    path: &Path,
    bytes: &[u8],
    permissions: &fs::Permissions,
) -> Result<(), CompactionError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(write_error(path))?;
    fs::set_permissions(path, permissions.clone()).map_err(write_error(path))?;
    file.write_all(bytes).map_err(write_error(path))?;
    file.sync_all().map_err(write_error(path))?;
    Ok(())
}

/// Parses only records written by the current rewrite. The retained prefix was
/// already parsed before planning, so reparsing it would duplicate the largest
/// allocation without strengthening these new-record checks.
pub(crate) fn read_appended_records(
    path: &Path,
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
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    file.seek(SeekFrom::Start(appended_offset))
        .map_err(|error| error.to_string())?;
    let mut remaining = expected_bytes;
    let mut buffer = [0_u8; 16 * 1024];
    while !remaining.is_empty() {
        let amount = remaining.len().min(buffer.len());
        file.read_exact(&mut buffer[..amount])
            .map_err(|error| format!("could not read appended records back: {error}"))?;
        if buffer[..amount] != remaining[..amount] {
            return Err("the rewritten appended bytes differ from the prepared records".into());
        }
        remaining = &remaining[amount..];
    }
    let mut trailing = [0_u8; 1];
    if file
        .read(&mut trailing)
        .map_err(|error| error.to_string())?
        != 0
    {
        return Err("the rewritten transcript has unexpected trailing bytes".into());
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

/// Removes the oldest backups of `file_name` beyond `keep` (at least one).
fn prune_backups(directory: &Path, file_name: &str, keep: usize) -> usize {
    let prefix = backup_prefix(file_name);
    let Ok(entries) = fs::read_dir(directory) else {
        return 0;
    };
    let mut backups: Vec<(std::time::SystemTime, String, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !(name.starts_with(&prefix) && name.ends_with(".bak")) {
                return None;
            }
            let modified = entry.metadata().and_then(|meta| meta.modified()).ok()?;
            Some((modified, name, entry.path()))
        })
        .collect();
    backups.sort_by(|left, right| (right.0, &right.1).cmp(&(left.0, &left.1)));
    backups
        .into_iter()
        .skip(keep.max(1))
        .filter(|(_, _, path)| fs::remove_file(path).is_ok())
        .count()
}

/// Replaces the transcript at `path` with its first `shape.keep_len` bytes
/// followed by `appended`, after the original proved unchanged since it was
/// read and the persisted appended bytes passed `verify`.
///
/// On any error the original is untouched and the files this call created
/// (temp file, backup) are removed.
pub(crate) fn commit_rewrite(
    path: &Path,
    snapshot: FileSnapshot,
    shape: TailShape,
    appended: &[u8],
    keep_backups: usize,
    verify: &AppendedVerifier<'_>,
) -> Result<Committed, CompactionError> {
    let current = read_transcript_bytes(path)?;
    if FileSnapshot::of(&current) != snapshot {
        return Err(CompactionError::TranscriptChanged {
            path: path.to_path_buf(),
        });
    }
    let metadata = fs::metadata(path).map_err(|error| CompactionError::Unreadable {
        path: path.to_path_buf(),
        error,
    })?;
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    let backup_path = unique_backup_path(directory, &file_name);
    write_new_file(&backup_path, &current, &metadata.permissions())?;

    let temp_path = directory.join(format!(
        ".{file_name}.compact-{}.tmp",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    ));
    let outcome = write_and_swap(
        path,
        &temp_path,
        &current,
        shape,
        appended,
        &metadata.permissions(),
        verify,
    );
    if let Err(error) = outcome {
        let _ = fs::remove_file(&temp_path);
        let _ = fs::remove_file(&backup_path);
        return Err(error);
    }
    if let Ok(directory_handle) = fs::File::open(directory) {
        // Best effort: not every platform can fsync a directory handle.
        let _ = directory_handle.sync_all();
    }
    let pruned_backups = prune_backups(directory, &file_name, keep_backups);
    Ok(Committed {
        backup_path,
        pruned_backups,
    })
}

fn write_and_swap(
    path: &Path,
    temp_path: &Path,
    current: &[u8],
    shape: TailShape,
    appended: &[u8],
    permissions: &fs::Permissions,
    verify: &AppendedVerifier<'_>,
) -> Result<(), CompactionError> {
    if appended.len() > MAX_APPENDED_VERIFY_BYTES {
        return Err(CompactionError::Verification(format!(
            "appended records exceed the {} MiB verification limit",
            MAX_APPENDED_VERIFY_BYTES / (1024 * 1024)
        )));
    }
    let appended_offset = shape.keep_len + usize::from(shape.needs_newline);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp_path)
        .map_err(write_error(temp_path))?;
    fs::set_permissions(temp_path, permissions.clone()).map_err(write_error(temp_path))?;
    file.write_all(&current[..shape.keep_len])
        .map_err(write_error(temp_path))?;
    if shape.needs_newline {
        file.write_all(b"\n").map_err(write_error(temp_path))?;
    }
    file.write_all(appended).map_err(write_error(temp_path))?;
    file.sync_all().map_err(write_error(temp_path))?;
    verify(temp_path, appended_offset as u64, appended).map_err(CompactionError::Verification)?;

    // Last stale check before the swap: the agent must not have appended.
    let length_now = fs::metadata(path)
        .map_err(|error| CompactionError::Unreadable {
            path: path.to_path_buf(),
            error,
        })?
        .len();
    if length_now != current.len() as u64 {
        return Err(CompactionError::TranscriptChanged {
            path: path.to_path_buf(),
        });
    }
    fs::rename(temp_path, path).map_err(write_error(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript_io::tail_shape;

    fn snapshot_of(path: &Path) -> (FileSnapshot, TailShape) {
        let bytes = fs::read(path).expect("read");
        (FileSnapshot::of(&bytes), tail_shape(&bytes))
    }

    #[test]
    fn rewrite_appends_backs_up_and_prunes() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("s.jsonl");
        fs::write(&path, "{\"a\":1}\n").expect("write");
        for round in 0..4 {
            let (snapshot, shape) = snapshot_of(&path);
            let committed =
                commit_rewrite(&path, snapshot, shape, b"{\"n\":1}\n", 2, &|_, _, _| Ok(()))
                    .unwrap_or_else(|error| panic!("round {round}: {error}"));
            assert!(committed.backup_path.exists());
        }
        let backups = fs::read_dir(dir.path())
            .expect("list")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".bak"))
            .count();
        assert_eq!(backups, 2);
        let text = fs::read_to_string(&path).expect("read");
        assert_eq!(text.lines().count(), 5);
        assert!(!fs::read_dir(dir.path())
            .expect("list")
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().ends_with(".tmp")));
    }

    #[test]
    fn a_torn_last_line_is_dropped_and_a_missing_newline_is_added() {
        let dir = tempfile::tempdir().expect("dir");
        let torn = dir.path().join("torn.jsonl");
        fs::write(&torn, "{\"a\":1}\n{\"b\":").expect("write");
        let (snapshot, shape) = snapshot_of(&torn);
        commit_rewrite(&torn, snapshot, shape, b"{\"n\":1}\n", 3, &|_, _, _| Ok(()))
            .expect("commit");
        assert_eq!(
            fs::read_to_string(&torn).expect("read"),
            "{\"a\":1}\n{\"n\":1}\n"
        );

        let bare = dir.path().join("bare.jsonl");
        fs::write(&bare, "{\"a\":1}").expect("write");
        let (snapshot, shape) = snapshot_of(&bare);
        commit_rewrite(&bare, snapshot, shape, b"{\"n\":1}\n", 3, &|_, _, _| Ok(()))
            .expect("commit");
        assert_eq!(
            fs::read_to_string(&bare).expect("read"),
            "{\"a\":1}\n{\"n\":1}\n"
        );
    }

    #[test]
    fn a_changed_file_is_refused_and_nothing_is_left_behind() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("s.jsonl");
        fs::write(&path, "{\"a\":1}\n").expect("write");
        let (snapshot, shape) = snapshot_of(&path);
        fs::write(&path, "{\"a\":1}\n{\"late\":true}\n").expect("agent appends");
        let error = commit_rewrite(&path, snapshot, shape, b"{}\n", 3, &|_, _, _| Ok(()))
            .err()
            .expect("refused");
        assert!(matches!(error, CompactionError::TranscriptChanged { .. }));
        assert_eq!(fs::read_dir(dir.path()).expect("list").count(), 1);
    }

    #[test]
    fn a_failed_verification_leaves_the_original_and_no_files() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("s.jsonl");
        fs::write(&path, "{\"a\":1}\n").expect("write");
        let (snapshot, shape) = snapshot_of(&path);
        let error = commit_rewrite(&path, snapshot, shape, b"{}\n", 3, &|_, _, _| {
            Err("broken".into())
        })
        .err()
        .expect("refused");
        assert!(matches!(error, CompactionError::Verification(_)));
        assert_eq!(fs::read_to_string(&path).expect("read"), "{\"a\":1}\n");
        assert_eq!(fs::read_dir(dir.path()).expect("list").count(), 1);
    }

    #[test]
    fn appended_readback_checks_only_the_new_suffix() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("s.jsonl");
        let prefix = vec![b'x'; 4 * 1024 * 1024];
        let appended = b"{\"type\":\"compacted\"}\n";
        let mut file = fs::File::create(&path).expect("create");
        file.write_all(&prefix).expect("write prefix");
        file.write_all(appended).expect("write suffix");
        drop(file);

        let records = read_appended_records(&path, prefix.len() as u64, 1, appended)
            .expect("read back appended suffix");
        assert_eq!(records[0]["type"], "compacted");
        assert!(read_appended_records(&path, prefix.len() as u64, 1, b"{}\n").is_err());
    }

    #[test]
    fn appended_verification_refuses_oversized_payloads() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("s.jsonl");
        fs::write(&path, "{}\n").expect("write");
        let (snapshot, shape) = snapshot_of(&path);
        let appended = vec![b' '; MAX_APPENDED_VERIFY_BYTES + 1];
        let error = commit_rewrite(&path, snapshot, shape, &appended, 1, &|_, _, _| Ok(()))
            .err()
            .expect("oversized suffix must be refused");
        assert!(matches!(error, CompactionError::Verification(_)));
        assert_eq!(fs::read_to_string(&path).expect("original remains"), "{}\n");
    }
}
