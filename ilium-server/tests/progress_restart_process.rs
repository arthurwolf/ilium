//! Process-boundary restart recovery for pane progress monitors.
//!
//! Unlike `progress_restart_recovery.rs`, which aborts a server task
//! in-process, this test runs the real `ilium-server` binary, kills it with
//! SIGKILL, and starts a second process on the same snapshot. The monitor
//! registered on the first process must come back with the same monitor ID,
//! stay `Healthy`, and keep reporting the probe's job rather than
//! `ObservationStopped`. Every path lives under one isolated temporary
//! directory, so the test never touches a live session or the user's config.

// The sole test and its shell probe already require Unix; keep their support
// code at the same boundary instead of compiling unused helpers on Windows.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use ilium_core::{ProgressMonitorHealth, ProgressTaskStatus, ROOT_ID};
use ilium_ipc::{
    read_frame, write_frame, ClientRequest, NewPaneKind, NewPaneWorkingDirectory, ServerEvent,
};
use ilium_transport::{Liveness, SessionEndpoint, SessionStream};

const EVENT_TIMEOUT: Duration = Duration::from_secs(10);

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

struct ServerPaths {
    project_directory: PathBuf,
    config_home: PathBuf,
    socket_path: PathBuf,
    snapshot_path: PathBuf,
    log_path: PathBuf,
}

