//! Bounded final-owner handoff onto the existing CPU bank.
//!
//! A permit is acquired before constructing a large value. Its envelope is
//! allocated at that point, so the final owner's `Drop` only moves a pointer
//! into a channel. One permit remains live until the CPU destroys the value.
use std::any::{type_name, Any};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::mem::size_of;
use std::ops::{Deref, DerefMut};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender, TrySendError};

use crate::budget::{QuotaGroup, RejectReason, StorageAdmission};
use crate::job::Retention;

pub const RETIREMENT_SLOTS: usize = 64;

/// Shared immutable leaf. The last Arc owner invokes `Retiring<T>::drop`,
/// including when that owner is a pending frame or native acknowledgement.
pub type RetiringArc<T> = Arc<Retiring<T>>;

pub(crate) type RetirementTask = dyn Any + Send;

/// One failed primary handoff keeps its exact original envelope, original
/// allocation guard, and original debit. Every entry still owns one live slot.
pub(crate) struct FailedRetirement {
    id: u64,
    payload_type: &'static str,
    task: Box<RetirementTask>,
    // Only recovery entries retain their original failed sender. A primary
    // entry carries None, so the channel never owns a clone of itself.
    retry_sender: Option<Sender<FailedRetirement>>,
}

struct RecoveryCustody {
    sender: Sender<FailedRetirement>,
    receiver: Receiver<FailedRetirement>,
    pending: AtomicUsize,
}

impl RecoveryCustody {
    fn new() -> Self {
        let (sender, receiver) = crossbeam_channel::bounded(RETIREMENT_SLOTS);
        Self {
            sender,
            receiver,
            pending: AtomicUsize::new(0),
        }
    }

    fn keep(&self, failed: FailedRetirement) {
        // Publication follows this increment; a concurrent owner can therefore
        // never observe a ticket and decrement an uncounted entry.
        self.pending.fetch_add(1, Ordering::AcqRel);
        match self.sender.try_send(failed) {
            Ok(()) => {}
            Err(TrySendError::Full(_failed) | TrySendError::Disconnected(_failed)) => {
                // Each entry retains one of 64 live slots. Full or disconnected
                // is an internal custody invariant violation. Aborting here
                // prevents destructor/debit loss onto UI.
                std::process::abort();
            }
        }
    }

    fn put_back(&self, failed: FailedRetirement) {
        // The ticket's live slot is still counted and the pending count already
        // includes it, so at most 63 other entries can occupy this 64-slot queue.
        match self.sender.try_send(failed) {
            Ok(()) => {}
            Err(TrySendError::Full(_failed) | TrySendError::Disconnected(_failed)) => {
                std::process::abort();
            }
        }
    }
}

fn rescue_orphaned_primary(
    receiver: &Receiver<FailedRetirement>,
    custody: &RecoveryCustody,
    metrics: &RetirementMetrics,
) {
    while let Ok(failed) = receiver.try_recv() {
        metrics.queued.fetch_sub(1, Ordering::AcqRel);
        custody.keep(failed);
    }
}

/// An exceptional primary-handoff result. Keep the execution/recovery owner
/// alive until this ticket has been resubmitted or taken as its exact type.
/// Dropping a ticket puts it back in finite custody without dropping payload.
#[must_use]
pub struct RetirementFailure {
    failed: Option<FailedRetirement>,
    custody: Arc<RecoveryCustody>,
    sender: Sender<FailedRetirement>,
    receiver: Receiver<FailedRetirement>,
    metrics: Arc<RetirementMetrics>,
    cpu_available: Arc<AtomicBool>,
}

impl RetirementFailure {
    pub fn id(&self) -> u64 {
        self.failed.as_ref().expect("live failure ticket").id
    }

