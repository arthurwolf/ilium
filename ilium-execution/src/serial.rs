//! Typed custody for accepted I/O operations after their serial adapter drops.
//! The existing I/O workers advance these groups; no callback waits for a job.
use std::any::{Any, TypeId};
use std::collections::VecDeque;
use std::mem::size_of;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::budget::{JobCost, StorageAdmission};
use crate::job::{Job, JobOutcome, JobPoll, Receipt, Retained, Retention};
use crate::pool::Reservation;
use crate::RejectReason;

/// An accepted original and its original, still-unsubmitted bank reservation.
pub struct SerialQueued<J: Job> {
    pub id: u64,
    pub job: J,
    pub cost: JobCost,
    pub reservation: Reservation,
}

/// The only submitted operation of a detached serial group.
pub struct SerialRunning<J: Job> {
    pub id: u64,
    pub cost: JobCost,
    pub receipt: Receipt<J>,
}

/// Why the bank stopped advancing a detached group. A non-successful result
/// never authorizes a younger write to overtake it.
pub enum SerialFailure<J: Job> {
    Outcome {
        id: u64,
        outcome: Retained<JobOutcome<J>>,
    },
    Publication {
        id: u64,
        reason: RejectReason,
        job: Retained<J>,
    },
    Lost {
        id: u64,
    },
    Cancelled,
    ReadinessPanicked {
        id: u64,
    },
    OutputDropPanicked {
        id: u64,
    },
}

/// Explicitly transferred recovery. Younger queued originals keep their
/// reservations; a failed publication keeps the original debit via Retained.
pub struct SerialRecovered<J: Job> {
    pub failure: SerialFailure<J>,
    pub queued: VecDeque<SerialQueued<J>>,
    // Destroy every original before releasing the group metadata lease.
    _storage: StorageAdmission,
}

struct ErasedRecovery {
    payload_type: TypeId,
    reserved: usize,
    value: Box<dyn Any + Send>,
}

/// Taking a recovery record is intentional ownership transfer. Dropping this
/// ticket before take returns the same typed originals to bank custody.
#[must_use]
pub struct SerialRecoveryTicket<J: Job> {
    recovered: Option<SerialRecovered<J>>,
    registry: Arc<SerialRegistry>,
}
impl<J: Job> SerialRecoveryTicket<J> {
    pub fn take(mut self) -> SerialRecovered<J> {
        let recovered = self.recovered.take().expect("live serial recovery ticket");
        self.registry
            .recovery_reserved
            .fetch_sub(recovered.queued.len(), Ordering::AcqRel);
        recovered
    }
}
impl<J: Job> Drop for SerialRecoveryTicket<J> {
    fn drop(&mut self) {
        if let Some(recovered) = self.recovered.take() {
            let reserved = recovered.queued.len();
            self.registry.put_back(ErasedRecovery {
                payload_type: TypeId::of::<J>(),
                reserved,
                value: Box::new(recovered),
            });
        }
    }
}

struct SerialState<J: Job> {
    queued: VecDeque<SerialQueued<J>>,
    running: Option<SerialRunning<J>>,
    ready: fn(&J) -> bool,
    storage: Option<StorageAdmission>,
}

enum Advance {
    Active,
    Complete,
    Failed(ErasedRecovery),
}

trait SerialTask: Send + Sync {
    fn advance(&self, cancelling: bool) -> Advance;
}

struct Claim<'a>(&'a AtomicBool);
impl Drop for Claim<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

