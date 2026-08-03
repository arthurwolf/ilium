//! Shared test infrastructure for `ilium-server`'s integration test
//! binaries (`smoke.rs`, `live_agent_detection.rs`, `shell_command_titles.rs`,
//! `debug_logging_process.rs`): a real
//! `ilium_server::run` bound to a tempdir UDS socket, plus the small
//! polling/frame-reading helpers every one of those tests needs. Lives
//! under `tests/common/` (not a top-level `tests/*.rs` file) specifically
//! so cargo treats it as a shared module, not a third standalone test
//! binary -- see the Rust integration-test convention for this layout.
//!
//! Hermetic by construction: every `TestServer` gets its own tempdir
//! socket/snapshot path, never touches a real `~/.local/share/ilium`,
//! and (session tree/detection logic itself aside) has no dependency on a
//! real `claude`/`codex` binary being installed anywhere on the host.
//!
//! `cargo` compiles this module fresh into *each* integration test binary
//! that declares `mod common;` -- one binary per top-level `tests/*.rs`
//! file, per the standard Rust integration-test layout this workspace
//! already follows (`ilium-pty`'s and `ilium-server`'s own `tests/`
//! directories). Since `smoke.rs` and `live_agent_detection.rs` each only
//! use a subset of what's defined here, whichever helper the *other* file
//! happens not to call trips `dead_code` in that specific binary's build
//! -- expected and harmless (the function is very much used, just not by
//! every consumer), so it's silenced at the module level rather than
//! chasing it per-function.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ilium_ipc::{read_frame, ServerEvent};
use ilium_server::config::{DetectionConfig, NotificationsConfig};
use ilium_server::{run, NoopSoundPlayer, ServerOptions, SoundPlayer};
use ilium_transport::{Liveness, SessionEndpoint, SessionStream};

