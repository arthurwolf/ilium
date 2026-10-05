//! Background scan of past agent sessions, used to calibrate "a lot".
//!
//! Real transcript stores reach tens of gigabytes, so the scan never parses a
//! whole file. Every session transcript already ends with (Claude Code) a
//! `cost-state` snapshot carrying the CLI's own dollar total, or (Codex) a
//! cumulative `total_token_usage`, so reading a bounded tail of each file is
//! enough. Results are cached by path, size and modification time, which makes
//! every scan after the first one cost a directory walk.
//!
//! One ordered finite job uses the shared I/O bank and a typed receipt.
//! The UI never waits on filesystem work.

use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt, RetirementReservation,
    RetiringArc,
};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use regex::bytes::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cost_model::PriceTable;
use crate::session_stats::TokenTotals;

const CACHE_VERSION: u32 = 2;
/// Tail read first; covers almost every transcript's final records.
const TAIL_BYTES: u64 = 4 * 1024 * 1024;
/// Second, wider attempt for files whose tail holds only bulky records.
const WIDE_TAIL_BYTES: u64 = 48 * 1024 * 1024;
const CODEX_HEAD_BYTES: u64 = 256 * 1024;
/// Sessions below this are noise (opened and abandoned), not calibration data.
const MINIMUM_SESSION_USD: f64 = 0.01;
/// Sessions that used less quota than this many percentage points are noise.
const MINIMUM_SESSION_QUOTA_POINTS: f64 = 0.05;
/// A finished scan is refreshed at most this often.
pub const RESCAN_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// What one transcript file contributes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HistorySource {
    /// Dollars the agent CLI recorded itself.
    Reported { usd: f64 },
    /// Cumulative tokens per model, priced with the current table.
    Tokens { models: Vec<(String, TokenTotals)> },
    /// No usable total in the file.
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub path: String,
    pub size: u64,
    pub mtime_ms: i64,
    pub source: HistorySource,
    /// Percentage points of each Codex quota window the session used up,
    /// from its first to its last rate-limit reading. Empty for Claude Code.
    #[serde(default)]
    pub quota: Vec<(String, f64)>,
}

impl HistoryEntry {
    fn retained_bytes(&self) -> usize {
        self.path.capacity()
            + self.quota.capacity() * std::mem::size_of::<(String, f64)>()
            + self
                .quota
                .iter()
                .map(|(name, _)| name.capacity())
                .sum::<usize>()
            + match &self.source {
                HistorySource::Tokens { models } => {
                    models.capacity() * std::mem::size_of::<(String, TokenTotals)>()
                        + models
                            .iter()
                            .map(|(name, _)| name.capacity())
                            .sum::<usize>()
                }
                _ => 0,
            }
    }
    /// Session dollars under `prices`, or `None` when unknown or negligible.
    pub fn usd(&self, prices: &PriceTable) -> Option<f64> {
        let usd = match &self.source {
            HistorySource::Reported { usd } => *usd,
            HistorySource::Tokens { models } => models
                .iter()
                .filter_map(|(model, tokens)| prices.cost(model, tokens))
                .sum(),
            HistorySource::Skipped => return None,
        };
        (usd >= MINIMUM_SESSION_USD).then_some(usd)
    }
}

impl HistoryEntry {
    /// Quota points the session used in `window` (`primary` or `secondary`),
    /// or `None` when the file has no reading or the use is negligible.
    pub fn quota_points(&self, window: &str) -> Option<f64> {
        self.quota
            .iter()
            .find(|(name, _)| name == window)
            .map(|(_, points)| *points)
            .filter(|points| *points >= MINIMUM_SESSION_QUOTA_POINTS)
    }
}

/// Ascending per-session quota use of `window`, ready for percentile
/// calibration under the quota metric.
pub fn sorted_quota(entries: &[HistoryEntry], window: &str) -> Vec<f64> {
    let mut points: Vec<f64> = entries
        .iter()
        .filter_map(|entry| entry.quota_points(window))
        .collect();
    points.sort_by(f64::total_cmp);
    points
}

/// Ascending session totals, ready for percentile calibration.
pub fn sorted_totals(entries: &[HistoryEntry], prices: &PriceTable) -> Vec<f64> {
    let mut totals: Vec<f64> = entries
        .iter()
        .filter_map(|entry| entry.usd(prices))
        .collect();
    totals.sort_by(f64::total_cmp);
    totals
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheFile {
    version: u32,
    entries: HashMap<String, HistoryEntry>,
}

/// Default cache location, next to the client's other per-user data.
pub fn default_cache_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "ilium")
        .map(|dirs| dirs.cache_dir().join("cost-history-v1.json"))
}

