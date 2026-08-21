//! Proves an installed fixture actually behaves the way its
//! [`FixtureBehavior`] says, by spawning one and talking to it.
//!
//! This file also has a second, structural job. Cargo builds a package's
//! **binary** targets only when that package has an integration test or
//! benchmark selected -- a package with nothing but unit tests never gets its
//! bins built, not even under `cargo test --workspace`. Every other crate's
//! tests locate `ilium-fixture-agent` by path, so without an integration test
//! here the binary would simply not exist when CI ran them, and every fixture
//! test in the workspace would fail on a missing file.
//!
//! So: do not delete this file to "simplify", and do not move its tests into
//! `src/lib.rs`. It is load-bearing for the build graph.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use ilium_test_fixtures::{behavior_file_for, install, FixtureBehavior, FIXTURE_BINARY_NAME};

#[test]
fn installing_a_fixture_writes_an_executable_and_its_behaviour_sidecar() {
    let directory = tempfile::tempdir().expect("temp dir");
    let installed = install(directory.path(), "codex", &FixtureBehavior::Idle);

    assert!(
        installed.path.is_file(),
        "install should produce a real file at {}",
        installed.path.display()
    );
    // Named for detection, not for the fixture mechanism: substring matching
    // in `ilium_detect` is what lets `codex.exe` satisfy the `codex` signature,
    // so the stem has to survive the platform's extension being appended.
    assert!(installed
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("codex")));
    assert!(behavior_file_for(&installed.path).is_file());
}

#[test]
fn an_installed_echo_fixture_reads_a_line_and_answers_with_its_prefix() {
    let directory = tempfile::tempdir().expect("temp dir");
    let installed = install(
        directory.path(),
        "line-echo",
        &FixtureBehavior::EchoSubmittedLine {
            prefix: "answered".to_string(),
        },
    );

    // Pipes rather than a pty: this asserts the sidecar was found and the
    // behaviour dispatched, which is the contract every other crate depends
    // on. Pty-specific behaviour is covered where it matters, in the server's
    // own end-to-end tests.
    let mut child = Command::new(&installed.path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| {
            panic!(
                "spawn the installed fixture at {}: {error}",
                installed.path.display()
            )
        });

    child
        .stdin
        .as_mut()
        .expect("piped stdin")
        .write_all(b"a submitted line\n")
        .expect("write to the fixture");

    let mut answer = String::new();
    BufReader::new(child.stdout.as_mut().expect("piped stdout"))
        .read_line(&mut answer)
        .expect("read the fixture's answer");

    // Killed rather than waited on: every behaviour lingers on purpose so a
    // test can never outlive its fixture by accident.
    let _ = child.kill();
    let _ = child.wait();

    assert_eq!(answer.trim_end(), "answered:<a submitted line>");
}

#[test]
fn a_fixture_without_its_sidecar_fails_loudly_instead_of_doing_nothing() {
    // A fixture that silently idled would surface as an unexplained detection
    // timeout in some distant test, so the failure has to be noisy and local.
    let directory = tempfile::tempdir().expect("temp dir");
    let installed = install(directory.path(), "codex", &FixtureBehavior::Idle);
    std::fs::remove_file(behavior_file_for(&installed.path)).expect("remove the sidecar");

    let output = Command::new(&installed.path)
        .stdin(Stdio::null())
        .output()
        .expect("run the fixture without its sidecar");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("has no behaviour file"),
        "expected a panic naming the missing sidecar, got: {stderr}"
    );
}

#[test]
fn the_fixture_binary_name_matches_the_bin_target_cargo_builds() {
    // `CARGO_BIN_EXE_<name>` is set by cargo only for a `[[bin]]` target that
    // actually exists in this package under that exact name, so this ties
    // `FIXTURE_BINARY_NAME` to the real `[[bin]]` in Cargo.toml rather than to
    // a second hand-written copy of the same string: renaming the `[[bin]]`
    // target now fails this test (or fails to compile it) instead of leaving
    // every other crate's path search silently pointed at a binary cargo
    // never built.
    let built = std::path::Path::new(env!("CARGO_BIN_EXE_ilium-fixture-agent"));
    assert_eq!(
        built.file_stem().and_then(|stem| stem.to_str()),
        Some(FIXTURE_BINARY_NAME)
    );
}