    pub fn payload_type(&self) -> &'static str {
        self.failed
            .as_ref()
            .expect("live failure ticket")
            .payload_type
    }

    /// Return the original typed owner with all of its retained allocation
    /// guards. An incorrect requested type leaves this same ticket untouched.
    pub fn try_into_typed<T: Send + 'static>(mut self) -> Result<Retiring<T>, Self> {
        let failed = self.failed.take().expect("live failure ticket");
        if !failed.task.is::<RetirementEnvelope<T>>() {
            self.failed = Some(failed);
            return Err(self);
        }
        let FailedRetirement {
            id,
            payload_type,
            task,
            retry_sender,
        } = failed;
        let envelope = match task.downcast::<RetirementEnvelope<T>>() {
            Ok(envelope) => envelope,
            Err(task) => {
                self.failed = Some(FailedRetirement {
                    id,
                    payload_type,
                    task,
                    retry_sender,
                });
                return Err(self);
            }
        };
        self.custody.pending.fetch_sub(1, Ordering::AcqRel);
        Ok(Retiring {
            id,
            envelope: Some(envelope),
            sender: retry_sender.unwrap_or_else(|| self.sender.clone()),
            receiver: self.receiver.clone(),
            metrics: Arc::clone(&self.metrics),
            custody: Arc::clone(&self.custody),
            cpu_available: Arc::clone(&self.cpu_available),
        })
    }

    /// Retry the same primary CPU queue without blocking. On refusal, the
    /// ticket stays recoverable, including its original debit and guards.
    pub fn try_requeue(mut self) -> Result<(), Self> {
        // A connected channel is not evidence that a CPU callback still owns
        // its receiver: Shared intentionally retains the receiver after exit.
        if !self.cpu_available.load(Ordering::Acquire) {
            return Err(self);
        }
        let mut failed = self.failed.take().expect("live failure ticket");
        let sender = failed
            .retry_sender
            .take()
            .unwrap_or_else(|| self.sender.clone());
        self.metrics.queued.fetch_add(1, Ordering::AcqRel);
        match sender.try_send(failed) {
            Ok(()) => {
                self.custody.pending.fetch_sub(1, Ordering::AcqRel);
                if !self.cpu_available.load(Ordering::Acquire) {
                    // The final worker may have drained just before this send.
                    rescue_orphaned_primary(&self.receiver, &self.custody, &self.metrics);
                }
                Ok(())
            }
            Err(TrySendError::Full(mut failed) | TrySendError::Disconnected(mut failed)) => {
                self.metrics.queued.fetch_sub(1, Ordering::AcqRel);
                self.metrics.handoff_failed.fetch_add(1, Ordering::Release);
                failed.retry_sender = Some(sender);
                self.failed = Some(failed);
                Err(self)
            }
        }
    }
}

impl Drop for RetirementFailure {
    fn drop(&mut self) {
        if let Some(failed) = self.failed.take() {
            self.custody.put_back(failed);
        }
    }
}

#[derive(Default)]
pub(crate) struct RetirementMetrics {
    pub(crate) queued: AtomicUsize,
    pub(crate) completed: AtomicUsize,
    pub(crate) panicked: AtomicUsize,
    pub(crate) handoff_failed: AtomicUsize,
}

pub(crate) struct RetirementState {
    sender: Sender<FailedRetirement>,
    receiver: Receiver<FailedRetirement>,
    custody: Arc<RecoveryCustody>,
    cpu_available: Arc<AtomicBool>,
    next_id: AtomicU64,
    live: Arc<AtomicUsize>,
    metrics: Arc<RetirementMetrics>,
}

