//! Ordered terminal emission. Admission precedes composition because graphics
//! protocols may consume one-time transmission state while rendering a widget.
//! A slot lives through its acknowledgement, bounding queued AND retained frames.
use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use ilium_execution::{QuotaGroup, StorageAdmission};
use ilium_platform::owned_worker::{spawn_owned, OwnedWorker, StopToken, WorkerKind};
use ratatui::backend::Backend;
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Position;
use tokio::sync::{mpsc, oneshot};

const FRAME_SLOTS: usize = 2;
pub const MAX_FRAME_BYTES: usize = 32 * 1024 * 1024;
// Two admitted frames (including ACKs), last emitted diff base, and1MiB
// owner/channel/error metadata which may outlive actual output-thread join.
const FRAME_STORAGE_BYTES: usize = (FRAME_SLOTS + 1) * MAX_FRAME_BYTES + 1024 * 1024;
// Encoder retained capacity64MiB and one resize blank32MiB.
// Diff streams directly; no whole-frame diff Vec is allocated.
// These are cooperative declarations, not allocator/native RSS guarantees.
const OUTPUT_WORKER_BYTES: usize = MAX_FRAME_BYTES * 3;

/// Whole-frame buffering with no implicit Drop flush. A failed explicit flush
/// consumes the attempted buffer; an uncertain output prefix is never retried.
pub struct TerminalOutput<W: Write> {
    writer: W,
    bytes: Vec<u8>,
}
impl<W: Write> TerminalOutput<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            bytes: Vec::new(),
        }
    }
}
impl<W: Write> Write for TerminalOutput<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self
            .bytes
            .len()
            .checked_add(bytes.len())
            .is_none_or(|size| size > MAX_FRAME_BYTES * 2)
        {
            return Err(io::Error::new(
                io::ErrorKind::OutOfMemory,
                "terminal encoding exceeds byte limit",
            ));
        }
        let required = self.bytes.len() + bytes.len();
        if required > self.bytes.capacity() {
            self.bytes.reserve_exact(required - self.bytes.len());
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        let written = self.writer.write_all(&self.bytes);
        self.bytes.clear();
        written?;
        self.writer.flush()
    }
}

struct FrameSlots {
    used: AtomicUsize,
    allocation: Arc<StorageAdmission>,
}
// Both terminal failure receipts share the original error allocation. The last
// receipt owns its storage declaration independently of actual worker retirement.
#[derive(Debug)]
struct GuardedFailure {
    error: io::Error,
    _allocation: Arc<StorageAdmission>,
}
impl std::fmt::Display for GuardedFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.error, f)
    }
}
impl std::error::Error for GuardedFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}
fn guarded_failure(error: io::Error, allocation: Arc<StorageAdmission>) -> io::Error {
    let kind = error.kind();
    io::Error::new(
        kind,
        Arc::new(GuardedFailure {
            error,
            _allocation: allocation,
        }),
    )
}
pub struct FrameReservation(Arc<FrameSlots>);

/// A buffer clone carries its original allocation even after the output owner
/// has joined. Public Arc clones cannot accidentally detach bytes from custody.
pub struct GuardedBuffer {
    buffer: Buffer,
    _allocation: Arc<StorageAdmission>,
}
impl std::ops::Deref for GuardedBuffer {
    type Target = Buffer;
    fn deref(&self) -> &Self::Target {
        &self.buffer
    }
}
impl Drop for FrameReservation {
    fn drop(&mut self) {
        self.0.used.fetch_sub(1, Ordering::AcqRel);
    }
}

pub struct SubmissionError {
    pub error: io::Error,
    pub frame: PreparedFrame,
}

impl std::fmt::Debug for SubmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubmissionError")
            .field("error", &self.error)
            .field("frame_id", &self.frame.frame_id)
            .finish()
    }
}

