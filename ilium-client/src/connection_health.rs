//! Local, payload-free observations owned by one transport connection.
//! Sampling never queries the peer or locks transport/codec state. Phases say
//! where a task last progressed, not whether the server has deadlocked.
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;

use tokio::io::AsyncWrite;

#[derive(Clone, Copy)]
pub(crate) enum Direction {
    Read,
    Write,
}

#[derive(Clone, Copy)]
#[repr(usize)]
pub(crate) enum ReadPhase {
    Header,
    Admission,
    Body,
    Decode,
    Retention,
    Publication,
}

#[derive(Clone, Copy)]
#[repr(usize)]
pub(crate) enum WritePhase {
    Waiting,
    Encoding,
    Writing,
    Flush,
}

const NOT_STARTED: usize = 0;
const RUNNING: usize = 1;
const CLOSED: usize = 2;
const FAILED: usize = 3;
const CANCELLED: usize = 4;
const NEVER: u64 = u64::MAX;

#[derive(Debug)]
struct Lane {
    state: AtomicUsize,
    phase: AtomicUsize,
    phase_since_ms: AtomicU64,
}
impl Lane {
    fn new() -> Self {
        Self {
            state: AtomicUsize::new(NOT_STARTED),
            phase: AtomicUsize::new(0),
            phase_since_ms: AtomicU64::new(0),
        }
    }
    fn phase(&self, phase: usize, now: u64) {
        // A pending I/O future can be polled many times. Its age must not reset.
        if self.phase.load(Ordering::Relaxed) != phase {
            self.phase_since_ms.store(now, Ordering::Relaxed);
            self.phase.store(phase, Ordering::Release);
        }
    }
    fn finish(&self, state: usize) {
        // Cancellation/Drop must not erase an already observed failure or EOF.
        let _ = self
            .state
            .compare_exchange(RUNNING, state, Ordering::AcqRel, Ordering::Acquire);
    }
    fn report(&self, now: u64, labels: &[&str]) -> String {
        let phase = self.phase.load(Ordering::Acquire);
        let state = match self.state.load(Ordering::Acquire) {
            NOT_STARTED => "not started",
            RUNNING => "running",
            CLOSED => "closed",
            FAILED => "failed",
            CANCELLED => "cancelled",
            _ => "unknown",
        };
        format!(
            "state={state}, phase={}, phase_age_ms={}",
            labels.get(phase).copied().unwrap_or("unknown"),
            now.saturating_sub(self.phase_since_ms.load(Ordering::Relaxed)),
        )
    }
}

#[derive(Debug)]
pub(crate) struct ConnectionHealth {
    started: Instant,
    server_process_id: Option<u32>,
    read: Lane,
    write: Lane,
    pub(crate) headers_received: AtomicU64,
    pub(crate) bodies_received: AtomicU64,
    pub(crate) events_decoded: AtomicU64,
    pub(crate) events_published: AtomicU64,
    pub(crate) requests_encoded: AtomicU64,
    pub(crate) requests_flushed: AtomicU64,
    pub(crate) last_declared_payload_bytes: AtomicU64,
    pub(crate) written_bytes: AtomicU64,
    last_publication_ms: AtomicU64,
}
impl ConnectionHealth {
    #[cfg(test)]
    pub(crate) fn new() -> Arc<Self> {
        Self::with_server_process_id(None)
    }
    pub(crate) fn with_server_process_id(server_process_id: Option<u32>) -> Arc<Self> {
        Arc::new(Self {
            started: Instant::now(),
            server_process_id,
            read: Lane::new(),
            write: Lane::new(),
            headers_received: AtomicU64::new(0),
            bodies_received: AtomicU64::new(0),
            events_decoded: AtomicU64::new(0),
            events_published: AtomicU64::new(0),
            requests_encoded: AtomicU64::new(0),
            requests_flushed: AtomicU64::new(0),
            last_declared_payload_bytes: AtomicU64::new(0),
            written_bytes: AtomicU64::new(0),
            last_publication_ms: AtomicU64::new(NEVER),
        })
    }
    fn now_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX - 1)
    }
    fn lane(&self, direction: Direction) -> &Lane {
        match direction {
            Direction::Read => &self.read,
            Direction::Write => &self.write,
        }
    }
    pub(crate) fn task(self: &Arc<Self>, direction: Direction) -> HealthTask {
        self.lane(direction).state.store(RUNNING, Ordering::Release);
        HealthTask {
            health: Arc::clone(self),
            direction,
        }
    }
    pub(crate) fn read_phase(&self, phase: ReadPhase) {
        self.read.phase(phase as usize, self.now_ms());
    }
    pub(crate) fn write_phase(&self, phase: WritePhase) {
        self.write.phase(phase as usize, self.now_ms());
    }
    pub(crate) fn published(&self) {
        self.events_published.fetch_add(1, Ordering::Relaxed);
        self.last_publication_ms
            .store(self.now_ms(), Ordering::Relaxed);
    }
    pub(crate) fn cancel(&self) {
        self.read.finish(CANCELLED);
        self.write.finish(CANCELLED);
    }
    pub(crate) fn report(&self) -> String {
        let now = self.now_ms();
        let published = self.last_publication_ms.load(Ordering::Relaxed);
        let age = if published == NEVER {
            "none yet".to_owned()
        } else {
            format!("{} ms", now.saturating_sub(published))
        };
        format!(
            "server_process_id={:?} (server build not sampled; client reconnect does not restart server)\nConnection read: {}; headers={}, bodies={}, decoded={}, published={}, last_declared_payload_bytes={}, last_publication_age={age}\nConnection write: {}; encoded={}, flushed={}, confirmed_written_bytes={}\nHeader wait can be idle or a partial header; request wait is normally idle. Ages and counters are concurrent local samples, not a server responsiveness measurement or proof of delivery/handling.",
            self.server_process_id,
            self.read.report(now, &["header wait (idle or partial header)", "decoder admission", "body", "decode", "decoded retention", "event publication"]),
            self.headers_received.load(Ordering::Relaxed),
            self.bodies_received.load(Ordering::Relaxed),
            self.events_decoded.load(Ordering::Relaxed),
            self.events_published.load(Ordering::Relaxed),
            self.last_declared_payload_bytes.load(Ordering::Relaxed),
            self.write.report(now, &["request wait (normally idle)", "encoding/admission", "writing", "flush"]),
            self.requests_encoded.load(Ordering::Relaxed),
            self.requests_flushed.load(Ordering::Relaxed),
            self.written_bytes.load(Ordering::Relaxed),
        )
    }
}

