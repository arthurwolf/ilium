//! Explicit startup admission without creating an Execution bank or a second supervisor.
//! Only the real process composition designates the permanent supervisor quota.
//! Ordinary independent QuotaGroups and fixture banks never perform that designation.
use crate::{QuotaGroup, RejectReason, WorkerAdmission};
use ilium_platform::owned_worker::{
    initialize_supervisor, reserve_owned_worker, supervisor_declared_bytes, WorkerReservation,
};
use std::fmt;
use std::io;
use std::sync::Mutex;
#[derive(Debug)]
pub enum WorkerStartError {
    Rejected(RejectReason),
    Platform(io::Error),
    DifferentProcessQuota,
}
impl fmt::Display for WorkerStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rejected(reason) => write!(formatter, "worker admission rejected: {reason:?}"),
            Self::Platform(error) => write!(formatter, "worker platform startup: {error}"),
            Self::DifferentProcessQuota => {
                formatter.write_str("process supervisor already belongs to another quota")
            }
        }
    }
}
impl std::error::Error for WorkerStartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Platform(error) => Some(error),
            _ => None,
        }
    }
}
struct ProcessSupervisorOwner {
    quota: QuotaGroup,
    // The supervisor and its physical debit live for the process lifetime.
    _admission: WorkerAdmission,
}
/// Bootstrap only: designate the real process root BEFORE startup logging or other owned workers.
/// Same-root calls reuse the original charge; foreign roots refuse without debiting either group.
/// Never call this from generic Execution::start, QuotaGroup construction, or an independent fixture.
/// true proves this call started the native supervisor; false reports an already existing singleton.
pub fn initialize_process_supervisor(quota: &QuotaGroup) -> Result<bool, WorkerStartError> {
    static PROCESS_OWNER: Mutex<Option<ProcessSupervisorOwner>> = Mutex::new(None);
    let mut installed = PROCESS_OWNER
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(owner) = installed.as_ref() {
        if !owner.quota.shares_root(quota) {
            return Err(WorkerStartError::DifferentProcessQuota);
        }
        return Ok(false);
    }
    let admission = quota
        .reserve_external_worker(1, supervisor_declared_bytes())
        .map_err(WorkerStartError::Rejected)?;
    let started = match initialize_supervisor() {
        Ok(started) => started,
        Err(error) => {
            // Releasing admission can invoke an external wake; leave the designation lock first.
            drop(installed);
            drop(admission);
            return Err(WorkerStartError::Platform(error));
        }
    };
    // Legacy pre-bootstrap creation is reuse, not proof of prior admission.
    *installed = Some(ProcessSupervisorOwner {
        quota: quota.clone(),
        _admission: admission,
    });
    Ok(started)
}
/// Background/bootstrap only: reserve a physical role before building actor channels or captures.
/// Resident bytes cover the caller's worker-lifetime state; add separate storage leases for longer retention.
/// Explicit stack bytes are a Builder request, not a bound on native mappings, guards, TLS, or allocator RSS.
/// This function never installs a process root and is safe to use with independent fixture quotas.
pub fn reserve_admitted_worker(
    quota: &QuotaGroup,
    stack_bytes: usize,
    resident_bytes: usize,
) -> Result<WorkerReservation<WorkerAdmission>, WorkerStartError> {
    if stack_bytes == 0 {
        return Err(WorkerStartError::Rejected(RejectReason::InvalidCost));
    }
    let bytes = stack_bytes
        .checked_add(resident_bytes)
        .ok_or(WorkerStartError::Rejected(RejectReason::InvalidCost))?;
    let admission = quota
        .reserve_external_worker(1, bytes)
        .map_err(WorkerStartError::Rejected)?;
    reserve_owned_worker(Some(stack_bytes), admission).map_err(WorkerStartError::Platform)
}
