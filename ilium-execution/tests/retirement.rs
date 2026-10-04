use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

use ilium_execution::{
    Client, ClientLimits, Execution, ExecutionConfig, JobCost, JobOutcome, JobPoll, Lane,
    LaneConfig, QuotaGroup, QuotaLimits, ShutdownMode,
};

const COST: JobCost = JobCost {
    input_bytes: 4096,
    result_bytes: 4096,
};

fn bank(cpu_threads: usize) -> (QuotaGroup, Execution, Client) {
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 4,
        jobs: 32,
        service_jobs: 0,
        input_bytes: 1024 * 1024,
        result_bytes: 1024 * 1024,
        worker_threads: cpu_threads,
        worker_bytes: 16 * 1024 * 1024,
    });
    let execution = Execution::start(
        quota.clone(),
        ExecutionConfig {
            cpu: LaneConfig {
                threads: cpu_threads,
                queue_slots: 16,
                priority: None,
                resident_bytes_per_thread: 4096,
            },
            io: LaneConfig {
                threads: 0,
                queue_slots: 0,
                priority: None,
                resident_bytes_per_thread: 0,
            },
            service: LaneConfig {
                threads: 0,
                queue_slots: 0,
                priority: None,
                resident_bytes_per_thread: 0,
            },
        },
    )
    .unwrap();
    let client = execution
        .client(ClientLimits {
            jobs: 8,
            service_jobs: 0,
            input_bytes: 128 * 1024,
            result_bytes: 128 * 1024,
        })
        .unwrap();
    (quota, execution, client)
}

fn wait_until(mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !done() {
        assert!(Instant::now() < deadline, "retirement did not finish");
        thread::yield_now();
    }
}

struct Spy {
    quota: QuotaGroup,
    notice: Sender<(ThreadId, usize)>,
}

impl Drop for Spy {
    fn drop(&mut self) {
        let observation = (thread::current().id(), self.quota.snapshot().worker_bytes);
        let _ = self.notice.send(observation);
    }
}

fn spy(
    client: &Client,
    quota: &QuotaGroup,
) -> (ilium_execution::Retiring<Spy>, Receiver<(ThreadId, usize)>) {
    let permit = client.retirement().try_reserve::<Spy>(4096).unwrap();
    let (notice, receiver) = mpsc::channel();
    (
        permit.attach(Spy {
            quota: quota.clone(),
            notice,
        }),
        receiver,
    )
}

#[test]
fn cancelled_before_start_returns_original_but_its_destructor_runs_on_cpu() {
    let (quota, mut execution, client) = bank(1);
    let ui_thread = thread::current().id();
    let baseline = quota.snapshot().worker_bytes;
    let (started_send, started_recv) = mpsc::sync_channel(1);
    let (release_send, release_recv) = mpsc::sync_channel(0);
    let blocker = client
        .try_submit(Lane::Cpu, COST, move |_| {
            started_send.send(()).unwrap();
            release_recv.recv().unwrap();
            Ok::<_, ()>(())
        })
        .unwrap();
    started_recv.recv_timeout(Duration::from_secs(3)).unwrap();
    let (original, notice) = spy(&client, &quota);
    let mut receipt = client
        .try_submit(Lane::Cpu, COST, move |_| Ok::<_, ()>(original))
        .unwrap();
    receipt.cancel();
    release_send.send(()).unwrap();
    let outcome = loop {
        match receipt.try_take() {
            JobPoll::Pending => thread::yield_now(),
            JobPoll::Ready(outcome) => break outcome,
            JobPoll::Lost | JobPoll::Taken => panic!("cancelled original lost"),
        }
    };
    assert!(matches!(outcome.view(), JobOutcome::NotStarted { .. }));
    drop(outcome); // UI relinquishes original typed callback.
    let (destructor_thread, charged_during_drop) =
        notice.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_ne!(destructor_thread, ui_thread);
    assert!(charged_during_drop > baseline);
    wait_until(|| execution.monitor().health().lanes[0].retirement_live == 0);
    assert_eq!(quota.snapshot().worker_bytes, baseline);
    drop(blocker);
    execution.request_shutdown(ShutdownMode::Cancel);
    assert_eq!(
        execution
            .join_until_background(Instant::now() + Duration::from_secs(3))
            .unwrap()
            .remaining_workers,
        0
    );
}

