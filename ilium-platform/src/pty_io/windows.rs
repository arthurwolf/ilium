use super::*;
use crate::owned_worker::{spawn_owned, WorkerKind};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::time::Duration;

const CHECK_INTERVAL: Duration = Duration::from_millis(10);
const CANCELLATION_GRACE: Duration = Duration::from_millis(100);

pub(super) struct Reader(Box<dyn Read + Send>);

impl Reader {
    pub(super) fn spawn(
        mut self,
        stop: StopToken,
        mut sink: impl FnMut(ReadMessage, &StopToken) -> bool + Send + 'static,
    ) -> io::Result<OwnedWorker> {
        spawn_owned(
            "ilium-conpty-read",
            WorkerKind::SynchronousIo,
            stop,
            || {},
            move |stop| {
                let mut buffer = [0; OUTPUT_CHUNK_BYTES];
                while !stop.is_stopped() {
                    let message = match self.0.read(&mut buffer) {
                        Ok(0) => ReadMessage::Eof,
                        Ok(n) => ReadMessage::Data(Arc::from(&buffer[..n])),
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            std::thread::sleep(CHECK_INTERVAL);
                            continue;
                        }
                        Err(error) => ReadMessage::Error(error.into()),
                    };
                    let terminal = !matches!(message, ReadMessage::Data(_));
                    if stop.is_stopped() || !sink(message, &stop) || terminal {
                        break;
                    }
                }
            },
        )
    }
}

pub(super) struct ShellProbe;
impl ShellProbe {
    pub(super) fn shell_owns_terminal(&self, process_id: u32) -> Option<bool> {
        crate::process_info::has_live_child(process_id).map(|has_child| !has_child)
    }
}

pub(super) fn open(
    master: &(dyn MasterPty + Send),
    stop: StopToken,
) -> io::Result<(Reader, Box<dyn PtyWriter>, ShellProbe)> {
    let reader = master
        .try_clone_reader()
        .map_err(|error| io::Error::other(error.to_string()))?;
    let writer = master
        .take_writer()
        .map_err(|error| io::Error::other(error.to_string()))?;
    Ok((
        Reader(reader),
        Box::new(PumpWriter::spawn(writer, stop.child())?),
        ShellProbe,
    ))
}

struct WriteJob {
    bytes: Arc<[u8]>,
    deadline: Instant,
    stop: StopToken,
    progress: Arc<AtomicUsize>,
    complete: SyncSender<Result<WriteSuccess, WriteFailure>>,
}

struct PumpWriter {
    requests: SyncSender<WriteJob>,
    pump: OwnedWorker,
    stopped: bool,
    // A cancelled pump parks its opaque writer here. I/O quarantine must not
    // implicitly send EOF; the owner drops this only at explicit session close.
    _retained_writer: Arc<std::sync::Mutex<Option<Box<dyn Write + Send>>>>,
}

impl PumpWriter {
    fn spawn(mut writer: Box<dyn Write + Send>, stop: StopToken) -> io::Result<Self> {
        // One writer, one in-flight request. No write is queued behind an
        // unacknowledged request and no pump holds a parser/control handle.
        let (requests, incoming) = mpsc::sync_channel::<WriteJob>(1);
        let retained = Arc::new(std::sync::Mutex::new(None));
        let park_writer = Arc::clone(&retained);
        let pump = spawn_owned(
            "ilium-conpty-write",
            WorkerKind::SynchronousIo,
            stop,
            || {},
            move |pump_stop| {
                while !pump_stop.is_stopped() {
                    let job = match incoming.recv_timeout(CHECK_INTERVAL) {
                        Ok(job) => job,
                        Err(RecvTimeoutError::Timeout) => continue,
                        Err(RecvTimeoutError::Disconnected) => break,
                    };
                    let result = deliver(&mut *writer, &job, &pump_stop);
                    // Capacity one and exactly one result; never a stalled send.
                    let _ = job.complete.try_send(result);
                }
                *park_writer
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) = Some(writer);
            },
        )?;
        Ok(Self {
            requests,
            pump,
            stopped: false,
            _retained_writer: retained,
        })
    }

    fn finish_cancellation(
        &mut self,
        response: &Receiver<Result<WriteSuccess, WriteFailure>>,
        progress: &AtomicUsize,
        requested: usize,
        reason: WriteFailureKind,
    ) -> Result<WriteSuccess, WriteFailure> {
        self.stopped = true;
        self.pump.ticket().cancel();
        // The supervisor repeats CancelSynchronousIo across the check-to-write
        // race. This receive waits for COMPLETION, not for cancellation success.
        match response.recv_timeout(CANCELLATION_GRACE) {
            Ok(Ok(mut success)) => {
                success.reusable = false;
                Ok(success)
            }
            Ok(Err(mut error)) => {
                error.reusable = false;
                Err(error)
            }
            Err(_) => Err(WriteFailure {
                kind: reason,
                definitely_written: progress.load(Ordering::Acquire),
                possibly_written: requested,
                settled: false,
                reusable: false,
            }),
        }
    }
}

