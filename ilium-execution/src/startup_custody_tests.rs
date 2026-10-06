//! Real native banks; the injected spawn error is an explicitly synthetic fixture.
use super::*;
use crate::QuotaLimits;

fn fixture() -> (QuotaGroup, ExecutionConfig) {
    let lane = |threads| LaneConfig {
        threads,
        queue_slots: if threads == 0 { 0 } else { 2 },
        priority: None,
        resident_bytes_per_thread: 4096,
    };
    (
        QuotaGroup::new(QuotaLimits {
            clients: 2,
            jobs: 2,
            service_jobs: 0,
            input_bytes: 4096,
            result_bytes: 4096,
            worker_threads: 2,
            worker_bytes: 1024 * 1024,
        }),
        ExecutionConfig {
            cpu: lane(2),
            io: lane(0),
            service: lane(0),
        },
    )
}

fn refuse_second(lane: Lane, index: usize) -> io::Result<()> {
    if lane == Lane::Cpu && index == 1 {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "synthetic second spawn refusal",
        ))
    } else {
        Ok(())
    }
}

#[test]
fn partial_spawn_returns_original_bank_and_physically_joins_it() {
    let (quota, config) = fixture();
    let failure = Execution::start_with_spawn_probe(quota.clone(), config, refuse_second)
        .err()
        .expect("second spawn refused");
    let (primary, partial) = failure.into_parts();
    assert!(
        matches!(&primary, StartError::Spawn { lane: Lane::Cpu, index: 1, source }
        if source.kind() == io::ErrorKind::PermissionDenied
        && source.to_string() == "synthetic second spawn refusal")
    );
    let mut bank = partial.expect("actual first native worker retained");
    assert_eq!(bank.workers.len(), 1);
    bank.request_shutdown(ShutdownMode::Cancel);
    let report = bank
        .join_until_background(Instant::now() + Duration::from_secs(5))
        .unwrap();
    assert!(report.shutdown_complete);
    assert_eq!(report.remaining_workers, 0);
    assert_eq!(report.observations.len(), 1);
    assert!(matches!(
        report.observations[0],
        JoinObservation::Exited {
            lane: Lane::Cpu,
            ..
        }
    ));
    drop(bank);
    assert_eq!(quota.snapshot().worker_threads, 0);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}

#[test]
fn legacy_partial_spawn_observes_exit_and_preserves_original_error_kind() {
    let (quota, config) = fixture();
    let failure = Execution::start_with_spawn_probe(quota.clone(), config, refuse_second)
        .err()
        .expect("second spawn refused");
    let error = failure.into_legacy_error();
    assert!(
        matches!(error, StartError::Spawn { lane: Lane::Cpu, index: 1, source }
        if source.kind() == io::ErrorKind::PermissionDenied
        && source.to_string() == "synthetic second spawn refusal")
    );
    assert_eq!(quota.snapshot().worker_threads, 0);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}

#[test]
fn invalid_start_has_no_partial_bank_or_physical_exit_claim() {
    let (quota, mut config) = fixture();
    config.cpu.queue_slots = 0;
    let (error, partial) = Execution::start_with_custody(quota.clone(), config)
        .err()
        .expect("invalid queue")
        .into_parts();
    assert!(matches!(error, StartError::InvalidConfig(_)));
    assert!(partial.is_none());
    assert_eq!(quota.snapshot().worker_threads, 0);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}

#[test]
fn retained_cleanup_error_is_a_typed_send_sync_source() {
    fn assert_send_sync<T: Send + Sync + std::error::Error>() {}
    assert_send_sync::<StartCleanupError>();
}

#[test]
fn deadline_failure_retains_actual_ticket_and_original_source_until_retry_join() {
    let (quota, config) = fixture();
    let mut bank = Execution::start_with_custody(quota.clone(), config).unwrap();
    let client = bank
        .client(ClientLimits {
            jobs: 1,
            service_jobs: 0,
            input_bytes: 1024,
            result_bytes: 1024,
        })
        .unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    // The admitted input includes the channel handles, although their messages
    // and the job result are units. The synthetic held job forces a real physical
    // deadline; construction tests separately prove the partial-bank path.
    let receipt = client
        .try_submit(
            Lane::Cpu,
            JobCost {
                input_bytes: 1024,
                result_bytes: 1024,
            },
            move |_| {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                Ok::<_, ()>(())
            },
        )
        .unwrap();
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    bank.request_shutdown(ShutdownMode::Cancel);
    let observed = bank.join_until_background(Instant::now());
    assert!(
        matches!(&observed, Ok(report) if !report.shutdown_complete && report.remaining_workers > 0)
    );
    let error = StartCleanupError {
        primary: StartError::Spawn {
            lane: Lane::Cpu,
            index: 1,
            source: io::Error::new(
                io::ErrorKind::PermissionDenied,
                "synthetic original spawn source",
            ),
        },
        state: std::sync::Mutex::new(StartCleanupState {
            execution: bank,
            observed,
        }),
    };
    assert!(quota.snapshot().worker_threads > 0);
    assert!(matches!(error.primary(), StartError::Spawn { source, .. }
        if source.kind() == io::ErrorKind::PermissionDenied));
    release_tx.send(()).unwrap();
    assert!(error
        .observe_background(Instant::now() + Duration::from_secs(5))
        .unwrap());
    drop(receipt);
    drop(client);
    drop(error);
    assert_eq!(quota.snapshot().worker_threads, 0);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}
