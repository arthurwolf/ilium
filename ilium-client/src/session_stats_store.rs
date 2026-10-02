//! Per-pane cache and background worker for [`crate::session_stats`].
//!
//! Parsing a transcript can mean reading gigabytes, so it never runs on the
//! UI thread. One worker thread per pane at a time reads only the bytes
//! appended since the previous pass (the [`StatsAccumulator`] is handed back
//! and forth between the store and the worker) and posts snapshots over a
//! channel that the UI tick drains. While a large file is still being scanned
//! the worker also posts partial snapshots, so the popover fills in live.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ilium_agent_session::TranscriptLocator;
use ilium_core::{AgentClass, NodeId};
use ilium_platform::thread_priority::{lower_current_thread, WorkerPriority};

use crate::session_stats::{SessionStats, StatsAccumulator};

/// Minimum spacing between background refreshes of one open popover. The
/// transcript is append-only and refreshes are incremental, so this only
/// bounds how quickly new activity shows up.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(3);

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
    events_tx: Sender<StatsEvent>,
    events_rx: Receiver<StatsEvent>,
    next_generation: u64,
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
        let (events_tx, events_rx) = channel();
        Self {
            entries: HashMap::new(),
            events_tx,
            events_rx,
            next_generation: 0,
        }
    }
}

impl SessionStatsStore {
    /// Transcript readers currently running.
    pub fn in_flight_count(&self) -> usize {
        self.entries
            .values()
            .filter(|entry| entry.in_flight)
            .count()
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
    }

    /// Drops the cache of every pane `is_live` rejects, so entries for closed
    /// panes do not accumulate.
    pub fn retain_panes(&mut self, is_live: impl Fn(NodeId) -> bool) {
        self.entries
            .retain(|pane_id, entry| entry.in_flight || is_live(*pane_id));
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
        let entry = self.entries.entry(pane_id).or_insert_with(|| StatsEntry {
            stats: None,
            state: LoadState::Idle,
            accumulator: None,
            transcript_path: None,
            request: request.clone(),
            generation: 0,
            in_flight: false,
            last_started: None,
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

        let accumulator = entry.accumulator.take();
        let known_path = entry.transcript_path.clone();
        let events_tx = self.events_tx.clone();
        std::thread::spawn(move || {
            run_pass(
                pane_id,
                request,
                generation,
                known_path,
                accumulator,
                events_tx,
            )
        });
        true
    }

    /// Applies every finished or partial result. Returns whether anything a
    /// viewer could see changed.
    pub fn drain_events(&mut self) -> bool {
        let mut changed = false;
        while let Ok(event) = self.events_rx.try_recv() {
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
fn claude_subagent_files(main_transcript: &std::path::Path) -> Vec<PathBuf> {
    const MAX_DEPTH: usize = 6;
    let mut files = Vec::new();
    let mut pending = vec![(
        main_transcript.with_extension("").join("subagents"),
        0_usize,
    )];
    while let Some((directory, depth)) = pending.pop() {
        let Ok(children) = std::fs::read_dir(&directory) else {
            continue;
        };
        for child in children.flatten() {
            let path = child.path();
            let Ok(kind) = child.file_type() else {
                continue;
            };
            if kind.is_dir() && depth < MAX_DEPTH {
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
    files
}

fn run_pass(
    pane_id: NodeId,
    request: StatsRequest,
    generation: u64,
    known_path: Option<PathBuf>,
    accumulator: Option<Box<StatsAccumulator>>,
    events_tx: Sender<StatsEvent>,
) {
    lower_current_thread(WorkerPriority::BelowNormal);
    let event_request = request.clone();
    let StatsRequest {
        class,
        session_id,
        project_path,
        home,
    } = request;
    let fail = |message: String| {
        let _ = events_tx.send(StatsEvent::Failed {
            pane_id,
            request: event_request.clone(),
            generation,
            message,
        });
    };

    // The locator walks the agent's session store, which for Codex means every
    // date directory, so the resolved path is cached after the first pass.
    let path = known_path.or_else(|| {
        TranscriptLocator::new(&home, &project_path)
            .transcript_for_session(&class, &session_id)
            .map(|transcript| transcript.path)
    });
    let Some(path) = path else {
        fail("No verified transcript file for this session yet.".to_string());
        return;
    };
    let mut accumulator =
        accumulator.unwrap_or_else(|| Box::new(StatsAccumulator::new(class.clone())));
    let result = accumulator.ingest_file(&path, |partial, done, total| {
        if done >= total {
            return;
        }
        let _ = events_tx.send(StatsEvent::Progress {
            pane_id,
            request: event_request.clone(),
            generation,
            stats: partial.snapshot(),
            done,
            total,
        });
    });
    if result.is_ok() && class == AgentClass::Claude {
        // Sub-agent calls are billed to this session but live in their own files.
        for extra in claude_subagent_files(&path) {
            let _ = accumulator.ingest_extra_file(&extra);
        }
    }
    match result {
        Ok(()) => {
            let stats = accumulator.snapshot();
            let _ = events_tx.send(StatsEvent::Finished {
                pane_id,
                request: event_request.clone(),
                generation,
                path,
                accumulator,
                stats,
            });
        }
        Err(error) => fail(format!("Could not read {}: {error}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut store = SessionStatsStore::default();
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

        let files = claude_subagent_files(&main_transcript);
        assert_eq!(files, vec![agent_file.clone()]);

        let request = StatsRequest {
            class: AgentClass::Claude,
            session_id: id.to_string(),
            project_path,
            home: home.path().to_path_buf(),
        };
        let mut store = SessionStatsStore::default();
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
