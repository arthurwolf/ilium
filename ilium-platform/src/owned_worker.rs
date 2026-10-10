//! Join ownership for PTY workers, including synchronous Windows I/O pumps.
//!
//! Cancellation has two distinct outcomes: observed thread exit, or a deadline
//! with the JoinHandle still owned here. Never detach a stuck pump and never
//! kill an OS thread. A bounded process-lifetime supervisor keeps retrying
//! Windows cancellation and joins only handles whose `is_finished()` is true.

use std::io;
use std::mem::size_of; // Describe requested Rust metadata without claiming native allocation bounds.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex}; // Serialize retryable initialization of the one supervisor.
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Sized for well over 1024 PTY panes (five or six workers each) plus the
/// execution banks; the registry is preallocated, so this is a fixed, small
/// table. This slot count is the only ceiling on the number of PTY panes.
pub const MAX_OWNED_WORKERS: usize = 8192;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)] // A request declaration is cheap to inspect.
pub struct WorkerMetadata {
    // Describe the original worker without cloning its captures.
    pub worker_id: u64,   // Preserve the monotone registry identity.
    pub kind: WorkerKind, // Record the caller's cancellation category.
    pub requested_stack_bytes: Option<usize>, // None means the platform default, never zero bytes.
} // End the immutable worker declaration.
  //
struct WorkerCompletion {
    // Completion must survive destruction of record-owned wake captures.
    exit: Mutex<Option<WorkerExit>>, // Publish only after native join and custody retirement.
    changed: Condvar, // Wake existing blocking observers without another helper thread.
    on_exit: Option<Arc<dyn Fn() + Send + Sync>>, // One finite hint only after physical join.
} // End the completion cell.
  //
struct WorkerState {
    stop: StopToken,
    wake: Arc<dyn Fn() + Send + Sync>,
    completion: Arc<WorkerCompletion>, // Observation does not own the physical custody.
    metadata: WorkerMetadata, // Preserve explicit/default stack provenance on surviving tickets.
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

    pub fn metadata(&self) -> WorkerMetadata {
        // Inspect the original request without native work.
        self.state.metadata // Return the immutable declaration, not an allocation measurement.
    } // End declaration inspection.
      //
    pub fn exit(&self) -> Option<WorkerExit> {
        *self
            .state
            .completion
            .exit
            .lock()
            .unwrap_or_else(|e| e.into_inner()) // Observe actual retirement.
    }

