//! Background provider-neutral title-inference workers: project-name
//! bootstrap (once per session) and per-pane session-title inference
//! (`session_naming::infer_pane_title`), each run on a dedicated
//! `std::thread` -- these make a blocking HTTP call, so they must never run
//! on the tokio event loop -- and bridged back into it the same way
//! crossterm input is (see `crate::run`): the worker thread holds a
//! `tokio::sync::mpsc::Sender` directly and calls its ordinary,
//! non-async `blocking_send` from off the runtime, so no second bridging
//! hop is needed.
//!
//! Session-title inference (`spawn_session_title_worker`) receives one
//! immutable `SessionTitleInput` captured by the automatic trigger router or
//! explicit row action before this background boundary.

use std::collections::HashSet;
use std::panic::{self, AssertUnwindSafe, UnwindSafe};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use ilium_agent_session::TranscriptLocator;
use ilium_core::{AgentClass, NodeId};
use ilium_inference::InferenceSettings;
use ilium_platform::thread_priority::{lower_current_thread, WorkerPriority};
use tokio::sync::mpsc::Sender;

use crate::naming::DualTitle;
use crate::project_naming::ProjectNameBootstrap;

/// Whether a finished naming worker's result came from a passive trigger
/// (a session ID just resolving, a turn finishing, every second Enter
/// press) or from the user explicitly clicking the tree row's "retitle"
/// icon (`App::action_request_retitle`). `crate::tick::apply_naming_worker_event`
/// applies the two differently: `Automatic` remains an automatic title the
/// server will not place over a user rename; `Manual` marks a still-current
/// result user-specified, the same as a typed rename. Both use the server's
/// expected-session-ID compare-and-set and are discarded if that session
/// changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleTrigger {
    Automatic,
    Manual,
}

/// A finished background naming result, forwarded into the main event loop.
pub enum NamingWorkerEvent {
    ProjectName(anyhow::Result<ProjectNameBootstrap>),
    SessionTitle(SessionTitleWorkerResult),
    TerminalTitle(NodeId, anyhow::Result<DualTitle>, TitleTrigger),
    InferenceTest {
        provider: ilium_inference::InferenceProviderKind,
        elapsed: Duration,
        result: anyhow::Result<crate::inference_test::InferenceTestResult>,
    },
    ProviderModels {
        provider: ilium_inference::InferenceProviderKind,
        endpoint: String,
        elapsed: Duration,
        result: anyhow::Result<Vec<String>>,
    },
    Restructure(RestructureWorkerResult),
    LastPromptTranscript(LastPromptTranscriptWorkerResult),
}

/// All immutable inputs captured when a session-title worker starts. Keeping
/// the session ID and title generation together makes the stale-result
/// contract explicit at the thread boundary instead of relying on callers to
/// preserve their ordering across a long parameter list.
pub struct SessionTitleWorkerRequest {
    pub home: PathBuf,
    pub input: crate::session_naming::SessionTitleInput,
    pub title_generation: u64,
    pub trigger: TitleTrigger,
}

/// Complete provider-boundary outcome forwarded to the event loop. Named
/// fields keep session/generation fencing distinct from optional diagnostic
/// request/response payloads as this event evolves.
pub struct SessionTitleWorkerResult {
    pub pane_id: NodeId,
    pub session_id: String,
    pub title_generation: u64,
    pub provider: ilium_inference::InferenceProviderKind,
    pub elapsed: Duration,
    pub rendered_prompt: Option<String>,
    pub raw_response: Option<String>,
    pub result: anyhow::Result<DualTitle>,
    pub trigger: TitleTrigger,
}

/// One restructure result plus the activity checkpoint visible to inference.
/// The server applies valid plans even when newer activity arrived, then uses
/// this checkpoint to keep that newer activity eligible for the next pass.
pub struct RestructureWorkerResult {
    pub project_id: NodeId,
    pub inference_activity_revisions: Vec<ilium_core::NodeActivityRevision>,
    pub result: anyhow::Result<ilium_core::RestructurePlan>,
}

/// Immutable inputs for one last-prompt-from-transcript check -- see
/// `App::last_prompt_transcript_context` for how the caller resolves these.
pub struct LastPromptTranscriptWorkerRequest {
    pub home: PathBuf,
    pub pane_id: NodeId,
    pub project_path: PathBuf,
    pub agent_class: AgentClass,
    pub session_id: String,
}

