//! Fake agent CLIs for the integration tests, as real executables.
//!
//! Every fake `codex`/`claude` used to be a `#!/bin/sh` script written into a
//! tempdir. That worked, and it is why `ilium-server`'s `smoke.rs` and
//! `live_agent_detection.rs` were `#![cfg(unix)]` at file level: Windows has
//! no `/bin/sh`, so the entire server-integration and agent-detection suites
//! simply did not run there. Since those are the two suites that cover the
//! product's core behaviour, "Windows is green" meant far less than it looked.
//!
//! The replacement is one real binary, [`FIXTURE_BINARY_NAME`], built by cargo
//! for whatever platform the suite is running on. A test [`install`]s a copy of
//! it under the name detection has to match (`codex`, `claude`, ...) and pairs
//! it with a [`FixtureBehavior`].
//!
//! # Why the behaviour travels in a sidecar file rather than argv
//!
//! The obvious design -- `ilium-fixture-agent working-then-idle --seconds 5` --
//! cannot work here. `ilium_server::session_id`'s "startup arguments" discovery
//! phase *parses the agent process's own argv* looking for a provider's resume
//! grammar (`--resume <id>`), and the tests drive that phase deliberately. Any
//! argument this crate injected would sit in the same argv the code under test
//! is reading, changing the very input the assertion is about.
//!
//! An environment variable has the opposite problem: a pane runs its command
//! through `$SHELL -c` (or `cmd.exe /C` on Windows), and `VAR=value program`
//! is POSIX shell syntax that `cmd.exe` does not understand.
//!
//! So the behaviour is written to `<executable path>.fixture.json`, which the
//! binary reads from its own [`std::env::current_exe`]. argv stays exactly what
//! the test passed, byte for byte, on every platform.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The cargo bin target this crate builds. Tests never invoke it under this
/// name -- [`install`] copies it to whatever the detection registry must
/// match -- but this is the name to look for under `target/<profile>/`.
pub const FIXTURE_BINARY_NAME: &str = "ilium-fixture-agent";

/// Suffix appended to an installed fixture's own path to find its behaviour.
/// A suffix (rather than a fixed file name in the same directory) so several
/// differently-behaved fixtures can share one directory.
pub const BEHAVIOR_FILE_SUFFIX: &str = ".fixture.json";