impl RetirementState {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = crossbeam_channel::bounded(RETIREMENT_SLOTS);
        Self {
            sender,
            receiver,
            custody: Arc::new(RecoveryCustody::new()),
            cpu_available: Arc::new(AtomicBool::new(true)),
            next_id: AtomicU64::new(1),
            live: Arc::new(AtomicUsize::new(0)),
            metrics: Arc::new(RetirementMetrics::default()),
        }
    }

    pub(crate) fn live(&self) -> usize {
        self.live.load(Ordering::Acquire)
    }

    pub(crate) fn queued(&self) -> usize {
        self.metrics.queued.load(Ordering::Acquire)
    }

    pub(crate) fn completed(&self) -> usize {
        self.metrics.completed.load(Ordering::Acquire)
    }

    pub(crate) fn panicked(&self) -> usize {
        self.metrics.panicked.load(Ordering::Acquire)
    }

    pub(crate) fn handoff_failed(&self) -> usize {
        self.metrics.handoff_failed.load(Ordering::Acquire)
    }

    pub(crate) fn recovery_pending(&self) -> usize {
        self.custody.pending.load(Ordering::Acquire)
    }

    pub(crate) fn try_take_failed(&self) -> Option<RetirementFailure> {
        if !self.cpu_available.load(Ordering::Acquire) {
            self.rescue_orphaned_primary();
        }
        self.custody
            .receiver
            .try_recv()
            .ok()
            .map(|failed| RetirementFailure {
                failed: Some(failed),
                custody: Arc::clone(&self.custody),
                sender: self.sender.clone(),
                receiver: self.receiver.clone(),
                metrics: Arc::clone(&self.metrics),
                cpu_available: Arc::clone(&self.cpu_available),
            })
    }

    /// Move stranded primary entries into typed recovery custody. Only call
    /// after the final CPU callback body has exited. This moves at most 64
    /// precharged envelopes and never destroys an original on the caller.
    pub(crate) fn mark_cpu_bodies_exited(&self) {
        self.cpu_available.store(false, Ordering::Release);
        self.rescue_orphaned_primary();
    }

    pub(crate) fn rescue_orphaned_primary(&self) {
        if self.cpu_available.load(Ordering::Acquire) {
            return;
        }
        rescue_orphaned_primary(&self.receiver, &self.custody, &self.metrics);
    }

    pub(crate) fn receiver(&self) -> &Receiver<FailedRetirement> {
        &self.receiver
    }

    pub(crate) fn recovery_receiver(&self) -> &Receiver<FailedRetirement> {
        &self.custody.receiver
    }

    pub(crate) fn run(&self, failed: FailedRetirement) {
        self.metrics.queued.fetch_sub(1, Ordering::AcqRel);
        // A panicking user destructor cannot take down both CPU consumers.
        // The envelope's other fields are unwound after its value; the slot
        // and storage guard are therefore retained through destruction.
        if catch_unwind(AssertUnwindSafe(|| drop(failed.task))).is_err() {
            self.metrics.panicked.fetch_add(1, Ordering::Relaxed);
        }
        self.metrics.completed.fetch_add(1, Ordering::Release);
    }

    /// A live CPU worker is the ordinary recovery owner. The exceptional
    /// mailbox is independent of the primary sender, so even a disconnected
    /// producer channel can still retire its exact original on this bank.
    pub(crate) fn run_failed(&self, failed: FailedRetirement) {
        self.custody.pending.fetch_sub(1, Ordering::AcqRel);
        if catch_unwind(AssertUnwindSafe(|| drop(failed.task))).is_err() {
            self.metrics.panicked.fetch_add(1, Ordering::Relaxed);
        }
        self.metrics.completed.fetch_add(1, Ordering::Release);
    }

    pub(crate) fn try_reserve<T: Send + 'static>(
        &self,
        quota: &QuotaGroup,
        declared_bytes: usize,
        still_open: impl Fn() -> bool,
    ) -> Result<RetirementReservation<T>, RejectReason> {
        if !still_open() {
            return Err(RejectReason::Closed);
        }
        if declared_bytes < size_of::<T>() {
            return Err(RejectReason::InvalidCost);
        }
        let id = self
            .next_id
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(1)
            })
            .map_err(|_| RejectReason::InvalidCost)?;
        let mut used = self.live.load(Ordering::Acquire);
        loop {
            if used >= RETIREMENT_SLOTS {
                return Err(RejectReason::QueueFull);
            }
            match self.live.compare_exchange_weak(
                used,
                used + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(actual) => used = actual,
            }
        }
        let slot = RetirementSlot {
            live: Arc::clone(&self.live),
        };
        // Both the payload declaration and envelope metadata are charged
        // before envelope construction. This is cooperative capacity accounting,
        // not an allocator-enforced RSS bound or a charge for opaque native data.
        let bytes = declared_bytes
            .checked_add(size_of::<RetirementEnvelope<T>>())
            .and_then(|n| {
                n.checked_add(
                    size_of::<Retiring<T>>()
                        .max(size_of::<RetirementReservation<T>>())
                        .max(size_of::<FailedRetirement>()),
                )
            })
            .and_then(|n| n.checked_add(2 * size_of::<usize>()))
            .ok_or(RejectReason::InvalidCost)?;
        let storage = quota.reserve_external_storage(bytes)?;
        if !still_open() {
            return Err(RejectReason::Closed);
        }
        Ok(RetirementReservation {
            id,
            envelope: Some(Box::new(RetirementEnvelope {
                value: None,
                retention: None,
                storage_guard: None,
                storage,
                _slot: slot,
            })),
            sender: self.sender.clone(),
            receiver: self.receiver.clone(),
            metrics: Arc::clone(&self.metrics),
            custody: Arc::clone(&self.custody),
            cpu_available: Arc::clone(&self.cpu_available),
        })
    }
}

