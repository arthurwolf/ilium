// Synthetic payloads exercise the actual CPU bank and recovery mailbox;
// this test does not construct or qualify the complete client App.
use super::*;
use std::collections::HashSet;
use std::sync::mpsc;

struct ForeignOriginal {
    index: usize,
    notice: mpsc::Sender<(usize, std::thread::ThreadId)>,
}
impl Drop for ForeignOriginal {
    fn drop(&mut self) {
        let _ = self.notice.send((self.index, std::thread::current().id()));
    }
}

struct RootOriginal {
    bytes: Vec<u8>,
    notice: mpsc::Sender<(usize, std::thread::ThreadId)>,
}
impl Drop for RootOriginal {
    fn drop(&mut self) {
        let _ = self.notice.send((63, std::thread::current().id()));
    }
}

#[test]
fn bounded_scan_recovers_last_original_without_destroying_foreign_custody() {
    let quota = QuotaGroup::new(crate::budget::QuotaLimits {
        clients: 1,
        jobs: 1,
        service_jobs: 0,
        input_bytes: 4096,
        result_bytes: 4096,
        worker_threads: 1,
        worker_bytes: 4 * 1024 * 1024,
    });
    let disabled = LaneConfig {
        threads: 0,
        queue_slots: 0,
        priority: None,
        resident_bytes_per_thread: 0,
    };
    let mut execution = Execution::start(
        quota.clone(),
        ExecutionConfig {
            cpu: LaneConfig {
                threads: 1,
                queue_slots: 1,
                priority: None,
                resident_bytes_per_thread: 4096,
            },
            io: disabled,
            service: disabled,
        },
    )
    .unwrap();
    let client = execution
        .client(ClientLimits {
            jobs: 1,
            service_jobs: 0,
            input_bytes: 4096,
            result_bytes: 4096,
        })
        .unwrap();
    let (started_sender, started_receiver) = mpsc::sync_channel(1);
    let (release_sender, release_receiver) = mpsc::sync_channel(0);
    let blocker = client
        .try_submit(
            Lane::Cpu,
            crate::budget::JobCost {
                input_bytes: 4096,
                result_bytes: 4096,
            },
            move |_| {
                let _ = started_sender.send(());
                let _ = release_receiver.recv();
                Ok::<(), ()>(())
            },
        )
        .unwrap();
    started_receiver
        .recv_timeout(Duration::from_secs(3))
        .unwrap();

    // Keep the actual CPU occupied while filling its finite recovery
    // mailbox, so the target is deterministically behind 63 other types.
    let caller = std::thread::current().id();
    let (notice_sender, notice_receiver) = mpsc::channel();
    let retirement = execution.retirement();
    for index in 0..63 {
        let mut original = retirement
            .try_reserve::<ForeignOriginal>(4096)
            .unwrap()
            .attach(ForeignOriginal {
                index,
                notice: notice_sender.clone(),
            });
        original.force_disconnected_for_test();
        drop(original);
    }
    let mut original = retirement
        .try_reserve::<RootOriginal>(16384)
        .unwrap()
        .attach(RootOriginal {
            bytes: vec![29; 8192],
            notice: notice_sender,
        });
    let original_address = original.bytes.as_ptr();
    original.force_disconnected_for_test();
    drop(original);
    assert_eq!(execution.monitor().health().lanes[0].retirement_live, 64);
    assert_eq!(
        execution.monitor().health().lanes[0].retirement_recovery_pending,
        64
    );
    assert!(notice_receiver.try_recv().is_err());

    let mut recovered = None;
    let mut inspected = 0;
    for _ in 0..crate::retirement::RETIREMENT_SLOTS {
        let Some(ticket) = retirement.try_take_failed() else {
            break;
        };
        inspected += 1;
        match ticket.try_into_typed::<RootOriginal>() {
            Ok(original) => {
                recovered = Some(original);
                break;
            }
            Err(ticket) => drop(ticket),
        }
    }
    let recovered = recovered.expect("last root original remains reachable within 64 slots");
    assert_eq!(inspected, 64);
    assert_eq!(recovered.bytes.as_ptr(), original_address);
    assert_eq!(recovered.bytes, vec![29; 8192]);
    assert_eq!(
        execution.monitor().health().lanes[0].retirement_recovery_pending,
        63
    );
    assert_eq!(execution.monitor().health().lanes[0].retirement_live, 64);
    assert!(notice_receiver.try_recv().is_err());

    // Returning the disconnected original preserves the same debit and
    // custody. The existing CPU consumes every original after release.
    drop(recovered);
    execution.request_shutdown(ShutdownMode::Cancel);
    release_sender.send(()).unwrap();
    drop(blocker);
    let report = execution
        .join_until_background(Instant::now() + Duration::from_secs(3))
        .unwrap();
    assert!(report.shutdown_complete);
    assert_eq!(report.remaining_workers, 0);
    assert_eq!(report.health.lanes[0].retirement_live, 0);
    assert_eq!(report.health.lanes[0].retirement_recovery_pending, 0);
    let mut destroyed = HashSet::new();
    for _ in 0..64 {
        let (index, thread) = notice_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        assert_ne!(thread, caller);
        assert!(destroyed.insert(index));
    }
    assert_eq!(destroyed.len(), 64);
    assert!(notice_receiver.try_recv().is_err());
    drop(client);
    drop(retirement);
    drop(execution);
    assert_eq!(quota.snapshot().worker_threads, 0);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}