/// What an installed fixture does when it runs.
///
/// Each variant reproduces one of the shell scripts these fixtures replaced;
/// the doc comments record the observable contract each test depends on, since
/// that contract -- not the implementation -- is what a future edit must
/// preserve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum FixtureBehavior {
    /// Sits there. Used by the argument-based session-ID discovery tests,
    /// which care only about argv and never about activity classification.
    Idle,

    /// Prints Codex's shared-footer goal suffix plus the literal working
    /// marker (`ilium_detect`'s `"esc to interrupt"`) once a second for
    /// `working_seconds`, then *clears the screen* and prints an idle line.
    ///
    /// The clear matters: `vt100::Screen::contents()` reflects the visible
    /// screen, not scrollback, so a real `Working -> Idle` reclassification
    /// needs the marker to stop being present, not merely to stop being
    /// reprinted.
    ///
    /// Afterwards it reads one line and echoes it back as `queued:<line>`,
    /// which is how the queued-prompt tests observe delivery.
    WorkingThenIdle { working_seconds: u32 },

    /// Prints the working marker, then holds that state until `marker_path`
    /// exists, then clears and reports completion.
    ///
    /// Marker-driven rather than timed on purpose: a fixture that works for a
    /// fixed two seconds is only *seen* working if a detection poll lands
    /// inside that window, and on a loaded CI runner the first poll after pane
    /// creation routinely does not. Timing luck is not the behaviour under
    /// test.
    WorkingUntilMarker { marker_path: PathBuf },

    /// Opens the file named by argv at `argument_index` (1-based, so `1` is
    /// the first argument after the program name, matching the `$1` the shell
    /// script used) and holds it open for the process's whole life.
    ///
    /// This is what `session_id`'s "open transcript descriptors" phase reads.
    HoldArgument { argument_index: usize },

    /// Renders Claude Code's real "resume full session" dialog in the bottom
    /// rows of the screen, then reads exactly one byte with the terminal in
    /// raw mode -- as the real dialog does, committing on a bare digit with no
    /// Enter. Prints `AUTO_RESUME_OK` only if that byte was `2`, so the test
    /// asserts on the keystroke actually injected rather than on the dialog
    /// merely having appeared.
    ResumePrompt,

    /// Reproduces Codex 0.144.6's `/clear` descriptor lifecycle: hold argv's
    /// `first_argument_index` open, and when `/clear` is submitted reset the
    /// conversation in place; the *next* submitted prompt opens
    /// `second_argument_index` as a second rollout without the process ever
    /// being replaced.
    ClearTransition {
        first_argument_index: usize,
        second_argument_index: usize,
    },

    /// Repaints a goal/activity footer whose *numbers* change every second
    /// while its structure does not, by homing the cursor rather than
    /// scrolling. Exercises the "screen changed but the classification did
    /// not" path.
    ChangeOnly,

    /// Minimal raw-mode Codex-like composer that exposes an active goal,
    /// accepts Ilium's `/goal pause`, one multiline progress result, and
    /// `/goal resume`, recording each semantic submission in order.
    GoalLifecycle { log_path: PathBuf },

    /// Prints the current contents of one file and exits. Used as a portable
    /// progress probe without depending on `cat`, PowerShell, or Python.
    PrintFile { path: PathBuf },

    /// Repaints one matching row, clears it, then displays a new matching
    /// occurrence in the same position. Used to distinguish redraws from
    /// genuinely separate Text Trigger appearances.
    RepaintThenReappear,

    /// Reads one line and echoes it back as `<prefix>:<line>`.
    ///
    /// Replaces the inline `IFS= read -r line; printf ...` command lines the
    /// submission tests used to spawn. Those proved a real Enter reached a
    /// real reader -- which a fixture proves just as well, without the pane's
    /// command line having to be POSIX shell syntax that `cmd.exe` cannot
    /// parse.
    EchoSubmittedLine { prefix: String },

    /// Prints nothing readable for `delay_seconds`, then clears the screen and
    /// shows Codex's composer cursor before reading a line and echoing it as
    /// `received-after-ready:<line>`.
    ///
    /// The delay is the point: it proves initial input waits for a *visibly*
    /// ready composer rather than merely racing the pty spawn.
    DelayedComposerThenEcho { delay_seconds: u32 },

    /// Prints `count` lines of `<prefix>-NNN`, zero-padded to three digits,
    /// then lingers. Used to fill a pane's scrollback with output whose last
    /// line is unambiguous to wait for.
    EmitNumberedLines { prefix: String, count: u32 },

    /// Writes `marker_text` to the path in `$marker_variable`, then runs the
    /// program named by `$target_variable` with this process's own arguments
    /// and exits with its status.
    ///
    /// Stands in for the client binary during the restart test: the marker
    /// proves the replacement on disk was the thing that ran, and chaining to
    /// the real target keeps the client actually working afterwards.
    RecordThenRunTarget {
        marker_variable: String,
        target_variable: String,
        marker_text: String,
    },

    /// Impersonates the user's shell. Invoked as `<this> -c <command line>`:
    /// when the command line is exactly `intercepted_command`, runs
    /// `replacement` instead; anything else is handed to the platform's real
    /// shell.
    ///
    /// Lets a test prove which agent a UI action launched, without a real
    /// agent CLI installed and without depending on `PATH` resolution order.
    ShellImpersonator {
        intercepted_command: String,
        replacement: PathBuf,
    },

    /// A minimal agent: records that it started, shows a composer, then
    /// records the first line submitted to it. Both go to `transcript_path`,
    /// so a test can tell "never launched" from "launched but never received
    /// the prompt" -- two failures that look identical from the screen alone.
    RecordSubmittedPrompt { transcript_path: PathBuf },

    /// Spawns `child_path` as its own child process, then lingers.
    ///
    /// Reproduces an interpreter-launcher install shape (e.g. Bun's global
    /// bin shim: `node <path-ending-in-codex>`) where the process sitting
    /// directly on the pane's shell never becomes the real agent CLI --
    /// it *spawns* the real native binary as a further child rather than
    /// exec-replacing itself. Install this behavior under a name
    /// `ilium_detect::identify_agent`'s interpreter list recognises (e.g.
    /// `node`), pass `child_path` a second fixture installed under an
    /// agent-matching name, and pass the spawned process an argument whose
    /// file name also matches that agent (see `identifying_process_names`'s
    /// argv-unwrapping fallback) to reproduce the ambiguity: two plausible
    /// matches at different tree depths, only one of which is real.
    SpawnChild { child_path: PathBuf },
}

/// A fixture executable installed on disk, ready to be spawned by absolute
/// path.
#[derive(Debug, Clone)]
pub struct InstalledFixture {
    /// Absolute path to the copied executable. Always spawn by this path,
    /// never by adding its directory to `PATH`: a `PATH`-resolved fake can
    /// lose a race against a real agent CLI installed on the machine running
    /// the suite, which is the one failure these fixtures exist to never risk.
    pub path: PathBuf,
}

