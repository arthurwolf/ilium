use ilium_execution::{
    ClientLimits, Execution, ExecutionConfig, JobCost, JobOutcome, JobPoll, Lane, LaneConfig,
    QuotaGroup, QuotaLimits, ShutdownMode,
};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

fn setup() -> (Execution, QuotaGroup) {
    setup_with_clients(2)
}

fn setup_with_clients(clients: usize) -> (Execution, QuotaGroup) {
    let quota = QuotaGroup::new(QuotaLimits {
        clients,
        jobs: 4,
        service_jobs: 0,
        input_bytes: 16_384,
        result_bytes: 16_384,
        worker_threads: 3,
        worker_bytes: 1_048_576,
    });
    let lane = |threads| LaneConfig {
        threads,
        queue_slots: if threads == 0 { 0 } else { 4 },
        priority: None,
        resident_bytes_per_thread: 1_024,
    };
    let execution = Execution::start(
        quota.clone(),
        ExecutionConfig {
            cpu: lane(2),
            io: lane(1),
            service: lane(0),
        },
    )
    .expect("start admitted threads");
    (execution, quota)
}

#[test]
fn siblings_cannot_multiply_parent_allowance_and_codec_headroom_survives_retained_results() {
    let (mut execution, quota) = setup_with_clients(8);
    let limits = |input_bytes| ClientLimits {
        jobs: 4,
        service_jobs: 0,
        input_bytes,
        result_bytes: 4096,
    };
    let parent = execution.client(limits(8192)).unwrap();
    let left = parent.child(limits(8192)).unwrap();
    let right = parent.child(limits(8192)).unwrap();
    let codec = execution.client(limits(8192)).unwrap();
    let cost = JobCost {
        input_bytes: 4096,
        result_bytes: 256,
    };
    let first = left
        .try_reserve_external(cost)
        .unwrap()
        .retain(vec![1_u8; 1024])
        .unwrap();
    let second = right
        .try_reserve_external(cost)
        .unwrap()
        .retain(vec![2_u8; 1024])
        .unwrap();
    assert_eq!(parent.usage().input_bytes, 8192);
    assert_eq!(left.usage().input_bytes, 4096);
    assert_eq!(
        right.try_reserve_external(cost).err(),
        Some(ilium_execution::RejectReason::InputBytes)
    );
    let output = codec
        .try_reserve_external(cost)
        .unwrap()
        .retain(vec![3_u8; 1024])
        .unwrap();
    assert_eq!(quota.snapshot().input_bytes, 12288);
    drop(first);
    let retry = right.try_reserve_external(cost).unwrap();
    assert_eq!(parent.usage().input_bytes, 8192);
    drop(retry);
    drop(left);
    drop(right);
    drop(parent);
    execution.request_shutdown(ShutdownMode::Drain);
    assert_eq!(
        execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap()
            .remaining_workers,
        0
    );
    assert_eq!(
        quota.snapshot().jobs,
        2,
        "retained child keeps parent identity alive after join"
    );
    drop(second);
    drop(output);
    assert_eq!(quota.snapshot().jobs, 0);
    assert_eq!(quota.snapshot().input_bytes, 0);
    assert_eq!(
        quota.snapshot().clients,
        1,
        "only independent codec handle remains"
    );
}

#[test]
fn child_ownership_depth_is_bounded_without_leaking_a_registration() {
    let (execution, quota) = setup_with_clients(16);
    let limits = ClientLimits {
        jobs: 1,
        service_jobs: 0,
        input_bytes: 4096,
        result_bytes: 256,
    };
    let mut deepest = execution.client(limits).unwrap();
    for _ in 1..8 {
        deepest = deepest.child(limits).unwrap();
    }
    assert_eq!(quota.snapshot().clients, 8);
    assert_eq!(
        deepest.child(limits).err(),
        Some(ilium_execution::RejectReason::InvalidCost)
    );
    assert_eq!(quota.snapshot().clients, 8);
    assert!(deepest.is_open());
    execution.request_shutdown(ShutdownMode::Drain);
    assert!(!deepest.is_open());
    assert_eq!(
        deepest.child(limits).err(),
        Some(ilium_execution::RejectReason::Closed)
    );
    drop(deepest);
    assert_eq!(quota.snapshot().clients, 0);
}