    /// Blocking wait, for a dedicated thread or Tokio `spawn_blocking` only.
    pub fn join_until(&self, deadline: Instant) -> Result<WorkerExit, JoinDeadline> {
        let mut exit = self
            .state
            .completion
            .exit
            .lock()
            .unwrap_or_else(|e| e.into_inner()); // Lock only completion.
        loop {
            if let Some(observed) = *exit {
                // Publication follows native join, custody destruction, and registry retirement.
                return Ok(observed);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(JoinDeadline { worker_id: self.id });
            }
            exit = self
                .state
                .completion // Wait independently of the native join and registry lock.
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
    custody: Option<Box<dyn Send>>, // New explicit physical leases live through actual native join.
}

struct Registry {
    next_id: u64,
    records: Vec<Record>,
    reserved: usize, // Unspawned reservations compete with every live or retiring native owner.
    joining: Option<WorkerMetadata>, // A handle being joined outside the lock still occupies its slot.
}

struct SupervisorState {
    registry: Mutex<Registry>,
    changed: Condvar,
}

struct Supervisor {
    state: Arc<SupervisorState>,
    // An intentionally process-lifetime service, not a detached per-pane task.
    _join: Mutex<JoinHandle<()>>,
    wake_capacity: usize, // Expose the preallocated wake vector capacity for qualification.
}

static SUPERVISOR: Mutex<Option<Arc<Supervisor>>> = Mutex::new(None); // Failed starts remain retryable.
                                                                      //
/// Requested supervisor stack; native guards, rounding, TLS, and RSS remain unmeasured.
pub fn supervisor_stack_bytes() -> usize {
    // Use one explicit bootstrap request on every platform.
    2 * 1024 * 1024 // Two MiB is a requested stack size, not a native allocation ceiling.
} // End the stack declaration.
  //
/// Declared observation state for one worker with one fresh root StopToken.
/// Callers separately charge wake/custody payloads and retain this declaration
/// through their last ticket. Parent stop chains and native Builder, thread,
/// allocator and Arc-layout overhead are not bounded by this allowance.
/// Registry records already belong to supervisor_declared_bytes.
pub fn worker_retained_state_bytes() -> usize {
    size_of::<WorkerState>()
        + size_of::<WorkerCompletion>()
        + size_of::<StopState>()
        + 6 * size_of::<usize>()
}

/// Requested permanent registry/wake storage plus the explicit supervisor stack.
/// Per-worker state, arbitrary captures, retained tickets, and native overhead are separate.
pub fn supervisor_declared_bytes() -> usize {
    // Use Rust layouts rather than an invented per-slot size.
    supervisor_stack_bytes() // Account the explicit stack request once per designated process root.
        + size_of::<Supervisor>() // Include the permanent supervisor container.
        + size_of::<SupervisorState>() // Include its mutex, registry header, and condition variable.
        + 4 * size_of::<usize>() // Allow for Arc bookkeeping; its allocation header has no public layout guarantee.
        + size_of::<Vec<Arc<dyn Fn() + Send + Sync>>>() // Include the permanent wake-vector header.
        + MAX_OWNED_WORKERS * size_of::<Record>() // Declare the requested fixed registry entries.
        + MAX_OWNED_WORKERS * size_of::<Arc<dyn Fn() + Send + Sync>>() // Declare reusable wake slots.
} // End the cooperative permanent-storage declaration.
  //
#[derive(Debug, Clone, Copy)] // Return one bounded observation without allocating an inventory.
pub struct SupervisorSnapshot {
    // Separate the actual singleton from independent quota ledgers.
    pub thread_id: std::thread::ThreadId, // Identify the one actual supervisor thread.
    pub registered_workers: usize,        // Include the native handle currently being joined.
    pub reserved_workers: usize, // Include unpublished reservations and native spawn/publication in progress.
    pub joining_worker: Option<WorkerMetadata>, // Expose native retirement without claiming progress.
    pub registry_capacity: usize, // Report Vec capacity rather than inventing allocator measurements.
    pub wake_capacity: usize,     // Report reusable wake capacity independently of the native heap.
    pub requested_stack_bytes: usize, // Report the explicit Builder request.
} // End the process-supervisor observation.
  //
pub fn supervisor_status() -> Option<SupervisorSnapshot> {
    // Observation never starts a supervisor.
    let installed = SUPERVISOR.lock().unwrap_or_else(|error| error.into_inner()); // Serialize publication.
    let supervisor = installed.as_ref()?; // Return absence before the first successful native spawn.
    let registry = supervisor
        .state
        .registry
        .lock()
        .unwrap_or_else(|error| error.into_inner()); // Snapshot slots.
    let thread_id = supervisor
        ._join
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .thread()
        .id(); // Stable identity.
    Some(SupervisorSnapshot {
        // Return metadata only; do not clone worker captures or handles.
        thread_id, // Preserve the actual singleton identity across all quota groups.
        registered_workers: registry.records.len() + usize::from(registry.joining.is_some()), // Count retirement.
        reserved_workers: registry.reserved, // Include slots not yet published, even during native spawn.
        joining_worker: registry.joining,    // Preserve the exact joining worker declaration.
        registry_capacity: registry.records.capacity(), // Expose actual Rust vector capacity.
        wake_capacity: supervisor.wake_capacity, // Expose the other permanent vector capacity.
        requested_stack_bytes: supervisor_stack_bytes(), // Do not substitute this value for measured RSS.
    }) // Complete the observation while the registry is coherent.
} // End non-creating supervisor inspection.
  //
/// Bootstrap only; true means this call started the singleton, false means reuse.
/// This platform primitive installs no QuotaGroup and cannot pin a fixture's ledger.
pub fn initialize_supervisor() -> io::Result<bool> {
    // Permit explicit admission before the first spawn.
    supervisor().map(|(_, started)| started) // Preserve exact cold-versus-existing provenance.
} // End explicit platform bootstrap.
  //
fn supervisor() -> io::Result<(Arc<Supervisor>, bool)> {
    // All legacy and admitted owners share this gate.
    let mut installed = SUPERVISOR.lock().unwrap_or_else(|error| error.into_inner()); // Serialize native creation.
    if let Some(supervisor) = installed.as_ref() {
        // Reuse the one published process-lifetime service.
        return Ok((Arc::clone(supervisor), false)); // Never spawn another service for another quota group.
    } // Finish the already-started guard.
    let mut records = Vec::new(); // Build fallible permanent storage before starting any native service.
    records
        .try_reserve_exact(MAX_OWNED_WORKERS)
        .map_err(io::Error::other)?; // Remove post-spawn registry growth.
    let mut wakes = Vec::new(); // Reuse this wake batch for the entire supervisor lifetime.
    wakes
        .try_reserve_exact(MAX_OWNED_WORKERS)
        .map_err(io::Error::other)?; // Remove allocations from wake scans.
    let wake_capacity = wakes.capacity(); // Record the actual requested-vector result for observation.
    let state = Arc::new(SupervisorState {
        // Construct the one shared platform registry.
        registry: Mutex::new(Registry {
            // Keep reservations and native retirement under one slot cap.
            next_id: 1, // Preserve monotone worker identities without reusing rejected identities.
            records,    // Transfer the preallocated native-handle storage.
            reserved: 0, // No caller has a pre-capture reservation yet.
            joining: None, // No native join is in progress during construction.
        }), // Finish registry construction before native spawn.
        changed: Condvar::new(), // Preserve the original bounded polling and wake mechanism.
    }); // Finish the process-lifetime registry allocation.
    let worker_state = Arc::clone(&state); // Give the supervisor its stable registry ownership.
    let join = std::thread::Builder::new() // Create only the original single supervisor service.
        .name("ilium-pty-joins".into()) // Preserve the existing native thread name.
        .stack_size(supervisor_stack_bytes()) // Apply the explicit, separately declared stack request.
        .spawn(move || supervise(worker_state, wakes))?; // A failure leaves the installation slot empty.
    let supervisor = Arc::new(Supervisor {
        // Retain the actual native handle for process lifetime.
        state,                   // Own the registry independently of any fixture QuotaGroup.
        _join: Mutex::new(join), // Do not detach the singleton's native handle.
        wake_capacity,           // Retain observable metadata for the permanent wake allocation.
    }); // Complete successful native service construction.
    *installed = Some(Arc::clone(&supervisor)); // Publish only after the native spawn succeeds.
    Ok((supervisor, true)) // Report that this call performed the one native startup.
} // End retryable singleton initialization.
  //
#[must_use] // Dropping this value releases an unused slot and its original custody.
pub struct WorkerReservation<C: Send + 'static> {
    // Admit before constructing expensive captures.
    supervisor: Arc<SupervisorState>, // Keep the shared slot ledger alive through publication or refusal.
    id: u64,                          // Fix worker identity before native spawn.
    stack_bytes: Option<usize>,       // Preserve default versus explicit stack intent.
    pending: bool,                    // Ensure an unused registry slot is released exactly once.
    custody: Option<C>, // Transfer this physical lease into the native record on success.
} // End the pre-spawn ownership reservation.
  //
/// Bootstrap/background only; reserve before building captures, channels, or native work.
/// Custody destruction must be bounded and nonpanicking; it always runs outside registry locks.
pub fn reserve_owned_worker<C: Send + 'static>(
    // Keep platform independent of concrete quota types.
    stack_bytes: Option<usize>, // None preserves the old default-stack caller contract.
    custody: C,                 // The caller has already obtained any required physical admission.
) -> io::Result<WorkerReservation<C>> {
    // Refusal creates no child thread and releases the supplied custody.
    if stack_bytes == Some(0) {
        // Reject an invalid explicit declaration before creating the singleton.
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "zero owned-worker stack request",
        )); // Fail clearly.
    } // Finish stack validation.
    let (supervisor, _) = supervisor()?; // Reuse the same singleton for legacy and admitted paths.
    let mut registry = supervisor
        .state
        .registry
        .lock()
        .unwrap_or_else(|error| error.into_inner()); // Admit slots.
    let occupied =
        registry.records.len() + registry.reserved + usize::from(registry.joining.is_some()); // All custody.
    if occupied >= MAX_OWNED_WORKERS {
        // Retiring and unspawned owners cannot evade the fixed cap.
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "PTY worker ownership capacity exhausted",
        )); // Refuse.
    } // Finish bounded slot admission.
    let id = registry.next_id; // Reserve the next original worker identity.
    registry.next_id = id
        .checked_add(1)
        .ok_or_else(|| io::Error::other("PTY worker identity exhausted"))?; // No wrap.
    registry.reserved += 1; // Charge the slot before the caller constructs its expensive captures.
    Ok(WorkerReservation {
        // Return a reversible reservation, without spawning a child body.
        supervisor: Arc::clone(&supervisor.state), // Keep the exact admitting registry.
        id,                                        // Preserve the original reserved identity.
        stack_bytes,                               // Preserve the requested native stack policy.
        pending: true, // Drop must release the slot until successful native publication.
        custody: Some(custody), // Keep the caller's original lease without invoking it.
    }) // The short registry guard is destroyed before the caller receives its reservation.
} // End slot and lifetime admission.
  //
