use ilium_execution::{
    ClientLimits, Execution, ExecutionConfig, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane,
    LaneConfig, QuotaGroup, QuotaLimits, RejectReason, ShutdownMode,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Barrier, Condvar, Mutex};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(5);
const COST: JobCost = JobCost {
    input_bytes: 256,
    result_bytes: 32,
};

fn limits() -> QuotaLimits {
    QuotaLimits {
        clients: 8,
        jobs: 64,
        service_jobs: 2,
        input_bytes: 64 * 1024,
        result_bytes: 64 * 1024,
        worker_threads: 8,
        worker_bytes: 8 * 1024 * 1024,
    }
}

fn client_limits() -> ClientLimits {
    ClientLimits {
        jobs: 64,
        service_jobs: 2,
        input_bytes: 64 * 1024,
        result_bytes: 64 * 1024,
    }
}

fn lane(threads: usize, queue_slots: usize) -> LaneConfig {
    LaneConfig {
        threads,
        queue_slots,
        priority: None,
        resident_bytes_per_thread: 1024,
    }
}

fn config(cpu_queue: usize, io_queue: usize, service_threads: usize) -> ExecutionConfig {
    ExecutionConfig {
        cpu: lane(1, cpu_queue),
        io: lane(1, io_queue),
        service: lane(service_threads, service_threads),
    }
}

fn joined(execution: &mut Execution) {
    let report = execution
        .join_until_background(Instant::now() + WAIT)
        .expect("background join allowed after shutdown");
    assert_eq!(report.remaining_workers, 0, "all physical workers exited");
}

fn wait_for_release(event: &Arc<(Mutex<()>, Condvar)>, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    let mut guard = event.0.lock().expect("release event lock");
    while !ready() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "admission release not observed");
        let (next, timeout) = event
            .1
            .wait_timeout(guard, remaining)
            .expect("release wait");
        guard = next;
        assert!(
            !timeout.timed_out() || ready(),
            "admission release deadline"
        );
    }
}

fn outcome<J: Job>(
    receipt: &mut ilium_execution::Receipt<J>,
) -> ilium_execution::Retained<JobOutcome<J>> {
    match receipt.try_take() {
        JobPoll::Ready(value) => value,
        JobPoll::Pending => panic!("joined bank must publish a terminal outcome"),
        JobPoll::Lost => panic!("accepted work lost its outcome"),
        JobPoll::Taken => panic!("receipt was already consumed"),
    }
}

