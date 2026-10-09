use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};

const CLIENTS: usize = 0;
const JOBS: usize = 1;
const SERVICES: usize = 2;
const INPUT: usize = 3;
const RESULT: usize = 4;
const THREADS: usize = 5;
const WORKER_BYTES: usize = 6;
const DIMENSIONS: usize = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    Busy,
    Closed,
    QueueFull,
    ServiceBankFull,
    ClientLimit,
    JobLimit,
    ServiceLimit,
    InputBytes,
    ResultBytes,
    WorkerLimit,
    WorkerBytes,
    InvalidCost,
    AccountingPoisoned,
}

/// Identifies the rejecting ledger relative to the requesting client. Distance
/// zero is the client itself; greater distances are its shared ancestors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionBoundary {
    Root,
    Client { ancestor_distance: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaResource {
    Clients,
    Jobs,
    Services,
    InputBytes,
    ResultBytes,
    WorkerThreads,
    WorkerBytes,
}

/// Captured from the actual failed dimension check while the admission gate
/// is held. Concurrent releases may follow; these numbers are never refreshed
/// from a later snapshot and contain no job payload or authored settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuotaRefusal {
    pub boundary: AdmissionBoundary,
    pub resource: QuotaResource,
    pub requested: usize,
    pub used: usize,
    pub limit: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmissionFailure {
    pub reason: RejectReason,
    /// None for lifecycle, queue and gate refusals that did not check a quota.
    pub quota: Option<QuotaRefusal>,
}
impl From<RejectReason> for AdmissionFailure {
    fn from(reason: RejectReason) -> Self {
        Self {
            reason,
            quota: None,
        }
    }
}

/// Bounds across all users of ONE shared QuotaGroup. Zero disables a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuotaLimits {
    pub clients: usize,
    pub jobs: usize,
    pub service_jobs: usize,
    pub input_bytes: usize,
    pub result_bytes: usize,
    pub worker_threads: usize,
    /// Physical worker resident declarations and bank metadata retained by
    /// monitors/clients after physical workers have joined.
    pub worker_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientLimits {
    pub jobs: usize,
    pub service_jobs: usize,
    pub input_bytes: usize,
    pub result_bytes: usize,
}

/// Simultaneously reserved until the last receipt/retained outcome is dropped.
/// input_bytes includes input, captures, and peak working memory. The reservation
/// is deliberately NOT shrunk on callback return: NotStarted can retain input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobCost {
    pub input_bytes: usize,
    pub result_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuotaSnapshot {
    pub limits: QuotaLimits,
    pub clients: usize,
    pub jobs: usize,
    pub service_jobs: usize,
    pub input_bytes: usize,
    pub result_bytes: usize,
    pub worker_threads: usize,
    pub worker_bytes: usize,
}

impl QuotaSnapshot {
    /// Checked because independently configured dimensions need not sum in usize.
    pub fn declared_bytes(&self) -> Option<usize> {
        self.input_bytes
            .checked_add(self.result_bytes)?
            .checked_add(self.worker_bytes)
    }
}

impl QuotaLimits {
    fn array(self) -> [usize; DIMENSIONS] {
        [
            self.clients,
            self.jobs,
            self.service_jobs,
            self.input_bytes,
            self.result_bytes,
            self.worker_threads,
            self.worker_bytes,
        ]
    }
}

pub(crate) struct Ledger {
    limits: QuotaLimits,
    used: [AtomicUsize; DIMENSIONS],
    admission: Mutex<()>,
    released: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl Ledger {
    fn new(limits: QuotaLimits) -> Self {
        Self {
            limits,
            used: std::array::from_fn(|_| AtomicUsize::new(0)),
            admission: Mutex::new(()),
            released: None,
        }
    }

    pub(crate) fn gate(&self) -> Result<MutexGuard<'_, ()>, RejectReason> {
        match self.admission.try_lock() {
            Ok(guard) => Ok(guard),
            Err(TryLockError::WouldBlock) => Err(RejectReason::Busy),
            Err(TryLockError::Poisoned(_)) => Err(RejectReason::AccountingPoisoned),
        }
    }

    fn check(&self, amount: &[usize; DIMENSIONS]) -> Result<(), RejectReason> {
        self.check_detailed(amount, AdmissionBoundary::Root)
            .map_err(|failure| failure.reason)
    }

    fn check_detailed(
        &self,
        amount: &[usize; DIMENSIONS],
        boundary: AdmissionBoundary,
    ) -> Result<(), AdmissionFailure> {
        let resources = [
            QuotaResource::Clients,
            QuotaResource::Jobs,
            QuotaResource::Services,
            QuotaResource::InputBytes,
            QuotaResource::ResultBytes,
            QuotaResource::WorkerThreads,
            QuotaResource::WorkerBytes,
        ];
        let reasons = [
            RejectReason::ClientLimit,
            RejectReason::JobLimit,
            RejectReason::ServiceLimit,
            RejectReason::InputBytes,
            RejectReason::ResultBytes,
            RejectReason::WorkerLimit,
            RejectReason::WorkerBytes,
        ];
        let limits = self.limits.array();
        for i in 0..DIMENSIONS {
            let used = self.used[i].load(Ordering::Acquire);
            if amount[i] > limits[i].saturating_sub(used) {
                return Err(AdmissionFailure {
                    reason: reasons[i],
                    quota: Some(QuotaRefusal {
                        boundary,
                        resource: resources[i],
                        requested: amount[i],
                        used,
                        limit: limits[i],
                    }),
                });
            }
        }
        Ok(())
    }

    // All additions are serialized by admission. Releases only subtract and
    // never acquire a gate, including when invoked by a UI-side Drop.
    fn add(&self, amount: &[usize; DIMENSIONS]) {
        for (used, amount) in self.used.iter().zip(amount) {
            used.fetch_add(*amount, Ordering::AcqRel);
        }
    }

    fn release(&self, amount: &[usize; DIMENSIONS]) {
        for (used, amount) in self.used.iter().zip(amount) {
            used.fetch_sub(*amount, Ordering::AcqRel);
        }
        if let Some(released) = &self.released {
            released();
        }
    }

    fn snapshot(&self) -> QuotaSnapshot {
        let a: [usize; DIMENSIONS] = std::array::from_fn(|i| self.used[i].load(Ordering::Acquire));
        QuotaSnapshot {
            limits: self.limits,
            clients: a[CLIENTS],
            jobs: a[JOBS],
            service_jobs: a[SERVICES],
            input_bytes: a[INPUT],
            result_bytes: a[RESULT],
            worker_threads: a[THREADS],
            worker_bytes: a[WORKER_BYTES],
        }
    }
}

/// Share this handle; constructing another group creates independent limits.
#[derive(Clone)]
pub struct QuotaGroup {
    pub(crate) ledger: Arc<Ledger>,
}

impl QuotaGroup {
    /// Whether both handles debit the same process-wide admission ledger.
    /// Equal limits alone do not establish shared ownership.
    pub fn shares_root(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.ledger, &other.ledger)
    }

    /// Bootstrap construction; performs no spawn or filesystem operation.
    pub fn new(limits: QuotaLimits) -> Self {
        Self {
            ledger: Arc::new(Ledger::new(limits)),
        }
    }

    /// Bootstrap wake hook for adapters waiting on retained-result or physical
    /// worker admission. Runs outside admission locks; must be bounded and must
    /// not panic. It carries a wake hint, never a job or output payload.
    pub fn new_with_admission_wake(
        limits: QuotaLimits,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let mut ledger = Ledger::new(limits);
        ledger.released = Some(Arc::new(wake));
        Self {
            ledger: Arc::new(ledger),
        }
    }

    /// Charge an existing worker and its native library threads. Keep this
    /// lease in its platform-owner wake closure through actual join/TLS exit.
    pub fn reserve_external_worker(
        &self,
        threads: usize,
        resident_bytes: usize,
    ) -> Result<WorkerAdmission, RejectReason> {
        if threads == 0 {
            return Err(RejectReason::InvalidCost);
        }
        let guard = self.ledger.gate()?;
        let mut amount = [0; DIMENSIONS];
        amount[THREADS] = threads;
        amount[WORKER_BYTES] = resident_bytes;
        self.ledger.check(&amount)?;
        self.ledger.add(&amount);
        drop(guard);
        Ok(WorkerAdmission {
            _debit: Debit {
                root: Arc::clone(&self.ledger),
                tenant: None,
                amount,
            },
        })
    }

    /// Reserve independently owned cache/snapshot storage without inventing
    /// a worker thread. Keep this lease through the last retained allocation,
    /// even when its producing worker has already joined.
    pub fn reserve_external_storage(
        &self,
        resident_bytes: usize,
    ) -> Result<StorageAdmission, RejectReason> {
        if resident_bytes == 0 {
            return Err(RejectReason::InvalidCost);
        }
        let guard = self.ledger.gate()?;
        let mut amount = [0; DIMENSIONS];
        amount[WORKER_BYTES] = resident_bytes;
        self.ledger.check(&amount)?;
        self.ledger.add(&amount);
        drop(guard);
        Ok(StorageAdmission {
            _debit: Debit {
                root: Arc::clone(&self.ledger),
                tenant: None,
                amount,
            },
        })
    }

    /// Individual counters are bounded. This is not a coherent instant across
    /// counters when admissions/releases run concurrently.
    pub fn snapshot(&self) -> QuotaSnapshot {
        self.ledger.snapshot()
    }

    /// Registers a pure aggregate admission identity independent of any bank.
    /// Clones share its counters; the existing root client cap bounds groups.
    pub fn admission_group(&self, limits: ClientLimits) -> Result<AdmissionGroup, RejectReason> {
        Ok(AdmissionGroup {
            quota: self.clone(),
            tenant: self.client(limits)?,
        })
    }

    pub(crate) fn client(&self, limits: ClientLimits) -> Result<Arc<Tenant>, RejectReason> {
        self.register_client(limits, None)
    }

    pub(crate) fn child_client(
        &self,
        limits: ClientLimits,
        parent: Arc<Tenant>,
    ) -> Result<Arc<Tenant>, RejectReason> {
        if !Arc::ptr_eq(&self.ledger, &parent._registration.root) {
            return Err(RejectReason::InvalidCost);
        }
        // Fixed depth keeps admission and destruction bounded; this is a
        // resource ownership tree, never an arbitrary recursive policy graph.
        if parent.depth >= 8 {
            return Err(RejectReason::InvalidCost);
        }
        self.register_client(limits, Some(parent))
    }

    fn register_client(
        &self,
        limits: ClientLimits,
        parent: Option<Arc<Tenant>>,
    ) -> Result<Arc<Tenant>, RejectReason> {
        let guard = self.ledger.gate()?;
        let mut amount = [0; DIMENSIONS];
        amount[CLIENTS] = 1;
        self.ledger.check(&amount)?;
        self.ledger.add(&amount);
        drop(guard);
        let registration = Debit {
            root: Arc::clone(&self.ledger),
            tenant: None,
            amount,
        };
        Ok(Arc::new(Tenant {
            depth: parent.as_ref().map_or(1, |parent| parent.depth + 1),
            parent,
            ledger: Arc::new(Ledger::new(QuotaLimits {
                clients: 0,
                jobs: limits.jobs,
                service_jobs: limits.service_jobs,
                input_bytes: limits.input_bytes,
                result_bytes: limits.result_bytes,
                worker_threads: 0,
                worker_bytes: 0,
            })),
            _registration: registration,
        }))
    }

    pub(crate) fn worker(&self, bytes: usize) -> Result<Debit, RejectReason> {
        let guard = self.ledger.gate()?;
        let mut amount = [0; DIMENSIONS];
        amount[THREADS] = 1;
        amount[WORKER_BYTES] = bytes;
        self.ledger.check(&amount)?;
        self.ledger.add(&amount);
        drop(guard);
        Ok(Debit {
            root: Arc::clone(&self.ledger),
            tenant: None,
            amount,
        })
    }

    /// Charge bank storage for the lifetime of its Shared allocation, which
    /// may outlive all physical workers through monitors and client handles.
    pub(crate) fn bank_metadata(&self, bytes: usize) -> Result<Debit, RejectReason> {
        let guard = self.ledger.gate()?;
        let mut amount = [0; DIMENSIONS];
        amount[WORKER_BYTES] = bytes;
        self.ledger.check(&amount)?;
        self.ledger.add(&amount);
        drop(guard);
        Ok(Debit {
            root: Arc::clone(&self.ledger),
            tenant: None,
            amount,
        })
    }
}