impl<C: Send + 'static> WorkerReservation<C> {
    // One reservation starts at most one native worker.
    pub fn spawn(
        // Keep the original public spawn contract for existing owners.
        self,
        name: &str,
        kind: WorkerKind,
        stop: StopToken,
        wake: impl Fn() + Send + Sync + 'static,
        body: impl FnOnce(StopToken) + Send + 'static,
    ) -> io::Result<OwnedWorker> {
        self.spawn_with_completion(name, kind, stop, wake, body, None)
    }

    /// The supervisor invokes this optional hint only after the native join and
    /// custody retirement have been published.
    pub fn spawn_with_completion(
        mut self,
        name: &str,
        kind: WorkerKind,
        stop: StopToken,
        wake: impl Fn() + Send + Sync + 'static,
        body: impl FnOnce(StopToken) + Send + 'static,
        on_exit: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> io::Result<OwnedWorker> {
        // Native failure leaves no registered handle and releases the unused slot.
        if name.as_bytes().contains(&0) {
            // Prevent Builder's invalid-name panic before handing over captures.
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "owned-worker name contains NUL",
            )); // Refuse safely.
        } // Finish native-name validation.
        let custody = self
            .custody
            .take()
            .map(|value| Box::new(value) as Box<dyn Send>); // Allocate before native spawn.
        let state = Arc::new(WorkerState {
            // Build observation and wake state after admission.
            stop: stop.clone(),   // Preserve the exact cancellation token graph.
            wake: Arc::new(wake), // Retain legacy capture semantics through surviving caller tickets.
            completion: Arc::new(WorkerCompletion {
                // Separate final observation from wake ownership.
                exit: Mutex::new(None), // Callback return cannot publish a joined outcome.
                changed: Condvar::new(), // Reuse the existing background join-wait interface.
                on_exit, // Keep an optional hint outside the native worker's physical custody.
            }), // Finish independent completion state.
            metadata: WorkerMetadata {
                // Describe the request actually used for this native body.
                worker_id: self.id, // Preserve the reserved identity.
                kind,               // Preserve its cancellation policy.
                requested_stack_bytes: self.stack_bytes, // Never label a default stack as zero.
            }, // Finish immutable worker metadata.
        }); // Finish pre-spawn allocations for the worker record.
        let notify = Arc::clone(&self.supervisor); // Keep completion wake ownership without a second service.
        let mut builder = std::thread::Builder::new().name(name.into()); // Preserve native naming.
        if let Some(bytes) = self.stack_bytes {
            // Apply an explicit request only for callers opting into it.
            builder = builder.stack_size(bytes); // Keep all legacy default-stack callers unchanged.
        } // Finish builder configuration.
        let handle = builder.spawn(move || {
            // Spawn outside the global registry lock with a reserved slot.
            body(stop); // Run the original body with its original cancellation token.
            notify.changed.notify_all(); // Prompt ordinary retirement; panic retirement still uses the interval.
        })?; // On native error, local custody and the still-pending reservation are both released.
        let mut registry = self
            .supervisor
            .registry
            .lock()
            .unwrap_or_else(|error| error.into_inner()); // Publish handle.
        registry.records.push(Record {
            // Preallocated capacity makes this publication allocation-free.
            handle,                    // Transfer the sole JoinHandle into supervisor custody.
            kind,                      // Preserve Windows synchronous-I/O cancellation behavior.
            state: Arc::clone(&state), // Keep wake and observation state until retirement.
            wake_sent: false,          // Parent cancellation has not yet sent its first wake.
            custody, // Retain new physical leases independently of informational tickets.
        }); // Complete ownership publication before converting the reservation count.
        registry.reserved -= 1; // Replace the reserved slot with exactly one registered native owner.
        self.pending = false; // Prevent Drop from releasing the now-registered slot.
        drop(registry); // Never execute caller code or custody destruction under the registry mutex.
        self.supervisor.changed.notify_all(); // Wake the one supervisor after publication.
        Ok(OwnedWorker {
            // Return logical cancellation ownership only after native custody is secure.
            ticket: WorkerTicket {
                // Retain the existing public ticket interface.
                id: self.id, // Preserve the reserved worker identity.
                state,       // Keep the caller's wake and completion observation.
                supervisor: Arc::clone(&self.supervisor), // Route cancellation to the same singleton.
            }, // Finish the original logical ticket.
        }) // Complete successful ownership transfer.
    } // End one-shot native spawning.
} // End reservation publication methods.
  //