fn load_cache(path: &Path) -> HashMap<String, HistoryEntry> {
    if std::fs::metadata(path).map_or(true, |metadata| metadata.len() > 16 * 1024 * 1024) {
        return HashMap::new();
    }
    let Ok(file) = std::fs::File::open(path) else {
        return HashMap::new();
    };
    let mut bytes = Vec::new();
    if file
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() > 16 * 1024 * 1024
    {
        return HashMap::new();
    }
    match serde_json::from_slice::<CacheFile>(&bytes) {
        Ok(file)
            if file.version == CACHE_VERSION
                && file.entries.len() <= 16_384
                && file
                    .entries
                    .iter()
                    .map(|(key, value)| key.capacity() + value.retained_bytes() + 256)
                    .sum::<usize>()
                    <= 16 * 1024 * 1024 =>
        {
            file.entries
        }
        _ => HashMap::new(),
    }
}

fn save_cache(
    path: &Path,
    entries: &HashMap<String, HistoryEntry>,
    should_stop: &dyn Fn() -> bool,
) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("Cost history cache has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let lock_path = path.with_extension("json.lock");
    let _lock = ilium_platform::file_lock::ExclusiveFileLock::try_acquire(&lock_path)?.ok_or_else(
        || {
            std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "Cost history cache writer busy",
            )
        },
    )?;
    let file = CacheFile {
        version: CACHE_VERSION,
        entries: entries.clone(),
    };
    let bytes = serde_json::to_vec(&file).map_err(std::io::Error::other)?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(std::io::Error::other(
            "Cost history cache byte bound exceeded",
        ));
    }
    let temporary = parent.join(format!(".ilium-cost-history-{}.tmp", uuid::Uuid::new_v4()));
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        ilium_platform::secure_fs::restrict_open_file_to_owner(&output)?;
        output.write_all(&bytes)?;
        output.sync_all()?;
        drop(output);
        if should_stop() {
            return Err(std::io::Error::other(
                "Cost history cancelled before cache publication",
            ));
        }
        ilium_platform::secure_fs::replace_file_durably(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// Transcript files modified within `days` of `now_ms`: Claude Code's
/// per-project session files and Codex's dated rollout files.
fn candidate_files(
    home: &Path,
    days: u16,
    now_ms: i64,
    should_stop: &dyn Fn() -> bool,
) -> Result<Vec<(PathBuf, u64, i64, bool)>, String> {
    let oldest_ms = now_ms - i64::from(days) * 86_400_000;
    let mut found = Vec::new();
    let mut path_bytes = 0;
    let mut overflow = false;
    let mut scanned = 0;
    let mut consider = |path: PathBuf, is_codex: bool| {
        if found.len() >= 16_384 || path_bytes + path.capacity() > 8 * 1024 * 1024 {
            overflow = true;
            return;
        }
        let Ok(metadata) = std::fs::metadata(&path) else {
            return;
        };
        let Some(mtime_ms) = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_millis() as i64)
        else {
            return;
        };
        if mtime_ms >= oldest_ms && metadata.len() > 0 {
            path_bytes += path.capacity();
            found.push((path, metadata.len(), mtime_ms, is_codex));
        }
    };

    if let Ok(projects) = std::fs::read_dir(home.join(".claude").join("projects")) {
        for project in projects.flatten() {
            scanned += 1;
            if should_stop() || scanned > 32_768 {
                return Err(
                    "Cost history scan cancelled or entry limit exceeded; calibration incomplete"
                        .into(),
                );
            }
            let Ok(files) = std::fs::read_dir(project.path()) else {
                continue;
            };
            for file in files.flatten() {
                scanned += 1;
                if should_stop() || scanned > 32_768 {
                    return Err("Cost history scan cancelled or entry limit exceeded; calibration incomplete".into());
                }
                let path = file.path();
                if path
                    .extension()
                    .is_some_and(|extension| extension == "jsonl")
                {
                    consider(path, false);
                }
            }
        }
    }
    let mut pending = vec![home.join(".codex").join("sessions")];
    while let Some(directory) = pending.pop() {
        let Ok(children) = std::fs::read_dir(&directory) else {
            continue;
        };
        for child in children.flatten() {
            scanned += 1;
            if should_stop() || scanned > 32_768 || pending.len() >= 4096 {
                return Err("Cost history scan cancelled or directory limit exceeded; calibration incomplete".into());
            }
            let path = child.path();
            let Ok(kind) = child.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if pending.iter().map(PathBuf::capacity).sum::<usize>() + path.capacity()
                    > 4 * 1024 * 1024
                {
                    return Err("Cost history pending directory byte bound exceeded; calibration incomplete".into());
                }
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "jsonl")
            {
                consider(path, true);
            }
        }
    }
    if overflow {
        return Err(
            "Cost history candidate byte/record bound exceeded; calibration incomplete".into(),
        );
    }
    Ok(found)
}

