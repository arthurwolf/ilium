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
//! Session-title inference (`spawn_session_title_worker`) is triggered by
//! `crate::title_inference::pane_ready_for_inference`, called from
//! `crate::run`'s event loop right after `render_cache::apply` -- see that
//! module's docs for the triggers (a pane's session ID/status becoming
//! usable, or a later `Done` transition retrying a still-untitled pane) and for
//! why the decision itself lives in a separate, pure, unit-testable
//! function rather than inline here.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use ilium_core::{AgentClass, NodeId};
use ilium_inference::InferenceSettings;
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
    SessionTitle(NodeId, String, u64, anyhow::Result<DualTitle>, TitleTrigger),
    TerminalTitle(NodeId, anyhow::Result<DualTitle>, TitleTrigger),
    InferenceTest {
        provider: ilium_inference::InferenceProviderKind,
        elapsed: Duration,
        result: anyhow::Result<crate::inference_test::InferenceTestResult>,
    },
    OllamaModels {
        endpoint: String,
        elapsed: Duration,
        result: anyhow::Result<Vec<String>>,
    },
    Restructure(anyhow::Result<ilium_core::RestructurePlan>),
}

/// All immutable inputs captured when a session-title worker starts. Keeping
/// the session ID and title generation together makes the stale-result
/// contract explicit at the thread boundary instead of relying on callers to
/// preserve their ordering across a long parameter list.
pub struct SessionTitleWorkerRequest {
    pub pane_id: NodeId,
    pub home: PathBuf,
    pub cwd: PathBuf,
    pub agent_class: AgentClass,
    pub session_id: String,
    pub title_generation: u64,
    pub trigger: TitleTrigger,
}

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
    ollama_models_in_flight: bool,
    restructure_in_flight: bool,
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
            ollama_models_in_flight: false,
            restructure_in_flight: false,
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
        std::thread::spawn(move || {
            let result = crate::project_naming::bootstrap_project_name(&cwd, &inference_settings);
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
            pane_id,
            home,
            cwd,
            agent_class,
            session_id,
            title_generation,
            trigger,
        } = request;
        if !self
            .session_title_in_flight
            .insert((pane_id, session_id.clone()))
        {
            return;
        }
        let events_tx = self.events_tx.clone();
        let inference_settings = self.inference_settings.clone();
        std::thread::spawn(move || {
            let result = crate::session_naming::infer_pane_title(
                &inference_settings,
                &home,
                &cwd,
                &agent_class,
                &session_id,
            );
            // See `spawn_project_name_worker`'s matching comment on why
            // `blocking_send` is correct here.
            let _ = events_tx.blocking_send(NamingWorkerEvent::SessionTitle(
                pane_id,
                session_id,
                title_generation,
                result,
                trigger,
            ));
        });
    }

    pub fn session_title_worker_finished(&mut self, pane_id: NodeId, session_id: &str) {
        self.session_title_in_flight
            .remove(&(pane_id, session_id.to_string()));
    }

    /// Spawns a terminal-screen title inference worker for `pane_id`,
    /// unless one is already running for it -- see `crate::terminal_naming`
    /// and `App::maybe_trigger_terminal_retitle`/`App::action_request_retitle`
    /// for what triggers this.
    pub fn spawn_terminal_title_worker(
        &mut self,
        pane_id: NodeId,
        screen_text: String,
        trigger: TitleTrigger,
    ) {
        if !self.terminal_title_in_flight.insert(pane_id) {
            return;
        }
        let events_tx = self.events_tx.clone();
        let inference_settings = self.inference_settings.clone();
        std::thread::spawn(move || {
            let result =
                crate::terminal_naming::infer_terminal_title(&inference_settings, &screen_text);
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
        std::thread::spawn(move || {
            let provider = settings.selected_provider;
            let started_at = std::time::Instant::now();
            let result = crate::inference_test::run(&settings);
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

    pub fn spawn_ollama_models_worker(&mut self) {
        if self.ollama_models_in_flight {
            return;
        }
        self.ollama_models_in_flight = true;
        let events_tx = self.events_tx.clone();
        let settings = self.inference_settings.clone();
        std::thread::spawn(move || {
            let endpoint = settings.ollama.base_url.clone();
            let started_at = std::time::Instant::now();
            let result = ilium_inference::provider_from_settings(&settings)
                .list_models()
                .map_err(anyhow::Error::from);
            let _ = events_tx.blocking_send(NamingWorkerEvent::OllamaModels {
                endpoint,
                elapsed: started_at.elapsed(),
                result,
            });
        });
    }
    pub fn ollama_models_worker_finished(&mut self) {
        self.ollama_models_in_flight = false;
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
        mut contexts: Vec<crate::restructure::LeafContext>,
        home: PathBuf,
        cwd: PathBuf,
    ) {
        if self.restructure_in_flight {
            return;
        }
        self.restructure_in_flight = true;
        let events_tx = self.events_tx.clone();
        let inference_settings = self.inference_settings.clone();
        std::thread::spawn(move || {
            crate::restructure::resolve_content_extracts(&mut contexts, &home, &cwd);
            let result = crate::restructure::infer_restructure_plan(&inference_settings, &contexts);
            // See `spawn_project_name_worker`'s matching comment on why
            // `blocking_send` is correct here.
            let _ = events_tx.blocking_send(NamingWorkerEvent::Restructure(result));
        });
    }

    pub fn restructure_worker_finished(&mut self) {
        self.restructure_in_flight = false;
    }
}
