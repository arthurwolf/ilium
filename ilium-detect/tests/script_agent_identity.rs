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
/// recognises, and runs it long enough to be observed.
fn write_sleeping_codex_script(directory: &std::path::Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let script_path = directory.join("codex");
    let mut file = std::fs::File::create(&script_path).expect("create the fake codex script");
    file.write_all(b"#!/bin/sh\nsleep 30\n")
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
    let mut child = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(script_path.to_string_lossy().as_ref())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn the fake codex script");

    // The child needs a moment to exec the script before it can be recognised
    // as anything, and process tables are refreshed, not awaited.
    let mut system = System::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    let identity = loop {
        refresh(&mut system);
        let identity = identify_agent(&system, Pid::from_u32(std::process::id()));
        if identity.is_some() || Instant::now() >= deadline {
            break identity;
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
