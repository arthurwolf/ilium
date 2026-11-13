//! Isolated real-PTY regressions; no agent CLI or user session is launched.
use super::*;
use ilium_core::{PaneContentKind, ROOT_ID};
use ilium_execution::{QuotaGroup, QuotaLimits};
use std::sync::{Condvar, Mutex};
use tokio::sync::Notify;

struct Fixture {
    state: Arc<ServerState>,
    pane_id: NodeId,
    _directory: tempfile::TempDir,
    playback_task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.playback_task.abort();
    }
}
impl Fixture {
    async fn new() -> Self {
        // Run with TMPDIR rooted in the job's marked mounted scratch directory.
        let directory = tempfile::tempdir().expect("isolated native-spawn fixture");
        let cwd = ilium_platform::paths::canonicalize(directory.path()).unwrap();
        let execution = ServerExecution::start().expect("isolated execution owner");
        let (sound_requests, playback_task) =
            crate::sounds::spawn(Arc::new(crate::NoopSoundPlayer), execution.client.clone());
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "native-spawn-test".into(),
            session_cwd: cwd.clone(),
            home_dir: cwd.clone(),
            snapshot_path: cwd.join("snapshot.json"),
            socket_path: cwd.join("session.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: crate::sounds::test_settings(ilium_sound::SoundSettings::default()),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: false,
        }));
        assert!(state.execution.set(execution).is_ok());
        let pane_id = {
            let mut tree = state.tree.write().await;
            let group = tree.add_group(ROOT_ID, "native-spawn").unwrap();
            tree.add_pane(group, "cat", PaneContentKind::Terminal)
                .unwrap()
        };
        Self {
            state,
            pane_id,
            _directory: directory,
            playback_task,
        }
    }

    fn request(&self) -> NativeSpawnRequest {
        NativeSpawnRequest {
            pane_id: self.pane_id,
            origin: TerminalOrigin::Command("exec cat".into()),
            cwd: self.state.session_cwd.clone(),
            before_generation: ilium_platform::secure_fs::spawn_directory_generation(
                &self.state.session_cwd,
            )
            .unwrap(),
            repository: None,
            custody_ticket: None,
            deferred_workspace: false,
        }
    }
}

#[derive(Default)]
struct Gate {
    released: Mutex<bool>,
    changed: Condvar,
}
impl Gate {
    fn wait(&self) {
        let mut released = self.released.lock().unwrap();
        while !*released {
            released = self.changed.wait(released).unwrap();
        }
    }
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.changed.notify_all();
    }
}
struct ReleaseOnDrop(Arc<Gate>);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}