#[test]
fn unread_finished_result_relinquished_on_ui_retires_on_cpu() {
    let (quota, mut execution, client) = bank(1);
    let ui_thread = thread::current().id();
    let (original, notice) = spy(&client, &quota);
    let (wake_send, wake_recv) = mpsc::channel();
    let waking = client.with_completion_wake(move || {
        let _ = wake_send.send(());
    });
    let receipt = waking
        .try_submit(Lane::Cpu, COST, move |_| Ok::<_, ()>(original))
        .unwrap();
    wake_recv.recv_timeout(Duration::from_secs(3)).unwrap();
    drop(receipt); // The published Finished result was never polled.
    let (destructor_thread, _) = notice.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_ne!(destructor_thread, ui_thread);
    wait_until(|| execution.monitor().health().lanes[0].retirement_live == 0);
    execution.request_shutdown(ShutdownMode::Cancel);
    assert_eq!(
        execution
            .join_until_background(Instant::now() + Duration::from_secs(3))
            .unwrap()
            .remaining_workers,
        0
    );
}

#[test]
fn nested_shared_leaf_keeps_original_allocation_guard_until_cpu_destruction() {
    let (quota, mut execution, client) = bank(1);
    let baseline = quota.snapshot().worker_bytes;
    let original_guard = std::sync::Arc::new(quota.reserve_external_storage(8192).unwrap());
    let permit = client.retirement().try_reserve::<Spy>(4096).unwrap();
    let (notice_sender, notice_receiver) = mpsc::channel();
    let mut leaf = permit.attach(Spy {
        quota: quota.clone(),
        notice: notice_sender,
    });
    leaf.set_storage_guard(std::sync::Arc::clone(&original_guard));
    let parent = std::sync::Arc::new(leaf);
    let last_leaf = std::sync::Arc::clone(&parent);
    drop(original_guard);
    drop(parent);
    assert!(quota.snapshot().worker_bytes >= baseline + 8192);
    drop(last_leaf);
    let (_, charged_during_drop) = notice_receiver
        .recv_timeout(Duration::from_secs(3))
        .unwrap();
    assert!(charged_during_drop >= baseline + 8192);
    wait_until(|| execution.monitor().health().lanes[0].retirement_live == 0);
    assert_eq!(quota.snapshot().worker_bytes, baseline);
    execution.request_shutdown(ShutdownMode::Cancel);
    assert_eq!(
        execution
            .join_until_background(Instant::now() + Duration::from_secs(3))
            .unwrap()
            .remaining_workers,
        0
    );
}

#[test]
fn eight_retained_old_and_new_pane_heaps_do_not_claim_document_jobs() {
    let (quota, mut execution, client) = bank(2);
    let baseline = quota.snapshot().worker_bytes;
    let mut old_and_new = Vec::new();
    for _ in 0..8 {
        let permit = client.retirement().try_reserve::<Vec<u8>>(4096).unwrap();
        old_and_new.push(permit.attach_shared(vec![7; 1024]));
    }
    assert_eq!(client.usage().jobs, 0);
    let reservations: Vec<_> = (0..8)
        .map(|_| client.try_reserve(Lane::Cpu, COST).unwrap())
        .collect();
    assert_eq!(client.usage().jobs, 8);
    drop(reservations);
    drop(old_and_new);
    wait_until(|| execution.monitor().health().lanes[0].retirement_live == 0);
    assert_eq!(quota.snapshot().worker_bytes, baseline);
    execution.request_shutdown(ShutdownMode::Cancel);
    assert_eq!(
        execution
            .join_until_background(Instant::now() + Duration::from_secs(3))
            .unwrap()
            .remaining_workers,
        0
    );
}

#[test]
fn retirement_capacity_refuses_before_payload_construction_without_using_jobs() {
    let (_, mut execution, client) = bank(2);
    let mut reservations = Vec::new();
    for _ in 0..ilium_execution::RETIREMENT_SLOTS {
        reservations.push(client.retirement().try_reserve::<Vec<u8>>(4096).unwrap());
    }
    assert_eq!(
        client.retirement().try_reserve::<Vec<u8>>(4096).err(),
        Some(ilium_execution::RejectReason::QueueFull)
    );
    assert_eq!(client.usage().jobs, 0);
    reservations.pop();
    let final_reservation = client.retirement().try_reserve::<Vec<u8>>(4096).unwrap();
    drop(final_reservation);
    drop(reservations);
    wait_until(|| execution.monitor().health().lanes[0].retirement_live == 0);
    execution.request_shutdown(ShutdownMode::Cancel);
    assert_eq!(
        execution
            .join_until_background(Instant::now() + Duration::from_secs(3))
            .unwrap()
            .remaining_workers,
        0
    );
}

#[test]
fn late_final_ack_after_cancel_still_reaches_cpu_before_physical_join() {
    let (quota, mut execution, client) = bank(1);
    let ui_thread = thread::current().id();
    let (original, notice) = spy(&client, &quota);
    execution.request_shutdown(ShutdownMode::Cancel);
    let joiner = thread::spawn(move || {
        execution
            .join_until_background(Instant::now() + Duration::from_secs(3))
            .unwrap()
    });
    drop(original); // Represents the last pending/emitted frame ACK owner.
    let (destructor_thread, _) = notice.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_ne!(destructor_thread, ui_thread);
    assert_eq!(joiner.join().unwrap().remaining_workers, 0);
    assert_eq!(quota.snapshot().worker_threads, 0);
}

