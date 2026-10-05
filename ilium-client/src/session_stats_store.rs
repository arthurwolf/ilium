//! Per-pane cache and background worker for [`crate::session_stats`].
//!
//! Parsing a transcript can mean reading gigabytes, so it never runs on the
//! UI thread. Bounded jobs on the shared I/O bank read only the bytes
//! appended since the previous pass (the [`StatsAccumulator`] is handed back
//! and forth between the store and the worker) and posts snapshots over a
//! channel that the UI tick drains. While a large file is still being scanned
//! the worker also posts partial snapshots, so the popover fills in live.

use ilium_execution::{Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt};
use std::collections::HashMap;
use std::path::PathBuf;
#[cfg(test)]
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ilium_agent_session::TranscriptLocator;
use ilium_core::{AgentClass, NodeId};

use crate::session_stats::{SessionStats, StatsAccumulator};

/// Minimum spacing between background refreshes of one open popover. The
/// transcript is append-only and refreshes are incremental, so this only
/// bounds how quickly new activity shows up.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(3);

/// Transcript readers allowed at once. One slot belongs to the background cost
/// overlay (see `MAX_CONCURRENT_STATS_WORKERS`), the rest stay free so an open
/// statistics dialog never waits behind multi-gigabyte background reads.
const MAX_STATS_PASSES: usize = 3;

/// Panes whose statistics are retained at once (each may hold up to 32 MiB).
const MAX_STATS_ENTRIES: usize = 16;

/// Everything the worker needs to find and read one pane's transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatsRequest {
    pub class: AgentClass,
    pub session_id: String,
    pub project_path: PathBuf,
    pub home: PathBuf,
}

/// What the popover can say about a pane's data right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadState {
    /// Nothing requested yet.
    Idle,
    /// A pass is running; `done`/`total` are bytes of the transcript.
    Loading { done: u64, total: u64 },
    /// The last pass finished; the cached snapshot is current as of then.
    Ready,
    /// No transcript could be read; the reason is shown verbatim.
    Unavailable(String),
}

/// One pane's cached statistics.
#[derive(Debug)]
pub struct StatsEntry {
    pub stats: Option<Arc<SessionStats>>,
    pub state: LoadState,
    accumulator: Option<Box<StatsAccumulator>>,
    transcript_path: Option<PathBuf>,
    request: StatsRequest,
    generation: u64,
    in_flight: bool,
    last_started: Option<Instant>,
    // Request and resolved path outlive every finite read envelope.
    _context_storage: Option<Arc<ilium_execution::StorageAdmission>>,
}

enum StatsEvent {
    Progress {
        pane_id: NodeId,
        request: StatsRequest,
        generation: u64,
        stats: SessionStats,
        done: u64,
        total: u64,
    },
    Finished {
        pane_id: NodeId,
        request: StatsRequest,
        generation: u64,
        path: PathBuf,
        accumulator: Box<StatsAccumulator>,
        stats: SessionStats,
    },
    Failed {
        pane_id: NodeId,
        request: StatsRequest,
        generation: u64,
        message: String,
    },
}

/// The store the app owns; see the module documentation.
pub struct SessionStatsStore {
    entries: HashMap<NodeId, StatsEntry>,
    client: Option<Client>,
    passes: Vec<ActivePass>,
    #[cfg(test)]
    events_tx: SyncSender<StatsEvent>,
    #[cfg(test)]
    events_rx: Receiver<StatsEvent>,
    next_generation: u64,
    diagnostic: Option<String>,
    storage_quota: ilium_execution::QuotaGroup,
}

impl std::fmt::Debug for SessionStatsStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionStatsStore")
            .field("entries", &self.entries.len())
            .finish()
    }
}

impl Default for SessionStatsStore {
    fn default() -> Self {
        #[cfg(test)]
        let (events_tx, events_rx) = sync_channel(8);
        Self {
            entries: HashMap::new(),
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
            passes: Vec::new(),
            #[cfg(test)]
            events_tx,
            #[cfg(test)]
            events_rx,
            next_generation: 0,
            diagnostic: None,
            storage_quota: crate::execution::process_quota(),
        }
    }
}

struct ActivePass {
    pane_id: NodeId,
    request: StatsRequest,
    generation: u64,
    progress: Arc<Mutex<Option<StatsEvent>>>,
    receipt: Receipt<StatsPass>,
}
struct StatsPass {
    pane_id: NodeId,
    request: StatsRequest,
    generation: u64,
    known_path: Option<PathBuf>,
    accumulator: Option<Box<StatsAccumulator>>,
    progress: Arc<Mutex<Option<StatsEvent>>>,
    storage: crate::session_stats::StatsRetention,
    storage_quota: ilium_execution::QuotaGroup,
}
impl Job for StatsPass {
    type Output = StatsEvent;
    type Error = String;
    fn run(self, context: JobContext) -> Result<StatsEvent, String> {
        run_pass(self, context)
    }
}
impl SessionStatsStore {
    pub(crate) fn diagnostic(&self) -> Option<&str> {
        self.diagnostic.as_deref()
    }
    pub(crate) fn configure_execution(&mut self, client: Client) {
        self.client = Some(client);
    }
    pub(crate) fn cancel_pending(&mut self) {
        for pass in &self.passes {
            pass.receipt.cancel();
        }
    }