struct SerialGroup<J: Job> {
    // Registered and charged at admission. Only Drop arms it, so a live
    // adapter remains the sole authority to reconcile and publish writes.
    armed: AtomicBool,
    claimed: AtomicBool,
    state: Mutex<Option<SerialState<J>>>,
}
impl<J: Job> SerialGroup<J> {
    fn failure(state: &mut SerialState<J>, failure: SerialFailure<J>) -> Advance {
        let recovered = SerialRecovered {
            failure,
            queued: std::mem::take(&mut state.queued),
            _storage: state.storage.take().expect("charged serial group"),
        };
        Advance::Failed(ErasedRecovery {
            payload_type: TypeId::of::<J>(),
            reserved: recovered.queued.len(),
            value: Box::new(recovered),
        })
    }
}
impl<J: Job> SerialTask for SerialGroup<J> {
    fn advance(&self, cancelling: bool) -> Advance {
        if !self.armed.load(Ordering::Acquire) {
            return Advance::Active;
        }
        if self
            .claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Advance::Active;
        }
        let _claim = Claim(&self.claimed);
        let mut state_guard = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let Some(state) = state_guard.as_mut() else {
            // The worker that resolved this group alone publishes its terminal
            // record into the registry. Another worker must not remove it first.
            return Advance::Active;
        };
        if let Some(mut running) = state.running.take() {
            match running.receipt.try_take() {
                JobPoll::Pending => {
                    state.running = Some(running);
                    return Advance::Active;
                }
                JobPoll::Ready(outcome) => {
                    if !matches!(outcome.view(), JobOutcome::Finished(Ok(_))) {
                        let failed = Self::failure(
                            state,
                            SerialFailure::Outcome {
                                id: running.id,
                                outcome,
                            },
                        );
                        *state_guard = None;
                        return failed;
                    }
                    // Arbitrary domain output destructors do not run under the
                    // serial group mutex. The claim still excludes peer scans.
                    let running_id = running.id;
                    drop(state_guard);
                    let dropped = catch_unwind(AssertUnwindSafe(|| drop(outcome)));
                    drop(running);
                    let drop_panicked = match dropped {
                        Ok(()) => false,
                        Err(payload) => {
                            drop(payload);
                            true
                        }
                    };
                    state_guard = self.state.lock().unwrap_or_else(|error| error.into_inner());
                    if drop_panicked {
                        let state = state_guard
                            .as_mut()
                            .expect("claimed serial group retains state");
                        let failed = Self::failure(
                            state,
                            SerialFailure::OutputDropPanicked { id: running_id },
                        );
                        *state_guard = None;
                        return failed;
                    }
                }
                JobPoll::Lost | JobPoll::Taken => {
                    let failed = Self::failure(state, SerialFailure::Lost { id: running.id });
                    *state_guard = None;
                    return failed;
                }
            }
        }
        let state = state_guard
            .as_mut()
            .expect("claimed serial group retains state");
        if cancelling && !state.queued.is_empty() {
            let failed = Self::failure(state, SerialFailure::Cancelled);
            *state_guard = None;
            return failed;
        }
        let Some(queued) = state.queued.pop_front() else {
            *state_guard = None;
            return Advance::Complete;
        };
        let ready = state.ready;
        // The group claim is held, but neither bank nor group mutex may be
        // held while calling adapter readiness code or submitting a job.
        drop(state_guard);
        let readiness = catch_unwind(AssertUnwindSafe(|| ready(&queued.job)));
        let mut state_guard = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let state = state_guard
            .as_mut()
            .expect("claimed serial group retains state");
        match readiness {
            Ok(false) => {
                state.queued.push_front(queued);
                Advance::Active
            }
            Err(payload) => {
                let id = queued.id;
                state.queued.push_front(queued);
                // A foreign panic payload may have its own destructor. Leave
                // every original in the group before running that destructor.
                drop(state_guard);
                drop(payload);
                let mut state_guard = self.state.lock().unwrap_or_else(|error| error.into_inner());
                let state = state_guard
                    .as_mut()
                    .expect("claimed serial group retains state");
                let failed = Self::failure(state, SerialFailure::ReadinessPanicked { id });
                *state_guard = None;
                failed
            }
            Ok(true) => {
                let SerialQueued {
                    id,
                    job,
                    cost,
                    reservation,
                } = queued;
                // If publication violates its pre-admitted queue invariant,
                // the cloned original hold preserves input/job debit.
                let retention: Retention = reservation.retention();
                drop(state_guard);
                let publication = reservation.submit(job);
                let mut state_guard = self.state.lock().unwrap_or_else(|error| error.into_inner());
                let state = state_guard
                    .as_mut()
                    .expect("claimed serial group retains state");
                match publication {
                    Ok(receipt) => {
                        state.running = Some(SerialRunning { id, cost, receipt });
                        drop(retention);
                        Advance::Active
                    }
                    Err(rejection) => {
                        let failed = Self::failure(
                            state,
                            SerialFailure::Publication {
                                id,
                                reason: rejection.reason,
                                job: retention.retain(rejection.value),
                            },
                        );
                        *state_guard = None;
                        failed
                    }
                }
            }
        }
    }
}

#[derive(Default)]
struct RegistryState {
    active: Vec<Arc<dyn SerialTask>>,
    recovery: VecDeque<ErasedRecovery>,
}

