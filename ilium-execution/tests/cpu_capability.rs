use ilium_execution::{
    ClientLimits, Execution, ExecutionConfig, JobCost, LaneConfig, QuotaGroup, QuotaLimits,
    RejectReason,
};
fn execution(cpu_threads: usize, cpu_slots: usize) -> Execution {
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 8,
        jobs: 8,
        service_jobs: 0,
        input_bytes: 16384,
        result_bytes: 16384,
        worker_threads: 2,
        worker_bytes: 1024 * 1024,
    });
    let lane = |threads, queue_slots| LaneConfig {
        threads,
        queue_slots,
        priority: None,
        resident_bytes_per_thread: 1024,
    };
    Execution::start(
        quota,
        ExecutionConfig {
            cpu: lane(cpu_threads, cpu_slots),
            io: lane(1, 4),
            service: lane(0, 0),
        },
    )
    .unwrap()
}
fn limits(jobs: usize, input_bytes: usize, result_bytes: usize) -> ClientLimits {
    ClientLimits {
        jobs,
        service_jobs: 0,
        input_bytes,
        result_bytes,
    }
}
#[test]
fn cpu_cost_ceiling_is_the_minimum_of_root_ancestors_and_current_tenant() {
    let execution = execution(1, 4);
    let parent = execution.client(limits(4, 8192, 32768)).unwrap();
    let middle = parent.child(limits(4, 32768, 12288)).unwrap();
    let leaf = middle.child(limits(4, 24576, 8192)).unwrap();
    assert_eq!(
        leaf.maximum_cpu_job_cost(),
        Ok(JobCost {
            input_bytes: 8192,
            result_bytes: 8192
        })
    );
    let wide = execution.client(limits(4, 32768, 32768)).unwrap();
    assert_eq!(
        wide.maximum_cpu_job_cost(),
        Ok(JobCost {
            input_bytes: 16384,
            result_bytes: 16384
        })
    );
    let zero_parent = execution.client(limits(0, 8192, 8192)).unwrap();
    let nonzero_child = zero_parent.child(limits(4, 8192, 8192)).unwrap();
    assert_eq!(
        nonzero_child.maximum_cpu_job_cost(),
        Err(RejectReason::JobLimit)
    );
    let zero_leaf = parent.child(limits(0, 8192, 8192)).unwrap();
    assert_eq!(
        zero_leaf.maximum_cpu_job_cost(),
        Err(RejectReason::JobLimit)
    );
}
#[test]
fn disabled_cpu_worker_or_queue_is_permanent_capability_refusal() {
    // Startup requires threads==0 iff slots==0. The actual disabled-bank
    // fixture covers both immutable conditions without an invalid constructor.
    let execution = execution(0, 0);
    let client = execution.client(limits(1, 8192, 8192)).unwrap();
    assert_eq!(
        client.maximum_cpu_job_cost(),
        Err(RejectReason::InvalidCost)
    );
}

#[test]
fn external_cost_ceiling_includes_ancestors_and_does_not_report_free_capacity() {
    let mut execution = execution(1, 4);
    let parent = execution.client(limits(4, 8192, 32768)).unwrap();
    let middle = parent.child(limits(4, 32768, 12288)).unwrap();
    let leaf = middle.child(limits(4, 24576, 8192)).unwrap();
    let expected = JobCost {
        input_bytes: 8192,
        result_bytes: 8192,
    };
    assert_eq!(leaf.maximum_external_job_cost(), Ok(expected));
    let reservation = leaf
        .try_reserve_external(JobCost {
            input_bytes: 4096,
            result_bytes: 4096,
        })
        .unwrap();
    // A capability ceiling stays immutable while actual occupancy changes.
    assert_eq!(leaf.maximum_external_job_cost(), Ok(expected));
    drop(reservation);
    assert!(matches!(
        leaf.try_reserve_external(JobCost {
            input_bytes: 8193,
            result_bytes: 4096,
        }),
        Err(RejectReason::InputBytes)
    ));
    let wide = execution.client(limits(4, 32768, 32768)).unwrap();
    assert_eq!(
        wide.maximum_external_job_cost(),
        Ok(JobCost {
            input_bytes: 16384,
            result_bytes: 16384,
        })
    );
    let zero_parent = execution.client(limits(0, 8192, 8192)).unwrap();
    let child = zero_parent.child(limits(4, 8192, 8192)).unwrap();
    assert_eq!(
        child.maximum_external_job_cost(),
        Err(RejectReason::JobLimit)
    );
    execution.request_shutdown(ilium_execution::ShutdownMode::Drain);
    assert_eq!(leaf.maximum_external_job_cost(), Err(RejectReason::Closed));
    let report = execution
        .join_until_background(std::time::Instant::now() + std::time::Duration::from_secs(5))
        .unwrap();
    assert!(report.shutdown_complete);
    assert_eq!(report.remaining_workers, 0);
    assert_eq!(leaf.quota_group().snapshot().jobs, 0);
}

#[test]
fn external_actor_capability_does_not_require_a_cpu_worker_or_queue() {
    let mut execution = execution(0, 0);
    let client = execution.client(limits(1, 8192, 8192)).unwrap();
    assert_eq!(
        client.maximum_cpu_job_cost(),
        Err(RejectReason::InvalidCost)
    );
    assert_eq!(
        client.maximum_external_job_cost(),
        Ok(JobCost {
            input_bytes: 8192,
            result_bytes: 8192,
        })
    );
    let reservation = client
        .try_reserve_external(JobCost {
            input_bytes: 4096,
            result_bytes: 4096,
        })
        .unwrap();
    drop(reservation);
    execution.request_shutdown(ilium_execution::ShutdownMode::Drain);
    assert_eq!(
        client.maximum_external_job_cost(),
        Err(RejectReason::Closed)
    );
    let report = execution
        .join_until_background(std::time::Instant::now() + std::time::Duration::from_secs(5))
        .unwrap();
    assert!(report.shutdown_complete);
    assert_eq!(report.remaining_workers, 0);
    assert_eq!(client.quota_group().snapshot().jobs, 0);
}