/// Scans the stores under `home`, reusing `cache_path` for unchanged files.
/// `should_stop` is polled between files so a superseded scan ends promptly.
pub fn scan(
    home: &Path,
    cache_path: Option<&Path>,
    days: u16,
    now_ms: i64,
    should_stop: &dyn Fn() -> bool,
) -> Result<Vec<HistoryEntry>, String> {
    let mut cache = cache_path.map(load_cache).unwrap_or_default();
    let mut entries = Vec::new();
    let mut fresh: HashMap<String, HistoryEntry> = HashMap::new();
    let mut parsed_any = false;
    let mut retained_bytes = 0;
    for (path, size, mtime_ms, is_codex) in candidate_files(home, days, now_ms, should_stop)? {
        if should_stop() {
            return Err("Cost history cancelled; calibration incomplete".into());
        }
        let key = path.to_string_lossy().into_owned();
        let entry = match cache.remove(&key) {
            Some(cached) if cached.size == size && cached.mtime_ms == mtime_ms => cached,
            _ => {
                parsed_any = true;
                HistoryEntry {
                    path: key.clone(),
                    size,
                    mtime_ms,
                    source: if is_codex {
                        read_codex(&path, size)
                    } else {
                        read_claude(&path, size)?
                    },
                    quota: if is_codex {
                        read_codex_quota(&path, size)
                    } else {
                        Vec::new()
                    },
                }
            }
        };
        let payload_bytes = match &entry.source {
            HistorySource::Tokens { models } => models
                .iter()
                .map(|(name, _)| name.capacity())
                .sum::<usize>(),
            _ => 0,
        };
        if entry.path.capacity() > 64 * 1024
            || payload_bytes > 64 * 1024
            || entry.quota.len() > 16
            || entry.quota.iter().any(|(name, _)| name.capacity() > 1024)
        {
            return Err(
                "Cost history record exceeds retained limits; calibration incomplete".into(),
            );
        }
        retained_bytes += entry.retained_bytes() + 256;
        if retained_bytes > 16 * 1024 * 1024 {
            return Err("Cost history retained byte limit exceeded; calibration incomplete".into());
        }
        fresh.insert(key, entry.clone());
        entries.push(entry);
    }
    if should_stop() {
        return Err("Cost history cancelled; calibration incomplete".into());
    }
    // Entries for files that vanished or aged out are dropped with `cache`.
    if let Some(path) = cache_path {
        if parsed_any || !cache.is_empty() {
            save_cache(path, &fresh, should_stop).map_err(|error| {
                format!("Cost history cache publication: {error}; calibration not refreshed")
            })?;
        }
    }
    Ok(entries)
}

fn read_range(path: &Path, size: u64, length: u64) -> Option<Vec<u8>> {
    let mut file = std::fs::File::open(path).ok()?;
    let start = size.saturating_sub(length);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::with_capacity((size - start) as usize);
    file.take(length).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

fn read_head(path: &Path, length: u64) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(length).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// The last complete line of `bytes` containing `needle`. A trailing
/// unterminated line may be mid-write and is ignored.
fn last_line_containing<'a>(bytes: &'a [u8], needle: &[u8]) -> Option<&'a [u8]> {
    let complete_end = bytes.iter().rposition(|byte| *byte == b'\n')?;
    let complete = &bytes[..complete_end];
    let position = complete
        .windows(needle.len())
        .rposition(|window| window == needle)?;
    let start = complete[..position]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |newline| newline + 1);
    let end = complete[position..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(complete.len(), |newline| position + newline);
    Some(&complete[start..end])
}

