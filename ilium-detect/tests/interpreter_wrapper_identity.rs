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

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
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

    // One last sample before cleanup chooses its targets, so a child that the
    // wrapper forked after the poll loop's final refresh is still visible
    // here rather than being left to linger.
    refresh(&mut system);

    // Cleanup (and the diagnostic that shares its process set) is scoped to
    // processes this test is *provably* responsible for: the wrapper, its
    // descendants, and anything running an executable out of this test's own
    // temporary directory.
    //
    // Selecting targets by process name instead -- "everything called codex
    // or node" -- reads the whole machine's process table (`refresh` above
    // populates every process, not just this subtree), so it would terminate
    // the developer's editor helpers and dev servers, their real agent CLI
    // sessions, and the fake `codex` another test binary is running in
    // parallel. None of those belong to this test.
    //
    // The install-directory clause covers the one case descent alone misses,
    // which is exactly the case this diagnostic exists for: a fixture whose
    // parent link the snapshot does not attribute to the wrapper (already
    // orphaned, or never observed as its child) would otherwise both escape
    // cleanup -- lingering for its full 60s `LINGER` duration -- and be
    // missing from the failure report. `cmd()[0]` rather than `exe()`
    // because `refresh` deliberately does not populate executable paths, and
    // the wrapper spawns its child by absolute path, so argv[0] is that path.
    let wrapper_pid = Pid::from_u32(wrapper_process.id());
    let install_prefix = install_dir.path();
    let mut cleanup_pids: Vec<Pid> = vec![wrapper_pid];
    let mut already_listed: HashSet<Pid> = HashSet::from([wrapper_pid]);
    for descendant_pid in descendants_of(&system, wrapper_pid) {
        if already_listed.insert(descendant_pid) {
            cleanup_pids.push(descendant_pid);
        }
    }
    for process in system.processes().values() {
        let was_started_from_the_install_directory = process
            .cmd()
            .first()
            .is_some_and(|program| Path::new(program).starts_with(install_prefix));
        if was_started_from_the_install_directory && already_listed.insert(process.pid()) {
            cleanup_pids.push(process.pid());
        }
    }

    // Described before anything is terminated, and reported before
    // asserting/unwrapping: a `None` here is exactly what a real Windows
    // failure would look like, and a bare panic makes that failure needlessly
    // costly to triage from CI log output alone.
    let observed: Vec<String> = cleanup_pids
        .iter()
        .filter_map(|pid| system.process(*pid))
        .map(|process| {
            format!(
                "pid={} name={:?} cmd={:?}",
                process.pid(),
                process.name(),
                process.cmd().iter().collect::<Vec<_>>()
            )
        })
        .collect();

    // Unconditional, not only on the success path: on a timeout failure
    // `identity` is `None`, and the native child the wrapper already forked
    // would otherwise be left with nothing holding a handle to its pid.
    for pid in &cleanup_pids {
        let _ = ilium_platform::process_control::terminate(pid.as_u32());
    }
    let _ = wrapper_process.wait();

    let subtree_report = if observed.is_empty() {
        "(none -- the wrapper had no live descendants and nothing was running \
         out of the install directory)"
            .to_string()
    } else {
        observed.join("\n  ")
    };

    let identity = identity.unwrap_or_else(|| {
        panic!(
            "the native child was not found as a codex-matching descendant of the wrapper.\n\
             the wrapper's subtree, plus everything started from {}:\n  {subtree_report}",
            install_prefix.display()
        )
    });

    assert_eq!(identity.class, ilium_core::AgentClass::Codex);
    assert!(
        identity.process_name.starts_with("codex-native"),
        "expected the native child to be matched, not the wrapper: {identity:?}\n\
         the wrapper's subtree:\n  {subtree_report}"
    );
}

/// Every descendant pid of `root` in `system`'s current snapshot,
/// breadth-first.
///
/// `ilium_detect`'s own `ProcessChildrenIndex` does this for detection, but
/// keeps its adjacency lookup private, and this test needs the transitive
/// closure rather than one level of children -- so the walk is rebuilt here
/// rather than widening a library API for a test's benefit.
///
/// The visited set guards against a cycle in an inconsistent snapshot: pid
/// reuse between the parent links of two processes sampled at slightly
/// different moments is unlikely, but a cleanup helper that can loop forever
/// would hang the suite rather than fail it.
fn descendants_of(system: &System, root: Pid) -> Vec<Pid> {
    let mut children_by_parent: HashMap<Pid, Vec<Pid>> = HashMap::new();
    for process in system.processes().values() {
        if let Some(parent_pid) = process.parent() {
            children_by_parent
                .entry(parent_pid)
                .or_default()
                .push(process.pid());
        }
    }

    let mut descendants: Vec<Pid> = Vec::new();
    let mut visited: HashSet<Pid> = HashSet::from([root]);
    let mut queue: VecDeque<Pid> = VecDeque::from([root]);
    while let Some(pid) = queue.pop_front() {
        let Some(children) = children_by_parent.get(&pid) else {
            continue;
        };
        for &child_pid in children {
            if visited.insert(child_pid) {
                descendants.push(child_pid);
                queue.push_back(child_pid);
            }
        }
    }
    descendants
}