pub(crate) struct SerialRegistry {
    state: Mutex<RegistryState>,
    recovery_reserved: AtomicUsize,
    completed: AtomicUsize,
}
impl SerialRegistry {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(RegistryState::default()),
            recovery_reserved: AtomicUsize::new(0),
            completed: AtomicUsize::new(0),
        })
    }
    fn add(&self, group: Arc<dyn SerialTask>) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .active
            .push(group);
    }
    fn discard(&self, group: &Arc<dyn SerialTask>) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(position) = state
            .active
            .iter()
            .position(|entry| Arc::ptr_eq(entry, group))
        {
            state.active.swap_remove(position);
        }
    }
    fn put_back(&self, failure: ErasedRecovery) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .recovery
            .push_back(failure);
    }
    pub(crate) fn advance(&self, cancelling: bool) {
        let mut index = 0;
        loop {
            let group = {
                let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
                state.active.get(index).cloned()
            };
            let Some(group) = group else { return };
            let result = group.advance(cancelling);
            if matches!(result, Advance::Active) {
                index += 1;
                continue;
            }
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            if let Some(position) = state
                .active
                .iter()
                .position(|entry| Arc::ptr_eq(entry, &group))
            {
                state.active.swap_remove(position);
                if let Advance::Failed(failure) = result {
                    self.recovery_reserved
                        .fetch_add(failure.reserved, Ordering::AcqRel);
                    state.recovery.push_back(failure);
                } else {
                    self.completed.fetch_add(1, Ordering::AcqRel);
                }
            }
            // The swapped entry at this index also needs inspection.
        }
    }
    pub(crate) fn active(&self) -> usize {
        // A live adapter may hold the last completed but still-unobserved
        // receipt after pending slots reach zero. Keep workers alive until
        // that adapter reconciles or transfers its registered custody.
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .active
            .len()
    }
    pub(crate) fn recovery_pending(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .recovery
            .len()
    }
    pub(crate) fn recovery_reserved(&self) -> usize {
        self.recovery_reserved.load(Ordering::Acquire)
    }
    pub(crate) fn completed(&self) -> usize {
        self.completed.load(Ordering::Acquire)
    }
    pub(crate) fn try_take_recovery<J: Job>(self: &Arc<Self>) -> Option<SerialRecoveryTicket<J>> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let position = state
            .recovery
            .iter()
            .position(|entry| entry.payload_type == TypeId::of::<J>())?;
        let erased = state.recovery.remove(position)?;
        let recovered = match erased.value.downcast::<SerialRecovered<J>>() {
            Ok(recovered) => recovered,
            Err(value) => {
                // Even an internal type-tag defect cannot destroy the exact
                // original or its reservation during an attempted readback.
                state.recovery.insert(
                    position,
                    ErasedRecovery {
                        payload_type: erased.payload_type,
                        reserved: erased.reserved,
                        value,
                    },
                );
                return None;
            }
        };
        Some(SerialRecoveryTicket {
            recovered: Some(*recovered),
            registry: Arc::clone(self),
        })
    }
}

/// Precharged metadata lease. It is acquired before a writer's first accepted
/// original and transferred, without a fallible step, at adapter Drop.
pub struct SerialCustody<J: Job> {
    registry: Arc<SerialRegistry>,
    group: Arc<SerialGroup<J>>,
    storage: Option<StorageAdmission>,
    detached: bool,
}
impl<J: Job> SerialCustody<J> {
    pub(crate) fn metadata_bytes(max_writes: usize) -> Result<usize, RejectReason> {
        // JobCost already covers J itself; these bytes cover serial node and
        // registry overhead, including one typed exceptional recovery record.
        let per_entry = size_of::<SerialQueued<J>>()
            .saturating_sub(size_of::<J>())
            .checked_add(32)
            .ok_or(RejectReason::InvalidCost)?;
        max_writes
            .checked_mul(per_entry)
            .and_then(|bytes| bytes.checked_add(size_of::<SerialGroup<J>>() + 512))
            .ok_or(RejectReason::InvalidCost)
    }
    pub(crate) fn new(registry: Arc<SerialRegistry>, storage: StorageAdmission) -> Self {
        let group = Arc::new(SerialGroup {
            armed: AtomicBool::new(false),
            claimed: AtomicBool::new(false),
            state: Mutex::new(None),
        });
        registry.add(Arc::clone(&group) as Arc<dyn SerialTask>);
        Self {
            registry,
            group,
            storage: Some(storage),
            detached: false,
        }
    }
    pub fn detach(
        mut self,
        queued: VecDeque<SerialQueued<J>>,
        running: Option<SerialRunning<J>>,
        ready: fn(&J) -> bool,
    ) {
        if queued.is_empty() && running.is_none() {
            return;
        }
        *self
            .group
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(SerialState {
            queued,
            running,
            ready,
            storage: self.storage.take(),
        });
        self.group.armed.store(true, Ordering::Release);
        self.detached = true;
    }
}
impl<J: Job> Drop for SerialCustody<J> {
    fn drop(&mut self) {
        if !self.detached {
            self.registry
                .discard(&(Arc::clone(&self.group) as Arc<dyn SerialTask>));
        }
    }
}
