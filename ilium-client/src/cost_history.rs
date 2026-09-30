//! Background scan of past agent sessions, used to calibrate "a lot".
//!
//! Real transcript stores reach tens of gigabytes, so the scan never parses a
//! whole file. Every session transcript already ends with (Claude Code) a
//! `cost-state` snapshot carrying the CLI's own dollar total, or (Codex) a
//! cumulative `total_token_usage`, so reading a bounded tail of each file is
//! enough. Results are cached by path, size and modification time, which makes
//! every scan after the first one cost a directory walk.
//!
//! The scan runs on one low-priority worker thread and reports through a
//! channel; the UI thread never waits on it.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ilium_platform::thread_priority::{lower_current_thread, WorkerPriority};
use regex::bytes::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cost_model::PriceTable;
use crate::session_stats::TokenTotals;

const CACHE_VERSION: u32 = 1;
/// Tail read first; covers almost every transcript's final records.
const TAIL_BYTES: u64 = 4 * 1024 * 1024;
/// Second, wider attempt for files whose tail holds only bulky records.
const WIDE_TAIL_BYTES: u64 = 48 * 1024 * 1024;
const CODEX_HEAD_BYTES: u64 = 256 * 1024;
/// Sessions below this are noise (opened and abandoned), not calibration data.
const MINIMUM_SESSION_USD: f64 = 0.01;
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
}

impl HistoryEntry {
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
    let Ok(bytes) = std::fs::read(path) else {
        return HashMap::new();
    };
    match serde_json::from_slice::<CacheFile>(&bytes) {
        Ok(file) if file.version == CACHE_VERSION => file.entries,
        _ => HashMap::new(),
    }
}

fn save_cache(path: &Path, entries: &HashMap<String, HistoryEntry>) {
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let file = CacheFile {
        version: CACHE_VERSION,
        entries: entries.clone(),
    };
    let Ok(bytes) = serde_json::to_vec(&file) else {
        return;
    };
    // Write-then-rename so a crash never leaves a half-written cache.
    let temporary = path.with_extension("json.tmp");
    if std::fs::write(&temporary, bytes).is_ok() {
        let _ = std::fs::rename(&temporary, path);
    }
}

/// Transcript files modified within `days` of `now_ms`: Claude Code's
/// per-project session files and Codex's dated rollout files.
fn candidate_files(home: &Path, days: u16, now_ms: i64) -> Vec<(PathBuf, u64, i64, bool)> {
    let oldest_ms = now_ms - i64::from(days) * 86_400_000;
    let mut found = Vec::new();
    let mut consider = |path: PathBuf, is_codex: bool| {
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
            found.push((path, metadata.len(), mtime_ms, is_codex));
        }
    };

    if let Ok(projects) = std::fs::read_dir(home.join(".claude").join("projects")) {
        for project in projects.flatten() {
            let Ok(files) = std::fs::read_dir(project.path()) else {
                continue;
            };
            for file in files.flatten() {
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
            let path = child.path();
            let Ok(kind) = child.file_type() else {
                continue;
            };
            if kind.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "jsonl")
            {
                consider(path, true);
            }
        }
    }
    found
}

/// Scans the stores under `home`, reusing `cache_path` for unchanged files.
/// `should_stop` is polled between files so a superseded scan ends promptly.
pub fn scan(
    home: &Path,
    cache_path: Option<&Path>,
    days: u16,
    now_ms: i64,
    should_stop: &dyn Fn() -> bool,
) -> Vec<HistoryEntry> {
    let mut cache = cache_path.map(load_cache).unwrap_or_default();
    let mut entries = Vec::new();
    let mut fresh: HashMap<String, HistoryEntry> = HashMap::new();
    let mut parsed_any = false;
    for (path, size, mtime_ms, is_codex) in candidate_files(home, days, now_ms) {
        if should_stop() {
            return entries;
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
                        read_claude(&path, size)
                    },
                }
            }
        };
        fresh.insert(key, entry.clone());
        entries.push(entry);
    }
    // Entries for files that vanished or aged out are dropped with `cache`.
    if let Some(path) = cache_path {
        if parsed_any || !cache.is_empty() {
            save_cache(path, &fresh);
        }
    }
    entries
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