#[test]
fn external_library_threads_share_the_cap_and_release_wakes_admission() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let released = Arc::new(AtomicUsize::new(0));
    let witness = Arc::clone(&released);
    let quota = QuotaGroup::new_with_admission_wake(
        QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: 3,
            worker_bytes: 4096,
        },
        move || {
            witness.fetch_add(1, Ordering::SeqCst);
        },
    );
    let lease = quota
        .reserve_external_worker(3, 4096)
        .expect("owner plus library threads");
    assert_eq!(quota.snapshot().worker_threads, 3);
    assert!(quota.reserve_external_worker(1, 0).is_err());
    assert_eq!(released.load(Ordering::SeqCst), 0);
    drop(lease);
    assert_eq!(quota.snapshot().worker_threads, 0);
    assert_eq!(quota.snapshot().worker_bytes, 0);
    assert_eq!(released.load(Ordering::SeqCst), 1);
}

#[test]
fn asynchronous_transfer_retains_charge_until_delivery_or_cancellation() {
    use std::future::Future;
    let (mut execution, quota) = setup();
    let client = execution
        .client(ClientLimits {
            jobs: 1,
            service_jobs: 0,
            input_bytes: 4096,
            result_bytes: 4096,
        })
        .expect("client");
    let mut receipt = client
        .try_submit(
            Lane::Cpu,
            JobCost {
                input_bytes: 4096,
                result_bytes: 4096,
            },
            |_| Ok::<_, ()>(vec![7_u8; 1024]),
        )
        .expect("job");
    execution.request_shutdown(ShutdownMode::Drain);
    assert_eq!(
        execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .expect("actual joins")
            .remaining_workers,
        0
    );
    let JobPoll::Ready(result) = receipt.try_take() else {
        panic!("finished result");
    };
    drop(receipt);
    let result = result.map(|outcome| match outcome {
        JobOutcome::Finished(Ok(bytes)) => bytes,
        _ => panic!("successful job"),
    });
    let mut delivery = Box::pin(result.map_async(|bytes| async move {
        std::hint::black_box(&bytes);
        std::future::pending::<()>().await;
        drop(bytes);
    }));
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(delivery.as_mut().poll(&mut context).is_pending());
    assert_eq!(quota.snapshot().jobs, 1);
    drop(delivery);
    assert_eq!(quota.snapshot().jobs, 0);
    assert_eq!(quota.snapshot().result_bytes, 0);
}

#[test]
fn separate_cores_can_run_jobs_concurrently_and_retained_results_keep_admission() {
    let (mut execution, quota) = setup();
    let client = execution
        .client(ClientLimits {
            jobs: 2,
            service_jobs: 0,
            input_bytes: 8_192,
            result_bytes: 8_192,
        })
        .expect("client");
    let barrier = Arc::new(Barrier::new(3));
    let (sent, arrived) = std::sync::mpsc::sync_channel(2);
    let caller = std::thread::current().id();
    let cost = JobCost {
        input_bytes: 4_096,
        result_bytes: 4_096,
    };
    let mut receipts = Vec::new();
    for number in 0..2 {
        let barrier = barrier.clone();
        let sent = sent.clone();
        receipts.push(
            client
                .try_submit(Lane::Cpu, cost, move |_| {
                    sent.send(std::thread::current().id())
                        .expect("thread evidence");
                    barrier.wait();
                    Ok::<_, ()>(number)
                })
                .expect("admit job"),
        );
    }
    let first = arrived
        .recv_timeout(Duration::from_secs(5))
        .expect("first worker running");
    let second = arrived
        .recv_timeout(Duration::from_secs(5))
        .expect("second worker running");
    assert_ne!(first, caller);
    assert_ne!(second, caller);
    assert_ne!(first, second);
    assert!(client.try_reserve(Lane::Cpu, cost).is_err());
    barrier.wait();
    execution.request_shutdown(ShutdownMode::Drain);
    let joined = execution
        .join_until_background(Instant::now() + Duration::from_secs(5))
        .expect("background join");
    assert_eq!(joined.remaining_workers, 0);
    let retained = receipts
        .iter_mut()
        .map(|receipt| match receipt.try_take() {
            JobPoll::Ready(value) => value,
            _ => panic!("joined job must have a retained result"),
        })
        .collect::<Vec<_>>();
    for result in &retained {
        assert!(matches!(result.view(), JobOutcome::Finished(Ok(_))));
    }
    assert_eq!(quota.snapshot().jobs, 2);
    drop(receipts);
    assert_eq!(
        quota.snapshot().jobs,
        2,
        "reading is not permission to forget retained bytes"
    );
    drop(retained);
    assert_eq!(quota.snapshot().jobs, 0);
    assert_eq!(quota.snapshot().input_bytes, 0);
    assert_eq!(quota.snapshot().result_bytes, 0);
}

