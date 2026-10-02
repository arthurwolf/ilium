//! Join ownership for PTY workers, including synchronous Windows I/O pumps.
//!
//! Cancellation has two distinct outcomes: observed thread exit, or a deadline
//! with the JoinHandle still owned here. Never detach a stuck pump and never
//! kill an OS thread. A bounded process-lifetime supervisor keeps retrying
//! Windows cancellation and joins only handles whose `is_finished()` is true.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const MAX_OWNED_WORKERS: usize = 1024;
const SUPERVISOR_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Clone, Default)]
pub struct StopToken(Arc<StopState>);

#[derive(Default)]
struct StopState {
    stopped: AtomicBool,
    parent: Option<StopToken>,
}

impl StopToken {
    pub fn child(&self) -> Self {
        Self(Arc::new(StopState {
            stopped: AtomicBool::new(false),
            parent: Some(self.clone()),
        }))
    }

    pub fn stop(&self) {
        self.0.stopped.store(true, Ordering::Release);
    }

    pub fn is_stopped(&self) -> bool {
        let mut current = Some(self);
        while let Some(token) = current {
            if token.0.stopped.load(Ordering::Acquire) {
                return true;
            }
            current = token.0.parent.as_ref();
        }
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerKind {
    Cooperative,
    SynchronousIo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerExit {
    Joined,
    Panicked,
}

/// A timeout is NOT a successful join. Its handle remains in the supervisor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JoinDeadline {
    pub worker_id: u64,
}

struct WorkerState {
    stop: StopToken,
    wake: Arc<dyn Fn() + Send + Sync>,
    exit: Mutex<Option<WorkerExit>>,
    changed: Condvar,
}

#[derive(Clone)]
pub struct WorkerTicket {
    id: u64,
    state: Arc<WorkerState>,
    supervisor: Arc<SupervisorState>,
}

impl WorkerTicket {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn cancel(&self) {
        self.state.stop.stop();
        (self.state.wake)();
        self.supervisor.changed.notify_all();
    }

    pub fn exit(&self) -> Option<WorkerExit> {
        *self.state.exit.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Blocking wait, for a dedicated thread or Tokio `spawn_blocking` only.
    pub fn join_until(&self, deadline: Instant) -> Result<WorkerExit, JoinDeadline> {
        let mut exit = self.state.exit.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(exit) = *exit {
                return Ok(exit);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(JoinDeadline { worker_id: self.id });
            }
            exit = self
                .state
                .changed
                .wait_timeout(exit, remaining)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
}

/// Dropping the logical owner requests cancellation without blocking a caller.
/// Actual JoinHandles always stay in the bounded supervisor until joined.
pub struct OwnedWorker {
    ticket: WorkerTicket,
}

impl OwnedWorker {
    pub fn ticket(&self) -> WorkerTicket {
        self.ticket.clone()
    }
}

impl Drop for OwnedWorker {
    fn drop(&mut self) {
        self.ticket.cancel();
    }
}

struct Record {
    handle: JoinHandle<()>,
    kind: WorkerKind,
    state: Arc<WorkerState>,
    wake_sent: bool,
}

struct Registry {
    next_id: u64,
    records: Vec<Record>,
}

struct SupervisorState {
    registry: Mutex<Registry>,
    changed: Condvar,
}

struct Supervisor {
    state: Arc<SupervisorState>,
    // An intentionally process-lifetime service, not a detached per-pane task.
    _join: Mutex<JoinHandle<()>>,
}

static SUPERVISOR: OnceLock<Result<Supervisor, String>> = OnceLock::new();

fn supervisor() -> io::Result<&'static Supervisor> {
    match SUPERVISOR.get_or_init(|| {
        let state = Arc::new(SupervisorState {
            registry: Mutex::new(Registry {
                next_id: 1,
                records: Vec::new(),
            }),
            changed: Condvar::new(),
        });
        let worker_state = Arc::clone(&state);
        std::thread::Builder::new()
            .name("ilium-pty-joins".into())
            .spawn(move || supervise(worker_state))
            .map(|join| Supervisor {
                state,
                _join: Mutex::new(join),
            })
            .map_err(|error| error.to_string())
    }) {
        Ok(supervisor) => Ok(supervisor),
        Err(message) => Err(io::Error::other(message.clone())),
    }
}

/// Start only after reserving a registry slot. Both live and retiring workers
/// count against the cap; a hung kernel cannot cause unbounded thread growth.
/// `wake` must be nonblocking and must not acquire any parser/registry lock.
pub fn spawn_owned(
    name: &str,
    kind: WorkerKind,
    stop: StopToken,
    wake: impl Fn() + Send + Sync + 'static,
    body: impl FnOnce(StopToken) + Send + 'static,
) -> io::Result<OwnedWorker> {
    let supervisor = supervisor()?;
    let mut registry = supervisor
        .state
        .registry
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if registry.records.len() >= MAX_OWNED_WORKERS {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "PTY worker ownership capacity exhausted",
        ));
    }
    let id = registry.next_id;
    registry.next_id = id
        .checked_add(1)
        .ok_or_else(|| io::Error::other("PTY worker identity exhausted"))?;
    let state = Arc::new(WorkerState {
        stop: stop.clone(),
        wake: Arc::new(wake),
        exit: Mutex::new(None),
        changed: Condvar::new(),
    });
    let notify = Arc::clone(&supervisor.state);
    let handle = std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            body(stop);
            notify.changed.notify_all();
        })?;
    registry.records.push(Record {
        handle,
        kind,
        state: Arc::clone(&state),
        wake_sent: false,
    });
    supervisor.state.changed.notify_all();
    Ok(OwnedWorker {
        ticket: WorkerTicket {
            id,
            state,
            supervisor: Arc::clone(&supervisor.state),
        },
    })
}

