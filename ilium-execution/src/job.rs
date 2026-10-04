use std::any::Any;
use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError};
use std::sync::Arc;

use ilium_platform::owned_worker::StopToken;

use crate::budget::{Debit, JobCost, RejectReason};
use crate::pool::{QueueSlot, Shared};

/// A callback owns all of its input. It must obey its declared memory bounds.
/// No callback runs under a foundation or platform registry lock.
pub trait Job: Send + 'static {
    type Output: Send + 'static;
    type Error: Send + 'static;
    fn run(self, context: JobContext) -> Result<Self::Output, Self::Error>;
}

impl<F, O, E> Job for F
where
    F: FnOnce(JobContext) -> Result<O, E> + Send + 'static,
    O: Send + 'static,
    E: Send + 'static,
{
    type Output = O;
    type Error = E;
    fn run(self, context: JobContext) -> Result<O, E> {
        self(context)
    }
}

pub struct JobContext {
    stop: StopToken,
    cost: JobCost,
}

impl JobContext {
    pub fn stop_requested(&self) -> bool {
        self.stop.is_stopped()
    }
    pub fn stop_token(&self) -> StopToken {
        self.stop.clone()
    }
    pub fn cost(&self) -> JobCost {
        self.cost
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    Cancelled,
    Shutdown,
}

pub enum JobOutcome<J: Job> {
    /// Exactly the callback's result. This is NOT an emitted-frame or durable
    /// write acknowledgement unless the DOMAIN result explicitly proves that.
    Finished(Result<J::Output, J::Error>),
    /// The callback was never invoked; all of its original input is retained.
    NotStarted { job: J, reason: SkipReason },
    /// The callback unwound. Input may have been consumed and effects may have
    /// occurred: neither cancellation nor rollback is asserted.
    Panicked,
}

pub struct Rejected<T> {
    pub reason: RejectReason,
    pub value: T,
}

impl<T> fmt::Debug for Rejected<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rejected")
            .field("reason", &self.reason)
            .finish_non_exhaustive()
    }
}

/// This wrapper keeps its entire original reservation through destruction.
/// Borrow data with view(), or transform it without dropping the reservation.
/// Splitting a value for a typed channel transfers an explicit retention guard;
/// the receiving container must keep that guard until its payload is destroyed.
/// Cloning referenced payloads still requires separate caller accounting.
#[must_use]
pub struct Retained<T> {
    // Field order is important: destroy payload before releasing its hold.
    value: T,
    hold: Arc<JobHold>,
}

/// An opaque lifetime charge for a payload transferred between typed owners.
/// Clones share one reservation, rather than admitting additional allocations.
/// Keep this as the last field of a charged container, after its payload. A
/// cloned payload that allocates new storage needs a separate reservation.
#[derive(Clone)]
#[must_use]
pub struct Retention {
    hold: Arc<JobHold>,
}

impl Retention {
    /// Reattaches the same charge after moving the original owned allocation.
    /// This does not authorize allocating a larger replacement payload.
    pub fn retain<T>(self, value: T) -> Retained<T> {
        Retained {
            value,
            hold: self.hold,
        }
    }
}

impl fmt::Debug for Retention {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Retention").finish_non_exhaustive()
    }
}

impl<T> Retained<T> {
    pub(crate) fn from_external(value: T, hold: Arc<JobHold>) -> Self {
        Self { value, hold }
    }

    pub fn view(&self) -> &T {
        &self.value
    }

    /// Moves the original payload and its charge to another typed owner.
    /// The consumer must retain the guard through queued and active use; this
    /// is not permission to drop the guard while retaining the allocation.
    pub fn into_parts(self) -> (T, Retention) {
        let Self { value, hold } = self;
        (value, Retention { hold })
    }

    /// Transfers owned data through an asynchronous adapter while keeping its
    /// reservation alive, including cancellation of the waiting future. This
    /// is for bounded I/O delivery, never CPU execution on a runtime thread.
    pub async fn map_async<U, F>(self, transform: impl FnOnce(T) -> F) -> Retained<U>
    where
        F: std::future::Future<Output = U>,
    {
        let Self { value, hold } = self;
        let value = transform(value).await;
        Retained { value, hold }
    }

