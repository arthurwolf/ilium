//! One original bank, one blocking observer, and acknowledged result custody.
use super::ClientExecution;
use ilium_execution::{JoinReport, JoinUseError, ShutdownMode};
use std::{
    io,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

struct ShutdownState {
    owner: Option<ClientExecution>,
    observation: Option<Result<JoinReport, JoinUseError>>,
}
impl ShutdownState {
    fn observe_background(&mut self, deadline: Instant) -> bool {
        let Some(owner) = &mut self.owner else {
            return true;
        };
        owner.execution.request_shutdown(ShutdownMode::Cancel);
        let observed = owner.execution.join_until_background(deadline);
        let complete = matches!(&observed, Ok(report) if report.shutdown_complete);
        self.observation = Some(observed);
        if complete {
            // Release initialized tenants only after the same bank joined and
            // every admitted/retiring original left its lifecycle inventory.
            self.owner = None;
        }
        complete
    }
}

struct BackgroundOwner {
    state: Arc<Mutex<ShutdownState>>,
    transferred: bool,
}
impl Drop for BackgroundOwner {
    fn drop(&mut self) {
        if self.transferred {
            return;
        }
        // Cancellation relinquishes observation, not the original bank. This
        // is the same existing blocking task, never a replacement bank/thread.
        // A blocked callback keeps its original quota charged until it exits.
        loop {
            let complete = self
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .observe_background(Instant::now() + Duration::from_secs(5));
            if complete {
                break;
            }
            // Zero-worker banks can still own external or recovery originals;
            // keep custody without spinning while those owners are unsettled.
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Deadline/refusal/panic retains the actual bank in the public I/O error.
pub struct ClientExecutionShutdownError {
    state: Arc<Mutex<ShutdownState>>,
    source: io::Error,
}
impl ClientExecutionShutdownError {
    pub fn retains_execution(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .owner
            .is_some()
    }
    pub fn with_observation<R>(
        &self,
        inspect: impl FnOnce(Option<&Result<JoinReport, JoinUseError>>) -> R,
    ) -> R {
        // Inspection runs under the custody lock. The callback must not
        // reenter this error's accessors or wait for its background observer.
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        inspect(state.observation.as_ref())
    }
    /// Explicit exceptional retry; call only from a background/bootstrap owner.
    pub fn observe_cleanup_background(&self, deadline: Instant) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .observe_background(deadline)
    }
}
impl std::fmt::Debug for ClientExecutionShutdownError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ClientExecutionShutdownError")
            .field("source", &self.source)
            .field("actual_bank_retained", &self.retains_execution())
            .finish()
    }
}
impl std::fmt::Display for ClientExecutionShutdownError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.source.fmt(formatter)
    }
}
impl std::error::Error for ClientExecutionShutdownError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

pub(super) async fn shutdown(owner: ClientExecution, deadline: Instant) -> io::Result<()> {
    shutdown_with(owner, deadline, || {}).await
}

pub(super) async fn shutdown_with(
    owner: ClientExecution,
    deadline: Instant,
    before_observation: impl FnOnce() + Send + 'static,
) -> io::Result<()> {
    let state = Arc::new(Mutex::new(ShutdownState {
        owner: Some(owner),
        observation: None,
    }));
    let background = Arc::clone(&state);
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let (transfer_tx, transfer_rx) = std::sync::mpsc::sync_channel(1);
    let observer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tokio::task::spawn_blocking(move || {
            let mut custody = BackgroundOwner {
                state: background,
                transferred: false,
            };
            before_observation();
            custody
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .observe_background(deadline);
            let announced = ready_tx.send(()).is_ok();
            custody.transferred = announced && transfer_rx.recv().is_ok();
        })
    }));
    let observer = match observer {
        Ok(observer) => observer,
        Err(panic) => {
            // Observer construction can panic before any background guard
            // exists (missing runtime or OS thread refusal). Keep the same
            // initialized bank in the returned error instead of unwinding it.
            let message = panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .unwrap_or("non-text panic while spawning execution shutdown observer");
            return Err(io::Error::other(ClientExecutionShutdownError {
                state,
                source: io::Error::other(message.to_owned()),
            }));
        }
    };
    let source = if ready_rx.await.is_err() {
        // Runtime refusal before enqueue retains the original through our Arc.
        // A panicking running task's background guard performs physical cleanup.
        match observer.await {
            Err(error) => Some(io::Error::other(error)),
            Ok(()) => Some(io::Error::other(
                "execution shutdown observer lost its announcement",
            )),
        }
    } else {
        let original = state.lock().unwrap_or_else(|error| error.into_inner());
        match original.observation.as_ref() {
            Some(Ok(report)) if report.shutdown_complete => None,
            Some(Ok(report)) if report.remaining_workers != 0 => Some(io::Error::other(format!(
                "execution shutdown deadline: {} workers still owned",
                report.remaining_workers
            ))),
            Some(Ok(report)) => {
                let cpu = &report.health.lanes[0];
                let lanes = report
                    .health
                    .lanes
                    .iter()
                    .enumerate()
                    .map(|(index, lane)| {
                        format!(
                            "lane {index}: waiting_or_reserved {}, enqueued {}, service_claims {}, running {}",
                            lane.waiting_or_reserved,
                            lane.enqueued,
                            lane.retained_service_claims,
                            lane.running
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                Some(io::Error::other(format!(
                    "execution shutdown incomplete: {} retirement originals live, {} in recovery custody; admitted work remains ({lanes})",
                    cpu.retirement_live, cpu.retirement_recovery_pending
                )))
            }
            Some(Err(error)) => Some(io::Error::other(format!("execution shutdown: {error:?}"))),
            None => Some(io::Error::other(
                "execution shutdown lacks a native observation",
            )),
        }
    };
    // No await between accepting the actual result owner and acknowledging
    // transfer. Canceling any earlier await leaves cleanup with BackgroundOwner.
    let result = match source {
        Some(source) => Err(io::Error::other(ClientExecutionShutdownError {
            state,
            source,
        })),
        None => Ok(()),
    };
    let _ = transfer_tx.send(());
    result
}