async fn wait_no_pty_workers(quota: &QuotaGroup, changed: &Notify) {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let notified = changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if quota.snapshot().worker_threads == 0 {
                break;
            }
            notified.await;
        }
    })
    .await
    .expect("all owned PTY workers, including child reaper, physically joined");
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_slow_native_result_keeps_fence_and_debit_until_callback_finishes() {
    let fixture = Fixture::new().await;
    let execution = fixture.state.execution.get().unwrap();
    let client = execution.client.clone();
    let changed = Arc::new(Notify::new());
    let wake = Arc::clone(&changed);
    let worker_count = 5;
    // Exactly one normal PTY, not a lowered production capacity gate.
    let quota = QuotaGroup::new_with_admission_wake(
        QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: worker_count,
            worker_bytes: worker_count * ilium_pty::PTY_WORKER_STACK_BYTES,
        },
        move || wake.notify_waiters(),
    );
    let request = fixture.request();
    let (cost, completion_bytes) = spawn_cost(&fixture.state, &request, None).unwrap();
    let storage = client.reserve_storage(completion_bytes).await.unwrap();
    let guard = Arc::clone(&fixture.state.workspace_spawn_lock)
        .lock_owned()
        .await;
    let job = NativeSpawnJob {
        request,
        session_name: fixture.state.session_name.clone(),
        socket_path: fixture.state.socket_path.clone(),
        worktree_root: None,
        config_dir: None,
        quota: quota.clone(),
        scope: NativeSpawnScope {
            _spawn_guard: guard,
            _repository: None,
            _storage: storage,
        },
    };
    let gate = Arc::new(Gate::default());
    let _release_on_unwind = ReleaseOnDrop(Arc::clone(&gate));
    let worker_gate = Arc::clone(&gate);
    let (entered, started) = tokio::sync::oneshot::channel();
    let reservation = client.reserve(Lane::Io, cost).await.unwrap();
    let worker_client = client.clone();
    let requester = tokio::spawn(async move {
        worker_client
            .run_reserved(reservation, move |context| {
                let spawned = job.run(context)?;
                let _ = entered.send((
                    std::thread::current().id(),
                    spawned.spawned.session.process_id(),
                ));
                worker_gate.wait(); // Deterministic slow native-completion boundary.
                Ok::<_, NativeSpawnError>(spawned)
            })
            .await
    });
    let (worker_thread, child) = tokio::time::timeout(Duration::from_secs(15), started)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(worker_thread, std::thread::current().id());
    assert!(child.is_some(), "real isolated child was created");
    requester.abort();
    match requester.await {
        Err(error) => assert!(error.is_cancelled()),
        Ok(_) => panic!("cancelled requester unexpectedly returned"),
    }
    assert!(fixture.state.workspace_spawn_lock.try_lock().is_err());
    assert_eq!(
        client.foundation.usage().jobs,
        1,
        "physical callback still owns General debit"
    );
    assert!(quota.snapshot().worker_threads > 0);
    tokio::time::sleep(Duration::from_millis(1)).await;
    // A current-thread runtime timer progressed while the worker stayed gated.
    assert!(fixture.state.workspace_spawn_lock.try_lock().is_err());
    gate.release();
    let turn = tokio::time::timeout(
        Duration::from_secs(15),
        fixture.state.workspace_spawn_lock.lock(),
    )
    .await
    .expect("cancelled result requests shutdown, then releases fence");
    drop(turn);
    wait_no_pty_workers(&quota, &changed).await;
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let completed = client.completion_notification();
            let notified = completed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if client.foundation.usage().jobs == 0 {
                break;
            }
            notified.await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn ready_spawn_has_no_general_job_during_publication_wait() {
    let fixture = Fixture::new().await;
    let execution = fixture.state.execution.get().unwrap();
    let initial_storage = execution.client.quota_group().snapshot().worker_bytes;
    let native = spawn(&fixture.state, fixture.request()).await.unwrap();
    assert_eq!(
        execution.client.foundation.usage().jobs,
        0,
        "production entrypoint must retire General credit before any caller registration wait"
    );
    assert!(
        execution.client.quota_group().snapshot().worker_bytes > initial_storage,
        "independent completion storage still covers the unregistered result"
    );
    assert!(fixture.state.workspace_spawn_lock.try_lock().is_err());
    assert_eq!(native.origin, TerminalOrigin::Command("exec cat".into()));
    assert!(native.spawned.session.process_id().is_some());
    let tree = fixture.state.tree.write().await;
    tokio::time::sleep(Duration::from_millis(1)).await;
    assert_eq!(
        execution.client.foundation.usage().jobs,
        0,
        "simulated registration contention must not recapture General jobs"
    );
    drop(tree);
    let NativeSpawned { spawned, scope, .. } = native;
    cleanup_session(execution, spawned.session).await;
    drop(scope);
    let turn = fixture.state.workspace_spawn_lock.lock().await;
    drop(turn);
    assert_eq!(
        execution.client.quota_group().snapshot().worker_bytes,
        initial_storage
    );
}

#[tokio::test(flavor = "current_thread")]
async fn stale_generation_refuses_native_spawn_and_releases_admission() {
    let fixture = Fixture::new().await;
    let mut request = fixture.request();
    assert!(
        request.before_generation.is_some(),
        "Unix generation is available"
    );
    request.before_generation = None;
    let error = match spawn(&fixture.state, request).await {
        Ok(_) => panic!("stale generation must never create a terminal"),
        Err(error) => error,
    };
    assert!(matches!(error, NativeSpawnError::Admission(ref message)
        if message.contains("changed identity")));
    assert!(fixture.state.workspace_spawn_lock.try_lock().is_ok());
    assert_eq!(
        fixture
            .state
            .execution
            .get()
            .unwrap()
            .client
            .foundation
            .usage()
            .jobs,
        0
    );
    assert!(fixture.state.panes.read().await.is_empty());
}