impl<C: Send + 'static> Drop for WorkerReservation<C> {
    // Roll back only unconsumed admissions.
    fn drop(&mut self) {
        // Never join or run native work on a dropping caller.
        if !self.pending {
            // A successfully registered handle now owns the slot.
            return; // Leave native retirement exclusively to the supervisor.
        } // Finish the consumed-reservation guard.
        let mut registry = self
            .supervisor
            .registry
            .lock()
            .unwrap_or_else(|error| error.into_inner()); // Release slot.
        registry.reserved -= 1; // Match the one successful slot admission.
        drop(registry); // Field destruction may release quota and invoke its wake outside this lock.
        self.supervisor.changed.notify_all(); // Prompt observers after unused capacity becomes available.
    } // Custody is destroyed after this method returns with no registry lock held.
} // End unused-reservation rollback.
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
    reserve_owned_worker(None, ())?.spawn(name, kind, stop, wake, body) // Preserve every legacy caller signature.
}

pub fn spawn_owned_with_completion(
    name: &str,
    kind: WorkerKind,
    stop: StopToken,
    wake: impl Fn() + Send + Sync + 'static,
    body: impl FnOnce(StopToken) + Send + 'static,
    on_exit: Arc<dyn Fn() + Send + Sync>,
) -> io::Result<OwnedWorker> {
    reserve_owned_worker(None, ())?.spawn_with_completion(
        name,
        kind,
        stop,
        wake,
        body,
        Some(on_exit),
    )
}