/// The most recent user message the worker found in the transcript, if any
/// -- `None` covers both "no transcript found yet" and "found one with no
/// user entries", neither worth distinguishing to the caller, which either
/// way just leaves the live-tracked value in place.
pub struct LastPromptTranscriptWorkerResult {
    pub pane_id: NodeId,
    pub session_id: String,
    pub last_prompt: Option<String>,
}

/// Initial wait before the first transcript read: the agent CLI needs a
/// moment to flush this turn's submitted message to its own session log, and
/// reading too early would just see the previous turn's already-applied
/// value (or no file at all yet on a session's very first prompt).
const LAST_PROMPT_TRANSCRIPT_INITIAL_DELAY: Duration = Duration::from_millis(400);
/// Polling interval between retries once the initial wait has elapsed.
const LAST_PROMPT_TRANSCRIPT_RETRY_INTERVAL: Duration = Duration::from_millis(400);
/// Upper bound on retries -- worst case ~2.8s total, well inside how long a
/// human already waits after hitting Enter before expecting the banner to
/// reflect what they typed.
const LAST_PROMPT_TRANSCRIPT_MAX_ATTEMPTS: u32 = 6;

/// Tracks which naming workers are currently in flight, so a caller never
/// accidentally spawns a second one for the same target while the first is
/// still running.
pub struct NamingWorkers {
    events_tx: Sender<NamingWorkerEvent>,
    inference_settings: InferenceSettings,
    project_name_in_flight: bool,
    session_title_in_flight: HashSet<(NodeId, String)>,
    terminal_title_in_flight: HashSet<NodeId>,
    inference_test_in_flight: bool,
    model_discovery_in_flight: bool,
    restructure_in_flight: HashSet<NodeId>,
    concurrency_limiter: Arc<InferenceConcurrencyLimiter>,
}

const MAX_CONCURRENT_INFERENCE_JOBS: usize = 2;

/// One process-wide client boundary prevents startup/completion triggers from
/// turning independent per-pane workers into an unbounded provider burst.
struct InferenceConcurrencyLimiter {
    active_jobs: Mutex<usize>,
    available: Condvar,
    maximum: usize,
}

impl InferenceConcurrencyLimiter {
    fn new(maximum: usize) -> Self {
        Self {
            active_jobs: Mutex::new(0),
            available: Condvar::new(),
            maximum: maximum.max(1),
        }
    }

    fn acquire(self: &Arc<Self>) -> InferencePermit {
        let mut active_jobs = self
            .active_jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while *active_jobs >= self.maximum {
            active_jobs = self
                .available
                .wait(active_jobs)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        *active_jobs += 1;
        InferencePermit {
            limiter: Arc::clone(self),
        }
    }
}

struct InferencePermit {
    limiter: Arc<InferenceConcurrencyLimiter>,
}

impl Drop for InferencePermit {
    fn drop(&mut self) {
        let mut active_jobs = self
            .limiter
            .active_jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *active_jobs = active_jobs.saturating_sub(1);
        self.limiter.available.notify_one();
    }
}

/// Contains a worker body's potential panic so a single bad turn (for
/// example a byte-slicing bug hit while scanning arbitrary terminal or
/// transcript text) degrades to a failed result instead of silently
/// dropping the worker's completion event. A dropped event would leave the
/// matching `*_in_flight` guard set forever, permanently disabling that
/// naming feature for the rest of the client's run -- mirrors
/// `search_workers::start`'s containment of the same risk.
fn catch_worker_panic<T>(
    worker_name: &str,
    body: impl FnOnce() -> anyhow::Result<T> + UnwindSafe,
) -> anyhow::Result<T> {
    panic::catch_unwind(body).unwrap_or_else(|panic_payload| {
        Err(anyhow::anyhow!(
            "{worker_name} worker panicked: {}",
            panic_payload_message(&panic_payload)
        ))
    })
}

/// Extracts a human-readable message from a caught panic payload.
/// `std::panic!` payloads are almost always `&str` or `String`; anything
/// else still logs usefully rather than silently swallowing the panic's
/// presence.
fn panic_payload_message(panic_payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = panic_payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    if let Some(message) = panic_payload.downcast_ref::<String>() {
        return message.clone();
    }
    "non-string panic payload".to_string()
}

impl NamingWorkers {
    pub fn new(
        events_tx: Sender<NamingWorkerEvent>,
        inference_settings: InferenceSettings,
    ) -> Self {
        Self {
            events_tx,
            inference_settings,
            project_name_in_flight: false,
            session_title_in_flight: HashSet::new(),
            terminal_title_in_flight: HashSet::new(),
            inference_test_in_flight: false,
            model_discovery_in_flight: false,
            restructure_in_flight: HashSet::new(),
            concurrency_limiter: Arc::new(InferenceConcurrencyLimiter::new(
                MAX_CONCURRENT_INFERENCE_JOBS,
            )),
        }
    }