fn spawn_server(paths: &ServerPaths) -> ChildGuard {
    let server_binary = std::env::var("CARGO_BIN_EXE_ilium-server")
        .expect("Cargo provides the ilium-server binary path to integration tests");
    let config_directory = paths.config_home.join("ilium");
    let child = Command::new(server_binary)
        .args([
            "--session-name",
            "progress-restart-process",
            "--socket-path",
            paths.socket_path.to_str().expect("socket path"),
            "--snapshot-path",
            paths.snapshot_path.to_str().expect("snapshot path"),
            "--session-cwd",
            paths.project_directory.to_str().expect("project path"),
            "--log-path",
            paths.log_path.to_str().expect("log path"),
        ])
        .env("XDG_CONFIG_HOME", &paths.config_home)
        .env(ilium_server::paths::CONFIG_DIR_ENV, &config_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn ilium-server process");
    ChildGuard(child)
}

/// Waits until a server accepts connections on `path`. Liveness rather than a
/// file check, so the same wait works for Windows named pipes.
async fn wait_for_liveness(path: &Path, expected: Liveness) {
    let endpoint = SessionEndpoint::from_path(path);
    let deadline = Instant::now() + EVENT_TIMEOUT;
    while endpoint.probe_liveness() != expected {
        assert!(
            Instant::now() < deadline,
            "socket {} never reached {expected:?}",
            path.display()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_for_not_live(path: &Path) {
    let endpoint = SessionEndpoint::from_path(path);
    let deadline = Instant::now() + EVENT_TIMEOUT;
    while endpoint.probe_liveness() == Liveness::Live {
        assert!(
            Instant::now() < deadline,
            "socket {} still accepts connections after kill",
            path.display()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn connect_and_attach(socket_path: &Path) -> SessionStream {
    let mut client = SessionEndpoint::from_path(socket_path)
        .connect()
        .await
        .expect("connect to the session socket");
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "progress-restart-process".to_owned(),
            hello: Default::default(),
        },
    )
    .await
    .expect("send attach request");
    read_until(&mut client, |event| {
        matches!(event, ServerEvent::InitialStateSyncComplete)
    })
    .await;
    client
}

async fn read_until(
    stream: &mut SessionStream,
    predicate: impl Fn(&ServerEvent) -> bool,
) -> ServerEvent {
    tokio::time::timeout(EVENT_TIMEOUT, async {
        loop {
            let event = read_frame(stream).await.expect("read server event");
            if predicate(&event) {
                return event;
            }
        }
    })
    .await
    .expect("expected server event before timeout")
}

async fn read_monitor_status(
    client: &mut SessionStream,
    pane_id: ilium_core::NodeId,
) -> ilium_ipc::ProgressMonitorStatus {
    write_frame(
        client,
        &ClientRequest::GetPaneProgressMonitorStatus {
            request_id: 1,
            pane_id,
        },
    )
    .await
    .expect("send status request");
    match read_until(client, |event| {
        matches!(event, ServerEvent::ProgressMonitorStatusReported { .. })
    })
    .await
    {
        ServerEvent::ProgressMonitorStatusReported {
            result: Ok(status), ..
        } => status,
        other => panic!("status request rejected: {other:?}"),
    }
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

#[cfg(unix)]
fn write_probe_script(directory: &Path, report: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let script = directory.join("progress-probe");
    std::fs::write(&script, format!("#!/bin/sh\ncat '{}'\n", report.display()))
        .expect("write probe script");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
        .expect("make probe script executable");
    script
}

#[cfg(unix)]
#[tokio::test]
async fn monitor_survives_a_killed_server_process_and_keeps_observing() {
    let fixtures = tempfile::tempdir().expect("create fixtures");
    let paths = ServerPaths {
        project_directory: fixtures.path().join("project"),
        config_home: fixtures.path().join("config-home"),
        socket_path: fixtures.path().join("progress-restart.sock"),
        snapshot_path: fixtures.path().join("progress-restart.snapshot.json"),
        log_path: fixtures.path().join("logs").join("server.txt"),
    };
    std::fs::create_dir_all(&paths.project_directory).expect("project directory");
    std::fs::create_dir_all(paths.config_home.join("ilium")).expect("config directory");
    std::fs::create_dir_all(paths.log_path.parent().expect("log directory"))
        .expect("log directory");
    std::fs::write(
        paths.config_home.join("ilium").join("config.toml"),
        "[notifications]\nenabled = false\n",
    )
    .expect("isolated config");
    let probe_report = fixtures.path().join("progress.json");
    std::fs::write(
        &probe_report,
        r#"{"job_id":"restart-process","status":"running","percent":40,"message":"Running the restart process fixture job - halfway"}"#,
    )
    .expect("write probe report");
    let probe = write_probe_script(fixtures.path(), &probe_report);

    // First server process: register the monitor on a new pane.
    let mut first = spawn_server(&paths);
    wait_for_liveness(&paths.socket_path, Liveness::Live).await;
    let mut client = connect_and_attach(&paths.socket_path).await;
    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            request_id: 1,
            parent_group: ROOT_ID,
            kind: NewPaneKind::PlainShell,
            working_directory: NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .expect("send new-pane request");
    let pane_id = match read_until(&mut client, |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await
    {
        ServerEvent::TreeSnapshot(tree) => *tree
            .pane_ids_in_tree_order()
            .first()
            .expect("new pane present in tree"),
        other => panic!("new pane snapshot missing: {other:?}"),
    };

    let probe_command = probe.display().to_string();
    write_frame(
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
    let monitor_id = match read_until(&mut client, |event| {
        matches!(event, ServerEvent::ProgressMonitorSetCompleted { .. })
    })
    .await
    {
        ServerEvent::ProgressMonitorSetCompleted {
            result: Ok(accepted),
            ..
        } => accepted.monitor_id,
        other => panic!("monitor registration rejected: {other:?}"),
    };
    wait_for_snapshot_containing(&paths.snapshot_path, &probe_command).await;
    let before = read_monitor_status(&mut client, pane_id).await;
    let before_progress = before
        .progress_monitors
        .into_iter()
        .next()
        .expect("monitor reports progress before the kill");
    assert_eq!(before_progress.monitor_id, monitor_id);
    assert_eq!(before_progress.report.status, ProgressTaskStatus::Running);
    drop(client);

    // SIGKILL the real process: no shutdown hook runs, the snapshot survives.
    first.0.kill().expect("kill first server process");
    first.0.wait().expect("reap first server process");
    // SIGKILL leaves the socket file behind, so the endpoint reads as a stale
    // listener rather than absent. Either way nothing is live.
    wait_for_not_live(&paths.socket_path).await;

    // Second server process on the same snapshot, as after a crash or reboot.
    let _second = spawn_server(&paths);
    wait_for_liveness(&paths.socket_path, Liveness::Live).await;
    let mut restarted = connect_and_attach(&paths.socket_path).await;
    let after = read_monitor_status(&mut restarted, pane_id).await;
    let after_progress = after
        .progress_monitors
        .into_iter()
        .next()
        .expect("restored monitor must report progress after the restart");
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
        !after_progress
            .report
            .message
            .to_lowercase()
            .contains("stopped unexpectedly"),
        "restored monitor reported a lost observation: {}",
        after_progress.report.message
    );
}
