//! The scan of agent transcripts for the compaction optimizer.
//!
//! The pure analysis lives in [`crate::analysis`]; this module owns the lifecycle and reads through a
//! [`TranscriptFs`] (the host `fs` import in the plugin, a temporary directory in tests):
//!
//! 1. **Listing** finds every transcript of one agent (a directory walk that also totals the bytes).
//! 2. **Reading** streams each file, in bounded chunks, through the crate's prefilter and `TraceBuilder`. Progress is
//!    by *bytes* (files run from 1 KB to several GB, so file-count progress would stall) in a shared
//!    [`ScanProgressCounters`]; the stop request is checked between chunks.
//! 3. **Analysing** dedupes resumed/forked transcripts across files and builds the [`CompactionReport`]
//!    (statistics, replay, optimizer).
//!
//! Only an explicit request starts a scan: there are no timers and no automatic rescans. There is no trace cache:
//! the host `fs` import gives the plugin no private writable folder, so every scan parses the files again.

mod listing;
pub mod source;
mod stream;

use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Mutex;

use crate::analysis::dedupe::dedupe_across_traces;
use crate::analysis::trace::TurnSample;
use crate::analysis::{AgentKind, SessionTrace};

use crate::report::{CompactionReport, ReportInput, ScanSummary};

pub use crate::report::ScanSettingsInput;

use listing::{list_transcripts, ListedFile};
use source::TranscriptFs;
use stream::{parse_file, FileOutcome};

/// Retained in-memory traces of one scan. A larger corpus keeps the most recent sessions and says so in the report.
pub const MAX_RETAINED_TRACE_BYTES: usize = 64 * 1024 * 1024;
/// Per-file warnings kept before the rest is only counted.
const MAX_FILE_WARNINGS: usize = 5;

// ------------------------------------------------------------------ progress

/// Where a running scan is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanPhase {
    /// Walking the directories.
    Listing,
    /// Streaming transcript files.
    Reading,
    /// Dedupe, statistics, replay and optimization.
    Analyzing,
}

impl ScanPhase {
    const fn code(self) -> u8 {
        match self {
            Self::Listing => 0,
            Self::Reading => 1,
            Self::Analyzing => 2,
        }
    }
    const fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Listing,
            1 => Self::Reading,
            _ => Self::Analyzing,
        }
    }
}

/// Lock-free progress of one scan, written by the job and read by the UI.
/// Cloned as an `Arc` into the job; every field is updated per read chunk or
/// per file, never per line.
#[derive(Debug, Default)]
pub struct ScanProgressCounters {
    phase: AtomicU8,
    files_found: AtomicU64,
    files_total: AtomicU64,
    files_done: AtomicU64,
    bytes_total: AtomicU64,
    bytes_done: AtomicU64,
    files_parsed: AtomicU64,
    files_cached: AtomicU64,
    files_skipped: AtomicU64,
    current_file: Mutex<String>,
}

impl ScanProgressCounters {
    pub fn phase(&self) -> ScanPhase {
        ScanPhase::from_code(self.phase.load(Ordering::Relaxed))
    }
    /// Files found so far (grows during [`ScanPhase::Listing`]).
    pub fn files_found(&self) -> u64 {
        self.files_found.load(Ordering::Relaxed)
    }
    pub fn files_total(&self) -> u64 {
        self.files_total.load(Ordering::Relaxed)
    }
    pub fn files_done(&self) -> u64 {
        self.files_done.load(Ordering::Relaxed)
    }
    pub fn bytes_total(&self) -> u64 {
        self.bytes_total.load(Ordering::Relaxed)
    }
    pub fn bytes_done(&self) -> u64 {
        self.bytes_done.load(Ordering::Relaxed)
    }
    /// Files parsed from their transcript in this scan.
    pub fn files_parsed(&self) -> u64 {
        self.files_parsed.load(Ordering::Relaxed)
    }
    /// Files served from the trace cache in this scan.
    pub fn files_from_cache(&self) -> u64 {
        self.files_cached.load(Ordering::Relaxed)
    }
    /// Files skipped because they vanished or could not be read.
    pub fn files_skipped(&self) -> u64 {
        self.files_skipped.load(Ordering::Relaxed)
    }
    pub fn current_file_name(&self) -> String {
        self.current_file
            .lock()
            .map(|name| name.clone())
            .unwrap_or_default()
    }

