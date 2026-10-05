//! Private rolling copies of native session JSON snapshots.

use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io::{self, copy};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Datelike, Duration as ChronoDuration, Months, Utc};
use ilium_execution::{JobCost, Lane, Retention};
use ilium_platform::secure_fs;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::execution::{ExecutionClient, ExecutionError};
use crate::state::ServerState;

const HALF_HOUR_SECONDS: i64 = 30 * 60;
const RETRY_INTERVAL: Duration = Duration::from_secs(60);
// The scan must finish before any deletion. A refusal keeps every published
// backup and retries pruning after the next successful capture.
const MAX_RETENTION_ENTRIES: usize = 16_384;
const MAX_RETENTION_CANDIDATES: usize = 4_096;
const MAX_BACKUP_PATH_BYTES: usize = 8 * 1024;
const MAX_BACKUP_NAME_BYTES: usize = 512;
// Includes "/.ilium/backups/" and its separators.
const BACKUP_DIRECTORY_COMPONENT_BYTES: usize = 16;
// One admitted candidate path is retained per matching entry. Sorting and
// deletion plans borrow those paths; this allowance covers their vector
// growth, hash buckets, and headers without charging a fixed 128 MiB per job.
const RETENTION_METADATA_BYTES_PER_CANDIDATE: usize = 1024;
const BACKUP_FIXED_SCRATCH_BYTES: usize = 4 * 1024 * 1024;
const BACKUP_RESULT_BYTES: usize = 16 * 1024;

struct ChargedBackupError {
    source: io::Error,
    _retention: Retention,
}

impl fmt::Debug for ChargedBackupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ChargedBackupError")
            .field("source", &self.source)
            .finish()
    }
}

impl fmt::Display for ChargedBackupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.source, formatter)
    }
}

impl std::error::Error for ChargedBackupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

struct Candidate {
    path: PathBuf,
    captured_at: DateTime<Utc>,
}

#[derive(Debug, Hash, PartialEq, Eq)]
enum Bucket {
    HalfHour(i64),
    Day(chrono::NaiveDate),
    Week(i32, u32),
    Month(i32, u32),
    Year(i32),
}

fn half_hour_bucket(time: DateTime<Utc>) -> i64 {
    time.timestamp().div_euclid(HALF_HOUR_SECONDS)
}

fn should_capture(enabled: bool, previous_bucket: Option<i64>, now: DateTime<Utc>) -> bool {
    enabled && previous_bucket != Some(half_hour_bucket(now))
}

/// Preserve an existing native snapshot before normalization or StartFresh
/// can replace it. A missing file on first launch needs no backup.
pub(crate) async fn capture_on_start(state: &ServerState) -> Option<i64> {
    if !state.session_backups_enabled() {
        return None;
    }
    let now = Utc::now();
    match capture_at(state, now).await {
        Ok(true) => Some(half_hour_bucket(now)),
        Ok(false) => None,
        Err(error) => {
            tracing::warn!("initial session backup failed: {error}");
            None
        }
    }
}

/// Owns one background copy loop for this session. Failed/missing captures
/// retry in the same bucket; a successful capture happens at most once per
/// half-hour bucket. Toggling off stops future copies; toggling on requests
/// a fresh capture without waiting for the next boundary.
pub(crate) fn spawn(state: Arc<ServerState>, initial_bucket: Option<i64>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut enabled_changes = state.watch_session_backups_enabled();
        let mut previous_bucket = initial_bucket;
        let mut interval = tokio::time::interval(RETRY_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = interval.tick() => {}
                result = enabled_changes.changed() => {
                    if result.is_err() {
                        break;
                    }
                    previous_bucket = None;
                }
            }

            let now = Utc::now();
            if !should_capture(state.session_backups_enabled(), previous_bucket, now) {
                continue;
            }
            match capture_at(&state, now).await {
                Ok(true) => previous_bucket = Some(half_hour_bucket(now)),
                Ok(false) => {}
                Err(error) => tracing::warn!("session backup failed: {error}"),
            }
        }
    })
}

async fn capture_at(state: &ServerState, now: DateTime<Utc>) -> io::Result<bool> {
    let client = &state
        .execution
        .get()
        .ok_or_else(|| io::Error::other("session backup execution is not initialized"))?
        .client;
    capture_with(
        client,
        &state.snapshot_path,
        &state.session_cwd,
        &state.session_name,
        now,
        capture,
    )
    .await
}

