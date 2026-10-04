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
    ProcessNotInitialized,
    DifferentRuntimeDeclaration,
}
impl fmt::Display for WorkerStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rejected(reason) => write!(formatter, "worker admission rejected: {reason:?}"),
            Self::Platform(error) => write!(formatter, "worker platform startup: {error}"),
            Self::DifferentProcessQuota => {
                formatter.write_str("process resources already belong to another quota")
            }
            Self::ProcessNotInitialized => {
                formatter.write_str("process resources are not initialized")
            }
            Self::DifferentRuntimeDeclaration => {
                formatter.write_str("process runtime capacity was already declared differently")
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
    runtime: Option<ProcessRuntimeAdmission>,
}
struct ProcessRuntimeAdmission {
    thread_capacity: usize,
    stack_bytes: usize,
    _admission: WorkerAdmission,
}
static PROCESS_OWNER: Mutex<Option<ProcessSupervisorOwner>> = Mutex::new(None);
/// Bootstrap only: designate the real process root BEFORE startup logging or other owned workers.
/// Same-root calls reuse the original charge; foreign roots refuse without debiting either group.
/// Never call this from generic Execution::start, QuotaGroup construction, or an independent fixture.
/// true proves this call started the native supervisor; false reports an already existing singleton.
pub fn initialize_process_supervisor(quota: &QuotaGroup) -> Result<bool, WorkerStartError> {
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
        runtime: None,
    });
    Ok(started)
}

/// Binary bootstrap only, after designating the process root and before building
/// the sole Tokio runtime. This accounts for existing library threads; it starts
/// no runtime, pool, or worker. The declaration includes async and maximum
/// blocking thread capacity, with the same explicit stack request as Builder.
///
/// Keep this debit for the process lifetime: Tokio's timed shutdown may leave
/// blocking callbacks alive, and dropping a local guard would release their
/// physical capacity early. Same-root/same-capacity repeats are idempotent;
/// independent fixture banks neither install nor adopt this declaration.
pub fn initialize_process_runtime_admission(
    quota: &QuotaGroup,
    thread_capacity: usize,
    stack_bytes: usize,
) -> Result<bool, WorkerStartError> {
    if thread_capacity == 0 || stack_bytes == 0 {
        return Err(WorkerStartError::Rejected(RejectReason::InvalidCost));
    }
    let bytes = thread_capacity
        .checked_mul(stack_bytes)
        .ok_or(WorkerStartError::Rejected(RejectReason::InvalidCost))?;
    let mut installed = PROCESS_OWNER
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let owner = installed
        .as_mut()
        .ok_or(WorkerStartError::ProcessNotInitialized)?;
    if !owner.quota.shares_root(quota) {
        return Err(WorkerStartError::DifferentProcessQuota);
    }
    if let Some(runtime) = &owner.runtime {
        if runtime.thread_capacity != thread_capacity || runtime.stack_bytes != stack_bytes {
            return Err(WorkerStartError::DifferentRuntimeDeclaration);
        }
        return Ok(false);
    }
    let admission = quota
        .reserve_external_worker(thread_capacity, bytes)
        .map_err(WorkerStartError::Rejected)?;
    owner.runtime = Some(ProcessRuntimeAdmission {
        thread_capacity,
        stack_bytes,
        _admission: admission,
    });
    Ok(true)
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
