//! Real CPU-bank shutdown; isolated quota, no production App fixture.
use super::*;

fn isolated_bank() -> (ClientExecution, QuotaGroup) {
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 4,
        jobs: 4,
        service_jobs: 0,
        input_bytes: 65536,
        result_bytes: 65536,
        worker_threads: 1,
        worker_bytes: MIB,
    });
    let disabled = LaneConfig {
        threads: 0,
        queue_slots: 0,
        priority: None,
        resident_bytes_per_thread: 0,
    };
    let execution = Execution::start(
        quota.clone(),
        ExecutionConfig {
            cpu: LaneConfig {
                threads: 1,
                queue_slots: 2,
                priority: None,
                resident_bytes_per_thread: 4096,
            },
            io: disabled,
            service: disabled,
        },
    )
    .expect("isolated actual bank");
    let general = execution
        .client(ClientLimits {
            jobs: 4,
            service_jobs: 0,
            input_bytes: 65536,
            result_bytes: 65536,
        })
        .expect("actual tenant");
    let location_search = general.clone();
    (
        ClientExecution {
            execution,
            general,
            location_search,
        },
        quota,
    )
}

struct ReleaseGate(Arc<(Mutex<bool>, std::sync::Condvar)>);
impl ReleaseGate {
    fn new() -> Self {
        Self(Arc::new((Mutex::new(false), std::sync::Condvar::new())))
    }
}
impl Drop for ReleaseGate {
    fn drop(&mut self) {
        let (lock, condition) = &*self.0;
        *lock.lock().unwrap_or_else(|error| error.into_inner()) = true;
        condition.notify_all();
    }
}

