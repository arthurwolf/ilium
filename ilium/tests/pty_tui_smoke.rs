//! PTY-driven smoke test for the actual `ilium` binary: the same
//! technique `ilium-pty/tests/pty_integration.rs` uses (a real pty via
//! `ilium_pty::PtySession`, not `std::process::Command` with inherited
//! stdio) applied one layer up, against the real CLI + TUI instead of a
//! trivial `echo`/`cat`.
//!
//! Two phases:
//! 1. `ilium new-pane -- cat` -- a non-interactive, non-attaching
//!    subcommand (plain `std::process::Command`, no pty needed: it never
//!    enters raw mode) that spawns this test's session's server and adds
//!    one terminal pane to it, then exits. This is the "create state
//!    without a TUI" half.
//! 2. `ilium --restart-server --cwd <dir>` -- the actual attaching form, explicitly
//!    replacing phase 1's server -- run inside a real pty at a fixed size.
//!    Its first rendered frame is
//!    asserted to contain structural chrome (the sidebar title from
//!    `ilium_client::tree_ui::sidebar_title`) and the pane created in
//!    phase 1 (named after its command line -- see `IDLE_PANE_LABEL`, which
//!    differs per platform -- per `TerminalOrigin::default_pane_name`). A scripted leader-key + help
//!    keystroke (`Ctrl+B` then `?`, configured through the real
//!    `[keyboard]` table) is then
//!    written to the pty, proving input routing and rendering are both
//!    alive end-to-end: the screen must change to show
//!    `ilium_client::help`'s overlay text.
//!
//! Isolation: `XDG_DATA_HOME`, `XDG_CONFIG_HOME`, and `XDG_RUNTIME_DIR`
//! are all pointed at one tempdir for every spawned process (the real
//! The project-session resolver prefers `XDG_RUNTIME_DIR` for the
//! session socket when it's set -- see `ilium/src/session.rs`'s
//! `socket_dir`, which must match that formula for this test, or indeed
//! the real CLI, to ever find its own spawned server), so nothing here
//! touches a real `~/.local/share/ilium` or a real running session. A
//! `.ilium/config.yaml` with a pre-set project name is written into the
//! session's cwd before attaching, so the client's background project-name
//! inference worker (which would otherwise call out to `ilium-kilo-gateway`,
//! a real network call this workspace's tests must never make) never
//! fires -- see `ilium_client::project_naming::load_stored_project_name`.
//!
//! Cleanup: the graceful path this workspace already ships, `ilium
//! kill-session <name>`, is reused rather than a raw process kill --
//! `ilium_client::run`'s event loop exits (and the process returns) the
//! moment the server closes every connection on `KillSession` (see
//! `ilium-client/src/lib.rs`'s `run_inner`), so this is both graceful
//! and self-verifying: the pty-attached process's own exit is awaited as
//! part of proving the shutdown path actually works, not just that the
//! one-shot subcommand returned. A force-kill + explicit tempdir cleanup
//! is kept only as a defensive fallback in case that ever hangs, so this
//! test itself cannot hang the suite even if the graceful path regresses.

//! Most of this suite is still Unix-only, and each such test says so at its
//! own `#[cfg(unix)]` rather than the whole file being gated: the fixtures are
//! `/bin/sh` scripts made executable with `chmod`, and one assertion reads the
//! session socket's filesystem type, which on Windows is a named pipe with no
//! filesystem presence at all. The attach-and-render test below carries no
//! such dependency and runs everywhere, so Windows has end-to-end coverage of
//! the part that matters most -- a real PTY, a detached server, IPC, rendering,
//! and keyboard routing. Porting the remainder is tracked in docs/TODO.md.

#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(unix)]
use ilium_client::connection::Connection;
#[cfg(unix)]
use ilium_core::Tree;
#[cfg(unix)]
use ilium_core::{RestructureNode, RestructurePlan};
#[cfg(unix)]
use ilium_ipc::ClientRequest;
#[cfg(unix)]
use ilium_ipc::ServerEvent;
use ilium_pty::{PtyCommand, PtySession};

/// How long phases of this test wait for the server/TUI/help overlay to
/// respond before giving up -- generous relative to `ilium-pty`'s own
/// 5s convention since this test additionally waits on a real spawned
/// `ilium-server` process starting up, not just a trivial child process.
const WAIT_TIMEOUT: Duration = Duration::from_secs(10);

/// Mirrors `ilium_client::tree_ui::RECENTLY_CREATED_PULSE_MS` (crate-private
/// there, so duplicated here rather than imported) -- the total window a
/// freshly created pane's row flashes for.
#[cfg(unix)]
const PULSE_WINDOW: Duration = Duration::from_millis(1400);

/// Session name this test's isolated `ilium` uses throughout -- fixed
/// (not randomized) since every process in this test shares one
/// dedicated tempdir-rooted `XDG_RUNTIME_DIR`/`XDG_DATA_HOME`, so there is
/// no real collision risk with a concurrently running suite or a real
/// user session.
const SESSION_NAME: &str = "default";

/// Project name pre-seeded into `.ilium/config.yaml` -- one word, so it
/// passes `ilium_client::naming::normalize_word_bounded`'s 1-2 word
/// bound unchanged, and distinctive enough that seeing it on screen can
/// only mean this test's own config file was read.
const PROJECT_NAME: &str = "Smoketest";

/// Polls `condition` until it's true or `timeout` elapses, without a
/// fixed sleep -- pty output latency (a real spawned `ilium-server`
/// starting up, a real render loop drawing a frame) is not deterministic
/// under test-runner load. Mirrors `ilium-pty`'s and
/// `ilium-server`'s own integration test helper of the same name.
async fn wait_until(condition: impl FnMut() -> bool, timeout: Duration) -> bool {
    wait_until_polling(condition, timeout, Duration::from_millis(20)).await
}

/// Waits for a condition that is only true for one *transient* frame, such as
/// a mid-animation label position.
///
/// The ordinary 20 ms cadence is wrong for these: the state is not late, it is
/// brief, so a longer timeout cannot recover a sample taken on either side of
/// it. Only a finer interval reduces the chance of stepping over the frame
/// entirely, which is what made the pane-removal assertion fail on a loaded
/// machine while passing when run alone.
#[cfg(unix)]
async fn wait_for_transient_frame(condition: impl FnMut() -> bool, timeout: Duration) -> bool {
    wait_until_polling(condition, timeout, Duration::from_millis(2)).await
}

async fn wait_until_polling(
    mut condition: impl FnMut() -> bool,
    timeout: Duration,
    poll_interval: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if condition() {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(poll_interval).await;
    }
}

/// Every env var override every process this test spawns needs, so all
/// of them -- the one-shot `new-pane`/`kill-session` subcommands and the
/// pty-attached long-lived TUI -- agree on where this session's socket,
/// data, and config live. See this file's module docs for why all three
/// (not just `XDG_DATA_HOME`/`XDG_CONFIG_HOME`) matter.
struct IsolatedXdgDirs {
    data_home: PathBuf,
    config_home: PathBuf,
    /// Where ilium itself reads `config.toml`, named explicitly because the
    /// platform default is only redirectable by `XDG_CONFIG_HOME` on Linux.
    ilium_config_dir: PathBuf,
    /// This test's own debug log root. The default is shared by every project
    /// on the machine, and these tests scan it for the session they just
    /// started -- against a root holding previous runs' directories, that scan
    /// finds somebody else's log.
    debug_log_dir: PathBuf,
    runtime_dir: PathBuf,
    /// Keeps the short runtime directory alive; dropping it removes the
    /// directory. Declared last so it outlives nothing that still needs it.
    _runtime_root: tempfile::TempDir,
}

impl IsolatedXdgDirs {
    fn under(root: &Path) -> std::io::Result<Self> {
        let data_home = root.join("data");
        let config_home = root.join("config");
        // Matches what `XDG_CONFIG_HOME` implies on Linux, so both levers name
        // the same directory and a seeded `config.toml` is found either way.
        let ilium_config_dir = config_home.join("ilium");
        let debug_log_dir = root.join("debug-logs");
        for dir in [&data_home, &config_home, &ilium_config_dir, &debug_log_dir] {
            std::fs::create_dir_all(dir)?;
        }
        // The runtime directory deliberately does *not* live under `root`. A
        // session socket must fit `sockaddr_un` (100 bytes here), and a
        // platform temporary directory can spend most of that budget on its
        // own: macOS hands out paths like
        // `/var/folders/df/djsxfhc17x95674wsm_g8s980000gn/T/.tmpR6qv4y/`, which
        // leaves no room for even an empty readable slug. A short prefix
        // directly under `/tmp` keeps the derived socket short while staying
        // unique per test, which parallel tests require.
        // Unix only: the length budget is a Unix concern. Windows session
        // endpoints are named pipes, which have no `sockaddr_un` limit and no
        // `/tmp` to be short under, so there the ordinary temporary root is
        // both correct and the only one that exists.
        #[cfg(unix)]
        let runtime_root = tempfile::Builder::new().prefix("il").tempdir_in("/tmp")?;
        #[cfg(windows)]
        let runtime_root = tempfile::Builder::new().prefix("il").tempdir()?;
        let runtime_dir = runtime_root.path().to_path_buf();
        Ok(Self {
            data_home,
            config_home,
            ilium_config_dir,
            debug_log_dir,
            runtime_dir,
            _runtime_root: runtime_root,
        })
    }

    /// Deliberately does *not* pin `SHELL`. Shell panes spawn `$SHELL`, so an
    /// inherited one does make pane contents depend on the host -- macOS ships
    /// bash 3.2, which prints a "the default interactive shell is now zsh"
    /// banner into every pane. But one test installs a *fake* shell through its
    /// own `.env("SHELL", ...)`, and these pairs are applied afterwards, so
    /// pinning here would silently overwrite that fixture. Tests that need a
    /// deterministic shell set it themselves.
    fn as_pairs(&self) -> [(&'static str, &Path); 5] {
        [
            ("XDG_DATA_HOME", &self.data_home),
            ("XDG_CONFIG_HOME", &self.config_home),
            ("XDG_RUNTIME_DIR", &self.runtime_dir),
            // `XDG_CONFIG_HOME` only redirects `directories` on Linux. macOS
            // resolves `~/Library/Application Support` and Windows `%APPDATA%`,
            // so without an explicit override these tests would read (and the
            // settings tests write) the real user's configuration there while
            // silently ignoring the one they seeded.
            (
                ilium_platform::paths::CONFIG_DIR_ENV,
                &self.ilium_config_dir,
            ),
            (
                ilium_platform::runtime_dir::DEBUG_LOG_DIR_ENV,
                &self.debug_log_dir,
            ),
        ]
    }
}

/// Best-effort teardown for the detached `ilium-server` this test's `xdg`
/// session owns, run from `Drop` so it still fires when a test panics (a
/// failed `assert!`) partway through -- before reaching its own explicit,
/// awaited `kill-session` call further down. Without this, a panic between
/// spawning the server (phase 1's `new-pane`, or `--restart-server` in phase 2) and
/// that explicit cleanup leaves the server running forever: it's a
/// deliberately detached daemon (see `ilium-server`'s "one process per
/// session" design), not something that dies with this test process.
///
/// Callers must construct this *after* the test's `tempfile::tempdir()`
/// (`temp_root`) is bound, and before anything that can spawn the server --
/// Rust drops locals in reverse declaration order, so this guard's `Drop`
/// runs, and its `kill-session` connects to the still-present UDS socket,
/// before `temp_root`'s own `Drop` deletes the directory the socket lives
/// under. Reordering the two declarations would make cleanup silently
/// no-op (`connect()` against an already-unlinked socket path).
struct KillSessionOnDrop<'a> {
    xdg: &'a IsolatedXdgDirs,
    cwd: PathBuf,
    session_name: &'static str,
    // Flipped by the test itself once its own explicit, awaited
    // `kill-session` call has already succeeded, so the success path (no
    // panic) doesn't fire a redundant, pointless second kill at drop time.
    already_cleaned_up: bool,
}

impl Drop for KillSessionOnDrop<'_> {
    fn drop(&mut self) {
        if self.already_cleaned_up {
            return;
        }
        let mut command = std::process::Command::new(ilium_binary());
        command
            .args(["kill-session", self.session_name])
            .current_dir(&self.cwd);
        for (key, value) in self.xdg.as_pairs() {
            command.env(key, value);
        }
        // Spawn (not `.output()`): `Drop` must never block indefinitely --
        // a hung `kill-session` here would hang the whole panic unwind, not
        // just this one already-failing test. Poll `try_wait` with a bound
        // instead, so this is best-effort but still actually waits long
        // enough, on the still-present socket, for the graceful shutdown to
        // land before giving up.
        if let Ok(mut child) = command.spawn() {
            let deadline = std::time::Instant::now() + WAIT_TIMEOUT;
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if std::time::Instant::now() >= deadline => {
                        // `std::process::Child` has no `kill_on_drop`
                        // (that's a tokio-only feature): simply letting
                        // `child` fall out of scope here would leak the
                        // still-running `kill-session` process as an
                        // orphan (and a zombie once it does exit, since
                        // nothing would ever reap it). Kill it and reap
                        // it explicitly before giving up.
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                    Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                    Err(_) => {
                        // `try_wait` itself failing (rather than reporting
                        // "still running") doesn't mean the child is gone --
                        // best-effort kill+reap here too, so an OS-level
                        // wait error can't silently leak a still-running
                        // `kill-session` process the same way the deadline
                        // branch above already guards against.
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                }
            }
        }
    }
}

/// A pane command that stays alive without producing output until it is
/// killed, and the label the tree shows for it (the server names a
/// command-backed pane after its joined command line -- see
/// `ilium_server::pane::TerminalOrigin::default_pane_name`).
///
/// `cat` with no arguments blocks on stdin forever. Windows has no `cat`;
/// `findstr x` is the nearest stock equivalent that keeps reading stdin
/// instead of exiting immediately, which several alternatives (`more`,
/// `pause`) do not.
#[cfg(unix)]
const IDLE_PANE_ARGUMENTS: &[&str] = &["cat"];
#[cfg(unix)]
const IDLE_PANE_LABEL: &str = "cat";
#[cfg(windows)]
const IDLE_PANE_ARGUMENTS: &[&str] = &["findstr", "x"];
#[cfg(windows)]
const IDLE_PANE_LABEL: &str = "findstr x";

/// The full argument list for the one-shot subcommand that creates one idle
/// pane, so the command and the label asserted against it cannot drift apart.
fn new_idle_pane_arguments() -> Vec<&'static str> {
    let mut arguments = vec!["new-pane", "--"];
    arguments.extend_from_slice(IDLE_PANE_ARGUMENTS);
    arguments
}

/// Resolves the `ilium` binary under test. Cargo's adjacent test binary is
/// the default, while `ILIUM_PTY_SMOKE_BINARY` lets the same isolated flow
/// prove a release-installed client and its sibling server after deployment.
fn ilium_binary() -> String {
    std::env::var_os("ILIUM_PTY_SMOKE_BINARY")
        .map(|binary_path| binary_path.to_string_lossy().into_owned())
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_ilium").to_string())
}

/// How long to wait for the detection loop to classify a freshly spawned
/// agent process.
///
/// Longer than [`WAIT_TIMEOUT`], which is a budget for the client to repaint
/// after an input it already received. Detection has to wait for the pane's
/// process to start, produce recognisable output, and then be caught by a
/// poll whose interval the test configures -- a chain of real process
/// scheduling that a loaded CI runner stretches well past a repaint. The
/// fixtures involved run until killed, so their state does not expire while
/// this waits: a longer bound cannot mask a regression here, it can only stop
/// reporting one that isn't there.
#[cfg(unix)]
const DETECTION_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a one-shot `ilium` subcommand gets to finish.
///
/// Deliberately longer than [`WAIT_TIMEOUT`], and deliberately longer than
/// the CLI's own worst case: `new-pane` bounds each of its three stages
/// separately (starting the detached server, receiving the baseline tree, and
/// confirming the pane), so it can legitimately take the sum of them before
/// reporting its own precise error. A harness bound shorter than that can only
/// ever replace that error with "did not exit in time", which says strictly
/// less. This is the patience to *observe* a failure, not an expectation about
/// how long success takes.
const ONE_SHOT_TIMEOUT: Duration = Duration::from_secs(30);

/// Numbers each one-shot subcommand's capture file, so concurrent tests
/// sharing a debug-log root cannot overwrite each other's evidence.
static ONE_SHOT_SEQUENCE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Runs a one-shot `ilium` subcommand (`new-pane`/`kill-session`) to
/// completion with the isolated XDG env applied, returning what it printed
/// (stdout and stderr merged, which every caller treats the same way) for
/// assertions. Panics (failing the test) if it doesn't exit within
/// [`WAIT_TIMEOUT`] -- a hang here means either the CLI or the detached
/// server it starts is broken, which is exactly what this test exists to
/// catch.
///
/// Output is captured to a file rather than through `Command::output()`
/// because a timeout drops that future along with everything the subcommand
/// had already written. What it printed before hanging is usually the whole
/// answer, so on timeout it is reported along with [`session_state_report`].
async fn run_one_shot(xdg: &IsolatedXdgDirs, cwd: &Path, args: &[&str]) -> std::process::Output {
    let sequence = ONE_SHOT_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let capture_path = xdg.debug_log_dir.join(format!("one-shot-{sequence}.log"));
    let capture = std::fs::File::create(&capture_path)
        .unwrap_or_else(|error| panic!("create one-shot capture {capture_path:?}: {error}"));
    let capture_error = capture
        .try_clone()
        .expect("share the one-shot capture between stdout and stderr");

    let mut command = tokio::process::Command::new(ilium_binary());
    command
        .args(args)
        .current_dir(cwd)
        .stdout(std::process::Stdio::from(capture))
        .stderr(std::process::Stdio::from(capture_error));
    for (key, value) in xdg.as_pairs() {
        command.env(key, value);
    }
    // Without this, a `command.status()` that hits the `timeout` below drops
    // its `Child` handle without killing the underlying process (tokio only
    // kills-on-drop when asked to): the one-shot subcommand would keep
    // running as an orphan indefinitely instead of dying with the timeout
    // that was supposed to bound it.
    command.kill_on_drop(true);

    let captured = || std::fs::read_to_string(&capture_path).unwrap_or_default();
    let status = tokio::time::timeout(ONE_SHOT_TIMEOUT, command.status())
        .await
        .unwrap_or_else(|_| {
            panic!(
                "`ilium {args:?}` did not exit within {ONE_SHOT_TIMEOUT:?}.\nit printed: {:?}\n{}",
                captured(),
                session_state_report(xdg)
            )
        })
        .unwrap_or_else(|error| panic!("failed to spawn `ilium {args:?}`: {error}"));

    std::process::Output {
        status,
        stdout: captured().into_bytes(),
        stderr: Vec::new(),
    }
}