pub fn registered_worker_count() -> usize {
    match SUPERVISOR.get() {
        Some(Ok(supervisor)) => supervisor
            .state
            .registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .records
            .len(),
        _ => 0,
    }
}

fn supervise(state: Arc<SupervisorState>) {
    loop {
        let mut registry = state.registry.lock().unwrap_or_else(|e| e.into_inner());
        let mut wakes = Vec::new();
        let mut index = 0;
        while index < registry.records.len() {
            let record = &mut registry.records[index];
            if record.state.stop.is_stopped() {
                if !record.wake_sent {
                    record.wake_sent = true;
                    wakes.push(Arc::clone(&record.state.wake));
                }
                if record.kind == WorkerKind::SynchronousIo {
                    cancel_synchronous_io(&record.handle);
                }
            }
            if !record.handle.is_finished() {
                index += 1;
                continue;
            }
            let record = registry.records.swap_remove(index);
            // Only this service joins; is_finished makes this a non-stalling join.
            let exit = if record.handle.join().is_ok() {
                WorkerExit::Joined
            } else {
                WorkerExit::Panicked
            };
            *record.state.exit.lock().unwrap_or_else(|e| e.into_inner()) = Some(exit);
            record.state.changed.notify_all();
        }
        if !wakes.is_empty() {
            drop(registry);
            // Parent cancellation must also wake an idle cooperative reader.
            // Never execute a callback under the supervisor registry lock.
            for wake in wakes {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| wake()));
            }
            continue;
        }
        if registry.records.is_empty() {
            drop(
                state
                    .changed
                    .wait(registry)
                    .unwrap_or_else(|e| e.into_inner()),
            );
        } else {
            drop(
                state
                    .changed
                    .wait_timeout(registry, SUPERVISOR_INTERVAL)
                    .unwrap_or_else(|e| e.into_inner())
                    .0,
            );
        }
    }
}

#[cfg(windows)]
fn cancel_synchronous_io(handle: &JoinHandle<()>) {
    use std::os::windows::io::AsRawHandle;
    // SAFETY: the supervisor owns this live thread handle until its sole join.
    // ERROR_NOT_FOUND is normal in the check-to-I/O window; the next pass retries.
    unsafe {
        windows_sys::Win32::System::IO::CancelSynchronousIo(handle.as_raw_handle());
    }
}

#[cfg(not(windows))]
fn cancel_synchronous_io(_handle: &JoinHandle<()>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timed_out_join_retains_ownership_and_later_observes_exit() {
        let (release, wait) = std::sync::mpsc::channel();
        let owner = spawn_owned(
            "join-retention-test",
            WorkerKind::Cooperative,
            StopToken::default(),
            || {},
            move |_| {
                let _ = wait.recv();
            },
        )
        .unwrap();
        let ticket = owner.ticket();
        drop(owner);
        assert_eq!(
            ticket.join_until(Instant::now()),
            Err(JoinDeadline {
                worker_id: ticket.id()
            })
        );
        release.send(()).unwrap();
        assert_eq!(
            ticket.join_until(Instant::now() + Duration::from_secs(2)),
            Ok(WorkerExit::Joined)
        );
    }

    #[test]
    fn parent_stop_reaches_nested_operation_without_stopping_parent_from_child() {
        let root = StopToken::default();
        let first = root.child();
        let second = root.child().child();
        first.stop();
        assert!(!root.is_stopped());
        assert!(!second.is_stopped());
        root.stop();
        assert!(second.is_stopped());
    }
}

#[cfg(test)]
mod parent_wake_tests {
    use super::*;

    #[test]
    fn parent_cancellation_wakes_an_idle_cooperative_worker() {
        let root = StopToken::default();
        let (wake, wait) = std::sync::mpsc::sync_channel(1);
        let worker = spawn_owned(
            "parent-wake-test",
            WorkerKind::Cooperative,
            root.child(),
            move || {
                let _ = wake.try_send(());
            },
            move |_| {
                let _ = wait.recv();
            },
        )
        .unwrap();
        root.stop(); // intentionally NOT worker.ticket().cancel()
        assert_eq!(
            worker
                .ticket()
                .join_until(Instant::now() + Duration::from_secs(2)),
            Ok(WorkerExit::Joined)
        );
    }
}