    /// Transcript readers currently running.
    pub fn in_flight_count(&self) -> usize {
        self.passes.len()
    }

    pub fn entry(&self, pane_id: NodeId) -> Option<&StatsEntry> {
        self.entries.get(&pane_id)
    }

    /// Hides a cache as soon as provider or transcript ownership changes.
    pub fn matching_entry(&self, pane_id: NodeId, request: &StatsRequest) -> Option<&StatsEntry> {
        self.entry(pane_id)
            .filter(|entry| entry.request == *request)
    }

    /// Reconciles current live or historical transcript owners before results
    /// are drained. In-flight work for removed owners cannot revive a cache.
    pub fn reconcile_contexts(&mut self, contexts: &HashMap<NodeId, StatsRequest>) -> bool {
        let previous = self.entries.len();
        self.entries
            .retain(|pane_id, entry| contexts.get(pane_id) == Some(&entry.request));
        for pass in &self.passes {
            if contexts.get(&pass.pane_id) != Some(&pass.request) {
                pass.receipt.cancel();
            }
        }
        previous != self.entries.len()
    }

    /// Seeds a finished snapshot for a pane, for tests of consumers that must
    /// not start a transcript worker.
    #[cfg(test)]
    pub(crate) fn insert_ready_for_test(&mut self, pane_id: NodeId, stats: Arc<SessionStats>) {
        self.entries.insert(
            pane_id,
            StatsEntry {
                stats: Some(stats),
                state: LoadState::Ready,
                accumulator: None,
                transcript_path: None,
                request: StatsRequest {
                    class: AgentClass::Codex,
                    session_id: String::new(),
                    project_path: PathBuf::new(),
                    home: PathBuf::new(),
                },
                generation: 0,
                in_flight: false,
                last_started: None,
                _context_storage: None,
            },
        );
    }

    /// Seeds an explicitly attributed snapshot without starting filesystem I/O.
    #[cfg(test)]
    pub fn insert_ready_for_request_for_test(
        &mut self,
        pane_id: NodeId,
        request: StatsRequest,
        stats: Arc<SessionStats>,
    ) {
        self.insert_ready_for_test(pane_id, stats);
        if let Some(entry) = self.entries.get_mut(&pane_id) {
            entry.request = request;
        }
    }

    /// Drops one pane's cache, e.g. when the pane is gone or its agent
    /// session changed so the old totals no longer apply.
    pub fn forget(&mut self, pane_id: NodeId) {
        self.entries.remove(&pane_id);
        for pass in &self.passes {
            if pass.pane_id == pane_id {
                pass.receipt.cancel();
            }
        }
    }

    /// Drops the cache of every pane `is_live` rejects, so entries for closed
    /// panes do not accumulate.
    pub fn retain_panes(&mut self, is_live: impl Fn(NodeId) -> bool) {
        self.entries.retain(|pane_id, _| is_live(*pane_id));
        for pass in &self.passes {
            if !is_live(pass.pane_id) {
                pass.receipt.cancel();
            }
        }
    }

    /// Starts a background pass for `pane_id` unless one is already running or
    /// the last one started less than [`REFRESH_INTERVAL`] ago. Returns
    /// whether a worker was launched.
    pub fn request_refresh(
        &mut self,
        pane_id: NodeId,
        request: StatsRequest,
        now: Instant,
    ) -> bool {
        self.request_refresh_with_priority(pane_id, request, now, false)
    }

    /// Like [`Self::request_refresh`], for a pane the user is looking at. When
    /// the cache is full the least recently refreshed idle entry of a pane
    /// nobody is viewing gives up its slot, so an open dialog is never left
    /// waiting for a statistics entry that background refreshes will not free.
    pub fn request_refresh_interactive(
        &mut self,
        pane_id: NodeId,
        request: StatsRequest,
        now: Instant,
    ) -> bool {
        self.request_refresh_with_priority(pane_id, request, now, true)
    }