#[test]
fn real_threads_and_cpu_io_lanes_progress_independently() {
    let quota = QuotaGroup::new(limits());
    let mut execution = Execution::start(quota, config(2, 2, 0)).expect("start");
    let client = execution.client(client_limits()).expect("client");
    let caller = std::thread::current().id();
    let (cpu_started_tx, cpu_started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let mut cpu = client
        .try_submit(Lane::Cpu, COST, move |_| {
            cpu_started_tx
                .send(std::thread::current().id())
                .expect("start evidence");
            release_rx.recv_timeout(WAIT).expect("release CPU");
            Ok::<_, ()>(())
        })
        .expect("CPU admitted");
    let cpu_thread = cpu_started_rx
        .recv_timeout(WAIT)
        .expect("CPU entered callback");
    let (io_tx, io_rx) = mpsc::sync_channel(1);
    let mut io = client
        .try_submit(Lane::Io, COST, move |_| {
            io_tx
                .send(std::thread::current().id())
                .expect("I/O evidence");
            Ok::<_, ()>(())
        })
        .expect("I/O admitted while CPU blocked");
    let io_thread = io_rx
        .recv_timeout(WAIT)
        .expect("I/O progressed during CPU block");
    assert_ne!(cpu_thread, caller);
    assert_ne!(io_thread, caller);
    assert_ne!(cpu_thread, io_thread);
    release_tx.send(()).expect("release CPU");
    execution.request_shutdown(ShutdownMode::Drain);
    joined(&mut execution);
    assert!(matches!(
        outcome(&mut cpu).view(),
        JobOutcome::Finished(Ok(()))
    ));
    assert!(matches!(
        outcome(&mut io).view(),
        JobOutcome::Finished(Ok(()))
    ));
}

#[test]
fn cpu_only_bank_supports_the_existing_standalone_codec_configuration() {
    let quota = QuotaGroup::new(limits());
    let mut execution = Execution::start(
        quota,
        ExecutionConfig {
            cpu: lane(1, 2),
            io: lane(0, 0),
            service: lane(0, 0),
        },
    )
    .expect("a disabled I/O bank is a supported configuration");
    let client = execution.client(client_limits()).expect("client");
    let mut receipt = client
        .try_submit(Lane::Cpu, COST, |_| Ok::<_, ()>(5_u8))
        .expect("CPU job");
    execution.request_shutdown(ShutdownMode::Drain);
    joined(&mut execution);
    assert!(matches!(
        outcome(&mut receipt).view(),
        JobOutcome::Finished(Ok(5))
    ));
}

#[test]
fn queue_and_cross_client_caps_return_exact_rejected_input() {
    let quota = QuotaGroup::new(QuotaLimits {
        jobs: 2,
        input_bytes: 2 * COST.input_bytes,
        result_bytes: 2 * COST.result_bytes,
        ..limits()
    });
    let mut execution = Execution::start(quota.clone(), config(1, 1, 0)).expect("start");
    let first_client = execution.client(client_limits()).expect("first client");
    let second_client = execution.client(client_limits()).expect("second client");
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let mut running = first_client
        .try_submit(Lane::Cpu, COST, move |_| {
            started_tx.send(()).expect("entered");
            release_rx.recv_timeout(WAIT).expect("released");
            Ok::<_, ()>(())
        })
        .expect("running job");
    started_rx.recv_timeout(WAIT).expect("worker running");
    let waiting = first_client
        .try_reserve(Lane::Cpu, COST)
        .expect("one queue slot");
    assert_eq!(
        first_client.try_reserve(Lane::Cpu, COST).err(),
        Some(RejectReason::QueueFull)
    );
    assert_eq!(
        second_client.try_reserve(Lane::Io, COST).err(),
        Some(RejectReason::JobLimit),
        "different tenants share the root quota"
    );
    let drops = Arc::new(AtomicUsize::new(0));
    let rejected = first_client
        .try_submit(
            Lane::Cpu,
            COST,
            DropCounterJob {
                drops: Arc::clone(&drops),
            },
        )
        .err()
        .expect("queue full returns the job");
    assert_eq!(rejected.reason, RejectReason::QueueFull);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    drop(rejected.value);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    drop(waiting);
    let retry = second_client
        .try_reserve(Lane::Io, COST)
        .expect("released slot");
    drop(retry);
    release_tx.send(()).expect("release");
    execution.request_shutdown(ShutdownMode::Drain);
    joined(&mut execution);
    assert!(matches!(
        outcome(&mut running).view(),
        JobOutcome::Finished(Ok(()))
    ));
}

struct DropCounterJob {
    drops: Arc<AtomicUsize>,
}
impl Drop for DropCounterJob {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}
impl Job for DropCounterJob {
    type Output = ();
    type Error = ();
    fn run(self, _context: JobContext) -> Result<(), ()> {
        Ok(())
    }
}

#[test]
fn retained_result_and_declared_bytes_keep_capacity_charged() {
    let event = Arc::new((Mutex::new(()), Condvar::new()));
    let wake = Arc::clone(&event);
    let quota = QuotaGroup::new_with_admission_wake(
        QuotaLimits {
            jobs: 2,
            input_bytes: 2 * COST.input_bytes,
            result_bytes: COST.result_bytes,
            ..limits()
        },
        move || {
            let _guard = wake.0.lock().expect("wake lock");
            wake.1.notify_all();
        },
    );
    let mut execution = Execution::start(quota.clone(), config(2, 2, 0)).expect("start");
    let (completed_tx, completed_rx) = mpsc::sync_channel(1);
    let first_client = execution
        .client(client_limits())
        .expect("first")
        .with_completion_wake(move || {
            let _ = completed_tx.try_send(());
        });
    let second_client = execution.client(client_limits()).expect("second");
    assert_eq!(
        first_client
            .try_reserve(
                Lane::Cpu,
                JobCost {
                    input_bytes: COST.input_bytes,
                    result_bytes: COST.result_bytes + 1,
                }
            )
            .err(),
        Some(RejectReason::ResultBytes),
        "oversized declaration is rejected before any result allocation"
    );
    let mut receipt = first_client
        .try_submit(Lane::Cpu, COST, |_| Ok::<_, ()>(vec![9_u8; 8]))
        .expect("job admitted");
    completed_rx.recv_timeout(WAIT).expect("completion wake");
    let retained = outcome(&mut receipt);
    assert!(matches!(retained.view(), JobOutcome::Finished(Ok(_))));
    assert_eq!(quota.snapshot().result_bytes, COST.result_bytes);
    assert_eq!(
        second_client.try_reserve(Lane::Io, COST).err(),
        Some(RejectReason::ResultBytes),
        "another client cannot spend retained result bytes"
    );
    drop(receipt);
    assert_eq!(quota.snapshot().result_bytes, COST.result_bytes);
    drop(retained);
    wait_for_release(&event, || quota.snapshot().result_bytes == 0);
    assert_eq!(quota.snapshot().result_bytes, 0);
    assert_eq!(quota.snapshot().input_bytes, 0);
    let retry = second_client
        .try_reserve(Lane::Io, COST)
        .expect("result budget returned");
    drop(retry);
    assert_eq!(
        first_client
            .try_reserve(
                Lane::Io,
                JobCost {
                    input_bytes: 2 * COST.input_bytes + 1,
                    result_bytes: 1,
                }
            )
            .err(),
        Some(RejectReason::InputBytes)
    );
    execution.request_shutdown(ShutdownMode::Drain);
    joined(&mut execution);
}

#[test]
fn cancellation_skips_queued_input_but_preserves_started_result() {
    let quota = QuotaGroup::new(limits());
    let mut execution = Execution::start(quota, config(2, 2, 0)).expect("start");
    let client = execution.client(client_limits()).expect("client");
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let mut started = client
        .try_submit(Lane::Cpu, COST, move |_| {
            started_tx.send(()).expect("entered");
            release_rx.recv_timeout(WAIT).expect("release");
            Ok::<_, ()>(77_u8)
        })
        .expect("started job");
    started_rx.recv_timeout(WAIT).expect("running");
    let ran = Arc::new(AtomicUsize::new(0));
    let run_flag = Arc::clone(&ran);
    let mut queued = client
        .try_submit(Lane::Cpu, COST, move |_| {
            run_flag.fetch_add(1, Ordering::SeqCst);
            Ok::<_, ()>(())
        })
        .expect("queued job");
    queued.cancel();
    started.cancel();
    release_tx.send(()).expect("release");
    execution.request_shutdown(ShutdownMode::Drain);
    joined(&mut execution);
    assert!(matches!(
        outcome(&mut started).view(),
        JobOutcome::Finished(Ok(77))
    ));
    assert!(matches!(
        outcome(&mut queued).view(),
        JobOutcome::NotStarted {
            reason: ilium_execution::SkipReason::Cancelled,
            ..
        }
    ));
    assert_eq!(ran.load(Ordering::SeqCst), 0);
}

#[test]
fn panic_is_contained_and_following_work_runs() {
    let quota = QuotaGroup::new(limits());
    let mut execution = Execution::start(quota, config(2, 2, 0)).expect("start");
    let client = execution.client(client_limits()).expect("client");
    let mut panicked = client
        .try_submit(Lane::Cpu, COST, |_| -> Result<(), ()> {
            panic!("deliberate callback panic")
        })
        .expect("panic job admitted");
    let mut next = client
        .try_submit(Lane::Cpu, COST, |_| Ok::<_, ()>(19_u8))
        .expect("following job admitted");
    execution.request_shutdown(ShutdownMode::Drain);
    joined(&mut execution);
    assert!(matches!(
        outcome(&mut panicked).view(),
        JobOutcome::Panicked
    ));
    assert!(matches!(
        outcome(&mut next).view(),
        JobOutcome::Finished(Ok(19))
    ));
    let health = execution.monitor().health();
    assert_eq!(health.lanes[0].panicked, 1);
    assert_eq!(health.lanes[0].succeeded, 1);
}

#[test]
fn reserved_publication_survives_drain_and_cancel_without_busy() {
    for mode in [ShutdownMode::Drain, ShutdownMode::Cancel] {
        let quota = QuotaGroup::new(limits());
        let mut execution = Execution::start(quota, config(2, 2, 0)).expect("start");
        let client = execution.client(client_limits()).expect("client");
        let reservation = client
            .try_reserve(Lane::Cpu, COST)
            .expect("preparation reserved");
        reservation
            .validate_job_type::<fn(JobContext) -> Result<u8, ()>>()
            .expect("preflight result type");
        execution.request_shutdown(mode);
        assert_eq!(
            client.try_reserve(Lane::Cpu, COST).err(),
            Some(RejectReason::Closed)
        );
        let mut receipt = reservation
            .submit((|_| Ok::<u8, ()>(11)) as fn(JobContext) -> Result<u8, ()>)
            .expect("accepted reservation publishes after close");
        joined(&mut execution);
        match mode {
            ShutdownMode::Drain => {
                assert!(matches!(
                    outcome(&mut receipt).view(),
                    JobOutcome::Finished(Ok(11))
                ));
            }
            ShutdownMode::Cancel => {
                assert!(matches!(
                    outcome(&mut receipt).view(),
                    JobOutcome::NotStarted {
                        reason: ilium_execution::SkipReason::Shutdown,
                        ..
                    }
                ));
            }
        }
    }
}

#[test]
fn simultaneous_pre_reserved_publications_use_bounded_fifo_capacity() {
    const PRODUCERS: usize = 24;
    let quota = QuotaGroup::new(limits());
    let mut execution = Execution::start(quota, config(PRODUCERS, 1, 0)).expect("start");
    let client = execution.client(client_limits()).expect("client");
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let mut blocker = client
        .try_submit(Lane::Cpu, COST, move |_| {
            started_tx.send(()).expect("entered");
            release_rx.recv_timeout(WAIT).expect("release");
            Ok::<_, ()>(())
        })
        .expect("block worker");
    started_rx.recv_timeout(WAIT).expect("worker occupied");
    let reservations = (0..PRODUCERS)
        .map(|_| client.try_reserve(Lane::Cpu, COST).expect("reserved slot"))
        .collect::<Vec<_>>();
    assert_eq!(
        client.try_reserve(Lane::Cpu, COST).err(),
        Some(RejectReason::QueueFull)
    );
    let barrier = Arc::new(Barrier::new(PRODUCERS + 1));
    let publishers = reservations
        .into_iter()
        .map(|reservation| {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                reservation.submit((|_| Ok::<u8, ()>(1)) as fn(JobContext) -> Result<u8, ()>)
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let mut receipts = publishers
        .into_iter()
        .map(|publisher| {
            publisher
                .join()
                .expect("publisher thread")
                .expect("no post-reservation Busy")
        })
        .collect::<Vec<_>>();
    assert_eq!(execution.monitor().health().lanes[0].enqueued, PRODUCERS);
    release_tx.send(()).expect("release");
    execution.request_shutdown(ShutdownMode::Drain);
    joined(&mut execution);
    assert!(matches!(
        outcome(&mut blocker).view(),
        JobOutcome::Finished(Ok(()))
    ));
    for receipt in &mut receipts {
        assert!(matches!(
            outcome(receipt).view(),
            JobOutcome::Finished(Ok(1))
        ));
    }
}

#[test]
fn service_capacity_stays_claimed_through_retained_outcome() {
    let event = Arc::new((Mutex::new(()), Condvar::new()));
    let wake = Arc::clone(&event);
    let quota = QuotaGroup::new_with_admission_wake(limits(), move || {
        let _guard = wake.0.lock().expect("wake lock");
        wake.1.notify_all();
    });
    let mut execution = Execution::start(quota, config(2, 2, 1)).expect("start");
    let (completed_tx, completed_rx) = mpsc::sync_channel(1);
    let client = execution
        .client(client_limits())
        .expect("client")
        .with_completion_wake(move || {
            let _ = completed_tx.try_send(());
        });
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let mut service = client
        .try_submit(Lane::Service, COST, move |_| {
            started_tx.send(()).expect("entered");
            release_rx.recv_timeout(WAIT).expect("release");
            Ok::<_, ()>(())
        })
        .expect("service admitted");
    started_rx.recv_timeout(WAIT).expect("service running");
    assert_eq!(
        client.try_reserve(Lane::Service, COST).err(),
        Some(RejectReason::ServiceBankFull)
    );
    release_tx.send(()).expect("release");
    completed_rx.recv_timeout(WAIT).expect("completion wake");
    let ready = outcome(&mut service);
    drop(service);
    assert_eq!(
        client.try_reserve(Lane::Service, COST).err(),
        Some(RejectReason::ServiceBankFull),
        "reading does not relinquish a retained service instance"
    );
    drop(ready);
    wait_for_release(&event, || {
        execution.monitor().health().lanes[2].retained_service_claims == 0
    });
    let reservation = client
        .try_reserve(Lane::Service, COST)
        .expect("capacity returned");
    drop(reservation);
    execution.request_shutdown(ShutdownMode::Drain);
    joined(&mut execution);
}

#[test]
fn hung_retirement_keeps_physical_credit_until_real_exit() {
    let (event_lock, event) = (Mutex::new(()), Condvar::new());
    let event = Arc::new((event_lock, event));
    let wake = Arc::clone(&event);
    let quota = QuotaGroup::new_with_admission_wake(
        QuotaLimits {
            worker_threads: 2,
            ..limits()
        },
        move || {
            let _guard = wake.0.lock().expect("wake lock");
            wake.1.notify_all();
        },
    );
    let mut execution = Execution::start(quota.clone(), config(2, 2, 0)).expect("start");
    let client = execution.client(client_limits()).expect("client");
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let receipt = client
        .try_submit(Lane::Cpu, COST, move |_| {
            started_tx.send(()).expect("entered");
            release_rx.recv_timeout(WAIT).expect("release");
            Ok::<_, ()>(())
        })
        .expect("worker admitted");
    started_rx
        .recv_timeout(WAIT)
        .expect("blocking callback running");
    execution.request_shutdown(ShutdownMode::Cancel);
    let report = execution
        .join_until_background(Instant::now())
        .expect("deadline observed");
    assert!(report.remaining_workers >= 1);
    drop(execution); // Existing platform supervisor retains the actual handle.
    drop(client);
    assert!(quota.snapshot().worker_threads >= 1);
    assert_eq!(
        quota.reserve_external_worker(2, 0).err(),
        Some(RejectReason::WorkerLimit)
    );
    release_tx.send(()).expect("allow real callback exit");
    drop(receipt);
    wait_for_release(&event, || quota.snapshot().worker_threads == 0);
    let replacement = quota
        .reserve_external_worker(2, 0)
        .expect("actual exit released slots");
    drop(replacement);
}

#[test]
fn bank_metadata_remains_charged_while_a_monitor_retains_the_bank() {
    const BYTE_LIMIT: usize = 1024 * 1024;
    let event = Arc::new((Mutex::new(()), Condvar::new()));
    let wake = Arc::clone(&event);
    let quota = QuotaGroup::new_with_admission_wake(
        QuotaLimits {
            worker_threads: 3,
            worker_bytes: BYTE_LIMIT,
            ..limits()
        },
        move || {
            let _guard = wake.0.lock().expect("wake lock");
            wake.1.notify_all();
        },
    );
    let mut execution = Execution::start(quota.clone(), config(2, 2, 0)).expect("start");
    let monitor = execution.monitor();
    let first_charge = quota.snapshot().worker_bytes;
    assert!(first_charge > 2 * 1024 && first_charge < BYTE_LIMIT);
    let filler = quota
        .reserve_external_worker(1, BYTE_LIMIT - first_charge)
        .expect("fill remaining declared budget");
    execution.request_shutdown(ShutdownMode::Drain);
    joined(&mut execution);
    drop(execution);
    wait_for_release(&event, || quota.snapshot().worker_threads == 1);
    let retained_metadata = monitor.health().quota.worker_bytes - (BYTE_LIMIT - first_charge);
    assert!(
        retained_metadata > 0,
        "bank metadata survives physical joins"
    );
    assert_eq!(monitor.health().quota.worker_threads, 1);
    let formerly_physical = first_charge - retained_metadata;
    assert_eq!(
        quota
            .reserve_external_worker(1, formerly_physical + 1)
            .err(),
        Some(RejectReason::WorkerBytes),
        "retained monitor still owns bank storage"
    );
    drop(monitor);
    let permitted = quota
        .reserve_external_worker(1, formerly_physical + 1)
        .expect("last bank reference released metadata");
    drop(permitted);
    drop(filler);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}

#[test]
fn result_release_does_not_wait_for_the_completion_hook_to_return() {
    let quota = QuotaGroup::new(limits());
    let mut execution = Execution::start(quota, config(1, 1, 0)).expect("execution");
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let release_rx = std::sync::Mutex::new(release_rx);
    // Deliberately block this normally nonblocking hint to expose publication
    // ordering deterministically, without racing a receiver against CPU exit.
    let client = execution
        .client(ClientLimits {
            jobs: 1,
            service_jobs: 0,
            input_bytes: COST.input_bytes,
            result_bytes: COST.result_bytes,
        })
        .expect("client")
        .with_completion_wake(move || {
            entered_tx.try_send(()).expect("single completion hint");
            release_rx
                .lock()
                .expect("release lock")
                .recv_timeout(WAIT)
                .expect("release hint");
        });
    let mut receipt = client
        .try_submit(Lane::Cpu, COST, |_| Ok::<_, ()>(42))
        .expect("job");
    entered_rx.recv_timeout(WAIT).expect("published result");
    let retained = outcome(&mut receipt);
    assert!(matches!(retained.view(), JobOutcome::Finished(Ok(42))));
    drop(retained);
    drop(receipt);
    let jobs_after_release = client.usage().jobs;
    let replacement = client.try_reserve(Lane::Cpu, COST);
    // Always unblock the physical owner before asserting the observed race.
    release_tx.send(()).expect("release completion hint");
    assert_eq!(
        jobs_after_release, 0,
        "callback ended before result publication"
    );
    drop(replacement.expect("released result admits another bounded job"));
    execution.request_shutdown(ShutdownMode::Cancel);
    let report = execution
        .join_until_background(Instant::now() + WAIT)
        .expect("join");
    assert_eq!(report.remaining_workers, 0);
}