async fn cleanup_session(execution: &ServerExecution, mut session: ilium_pty::PtySession) {
    let report = execution
        .client
        .run(
            Lane::Io,
            JobCost {
                input_bytes: NATIVE_WORKING_BYTES,
                result_bytes: 4096,
            },
            move |_| {
                let report = session.shutdown_blocking(Duration::from_secs(15))?;
                drop(session);
                Ok::<_, PtyError>(report)
            },
        )
        .await
        .expect("admitted isolated fixture cleanup");
    assert!(
        report.view().pending.is_empty(),
        "fixture PTY workers must physically join"
    );
    assert!(report.view().panicked.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn busy_spawn_fence_retains_neither_general_nor_completion_credit() {
    let fixture = Fixture::new().await;
    let execution = fixture.state.execution.get().unwrap();
    let initial_storage = execution.client.quota_group().snapshot().worker_bytes;
    let held_fence = fixture.state.workspace_spawn_lock.lock().await;
    let request = fixture.request();
    let mut future = std::pin::pin!(spawn(&fixture.state, request));
    {
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(std::future::Future::poll(future.as_mut(), &mut context).is_pending());
    }
    assert_eq!(
        execution.client.foundation.usage().jobs,
        0,
        "waiting launch must release its prepared General reservation"
    );
    assert_eq!(
        execution.client.quota_group().snapshot().worker_bytes,
        initial_storage,
        "waiting launch must also release independent completion storage"
    );
    drop(held_fence);
    let native = tokio::time::timeout(Duration::from_secs(15), future)
        .await
        .unwrap()
        .unwrap();
    let NativeSpawned { spawned, scope, .. } = native;
    cleanup_session(execution, spawned.session).await;
    drop(scope);
}

#[tokio::test(flavor = "current_thread")]
async fn removed_node_refuses_actual_spawn_entrypoint_before_child_creation() {
    let fixture = Fixture::new().await;
    let request = fixture.request();
    fixture
        .state
        .tree
        .write()
        .await
        .remove_node(fixture.pane_id)
        .unwrap();
    let error = match spawn(&fixture.state, request).await {
        Ok(_) => panic!("removed pane must not launch a native child"),
        Err(error) => error,
    };
    assert!(matches!(error, NativeSpawnError::NodeRemoved(pane_id) if pane_id == fixture.pane_id));
    let execution = fixture.state.execution.get().unwrap();
    assert_eq!(execution.pty_quota_group().snapshot().worker_threads, 0);
    assert_eq!(execution.client.foundation.usage().jobs, 0);
    assert!(fixture.state.workspace_spawn_lock.try_lock().is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn actual_handler_registers_real_pty_and_preserves_terminal_input() {
    let fixture = Fixture::new().await;
    let cwd = fixture.state.session_cwd.clone();
    crate::ipc::handlers::spawn_and_register_pane_in_directory(
        &fixture.state,
        fixture.pane_id,
        crate::pane::PaneSnapshotKind::Terminal(TerminalOrigin::Command("exec cat".into())),
        &cwd,
    )
    .await
    .expect("real shared handler must register the offloaded terminal");
    let (input, mut output) = {
        let panes = fixture.state.panes.read().await;
        let Some(crate::pane::PaneResource::Terminal(runtime)) = panes.get(&fixture.pane_id) else {
            panic!("actual terminal runtime must be present");
        };
        assert_eq!(runtime.origin, TerminalOrigin::Command("exec cat".into()));
        (
            runtime.session.input_handle(),
            runtime.session.subscribe_output_bytes(),
        )
    };
    let marker = b"NATIVE_SPAWN_OFFLOAD_INPUT_OK";
    input
        .write(b"NATIVE_SPAWN_OFFLOAD_INPUT_OK\n")
        .unwrap()
        .wait()
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        let mut received = Vec::new();
        loop {
            let chunk = output.recv().await.unwrap();
            received.extend_from_slice(&chunk.bytes);
            assert!(
                received.len() < 8192,
                "isolated cat output must remain bounded"
            );
            if received
                .windows(marker.len())
                .any(|window| window == marker)
            {
                break;
            }
        }
    })
    .await
    .expect("input marker must reach the real PTY output");
    drop(output);
    drop(input);
    let resource = fixture
        .state
        .panes
        .write()
        .await
        .remove(&fixture.pane_id)
        .unwrap();
    let crate::pane::PaneResource::Terminal(mut runtime) = resource else {
        panic!("fixture owns exactly one terminal");
    };
    runtime.abort_background_tasks();
    // A Drop implementation prevents moving the session out of the runtime;
    // perform physical cleanup while this exact runtime remains the owner.
    let cleanup = fixture
        .state
        .execution
        .get()
        .unwrap()
        .client
        .run(
            Lane::Io,
            JobCost {
                input_bytes: NATIVE_WORKING_BYTES,
                result_bytes: 4096,
            },
            move |_| {
                let report = runtime.session.shutdown_blocking(Duration::from_secs(15))?;
                drop(runtime);
                Ok::<_, PtyError>(report)
            },
        )
        .await
        .unwrap();
    assert!(cleanup.view().pending.is_empty());
    assert!(cleanup.view().panicked.is_empty());
    assert!(fixture.state.panes.read().await.is_empty());
}
