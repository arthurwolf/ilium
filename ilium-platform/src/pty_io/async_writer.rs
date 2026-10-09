//! One bounded transport-only write pump. The PTY state owner starts a write,
//! then keeps processing eligible output while this worker advances bytes.
//! The pump never owns a parser, geometry control, child, or server state.

use super::{PtyWriter, WriteFailure, WriteFailureKind, WriteSuccess};
use crate::owned_worker::{
    OwnedWorker, StopToken, WorkerKind, WorkerReservation, WorkerTicket, reserve_owned_worker,
};
use std::io;
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CANCELLATION_GRACE: Duration = Duration::from_millis(100);

/// The pump blocks on this channel. Cancellation sends `Wake` so an idle pump
/// never needs a timed poll: with hundreds of panes a 10 ms receive timeout
/// was tens of thousands of empty wake-ups a second.
enum PumpMessage {
    Write(WriteJob),
    Wake,
}

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
    requests: SyncSender<PumpMessage>,
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
        let reservation = reserve_owned_worker(None, ())?;
        Self::spawn_reserved(native, stop, on_setup_failure, reservation)
    }

    /// Start the writer under a caller's physical-resource custody lease.
    /// The lease remains in the process-owned registry until the native thread
    /// has joined, even when the writer is quarantined during shutdown.
    pub fn spawn_reserved<C: Send + 'static>(
        native: Box<dyn PtyWriter>,
        stop: StopToken,
        on_setup_failure: impl FnOnce(),
        reservation: WorkerReservation<C>,
    ) -> io::Result<Self> {
        Self::spawn_reserved_with_completion_wake(
            native,
            stop,
            on_setup_failure,
            || {},
            reservation,
        )
    }

    /// As `spawn_reserved`, with a nonblocking wake after each write receipt is
    /// published. Stateful owners can sleep until either this wake or the
    /// active operation's deadline instead of polling its receipt channel.
    pub fn spawn_reserved_with_completion_wake<C: Send + 'static>(
        native: Box<dyn PtyWriter>,
        stop: StopToken,
        on_setup_failure: impl FnOnce(),
        on_completion: impl Fn() + Send + 'static,
        reservation: WorkerReservation<C>,
    ) -> io::Result<Self> {
        let native_ticket = native.worker_ticket();
        let native_slot = Arc::new(Mutex::new(Some(native)));
        let worker_slot = Arc::clone(&native_slot);
        let (requests, incoming) = mpsc::sync_channel::<PumpMessage>(1);
        let cancellation_wake = requests.clone();
        let worker_result = reservation.spawn(
            "ilium-pty-write-dispatch",
            WorkerKind::Cooperative,
            stop,
            // A full slot already holds a job, which wakes the pump anyway.
            move || {
                let _ = cancellation_wake.try_send(PumpMessage::Wake);
            },
            move |worker_stop| {
                let mut native = worker_slot
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take()
                    .expect("native writer has one transport owner");
                while !worker_stop.is_stopped() {
                    let job = match incoming.recv() {
                        Ok(PumpMessage::Write(job)) => job,
                        Ok(PumpMessage::Wake) => continue,
                        Err(mpsc::RecvError) => break,
                    };
                    let result = native.write_until(job.bytes, job.deadline, job.stop);
                    let _ = job.complete.try_send(result);
                    on_completion();
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

    /// Time until the next required owner poll: the write deadline before
    /// cancellation, then the end of the bounded cancellation grace period.
    /// Completion itself wakes the owner independently of this timer.
    pub fn next_poll_delay(&self) -> Option<Duration> {
        self.active.as_ref().map(|active| {
            let deadline = active
                .cancellation_started
                .map(|started| started + CANCELLATION_GRACE)
                .unwrap_or(active.deadline);
            deadline.saturating_duration_since(Instant::now())
        })
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
        match self.requests.try_send(PumpMessage::Write(job)) {
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

    struct ImmediateWriter;
    impl PtyWriter for ImmediateWriter {
        fn write_until(
            &mut self,
            bytes: Arc<[u8]>,
            _: Instant,
            _: StopToken,
        ) -> Result<WriteSuccess, WriteFailure> {
            Ok(WriteSuccess {
                written: bytes.len(),
                reusable: true,
            })
        }
    }

    #[test]
    fn completed_write_wakes_owner_after_publishing_the_receipt() {
        let (wake_sender, wake_receiver) = mpsc::sync_channel(1);
        let mut writer = AsyncWriter::spawn_reserved_with_completion_wake(
            Box::new(ImmediateWriter),
            StopToken::default(),
            || {},
            move || {
                let _ = wake_sender.try_send(());
            },
            reserve_owned_worker(None, ()).unwrap(),
        )
        .unwrap();
        writer
            .begin(
                Arc::from(b"receipt".as_slice()),
                Instant::now() + Duration::from_secs(2),
                StopToken::default(),
            )
            .unwrap();

        assert!(writer.next_poll_delay().is_some());
        wake_receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("write completion must wake the owner");
        match writer.poll() {
            WriteProgress::Finished(Ok(success)) => assert_eq!(success.written, 7),
            WriteProgress::Finished(Err(error)) => panic!("write failed: {error:?}"),
            WriteProgress::Pending => panic!("owner woke before the write receipt was ready"),
        }
        assert!(writer.next_poll_delay().is_none());
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
