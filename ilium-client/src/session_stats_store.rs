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
#[derive(Debug, Clone)]
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
    session_id: String,
    in_flight: bool,
    last_started: Option<Instant>,
}

enum StatsEvent {
    Progress {
        pane_id: NodeId,
        session_id: String,
        stats: SessionStats,
        done: u64,
        total: u64,
    },
    Finished {
        pane_id: NodeId,
        session_id: String,
        path: PathBuf,
        accumulator: Box<StatsAccumulator>,
        stats: SessionStats,
    },
    Failed {
        pane_id: NodeId,
        session_id: String,
        message: String,
    },
}

/// The store the app owns; see the module documentation.
pub struct SessionStatsStore {
    entries: HashMap<NodeId, StatsEntry>,
    events_tx: Sender<StatsEvent>,
    events_rx: Receiver<StatsEvent>,
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
        }
    }
}

impl SessionStatsStore {
    pub fn entry(&self, pane_id: NodeId) -> Option<&StatsEntry> {
        self.entries.get(&pane_id)
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
            session_id: request.session_id.clone(),
            in_flight: false,
            last_started: None,
        });
        // A different session in the same pane (the agent was restarted or
        // resumed) invalidates everything cached, including the accumulator.
        if entry.session_id != request.session_id {
            *entry = StatsEntry {
                stats: None,
                state: LoadState::Idle,
                accumulator: None,
                transcript_path: None,
                session_id: request.session_id.clone(),
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
        entry.in_flight = true;
        entry.last_started = Some(now);
        if entry.stats.is_none() {
            entry.state = LoadState::Loading { done: 0, total: 0 };
        }

        let accumulator = entry.accumulator.take();
        let known_path = entry.transcript_path.clone();
        let events_tx = self.events_tx.clone();
        std::thread::spawn(move || run_pass(pane_id, request, known_path, accumulator, events_tx));
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
                session_id,
                stats,
                done,
                total,
            } => {
                let Some(entry) = self.live_entry(pane_id, &session_id) else {
                    return false;
                };
                entry.stats = Some(Arc::new(stats));
                entry.state = LoadState::Loading { done, total };
                true
            }
            StatsEvent::Finished {
                pane_id,
                session_id,
                path,
                accumulator,
                stats,
            } => {
                let Some(entry) = self.live_entry(pane_id, &session_id) else {
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
                session_id,
                message,
            } => {
                let Some(entry) = self.live_entry(pane_id, &session_id) else {
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
    fn live_entry(&mut self, pane_id: NodeId, session_id: &str) -> Option<&mut StatsEntry> {
        self.entries
            .get_mut(&pane_id)
            .filter(|entry| entry.session_id == session_id)
    }
}

fn run_pass(
    pane_id: NodeId,
    request: StatsRequest,
    known_path: Option<PathBuf>,
    accumulator: Option<Box<StatsAccumulator>>,
    events_tx: Sender<StatsEvent>,
) {
    lower_current_thread(WorkerPriority::BelowNormal);
    let StatsRequest {
        class,
        session_id,
        project_path,
        home,
    } = request;
    let fail = |message: String| {
        let _ = events_tx.send(StatsEvent::Failed {
            pane_id,
            session_id: session_id.clone(),
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
    let mut accumulator = accumulator.unwrap_or_else(|| Box::new(StatsAccumulator::new(class)));
    let result = accumulator.ingest_file(&path, |partial, done, total| {
        if done >= total {
            return;
        }
        let _ = events_tx.send(StatsEvent::Progress {
            pane_id,
            session_id: session_id.clone(),
            stats: partial.snapshot(),
            done,
            total,
        });
    });
    match result {
        Ok(()) => {
            let stats = accumulator.snapshot();
            let _ = events_tx.send(StatsEvent::Finished {
                pane_id,
                session_id: session_id.clone(),
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
        let project_path = project.path().canonicalize().unwrap();
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