/// Created before spawning, so aborting an unpolled task is visible as well.
pub(crate) struct HealthTask {
    pub(crate) health: Arc<ConnectionHealth>,
    direction: Direction,
}
impl HealthTask {
    pub(crate) fn finish(&self, success: bool) {
        self.health
            .lane(self.direction)
            .finish(if success { CLOSED } else { FAILED });
    }
}
impl Drop for HealthTask {
    fn drop(&mut self) {
        self.health.lane(self.direction).finish(CANCELLED);
    }
}

/// Observe the actual AsyncWrite boundary, including partial writes and flush.
/// FrameWriter's timeouts and vectored-write support remain unchanged.
pub(crate) struct ObservedWriter<W> {
    inner: W,
    health: Arc<ConnectionHealth>,
}
impl<W> ObservedWriter<W> {
    pub(crate) fn new(inner: W, health: Arc<ConnectionHealth>) -> Self {
        Self { inner, health }
    }
    fn record(&self, result: &Poll<io::Result<usize>>) {
        if let Poll::Ready(Ok(bytes)) = result {
            self.health
                .written_bytes
                .fetch_add(*bytes as u64, Ordering::Relaxed);
        }
    }
}
impl<W: AsyncWrite + Unpin> AsyncWrite for ObservedWriter<W> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.health.write_phase(WritePhase::Writing);
        let result = Pin::new(&mut self.inner).poll_write(context, bytes);
        self.record(&result);
        result
    }
    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        self.health.write_phase(WritePhase::Writing);
        let result = Pin::new(&mut self.inner).poll_write_vectored(context, bytes);
        self.record(&result);
        result
    }
    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.health.write_phase(WritePhase::Flush);
        Pin::new(&mut self.inner).poll_flush(context)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_preserves_phase_and_does_not_erase_failed_or_closed() {
        let health = ConnectionHealth::new();
        let task = health.task(Direction::Read);
        health.read_phase(ReadPhase::Retention);
        task.finish(false);
        drop(task);
        health.cancel();
        assert_eq!(health.read.state.load(Ordering::Relaxed), FAILED);
        assert_eq!(
            health.read.phase.load(Ordering::Relaxed),
            ReadPhase::Retention as usize
        );
        let task = health.task(Direction::Write);
        task.finish(true);
        drop(task);
        health.cancel();
        assert_eq!(health.write.state.load(Ordering::Relaxed), CLOSED);
    }

    #[test]
    fn repeated_pending_io_phase_does_not_restart_age() {
        let health = ConnectionHealth::new();
        health.write_phase(WritePhase::Flush);
        health.write.phase_since_ms.store(123, Ordering::Relaxed);
        for _ in 0..10_000 {
            health.write_phase(WritePhase::Flush);
        }
        assert_eq!(health.write.phase_since_ms.load(Ordering::Relaxed), 123);
    }

    #[test]
    fn unpolled_future_cancellation_is_retained() {
        let health = ConnectionHealth::new();
        let task = health.task(Direction::Read);
        let future = async move {
            let _owned_task = task;
            std::future::pending::<()>().await
        };
        drop(future);
        assert_eq!(health.read.state.load(Ordering::Relaxed), CANCELLED);
        assert!(health.report().contains("Header wait can be idle"));
        assert!(health.report().contains("last_publication_age=none yet"));
    }
}