    /// Spawns the one-shot project-name bootstrap worker, unless one is
    /// already running. A no-op call (e.g. a stored name already loaded
    /// synchronously at startup) is the caller's responsibility to avoid.
    pub fn spawn_project_name_worker(&mut self, cwd: PathBuf) {
        if self.project_name_in_flight {
            return;
        }
        self.project_name_in_flight = true;
        let events_tx = self.events_tx.clone();
        let inference_settings = self.inference_settings.clone();
        let concurrency_limiter = Arc::clone(&self.concurrency_limiter);
        std::thread::spawn(move || {
            // Background inference must never compete with the render loop
            // for CPU -- same convention as `search_workers::start`.
            lower_current_thread(WorkerPriority::BelowNormal);
            let _permit = concurrency_limiter.acquire();
            let result = catch_worker_panic("project name", || {
                crate::project_naming::bootstrap_project_name(&cwd, &inference_settings)
            });
            // `blocking_send` (not the async `send`) since this closure
            // runs on a plain `std::thread`, not a tokio task -- exactly
            // the case that method exists for. It only ever actually
            // blocks if the main loop is unusually far behind, since this
            // channel carries at most one message per worker.
            let _ = events_tx.blocking_send(NamingWorkerEvent::ProjectName(result));
        });
    }

    pub fn project_name_worker_finished(&mut self) {
        self.project_name_in_flight = false;
    }

    /// Spawns a session-title inference worker for `pane_id`, unless one is
    /// already running for it -- see the module docs for what triggers this.
    pub fn spawn_session_title_worker(&mut self, request: SessionTitleWorkerRequest) {
        let SessionTitleWorkerRequest {
            home,
            input,
            title_generation,
            trigger,
        } = request;
        let pane_id = input.pane_id;
        let session_id = input.session_id.clone();
        if !self
            .session_title_in_flight
            .insert((pane_id, session_id.clone()))
        {
            return;
        }
        let events_tx = self.events_tx.clone();
        let inference_settings = self.inference_settings.clone();
        let provider = inference_settings.selected_provider;
        let concurrency_limiter = Arc::clone(&self.concurrency_limiter);
        std::thread::spawn(move || {
            // See `spawn_project_name_worker` on why every naming worker
            // thread lowers its own scheduling priority first.
            lower_current_thread(WorkerPriority::BelowNormal);
            let _permit = concurrency_limiter.acquire();
            let started_at = Instant::now();
            let trace = panic::catch_unwind(AssertUnwindSafe(|| {
                crate::session_naming::infer_pane_title_with_trace(
                    &inference_settings,
                    &home,
                    &input,
                )
            }))
            .unwrap_or_else(|panic_payload| {
                crate::session_naming::SessionTitleInferenceTrace {
                    rendered_prompt: None,
                    raw_response: None,
                    result: Err(anyhow::anyhow!(
                        "session title worker panicked: {}",
                        panic_payload_message(&panic_payload)
                    )),
                }
            });
            let elapsed = started_at.elapsed();
            // See `spawn_project_name_worker`'s matching comment on why
            // `blocking_send` is correct here.
            let _ = events_tx.blocking_send(NamingWorkerEvent::SessionTitle(
                SessionTitleWorkerResult {
                    pane_id,
                    session_id,
                    title_generation,
                    provider,
                    elapsed,
                    rendered_prompt: trace.rendered_prompt,
                    raw_response: trace.raw_response,
                    result: trace.result,
                    trigger,
                },
            ));
        });
    }