    fn request_refresh_with_priority(
        &mut self,
        pane_id: NodeId,
        request: StatsRequest,
        now: Instant,
        is_interactive: bool,
    ) -> bool {
        if self.passes.len() >= MAX_STATS_PASSES {
            return false;
        }
        if self.entries.len() >= MAX_STATS_ENTRIES && !self.entries.contains_key(&pane_id) {
            let victim = is_interactive
                .then(|| {
                    self.entries
                        .iter()
                        .filter(|(_, entry)| !entry.in_flight)
                        .min_by_key(|(_, entry)| entry.last_started)
                        .map(|(id, _)| *id)
                })
                .flatten();
            match victim {
                Some(victim) => {
                    self.entries.remove(&victim);
                }
                None => {
                    self.diagnostic = Some(
                        "Statistics cache capacity reached; requested pane has no complete statistics"
                            .into(),
                    );
                    return false;
                }
            }
        }
        if request.session_id.capacity() > 64 * 1024
            || request.project_path.capacity() > 64 * 1024
            || request.home.capacity() > 64 * 1024
            || matches!(&request.class, AgentClass::Other(name) if name.capacity() > 64 * 1024)
        {
            self.diagnostic = Some("Statistics context exceeds retained bounds".into());
            return false;
        }
        let Some(client) = self.client.clone() else {
            self.diagnostic = Some("Statistics worker unavailable".into());
            return false;
        };
        let context_bytes = 128 * 1024
            + request.session_id.capacity()
            + request.home.capacity()
            + request.project_path.capacity()
            + match &request.class {
                AgentClass::Other(name) => name.capacity(),
                _ => 0,
            };
        let context_storage = match self.storage_quota.reserve_external_storage(context_bytes) {
            Ok(storage) => Arc::new(storage),
            Err(reason) => {
                let message = format!("Statistics context storage admission: {reason:?}");
                self.diagnostic = Some(message.clone());
                // A refusal other than momentary contention would otherwise leave
                // the dialog on "Reading the session transcript" for good.
                if reason != ilium_execution::RejectReason::Busy {
                    let entry = self.entries.entry(pane_id).or_insert_with(|| StatsEntry {
                        stats: None,
                        state: LoadState::Idle,
                        accumulator: None,
                        transcript_path: None,
                        request: request.clone(),
                        generation: 0,
                        in_flight: false,
                        last_started: None,
                        _context_storage: None,
                    });
                    if entry.stats.is_none() && entry.request == request {
                        entry.state = LoadState::Unavailable(message);
                    }
                }
                return false;
            }
        };
        let entry = self.entries.entry(pane_id).or_insert_with(|| StatsEntry {
            stats: None,
            state: LoadState::Idle,
            accumulator: None,
            transcript_path: None,
            request: request.clone(),
            generation: 0,
            in_flight: false,
            last_started: None,
            _context_storage: Some(Arc::clone(&context_storage)),
        });
        // Provider, session and transcript-store ownership all fence the cache,
        // including the incremental accumulator and resolved path.
        if entry.request != request {
            *entry = StatsEntry {
                stats: None,
                state: LoadState::Idle,
                accumulator: None,
                transcript_path: None,
                request: request.clone(),
                generation: 0,
                in_flight: false,
                last_started: None,
                _context_storage: Some(Arc::clone(&context_storage)),
            };
        }
        if entry.in_flight {
            return false;
        }
        if entry
            .last_started
            .is_some_and(|started| now.duration_since(started) < REFRESH_INTERVAL)
        {
            return false;
        }
        let Some(generation) = self.next_generation.checked_add(1) else {
            return false;
        };
        self.next_generation = generation;
        entry.generation = generation;
        entry.in_flight = true;
        entry.last_started = Some(now);
        if entry.stats.is_none() {
            entry.state = LoadState::Loading { done: 0, total: 0 };
        }

        let storage = match self
            .storage_quota
            .reserve_external_storage(32 * 1024 * 1024)
        {
            Ok(storage) => crate::session_stats::StatsRetention::new(Arc::new(storage)),
            Err(reason) => {
                entry.in_flight = false;
                entry.last_started = None;
                entry.state =
                    LoadState::Unavailable(format!("Statistics storage admission: {reason:?}"));
                return false;
            }
        };
        let progress = Arc::new(Mutex::new(None));
        let job = StatsPass {
            pane_id,
            request: request.clone(),
            generation,
            known_path: entry.transcript_path.clone(),
            accumulator: entry.accumulator.take(),
            progress: Arc::clone(&progress),
            storage,
            storage_quota: self.storage_quota.clone(),
        };
        match client.try_submit(
            Lane::Io,
            // The pass streams the transcript through a 1 MiB reader and 1 MiB
            // line buffer; retained statistics are charged separately as
            // external storage. Declaring more only starves the shared 512 MiB
            // input budget (history scan, cost overlay) of admissions.
            JobCost {
                input_bytes: 16 * 1024 * 1024,
                result_bytes: 1024 * 1024,
            },
            job,
        ) {
            Ok(receipt) => self.passes.push(ActivePass {
                pane_id,
                request,
                generation,
                progress,
                receipt,
            }),
            Err(rejected) => {
                entry.accumulator = rejected.value.accumulator;
                entry.in_flight = false;
                entry.last_started = None;
                entry.state = LoadState::Unavailable(format!(
                    "Statistics admission: {:?}; refresh retained for retry",
                    rejected.reason
                ));
                return false;
            }
        }

        self.diagnostic = None;
        true
    }

    /// Applies every finished or partial result. Returns whether anything a
    /// viewer could see changed.
    pub fn drain_events(&mut self) -> bool {
        let mut changed = false;
        #[cfg(test)]
        while let Ok(event) = self.events_rx.try_recv() {
            changed |= self.apply(event);
        }
        let mut events = Vec::new();
        self.passes.retain_mut(|pass| {
            if let Ok(mut progress) = pass.progress.try_lock() {
                if let Some(event) = progress.take() {
                    events.push(event);
                }
            }
            match pass.receipt.try_take() {
                JobPoll::Pending => true,
                JobPoll::Ready(outcome) => {
                    let (outcome, hold) = outcome.into_parts();
                    let event = match outcome {
                        JobOutcome::Finished(Ok(event)) => event,
                        JobOutcome::Finished(Err(message)) => StatsEvent::Failed {
                            pane_id: pass.pane_id,
                            request: pass.request.clone(),
                            generation: pass.generation,
                            message,
                        },
                        _ => StatsEvent::Failed {
                            pane_id: pass.pane_id,
                            request: pass.request.clone(),
                            generation: pass.generation,
                            message: "Statistics pass cancelled or failed; totals are incomplete"
                                .into(),
                        },
                    };
                    drop(hold);
                    events.push(event);
                    false
                }
                _ => {
                    events.push(StatsEvent::Failed {
                        pane_id: pass.pane_id,
                        request: pass.request.clone(),
                        generation: pass.generation,
                        message: "Statistics receipt lost; totals are incomplete".into(),
                    });
                    false
                }
            }
        });
        for event in events {
            changed |= self.apply(event);
        }

        changed
    }