async fn capture_with<F>(
    client: &ExecutionClient,
    snapshot_path: &Path,
    project_root: &Path,
    session_name: &str,
    now: DateTime<Utc>,
    capture_fn: F,
) -> io::Result<bool>
where
    F: FnOnce(&Path, &Path, &str, DateTime<Utc>) -> io::Result<bool> + Send + 'static,
{
    // Compute the full native declaration from borrowed lengths, and clone
    // state only after the existing bank admits that job and its scratch.
    let cost = capture_job_cost(snapshot_path, project_root, session_name)?;
    let reservation = client
        .reserve(Lane::Io, cost)
        .await
        .map_err(|reason| io::Error::other(format!("session backup admission: {reason:?}")))?;
    let snapshot_path = snapshot_path.to_path_buf();
    let project_root = project_root.to_path_buf();
    let session_name = session_name.to_owned();
    match client
        .run_reserved(reservation, move |_context| {
            capture_fn(&snapshot_path, &project_root, &session_name, now)
        })
        .await
    {
        Ok(completion) => Ok(*completion.view()),
        Err(ExecutionError::Failed(failure)) => {
            let (source, retention) = failure.into_parts();
            Err(io::Error::new(
                source.kind(),
                ChargedBackupError {
                    source,
                    _retention: retention,
                },
            ))
        }
        Err(error) => Err(io::Error::other(format!("backup worker failed: {error}"))),
    }
}

fn capture_job_cost(
    snapshot_path: &Path,
    project_root: &Path,
    session_name: &str,
) -> io::Result<JobCost> {
    let source_bytes = snapshot_path.as_os_str().len();
    let project_bytes = project_root.as_os_str().len();
    let session_bytes = session_name.len();
    if session_bytes > MAX_BACKUP_NAME_BYTES {
        return Err(backup_path_limit_error());
    }
    // Four encoded bytes per UTF-8 byte conservatively cover platform path
    // representation. Include the directory separators and largest filename
    // the retention scanner will accept, before allocating any copied path.
    let max_candidate_path_bytes = session_bytes
        .checked_mul(4)
        .and_then(|bytes| bytes.checked_add(project_bytes))
        .and_then(|bytes| bytes.checked_add(BACKUP_DIRECTORY_COMPONENT_BYTES))
        .and_then(|bytes| bytes.checked_add(MAX_BACKUP_NAME_BYTES + 1))
        .ok_or_else(backup_path_limit_error)?;
    if source_bytes > MAX_BACKUP_PATH_BYTES || max_candidate_path_bytes > MAX_BACKUP_PATH_BYTES {
        return Err(backup_path_limit_error());
    }
    let candidate_path_bytes = max_candidate_path_bytes
        .checked_mul(MAX_RETENTION_CANDIDATES)
        .ok_or_else(backup_path_limit_error)?;
    let metadata_bytes = MAX_RETENTION_CANDIDATES
        .checked_mul(RETENTION_METADATA_BYTES_PER_CANDIDATE)
        .ok_or_else(backup_path_limit_error)?;
    let input_bytes = candidate_path_bytes
        .checked_add(metadata_bytes)
        .and_then(|bytes| bytes.checked_add(source_bytes))
        .and_then(|bytes| bytes.checked_add(project_bytes))
        .and_then(|bytes| bytes.checked_add(session_bytes))
        .and_then(|bytes| bytes.checked_add(BACKUP_FIXED_SCRATCH_BYTES))
        .ok_or_else(backup_path_limit_error)?;
    Ok(JobCost {
        input_bytes,
        result_bytes: BACKUP_RESULT_BYTES,
    })
}

fn backup_path_limit_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "session backup path exceeds bounded native allocation",
    )
}