impl PtyWriter for PumpWriter {
    fn write_until(
        &mut self,
        bytes: Arc<[u8]>,
        deadline: Instant,
        stop: StopToken,
    ) -> Result<WriteSuccess, WriteFailure> {
        if self.stopped {
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
        let progress = Arc::new(AtomicUsize::new(0));
        let (complete, response) = mpsc::sync_channel(1);
        let job = WriteJob {
            bytes,
            deadline,
            stop: stop.clone(),
            progress: Arc::clone(&progress),
            complete,
        };
        match self.requests.try_send(job) {
            Ok(()) => {}
            Err(TrySendError::Disconnected(_)) | Err(TrySendError::Full(_)) => {
                self.stopped = true;
                self.pump.ticket().cancel();
                return Err(WriteFailure::exact(
                    WriteFailureKind::WorkerStopped,
                    0,
                    false,
                ));
            }
        }
        loop {
            match response.try_recv() {
                Ok(result) => return result,
                Err(mpsc::TryRecvError::Disconnected) => {
                    return self.finish_cancellation(
                        &response,
                        &progress,
                        requested,
                        WriteFailureKind::WorkerStopped,
                    );
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
            if stop.is_stopped() {
                return self.finish_cancellation(
                    &response,
                    &progress,
                    requested,
                    WriteFailureKind::Cancelled,
                );
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return self.finish_cancellation(
                    &response,
                    &progress,
                    requested,
                    WriteFailureKind::Timeout,
                );
            }
            match response.recv_timeout(remaining.min(CHECK_INTERVAL)) {
                Ok(result) => return result,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return self.finish_cancellation(
                        &response,
                        &progress,
                        requested,
                        WriteFailureKind::WorkerStopped,
                    );
                }
            }
        }
    }

    fn worker_ticket(&self) -> Option<WorkerTicket> {
        Some(self.pump.ticket())
    }
}

fn deliver(
    writer: &mut dyn Write,
    job: &WriteJob,
    pump_stop: &StopToken,
) -> Result<WriteSuccess, WriteFailure> {
    let mut written = 0;
    loop {
        // Even flush is synchronous opaque Write I/O; keep it on THIS pump.
        if pump_stop.is_stopped() || job.stop.is_stopped() {
            return Err(WriteFailure::exact(
                WriteFailureKind::Cancelled,
                written,
                written == 0 && !pump_stop.is_stopped(),
            ));
        }
        if Instant::now() >= job.deadline {
            return Err(WriteFailure::exact(
                WriteFailureKind::Timeout,
                written,
                written == 0,
            ));
        }
        if written == job.bytes.len() {
            match writer.flush() {
                Ok(()) => {
                    return Ok(WriteSuccess {
                        written,
                        reusable: !pump_stop.is_stopped(),
                    })
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(CHECK_INTERVAL);
                    continue;
                }
                Err(error) => {
                    return Err(WriteFailure::exact(
                        WriteFailureKind::Io(error.into()),
                        written,
                        false,
                    ))
                }
            }
        }
        match writer.write(&job.bytes[written..]) {
            Ok(0) => {
                return Err(WriteFailure::exact(
                    WriteFailureKind::Io(
                        io::Error::new(io::ErrorKind::WriteZero, "zero-length ConPTY write").into(),
                    ),
                    written,
                    false,
                ))
            }
            Ok(n) => {
                written += n;
                job.progress.store(written, Ordering::Release);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(CHECK_INTERVAL)
            }
            Err(error) => {
                return Err(WriteFailure::exact(
                    WriteFailureKind::Io(error.into()),
                    written,
                    false,
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct ScriptedWriter {
        steps: VecDeque<io::Result<usize>>,
        bytes: Arc<std::sync::Mutex<Vec<u8>>>,
    }
    impl Write for ScriptedWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let n = self
                .steps
                .pop_front()
                .unwrap_or(Ok(bytes.len()))?
                .min(bytes.len());
            self.bytes.lock().unwrap().extend_from_slice(&bytes[..n]);
            Ok(n)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn opaque_pump_retries_only_unwritten_bytes() {
        let captured = Arc::new(std::sync::Mutex::new(Vec::new()));
        let writer = ScriptedWriter {
            steps: VecDeque::from([
                Err(io::ErrorKind::Interrupted.into()),
                Ok(2),
                Err(io::ErrorKind::WouldBlock.into()),
                Ok(1),
            ]),
            bytes: Arc::clone(&captured),
        };
        let mut pump = PumpWriter::spawn(Box::new(writer), StopToken::default()).unwrap();
        let result = pump
            .write_until(
                Arc::from(b"abcdef".as_slice()),
                Instant::now() + Duration::from_secs(2),
                StopToken::default(),
            )
            .unwrap();
        assert_eq!(result.written, 6);
        assert_eq!(&*captured.lock().unwrap(), b"abcdef");
        let ticket = pump.pump.ticket();
        drop(pump);
        assert!(ticket
            .join_until(Instant::now() + Duration::from_secs(2))
            .is_ok());
    }
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;

    struct OpaqueStall {
        started: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }
    impl Write for OpaqueStall {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let _ = self.started.send(());
            // Not OS I/O: deliberately cannot be cancelled by CancelSynchronousIo.
            // Proves that an unacknowledged cancellation is never called success.
            self.release.recv().map_err(|_| io::ErrorKind::BrokenPipe)?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn unacknowledged_cancel_is_typed_and_the_handle_remains_owned_until_exit() {
        let (started, wait_started) = mpsc::channel();
        let (release, wait_release) = mpsc::channel();
        let pump = PumpWriter::spawn(
            Box::new(OpaqueStall {
                started,
                release: wait_release,
            }),
            StopToken::default(),
        )
        .unwrap();
        let ticket = pump.pump.ticket();
        let stop = StopToken::default();
        let worker_stop = stop.clone();
        let thread = std::thread::spawn(move || {
            let mut pump = pump;
            let result = pump.write_until(
                Arc::from(b"abcdef".as_slice()),
                Instant::now() + Duration::from_secs(5),
                worker_stop,
            );
            (pump, result)
        });
        wait_started.recv_timeout(Duration::from_secs(2)).unwrap();
        stop.stop();
        let (pump, result) = thread.join().unwrap();
        let failure = result.unwrap_err();
        assert_eq!(failure.kind, WriteFailureKind::Cancelled);
        assert!(!failure.settled);
        assert_eq!(failure.definitely_written, 0);
        assert_eq!(failure.possibly_written, 6);
        assert!(ticket.join_until(Instant::now()).is_err());
        release.send(()).unwrap();
        assert!(ticket
            .join_until(Instant::now() + Duration::from_secs(2))
            .is_ok());
        assert!(
            pump._retained_writer.lock().unwrap().is_some(),
            "quarantine retains opaque writer ownership, rather than injecting EOF"
        );
        drop(pump);
    }
}