    fn apply(&mut self, event: StatsEvent) -> bool {
        match event {
            StatsEvent::Progress {
                pane_id,
                request,
                generation,
                stats,
                done,
                total,
            } => {
                let Some(entry) = self.live_entry(pane_id, &request, generation) else {
                    return false;
                };
                entry.stats = Some(Arc::new(stats));
                entry.state = LoadState::Loading { done, total };
                true
            }
            StatsEvent::Finished {
                pane_id,
                request,
                generation,
                path,
                accumulator,
                stats,
            } => {
                let Some(entry) = self.live_entry(pane_id, &request, generation) else {
                    return false;
                };
                entry.in_flight = false;
                entry.transcript_path = Some(path);
                entry.accumulator = Some(accumulator);
                entry.stats = Some(Arc::new(stats));
                entry.state = LoadState::Ready;
                true
            }
            StatsEvent::Failed {
                pane_id,
                request,
                generation,
                message,
            } => {
                let Some(entry) = self.live_entry(pane_id, &request, generation) else {
                    return false;
                };
                entry.in_flight = false;
                entry.state = LoadState::Unavailable(message);
                true
            }
        }
    }

    /// The entry a worker result belongs to, provided it still describes the
    /// same session: a result for a superseded session is discarded.
    fn live_entry(
        &mut self,
        pane_id: NodeId,
        request: &StatsRequest,
        generation: u64,
    ) -> Option<&mut StatsEntry> {
        self.entries
            .get_mut(&pane_id)
            .filter(|entry| entry.request == *request && entry.generation == generation)
    }
}

