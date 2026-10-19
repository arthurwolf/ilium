use super::*;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Disconnect blocked callbacks on success or unwinding, before bank teardown.
/// Tests never leave native workers waiting for a test assertion to succeed.
#[derive(Default)]
struct ReleaseWorkers(Vec<mpsc::Sender<()>>);

impl Drop for ReleaseWorkers {
    fn drop(&mut self) {
        self.0.clear();
    }
}

#[tokio::test]
async fn detection_progresses_with_full_general_and_codec_envelopes_and_busy_workers() {
    let mut owner = ServerExecution::start().expect("server execution");
    let root = owner.quota_group();
    let mut release = ReleaseWorkers::default();
    let (started_sender, started_receiver) = mpsc::channel();
    let mut receipts = Vec::new();
    for lane in [Lane::Cpu, Lane::Cpu, Lane::Io, Lane::Io] {
        let (release_sender, release_receiver) = mpsc::channel();
        release.0.push(release_sender);
        let started = started_sender.clone();
        receipts.push(
            owner
                .client
                .foundation
                .try_reserve(
                    lane,
                    JobCost {
                        input_bytes: 1024,
                        result_bytes: 1024,
                    },
                )
                .expect("general worker admission")
                .submit(move |_| {
                    let _ = started.send(());
                    let _ = release_receiver.recv();
                    Ok::<_, io::Error>(())
                })
                .map_err(|error| error.reason)
                .expect("blocked native callback"),
        );
    }
    for _ in 0..4 {
        started_receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("all general CPU and I/O workers entered their callbacks");
    }

    // Reserve accounting envelopes only: no giant test payloads are allocated.
    // Existing blocked callbacks already debit part of the general tenant.
    let usage = owner.client.foundation.usage();
    let general = owner
        .client
        .foundation
        .try_reserve(
            Lane::Io,
            JobCost {
                input_bytes: GENERAL_INPUT_BYTES - usage.input_bytes,
                result_bytes: GENERAL_RESULT_BYTES - usage.result_bytes,
            },
        )
        .expect("remaining general input/result envelope");
    let codec_cost = JobCost {
        input_bytes: 128 * MIB,
        result_bytes: 192 * MIB,
    };
    let decoders: Vec<_> = (0..4)
        .map(|_| {
            owner
                .decoder
                .foundation
                .try_reserve(Lane::Cpu, codec_cost)
                .expect("full existing decoder capacity")
        })
        .collect();
    let encoders: Vec<_> = (0..2)
        .map(|_| {
            owner
                .encoder
                .foundation
                .try_reserve(Lane::Cpu, codec_cost)
                .expect("full existing encoder capacity")
        })
        .collect();
    let full = root.snapshot();
    let full_input = full.input_bytes == full.limits.input_bytes;
    let full_result = full.result_bytes == full.limits.result_bytes;
    let storage = root
        .reserve_external_storage(full.limits.worker_bytes - full.worker_bytes)
        .expect("remaining shared worker storage");
    let storage_full = root.snapshot().worker_bytes == full.limits.worker_bytes;

    let detection_result = tokio::time::timeout(Duration::from_secs(10), async {
        for lane in [Lane::Cpu, Lane::Io] {
            let result = owner
                .detection
                .run(
                    lane,
                    JobCost {
                        input_bytes: 128,
                        result_bytes: 128,
                    },
                    |_| Ok::<_, io::Error>(42_u64),
                )
                .await?;
            if *result.view() != 42 {
                return Err(ExecutionError::Lost);
            }
        }
        Ok::<_, ExecutionError<io::Error>>(())
    })
    .await;

    // Release physical callbacks before checking the result, including timeout.
    drop(release);
    drop((storage, general, decoders, encoders, receipts));
    owner.request_shutdown();
    let joined = owner
        .test_join_until_background(Instant::now() + PHYSICAL_JOIN_TEST_WAIT)
        .expect("join both execution banks");
    assert!(full_input, "general and codec inputs exactly fill root");
    assert!(full_result, "general and codec results exactly fill root");
    assert!(storage_full, "unreserved root worker storage is exhausted");
    detection_result
        .expect("detection progresses while all general workers stay blocked")
        .expect("reserved detection CPU and I/O admissions succeed");
    assert!(joined.shutdown_complete, "both banks physically joined");
}

#[tokio::test]
async fn retained_detection_output_keeps_partition_until_final_result_release() {
    let mut owner = ServerExecution::start().expect("server execution");
    let root = owner.quota_group();
    let result = owner
        .detection
        .run(
            Lane::Cpu,
            JobCost {
                input_bytes: 128,
                result_bytes: 128,
            },
            |_| Ok::<_, io::Error>(42_u64),
        )
        .await
        .expect("retained detection output");
    owner.request_shutdown();
    let joined = owner
        .test_join_until_background(Instant::now() + PHYSICAL_JOIN_TEST_WAIT)
        .expect("join both banks");
    assert!(joined.shutdown_complete, "physical workers have retired");
    drop(owner);
    // No child client, monitor, or quota clone survives here. Only the retained
    // result can keep the detection ledger and its parent reservation alive.
    let held_bytes = root.snapshot().worker_bytes;
    assert_eq!(*result.view(), 42);
    drop(result);
    let released_bytes = root.snapshot().worker_bytes;
    assert_eq!(
        held_bytes - released_bytes,
        DETECTION_INPUT_BYTES + DETECTION_RESULT_BYTES + DETECTION_WORKER_BYTES,
        "the complete carved partition follows the retained result"
    );
}