pub struct PreparedFrame {
    pub buffer: Arc<GuardedBuffer>,
    pub cursor: Option<Position>,
    pub frame_id: u64,
    pub layout_revision: u64,
    pub prepared_at: Instant,
    _reservation: FrameReservation,
}
impl PreparedFrame {
    pub fn new(
        reservation: FrameReservation,
        mut buffer: Buffer,
        cursor: Option<Position>,
        frame_id: u64,
        layout_revision: u64,
    ) -> io::Result<Self> {
        buffer.content.shrink_to_fit();
        // Charge the Vec's observable retained capacity. CompactString does
        // not expose its capacity through Cell; twice visible symbol bytes is
        // an explicit conservative allowance, not an allocator hard bound.
        let bytes = buffer
            .content
            .capacity()
            .checked_mul(std::mem::size_of::<Cell>())
            .and_then(|base| {
                buffer.content.iter().try_fold(base, |bytes, cell| {
                    bytes.checked_add(cell.symbol().len().checked_mul(2)?)
                })
            });
        if bytes.is_none_or(|bytes| bytes > MAX_FRAME_BYTES) {
            return Err(guarded_failure(
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "terminal frame exceeds presentation byte limit",
                ),
                reservation.0.allocation.clone(),
            ));
        }
        Ok(Self {
            buffer: Arc::new(GuardedBuffer {
                buffer,
                _allocation: reservation.0.allocation.clone(),
            }),
            cursor,
            frame_id,
            layout_revision,
            prepared_at: Instant::now(),
            _reservation: reservation,
        })
    }
}

pub struct PresentedFrame {
    pub frame: PreparedFrame,
    pub emitted_at: Instant,
    pub emission_duration: Duration,
}

struct Queue {
    frames: VecDeque<PreparedFrame>,
    closing: bool,
}
struct Shared {
    queue: Mutex<Queue>,
    changed: Condvar,
}