/// Physical-resource lease. Its lifetime must match actual worker/library
/// ownership, including retirement; callback return alone cannot release it.
#[must_use]
pub struct WorkerAdmission {
    _debit: Debit,
}

/// Declared persistent storage, independent of physical thread lifetime.
/// Use one Arc of this lease across immutable clones of the same allocation.
#[must_use]
pub struct StorageAdmission {
    _debit: Debit,
}
impl StorageAdmission {
    /// Number of resident worker bytes covered by this lease.
    pub fn resident_bytes(&self) -> usize {
        self._debit.amount[WORKER_BYTES]
    }

    /// Whether this held allocation debits the exact supplied native root.
    /// Matching limits do not establish provenance. Inspecting never changes usage.
    pub fn shares_root(&self, quota: &QuotaGroup) -> bool {
        Arc::ptr_eq(&self._debit.root, &quota.ledger)
    }
}
impl std::fmt::Debug for StorageAdmission {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StorageAdmission")
            .field("resident_bytes", &self._debit.amount[WORKER_BYTES])
            .finish()
    }
}

/// Pure shared quota hierarchy, with no worker bank or shutdown ownership.
/// Binding a client preserves that bank's own lifecycle and finite queue.
#[derive(Clone)]
pub struct AdmissionGroup {
    quota: QuotaGroup,
    tenant: Arc<Tenant>,
}
impl AdmissionGroup {
    pub fn child(&self, limits: ClientLimits) -> Result<Self, RejectReason> {
        Ok(Self {
            quota: self.quota.clone(),
            tenant: self.bind_child(&self.quota, limits)?,
        })
    }
    pub fn usage(&self) -> QuotaSnapshot {
        self.tenant.snapshot()
    }
    pub(crate) fn bind_child(
        &self,
        quota: &QuotaGroup,
        limits: ClientLimits,
    ) -> Result<Arc<Tenant>, RejectReason> {
        if !Arc::ptr_eq(&self.quota.ledger, &quota.ledger) {
            return Err(RejectReason::InvalidCost);
        }
        quota.child_client(limits, Arc::clone(&self.tenant))
    }
}

