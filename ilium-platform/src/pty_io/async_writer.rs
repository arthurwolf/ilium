//! One bounded transport-only write pump. The PTY state owner starts a write,
//! then keeps processing eligible output while this worker advances bytes.
//! The pump never owns a parser, geometry control, child, or server state.

use super::{PtyWriter, WriteFailure, WriteFailureKind, WriteSuccess};
use crate::owned_worker::{spawn_owned, OwnedWorker, StopToken, WorkerKind, WorkerTicket};
use std::io;
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CHECK_INTERVAL: Duration = Duration::from_millis(10);
const CANCELLATION_GRACE: Duration = Duration::from_millis(100);

struct WriteJob {
    bytes: Arc<[u8]>,
    deadline: Instant,
    stop: StopToken,
    complete: SyncSender<Result<WriteSuccess, WriteFailure>>,
}

struct ActiveWrite {
    response: Receiver<Result<WriteSuccess, WriteFailure>>,
    requested: usize,
    deadline: Instant,
    stop: StopToken,
    cancellation_started: Option<Instant>,
    cancellation_reason: Option<WriteFailureKind>,
}

pub enum WriteProgress {
    Pending,
    Finished(Result<WriteSuccess, WriteFailure>),
}

/// At most one owned write is in flight. A timeout never drops the worker's
/// JoinHandle; its ticket stays with the bounded supervisor until exit.
pub struct AsyncWriter {
    requests: SyncSender<WriteJob>,
    worker: OwnedWorker,
    native_ticket: Option<WorkerTicket>,
    active: Option<ActiveWrite>,
    reusable: bool,
    // Quarantine retains the native writer until the state owner is dropped.
    _retained_writer: Arc<Mutex<Option<Box<dyn PtyWriter>>>>,
}

impl AsyncWriter {
    /// If worker creation fails, invoke child cleanup while the native writer
    /// is still retained in `native_slot`. The master/control owner has not
    /// been dropped yet. Once spawned, only the transport thread takes it.
    pub fn spawn(
        native: Box<dyn PtyWriter>,
        stop: StopToken,
        on_setup_failure: impl FnOnce(),
    ) -> io::Result<Self> {
        let native_ticket = native.worker_ticket();
        let native_slot = Arc::new(Mutex::new(Some(native)));
        let worker_slot = Arc::clone(&native_slot);
        let (requests, incoming) = mpsc::sync_channel::<WriteJob>(1);
        let worker_result = spawn_owned(
            "ilium-pty-write-dispatch",
            WorkerKind::Cooperative,
            stop,
            || {},
            move |worker_stop| {
                let mut native = worker_slot
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take()
                    .expect("native writer has one transport owner");
                while !worker_stop.is_stopped() {
                    let job = match incoming.recv_timeout(CHECK_INTERVAL) {
                        Ok(job) => job,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    let result = native.write_until(job.bytes, job.deadline, job.stop);
                    let _ = job.complete.try_send(result);
                }
                // Cancellation retires transport work, not the PTY lifetime.
                // In particular a ConPTY writer drop can signal EOF to a child.
                *worker_slot
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) = Some(native);
            },
        );
        let worker = match worker_result {
            Ok(worker) => worker,
            Err(error) => {
                on_setup_failure();
                drop(native_slot);
                return Err(error);
            }
        };
        Ok(Self {
            requests,
            worker,
            native_ticket,
            active: None,
            reusable: true,
            _retained_writer: native_slot,
        })
    }

    pub fn tickets(&self) -> Vec<WorkerTicket> {
        let mut tickets = vec![self.worker.ticket()];
        tickets.extend(self.native_ticket.clone());
        tickets
    }

    pub fn begin(
        &mut self,
        bytes: Arc<[u8]>,
        deadline: Instant,
        stop: StopToken,
    ) -> Result<(), WriteFailure> {
        if !self.reusable || self.active.is_some() {
            return Err(WriteFailure::exact(
                WriteFailureKind::WorkerStopped,
                0,
                false,
            ));
        }
        if stop.is_stopped() {
            return Err(WriteFailure::exact(WriteFailureKind::Cancelled, 0, true));
        }
        if Instant::now() >= deadline {
            return Err(WriteFailure::exact(WriteFailureKind::Timeout, 0, true));
        }
        let requested = bytes.len();
        let (complete, response) = mpsc::sync_channel(1);
        let job = WriteJob {
            bytes,
            deadline,
            stop: stop.clone(),
            complete,
        };
        match self.requests.try_send(job) {
            Ok(()) => {
                self.active = Some(ActiveWrite {
                    response,
                    requested,
                    deadline,
                    stop,
                    cancellation_started: None,
                    cancellation_reason: None,
                });
                Ok(())
            }
            Err(TrySendError::Disconnected(_)) | Err(TrySendError::Full(_)) => {
                self.reusable = false;
                Err(WriteFailure::exact(
                    WriteFailureKind::WorkerStopped,
                    0,
                    false,
                ))
            }
        }
    }