fn read_claude(path: &Path, size: u64) -> Result<HistorySource, String> {
    for length in [TAIL_BYTES, WIDE_TAIL_BYTES] {
        let Some(bytes) = read_range(path, size, length) else {
            return Ok(HistorySource::Skipped);
        };
        if let Some(line) = last_line_containing(&bytes, br#""type":"cost-state""#) {
            if line.len() > 4 * 1024 * 1024 {
                return Err(
                    "Cost history cost-state line exceeds4MiB; calibration incomplete".into(),
                );
            }
            // A tail read can start mid-line, but a line that holds the
            // needle and starts at the buffer start may be cut; JSON parsing
            // rejects such a fragment and the wider attempt takes over.
            if let Some(usd) = serde_json::from_slice::<Value>(line)
                .ok()
                .and_then(|record| record.get("totalCostUSD").and_then(Value::as_f64))
            {
                return Ok(HistorySource::Reported { usd });
            }
        }
        if size <= length {
            break;
        }
    }
    Ok(HistorySource::Skipped)
}

fn model_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r#""model"\s*:\s*"([^"]+)""#).expect("the model pattern is a valid regex")
    })
}

fn read_codex(path: &Path, size: u64) -> HistorySource {
    let Some(tail) = read_range(path, size, TAIL_BYTES) else {
        return HistorySource::Skipped;
    };
    let Some(line) = last_line_containing(&tail, br#""total_token_usage""#) else {
        return HistorySource::Skipped;
    };
    let Some(total) = serde_json::from_slice::<Value>(line)
        .ok()
        .and_then(|record| {
            record
                .get("payload")?
                .get("info")?
                .get("total_token_usage")
                .map(crate::session_stats::codex_tokens)
        })
    else {
        return HistorySource::Skipped;
    };
    let model_in = |bytes: &[u8]| {
        model_pattern()
            .captures_iter(bytes)
            .last()
            .and_then(|capture| capture.get(1))
            .map(|found| String::from_utf8_lossy(found.as_bytes()).into_owned())
    };
    let model = model_in(&tail)
        .or_else(|| read_head(path, CODEX_HEAD_BYTES).and_then(|head| model_in(&head)));
    match model {
        Some(model) => HistorySource::Tokens {
            models: vec![(model, total)],
        },
        None => HistorySource::Skipped,
    }
}

/// One Codex rate-limit reading: used percent and the reset instant that
/// names the window period.
type QuotaReading = (String, f64, Option<i64>);

fn quota_readings(line: &[u8]) -> Vec<QuotaReading> {
    if line.len() > 4 * 1024 * 1024 {
        return Vec::new();
    }
    let Some(limits) = serde_json::from_slice::<Value>(line)
        .ok()
        .and_then(|record| record.get("payload")?.get("rate_limits").cloned())
    else {
        return Vec::new();
    };
    ["primary", "secondary"]
        .into_iter()
        .filter_map(|key| {
            let window = limits.get(key).filter(|window| window.is_object())?;
            Some((
                key.to_owned(),
                window.get("used_percent")?.as_f64()?,
                window.get("resets_at").and_then(Value::as_i64),
            ))
        })
        .collect()
}

/// The first complete line of `bytes` carrying an object-valued rate limit.
fn first_quota_line(bytes: &[u8]) -> Option<&[u8]> {
    bytes
        .split(|byte| *byte == b'\n')
        .take(bytes.iter().filter(|byte| **byte == b'\n').count())
        .find(|line| {
            line.windows(RATE_LIMIT_NEEDLE.len())
                .any(|window| window == RATE_LIMIT_NEEDLE)
        })
}

const RATE_LIMIT_NEEDLE: &[u8] = br#""rate_limits":{"#;

/// Quota points a Codex session used per window: the rise from its first to
/// its last reading when both belong to the same window period, otherwise the
/// last reading alone (the period began after the first one).
fn read_codex_quota(path: &Path, size: u64) -> Vec<(String, f64)> {
    let Some(tail) = read_range(path, size, TAIL_BYTES) else {
        return Vec::new();
    };
    let Some(last_line) = last_line_containing(&tail, RATE_LIMIT_NEEDLE) else {
        return Vec::new();
    };
    let last = quota_readings(last_line);
    let first = read_head(path, CODEX_HEAD_BYTES)
        .and_then(|head| first_quota_line(&head).map(quota_readings))
        .unwrap_or_default();
    last.into_iter()
        .map(|(name, used, resets)| {
            let used_before = first
                .iter()
                .find(|(first_name, _, first_resets)| {
                    *first_name == name && *first_resets == resets
                })
                .map_or(0.0, |(_, first_used, _)| *first_used);
            (name, (used - used_before).max(0.0))
        })
        .collect()
}

// ------------------------------------------------------------------ worker

