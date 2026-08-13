//! The fake agent CLI the integration tests spawn.
//!
//! Reads its behaviour from the sidecar file beside its own executable (see
//! `ilium_test_fixtures`'s crate docs for why the behaviour cannot travel in
//! argv or the environment), then reproduces exactly one of the shell scripts
//! these fixtures replaced.
//!
//! Everything here is deliberately written against the standard library and
//! `crossterm` only, so it behaves the same under a Unix pty and under
//! Windows' ConPTY -- which is the entire point of the crate.

use std::fs::File;
use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use ilium_test_fixtures::{behavior_file_for, FixtureBehavior};

/// How long a fixture lingers after finishing its scripted behaviour.
///
/// Long enough that no test can outlive it by accident, and harmless: every
/// fixture is spawned into a pane the test kills, and the OS reaps it with the
/// session either way.
const LINGER: Duration = Duration::from_secs(60);

/// Poll interval while waiting on a marker file. Short relative to the
/// detection loop's own poll interval in these tests (100-200ms), so the
/// fixture is never the thing that makes a transition look slow.
const MARKER_POLL_INTERVAL: Duration = Duration::from_millis(50);

fn main() {
    let behavior = load_behavior();
    match behavior {
        FixtureBehavior::Idle => linger(),
        FixtureBehavior::WorkingThenIdle { working_seconds } => {
            run_working_then_idle(working_seconds)
        }
        FixtureBehavior::WorkingUntilMarker { marker_path } => {
            run_working_until_marker(&marker_path)
        }
        FixtureBehavior::HoldArgument { argument_index } => {
            // Bound to a named local, not `_`: `let _ = File::open(..)` drops
            // the handle immediately, which is precisely the opposite of this
            // fixture's whole purpose.
            let _held_open = open_argument(argument_index);
            linger();
        }
        FixtureBehavior::ResumePrompt => run_resume_prompt(),
        FixtureBehavior::ClearTransition {
            first_argument_index,
            second_argument_index,
        } => run_clear_transition(first_argument_index, second_argument_index),
        FixtureBehavior::ChangeOnly => run_change_only(),
        FixtureBehavior::EchoSubmittedLine { prefix } => {
            if let Some(line) = read_submitted_line() {
                emit(&format!("{prefix}:<{line}>\r\n"));
            }
            linger();
        }
        FixtureBehavior::DelayedComposerThenEcho { delay_seconds } => {
            run_delayed_composer_then_echo(delay_seconds)
        }
        FixtureBehavior::EmitNumberedLines { prefix, count } => {
            for number in 1..=count {
                emit(&format!("{prefix}-{number:03}\r\n"));
            }
            linger();
        }
        FixtureBehavior::RecordThenRunTarget {
            marker_variable,
            target_variable,
            marker_text,
        } => run_record_then_target(&marker_variable, &target_variable, &marker_text),
        FixtureBehavior::ShellImpersonator {
            intercepted_command,
            replacement,
        } => run_shell_impersonator(&intercepted_command, &replacement),
        FixtureBehavior::RecordSubmittedPrompt { transcript_path } => {
            run_record_submitted_prompt(&transcript_path)
        }
        FixtureBehavior::SpawnChild { child_path } => run_spawn_child(&child_path),
    }
}

/// Spawns `child_path` as this process's own child, then lingers.
///
/// The child is reaped on its own thread rather than this one: this process
/// still needs to linger (so a caller has time to walk the process tree and
/// find the child), but blocking here on `Child::wait` would defeat that --
/// the reaping thread outlives `linger()`'s own duration, so the child never
/// sits as a zombie in the meantime either.
fn run_spawn_child(child_path: &std::path::Path) {
    let mut child = std::process::Command::new(child_path)
        .spawn()
        .unwrap_or_else(|error| panic!("spawn child fixture {}: {error}", child_path.display()));
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    linger();
}

/// Records that the replacement ran, then becomes the real program.
///
/// On Unix this `exec`s, replacing the process image, so the pid keeps naming
/// the same process while its executable changes -- which is what an upgrade
/// of a running binary actually looks like, and what the caller observes.
/// Windows has no `exec`, so there the target runs as a child and this process
/// waits on it: the marker and the target's behaviour are identical, only the
/// process identity differs.
fn run_record_then_target(marker_variable: &str, target_variable: &str, marker_text: &str) {
    let marker_path = std::env::var_os(marker_variable)
        .unwrap_or_else(|| panic!("fixture expected ${marker_variable} to name a marker path"));
    std::fs::write(&marker_path, marker_text)
        .unwrap_or_else(|error| panic!("fixture writing its marker: {error}"));

    let target = std::env::var_os(target_variable)
        .unwrap_or_else(|| panic!("fixture expected ${target_variable} to name a program"));
    let mut command = std::process::Command::new(&target);
    command.args(std::env::args_os().skip(1));

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        // Only returns on failure.
        let error = command.exec();
        panic!("fixture exec'ing {target:?}: {error}");
    }

    #[cfg(not(unix))]
    {
        let status = command
            .status()
            .unwrap_or_else(|error| panic!("fixture running {target:?}: {error}"));
        std::process::exit(status.code().unwrap_or(1));
    }
}

