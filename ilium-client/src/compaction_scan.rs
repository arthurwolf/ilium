//! Background scan of agent transcripts for the compaction optimizer.
//!
//! The pure analysis lives in `ilium-compaction-analysis`; this module owns
//! the I/O and the lifecycle, following [`crate::cost_history`]:
//!
//! 1. **Listing** finds every transcript of one agent (a fast directory walk
//!    that also totals the bytes).
//! 2. **Reading** streams each file, in bounded chunks, through the crate's
//!    prefilter and `TraceBuilder`. Progress is by *bytes* (files run from
//!    1 KB to several GB, so file-count progress would stall) in a shared
//!    [`ScanProgressCounters`]; the stop request is checked between chunks.
//!    Unchanged files come from a per-agent trace cache keyed by
//!    `(path, size, mtime)`.
//! 3. **Analysing** dedupes resumed/forked transcripts across files and builds
//!    the [`CompactionReport`] (statistics, replay, optimizer).
//!
//! One ordered finite job per agent runs on [`Lane::Io`] with a streaming
//! [`JobCost`]; the previous complete report survives a failed or cancelled
//! scan ([`CompactionOptimizer::report`]). Only an explicit button press
//! starts a scan: there are no timers and no automatic rescans.
//!
//! Priority: the I/O lane's worker threads already run at
//! `WorkerPriority::BelowNormal` (see `execution::bank_config`). The job does
//! not lower its thread further, because `thread_priority` lowering is
//! one-way and the pool thread is shared with interactive work (geocoding,
//! naming); instead it yields between files and pauses briefly every few tens
//! of megabytes so other `Lane::Io` jobs are never starved.

mod cache;
mod listing;
mod stream;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ilium_compaction_analysis::dedupe::dedupe_across_traces;
use ilium_compaction_analysis::{AgentKind, SessionTrace};
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt, RetirementReservation,
    RetiringArc,
};

use crate::compaction_report::{CompactionReport, ReportInput, ScanSummary};

pub use crate::compaction_report::ScanSettingsInput;
pub use cache::{cache_file_name, default_cache_dir, default_cache_path};

use cache::{CacheItem, CachedTrace};
use listing::{list_transcripts, ListedFile};
use stream::{parse_file, FileOutcome};

/// Retained in-memory traces of one scan (the 64 MiB result cap of
/// `cost_history::HistoryGeneration`). A larger corpus keeps the most recent
/// sessions and says so in the report.
pub const MAX_RETAINED_TRACE_BYTES: usize = 64 * 1024 * 1024;
/// Declared size of the retained report (a few hundred rows at most).
const REPORT_RESERVATION_BYTES: usize = 2 * 1024 * 1024;
/// Declared peak of one scan: the retained traces, one oversize line plus its
/// parse transients, and the loaded cache.
const SCAN_COST: JobCost = JobCost {
    input_bytes: 160 * 1024 * 1024,
    result_bytes: REPORT_RESERVATION_BYTES,
};
/// Bytes read between two short pauses that let other I/O jobs run.
const PAUSE_EVERY_BYTES: u64 = 32 * 1024 * 1024;
const PAUSE: Duration = Duration::from_millis(2);
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

/// What the UI draws while a scan runs.
#[derive(Debug, Clone, PartialEq)]
pub struct ScanProgress {
    pub phase: ScanPhase,
    pub files_done: u64,
    pub files_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub elapsed: Duration,
    /// Name (not path) of the file being read.
    pub current_file_name: String,
}

impl ScanProgress {
    /// Bytes done as a 0..1 fraction (0 when the total is unknown).
    pub fn fraction(&self) -> f64 {
        if self.bytes_total == 0 {
            0.0
        } else {
            (self.bytes_done as f64 / self.bytes_total as f64).clamp(0.0, 1.0)
        }
    }
}

