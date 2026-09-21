//! Pins how an agent CLI *installed as a script* is reported by the OS.
//!
//! This is the one detection behaviour that cannot be covered by the fixture
//! tests, because it is not about ilium's own logic: it is about what the
//! kernel and `sysinfo` say a process is called. Agent CLIs are commonly
//! shipped as shebang scripts -- `#!/usr/bin/env node` wrappers, or shell
//! launchers -- and the platforms disagree about the answer. Linux reports such
//! a process under the script's own name; macOS reports the *interpreter*.
//!
//! `identify_agent` therefore cannot rely on the process name alone, and this
//! test is what proves the fallback works against a real process rather than an
//! assumption about one.
//!
//! Unix-only: the fixture is a `#!/bin/sh` script, which Windows has no
//! equivalent of. Windows ships agent CLIs as real executables, where the
//! process name is the file name and no fallback is involved.
#![cfg(unix)]

use std::io::Write;
use std::time::{Duration, Instant};

use ilium_detect::{identify_agent, refresh};
use sysinfo::{Pid, System};

/// Writes an executable `#!/bin/sh` script under a name the built-in registry
/// recognises, which once spawned stays alive long enough to be observed.
///
/// It idles in one-second sleeps rather than a single long one because each
/// `sleep` is a forked child: killing the script leaves whichever `sleep` was
/// running orphaned, and a short one outlives the test by at most a second
/// instead of half a minute. The loop is bounded rather than infinite so a run
/// that never reaches its cleanup -- a killed test binary, a panic in the
/// harness itself -- still cannot leave the script running indefinitely.
fn write_sleeping_codex_script(directory: &std::path::Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let script_path = directory.join("codex");
    let mut file = std::fs::File::create(&script_path).expect("create the fake codex script");
    file.write_all(
        b"#!/bin/sh\n\
          elapsed=0\n\
          while [ \"$elapsed\" -lt 30 ]; do\n\
          sleep 1\n\
          elapsed=$((elapsed + 1))\n\
          done\n",
    )
    .expect("write the fake codex script");
    file.set_permissions(std::fs::Permissions::from_mode(0o700))
        .expect("make the fake codex script executable");
    script_path
}

/// A shebang script named after a known agent must be identified as that agent,
/// however the platform happens to name the running process.
#[test]
fn a_shebang_script_named_after_an_agent_is_identified_as_that_agent() {
    let directory = tempfile::tempdir().expect("temp dir");
    let script_path = write_sleeping_codex_script(directory.path());

    // Spawned through a shell exactly as a pane runs a command line, so the
    // process tree matches what detection actually walks in production.
    //
    // The script is handed to the shell as `$0` rather than interpolated into
    // the command line: a temporary directory whose path contains a space (or
    // any other shell metacharacter) would otherwise be word-split into
    // arguments and never run at all, and a non-UTF-8 path would be corrupted
    // by the lossy conversion interpolating it requires. Passing it as an
    // argument keeps the exact `OsStr` the filesystem gave us.
    //
    // It also keeps the assertion honest: with the path spelled out inside the
    // command line, the `/bin/sh -c <path>` wrapper matches the registry on its
    // own argv (see `identifying_process_names`) from the instant it starts, so
    // the test could pass having never observed the script as a running
    // process at all. Behind `$0`, only the script itself can match.
    let mut child = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg("\"$0\"")
        .arg(&script_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn the fake codex script");

    // The child needs a moment to exec the script before it can be recognised
    // as anything, and process tables are refreshed, not awaited.
    //
    // `child` -- not this test binary's own pid -- is the analogue of "the
    // pane's directly-spawned child" that `identify_agent` expects: it is
    // the `/bin/sh -c` process the assertion is actually about,
    // whether the shell exec-replaces itself into the script (depth 0) or
    // forks it as a child (depth 1). Anchoring on the test harness's own pid
    // instead would happen to still find it (the harness is `child`'s
    // parent), but it walks a tree scoped to the whole test process rather
    // than to this test's own spawned subtree, which is not what "the
    // process tree matches what detection actually walks in production"
    // above claims.
    //
    // Each attempt samples into a *fresh* `System` rather than re-refreshing
    // one: `refresh` reads a process's command line with
    // `UpdateKind::OnlyIfNotSet` (arguments do not change -- except across an
    // exec, which is precisely what is being waited for here). A snapshot
    // taken in the window between fork and exec would latch the shell's
    // pre-exec `sh -c "$0" <script>` argv for the rest of the poll, and no
    // later refresh would ever replace it with the script's own -- a
    // guaranteed timeout on any platform that reports the interpreter's name
    // and so has nothing but the argv left to match on. A fresh snapshot per
    // attempt is what a server booting into an already-running pane does
    // anyway, and costs nothing at this cadence.
    let script_process_pid = Pid::from_u32(child.id());
    let deadline = Instant::now() + Duration::from_secs(10);
    let (identity, system) = loop {
        let mut system = System::new();
        refresh(&mut system);
        let identity = identify_agent(&system, script_process_pid);
        if identity.is_some() || Instant::now() >= deadline {
            break (identity, system);
        }
        std::thread::sleep(Duration::from_millis(100));
    };

    // Reported before asserting: when this fails, what the platform called the
    // process is the entire answer, and it is not recoverable from a bare
    // "expected Some, got None".
    let observed: Vec<String> = system
        .processes()
        .values()
        .filter(|process| {
            let name = process.name().to_string_lossy().to_lowercase();
            let arguments = process
                .cmd()
                .iter()
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(" ");
            name.contains("codex") || arguments.contains("codex")
        })
        .map(|process| {
            format!(
                "name={:?} cmd={:?}",
                process.name(),
                process.cmd().iter().collect::<Vec<_>>()
            )
        })
        .collect();

    // The shell is free to fork the script rather than exec-replacing itself
    // into it (the depth-1 shape the anchoring comment above describes), and
    // killing the wrapper does not kill what it forked: the script would then
    // idle on for the rest of its bounded lifetime with nothing left holding
    // its pid. Terminating the process detection actually matched covers that
    // shape as well as the depth-0 one, and is safe by construction -- the
    // walk that produced this pid is scoped to `child`'s own subtree, so it
    // can never name a real `codex` running elsewhere on the machine.
    if let Some(identified) = identity.as_ref().filter(|found| found.pid != child.id()) {
        let _ = ilium_platform::process_control::terminate(identified.pid);
    }
    let _ = child.kill();
    let _ = child.wait();

    let identity = identity.unwrap_or_else(|| {
        panic!(
            "a shebang script named `codex` was not identified as an agent.\n\
             processes mentioning codex:\n  {}",
            if observed.is_empty() {
                "(none -- the platform reports neither the name nor the arguments)".to_string()
            } else {
                observed.join("\n  ")
            }
        )
    });

    assert_eq!(
        identity.class,
        ilium_core::AgentClass::Codex,
        "identified the wrong agent: {identity:?}"
    );
}
