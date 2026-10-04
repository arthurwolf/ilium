use ilium_execution::{
    ClientLimits, Execution, ExecutionConfig, JobCost, JobOutcome, JobPoll, Lane, LaneConfig,
    QuotaGroup, QuotaLimits, RejectReason, Retained, ShutdownMode,
};
use std::time::{Duration, Instant};

fn limits() -> QuotaLimits {
    QuotaLimits {
        clients: 32,
        jobs: 8,
        service_jobs: 1,
        input_bytes: 768,
        result_bytes: 768,
        worker_threads: 4,
        worker_bytes: 64 * 1024,
    }
}
fn client_limits(bytes: usize) -> ClientLimits {
    ClientLimits {
        jobs: 6,
        service_jobs: 0,
        input_bytes: bytes,
        result_bytes: bytes,
    }
}
fn bank(quota: QuotaGroup) -> Execution {
    let disabled = LaneConfig {
        threads: 0,
        queue_slots: 0,
        priority: None,
        resident_bytes_per_thread: 0,
    };
    Execution::start(
        quota,
        ExecutionConfig {
            cpu: LaneConfig {
                threads: 1,
                queue_slots: 4,
                priority: None,
                resident_bytes_per_thread: 1024,
            },
            io: disabled,
            service: disabled,
        },
    )
    .unwrap()
}
fn produced(client: &ilium_execution::Client, bytes: usize) -> Retained<Vec<u8>> {
    let original = vec![7u8; bytes];
    let mut receipt = client
        .try_submit(
            Lane::Cpu,
            JobCost {
                input_bytes: bytes,
                result_bytes: bytes,
            },
            move |_| Ok::<_, std::convert::Infallible>(original),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match receipt.try_take() {
            JobPoll::Ready(outcome) => {
                return outcome.map(|outcome| match outcome {
                    JobOutcome::Finished(Ok(value)) => value,
                    JobOutcome::Finished(Err(error)) => match error {},
                    JobOutcome::NotStarted { .. } => panic!("actual CPU callback was not started"),
                    JobOutcome::Panicked => panic!("actual CPU callback panicked"),
                })
            }
            JobPoll::Pending if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(1))
            }
            _ => panic!("actual CPU completion missing"),
        }
    }
}

#[test]
fn mixed_banks_share_general_retained_credit_and_leave_both_codec_directions_admissible() {
    let quota = QuotaGroup::new(limits());
    let general = quota.admission_group(client_limits(512)).unwrap();
    let mut interactive = bank(quota.clone());
    let mut standalone = bank(quota.clone());
    let documents = interactive
        .client_in_group(&general, client_limits(512))
        .unwrap();
    let outgoing = standalone
        .client_in_group(&general.clone(), client_limits(512))
        .unwrap();
    let decoder_group = quota.admission_group(client_limits(128)).unwrap();
    let encoder_group = quota.admission_group(client_limits(128)).unwrap();
    let decoder = standalone
        .client_in_group(&decoder_group, client_limits(128))
        .unwrap();
    let other_decoder = interactive
        .client_in_group(&decoder_group, client_limits(128))
        .unwrap();
    let encoder = interactive
        .client_in_group(&encoder_group, client_limits(128))
        .unwrap();
    let document_result = produced(&documents, 256);
    let outgoing_result = produced(&outgoing, 256);
    let original_pointer = outgoing_result.view().as_ptr();
    assert_eq!(general.usage().input_bytes, 512);
    // External actors must declare their reservation metadata before the
    // group checks bytes. A one-byte cost tests InvalidCost, not saturation.
    let refusal_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let refusal = outgoing.try_reserve_external(JobCost {
            input_bytes: std::mem::size_of::<ilium_execution::ExternalReservation>(),
            result_bytes: 1,
        });
        if matches!(refusal, Err(RejectReason::Busy)) {
            assert!(
                Instant::now() < refusal_deadline,
                "admission ledger became available"
            );
            std::thread::yield_now();
            continue;
        }
        assert_eq!(refusal.err(), Some(RejectReason::InputBytes));
        break;
    }
    // This real finite reservation models the production admitted header
    // waiting for its body: no callback has run and ownership remains charged.
    let admitted_body = decoder
        .try_reserve(
            Lane::Cpu,
            JobCost {
                input_bytes: 128,
                result_bytes: 128,
            },
        )
        .unwrap();
    assert!(matches!(
        other_decoder.try_reserve(
            Lane::Cpu,
            JobCost {
                input_bytes: 128,
                result_bytes: 128
            }
        ),
        Err(RejectReason::InputBytes)
    ));
    let encoded = produced(&encoder, 128);
    assert_eq!(
        quota.snapshot().input_bytes,
        768,
        "a blocked admitted body cannot consume opposite-direction headroom"
    );
    drop(admitted_body);
    let decoded = produced(&decoder, 128);
    assert_eq!(quota.snapshot().input_bytes, 768);
    assert_eq!(quota.snapshot().result_bytes, 768);
    interactive.request_shutdown(ShutdownMode::Cancel);
    interactive
        .join_until_background(Instant::now() + Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        general.usage().input_bytes,
        512,
        "physical bank exit cannot release retained payloads"
    );
    assert_eq!(outgoing_result.view().as_ptr(), original_pointer);
    drop(document_result);
    drop(encoded);
    let later = produced(&outgoing, 64);
    assert_eq!(
        later.view().len(),
        64,
        "closing another bank cannot close this group or bank"
    );
    assert_eq!(general.usage().input_bytes, 320);
    drop(later);
    drop(outgoing_result);
    drop(decoded);
    assert_eq!(general.usage().jobs, 0);
    assert_eq!(quota.snapshot().input_bytes, 0);
    standalone.request_shutdown(ShutdownMode::Cancel);
    standalone
        .join_until_background(Instant::now() + Duration::from_secs(5))
        .unwrap();
}

#[test]
fn admission_groups_reject_foreign_quota_and_bound_depth_and_registration() {
    let quota = QuotaGroup::new(limits());
    let foreign = QuotaGroup::new(limits());
    let group = quota.admission_group(client_limits(512)).unwrap();
    let mut execution = bank(foreign.clone());
    assert!(matches!(
        execution.client_in_group(&group, client_limits(512)),
        Err(RejectReason::InvalidCost)
    ));
    assert_eq!(foreign.snapshot().clients, 0);
    assert_eq!(quota.snapshot().clients, 1);
    let mut deepest = group.clone();
    for _ in 1..8 {
        deepest = deepest.child(client_limits(512)).unwrap();
    }
    assert!(matches!(
        deepest.child(client_limits(512)),
        Err(RejectReason::InvalidCost)
    ));
    assert_eq!(quota.snapshot().clients, 8);
    drop(deepest);
    drop(group);
    assert_eq!(quota.snapshot().clients, 0);
    execution.request_shutdown(ShutdownMode::Cancel);
    execution
        .join_until_background(Instant::now() + Duration::from_secs(5))
        .unwrap();
    let registration_quota = QuotaGroup::new(QuotaLimits {
        clients: 1,
        ..limits()
    });
    let group = registration_quota
        .admission_group(client_limits(512))
        .unwrap();
    assert!(matches!(
        group.child(client_limits(512)),
        Err(RejectReason::ClientLimit)
    ));
}