/// What this test's isolated session looks like on disk right now: whether a
/// runtime endpoint was ever created, and whatever the client and the
/// detached server wrote to their own logs.
///
/// A one-shot subcommand that never returns has either failed to start the
/// server or failed to reach it, and those two look identical from the
/// outside. The runtime directory tells them apart -- on Unix the session
/// socket is a file that either exists or does not; on Windows the endpoint
/// is a named pipe with no filesystem presence, so there the logs are the
/// only evidence there is.
fn session_state_report(xdg: &IsolatedXdgDirs) -> String {
    let mut report = String::new();
    for (label, directory) in [
        ("runtime", xdg.runtime_dir.join("ilium")),
        ("runtime root", xdg.runtime_dir.clone()),
        ("debug logs", xdg.debug_log_dir.clone()),
    ] {
        match std::fs::read_dir(&directory) {
            Ok(entries) => {
                let names: Vec<String> = entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect();
                report.push_str(&format!("{label} {directory:?}: {names:?}\n"));
            }
            Err(error) => report.push_str(&format!("{label} {directory:?}: {error}\n")),
        }
    }
    for (path, contents) in walk_log_files(&xdg.debug_log_dir) {
        report.push_str(&format!("--- {path:?}\n{contents}\n"));
    }
    // The decisive question behind a hung subcommand is whether the detached
    // server was ever started, and nothing else here answers it: file logging
    // is off by default (and one test asserts that), so an absent log proves
    // nothing either way.
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let servers: Vec<String> = system
        .processes()
        .values()
        .filter(|process| {
            process
                .name()
                .to_string_lossy()
                .to_lowercase()
                .contains("ilium")
        })
        .map(|process| {
            format!(
                "{} pid={} cmd={:?}",
                process.name().to_string_lossy(),
                process.pid(),
                process.cmd()
            )
        })
        .collect();
    report.push_str(&format!("ilium processes alive: {servers:#?}\n"));
    report
}

/// Every readable file under `root`, one level of session directories deep --
/// the shape `ilium_logging` writes: one directory per session, holding its
/// timestamped log plus the `.active-log-path` pointer.
fn walk_log_files(root: &Path) -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return files;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            files.extend(walk_log_files(&path));
        } else if let Ok(contents) = std::fs::read_to_string(&path) {
            files.push((path, contents));
        }
    }
    files
}