/// One admitted generation; its last reader retires the original allocation on CPU.
#[derive(Debug)]
struct HistoryGeneration {
    entries: Vec<HistoryEntry>,
}
#[derive(Clone, Default)]
pub(crate) struct HistorySnapshot {
    generation: Option<RetiringArc<HistoryGeneration>>,
    revision: u64,
    scanning: bool,
    has_result: bool,
}
impl HistorySnapshot {
    pub(crate) fn entries(&self) -> &[HistoryEntry] {
        self.generation
            .as_ref()
            .map_or(&[], |generation| generation.entries.as_slice())
    }
    pub(crate) const fn revision(&self) -> u64 {
        self.revision
    }
    pub(crate) const fn is_scanning(&self) -> bool {
        self.scanning
    }
    pub(crate) const fn has_result(&self) -> bool {
        self.has_result
    }
}
struct HistoryResult {
    generation: RetiringArc<HistoryGeneration>,
}
struct HistoryScan {
    home: PathBuf,
    cache_path: Option<PathBuf>,
    days: u16,
    generation: RetirementReservation<HistoryGeneration>,
}
impl Job for HistoryScan {
    type Output = HistoryResult;
    type Error = String;
    fn run(self, context: JobContext) -> Result<HistoryResult, String> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis() as i64);
        scan(
            &self.home,
            self.cache_path.as_deref(),
            self.days,
            now_ms,
            &|| context.stop_requested(),
        )
        .and_then(|entries| {
            let bytes = entries.capacity() * std::mem::size_of::<HistoryEntry>()
                + entries
                    .iter()
                    .map(HistoryEntry::retained_bytes)
                    .sum::<usize>()
                + 1024 * 1024;
            if bytes > 64 * 1024 * 1024 {
                return Err(
                    "Cost history physical capacity exceeded admission; calibration incomplete"
                        .into(),
                );
            }
            Ok(HistoryResult {
                generation: self.generation.attach_shared(HistoryGeneration { entries }),
            })
        })
    }
}
struct ActiveHistory {
    home: PathBuf,
    cache_path: Option<PathBuf>,
    days: u16,
    receipt: Receipt<HistoryScan>,
}
/// One ordered finite history job; the previous complete result survives a
/// failed/cancelled scan and retains its quota through its real ownership.
#[cfg_attr(not(test), derive(Default))]
pub struct CostHistory {
    client: Option<Client>,
    active: Option<ActiveHistory>,
    generation: Option<RetiringArc<HistoryGeneration>>,
    scanned_for: Option<(PathBuf, Option<PathBuf>, u16)>,
    in_flight: bool,
    discard_active: bool,
    finished_at: Option<Instant>,
    revision: u64,
    error: Option<String>,
}
#[cfg(test)]
impl Default for CostHistory {
    fn default() -> Self {
        Self {
            client: {
                #[cfg(test)]
                {
                    Some(crate::execution::test_client())
                }
                #[cfg(not(test))]
                {
                    None
                }
            },
            active: None,
            generation: None,
            scanned_for: None,
            in_flight: false,
            discard_active: false,
            finished_at: None,
            revision: 0,
            error: None,
        }
    }
}
impl std::fmt::Debug for CostHistory {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CostHistory")
            .field("entries", &self.entries().len())
            .field("in_flight", &self.in_flight)
            .finish()
    }
}