struct RetirementSlot {
    live: Arc<AtomicUsize>,
}

impl Drop for RetirementSlot {
    fn drop(&mut self) {
        self.live.fetch_sub(1, Ordering::AcqRel);
    }
}

// Field order is essential: the original value dies before its allocation
// charge, and the finite slot is released last, including during unwind.
struct RetirementEnvelope<T> {
    value: Option<T>,
    retention: Option<Retention>,
    storage_guard: Option<Arc<StorageAdmission>>,
    storage: StorageAdmission,
    _slot: RetirementSlot,
}

/// Preallocated, typed custody. Construct this before any expensive payload.
/// Dropping an unused reservation only destroys its small empty envelope.
#[must_use]
pub struct RetirementReservation<T: Send + 'static> {
    id: u64,
    envelope: Option<Box<RetirementEnvelope<T>>>,
    sender: Sender<FailedRetirement>,
    receiver: Receiver<FailedRetirement>,
    metrics: Arc<RetirementMetrics>,
    custody: Arc<RecoveryCustody>,
    cpu_available: Arc<AtomicBool>,
}

impl<T: Send + 'static> RetirementReservation<T> {
    pub fn attach(mut self, value: T) -> Retiring<T> {
        let mut envelope = self.envelope.take().expect("reservation used once");
        envelope.value = Some(value);
        Retiring {
            id: self.id,
            envelope: Some(envelope),
            sender: self.sender.clone(),
            receiver: self.receiver.clone(),
            metrics: Arc::clone(&self.metrics),
            custody: Arc::clone(&self.custody),
            cpu_available: Arc::clone(&self.cpu_available),
        }
    }

    pub fn attach_shared(self, value: T) -> RetiringArc<T>
    where
        T: Sync,
    {
        Arc::new(self.attach(value))
    }
}

/// A value whose final owner gives its original allocation to the CPU bank.
/// Shared leaves must each use this type; wrapping only their outer parent
/// does not control a separately cloned `Arc`'s last owner.
#[must_use]
pub struct Retiring<T: Send + 'static> {
    id: u64,
    envelope: Option<Box<RetirementEnvelope<T>>>,
    sender: Sender<FailedRetirement>,
    receiver: Receiver<FailedRetirement>,
    metrics: Arc<RetirementMetrics>,
    custody: Arc<RecoveryCustody>,
    cpu_available: Arc<AtomicBool>,
}