pub fn registered_worker_count() -> usize {
    supervisor_status().map_or(0, |snapshot| snapshot.registered_workers) // Count a handle even during native join.
}

fn supervise(state: Arc<SupervisorState>, mut wakes: Vec<Arc<dyn Fn() + Send + Sync>>) {
    // One reused wake batch.
    loop {
        // Keep the original single process-lifetime retirement owner.
        let mut registry = state
            .registry
            .lock()
            .unwrap_or_else(|error| error.into_inner()); // Inspect bounded records.
        let mut ready = None; // Select at most one native join without allocating a retirement queue.
        for (index, record) in registry.records.iter_mut().enumerate() {
            // Scan only the admitted records.
            if record.handle.is_finished() {
                // Main-body completion is a hint, not proof of finished native TLS.
                if ready.is_none() {
                    // Preserve one sole native join owner.
                    ready = Some(index); // Retain this exact original record for retirement.
                } // Finish first-candidate selection.
                continue; // Never retain this finished record's wake in the reusable batch.
            } // Finish the body-completion branch.
            if !record.state.stop.is_stopped() {
                // Running uncancelled workers require no callback.
                continue; // Keep cancellation bookkeeping bounded by the admitted record count.
            } // Finish the live-worker guard.
            if !record.wake_sent {
                // Parent cancellation sends one cooperative wake.
                record.wake_sent = true; // Mark before invoking the callback outside the lock.
                wakes.push(Arc::clone(&record.state.wake)); // The preallocated vector cannot grow past the cap.
            } // Finish one-time cooperative notification.
            if record.kind == WorkerKind::SynchronousIo {
                // Preserve the existing native cancellation category.
                cancel_synchronous_io(&record.handle); // Retry while this supervisor is making progress.
            } // Finish synchronous-I/O cancellation for this scan.
        } // Finish the bounded registry pass.
        let joining = ready.map(|index| {
            // Remove one handle while preserving its occupied slot.
            let record = registry.records.swap_remove(index); // No allocation or native join occurs here.
            registry.joining = Some(record.state.metadata); // Continue counting this original native owner.
            record // Transfer the sole JoinHandle into this supervisor's local custody.
        }); // Finish selection without releasing physical admission.
        if joining.is_none() && wakes.is_empty() {
            // Preserve the original idle/active wait distinction.
            if registry.records.is_empty() {
                // Unspawned reservations do not require native polling.
                drop(
                    state
                        .changed
                        .wait(registry)
                        .unwrap_or_else(|error| error.into_inner()),
                ); // Wait for publication.
            } else {
                // Live native bodies still require bounded cancellation/retirement checks.
                drop(
                    state
                        .changed
                        .wait_timeout(registry, SUPERVISOR_INTERVAL)
                        .unwrap_or_else(|error| error.into_inner())
                        .0,
                ); // Retry.
            } // Finish the idle branch without retaining any wake capture.
            continue; // Rescan only after notification or the existing interval.
        } // Finish the no-work guard.
        drop(registry); // No caller wake, native join, or resource destructor runs under the registry lock.
        for wake in wakes.drain(..) {
            // Reuse allocated slots without retaining old wake captures.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| wake()));
            // Preserve wake panic isolation.
        } // Destroy every temporary wake before selecting another retirement.
        let Some(record) = joining else {
            // A wake-only pass has no native handle to retire.
            continue; // Return to the one registry without constructing any helper thread.
        }; // Finish the native-join guard.
        let Record {
            handle,
            state: worker_state,
            custody,
            ..
        } = record; // Keep all physical custody through join.
        let joined = handle.join().is_ok(); // Includes TLS exit.
                                            // Native TLS can park this sole supervisor here; registry access remains available, not global reaper progress.
                                            // Custody destructors are required to be nonpanicking, but a foreign
                                            // destructor must not strand the process's only join owner. A panic
                                            // becomes a failed retirement observation after the native join.
        let custody_dropped =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(custody))).is_ok();
        let completion = Arc::clone(&worker_state.completion); // Retain observation without retaining legacy wake captures.
        let state_dropped =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(worker_state))).is_ok();
        let exit = if joined && custody_dropped && state_dropped {
            WorkerExit::Joined
        } else {
            WorkerExit::Panicked
        };
        let mut registry = state
            .registry
            .lock()
            .unwrap_or_else(|error| error.into_inner()); // Retire the counted join slot.
        registry.joining = None; // Native join and resource destruction have now completed.
        *completion
            .exit
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(exit); // Publish only the proved outcome.
        completion.changed.notify_all(); // Wake existing join observers after all record custody has retired.
        state.changed.notify_all(); // Expose newly available registry capacity to later callers.
        let on_exit = completion.on_exit.as_ref().map(Arc::clone);
        drop(registry);
        if let Some(on_exit) = on_exit {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| on_exit()));
        }
    } // Release the short registry guard before beginning the next scan.
} // End the single native retirement supervisor.

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
mod supervisor_panic_retirement_tests {
    use super::*;
    use std::sync::mpsc::{self, SyncSender};