/// Sub-agent transcripts of a Claude Code session: every `.jsonl` below
/// `<project dir>/<session id>/subagents/`, including per-workflow folders.
fn claude_subagent_files(main_transcript: &std::path::Path) -> Result<Vec<PathBuf>, String> {
    const MAX_DEPTH: usize = 6;
    let mut files = Vec::new();
    let mut path_bytes = 0;
    let mut scanned = 0;
    let mut pending = vec![(
        main_transcript.with_extension("").join("subagents"),
        0_usize,
    )];
    while let Some((directory, depth)) = pending.pop() {
        let children = match std::fs::read_dir(&directory) {
            Ok(children) => children,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(format!(
                    "Subagent transcript inventory: {error}; statistics incomplete"
                ))
            }
        };
        for child in children.flatten() {
            scanned += 1;
            if scanned > 4096 || files.len() + pending.len() >= 4096 {
                return Err(
                    "Subagent transcript inventory exceeds 4096 entries; statistics incomplete"
                        .into(),
                );
            }
            let path = child.path();
            path_bytes += path.capacity();
            if path.capacity() > 64 * 1024 || path_bytes > 1024 * 1024 {
                return Err(
                    "Subagent transcript path exceeds bounds; statistics incomplete".into(),
                );
            }
            let Ok(kind) = child.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if depth >= MAX_DEPTH {
                    return Err("Subagent transcript depth exceeds6; statistics incomplete".into());
                }
                pending.push((path, depth + 1));
            } else if path
                .extension()
                .is_some_and(|extension| extension == "jsonl")
            {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn run_pass(pass: StatsPass, context: JobContext) -> Result<StatsEvent, String> {
    let StatsPass {
        pane_id,
        request,
        generation,
        known_path,
        accumulator,
        progress,
        storage,
        storage_quota,
    } = pass;
    let locator = TranscriptLocator::new_bounded(
        &request.home,
        &request.project_path,
        ilium_agent_session::TranscriptReadLimits {
            line_bytes: 1024 * 1024,
            total_read_bytes: 16 * 1024 * 1024,
            scanned_entries: 4096,
            retained_path_bytes: 1024 * 1024,
        },
    );
    let path = known_path.or_else(|| {
        locator
            .transcript_for_session(&request.class, &request.session_id)
            .map(|transcript| transcript.path)
    });
    if locator.read_limit_reached() {
        return Err("Statistics transcript discovery exceeded bounded evidence limits".into());
    }
    let Some(path) = path else {
        return Err("No verified transcript file for this session yet.".into());
    };
    if path.capacity() > 64 * 1024 {
        return Err("Statistics transcript path exceeds bounded storage".into());
    }
    let mut accumulator =
        accumulator.unwrap_or_else(|| Box::new(StatsAccumulator::new(request.class.clone())));
    accumulator.attach_retention(storage);
    accumulator
        .ingest_file_cancellable(
            &path,
            || context.stop_requested(),
            |partial, done, total| {
                if done >= total {
                    return;
                }
                let quota = &storage_quota;
                let Ok(peak) = quota.reserve_external_storage(16 * 1024 * 1024) else {
                    return;
                };
                let Ok(mut slot) = progress.try_lock() else {
                    return;
                };
                let mut stats = partial.snapshot();
                let bytes = stats.retained_bytes().saturating_add(1024 * 1024);
                if bytes > 16 * 1024 * 1024 {
                    return;
                }
                let hold = quota.reserve_external_storage(bytes).unwrap_or(peak);
                stats.retention = crate::session_stats::StatsRetention::new(Arc::new(hold));
                *slot = Some(StatsEvent::Progress {
                    pane_id,
                    request: request.clone(),
                    generation,
                    stats,
                    done,
                    total,
                });
            },
        )
        .map_err(|error| {
            format!(
                "Could not read {}: {error}; statistics incomplete",
                path.display()
            )
        })?;
    if request.class == AgentClass::Claude {
        for extra in claude_subagent_files(&path)? {
            if context.stop_requested() {
                return Err("Statistics cancelled; totals incomplete".into());
            }
            accumulator.ingest_extra_file(&extra).map_err(|error| {
                format!(
                    "Could not read subagent {}: {error}; statistics incomplete",
                    extra.display()
                )
            })?;
        }
    }
    if context.stop_requested() {
        return Err("Statistics cancelled; totals incomplete".into());
    }
    let mut stats = accumulator.snapshot();
    // The admitted peak remains alive across snapshot allocation and every
    // attempted shrink. A refused shrink keeps the safe independent peak;
    // it never pins a finite job credit or introduces an uncharged gap.
    let bytes = accumulator
        .retained_bytes()
        .saturating_add(stats.retained_bytes())
        .saturating_add(1024 * 1024);
    if bytes > 32 * 1024 * 1024 {
        return Err(
            "Statistics final physical capacity exceeds admission; totals incomplete".into(),
        );
    }
    if let Ok(hold) = storage_quota.reserve_external_storage(bytes) {
        let retention = crate::session_stats::StatsRetention::new(Arc::new(hold));
        accumulator.attach_retention(retention.clone());
        stats.retention = retention;
    }
    Ok(StatsEvent::Finished {
        pane_id,
        request,
        generation,
        path,
        accumulator,
        stats,
    })
}
impl Drop for SessionStatsStore {
    fn drop(&mut self) {
        self.cancel_pending();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These transcript/throttle fixtures own their tenant and IO bank;
    // unrelated parallel tests cannot consume their first-pass admission.
    fn isolated_stats_store() -> (ilium_execution::Execution, SessionStatsStore) {
        use ilium_execution::{
            ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits,
        };
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 1,
            jobs: 16,
            service_jobs: 0,
            input_bytes: 384 * 1024 * 1024,
            result_bytes: 256 * 1024 * 1024,
            worker_threads: 1,
            worker_bytes: 2 * 1024 * 1024,
        });
        let bank = |threads| LaneConfig {
            threads,
            queue_slots: if threads == 0 { 0 } else { 4 },
            priority: None,
            resident_bytes_per_thread: if threads == 0 { 0 } else { 1024 * 1024 },
        };
        let owner = Execution::start(
            quota,
            ExecutionConfig {
                cpu: bank(0),
                io: bank(1),
                service: bank(0),
            },
        )
        .unwrap();
        let client = owner
            .client(ClientLimits {
                jobs: 16,
                service_jobs: 0,
                input_bytes: 384 * 1024 * 1024,
                result_bytes: 256 * 1024 * 1024,
            })
            .unwrap();
        let mut store = SessionStatsStore::default();
        store.configure_execution(client);
        (owner, store)
    }

    fn write_claude_transcript(home: &std::path::Path, project: &std::path::Path, id: &str) {
        let slug: String = project
            .to_string_lossy()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let directory = home.join(".claude").join("projects").join(slug);
        std::fs::create_dir_all(&directory).unwrap();
        let lines = [
            serde_json::json!({
                "type": "user", "sessionId": id, "cwd": project,
                "timestamp": "2026-09-25T10:00:00.000Z",
                "message": {"content": "hello there"}
            }),
            serde_json::json!({
                "type": "assistant", "sessionId": id, "cwd": project,
                "timestamp": "2026-09-25T10:00:05.000Z",
                "message": {"id": "msg_1", "model": "claude-sonnet-5", "content": [],
                    "usage": {"input_tokens": 3, "output_tokens": 9,
                        "cache_read_input_tokens": 100, "cache_creation_input_tokens": 0}}
            }),
        ];
        let body: String = lines.iter().map(|line| format!("{line}\n")).collect();
        std::fs::write(directory.join(format!("{id}.jsonl")), body).unwrap();
    }

    fn wait_for(store: &mut SessionStatsStore, pane_id: NodeId) {
        for _ in 0..200 {
            store.drain_events();
            if store
                .entry(pane_id)
                .is_some_and(|entry| entry.state != LoadState::Idle)
                && store
                    .entry(pane_id)
                    .is_some_and(|entry| !matches!(entry.state, LoadState::Loading { .. }))
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("worker did not finish");
    }

    #[test]
    fn twelve_installed_statistics_release_the_single_finite_job_slot() {
        use ilium_execution::{
            ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits,
            ShutdownMode,
        };
        let mib = 1024 * 1024;
        // Persistent sources are isolated only for this resource forcing test;
        // production always shares execution::process_quota(). The 64MiB
        // physical budget covers one admitted32MiB peak plus installed sources.
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 1,
            jobs: 1,
            service_jobs: 0,
            input_bytes: 128 * mib,
            result_bytes: mib,
            worker_threads: 1,
            worker_bytes: 64 * mib,
        });
        let lane = |threads| LaneConfig {
            threads,
            queue_slots: if threads == 0 { 0 } else { 1 },
            priority: None,
            resident_bytes_per_thread: if threads == 0 { 0 } else { mib },
        };
        let mut owner = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: lane(0),
                io: lane(1),
                service: lane(0),
            },
        )
        .unwrap();
        let client = owner
            .client(ClientLimits {
                jobs: 1,
                service_jobs: 0,
                input_bytes: 128 * mib,
                result_bytes: mib,
            })
            .unwrap();
        let mut store = SessionStatsStore::default();
        store.configure_execution(client);
        store.storage_quota = quota.clone();
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        for number in 0..12 {
            // Discovery verifies provider UUIDs, including for synthetic fixtures.
            let session = format!("00000000-0000-4000-8000-{number:012x}");
            write_claude_transcript(home.path(), project.path(), &session);
            let pane = NodeId(3000 + number);
            assert!(store.request_refresh(
                pane,
                StatsRequest {
                    class: AgentClass::Claude,
                    session_id: session,
                    project_path: project.path().to_path_buf(),
                    home: home.path().to_path_buf()
                },
                Instant::now()
            ));
            wait_for(&mut store, pane);
            let entry = store.entry(pane).unwrap();
            assert_eq!(entry.state, LoadState::Ready);
            let stats = entry.stats.as_ref().unwrap();
            assert_eq!(stats.prompt_count, 1);
            assert_eq!(stats.tokens.output, 9);
            assert_eq!(
                quota.snapshot().jobs,
                0,
                "installed cache must not pin the single job credit"
            );
            assert_eq!(quota.snapshot().input_bytes, 0);
            assert_eq!(quota.snapshot().result_bytes, 0);
        }
        let exported = Arc::clone(store.entry(NodeId(3000)).unwrap().stats.as_ref().unwrap());
        assert_eq!(store.entries.len(), 12);
        drop(store);
        owner.request_shutdown(ShutdownMode::Cancel);
        assert_eq!(
            owner
                .join_until_background(Instant::now() + Duration::from_secs(5))
                .unwrap()
                .remaining_workers,
            0
        );
        drop(owner);
        assert_eq!(quota.snapshot().jobs, 0);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert!(
            quota.snapshot().worker_bytes > 0,
            "exported immutable source survives its producer"
        );
        drop(exported);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn queued_cancelled_pass_keeps_its_slot_until_real_worker_receipt() {
        use ilium_execution::{
            ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits,
            ShutdownMode,
        };
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 1,
            jobs: 16,
            service_jobs: 0,
            input_bytes: 512 * 1024 * 1024,
            result_bytes: 256 * 1024 * 1024,
            worker_threads: 1,
            // Resident IO storage plus shared bank/queue metadata.
            worker_bytes: 2 * 1024 * 1024,
        });
        let bank = |threads| LaneConfig {
            threads,
            queue_slots: if threads == 0 { 0 } else { 4 },
            priority: None,
            resident_bytes_per_thread: if threads == 0 { 0 } else { 1024 * 1024 },
        };
        let mut owner = Execution::start(
            quota,
            ExecutionConfig {
                cpu: bank(0),
                io: bank(1),
                service: bank(0),
            },
        )
        .unwrap();
        let client = owner
            .client(ClientLimits {
                jobs: 16,
                service_jobs: 0,
                input_bytes: 384 * 1024 * 1024,
                result_bytes: 256 * 1024 * 1024,
            })
            .unwrap();
        let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let mut blocked = client
            .try_submit(
                Lane::Io,
                JobCost {
                    input_bytes: 1024 * 1024,
                    result_bytes: 1024 * 1024,
                },
                move |_: JobContext| -> Result<(), String> {
                    started_tx.send(std::thread::current().id()).unwrap();
                    release_rx.recv().unwrap();
                    Ok(())
                },
            )
            .unwrap();
        assert_ne!(
            started_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
            std::thread::current().id()
        );
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let mut store = SessionStatsStore::default();
        store.configure_execution(client.clone());
        let started = Instant::now();
        assert!(store.request_refresh(
            NodeId(88),
            StatsRequest {
                class: AgentClass::Codex,
                session_id: "synthetic-session".into(),
                project_path: project.path().to_path_buf(),
                home: home.path().to_path_buf()
            },
            started
        ));
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(store.in_flight_count(), 1);
        assert!(store.reconcile_contexts(&HashMap::new()));
        assert_eq!(
            store.in_flight_count(),
            1,
            "cancel request cannot pretend the IO owner already retired"
        );
        release_tx.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while store.in_flight_count() != 0 {
            store.drain_events();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(store.entry(NodeId(88)).is_none());
        loop {
            match blocked.try_take() {
                JobPoll::Pending => {
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(1));
                }
                JobPoll::Ready(result) => {
                    drop(result);
                    break;
                }
                _ => panic!("blocker receipt lost"),
            }
        }
        drop(store);
        drop(client);
        owner.request_shutdown(ShutdownMode::Drain);
        assert_eq!(
            owner
                .join_until_background(Instant::now() + Duration::from_secs(3))
                .unwrap()
                .remaining_workers,
            0
        );
    }

    #[test]
    fn a_pass_reads_the_transcript_and_a_second_pass_is_throttled() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        // The locator slugs the platform-canonical form; `std`'s Windows
        // `\\?\` spelling would land the fixture in a different directory.
        let project_path = ilium_platform::paths::canonicalize(project.path()).unwrap();
        let id = "11111111-1111-4111-8111-111111111111";
        write_claude_transcript(home.path(), &project_path, id);

        let request = StatsRequest {
            class: AgentClass::Claude,
            session_id: id.to_string(),
            project_path,
            home: home.path().to_path_buf(),
        };
        let (mut owner, mut store) = isolated_stats_store();
        let pane_id = NodeId(7);
        let started = Instant::now();
        assert!(store.request_refresh(pane_id, request.clone(), started));
        wait_for(&mut store, pane_id);

        let entry = store.entry(pane_id).unwrap();
        assert_eq!(entry.state, LoadState::Ready);
        let stats = entry.stats.as_ref().unwrap();
        assert_eq!(stats.prompt_count, 1);
        assert_eq!(stats.tokens.output, 9);

        assert!(
            !store.request_refresh(pane_id, request.clone(), started + Duration::from_secs(1)),
            "a refresh inside the interval is skipped"
        );
        assert!(store.request_refresh(pane_id, request, started + REFRESH_INTERVAL));
        wait_for(&mut store, pane_id);
        assert_eq!(store.entry(pane_id).unwrap().state, LoadState::Ready);
        drop(store);
        owner.request_shutdown(ilium_execution::ShutdownMode::Drain);
        assert_eq!(
            owner
                .join_until_background(Instant::now() + Duration::from_secs(5))
                .unwrap()
                .remaining_workers,
            0,
            "isolated statistics IO worker joined"
        );
    }

    #[test]
    fn claude_subagent_files_count_toward_the_session_and_are_read_incrementally() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let project_path = ilium_platform::paths::canonicalize(project.path()).unwrap();
        let id = "33333333-3333-4333-8333-333333333333";
        write_claude_transcript(home.path(), &project_path, id);
        let main_transcript = TranscriptLocator::new(home.path(), &project_path)
            .transcript_for_session(&AgentClass::Claude, id)
            .unwrap()
            .path;
        // Workflow runs nest their agents one folder deeper.
        let nested = main_transcript
            .with_extension("")
            .join("subagents")
            .join("workflows")
            .join("wf_1");
        std::fs::create_dir_all(&nested).unwrap();
        let side_call = |message_id: &str, output: u64| {
            format!(
                "{}\n",
                serde_json::json!({
                    "type": "assistant", "sessionId": id, "isSidechain": true,
                    "timestamp": "2026-09-25T10:01:00.000Z",
                    "message": {"id": message_id, "model": "claude-sonnet-5", "content": [],
                        "usage": {"input_tokens": 1, "output_tokens": output,
                            "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0}}
                })
            )
        };
        let agent_file = nested.join("agent-a1.jsonl");
        std::fs::write(&agent_file, side_call("msg_side_1", 1000)).unwrap();

        let files = claude_subagent_files(&main_transcript).unwrap();
        assert_eq!(files, vec![agent_file.clone()]);

        let request = StatsRequest {
            class: AgentClass::Claude,
            session_id: id.to_string(),
            project_path,
            home: home.path().to_path_buf(),
        };
        let (mut owner, mut store) = isolated_stats_store();
        let pane_id = NodeId(9);
        let started = Instant::now();
        store.request_refresh(pane_id, request.clone(), started);
        wait_for(&mut store, pane_id);
        let stats = store.entry(pane_id).unwrap().stats.clone().unwrap();
        assert_eq!(
            stats.tokens.output,
            9 + 1000,
            "main file plus the sub-agent"
        );

        // A second pass reads only what the sub-agent file gained, and never
        // counts the same message id twice.
        let mut grown = std::fs::read_to_string(&agent_file).unwrap();
        grown.push_str(&side_call("msg_side_1", 1000));
        grown.push_str(&side_call("msg_side_2", 500));
        std::fs::write(&agent_file, grown).unwrap();
        assert!(store.request_refresh(pane_id, request, started + REFRESH_INTERVAL));
        // The entry is already Ready from the first pass, so wait for the
        // second pass's result itself rather than for a state change.
        for _ in 0..250 {
            store.drain_events();
            let output = store
                .entry(pane_id)
                .unwrap()
                .stats
                .as_ref()
                .unwrap()
                .tokens
                .output;
            if output != 9 + 1000 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let stats = store.entry(pane_id).unwrap().stats.clone().unwrap();
        assert_eq!(stats.tokens.output, 9 + 1000 + 500);
        drop(store);
        owner.request_shutdown(ilium_execution::ShutdownMode::Drain);
        assert_eq!(
            owner
                .join_until_background(Instant::now() + Duration::from_secs(5))
                .unwrap()
                .remaining_workers,
            0,
            "isolated statistics IO worker joined"
        );
    }

    #[test]
    fn a_missing_transcript_reports_unavailable() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let request = StatsRequest {
            class: AgentClass::Claude,
            session_id: "22222222-2222-4222-8222-222222222222".to_string(),
            project_path: project.path().to_path_buf(),
            home: home.path().to_path_buf(),
        };
        let mut store = SessionStatsStore::default();
        let pane_id = NodeId(3);
        store.request_refresh(pane_id, request, Instant::now());
        wait_for(&mut store, pane_id);
        assert!(matches!(
            store.entry(pane_id).unwrap().state,
            LoadState::Unavailable(_)
        ));
    }
}

