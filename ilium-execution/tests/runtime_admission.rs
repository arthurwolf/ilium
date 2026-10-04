//! One test per integration process: permanent runtime designation is process
//! state, so unrelated fixture roots must never share this executable's owner.
use ilium_execution::{
    initialize_process_runtime_admission, initialize_process_supervisor, QuotaGroup, QuotaLimits,
    RejectReason, WorkerStartError,
};
use ilium_platform::owned_worker::supervisor_declared_bytes;

fn quota() -> QuotaGroup {
    QuotaGroup::new(QuotaLimits {
        clients: 0,
        jobs: 0,
        service_jobs: 0,
        input_bytes: 0,
        result_bytes: 0,
        worker_threads: 7,
        worker_bytes: supervisor_declared_bytes() + 6 * 2 * 1024 * 1024,
    })
}

#[test]
fn runtime_capacity_refuses_pressure_and_foreign_roots_then_retains_one_process_debit() {
    let process = quota();
    assert!(matches!(
        initialize_process_runtime_admission(&process, 6, 2 * 1024 * 1024),
        Err(WorkerStartError::ProcessNotInitialized)
    ));
    assert_eq!(process.snapshot().worker_threads, 0);
    initialize_process_supervisor(&process).expect("process designation");
    let occupied = process
        .reserve_external_worker(1, 1)
        .expect("temporary competing role");
    assert!(matches!(
        initialize_process_runtime_admission(&process, 6, 2 * 1024 * 1024),
        Err(WorkerStartError::Rejected(RejectReason::WorkerLimit))
    ));
    assert_eq!(
        process.snapshot().worker_threads,
        2,
        "failed admission is reversible"
    );
    drop(occupied);
    let declared_bytes = supervisor_declared_bytes() + 6 * 2 * 1024 * 1024;
    assert!(initialize_process_runtime_admission(&process, 6, 2 * 1024 * 1024).expect("admitted"));
    assert_eq!(process.snapshot().worker_threads, 7);
    assert_eq!(process.snapshot().worker_bytes, declared_bytes);
    assert!(
        !initialize_process_runtime_admission(&process.clone(), 6, 2 * 1024 * 1024)
            .expect("same root")
    );
    assert!(matches!(
        initialize_process_runtime_admission(&process, 5, 2 * 1024 * 1024),
        Err(WorkerStartError::DifferentRuntimeDeclaration)
    ));
    let foreign = quota();
    assert!(matches!(
        initialize_process_runtime_admission(&foreign, 6, 2 * 1024 * 1024),
        Err(WorkerStartError::DifferentProcessQuota)
    ));
    assert_eq!(foreign.snapshot().worker_threads, 0);
    assert_eq!(process.snapshot().worker_threads, 7);
    assert_eq!(process.snapshot().worker_bytes, declared_bytes);
    assert!(matches!(
        process.reserve_external_worker(1, 1),
        Err(RejectReason::WorkerLimit)
    ));
    let observer = process.clone();
    drop(process);
    assert_eq!(
        observer.snapshot().worker_threads,
        7,
        "local handles cannot release live runtime capacity"
    );
}