    struct PanicOnDrop(SyncSender<()>);
    impl Drop for PanicOnDrop {
        fn drop(&mut self) {
            let _ = self.0.try_send(());
            panic!("deliberate retirement destructor failure");
        }
    }

    fn assert_later_worker_joins() {
        let worker = spawn_owned(
            "after-retirement-panic",
            WorkerKind::Cooperative,
            StopToken::default(),
            || {},
            |_| {},
        )
        .expect("the singleton still accepts workers");
        let ticket = worker.ticket();
        drop(worker);
        assert_eq!(
            ticket.join_until(Instant::now() + Duration::from_secs(5)),
            Ok(WorkerExit::Joined),
            "a destructor panic must not kill the sole join supervisor"
        );
    }

    #[test]
    fn panicking_custody_does_not_strand_the_supervisor() {
        let (dropped_sender, dropped) = mpsc::sync_channel(1);
        let (release, parked) = mpsc::sync_channel(1);
        let worker = reserve_owned_worker(None, PanicOnDrop(dropped_sender))
            .expect("reserve")
            .spawn(
                "panicking-custody",
                WorkerKind::Cooperative,
                StopToken::default(),
                || {},
                move |_| {
                    let _ = parked.recv();
                },
            )
            .expect("spawn");
        drop(worker);
        release.send(()).expect("release native body");
        dropped
            .recv_timeout(Duration::from_secs(5))
            .expect("custody destructor ran");
        assert_later_worker_joins();
    }