#[test]
fn unsubmitted_reservations_fence_drain_until_the_caller_releases_them() {
    let (mut execution, quota) = setup();
    let client = execution
        .client(ClientLimits {
            jobs: 1,
            service_jobs: 0,
            input_bytes: 4_096,
            result_bytes: 4_096,
        })
        .expect("client");
    let reservation = client
        .try_reserve(
            Lane::Io,
            JobCost {
                input_bytes: 4_096,
                result_bytes: 4_096,
            },
        )
        .expect("reserve before retaining input");
    execution.request_shutdown(ShutdownMode::Drain);
    assert!(client
        .try_reserve(
            Lane::Io,
            JobCost {
                input_bytes: 1,
                result_bytes: 1,
            }
        )
        .is_err());
    let first = execution
        .join_until_background(Instant::now())
        .expect("deadline observation");
    assert!(first.remaining_workers > 0);
    assert_eq!(quota.snapshot().jobs, 1);
    drop(reservation);
    let final_join = execution
        .join_until_background(Instant::now() + Duration::from_secs(5))
        .expect("final join");
    assert_eq!(final_join.remaining_workers, 0);
    assert_eq!(quota.snapshot().jobs, 0);
    assert_eq!(quota.snapshot().worker_threads, 0);
}

#[test]
fn external_actor_payloads_use_shared_bytes_without_consuming_bank_slots() {
    let (mut execution, quota) = setup();
    let client = execution
        .client(ClientLimits {
            jobs: 4,
            service_jobs: 0,
            input_bytes: 16_384,
            result_bytes: 16_384,
        })
        .unwrap();
    let external = client
        .try_reserve_external(JobCost {
            input_bytes: 4096,
            result_bytes: 256,
        })
        .unwrap();
    let held = external.retain(vec![0_u8; 1024]).unwrap();
    assert_eq!(execution.monitor().health().lanes[0].waiting_or_reserved, 0);
    assert_eq!(execution.monitor().health().lanes[1].waiting_or_reserved, 0);
    assert_eq!(quota.snapshot().jobs, 1);
    assert_eq!(quota.snapshot().input_bytes, 4096);
    let cpu = client
        .try_reserve(
            Lane::Cpu,
            JobCost {
                input_bytes: 4096,
                result_bytes: 256,
            },
        )
        .unwrap();
    assert_eq!(quota.snapshot().jobs, 2);
    drop(cpu);
    let held = held.map(|bytes| bytes.len());
    assert_eq!(*held.view(), 1024);
    assert_eq!(quota.snapshot().input_bytes, 4096);
    execution.request_shutdown(ShutdownMode::Drain);
    assert_eq!(
        client
            .try_reserve_external(JobCost {
                input_bytes: 4096,
                result_bytes: 256
            })
            .err(),
        Some(ilium_execution::RejectReason::Closed)
    );
    let report = execution
        .join_until_background(Instant::now() + Duration::from_secs(2))
        .unwrap();
    assert_eq!(report.remaining_workers, 0);
    assert_eq!(
        quota.snapshot().jobs,
        1,
        "external lifetime outlives bank exit"
    );
    drop(held);
    assert_eq!(quota.snapshot().jobs, 0);
}

#[test]
fn transferred_payload_keeps_its_charge_through_queued_clones_and_worker_shutdown() {
    let (mut execution, quota) = setup();
    let client = execution
        .client(ClientLimits {
            jobs: 1,
            service_jobs: 0,
            input_bytes: 4096,
            result_bytes: 256,
        })
        .unwrap();
    let original = vec![42_u8; 1024];
    let original_pointer = original.as_ptr();
    let held = client
        .try_reserve_external(JobCost {
            input_bytes: 4096,
            result_bytes: 256,
        })
        .unwrap()
        .retain(original)
        .unwrap();
    let (payload, retention) = held.into_parts();
    assert_eq!(payload.as_ptr(), original_pointer);
    let queued_retention = retention.clone();
    let payload = retention.retain(payload);
    assert_eq!(quota.snapshot().jobs, 1);
    execution.request_shutdown(ShutdownMode::Drain);
    assert_eq!(
        execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap()
            .remaining_workers,
        0
    );
    drop(payload);
    assert_eq!(
        quota.snapshot().jobs,
        1,
        "queued owner still retains the same charge"
    );
    drop(queued_retention);
    assert_eq!(quota.snapshot().jobs, 0);
    assert_eq!(quota.snapshot().input_bytes, 0);
    assert_eq!(quota.snapshot().result_bytes, 0);
}