struct PanicDrop;
impl Drop for PanicDrop {
    fn drop(&mut self) {
        panic!("forced retirement destructor panic");
    }
}

#[test]
fn panicking_destructor_releases_slot_and_cpu_bank_can_join() {
    let (_, mut execution, client) = bank(1);
    let permit = client.retirement().try_reserve::<PanicDrop>(4096).unwrap();
    drop(permit.attach(PanicDrop));
    wait_until(|| execution.monitor().health().lanes[0].retirement_panicked == 1);
    assert_eq!(execution.monitor().health().lanes[0].retirement_live, 0);
    execution.request_shutdown(ShutdownMode::Cancel);
    assert_eq!(
        execution
            .join_until_background(Instant::now() + Duration::from_secs(3))
            .unwrap()
            .remaining_workers,
        0
    );
}

#[test]
fn panicking_retirement_does_not_stop_the_only_cpu_worker() {
    let (_, mut execution, client) = bank(1);
    let permit = client.retirement().try_reserve::<PanicDrop>(4096).unwrap();
    drop(permit.attach(PanicDrop));
    wait_until(|| execution.monitor().health().lanes[0].retirement_panicked == 1);
    let mut ordinary = client
        .try_submit(Lane::Cpu, COST, move |_| {
            Ok::<_, ()>(thread::current().id())
        })
        .unwrap();
    let worker_thread = loop {
        match ordinary.try_take() {
            JobPoll::Pending => thread::yield_now(),
            JobPoll::Ready(answer) => match answer.view() {
                JobOutcome::Finished(Ok(thread_id)) => break *thread_id,
                _ => panic!("ordinary work failed after retirement destructor panic"),
            },
            JobPoll::Lost | JobPoll::Taken => panic!("ordinary work disappeared"),
        }
    };
    assert_ne!(worker_thread, thread::current().id());
    drop(ordinary);
    execution.request_shutdown(ShutdownMode::Cancel);
    assert_eq!(
        execution
            .join_until_background(Instant::now() + Duration::from_secs(3))
            .unwrap()
            .remaining_workers,
        0
    );
}

struct BlockingDrop {
    started: Sender<ThreadId>,
    release: Receiver<()>,
}
impl Drop for BlockingDrop {
    fn drop(&mut self) {
        let _ = self.started.send(thread::current().id());
        let _ = self.release.recv();
    }
}

#[test]
fn blocked_retirement_keeps_physical_admission_and_other_cpu_progresses() {
    let (quota, mut execution, client) = bank(2);
    let (started_send, started_recv) = mpsc::channel();
    let (release_send, release_recv) = mpsc::channel();
    let permit = client
        .retirement()
        .try_reserve::<BlockingDrop>(4096)
        .unwrap();
    let reservation = client.try_reserve(Lane::Cpu, COST).unwrap();
    let mut payload = permit.attach(BlockingDrop {
        started: started_send,
        release: release_recv,
    });
    payload.set_retention(reservation.retention());
    drop(reservation);
    drop(payload);
    let blocked_thread = started_recv.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(
        quota.snapshot().jobs,
        1,
        "original job guard must outlive its destructor"
    );
    let mut ordinary = client
        .try_submit(Lane::Cpu, COST, move |_| {
            Ok::<_, ()>(thread::current().id())
        })
        .unwrap();
    let other_thread = loop {
        match ordinary.try_take() {
            JobPoll::Pending => thread::yield_now(),
            JobPoll::Ready(answer) => match answer.view() {
                JobOutcome::Finished(Ok(thread_id)) => break *thread_id,
                _ => panic!("ordinary CPU work did not finish"),
            },
            JobPoll::Lost | JobPoll::Taken => panic!("ordinary CPU result lost"),
        }
    };
    assert_ne!(blocked_thread, other_thread);
    drop(ordinary);
    execution.request_shutdown(ShutdownMode::Cancel);
    let first = execution
        .join_until_background(Instant::now() + Duration::from_millis(100))
        .unwrap();
    assert!(first.remaining_workers >= 1);
    assert!(!first.shutdown_complete);
    assert!(quota.snapshot().worker_threads >= 1);
    release_send.send(()).unwrap();
    wait_until(|| quota.snapshot().jobs == 0);
    let final_report = execution
        .join_until_background(Instant::now() + Duration::from_secs(3))
        .unwrap();
    assert_eq!(final_report.remaining_workers, 0);
    assert!(final_report.shutdown_complete);
    assert_eq!(quota.snapshot().worker_threads, 0);
}
