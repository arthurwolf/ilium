//! These fixtures use the actual unchanged seven-thread bank and admission APIs.
use super::*;

fn fixture(clients: usize) -> (QuotaGroup, ilium_execution::AdmissionGroup) {
    let mut limits = process_quota().snapshot().limits;
    limits.clients = clients;
    let quota = QuotaGroup::new(limits);
    let aggregate = quota
        .admission_group(ClientLimits {
            jobs: 60,
            service_jobs: 1,
            input_bytes: 512 * MIB,
            result_bytes: 512 * MIB,
        })
        .unwrap();
    (quota, aggregate)
}

#[test]
fn child_admission_failure_retains_actual_general_until_physical_exit() {
    let (quota, aggregate) = fixture(2);
    let mut state = ClientStartupState::default();
    let error = state
        .populate(quota.clone(), bank_config(), &aggregate)
        .unwrap_err();
    assert!(matches!(
        error,
        StartError::Admission(RejectReason::ClientLimit)
    ));
    assert!(state.execution.is_some());
    assert!(state.general.is_some());
    assert!(state.location_search.is_none());
    state.primary = Some(error);
    state.observe_cleanup_background(Instant::now() + Duration::from_secs(5));
    let report = state.observation.as_ref().unwrap().as_ref().unwrap();
    assert!(report.shutdown_complete);
    assert_eq!(
        report.observations.len(),
        CPU_THREADS + IO_THREADS + SERVICE_THREADS
    );
    assert!(state.execution.is_none());
    assert!(state.general.is_none());
    assert_eq!(quota.snapshot().clients, 1);
    drop(aggregate);
    assert_eq!(quota.snapshot().clients, 0);
    assert_eq!(quota.snapshot().worker_threads, 0);
    assert_eq!(quota.snapshot().worker_bytes, 0);
    assert!(matches!(
        state.primary,
        Some(StartError::Admission(RejectReason::ClientLimit))
    ));
}

#[test]
fn busy_retry_requires_disposal_of_the_actual_original_bank() {
    let (quota, aggregate) = fixture(3);
    let mut state = ClientStartupState::default();
    state
        .populate(quota.clone(), bank_config(), &aggregate)
        .unwrap();
    // Synthetic primary Busy isolates retry eligibility; the retained bank is real.
    state.primary = Some(StartError::Admission(RejectReason::Busy));
    let error = ClientExecutionStartError {
        state: Arc::new(Mutex::new(state)),
        observer: None,
    };
    assert!(!error.is_retryable_busy());
    assert!(error.observe_cleanup_background(Instant::now() + Duration::from_secs(5)));
    assert!(error.is_retryable_busy());
    assert_eq!(quota.snapshot().worker_threads, 0);
    assert_eq!(quota.snapshot().clients, 1);
    drop(aggregate);
    assert_eq!(quota.snapshot().clients, 0);
}

#[test]
fn cancelled_async_bootstrap_physically_disposes_its_successful_original_bank() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (quota, aggregate) = fixture(3);
    let background_quota = quota.clone();
    let background_aggregate = aggregate.clone();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    let (finished_tx, finished_rx) = tokio::sync::oneshot::channel();
    runtime.block_on(async move {
        let mut startup = Box::pin(ClientExecution::start_async_with(move |state| {
            state.populate(background_quota, bank_config(), &background_aggregate).unwrap();
            let _ = entered_tx.send(());
            release_rx.recv().unwrap();
        }, move || { let _ = finished_tx.send(()); }));
        tokio::select! {
            result = &mut startup => panic!("controlled bootstrap unexpectedly finished: {}", result.is_ok()),
            entered = entered_rx => entered.unwrap(),
        }
        assert_eq!(quota.snapshot().worker_threads, CPU_THREADS + IO_THREADS + SERVICE_THREADS);
        drop(startup);
        release_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(10), finished_rx).await.unwrap().unwrap();
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
        assert_eq!(quota.snapshot().clients, 1);
        drop(aggregate);
        assert_eq!(quota.snapshot().clients, 0);
    });
}

#[test]
fn startup_failure_is_send_sync_for_typed_root_io_error_custody() {
    fn assert_send_sync<T: Send + Sync + std::error::Error>() {}
    assert_send_sync::<ClientExecutionStartError>();
}

#[test]
fn selected_v8_helper_fits_declared_client_roles_and_retiring_helpers_still_compete() {
    // Isolated ledger, real admission API: no process-global fixture pressure.
    let quota = QuotaGroup::new(process_quota().snapshot().limits);
    let native_roles = CPU_THREADS + IO_THREADS + SERVICE_THREADS + 27 + 3 + 1 + 6;
    let helper_limits = ilium_animation_js::helper::HelperLimits::default();
    let helper_roles = helper_limits.sandbox.maximum_tasks as usize + 2;
    let native = quota.reserve_external_worker(native_roles, 0).unwrap();
    let selected = quota
        .reserve_external_worker(helper_roles, 0)
        .expect("selected V8 helper must fit alongside declared native client roles");
    assert!(matches!(
        quota.reserve_external_worker(helper_roles, 0),
        Err(RejectReason::WorkerLimit)
    ));
    assert_eq!(quota.snapshot().worker_threads, native_roles + helper_roles);
    // An owner retained during physical retirement keeps precisely this debit.
    drop(selected);
    let replacement = quota.reserve_external_worker(helper_roles, 0).unwrap();
    drop(replacement);
    drop(native);
    assert_eq!(quota.snapshot().worker_threads, 0);
}

#[test]
fn panicking_bootstrap_joins_actual_bank_before_returning_typed_observer_failure() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (quota, aggregate) = fixture(3);
    let background_quota = quota.clone();
    let background_aggregate = aggregate.clone();
    let error = runtime
        .block_on(ClientExecution::start_async_with(
            move |state| {
                state
                    .populate(background_quota, bank_config(), &background_aggregate)
                    .unwrap();
                panic!("synthetic bootstrap observer panic after real bank admission");
            },
            || {},
        ))
        .err()
        .expect("actual observer panic");
    assert!(error.observer.as_ref().unwrap().is_panic());
    assert!(!error.retains_execution());
    assert!(!error.is_retryable_busy());
    assert_eq!(quota.snapshot().worker_threads, 0);
    assert_eq!(quota.snapshot().worker_bytes, 0);
    assert_eq!(quota.snapshot().clients, 1);
    drop(aggregate);
    assert_eq!(quota.snapshot().clients, 0);
}