fn read_claude(path: &Path, size: u64) -> HistorySource {
    for length in [TAIL_BYTES, WIDE_TAIL_BYTES] {
        let Some(bytes) = read_range(path, size, length) else {
            return HistorySource::Skipped;
        };
        if let Some(line) = last_line_containing(&bytes, br#""type":"cost-state""#) {
            // A tail read can start mid-line, but a line that holds the
            // needle and starts at the buffer start may be cut; JSON parsing
            // rejects such a fragment and the wider attempt takes over.
            if let Some(usd) = serde_json::from_slice::<Value>(line)
                .ok()
                .and_then(|record| record.get("totalCostUSD").and_then(Value::as_f64))
            {
                return HistorySource::Reported { usd };
            }
        }
        if size <= length {
            break;
        }
    }
    HistorySource::Skipped
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

// ------------------------------------------------------------------ worker

enum HistoryEvent {
    Finished {
        generation: u64,
        days: u16,
        entries: Vec<HistoryEntry>,
    },
}

/// Owns the scan worker and the latest finished result.
pub struct CostHistory {
    events_tx: Sender<HistoryEvent>,
    events_rx: Receiver<HistoryEvent>,
    entries: Vec<HistoryEntry>,
    scanned_days: Option<u16>,
    in_flight: bool,
    generation: u64,
    finished_at: Option<Instant>,
    /// Bumped whenever `entries` change, so callers can cache derived data.
    revision: u64,
}

impl Default for CostHistory {
    fn default() -> Self {
        let (events_tx, events_rx) = channel();
        Self {
            events_tx,
            events_rx,
            entries: Vec::new(),
            scanned_days: None,
            in_flight: false,
            generation: 0,
            finished_at: None,
            revision: 0,
        }
    }
}

impl std::fmt::Debug for CostHistory {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CostHistory")
            .field("entries", &self.entries.len())
            .field("in_flight", &self.in_flight)
            .finish()
    }
}

impl CostHistory {
    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
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
        if self.in_flight {
            return false;
        }
        let is_fresh = self.scanned_days == Some(days)
            && self
                .finished_at
                .is_some_and(|finished| now.duration_since(finished) < RESCAN_INTERVAL);
        if is_fresh {
            return false;
        }
        self.in_flight = true;
        self.generation += 1;
        let generation = self.generation;
        let events_tx = self.events_tx.clone();
        std::thread::spawn(move || {
            lower_current_thread(WorkerPriority::Lowest);
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_millis() as i64);
            let entries = scan(&home, cache_path.as_deref(), days, now_ms, &|| false);
            let _ = events_tx.send(HistoryEvent::Finished {
                generation,
                days,
                entries,
            });
        });
        true
    }

    /// Applies a finished scan. Returns whether the result changed.
    pub fn drain_events(&mut self, now: Instant) -> bool {
        let mut changed = false;
        while let Ok(HistoryEvent::Finished {
            generation,
            days,
            entries,
        }) = self.events_rx.try_recv()
        {
            if generation != self.generation {
                continue;
            }
            self.in_flight = false;
            self.scanned_days = Some(days);
            self.finished_at = Some(now);
            if self.entries != entries {
                self.entries = entries;
                self.revision += 1;
                changed = true;
            }
            // The first completion always counts as a change: "calibrating"
            // turns into a real scale even when the store was empty.
            changed |= self.revision == 0;
        }
        changed
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
        let entries = scan(home.path(), None, 30, now_ms, &|| false);
        assert_eq!(entries.len(), 2);
        let totals = sorted_totals(&entries, &PriceTable::default());
        assert_eq!(totals, vec![12.5]);
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
        let entries = scan(home.path(), None, 30, now_ms, &|| false);
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
        assert!(scan(home.path(), None, 30, far_future, &|| false).is_empty());
    }

    #[test]
    fn unchanged_files_come_from_the_cache_and_changes_are_reparsed() {
        let home = tempfile::tempdir().unwrap();
        let cache = home.path().join("cache").join("history.json");
        claude_file(home.path(), "-proj", "a", Some(3.0));
        let now_ms = chrono::Utc::now().timestamp_millis();
        let first = scan(home.path(), Some(&cache), 30, now_ms, &|| false);
        assert_eq!(first.len(), 1);

        // Poison the cached total; an unchanged file must return the cache.
        let mut cached = load_cache(&cache);
        for entry in cached.values_mut() {
            entry.source = HistorySource::Reported { usd: 999.0 };
        }
        save_cache(&cache, &cached);
        let second = scan(home.path(), Some(&cache), 30, now_ms, &|| false);
        assert_eq!(second[0].source, HistorySource::Reported { usd: 999.0 });

        // A changed file is parsed again (its mtime moves on).
        std::thread::sleep(Duration::from_millis(25));
        claude_file(home.path(), "-proj", "a", Some(7.0));
        let third = scan(home.path(), Some(&cache), 30, now_ms, &|| false);
        assert_eq!(third[0].source, HistorySource::Reported { usd: 7.0 });
    }

    #[test]
    fn a_stop_request_ends_the_scan_without_panicking() {
        let home = tempfile::tempdir().unwrap();
        claude_file(home.path(), "-proj", "a", Some(5.0));
        let now_ms = chrono::Utc::now().timestamp_millis();
        assert!(scan(home.path(), None, 30, now_ms, &|| true).is_empty());
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
