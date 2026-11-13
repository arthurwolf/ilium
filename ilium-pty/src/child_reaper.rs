//! Prepaid child custody established before native process creation. A single
//! startup handoff joins the existing reaper with either its child or an
//! abandoned spawn; cancellation cannot discard an in-flight native spawn.

use super::{retain_child_exit, PtyChildExit};
use crate::admission::PtyWorkerReservation;
use ilium_platform::owned_worker::{OwnedWorker, StopToken, WorkerKind};
use portable_pty::Child;
use std::io;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const CHILD_REAP_POLL_INTERVAL: Duration = Duration::from_millis(50);
const CHILD_REAP_IDLE_POLL_INTERVAL: Duration = Duration::from_secs(1);
const CHILD_KILL_RETRY_INTERVAL: Duration = Duration::from_secs(1);
type SharedChild = Arc<Mutex<Box<dyn Child + Send + Sync>>>;

struct ReaperChild {
    control: SharedChild,
    process_id: Option<u32>,
}

#[derive(Default)]
enum Startup {
    #[default]
    Pending,
    Ready(ReaperChild),
    Abandoned,
    Taken,
}

#[derive(Default)]
struct WakeState {
    is_woken: bool,
    startup: Startup,
}

#[derive(Default)]
struct ReaperWake {
    state: Mutex<WakeState>,
    changed: Condvar,
}

impl ReaperWake {
    fn wake(&self) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_woken = true;
        self.changed.notify_all();
    }

    fn wait_for_child(&self) -> Option<ReaperChild> {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let mut state = self
            .changed
            .wait_while(state, |state| matches!(state.startup, Startup::Pending))
            .unwrap_or_else(|error| error.into_inner());
        // Stop is deliberately NOT an exit condition here: a native spawn may
        // still be running. Only its linear producer can resolve this handoff.
        match std::mem::replace(&mut state.startup, Startup::Taken) {
            Startup::Ready(child) => Some(child),
            Startup::Abandoned => None,
            Startup::Pending | Startup::Taken => None,
        }
    }

    fn sleep(&self, timeout: Duration) {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let (mut state, _) = self
            .changed
            .wait_timeout_while(state, timeout, |state| !state.is_woken)
            .unwrap_or_else(|error| error.into_inner());
        state.is_woken = false;
    }
}

/// The only producer of a child handoff. Every pre-publication error or unwind
/// resolves Pending to Abandoned and wakes the already supervised worker.
struct PendingReaperStart {
    wake: Arc<ReaperWake>,
    published: bool,
}

impl PendingReaperStart {
    fn publish(mut self, child: ReaperChild) {
        self.published = true;
        self.wake
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .startup = Startup::Ready(child);
        self.wake.changed.notify_all();
    }
}

impl Drop for PendingReaperStart {
    fn drop(&mut self) {
        if !self.published {
            self.wake
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .startup = Startup::Abandoned;
            self.wake.changed.notify_all();
        }
    }
}

/// Successful construction proves the existing reserved native worker was
/// started before the caller enters portable-pty's process creation callback.
/// The fields drop in order: abandon startup, then cancel the physical worker.
pub(super) struct PendingChildReaper {
    start: PendingReaperStart,
    worker: OwnedWorker,
}

impl PendingChildReaper {
    pub(super) fn start(
        reservation: PtyWorkerReservation,
        receipt: Arc<Mutex<Option<PtyChildExit>>>,
    ) -> io::Result<Self> {
        let wake = Arc::new(ReaperWake::default());
        let start = PendingReaperStart {
            wake: Arc::clone(&wake),
            published: false,
        };
        let cancellation_wake = Arc::clone(&wake);
        let worker = reservation.spawn(
            "ilium-pty-child-reaper",
            WorkerKind::Cooperative,
            StopToken::default(),
            move || cancellation_wake.wake(),
            move |stop| {
                if let Some(child) = wake.wait_for_child() {
                    reap(child, &receipt, &wake, &stop);
                }
            },
        )?;
        Ok(Self { start, worker })
    }

    /// Publish while the caller holds the child mutex through birth-identity
    /// capture. Custody exists immediately, but no wait/reap can recycle the
    /// process ID before that capture finishes (including during unwind).
    pub(super) fn attach(self, control: SharedChild, process_id: Option<u32>) -> OwnedWorker {
        self.start.publish(ReaperChild {
            control,
            process_id,
        });
        self.worker
    }
}

fn reap(
    child: ReaperChild,
    receipt: &Mutex<Option<PtyChildExit>>,
    wake: &ReaperWake,
    stop: &StopToken,
) {
    let ReaperChild {
        control,
        process_id,
    } = child;
    let mut termination_requested = false;
    let mut next_kill_attempt = Instant::now();
    let mut kill_failed = false;
    let mut query_failed = false;
    let mut poll_interval = CHILD_REAP_POLL_INTERVAL;
    loop {
        let exited = {
            let mut child = control.lock().unwrap_or_else(|error| error.into_inner());
            let status = child.try_wait();
            match status {
                Ok(Some(status)) => {
                    retain_child_exit(receipt, process_id, status);
                    true
                }
                status => {
                    match status {
                        Err(error) if !query_failed => {
                            query_failed = true;
                            tracing::warn!(?process_id, %error,
                                "PTY child status query failed; supervised reaper retains custody and retries");
                        }
                        Ok(None) if query_failed => {
                            query_failed = false;
                            tracing::info!(?process_id, "PTY child status query recovered");
                        }
                        _ => {}
                    }
                    if stop.is_stopped()
                        && !termination_requested
                        && Instant::now() >= next_kill_attempt
                    {
                        match child.kill() {
                            Ok(()) => {
                                termination_requested = true;
                                if kill_failed {
                                    tracing::info!(
                                        ?process_id,
                                        "PTY child termination request recovered; waiting for reap"
                                    );
                                }
                            }
                            Err(error) => {
                                next_kill_attempt = Instant::now() + CHILD_KILL_RETRY_INTERVAL;
                                if !kill_failed {
                                    kill_failed = true;
                                    tracing::warn!(?process_id, %error,
                                        "PTY child termination failed; supervised reaper retains custody and retries once per second");
                                }
                            }
                        }
                    }
                    false
                }
            }
        };
        if exited {
            break;
        }
        if stop.is_stopped() {
            poll_interval = CHILD_REAP_POLL_INTERVAL;
        }
        wake.sleep(poll_interval);
        poll_interval = poll_interval
            .saturating_mul(2)
            .min(CHILD_REAP_IDLE_POLL_INTERVAL);
    }
}

#[cfg(all(test, unix))]
#[path = "child_reaper_tests.rs"]
mod tests;