/// One actual OS thread is the sole backend owner. Errors are terminal: after
/// partial output we cannot know the screen state and must never retry a prefix.
pub struct Presenter {
    shared: Arc<Shared>,
    slots: Arc<FrameSlots>,
    pub acknowledgements: mpsc::Receiver<io::Result<PresentedFrame>>,
    completion: Option<oneshot::Receiver<io::Result<()>>>,
    _worker: OwnedWorker,
}
impl Presenter {
    pub fn start<B: Backend<Error = io::Error> + Send + 'static>(
        backend: B,
        quota: &QuotaGroup,
    ) -> io::Result<Self> {
        Self::start_with_cleanup(backend, quota, || {})
    }

    /// Cleanup is owned by the output thread too: unwinding or cancellation
    /// cannot restore the terminal while another thread still writes into it.
    pub fn start_with_cleanup<B: Backend<Error = io::Error> + Send + 'static>(
        backend: B,
        quota: &QuotaGroup,
        cleanup: impl FnOnce() + Send + 'static,
    ) -> io::Result<Self> {
        // Admit before allocating owner/channel state or creating an OS thread.
        // Refusal drops the caller's cleanup capture before any output exists.
        let worker_admission = quota
            .reserve_external_worker(1, OUTPUT_WORKER_BYTES)
            .map_err(|reason| {
                io::Error::new(
                    io::ErrorKind::WouldBlock,
                    format!("presentation worker admission: {reason:?}"),
                )
            })?;
        let allocation = Arc::new(
            quota
                .reserve_external_storage(FRAME_STORAGE_BYTES)
                .map_err(|reason| {
                    io::Error::new(
                        io::ErrorKind::WouldBlock,
                        format!("presentation frame storage admission: {reason:?}"),
                    )
                })?,
        );
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue {
                frames: VecDeque::with_capacity(FRAME_SLOTS),
                closing: false,
            }),
            changed: Condvar::new(),
        });
        let slots = Arc::new(FrameSlots {
            used: AtomicUsize::new(0),
            allocation,
        });
        // The extra error slot ensures a backend error cannot wait behind both
        // successful frame receipts while the UI is performing shutdown.
        let (acks, acknowledgements) = mpsc::channel(FRAME_SLOTS + 1);
        let (done, completion) = oneshot::channel();
        let wake = shared.clone();
        let owned = shared.clone();
        let failure_allocation = slots.allocation.clone();
        let wake_allocation = slots.allocation.clone();
        let worker = spawn_owned(
            "ilium-presentation",
            WorkerKind::SynchronousIo,
            StopToken::default(),
            move || {
                // Supervisor retains this closure through actual OS join/TLS.
                let _admission = (&worker_admission, &wake_allocation);
                wake.changed.notify_all();
            },
            move |stop| {
                let closing = owned.clone();
                let restoration = RestoreOnDrop(Some(move || {
                    let mut queue = closing.queue.lock().unwrap_or_else(|e| e.into_inner());
                    queue.closing = true;
                    queue.frames.clear();
                    drop(queue);
                    cleanup();
                }));
                let result = match emit_frames(backend, &owned, &stop, &acks) {
                    Ok(()) => Ok(()),
                    Err(error) => {
                        let kind = error.kind();
                        let failure = Arc::new(GuardedFailure {
                            error,
                            _allocation: failure_allocation,
                        });
                        let _ = acks.try_send(Err(io::Error::new(kind, failure.clone())));
                        Err(io::Error::new(kind, failure))
                    }
                };
                drop(restoration);
                let _ = done.send(result);
            },
        )
        .map_err(|error| guarded_failure(error, slots.allocation.clone()))?;
        Ok(Self {
            shared,
            slots,
            acknowledgements,
            completion: Some(completion),
            _worker: worker,
        })
    }
    /// Never wait for terminal throughput on the UI thread.
    pub fn try_reserve(&self) -> Option<FrameReservation> {
        if self.shared.queue.try_lock().ok()?.closing {
            return None;
        }
        self.slots
            .used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |slots| {
                (slots < FRAME_SLOTS).then_some(slots + 1)
            })
            .ok()?;
        Some(FrameReservation(self.slots.clone()))
    }
    pub fn outstanding_frames(&self) -> usize {
        self.slots.used.load(Ordering::Acquire)
    }
    pub fn submit(&self, frame: PreparedFrame) -> Result<(), SubmissionError> {
        if !Arc::ptr_eq(&frame._reservation.0, &self.slots) {
            return Err(SubmissionError {
                error: guarded_failure(
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "frame reservation belongs to another presenter",
                    ),
                    frame._reservation.0.allocation.clone(),
                ),
                frame,
            });
        }
        let mut queue = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        if queue.closing || queue.frames.len() >= FRAME_SLOTS {
            return Err(SubmissionError {
                error: guarded_failure(
                    io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "presentation owner closed or capacity violated",
                    ),
                    frame._reservation.0.allocation.clone(),
                ),
                frame,
            });
        }
        queue.frames.push_back(frame);
        drop(queue);
        self.shared.changed.notify_one();
        Ok(())
    }
    /// Drain admitted graphics effects before terminal restoration. Waiting is
    /// asynchronous; no join or terminal syscall executes on the UI thread.
    pub async fn shutdown(&mut self) -> io::Result<()> {
        self.shared
            .queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .closing = true;
        self.shared.changed.notify_all();
        let Some(completion) = self.completion.take() else {
            return Ok(());
        };
        completion.await.map_err(|_| {
            guarded_failure(
                io::Error::other("presentation owner panicked"),
                self.slots.allocation.clone(),
            )
        })?
    }
}

struct RestoreOnDrop<F: FnOnce()>(Option<F>);
impl<F: FnOnce()> Drop for RestoreOnDrop<F> {
    fn drop(&mut self) {
        if let Some(cleanup) = self.0.take() {
            cleanup();
        }
    }
}

