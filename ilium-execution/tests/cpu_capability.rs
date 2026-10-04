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