pub(crate) struct Tenant {
    pub(crate) ledger: Arc<Ledger>,
    parent: Option<Arc<Tenant>>,
    depth: usize,
    // Kept alive by every job debit, including retained results.
    _registration: Debit,
}

impl Tenant {
    pub(crate) fn maximum_job_cost(&self) -> Result<JobCost, RejectReason> {
        let mut cost = JobCost {
            input_bytes: usize::MAX,
            result_bytes: usize::MAX,
        };
        let mut current = Some(self);
        while let Some(tenant) = current {
            let limits = tenant.ledger.limits;
            if limits.jobs == 0 {
                return Err(RejectReason::JobLimit);
            }
            cost.input_bytes = cost.input_bytes.min(limits.input_bytes);
            cost.result_bytes = cost.result_bytes.min(limits.result_bytes);
            current = tenant.parent.as_deref();
        }
        Ok(cost)
    }

    pub(crate) fn snapshot(&self) -> QuotaSnapshot {
        self.ledger.snapshot()
    }
}

pub(crate) struct Debit {
    root: Arc<Ledger>,
    tenant: Option<Arc<Tenant>>,
    amount: [usize; DIMENSIONS],
}

impl Drop for Debit {
    fn drop(&mut self) {
        if let Some(tenant) = &self.tenant {
            let mut current = Some(tenant.as_ref());
            while let Some(tenant) = current {
                tenant.ledger.release(&self.amount);
                current = tenant.parent.as_deref();
            }
        }
        self.root.release(&self.amount);
    }
}