fn emit_frames<B: Backend<Error = io::Error>>(
    mut backend: B,
    shared: &Shared,
    stop: &StopToken,
    acknowledgements: &mpsc::Sender<io::Result<PresentedFrame>>,
) -> io::Result<()> {
    let mut emitted: Option<Arc<GuardedBuffer>> = None;
    loop {
        let frame = {
            let mut queue = shared.queue.lock().unwrap_or_else(|e| e.into_inner());
            while queue.frames.is_empty() && !queue.closing && !stop.is_stopped() {
                queue = shared
                    .changed
                    .wait(queue)
                    .unwrap_or_else(|e| e.into_inner());
            }
            if stop.is_stopped() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "presentation cancelled before drain",
                ));
            }
            match queue.frames.pop_front() {
                Some(frame) => frame,
                None => return backend.flush(),
            }
        };
        let started = Instant::now();
        emit_one(
            &mut backend,
            emitted.as_ref().map(|buffer| &***buffer),
            &frame,
        )?;
        // Only a complete successful backend flush establishes the next diff base.
        emitted = Some(frame.buffer.clone());
        let acknowledgement = PresentedFrame {
            frame,
            emitted_at: Instant::now(),
            emission_duration: started.elapsed(),
        };
        acknowledgements
            .try_send(Ok(acknowledgement))
            .map_err(|_| io::Error::other("presentation receipt channel unavailable"))?;
    }
}

fn emit_one<B: Backend<Error = io::Error>>(
    backend: &mut B,
    previous: Option<&Buffer>,
    frame: &PreparedFrame,
) -> io::Result<()> {
    let current = &frame.buffer;
    let blank;
    let previous = match previous {
        Some(previous) if previous.area == current.area => previous,
        _ => {
            backend.clear()?;
            blank = Buffer::empty(current.area);
            &blank
        }
    };
    backend.draw(previous.diff_iter(current))?;
    match frame.cursor {
        Some(cursor) => {
            backend.show_cursor()?;
            backend.set_cursor_position(cursor)?;
        }
        None => backend.hide_cursor()?,
    }
    backend.flush()
}

