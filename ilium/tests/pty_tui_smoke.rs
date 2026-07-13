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
//! 2. `ilium --fresh --cwd <dir>` -- the actual attaching form, explicitly
//!    replacing phase 1's server -- run inside a real pty at a fixed size.
//!    Its first rendered frame is
//!    asserted to contain structural chrome (the sidebar title from
//!    `ilium_client::tree_ui::sidebar_title`) and the pane created in
//!    phase 1 (named after its command line, `"cat"`, per
//!    `TerminalOrigin::default_pane_name`). A scripted leader-key + help
//!    keystroke (`Ctrl+A` then `?`, see `ilium_client::keymap`) is then
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
/// spawning the server (phase 1's `new-pane`, or `--fresh` in phase 2) and
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
                    Ok(None) if std::time::Instant::now() >= deadline => break,
                    Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                    Err(_) => break,
                }
            }
        }
    }
}

/// Path to the `ilium` binary this test itself was built alongside --
/// Cargo sets this for every integration test in a package that also has
/// a `[[bin]]` (or, as here, the implicit `src/main.rs` binary) target.
fn ilium_binary() -> &'static str {
    env!("CARGO_BIN_EXE_ilium")
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

#[tokio::test]
async fn attaching_tui_renders_the_pane_created_by_new_pane_and_responds_to_the_help_keystroke() {
    let temp_root = tempfile::tempdir().expect("create tempdir");
    let xdg = IsolatedXdgDirs::under(temp_root.path()).expect("create isolated XDG dirs");
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
        .arg("--fresh")
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
    // documented leader key -- `ilium_client::keymap::is_leader_key`)
    // followed by `?` (`ilium_client::keymap::Action::Help`'s bound
    // letter) must flip the render to show
    // `ilium_client::help::render`'s overlay -- proof that keystrokes
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

    // A `PlainShell` pane row is `>` (node icon, padded to
    // `tree_ui::NODE_ICON_COLUMN_WIDTH`) + an empty activity-icon column
    // (padded to `tree_ui::ACTIVITY_ICON_COLUMN_WIDTH`) + the name -- four
    // spaces between `>` and `shell`, not one, per `tree_ui::node_label`.
    let rows = tui.with_screen(|screen| rows_containing(screen, ">    shell"));
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