fn run_shell_impersonator(intercepted_command: &str, replacement: &std::path::Path) {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    // `-c <command line>` is how every shell ilium spawns is invoked, except
    // `cmd.exe`; `ilium_server::pane::shell_command` picks the flag from the
    // shell's name, and this fixture is not named `cmd`.
    let intercepted = matches!(arguments.as_slice(), [flag, command] if flag == "-c" && command == intercepted_command);
    let status = if intercepted {
        std::process::Command::new(replacement).status()
    } else {
        let (shell, flag) = real_shell();
        std::process::Command::new(shell)
            .arg(flag)
            .args(&arguments[arguments.len().min(1)..])
            .status()
    };
    let status = status.unwrap_or_else(|error| panic!("fixture shell dispatching: {error}"));
    std::process::exit(status.code().unwrap_or(1));
}

/// The platform's own shell and its run-one-command flag, mirroring
/// `ilium_server::pane::shell_command`'s fallbacks.
fn real_shell() -> (String, &'static str) {
    if cfg!(windows) {
        let shell = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());
        (shell, "/C")
    } else {
        ("/bin/sh".to_string(), "-c")
    }
}

fn run_record_submitted_prompt(transcript_path: &std::path::Path) {
    std::fs::write(transcript_path, "STARTED")
        .unwrap_or_else(|error| panic!("fixture recording its start: {error}"));
    // The leading spaces match the composer shape `ilium-detect` looks for.
    emit("  send a message\r\n");
    if let Some(line) = read_submitted_line() {
        std::fs::write(transcript_path, line)
            .unwrap_or_else(|error| panic!("fixture recording the submitted prompt: {error}"));
    }
    linger();
}

/// Reads the sidecar written by `ilium_test_fixtures::install`.
///
/// Panics loudly rather than defaulting to some benign behaviour: a fixture
/// that silently does nothing would surface as an unexplained detection
/// timeout in a test far from the actual mistake.
fn load_behavior() -> FixtureBehavior {
    let executable = std::env::current_exe().expect("a running fixture has a path");
    let behavior_path = behavior_file_for(&executable);
    let contents = std::fs::read(&behavior_path).unwrap_or_else(|error| {
        panic!(
            "fixture {} has no behaviour file at {}: {error}",
            executable.display(),
            behavior_path.display()
        )
    });
    serde_json::from_slice(&contents).unwrap_or_else(|error| {
        panic!(
            "parse fixture behaviour at {}: {error}",
            behavior_path.display()
        )
    })
}

/// Writes `text` and flushes.
///
/// Every write here has to reach the pty before the fixture blocks on
/// something, because the detection loop's whole input is what is currently on
/// screen. Buffered output that arrives after the poll is output the test
/// never sees.
fn emit(text: &str) {
    let mut stdout = std::io::stdout();
    stdout
        .write_all(text.as_bytes())
        .expect("write to the fixture's stdout");
    stdout.flush().expect("flush the fixture's stdout");
}

/// Clears the screen and homes the cursor.
///
/// Load-bearing, not cosmetic: see `FixtureBehavior::WorkingThenIdle`.
fn clear_screen() {
    emit("\x1b[2J\x1b[H");
}

fn linger() {
    std::thread::sleep(LINGER);
}

/// Resolves argv at a 1-based index, matching the `$1`/`$2` the replaced shell
/// scripts used.
fn argument(index: usize) -> PathBuf {
    let arguments: Vec<String> = std::env::args().collect();
    arguments
        .get(index)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("fixture expected argv[{index}], got {arguments:?}"))
}

fn open_argument(index: usize) -> File {
    let path = argument(index);
    File::open(&path).unwrap_or_else(|error| panic!("fixture opening {}: {error}", path.display()))
}

/// Reads one line from stdin, without the trailing newline.
///
/// `None` at end of input, so a fixture whose pane is closed mid-wait exits
/// instead of spinning.
fn read_submitted_line() -> Option<String> {
    let mut line = String::new();
    match std::io::stdin().lock().read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(line.trim_end_matches(['\r', '\n']).to_string()),
    }
}

fn run_working_then_idle(working_seconds: u32) {
    for _ in 0..working_seconds {
        // Both rows in one write, so no observer can see the goal line
        // without the activity line -- see `run_change_only`.
        emit(
            "gpt-5.6-sol xhigh · workspace · Working · Pursuing goal (5m)\r\n\
             Cogitating (esc to interrupt)\r\n",
        );
        std::thread::sleep(Duration::from_secs(1));
    }
    clear_screen();
    emit("Done. Ready for the next instruction.\r\n");
    if let Some(queued_prompt) = read_submitted_line() {
        emit(&format!("queued:<{queued_prompt}>\r\n"));
    }
    linger();
}

