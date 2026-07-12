//! PTY-driven smoke test for the actual `illium` binary: the same
//! technique `illium-pty/tests/pty_integration.rs` uses (a real pty via
//! `illium_pty::PtySession`, not `std::process::Command` with inherited
//! stdio) applied one layer up, against the real CLI + TUI instead of a
//! trivial `echo`/`cat`.
//!
//! Two phases:
//! 1. `illium new-pane -- cat` -- a non-interactive, non-attaching
//!    subcommand (plain `std::process::Command`, no pty needed: it never
//!    enters raw mode) that spawns this test's session's server and adds
//!    one terminal pane to it, then exits. This is the "create state
//!    without a TUI" half.
//! 2. Bare `illium --cwd <dir>` -- the actual attaching form -- run
//!    inside a real pty at a fixed size. Its first rendered frame is
//!    asserted to contain structural chrome (the sidebar title from
//!    `illium_client::tree_ui::sidebar_title`) and the pane created in
//!    phase 1 (named after its command line, `"cat"`, per
//!    `TerminalOrigin::default_pane_name`). A scripted leader-key + help
//!    keystroke (`Ctrl+A` then `?`, see `illium_client::keymap`) is then
//!    written to the pty, proving input routing and rendering are both
//!    alive end-to-end: the screen must change to show
//!    `illium_client::help`'s overlay text.
//!
//! Isolation: `XDG_DATA_HOME`, `XDG_CONFIG_HOME`, and `XDG_RUNTIME_DIR`
//! are all pointed at one tempdir for every spawned process (the real
//! `illium-server::paths::resolve` prefers `XDG_RUNTIME_DIR` for the
//! session socket when it's set -- see `illium/src/session.rs`'s
//! `socket_dir`, which must match that formula for this test, or indeed
//! the real CLI, to ever find its own spawned server), so nothing here
//! touches a real `~/.local/share/illium` or a real running session. A
//! `.illium/config.yaml` with a pre-set project name is written into the
//! session's cwd before attaching, so the client's background project-name
//! inference worker (which would otherwise call out to `illium-kilo-gateway`,
//! a real network call this workspace's tests must never make) never
//! fires -- see `illium_client::project_naming::load_stored_project_name`.
//!
//! Cleanup: the graceful path this workspace already ships, `illium
//! kill-session <name>`, is reused rather than a raw process kill --
//! `illium_client::run`'s event loop exits (and the process returns) the
//! moment the server closes every connection on `KillSession` (see
//! `illium-client/src/lib.rs`'s `run_inner`), so this is both graceful
//! and self-verifying: the pty-attached process's own exit is awaited as
//! part of proving the shutdown path actually works, not just that the
//! one-shot subcommand returned. A force-kill + explicit tempdir cleanup
//! is kept only as a defensive fallback in case that ever hangs, so this
//! test itself cannot hang the suite even if the graceful path regresses.

use std::path::{Path, PathBuf};
use std::time::Duration;

use illium_pty::{PtyCommand, PtySession};

/// How long phases of this test wait for the server/TUI/help overlay to
/// respond before giving up -- generous relative to `illium-pty`'s own
/// 5s convention since this test additionally waits on a real spawned
/// `illium-server` process starting up, not just a trivial child process.
const WAIT_TIMEOUT: Duration = Duration::from_secs(10);

/// Session name this test's isolated `illium` uses throughout -- fixed
/// (not randomized) since every process in this test shares one
/// dedicated tempdir-rooted `XDG_RUNTIME_DIR`/`XDG_DATA_HOME`, so there is
/// no real collision risk with a concurrently running suite or a real
/// user session.
const SESSION_NAME: &str = "default";

/// Project name pre-seeded into `.illium/config.yaml` -- one word, so it
/// passes `illium_client::naming::normalize_word_bounded`'s 1-2 word
/// bound unchanged, and distinctive enough that seeing it on screen can
/// only mean this test's own config file was read.
const PROJECT_NAME: &str = "Smoketest";