    /// Run only a bounded/nonblocking transformation on the UI. A recovery
    /// adapter can inspect NotStarted and move its job inside this closure;
    /// retry admission must succeed before it relinquishes the old charge.
    pub fn map<U>(self, transform: impl FnOnce(T) -> U) -> Retained<U> {
        let Self { value, hold } = self;
        Retained {
            value: transform(value),
            hold,
        }
    }
}

pub enum JobPoll<J: Job> {
    Pending,
    Ready(Retained<JobOutcome<J>>),
    /// Sender disappeared without delivering an outcome. Never call this a
    /// successful write or a successfully cancelled operation.
    Lost,
    /// A terminal result was already taken from this receipt.
    Taken,
}

/// Dropping this receipt explicitly abandons delivery; it does NOT cancel a
/// running or ordered operation. cancel() is a separate cooperative request.
/// Retain receipts for ordered/durable operations until their actor resolves
/// them. Drop a polled receipt promptly: it conservatively holds its charge.
#[must_use]
pub struct Receipt<J: Job> {
    receiver: Receiver<Retained<JobOutcome<J>>>,
    stop: StopToken,
    metrics: Arc<Metrics>,
    taken: bool,
    // Keep empty-but-live receipts budgeted too. Last field releases last.
    _hold: Arc<JobHold>,
}

impl<J: Job> Receipt<J> {
    pub fn cancel(&self) {
        self.stop.stop();
    }

    /// Shares the original admission with a bounded delivery owner.
    /// Destroy its original payload before this guard; it admits no new allocations.
    pub fn retention(&self) -> Retention {
        Retention {
            hold: Arc::clone(&self._hold),
        }
    }

    pub fn try_take(&mut self) -> JobPoll<J> {
        if self.taken {
            return JobPoll::Taken;
        }
        match self.receiver.try_recv() {
            Ok(value) => {
                self.taken = true;
                JobPoll::Ready(value)
            }
            Err(TryRecvError::Empty) => JobPoll::Pending,
            Err(TryRecvError::Disconnected) => {
                self.taken = true;
                self.metrics.lost.fetch_add(1, Ordering::Relaxed);
                JobPoll::Lost
            }
        }
    }
}