#[cfg(test)]
mod independent_identity_tests {
    use super::*;
    fn request() -> StatsRequest {
        StatsRequest {
            class: AgentClass::Claude,
            session_id: "same-id".into(),
            project_path: PathBuf::from("/fixture/old-project"),
            home: PathBuf::from("/fixture/old-home"),
        }
    }
    fn seed(
        store: &mut SessionStatsStore,
        pane_id: NodeId,
        request: StatsRequest,
        generation: u64,
    ) -> Arc<SessionStats> {
        let stats = Arc::new(SessionStats {
            prompt_count: 991,
            ..SessionStats::default()
        });
        store.insert_ready_for_test(pane_id, stats.clone());
        let entry = store.entries.get_mut(&pane_id).unwrap();
        entry.request = request;
        entry.generation = generation;
        entry.in_flight = true;
        entry.transcript_path = Some(PathBuf::from("/fixture/old.jsonl"));
        stats
    }
    #[test]
    fn same_session_string_in_another_owner_must_not_reuse_prior_stats() {
        for changed_field in 0..3 {
            let mut store = SessionStatsStore::default();
            let pane_id = NodeId(700);
            let original = request();
            seed(&mut store, pane_id, original.clone(), 0);
            let mut replacement = original.clone();
            match changed_field {
                0 => replacement.class = AgentClass::Codex,
                1 => replacement.project_path = PathBuf::from("/fixture/new-project"),
                _ => replacement.home = PathBuf::from("/fixture/new-home"),
            }
            assert!(store.matching_entry(pane_id, &replacement).is_none());
            assert!(store.request_refresh(pane_id, replacement, Instant::now()));
            assert!(
                store.entry(pane_id).unwrap().stats.is_none(),
                "old owner snapshot must be hidden immediately"
            );
            assert!(store.entry(pane_id).unwrap().transcript_path.is_none());
        }
    }
    #[test]
    fn unresolved_replacement_drops_inflight_cache_and_all_late_result_kinds() {
        let mut store = SessionStatsStore::default();
        let pane_id = NodeId(701);
        let original = request();
        seed(&mut store, pane_id, original.clone(), 11);
        assert!(store.reconcile_contexts(&HashMap::new()));
        let events = [
            StatsEvent::Progress {
                pane_id,
                request: original.clone(),
                generation: 11,
                stats: SessionStats::default(),
                done: 1,
                total: 2,
            },
            StatsEvent::Failed {
                pane_id,
                request: original.clone(),
                generation: 11,
                message: "old".into(),
            },
            StatsEvent::Finished {
                pane_id,
                request: original,
                generation: 11,
                path: PathBuf::from("old"),
                accumulator: Box::new(StatsAccumulator::new(AgentClass::Claude)),
                stats: SessionStats::default(),
            },
        ];
        for event in events {
            store.events_tx.send(event).unwrap();
        }
        assert!(!store.drain_events());
        assert!(store.entry(pane_id).is_none());
    }
    #[test]
    fn same_historical_transcript_keeps_exact_cached_snapshot() {
        let mut store = SessionStatsStore::default();
        let pane_id = NodeId(702);
        let original = request();
        let stats = seed(&mut store, pane_id, original.clone(), 12);
        assert!(!store.reconcile_contexts(&HashMap::from([(pane_id, original.clone())])));
        assert!(Arc::ptr_eq(
            &stats,
            store
                .matching_entry(pane_id, &original)
                .unwrap()
                .stats
                .as_ref()
                .unwrap()
        ));
    }
    #[test]
    fn returning_to_same_identity_rejects_worker_from_earlier_generation() {
        let mut store = SessionStatsStore::default();
        let pane_id = NodeId(703);
        let original = request();
        seed(&mut store, pane_id, original.clone(), 31);
        store.reconcile_contexts(&HashMap::new());
        let stats = seed(&mut store, pane_id, original.clone(), 32);
        assert!(!store.apply(StatsEvent::Progress {
            pane_id,
            request: original.clone(),
            generation: 31,
            stats: SessionStats::default(),
            done: 1,
            total: 2
        }));
        assert!(!store.apply(StatsEvent::Failed {
            pane_id,
            request: original.clone(),
            generation: 31,
            message: "old".into()
        }));
        assert!(!store.apply(StatsEvent::Finished {
            pane_id,
            request: original,
            generation: 31,
            path: PathBuf::from("old"),
            accumulator: Box::new(StatsAccumulator::new(AgentClass::Claude)),
            stats: SessionStats::default()
        }));
        assert!(Arc::ptr_eq(
            &stats,
            store.entry(pane_id).unwrap().stats.as_ref().unwrap()
        ));
    }
}
