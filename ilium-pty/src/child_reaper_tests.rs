//! Deterministic adapter custody tests, plus the real failed-spawn boundary.
use super::*;
use crate::admission::{PtyWorkerReservations, PTY_WORKERS_PER_SESSION, PTY_WORKER_STACK_BYTES};
use crate::{PtyCommand, PtySession};
use ilium_execution::{QuotaGroup, QuotaLimits};
use ilium_platform::owned_worker::WorkerExit;
use portable_pty::{ChildKiller, ExitStatus};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Debug, Default)]
struct ChildState {
    kill_calls: AtomicUsize,
    query_calls: AtomicUsize,
    drops: AtomicUsize,
    failures_before_kill: usize,
    first_query_fails: bool,
    exited: AtomicBool,
}

#[derive(Debug)]
struct ControlledChild(Arc<ChildState>);
impl Drop for ControlledChild {
    fn drop(&mut self) {
        self.0.drops.fetch_add(1, Ordering::SeqCst);
    }
}
impl ChildKiller for ControlledChild {
    fn kill(&mut self) -> io::Result<()> {
        let attempt = self.0.kill_calls.fetch_add(1, Ordering::SeqCst);
        if attempt < self.0.failures_before_kill {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "synthetic kill refusal",
            ));
        }
        self.0.exited.store(true, Ordering::SeqCst);
        Ok(())
    }
    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(Self(Arc::clone(&self.0)))
    }
}
impl Child for ControlledChild {
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let count = self.0.query_calls.fetch_add(1, Ordering::SeqCst);
        if count == 0 && self.0.first_query_fails {
            return Err(io::Error::other("synthetic transient status failure"));
        }
        Ok(self
            .0
            .exited
            .load(Ordering::SeqCst)
            .then(|| ExitStatus::with_exit_code(23)))
    }
    fn wait(&mut self) -> io::Result<ExitStatus> {
        panic!("partial setup must use supervised nonblocking reaping, never ignored wait")
    }
    fn process_id(&self) -> Option<u32> {
        None
    }
}

fn limits() -> QuotaLimits {
    QuotaLimits {
        clients: 0,
        jobs: 0,
        service_jobs: 0,
        input_bytes: 0,
        result_bytes: 0,
        worker_threads: PTY_WORKERS_PER_SESSION,
        worker_bytes: PTY_WORKERS_PER_SESSION * PTY_WORKER_STACK_BYTES,
    }
}

fn pending() -> (
    PendingChildReaper,
    Arc<Mutex<Option<PtyChildExit>>>,
    QuotaGroup,
) {
    let quota = QuotaGroup::new(limits());
    let reservations = PtyWorkerReservations::new(Some(&quota)).unwrap();
    let receipt = Arc::new(Mutex::new(None));
    let pending =
        PendingChildReaper::start(reservations.child_reaper, Arc::clone(&receipt)).unwrap();
    // No owner threads are started in this fixture. Their reservations retire
    // here; the shared full-session debit stays with the physical reaper.
    drop(reservations.owner);
    (pending, receipt, quota)
}

fn attach(
    pending: PendingChildReaper,
    state: Arc<ChildState>,
) -> (OwnedWorker, SharedChild, Option<u32>) {
    let control: SharedChild = Arc::new(Mutex::new(Box::new(ControlledChild(state))));
    let worker = pending.attach(Arc::clone(&control), None);
    (worker, control, None)
}

#[test]
fn published_child_cannot_reap_until_birth_identity_guard_releases() {
    let (pending, receipt, quota) = pending();
    let state = Arc::new(ChildState::default());
    state.exited.store(true, Ordering::SeqCst);
    let control: SharedChild = Arc::new(Mutex::new(Box::new(ControlledChild(Arc::clone(&state)))));
    let birth_identity_guard = control.lock().unwrap();
    let worker = pending.attach(Arc::clone(&control), None);
    let ticket = worker.ticket();
    ticket.cancel(); // Cancellation cannot bypass the birth-identity barrier.
    assert!(ticket
        .join_until(Instant::now() + Duration::from_millis(50))
        .is_err());
    assert_eq!(state.query_calls.load(Ordering::SeqCst), 0);
    assert!(receipt.lock().unwrap().is_none());
    assert_eq!(quota.snapshot().worker_threads, PTY_WORKERS_PER_SESSION);
    drop(birth_identity_guard);
    assert_eq!(
        ticket
            .join_until(Instant::now() + Duration::from_secs(3))
            .unwrap(),
        WorkerExit::Joined
    );
    assert!(receipt.lock().unwrap().is_some());
    assert_eq!(quota.snapshot().worker_threads, 0);
    drop((worker, control));
}