#[test]
fn immutable_storage_survives_worker_join_until_its_last_clone_without_using_thread_or_job_slots() {
    let (mut execution, quota) = setup();
    let before = quota.snapshot();
    let storage = Arc::new(
        quota
            .reserve_external_storage(4096)
            .expect("storage before capture"),
    );
    let consumer = Arc::clone(&storage);
    let during = quota.snapshot();
    assert_eq!(during.worker_bytes, before.worker_bytes + 4096);
    assert_eq!(during.worker_threads, before.worker_threads);
    assert_eq!(during.jobs, before.jobs);
    assert_eq!(during.result_bytes, before.result_bytes);
    assert!(matches!(
        quota.reserve_external_storage(1_048_576),
        Err(ilium_execution::RejectReason::WorkerBytes)
    ));
    assert!(matches!(
        quota.reserve_external_storage(0),
        Err(ilium_execution::RejectReason::InvalidCost)
    ));
    execution.request_shutdown(ShutdownMode::Drain);
    assert_eq!(
        execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .expect("joined")
            .remaining_workers,
        0
    );
    drop(execution);
    drop(storage);
    assert_eq!(quota.snapshot().worker_threads, 0);
    assert_eq!(quota.snapshot().worker_bytes, 4096);
    drop(consumer);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}

#[test]
fn reservation_retention_preserves_refused_original_until_charged_destruction() {
    use ilium_execution::{Job, JobContext, RejectReason};
    use std::sync::atomic::{AtomicBool, Ordering};

    struct RefusedPayload {
        bytes: Vec<u8>,
        quota: QuotaGroup,
        observed_charged_drop: Arc<AtomicBool>,
    }
    impl Drop for RefusedPayload {
        fn drop(&mut self) {
            let usage = self.quota.snapshot();
            assert_eq!(
                (usage.jobs, usage.input_bytes, usage.result_bytes),
                (1, 4096, 256)
            );
            self.observed_charged_drop.store(true, Ordering::Release);
        }
    }
    impl Job for RefusedPayload {
        type Output = [u8; 8192];
        type Error = std::convert::Infallible;
        fn run(self, _context: JobContext) -> Result<Self::Output, Self::Error> {
            panic!("invalid result declaration must refuse before execution");
        }
    }

    let (mut execution, quota) = setup();
    let client = execution
        .client(ClientLimits {
            jobs: 2,
            service_jobs: 0,
            input_bytes: 8192,
            result_bytes: 8192,
        })
        .unwrap();
    let reservation = client
        .try_reserve(
            Lane::Cpu,
            JobCost {
                input_bytes: 4096,
                result_bytes: 256,
            },
        )
        .unwrap();
    let retention = reservation.retention();
    let clone = retention.clone();
    assert_eq!(
        quota.snapshot().jobs,
        1,
        "sharing never takes a second credit"
    );
    let bytes = vec![42; 1024];
    let pointer = bytes.as_ptr();
    let observed = Arc::new(AtomicBool::new(false));
    let rejected = match reservation.submit(RefusedPayload {
        bytes,
        quota: quota.clone(),
        observed_charged_drop: Arc::clone(&observed),
    }) {
        Ok(_) => panic!("undersized result declaration was accepted"),
        Err(rejected) => rejected,
    };
    assert_eq!(rejected.reason, RejectReason::InvalidCost);
    assert_eq!(rejected.value.bytes.as_ptr(), pointer);
    assert_eq!(rejected.value.bytes, vec![42; 1024]);
    assert_eq!(quota.snapshot().jobs, 1);
    let retained = retention.retain(rejected.value);
    drop(clone);
    assert!(!observed.load(Ordering::Acquire));
    assert_eq!(quota.snapshot().input_bytes, 4096);
    drop(retained);
    assert!(observed.load(Ordering::Acquire));
    let usage = quota.snapshot();
    assert_eq!(
        (usage.jobs, usage.input_bytes, usage.result_bytes),
        (0, 0, 0)
    );
    execution.request_shutdown(ShutdownMode::Drain);
    assert_eq!(
        execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap()
            .remaining_workers,
        0
    );
}