    /// Never blocks on a native write. It observes a completion or asks the
    /// owned worker to cancel, then gives cancellation 100 ms to settle.
    /// An unsettled return is conservative: 0..requested bytes may exist.
    pub fn poll(&mut self) -> WriteProgress {
        let Some(active) = self.active.as_mut() else {
            return WriteProgress::Finished(Err(WriteFailure::exact(
                WriteFailureKind::WorkerStopped,
                0,
                false,
            )));
        };
        match active.response.try_recv() {
            Ok(mut result) => {
                // Our deadline requests cancellation through the same token
                // as an external abort. Preserve the initiating cause without
                // changing the transport's exact prefix/uncertainty evidence.
                if active.cancellation_reason == Some(WriteFailureKind::Timeout) {
                    if let Err(error) = &mut result {
                        if error.kind == WriteFailureKind::Cancelled {
                            error.kind = WriteFailureKind::Timeout;
                        }
                    }
                }
                self.reusable &= result
                    .as_ref()
                    .map_or_else(|error| error.reusable, |success| success.reusable);
                self.active = None;
                return WriteProgress::Finished(result);
            }
            Err(TryRecvError::Disconnected) => {
                active.cancellation_reason = Some(WriteFailureKind::WorkerStopped);
            }
            Err(TryRecvError::Empty) => {}
        }
        let now = Instant::now();
        if active.cancellation_started.is_none() {
            let reason = active.cancellation_reason.clone().or_else(|| {
                if active.stop.is_stopped() {
                    Some(WriteFailureKind::Cancelled)
                } else if now >= active.deadline {
                    Some(WriteFailureKind::Timeout)
                } else {
                    None
                }
            });
            if let Some(reason) = reason {
                active.stop.stop();
                active.cancellation_started = Some(now);
                active.cancellation_reason = Some(reason);
            }
        }
        if active
            .cancellation_started
            .is_some_and(|start| now.duration_since(start) >= CANCELLATION_GRACE)
        {
            self.worker.ticket().cancel();
            let failure = WriteFailure {
                kind: active
                    .cancellation_reason
                    .clone()
                    .unwrap_or(WriteFailureKind::WorkerStopped),
                definitely_written: 0,
                possibly_written: active.requested,
                settled: false,
                reusable: false,
            };
            self.active = None;
            self.reusable = false;
            return WriteProgress::Finished(Err(failure));
        }
        WriteProgress::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct CancelledWriter {
        dropped: Arc<AtomicBool>,
    }
    impl PtyWriter for CancelledWriter {
        fn write_until(
            &mut self,
            _: Arc<[u8]>,
            _: Instant,
            stop: StopToken,
        ) -> Result<WriteSuccess, WriteFailure> {
            while !stop.is_stopped() {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(WriteFailure::exact(WriteFailureKind::Cancelled, 0, true))
        }
    }
    impl Drop for CancelledWriter {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::Release);
        }
    }

    #[test]
    fn deadline_cause_survives_native_cancellation_and_writer_is_retained() {
        let dropped = Arc::new(AtomicBool::new(false));
        let stop = StopToken::default();
        let mut writer = AsyncWriter::spawn(
            Box::new(CancelledWriter {
                dropped: Arc::clone(&dropped),
            }),
            stop.clone(),
            || {},
        )
        .unwrap();
        writer
            .begin(
                Arc::from(b"data".as_slice()),
                Instant::now() + Duration::from_millis(20),
                stop.child(),
            )
            .unwrap();
        let limit = Instant::now() + Duration::from_secs(2);
        let result = loop {
            if let WriteProgress::Finished(result) = writer.poll() {
                break result;
            }
            assert!(Instant::now() < limit);
            std::thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(result.unwrap_err().kind, WriteFailureKind::Timeout);
        stop.stop();
        let tickets = writer.tickets();
        for ticket in &tickets {
            assert_eq!(
                ticket.join_until(Instant::now() + Duration::from_secs(2)),
                Ok(crate::owned_worker::WorkerExit::Joined)
            );
        }
        assert!(
            !dropped.load(Ordering::Acquire),
            "retired transport must not signal native EOF"
        );
        drop(writer);
        assert!(dropped.load(Ordering::Acquire));
    }
}
