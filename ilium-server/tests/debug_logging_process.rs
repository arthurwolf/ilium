//! Process-boundary proof for the detached server's durable diagnostics.

use std::process::{Child, Command, Stdio};
use std::time::Duration;

use ilium_core::ROOT_ID;
use ilium_ipc::{read_frame, write_frame, ClientRequest, ServerEvent};
use ilium_transport::{Liveness, SessionEndpoint, SessionStream};

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// Waits until a server is actually listening on `path`.
///
/// Liveness, not a filesystem check: a Windows session endpoint is a named
/// pipe with no file to appear, so waiting for one would never return there.
async fn wait_for_socket(path: &std::path::Path) {
    let endpoint = SessionEndpoint::from_path(path);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while endpoint.probe_liveness() != Liveness::Live {
        assert!(
            tokio::time::Instant::now() < deadline,
            "server never began listening on {}",
            path.display()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn read_until(
    stream: &mut SessionStream,
    predicate: impl Fn(&ServerEvent) -> bool,
) -> ServerEvent {
    tokio::time::timeout(Duration::from_secs(5), async {
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

#[tokio::test]
async fn detached_server_process_logs_major_ipc_actions_and_errors() {
    let directory = tempfile::tempdir().expect("tempdir");
    let project_directory = directory.path().join("project");
    let config_home = directory.path().join("config-home");
    let config_directory = config_home.join("ilium");
    let socket_path = directory.path().join("debug-session.sock");
    let snapshot_path = directory.path().join("debug-session.json");
    let log_directory = directory.path().join("logs");
    let log_path = log_directory.join("log-2026-07-19_20-00-00.000.txt");
    std::fs::create_dir_all(&project_directory).expect("project directory");
    std::fs::create_dir_all(&config_directory).expect("config directory");
    std::fs::write(
        config_directory.join("config.toml"),
        "[debug]\nfile_logging_enabled = true\n[notifications]\nenabled = false\n",
    )
    .expect("debug config");

    let child = Command::new(env!("CARGO_BIN_EXE_ilium-server"))
        .args([
            "--session-name",
            "debug-session",
            "--socket-path",
            socket_path.to_str().expect("socket path"),
            "--snapshot-path",
            snapshot_path.to_str().expect("snapshot path"),
            "--session-cwd",
            project_directory.to_str().expect("project path"),
            "--log-path",
            log_path.to_str().expect("log path"),
        ])
        .env("XDG_CONFIG_HOME", &config_home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn ilium-server process");
    let mut child = ChildGuard(child);
    wait_for_socket(&socket_path).await;

    let mut client = SessionEndpoint::from_path(&socket_path)
        .connect()
        .await
        .expect("connect primary client");
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "debug-session".to_owned(),
        },
    )
    .await
    .expect("attach primary client");
    read_until(&mut client, |event| {
        matches!(event, ServerEvent::InitialStateSyncComplete)
    })
    .await;

    write_frame(
        &mut client,
        &ClientRequest::NewGroup {
            parent_group: ROOT_ID,
            name: "logged group".to_owned(),
        },
    )
    .await
    .expect("send major structural action");
    read_until(&mut client, |event| {
        matches!(event, ServerEvent::TreeSnapshot(_))
    })
    .await;

    let mut rejected_client = SessionEndpoint::from_path(&socket_path)
        .connect()
        .await
        .expect("connect rejected client");
    write_frame(
        &mut rejected_client,
        &ClientRequest::Attach {
            session: "wrong-session".to_owned(),
        },
    )
    .await
    .expect("send rejected attach");
    let rejection = read_until(&mut rejected_client, |event| {
        matches!(event, ServerEvent::Error { .. })
    })
    .await;
    assert!(
        matches!(rejection, ServerEvent::Error { message } if message.contains("wrong-session"))
    );

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("request shutdown");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let exit_status = loop {
        if let Some(exit_status) = child.0.try_wait().expect("server status") {
            break exit_status;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "server did not stop after KillSession"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert!(exit_status.success(), "server exited with {exit_status}");

    let log = std::fs::read_to_string(&log_path).expect("server diagnostic file");
    assert!(log.contains("ilium-server starting"));
    assert!(log.contains("detached session server listening"));
    assert!(log.contains("request_name=\"attach\""));
    assert!(log.contains("request_name=\"new_group\""));
    assert!(log.contains("request_name=\"kill_session\""));
    assert!(log.contains("request failed"));
    assert!(log.contains("wrong-session"));
    assert_owner_only(&log_directory, &log_path);
}

/// Owner-only modes are a Unix concept; on Windows the same protection comes
/// from the log directory's inherited profile ACL, which exposes no mode bits
/// to compare. See `ilium_platform::secure_fs`.
#[cfg(unix)]
fn assert_owner_only(log_directory: &std::path::Path, log_path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;

    assert_eq!(
        std::fs::metadata(log_directory)
            .expect("log directory metadata")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(log_path)
            .expect("log metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[cfg(not(unix))]
fn assert_owner_only(log_directory: &std::path::Path, log_path: &std::path::Path) {
    assert!(log_directory.is_dir());
    assert!(log_path.is_file());
}