/// Copies the built fixture binary into `directory` under `name`, and records
/// `behavior` beside it.
///
/// `name` is the bare name detection must match (`codex`, `claude`); the
/// platform's executable extension is appended here, so callers never write
/// `.exe`. Substring matching in `ilium_detect::match_process_name_with_extra`
/// means `codex.exe` still matches the `codex` signature.
pub fn install(directory: &Path, name: &str, behavior: &FixtureBehavior) -> InstalledFixture {
    let path = directory.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    let source = fixture_binary_path();
    std::fs::copy(&source, &path)
        .unwrap_or_else(|error| panic!("copy {} to {}: {error}", source.display(), path.display()));
    make_executable(&path);

    let behavior_path = behavior_file_for(&path);
    let serialized =
        serde_json::to_vec_pretty(behavior).expect("a FixtureBehavior always serializes");
    std::fs::write(&behavior_path, serialized)
        .unwrap_or_else(|error| panic!("write {}: {error}", behavior_path.display()));

    InstalledFixture { path }
}

/// The sidecar path holding an installed fixture's behaviour. Shared by
/// [`install`] and the binary so the two can never disagree about it.
pub fn behavior_file_for(executable: &Path) -> PathBuf {
    let mut file_name = executable.as_os_str().to_os_string();
    file_name.push(BEHAVIOR_FILE_SUFFIX);
    PathBuf::from(file_name)
}

/// Locates the fixture binary cargo built for this test run.
///
/// `CARGO_BIN_EXE_<name>` only exists for bins in the *same* package as the
/// test, and these tests live in `ilium-server`, `ilium` and `ilium-detect`, so
/// the path is derived from the running test executable instead.
///
/// Deliberately does *not* shell out to cargo when the binary is missing. An
/// inner `cargo build` blocks on the outer `cargo test`'s build lock, and
/// builds under a different feature unification than the outer invocation, so
/// the "helpful" fallback turned a clear failure into tests that hung for
/// minutes and then timed out. A precise panic is worth more than a fallback
/// that fights the build it is running inside.
fn fixture_binary_path() -> PathBuf {
    static RESOLVED: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    RESOLVED.get_or_init(resolve_fixture_binary_path).clone()
}

fn resolve_fixture_binary_path() -> PathBuf {
    let test_executable = std::env::current_exe().expect("the running test has a path");
    let candidates = fixture_binary_candidates(&test_executable);
    candidates
        .iter()
        .find(|candidate| candidate.is_file())
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "the `{FIXTURE_BINARY_NAME}` binary these tests spawn has not been built.\n\
                 `cargo test --workspace` (what CI runs) builds it; a single-package \
                 `cargo test -p ...` does not.\n\
                 Fix with: cargo build -p ilium-test-fixtures --bin {FIXTURE_BINARY_NAME}\n\
                 Looked in: {candidates:#?}",
            )
        })
}

/// Every directory cargo could plausibly have put the bin in, given where it
/// put this test.
///
/// Derived from the test executable's own ancestors rather than assumed,
/// because the layout differs between a plain build (`target/debug/`) and a
/// cross-compiled one (`target/<triple>/debug/`), and `CARGO_TARGET_DIR` can
/// move the root anywhere.
fn fixture_binary_candidates(test_executable: &Path) -> Vec<PathBuf> {
    let binary_name = format!("{FIXTURE_BINARY_NAME}{}", std::env::consts::EXE_SUFFIX);
    let mut directories: Vec<PathBuf> = Vec::new();
    let mut remember = |directory: PathBuf| {
        if !directories.contains(&directory) {
            directories.push(directory);
        }
    };

    // The test binary itself lives in `<profile>/deps/`; cargo has also placed
    // bins directly beside tests in the past, so both are worth checking.
    if let Some(deps_directory) = test_executable.parent() {
        remember(deps_directory.to_path_buf());
    }
    for ancestor in test_executable.ancestors() {
        match ancestor.file_name().and_then(|name| name.to_str()) {
            Some(profile @ ("debug" | "release")) => {
                let _ = profile;
                remember(ancestor.to_path_buf());
            }
            Some("target") => {
                remember(ancestor.join("debug"));
                remember(ancestor.join("release"));
            }
            _ => {}
        }
    }

    directories
        .into_iter()
        .map(|directory| directory.join(&binary_name))
        .collect()
}

