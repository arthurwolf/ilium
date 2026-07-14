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
//!    phase 1 (named after its command line, `"cat"`, per
//!    `TerminalOrigin::default_pane_name`). A scripted leader-key + help
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

use std::path::{Path, PathBuf};
use std::time::Duration;

use ilium_pty::{PtyCommand, PtySession};

/// How long phases of this test wait for the server/TUI/help overlay to
/// respond before giving up -- generous relative to `ilium-pty`'s own
/// 5s convention since this test additionally waits on a real spawned
/// `ilium-server` process starting up, not just a trivial child process.
const WAIT_TIMEOUT: Duration = Duration::from_secs(10);

/// Mirrors `ilium_client::tree_ui::RECENTLY_CREATED_PULSE_MS` (crate-private
/// there, so duplicated here rather than imported) -- the total window a
/// freshly created pane's row flashes for.
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

/// Resolves the `ilium` binary under test. Cargo's adjacent test binary is
/// the default, while `ILIUM_PTY_SMOKE_BINARY` lets the same isolated flow
/// prove a release-installed client and its sibling server after deployment.
fn ilium_binary() -> String {
    std::env::var_os("ILIUM_PTY_SMOKE_BINARY")
        .map(|binary_path| binary_path.to_string_lossy().into_owned())
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_ilium").to_string())
}

/// Runs a one-shot `ilium` subcommand (`new-pane`/`kill-session`) to
/// completion with the isolated XDG env applied, returning its captured
/// stdout+stderr for assertions. Panics (failing the test) if it doesn't
/// exit within [`WAIT_TIMEOUT`] -- a hang here means the CLI itself is
/// broken, which is exactly what this test exists to catch.
async fn run_one_shot(xdg: &IsolatedXdgDirs, cwd: &Path, args: &[&str]) -> std::process::Output {
    let mut command = tokio::process::Command::new(ilium_binary());
    command.args(args).current_dir(cwd);
    for (key, value) in xdg.as_pairs() {
        command.env(key, value);
    }
    // Without this, a `command.output()` that hits the `timeout` below drops
    // its `Child` handle without killing the underlying process (tokio only
    // kills-on-drop when asked to): the one-shot subcommand would keep
    // running as an orphan indefinitely instead of dying with the timeout
    // that was supposed to bound it.
    command.kill_on_drop(true);

    let output = tokio::time::timeout(WAIT_TIMEOUT, command.output())
        .await
        .unwrap_or_else(|_| panic!("`ilium {args:?}` did not exit within {WAIT_TIMEOUT:?}"))
        .unwrap_or_else(|error| panic!("failed to spawn `ilium {args:?}`: {error}"));
    output
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
        "[keyboard]\nshortcut_base = \"b\"\n",
    )
    .expect("write isolated keyboard config");
}