    fn set_phase(&self, phase: ScanPhase) {
        self.phase.store(phase.code(), Ordering::Relaxed);
    }
    fn set_current_file(&self, name: &str) {
        if let Ok(mut current) = self.current_file.lock() {
            name.clone_into(&mut current);
        }
    }
}

// ------------------------------------------------------------- scan function

/// A scan that ended because a stop was requested.
#[derive(Debug, PartialEq, Eq)]
pub struct ScanCancelled;

/// The traces of one scan, sorted and deduped, and what the scan saw.
#[derive(Debug)]
pub struct ScanOutput {
    pub traces: Vec<SessionTrace>,
    pub summary: ScanSummary,
}

struct ScannedFile {
    listed: ListedFile,
    trace: SessionTrace,
}

impl ScannedFile {
    fn is_subagent(&self) -> bool {
        self.listed.is_subagent || self.trace.is_subagent
    }
}

/// Phases 1 and 2 plus the dedupe: lists, reads, caps the retained size and returns the traces in the order the
/// research used (main sessions before subagents, then file name).
pub fn scan_corpus(
    agent: AgentKind,
    fs: &dyn TranscriptFs,
    retention_cap_bytes: usize,
    counters: &ScanProgressCounters,
    should_stop: &dyn Fn() -> bool,
) -> Result<ScanOutput, ScanCancelled> {
    counters.set_phase(ScanPhase::Listing);
    let mut listing = list_transcripts(agent, fs, counters, should_stop);
    if listing.was_stopped || should_stop() {
        return Err(ScanCancelled);
    }
    let mut summary = ScanSummary {
        files_listed: listing.files.len() as u64,
        bytes_total: listing.bytes_total,
        warnings: std::mem::take(&mut listing.warnings),
        ..ScanSummary::default()
    };
    counters
        .files_total
        .store(listing.files.len() as u64, Ordering::Relaxed);
    counters.set_phase(ScanPhase::Reading);

    let mut files = listing.files;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let mut scanned: Vec<ScannedFile> = Vec::with_capacity(files.len());
    let mut skipped_warnings = 0_u64;
    for listed in files {
        if should_stop() {
            return Err(ScanCancelled);
        }
        counters.set_current_file(&listed.file_name());
        let scanned_file = match read_file(agent, fs, &listed, counters, should_stop) {
            FileOutcome::Parsed { trace } => {
                counters.files_parsed.fetch_add(1, Ordering::Relaxed);
                summary.files_parsed += 1;
                if file_changed_since_listing(fs, &listed) {
                    summary.files_changed_during_scan += 1;
                }
                Some(ScannedFile {
                    listed,
                    trace: *trace,
                })
            }
            FileOutcome::Stopped => return Err(ScanCancelled),
            FileOutcome::Unreadable(error) => {
                summary.files_skipped += 1;
                counters.files_skipped.fetch_add(1, Ordering::Relaxed);
                if skipped_warnings < MAX_FILE_WARNINGS as u64 {
                    skipped_warnings += 1;
                    summary
                        .warnings
                        .push(format!("Skipped {}: {error}", listed.path.display()));
                }
                None
            }
        };
        counters.files_done.fetch_add(1, Ordering::Relaxed);
        scanned.extend(scanned_file);
    }
    if summary.files_skipped > skipped_warnings {
        summary.warnings.push(format!(
            "{} more files could not be read and were skipped.",
            summary.files_skipped - skipped_warnings
        ));
    }
    if summary.files_changed_during_scan > 0 {
        summary.warnings.push(format!(
            "{} files changed while they were read (live sessions); they were analysed as read and will be re-read by the next scan.",
            summary.files_changed_during_scan
        ));
    }

    counters.set_phase(ScanPhase::Analyzing);
    summary.sessions_dropped_for_cap = apply_retention_cap(&mut scanned, retention_cap_bytes);
    if should_stop() {
        return Err(ScanCancelled);
    }

    scanned.sort_by(|left, right| {
        left.is_subagent()
            .cmp(&right.is_subagent())
            .then_with(|| left.listed.file_name().cmp(&right.listed.file_name()))
            .then_with(|| left.listed.path.cmp(&right.listed.path))
    });
    let mut traces: Vec<SessionTrace> = scanned.into_iter().map(|file| file.trace).collect();
    let removed = dedupe_across_traces(&mut traces);
    summary.duplicate_turns_removed = removed.turns_removed as u64;
    summary.duplicate_compactions_removed = removed.compactions_removed as u64;
    Ok(ScanOutput { traces, summary })
}

