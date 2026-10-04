use ilium_execution::{
    initialize_process_supervisor, reserve_admitted_worker, QuotaGroup, QuotaLimits, RejectReason,
    WorkerExit, WorkerStartError,
};
use ilium_platform::owned_worker::{
    supervisor_declared_bytes, supervisor_status, StopToken, WorkerKind,
};
use std::{
    cell::RefCell,
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};

const STACK: usize = 256 * 1024;
const RESIDENT: usize = 4096;

fn quota(threads: usize, bytes: usize) -> QuotaGroup {
    QuotaGroup::new(QuotaLimits {
        clients: 0,
        jobs: 0,
        service_jobs: 0,
        input_bytes: 0,
        result_bytes: 0,
        worker_threads: threads,
        worker_bytes: bytes,
    })
}

struct TlsBarrier {
    entered: Sender<()>,
    release: Receiver<()>,
}
impl Drop for TlsBarrier {
    fn drop(&mut self) {
        let _ = self.entered.send(());
        let _ = self.release.recv_timeout(Duration::from_secs(5));
    }
}
thread_local! {
    static TLS_BARRIER: RefCell<Option<TlsBarrier>> = const { RefCell::new(None) };
}

// This integration binary has one test, so its process-global designation
// cannot race another test's unrelated QuotaGroup. The platform's singleton
// may service other test binaries, but those are different processes.
#[test]
fn startup_designates_one_supervisor_and_keeps_fixture_native_leases_until_join() {
    assert!(
        supervisor_status().is_none(),
        "no implicit pre-bootstrap supervisor"
    );
    let process = quota(1, supervisor_declared_bytes());
    assert!(initialize_process_supervisor(&process).expect("first explicit bootstrap"));
    let first = supervisor_status().expect("installed supervisor");
    assert_eq!(first.registered_workers, 0);
    assert_eq!(first.reserved_workers, 0);
    assert!(first.registry_capacity >= 1024);
    assert!(first.wake_capacity >= 1024);
    assert_eq!(process.snapshot().worker_threads, 1);
    assert_eq!(process.snapshot().worker_bytes, supervisor_declared_bytes());
    assert!(!initialize_process_supervisor(&process).expect("idempotent same root"));
    assert_eq!(process.snapshot().worker_threads, 1);
    let foreign = quota(1, supervisor_declared_bytes());
    assert!(matches!(
        initialize_process_supervisor(&foreign),
        Err(WorkerStartError::DifferentProcessQuota)
    ));
    assert_eq!(foreign.snapshot().worker_threads, 0);
    assert_eq!(
        supervisor_status().expect("still installed").thread_id,
        first.thread_id
    );

    let fixture_a = quota(1, STACK + RESIDENT);
    let fixture_b = quota(1, STACK + RESIDENT);
    assert!(!fixture_a.shares_root(&fixture_b));
    let (tls_entered_tx, tls_entered_rx) = mpsc::channel();
    let (tls_release_tx, tls_release_rx) = mpsc::channel();
    let native_a = reserve_admitted_worker(&fixture_a, STACK, RESIDENT)
        .expect("independent fixture admission")
        .spawn(
            "tls-fixture",
            WorkerKind::Cooperative,
            StopToken::default(),
            || {},
            move |_| {
                TLS_BARRIER.with(|slot| {
                    *slot.borrow_mut() = Some(TlsBarrier {
                        entered: tls_entered_tx,
                        release: tls_release_rx,
                    });
                });
            },
        )
        .expect("native fixture A");
    let ticket_a = native_a.ticket();
    assert_eq!(ticket_a.metadata().requested_stack_bytes, Some(STACK));
    assert_eq!(ticket_a.metadata().kind, WorkerKind::Cooperative);
    drop(native_a);
    tls_entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("native TLS destructor parked");
    assert_eq!(
        ticket_a.exit(),
        None,
        "callback return is not actual native retirement"
    );
    assert_eq!(fixture_a.snapshot().worker_threads, 1);
    assert_eq!(fixture_a.snapshot().worker_bytes, STACK + RESIDENT);
    let join_deadline = Instant::now() + Duration::from_secs(5);
    while supervisor_status()
        .expect("singleton")
        .joining_worker
        .is_none_or(|worker| worker.worker_id != ticket_a.id())
    {
        assert!(
            Instant::now() < join_deadline,
            "supervisor never entered A's TLS join"
        );
        std::thread::sleep(Duration::from_millis(1));
    }

    let native_b = reserve_admitted_worker(&fixture_b, STACK, RESIDENT)
        .expect("second independent fixture admission")
        .spawn(
            "second-fixture",
            WorkerKind::Cooperative,
            StopToken::default(),
            || {},
            |_| {},
        )
        .expect("native fixture B");
    let ticket_b = native_b.ticket();
    drop(native_b);
    // The sole supervisor is blocked joining A's TLS. B remains charged
    // and its ticket cannot report a join until that first native join ends.
    assert_eq!(fixture_b.snapshot().worker_threads, 1);
    assert_eq!(ticket_b.exit(), None);
    tls_release_tx.send(()).expect("release TLS");
    assert_eq!(
        ticket_a.join_until(Instant::now() + Duration::from_secs(5)),
        Ok(WorkerExit::Joined)
    );
    assert_eq!(
        ticket_b.join_until(Instant::now() + Duration::from_secs(5)),
        Ok(WorkerExit::Joined)
    );
    assert_eq!(fixture_a.snapshot().worker_threads, 0);
    assert_eq!(fixture_b.snapshot().worker_threads, 0);
    assert_eq!(fixture_a.snapshot().worker_bytes, 0);
    assert_eq!(fixture_b.snapshot().worker_bytes, 0);
    assert_eq!(
        process.snapshot().worker_threads,
        1,
        "permanent charge stays with process"
    );
    assert_eq!(
        supervisor_status().expect("singleton").thread_id,
        first.thread_id
    );

    let exhausted = quota(0, STACK + RESIDENT);
    assert!(matches!(
        reserve_admitted_worker(&exhausted, STACK, RESIDENT),
        Err(WorkerStartError::Rejected(RejectReason::WorkerLimit))
    ));
    assert_eq!(exhausted.snapshot().worker_threads, 0);
    assert_eq!(
        supervisor_status()
            .expect("no new child")
            .registered_workers,
        0
    );

    let rejected = quota(1, STACK + RESIDENT);
    let reservation =
        reserve_admitted_worker(&rejected, STACK, RESIDENT).expect("pre-native admission");
    assert_eq!(rejected.snapshot().worker_threads, 1);
    let body_ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let body_flag = std::sync::Arc::clone(&body_ran);
    let failure = reservation.spawn(
        "bad\0name",
        WorkerKind::Cooperative,
        StopToken::default(),
        || {},
        move |_| {
            body_flag.store(true, std::sync::atomic::Ordering::Release);
        },
    );
    assert!(
        failure.is_err(),
        "invalid native Builder name refuses before spawn"
    );
    assert!(!body_ran.load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(rejected.snapshot().worker_threads, 0);
    assert_eq!(rejected.snapshot().worker_bytes, 0);
    assert_eq!(
        supervisor_status().expect("slot released").reserved_workers,
        0
    );
}