/// Call with BOTH root and leaf admission guards held. The shared root gate
/// serializes every ancestor check/add, so sibling jobs cannot over-admit.
/// Releases use atomic subtraction; no user code runs under admission locks.
pub(crate) fn job_debit(
    root: &Arc<Ledger>,
    tenant: &Arc<Tenant>,
    cost: JobCost,
    service: bool,
) -> Result<Debit, RejectReason> {
    job_debit_detailed(root, tenant, cost, service).map_err(|failure| failure.reason)
}

pub(crate) fn job_debit_detailed(
    root: &Arc<Ledger>,
    tenant: &Arc<Tenant>,
    cost: JobCost,
    service: bool,
) -> Result<Debit, AdmissionFailure> {
    let mut amount = [0; DIMENSIONS];
    amount[JOBS] = 1;
    amount[SERVICES] = usize::from(service);
    amount[INPUT] = cost.input_bytes;
    amount[RESULT] = cost.result_bytes;
    root.check_detailed(&amount, AdmissionBoundary::Root)?;
    let mut current = Some(tenant.as_ref());
    let mut ancestor_distance = 0;
    while let Some(ancestor) = current {
        ancestor
            .ledger
            .check_detailed(&amount, AdmissionBoundary::Client { ancestor_distance })?;
        ancestor_distance += 1;
        current = ancestor.parent.as_deref();
    }
    root.add(&amount);
    let mut current = Some(tenant.as_ref());
    while let Some(ancestor) = current {
        ancestor.ledger.add(&amount);
        current = ancestor.parent.as_deref();
    }
    Ok(Debit {
        root: Arc::clone(root),
        tenant: Some(Arc::clone(tenant)),
        amount,
    })
}