#[tokio::test]
async fn attaching_tui_renders_the_pane_created_by_new_pane_and_responds_to_the_help_keystroke() {
    let temp_root = tempfile::tempdir().expect("create tempdir");
    let xdg = IsolatedXdgDirs::under(temp_root.path()).expect("create isolated XDG dirs");
    seed_keyboard_config(&xdg);
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
    let new_pane_output = {
        let mut command = tokio::process::Command::new(ilium_binary());
        command
            .args(["new-pane", "--", "cat"])
            .current_dir(&project_dir);
        for (key, value) in xdg.as_pairs() {
            command.env(key, value);
        }
        // See `run_one_shot`'s identical setting: without it, a timeout on
        // the `command.output()` below would leave this one-shot subcommand
        // running as an orphan instead of actually dying with the timeout.
        command.kill_on_drop(true);
        tokio::time::timeout(WAIT_TIMEOUT, command.output())
            .await
            .expect("`ilium new-pane -- cat` did not exit in time")
            .expect("failed to spawn `ilium new-pane -- cat`")
    };
    assert!(
        new_pane_output.status.success(),
        "`ilium new-pane -- cat` failed: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&new_pane_output.stdout),
        String::from_utf8_lossy(&new_pane_output.stderr)
    );

    // Phase 2: the actual attaching form, inside a real pty at a fixed
    // size -- exactly the technique `ilium-pty/tests/pty_integration.rs`
    // uses for a trivial command, applied to the real CLI/TUI binary.
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
    tui.write(b"\x1b[B").expect("writing Down arrow"); // selects the first tree entry
    tui.write(b"\x1b[C").expect("writing Right arrow"); // expands it
    let pane_listed = wait_until(
        || {
            let screen = tui.screen_text();
            screen.contains("cat") && screen.contains("📟")
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
            screen.contains("keyboard reference") && screen.contains("Ctrl+B then ?")
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
    // agent identifier and both per-agent icons, then switch to Keyboard,
    // select a custom warned letter, and restore the tmux preset. These
    // assertions cover actual rendered controls and persisted config rather
    // than only config/keymap units.
    tui.write(b"\x1b").expect("closing Help with Esc");
    let help_closed = wait_until(
        || !tui.screen_text().contains("keyboard reference"),
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        help_closed,
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
            screen.contains("Agent identifier")
                && screen.contains("Full name")
                && screen.contains("Claude icon")
                && screen.contains("🧠 Brain")
                && screen.contains("Codex icon")
                && screen.contains("⚙️")
                && screen.contains("Gear")
        },
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        agent_controls_shown,
        "expected all agent identifier controls in User Appearance, got: {:?}",
        tui.screen_text()
    );
    tui.write(b"\x1b[B\x1b[B\x1b[C\x1b[C\x1b[B\x1b[C\x1b[B\x1b[C")
        .expect("selecting icon mode, Claude magic wand, and Codex tools");
    let agent_controls_persisted = wait_until(
        || {
            let config = std::fs::read_to_string(xdg.config_home.join("ilium").join("config.toml"))
                .unwrap_or_default();
            config.contains("agent_identifier_mode = \"icon\"")
                && config.contains("claude_agent_icon = \"magic_wand\"")
                && config.contains("codex_agent_icon = \"tools\"")
        },
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        agent_controls_persisted,
        "expected agent identifier choices to persist, config={:?}",
        std::fs::read_to_string(xdg.config_home.join("ilium").join("config.toml"))
    );
    let selected_icons_shown = wait_until(
        || {
            let screen = tui.screen_text();
            screen.contains("Selected icon")
                && screen.contains("🪄")
                && screen.contains("Magic wand")
                && screen.contains("🛠️")
                && screen.contains("Tools")
        },
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        selected_icons_shown,
        "expected selected agent icon controls to update live, got: {:?}",
        tui.screen_text()
    );
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

    tui.write(b"C")
        .expect("selecting custom shortcut base Ctrl+C");
    let custom_warning_shown = wait_until(
        || {
            let screen = tui.screen_text();
            screen.contains("Warning: Ctrl+C") && screen.contains("interrupt/SIGINT")
        },
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        custom_warning_shown,
        "expected the specific Ctrl+C warning, got: {:?}",
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

    // One more Tab enters the new Sound settings surface. Exercise a real
    // event checkbox without activating Preview, so this remains a silent
    // automated test while proving rendering, keyboard interaction, atomic
    // config persistence, and live request dispatch all occur in the real TUI.
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
    tui.write(b"\x1b[B\x1b[B\x1b[B ")
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

/// Drives split creation, multi-pane rendering, and focus-specific input
/// through the real CLI, detached server, IPC connection, PTYs, and TUI.
/// Unit tests pin exact rectangles; this test proves those layers remain
/// connected when two live terminal streams share the right panel.
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

    // Focus and expand the default group, then open the split dialog from
    // the real default leader binding (Ctrl+A, Shift+W).
    tui.write(b"\x01t").expect("focus tree");
    tui.write(b"\x1b[B").expect("select default group");
    tui.write(b"\x1b[C").expect("expand default group");
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
        || tui.screen_text().matches("cat").count() >= 4,
        WAIT_TIMEOUT,
    )
    .await;
    assert!(split_children_visible, "split child rows did not render");
    tui.write(b"\x1b[B").expect("select first split child");
    tui.write(b"\r").expect("focus first split child");
    tui.write(b"left-route\r")
        .expect("type into first split child");
    let first_routed = wait_until(|| tui.screen_text().contains("left-route"), WAIT_TIMEOUT).await;
    assert!(first_routed, "first split child did not receive input");

    tui.write(b"\x01t").expect("return focus to split tree");
    tui.write(b"\x1b[B").expect("select second split child");
    tui.write(b"\r").expect("focus second split child");
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
fn rows_containing(screen: &vt100::Screen, needle: &str) -> Vec<u16> {
    let cols = screen.size().1;
    screen
        .rows(0, cols)
        .enumerate()
        .filter(|(_, text)| text.contains(needle))
        .map(|(index, _)| index as u16)
        .collect()
}

/// True if any cell in `row` is currently rendered in inverse video --
/// exactly what `ilium_client::tree_ui`'s creation-pulse flash
/// (`Modifier::REVERSED`, applied by `apply_recent_pulse`) produces on a
/// freshly created node's row.
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

/// Covers the feature this file's other test doesn't: a freshly created
/// pane must visibly flash (so a click on the tree panel's creation
/// toolbar is obviously followed by something appearing -- see
/// `ilium_client::tree_ui`'s `RECENTLY_CREATED_PULSE_MS`/
/// `apply_recent_pulse`), the flash must fade once its window elapses, and
/// this must hold even when several panes are created in one burst.
///
/// `Ctrl+A c` (`ilium_client::keymap::Action::NewTerminal`) drives exactly
/// the same `App::action_new_terminal` the tree panel's "new shell"
/// toolbar button (`TreeToolbarAction::Shell`) calls -- the pulse itself
/// lives entirely downstream of that call, in `render_cache`/`tree_ui`, so
/// this exercises the real end-to-end pipeline (keystroke -> server ->
/// tree snapshot -> render) without needing to reverse-engineer the
/// toolbar's exact pixel position at this pty's fixed size.
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

    // A freshly attached client starts with every group collapsed --
    // expand the default group (same technique as this file's other test)
    // so the two new "shell" panes are actually listed.
    tui.write(b"\x01t")
        .expect("writing Ctrl+A then t (FocusTree)");
    tui.write(b"\x1b[B").expect("writing Down arrow");
    tui.write(b"\x1b[C").expect("writing Right arrow");

    let both_panes_listed = wait_until(
        || tui.screen_text().matches("shell").count() >= 2,
        WAIT_TIMEOUT,
    )
    .await;
    assert!(
        both_panes_listed,
        "expected two \"shell\" pane rows after a two-press create burst, got: {:?}",
        tui.screen_text()
    );

    // A `PlainShell` pane row is `📟` (node icon, padded to
    // `tree_ui::NODE_ICON_COLUMN_WIDTH`) + an empty activity-icon column
    // (padded to `tree_ui::ACTIVITY_ICON_COLUMN_WIDTH`) + the name. Match
    // that fixed-width label rather than the toolbar's separate 📟 button.
    let rows = tui.with_screen(|screen| rows_containing(screen, "📟   shell"));
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
            "#!/bin/sh\nprintf 'STARTED' > '{}'\nIFS= read -r line\nprintf '%s' \"$line\" > '{}'\nsleep 30\n",
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

    tui.write(&sgr_mouse_down(0, context_column + 1, target_row + 1))
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