    pub fn session_title_worker_finished(&mut self, pane_id: NodeId, session_id: &str) {
        self.session_title_in_flight
            .remove(&(pane_id, session_id.to_string()));
    }

    /// Spawns a background check of the agent CLI's own session transcript
    /// for `request.pane_id`'s most recent user message -- the "OR get it
    /// from the .jsonl history file" fallback/upgrade for the last-prompt
    /// banner. Deliberately no in-flight dedup: each call is one bounded,
    /// idempotent file read (worst case ~2.8s), so an Enter press racing a
    /// still-running prior check just costs a redundant read rather than
    /// risking a dropped update.
    pub fn spawn_last_prompt_transcript_worker(
        &mut self,
        request: LastPromptTranscriptWorkerRequest,
    ) {
        let LastPromptTranscriptWorkerRequest {
            home,
            pane_id,
            project_path,
            agent_class,
            session_id,
        } = request;
        let events_tx = self.events_tx.clone();
        std::thread::spawn(move || {
            // See `spawn_project_name_worker` on why every naming worker
            // thread lowers its own scheduling priority first.
            lower_current_thread(WorkerPriority::BelowNormal);
            std::thread::sleep(LAST_PROMPT_TRANSCRIPT_INITIAL_DELAY);
            let mut last_prompt = None;
            for attempt in 0..LAST_PROMPT_TRANSCRIPT_MAX_ATTEMPTS {
                last_prompt = TranscriptLocator::new(&home, &project_path)
                    .transcript_for_session(&agent_class, &session_id)
                    .and_then(|transcript| {
                        crate::transcript_context::recent_user_prompts(
                            &agent_class,
                            &transcript.path,
                        )
                        .ok()
                    })
                    .and_then(|prompts| prompts.into_iter().next_back());
                if last_prompt.is_some() || attempt + 1 == LAST_PROMPT_TRANSCRIPT_MAX_ATTEMPTS {
                    break;
                }
                std::thread::sleep(LAST_PROMPT_TRANSCRIPT_RETRY_INTERVAL);
            }
            // See `spawn_project_name_worker`'s matching comment on why
            // `blocking_send` is correct here.
            let _ = events_tx.blocking_send(NamingWorkerEvent::LastPromptTranscript(
                LastPromptTranscriptWorkerResult {
                    pane_id,
                    session_id,
                    last_prompt,
                },
            ));
        });
    }

    /// Spawns a terminal-screen title inference worker for `input.pane_id`,
    /// unless one is already running for it -- see `crate::terminal_naming`
    /// and the manual/automatic request paths in `App`.
    pub fn spawn_terminal_title_worker(
        &mut self,
        input: crate::terminal_naming::TerminalTitleInput,
        trigger: TitleTrigger,
    ) {
        let pane_id = input.pane_id;
        if !self.terminal_title_in_flight.insert(pane_id) {
            return;
        }
        let events_tx = self.events_tx.clone();
        let inference_settings = self.inference_settings.clone();
        let concurrency_limiter = Arc::clone(&self.concurrency_limiter);
        std::thread::spawn(move || {
            // See `spawn_project_name_worker` on why every naming worker
            // thread lowers its own scheduling priority first.
            lower_current_thread(WorkerPriority::BelowNormal);
            let _permit = concurrency_limiter.acquire();
            let result = catch_worker_panic("terminal title", || {
                crate::terminal_naming::infer_terminal_title(&inference_settings, &input)
            });
            // See `spawn_project_name_worker`'s matching comment on why
            // `blocking_send` is correct here.
            let _ =
                events_tx.blocking_send(NamingWorkerEvent::TerminalTitle(pane_id, result, trigger));
        });
    }

    pub fn terminal_title_worker_finished(&mut self, pane_id: NodeId) {
        self.terminal_title_in_flight.remove(&pane_id);
    }

    pub fn set_inference_settings(&mut self, settings: InferenceSettings) {
        self.inference_settings = settings;
    }

