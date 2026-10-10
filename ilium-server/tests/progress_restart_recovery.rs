//! Restart recovery for pane progress monitors.
//!
//! A monitor registered on one server must survive that server being killed
//! and started again on the same snapshot: the restarted server restores it,
//! keeps the same monitor ID, and keeps observing the task. The monitor must
//! not be reported as `ObservationStopped` (the "waiting forever" state this
//! guards against). The restart is simulated by aborting the server task
//! in-process; the snapshot file is the only state carried across.

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::{expect_event, read_initial_state};
use ilium_core::{ProgressMonitorHealth, ProgressTaskStatus};
use ilium_ipc::{ClientRequest, ServerEvent};
use ilium_server::config::{
    DetectionConfig, HttpApiConfig, NotificationsConfig, SessionRecoveryConfig,
};
use ilium_server::{run, NoopSoundPlayer, ServerOptions};
use ilium_transport::{Liveness, SessionEndpoint, SessionStream};

const EVENT_TIMEOUT: Duration = Duration::from_secs(10);

/// Builds the same options a real launch would use, pointed at fixed paths so
/// a second server instance can start on the state the first one saved.
fn server_options(
    session_name: &str,
    socket_path: PathBuf,
    snapshot_path: PathBuf,
    launch_directory: &Path,
) -> ServerOptions {
    ServerOptions {
        session_name: session_name.to_string(),
        socket_path,
        snapshot_path,
        ready_log_metadata: None,
        session_cwd: ilium_platform::paths::canonicalize(launch_directory)
            .expect("canonical launch directory"),
        home_dir: ilium_platform::paths::canonicalize(launch_directory)
            .expect("canonical home directory"),
        detection_config: DetectionConfig::default(),
        notifications_config: NotificationsConfig {
            enabled: false,
            suppress_redundant_task_outcomes: false,
            ..NotificationsConfig::default()
        },
        sound_settings: ilium_sound::SoundSettings::default(),
        sound_config_path: None,
        sound_player: std::sync::Arc::new(NoopSoundPlayer),
        custom_signatures: Vec::new(),
        session_recovery: SessionRecoveryConfig::RestoreAutomatically,
        agent_debug_menu_enabled: false,
        http_api: HttpApiConfig { port: 0 },
        progress_monitor_enabled: true,
        session_backups_enabled: false,
    }
}

