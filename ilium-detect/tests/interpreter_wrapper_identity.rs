//! Cross-platform coverage for the interpreter-wrapper-vs-native-child
//! ranking `identify_agent` applies (added in 737099d).
//!
//! Some installs (e.g. Bun's global bin shim, `node /home/.../bin/codex`) put
//! a JS *launcher* directly on the pane's shell -- matched only by
//! unwrapping its argv, never by its own kernel name -- which then spawns
//! the real native CLI as a further child instead of exec-replacing itself.
//! `identify_agent` must prefer that native child even though it sits one
//! level deeper, because the launcher never held a transcript file open.
//!
//! The original regression test for this (`ilium-detect/src/lib.rs`'s
//! `identify_agent_prefers_a_native_child_over_an_interpreter_wrapper_match`)
//! reproduces the wrapper and native binaries as `#!/bin/sh` scripts, which
//! Windows has no equivalent of, so it's `#[cfg(unix)]`-gated (see
//! `docs/TODO.md`'s Windows chapter). This file covers the same ranking
//! logic using `ilium-test-fixtures`' real, cross-platform executables
//! instead of shebang scripts, so the logic itself has Windows coverage even
//! though the shebang-specific test does not.
//!
//! The wrapper itself is a *valid* match the instant it starts -- before it
//! has forked its own child -- so the assertion this test needs is "waited
//! for the native match to appear," never "found a match": breaking on the
//! first `Some` intermittently accepts the wrapper's own premature match
//! instead, under exactly the CPU pressure that makes this the whole point
//! of testing.

use std::time::{Duration, Instant};

use ilium_detect::{identify_agent, refresh};
use ilium_test_fixtures::{install, FixtureBehavior};
use sysinfo::{Pid, System};

/// The native child, one level deeper than the wrapper, must be identified
/// as the agent even though the wrapper is the shallower match.
#[test]
fn identify_agent_prefers_a_native_child_over_an_interpreter_wrapper_match() {
    let install_dir = tempfile::tempdir().expect("temp dir for installed fixtures");

    // Named "codex-native" (not bare "codex") so a test failure that reports
    // the wrong `process_name` distinguishes "matched the wrapper" from
    // "matched the native child" at a glance.
    let native = install(install_dir.path(), "codex-native", &FixtureBehavior::Idle);

    // "node" is in `identify_agent`'s built-in interpreter list, so the
    // wrapper is matched only by unwrapping its argv, never by its own
    // kernel name -- exactly the ambiguity 737099d's ranking resolves.
    let wrapper = install(
        install_dir.path(),
        "node",
        &FixtureBehavior::SpawnChild {
            child_path: native.path.clone(),
        },
    );

    // The argument is never read by the wrapper (its behavior comes from the
    // sidecar file, not argv) -- it only needs a file name containing
    // "codex" so the wrapper matches via `identifying_process_names`'s
    // argv-unwrapping fallback, exactly like Bun's shim being handed its own
    // script path as `node`'s argument.
    let mut wrapper_process = std::process::Command::new(&wrapper.path)
        .arg(install_dir.path().join("codex-shim"))
        .spawn()
        .expect("spawn the wrapper fixture");

    // Polls for the *native* match specifically, not merely for any match:
    // the wrapper itself is a valid (interpreted) match the moment it starts,
    // before it has forked its own child, so breaking on `is_some()` would
    // race the child's fork and intermittently observe -- and wrongly
    // accept -- the wrapper's own premature match instead of waiting for the
    // ranking logic to find the deeper native one, exactly the distinction
    // this test exists to check.
    let mut system = System::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    let identity = loop {
        refresh(&mut system);
        let identity = identify_agent(&system, Pid::from_u32(wrapper_process.id()));
        let found_native_match = identity
            .as_ref()
            .is_some_and(|identity| identity.process_name.starts_with("codex-native"));
        if found_native_match || Instant::now() >= deadline {
            break identity;
        }
        std::thread::sleep(Duration::from_millis(100));
    };

    // Reported before asserting/unwrapping, and cleanup runs unconditionally
    // (not only on the success path): a `None` here is exactly what a real
    // Windows failure would look like, and both a bare panic and a leaked
    // 60s-lingering wrapper/child pair make that failure needlessly costly
    // to triage from CI log output alone.
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
            name.contains("codex") || name.contains("node") || arguments.contains("codex")
        })
        .map(|process| {
            format!(
                "pid={} name={:?} cmd={:?}",
                process.pid(),
                process.name(),
                process.cmd().iter().collect::<Vec<_>>()
            )
        })
        .collect();

    let _ = ilium_platform::process_control::terminate(wrapper_process.id());
    if let Some(identity) = &identity {
        let _ = ilium_platform::process_control::terminate(identity.pid);
    }
    let _ = wrapper_process.wait();

    let identity = identity.unwrap_or_else(|| {
        panic!(
            "the native child was not found as a codex-matching descendant of the wrapper.\n\
             processes mentioning codex/node:\n  {}",
            if observed.is_empty() {
                "(none)".to_string()
            } else {
                observed.join("\n  ")
            }
        )
    });

    assert_eq!(identity.class, ilium_core::AgentClass::Codex);
    assert!(
        identity.process_name.starts_with("codex-native"),
        "expected the native child to be matched, not the wrapper: {identity:?}"
    );
}
