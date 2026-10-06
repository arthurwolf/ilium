//! Bounded, cooperative worker execution.
//!
//! Create one QuotaGroup at the process composition root and share it among
//! every Execution that must share limits. Different groups and different
//! processes are NOT mutually accounted. Nothing here performs filesystem I/O.
//!
//! Execution::start and join_until_background are background/bootstrap APIs.
//! The interactive APIs only try admission, signal shutdown, or poll receipts.
//! A service is one long-lived Job on the separate prestarted service bank;
//! its actor mailbox and semantic acknowledgements belong to its adapter.
//!
//! JobCost is a cooperative upper-bound declaration, not an allocator or RSS
//! limit. Include owned input/captures and peak scratch in input_bytes; include
//! either success or error storage in result_bytes. Reserve before building
//! large inputs. Library-owned threads, caches and retained clones require
//! separate admission. Bank metadata is charged against worker_bytes until the
//! last monitor/client reference is released. Per-job metadata is bounded by
//! admitted item counts, not payload bytes.
//!
//! Never synchronously wait for another job from any execution-bank callback.
//! Never block a bank callback on delivery to a UI channel. Use a receipt or
//! an adapter-owned bounded mailbox with an explicit full/closed policy.
//! Destructors, panic hooks, and Retained::map transformations must not block
//! the UI. Abort, OOM, unsafe code and double panics are not isolated here.

mod budget;
mod job;
mod pool;
mod retirement;
mod startup; // Admit startup owners without creating a bank or changing fixture semantics.

pub use budget::{
    AdmissionBoundary, AdmissionFailure, AdmissionGroup, ClientLimits, JobCost, QuotaGroup,
    QuotaLimits, QuotaRefusal, QuotaResource, QuotaSnapshot, RejectReason, StorageAdmission,
    WorkerAdmission,
};
pub use ilium_platform::owned_worker::WorkerExit;
pub use ilium_platform::thread_priority::WorkerPriority;
pub use job::{
    Job, JobContext, JobOutcome, JobPoll, Receipt, Rejected, Retained, Retention, SkipReason,
};
pub use pool::{
    Client, Execution, ExecutionConfig, ExecutionMonitor, ExternalReservation, Health,
    JoinObservation, JoinReport, JoinUseError, Lane, LaneConfig, LaneHealth, Phase, Reservation,
    RetirementHandle, ShutdownMode, StartCleanupError, StartError, StartFailure,
};
pub use retirement::{
    RetirementFailure, RetirementReservation, Retiring, RetiringArc, RETIREMENT_SLOTS,
};
//
pub use startup::{
    initialize_process_runtime_admission,
    // Expose the explicit process designation and pre-capture worker reservation.
    initialize_process_supervisor,
    reserve_admitted_worker,
    WorkerStartError, // Reuse existing quota primitives.
}; // End startup ownership exports.