// Owner-only: this copy is never run by anything but this test process's own
// children. `ilium-platform` owns every OS-specific permission decision, this
// crate included, so the actual `chmod` (and the Windows no-op, since
// executability there comes from the extension `std::env::consts::EXE_SUFFIX`
// already gave the copy) lives there.
fn make_executable(path: &Path) {
    ilium_platform::secure_fs::restrict_executable_file_to_owner(path)
        .unwrap_or_else(|error| panic!("chmod {}: {error}", path.display()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn behavior_file_sits_beside_the_executable_including_its_extension() {
        // Deliberately checked with an extension present: appending to the
        // full file name (rather than replacing the extension) is what lets
        // `codex.exe` and a hypothetical `codex` coexist in one directory.
        let executable = Path::new("/tmp/fixtures/codex.exe");
        assert_eq!(
            behavior_file_for(executable),
            PathBuf::from("/tmp/fixtures/codex.exe.fixture.json")
        );
    }

    /// Exhaustively matched, with no wildcard arm, so a new
    /// [`FixtureBehavior`] variant fails this file's own compile rather than
    /// silently going untested by
    /// `every_behavior_round_trips_through_its_sidecar_encoding` below.
    fn assert_every_variant_is_covered(behavior: &FixtureBehavior) {
        match behavior {
            FixtureBehavior::Idle
            | FixtureBehavior::WorkingThenIdle { .. }
            | FixtureBehavior::WorkingUntilMarker { .. }
            | FixtureBehavior::HoldArgument { .. }
            | FixtureBehavior::ResumePrompt
            | FixtureBehavior::ClearTransition { .. }
            | FixtureBehavior::ChangeOnly
            | FixtureBehavior::GoalLifecycle { .. }
            | FixtureBehavior::PrintFile { .. }
            | FixtureBehavior::RepaintThenReappear
            | FixtureBehavior::EchoSubmittedLine { .. }
            | FixtureBehavior::DelayedComposerThenEcho { .. }
            | FixtureBehavior::EmitNumberedLines { .. }
            | FixtureBehavior::RecordThenRunTarget { .. }
            | FixtureBehavior::ShellImpersonator { .. }
            | FixtureBehavior::RecordSubmittedPrompt { .. }
            | FixtureBehavior::SpawnChild { .. } => {}
        }
    }

    #[test]
    fn every_behavior_round_trips_through_its_sidecar_encoding() {
        // The binary and the installer are separate processes that agree only
        // via this encoding, so a variant that fails to round-trip is a
        // fixture that silently does the wrong thing.
        let behaviors = [
            FixtureBehavior::Idle,
            FixtureBehavior::WorkingThenIdle { working_seconds: 5 },
            FixtureBehavior::WorkingUntilMarker {
                marker_path: PathBuf::from("/tmp/marker"),
            },
            FixtureBehavior::HoldArgument { argument_index: 1 },
            FixtureBehavior::ResumePrompt,
            FixtureBehavior::ClearTransition {
                first_argument_index: 1,
                second_argument_index: 2,
            },
            FixtureBehavior::ChangeOnly,
            FixtureBehavior::GoalLifecycle {
                log_path: PathBuf::from("/tmp/goal-lifecycle"),
            },
            FixtureBehavior::PrintFile {
                path: PathBuf::from("/tmp/progress-report"),
            },
            FixtureBehavior::RepaintThenReappear,
            FixtureBehavior::EchoSubmittedLine {
                prefix: "queued".to_string(),
            },
            FixtureBehavior::DelayedComposerThenEcho { delay_seconds: 3 },
            FixtureBehavior::EmitNumberedLines {
                prefix: "line".to_string(),
                count: 42,
            },
            FixtureBehavior::RecordThenRunTarget {
                marker_variable: "ILIUM_MARKER".to_string(),
                target_variable: "ILIUM_TARGET".to_string(),
                marker_text: "ran".to_string(),
            },
            FixtureBehavior::ShellImpersonator {
                intercepted_command: "launch codex".to_string(),
                replacement: PathBuf::from("/tmp/fake-codex"),
            },
            FixtureBehavior::RecordSubmittedPrompt {
                transcript_path: PathBuf::from("/tmp/transcript"),
            },
            FixtureBehavior::SpawnChild {
                child_path: PathBuf::from("/tmp/fake-child"),
            },
        ];
        for behavior in &behaviors {
            assert_every_variant_is_covered(behavior);
        }
        for behavior in behaviors {
            let encoded = serde_json::to_vec(&behavior).expect("serialize");
            let decoded: FixtureBehavior = serde_json::from_slice(&encoded).expect("deserialize");
            assert_eq!(decoded, behavior);
        }
    }
}