    pub fn spawn_inference_test_worker(&mut self) {
        if self.inference_test_in_flight {
            return;
        }
        self.inference_test_in_flight = true;
        let events_tx = self.events_tx.clone();
        let settings = self.inference_settings.clone();
        let concurrency_limiter = Arc::clone(&self.concurrency_limiter);
        std::thread::spawn(move || {
            // See `spawn_project_name_worker` on why every naming worker
            // thread lowers its own scheduling priority first.
            lower_current_thread(WorkerPriority::BelowNormal);
            let _permit = concurrency_limiter.acquire();
            let provider = settings.selected_provider;
            let started_at = std::time::Instant::now();
            let result =
                catch_worker_panic("inference test", || crate::inference_test::run(&settings));
            let _ = events_tx.blocking_send(NamingWorkerEvent::InferenceTest {
                provider,
                elapsed: started_at.elapsed(),
                result,
            });
        });
    }

    pub fn inference_test_worker_finished(&mut self) {
        self.inference_test_in_flight = false;
    }

    pub fn spawn_model_discovery_worker(
        &mut self,
        provider: ilium_inference::InferenceProviderKind,
    ) {
        if self.model_discovery_in_flight {
            return;
        }
        self.model_discovery_in_flight = true;
        let events_tx = self.events_tx.clone();
        let mut settings = self.inference_settings.clone();
        settings.selected_provider = provider;
        let concurrency_limiter = Arc::clone(&self.concurrency_limiter);
        std::thread::spawn(move || {
            // See `spawn_project_name_worker` on why every naming worker
            // thread lowers its own scheduling priority first.
            lower_current_thread(WorkerPriority::BelowNormal);
            let _permit = concurrency_limiter.acquire();
            let endpoint = match provider {
                ilium_inference::InferenceProviderKind::KiloGateway => {
                    ilium_inference::kilo_gateway_model_catalog_url()
                }
                ilium_inference::InferenceProviderKind::Ollama => format!(
                    "{}/api/tags",
                    settings.ollama.base_url.trim_end_matches('/')
                ),
                _ => provider.label().to_string(),
            };
            let started_at = std::time::Instant::now();
            let result = catch_worker_panic("model discovery", || {
                ilium_inference::provider_from_settings(&settings)
                    .list_models()
                    .map_err(anyhow::Error::from)
            });
            let _ = events_tx.blocking_send(NamingWorkerEvent::ProviderModels {
                provider,
                endpoint,
                elapsed: started_at.elapsed(),
                result,
            });
        });
    }
    pub fn model_discovery_worker_finished(&mut self) {
        self.model_discovery_in_flight = false;
    }

    /// Spawns the whole-tree restructure worker, unless one is already
    /// running -- `App::structure_loading` is this method's matching
    /// per-`App` guard, kept there (not read from here) since only `App`
    /// decides whether to gather `contexts` at all. Resolves agent
    /// transcripts (`crate::restructure::resolve_content_extracts`) before
    /// calling the LLM, mirroring `spawn_session_title_worker`'s own
    /// disk-I/O-inside-the-closure pattern.
    pub fn spawn_restructure_worker(
        &mut self,
        request: crate::app::PendingRestructureRequest,
        home: PathBuf,
    ) {
        let crate::app::PendingRestructureRequest {
            project_id,
            project_cwd,
            mut contexts,
            protected_split_views,
            current_structure,
            inference_activity_revisions,
            ..
        } = request;
        if !self.restructure_in_flight.insert(project_id) {
            return;
        }
        let events_tx = self.events_tx.clone();
        let inference_settings = self.inference_settings.clone();
        let concurrency_limiter = Arc::clone(&self.concurrency_limiter);
        std::thread::spawn(move || {
            // Lowered before `resolve_content_extracts`'s bulk transcript
            // reads, not just the LLM call -- see `spawn_project_name_worker`.
            lower_current_thread(WorkerPriority::BelowNormal);
            let result = panic::catch_unwind(AssertUnwindSafe(|| {
                crate::restructure::resolve_content_extracts(&mut contexts, &home, &project_cwd);
                let _permit = concurrency_limiter.acquire();
                crate::restructure::infer_restructure_plan_with_protected_splits(
                    &inference_settings,
                    &contexts,
                    &current_structure,
                    &protected_split_views,
                )
            }))
            .unwrap_or_else(|panic_payload| {
                Err(anyhow::anyhow!(
                    "restructure worker panicked: {}",
                    panic_payload_message(&panic_payload)
                ))
            });
            // See `spawn_project_name_worker`'s matching comment on why
            // `blocking_send` is correct here.
            let _ =
                events_tx.blocking_send(NamingWorkerEvent::Restructure(RestructureWorkerResult {
                    project_id,
                    inference_activity_revisions,
                    result,
                }));
        });
    }