fn capture(
    snapshot_path: &Path,
    project_root: &Path,
    session_name: &str,
    captured_at: DateTime<Utc>,
) -> io::Result<bool> {
    let ilium_dir = project_root.join(".ilium");
    let sessions_dir = ilium_dir.join("sessions");
    if snapshot_path.parent() != Some(sessions_dir.as_path())
        || snapshot_path.file_stem().and_then(|name| name.to_str()) != Some(session_name)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "snapshot path does not match this project's session storage path",
        ));
    }
    ensure_private_directory(&ilium_dir)?;
    ensure_private_directory(&sessions_dir)?;

    let mut source = match secure_fs::private_open_options()
        .read(true)
        .open(snapshot_path)
    {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if !source.metadata()?.is_file() {
        return Err(io::Error::other("session snapshot is not a regular file"));
    }

    let backup_dir = backup_directory(project_root, session_name)?;
    ensure_private_directory(&project_root.join(".ilium").join("backups"))?;
    ensure_private_directory(&backup_dir)?;

    let unique_id = Uuid::new_v4();
    let temporary_path = backup_dir.join(format!(".pending-{unique_id}.tmp"));
    let final_path = backup_dir.join(format!(
        "{}-{unique_id}.json",
        captured_at.timestamp_millis()
    ));
    let mut destination = secure_fs::private_open_options()
        .write(true)
        .create_new(true)
        .open(&temporary_path)?;
    let result = copy(&mut source, &mut destination).and_then(|_| destination.sync_all());
    drop(destination);
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary_path);
        return Err(error);
    }

    // A hard link publishes the complete synced file atomically and refuses
    // to replace an existing history entry.
    let publication = fs::hard_link(&temporary_path, &final_path);
    let _ = fs::remove_file(&temporary_path);
    publication?;
    if let Err(error) = prune(&backup_dir, captured_at) {
        tracing::warn!(
            backup_directory = %backup_dir.display(),
            "saved session backup but retention pruning failed: {error}"
        );
    }
    Ok(true)
}

fn ensure_private_directory(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            secure_fs::restrict_directory_to_owner(path)
        }
        Ok(_) => Err(io::Error::other(
            "refusing symlink or non-directory backup path",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            secure_fs::create_private_directory(path)
        }
        Err(error) => Err(error),
    }
}

fn backup_directory(project_root: &Path, session_name: &str) -> io::Result<PathBuf> {
    let mut components = Path::new(session_name).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(io::Error::other("invalid session backup directory name"));
    }
    Ok(project_root
        .join(".ilium")
        .join("backups")
        .join(session_name))
}

fn parse_candidate_name(name: &str) -> Option<DateTime<Utc>> {
    let without_extension = name.strip_suffix(".json")?;
    let (milliseconds, unique_id) = without_extension.split_once('-')?;
    milliseconds.parse::<i64>().ok()?;
    Uuid::parse_str(unique_id).ok()?;
    DateTime::<Utc>::from_timestamp_millis(milliseconds.parse().ok()?)
}

fn read_candidates(directory: &Path) -> io::Result<Vec<Candidate>> {
    read_candidates_bounded(directory, MAX_RETENTION_ENTRIES, MAX_RETENTION_CANDIDATES)
}

fn read_candidates_bounded(
    directory: &Path,
    max_entries: usize,
    max_candidates: usize,
) -> io::Result<Vec<Candidate>> {
    let mut candidates = Vec::new();
    for (scanned_entries, entry) in fs::read_dir(directory)?.enumerate() {
        if scanned_entries >= max_entries {
            return Err(io::Error::other(format!(
                "session backup retention scan exceeds {max_entries} entries"
            )));
        }
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        if name.len() > MAX_BACKUP_NAME_BYTES {
            return Err(io::Error::other(
                "session backup retention filename exceeds bounded allocation",
            ));
        }
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(captured_at) = parse_candidate_name(name) else {
            continue;
        };
        if candidates.len() >= max_candidates {
            return Err(io::Error::other(format!(
                "session backup retention scan exceeds {max_candidates} candidates"
            )));
        }
        let path_bytes = directory
            .as_os_str()
            .len()
            .checked_add(1)
            .and_then(|length| length.checked_add(name.len()))
            .ok_or_else(|| io::Error::other("session backup retention path length overflow"))?;
        if path_bytes > MAX_BACKUP_PATH_BYTES {
            return Err(io::Error::other(
                "session backup retention path exceeds bounded allocation",
            ));
        }
        // DirEntry::path() may grow its PathBuf beyond the checked length.
        // Reserve the exact declared path capacity before joining these same
        // two components, then refuse any platform-specific capacity excess.
        let mut path = PathBuf::with_capacity(path_bytes);
        path.push(directory);
        path.push(name);
        if path.capacity() > path_bytes {
            return Err(io::Error::other(
                "session backup retention path capacity exceeds admission",
            ));
        }
        candidates.push(Candidate { path, captured_at });
    }
    Ok(candidates)
}

