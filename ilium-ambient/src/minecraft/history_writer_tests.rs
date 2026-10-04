//! Synthetic private repository tests; no real worlds or presentation I/O.
use super::super::{history_store, tours::Controller};
use super::*;
use std::time::{Duration, Instant};

fn fixture() -> (tempfile::TempDir, Repository, std::path::PathBuf) {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("private synthetic History");
    let repository = Repository::new(directory.clone()).unwrap();
    (temporary, repository, directory)
}
fn history(completed: u64) -> History {
    let mut value = serde_json::to_value(History::default()).unwrap();
    value["completed"] = completed.into();
    value["revision"] = completed.into();
    let history: History = serde_json::from_value(value).unwrap();
    Controller::new(1, history).unwrap();
    history
}
// Test-only finite waiting; production callbacks never block/wait for receipts.
fn receipt(writer: &Writer) -> Receipt {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match writer.take_receipt() {
            Ok(Some(value)) => return value,
            Ok(None) | Err(Error::Busy) => (),
            error => panic!("unexpected receipt error: {error:?}"),
        }
        assert!(Instant::now() < deadline, "worker failed to issue receipt");
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn submit(writer: &Writer, serial: u64, history: History) -> Admission {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match writer.submit(serial, history) {
            Ok(value) => return value,
            Err(Error::Busy) => (),
            error => panic!("unexpected admission error: {error:?}"),
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}
#[test]
fn accepted_snapshot_commits_and_reloads_actual_history() {
    let (_temporary, repository, _) = fixture();
    let writer = Writer::start(repository.clone(), 0, &crate::resources::test_resources()).unwrap();
    submit(&writer, 1, history(1));
    let result = receipt(&writer);
    assert_eq!(result.serial, 1);
    assert_eq!(result.status, Status::Committed);
    assert_eq!(result.revision, Some(1));
    assert!(result.error.is_none());
    assert_eq!(repository.load(&|| false).unwrap().history(), history(1));
}
#[test]
fn own_sequential_commits_advance_expected_cas_without_self_conflict() {
    let (_temporary, repository, _) = fixture();
    let writer = Writer::start(repository.clone(), 0, &crate::resources::test_resources()).unwrap();
    for serial in 1..=4 {
        submit(&writer, serial, history(serial));
        let result = receipt(&writer);
        assert_eq!(result.status, Status::Committed);
        assert_eq!(result.revision, Some(serial));
    }
    assert_eq!(repository.load(&|| false).unwrap().history(), history(4));
}
#[test]
fn zero_duplicate_and_backwards_serials_never_replace_accepted_latest() {
    let (_temporary, repository, _) = fixture();
    let writer = Writer::start(repository.clone(), 0, &crate::resources::test_resources()).unwrap();
    assert_eq!(writer.submit(0, history(0)), Err(Error::StaleSerial));
    submit(&writer, 3, history(3));
    receipt(&writer);
    assert_eq!(writer.submit(3, history(9)), Err(Error::StaleSerial));
    assert_eq!(writer.submit(2, history(9)), Err(Error::StaleSerial));
    assert_eq!(repository.load(&|| false).unwrap().history(), history(3));
}
#[test]
fn external_conflict_is_sticky_and_never_overwrites_authoritative_history() {
    let (_temporary, repository, _) = fixture();
    let writer = Writer::start(repository.clone(), 0, &crate::resources::test_resources()).unwrap();
    repository.commit_history(0, history(5), &|| false).unwrap();
    submit(&writer, 1, history(1));
    let result = receipt(&writer);
    assert_eq!(result.status, Status::ReloadRequired);
    assert_eq!(result.revision, Some(1));
    assert!(result.error.as_ref().unwrap().len() <= 512);
    assert_eq!(writer.submit(2, history(2)), Err(Error::ReloadRequired));
    assert_eq!(repository.load(&|| false).unwrap().history(), history(5));
}
#[test]
fn corruption_remains_untouched_and_produces_bounded_sticky_failure() {
    let (_temporary, repository, directory) = fixture();
    repository.load(&|| false).unwrap();
    std::fs::write(
        directory.join("history.json"),
        b"synthetic corrupt authored data",
    )
    .unwrap();
    let writer = Writer::start(repository, 0, &crate::resources::test_resources()).unwrap();
    submit(&writer, 1, history(1));
    let result = receipt(&writer);
    assert_eq!(result.status, Status::ReloadRequired);
    assert!(result.error.unwrap().len() <= 512);
    assert_eq!(writer.submit(2, history(2)), Err(Error::ReloadRequired));
    assert_eq!(
        std::fs::read(directory.join("history.json")).unwrap(),
        b"synthetic corrupt authored data"
    );
}
#[test]
fn explicit_close_drains_latest_and_remains_nonblocking() {
    let (_temporary, repository, _) = fixture();
    let mut writer =
        Writer::start(repository.clone(), 0, &crate::resources::test_resources()).unwrap();
    submit(&writer, 1, history(1));
    let before = Instant::now();
    writer.close();
    assert!(before.elapsed() < Duration::from_millis(100));
    assert_eq!(writer.submit(2, history(2)), Err(Error::Closed));
    assert_eq!(receipt(&writer).status, Status::Committed);
    assert_eq!(repository.load(&|| false).unwrap().history(), history(1));
}
#[test]
fn busy_repository_retry_is_finite_and_rejects_further_admission_after_failure() {
    let (_temporary, repository, directory) = fixture();
    repository.load(&|| false).unwrap();
    let root = ilium_platform::secure_fs::NoFollowDirectory::open_root(&directory).unwrap();
    let lock = ilium_platform::file_lock::ExclusiveFileLock::try_acquire_opened(
        root.open_regular("history.lock".as_ref()).unwrap(),
    )
    .unwrap()
    .unwrap();
    let writer = Writer::start(repository, 0, &crate::resources::test_resources()).unwrap();
    submit(&writer, 1, history(1));
    let before = Instant::now();
    let result = receipt(&writer);
    assert!(before.elapsed() < Duration::from_secs(1));
    assert_eq!(result.status, Status::ReloadRequired);
    assert!(result.error.unwrap().contains("busy"));
    assert_eq!(writer.submit(2, history(2)), Err(Error::ReloadRequired));
    drop(lock);
}

fn gated_writer(
    repository: Repository,
) -> (
    Writer,
    std::sync::mpsc::Receiver<()>,
    std::sync::mpsc::Sender<()>,
) {
    let (entered_sender, entered) = std::sync::mpsc::channel();
    let (release, gate) = std::sync::mpsc::channel();
    let initial = AtomicBool::new(true);
    let writer = Writer::start_inner(
        repository,
        0,
        &crate::resources::test_resources(),
        move |repository, revision, history| {
            if initial.swap(false, Ordering::Relaxed) {
                entered_sender.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            commit(repository, revision, history)
        },
    )
    .unwrap();
    (writer, entered, release)
}

#[test]
fn admission_close_is_not_a_catalog_binding_barrier_while_commit_is_in_flight() {
    let (_temporary, repository, _) = fixture();
    let (mut writer, entered, release) = gated_writer(repository.clone());
    assert!(!writer.is_drained());
    submit(&writer, 1, history(1));
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    submit(&writer, 2, history(2));
    writer.close();
    let incorrectly_drained = writer.is_drained();
    release.send(()).unwrap();
    assert!(
        !incorrectly_drained,
        "catalog binding must not race an accepted, blocked history write"
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while !writer.is_drained() {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    let final_receipt = writer.take_receipt().unwrap().unwrap();
    assert_eq!(final_receipt.serial, 2);
    assert_eq!(final_receipt.status, Status::Committed);
    let authoritative = repository.load(&|| false).unwrap();
    assert_eq!(authoritative.history(), history(2));
    assert_eq!(authoritative.revision(), final_receipt.revision.unwrap());
}

#[test]
fn drained_failure_never_certifies_history_commit_or_reopens_admission() {
    let (_temporary, repository, _) = fixture();
    repository.commit_history(0, history(9), &|| false).unwrap();
    let writer = Writer::start(repository.clone(), 0, &crate::resources::test_resources()).unwrap();
    submit(&writer, 1, history(1));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !writer.is_drained() {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    let result = writer.take_receipt().unwrap().unwrap();
    assert_eq!(result.status, Status::ReloadRequired);
    assert_eq!(result.revision, Some(1));
    assert_eq!(writer.submit(2, history(2)), Err(Error::ReloadRequired));
    assert_eq!(repository.load(&|| false).unwrap().history(), history(9));
}

#[test]
fn rapid_full_snapshots_coalesce_to_latest_and_close_drains_both_accepted_slots() {
    let (_temporary, repository, _) = fixture();
    let (mut writer, entered, release) = gated_writer(repository.clone());
    submit(&writer, 1, history(1));
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(submit(&writer, 2, history(2)), Admission::Queued);
    for serial in 3..=100 {
        assert_eq!(
            submit(&writer, serial, history(serial)),
            Admission::Coalesced
        );
    }
    assert!(writer.take_receipt().unwrap().is_none());
    let before = Instant::now();
    writer.close();
    assert!(
        before.elapsed() < Duration::from_millis(100),
        "close must not join a blocked commit"
    );
    assert!(writer.shared.closed.load(Ordering::Acquire));
    assert_eq!(writer.submit(101, history(101)), Err(Error::Closed));
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let result = receipt(&writer);
        assert_eq!(result.status, Status::Committed);
        if result.serial == 100 {
            assert_eq!(result.revision, Some(2));
            break;
        }
        assert!(Instant::now() < deadline);
    }
    let saved = repository.load(&|| false).unwrap();
    assert_eq!(
        saved.revision(),
        2,
        "only one in-flight plus latest coalesced snapshot committed"
    );
    assert_eq!(saved.history(), history(100));
}

#[test]
fn dropping_blocked_writer_drains_pending_on_background_cleanup_owner() {
    let (_temporary, repository, _) = fixture();
    let (writer, entered, release) = gated_writer(repository.clone());
    submit(&writer, 1, history(1));
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    submit(&writer, 2, history(2));
    let shared = Arc::clone(&writer.shared);
    let before = Instant::now();
    drop(writer);
    assert!(before.elapsed() < Duration::from_millis(100));
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if worker_state(&shared)
            .receipt
            .as_ref()
            .is_some_and(|receipt| receipt.serial == 2)
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    let saved = repository.load(&|| false).unwrap();
    assert_eq!(saved.history(), history(2));
    assert_eq!(saved.revision(), 2);
}

#[test]
fn presentation_lock_contention_is_immediate_and_never_accepts_a_serial() {
    let (_temporary, repository, _) = fixture();
    let writer = Writer::start(repository, 0, &crate::resources::test_resources()).unwrap();
    let guard = writer.shared.state.lock().unwrap();
    let before = Instant::now();
    assert_eq!(writer.submit(1, history(1)), Err(Error::Busy));
    assert_eq!(writer.take_receipt(), Err(Error::Busy));
    assert!(before.elapsed() < Duration::from_millis(100));
    drop(guard);
    submit(&writer, 1, history(1));
    assert_eq!(receipt(&writer).status, Status::Committed);
}

#[test]
fn published_error_reloads_authority_but_stays_sticky_instead_of_retrying_old_revision() {
    let (_temporary, repository, _) = fixture();
    let writer = Writer::start_inner(
        repository.clone(),
        0,
        &crate::resources::test_resources(),
        |repository, revision, history| {
            // Inject only the post-publication confirmation failure. Actual
            // repository rename/readback and authoritative reload remain real.
            let committed = repository.commit_history(revision, history, &|| false)?;
            Err(history_store::Error::Published {
                revision: committed.revision(),
                source: Box::new(history_store::Error::Io(std::io::Error::other(
                    "synthetic confirmation failure",
                ))),
            })
        },
    )
    .unwrap();
    submit(&writer, 1, history(1));
    let result = receipt(&writer);
    assert_eq!(result.status, Status::ReloadRequired);
    assert_eq!(result.revision, Some(1));
    assert!(result
        .error
        .unwrap()
        .contains("authoritative reload observed revision 1"));
    assert_eq!(writer.submit(2, history(2)), Err(Error::ReloadRequired));
    let saved = repository.load(&|| false).unwrap();
    assert_eq!(saved.revision(), 1);
    assert_eq!(saved.history(), history(1));
}

#[test]
fn published_error_with_failed_reload_never_claims_an_observed_revision() {
    let (_temporary, repository, directory) = fixture();
    let corrupt = b"synthetic corruption after publication";
    let writer = Writer::start_inner(
        repository,
        0,
        &crate::resources::test_resources(),
        move |repository, revision, history| {
            let committed = repository.commit_history(revision, history, &|| false)?;
            std::fs::write(directory.join("history.json"), corrupt).unwrap();
            Err(history_store::Error::Published {
                revision: committed.revision(),
                source: Box::new(history_store::Error::Io(std::io::Error::other(
                    "synthetic confirmation failure",
                ))),
            })
        },
    )
    .unwrap();
    submit(&writer, 1, history(1));
    let result = receipt(&writer);
    assert_eq!(result.status, Status::ReloadRequired);
    assert_eq!(result.revision, None);
    assert!(result
        .error
        .unwrap()
        .contains("authoritative reload failed"));
    assert_eq!(writer.submit(2, history(2)), Err(Error::ReloadRequired));
}

#[test]
fn worker_panic_reports_latest_accepted_serial_and_never_silently_loses_pending_history() {
    let (_temporary, repository, _) = fixture();
    let (entered_sender, entered) = std::sync::mpsc::channel();
    let (release, gate) = std::sync::mpsc::channel();
    let writer = Writer::start_inner(
        repository.clone(),
        0,
        &crate::resources::test_resources(),
        move |_, _, _| {
            entered_sender.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            panic!("synthetic worker fault before publication");
        },
    )
    .unwrap();
    submit(&writer, 1, history(1));
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    submit(&writer, 2, history(2));
    release.send(()).unwrap();
    let result = receipt(&writer);
    assert_eq!(result.serial, 2);
    assert_eq!(result.status, Status::ReloadRequired);
    assert!(result.error.unwrap().contains("panicked"));
    assert_eq!(writer.submit(3, history(3)), Err(Error::ReloadRequired));
    assert_eq!(repository.load(&|| false).unwrap().revision(), 0);
}

#[test]
fn long_unicode_worker_error_is_utf8_and_at_most_512_bytes() {
    let (_temporary, repository, _) = fixture();
    let writer = Writer::start_inner(
        repository,
        0,
        &crate::resources::test_resources(),
        |_, _, _| {
            Err(history_store::Error::Io(std::io::Error::other(
                "雪".repeat(1000),
            )))
        },
    )
    .unwrap();
    submit(&writer, 1, history(1));
    let message = receipt(&writer).error.unwrap();
    assert!(message.len() <= 512);
    assert!(message.contains('雪'));
    assert_eq!(writer.submit(2, history(2)), Err(Error::ReloadRequired));
}

#[test]
fn independently_stopped_worker_drains_accepted_latest_then_closes_admission() {
    let (_temporary, repository, _) = fixture();
    let (writer, entered, release) = gated_writer(repository.clone());
    submit(&writer, 1, history(1));
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    writer
        .worker
        .as_ref()
        .unwrap()
        .stop_flag()
        .store(true, Ordering::Relaxed);
    submit(&writer, 2, history(2));
    release.send(()).unwrap();
    // The stop flag does not cancel accepted disk transactions. Observe their
    // terminal lifecycle with the same finite budget as the other disk tests;
    // closure alone cannot distinguish a slow drain from a failed commit.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !writer.is_drained() {
        assert!(
            Instant::now() < deadline,
            "stopped worker did not finish accepted transactions"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let final_receipt = writer.take_receipt().unwrap().unwrap();
    assert_eq!(final_receipt.serial, 2);
    assert_eq!(final_receipt.status, Status::Committed);
    assert_eq!(final_receipt.revision, Some(2));
    assert!(final_receipt.error.is_none());
    assert!(writer.shared.closed.load(Ordering::Acquire));
    assert_eq!(writer.submit(3, history(3)), Err(Error::Closed));
    let saved = repository.load(&|| false).unwrap();
    assert_eq!(saved.revision(), 2);
    assert_eq!(saved.history(), history(2));
}

#[test]
fn transient_actual_repository_lock_contention_recovers_within_finite_retry_budget() {
    let (_temporary, repository, directory) = fixture();
    repository.load(&|| false).unwrap();
    let root = ilium_platform::secure_fs::NoFollowDirectory::open_root(&directory).unwrap();
    let mut lock = Some(
        ilium_platform::file_lock::ExclusiveFileLock::try_acquire_opened(
            root.open_regular("history.lock".as_ref()).unwrap(),
        )
        .unwrap()
        .unwrap(),
    );
    let mut attempts = 0;
    let result = retry_busy(|| {
        attempts += 1;
        if attempts == 3 {
            drop(lock.take());
        }
        repository.commit_history(0, history(1), &|| false)
    })
    .unwrap();
    assert_eq!(attempts, 3);
    assert_eq!(result.revision(), 1);
    assert_eq!(repository.load(&|| false).unwrap().history(), history(1));
}

#[test]
fn invalid_history_failure_preserves_previous_authority() {
    let (_temporary, repository, _) = fixture();
    repository.commit_history(0, history(1), &|| false).unwrap();
    let writer = Writer::start(repository.clone(), 1, &crate::resources::test_resources()).unwrap();
    let mut value = serde_json::to_value(History::default()).unwrap();
    value["completed"] = u64::MAX.into();
    let invalid = serde_json::from_value(value).unwrap();
    submit(&writer, 1, invalid);
    assert_eq!(receipt(&writer).status, Status::ReloadRequired);
    assert_eq!(repository.load(&|| false).unwrap().history(), history(1));
    assert_eq!(repository.load(&|| false).unwrap().revision(), 1);
}

fn isolated_writer_host() -> (
    ilium_execution::Execution,
    crate::resources::AmbientResources,
    ilium_execution::QuotaGroup,
) {
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits,
    };
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 2,
        jobs: 2,
        service_jobs: 0,
        input_bytes: 4096,
        result_bytes: 4096,
        worker_threads: 2,
        worker_bytes: HISTORY_WORKER_BYTES + 64 * 1024,
    });
    let disabled = LaneConfig {
        threads: 0,
        queue_slots: 0,
        priority: None,
        resident_bytes_per_thread: 0,
    };
    let execution = Execution::start(
        quota.clone(),
        ExecutionConfig {
            cpu: LaneConfig {
                threads: 1,
                queue_slots: 2,
                priority: None,
                resident_bytes_per_thread: 1024,
            },
            io: disabled,
            service: disabled,
        },
    )
    .unwrap();
    let client = execution
        .client(ClientLimits {
            jobs: 2,
            service_jobs: 0,
            input_bytes: 4096,
            result_bytes: 4096,
        })
        .unwrap();
    (
        execution,
        crate::resources::AmbientResources::new(client),
        quota,
    )
}

#[test]
fn blocked_commit_keeps_its_independent_host_claim_after_writer_drop_until_join() {
    let (_temporary, repository, _) = fixture();
    let (mut execution, resources, quota) = isolated_writer_host();
    let baseline = quota.snapshot().worker_bytes;
    let (entered_sender, entered) = std::sync::mpsc::sync_channel(1);
    let (release, gate) = std::sync::mpsc::sync_channel(1);
    let writer = Writer::start_inner(
        repository,
        0,
        &resources,
        move |repository, revision, history| {
            entered_sender.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            commit(repository, revision, history)
        },
    )
    .unwrap();
    let ticket = writer.worker.as_ref().unwrap().join_observer().unwrap();
    submit(&writer, 1, history(1));
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    drop(writer);
    assert_eq!(quota.snapshot().worker_threads, 2);
    assert_eq!(
        quota.snapshot().worker_bytes,
        baseline + HISTORY_WORKER_BYTES
    );
    assert!(resources
        .reserve_worker(WorkerCost {
            threads: 1,
            resident_bytes: HISTORY_WORKER_BYTES
        })
        .is_err());
    assert!(ticket.exit().is_none());
    release.send(()).unwrap();
    ticket
        .join_until(Instant::now() + Duration::from_secs(5))
        .unwrap();
    assert_eq!(quota.snapshot().worker_threads, 2);
    drop(ticket);
    assert_eq!(quota.snapshot().worker_threads, 1);
    assert_eq!(quota.snapshot().worker_bytes, baseline);
    execution.request_shutdown(ilium_execution::ShutdownMode::Cancel);
    execution
        .join_until_background(Instant::now() + Duration::from_secs(5))
        .unwrap();
}

#[test]
fn rejected_history_thread_admission_never_starts_commit_callback() {
    let (_temporary, repository, _) = fixture();
    let (mut execution, resources, quota) = isolated_writer_host();
    let claim = resources
        .reserve_worker(WorkerCost {
            threads: 1,
            resident_bytes: HISTORY_WORKER_BYTES,
        })
        .unwrap();
    let called = Arc::new(AtomicBool::new(false));
    let callback_called = Arc::clone(&called);
    let result = Writer::start_inner(
        repository,
        0,
        &resources,
        move |repository, revision, history| {
            callback_called.store(true, Ordering::Release);
            commit(repository, revision, history)
        },
    );
    assert!(result.is_err());
    assert!(!called.load(Ordering::Acquire));
    assert_eq!(quota.snapshot().worker_threads, 2);
    drop(claim);
    execution.request_shutdown(ilium_execution::ShutdownMode::Cancel);
    execution
        .join_until_background(Instant::now() + Duration::from_secs(5))
        .unwrap();
}