/// Streams one file, keeping the byte counters exact whatever happens: the counters always end up at the file's
/// listed size.
fn read_file(
    agent: AgentKind,
    fs: &dyn TranscriptFs,
    listed: &ListedFile,
    counters: &ScanProgressCounters,
    should_stop: &dyn Fn() -> bool,
) -> FileOutcome {
    let mut counted = 0_u64;
    let outcome = parse_file(
        agent,
        fs,
        &listed.path,
        listed.size,
        listed.is_subagent,
        &mut |bytes| {
            counted += bytes as u64;
            counters
                .bytes_done
                .fetch_add(bytes as u64, Ordering::Relaxed);
            !should_stop()
        },
    );
    // A file that shrank or vanished delivered fewer bytes than listed.
    if !matches!(outcome, FileOutcome::Stopped) {
        counters
            .bytes_done
            .fetch_add(listed.size.saturating_sub(counted), Ordering::Relaxed);
    }
    outcome
}

/// Whether the file is now smaller than when it was listed or gone (appends are expected and harmless: only the
/// listed prefix was read).
fn file_changed_since_listing(fs: &dyn TranscriptFs, listed: &ListedFile) -> bool {
    fs.metadata(&listed.path)
        .map_or(true, |info| info.length < listed.size)
}

/// Rough in-memory size of a trace, for the retained-result cap.
fn retained_bytes(trace: &SessionTrace) -> usize {
    std::mem::size_of::<SessionTrace>()
        + trace.turns.capacity() * std::mem::size_of::<TurnSample>()
        + trace.turn_id_hashes.capacity() * std::mem::size_of::<u32>()
        + trace.compactions.capacity()
            * std::mem::size_of::<crate::analysis::trace::CompactionEvent>()
        + trace
            .models
            .iter()
            .map(|model| model.capacity() + std::mem::size_of::<String>())
            .sum::<usize>()
}

/// Drops the oldest sessions until the retained traces fit the cap; returns how many were dropped.
fn apply_retention_cap(scanned: &mut Vec<ScannedFile>, cap_bytes: usize) -> u64 {
    let total: usize = scanned.iter().map(|file| retained_bytes(&file.trace)).sum();
    if total <= cap_bytes {
        return 0;
    }
    scanned.sort_by_key(|file| std::cmp::Reverse(file.listed.mtime_ms));
    let mut kept_bytes = 0_usize;
    let mut keep = 0_usize;
    for file in scanned.iter() {
        let bytes = retained_bytes(&file.trace);
        if kept_bytes + bytes > cap_bytes {
            break;
        }
        kept_bytes += bytes;
        keep += 1;
    }
    let dropped = scanned.len() - keep;
    scanned.truncate(keep);
    dropped as u64
}

/// Scans one agent and builds its report. `progress` is called after the listing, after each file and before the
/// analysis, so the caller can report the byte fraction to its host.
pub fn run_scan(
    agent: AgentKind,
    fs: &dyn TranscriptFs,
    settings: &ScanSettingsInput,
    generated_at_unix_ms: i64,
    counters: &ScanProgressCounters,
    should_stop: &dyn Fn() -> bool,
) -> Result<CompactionReport, ScanCancelled> {
    let mut output = scan_corpus(agent, fs, MAX_RETAINED_TRACE_BYTES, counters, should_stop)?;
    output.summary.scan_seconds = 0.0;
    counters.set_phase(ScanPhase::Analyzing);
    let report = CompactionReport::build(&ReportInput {
        agent,
        traces: &output.traces,
        settings,
        scan: &output.summary,
        generated_at_unix_ms,
    });
    if should_stop() {
        return Err(ScanCancelled);
    }
    Ok(report)
}

#[cfg(test)]
pub(crate) mod fixtures;
#[cfg(test)]
mod test_fs;
#[cfg(test)]
mod tests;