/// What the UI shows for one agent.
#[derive(Debug, Clone)]
pub enum ScanView<'a> {
    /// No scan has run (or the last one produced nothing to show).
    Idle,
    /// Phase 1: files found so far.
    Listing { files_found: u64 },
    /// Phase 2 and 3: byte progress.
    Scanning(ScanProgress),
    /// A complete report.
    Ready(&'a CompactionReport),
    /// The last scan failed; an earlier report may still be available through
    /// [`CompactionOptimizer::report`].
    Failed(&'a str),
    /// The last scan was cancelled; an earlier report may still be available.
    Cancelled,
}

// ------------------------------------------------------------- scan function

/// A scan that ended because a stop was requested.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ScanCancelled;

/// The traces of one scan, sorted and deduped, and what the scan saw.
#[derive(Debug)]
pub(crate) struct ScanOutput {
    pub traces: Vec<SessionTrace>,
    pub summary: ScanSummary,
}

struct ScannedFile {
    listed: ListedFile,
    trace: SessionTrace,
    /// Whether the cache may keep the trace (the file did not change under us).
    is_cacheable: bool,
}

impl ScannedFile {
    fn is_subagent(&self) -> bool {
        self.listed.is_subagent || self.trace.is_subagent
    }
}

/// Phases 1 and 2 plus the dedupe: lists, reads (cache first), caps the
/// retained size, persists the cache and returns the traces in the order the
/// research used (main sessions before subagents, then file name).
pub(crate) fn scan_corpus(
    agent: AgentKind,
    home: &Path,
    cache_path: Option<&Path>,
    retention_cap_bytes: usize,
    counters: &ScanProgressCounters,
    should_stop: &dyn Fn() -> bool,
) -> Result<ScanOutput, ScanCancelled> {
    counters.set_phase(ScanPhase::Listing);
    let mut listing = list_transcripts(agent, home, counters, should_stop);
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

    let mut cached = cache_path
        .map(|path| cache::load(path, agent))
        .unwrap_or_default();
    let mut files = listing.files;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let mut scanned: Vec<ScannedFile> = Vec::with_capacity(files.len());
    let mut skipped_warnings = 0_u64;
    let mut since_pause = 0_u64;
    for listed in files {
        if should_stop() {
            return Err(ScanCancelled);
        }
        counters.set_current_file(&listed.file_name());
        let key = listed.path.to_string_lossy().into_owned();
        let hit = take_cache_hit(&mut cached, &key, &listed, agent);
        let scanned_file = match hit {
            Some(trace) => {
                counters
                    .bytes_done
                    .fetch_add(listed.size, Ordering::Relaxed);
                counters.files_cached.fetch_add(1, Ordering::Relaxed);
                summary.files_from_cache += 1;
                Some(ScannedFile {
                    listed,
                    trace,
                    is_cacheable: true,
                })
            }
            None => match read_file(agent, &listed, counters, should_stop, &mut since_pause) {
                FileOutcome::Parsed { trace } => {
                    counters.files_parsed.fetch_add(1, Ordering::Relaxed);
                    summary.files_parsed += 1;
                    let is_cacheable = !file_changed_since_listing(&listed);
                    if !is_cacheable {
                        summary.files_changed_during_scan += 1;
                    }
                    Some(ScannedFile {
                        listed,
                        trace: *trace,
                        is_cacheable,
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
            },
        };
        counters.files_done.fetch_add(1, Ordering::Relaxed);
        scanned.extend(scanned_file);
        std::thread::yield_now();
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

    // The cache keeps every parsed session, including those the retention cap
    // drops below: re-parsing the largest corpora is the slowest scan there is.
    counters.set_phase(ScanPhase::Analyzing);
    persist_cache(
        agent,
        cache_path,
        &scanned,
        &cached,
        summary.files_parsed > 0,
        should_stop,
        &mut summary,
    );
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

/// The cached trace of `listed` when its size and mtime still match.
fn take_cache_hit(
    cached: &mut HashMap<String, CachedTrace>,
    key: &str,
    listed: &ListedFile,
    agent: AgentKind,
) -> Option<SessionTrace> {
    let entry = cached.get(key)?;
    let is_current = entry.size == listed.size
        && entry.mtime_ms == listed.mtime_ms
        && entry.trace.agent == agent
        && (agent != AgentKind::ClaudeCode || entry.trace.is_subagent == listed.is_subagent);
    if !is_current {
        return None;
    }
    cached.remove(key).map(|entry| entry.trace)
}

/// Streams one file, keeping the byte counters exact whatever happens: the
/// counters always end up at the file's listed size.
fn read_file(
    agent: AgentKind,
    listed: &ListedFile,
    counters: &ScanProgressCounters,
    should_stop: &dyn Fn() -> bool,
    since_pause: &mut u64,
) -> FileOutcome {
    let mut counted = 0_u64;
    let outcome = parse_file(
        agent,
        &listed.path,
        listed.size,
        listed.is_subagent,
        &mut |bytes| {
            counted += bytes as u64;
            counters
                .bytes_done
                .fetch_add(bytes as u64, Ordering::Relaxed);
            *since_pause += bytes as u64;
            if *since_pause >= PAUSE_EVERY_BYTES {
                *since_pause = 0;
                std::thread::sleep(PAUSE);
            }
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

/// Whether the file is now smaller than when it was listed or gone (appends
/// are expected and harmless: only the listed prefix was read).
fn file_changed_since_listing(listed: &ListedFile) -> bool {
    std::fs::metadata(&listed.path).map_or(true, |metadata| metadata.len() < listed.size)
}

/// Drops the oldest sessions until the retained traces fit the cap; returns
/// how many were dropped.
fn apply_retention_cap(scanned: &mut Vec<ScannedFile>, cap_bytes: usize) -> u64 {
    let total: usize = scanned
        .iter()
        .map(|file| cache::retained_bytes(&file.trace))
        .sum();
    if total <= cap_bytes {
        return 0;
    }
    scanned.sort_by_key(|file| std::cmp::Reverse(file.listed.mtime_ms));
    let mut kept_bytes = 0_usize;
    let mut keep = 0_usize;
    for file in scanned.iter() {
        let bytes = cache::retained_bytes(&file.trace);
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

/// Writes the cache when something changed. A failure is a warning: the next
/// scan is slower, the report is unaffected.
fn persist_cache(
    agent: AgentKind,
    cache_path: Option<&Path>,
    scanned: &[ScannedFile],
    stale: &HashMap<String, CachedTrace>,
    changed: bool,
    should_stop: &dyn Fn() -> bool,
    summary: &mut ScanSummary,
) {
    let Some(path) = cache_path else {
        return;
    };
    if !changed && stale.is_empty() {
        return;
    }
    let keys: Vec<String> = scanned
        .iter()
        .map(|file| file.listed.path.to_string_lossy().into_owned())
        .collect();
    let items: Vec<CacheItem<'_>> = scanned
        .iter()
        .zip(&keys)
        .filter(|(file, _)| file.is_cacheable)
        .map(|(file, key)| CacheItem {
            path: key,
            size: file.listed.size,
            mtime_ms: file.listed.mtime_ms,
            trace: &file.trace,
        })
        .collect();
    if let Err(error) = cache::save(path, agent, &items, should_stop) {
        summary.warnings.push(format!(
            "The trace cache was not updated ({error}); the next scan will be slower."
        ));
    }
}

// ----------------------------------------------------------------------- job

/// The retained result of one successful scan.
#[derive(Debug)]
struct ScanGeneration {
    report: CompactionReport,
}

struct ScanResult {
    generation: RetiringArc<ScanGeneration>,
}

struct CompactionScan {
    agent: AgentKind,
    home: PathBuf,
    cache_path: Option<PathBuf>,
    settings: ScanSettingsInput,
    counters: Arc<ScanProgressCounters>,
    generation: RetirementReservation<ScanGeneration>,
}

impl Job for CompactionScan {
    type Output = ScanResult;
    type Error = ScanCancelled;

    fn run(self, context: JobContext) -> Result<ScanResult, ScanCancelled> {
        let started = Instant::now();
        let should_stop = || context.stop_requested();
        let mut output = scan_corpus(
            self.agent,
            &self.home,
            self.cache_path.as_deref(),
            MAX_RETAINED_TRACE_BYTES,
            &self.counters,
            &should_stop,
        )?;
        output.summary.scan_seconds = started.elapsed().as_secs_f64();
        self.counters.set_phase(ScanPhase::Analyzing);
        let report = CompactionReport::build(&ReportInput {
            agent: self.agent,
            traces: &output.traces,
            settings: &self.settings,
            scan: &output.summary,
            generated_at_unix_ms: chrono::Utc::now().timestamp_millis(),
        });
        if should_stop() {
            return Err(ScanCancelled);
        }
        Ok(ScanResult {
            generation: self.generation.attach_shared(ScanGeneration { report }),
        })
    }
}

// --------------------------------------------------------------------- state

#[derive(Debug, Clone, PartialEq, Eq)]
enum Status {
    Idle,
    Failed(String),
    Cancelled,
}

struct ActiveScan {
    receipt: Receipt<CompactionScan>,
    counters: Arc<ScanProgressCounters>,
    started_at: Instant,
}

/// What `drain_events` last reported, to detect a progress tick.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ProgressStamp {
    phase: u8,
    files_found: u64,
    files_done: u64,
    bytes_permille: u64,
    elapsed_seconds: u64,
}

struct AgentScanState {
    active: Option<ActiveScan>,
    generation: Option<RetiringArc<ScanGeneration>>,
    status: Status,
    last_stamp: ProgressStamp,
    /// The `now` of the last `drain_events`, so `view` is free of clocks.
    last_now: Option<Instant>,
}

impl AgentScanState {
    const fn new() -> Self {
        Self {
            active: None,
            generation: None,
            status: Status::Idle,
            last_stamp: ProgressStamp {
                phase: 0,
                files_found: 0,
                files_done: 0,
                bytes_permille: 0,
                elapsed_seconds: 0,
            },
            last_now: None,
        }
    }

    fn stamp(active: &ActiveScan, now: Instant) -> ProgressStamp {
        let counters = &active.counters;
        let total = counters.bytes_total().max(1);
        ProgressStamp {
            phase: counters.phase().code(),
            files_found: counters.files_found(),
            files_done: counters.files_done(),
            bytes_permille: counters.bytes_done().min(total) * 1000 / total,
            elapsed_seconds: now.saturating_duration_since(active.started_at).as_secs(),
        }
    }
}

/// Client-side state of the compaction optimizer for both agents: one
/// [`ScanView`] each, driven by explicit [`Self::start_scan`] calls and
/// advanced by [`Self::drain_events`] on the UI tick.
pub struct CompactionOptimizer {
    client: Option<Client>,
    claude: AgentScanState,
    codex: AgentScanState,
}

impl std::fmt::Debug for CompactionOptimizer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CompactionOptimizer")
            .field("claude_scanning", &self.claude.active.is_some())
            .field("codex_scanning", &self.codex.active.is_some())
            .finish()
    }
}

impl Default for CompactionOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

impl CompactionOptimizer {
    /// Without an execution client (outside tests) until
    /// [`Self::configure_execution`] gives it one, like `CostHistory`.
    pub fn new() -> Self {
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
            claude: AgentScanState::new(),
            codex: AgentScanState::new(),
        }
    }

    /// An optimizer that runs its scans on `client`.
    pub fn with_client(client: Client) -> Self {
        let mut optimizer = Self::new();
        optimizer.client = Some(client);
        optimizer
    }

    /// Supplies the execution client (the composition root's I/O tenant).
    pub fn configure_execution(&mut self, client: Client) {
        self.client = Some(client);
    }

    fn state(&self, agent: AgentKind) -> &AgentScanState {
        match agent {
            AgentKind::ClaudeCode => &self.claude,
            AgentKind::Codex => &self.codex,
        }
    }

    fn state_mut(&mut self, agent: AgentKind) -> &mut AgentScanState {
        match agent {
            AgentKind::ClaudeCode => &mut self.claude,
            AgentKind::Codex => &mut self.codex,
        }
    }

    /// Whether a scan of `agent` is running.
    pub fn is_scanning(&self, agent: AgentKind) -> bool {
        self.state(agent).active.is_some()
    }

    /// The report of the last successful scan, whatever the later scans did.
    pub fn report(&self, agent: AgentKind) -> Option<&CompactionReport> {
        self.state(agent)
            .generation
            .as_ref()
            .map(|generation| &generation.report)
    }

    /// The shared progress counters of the running scan, if any.
    pub fn counters(&self, agent: AgentKind) -> Option<&Arc<ScanProgressCounters>> {
        self.state(agent)
            .active
            .as_ref()
            .map(|active| &active.counters)
    }

    /// What to draw for `agent`.
    pub fn view(&self, agent: AgentKind) -> ScanView<'_> {
        let state = self.state(agent);
        if let Some(active) = &state.active {
            let counters = &active.counters;
            return match counters.phase() {
                ScanPhase::Listing => ScanView::Listing {
                    files_found: counters.files_found(),
                },
                phase => ScanView::Scanning(ScanProgress {
                    phase,
                    files_done: counters.files_done(),
                    files_total: counters.files_total(),
                    bytes_done: counters.bytes_done(),
                    bytes_total: counters.bytes_total(),
                    elapsed: state.last_now.map_or(Duration::ZERO, |now| {
                        now.saturating_duration_since(active.started_at)
                    }),
                    current_file_name: counters.current_file_name(),
                }),
            };
        }
        match &state.status {
            Status::Failed(message) => ScanView::Failed(message),
            Status::Cancelled => ScanView::Cancelled,
            Status::Idle => state
                .generation
                .as_ref()
                .map_or(ScanView::Idle, |generation| {
                    ScanView::Ready(&generation.report)
                }),
        }
    }

    /// Starts a scan of `agent`'s transcripts below `home`, caching traces in
    /// `cache_dir` (`None` disables the cache). Returns whether a worker was
    /// started; `false` when one already runs or admission failed (the reason
    /// is then in [`ScanView::Failed`]).
    pub fn start_scan(
        &mut self,
        agent: AgentKind,
        home: PathBuf,
        cache_dir: Option<PathBuf>,
        settings: ScanSettingsInput,
    ) -> bool {
        if self.state(agent).active.is_some() {
            return false;
        }
        let Some(client) = self.client.clone() else {
            self.state_mut(agent).status =
                Status::Failed("Compaction scan worker unavailable".into());
            return false;
        };
        let generation = match client
            .retirement()
            .try_reserve::<ScanGeneration>(REPORT_RESERVATION_BYTES)
        {
            Ok(generation) => generation,
            Err(reason) => {
                self.state_mut(agent).status =
                    Status::Failed(format!("Compaction scan storage admission: {reason:?}"));
                return false;
            }
        };
        let counters = Arc::new(ScanProgressCounters::default());
        let job = CompactionScan {
            agent,
            home,
            cache_path: cache_dir.map(|directory| directory.join(cache_file_name(agent))),
            settings,
            counters: Arc::clone(&counters),
            generation,
        };
        match client.try_submit(Lane::Io, SCAN_COST, job) {
            Ok(receipt) => {
                let state = self.state_mut(agent);
                state.active = Some(ActiveScan {
                    receipt,
                    counters,
                    started_at: Instant::now(),
                });
                state.status = Status::Idle;
                state.last_stamp = ProgressStamp::default();
                state.last_now = None;
                true
            }
            Err(rejected) => {
                self.state_mut(agent).status =
                    Status::Failed(format!("Compaction scan admission: {:?}", rejected.reason));
                false
            }
        }
    }

    /// Asks the running scan of `agent` to stop; the previous report stays.
    /// The view turns [`ScanView::Cancelled`] once the worker has returned.
    pub fn cancel_scan(&mut self, agent: AgentKind) {
        if let Some(active) = &self.state(agent).active {
            active.receipt.cancel();
        }
    }

    /// Applies finished scans and reports progress. Returns whether anything
    /// the UI shows changed (a progress tick or a finished scan).
    pub fn drain_events(&mut self, now: Instant) -> bool {
        let claude = Self::drain_agent(&mut self.claude, now);
        let codex = Self::drain_agent(&mut self.codex, now);
        claude || codex
    }

    fn drain_agent(state: &mut AgentScanState, now: Instant) -> bool {
        let Some(active) = &mut state.active else {
            return false;
        };
        state.last_now = Some(now);
        let outcome = match active.receipt.try_take() {
            JobPoll::Pending => {
                let stamp = AgentScanState::stamp(active, now);
                let changed = stamp != state.last_stamp;
                state.last_stamp = stamp;
                return changed;
            }
            JobPoll::Ready(outcome) => Some(outcome),
            _ => None,
        };
        if state.active.take().is_none() {
            return false;
        }
        let Some(outcome) = outcome else {
            state.status = Status::Failed("Compaction scan receipt lost".into());
            return true;
        };
        let (outcome, _retention) = outcome.into_parts();
        match outcome {
            JobOutcome::Finished(Ok(ScanResult { generation })) => {
                state.generation = Some(generation);
                state.status = Status::Idle;
            }
            JobOutcome::Finished(Err(ScanCancelled)) | JobOutcome::NotStarted { .. } => {
                state.status = Status::Cancelled;
            }
            JobOutcome::Panicked => {
                state.status = Status::Failed("Compaction scan worker failed".into());
            }
        }
        true
    }

    /// Stops every running scan and forgets nothing else (used on shutdown).
    pub fn cancel_all(&mut self) {
        for agent in AgentKind::ALL {
            self.cancel_scan(agent);
        }
    }
}

impl Drop for CompactionOptimizer {
    fn drop(&mut self) {
        self.cancel_all();
    }
}

#[cfg(test)]
pub(crate) mod fixtures;
#[cfg(test)]
mod tests;