async fn wait_for_liveness(socket_path: &Path, expected: Liveness, within: Duration) -> bool {
    let deadline = Instant::now() + within;
    loop {
        if SessionEndpoint::from_path(socket_path).probe_liveness() == expected {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn start_server(options: ServerOptions) -> tokio::task::JoinHandle<()> {
    let socket_path = options.socket_path.clone();
    let task = tokio::spawn(async move {
        if let Err(error) = run(options).await {
            eprintln!("ilium-server run() exited with an error: {error}");
        }
    });
    let bound = wait_for_liveness(&socket_path, Liveness::Live, Duration::from_secs(5)).await;
    assert!(bound, "server did not bind its socket in time");
    task
}

/// The server sends the attach snapshot only after an `Attach` request, so a
/// freshly connected client must send one before reading the initial state.
async fn attach_and_read_initial_state(
    client: &mut SessionStream,
    session_name: &str,
    timeout: Duration,
) -> (ilium_core::Tree, Vec<ServerEvent>) {
    ilium_ipc::write_frame(
        client,
        &ClientRequest::Attach {
            session: session_name.to_string(),
        },
    )
    .await
    .expect("send the attach request");
    read_initial_state(client, timeout).await
}

/// The only pane in a tree. A fresh session has no pane until one is created
/// (the launch project gets its default group only with the first pane).
fn sole_pane(tree: &ilium_core::Tree) -> ilium_core::NodeId {
    let panes = tree.pane_ids_in_tree_order();
    assert_eq!(panes.len(), 1, "expected exactly one pane, found {panes:?}");
    panes[0]
}

/// Creates a plain shell pane in the launch project and returns its ID from
/// the resulting tree snapshot.
async fn create_pane(client: &mut SessionStream) -> ilium_core::NodeId {
    ilium_ipc::write_frame(
        client,
        &ClientRequest::NewPane {
            parent_group: ilium_core::ROOT_ID,
            kind: ilium_ipc::NewPaneKind::PlainShell,
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .expect("send the new-pane request");
    let event = expect_event(client, EVENT_TIMEOUT, |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;
    match event {
        ServerEvent::TreeSnapshot(tree) => sole_pane(&tree),
        other => panic!("new pane snapshot missing: {other:?}"),
    }
}

async fn connect(socket_path: &Path) -> SessionStream {
    SessionEndpoint::from_path(socket_path)
        .connect()
        .await
        .expect("connect to the session socket")
}

async fn wait_for_snapshot_containing(snapshot_path: &Path, needle: &str) {
    let deadline = Instant::now() + EVENT_TIMEOUT;
    loop {
        let saved = std::fs::read_to_string(snapshot_path).unwrap_or_default();
        if saved.contains(needle) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "snapshot never recorded the progress monitor command"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// A probe that prints the report file verbatim. A plain script rather than
/// the fixture binary: that binary is only built for its own test run, so
/// using it here picks up whatever cargo left behind and fails unpredictably.
fn write_probe_script(directory: &Path, report: &Path) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let script = directory.join("progress-probe");
        std::fs::write(&script, format!("#!/bin/sh\ncat '{}'\n", report.display()))
            .expect("write probe script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("make probe script executable");
        script
    }
    #[cfg(windows)]
    {
        let script = directory.join("progress-probe.cmd");
        std::fs::write(&script, format!("@type \"{}\"\r\n", report.display()))
            .expect("write probe script");
        script
    }
}

async fn read_monitor_status(
    client: &mut SessionStream,
    pane_id: ilium_core::NodeId,
) -> ilium_ipc::ProgressMonitorStatus {
    ilium_ipc::write_frame(
        client,
        &ClientRequest::GetPaneProgressMonitorStatus {
            request_id: 1,
            pane_id,
        },
    )
    .await
    .expect("send status request");
    let event = expect_event(client, EVENT_TIMEOUT, |event| {
        matches!(event, ServerEvent::ProgressMonitorStatusReported { .. })
    })
    .await;
    match event {
        ServerEvent::ProgressMonitorStatusReported {
            result: Ok(status), ..
        } => status,
        other => panic!("status request rejected: {other:?}"),
    }
}

#[tokio::test]
async fn monitor_survives_server_restart_and_keeps_observing() {
    let fixtures = tempfile::tempdir().expect("create fixtures");
    let launch_directory = fixtures.path().join("project");
    std::fs::create_dir_all(&launch_directory).expect("create launch directory");
    let probe_report = fixtures.path().join("progress.json");
    std::fs::write(
        &probe_report,
        r#"{"job_id":"restart-recovery","status":"running","percent":40,"message":"Running the restart recovery fixture job - halfway"}"#,
    )
    .expect("write probe report");
    let probe = write_probe_script(fixtures.path(), &probe_report);

    let socket_dir = tempfile::Builder::new()
        .prefix("ir")
        .tempdir_in("/tmp")
        .expect("create short socket directory");
    let socket_path = socket_dir.path().join("restart.sock");
    let snapshot_path = fixtures.path().join("restart.snapshot.json");
    let session_name = "restart-recovery";

    // First server instance: register the monitor on the launch pane.
    let first = start_server(server_options(
        session_name,
        socket_path.clone(),
        snapshot_path.clone(),
        &launch_directory,
    ))
    .await;
    let mut client = connect(&socket_path).await;
    let (tree, _) = attach_and_read_initial_state(&mut client, session_name, EVENT_TIMEOUT).await;
    assert!(
        tree.pane_ids_in_tree_order().is_empty(),
        "a fresh session must start without panes"
    );
    let pane_id = create_pane(&mut client).await;
    let probe_command = probe.display().to_string();
    ilium_ipc::write_frame(
        &mut client,
        &ClientRequest::SetPaneProgressMonitor {
            request_id: 2,
            pane_id,
            command: probe_command.clone(),
            interval_seconds: 1,
        },
    )
    .await
    .expect("send set-monitor request");
    let accepted = expect_event(&mut client, EVENT_TIMEOUT, |event| {
        matches!(event, ServerEvent::ProgressMonitorSetCompleted { .. })
    })
    .await;
    let monitor_id = match accepted {
        ServerEvent::ProgressMonitorSetCompleted {
            result: Ok(accepted),
            ..
        } => accepted.monitor_id,
        other => panic!("monitor registration rejected: {other:?}"),
    };
    wait_for_snapshot_containing(&snapshot_path, &probe_command).await;
    let before = read_monitor_status(&mut client, pane_id).await;
    let before_progress = before
        .progress_monitors
        .into_iter()
        .next()
        .expect("monitor reports progress before restart");
    assert_eq!(before_progress.monitor_id, monitor_id);
    assert_eq!(before_progress.report.status, ProgressTaskStatus::Running);
    drop(client);

    // Simulate a crash: the server task dies, the snapshot file stays.
    first.abort();
    let _ = first.await;
    wait_for_liveness(&socket_path, Liveness::Absent, Duration::from_secs(5)).await;

    // Second server instance starts on the same snapshot, as after a reboot.
    let second = start_server(server_options(
        session_name,
        socket_path.clone(),
        snapshot_path.clone(),
        &launch_directory,
    ))
    .await;
    let mut restarted = connect(&socket_path).await;
    let (restarted_tree, _) =
        attach_and_read_initial_state(&mut restarted, session_name, EVENT_TIMEOUT).await;
    let restored_pane = sole_pane(&restarted_tree);
    assert_eq!(restored_pane, pane_id, "pane identity must survive restart");

    let after = read_monitor_status(&mut restarted, restored_pane).await;
    let after_progress = after
        .progress_monitors
        .into_iter()
        .next()
        .expect("restored monitor must report progress after restart");
    assert_eq!(
        after_progress.monitor_id, monitor_id,
        "restart must keep the monitor, not register a new one"
    );
    assert_eq!(
        after_progress.monitor_health,
        ProgressMonitorHealth::Healthy,
        "restored monitor must keep observing, not be marked failed"
    );
    assert!(
        !after_progress.report.message.contains("ObservationStopped")
            && !after_progress
                .report
                .message
                .to_lowercase()
                .contains("stopped unexpectedly"),
        "restored monitor reported a lost observation: {}",
        after_progress.report.message
    );

    second.abort();
    let _ = second.await;
}