fn bucket_for(now: DateTime<Utc>, captured_at: DateTime<Utc>) -> (usize, Bucket) {
    let age = now.signed_duration_since(captured_at);
    if age < ChronoDuration::days(1) {
        return (0, Bucket::HalfHour(half_hour_bucket(captured_at)));
    }
    if age < ChronoDuration::days(7) {
        return (1, Bucket::Day(captured_at.date_naive()));
    }
    if age < ChronoDuration::days(35) {
        let week = captured_at.iso_week();
        return (2, Bucket::Week(week.year(), week.week()));
    }

    // Thirty-five days gives the weekly tier enough room to cover the
    // four-week window across UTC week boundaries. The monthly tier spans twelve
    // calendar months; older files are reduced to one per UTC year.
    let monthly_start = now - ChronoDuration::days(35);
    let yearly_start = monthly_start
        .checked_sub_months(Months::new(12))
        .unwrap_or(monthly_start - ChronoDuration::days(366));
    if captured_at >= yearly_start {
        return (3, Bucket::Month(captured_at.year(), captured_at.month()));
    }
    (4, Bucket::Year(captured_at.year()))
}

fn deletion_path_references(now: DateTime<Utc>, candidates: &[Candidate]) -> Vec<&Path> {
    const LIMITS: [usize; 5] = [48, 7, 4, 12, usize::MAX];
    let mut ordered = candidates.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        right
            .captured_at
            .cmp(&left.captured_at)
            .then_with(|| right.path.cmp(&left.path))
    });

    let mut seen = HashSet::new();
    let mut counts = [0usize; 5];
    let mut deletions = Vec::new();
    for candidate in ordered {
        if candidate.captured_at > now {
            continue;
        }
        let (tier, bucket) = bucket_for(now, candidate.captured_at);
        if seen.contains(&bucket) || counts[tier] >= LIMITS[tier] {
            deletions.push(candidate.path.as_path());
            continue;
        }
        seen.insert(bucket);
        counts[tier] += 1;
    }
    deletions
}

#[cfg(test)]
fn deletion_paths(now: DateTime<Utc>, candidates: &[Candidate]) -> Vec<PathBuf> {
    deletion_path_references(now, candidates)
        .into_iter()
        .map(Path::to_path_buf)
        .collect()
}

fn prune(directory: &Path, now: DateTime<Utc>) -> io::Result<()> {
    let candidates = read_candidates(directory)?;
    remove_deletions(now, &candidates)
}

#[cfg(test)]
fn prune_bounded(
    directory: &Path,
    now: DateTime<Utc>,
    max_entries: usize,
    max_candidates: usize,
) -> io::Result<()> {
    let candidates = read_candidates_bounded(directory, max_entries, max_candidates)?;
    remove_deletions(now, &candidates)
}