    #[test]
    fn panicking_legacy_wake_capture_does_not_strand_the_supervisor() {
        let (dropped_sender, dropped) = mpsc::sync_channel(1);
        let (release, parked) = mpsc::sync_channel(1);
        let capture = PanicOnDrop(dropped_sender);
        let worker = spawn_owned(
            "panicking-wake-capture",
            WorkerKind::Cooperative,
            StopToken::default(),
            move || {
                let _ = &capture;
            },
            move |_| {
                let _ = parked.recv();
            },
        )
        .expect("spawn");
        drop(worker);
        release.send(()).expect("release native body");
        dropped
            .recv_timeout(Duration::from_secs(5))
            .expect("wake capture destructor ran");
        assert_later_worker_joins();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_hint_follows_physical_join_and_custody_retirement() {
        use std::sync::atomic::AtomicUsize;
        struct Admission(Arc<AtomicUsize>);
        impl Drop for Admission {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::AcqRel);
            }
        }
        let remaining = Arc::new(AtomicUsize::new(1));
        let observed = Arc::clone(&remaining);
        let (release, wait) = std::sync::mpsc::sync_channel(1);
        let (notified, notification) = std::sync::mpsc::sync_channel(1);
        let worker = reserve_owned_worker(None, Admission(remaining))
            .unwrap()
            .spawn_with_completion(
                "post-join-video-hint",
                WorkerKind::Cooperative,
                StopToken::default(),
                || {},
                move |_| {
                    let _ = wait.recv();
                },
                Some(Arc::new(move || {
                    let _ = notified.try_send(observed.load(Ordering::Acquire));
                })),
            )
            .unwrap();
        let ticket = worker.ticket();
        drop(worker);
        assert_eq!(ticket.exit(), None);
        release.send(()).unwrap();
        assert_eq!(notification.recv_timeout(Duration::from_secs(5)), Ok(0));
        assert_eq!(ticket.exit(), Some(WorkerExit::Joined));
    }

    #[test]
    fn actual_join_releases_supervisor_custody_before_last_ticket_is_dropped() {
        use std::sync::atomic::AtomicUsize;
        struct Admission(Arc<AtomicUsize>);
        impl Drop for Admission {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::AcqRel);
            }
        }
        for _ in 0..32 {
            let remaining = Arc::new(AtomicUsize::new(1));
            let admission = Admission(Arc::clone(&remaining));
            let (release, wait) = std::sync::mpsc::sync_channel(1);
            let owner = spawn_owned(
                "join-admission-test",
                WorkerKind::Cooperative,
                StopToken::default(),
                move || {
                    let _hold_until_last_owner = &admission;
                },
                move |_| {
                    wait.recv().unwrap();
                },
            )
            .unwrap();
            let ticket = owner.ticket();
            drop(owner);
            release.send(()).unwrap();
            assert_eq!(
                ticket.join_until(Instant::now() + Duration::from_secs(2)),
                Ok(WorkerExit::Joined)
            );
            assert_eq!(
                remaining.load(Ordering::Acquire),
                1,
                "caller ticket retains admission"
            );
            drop(ticket);
            assert_eq!(
                remaining.load(Ordering::Acquire),
                0,
                "no supervisor wake or record outlives join"
            );
        }
    }

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
