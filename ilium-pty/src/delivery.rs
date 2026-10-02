//! Completion vocabulary. Acceptance, completed delivery, and cancellation are
//! deliberately different events. No failure authorizes automatic input replay.

use ilium_platform::owned_worker::StopToken;
use ilium_platform::pty_io::{IoFailure, WriteFailure, WriteFailureKind};
use std::fmt;
use std::sync::{Arc, Condvar, Mutex};
use tokio::sync::watch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationKind {
    Input,
    Mouse,
    Resize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub operation_id: u64,
    pub kind: OperationKind,
    /// Accepted by the PTY writer, not an assertion that the child consumed it.
    pub bytes_written: usize,
    pub size: Option<(u16, u16)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownReason {
    Requested,
    Eof,
    ReaderFailed,
    WriterFailed,
    ParserUnavailable,
    GeometryLost,
    OwnerPanicked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryFailure {
    Overloaded {
        outstanding_commands: usize,
        outstanding_bytes: usize,
    },
    TooLarge {
        requested: usize,
        maximum: usize,
    },
    Timeout,
    Cancelled,
    /// A known nonzero prefix (possibly the full payload with failed flush).
    PartialWrite {
        requested: usize,
        written: usize,
        cause: WriteFailureKind,
    },
    /// The pump/owner has not proved the final outcome. Never retry this input.
    UnconfirmedWrite {
        requested: usize,
        definitely_written: usize,
        possibly_written: usize,
        cause: WriteFailureKind,
    },
    Io(IoFailure),
    Shutdown(ShutdownReason),
    Resize {
        requested: (u16, u16),
        previous: (u16, u16),
        restored: bool,
        message: String,
    },
    ParserBusy,
    IdentityExhausted,
    OwnerLost,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryError {
    /// A command's accepted queue identity, or None for rejection before
    /// admission and for internal output/reply diagnostics.
    pub operation_id: Option<u64>,
    pub failure: DeliveryFailure,
}

impl DeliveryError {
    pub(crate) fn new(id: Option<u64>, failure: DeliveryFailure) -> Self {
        Self {
            operation_id: id,
            failure,
        }
    }

    pub(crate) fn from_write(id: Option<u64>, requested: usize, error: WriteFailure) -> Self {
        let failure = if !error.settled {
            DeliveryFailure::UnconfirmedWrite {
                requested,
                definitely_written: error.definitely_written,
                possibly_written: error.possibly_written,
                cause: error.kind,
            }
        } else if error.definitely_written > 0 {
            DeliveryFailure::PartialWrite {
                requested,
                written: error.definitely_written,
                cause: error.kind,
            }
        } else {
            match error.kind {
                WriteFailureKind::Timeout => DeliveryFailure::Timeout,
                WriteFailureKind::Cancelled => DeliveryFailure::Cancelled,
                WriteFailureKind::Io(error) => DeliveryFailure::Io(error),
                WriteFailureKind::WorkerStopped => {
                    DeliveryFailure::Shutdown(ShutdownReason::WriterFailed)
                }
            }
        };
        Self::new(id, failure)
    }

    /// Evidence only, NOT a replay policy. Cancelled/expired/rejected requests
    /// may no longer be semantically applicable even when no bytes were sent.
    pub fn proves_zero_delivery(&self) -> bool {
        matches!(
            self.failure,
            DeliveryFailure::Overloaded { .. }
                | DeliveryFailure::TooLarge { .. }
                | DeliveryFailure::Timeout
                | DeliveryFailure::Cancelled
                | DeliveryFailure::Shutdown(_)
                | DeliveryFailure::ParserBusy
                | DeliveryFailure::IdentityExhausted
        )
    }
}

impl fmt::Display for DeliveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PTY operation {:?}: {:?}",
            self.operation_id, self.failure
        )
    }
}
impl std::error::Error for DeliveryError {}

type ResultValue = Result<Delivery, DeliveryError>;

pub(crate) struct Completion {
    result: Mutex<Option<ResultValue>>,
    ready: Condvar,
    changed: watch::Sender<bool>,
}

impl Completion {
    pub(crate) fn new() -> Arc<Self> {
        let (changed, _) = watch::channel(false);
        Arc::new(Self {
            result: Mutex::new(None),
            ready: Condvar::new(),
            changed,
        })
    }

    pub(crate) fn finish(&self, result: ResultValue) {
        let mut slot = self.result.lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_none() {
            *slot = Some(result);
            self.changed.send_replace(true);
            self.ready.notify_all();
        }
    }

    fn result(&self) -> Option<ResultValue> {
        self.result
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    async fn wait(&self) -> ResultValue {
        let mut changed = self.changed.subscribe();
        loop {
            if let Some(result) = self.result() {
                return result;
            }
            // This Completion owns the sender, so it cannot disappear here.
            let _ = changed.changed().await;
        }
    }

    fn wait_blocking(&self) -> ResultValue {
        let mut result = self.result.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(value) = result.as_ref() {
                return value.clone();
            }
            result = self.ready.wait(result).unwrap_or_else(|e| e.into_inner());
        }
    }
}

/// A retained observer exposes the owner's final disposition even after the
/// submitting future was aborted. An UnconfirmedWrite remains uncertain; this
/// observer does not claim later native completion. Dropping it has no effect.
#[derive(Clone)]
pub struct DeliveryObserver {
    completion: Arc<Completion>,
}

impl DeliveryObserver {
    pub fn result(&self) -> Option<ResultValue> {
        self.completion.result()
    }
    pub async fn wait(&self) -> ResultValue {
        self.completion.wait().await
    }
}

/// Dropping a receipt requests cancellation. It does not withdraw already
/// written bytes, acknowledge delivery, or make replay safe. The owner always
/// reports its disposition; observers retain that result across cancellation.
/// A synchronous native resize that never returns is an explicit unsupported
/// hard-deadline case: cancelling its receipt cannot forcibly unwind the OS call.
#[must_use = "admission is not delivery; await this receipt"]
pub struct DeliveryReceipt {
    pub operation_id: u64,
    completion: Arc<Completion>,
    stop: StopToken,
    wake: Arc<dyn Fn() + Send + Sync>,
    cancel_on_drop: bool,
}

impl DeliveryReceipt {
    pub(crate) fn new(
        id: u64,
        completion: Arc<Completion>,
        stop: StopToken,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        Self {
            operation_id: id,
            completion,
            stop,
            wake: Arc::new(wake),
            cancel_on_drop: true,
        }
    }
    pub fn observer(&self) -> DeliveryObserver {
        DeliveryObserver {
            completion: Arc::clone(&self.completion),
        }
    }
    pub fn cancel(&self) {
        self.stop.stop();
        (self.wake)();
    }
    pub async fn wait(mut self) -> ResultValue {
        let result = self.completion.wait().await;
        self.cancel_on_drop = false;
        result
    }
    pub async fn cancel_and_wait(self) -> ResultValue {
        self.cancel();
        self.wait().await
    }
    /// For compatibility with synchronous PtySession::write/resize, never an
    /// async server call made while holding shared tree/pane registry locks.
    pub fn wait_blocking(mut self) -> ResultValue {
        let result = self.completion.wait_blocking();
        self.cancel_on_drop = false;
        result
    }
}

impl Drop for DeliveryReceipt {
    fn drop(&mut self) {
        if self.cancel_on_drop {
            self.cancel();
        }
    }
}

impl fmt::Debug for DeliveryReceipt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeliveryReceipt")
            .field("operation_id", &self.operation_id)
            .field("completed", &self.completion.result().is_some())
            .finish_non_exhaustive()
    }
}