/// Finds the one socket owned by this test's isolated runtime directory and
/// returns both its stable path and the detached server peer PID.
#[cfg(unix)]
async fn isolated_server_identity(xdg: &IsolatedXdgDirs) -> (PathBuf, u32) {
    let socket_directory = xdg.runtime_dir.join("ilium");
    let socket_paths = std::fs::read_dir(&socket_directory)
        .unwrap_or_else(|error| {
            panic!("read isolated socket directory {socket_directory:?}: {error}")
        })
        .filter_map(|entry| {
            let entry = entry.expect("read isolated socket entry");
            entry
                .file_type()
                .expect("read isolated socket entry type")
                .is_socket()
                .then(|| entry.path())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        socket_paths.len(),
        1,
        "expected exactly one isolated session socket, got {socket_paths:?}"
    );
    let socket_path = socket_paths[0].clone();
    let stream = tokio::net::UnixStream::connect(&socket_path)
        .await
        .unwrap_or_else(|error| {
            panic!("connect to isolated server socket {socket_path:?}: {error}")
        });
    let process_id = stream
        .peer_cred()
        .expect("read isolated server peer credentials")
        .pid()
        .expect("isolated server socket should report a peer PID");
    let process_id = u32::try_from(process_id).expect("server PID should fit u32");
    (socket_path, process_id)
}

/// Waits for the next authoritative tree snapshot on an attached control
/// connection. The PTY client remains the UI under test; this second client
/// exists only to submit a deterministic restructure request and inspect the
/// same broadcast snapshot every real attached client receives.
#[cfg(unix)]
async fn receive_tree_snapshot(connection: &mut Connection, context: &str) -> Tree {
    tokio::time::timeout(WAIT_TIMEOUT, async {
        while let Some(event) = connection.events.recv().await {
            if let ServerEvent::TreeSnapshot(tree) = event {
                return tree;
            }
        }
        panic!("{context}: server closed the control connection before a tree snapshot");
    })
    .await
    .unwrap_or_else(|_| panic!("{context}: no tree snapshot within {WAIT_TIMEOUT:?}"))
}

/// Writes `.ilium/config.yaml` with a pre-set project name into `cwd`,
/// matching `ilium_client::project_config`'s on-disk format (a plain
/// YAML mapping under the `project name` key) closely enough for
/// `project_naming::load_stored_project_name` to read it back -- see
/// this file's module docs for why this must happen before attaching.
fn seed_project_config(cwd: &Path) {
    let ilium_dir = cwd.join(".ilium");
    std::fs::create_dir_all(&ilium_dir).expect("create .ilium dir");
    std::fs::write(
        ilium_dir.join("config.yaml"),
        format!("project name: {PROJECT_NAME}\n"),
    )
    .expect("write .ilium/config.yaml");
}

/// Writes the user-wide client config inside this test's isolated XDG root,
/// proving the TUI reads a non-default shortcut base through the real config
/// path rather than only exercising an in-memory unit-test value.
fn seed_keyboard_config(xdg: &IsolatedXdgDirs) {
    let ilium_config_dir = xdg.config_home.join("ilium");
    std::fs::create_dir_all(&ilium_config_dir).expect("create isolated ilium config dir");
    std::fs::write(
        ilium_config_dir.join("config.toml"),
        // Deliberately no `[debug] file_logging_enabled`: this test asserts
        // further down that the Settings screen shows it off by default, so
        // seeding it on here would be seeding the answer.
        "[keyboard]\nshortcut_base = \"b\"\n",
    )
    .expect("write isolated keyboard config");
}

/// Enables only the row-management controls in smoke scenarios that assert
/// direct rename/reorder mouse gestures. The product default remains off.
fn seed_tree_row_management_controls(xdg: &IsolatedXdgDirs) {
    let ilium_config_dir = xdg.config_home.join("ilium");
    std::fs::create_dir_all(&ilium_config_dir).expect("create isolated ilium config dir");
    let config_path = ilium_config_dir.join("config.toml");
    let existing = std::fs::read_to_string(&config_path).unwrap_or_default();
    let separator = (!existing.is_empty() && !existing.ends_with('\n')).then_some("\n");
    std::fs::write(
        config_path,
        format!(
            "{existing}{}[ui]\nshow_tree_row_management_controls = true\n",
            separator.unwrap_or_default()
        ),
    )
    .expect("write isolated row-management config");
}

/// Enables the otherwise opt-in agent-debug surface for an isolated client
/// and detached server without touching the developer's real config.
#[cfg(unix)]
fn seed_agent_debug_config(xdg: &IsolatedXdgDirs) {
    let ilium_config_dir = xdg.config_home.join("ilium");
    std::fs::create_dir_all(&ilium_config_dir).expect("create isolated ilium config dir");
    std::fs::write(
        ilium_config_dir.join("config.toml"),
        "[debug]\nfile_logging_enabled = true\n\n[detection]\nworking_poll_seconds = 1\n\n[ui]\nagent_debug_menu_enabled = true\n",
    )
    .expect("write isolated agent-debug config");
}

/// Finds the timestamped private process log that contains this test's exact
/// canonical project path. Session directory names are intentionally hashed,
/// so the active-log metadata is the authoritative path boundary.
fn active_log_path_from_metadata(metadata: &str) -> Option<PathBuf> {
    if metadata.starts_with("pid=") {
        return metadata
            .lines()
            .find_map(|line| line.strip_prefix("log_path="))
            .filter(|path| !path.is_empty())
            .map(PathBuf::from);
    }
    (!metadata.is_empty()).then(|| PathBuf::from(metadata))
}

#[cfg(unix)]
fn process_log_for_project(log_root: &Path, project_dir: &Path) -> Option<(PathBuf, String)> {
    let project_path = project_dir
        .canonicalize()
        .ok()?
        .to_string_lossy()
        .into_owned();
    // The caller's own root, passed in rather than resolved here: the spawned
    // processes are given `DEBUG_LOG_DIR_ENV`, but this scan runs in the test
    // process, which never sees it and would otherwise read the machine-wide
    // root shared with every other project.
    let log_root = std::fs::read_dir(log_root).ok()?;
    for session_entry in log_root.filter_map(Result::ok) {
        let Ok(file_type) = session_entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let active_path = session_entry.path().join(".active-log-path");
        let Ok(metadata) = std::fs::read_to_string(active_path) else {
            continue;
        };
        let Some(log_path) = active_log_path_from_metadata(&metadata) else {
            continue;
        };
        let Ok(contents) = std::fs::read_to_string(&log_path) else {
            continue;
        };
        if contents.contains(&project_path) {
            return Some((log_path, contents));
        }
    }
    None
}

/// Every `submitted input` value the log recorded for an accepted prompt.
///
/// Parsed rather than substring-matched: the record is a serialized
/// `AgentDebugField`, and whether its keys come out in declaration order or
/// alphabetically depends on whether some crate in the build enables
/// `serde_json/preserve_order` -- which differs between a whole-workspace
/// build and a single-package one. Matching `"label":...,"value":...` as
/// adjacent text therefore asserts a feature-unification detail rather than
/// what the log actually says.
#[cfg(unix)]
fn logged_prompt_submissions(contents: &str) -> Vec<String> {
    contents
        .lines()
        .filter(|line| line.contains("event_kind=PromptSubmitted"))
        .filter_map(|line| line.split_once("agent_event="))
        .filter_map(|(_, json)| serde_json::from_str::<serde_json::Value>(json).ok())
        .flat_map(|event| {
            event["fields"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|field| field["label"] == "submitted input")
                .filter_map(|field| field["value"].as_str().map(str::to_owned))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Everything the server recorded about identifying and classifying agents
/// in this session.
///
/// When a pane that is running a recognisable agent is not shown as one, the
/// detector already wrote down why -- which process it looked at, which
/// signature it compared, and what it concluded. Reading that back is the
/// difference between "detection did not happen" and knowing which step
/// declined.
#[cfg(unix)]
fn detection_diagnostics(log_root: &Path, project_dir: &Path) -> String {
    let Some((log_path, contents)) = process_log_for_project(log_root, project_dir) else {
        return format!(
            "no process log found for {project_dir:?}\n{}",
            process_log_diagnostics(log_root, project_dir)
        );
    };
    let decisions: Vec<&str> = contents
        .lines()
        .filter(|line| {
            line.contains("identity") || line.contains("Detection") || line.contains("detection")
        })
        .collect();
    format!(
        "detection decisions in {log_path:?}:\n  {}",
        if decisions.is_empty() {
            "(none -- the detector never recorded a decision for any pane)".to_string()
        } else {
            decisions.join("\n  ")
        }
    )
}

/// Explains what a failed process-log expectation actually saw. Every step the
/// match depends on is reported -- the canonical project path, each session
/// directory's active-log metadata, whether the file it names is readable, and
/// the prompt records it holds -- because the interesting answers ("the path
/// resolved to a different prefix", "the log exists but recorded a shorter
/// prompt because the keystrokes went to the tree") are all invisible in a bare
/// "expected X, found nothing".
#[cfg(unix)]
fn process_log_diagnostics(log_root: &Path, project_dir: &Path) -> String {
    let mut report = format!(
        "log root: {log_root:?}\nproject dir as given: {project_dir:?}\ncanonical project path: {:?}\n",
        project_dir.canonicalize()
    );
    let Ok(entries) = std::fs::read_dir(log_root) else {
        report.push_str("log root is not readable\n");
        return report;
    };
    for session_entry in entries.filter_map(Result::ok) {
        let session_path = session_entry.path();
        report.push_str(&format!("session dir {session_path:?}\n"));
        let metadata = std::fs::read_to_string(session_path.join(".active-log-path"));
        report.push_str(&format!("  active-log metadata: {metadata:?}\n"));
        let Some(log_path) = metadata
            .ok()
            .as_deref()
            .and_then(active_log_path_from_metadata)
        else {
            continue;
        };
        let Ok(contents) = std::fs::read_to_string(&log_path) else {
            report.push_str(&format!("  log {log_path:?} is not readable\n"));
            continue;
        };
        report.push_str(&format!(
            "  log {log_path:?} holds {} bytes\n",
            contents.len()
        ));
        report.push_str(&format!(
            "  parsed submitted inputs: {:?}\n",
            logged_prompt_submissions(&contents)
        ));
        for line in contents
            .lines()
            .filter(|line| line.contains("PromptSubmitted") || line.contains("submitted input"))
        {
            report.push_str(&format!("  prompt record: {line}\n"));
        }
    }
    report
}

/// Produces a deterministic process literally named `codex` whose visible
/// status changes only in volatile counters. The detector must keep polling
/// it while the journal retains one semantic conclusion.
#[cfg(unix)]
fn write_change_only_fake_codex(directory: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = directory.join("codex");
    let script = "#!/bin/sh\n\
        counter=1\n\
        while :; do\n\
          printf '\\033[Hmodel · workspace · Working · Pursuing goal (%sm)\\033[K\\n' \"$counter\"\n\
          printf 'Cogitating (esc to interrupt) · %ss · %s tokens\\033[K' \"$counter\" \"$counter\"\n\
          counter=$((counter + 1))\n\
          sleep 1\n\
        done\n";
    std::fs::write(&path, script).expect("write change-only fake Codex");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
        .expect("make change-only fake Codex executable");
    path
}

#[tokio::test]
async fn attaching_tui_renders_the_pane_created_by_new_pane_and_responds_to_the_help_keystroke() {
    let temp_root = tempfile::tempdir().expect("create tempdir");
    let xdg = IsolatedXdgDirs::under(temp_root.path()).expect("create isolated XDG dirs");
    seed_keyboard_config(&xdg);
    seed_tree_row_management_controls(&xdg);
    let project_dir = temp_root.path().join("project");
    std::fs::create_dir_all(&project_dir).expect("create project dir");
    seed_project_config(&project_dir);
    // Declared after `temp_root` (see `KillSessionOnDrop`'s doc comment) and
    // before phase 1, which is the earliest point that can spawn the
    // server this guard exists to not leak.
    let mut cleanup_guard = KillSessionOnDrop {
        xdg: &xdg,
        cwd: project_dir.clone(),
        session_name: SESSION_NAME,
        already_cleaned_up: false,
    };

    // Phase 1: `ilium new-pane -- cat` -- no `--cwd` flag on this
    // subcommand (see `ilium/src/main.rs`'s doc comment on `NewPane`),
    // so the pane it creates is rooted wherever this subprocess's own
    // cwd is; pointed at `project_dir` so it lands in the same place
    // phase 2 attaches to, though `NewPane` itself doesn't care since
    // `cat` needs no real filesystem content.
    let new_pane_output = run_one_shot(&xdg, &project_dir, &new_idle_pane_arguments()).await;
    assert!(
        new_pane_output.status.success(),
        "`ilium {:?}` failed: {:?}",
        new_idle_pane_arguments(),
        String::from_utf8_lossy(&new_pane_output.stdout)
    );

    // Phase 2: the actual attaching form, inside a real pty at a fixed
    // size -- exactly the technique `ilium-pty/tests/pty_integration.rs`
    // uses for a trivial command, applied to the real CLI/TUI binary.
    let attach_command = PtyCommand::new(ilium_binary(), &project_dir, 44, 120)
        .arg("--restart-server")
        .arg("--cwd")
        .arg(project_dir.to_string_lossy().to_string());
    let attach_command = xdg
        .as_pairs()
        .into_iter()
        .fold(attach_command, |command, (key, value)| {
            command.env(key, value.to_string_lossy().to_string())
        });
    let mut tui = PtySession::spawn(attach_command).expect("spawning `ilium` under a pty");

    // Structural assertion #1: the sidebar chrome
    // (`ilium_client::tree_ui::sidebar_title`) shows this test's seeded
    // project name, and the group phase 1's pane landed in
    // (`ilium-server::ipc::handlers::pane_snapshot_kind_for` creates a
    // default group for a bare `NewPane`, per `ilium-server/tests/smoke.rs`)
    // is listed by name. Together these prove: the server was found and
    // attached to, the tree snapshot round-tripped over IPC, and
    // rendering produced real screen content -- not just that the
    // process didn't crash.
    let rendered = wait_until(
        || {
            let screen = tui.screen_text();
            screen.contains(PROJECT_NAME) && screen.contains("default")
        },
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        rendered,
        "expected the sidebar title {PROJECT_NAME:?} and the \"default\" group on screen, got: {:?}",
        tui.screen_text()
    );
    assert!(
        !tui.screen_text().contains("keyboard reference"),
        "the help overlay should not be showing before the help keystroke is sent"
    );

    // The footer -- status bar plus voice control -- is the bottom row of the
    // frame, and every later assertion about it assumes it is drawn at all.
    // Checked here, on the plain frame, because that is the only moment its
    // absence has an unambiguous cause: no overlay is covering it, so the
    // frame's own borders say whether the client's layout fits the terminal
    // it was given.
    assert!(
        wait_until(|| tui.screen_text().contains("VOICE"), WAIT_TIMEOUT).await,
        "expected the footer's voice control on the plain frame. terminal is \
         {:?}, the frame's top border is on row(s) {:?} and its bottom border \
         on row(s) {:?}, bottom rows are {:#?}",
        tui.with_screen(|screen| screen.size()),
        tui.with_screen(|screen| rows_containing(screen, "\u{256d}")),
        tui.with_screen(|screen| rows_containing(screen, "\u{2570}")),
        tui.with_screen(|screen| bottom_rows(screen, 3)),
    );

    // A freshly attached client starts with every group collapsed and
    // focus on the (empty) pane panel, so phase 1's "cat" pane isn't
    // actually listed yet -- expand it to also prove out tree navigation
    // (`ilium_client::app::App::handle_tree_key`), not just leader-key
    // dispatch: `Ctrl+B then t` (`Action::FocusTree`) moves focus to the
    // tree, Down selects its first entry (the "default" group -- see
    // `tui_tree_widget::TreeState::key_down`'s "nothing selected ->
    // select the first item" behavior), and Right expands it
    // (`TreeState::key_right`).
    tui.write(b"\x02t")
        .expect("writing Ctrl+B then t (FocusTree)");
    tui.write(b"j").expect("selecting the first tree entry");
    tui.write(b"l").expect("expanding the selected tree entry");
    let pane_listed = wait_until(
        || {
            let screen = tui.screen_text();
            screen.contains(IDLE_PANE_LABEL) && screen.contains("📟")
        },
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        pane_listed,
        "expected the \"cat\" pane to appear in the tree after expanding its group, got: {:?}",
        tui.screen_text()
    );
    let focused_footer_shown = wait_until(|| tui.screen_text().contains("🎚️"), WAIT_TIMEOUT).await;
    assert!(
        focused_footer_shown,
        "expected the settings sliders to be visible while the tree has keyboard focus, got: {:?}",
        tui.screen_text()
    );

    // The focused sidebar widens over 180 ms. Resolve the physical toolbar
    // cell only after that transition settles so its coordinate cannot move
    // between screen capture and the synthetic mouse press under CPU load.
    tokio::time::sleep(Duration::from_millis(250)).await;

    // Exercise the complete physical click. The release itself is harmless;
    // the historical close happened on the next periodic maintenance tick,
    // when the workspace-search poll replaced every non-search mode with
    // Normal while merely checking whether a debounced scan was due.
    let (_, settings_row) = tui
        .with_screen(|screen| first_cell_containing(screen, "🎚️"))
        .expect("rendered settings toolbar icon should have a concrete row");
    // The emoji's vt100 payload can occupy a different cell than ratatui's
    // Unicode-width calculation under load. The Settings hit box is anchored
    // to the tree's right edge, so click its stable interior cell two columns
    // left of the rendered tree/pane divider instead of deriving x from the
    // grapheme-storage cell.
    let (tree_divider_column, _) = tui
        .with_screen(|screen| first_cell_containing(screen, "┬"))
        .expect("tree/pane divider should have a concrete cell");
    let settings_column = tree_divider_column.saturating_sub(2);
    tui.write(&sgr_mouse_down(0, settings_column, settings_row))
        .expect("pressing the settings toolbar icon");
    assert!(
        wait_until(|| tui.screen_text().contains("⚙ Settings"), WAIT_TIMEOUT).await,
        "expected the settings press to open Settings, got: {:?}",
        tui.screen_text()
    );
    tui.write(&sgr_mouse_up(settings_column, settings_row))
        .expect("releasing the settings toolbar icon");
    // The default non-animation poll interval is 250 ms. Waiting through
    // more than one cycle proves maintenance no longer destroys Settings.
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(
        tui.screen_text().contains("⚙ Settings"),
        "Settings closed after the opening click and maintenance ticks: {:?}",
        tui.screen_text()
    );

    // Bisects where the footer is lost on a host that clips or scrolls
    // differently: it is present on the plain frame (asserted above), so if it
    // is still here with Settings open, only the tab navigated to below can
    // have displaced it.
    assert!(
        tui.screen_text().contains("VOICE"),
        "the footer's voice control vanished when Settings opened. bottom rows are {:#?}",
        tui.with_screen(|screen| bottom_rows(screen, 3)),
    );

    // Settings opens on User Interface. Eight real Tab key events reach the
    // Voice control tab in the registry order, proving the feature is wired
    // into the same navigable settings surface as every established tab.
    tui.write(b"\t\t\t\t\t\t\t\t")
        .expect("navigating to Voice control settings");
    let voice_settings_shown = wait_until(
        || {
            let screen = tui.screen_text();
            screen.contains("Voice control")
                && screen.contains("OpenAI API key")
                && screen.contains("Reasoning effort")
                && screen.contains("VAD eagerness")
                && screen.contains("Confirm terminal submissions")
                && screen.contains("Custom prompt")
        },
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        voice_settings_shown,
        "expected the complete Voice control settings surface, got: {:?}",
        tui.screen_text()
    );
    assert!(
        tui.screen_text().contains("VOICE OFF") && tui.screen_text().contains("F8"),
        "expected the global voice control over Settings, and it was present \
         both on the plain frame and with Settings first opened, so the Voice \
         control tab's own rendering displaced it. terminal is {:?}, rows \
         mentioning VOICE are {:?}, bottom rows are {:#?}",
        tui.with_screen(|screen| screen.size()),
        tui.with_screen(|screen| rows_containing(screen, "VOICE")),
        tui.with_screen(|screen| bottom_rows(screen, 3)),
    );

    // Open the API-key child dialog from the real Settings screen. Both the
    // parent header/tab and the child hint must remain in the same captured
    // terminal frame; this is the regression boundary for stacked modals.
    tui.write(b"j\r")
        .expect("opening the voice API-key child dialog");
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                screen.contains("⚙ Settings")
                    && screen.contains("Voice control")
                    && screen.contains("Protected value")
                    && screen.contains("Keep existing")
                    && screen.contains("Replace")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "expected the API-key dialog over still-open Settings, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"smoke-key")
        .expect("typing the isolated smoke-test API key");
    let api_key_layout = ilium_client::modal::text_prompt_dialog_layout_for_size(120, 44);
    let replace_button = api_key_layout.actions.confirm_button;
    tui.write(&sgr_mouse_down(
        0,
        replace_button.x + replace_button.width / 2,
        replace_button.y,
    ))
    .expect("click Replace in the nested API-key dialog");
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                screen.contains("⚙ Settings")
                    && screen.contains("Voice control")
                    && !screen.contains("Keep existing")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "expected committing the child to reveal the same Settings parent, got: {:?}",
        tui.screen_text()
    );

    tui.write(b"\x1b")
        .expect("closing mouse-opened Settings with Esc");
    assert!(
        wait_until(|| !tui.screen_text().contains("⚙ Settings"), WAIT_TIMEOUT).await,
        "expected mouse-opened Settings to close before continuing"
    );
    assert!(
        tui.screen_text().contains("VOICE OFF") && tui.screen_text().contains("F8"),
        "expected the global voice control after Settings closed, got: {:?}",
        tui.screen_text()
    );

    // Move the real terminal pointer over the terminal row. This verifies
    // the final PTY-rendered action strip, not only its in-memory TestBackend
    // buffer: every action must remain visible, ordered, and separated after
    // crossterm writes it to a vt100 terminal surface.
    let terminal_rows =
        tui.with_screen(|screen| rows_containing_in_order(screen, &["📟", IDLE_PANE_LABEL]));
    assert_eq!(
        terminal_rows.len(),
        1,
        "expected one terminal row before hovering its actions, got: {:?}",
        tui.screen_text()
    );
    tui.write(&sgr_mouse_move(8, terminal_rows[0]))
        .expect("moving the pointer over the terminal row");
    let row_actions_shown = wait_until(
        || {
            tui.with_screen(|screen| {
                !rows_containing_in_order(screen, &["✏️", "🔼", "🔽", "🚫", "♻️"]).is_empty()
            })
        },
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        row_actions_shown,
        "expected the complete normal-icon row actions after hover, got: {:?}",
        tui.screen_text()
    );

    // Open the real tree context menu with a right click, then activate its
    // adjacent Order by submenu through the rendered menu row. The checked
    // Manual label proves the submenu reflects the live setting before a
    // different choice is selected.
    let default_rows = tui.with_screen(|screen| rows_containing(screen, "default"));
    assert_eq!(
        default_rows.len(),
        1,
        "expected one default-group row before opening its context menu, got: {:?}",
        tui.screen_text()
    );
    let context_column = 8;
    tui.write(&sgr_mouse_down(2, context_column, default_rows[0]))
        .expect("right-clicking the default group");
    assert!(
        wait_until(|| tui.screen_text().contains("Order by"), WAIT_TIMEOUT).await,
        "expected Order by in the tree context menu, got: {:?}",
        tui.screen_text()
    );
    let order_rows = tui.with_screen(|screen| rows_containing(screen, "Order by"));
    assert_eq!(
        order_rows.len(),
        1,
        "expected one Order by action row, got: {:?}",
        tui.screen_text()
    );
    tui.write(&sgr_mouse_down(0, context_column + 1, order_rows[0]))
        .expect("opening the Order by submenu");
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                // The check mark and the label are asserted separately because
                // a configurable order icon renders between them, so pinning
                // the adjacent pair would break on any icon change.
                screen.contains('✓')
                    && screen.contains("Manual")
                    && screen.contains("Type")
                    && screen.contains("Age up (newest first)")
                    && screen.contains("Age down (oldest first)")
                    && screen.contains("Name A-Z")
                    && screen.contains("Name Z-A")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "expected the complete checked Order by submenu, got: {:?}",
        tui.screen_text()
    );
    // Avoid escape-prefixed arrows in the real PTY: under load the terminal
    // can surface a standalone Escape before completing a CSI sequence,
    // which correctly dismisses a context menu. `j` has the same menu
    // navigation contract without that ambiguous prefix.
    tui.write(b"jjj\r")
        .expect("selecting Age down from the Order by submenu");
    let age_order_persisted = wait_until(
        || {
            std::fs::read_to_string(xdg.config_home.join("ilium").join("config.toml"))
                .unwrap_or_default()
                .contains("tree_order = \"age_descending\"")
        },
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        age_order_persisted,
        "expected context-menu ordering to persist, config={:?}",
        std::fs::read_to_string(xdg.config_home.join("ilium").join("config.toml"))
    );
    // Structural assertion #2: input routing. `Ctrl+B` (0x02, selected in
    // this test's isolated config -- `ilium_client::keymap::is_leader_key`)
    // followed by `?` (`ilium_client::keymap::Action::Help`'s bound
    // letter) must flip the render to show
    // `ilium_client::help::render`'s overlay -- proof that keystrokes
    // typed into this pty actually reach the input-dispatch state
    // machine and that its effect actually reaches the next rendered
    // frame, not just that *a* frame renders.
    tui.write(b"\x02?")
        .expect("writing the leader+help keystroke to the pty");
    let help_shown = wait_until(
        || {
            let screen = tui.screen_text();
            screen.contains("keyboard reference") && screen.contains("Ctrl+B ?")
        },
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        help_shown,
        "expected the help overlay after Ctrl+B then ?, got: {:?}",
        tui.screen_text()
    );

    // Exercise the real full-screen settings path too: configure the tree's
    // agent identifier, exercise the dedicated icon editor, then switch to
    // Keyboard and reapply the tmux preset. These
    // assertions cover actual rendered controls and persisted config rather
    // than only config/keymap units.
    tui.write(b"\x1b").expect("closing Help with Esc");
    assert!(
        wait_until(
            || !tui.screen_text().contains("keyboard reference"),
            WAIT_TIMEOUT
        )
        .await,
        "expected Help to close before opening Settings"
    );
    tui.write(b"\x02S")
        .expect("opening Settings with Ctrl+B then S");
    let settings_shown = wait_until(
        || tui.screen_text().contains("\u{2699} Settings"),
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        settings_shown,
        "expected Settings to open, got: {:?}",
        tui.screen_text()
    );
    let agent_controls_shown = wait_until(
        || {
            let screen = tui.screen_text();
            screen.contains("Fixed")
                && screen.contains("Focus-dependent")
                && screen.contains("Width-dependent")
                && screen.contains("Unfocused width")
                && screen.contains("Focused width")
                && screen.contains("Tree order")
                && screen.contains("Age down (oldest first)")
                && screen.contains("Agent identifier")
                && screen.contains("Full name")
        },
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        agent_controls_shown,
        "expected agent identifier controls in User Appearance, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"l")
        .expect("selecting the width-dependent sizing card");
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                let config =
                    std::fs::read_to_string(xdg.config_home.join("ilium").join("config.toml"))
                        .unwrap_or_default();
                screen.contains("Minimum terminal width")
                    && screen.contains("At ≥")
                    && config.contains("left_panel_sizing_mode = \"terminal_width_dependent\"")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "expected width-dependent controls and persistence, screen={:?}",
        tui.screen_text()
    );
    tui.write(b"h")
        .expect("returning to the focus-dependent sizing card");
    // Use the settings view's `j`/`l` aliases rather than escape-prefixed
    // arrows: a real PTY can deliver an isolated escape before the rest of
    // a CSI sequence, which would legitimately close this full-screen view.
    tui.write(b"jjjjjll")
        .expect("selecting icon mode for agent identifiers");
    let agent_controls_persisted = wait_until(
        || {
            let config = std::fs::read_to_string(xdg.config_home.join("ilium").join("config.toml"))
                .unwrap_or_default();
            config.contains("agent_identifier_mode = \"icon\"")
        },
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        agent_controls_persisted,
        "expected the agent identifier choice to persist, config={:?}",
        std::fs::read_to_string(xdg.config_home.join("ilium").join("config.toml"))
    );
    // Icons follows Appearance. Exercise the live table, demo/real toolbar,
    // and full-screen catalogue before continuing to Keyboard.
    tui.write(b"\t")
        .expect("switching to the Icons settings tab");
    let icons_tab_shown = wait_until(
        || {
            let screen = tui.screen_text();
            screen.contains("Icon assignments")
                && screen.contains("Sidebar preview")
                && screen.contains("Terminal")
                && screen.contains("[+]")
        },
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        icons_tab_shown,
        "expected the two-sided Icons settings tab, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"lr")
        .expect("changing the group icon and switching the preview to the real tree");
    assert!(
        wait_until(
            || std::fs::read_to_string(xdg.config_home.join("ilium").join("config.toml"))
                .is_ok_and(|config| config.contains("group = \"📂\"")),
            WAIT_TIMEOUT,
        )
        .await,
        "expected a live Icons-table change to persist"
    );
    tui.write(b"\r")
        .expect("opening the full-screen icon catalogue");
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                screen.contains("Icon catalogue")
                    && screen.contains("Quick picks")
                    && screen.contains("Official UTF-8 chapters first")
                    && screen.contains("12372 total")
                    && screen.contains("[● Multi-column]")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "expected the full-screen grouped icon catalogue, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"v")
        .expect("switching the catalogue to its single-column inspection view");
    assert!(
        wait_until(
            || tui.screen_text().contains("[● Single column]"),
            WAIT_TIMEOUT
        )
        .await,
        "expected the icon catalogue single-column switch, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"v")
        .expect("restoring the default multi-column catalogue view");
    assert!(
        wait_until(
            || tui.screen_text().contains("[● Multi-column]"),
            WAIT_TIMEOUT
        )
        .await,
        "expected the icon catalogue multi-column switch, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"j\r")
        .expect("selecting a catalogue icon for the group");
    tui.write(b"\t")
        .expect("switching to the Keyboard settings tab");
    let keyboard_tab_shown = wait_until(
        || {
            let screen = tui.screen_text();
            screen.contains("Shortcut base")
                && screen.contains("Ctrl+B")
                && screen.contains("Recommended")
        },
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        keyboard_tab_shown,
        "expected the Keyboard settings tab with Ctrl+B selected, got: {:?}",
        tui.screen_text()
    );

    tui.write(b"2").expect("restoring the Ctrl+B preset");
    let preset_restored = wait_until(
        || tui.screen_text().contains("Recommended: Ctrl+B"),
        WAIT_TIMEOUT,
    )
    .await;
    assert!(preset_restored, "expected Ctrl+B preset to restore");
    let persisted_config =
        std::fs::read_to_string(xdg.config_home.join("ilium").join("config.toml"))
            .expect("read settings-persisted keyboard config");
    assert!(
        persisted_config.contains("shortcut_base = \"b\""),
        "expected the restored Ctrl+B preset to persist, got: {persisted_config:?}"
    );

    // The Kanban Board tab owns card compactness and column sizing
    // independently from general appearance. Prove both defaults, live
    // adjustment, and isolated persistence before continuing to Sound.
    tui.write(b"\t\t\t\t")
        .expect("switching to the Kanban Board settings tab");
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                screen.contains("Card preview lines")
                    && screen.contains("‹ 3 ›")
                    && screen.contains("Minimum column width")
                    && screen.contains("‹ 20 ›")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "expected the Kanban Board three-line default, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"l")
        .expect("increase board card previews to four lines");
    assert!(
        wait_until(
            || std::fs::read_to_string(xdg.config_home.join("ilium").join("config.toml"))
                .is_ok_and(|config| config.contains("card_preview_lines = 4")),
            WAIT_TIMEOUT,
        )
        .await,
        "expected Kanban Board setting to persist"
    );
    tui.write(b"jl")
        .expect("select and increase the minimum board column width");
    assert!(
        wait_until(
            || std::fs::read_to_string(xdg.config_home.join("ilium").join("config.toml"))
                .is_ok_and(|config| config.contains("minimum_column_width = 21")),
            WAIT_TIMEOUT,
        )
        .await,
        "expected minimum board column width to persist"
    );

    // Sound follows Kanban Board. Exercise a real event checkbox without
    // activating Preview, so this remains a silent automated test while
    // proving live request dispatch still occurs in the real TUI.
    tui.write(b"\t")
        .expect("switching to the Sound settings tab");
    let sound_tab_shown = wait_until(
        || {
            let screen = tui.screen_text();
            screen.contains("Sound source")
                && screen.contains("System beep")
                && screen.contains("Agent finished")
                && screen.contains("Discovered system sound folders")
        },
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        sound_tab_shown,
        "expected the Sound settings tab, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"jjj ")
        .expect("moving to and toggling Agent finished");
    let sound_event_disabled = wait_until(
        || {
            let config = std::fs::read_to_string(xdg.config_home.join("ilium").join("config.toml"))
                .unwrap_or_default();
            config.contains("agent_finished = false")
        },
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        sound_event_disabled,
        "expected Agent finished sound checkbox to persist as disabled, config={:?}",
        std::fs::read_to_string(xdg.config_home.join("ilium").join("config.toml"))
    );

    // Triggers follows Voice control and Inference. Verify the installed
    // binary renders the new event-to-actions surface, persists an opt-in
    // startup action immediately, and keeps a later event visible while its
    // longer document scrolls to follow keyboard selection.
    tui.write(b"\t\t\t")
        .expect("switching to the Triggers settings tab");
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                screen.contains("AUTOMATION ROUTER")
                    && screen.contains("Startup complete")
                    && screen.contains("All projects")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "expected the Triggers settings tab, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"l ")
        .expect("enabling the startup all-project restructure trigger");
    let startup_trigger_persisted = wait_until(
        || {
            let config = std::fs::read_to_string(xdg.config_home.join("ilium").join("config.toml"))
                .unwrap_or_default();
            config.contains("startup_complete = [\"restructure_all_projects\"]")
        },
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        startup_trigger_persisted,
        "expected the startup trigger to persist, config={:?}",
        std::fs::read_to_string(xdg.config_home.join("ilium").join("config.toml"))
    );
    tui.write(b"jjjjjj")
        .expect("moving to the Agent finishes work trigger event");
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                screen.contains("Agent finishes work")
                    && screen.contains("Retitle")
                    && screen.contains("Project")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "expected keyboard-following scroll to reveal Agent finishes work, got: {:?}",
        tui.screen_text()
    );

    // Debug follows Triggers. Its real toggle must preserve the fresh-install
    // default (no `.txt` file before opt-in), persist the choice, open both
    // process writers immediately, and make the next structural request land
    // in the server's major-action trail.
    tui.write(b"\t")
        .expect("switching to the Debug settings tab");
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                screen.contains("Debug")
                    && screen.contains("File logging")
                    && screen.contains("‹ Off ›")
                    && screen.contains("LLM requests/responses")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "expected disabled-by-default Debug file logging, got: {:?}",
        tui.screen_text()
    );
    let socket_file_name = std::fs::read_dir(xdg.runtime_dir.join("ilium"))
        .expect("read isolated socket directory")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .find(|name| name.to_string_lossy().ends_with(".sock"))
        .expect("isolated session socket");
    // This test's own root, not the machine-wide default: the spawned
    // processes were given `DEBUG_LOG_DIR_ENV`, but this lookup runs in the
    // test process, which never sees it.
    let session_log_directory = xdg
        .debug_log_dir
        .join(socket_file_name.to_string_lossy().trim_end_matches(".sock"));
    let active_log_path = active_log_path_from_metadata(
        &std::fs::read_to_string(session_log_directory.join(".active-log-path"))
            .expect("read active log metadata"),
    )
    .expect("active log metadata contains a log path");
    assert!(
        !active_log_path.exists(),
        "disabled logging unexpectedly created {}",
        active_log_path.display()
    );

    tui.write(b" ").expect("enabling Debug file logging");
    assert!(
        wait_until(
            || {
                let config =
                    std::fs::read_to_string(xdg.config_home.join("ilium").join("config.toml"))
                        .unwrap_or_default();
                let log = std::fs::read_to_string(&active_log_path).unwrap_or_default();
                config.contains("[debug]")
                    && config.contains("file_logging_enabled = true")
                    && log.contains("client file logging enabled from Debug settings")
                    && log.contains("server file logging enabled from Debug settings")
                    && log.contains("client file logging synchronized from server")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "expected both process writers to enable at {}, config={:?}, log={:?}",
        active_log_path.display(),
        std::fs::read_to_string(xdg.config_home.join("ilium").join("config.toml")),
        std::fs::read_to_string(&active_log_path)
    );
    let logged_action = run_one_shot(&xdg, &project_dir, &new_idle_pane_arguments()).await;
    assert!(
        logged_action.status.success(),
        "creating a logged pane failed: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&logged_action.stdout),
        String::from_utf8_lossy(&logged_action.stderr)
    );
    assert!(
        wait_until(
            || std::fs::read_to_string(&active_log_path)
                .is_ok_and(|log| log.contains("request_name=\"new_pane\"")),
            WAIT_TIMEOUT,
        )
        .await,
        "expected new-pane action in {}, got: {:?}",
        active_log_path.display(),
        std::fs::read_to_string(&active_log_path)
    );

    // Cleanup: reuse the CLI's own graceful `kill-session` subcommand
    // (see this file's module docs) rather than killing the pty-attached
    // process directly -- its own exit, awaited below, is this test's
    // proof the graceful shutdown path (server closes every connection
    // on `KillSession`, `ilium_client::run`'s event loop then exits on
    // its own) actually works end-to-end.
    let kill_output = run_one_shot(&xdg, &project_dir, &["kill-session", SESSION_NAME]).await;
    assert!(
        kill_output.status.success(),
        "`ilium kill-session {SESSION_NAME}` failed: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&kill_output.stdout),
        String::from_utf8_lossy(&kill_output.stderr)
    );
    // The explicit, awaited cleanup above already succeeded -- the
    // drop-time guard no longer has anything to do.
    cleanup_guard.already_cleaned_up = true;

    let attached_process_exited = wait_until(|| tui.has_exited(), WAIT_TIMEOUT).await;
    if !attached_process_exited {
        // Defensive fallback only -- see this file's module docs. Getting
        // here means the graceful shutdown path itself regressed, which
        // is a real bug the assertion above already failed loudly on;
        // this just keeps one broken run from hanging the whole suite.
        tui.kill()
            .expect("force-killing the pty-attached ilium process");
    }
    assert!(
        attached_process_exited,
        "the pty-attached `ilium` process should exit on its own once `kill-session` \
         closes the connection, not need a force kill"
    );
}

/// Replaces the executable path underneath a live client with a marker shim,
/// activates Restart through the real right-click menu, and proves the same
/// process loads the replacement before reattaching to the untouched server.
#[cfg(unix)]
#[tokio::test]
async fn right_click_restart_reloads_only_the_client_and_preserves_the_server() {
    use std::os::unix::fs::PermissionsExt;

    let temp_root = tempfile::tempdir().expect("create tempdir");
    let xdg = IsolatedXdgDirs::under(temp_root.path()).expect("create isolated XDG dirs");
    seed_keyboard_config(&xdg);
    let project_dir = temp_root.path().join("client-restart-project");
    std::fs::create_dir_all(&project_dir).expect("create project dir");
    seed_project_config(&project_dir);
    let mut cleanup_guard = KillSessionOnDrop {
        xdg: &xdg,
        cwd: project_dir.clone(),
        session_name: SESSION_NAME,
        already_cleaned_up: false,
    };

    // Start the detached server with a durable terminal fixture before the
    // copied client attaches. The copied binary therefore never needs a
    // sibling `ilium-server` executable in its temporary directory.
    let new_pane_output = run_one_shot(&xdg, &project_dir, &["new-pane", "--", "cat"]).await;
    assert!(
        new_pane_output.status.success(),
        "creating the client-restart fixture failed: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&new_pane_output.stdout),
        String::from_utf8_lossy(&new_pane_output.stderr)
    );
    let (server_socket_before, server_process_id_before) = isolated_server_identity(&xdg).await;

    // Run through a private hard link so the test can atomically replace
    // exactly the path captured by `attach_or_create` without allocating a
    // second binary-sized file or touching Cargo's shared test artifact.
    let original_binary = std::fs::canonicalize(ilium_binary()).expect("canonicalize ilium binary");
    let binary_directory = original_binary
        .parent()
        .expect("ilium binary should have a parent directory");
    let restartable_path_guard = tempfile::Builder::new()
        .prefix("ilium-client-restart-")
        .tempfile_in(binary_directory)
        .expect("reserve restartable binary path")
        .into_temp_path();
    std::fs::remove_file(&restartable_path_guard).expect("clear restartable binary path");
    std::fs::hard_link(&original_binary, &restartable_path_guard)
        .expect("hard-link restartable ilium binary");
    let restartable_binary = restartable_path_guard.to_path_buf();
    let restart_marker = temp_root.path().join("client-restarted.marker");

    let attach_command = PtyCommand::new(
        restartable_binary.to_string_lossy().to_string(),
        &project_dir,
        40,
        120,
    )
    .arg("--cwd")
    .arg(project_dir.to_string_lossy().to_string())
    .env(
        "ILIUM_RESTART_MARKER",
        restart_marker.to_string_lossy().to_string(),
    )
    .env(
        "ILIUM_RESTART_TARGET",
        original_binary.to_string_lossy().to_string(),
    );
    let attach_command = xdg
        .as_pairs()
        .into_iter()
        .fold(attach_command, |command, (key, value)| {
            command.env(key, value.to_string_lossy().to_string())
        });
    let mut tui = PtySession::spawn(attach_command).expect("spawn restartable client under PTY");
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                screen.contains(PROJECT_NAME) && screen.contains("default")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "expected initial client UI before restart, got: {:?}",
        tui.screen_text()
    );
    let client_process_id = tui.process_id().expect("client PTY should report a PID");
    // Asks `ilium_platform` rather than reading `/proc` directly: procfs does
    // not exist on macOS, where this assertion used to fail with NotFound.
    assert_eq!(
        ilium_platform::process_info::executable_path(client_process_id)
            .expect("read initial client executable"),
        restartable_binary
    );

    // GNU install-style replacement unlinks the running image and installs a
    // new directory entry. This shim records that the newly installed path was
    // executed, then hands control back to the real test binary with the exact
    // reconstructed project/session arguments.
    let replacement_path_guard = tempfile::Builder::new()
        .prefix("ilium-client-replacement-")
        .tempfile_in(binary_directory)
        .expect("reserve replacement client path")
        .into_temp_path();
    let replacement_binary = replacement_path_guard.to_path_buf();
    std::fs::write(
        &replacement_binary,
        "#!/bin/sh\nprintf 'reloaded\\n' > \"$ILIUM_RESTART_MARKER\"\nexec \"$ILIUM_RESTART_TARGET\" \"$@\"\n",
    )
    .expect("write replacement client shim");
    std::fs::set_permissions(&replacement_binary, std::fs::Permissions::from_mode(0o755))
        .expect("make replacement client shim executable");
    std::fs::rename(&replacement_binary, &restartable_binary)
        .expect("atomically replace running client path");

    let default_rows = tui.with_screen(|screen| rows_containing(screen, "default"));
    assert_eq!(default_rows.len(), 1, "expected one default-group row");
    let menu_column = 8;
    tui.write(&sgr_mouse_down(2, menu_column, default_rows[0]))
        .expect("right-click default group");
    assert!(
        wait_until(|| tui.screen_text().contains("Restart"), WAIT_TIMEOUT).await,
        "expected Restart in the real context menu, got: {:?}",
        tui.screen_text()
    );
    let restart_rows = tui.with_screen(|screen| rows_containing(screen, "Restart"));
    assert_eq!(restart_rows.len(), 1, "expected one rendered Restart row");
    tui.write(&sgr_mouse_down(0, menu_column + 1, restart_rows[0]))
        .expect("click Restart context action");

    assert!(
        wait_until(|| restart_marker.is_file(), WAIT_TIMEOUT).await,
        "replacement executable path was not loaded after Restart"
    );
    assert!(
        wait_until(
            || {
                ilium_platform::process_info::executable_path(client_process_id)
                    .is_some_and(|path| path == original_binary)
            },
            WAIT_TIMEOUT,
        )
        .await,
        "client PID should have exec'd the replacement target"
    );
    assert_eq!(
        tui.process_id(),
        Some(client_process_id),
        "exec should preserve the client PID while replacing its executable image"
    );
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                screen.contains(PROJECT_NAME)
                    && screen.contains("default")
                    && !screen.contains("Restart")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "restarted client did not redraw the attached session, got: {:?}",
        tui.screen_text()
    );

    let (server_socket_after, server_process_id_after) = isolated_server_identity(&xdg).await;
    assert_eq!(server_socket_after, server_socket_before);
    assert_eq!(server_process_id_after, server_process_id_before);

    // Expansion state is intentionally client-local and resets on restart;
    // expanding again must reveal the same server-owned terminal pane.
    tui.write(b"\x02t\x1b[B\x1b[C")
        .expect("focus tree and expand restored group");
    assert!(
        wait_until(|| tui.screen_text().contains("cat"), WAIT_TIMEOUT).await,
        "expected the server-owned cat pane after client restart, got: {:?}",
        tui.screen_text()
    );

    let kill_output = run_one_shot(&xdg, &project_dir, &["kill-session", SESSION_NAME]).await;
    assert!(kill_output.status.success(), "kill-session should succeed");
    cleanup_guard.already_cleaned_up = true;
    let exited = wait_until(|| tui.has_exited(), WAIT_TIMEOUT).await;
    if !exited {
        tui.kill().expect("force-kill restarted client");
    }
    assert!(exited, "restarted client should exit after session cleanup");
}

/// Drives split creation, multi-pane rendering, and focus-specific input
/// through the real CLI, detached server, IPC connection, PTYs, and TUI.
/// Unit tests pin exact rectangles; this test proves those layers remain
/// connected when two live terminal streams share the right panel.
#[cfg(unix)]
#[tokio::test]
async fn split_view_renders_two_live_panes_and_routes_input_to_each_active_slot() {
    let temp_root = tempfile::tempdir().expect("create tempdir");
    let xdg = IsolatedXdgDirs::under(temp_root.path()).expect("create isolated XDG dirs");
    let project_dir = temp_root.path().join("split-project");
    std::fs::create_dir_all(&project_dir).expect("create project dir");
    seed_project_config(&project_dir);
    let mut cleanup_guard = KillSessionOnDrop {
        xdg: &xdg,
        cwd: project_dir.clone(),
        session_name: SESSION_NAME,
        already_cleaned_up: false,
    };

    for pane_number in 1..=2 {
        let output = run_one_shot(&xdg, &project_dir, &["new-pane", "--", "cat"]).await;
        assert!(
            output.status.success(),
            "creating split fixture pane {pane_number} failed: stdout={:?} stderr={:?}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let attach_command = PtyCommand::new(ilium_binary(), &project_dir, 40, 120)
        .arg("--restart-server")
        .arg("--cwd")
        .arg(project_dir.to_string_lossy().to_string());
    let attach_command = xdg
        .as_pairs()
        .into_iter()
        .fold(attach_command, |command, (key, value)| {
            command.env(key, value.to_string_lossy().to_string())
        });
    let mut tui = PtySession::spawn(attach_command).expect("spawn split-view TUI under a PTY");

    let group_rendered = wait_until(|| tui.screen_text().contains("default"), WAIT_TIMEOUT).await;
    assert!(
        group_rendered,
        "default group did not render: {:?}",
        tui.screen_text()
    );

    // The project and its default group are restored expanded, so select the
    // default group directly before opening the split dialog.
    tui.write(b"\x01t").expect("focus tree");
    tui.write(b"\x1b[B").expect("select default group");
    let both_fixture_panes = wait_until(
        || tui.screen_text().matches("cat").count() >= 2,
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        both_fixture_panes,
        "two fixture panes did not render in the tree: {:?}",
        tui.screen_text()
    );
    tui.write(b"\x01W").expect("open split orientation dialog");
    let orientation_dialog = wait_until(
        || {
            let screen = tui.screen_text();
            screen.contains("New split view") && screen.contains("Vertical")
        },
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        orientation_dialog,
        "split orientation dialog did not render"
    );

    // Keep the default vertical orientation, select both eligible panes in
    // the optional checkbox dialog, and commit one atomic creation request.
    tui.write(b"\r").expect("continue to split member selector");
    let member_dialog = wait_until(
        || tui.screen_text().contains("Add panes to split"),
        WAIT_TIMEOUT,
    )
    .await;
    assert!(member_dialog, "split member dialog did not render");
    tui.write(b" ").expect("select first split pane");
    tui.write(b"\x1b[B").expect("select second split pane row");
    tui.write(b" ").expect("select second split pane");
    tui.write(b"\r").expect("create split view");
    let split_created = wait_until(
        || tui.screen_text().contains("Vertical split"),
        WAIT_TIMEOUT,
    )
    .await;
    assert!(split_created, "created split did not appear in the tree");

    // The group remains selected after the server snapshot. Move to its new
    // split child and display it; both live right-panel viewport titles must
    // be present even while the split row itself remains collapsed.
    tui.write(b"\x1b[B").expect("select split view");
    tui.write(b"\r").expect("display split view");
    let both_viewports = wait_until(
        || tui.screen_text().matches("cat").count() >= 2,
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        both_viewports,
        "two live split viewports did not render: {:?}",
        tui.screen_text()
    );

    // Expand the selected split before descending into its children. Select
    // each child through the tree and type a distinct marker. Since
    // both markers remain visible together, the active child receives input
    // while split presentation continues rendering every member.
    tui.write(b"\x1b[C").expect("expand selected split view");
    let split_children_visible = wait_until(
        || tui.screen_text().matches("cat").count() >= 2,
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        split_children_visible,
        "split child rows did not render: {:?}",
        tui.screen_text()
    );
    let split_child_rows =
        tui.with_screen(|screen| rows_containing_in_order(screen, &["📟", IDLE_PANE_LABEL]));
    assert_eq!(split_child_rows.len(), 2, "expected two split child rows");
    tui.write(&sgr_mouse_down(0, 8, split_child_rows[0]))
        .expect("focus first split child");
    tui.write(&sgr_mouse_up(8, split_child_rows[0]))
        .expect("release first split child click");
    tui.write(b"\r").expect("display first split child");
    tui.write(b"left-route\r")
        .expect("type into first split child");
    let first_routed = wait_until(|| tui.screen_text().contains("left-route"), WAIT_TIMEOUT).await;
    assert!(first_routed, "first split child did not receive input");

    tui.write(b"\x01t").expect("return focus to split tree");
    tui.write(&sgr_mouse_down(0, 8, split_child_rows[1]))
        .expect("focus second split child");
    tui.write(&sgr_mouse_up(8, split_child_rows[1]))
        .expect("release second split child click");
    tui.write(b"\r").expect("display second split child");
    tui.write(b"right-route\r")
        .expect("type into second split child");
    let both_routed = wait_until(
        || {
            let screen = tui.screen_text();
            screen.contains("left-route") && screen.contains("right-route")
        },
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        both_routed,
        "split input was not independently routed: {:?}",
        tui.screen_text()
    );

    // Attach a second real client to obtain stable ids from the authoritative
    // tree. This leaves the focused PTY client untouched while avoiding an
    // LLM/network dependency in a deterministic smoke test.
    let (socket_path, _) = isolated_server_identity(&xdg).await;
    let mut control_connection = Connection::connect(&socket_path, SESSION_NAME.to_string())
        .await
        .expect("attach restructure control connection");
    let tree_before_restructure =
        receive_tree_snapshot(&mut control_connection, "initial control attach").await;
    let project_ids = tree_before_restructure.project_ids();
    assert_eq!(project_ids.len(), 1, "expected one isolated project");
    let project_id = project_ids[0];
    let pane_ids = tree_before_restructure.pane_ids_in_tree_order();
    assert_eq!(pane_ids.len(), 2, "expected two split panes");
    let split_view_id = tree_before_restructure
        .parent_of(pane_ids[0])
        .expect("first pane should have a split parent");
    assert!(
        tree_before_restructure
            .get(split_view_id)
            .is_some_and(ilium_core::Node::is_split_view),
        "fixture panes should be inside a split view"
    );
    assert_eq!(
        tree_before_restructure.parent_of(pane_ids[1]),
        Some(split_view_id),
        "both fixture panes should share the split view"
    );
    let original_split_parent_id = tree_before_restructure
        .parent_of(split_view_id)
        .expect("split view should have a project-local parent");
    let original_orientation = tree_before_restructure
        .split_orientation(split_view_id)
        .expect("split view should expose its orientation");
    let original_split_children = tree_before_restructure
        .children_of(split_view_id)
        .expect("split view should expose its children")
        .to_vec();

    // Model output may rebuild ordinary groups and retitle panes, but it can
    // represent the user-owned split only by its existing id and exact child
    // order. Submitting this over IPC exercises the production server path.
    let retitled_split_children = original_split_children
        .iter()
        .enumerate()
        .map(|(index, pane_id)| RestructureNode::Pane {
            id: *pane_id,
            title: format!("AI pane {}", index + 1),
            short_title: None,
            icon: None,
        })
        .collect();
    let restructure_plan = RestructurePlan {
        children: vec![RestructureNode::Group {
            title: "AI regrouped work".to_string(),
            short_title: None,
            icon: None,
            children: vec![RestructureNode::ExistingSplitView {
                id: split_view_id,
                children: retitled_split_children,
            }],
        }],
    };
    let inference_activity_revisions = tree_before_restructure
        .project_activity_revisions(project_id)
        .expect("project activity revisions should be readable");
    control_connection
        .requests
        .send(ClientRequest::ApplyProjectRestructurePlan {
            project_id,
            plan: restructure_plan,
            inference_activity_revisions,
        })
        .await
        .expect("submit deterministic project restructure");
    let tree_after_restructure =
        receive_tree_snapshot(&mut control_connection, "project restructure").await;

    // A newly authored ordinary group now wraps the split, while every
    // split-owned property and pane identity remains stable across the
    // server mutation.
    let restructured_group_id = tree_after_restructure
        .parent_of(split_view_id)
        .expect("preserved split should have its restructured group parent");
    assert_ne!(
        restructured_group_id, original_split_parent_id,
        "AI-authored hierarchy should replace the split's old placement"
    );
    assert_eq!(
        tree_after_restructure
            .get(restructured_group_id)
            .expect("restructured group should exist")
            .name,
        "AI regrouped work"
    );
    assert_eq!(
        tree_after_restructure.split_orientation(split_view_id),
        Some(original_orientation),
        "split orientation should survive"
    );
    assert_eq!(
        tree_after_restructure
            .children_of(split_view_id)
            .expect("preserved split should expose its children"),
        original_split_children,
        "split membership and order should survive"
    );
    assert_eq!(
        tree_after_restructure
            .get(pane_ids[0])
            .expect("first pane should survive")
            .name,
        "AI pane 1"
    );
    assert_eq!(
        tree_after_restructure
            .get(pane_ids[1])
            .expect("second pane should survive")
            .name,
        "AI pane 2"
    );

    // Seeing both old terminal streams under the new titles proves the PTY
    // client consumed the snapshot without losing the displayed split. A new
    // marker then proves pane focus/input routing also survived rather than
    // being kicked to an empty right panel.
    let preserved_split_rendered = wait_until(
        || {
            let screen = tui.screen_text();
            screen.contains("AI pane 1")
                && screen.contains("AI pane 2")
                && screen.contains("left-route")
                && screen.contains("right-route")
        },
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        preserved_split_rendered,
        "focused split disappeared after restructure: {:?}",
        tui.screen_text()
    );
    tui.write(b"focus-survived\r")
        .expect("type into the still-focused split child");
    assert!(
        wait_until(
            || tui.screen_text().contains("focus-survived"),
            WAIT_TIMEOUT,
        )
        .await,
        "focused split child stopped receiving input after restructure: {:?}",
        tui.screen_text()
    );

    let kill_output = run_one_shot(&xdg, &project_dir, &["kill-session", SESSION_NAME]).await;
    assert!(
        kill_output.status.success(),
        "split test session cleanup failed"
    );
    cleanup_guard.already_cleaned_up = true;
    let exited = wait_until(|| tui.has_exited(), WAIT_TIMEOUT).await;
    if !exited {
        tui.kill().expect("force-kill split-view TUI");
    }
    assert!(exited, "split-view TUI did not exit after session cleanup");
}

/// Every 0-based row whose text contains `needle`, in top-to-bottom order
/// -- used to locate a specific pane's row in the rendered tree panel so
/// its cells can be inspected for the creation-pulse flash, which
/// `screen_text()`'s plain-text dump alone can't reveal (it drops all
/// styling). A whole terminal row spans both the tree and pane panels
/// side by side (e.g. `"│    > shell   │no pane selected...│"`), so this
/// looks for `needle` anywhere in the row rather than at its end. Built
/// from `Screen::rows` (one string per row, by construction) rather than
/// splitting `contents()` on newlines, so the returned index always
/// matches `Screen::cell`'s row argument exactly.
/// The last `count` rendered rows. The footer -- status bar and voice control
/// -- is the bottom row of the frame, so when an assertion about it fails, the
/// question is what is actually there, not what the other forty rows say.
fn bottom_rows(screen: &vt100::Screen, count: usize) -> Vec<String> {
    let (rows, columns) = screen.size();
    let all: Vec<String> = screen.rows(0, columns).collect();
    all.into_iter()
        .skip(usize::from(rows).saturating_sub(count))
        .collect()
}

fn rows_containing(screen: &vt100::Screen, needle: &str) -> Vec<u16> {
    let cols = screen.size().1;
    screen
        .rows(0, cols)
        .enumerate()
        .filter(|(_, text)| text.contains(needle))
        .map(|(index, _)| index as u16)
        .collect()
}

/// Finds text only inside the leftmost rendered cells. A full terminal row
/// also contains the right pane, whose title may repeat an agent label and
/// must never be mistaken for the corresponding tree row during mouse tests.
#[cfg(unix)]
fn rows_containing_before_column(
    screen: &vt100::Screen,
    needle: &str,
    column_limit: u16,
) -> Vec<u16> {
    let (rows, columns) = screen.size();
    let column_limit = column_limit.min(columns);
    (0..rows)
        .filter(|row| {
            let left_cells: String = (0..column_limit)
                .filter_map(|column| screen.cell(*row, column))
                .map(vt100::Cell::contents)
                .collect();
            left_cells.contains(needle)
        })
        .collect()
}

/// Returns terminal rows containing every needle from left to right. Exact
/// spaces are intentionally ignored because vt100 and real terminal emulators
/// can assign different presentation widths to VS16 emoji while preserving
/// the fixed cell coordinates ilium sent.
fn rows_containing_in_order(screen: &vt100::Screen, needles: &[&str]) -> Vec<u16> {
    let cols = screen.size().1;
    screen
        .rows(0, cols)
        .enumerate()
        .filter(|(_, text)| {
            let mut remainder = text.as_str();
            for needle in needles {
                let Some(offset) = remainder.find(needle) else {
                    return false;
                };
                remainder = &remainder[offset + needle.len()..];
            }
            true
        })
        .map(|(index, _)| index as u16)
        .collect()
}

/// Finds the first terminal cell whose grapheme payload contains `needle`.
/// Wide emoji can be stored as one multi-codepoint cell followed by a blank
/// continuation cell, so matching cell contents is more reliable than byte
/// offsets in a flattened screen row for mouse-coordinate assertions.
fn first_cell_containing(screen: &vt100::Screen, needle: &str) -> Option<(u16, u16)> {
    let (rows, columns) = screen.size();
    for row in 0..rows {
        for column in 0..columns {
            if screen
                .cell(row, column)
                .is_some_and(|cell| cell.contents().contains(needle))
            {
                return Some((column, row));
            }
        }
    }
    None
}

/// True if any cell in `row` is currently rendered in inverse video --
/// exactly what `ilium_client::tree_ui`'s creation-pulse flash
/// (`Modifier::REVERSED`, applied by `apply_recent_pulse`) produces on a
/// freshly created node's row.
#[cfg(unix)]
fn row_has_inverse_cell(screen: &vt100::Screen, row: u16) -> bool {
    let cols = screen.size().1;
    (0..cols).any(|col| screen.cell(row, col).is_some_and(vt100::Cell::inverse))
}

/// Encodes one xterm SGR mouse-button press using crossterm's expected
/// one-based wire coordinates. Button `0` is left and `2` is right.
fn sgr_mouse_down(button: u8, column: u16, row: u16) -> Vec<u8> {
    format!(
        "\x1b[<{button};{};{}M",
        column.saturating_add(1),
        row.saturating_add(1)
    )
    .into_bytes()
}

/// Encodes an xterm SGR left-button release. Tree selection starts drag
/// tracking on press, so click-only tests must release explicitly to clear
/// that state without accidentally carrying it into a later row action.
fn sgr_mouse_up(column: u16, row: u16) -> Vec<u8> {
    format!(
        "\x1b[<0;{};{}m",
        column.saturating_add(1),
        row.saturating_add(1)
    )
    .into_bytes()
}

/// Encodes xterm SGR pointer motion while the left button remains held.
/// Crossterm exposes this as `MouseEventKind::Drag(MouseButton::Left)`.
#[cfg(unix)]
fn sgr_mouse_drag(column: u16, row: u16) -> Vec<u8> {
    format!(
        "\x1b[<32;{};{}M",
        column.saturating_add(1),
        row.saturating_add(1)
    )
    .into_bytes()
}

/// Encodes pointer motion with no button held. Hover-only row actions are
/// driven by this exact xterm SGR event in the real TUI.
fn sgr_mouse_move(column: u16, row: u16) -> Vec<u8> {
    format!(
        "\x1b[<35;{};{}M",
        column.saturating_add(1),
        row.saturating_add(1)
    )
    .into_bytes()
}

/// Covers the feature this file's other test doesn't: a freshly created pane
/// must first settle from its insertion slide, then visibly flash (so a click
/// on the tree panel's creation toolbar is obviously followed by something
/// appearing -- see `ilium_client::tree_transitions` and
/// `ilium_client::tree_ui`'s `apply_recent_pulse`). The flash must fade once
/// its window elapses, including when several panes are created in one burst.
///
/// `Ctrl+A c` (`ilium_client::keymap::Action::NewTerminal`) drives exactly
/// the same `App::action_new_terminal` the tree panel's "new shell"
/// toolbar button (`TreeToolbarAction::Shell`) calls -- the pulse itself
/// lives entirely downstream of that call, in `render_cache`/`tree_ui`, so
/// this exercises the real end-to-end pipeline (keystroke -> server ->
/// tree snapshot -> render) without needing to reverse-engineer the
/// toolbar's exact pixel position at this pty's fixed size.
#[cfg(unix)]
#[tokio::test]
async fn newly_created_panes_flash_and_the_flash_fades_including_for_a_multi_create_burst() {
    let temp_root = tempfile::tempdir().expect("create tempdir");
    let xdg = IsolatedXdgDirs::under(temp_root.path()).expect("create isolated XDG dirs");
    let project_dir = temp_root.path().join("project");
    std::fs::create_dir_all(&project_dir).expect("create project dir");
    seed_project_config(&project_dir);
    // See `KillSessionOnDrop`'s doc comment for why this must be declared
    // after `temp_root` and before anything that can spawn the server.
    let mut cleanup_guard = KillSessionOnDrop {
        xdg: &xdg,
        cwd: project_dir.clone(),
        session_name: SESSION_NAME,
        already_cleaned_up: false,
    };

    let attach_command = PtyCommand::new(ilium_binary(), &project_dir, 40, 120)
        .arg("--cwd")
        .arg(project_dir.to_string_lossy().to_string());
    let attach_command = xdg
        .as_pairs()
        .into_iter()
        .fold(attach_command, |command, (key, value)| {
            command.env(key, value.to_string_lossy().to_string())
        });
    let mut tui = PtySession::spawn(attach_command).expect("spawning `ilium` under a pty");

    let rendered = wait_until(|| tui.screen_text().contains(PROJECT_NAME), WAIT_TIMEOUT).await;
    assert!(
        rendered,
        "expected the sidebar title on screen, got: {:?}",
        tui.screen_text()
    );

    // A "multi-create burst": two presses back-to-back with no wait in
    // between, so both `NewPane` requests are in flight (and very likely
    // land in the same or an immediately following tree snapshot) before
    // either pane's flash window has a chance to elapse.
    tui.write(b"\x01c")
        .expect("writing Ctrl+A then c (NewTerminal) once");
    tui.write(b"\x01c")
        .expect("writing Ctrl+A then c (NewTerminal) again");

    // Both requests create their pane under the tree's default group
    // (freshly created server-side by the first request, since this
    // session started with none) -- wait for it to actually appear before
    // navigating to it, since the tree-navigation keys below are no-ops
    // against a still-empty local tree mirror and would otherwise race
    // the `TreeSnapshot` these `NewPane` requests are still in flight for.
    let default_group_listed =
        wait_until(|| tui.screen_text().contains("default"), WAIT_TIMEOUT).await;
    assert!(
        default_group_listed,
        "expected the \"default\" group to appear after the create burst, got: {:?}",
        tui.screen_text()
    );

    // The restored project/default hierarchy is already expanded, so select
    // the default group without toggling it closed.
    tui.write(b"\x01t")
        .expect("writing Ctrl+A then t (FocusTree)");
    tui.write(b"\x1b[B").expect("writing Down arrow");

    // Wait for the spatial insertion transition itself to settle, not merely
    // for two partial labels to become visible while their rows are still
    // moving right. The creation pulse starts only after this condition can
    // become true, which pins the requested slide-then-blink sequence.
    let both_panes_listed = wait_until(
        || tui.with_screen(|screen| rows_containing_in_order(screen, &["📟", "shell"]).len() == 2),
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        both_panes_listed,
        "expected two \"shell\" pane rows after a two-press create burst, got: {:?}",
        tui.screen_text()
    );

    // A `PlainShell` pane row is `📟` plus either an empty activity slot or
    // its event-driven Angular frame, followed by the title. Match those
    // stable ordered fields rather than assuming the activity slot is idle.
    let rows = tui.with_screen(|screen| rows_containing_in_order(screen, &["📟", "shell"]));
    assert_eq!(
        rows.len(),
        2,
        "expected exactly two rows ending in \"shell\", got rows {rows:?} on screen: {:?}",
        tui.screen_text()
    );
    let (first_row, second_row) = (rows[0], rows[1]);

    // Each row must flash at some point inside its own flash window --
    // checked independently (not required to coincide on the same
    // instant), since the two panes' windows start a few milliseconds
    // apart and the flash itself toggles on/off every
    // `RECENTLY_CREATED_PULSE_PHASE_MS`.
    let first_flashed = wait_until(
        || tui.with_screen(|screen| row_has_inverse_cell(screen, first_row)),
        PULSE_WINDOW,
    )
    .await;
    assert!(
        first_flashed,
        "expected the first newly created pane's row ({first_row}) to flash, got: {:?}",
        tui.screen_text()
    );
    let second_flashed = wait_until(
        || tui.with_screen(|screen| row_has_inverse_cell(screen, second_row)),
        PULSE_WINDOW,
    )
    .await;
    assert!(
        second_flashed,
        "expected the second newly created pane's row ({second_row}) to flash too -- \
         a multi-create burst must flash every new pane, not just the first, got: {:?}",
        tui.screen_text()
    );

    // Once the flash window has fully elapsed (generous margin past
    // `PULSE_WINDOW` since the second pane's window starts slightly later
    // than the first's), the row must stop flashing for good -- a
    // deterministic check, not a poll, since an expired flash never
    // re-lights on any later phase.
    tokio::time::sleep(PULSE_WINDOW + Duration::from_millis(600)).await;
    let (first_still_flashing, second_still_flashing) = tui.with_screen(|screen| {
        (
            row_has_inverse_cell(screen, first_row),
            row_has_inverse_cell(screen, second_row),
        )
    });
    assert!(
        !first_still_flashing,
        "expected the first pane's flash to have faded by now, got: {:?}",
        tui.screen_text()
    );
    assert!(
        !second_still_flashing,
        "expected the second pane's flash to have faded by now, got: {:?}",
        tui.screen_text()
    );

    // The selected default group contains both panes, so Close opens the real
    // destructive confirmation. Cancel it with the rendered mouse button and
    // prove the obscured tree received no leaked click or close request.
    let default_row = tui.with_screen(|screen| rows_containing(screen, "default"))[0];
    tui.write(&sgr_mouse_down(0, 8, default_row))
        .expect("select the populated default group");
    tui.write(&sgr_mouse_up(8, default_row))
        .expect("release the populated default group click");
    tui.write(b"\x01x")
        .expect("open close confirmation for the populated default group");
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                screen.contains("Close this item?")
                    && screen.contains("Keep open")
                    && screen.contains("Close")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "expected aerated close confirmation buttons, got: {:?}",
        tui.screen_text()
    );
    let confirmation_layout = ilium_client::modal::confirm_dialog_layout_for_size(120, 40);
    let cancel_button = confirmation_layout.actions.cancel_button;
    tui.write(&sgr_mouse_down(
        0,
        cancel_button.x + cancel_button.width / 2,
        cancel_button.y,
    ))
    .expect("click Keep open in the real close dialog");
    assert!(
        wait_until(
            || !tui.screen_text().contains("Keep open") && tui.screen_text().contains("default"),
            WAIT_TIMEOUT,
        )
        .await,
        "mouse cancellation should preserve the default group, got: {:?}",
        tui.screen_text()
    );
    // The group-selection click above also toggled it closed. Expand it again
    // and prove both child panes survived the cancelled confirmation.
    tui.write(&sgr_mouse_down(0, 8, default_row))
        .expect("expand the preserved default group");
    tui.write(&sgr_mouse_up(8, default_row))
        .expect("release the preserved default group click");
    assert!(
        wait_until(
            || {
                tui.with_screen(|screen| {
                    rows_containing_in_order(screen, &["📟", "shell"]).len() == 2
                })
            },
            WAIT_TIMEOUT,
        )
        .await,
        "mouse cancellation should preserve both panes, got: {:?}",
        tui.screen_text()
    );

    // Select the first pane and close it through the real leader action. While
    // the authoritative tree already contains only one pane, the old snapshot
    // should remain on screen briefly with one of the two labels translated
    // left. Two names plus only one settled fixed-width label distinguishes
    // that exit frame from both the pre-close and post-transition states.
    tui.write(b"\x1b[B\x1b[B")
        .expect("selecting the first pane row below the default group");
    tui.write(b"\x01x")
        .expect("writing Ctrl+A then x (ClosePane)");
    let removal_motion_observed = wait_for_transient_frame(
        || {
            tui.screen_text().matches("shell").count() >= 3
                && tui.with_screen(|screen| {
                    rows_containing_in_order(screen, &["📟", "shell"]).len() == 1
                })
        },
        Duration::from_millis(500),
    )
    .await;
    assert!(
        removal_motion_observed,
        "expected one departing pane label to slide left before disappearing, got: {:?}",
        tui.screen_text()
    );
    let transition_duration_ms =
        u64::try_from(ilium_client::tree_transitions::TREE_ENTRY_TRANSITION_MS)
            .expect("tree-entry transition duration should fit u64");
    tokio::time::sleep(Duration::from_millis(transition_duration_ms + 100)).await;
    let remaining_pane_rows =
        tui.with_screen(|screen| rows_containing_in_order(screen, &["📟", "shell"]).len());
    assert_eq!(
        remaining_pane_rows,
        1,
        "expected the departing pane row to disappear after its exit transition, got: {:?}",
        tui.screen_text()
    );

    // Closing the selected first pane must also activate the surviving row
    // below it. Input now goes straight to that pane without another tree
    // click or Enter, proving selection and right-panel routing together.
    tui.write(b"printf 'close-successor-active\\n'\r")
        .expect("writing into the automatically activated successor pane");
    let successor_received_input = wait_until(
        || tui.screen_text().contains("close-successor-active"),
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        successor_received_input,
        "expected input to reach the successor selected after close, got: {:?}",
        tui.screen_text()
    );

    // Return to the tree, select the remaining pane's parent, then confirm its
    // destructive close with the other rendered button. This complements the
    // cancellation proof above and exercises both mouse outcomes end to end.
    let default_row = tui.with_screen(|screen| rows_containing(screen, "default"))[0];
    tui.write(&sgr_mouse_down(0, 8, default_row))
        .expect("select the final populated default group");
    tui.write(&sgr_mouse_up(8, default_row))
        .expect("release the final populated default group click");
    tui.write(b"\x01x")
        .expect("open close confirmation for the final populated group");
    assert!(
        wait_until(|| tui.screen_text().contains("Keep open"), WAIT_TIMEOUT).await,
        "expected the close confirmation before clicking Close, got: {:?}",
        tui.screen_text()
    );
    let close_button = confirmation_layout.actions.confirm_button;
    tui.write(&sgr_mouse_down(
        0,
        close_button.x + close_button.width / 2,
        close_button.y,
    ))
    .expect("click Close in the real close dialog");
    assert!(
        wait_until(
            || {
                !tui.screen_text().contains("Keep open")
                    && tui.with_screen(|screen| {
                        rows_containing_in_order(screen, &["📟", "shell"]).is_empty()
                    })
            },
            WAIT_TIMEOUT,
        )
        .await,
        "mouse confirmation should close the populated group, got: {:?}",
        tui.screen_text()
    );

    let kill_output = run_one_shot(&xdg, &project_dir, &["kill-session", SESSION_NAME]).await;
    assert!(
        kill_output.status.success(),
        "`ilium kill-session {SESSION_NAME}` failed: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&kill_output.stdout),
        String::from_utf8_lossy(&kill_output.stderr)
    );
    // The explicit, awaited cleanup above already succeeded -- the
    // drop-time guard no longer has anything to do.
    cleanup_guard.already_cleaned_up = true;

    let attached_process_exited = wait_until(|| tui.has_exited(), WAIT_TIMEOUT).await;
    if !attached_process_exited {
        tui.kill()
            .expect("force-killing the pty-attached ilium process");
    }
    assert!(
        attached_process_exited,
        "the pty-attached `ilium` process should exit on its own once `kill-session` \
         closes the connection, not need a force kill"
    );
}

/// Drives the full editor-line workflow through the real TUI and detached
/// server. A fake `codex` executable records stdin locally, so this proves the
/// modal's selected agent, generated prompt, and final Enter reached the live
/// child process without invoking any real installed agent or network access.
#[cfg(unix)]
#[tokio::test]
async fn editor_line_context_menu_creates_selected_agent_and_submits_the_prompt() {
    use std::os::unix::fs::PermissionsExt;

    let temp_root = tempfile::tempdir().expect("create tempdir");
    let xdg = IsolatedXdgDirs::under(temp_root.path()).expect("create isolated XDG dirs");
    let project_dir = temp_root.path().join("project");
    std::fs::create_dir_all(&project_dir).expect("create project dir");
    seed_project_config(&project_dir);
    let source_path = project_dir.join("task.txt");
    std::fs::write(
        &source_path,
        "first line\nCREATE_AGENT_TARGET_LINE\nlast line\n",
    )
    .expect("write source file");

    let fake_bin_dir = temp_root.path().join("fake-bin");
    std::fs::create_dir_all(&fake_bin_dir).expect("create fake bin dir");
    let fake_codex_path = fake_bin_dir.join("codex");
    let fake_output_path = temp_root.path().join("fake-codex-input.txt");
    std::fs::write(
        &fake_codex_path,
        format!(
            "#!/bin/sh\nprintf 'STARTED' > '{}'\nprintf '  send a message\\n'\nIFS= read -r line\nprintf '%s' \"$line\" > '{}'\nsleep 30\n",
            fake_output_path.display(),
            fake_output_path.display(),
        ),
    )
    .expect("write fake codex executable");
    let mut permissions = std::fs::metadata(&fake_codex_path)
        .expect("stat fake codex executable")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&fake_codex_path, permissions).expect("make fake codex executable");
    let fake_shell_path = fake_bin_dir.join("test-shell");
    std::fs::write(
        &fake_shell_path,
        format!(
            "#!/bin/sh\nif [ \"$1\" = '-c' ] && [ \"$2\" = 'codex' ]; then exec '{}'; fi\nexec /bin/sh \"$@\"\n",
            fake_codex_path.display()
        ),
    )
    .expect("write fake shell executable");
    let mut permissions = std::fs::metadata(&fake_shell_path)
        .expect("stat fake shell executable")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&fake_shell_path, permissions).expect("make fake shell executable");
    let mut cleanup_guard = KillSessionOnDrop {
        xdg: &xdg,
        cwd: project_dir.clone(),
        session_name: SESSION_NAME,
        already_cleaned_up: false,
    };
    let inherited_path = std::env::var("PATH").unwrap_or_default();
    let fake_path = format!("{}:{inherited_path}", fake_bin_dir.display());
    let attach_command = PtyCommand::new(ilium_binary(), &project_dir, 40, 120)
        .arg("--cwd")
        .arg(project_dir.to_string_lossy().to_string())
        .env("PATH", fake_path)
        .env("SHELL", fake_shell_path.to_string_lossy().to_string());
    let attach_command = xdg
        .as_pairs()
        .into_iter()
        .fold(attach_command, |command, (key, value)| {
            command.env(key, value.to_string_lossy().to_string())
        });
    let mut tui = PtySession::spawn(attach_command).expect("spawn ilium under a pty");

    assert!(
        wait_until(|| tui.screen_text().contains(PROJECT_NAME), WAIT_TIMEOUT).await,
        "expected initial TUI frame, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"\x01e").expect("open editor file picker");
    assert!(
        wait_until(|| tui.screen_text().contains("task.txt"), WAIT_TIMEOUT).await,
        "expected task.txt in file picker, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"\x1b[B\r")
        .expect("select task.txt below the parent entry");
    assert!(
        wait_until(
            || tui.screen_text().contains("CREATE_AGENT_TARGET_LINE"),
            WAIT_TIMEOUT,
        )
        .await,
        "expected source file in editor, got: {:?}",
        tui.screen_text()
    );

    let target_rows = tui.with_screen(|screen| rows_containing(screen, "CREATE_AGENT_TARGET_LINE"));
    assert_eq!(
        target_rows.len(),
        1,
        "expected one visible target source line"
    );
    let target_row = target_rows[0];
    let context_column = 70;
    tui.write(&sgr_mouse_down(2, context_column, target_row))
        .expect("right-click target source line");
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                screen.contains("Line actions") && screen.contains("Create agent from line")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "expected editor line context menu, got: {:?}",
        tui.screen_text()
    );

    // The menu renders its first item directly below its title. Create Agent
    // is the third canonical line action after Copy line and Copy entire file.
    tui.write(&sgr_mouse_down(0, context_column + 1, target_row + 3))
        .expect("click create-agent line action");
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                screen.contains("Create agent from line")
                    && screen.contains("Claude")
                    && screen.contains("Codex")
                    && screen.contains("Task prompt")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "expected create-agent modal, got: {:?}",
        tui.screen_text()
    );

    let dialog = ilium_client::agent_from_line::dialog_layout_for_size(120, 40);
    tui.write(&sgr_mouse_down(
        0,
        dialog.agent_row.x + 24,
        dialog.agent_row.y,
    ))
    .expect("select Codex in the modal");
    assert!(
        wait_until(|| tui.screen_text().contains("(●) Codex"), WAIT_TIMEOUT).await,
        "expected Codex selection, got: {:?}",
        tui.screen_text()
    );
    tui.write(&sgr_mouse_down(
        0,
        dialog.create_button.x + dialog.create_button.width / 2,
        dialog.create_button.y,
    ))
    .expect("click Create agent");

    let prompt_submitted = wait_until(
        || {
            std::fs::read_to_string(&fake_output_path).is_ok_and(|input| {
                input.starts_with("/goal please do the following task:")
                    && input.contains("CREATE_AGENT_TARGET_LINE")
                    && input.contains(&source_path.display().to_string())
                    && input.contains("at line 2")
            })
        },
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        prompt_submitted,
        "fake Codex should receive the generated prompt plus Enter; recorded={:?}, screen={:?}",
        std::fs::read_to_string(&fake_output_path),
        tui.screen_text()
    );

    let kill_output = run_one_shot(&xdg, &project_dir, &["kill-session", SESSION_NAME]).await;
    assert!(kill_output.status.success(), "kill-session should succeed");
    cleanup_guard.already_cleaned_up = true;
    let exited = wait_until(|| tui.has_exited(), WAIT_TIMEOUT).await;
    if !exited {
        tui.kill().expect("force-kill create-agent TUI");
    }
    assert!(
        exited,
        "create-agent TUI did not exit after session cleanup"
    );
}

/// Proves both user-facing existing-Markdown entry points through the real
/// mouse/keyboard TUI and detached server: a Markdown editor row's context
/// action and the generic New board dialog's file picker. The source files
/// use ordinary todo syntax (`#` plus `* [ ]`) rather than only ilium's
/// canonical writer syntax, and creation must leave both files untouched.
#[cfg(unix)]
#[tokio::test]
async fn existing_markdown_creates_populated_boards_from_tree_and_dialog() {
    let temp_root = tempfile::tempdir().expect("create tempdir");
    let xdg = IsolatedXdgDirs::under(temp_root.path()).expect("create isolated XDG dirs");
    let project_dir = temp_root.path().join("board-project");
    std::fs::create_dir_all(&project_dir).expect("create board project dir");
    seed_project_config(&project_dir);
    let context_source = "# Context column\n\n* [ ] Context task\n\n## Queue one\n\n## Queue two\n\n## Queue three\n\n## Queue four\n\n## Queue five\n";
    let context_path = project_dir.join("context.md");
    std::fs::write(&context_path, context_source).expect("write context Markdown");
    let detail_lines = (0..60)
        .map(|index| match index {
            0 => "DETAIL TOP".to_string(),
            59 => "DETAIL BOTTOM".to_string(),
            _ => format!("Detail line {index:02}"),
        })
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let dialog_source = format!("# Dialog column\n\n* [ ] Dialog task\n{detail_lines}\n");
    let dialog_path = project_dir.join("dialog.md");
    std::fs::write(&dialog_path, &dialog_source).expect("write dialog Markdown");
    let mut cleanup_guard = KillSessionOnDrop {
        xdg: &xdg,
        cwd: project_dir.clone(),
        session_name: SESSION_NAME,
        already_cleaned_up: false,
    };

    let attach_command = PtyCommand::new(ilium_binary(), &project_dir, 40, 120)
        .arg("--cwd")
        .arg(project_dir.to_string_lossy().to_string());
    let attach_command = xdg
        .as_pairs()
        .into_iter()
        .fold(attach_command, |command, (key, value)| {
            command.env(key, value.to_string_lossy().to_string())
        });
    let mut tui = PtySession::spawn(attach_command).expect("spawn board TUI under a PTY");
    assert!(
        wait_until(|| tui.screen_text().contains(PROJECT_NAME), WAIT_TIMEOUT).await,
        "expected initial board TUI frame, got: {:?}",
        tui.screen_text()
    );

    // Open context.md as an editor, then use the general board dialog to bind
    // a board to the same existing Markdown document.
    tui.write(b"\x01e").expect("open editor file picker");
    assert!(
        wait_until(|| tui.screen_text().contains("context.md"), WAIT_TIMEOUT).await,
        "expected Markdown files in editor picker, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"\x1b[B\r")
        .expect("select context.md below the parent entry");
    assert!(
        wait_until(|| tui.screen_text().contains("Context task"), WAIT_TIMEOUT).await,
        "expected context.md in editor, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"\x01B")
        .expect("open context board creation dialog");
    assert!(
        wait_until(|| tui.screen_text().contains("New board"), WAIT_TIMEOUT).await,
        "expected board creation dialog, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"\x7f\x7f\x7f\x7f\x7fContext\x10")
        .expect("name context board and open its path picker");
    assert!(
        wait_until(|| tui.screen_text().contains("context.md"), WAIT_TIMEOUT).await,
        "expected context.md in board path picker, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"\x1b[B\r\r")
        .expect("select context.md and create its board");
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                // The board renderer substitutes the fill space of an unchecked
                // checkbox with a non-breaking space (see `atomic_checkbox_title`
                // in board_ui.rs) so wrapping can never split "[ ]" mid-marker.
                screen.contains("Context column")
                    && screen.contains("[\u{a0}] Context task")
                    && screen.contains("drop a card here")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "expected populated context-backed board, got: {:?}",
        tui.screen_text()
    );

    // Six columns cannot fit at the default 20-cell minimum. The board must
    // expose a horizontal scrollbar and follow keyboard selection to columns
    // outside the first page without narrowing the visible columns.
    assert!(
        wait_until(|| tui.screen_text().contains('▶'), WAIT_TIMEOUT).await,
        "expected horizontal board scrollbar, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"\x1b[C\x1b[C\x1b[C\x1b[C\x1b[C")
        .expect("navigate to the last context-board column");
    assert!(
        wait_until(|| tui.screen_text().contains("Queue five"), WAIT_TIMEOUT).await,
        "selected off-page column should scroll into view, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"\x1b[D\x1b[D\x1b[D\x1b[D\x1b[D")
        .expect("navigate back to the first context-board column");
    assert!(
        wait_until(
            || tui.screen_text().contains("Context column"),
            WAIT_TIMEOUT
        )
        .await,
        "first column should scroll back into view, got: {:?}",
        tui.screen_text()
    );

    // Cards use border state for selection and do not repeat generic `card`
    // or `selected` titles along their top edge.
    let context_card_row =
        tui.with_screen(|screen| rows_containing(screen, "[\u{a0}] Context task")[0]);
    let context_card_border = tui.with_screen(|screen| {
        let border_row = context_card_row.saturating_sub(1);
        let columns = screen.size().1;
        let border_start = (25..columns)
            .find(|column| {
                screen
                    .cell(border_row, *column)
                    .is_some_and(|cell| cell.contents() == "┌")
            })
            .expect("find Context task card border");
        (border_start..border_start.saturating_add(20).min(columns))
            .filter_map(|column| screen.cell(border_row, column))
            .map(|cell| cell.contents())
            .collect::<String>()
            .to_lowercase()
    });
    assert!(!context_card_border.contains(" card "));
    assert!(!context_card_border.contains(" selected "));

    // A direct click on the rendered task marker toggles it and commits the
    // Markdown document before any later input event.
    let context_checkbox_column = tui.with_screen(|screen| {
        let columns = screen.size().1;
        (25..columns)
            .find(|column| {
                screen
                    .cell(context_card_row, *column)
                    .is_some_and(|cell| cell.contents() == "[")
            })
            .expect("find Context task checkbox")
    });
    tui.write(&sgr_mouse_down(
        0,
        context_checkbox_column + 1,
        context_card_row,
    ))
    .expect("click Context task checkbox");
    tui.write(&sgr_mouse_up(context_checkbox_column + 1, context_card_row))
        .expect("release Context task checkbox");
    assert!(
        wait_until(
            || std::fs::read_to_string(&context_path)
                .is_ok_and(|source| source.contains("- [x] Context task")),
            WAIT_TIMEOUT,
        )
        .await,
        "checkbox click should save immediately"
    );

    // Persistence and PTY redraw travel through separate observers. Wait for
    // the checked marker to reach the captured screen before resolving its
    // click coordinates instead of indexing an as-yet-unread frame.
    assert!(
        wait_until(
            || tui.screen_text().contains("[x] Context task"),
            WAIT_TIMEOUT,
        )
        .await,
        "checked Context task should redraw, got: {:?}",
        tui.screen_text()
    );

    // A complete click on the remaining card surface opens the editable
    // title/body panel in the rightmost third.
    let (context_card_column, context_card_row) = tui.with_screen(|screen| {
        let row = rows_containing(screen, "[x] Context task")[0];
        let columns = screen.size().1;
        let column = (25..columns)
            .find(|column| {
                screen
                    .cell(row, *column)
                    .is_some_and(|cell| cell.contents() == "C")
            })
            .expect("find Context task card column");
        (column, row)
    });
    tui.write(&sgr_mouse_down(0, context_card_column, context_card_row))
        .expect("press Context task card");
    tui.write(&sgr_mouse_up(context_card_column, context_card_row))
        .expect("release Context task card");
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                screen.contains("Card details")
                    && screen.contains("Title")
                    && screen.contains("Notes")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "expected click-open card detail panel, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"!").expect("append to card title");
    assert!(
        wait_until(
            || std::fs::read_to_string(&context_path)
                .is_ok_and(|source| source.contains("- [x] Context task!")),
            WAIT_TIMEOUT,
        )
        .await,
        "title keystroke should save immediately"
    );
    tui.write(b"\tLive note")
        .expect("switch to Notes and type a body");
    assert!(
        wait_until(
            || std::fs::read_to_string(&context_path)
                .is_ok_and(|source| source.contains("  Live note")),
            WAIT_TIMEOUT,
        )
        .await,
        "body keystrokes should save immediately"
    );
    tui.write(b"\x1b").expect("close card details with Escape");
    assert!(
        wait_until(|| !tui.screen_text().contains("Card details"), WAIT_TIMEOUT).await,
        "expected card detail panel to close, got: {:?}",
        tui.screen_text()
    );

    // The generic New board path must make the same adapter decision. Its
    // picker starts on `..`; context.md is first and dialog.md second, so two
    // Down events select dialog.md before Enter returns to the create form.
    tui.write(b"\x01B").expect("open New board dialog");
    assert!(
        wait_until(|| tui.screen_text().contains("New board"), WAIT_TIMEOUT).await,
        "expected New board dialog, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"p")
        .expect("type a literal p into the board name");
    assert!(
        wait_until(|| tui.screen_text().contains("Boardp"), WAIT_TIMEOUT).await,
        "plain p should edit the board name instead of opening Browse, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"\x7f\x10")
        .expect("restore the name and open board path picker with Ctrl+P");
    assert!(
        wait_until(|| tui.screen_text().contains("dialog.md"), WAIT_TIMEOUT).await,
        "expected dialog.md in board path picker, got: {:?}",
        tui.screen_text()
    );
    // The two key presses below assume the listing is exactly `..`,
    // `context.md`, `dialog.md`. Asserting that first means a platform which
    // surfaces an extra entry reports what it actually showed, instead of
    // silently selecting the wrong file and failing later on the create form.
    let picker_rows = tui.with_screen(|screen| {
        let cols = screen.size().1;
        screen
            .rows(0, cols)
            .filter(|row| row.contains(".md") || row.contains(".."))
            .map(|row| row.trim().to_string())
            .collect::<Vec<_>>()
    });
    // Matched on the picker's own file rows. A bare name would also hit the
    // sidebar, which lists `context.md` as an open pane well above the dialog.
    let dialog_below_context = tui.with_screen(|screen| {
        let context_row = rows_containing_in_order(screen, &["\u{1f4c4} context.md"]);
        let dialog_row = rows_containing_in_order(screen, &["\u{1f4c4} dialog.md"]);
        match (context_row.first(), dialog_row.first()) {
            (Some(context), Some(dialog)) => *dialog == context + 1,
            _ => false,
        }
    });
    assert!(
        dialog_below_context,
        "board path picker should list dialog.md directly below context.md, rows were: {picker_rows:#?}"
    );
    tui.write(b"\x1b[B\x1b[B\r")
        .expect("select dialog.md below parent and context.md");
    // The form must come back carrying a storage path, but the *filename* is
    // deliberately not required on screen: the field clips its value to the
    // widget's width, so whether `dialog.md` survives that clip depends on how
    // long the platform's temporary root happens to be -- macOS's
    // `/private/var/folders/<...>/T` fills the field before reaching the file
    // name where Linux's `/tmp` does not. What was actually selected is proven
    // below, by the board this form goes on to create.
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                screen.contains("New board") && screen.contains("Storage path")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "expected the create form to return with a storage path, got: {:?}",
        tui.screen_text()
    );
    let board_dialog = ilium_client::modal::create_board_dialog_layout_for_size(120, 40);
    let create_board_button = board_dialog.actions.confirm_button;
    tui.write(&sgr_mouse_down(
        0,
        create_board_button.x + create_board_button.width / 2,
        create_board_button.y,
    ))
    .expect("click Create board in the restored creation form");
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                screen.contains("Dialog column") && screen.contains("[\u{a0}] Dialog task")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "expected populated dialog-backed board, got: {:?}",
        tui.screen_text()
    );
    let edited_context_source = std::fs::read_to_string(&context_path).unwrap();
    assert!(edited_context_source.contains("- [x] Context task!"));
    assert!(edited_context_source.contains("  Live note"));
    assert_eq!(
        std::fs::read_to_string(&dialog_path).unwrap(),
        dialog_source,
        "dialog creation must not rewrite existing Markdown"
    );

    // Keyboard selection and Enter open the shared one-third detail editor.
    // Long notes begin at their top and Esc restores the full board width.
    tui.write(b"\x1b[B\r")
        .expect("select Dialog task and open its details");
    assert!(
        wait_until(|| tui.screen_text().contains("DETAIL TOP"), WAIT_TIMEOUT).await,
        "expected top of card details, got: {:?}",
        tui.screen_text()
    );
    assert!(
        !tui.screen_text().contains("DETAIL BOTTOM"),
        "detail bottom should begin below the visible panel"
    );
    tui.write(b"\x1b").expect("close card details with Esc");
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                !screen.contains("DETAIL TOP") && !screen.contains("DETAIL BOTTOM")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "detail body should disappear after Esc, got: {:?}",
        tui.screen_text()
    );
    // Exercise the complete file-backed mutation path. The imported board
    // starts with its column header selected, `n` adds a second card, and a
    // real mouse drag must expose feedback before persisting its reorder.
    tui.write(b"n").expect("open New card prompt");
    assert!(
        wait_until(|| tui.screen_text().contains("New card"), WAIT_TIMEOUT).await,
        "expected New card prompt, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"Second task\r").expect("add second board card");
    assert!(
        wait_until(
            || std::fs::read_to_string(&dialog_path)
                .is_ok_and(|source| source.contains("- Second task")),
            WAIT_TIMEOUT,
        )
        .await,
        "second card should persist to Markdown"
    );
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                screen.contains("Second task") && screen.contains("[\u{a0}] Dialog task")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "persisted cards should render before their rows are used for mouse input, got: {:?}",
        tui.screen_text()
    );
    let (second_task_column, second_task_row, dialog_task_row) = tui.with_screen(|screen| {
        let second_task_row = rows_containing(screen, "Second task")[0];
        let dialog_task_row = rows_containing(screen, "[\u{a0}] Dialog task")[0];
        let columns = screen.size().1;
        let second_task_column = (25..columns)
            .find(|column| {
                screen
                    .cell(second_task_row, *column)
                    .is_some_and(|cell| cell.contents() == "S")
            })
            .expect("find Second task text");
        (second_task_column, second_task_row, dialog_task_row)
    });
    tui.write(&sgr_mouse_down(0, second_task_column, second_task_row))
        .expect("press Second task for drag");
    tui.write(&sgr_mouse_drag(second_task_column, dialog_task_row))
        .expect("drag Second task above Dialog task");
    assert!(
        wait_until(|| tui.screen_text().contains('━'), WAIT_TIMEOUT).await,
        "active card drag should show a visible insertion line, got: {:?}",
        tui.screen_text()
    );
    tui.write(&sgr_mouse_up(second_task_column, dialog_task_row))
        .expect("drop Second task above Dialog task");
    assert!(
        wait_until(
            || std::fs::read_to_string(&dialog_path).is_ok_and(|source| {
                source.contains("## Dialog column\n- Second task\n- [ ] Dialog task")
            }),
            WAIT_TIMEOUT,
        )
        .await,
        "mouse drop should reorder cards in the Markdown file"
    );

    // Add a destination column, move the selected card across, then navigate
    // from its first card back to the explicit column-header selection and
    // rename that populated column. This was impossible when selection used
    // an implicit always-present card index.
    tui.write(b"cDone\r").expect("add Done column");
    assert!(
        wait_until(|| tui.screen_text().contains("Done 0"), WAIT_TIMEOUT).await,
        "expected empty Done column, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"\x1b[D\x1b[B\x1b[1;2C")
        .expect("select first card and move it to Done");
    assert!(
        wait_until(
            || std::fs::read_to_string(&dialog_path).is_ok_and(|source| {
                source.contains("## Dialog column\n- [ ] Dialog task")
                    && source.contains("## Done\n- Second task")
            }),
            WAIT_TIMEOUT,
        )
        .await,
        "Shift+Right should move the card across columns"
    );
    tui.write(b"\x1b[Ae\x7f\x7f\x7f\x7fCompleted\r")
        .expect("select and rename the populated Done column");
    assert!(
        wait_until(
            || std::fs::read_to_string(&dialog_path)
                .is_ok_and(|source| source.contains("## Completed\n- Second task")),
            WAIT_TIMEOUT,
        )
        .await,
        "populated column rename should persist"
    );

    // An out-of-band edit must be preserved. The stale local mutation is
    // rejected and rolled back; `r` then adopts the external revision so a
    // later edit can commit normally.
    let external_source = std::fs::read_to_string(&dialog_path)
        .unwrap()
        .replace("- Second task\n", "- Second task\n- External task\n");
    std::fs::write(&dialog_path, &external_source).expect("write external board revision");
    tui.write(b"nStale task\r")
        .expect("attempt mutation from stale board state");
    assert!(
        wait_until(
            || tui
                .screen_text()
                .contains("press r to reload before editing"),
            WAIT_TIMEOUT,
        )
        .await,
        "expected stale-write guidance, got: {:?}",
        tui.screen_text()
    );
    assert_eq!(
        std::fs::read_to_string(&dialog_path).unwrap(),
        external_source,
        "stale board mutation must not overwrite the external edit"
    );
    tui.write(b"r\x1b[CnReloaded task\r")
        .expect("reload external revision and commit a new card");
    assert!(
        wait_until(
            || std::fs::read_to_string(&dialog_path)
                .is_ok_and(|source| source.contains("- Reloaded task")),
            WAIT_TIMEOUT,
        )
        .await,
        "reloaded board should accept a new persisted mutation"
    );

    // Detach this client and attach a fresh one to the same detached server.
    // The new client must hydrate the board from the final Markdown state.
    tui.write(b"\x01d").expect("detach first board client");
    assert!(
        wait_until(|| tui.has_exited(), WAIT_TIMEOUT).await,
        "first board client should exit after detach"
    );
    let reattach_command = PtyCommand::new(ilium_binary(), &project_dir, 40, 120)
        .arg("--cwd")
        .arg(project_dir.to_string_lossy().to_string());
    let reattach_command = xdg
        .as_pairs()
        .into_iter()
        .fold(reattach_command, |command, (key, value)| {
            command.env(key, value.to_string_lossy().to_string())
        });
    let mut tui = PtySession::spawn(reattach_command).expect("reattach board TUI under a PTY");
    assert!(
        wait_until(|| tui.screen_text().contains("▦    Board"), WAIT_TIMEOUT).await,
        "fresh client should list the dialog-backed board, got: {:?}",
        tui.screen_text()
    );
    let dialog_board_rows = tui.with_screen(|screen| rows_containing(screen, "▦    Board"));
    assert_eq!(dialog_board_rows.len(), 1);
    tui.write(&sgr_mouse_down(0, 8, dialog_board_rows[0]))
        .expect("focus dialog-backed board after reattach");
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                screen.contains("Completed")
                    && screen.contains("External task")
                    && screen.contains("Reloaded task")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "fresh client should hydrate final board state, got: {:?}",
        tui.screen_text()
    );

    let kill_output = run_one_shot(&xdg, &project_dir, &["kill-session", SESSION_NAME]).await;
    assert!(kill_output.status.success(), "kill-session should succeed");
    cleanup_guard.already_cleaned_up = true;
    let exited = wait_until(|| tui.has_exited(), WAIT_TIMEOUT).await;
    if !exited {
        tui.kill().expect("force-kill board TUI");
    }
    assert!(exited, "board TUI did not exit after session cleanup");
}

/// Drives the terminal-row context action, complete scheduling dialog,
/// countdown rendering, and delayed PTY delivery through the real TUI and
/// detached server. The live child is `cat`, so the submitted marker can be
/// observed only after the server writes the scheduled text plus Enter.
#[cfg(unix)]
#[tokio::test]
async fn terminal_context_menu_schedules_countdown_and_delivers_input() {
    let temp_root = tempfile::tempdir().expect("create tempdir");
    let xdg = IsolatedXdgDirs::under(temp_root.path()).expect("create isolated XDG dirs");
    let project_dir = temp_root.path().join("scheduled-input-project");
    std::fs::create_dir_all(&project_dir).expect("create project dir");
    seed_project_config(&project_dir);
    let mut cleanup_guard = KillSessionOnDrop {
        xdg: &xdg,
        cwd: project_dir.clone(),
        session_name: SESSION_NAME,
        already_cleaned_up: false,
    };

    // Create a deterministic terminal whose stdin is echoed back to its
    // viewport, then attach the real TUI to the same isolated session.
    let new_pane_output = run_one_shot(&xdg, &project_dir, &["new-pane", "--", "cat"]).await;
    assert!(
        new_pane_output.status.success(),
        "creating scheduled-input fixture pane failed: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&new_pane_output.stdout),
        String::from_utf8_lossy(&new_pane_output.stderr)
    );
    let attach_command = PtyCommand::new(ilium_binary(), &project_dir, 40, 120)
        .arg("--restart-server")
        .arg("--cwd")
        .arg(project_dir.to_string_lossy().to_string());
    let attach_command = xdg
        .as_pairs()
        .into_iter()
        .fold(attach_command, |command, (key, value)| {
            command.env(key, value.to_string_lossy().to_string())
        });
    let mut tui = PtySession::spawn(attach_command).expect("spawn scheduled-input TUI under PTY");

    assert!(
        wait_until(|| tui.screen_text().contains("default"), WAIT_TIMEOUT).await,
        "expected default group in scheduled-input TUI, got: {:?}",
        tui.screen_text()
    );

    // The restored default group is already expanded. Locate the real pane
    // row and use its context menu directly, avoiding unrelated selection
    // state while verifying scheduling.
    assert!(
        wait_until(
            || tui.screen_text().contains("\u{1f4df}   cat"),
            WAIT_TIMEOUT
        )
        .await,
        "expected terminal row after expanding default group, got: {:?}",
        tui.screen_text()
    );
    // Locate the actual rendered row and right-click it. The popup's second
    // content row is the terminal-only scheduled-input action.
    let terminal_rows = tui.with_screen(|screen| rows_containing(screen, "\u{1f4df}   cat"));
    assert_eq!(
        terminal_rows.len(),
        1,
        "expected one terminal tree row, got: {:?}",
        tui.screen_text()
    );
    let terminal_row = terminal_rows[0];
    let menu_column = 8;
    tui.write(&sgr_mouse_down(0, menu_column, terminal_row))
        .expect("focus terminal row before scheduling");
    tui.write(&sgr_mouse_up(menu_column, terminal_row))
        .expect("release terminal row focus click");
    tui.write(&sgr_mouse_down(2, menu_column, terminal_row))
        .expect("right-click terminal row");
    assert!(
        wait_until(
            || tui.screen_text().contains("Hit key(s) X time from now"),
            WAIT_TIMEOUT
        )
        .await,
        "expected scheduled-input context action, got: {:?}",
        tui.screen_text()
    );
    tui.write(&sgr_mouse_down(0, menu_column + 1, terminal_row + 2))
        .expect("click scheduled-input context action");
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                screen.contains("Schedule input for cat")
                    && screen.contains("Hours")
                    && screen.contains("Minutes")
                    && screen.contains("Seconds")
                    && screen.contains("Send Enter after the text")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "expected complete scheduled-input dialog, got: {:?}",
        tui.screen_text()
    );

    // Hours is initially focused. Move to Seconds, replace the default 30
    // with four seconds, enter a distinctive payload, retain the checked
    // Enter policy, and submit from the explicit button.
    tui.write(b"\t\t\x7f\x7f4\tscheduled-live-marker\t\t\r")
        .expect("fill and submit scheduled-input dialog");
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                screen.contains("4s") && screen.contains("cat")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "expected live human countdown before pane title, got: {:?}",
        tui.screen_text()
    );
    assert!(
        wait_until(
            || tui.screen_text().contains("scheduled-live-marker"),
            WAIT_TIMEOUT,
        )
        .await,
        "expected scheduled text plus Enter to reach cat after countdown, got: {:?}",
        tui.screen_text()
    );

    let kill_output = run_one_shot(&xdg, &project_dir, &["kill-session", SESSION_NAME]).await;
    assert!(kill_output.status.success(), "kill-session should succeed");
    cleanup_guard.already_cleaned_up = true;
    let exited = wait_until(|| tui.has_exited(), WAIT_TIMEOUT).await;
    if !exited {
        tui.kill().expect("force-kill scheduled-input TUI");
    }
    assert!(
        exited,
        "scheduled-input TUI did not exit after session cleanup"
    );
}

/// Proves the exact nested-boundary gesture through the real mouse/TUI,
/// client request, detached server, and shared tree domain. The row starts
/// below its nested group; clicking the rendered Up action must outdent it
/// into the enclosing group immediately before that former parent.
#[cfg(unix)]
#[tokio::test]
async fn clicking_up_on_a_boundary_pane_exits_its_nested_group() {
    let temp_root = tempfile::tempdir().expect("create tempdir");
    let xdg = IsolatedXdgDirs::under(temp_root.path()).expect("create isolated XDG dirs");
    seed_tree_row_management_controls(&xdg);
    let project_dir = temp_root.path().join("p");
    std::fs::create_dir_all(&project_dir).expect("create project dir");
    seed_project_config(&project_dir);
    let mut cleanup_guard = KillSessionOnDrop {
        xdg: &xdg,
        cwd: project_dir.clone(),
        session_name: SESSION_NAME,
        already_cleaned_up: false,
    };

    let fixture_output = run_one_shot(&xdg, &project_dir, &["new-pane", "--", "cat"]).await;
    assert!(
        fixture_output.status.success(),
        "creating boundary-move fixture failed: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&fixture_output.stdout),
        String::from_utf8_lossy(&fixture_output.stderr)
    );

    let attach_command = PtyCommand::new(ilium_binary(), &project_dir, 40, 120)
        .arg("--restart-server")
        .arg("--cwd")
        .arg(project_dir.to_string_lossy().to_string());
    let attach_command = xdg
        .as_pairs()
        .into_iter()
        .fold(attach_command, |command, (key, value)| {
            command.env(key, value.to_string_lossy().to_string())
        })
        // This test asserts against specific tree rows while a shell pane is
        // open, so the shell's own greeting has to be predictable. macOS ships
        // bash 3.2 as the login shell, which prints a multi-line "the default
        // interactive shell is now zsh" banner into the pane and pushes those
        // rows out of place.
        .env("SHELL", "/bin/sh");
    let mut tui = PtySession::spawn(attach_command).expect("spawn boundary-move TUI");

    assert!(
        wait_until(|| tui.screen_text().contains("default"), WAIT_TIMEOUT).await,
        "expected default group in boundary-move TUI, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"\x01t\x1b[B\x1b[C")
        .expect("focus tree, select default group, and expand it");

    // The selected default group is the create dialog's preselected parent.
    // Typing a name and pressing Enter therefore creates a genuinely nested
    // group through the same UI path a user follows.
    tui.write(b"\x01gnested\r")
        .expect("create nested group through the real dialog");
    assert!(
        wait_until(|| tui.screen_text().contains("nested"), WAIT_TIMEOUT).await,
        "expected nested group after dialog commit, got: {:?}",
        tui.screen_text()
    );

    // Select the nested group with a complete click, expand it, then create
    // one plain shell using the normal leader action. This makes that shell
    // both the first and last child, covering either boundary direction.
    let nested_row = tui.with_screen(|screen| rows_containing(screen, "nested"))[0];
    tui.write(&sgr_mouse_down(0, 8, nested_row))
        .expect("press nested group row");
    tui.write(&sgr_mouse_up(8, nested_row))
        .expect("release nested group row");
    tui.write(b"\x1b[C\x01c")
        .expect("expand nested group and create its pane");
    assert!(
        wait_until(
            || {
                tui.with_screen(|screen| {
                    rows_containing_in_order(screen, &["📟", "shell"]).len() == 1
                })
            },
            WAIT_TIMEOUT,
        )
        .await,
        "expected one shell inside nested group, got: {:?}",
        tui.screen_text()
    );

    let nested_row_before = tui.with_screen(|screen| rows_containing(screen, "nested"))[0];
    let shell_row_before =
        tui.with_screen(|screen| rows_containing_in_order(screen, &["📟", "shell"]))[0];
    assert!(
        nested_row_before < shell_row_before,
        "fixture pane must begin below its nested parent: {:?}",
        tui.screen_text()
    );

    // Hover reveals the action overlay and expands the tree horizontally.
    // Read the Up icon's actual terminal cell rather than duplicating the
    // renderer's animated width or fixed-slot geometry in this PTY test.
    tui.write(&sgr_mouse_move(8, shell_row_before))
        .expect("hover boundary pane row");
    assert!(
        wait_until(
            || {
                tui.with_screen(|screen| {
                    let columns = screen.size().1;
                    (0..columns).any(|column| {
                        screen
                            .cell(shell_row_before, column)
                            .is_some_and(|cell| cell.contents().contains("🔼"))
                    })
                })
            },
            WAIT_TIMEOUT,
        )
        .await,
        "expected the hovered pane's Up icon, got: {:?}",
        tui.screen_text()
    );
    let move_up_column = tui.with_screen(|screen| {
        let columns = screen.size().1;
        (0..columns)
            .find(|column| {
                screen
                    .cell(shell_row_before, *column)
                    .is_some_and(|cell| cell.contents().contains("🔼"))
            })
            .expect("find rendered Up action column")
    });
    tui.write(&sgr_mouse_down(0, move_up_column, shell_row_before))
        .expect("click boundary pane Up action");

    assert!(
        wait_until(
            || {
                tui.with_screen(|screen| {
                    let nested_rows = rows_containing(screen, "nested");
                    let shell_rows = rows_containing_in_order(screen, &["📟", "shell"]);
                    nested_rows.len() == 1
                        && shell_rows.len() == 1
                        && shell_rows[0] < nested_rows[0]
                })
            },
            WAIT_TIMEOUT,
        )
        .await,
        "clicked pane should exit immediately before its former group: {:?}",
        tui.screen_text()
    );

    let kill_output = run_one_shot(&xdg, &project_dir, &["kill-session", SESSION_NAME]).await;
    assert!(kill_output.status.success(), "kill-session should succeed");
    cleanup_guard.already_cleaned_up = true;
    let exited = wait_until(|| tui.has_exited(), WAIT_TIMEOUT).await;
    if !exited {
        tui.kill().expect("force-kill boundary-move TUI");
    }
    assert!(exited, "boundary-move TUI did not exit after cleanup");
}

/// Drives the persisted folder browser through a real client/server TUI:
/// create a root from the folder-only picker, expand four nested directory
/// rows by mouse, then open the deep file. This protects the complete widget
/// identifier path virtual rows need; selecting only a synthetic final ID
/// makes the first level appear but breaks at the next directory.
#[cfg(unix)]
#[tokio::test]
async fn folder_browser_expands_nested_directories_and_opens_a_deep_file() {
    let temp_root = tempfile::tempdir().expect("create tempdir");
    let xdg = IsolatedXdgDirs::under(temp_root.path()).expect("create isolated XDG dirs");
    let project_dir = temp_root.path().join("folder-project");
    let nested_directory = project_dir.join("alpha").join("beta").join("gamma");
    std::fs::create_dir_all(&nested_directory).expect("create nested project folders");
    let deep_file = nested_directory.join("deep.rs");
    std::fs::write(
        &deep_file,
        "const DEEP_FOLDER_EDITOR_PROOF: &str = \"opened\";\n",
    )
    .expect("write deep editor fixture");
    seed_project_config(&project_dir);
    let mut cleanup_guard = KillSessionOnDrop {
        xdg: &xdg,
        cwd: project_dir.clone(),
        session_name: SESSION_NAME,
        already_cleaned_up: false,
    };

    let attach_command = PtyCommand::new(ilium_binary(), &project_dir, 40, 120)
        .arg("--cwd")
        .arg(project_dir.to_string_lossy().to_string());
    let attach_command = xdg
        .as_pairs()
        .into_iter()
        .fold(attach_command, |command, (key, value)| {
            command.env(key, value.to_string_lossy().to_string())
        });
    let mut tui = PtySession::spawn(attach_command).expect("spawn folder-browser TUI");
    assert!(
        wait_until(|| tui.screen_text().contains(PROJECT_NAME), WAIT_TIMEOUT).await,
        "expected initial TUI frame, got: {:?}",
        tui.screen_text()
    );

    // Folder pickers navigate on Enter; Tab then Enter explicitly commits
    // the current directory through the bottom action without displaying files.
    tui.write(b"\x01f").expect("open folder picker");
    assert!(
        wait_until(|| tui.screen_text().contains("Open Folder"), WAIT_TIMEOUT).await,
        "expected folder picker, got: {:?}",
        tui.screen_text()
    );
    assert!(
        tui.screen_text().contains("Add Folder"),
        "expected explicit folder-creation action, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"\t\r").expect("confirm current project folder");
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                screen.contains("folder-project") && !screen.contains("Open Folder")
            },
            WAIT_TIMEOUT
        )
        .await,
        "expected persisted folder root after picker closed, got: {:?}",
        tui.screen_text()
    );

    // Wait for the server-confirmed folder node, not merely for the already
    // visible project row. Both share a basename and the client restores the
    // project's persisted expanded state when that snapshot arrives.
    assert!(
        wait_until(
            || tui.with_screen(|screen| rows_containing(screen, "folder-project").len() >= 2),
            WAIT_TIMEOUT,
        )
        .await,
        "expected server-confirmed folder root inside project, got: {:?}",
        tui.screen_text()
    );

    // Locate rows after each render so indentation never becomes a guessed
    // coordinate; each click follows the same hit-test and expansion path a
    // real user uses.
    let root_rows = tui.with_screen(|screen| rows_containing(screen, "folder-project"));
    let folder_row = *root_rows
        .last()
        .expect("expected persisted folder-root row");
    tui.write(&sgr_mouse_down(0, 8, folder_row))
        .expect("expand folder root");
    tui.write(&sgr_mouse_up(8, folder_row))
        .expect("release folder-root click");
    assert!(
        wait_until(|| tui.screen_text().contains("alpha"), WAIT_TIMEOUT).await,
        "expected first nested directory, got: {:?}",
        tui.screen_text()
    );
    for (directory, next_entry) in [("alpha", "beta"), ("beta", "gamma"), ("gamma", "deep.rs")] {
        let rows = tui.with_screen(|screen| rows_containing(screen, directory));
        assert_eq!(rows.len(), 1, "expected one {directory:?} row");
        tui.write(&sgr_mouse_down(0, 8, rows[0]))
            .unwrap_or_else(|error| panic!("expand {directory}: {error}"));
        tui.write(&sgr_mouse_up(8, rows[0]))
            .unwrap_or_else(|error| panic!("release {directory} click: {error}"));
        assert!(
            wait_until(|| tui.screen_text().contains(next_entry), WAIT_TIMEOUT).await,
            "expected {next_entry:?} after expanding {directory:?}, got: {:?}",
            tui.screen_text()
        );
    }
    assert!(
        wait_until(|| tui.screen_text().contains("deep.rs"), WAIT_TIMEOUT).await,
        "expected deep file after recursive expansion, got: {:?}",
        tui.screen_text()
    );

    let file_rows = tui.with_screen(|screen| rows_containing(screen, "deep.rs"));
    assert_eq!(file_rows.len(), 1, "expected one deep file row");
    tui.write(&sgr_mouse_down(0, 8, file_rows[0]))
        .expect("open deep file in editor");
    tui.write(&sgr_mouse_up(8, file_rows[0]))
        .expect("release deep-file click");
    assert!(
        wait_until(
            || tui.screen_text().contains("DEEP_FOLDER_EDITOR_PROOF"),
            WAIT_TIMEOUT,
        )
        .await,
        "expected deep file editor content, got: {:?}",
        tui.screen_text()
    );

    let kill_output = run_one_shot(&xdg, &project_dir, &["kill-session", SESSION_NAME]).await;
    assert!(kill_output.status.success(), "kill-session should succeed");
    cleanup_guard.already_cleaned_up = true;
    let exited = wait_until(|| tui.has_exited(), WAIT_TIMEOUT).await;
    if !exited {
        tui.kill().expect("force-kill folder-browser TUI");
    }
    assert!(exited, "folder-browser TUI did not exit after cleanup");
}

/// Proves the user-facing debug workflow against the real client, detached
/// server, process detector, mouse hit testing, path prompt, and filesystem.
/// The fake agent changes only counters between polls, so two exports several
/// ticks apart must retain exactly the same number of detection decisions.
/// Focusing and unfocusing the left panel also produces real PTY resizes; the
/// default toolbar filter hides them until the operator explicitly reveals
/// them, and Save follows the same active policy.
#[cfg(unix)]
#[tokio::test]
async fn agent_debug_log_filters_panel_resizes_and_saves_the_active_view() {
    let temp_root = tempfile::tempdir().expect("create tempdir");
    let xdg = IsolatedXdgDirs::under(temp_root.path()).expect("create isolated XDG dirs");
    seed_agent_debug_config(&xdg);
    let project_dir = temp_root.path().join("agent-debug-project");
    let fixture_directory = temp_root.path().join("fixture-bin");
    std::fs::create_dir_all(&project_dir).expect("create project directory");
    std::fs::create_dir_all(&fixture_directory).expect("create fixture directory");
    seed_project_config(&project_dir);
    let fake_codex = write_change_only_fake_codex(&fixture_directory);
    let mut cleanup_guard = KillSessionOnDrop {
        xdg: &xdg,
        cwd: project_dir.clone(),
        session_name: SESSION_NAME,
        already_cleaned_up: false,
    };

    let fake_codex_argument = fake_codex.to_string_lossy().to_string();
    let new_pane_output = run_one_shot(
        &xdg,
        &project_dir,
        &["new-pane", "--", &fake_codex_argument],
    )
    .await;
    assert!(
        new_pane_output.status.success(),
        "creating fake Codex pane failed: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&new_pane_output.stdout),
        String::from_utf8_lossy(&new_pane_output.stderr)
    );

    let attach_command = PtyCommand::new(ilium_binary(), &project_dir, 44, 140)
        .arg("--cwd")
        .arg(project_dir.to_string_lossy().to_string());
    let attach_command = xdg
        .as_pairs()
        .into_iter()
        .fold(attach_command, |command, (key, value)| {
            command.env(key, value.to_string_lossy().to_string())
        });
    let mut tui = PtySession::spawn(attach_command).expect("spawn agent-debug TUI");

    assert!(
        wait_until(
            || {
                tui.with_screen(|screen| {
                    !rows_containing_before_column(screen, "Codex:", 60).is_empty()
                })
            },
            DETECTION_TIMEOUT,
        )
        .await,
        "expected a detected working Codex row.\n{}\nscreen: {:?}",
        detection_diagnostics(&xdg.debug_log_dir, &project_dir),
        tui.screen_text()
    );
    let agent_rows = tui.with_screen(|screen| rows_containing_before_column(screen, "Codex:", 60));
    assert_eq!(agent_rows.len(), 1, "expected one detected Codex tree row");
    let agent_row = agent_rows[0];

    tui.write(&sgr_mouse_down(0, 8, agent_row))
        .expect("focus detected Codex row");
    tui.write(&sgr_mouse_up(8, agent_row))
        .expect("release detected Codex row");
    assert!(
        wait_until(
            || tui.screen_text().contains("Cogitating (esc to interrupt)"),
            WAIT_TIMEOUT,
        )
        .await,
        "expected focused fake Codex terminal, got: {:?}",
        tui.screen_text()
    );

    // Selecting the tree row reveals the pane; a click inside the terminal
    // moves keyboard focus from the tree to the PTY before typing.
    tui.write(&sgr_mouse_down(0, 80, 10))
        .expect("focus the fake Codex PTY");
    tui.write(&sgr_mouse_up(80, 10))
        .expect("release the fake Codex PTY focus click");
    tui.write(b"diagnostic-prompt\r")
        .expect("submit an exact prompt to the fake Codex pane");
    assert!(
        wait_until(
            || {
                process_log_for_project(&xdg.debug_log_dir, &project_dir).is_some_and(
                    |(_, contents)| {
                        logged_prompt_submissions(&contents)
                            .iter()
                            .any(|submission| submission == "diagnostic-prompt")
                    },
                )
            },
            WAIT_TIMEOUT,
        )
        .await,
        "expected exact submitted input in the private process log.\n{}\nscreen: {:?}",
        process_log_diagnostics(&xdg.debug_log_dir, &project_dir),
        tui.screen_text()
    );

    // Exercise focus-owned expansion and contraction explicitly instead of
    // relying on pointer hover duration. Waiting past the 180 ms transition
    // ensures both endpoints reached the PTY/server journal before opening it.
    tui.write(b"\x01t")
        .expect("focus the left tree panel for resize provenance");
    tokio::time::sleep(Duration::from_millis(300)).await;
    tui.write(b"\x01p")
        .expect("return focus to the active pane for resize provenance");
    tokio::time::sleep(Duration::from_millis(300)).await;

    let menu_column = 80;
    let menu_row = 10;
    tui.write(&sgr_mouse_down(2, menu_column, menu_row))
        .expect("right-click detected Codex terminal");
    assert!(
        wait_until(
            || tui.screen_text().contains("Show debug log"),
            WAIT_TIMEOUT,
        )
        .await,
        "expected detected-agent context action, got: {:?}",
        tui.screen_text()
    );
    tui.write(&sgr_mouse_down(0, menu_column + 1, menu_row + 1))
        .expect("open agent debug log");
    tui.write(b"\x1b[H")
        .expect("jump to the oldest retained debug events");
    // The chrome and the filter summary are one screen's worth by
    // construction, so they are asserted together on the opened view.
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                screen.contains("Agent debug log")
                    && screen.contains("Save log")
                    && screen.contains("Panel resizes hidden")
                    && screen.contains("panel resize events hid")
                    && screen.contains("DETECTION")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "expected the retained history's chrome and filter summary, got: {:?}",
        tui.screen_text()
    );

    // The three decisions are not: every one of them quotes absolute paths,
    // which wrap to a height that depends on how long the platform's
    // temporary directory happens to be -- macOS's
    // `/private/var/folders/<...>/T` pushes the last decision below the fold
    // where Linux's `/tmp` does not. Requiring them on one screen would be
    // asserting the width of a path, so they are collected while walking the
    // log the way an operator reads it.
    let mut unseen = vec![
        "Agent identity decision",
        "Activity decision",
        "Goal decision",
    ];
    for scroll in 0..40 {
        let screen = tui.screen_text();
        unseen.retain(|decision| !screen.contains(decision));
        if unseen.is_empty() {
            break;
        }
        // The first pass only reads what opening the view already showed;
        // afterwards, one line of travel per pass so nothing is stepped over.
        if scroll > 0 {
            tui.write(b"\x1b[B").expect("scroll the retained history");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        unseen.is_empty(),
        "retained detection history never showed {unseen:?}, last screen: {:?}",
        tui.screen_text()
    );

    // Back to the oldest events, so the Save button this test clicks next is
    // found at the position the view opens in rather than wherever the scan
    // above stopped.
    tui.write(b"\x1b[H")
        .expect("return to the oldest retained debug events");

    tokio::time::sleep(Duration::from_millis(2200)).await;
    let (save_column, save_row) = tui
        .with_screen(|screen| first_cell_containing(screen, "💾"))
        .expect("find top Save button");
    tui.write(&sgr_mouse_down(0, save_column, save_row))
        .expect("click top Save button");
    assert!(
        wait_until(
            || {
                let screen = tui.screen_text();
                screen.contains("Save agent debug log")
                    && screen.contains("Cancel")
                    && screen.contains("Save")
            },
            WAIT_TIMEOUT,
        )
        .await,
        "expected editable destination path prompt, got: {:?}",
        tui.screen_text()
    );
    let save_prompt_layout = ilium_client::modal::text_prompt_dialog_layout_for_size(140, 44);
    let save_prompt_button = save_prompt_layout.actions.confirm_button;
    tui.write(&sgr_mouse_down(
        0,
        save_prompt_button.x + save_prompt_button.width / 2,
        save_prompt_button.y,
    ))
    .expect("click Save in the destination path prompt");
    assert!(
        wait_until(
            || tui.screen_text().contains("Saved agent debug log to"),
            WAIT_TIMEOUT,
        )
        .await,
        "expected successful first save status, got: {:?}",
        tui.screen_text()
    );

    let exported_logs = || {
        let mut paths: Vec<_> = std::fs::read_dir(&project_dir)
            .expect("read project exports")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name().is_some_and(|name| {
                    let name = name.to_string_lossy();
                    name.starts_with("ilium-") && name.contains("-debug-") && name.ends_with(".log")
                })
            })
            .collect();
        paths.sort();
        paths
    };
    let first_paths = exported_logs();
    assert_eq!(first_paths.len(), 1, "expected one first export");
    let first_report = std::fs::read_to_string(&first_paths[0]).expect("read first export");
    let first_detection_count = first_report.matches("[DETECTION]").count();
    assert!(
        first_detection_count > 0,
        "first report needs a detection decision"
    );
    assert!(
        first_report.contains("Filter: left-panel focus/hover animation resize events excluded")
    );
    assert!(!first_report.contains("Left panel focus/hover animation"));

    tokio::time::sleep(Duration::from_millis(2200)).await;
    tui.write(b"r\x1b[H")
        .expect("reveal panel animation resizes and jump to oldest history");
    assert!(
        wait_until(
            || tui.screen_text().contains("Panel resizes shown"),
            WAIT_TIMEOUT,
        )
        .await,
        "expected disabled resize filter switch, got: {:?}",
        tui.screen_text()
    );
    let mut panel_resize_evidence_visible = tui
        .screen_text()
        .contains("Left panel focus/hover animation");
    for _ in 0..20 {
        if panel_resize_evidence_visible {
            break;
        }
        tui.write(b"\x1b[6~")
            .expect("page toward newer resize evidence");
        tokio::time::sleep(Duration::from_millis(40)).await;
        panel_resize_evidence_visible = tui
            .screen_text()
            .contains("Left panel focus/hover animation");
    }
    assert!(
        panel_resize_evidence_visible,
        "revealing the filter should expose typed panel-resize evidence: {:?}",
        tui.screen_text()
    );
    tui.write(b"s").expect("open second export path prompt");
    assert!(
        wait_until(
            || tui.screen_text().contains("Save agent debug log"),
            WAIT_TIMEOUT,
        )
        .await,
        "expected second destination prompt, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"\r").expect("accept second default export path");
    assert!(
        wait_until(|| exported_logs().len() == 2, WAIT_TIMEOUT).await,
        "expected a second timestamped export"
    );
    let second_paths = exported_logs();
    let second_report = std::fs::read_to_string(&second_paths[1]).expect("read second export");
    assert!(
        second_report.contains("Filter: left-panel focus/hover animation resize events included")
    );
    assert!(second_report.contains("Left panel focus/hover animation"));
    assert_eq!(
        second_report.matches("[DETECTION]").count(),
        first_detection_count,
        "counter-only polls must not add detection entries between exports"
    );
    for expected in [
        "Agent identity decision",
        "Activity decision",
        "Goal decision",
        "Matched activity evidence",
        "<number>",
    ] {
        assert!(
            second_report.contains(expected),
            "saved report should contain {expected:?}: {second_report}"
        );
    }
    for forbidden in ["phase 1", "request generation", "repetition_count"] {
        assert!(
            !second_report.contains(forbidden),
            "saved report should not expose {forbidden:?}: {second_report}"
        );
    }

    let (process_log_path, process_log) = process_log_for_project(&xdg.debug_log_dir, &project_dir)
        .expect("find this session's process log");
    assert_eq!(
        std::fs::metadata(&process_log_path)
            .expect("process log metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(process_log_path.parent().expect("process log parent"))
            .expect("process log directory metadata")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    for expected in [
        "event_kind=SessionDiscovery",
        "Canonical project boundary",
        "Phase: Open transcript descriptors",
        "event_kind=PromptSubmitted",
        "diagnostic-prompt",
    ] {
        assert!(
            process_log.contains(expected),
            "private process log should contain {expected:?}: {process_log}"
        );
    }
    let prompt_line = process_log
        .lines()
        .find(|line| line.contains("event_kind=PromptSubmitted"))
        .expect("prompt event line");
    assert!(
        !prompt_line.contains("\"process_id\":null"),
        "prompt event should identify the detected agent PID: {prompt_line}"
    );
    assert!(!process_log.contains("[redacted: available in Agent debug journal]"));

    let kill_output = run_one_shot(&xdg, &project_dir, &["kill-session", SESSION_NAME]).await;
    assert!(kill_output.status.success(), "kill-session should succeed");
    cleanup_guard.already_cleaned_up = true;
    let exited = wait_until(|| tui.has_exited(), WAIT_TIMEOUT).await;
    if !exited {
        tui.kill().expect("force-kill agent-debug TUI");
    }
    assert!(exited, "agent-debug TUI did not exit after cleanup");
}