fn run_working_until_marker(marker_path: &std::path::Path) {
    emit("Cogitating (esc to interrupt)\r\n");
    while !marker_path.exists() {
        std::thread::sleep(MARKER_POLL_INTERVAL);
    }
    clear_screen();
    emit("Done. Ready for the next instruction.\r\n");
    linger();
}

/// Claude Code's real "resume full session" dialog, positioned in the bottom
/// rows of the screen because `ilium_detect::interstitial_prompt_response`
/// only matches anchors found there -- that positional rule is what stops the
/// same wording, quoted inside a transcript, from being answered as a prompt.
fn run_resume_prompt() {
    /// Blank lines printed first, to push the dialog down into the anchor rows
    /// of a `DEFAULT_PANE_ROWS`-tall screen.
    const LEADING_BLANK_ROWS: usize = 15;

    for _ in 0..LEADING_BLANK_ROWS {
        emit("\r\n");
    }
    emit("  This session is 3d 17h old and 470.9k tokens.\r\n");
    emit("\r\n");
    emit(
        "  Resuming the full session will consume a substantial portion of your \
         usage limits. We recommend resuming from a summary.\r\n",
    );
    emit("\r\n");
    emit("  > 1. Resume from summary (recommended)\r\n");
    emit("    2. Resume full session as-is\r\n");
    emit("    3. Don't ask me again\r\n");
    emit("\r\n");
    emit("  Enter to confirm . Esc to cancel\r\n");

    let key = read_single_key();
    clear_screen();
    if key == Some(b'2') {
        emit("AUTO_RESUME_OK\r\n");
    } else {
        emit(&format!("AUTO_RESUME_UNEXPECTED({key:?})\r\n"));
    }
    linger();
}

/// Reads exactly one byte with line discipline disabled.
///
/// Raw mode is what makes a bare digit arrive without an Enter, which is the
/// behaviour the real dialog has and therefore the behaviour the auto-answer
/// is written against. `crossterm` is used rather than a `#[cfg]` pair of
/// `tcsetattr`/`SetConsoleMode` calls because it already owns exactly this
/// difference for the client.
fn read_single_key() -> Option<u8> {
    crossterm::terminal::enable_raw_mode().expect("fixture entering raw mode");
    let mut byte = [0u8; 1];
    let outcome = std::io::stdin().read_exact(&mut byte);
    // Restored before returning so the fixture's own later output is not
    // affected by the mode it borrowed.
    crossterm::terminal::disable_raw_mode().expect("fixture leaving raw mode");
    outcome.ok().map(|()| byte[0])
}

fn run_clear_transition(first_argument_index: usize, second_argument_index: usize) {
    let _first_rollout = open_argument(first_argument_index);
    let mut second_rollout: Option<File> = None;
    let mut cleared = false;

    emit("Done. Ready for the next instruction.\r\n");
    while let Some(submitted) = read_submitted_line() {
        if submitted == "/clear" {
            cleared = true;
            clear_screen();
            emit("new conversation started\r\n");
        } else if cleared {
            // The second rollout opens only on the prompt *after* `/clear`,
            // which is the exact lifecycle Codex 0.144.6 has and the reason
            // rebinding cannot simply key off the process identity.
            second_rollout = Some(open_argument(second_argument_index));
            cleared = false;
            emit("23\r\n");
        }
    }
    drop(second_rollout);
}

/// Unreadable for a moment, then visibly ready.
///
/// The first line exists so the pane is demonstrably alive and producing
/// output while still *not* showing a composer -- otherwise "waited for
/// readiness" and "was slow to spawn" would look identical from outside.
fn run_delayed_composer_then_echo(delay_seconds: u32) {
    emit("starting codex...\r\n");
    std::thread::sleep(Duration::from_secs(u64::from(delay_seconds)));
    clear_screen();
    // The leading `›` is the stable part of Codex's composer contract; the
    // placeholder text after it rotates and is deliberately not asserted on.
    emit("› Explain this codebase\r\n");
    if let Some(line) = read_submitted_line() {
        emit(&format!("received-after-ready:<{line}>\r\n"));
    }
    linger();
}

/// Repaints in place: cursor home, then two rows whose numbers change while
/// their structure does not.
fn run_change_only() {
    let mut counter: u64 = 1;
    loop {
        // Built as one string rather than two `emit` calls. Note this is not
        // yet a single syscall: Rust's stdout is a `LineWriter`, so it still
        // flushes at the embedded newline. It narrows the window rather than
        // closing it, and did not fix the torn-frame failures it was aimed at
        // -- see docs/TODO.md.
        emit(&format!(
            "\x1b[Hmodel · workspace · Working · Pursuing goal ({counter}m)\x1b[K\r\n\
             Cogitating (esc to interrupt) · {counter}s · {counter} tokens\x1b[K"
        ));
        counter += 1;
        std::thread::sleep(Duration::from_secs(1));
    }
}