/// Polls `condition` until it's true or `timeout` elapses, without a
/// fixed sleep -- pty output latency (a real spawned `illium-server`
/// starting up, a real render loop drawing a frame) is not deterministic
/// under test-runner load. Mirrors `illium-pty`'s and
/// `illium-server`'s own integration test helper of the same name.
async fn wait_until(mut condition: impl FnMut() -> bool, timeout: Duration) -> bool {
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

/// Every env var override every process this test spawns needs, so all
/// of them -- the one-shot `new-pane`/`kill-session` subcommands and the
/// pty-attached long-lived TUI -- agree on where this session's socket,
/// data, and config live. See this file's module docs for why all three
/// (not just `XDG_DATA_HOME`/`XDG_CONFIG_HOME`) matter.
struct IsolatedXdgDirs {
    data_home: PathBuf,
    config_home: PathBuf,
    runtime_dir: PathBuf,
}

impl IsolatedXdgDirs {
    fn under(root: &Path) -> std::io::Result<Self> {
        let data_home = root.join("data");
        let config_home = root.join("config");
        let runtime_dir = root.join("runtime");
        for dir in [&data_home, &config_home, &runtime_dir] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(Self {
            data_home,
            config_home,
            runtime_dir,
        })
    }

    fn as_pairs(&self) -> [(&'static str, &Path); 3] {
        [
            ("XDG_DATA_HOME", &self.data_home),
            ("XDG_CONFIG_HOME", &self.config_home),
            ("XDG_RUNTIME_DIR", &self.runtime_dir),
        ]
    }
}

/// Path to the `illium` binary this test itself was built alongside --
/// Cargo sets this for every integration test in a package that also has
/// a `[[bin]]` (or, as here, the implicit `src/main.rs` binary) target.
fn illium_binary() -> &'static str {
    env!("CARGO_BIN_EXE_illium")
}

/// Runs a one-shot `illium` subcommand (`new-pane`/`kill-session`) to
/// completion with the isolated XDG env applied, returning its captured
/// stdout+stderr for assertions. Panics (failing the test) if it doesn't
/// exit within [`WAIT_TIMEOUT`] -- a hang here means the CLI itself is
/// broken, which is exactly what this test exists to catch.
async fn run_one_shot(xdg: &IsolatedXdgDirs, cwd: &Path, args: &[&str]) -> std::process::Output {
    let mut command = tokio::process::Command::new(illium_binary());
    command.args(args).current_dir(cwd);
    for (key, value) in xdg.as_pairs() {
        command.env(key, value);
    }

    let output = tokio::time::timeout(WAIT_TIMEOUT, command.output())
        .await
        .unwrap_or_else(|_| panic!("`illium {args:?}` did not exit within {WAIT_TIMEOUT:?}"))
        .unwrap_or_else(|error| panic!("failed to spawn `illium {args:?}`: {error}"));
    output
}

/// Writes `.illium/config.yaml` with a pre-set project name into `cwd`,
/// matching `illium_client::project_config`'s on-disk format (a plain
/// YAML mapping under the `project name` key) closely enough for
/// `project_naming::load_stored_project_name` to read it back -- see
/// this file's module docs for why this must happen before attaching.
fn seed_project_config(cwd: &Path) {
    let illium_dir = cwd.join(".illium");
    std::fs::create_dir_all(&illium_dir).expect("create .illium dir");
    std::fs::write(
        illium_dir.join("config.yaml"),
        format!("project name: {PROJECT_NAME}\n"),
    )
    .expect("write .illium/config.yaml");
}

#[tokio::test]
async fn attaching_tui_renders_the_pane_created_by_new_pane_and_responds_to_the_help_keystroke() {
    let temp_root = tempfile::tempdir().expect("create tempdir");
    let xdg = IsolatedXdgDirs::under(temp_root.path()).expect("create isolated XDG dirs");
    let project_dir = temp_root.path().join("project");
    std::fs::create_dir_all(&project_dir).expect("create project dir");
    seed_project_config(&project_dir);

    // Phase 1: `illium new-pane -- cat` -- no `--cwd` flag on this
    // subcommand (see `illium/src/main.rs`'s doc comment on `NewPane`),
    // so the pane it creates is rooted wherever this subprocess's own
    // cwd is; pointed at `project_dir` so it lands in the same place
    // phase 2 attaches to, though `NewPane` itself doesn't care since
    // `cat` needs no real filesystem content.
    let new_pane_output = {
        let mut command = tokio::process::Command::new(illium_binary());
        command
            .args(["new-pane", "--", "cat"])
            .current_dir(&project_dir);
        for (key, value) in xdg.as_pairs() {
            command.env(key, value);
        }
        tokio::time::timeout(WAIT_TIMEOUT, command.output())
            .await
            .expect("`illium new-pane -- cat` did not exit in time")
            .expect("failed to spawn `illium new-pane -- cat`")
    };
    assert!(
        new_pane_output.status.success(),
        "`illium new-pane -- cat` failed: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&new_pane_output.stdout),
        String::from_utf8_lossy(&new_pane_output.stderr)
    );

    // Phase 2: the actual attaching form, inside a real pty at a fixed
    // size -- exactly the technique `illium-pty/tests/pty_integration.rs`
    // uses for a trivial command, applied to the real CLI/TUI binary.
    let attach_command = PtyCommand::new(illium_binary(), &project_dir, 40, 120)
        .arg("--cwd")
        .arg(project_dir.to_string_lossy().to_string());
    let attach_command = xdg
        .as_pairs()
        .into_iter()
        .fold(attach_command, |command, (key, value)| {
            command.env(key, value.to_string_lossy().to_string())
        });
    let mut tui = PtySession::spawn(attach_command).expect("spawning `illium` under a pty");

    // Structural assertion #1: the sidebar chrome
    // (`illium_client::tree_ui::sidebar_title`) shows this test's seeded
    // project name, and the group phase 1's pane landed in
    // (`illium-server::ipc::handlers::pane_snapshot_kind_for` creates a
    // default group for a bare `NewPane`, per `illium-server/tests/smoke.rs`)
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

    // A freshly attached client starts with every group collapsed and
    // focus on the (empty) pane panel, so phase 1's "cat" pane isn't
    // actually listed yet -- expand it to also prove out tree navigation
    // (`illium_client::app::App::handle_tree_key`), not just leader-key
    // dispatch: `Ctrl+A then t` (`Action::FocusTree`) moves focus to the
    // tree, Down selects its first entry (the "default" group -- see
    // `tui_tree_widget::TreeState::key_down`'s "nothing selected ->
    // select the first item" behavior), and Right expands it
    // (`TreeState::key_right`).
    tui.write(b"\x01t")
        .expect("writing Ctrl+A then t (FocusTree)");
    tui.write(b"\x1b[B").expect("writing Down arrow"); // selects the first tree entry
    tui.write(b"\x1b[C").expect("writing Right arrow"); // expands it
    let pane_listed = wait_until(|| tui.screen_text().contains("cat"), WAIT_TIMEOUT).await;
    assert!(
        pane_listed,
        "expected the \"cat\" pane to appear in the tree after expanding its group, got: {:?}",
        tui.screen_text()
    );

    // Structural assertion #2: input routing. `Ctrl+A` (0x01, the
    // documented leader key -- `illium_client::keymap::is_leader_key`)
    // followed by `?` (`illium_client::keymap::Action::Help`'s bound
    // letter) must flip the render to show
    // `illium_client::help::render`'s overlay -- proof that keystrokes
    // typed into this pty actually reach the input-dispatch state
    // machine and that its effect actually reaches the next rendered
    // frame, not just that *a* frame renders.
    tui.write(b"\x01?")
        .expect("writing the leader+help keystroke to the pty");
    let help_shown = wait_until(
        || {
            let screen = tui.screen_text();
            screen.contains("keyboard reference") && screen.contains("Ctrl+A then ?")
        },
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        help_shown,
        "expected the help overlay after Ctrl+A then ?, got: {:?}",
        tui.screen_text()
    );

    // Cleanup: reuse the CLI's own graceful `kill-session` subcommand
    // (see this file's module docs) rather than killing the pty-attached
    // process directly -- its own exit, awaited below, is this test's
    // proof the graceful shutdown path (server closes every connection
    // on `KillSession`, `illium_client::run`'s event loop then exits on
    // its own) actually works end-to-end.
    let kill_output = run_one_shot(&xdg, &project_dir, &["kill-session", SESSION_NAME]).await;
    assert!(
        kill_output.status.success(),
        "`illium kill-session {SESSION_NAME}` failed: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&kill_output.stdout),
        String::from_utf8_lossy(&kill_output.stderr)
    );

    let attached_process_exited = wait_until(|| tui.has_exited(), WAIT_TIMEOUT).await;
    if !attached_process_exited {
        // Defensive fallback only -- see this file's module docs. Getting
        // here means the graceful shutdown path itself regressed, which
        // is a real bug the assertion above already failed loudly on;
        // this just keeps one broken run from hanging the whole suite.
        tui.kill()
            .expect("force-killing the pty-attached illium process");
    }
    assert!(
        attached_process_exited,
        "the pty-attached `illium` process should exit on its own once `kill-session` \
         closes the connection, not need a force kill"
    );
}
