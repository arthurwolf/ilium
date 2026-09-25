//! Private rolling copies of native session JSON snapshots.

use std::collections::HashSet;
use std::fs;
use std::io::{self, copy};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Datelike, Duration as ChronoDuration, Months, Utc};
use ilium_platform::secure_fs;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::state::ServerState;

const HALF_HOUR_SECONDS: i64 = 30 * 60;
const RETRY_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Clone)]
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
    let snapshot_path = state.snapshot_path.clone();
    let project_root = state.session_cwd.clone();
    let session_name = state.session_name.clone();
    tokio::task::spawn_blocking(move || capture(&snapshot_path, &project_root, &session_name, now))
        .await
        .map_err(|error| io::Error::other(format!("backup worker failed: {error}")))?
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
    let mut candidates = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(captured_at) = parse_candidate_name(&name) else {
            continue;
        };
        candidates.push(Candidate {
            path: entry.path(),
            captured_at,
        });
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

fn deletion_paths(now: DateTime<Utc>, candidates: &[Candidate]) -> Vec<PathBuf> {
    const LIMITS: [usize; 5] = [48, 7, 4, 12, usize::MAX];
    let mut ordered = candidates.to_vec();
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
            deletions.push(candidate.path);
            continue;
        }
        seen.insert(bucket);
        counts[tier] += 1;
    }
    deletions
}

fn prune(directory: &Path, now: DateTime<Utc>) -> io::Result<()> {
    let candidates = read_candidates(directory)?;
    for path in deletion_paths(now, &candidates) {
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
}