#[tokio::test]
async fn blocked_shutdown_returns_inspectable_custody_instead_of_only_text() {
    let (execution, quota) = isolated_bank();
    let client = execution.general.clone();
    let gate = ReleaseGate::new();
    let worker_gate = Arc::clone(&gate.0);
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let receipt = client
        .try_reserve(
            ilium_execution::Lane::Cpu,
            JobCost {
                input_bytes: 4096,
                result_bytes: 4096,
            },
        )
        .expect("actual CPU reservation")
        .submit(move |_| {
            entered_tx.send(()).expect("test start observer");
            let (lock, condition) = &*worker_gate;
            let mut released = lock.lock().unwrap();
            while !*released {
                released = condition.wait(released).unwrap();
            }
            Ok::<(), ()>(())
        })
        .unwrap_or_else(|error| panic!("actual admitted CPU blocker: {:?}", error.reason));
    entered_rx.await.expect("callback physically started");
    let failure = execution
        .shutdown()
        .await
        .expect_err("native join deadline");
    assert!(failure.to_string().contains("1 workers still owned"));
    let retained = failure
        .get_ref()
        .and_then(|source| {
            source.downcast_ref::<execution_shutdown::ClientExecutionShutdownError>()
        })
        .expect("public failure owns the actual bank");
    assert!(retained.retains_execution());
    retained.with_observation(|observation| {
        let report = observation.unwrap().as_ref().unwrap();
        assert_eq!(report.remaining_workers, 1);
        assert!(!report.shutdown_complete);
    });
    let inspectable = failure
        .get_ref()
        .and_then(std::error::Error::source)
        .is_some();
    // Release all task-owned native work before testing the original failure.
    drop(gate);
    drop(receipt);
    drop(client);
    assert!(retained.observe_cleanup_background(Instant::now() + Duration::from_secs(5)));
    assert!(!retained.retains_execution());
    drop(failure);
    tokio::time::timeout(Duration::from_secs(5), async {
        while quota.snapshot().worker_threads != 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("actual supervisor completed physical cleanup");
    assert!(
        inspectable,
        "shutdown deadline must return inspectable failure custody, not only a formatted message"
    );
}

#[tokio::test]
async fn cancelled_observer_keeps_original_bank_until_physical_exit() {
    let (execution, quota) = isolated_bank();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let gate = ReleaseGate::new();
    let observer_gate = Arc::clone(&gate.0);
    let mut shutdown = Box::pin(execution_shutdown::shutdown_with(
        execution,
        Instant::now() + Duration::from_millis(10),
        move || {
            entered_tx.send(()).unwrap();
            let (lock, condition) = &*observer_gate;
            let mut released = lock.lock().unwrap();
            while !*released {
                released = condition.wait(released).unwrap();
            }
        },
    ));
    tokio::select! {
        _ = entered_rx => {},
        result = &mut shutdown => panic!("controlled observer returned early: {result:?}"),
    }
    drop(shutdown);
    assert_eq!(quota.snapshot().worker_threads, 1);
    assert_eq!(quota.snapshot().clients, 1);
    drop(gate);
    tokio::time::timeout(Duration::from_secs(5), async {
        while quota.snapshot().worker_threads != 0 || quota.snapshot().clients != 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("canceled observation cleaned up the same bank");
    assert_eq!(quota.snapshot().worker_bytes, 0);
}

#[tokio::test]
async fn cancelled_join_keeps_blocked_cpu_and_tenant_charged_until_release() {
    let (execution, quota) = isolated_bank();
    let gate = ReleaseGate::new();
    let worker_gate = Arc::clone(&gate.0);
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let receipt = execution
        .general
        .try_reserve(
            ilium_execution::Lane::Cpu,
            JobCost {
                input_bytes: 4096,
                result_bytes: 4096,
            },
        )
        .unwrap()
        .submit(move |_| {
            started_tx.send(()).unwrap();
            let (lock, condition) = &*worker_gate;
            let mut released = lock.lock().unwrap();
            while !*released {
                released = condition.wait(released).unwrap();
            }
            Ok::<(), ()>(())
        })
        .unwrap_or_else(|error| panic!("actual CPU blocker: {:?}", error.reason));
    started_rx.await.unwrap();
    let (observer_tx, observer_rx) = tokio::sync::oneshot::channel();
    let mut shutdown = Box::pin(execution_shutdown::shutdown_with(
        execution,
        Instant::now() + Duration::from_millis(10),
        move || observer_tx.send(()).unwrap(),
    ));
    tokio::select! {
        _ = observer_rx => {},
        result = &mut shutdown => panic!("blocked native join returned early: {result:?}"),
    }
    drop(shutdown);
    assert_eq!(quota.snapshot().worker_threads, 1);
    assert_eq!(quota.snapshot().clients, 1);
    assert!(quota.snapshot().worker_bytes > 0);
    drop(receipt);
    drop(gate);
    tokio::time::timeout(Duration::from_secs(5), async {
        while quota.snapshot().worker_threads != 0 || quota.snapshot().clients != 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("same blocked bank physically joined after release");
    assert_eq!(quota.snapshot().worker_bytes, 0);
}

#[tokio::test]
async fn panicking_observer_reports_failure_after_original_bank_cleanup() {
    let (execution, quota) = isolated_bank();
    let failure = execution_shutdown::shutdown_with(
        execution,
        Instant::now() + Duration::from_secs(5),
        || panic!("controlled observer panic before join"),
    )
    .await
    .expect_err("original observer failure");
    let retained = failure
        .get_ref()
        .and_then(|source| {
            source.downcast_ref::<execution_shutdown::ClientExecutionShutdownError>()
        })
        .expect("typed observation custody");
    assert!(!retained.retains_execution());
    retained.with_observation(|observation| {
        assert!(observation.unwrap().as_ref().unwrap().shutdown_complete);
    });
    assert_eq!(quota.snapshot().worker_threads, 0);
    assert_eq!(quota.snapshot().clients, 0);
    assert!(failure.to_string().contains("controlled observer panic"));
}

#[test]
fn shutdown_failure_can_be_retained_in_public_send_sync_io_error() {
    fn assert_error<T: Send + Sync + std::error::Error>() {}
    assert_error::<execution_shutdown::ClientExecutionShutdownError>();
}

#[test]
fn cancelled_queued_observer_survives_runtime_shutdown() {
    // Saturate the actual Tokio blocking pool before polling shutdown. This
    // distinguishes cancellation before callback entry from the running cases.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    let blocker = ReleaseGate::new();
    let blocker_gate = Arc::clone(&blocker.0);
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let _blocking_task = runtime.spawn_blocking(move || {
        started_tx.send(()).unwrap();
        let (lock, condition) = &*blocker_gate;
        let mut released = lock.lock().unwrap();
        while !*released {
            released = condition.wait(released).unwrap();
        }
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();

    let (execution, quota) = isolated_bank();
    let observer_gate = ReleaseGate::new();
    let observer_wait = Arc::clone(&observer_gate.0);
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let mut shutdown = Box::pin(execution_shutdown::shutdown_with(
        execution,
        Instant::now() + Duration::from_secs(5),
        move || {
            let _ = entered_tx.send(());
            let (lock, condition) = &*observer_wait;
            let mut released = lock.lock().unwrap();
            while !*released {
                released = condition.wait(released).unwrap();
            }
        },
    ));
    runtime.block_on(std::future::poll_fn(|context| {
        assert!(std::future::Future::poll(shutdown.as_mut(), context).is_pending());
        std::task::Poll::Ready(())
    }));
    assert!(matches!(
        entered_rx.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    drop(shutdown);
    runtime.shutdown_timeout(Duration::from_millis(10));
    let queued_clients = quota.snapshot().clients;
    drop(blocker);
    let entered = entered_rx.recv_timeout(Duration::from_secs(5)).is_ok();
    let running_clients = quota.snapshot().clients;
    // Always release isolated native work before asserting the outcome.
    drop(observer_gate);
    let cleanup_deadline = Instant::now() + Duration::from_secs(5);
    while (quota.snapshot().worker_threads != 0 || quota.snapshot().clients != 0)
        && Instant::now() < cleanup_deadline
    {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(queued_clients, 1, "queued original tenant stays charged");
    assert!(
        entered,
        "already queued blocking observer runs after runtime shutdown"
    );
    assert_eq!(running_clients, 1, "running original bank still retained");
    assert_eq!(quota.snapshot().worker_threads, 0);
    assert_eq!(quota.snapshot().clients, 0);
}

#[tokio::test]
async fn earlier_cleanup_failure_cannot_discard_actual_execution_custody() {
    let (execution, quota) = isolated_bank();
    // Keep a real preconstruction retirement slot alive after ordinary queue
    // work ended. The CPU owner must remain charged until that slot is settled.
    let retained = execution
        .execution
        .retirement()
        .try_reserve::<Vec<u8>>(4096)
        .unwrap();
    let failure = execution_shutdown::shutdown_with(
        execution,
        Instant::now() + Duration::from_millis(50),
        || {},
    )
    .await
    .expect_err("preconstruction retirement slot still retained");
    let combined = crate::error::preserve_execution_shutdown_error(
        failure,
        Some(crate::error::ClientError::TerminalSetup(
            std::io::Error::other("original preceding terminal cleanup failure"),
        )),
    );
    let crate::error::ClientError::TerminalSetup(error) = &combined else {
        panic!("execution cleanup wrapper");
    };
    let wrapper = error
        .get_ref()
        .unwrap()
        .downcast_ref::<crate::error::ExecutionShutdownFailure>()
        .expect("same execution failure and earlier error");
    assert!(wrapper
        .previous()
        .unwrap()
        .to_string()
        .contains("original preceding terminal cleanup failure"));
    let custody = wrapper
        .shutdown()
        .get_ref()
        .unwrap()
        .downcast_ref::<execution_shutdown::ClientExecutionShutdownError>()
        .expect("actual bank still publicly inspectable");
    assert!(custody.retains_execution());
    custody.with_observation(|observation| {
        let report = observation.unwrap().as_ref().unwrap();
        assert_eq!(report.remaining_workers, 1);
        assert_eq!(report.health.lanes[0].retirement_live, 1);
        assert!(!report.shutdown_complete);
    });
    drop(retained);
    assert!(custody.observe_cleanup_background(Instant::now() + Duration::from_secs(5)));
    assert_eq!(quota.snapshot().worker_threads, 0);
    assert_eq!(quota.snapshot().clients, 0);
}

#[test]
fn observer_spawn_panic_returns_original_bank_custody() {
    struct Wake;
    impl std::task::Wake for Wake {
        fn wake(self: Arc<Self>) {}
    }
    let (execution, quota) = isolated_bank();
    let waker = std::task::Waker::from(Arc::new(Wake));
    let mut context = std::task::Context::from_waker(&waker);
    let mut shutdown = Box::pin(execution_shutdown::shutdown_with(
        execution,
        Instant::now() + Duration::from_secs(5),
        || {},
    ));
    // No runtime: Tokio rejects observer construction by panicking. The seam
    // must return the same bank rather than unwind its final explicit owner.
    let observed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        std::future::Future::poll(shutdown.as_mut(), &mut context)
    }));
    drop(shutdown);
    if let Ok(std::task::Poll::Ready(Err(failure))) = &observed {
        let retained = failure
            .get_ref()
            .and_then(|source| {
                source.downcast_ref::<execution_shutdown::ClientExecutionShutdownError>()
            })
            .expect("spawn refusal returns original bank");
        assert!(retained.retains_execution());
        assert!(retained.observe_cleanup_background(Instant::now() + Duration::from_secs(5)));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while quota.snapshot().worker_threads != 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        matches!(observed, Ok(std::task::Poll::Ready(Err(_)))),
        "observer spawn panic must become inspectable original-bank refusal"
    );
    assert_eq!(quota.snapshot().worker_threads, 0);
    assert_eq!(quota.snapshot().clients, 0);
}