impl CostHistory {
    pub fn entries(&self) -> &[HistoryEntry] {
        self.generation
            .as_ref()
            .map_or(&[], |generation| generation.entries.as_slice())
    }
    pub(crate) fn snapshot(&self) -> HistorySnapshot {
        HistorySnapshot {
            generation: self.generation.clone(),
            revision: self.revision,
            scanning: self.in_flight,
            has_result: self.finished_at.is_some(),
        }
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether a scan is running or has never completed.
    pub fn is_scanning(&self) -> bool {
        self.in_flight
    }

    pub fn has_result(&self) -> bool {
        self.finished_at.is_some()
    }

    /// Starts a scan when none is running and the last result is missing,
    /// stale, or covers a different number of days. Returns whether a worker
    /// was started.
    pub fn request_scan(
        &mut self,
        home: PathBuf,
        cache_path: Option<PathBuf>,
        days: u16,
        now: Instant,
    ) -> bool {
        if self
            .scanned_for
            .as_ref()
            .is_some_and(|(old_home, old_cache, _)| *old_home != home || *old_cache != cache_path)
        {
            self.generation = None;
            self.scanned_for = None;
            self.finished_at = None;
            self.revision = self.revision.wrapping_add(1);
        }
        if let Some(active) = &self.active {
            if active.home != home || active.cache_path != cache_path || active.days != days {
                active.receipt.cancel();
                self.discard_active = true;
            }
            return false;
        }
        if home.capacity() > 64 * 1024
            || cache_path
                .as_ref()
                .is_some_and(|path| path.capacity() > 64 * 1024)
        {
            self.error = Some("Cost history path bound exceeded".into());
            return false;
        }
        let is_fresh = self
            .scanned_for
            .as_ref()
            .is_some_and(|(old_home, old_cache, old_days)| {
                old_home == &home && old_cache == &cache_path && *old_days == days
            })
            && self
                .finished_at
                .is_some_and(|finished| now.duration_since(finished) < RESCAN_INTERVAL);
        if is_fresh {
            return false;
        }
        let Some(client) = &self.client else {
            self.error = Some("Cost history worker unavailable".into());
            return false;
        };
        let generation = match client
            .retirement()
            .try_reserve::<HistoryGeneration>(64 * 1024 * 1024)
        {
            Ok(generation) => generation,
            Err(reason) => {
                self.error = Some(format!("Cost history storage admission: {reason:?}"));
                return false;
            }
        };
        let job = HistoryScan {
            home: home.clone(),
            cache_path: cache_path.clone(),
            days,
            generation,
        };
        match client.try_submit(
            Lane::Io,
            JobCost {
                input_bytes: 64 * 1024 * 1024,
                result_bytes: 1024 * 1024,
            },
            job,
        ) {
            Ok(receipt) => {
                self.active = Some(ActiveHistory {
                    home,
                    cache_path,
                    days,
                    receipt,
                });
                self.in_flight = true;
                self.discard_active = false;
                self.error = None;
            }
            Err(rejected) => {
                self.error = Some(format!(
                    "Cost history admission: {:?}; calibration not refreshed",
                    rejected.reason
                ));
                return false;
            }
        }

        true
    }

    /// Applies a finished scan. Returns whether the result changed.
    pub fn drain_events(&mut self, now: Instant) -> bool {
        let Some(active) = &mut self.active else {
            return false;
        };
        let outcome = match active.receipt.try_take() {
            JobPoll::Pending => return false,
            JobPoll::Ready(outcome) => Some(outcome),
            _ => None,
        };
        let Some(ActiveHistory {
            home,
            cache_path: cache,
            days,
            ..
        }) = self.active.take()
        else {
            return false;
        };
        self.in_flight = false;
        let Some(outcome) = outcome else {
            self.error = Some("Cost history receipt lost; calibration incomplete".into());
            return true;
        };
        let (outcome, _temporary_retention) = outcome.into_parts();
        if self.discard_active {
            self.discard_active = false;
            self.error = Some("Cost history owner/settings changed; stale result discarded".into());
            return true;
        }
        match outcome {
            JobOutcome::Finished(Ok(HistoryResult { generation })) => {
                // Every successful generation gets a revision. Equality across up to
                // 32768 entries belongs to CPU derivation, never this coordinator.
                let changed = true;
                self.generation = Some(generation);
                self.scanned_for = Some((home, cache, days));
                self.finished_at = Some(now);
                self.revision = self.revision.wrapping_add(1);
                self.error = None;
                changed
            }
            JobOutcome::Finished(Err(error)) => {
                self.error = Some(error);
                true
            }
            _ => {
                self.error =
                    Some("Cost history cancelled or failed; calibration not refreshed".into());
                true
            }
        }
    }
    /// Reconfiguration invalidates the old cache intent even when a new scan
    /// is disabled. A running publication may finish at its captured path;
    /// cancellation never asserts rollback, and its result cannot be installed.
    pub(crate) fn invalidate_cache(&mut self) {
        if let Some(active) = &self.active {
            active.receipt.cancel();
            self.discard_active = true;
        }
        self.generation = None;
        self.scanned_for = None;
        self.finished_at = None;
        self.revision = self.revision.wrapping_add(1);
    }
    pub(crate) fn configure_execution(&mut self, client: Client) {
        self.client = Some(client);
    }
    pub(crate) fn cancel_pending(&mut self) {
        if let Some(active) = self.active.take() {
            active.receipt.cancel();
            // Dropping the receiver also releases an already-ready generation.
            // An in-flight result is destroyed by its original I/O publisher.
        }
        self.generation = None;
        self.in_flight = false;
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}
impl Drop for CostHistory {
    fn drop(&mut self) {
        self.cancel_pending();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, lines: &[String]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, lines.join("\n") + "\n").unwrap();
    }

    fn claude_file(home: &Path, project: &str, id: &str, usd: Option<f64>) {
        let mut lines = vec![r#"{"type":"user","message":{"content":"hi"}}"#.to_owned()];
        if let Some(usd) = usd {
            lines.push(format!(
                r#"{{"type":"cost-state","sessionId":"{id}","totalCostUSD":{}}}"#,
                usd / 2.0
            ));
            lines.push(format!(
                r#"{{"type":"cost-state","sessionId":"{id}","totalCostUSD":{usd}}}"#
            ));
        }
        lines.push(r#"{"type":"assistant","message":{"content":[]}}"#.to_owned());
        write(
            &home
                .join(".claude/projects")
                .join(project)
                .join(format!("{id}.jsonl")),
            &lines,
        );
    }

    fn codex_file(home: &Path, id: &str, model: &str, input: u64, cached: u64, output: u64) {
        let lines = vec![
            format!(r#"{{"type":"turn_context","payload":{{"model":"{model}"}}}}"#),
            format!(
                r#"{{"type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":{input},"cached_input_tokens":{cached},"output_tokens":{output},"reasoning_output_tokens":0}}}}}}}}"#
            ),
        ];
        write(
            &home
                .join(".codex/sessions/2026/09/30")
                .join(format!("rollout-{id}.jsonl")),
            &lines,
        );
    }

    #[test]
    fn reads_the_last_reported_claude_total_and_skips_files_without_one() {
        let home = tempfile::tempdir().unwrap();
        claude_file(home.path(), "-proj", "a", Some(12.5));
        claude_file(home.path(), "-proj", "b", None);
        let now_ms = chrono::Utc::now().timestamp_millis();
        let entries = scan(home.path(), None, 30, now_ms, &|| false).unwrap();
        assert_eq!(entries.len(), 2);
        let totals = sorted_totals(&entries, &PriceTable::default());
        assert_eq!(totals, vec![12.5]);
    }

    fn quota_line(used: f64, resets_at: i64) -> String {
        format!(
            r#"{{"type":"event_msg","payload":{{"type":"token_count","info":null,"rate_limits":{{"primary":{{"used_percent":{used},"window_minutes":300,"resets_at":{resets_at}}},"secondary":{{"used_percent":40.0,"window_minutes":10080,"resets_at":9}}}}}}}}"#
        )
    }

    fn codex_quota_file(home: &Path, id: &str, readings: &[(f64, i64)]) {
        let lines: Vec<String> = readings
            .iter()
            .map(|(used, resets_at)| quota_line(*used, *resets_at))
            .collect();
        write(
            &home
                .join(".codex/sessions/2026/09/30")
                .join(format!("rollout-{id}.jsonl")),
            &lines,
        );
    }

    #[test]
    fn codex_quota_use_is_the_rise_inside_one_window_period() {
        let home = tempfile::tempdir().unwrap();
        codex_quota_file(
            home.path(),
            "rise",
            &[(20.0, 100), (24.0, 100), (31.5, 100)],
        );
        // The window reset mid-session: only the new period's reading counts.
        codex_quota_file(home.path(), "reset", &[(90.0, 100), (6.0, 200)]);
        codex_quota_file(home.path(), "flat", &[(50.0, 100), (50.0, 100)]);
        let now_ms = chrono::Utc::now().timestamp_millis();
        let entries = scan(home.path(), None, 30, now_ms, &|| false).unwrap();
        assert_eq!(entries.len(), 3);
        let mut primary = sorted_quota(&entries, "primary");
        primary
            .iter_mut()
            .for_each(|value| *value = (*value * 10.0).round() / 10.0);
        assert_eq!(primary, vec![6.0, 11.5], "flat session is negligible");
        assert!(
            sorted_quota(&entries, "secondary").is_empty(),
            "a constant window consumed nothing"
        );
    }

    #[test]
    fn claude_sessions_carry_no_quota() {
        let home = tempfile::tempdir().unwrap();
        claude_file(home.path(), "-proj", "a", Some(3.0));
        let now_ms = chrono::Utc::now().timestamp_millis();
        let entries = scan(home.path(), None, 30, now_ms, &|| false).unwrap();
        assert!(sorted_quota(&entries, "primary").is_empty());
    }

    #[test]
    fn prices_codex_sessions_from_cumulative_tokens_and_the_last_model() {
        let home = tempfile::tempdir().unwrap();
        codex_file(
            home.path(),
            "a",
            "gpt-5.6-sol",
            2_000_000,
            1_000_000,
            100_000,
        );
        codex_file(home.path(), "b", "gpt-reserve", 10, 0, 10);
        let now_ms = chrono::Utc::now().timestamp_millis();
        let entries = scan(home.path(), None, 30, now_ms, &|| false).unwrap();
        let totals = sorted_totals(&entries, &PriceTable::default());
        // 1M fresh x $4 + 1M cached x $0.40 + 0.1M output x $20.
        assert_eq!(totals.len(), 1);
        assert!((totals[0] - 6.4).abs() < 1e-9, "{totals:?}");
    }

    #[test]
    fn old_files_are_ignored() {
        let home = tempfile::tempdir().unwrap();
        claude_file(home.path(), "-proj", "a", Some(5.0));
        let now_ms = chrono::Utc::now().timestamp_millis();
        let far_future = now_ms + 200 * 86_400_000;
        assert!(scan(home.path(), None, 30, far_future, &|| false)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn unchanged_files_come_from_the_cache_and_changes_are_reparsed() {
        let home = tempfile::tempdir().unwrap();
        let cache = home.path().join("cache").join("history.json");
        claude_file(home.path(), "-proj", "a", Some(3.0));
        let now_ms = chrono::Utc::now().timestamp_millis();
        let first = scan(home.path(), Some(&cache), 30, now_ms, &|| false).unwrap();
        assert_eq!(first.len(), 1);

        // Poison the cached total; an unchanged file must return the cache.
        let mut cached = load_cache(&cache);
        for entry in cached.values_mut() {
            entry.source = HistorySource::Reported { usd: 999.0 };
        }
        save_cache(&cache, &cached, &|| false).unwrap();
        let second = scan(home.path(), Some(&cache), 30, now_ms, &|| false).unwrap();
        assert_eq!(second[0].source, HistorySource::Reported { usd: 999.0 });

        // A changed file is parsed again (its mtime moves on).
        std::thread::sleep(Duration::from_millis(25));
        claude_file(home.path(), "-proj", "a", Some(7.0));
        let third = scan(home.path(), Some(&cache), 30, now_ms, &|| false).unwrap();
        assert_eq!(third[0].source, HistorySource::Reported { usd: 7.0 });
    }

    #[test]
    fn a_stop_request_ends_the_scan_without_panicking() {
        let home = tempfile::tempdir().unwrap();
        claude_file(home.path(), "-proj", "a", Some(5.0));
        let now_ms = chrono::Utc::now().timestamp_millis();
        assert!(scan(home.path(), None, 30, now_ms, &|| true)
            .unwrap_err()
            .contains("incomplete"));
    }

    #[test]
    fn cancelled_scan_is_an_error_and_does_not_publish_partial_cache() {
        let home = tempfile::tempdir().unwrap();
        claude_file(home.path(), "-synthetic-project", "synthetic-id", Some(4.0));
        let cache = home.path().join("synthetic-cache.json");
        std::fs::write(&cache, b"original-cache-bytes").unwrap();
        let result = scan(
            home.path(),
            Some(&cache),
            30,
            chrono::Utc::now().timestamp_millis(),
            &|| true,
        );
        assert!(result.unwrap_err().contains("incomplete"));
        assert_eq!(std::fs::read(&cache).unwrap(), b"original-cache-bytes");
    }

    #[test]
    fn last_line_extraction_handles_missing_and_partial_lines() {
        let bytes = b"one\ntwo needle\nthree\nfour needle tail";
        assert_eq!(
            last_line_containing(bytes, b"needle"),
            Some(&b"two needle"[..])
        );
        assert_eq!(last_line_containing(b"no newline needle", b"needle"), None);
        assert_eq!(last_line_containing(bytes, b"absent"), None);
    }

    #[test]
    fn worker_delivers_a_result_and_then_honours_the_rescan_interval() {
        let home = tempfile::tempdir().unwrap();
        claude_file(home.path(), "-proj", "a", Some(4.0));
        let mut history = CostHistory::default();
        let start = Instant::now();
        assert!(history.request_scan(home.path().to_path_buf(), None, 30, start));
        assert!(!history.request_scan(home.path().to_path_buf(), None, 30, start));
        let deadline = Instant::now() + Duration::from_secs(10);
        while history.is_scanning() && Instant::now() < deadline {
            history.drain_events(Instant::now());
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(history.has_result());
        assert_eq!(history.entries().len(), 1);
        assert!(!history.request_scan(home.path().to_path_buf(), None, 30, Instant::now()));
        // A different window forces a new scan.
        assert!(history.request_scan(home.path().to_path_buf(), None, 60, Instant::now()));
    }
}