#[cfg(test)]
mod storage_root_tests {
    use super::*;
    fn quota() -> QuotaGroup {
        QuotaGroup::new(QuotaLimits {
            clients: 1,
            jobs: 1,
            service_jobs: 0,
            input_bytes: 1024,
            result_bytes: 1024,
            worker_threads: 1,
            worker_bytes: 4096,
        })
    }
    #[test]
    fn original_storage_admission_recognizes_cloned_root() {
        let original = quota();
        let alias = original.clone();
        let storage = original.reserve_external_storage(128).unwrap();
        assert!(storage.shares_root(&original));
        assert!(storage.shares_root(&alias));
        assert_eq!(alias.snapshot().worker_bytes, 128);
    }
    #[test]
    fn independent_root_with_identical_limits_is_not_storage_authority() {
        let original = quota();
        let independent = quota();
        assert_eq!(original.snapshot().limits, independent.snapshot().limits);
        let storage = original.reserve_external_storage(128).unwrap();
        assert!(!storage.shares_root(&independent));
        assert_eq!(original.snapshot().worker_bytes, 128);
        assert_eq!(independent.snapshot().worker_bytes, 0);
    }
    #[test]
    fn inspecting_provenance_never_debits_or_releases_original_storage() {
        let original = quota();
        let independent = quota();
        let storage = original.reserve_external_storage(128).unwrap();
        for _ in 0..16 {
            let _ = storage.shares_root(&original);
            let _ = storage.shares_root(&independent);
            assert_eq!(original.snapshot().worker_bytes, 128);
            assert_eq!(independent.snapshot().worker_bytes, 0);
        }
        drop(storage);
        assert_eq!(original.snapshot().worker_bytes, 0);
        assert_eq!(independent.snapshot().worker_bytes, 0);
    }
}