#[test]
fn abandoned_native_spawn_retires_prestarted_worker_and_full_debit() {
    let (pending, receipt, quota) = pending();
    let ticket = pending.worker.ticket();
    assert_eq!(quota.snapshot().worker_threads, PTY_WORKERS_PER_SESSION);
    assert!(ticket.exit().is_none());
    drop(pending); // Same handoff resolution as spawn_command returning Err.
    assert_eq!(
        ticket
            .join_until(Instant::now() + Duration::from_secs(3))
            .unwrap(),
        WorkerExit::Joined
    );
    assert!(receipt.lock().unwrap().is_none());
    assert_eq!(quota.snapshot().worker_threads, 0);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}

#[test]
fn cancellation_during_native_spawn_waits_for_handoff_then_reaps_child() {
    let (pending, receipt, quota) = pending();
    let ticket = pending.worker.ticket();
    ticket.cancel();
    assert!(ticket
        .join_until(Instant::now() + Duration::from_millis(30))
        .is_err());
    assert_eq!(quota.snapshot().worker_threads, PTY_WORKERS_PER_SESSION);
    let state = Arc::new(ChildState::default());
    let (worker, control, process_id) = attach(pending, Arc::clone(&state));
    assert_eq!(process_id, None);
    assert_eq!(
        ticket
            .join_until(Instant::now() + Duration::from_secs(3))
            .unwrap(),
        WorkerExit::Joined
    );
    assert_eq!(state.kill_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        receipt.lock().unwrap().as_ref().unwrap().cause,
        super::super::PtyExitCause::ExitCode(23)
    );
    assert_eq!(quota.snapshot().worker_threads, 0);
    drop((worker, control));
    assert_eq!(state.drops.load(Ordering::SeqCst), 1);
}

#[test]
fn setup_failure_keeps_control_through_failed_kill_then_retries() {
    let (pending, receipt, quota) = pending();
    let state = Arc::new(ChildState {
        failures_before_kill: 1,
        first_query_fails: true,
        ..ChildState::default()
    });
    let (worker, control, _) = attach(pending, Arc::clone(&state));
    let ticket = worker.ticket();
    let started = Instant::now();
    // Actual setup-failure cleanup calls this same registered ticket, without
    // a second unsupervised native kill or wait on the spawning thread.
    ticket.cancel();
    drop((worker, control));
    assert!(ticket
        .join_until(started + Duration::from_millis(300))
        .is_err());
    assert_eq!(quota.snapshot().worker_threads, PTY_WORKERS_PER_SESSION);
    assert_eq!(state.drops.load(Ordering::SeqCst), 0);
    assert_eq!(
        ticket.join_until(started + Duration::from_secs(4)).unwrap(),
        WorkerExit::Joined
    );
    assert!(started.elapsed() >= CHILD_KILL_RETRY_INTERVAL);
    assert_eq!(state.kill_calls.load(Ordering::SeqCst), 2);
    assert!(state.query_calls.load(Ordering::SeqCst) > 1);
    assert_eq!(state.drops.load(Ordering::SeqCst), 1);
    assert!(receipt.lock().unwrap().is_some());
    assert_eq!(quota.snapshot().worker_threads, 0);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}

#[test]
fn child_exit_before_publication_is_retained_without_spurious_termination() {
    let (pending, receipt, quota) = pending();
    let state = Arc::new(ChildState::default());
    state.exited.store(true, Ordering::SeqCst);
    let (worker, control, _) = attach(pending, Arc::clone(&state));
    assert_eq!(
        worker
            .ticket()
            .join_until(Instant::now() + Duration::from_secs(3))
            .unwrap(),
        WorkerExit::Joined
    );
    assert_eq!(state.kill_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        receipt.lock().unwrap().as_ref().unwrap().cause,
        super::super::PtyExitCause::ExitCode(23)
    );
    assert_eq!(quota.snapshot().worker_threads, 0);
    drop((worker, control));
    assert_eq!(state.drops.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn real_failed_native_spawn_retires_prestarted_worker() {
    let changed = Arc::new(tokio::sync::Notify::new());
    let wake = Arc::clone(&changed);
    let quota = QuotaGroup::new_with_admission_wake(limits(), move || wake.notify_waiters());
    // This nonexistent program path creates no file. TMPDIR is job scratch.
    let directory = std::env::temp_dir();
    let absent_program = directory.join(format!(
        "ilium-absent-pty-command-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    let result = PtySession::spawn_with_quota(
        PtyCommand::new(absent_program.to_string_lossy(), &directory, 24, 80),
        quota.clone(),
    );
    assert!(matches!(result, Err(crate::PtyError::Spawn(_))));
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let notified = changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if quota.snapshot().worker_threads == 0 {
                break;
            }
            notified.await;
        }
    })
    .await
    .expect("prestarted native reaper physically retired after spawn error");
    assert_eq!(quota.snapshot().worker_bytes, 0);
}