#[cfg(test)]
pub(crate) fn test_quota() -> QuotaGroup {
    QuotaGroup::new(ilium_execution::QuotaLimits {
        clients: 0,
        jobs: 0,
        service_jobs: 0,
        input_bytes: 0,
        result_bytes: 0,
        worker_threads: 1,
        worker_bytes: FRAME_STORAGE_BYTES + OUTPUT_WORKER_BYTES,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::CrosstermBackend;
    use ratatui::layout::Rect;
    use std::io::Write;

    struct GatedWrite {
        entered: std::sync::mpsc::Sender<()>,
        release: std::sync::mpsc::Receiver<()>,
        bytes: Arc<Mutex<Vec<u8>>>,
        first: bool,
        fail: bool,
    }
    impl Write for GatedWrite {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.first {
                self.first = false;
                self.entered.send(()).unwrap();
                self.release.recv().unwrap();
            }
            if self.fail {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "forced partial stream failure",
                ));
            }
            self.bytes.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    fn frame(presenter: &Presenter, id: u64, text: &str) -> PreparedFrame {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 2));
        buffer[(0, 0)].set_symbol(text);
        PreparedFrame::new(
            presenter.try_reserve().unwrap(),
            buffer,
            Some(Position::new(1, 1)),
            id,
            id,
        )
        .unwrap()
    }
    #[tokio::test]
    async fn blocked_logical_drop_retains_worker_and_escaped_frame_until_actual_join() {
        let quota = test_quota();
        let (entered, observed) = std::sync::mpsc::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        let restored = Arc::new(AtomicUsize::new(0));
        let cleanup = restored.clone();
        let presenter = Presenter::start_with_cleanup(
            CrosstermBackend::new(GatedWrite {
                entered,
                release: blocked,
                bytes: Arc::default(),
                first: true,
                fail: false,
            }),
            &quota,
            move || {
                cleanup.fetch_add(1, Ordering::Release);
            },
        )
        .unwrap();
        let prepared = frame(&presenter, 1, "ESCAPED_UPLOAD");
        let escaped = prepared.buffer.clone();
        presenter.submit(prepared).unwrap();
        observed.recv_timeout(Duration::from_secs(2)).unwrap();
        let ticket = presenter._worker.ticket();
        drop(presenter);
        assert_eq!(restored.load(Ordering::Acquire), 0);
        assert_eq!(quota.snapshot().worker_threads, 1);
        assert_eq!(
            quota.snapshot().worker_bytes,
            OUTPUT_WORKER_BYTES + FRAME_STORAGE_BYTES
        );
        assert!(ticket.join_until(Instant::now()).is_err());
        release.send(()).unwrap();
        ticket
            .join_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
        assert_eq!(restored.load(Ordering::Acquire), 1);
        drop(ticket);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, FRAME_STORAGE_BYTES);
        assert_eq!(escaped[(0, 0)].symbol(), "ESCAPED_UPLOAD");
        drop(escaped);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[tokio::test]
    async fn failure_receipts_share_original_storage_after_actual_worker_join() {
        let quota = test_quota();
        let (entered, _) = std::sync::mpsc::channel();
        let (_, gate) = std::sync::mpsc::channel();
        let mut presenter = Presenter::start(
            CrosstermBackend::new(GatedWrite {
                entered,
                release: gate,
                bytes: Arc::default(),
                first: false,
                fail: true,
            }),
            &quota,
        )
        .unwrap();
        presenter.submit(frame(&presenter, 1, "FAILED")).unwrap();
        let acknowledgement = presenter.acknowledgements.recv().await.unwrap();
        let ack_error = match acknowledgement {
            Err(error) => error,
            Ok(_) => panic!("failed output must not acknowledge a frame"),
        };
        let completion_error = presenter.shutdown().await.unwrap_err();
        assert_eq!(ack_error.kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(completion_error.kind(), io::ErrorKind::BrokenPipe);
        let original = |error: &io::Error| {
            error
                .get_ref()
                .unwrap()
                .downcast_ref::<Arc<GuardedFailure>>()
                .unwrap()
                .clone()
        };
        assert!(Arc::ptr_eq(
            &original(&ack_error),
            &original(&completion_error)
        ));
        let ticket = presenter._worker.ticket();
        drop(presenter);
        ticket
            .join_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
        drop(ticket);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, FRAME_STORAGE_BYTES);
        drop(ack_error);
        assert_eq!(quota.snapshot().worker_bytes, FRAME_STORAGE_BYTES);
        drop(completion_error);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn storage_refusal_precedes_spawn_and_drops_original_cleanup_capture_once() {
        struct Cleanup(Arc<AtomicUsize>);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Release);
            }
        }
        let restored = Arc::new(AtomicUsize::new(0));
        let original = Cleanup(restored.clone());
        let mut limits = test_quota().snapshot().limits;
        limits.worker_bytes -= 1;
        let quota = QuotaGroup::new(limits);
        let result = Presenter::start_with_cleanup(
            CrosstermBackend::new(TerminalOutput::new(Vec::<u8>::new())),
            &quota,
            move || drop(original),
        );
        assert_eq!(result.err().unwrap().kind(), io::ErrorKind::WouldBlock);
        assert_eq!(restored.load(Ordering::Acquire), 1);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn thread_refusal_precedes_spawn_and_initial_output_has_no_buffer_allocation() {
        let mut limits = test_quota().snapshot().limits;
        limits.worker_threads = 0;
        let quota = QuotaGroup::new(limits);
        let output = TerminalOutput::new(Vec::<u8>::new());
        assert_eq!(output.bytes.capacity(), 0);
        let result = Presenter::start(CrosstermBackend::new(output), &quota);
        assert_eq!(result.err().unwrap().kind(), io::ErrorKind::WouldBlock);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[tokio::test]
    async fn blocked_output_bounds_admission_and_emits_fifo_before_shutdown() {
        let (entered, wait) = std::sync::mpsc::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let mut presenter = Presenter::start(
            CrosstermBackend::new(GatedWrite {
                entered,
                release: gate,
                bytes: bytes.clone(),
                first: true,
                fail: false,
            }),
            &test_quota(),
        )
        .unwrap();
        presenter
            .submit(frame(&presenter, 1, "FIRST_UPLOAD"))
            .unwrap();
        wait.recv_timeout(Duration::from_secs(2)).unwrap();
        presenter.submit(frame(&presenter, 2, "SECOND")).unwrap();
        assert!(presenter.try_reserve().is_none());
        assert_eq!(presenter.outstanding_frames(), 2);
        release.send(()).unwrap();
        let first = presenter.acknowledgements.recv().await.unwrap().unwrap();
        assert_eq!(first.frame.frame_id, 1);
        let second = presenter.acknowledgements.recv().await.unwrap().unwrap();
        assert_eq!(second.frame.frame_id, 2);
        assert!(
            presenter.try_reserve().is_none(),
            "retained receipts remain charged"
        );
        drop(first);
        drop(second);
        assert!(presenter.try_reserve().is_some());
        presenter.shutdown().await.unwrap();
        let bytes = bytes.lock().unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.find("FIRST_UPLOAD").unwrap() < text.find("SECOND").unwrap());
    }
    #[tokio::test]
    async fn failed_output_never_acknowledges_a_frame() {
        let (entered, wait) = std::sync::mpsc::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let mut presenter = Presenter::start(
            CrosstermBackend::new(GatedWrite {
                entered,
                release: gate,
                bytes: Arc::default(),
                first: true,
                fail: true,
            }),
            &test_quota(),
        )
        .unwrap();
        presenter.submit(frame(&presenter, 1, "FAIL")).unwrap();
        wait.recv_timeout(Duration::from_secs(2)).unwrap();
        release.send(()).unwrap();
        assert!(presenter.acknowledgements.recv().await.unwrap().is_err());
        assert!(presenter.shutdown().await.is_err());
    }
    #[tokio::test]
    async fn reservations_cannot_cross_output_owners() {
        let mut first =
            Presenter::start(CrosstermBackend::new(Vec::<u8>::new()), &test_quota()).unwrap();
        let mut second =
            Presenter::start(CrosstermBackend::new(Vec::<u8>::new()), &test_quota()).unwrap();
        let rejected = second.submit(frame(&first, 1, "OWNED")).unwrap_err();
        assert_eq!(rejected.error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(first.outstanding_frames(), 1);
        assert_eq!(second.outstanding_frames(), 0);
        first.submit(rejected.frame).unwrap();
        let emitted = first.acknowledgements.recv().await.unwrap().unwrap();
        assert_eq!(emitted.frame.frame_id, 1);
        first.shutdown().await.unwrap();
        second.shutdown().await.unwrap();
    }

    #[test]
    fn diff_cursor_and_resize_follow_the_actual_emitted_base() {
        let slots = Arc::new(FrameSlots {
            used: AtomicUsize::new(1),
            allocation: Arc::new(
                test_quota()
                    .reserve_external_storage(FRAME_STORAGE_BYTES)
                    .unwrap(),
            ),
        });
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 2));
        buffer[(0, 0)].set_symbol("A");
        let first = PreparedFrame::new(
            FrameReservation(slots.clone()),
            buffer,
            Some(Position::new(2, 1)),
            1,
            0,
        )
        .unwrap();
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let (entered, _) = std::sync::mpsc::channel();
        let (_, gate) = std::sync::mpsc::channel();
        let mut backend = CrosstermBackend::new(GatedWrite {
            entered,
            release: gate,
            bytes: bytes.clone(),
            first: false,
            fail: false,
        });
        emit_one(&mut backend, None, &first).unwrap();
        let first_bytes = String::from_utf8_lossy(&bytes.lock().unwrap()).into_owned();
        assert!(first_bytes.contains("A"));
        assert!(
            first_bytes.contains("\x1b[2;3H"),
            "cursor accompanies frame"
        );
        bytes.lock().unwrap().clear();
        emit_one(&mut backend, Some(&first.buffer), &first).unwrap();
        assert!(
            !String::from_utf8_lossy(&bytes.lock().unwrap())
                .into_owned()
                .contains('A'),
            "unchanged cells are not emitted"
        );
        bytes.lock().unwrap().clear();
        slots.used.fetch_add(1, Ordering::Relaxed);
        let mut resized_buffer = Buffer::empty(Rect::new(0, 0, 6, 3));
        resized_buffer[(0, 0)].set_symbol("A");
        let resized =
            PreparedFrame::new(FrameReservation(slots), resized_buffer, None, 2, 1).unwrap();
        emit_one(&mut backend, Some(&first.buffer), &resized).unwrap();
        let resized_bytes = String::from_utf8_lossy(&bytes.lock().unwrap()).into_owned();
        assert!(
            resized_bytes.contains("\x1b[2J"),
            "resize invalidates old base"
        );
        assert!(resized_bytes.contains('A'));
        assert!(
            resized_bytes.contains("\x1b[?25l"),
            "hidden cursor is emitted with new geometry"
        );
    }

    #[tokio::test]
    async fn restoration_waits_for_blocked_output_and_drain() {
        let (entered, wait) = std::sync::mpsc::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let restored = Arc::new(AtomicUsize::new(0));
        let cleanup = restored.clone();
        let mut presenter = Presenter::start_with_cleanup(
            CrosstermBackend::new(GatedWrite {
                entered,
                release: gate,
                bytes: Arc::default(),
                first: true,
                fail: false,
            }),
            &test_quota(),
            move || {
                cleanup.fetch_add(1, Ordering::Release);
            },
        )
        .unwrap();
        presenter
            .submit(frame(&presenter, 1, "RESTORE_AFTER"))
            .unwrap();
        wait.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(restored.load(Ordering::Acquire), 0);
        release.send(()).unwrap();
        presenter.shutdown().await.unwrap();
        assert_eq!(restored.load(Ordering::Acquire), 1);
        assert_eq!(
            presenter
                .acknowledgements
                .recv()
                .await
                .unwrap()
                .unwrap()
                .frame
                .frame_id,
            1
        );
    }
    #[test]
    fn uncertain_output_tail_is_not_retried_on_drop() {
        struct PartialFailure(Arc<AtomicUsize>);
        impl Write for PartialFailure {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if self.0.fetch_add(1, Ordering::Relaxed) == 0 {
                    return Ok(bytes.len().min(2));
                }
                Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "failure after accepted prefix",
                ))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let writes = Arc::new(AtomicUsize::new(0));
        let mut output = TerminalOutput::new(PartialFailure(writes.clone()));
        output.write_all(b"FRAME").unwrap();
        assert!(output.flush().is_err());
        assert!(output.bytes.is_empty());
        drop(output);
        assert_eq!(
            writes.load(Ordering::Relaxed),
            2,
            "Drop must never retry buffered tail"
        );
    }

    #[tokio::test]
    async fn output_panic_restores_on_output_thread_and_reports_failure() {
        struct PanicWrite;
        impl Write for PanicWrite {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                panic!("forced output panic");
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let (restored, restoration) = std::sync::mpsc::channel();
        let ui_thread = std::thread::current().id();
        let mut presenter = Presenter::start_with_cleanup(
            CrosstermBackend::new(PanicWrite),
            &test_quota(),
            move || {
                restored.send(std::thread::current().id()).unwrap();
            },
        )
        .unwrap();
        presenter.submit(frame(&presenter, 1, "PANIC")).unwrap();
        assert_ne!(
            restoration.recv_timeout(Duration::from_secs(2)).unwrap(),
            ui_thread
        );
        assert!(presenter.shutdown().await.is_err());
        assert!(presenter.try_reserve().is_none());
    }
    #[test]
    fn oversized_graphics_symbol_releases_reserved_capacity() {
        let slots = Arc::new(FrameSlots {
            used: AtomicUsize::new(1),
            allocation: Arc::new(
                test_quota()
                    .reserve_external_storage(FRAME_STORAGE_BYTES)
                    .unwrap(),
            ),
        });
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 1));
        buffer[(0, 0)].set_symbol(&"x".repeat(MAX_FRAME_BYTES / 2 + 1));
        let result = PreparedFrame::new(FrameReservation(slots.clone()), buffer, None, 1, 1);
        assert!(result.is_err());
        assert_eq!(slots.used.load(Ordering::Acquire), 0);
    }
}
