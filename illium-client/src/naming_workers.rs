//! Background `illium-kilo-gateway` title-inference workers: project-name
//! bootstrap (once per session) and per-pane session-title inference
//! (`session_naming::infer_pane_title`), each run on a dedicated
//! `std::thread` -- these make a blocking HTTP call, so they must never run
//! on the tokio event loop -- and bridged back into it the same way
//! crossterm input is (see `crate::run`): the worker thread holds a
//! `tokio::sync::mpsc::UnboundedSender` directly and calls its ordinary,
//! non-async `send` from off the runtime, so no second bridging hop is
//! needed.
//!
//! Session-title inference is wired here as reusable, tested machinery
//! (`spawn_session_title_worker`) but nothing in this crate calls it yet:
//! it needs an agent pane's session ID, and `illium_ipc::PaneStatus` (via
//! `ServerEvent::PaneStatusChanged`) only carries `AgentClass` +
//! `AgentActivity`, not a session ID -- that's server-side information
//! (the server walks the real process tree) not yet exposed over the wire.
//! Wiring the trigger is future work once the protocol carries it; see
//! `crate::session_transcript`'s module docs for the matching gap on the
//! transcript-lookup side.

use std::collections::HashSet;
use std::path::PathBuf;

use illium_core::{AgentClass, NodeId};
use illium_kilo_gateway::KiloGatewayClient;
use tokio::sync::mpsc::UnboundedSender;

use crate::project_naming::ProjectNameBootstrap;

/// A finished background naming result, forwarded into the main event loop.
pub enum NamingWorkerEvent {
    ProjectName(anyhow::Result<ProjectNameBootstrap>),
    SessionTitle(NodeId, anyhow::Result<String>),
}

/// Tracks which naming workers are currently in flight, so a caller never
/// accidentally spawns a second one for the same target while the first is
/// still running.
pub struct NamingWorkers {
    events_tx: UnboundedSender<NamingWorkerEvent>,
    project_name_in_flight: bool,
    session_title_in_flight: HashSet<NodeId>,
}

impl NamingWorkers {
    pub fn new(events_tx: UnboundedSender<NamingWorkerEvent>) -> Self {
        Self {
            events_tx,
            project_name_in_flight: false,
            session_title_in_flight: HashSet::new(),
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
            let _ = events_tx.send(NamingWorkerEvent::ProjectName(result));
        });
    }

    pub fn project_name_worker_finished(&mut self) {
        self.project_name_in_flight = false;
    }

    /// Spawns a session-title inference worker for `pane_id`, unless one is
    /// already running for it. See the module docs for why nothing in this
    /// crate calls this yet.
    #[allow(dead_code)]
    pub fn spawn_session_title_worker(
        &mut self,
        pane_id: NodeId,
        home: PathBuf,
        cwd: PathBuf,
        agent_class: AgentClass,
        session_id: String,
    ) {
        if !self.session_title_in_flight.insert(pane_id) {
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
            let _ = events_tx.send(NamingWorkerEvent::SessionTitle(pane_id, result));
        });
    }

    pub fn session_title_worker_finished(&mut self, pane_id: NodeId) {
        self.session_title_in_flight.remove(&pane_id);
    }
}