impl<T: Send + 'static> Retiring<T> {
    #[cfg(test)]
    pub(crate) fn force_disconnected_for_test(&mut self) {
        let (sender, receiver) = crossbeam_channel::bounded(1);
        drop(receiver);
        self.sender = sender;
    }

    /// A shared allocation guard that must outlive this leaf even if its
    /// containing parent has already been released on another thread.
    pub fn set_storage_guard(&mut self, guard: Arc<StorageAdmission>) {
        let envelope = self.envelope.as_mut().expect("retiring payload present");
        assert!(envelope.storage_guard.is_none(), "storage guard set once");
        envelope.storage_guard = Some(guard);
    }

    /// Keep the original job debit through a NotStarted or delivered value's
    /// actual CPU destruction. Detach after transient unpublished rejection so
    /// a retrying UI slot cannot consume one of the eight document jobs.
    pub fn set_retention(&mut self, retention: Retention) {
        let envelope = self.envelope.as_mut().expect("retiring payload present");
        assert!(
            envelope.retention.is_none(),
            "retention must be detached before retry"
        );
        envelope.retention = Some(retention);
    }

    pub fn clear_retention(&mut self) {
        self.envelope
            .as_mut()
            .expect("retiring payload present")
            .retention = None;
    }

    /// Transform inside a CPU callback while this envelope retains the old
    /// job/storage guards. The callback must destroy the original allocation
    /// before it returns or move it into a separately admitted retiring result.
    pub fn try_consume_on_cpu<U>(mut self, transform: impl FnOnce(T) -> U) -> Result<U, Self> {
        if !crate::pool::on_cpu_bank_thread() {
            return Err(self);
        }
        let mut envelope = self.envelope.take().expect("retiring payload present");
        let value = envelope.value.take().expect("retiring value present");
        let result = transform(value);
        drop(envelope);
        Ok(result)
    }

    /// Move the original value through one CPU callback and keep the same
    /// preadmitted envelope for its continuation. An error destroys the old
    /// value and every guard on that CPU callback before returning the error.
    pub fn try_update_on_cpu<E>(
        mut self,
        transform: impl FnOnce(T) -> Result<T, E>,
    ) -> Result<Result<Self, E>, Self> {
        if !crate::pool::on_cpu_bank_thread() {
            return Err(self);
        }
        let mut envelope = self.envelope.take().expect("retiring payload present");
        let value = envelope.value.take().expect("retiring value present");
        match transform(value) {
            Ok(next) => {
                envelope.value = Some(next);
                self.envelope = Some(envelope);
                Ok(Ok(self))
            }
            Err(error) => {
                drop(envelope);
                Ok(Err(error))
            }
        }
    }

    /// Only a bank callback may consume and replace the payload. The retained
    /// charge covers the caller-declared peak of both old and new values.
    pub fn try_map_on_cpu<U: Send + 'static>(
        mut self,
        transform: impl FnOnce(T) -> U,
    ) -> Result<Retiring<U>, Self> {
        if !crate::pool::on_cpu_bank_thread() {
            return Err(self);
        }
        let envelope = self.envelope.take().expect("retiring payload present");
        let RetirementEnvelope {
            value,
            retention,
            storage_guard,
            storage,
            _slot,
        } = *envelope;
        let new_value = transform(value.expect("retiring value present"));
        Ok(Retiring {
            id: self.id,
            envelope: Some(Box::new(RetirementEnvelope {
                value: Some(new_value),
                retention,
                storage_guard,
                storage,
                _slot,
            })),
            sender: self.sender.clone(),
            receiver: self.receiver.clone(),
            metrics: Arc::clone(&self.metrics),
            custody: Arc::clone(&self.custody),
            cpu_available: Arc::clone(&self.cpu_available),
        })
    }
}

impl<T: Send + 'static> Deref for Retiring<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.envelope
            .as_ref()
            .and_then(|envelope| envelope.value.as_ref())
            .expect("retiring value present")
    }
}