    pub fn restructure_worker_finished(&mut self, project_id: NodeId) {
        self.restructure_in_flight.remove(&project_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End-to-end regression for the last-prompt banner's ".jsonl transcript"
    /// fallback: writes a synthetic Claude Code transcript to a temp home
    /// directory (same shape `ilium-agent-session`'s own fixtures use),
    /// spawns the real worker against it, and asserts the worker's result
    /// actually carries the transcript's last user message back out. This is
    /// the durable regression coverage the throwaway
    /// `diagnose_real_claude_last_prompt` PTY test could not provide, since a
    /// nested-under-test `claude` process has transcript saving disabled.
    #[test]
    fn last_prompt_transcript_worker_reads_the_most_recent_user_message() {
        let home = tempfile::tempdir().expect("temp home dir");
        let project_path = std::path::Path::new("/work/ilium-transcript-test");
        let session_id = "33333333-3333-4333-8333-333333333333";
        // Mirrors Claude Code's own project-directory slug (every non-ASCII-
        // alphanumeric character becomes `-`), duplicated here rather than
        // reaching into `ilium-agent-session`'s private `slugify_claude_project_path`.
        let slug: String = project_path
            .to_string_lossy()
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character
                } else {
                    '-'
                }
            })
            .collect();
        let project_dir = home.path().join(".claude").join("projects").join(slug);
        std::fs::create_dir_all(&project_dir).expect("create claude project dir");
        let transcript_path = project_dir.join(format!("{session_id}.jsonl"));
        let lines = [
            serde_json::json!({
                "type": "user",
                "sessionId": session_id,
                "cwd": project_path,
                "message": {"content": "first prompt, superseded"}
            }),
            serde_json::json!({
                "type": "assistant",
                "sessionId": session_id,
                "cwd": project_path,
                "message": {"content": [{"type": "text", "text": "on it"}]}
            }),
            serde_json::json!({
                "type": "user",
                "sessionId": session_id,
                "cwd": project_path,
                "message": {"content": "fix the failing tests"}
            }),
        ]
        .map(|entry| entry.to_string())
        .join("\n");
        std::fs::write(&transcript_path, lines).expect("write synthetic transcript");

        let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(1);
        let mut workers = NamingWorkers::new(events_tx, InferenceSettings::default());
        let pane_id = NodeId(11);
        workers.spawn_last_prompt_transcript_worker(LastPromptTranscriptWorkerRequest {
            home: home.path().to_path_buf(),
            pane_id,
            project_path: project_path.to_path_buf(),
            agent_class: AgentClass::Claude,
            session_id: session_id.to_string(),
        });

        let event = events_rx
            .blocking_recv()
            .expect("worker reports its result");
        let NamingWorkerEvent::LastPromptTranscript(result) = event else {
            panic!("expected a LastPromptTranscript event");
        };
        assert_eq!(result.pane_id, pane_id);
        assert_eq!(result.session_id, session_id);
        assert_eq!(
            result.last_prompt,
            Some("fix the failing tests".to_string()),
            "must pick the most recent user message, not the superseded first one"
        );
    }

    #[test]
    fn inference_concurrency_limiter_never_admits_more_than_its_capacity() {
        let limiter = Arc::new(InferenceConcurrencyLimiter::new(2));
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let mut handles = Vec::new();

        for worker in 0..3 {
            let limiter = Arc::clone(&limiter);
            let release = Arc::clone(&release);
            let entered_tx = entered_tx.clone();
            handles.push(std::thread::spawn(move || {
                let _permit = limiter.acquire();
                entered_tx.send(worker).expect("report admitted worker");
                let (lock, available) = &*release;
                let mut is_released = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                while !*is_released {
                    is_released = available
                        .wait(is_released)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
            }));
        }
        drop(entered_tx);

        entered_rx.recv().expect("first worker admitted");
        entered_rx.recv().expect("second worker admitted");
        assert!(entered_rx.try_recv().is_err());

        let (lock, available) = &*release;
        *lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
        available.notify_all();
        entered_rx
            .recv()
            .expect("queued worker eventually admitted");
        for handle in handles {
            handle.join().expect("worker exits cleanly");
        }
    }
}