fn remove_deletions(now: DateTime<Utc>, candidates: &[Candidate]) -> io::Result<()> {
    for path in deletion_path_references(now, candidates) {
        fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, minute, 0)
            .single()
            .expect("valid UTC test date")
    }

    fn candidate(name: &str, captured_at: DateTime<Utc>) -> Candidate {
        Candidate {
            path: PathBuf::from(name),
            captured_at,
        }
    }

    #[test]
    fn retention_keeps_recent_half_hours_daily_weekly_monthly_and_yearly_samples() {
        let now = at(2026, 9, 23, 12, 15);
        let recent = (0..48)
            .map(|index| {
                candidate(
                    &format!("h{index}"),
                    now - ChronoDuration::minutes(30 * index),
                )
            })
            .collect::<Vec<_>>();
        assert!(deletion_paths(now, &recent).is_empty());

        let daily_now = at(2026, 9, 23, 0, 1);
        let daily = (0..7)
            .map(|index| {
                let day = 22 - index;
                candidate(&format!("d{day}"), at(2026, 9, day, 0, 1))
            })
            .collect::<Vec<_>>();
        assert!(deletion_paths(daily_now, &daily).is_empty());

        let weekly = (0..5)
            .map(|index| {
                let age_days = 8 + index * 6;
                candidate(&format!("w{index}"), now - ChronoDuration::days(age_days))
            })
            .collect::<Vec<_>>();
        assert_eq!(deletion_paths(now, &weekly).len(), 1);

        let mut monthly = vec![candidate("m-2025-08", at(2025, 8, 20, 12, 0))];
        monthly.extend(
            (9..=12).map(|month| candidate(&format!("m-2025-{month}"), at(2025, month, 1, 12, 0))),
        );
        monthly.extend(
            (1..=8).map(|month| candidate(&format!("m-2026-{month}"), at(2026, month, 1, 12, 0))),
        );
        assert_eq!(deletion_paths(now, &monthly).len(), 1);

        let yearly = (2018..=2024)
            .map(|year| candidate(&format!("y{year}"), at(year, 6, 1, 12, 0)))
            .collect::<Vec<_>>();
        assert!(deletion_paths(now, &yearly).is_empty());
    }

    #[test]
    fn duplicate_buckets_keep_the_newest_and_future_or_unknown_files_are_preserved() {
        let now = at(2026, 9, 23, 12, 15);
        let old = candidate("old", now - ChronoDuration::minutes(20));
        let new = candidate("new", now - ChronoDuration::minutes(16));
        let future = candidate("future", now + ChronoDuration::hours(2));
        assert_eq!(
            deletion_paths(now, &[old, new, future]),
            vec![PathBuf::from("old")]
        );
    }

    #[test]
    fn capture_schedule_retries_missing_buckets_and_respects_disable() {
        let now = at(2026, 9, 23, 12, 15);
        assert!(should_capture(true, None, now));
        assert!(!should_capture(true, Some(half_hour_bucket(now)), now));
        assert!(!should_capture(false, None, now));
        assert!(should_capture(
            true,
            Some(half_hour_bucket(now)),
            now + ChronoDuration::minutes(30)
        ));
    }

    #[tokio::test]
    async fn admission_cost_scales_with_paths_and_refuses_excess_before_submission() {
        let short_root = Path::new("/p");
        let short_snapshot = Path::new("/p/.ilium/sessions/default.json");
        let short_cost =
            capture_job_cost(short_snapshot, short_root, "default").expect("short path cost");
        let longer_root = PathBuf::from(format!("/{}", "nested".repeat(160)));
        let longer_snapshot = longer_root.join(".ilium/sessions/default.json");
        let longer_cost =
            capture_job_cost(&longer_snapshot, &longer_root, "default").expect("longer path cost");
        assert!(short_cost.input_bytes < longer_cost.input_bytes);
        assert!(short_cost.input_bytes < 128 * 1024 * 1024);

        let owner = crate::execution::ServerExecution::start().expect("real server bank");
        let before_jobs = owner.quota_group().snapshot().jobs;
        let excessive_path = PathBuf::from("x".repeat(MAX_BACKUP_PATH_BYTES + 1));
        let error = capture_with(
            &owner.client,
            &excessive_path,
            short_root,
            "default",
            at(2026, 9, 23, 12, 0),
            |_, _, _, _| -> io::Result<bool> {
                panic!("an overlong path must never reach the native callback")
            },
        )
        .await
        .expect_err("overlong path must refuse before bank submission");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(owner.quota_group().snapshot().jobs, before_jobs);
        owner.request_shutdown();
    }

    #[test]
    fn capture_publishes_exact_bytes_without_changing_the_active_file() {
        let root = tempfile::tempdir().expect("temp project");
        let sessions = root.path().join(".ilium").join("sessions");
        fs::create_dir_all(&sessions).expect("create sessions directory");
        let snapshot = sessions.join("default.json");
        fs::write(&snapshot, b"{\"tree\":\"first\"}").expect("write snapshot");

        assert!(
            capture(&snapshot, root.path(), "default", at(2026, 9, 23, 12, 0)).expect("capture")
        );
        assert_eq!(
            fs::read(&snapshot).expect("active snapshot"),
            b"{\"tree\":\"first\"}"
        );
        let backup_dir = backup_directory(root.path(), "default").expect("backup directory");
        let backups = read_candidates(&backup_dir).expect("read backups");
        assert_eq!(backups.len(), 1);
        assert_eq!(
            fs::read(&backups[0].path).expect("backup bytes"),
            b"{\"tree\":\"first\"}"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&backups[0].path)
                    .expect("file metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(&backup_dir)
                    .expect("dir metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }

    #[test]
    fn missing_snapshot_does_not_create_a_backup_file() {
        let root = tempfile::tempdir().expect("temp project");
        let sessions = root.path().join(".ilium").join("sessions");
        fs::create_dir_all(&sessions).expect("create sessions directory");
        let snapshot = sessions.join("default.json");

        assert!(!capture(&snapshot, root.path(), "default", Utc::now())
            .expect("missing snapshot is not an error"));
        let backup_dir = backup_directory(root.path(), "default").expect("backup directory");
        assert!(!backup_dir.exists());
    }

    #[cfg(unix)]
    #[test]
    fn capture_refuses_source_and_backup_directory_symlinks() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().expect("temp root");
        let outside = tempfile::tempdir().expect("outside root");
        let sessions = root.path().join(".ilium").join("sessions");
        fs::create_dir_all(&sessions).expect("create sessions");
        let snapshot = sessions.join("default.json");
        let target = outside.path().join("snapshot.json");
        fs::write(&target, b"outside").expect("write target");
        symlink(&target, &snapshot).expect("create source symlink");
        assert!(capture(&snapshot, root.path(), "default", Utc::now()).is_err());
        fs::remove_file(&snapshot).expect("remove source symlink");
        fs::write(&snapshot, b"{}").expect("write source");
        symlink(outside.path(), root.path().join(".ilium").join("backups"))
            .expect("create backup directory symlink");
        assert!(capture(&snapshot, root.path(), "default", Utc::now()).is_err());
        assert_eq!(fs::read(target).expect("read outside target"), b"outside");
    }

    #[tokio::test]
    async fn admitted_capture_uses_the_real_io_bank_and_preserves_snapshot_bytes() {
        let owner = crate::execution::ServerExecution::start().expect("real server bank");
        let root = tempfile::tempdir().expect("temp project");
        let sessions = root.path().join(".ilium").join("sessions");
        fs::create_dir_all(&sessions).expect("create sessions directory");
        let snapshot = sessions.join("default.json");
        let original = b"{\"tree\":\"banked\"}";
        fs::write(&snapshot, original).expect("write original snapshot");
        let caller_thread = std::thread::current().id();
        assert!(capture_with(
            &owner.client,
            &snapshot,
            root.path(),
            "default",
            at(2026, 9, 23, 12, 0),
            move |path, project, session, now| {
                assert_ne!(std::thread::current().id(), caller_thread);
                capture(path, project, session, now)
            },
        )
        .await
        .expect("banked capture"));
        assert_eq!(fs::read(&snapshot).expect("unchanged snapshot"), original);
        let backup_dir = backup_directory(root.path(), "default").expect("backup directory");
        let backups = read_candidates(&backup_dir).expect("published backups");
        assert_eq!(backups.len(), 1);
        assert_eq!(fs::read(&backups[0].path).expect("backup bytes"), original);
        owner.request_shutdown();
    }

    #[tokio::test]
    async fn aborted_waiter_keeps_blocked_native_capture_admitted_until_return() {
        let owner = crate::execution::ServerExecution::start().expect("real server bank");
        let baseline_jobs = owner.quota_group().snapshot().jobs;
        let client = owner.client.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finished_tx, finished_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let root = tempfile::tempdir().expect("temp project");
        let project_root = root.path().to_path_buf();
        let snapshot = project_root.join(".ilium/sessions/default.json");
        let waiter = tokio::spawn(async move {
            capture_with(
                &client,
                &snapshot,
                &project_root,
                "default",
                at(2026, 9, 23, 12, 0),
                move |_, _, _, _| {
                    let _ = started_tx.send(());
                    release_rx.recv().expect("release blocked native callback");
                    let _ = finished_tx.send(());
                    Ok(true)
                },
            )
            .await
        });
        started_rx.await.expect("native callback started");
        assert_eq!(owner.quota_group().snapshot().jobs, baseline_jobs + 1);
        waiter.abort();
        assert!(matches!(waiter.await, Err(error) if error.is_cancelled()));
        assert_eq!(owner.quota_group().snapshot().jobs, baseline_jobs + 1);
        release_tx.send(()).expect("release native callback");
        finished_rx.await.expect("native callback finished");
        let completed = owner.client.completion_notification();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let notified = completed.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if owner.quota_group().snapshot().jobs == baseline_jobs {
                    break;
                }
                notified.await;
            }
        })
        .await
        .expect("native admission eventually released");
        owner.request_shutdown();
    }

    #[tokio::test]
    async fn native_error_keeps_its_original_kind_and_admission_until_dropped() {
        let owner = crate::execution::ServerExecution::start().expect("real server bank");
        let baseline_jobs = owner.quota_group().snapshot().jobs;
        let root = tempfile::tempdir().expect("temp project");
        let snapshot = root.path().join(".ilium/sessions/default.json");
        let error = capture_with(
            &owner.client,
            &snapshot,
            root.path(),
            "default",
            at(2026, 9, 23, 12, 0),
            |_, _, _, _| {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "original I/O failure",
                ))
            },
        )
        .await
        .expect_err("native error");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(error.to_string(), "original I/O failure");
        assert_eq!(
            error
                .get_ref()
                .and_then(|wrapped| wrapped.source())
                .map(|source| source.to_string())
                .as_deref(),
            Some("original I/O failure")
        );
        assert_eq!(owner.quota_group().snapshot().jobs, baseline_jobs + 1);
        drop(error);
        assert_eq!(owner.quota_group().snapshot().jobs, baseline_jobs);
        owner.request_shutdown();
    }

    #[tokio::test]
    async fn closed_bank_refusal_keeps_original_and_history_for_retry() {
        let root = tempfile::tempdir().expect("temp project");
        let sessions = root.path().join(".ilium").join("sessions");
        fs::create_dir_all(&sessions).expect("create sessions directory");
        let snapshot = sessions.join("default.json");
        let original = b"{\"tree\":\"before-retry\"}";
        fs::write(&snapshot, original).expect("write original snapshot");
        assert!(
            capture(&snapshot, root.path(), "default", at(2026, 9, 23, 12, 0))
                .expect("initial history")
        );
        let backup_dir = backup_directory(root.path(), "default").expect("backup directory");
        let before = read_candidates(&backup_dir).expect("initial history");
        assert_eq!(before.len(), 1);
        let original_history_path = before[0].path.clone();
        let original_history = fs::read(&original_history_path).expect("initial backup bytes");

        let closed_owner = crate::execution::ServerExecution::start().expect("real server bank");
        closed_owner.request_shutdown();
        let refusal = capture_with(
            &closed_owner.client,
            &snapshot,
            root.path(),
            "default",
            at(2026, 9, 23, 12, 30),
            capture,
        )
        .await
        .expect_err("closed bank must refuse before copying");
        assert!(refusal.to_string().contains("Closed"));
        assert_eq!(fs::read(&snapshot).expect("original snapshot"), original);
        assert_eq!(
            fs::read(&original_history_path).expect("original history"),
            original_history
        );
        assert_eq!(
            read_candidates(&backup_dir)
                .expect("history after refusal")
                .len(),
            1
        );

        let retry_owner = crate::execution::ServerExecution::start().expect("retry bank");
        assert!(capture_with(
            &retry_owner.client,
            &snapshot,
            root.path(),
            "default",
            at(2026, 9, 23, 12, 30),
            capture,
        )
        .await
        .expect("retry capture"));
        assert_eq!(fs::read(&snapshot).expect("original snapshot"), original);
        assert_eq!(
            fs::read(&original_history_path).expect("original history"),
            original_history
        );
        assert_eq!(
            read_candidates(&backup_dir)
                .expect("history after retry")
                .len(),
            2
        );
        retry_owner.request_shutdown();
    }

    #[test]
    fn incomplete_candidate_scan_refuses_before_deleting_any_backup() {
        let root = tempfile::tempdir().expect("temp project");
        let backup_dir = backup_directory(root.path(), "default").expect("backup directory");
        fs::create_dir_all(&backup_dir).expect("create backup directory");
        let now = at(2026, 9, 23, 12, 15);
        let first = backup_dir.join(format!(
            "{}-{}.json",
            at(2026, 9, 23, 12, 1).timestamp_millis(),
            Uuid::new_v4()
        ));
        let second = backup_dir.join(format!(
            "{}-{}.json",
            at(2026, 9, 23, 12, 2).timestamp_millis(),
            Uuid::new_v4()
        ));
        fs::write(&first, b"older").expect("first backup");
        fs::write(&second, b"newer").expect("second backup");
        assert_eq!(
            deletion_paths(now, &read_candidates(&backup_dir).expect("complete scan")).len(),
            1
        );
        let refusal = prune_bounded(&backup_dir, now, 2, 1)
            .expect_err("candidate limit must reject incomplete scan");
        assert!(refusal.to_string().contains("candidates"));
        assert_eq!(fs::read(&first).expect("first backup survives"), b"older");
        assert_eq!(fs::read(&second).expect("second backup survives"), b"newer");
    }
}