impl<T: fmt::Debug + Send + 'static> fmt::Debug for Retiring<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.deref().fmt(formatter)
    }
}

impl<T: PartialEq + Send + 'static> PartialEq for Retiring<T> {
    fn eq(&self, other: &Self) -> bool {
        self.deref().eq(other.deref())
    }
}

impl<T: Eq + Send + 'static> Eq for Retiring<T> {}

impl<T: Hash + Send + 'static> Hash for Retiring<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.deref().hash(state);
    }
}

impl<T: Send + 'static> DerefMut for Retiring<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.envelope
            .as_mut()
            .and_then(|envelope| envelope.value.as_mut())
            .expect("retiring value present")
    }
}

impl<T: Send + 'static> Drop for Retiring<T> {
    fn drop(&mut self) {
        let Some(envelope) = self.envelope.take() else {
            return;
        };
        let failed = FailedRetirement {
            id: self.id,
            payload_type: type_name::<T>(),
            task: envelope,
            retry_sender: None,
        };
        if !self.cpu_available.load(Ordering::Acquire) {
            self.custody.keep(FailedRetirement {
                retry_sender: Some(self.sender.clone()),
                ..failed
            });
            return;
        }
        self.metrics.queued.fetch_add(1, Ordering::AcqRel);
        match self.sender.try_send(failed) {
            Ok(()) => {
                if !self.cpu_available.load(Ordering::Acquire) {
                    rescue_orphaned_primary(&self.receiver, &self.custody, &self.metrics);
                }
            }
            Err(TrySendError::Full(failed) | TrySendError::Disconnected(failed)) => {
                self.metrics.queued.fetch_sub(1, Ordering::AcqRel);
                self.metrics.handoff_failed.fetch_add(1, Ordering::Release);
                self.custody.keep(FailedRetirement {
                    retry_sender: Some(self.sender.clone()),
                    ..failed
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use crate::budget::{QuotaGroup, QuotaLimits};

    use super::*;

    struct Spy {
        quota: QuotaGroup,
        notice: mpsc::Sender<(thread::ThreadId, usize)>,
    }

    impl Drop for Spy {
        fn drop(&mut self) {
            let _ = self
                .notice
                .send((thread::current().id(), self.quota.snapshot().worker_bytes));
        }
    }

    #[test]
    fn disconnected_primary_preserves_typed_original_and_debit_until_cpu_destruction() {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: 0,
            worker_bytes: 1024 * 1024,
        });
        let baseline = quota.snapshot().worker_bytes;
        let state = RetirementState::new();
        let original_guard = Arc::new(quota.reserve_external_storage(8192).unwrap());
        let permit = state.try_reserve::<Spy>(&quota, 4096, || true).unwrap();
        let (notice_sender, notice_receiver) = mpsc::channel();
        let mut original = permit.attach(Spy {
            quota: quota.clone(),
            notice: notice_sender,
        });
        original.set_storage_guard(Arc::clone(&original_guard));
        drop(original_guard);
        let original_address = (&*original as *const Spy) as usize;
        let ui_thread = thread::current().id();
        let (disconnected_sender, disconnected_receiver) = crossbeam_channel::bounded(1);
        drop(disconnected_receiver);
        original.sender = disconnected_sender;
        drop(original);

        assert!(notice_receiver.try_recv().is_err());
        assert_eq!(state.live(), 1);
        assert_eq!(state.recovery_pending(), 1);
        assert_eq!(state.handoff_failed(), 1);
        assert!(quota.snapshot().worker_bytes >= baseline + 8192);

        let failure = state.try_take_failed().unwrap();
        assert_eq!(failure.payload_type(), type_name::<Spy>());
        let id = failure.id();
        let failure = failure.try_into_typed::<Vec<u8>>().err().unwrap();
        assert_eq!(failure.id(), id);
        drop(failure); // A dropped ticket returns to bounded custody.
        assert_eq!(state.recovery_pending(), 1);

        let failure = state.try_take_failed().unwrap();
        let failure = failure.try_requeue().err().unwrap();
        assert_eq!(state.recovery_pending(), 1);
        let mut recovered = failure.try_into_typed::<Spy>().ok().unwrap();
        assert_eq!((&*recovered as *const Spy) as usize, original_address);
        assert_eq!(state.recovery_pending(), 0);
        assert_eq!(state.live(), 1);
        recovered.sender = state.sender.clone();
        drop(recovered);
        let task = state.receiver().try_recv().unwrap();
        let cpu = thread::spawn(move || state.run(task));
        cpu.join().unwrap();
        let (destructor_thread, charged_during_drop) = notice_receiver
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        assert_ne!(destructor_thread, ui_thread);
        assert!(charged_during_drop >= baseline + 8192);
        assert_eq!(quota.snapshot().worker_bytes, baseline);
    }

    #[test]
    fn final_cpu_body_rescues_connected_primary_and_late_holder_as_typed_originals() {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: 0,
            worker_bytes: 1024 * 1024,
        });
        let baseline = quota.snapshot().worker_bytes;
        let state = RetirementState::new();
        let (notice_sender, notice_receiver) = mpsc::channel();
        let first = state
            .try_reserve::<Spy>(&quota, 4096, || true)
            .unwrap()
            .attach(Spy {
                quota: quota.clone(),
                notice: notice_sender.clone(),
            });
        let second = state
            .try_reserve::<Spy>(&quota, 4096, || true)
            .unwrap()
            .attach(Spy {
                quota: quota.clone(),
                notice: notice_sender,
            });
        let first_address = (&*first as *const Spy) as usize;
        let second_address = (&*second as *const Spy) as usize;
        let ui_thread = thread::current().id();

        // The primary Sender is still connected because State owns Receiver,
        // even though no CPU callback can receive any more.
        drop(first);
        assert_eq!(state.queued(), 1);
        assert_eq!(state.recovery_pending(), 0);
        state.mark_cpu_bodies_exited();
        assert_eq!(state.queued(), 0);
        assert_eq!(state.recovery_pending(), 1);
        drop(second);
        assert_eq!(state.recovery_pending(), 2);
        assert!(notice_receiver.try_recv().is_err());

        let primary_ticket = state.try_take_failed().unwrap();
        assert_eq!(primary_ticket.payload_type(), type_name::<Spy>());
        let primary_ticket = primary_ticket.try_requeue().err().unwrap();
        let first_original = primary_ticket.try_into_typed::<Spy>().ok().unwrap();
        assert_eq!((&*first_original as *const Spy) as usize, first_address);
        drop(first_original);
        assert_eq!(state.recovery_pending(), 2);
        let second_ticket = state.try_take_failed().unwrap();
        let second_original = second_ticket.try_into_typed::<Spy>().ok().unwrap();
        assert_eq!((&*second_original as *const Spy) as usize, second_address);
        drop(second_original);
        assert_eq!(state.recovery_pending(), 2);
        assert_eq!(state.live(), 2);
        assert!(quota.snapshot().worker_bytes > baseline);

        // This test-only executor closes its fixtures off the UI thread. The
        // production contract reports incomplete shutdown until an existing
        // bank owner can do so; it never silently runs a new rescue thread.
        let cleanup = thread::spawn(move || {
            for _ in 0..2 {
                let failed = state.recovery_receiver().try_recv().unwrap();
                state.run_failed(failed);
            }
            assert_eq!(state.live(), 0);
            assert_eq!(state.recovery_pending(), 0);
        });
        cleanup.join().unwrap();
        for _ in 0..2 {
            let (destructor_thread, charged_during_drop) = notice_receiver
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
            assert_ne!(destructor_thread, ui_thread);
            assert!(charged_during_drop > baseline);
        }
        assert_eq!(quota.snapshot().worker_bytes, baseline);
    }
}
