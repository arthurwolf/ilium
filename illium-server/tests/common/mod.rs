//! Shared test infrastructure for `illium-server`'s integration test
//! binaries (`smoke.rs`, `live_agent_detection.rs`): a real
//! `illium_server::run` bound to a tempdir UDS socket, plus the small
//! polling/frame-reading helpers every one of those tests needs. Lives
//! under `tests/common/` (not a top-level `tests/*.rs` file) specifically
//! so cargo treats it as a shared module, not a third standalone test
//! binary -- see the Rust integration-test convention for this layout.
//!
//! Hermetic by construction: every `TestServer` gets its own tempdir
//! socket/snapshot path, never touches a real `~/.local/share/illium`,
//! and (session tree/detection logic itself aside) has no dependency on a
//! real `claude`/`codex` binary being installed anywhere on the host.
//!
//! `cargo` compiles this module fresh into *each* integration test binary
//! that declares `mod common;` -- one binary per top-level `tests/*.rs`
//! file, per the standard Rust integration-test layout this workspace
//! already follows (`illium-pty`'s and `illium-server`'s own `tests/`
//! directories). Since `smoke.rs` and `live_agent_detection.rs` each only
//! use a subset of what's defined here, whichever helper the *other* file
//! happens not to call trips `dead_code` in that specific binary's build
//! -- expected and harmless (the function is very much used, just not by
//! every consumer), so it's silenced at the module level rather than
//! chasing it per-function.
#![allow(dead_code)]

use std::path::PathBuf;
use std::time::Duration;

use illium_ipc::{read_frame, ServerEvent};
use illium_server::config::{DetectionConfig, NotificationsConfig};
use illium_server::{run, ServerOptions};
use tokio::net::UnixStream;

/// Polls `condition` until it's true or `timeout` elapses, without a fixed
/// sleep -- the server binding its socket (or, for
/// `live_agent_detection.rs`, the real detection loop's next tick) is not
/// instant, and a fixed sleep would be either flaky (too short) or slow
/// (long enough to never flake). Mirrors `illium-pty`'s integration test
/// helper of the same name.
pub async fn wait_until(mut condition: impl FnMut() -> bool, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if condition() {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Reads frames from `stream` until one matches `predicate`, ignoring
/// (but not losing track of, via the timeout) any that don't -- the
/// detection loop or another connection could in principle interleave
/// unrelated broadcasts, so a strict "the very next frame must match"
/// assertion would be a flaky test for the wrong reason.
pub async fn expect_event(
    stream: &mut UnixStream,
    timeout: Duration,
    predicate: impl Fn(&ServerEvent) -> bool,
) -> ServerEvent {
    tokio::time::timeout(timeout, async {
        loop {
            let event: ServerEvent = read_frame(stream).await.expect("read a server event");
            if predicate(&event) {
                return event;
            }
        }
    })
    .await
    .expect("timed out waiting for the expected server event")
}

pub struct TestServer {
    pub socket_path: PathBuf,
    pub server_task: tokio::task::JoinHandle<Result<(), illium_server::error::ServerError>>,
}

impl TestServer {
    /// Starts a real server for `session_name` with the default detection
    /// cadence (README "Poll cadence" -- `working_poll_interval` measured
    /// in seconds), the right choice for every test that only cares about
    /// tree/IPC behavior and never waits on a detection-loop transition.
    pub async fn start(session_name: &str) -> Self {
        Self::start_with_detection_config(session_name, DetectionConfig::default()).await
    }

    /// Same as [`Self::start`], but with a caller-chosen `detection_config`
    /// -- `live_agent_detection.rs` uses this to shrink the poll intervals
    /// so its real end-to-end `Working -> Idle` transition shows up within
    /// a short, deterministic test timeout instead of the real default's
    /// 5s/45s cadence.
    pub async fn start_with_detection_config(
        session_name: &str,
        detection_config: DetectionConfig,
    ) -> Self {
        let dir = tempfile::tempdir().expect("create tempdir");
        let socket_path = dir.path().join(format!("{session_name}.sock"));
        let snapshot_path = dir.path().join(format!("{session_name}.snapshot.json"));

        let options = ServerOptions {
            session_name: session_name.to_string(),
            socket_path: socket_path.clone(),
            snapshot_path,
            detection_config,
            notifications_config: NotificationsConfig::default(),
            custom_signatures: Vec::new(),
        };

        // Leaked, not dropped: `TempDir` deletes its directory on drop, but
        // this test's `illium-server` instance needs the directory (and
        // the socket file inside it) to outlive the whole test, not just
        // this constructor call. The OS reclaims the tempdir on process
        // exit either way.
        std::mem::forget(dir);

        let server_task = tokio::spawn(run(options));

        let bound = wait_until(|| socket_path.exists(), Duration::from_secs(5)).await;
        assert!(bound, "server did not bind its socket in time");

        Self {
            socket_path,
            server_task,
        }
    }

    pub async fn connect(&self) -> UnixStream {
        UnixStream::connect(&self.socket_path)
            .await
            .expect("connect to the session socket")
    }
}