impl<J: Job> Drop for Receipt<J> {
    fn drop(&mut self) {
        if !self.taken {
            self.metrics.abandoned.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub(crate) struct JobHold {
    pub(crate) _debit: Debit,
    pub(crate) service_slots: Option<Arc<AtomicUsize>>,
}

impl Drop for JobHold {
    fn drop(&mut self) {
        if let Some(slots) = &self.service_slots {
            slots.fetch_sub(1, Ordering::AcqRel);
        }
        // debit drops afterwards. All payload/queue/callback references have
        // ended before the last Arc<JobHold> can reach this point.
    }
}

#[derive(Default)]
pub(crate) struct Metrics {
    pub(crate) running: AtomicUsize,
    pub(crate) succeeded: AtomicUsize,
    pub(crate) failed: AtomicUsize,
    pub(crate) skipped: AtomicUsize,
    pub(crate) panicked: AtomicUsize,
    pub(crate) abandoned: AtomicUsize,
    pub(crate) undelivered: AtomicUsize,
    pub(crate) lost: AtomicUsize,
    pub(crate) wake_panics: AtomicUsize,
    pub(crate) bodies_exited: AtomicUsize,
    pub(crate) joined: AtomicUsize,
    pub(crate) thread_panics: AtomicUsize,
}

pub(crate) type Wake = Arc<dyn Fn() + Send + Sync>;

pub(crate) trait ErasedJob: Any + Send {
    fn execute(self: Box<Self>, shared: &Shared);
    fn into_any(self: Box<Self>) -> Box<dyn Any + Send>;
}

struct Task<J: Job> {
    job: J,
    sender: SyncSender<Retained<JobOutcome<J>>>,
    stop: StopToken,
    cost: JobCost,
    queue: QueueSlot,
    metrics: Arc<Metrics>,
    hold: Arc<JobHold>,
    wake: Option<Wake>,
}

struct Running(Arc<Metrics>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.running.fetch_sub(1, Ordering::AcqRel);
    }
}

impl<J: Job> ErasedJob for Task<J> {
    fn into_any(self: Box<Self>) -> Box<dyn Any + Send> {
        self
    }

    fn execute(self: Box<Self>, shared: &Shared) {
        let Self {
            job,
            sender,
            stop,
            cost,
            queue,
            metrics,
            hold,
            wake,
        } = *self;
        // Outside the queue lock; admission can now reuse this waiting slot.
        drop(queue);
        let outcome = if shared.cancelling() {
            metrics.skipped.fetch_add(1, Ordering::Relaxed);
            JobOutcome::NotStarted {
                job,
                reason: SkipReason::Shutdown,
            }
        } else if stop.is_stopped() {
            metrics.skipped.fetch_add(1, Ordering::Relaxed);
            JobOutcome::NotStarted {
                job,
                reason: SkipReason::Cancelled,
            }
        } else {
            metrics.running.fetch_add(1, Ordering::AcqRel);
            let running = Running(Arc::clone(&metrics));
            let answer = catch_unwind(AssertUnwindSafe(|| job.run(JobContext { stop, cost })));
            drop(running);
            match answer {
                Ok(value) => {
                    if value.is_ok() {
                        metrics.succeeded.fetch_add(1, Ordering::Relaxed);
                    } else {
                        metrics.failed.fetch_add(1, Ordering::Relaxed);
                    }
                    // Never relabel an actual callback result after cancellation:
                    // an irreversible effect may already have completed.
                    JobOutcome::Finished(value)
                }
                Err(payload) => {
                    metrics.panicked.fetch_add(1, Ordering::Relaxed);
                    // Do not retain arbitrary-sized panic payloads in outcomes.
                    // User destructors/panic hooks are subject to the Job contract.
                    drop(payload);
                    JobOutcome::Panicked
                }
            }
        };
        let value = Retained {
            value: outcome,
            hold,
        };
        // The callback (including its captures) has finished before publication.
        // Transfer its sole task hold into the result: an extra execution Arc
        // here would outlive the completion wake and could leave admission
        // waiters asleep after the consumer releases the published result.
        // One producer sends exactly once into an initially empty one-slot
        // channel. Full is an invariant failure, handled like disconnection;
        // neither branch blocks a bank worker or claims delivery.
        if let Err(rejected) = sender.try_send(value) {
            metrics.undelivered.fetch_add(1, Ordering::Relaxed);
            drop(rejected);
        } else if let Some(wake) = wake {
            // A hint over the existing event channel, not a second result path.
            // Contract: bounded/nonblocking; never blocking_send or filesystem I/O.
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| wake())) {
                metrics.wake_panics.fetch_add(1, Ordering::Relaxed);
                drop(payload); // the already-published result remains recoverable
            }
        }
    }
}

/// A failed queue publication returns the same typed job to its caller. The
/// erased value is provably Task<J>: it is the value created by this submit.
pub(crate) fn recover_unpublished<J: Job>(task: Box<dyn ErasedJob>, mut receipt: Receipt<J>) -> J {
    receipt.taken = true; // No accepted work was abandoned.
    let task = task
        .into_any()
        .downcast::<Task<J>>()
        .unwrap_or_else(|_| unreachable!("queue returned a different job type"));
    let Task { job, .. } = *task;
    job
}

pub(crate) fn task<J: Job>(
    job: J,
    cost: JobCost,
    stop: StopToken,
    queue: QueueSlot,
    metrics: Arc<Metrics>,
    hold: Arc<JobHold>,
    wake: Option<Wake>,
) -> (Box<dyn ErasedJob>, Receipt<J>) {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let receipt = Receipt {
        receiver,
        stop: stop.clone(),
        metrics: Arc::clone(&metrics),
        taken: false,
        _hold: Arc::clone(&hold),
    };
    (
        Box::new(Task {
            job,
            sender,
            stop,
            cost,
            queue,
            metrics,
            hold,
            wake,
        }),
        receipt,
    )
}
