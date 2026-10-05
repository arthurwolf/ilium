#[cfg(test)]
mod tests {
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, JobContext, JobCost, Lane, LaneConfig,
        QuotaGroup, QuotaLimits, Retiring,
    };
    use std::{
        sync::mpsc::{self, SyncSender},
        thread::{self, ThreadId},
        time::{Duration, Instant},
    };

    struct Released {
        label: &'static str,
        thread: ThreadId,
        pointer: usize,
        bytes: usize,
        charged: usize,
    }
    struct Original {
        label: &'static str,
        payload: Vec<u8>,
        quota: QuotaGroup,
        released: SyncSender<Released>,
    }
    impl Drop for Original {
        fn drop(&mut self) {
            assert!(self.payload.iter().all(|byte| *byte == 0x37));
            self.released
                .try_send(Released {
                    label: self.label,
                    thread: thread::current().id(),
                    pointer: self.payload.as_ptr() as usize,
                    bytes: self.payload.len(),
                    charged: self.quota.snapshot().worker_bytes,
                })
                .unwrap();
        }
    }
    struct CombinedOriginals {
        _scene: Retiring<Original>,
        _editor: Retiring<Original>,
        released: SyncSender<Released>,
    }
    impl Drop for CombinedOriginals {
        fn drop(&mut self) {
            self.released
                .try_send(Released {
                    label: "outer",
                    thread: thread::current().id(),
                    pointer: 0,
                    bytes: 0,
                    charged: 0,
                })
                .unwrap();
        }
    }
    struct ShutdownError {
        // Exact production custody order: original envelopes precede bank.
        _original: Retiring<CombinedOriginals>,
        _execution: Execution,
    }

    #[test]
    fn dropping_error_then_bank_retires_both_nested_originals_before_physical_exit() {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 2,
            jobs: 2,
            service_jobs: 0,
            input_bytes: 16 * 1024,
            result_bytes: 16 * 1024,
            worker_threads: 1,
            worker_bytes: 4 * 1024 * 1024,
        });
        let disabled = LaneConfig {
            threads: 0,
            queue_slots: 0,
            priority: None,
            resident_bytes_per_thread: 0,
        };
        let bank = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: LaneConfig {
                    threads: 1,
                    queue_slots: 2,
                    priority: None,
                    resident_bytes_per_thread: 64 * 1024,
                },
                io: disabled,
                service: disabled,
            },
        )
        .unwrap();
        let monitor = bank.monitor();
        let client = bank
            .client(ClientLimits {
                jobs: 2,
                service_jobs: 0,
                input_bytes: 16 * 1024,
                result_bytes: 16 * 1024,
            })
            .unwrap();
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let blocker = move |_context: JobContext| {
            entered_tx.send(thread::current().id()).unwrap();
            release_rx.recv().unwrap();
            Ok::<_, ()>(())
        };
        let receipt = client
            .try_submit(
                Lane::Cpu,
                JobCost {
                    input_bytes: std::mem::size_of_val(&blocker) + 1024,
                    result_bytes: 8,
                },
                blocker,
            )
            .unwrap();
        let cpu_thread = entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let caller = thread::current().id();
        assert_ne!(cpu_thread, caller);
        let (released_tx, released_rx) = mpsc::sync_channel(8);
        let mut pointers = Vec::new();
        let mut capture = |label| {
            let permit = bank
                .retirement()
                .try_reserve::<Original>(64 * 1024 + 4096)
                .unwrap();
            let payload = vec![0x37; 64 * 1024];
            pointers.push(payload.as_ptr() as usize);
            permit.attach(Original {
                label,
                payload,
                quota: quota.clone(),
                released: released_tx.clone(),
            })
        };
        let scene = capture("scene");
        let editor = capture("editor");
        let outer = bank
            .retirement()
            .try_reserve::<CombinedOriginals>(4096)
            .unwrap()
            .attach(CombinedOriginals {
                _scene: scene,
                _editor: editor,
                released: released_tx,
            });
        let error = ShutdownError {
            _original: outer,
            _execution: bank,
        };
        assert_eq!(monitor.health().lanes[0].retirement_live, 3);
        drop(error);
        assert!(released_rx.try_recv().is_err());
        assert_eq!(monitor.health().lanes[0].retirement_live, 3);
        assert_eq!(quota.snapshot().worker_threads, 1);
        release_tx.send(()).unwrap();
        let outer = released_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let scene = released_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let editor = released_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            [outer.label, scene.label, editor.label],
            ["outer", "scene", "editor"]
        );
        for report in [&outer, &scene, &editor] {
            assert_eq!(report.thread, cpu_thread);
            assert_ne!(report.thread, caller);
        }
        assert_eq!([scene.pointer, editor.pointer], [pointers[0], pointers[1]]);
        assert_eq!(scene.bytes, 64 * 1024);
        assert_eq!(editor.bytes, 64 * 1024);
        assert!(scene.charged >= 64 * 1024);
        assert!(editor.charged >= 64 * 1024);
        let deadline = Instant::now() + Duration::from_secs(5);
        // Physical admission is released by the supervisor after actual native
        // join; callback completion alone cannot satisfy this condition.
        while quota.snapshot().worker_threads != 0 {
            assert!(
                Instant::now() < deadline,
                "native CPU owner did not physically retire"
            );
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(monitor.health().lanes[0].retirement_live, 0);
        assert_eq!(monitor.health().lanes[0].retirement_completed, 3);
        assert_eq!(monitor.health().lanes[0].retirement_recovery_pending, 0);
        assert!(released_rx.try_recv().is_err());
        drop(receipt);
        drop(client);
        drop(monitor);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
}