/// Polls `condition` until it's true or `timeout` elapses, without a fixed
/// sleep -- the server binding its socket (or, for
/// `live_agent_detection.rs`, the real detection loop's next tick) is not
/// instant, and a fixed sleep would be either flaky (too short) or slow
/// (long enough to never flake). Mirrors `ilium-pty`'s integration test
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
    stream: &mut SessionStream,
    timeout: Duration,
    predicate: impl Fn(&ServerEvent) -> bool,
) -> ServerEvent {
    // Every event seen while waiting, kept outside the future so a timeout can
    // still report them. "No matching event arrived" says nothing on its own;
    // what the server *did* send usually names the cause immediately.
    let observed = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let observed_in_loop = std::sync::Arc::clone(&observed);
    tokio::time::timeout(timeout, async move {
        loop {
            let event: ServerEvent = read_frame(stream).await.expect("read a server event");
            if let Ok(mut seen) = observed_in_loop.lock() {
                // Debug entries carry the server's own reasoning -- which agent
                // it identified and why -- so they are kept at length while
                // bulk traffic like `ScreenUpdate` is clipped to a label.
                let rendered = format!("{event:?}");
                let budget = if matches!(event, ServerEvent::PaneDebugEntryAppended { .. }) {
                    1200
                } else {
                    160
                };
                seen.push(rendered.chars().take(budget).collect());
            }
            if predicate(&event) {
                return event;
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        let seen = observed
            .lock()
            .map(|seen| {
                if seen.is_empty() {
                    "(none)".to_string()
                } else {
                    seen.join("\n  ")
                }
            })
            .unwrap_or_else(|_| "(unavailable)".to_string());
        panic!(
            "timed out waiting for the expected server event\nevents received:\n  {seen}\nwhat the detector sees:\n{}\nlive process table:\n{}",
            detector_view(),
            process_table_snapshot()
        )
    })
}

/// What the *detector* sees, as opposed to what `ps` reports.
///
/// The two can disagree, and only the first one matters: detection matches on
/// `sysinfo`'s view of a process's name and command line, which on macOS is
/// assembled differently from `ps` output and can be incomplete for a process
/// that has just started. When a pane running a recognisable agent is never
/// classified, the question is which names the detector had to match against
/// and what it concluded from them -- neither of which a process table shows.
fn detector_view() -> String {
    let mut system = sysinfo::System::new();
    ilium_detect::refresh(&mut system);
    let mut report = String::new();
    for (pid, process) in system.processes() {
        let arguments = process
            .cmd()
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ");
        let name = process.name().to_string_lossy().into_owned();
        if !name.contains("codex")
            && !name.contains("claude")
            && !arguments.contains("codex")
            && !arguments.contains("claude")
        {
            continue;
        }
        report.push_str(&format!(
            "  pid={pid} name={name:?} cmd={arguments:?}\n    candidates: {:?}\n    identified: {:?}\n",
            ilium_detect::identifying_process_names(process),
            ilium_detect::identify_agent(&system, *pid),
        ));
    }
    if report.is_empty() {
        report.push_str("  (the detector sees no process mentioning an agent)\n");
    }
    report
}

/// What the machine's process table looked like when a wait gave up.
///
/// Agent detection matches on process names and command lines, and those differ
/// per platform in ways that are invisible from a bare timeout: a shebang
/// script is reported under its own name on Linux but under its interpreter's
/// on macOS. Printing the table turns "no event arrived" into evidence about
/// why, without another round trip through CI.
fn process_table_snapshot() -> String {
    let output = if cfg!(windows) {
        std::process::Command::new("wmic")
            .args([
                "process",
                "get",
                "ProcessId,ParentProcessId,Name,CommandLine",
            ])
            .output()
    } else {
        std::process::Command::new("ps")
            .args(["-Ao", "pid,ppid,comm,args"])
            .output()
    };
    match output {
        Ok(output) => String::from_utf8_lossy(&output.stdout)
            .lines()
            // Only lines plausibly related to a pane keep this readable; a CI
            // runner's full table is hundreds of unrelated processes.
            .filter(|line| {
                ["codex", "claude", "sh", "cmd.exe", "ilium"]
                    .iter()
                    .any(|needle| line.contains(needle))
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Err(error) => format!("could not read the process table: {error}"),
    }
}

pub struct TestServer {
    pub socket_path: PathBuf,
    /// Keeps the short socket directory alive for this server's lifetime;
    /// dropping it removes the directory. See `short_socket_dir`.
    _socket_dir: tempfile::TempDir,
    pub snapshot_path: PathBuf,
    pub project_cwd: PathBuf,
    pub home_dir: PathBuf,
    pub server_task: tokio::task::JoinHandle<Result<(), ilium_server::error::ServerError>>,
    /// Owns the socket/snapshot directory for as long as this `TestServer`
    /// (and thus the running server) is alive. `TempDir` deletes its
    /// directory on drop, so keeping it as a field -- instead of
    /// `std::mem::forget`-ing it -- ties its lifetime to the struct's own
    /// RAII scope: the directory is cleaned up the moment the test that
    /// owns this `TestServer` finishes, rather than leaking one tempdir on
    /// disk per test for the remaining life of the whole test-binary
    /// process. Never read directly, hence the leading underscore.
    _tempdir: tempfile::TempDir,
}

impl Drop for TestServer {
    /// Aborts the spawned server task unconditionally, including when it
    /// already finished on its own (`JoinHandle::abort` is a harmless no-op
    /// on a completed task). Without this, a test that panics on an
    /// assertion or a timed-out `expect_event` before it ever reaches its
    /// own `KillSession`/shutdown-join code leaves this `TestServer` value
    /// dropped mid-unwind -- and dropping a `JoinHandle` alone only detaches
    /// it, it does not cancel the task. The spawned server (its PTYs, UDS
    /// listener, and detection loop) would otherwise keep running forever
    /// in the background for the rest of this test binary's process.
    fn drop(&mut self) {
        self.server_task.abort();
    }
}

/// A short-lived directory directly under `/tmp`, short enough that a session
/// socket bound inside it fits `sockaddr_un` on every platform.
///
/// Windows endpoints are named pipes with no filesystem presence, so the
/// directory is merely unused there rather than wrong.
fn short_socket_dir() -> tempfile::TempDir {
    let parent = if cfg!(windows) {
        std::env::temp_dir()
    } else {
        std::path::PathBuf::from("/tmp")
    };
    tempfile::Builder::new()
        .prefix("is")
        .tempdir_in(parent)
        .expect("create short socket directory")
}

impl TestServer {
    /// Starts a real server for `session_name` with the default detection
    /// cadence (ARCHITECTURE.md "Poll cadence" -- `working_poll_interval` measured
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
        Self::start_with_sound_player_and_agent_debug(
            session_name,
            detection_config,
            ilium_sound::SoundSettings::default(),
            Arc::new(NoopSoundPlayer),
            false,
        )
        .await
    }

    /// Starts the real server with durable semantic agent history enabled.
    /// Session lifecycle integration tests use this to inspect the same
    /// typed evidence the TUI and `/tmp` bridge consume.
    pub async fn start_with_agent_debug(
        session_name: &str,
        detection_config: DetectionConfig,
    ) -> Self {
        Self::start_with_sound_player_and_agent_debug(
            session_name,
            detection_config,
            ilium_sound::SoundSettings::default(),
            Arc::new(NoopSoundPlayer),
            true,
        )
        .await
    }

    /// Starts a server with an injected player, allowing the live detection
    /// integration test to prove dispatch without producing real audio.
    pub async fn start_with_sound_player(
        session_name: &str,
        detection_config: DetectionConfig,
        sound_settings: ilium_sound::SoundSettings,
        sound_player: Arc<dyn SoundPlayer>,
    ) -> Self {
        Self::start_with_sound_player_and_agent_debug(
            session_name,
            detection_config,
            sound_settings,
            sound_player,
            false,
        )
        .await
    }

    async fn start_with_sound_player_and_agent_debug(
        session_name: &str,
        detection_config: DetectionConfig,
        sound_settings: ilium_sound::SoundSettings,
        sound_player: Arc<dyn SoundPlayer>,
        agent_debug_menu_enabled: bool,
    ) -> Self {
        let dir = tempfile::tempdir().expect("create tempdir");
        let snapshot_path = dir.path().join(format!("{session_name}.snapshot.json"));
        // The socket deliberately does not live under `dir`. A Unix socket
        // address is a fixed-size `sockaddr_un` -- 104 bytes on macOS -- and a
        // platform temporary directory can spend most of that on its own:
        // macOS hands out paths like
        // `/var/folders/df/djsxfhc17x95674wsm_g8s980000gn/T/.tmpR6qv4y/`, so
        // binding inside one fails outright. A short prefix directly under
        // `/tmp` stays unique per test while leaving room for the name.
        let socket_dir = short_socket_dir();
        let socket_path = socket_dir.path().join(format!("{session_name}.sock"));

        let options = ServerOptions {
            session_name: session_name.to_string(),
            socket_path: socket_path.clone(),
            snapshot_path: snapshot_path.clone(),
            ready_log_metadata: None,
            session_cwd: dir.path().to_path_buf(),
            home_dir: dir.path().to_path_buf(),
            detection_config,
            notifications_config: NotificationsConfig { enabled: false },
            sound_settings,
            sound_config_path: None,
            sound_player,
            custom_signatures: Vec::new(),
            session_recovery: ilium_server::config::SessionRecoveryConfig::RestoreAutomatically,
            agent_debug_menu_enabled,
            http_api: ilium_server::config::HttpApiConfig { port: 0 },
        };

        // Wrapped so a server that gives up *after* binding says so. Nothing
        // inspects this task's result until the test ends, so an error here
        // otherwise surfaces only as a client that connects successfully and
        // then receives nothing at all -- which is exactly the shape the macOS
        // detection failures take.
        let server_task = tokio::spawn(async move {
            let result = run(options).await;
            if let Err(error) = &result {
                eprintln!("ilium-server run() exited with an error: {error}");
            }
            result
        });

        // Liveness, not a filesystem check: a Windows session endpoint is a
        // named pipe with no file to appear, so `Path::exists` would wait
        // forever there.
        let bound = wait_until(
            || SessionEndpoint::from_path(&socket_path).probe_liveness() == Liveness::Live,
            Duration::from_secs(5),
        )
        .await;
        if !bound {
            // Bail out before the task's `JoinHandle` is ever stored
            // anywhere reachable -- without this, a bind timeout would
            // leave the spawned server task (PTYs, detection loop, socket
            // listener and all) running forever in the background with no
            // handle left to abort it, since the handle about to be
            // dropped by this `assert!` unwinding does not cancel the task
            // it points to.
            server_task.abort();
        }
        assert!(bound, "server did not bind its socket in time");

        Self {
            socket_path,
            _socket_dir: socket_dir,
            snapshot_path,
            project_cwd: dir.path().to_path_buf(),
            home_dir: dir.path().to_path_buf(),
            server_task,
            // Held for the struct's lifetime (see the field doc) instead
            // of being forgotten -- the directory it owns is exactly the
            // one `socket_path`/`snapshot_path`/`session_cwd` above point
            // into, so it must outlive them, and RAII already guarantees
            // that for free.
            _tempdir: dir,
        }
    }

    pub async fn connect(&self) -> SessionStream {
        SessionEndpoint::from_path(&self.socket_path)
            .connect()
            .await
            .expect("connect to the session socket")
    }
}
