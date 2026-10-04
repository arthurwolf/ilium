use ilium_execution::{
    AdmissionBoundary, Client, ClientLimits, Execution, ExecutionConfig, JobCost, Lane, LaneConfig,
    QuotaGroup, QuotaLimits, QuotaResource, RejectReason, ShutdownMode,
};
use std::time::{Duration, Instant};
const MIB: usize = 1024 * 1024;
fn limits(input_bytes: usize) -> ClientLimits {
    ClientLimits {
        jobs: 16,
        service_jobs: 0,
        input_bytes,
        result_bytes: 256 * MIB,
    }
}
fn setup() -> (Execution, QuotaGroup, Client) {
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 32,
        jobs: 64,
        service_jobs: 0,
        input_bytes: 768 * MIB,
        result_bytes: 768 * MIB,
        worker_threads: 1,
        worker_bytes: 64 * MIB,
    });
    let group = quota.admission_group(limits(512 * MIB)).unwrap();
    let disabled = LaneConfig {
        threads: 0,
        queue_slots: 0,
        priority: None,
        resident_bytes_per_thread: 0,
    };
    let owner = Execution::start(
        quota.clone(),
        ExecutionConfig {
            cpu: LaneConfig {
                threads: 1,
                queue_slots: 16,
                priority: None,
                resident_bytes_per_thread: MIB,
            },
            io: disabled,
            service: disabled,
        },
    )
    .unwrap();
    let client = owner.client_in_group(&group, limits(512 * MIB)).unwrap();
    (owner, quota, client)
}
fn cost(bytes: usize) -> JobCost {
    JobCost {
        input_bytes: bytes,
        result_bytes: 0,
    }
}
fn finish(mut owner: Execution) {
    owner.request_shutdown(ShutdownMode::Drain);
    let report = owner
        .join_until_background(Instant::now() + Duration::from_secs(5))
        .unwrap();
    assert!(report.shutdown_complete);
    assert_eq!(report.remaining_workers, 0);
}
#[test]
fn local_statistics_refusal_captures_original_dimension_before_release() {
    let (owner, quota, general) = setup();
    let statistics = general.child(limits(384 * MIB)).unwrap();
    let first = statistics.try_reserve(Lane::Cpu, cost(128 * MIB)).unwrap();
    let history = statistics.try_reserve(Lane::Cpu, cost(256 * MIB)).unwrap();
    let before = quota.snapshot();
    let failure = statistics
        .try_reserve_detailed(Lane::Cpu, cost(64 * MIB))
        .err()
        .unwrap();
    assert_eq!(failure.reason, RejectReason::InputBytes);
    let detail = failure.quota.unwrap();
    assert_eq!(
        detail.boundary,
        AdmissionBoundary::Client {
            ancestor_distance: 0
        }
    );
    assert_eq!(detail.resource, QuotaResource::InputBytes);
    assert_eq!(
        (detail.requested, detail.used, detail.limit),
        (64 * MIB, 384 * MIB, 384 * MIB)
    );
    assert_eq!(quota.snapshot().input_bytes, before.input_bytes);
    assert_eq!(quota.snapshot().jobs, before.jobs);
    drop(first);
    drop(history);
    assert_eq!(quota.snapshot().input_bytes, 0);
    assert_eq!(
        detail.used,
        384 * MIB,
        "captured rejection is not a later snapshot"
    );
    drop(statistics);
    drop(general);
    finish(owner);
}
#[test]
fn general_ancestor_refusal_distinguishes_free_filesystem_child() {
    let (owner, quota, general) = setup();
    let parser = general.child(limits(128 * MIB)).unwrap();
    let statistics = general.child(limits(384 * MIB)).unwrap();
    let filesystem = general.child(limits(128 * MIB)).unwrap();
    let parser_hold = parser.try_reserve(Lane::Cpu, cost(128 * MIB)).unwrap();
    let history = statistics.try_reserve(Lane::Cpu, cost(256 * MIB)).unwrap();
    let stats = statistics.try_reserve(Lane::Cpu, cost(128 * MIB)).unwrap();
    let detail = filesystem
        .try_reserve_detailed(Lane::Cpu, cost(8 * MIB))
        .err()
        .unwrap()
        .quota
        .unwrap();
    assert_eq!(
        detail.boundary,
        AdmissionBoundary::Client {
            ancestor_distance: 1
        }
    );
    assert_eq!(
        (detail.requested, detail.used, detail.limit),
        (8 * MIB, 512 * MIB, 512 * MIB)
    );
    assert_eq!(filesystem.usage().input_bytes, 0);
    assert_eq!(quota.snapshot().input_bytes, 512 * MIB);
    drop(stats);
    drop(history);
    drop(parser_hold);
    drop(filesystem);
    drop(statistics);
    drop(parser);
    drop(general);
    finish(owner);
}
#[test]
fn independent_codec_holds_report_root_instead_of_free_general() {
    let (owner, quota, general) = setup();
    let root_sibling = owner.client(limits(768 * MIB)).unwrap();
    let held = root_sibling
        .try_reserve(Lane::Cpu, cost(768 * MIB))
        .unwrap();
    let detail = general
        .try_reserve_detailed(Lane::Cpu, cost(8 * MIB))
        .err()
        .unwrap()
        .quota
        .unwrap();
    assert_eq!(detail.boundary, AdmissionBoundary::Root);
    assert_eq!(
        (detail.requested, detail.used, detail.limit),
        (8 * MIB, 768 * MIB, 768 * MIB)
    );
    assert_eq!(general.usage().input_bytes, 0);
    assert_eq!(quota.snapshot().input_bytes, 768 * MIB);
    drop(held);
    drop(root_sibling);
    drop(general);
    finish(owner);
}
