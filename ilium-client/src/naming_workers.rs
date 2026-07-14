//! Background `ilium-kilo-gateway` title-inference workers: project-name
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

use ilium_core::{AgentClass, NodeId};
use ilium_kilo_gateway::KiloGatewayClient;
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
    SessionTitle(NodeId, String, anyhow::Result<DualTitle>, TitleTrigger),
    TerminalTitle(NodeId, anyhow::Result<DualTitle>, TitleTrigger),
}

/// Tracks which naming workers are currently in flight, so a caller never
/// accidentally spawns a second one for the same target while the first is
/// still running.
pub struct NamingWorkers {
    events_tx: Sender<NamingWorkerEvent>,
    project_name_in_flight: bool,
    session_title_in_flight: HashSet<(NodeId, String)>,
    terminal_title_in_flight: HashSet<NodeId>,
}

impl NamingWorkers {
    pub fn new(events_tx: Sender<NamingWorkerEvent>) -> Self {
        Self {
            events_tx,
            project_name_in_flight: false,
            session_title_in_flight: HashSet::new(),
            terminal_title_in_flight: HashSet::new(),
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
        std::thread::spawn(move || {
            let result =
                crate::project_naming::bootstrap_project_name(&cwd, &KiloGatewayClient::default());
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
    pub fn spawn_session_title_worker(
        &mut self,
        pane_id: NodeId,
        home: PathBuf,
        cwd: PathBuf,
        agent_class: AgentClass,
        session_id: String,
        trigger: TitleTrigger,
    ) {
        if !self
            .session_title_in_flight
            .insert((pane_id, session_id.clone()))
        {
            return;
        }
        let events_tx = self.events_tx.clone();
        std::thread::spawn(move || {
            let result = crate::session_naming::infer_pane_title(
                &KiloGatewayClient::default(),
                &home,
                &cwd,
                &agent_class,
                &session_id,
            );
            // See `spawn_project_name_worker`'s matching comment on why
            // `blocking_send` is correct here.
            let _ = events_tx.blocking_send(NamingWorkerEvent::SessionTitle(
                pane_id, session_id, result, trigger,
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
        std::thread::spawn(move || {
            let result = crate::terminal_naming::infer_terminal_title(
                &KiloGatewayClient::default(),
                &screen_text,
            );
            // See `spawn_project_name_worker`'s matching comment on why
            // `blocking_send` is correct here.
            let _ =
                events_tx.blocking_send(NamingWorkerEvent::TerminalTitle(pane_id, result, trigger));
        });
    }

    pub fn terminal_title_worker_finished(&mut self, pane_id: NodeId) {
        self.terminal_title_in_flight.remove(&pane_id);
    }
}
