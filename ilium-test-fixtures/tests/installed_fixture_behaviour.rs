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

use ilium_test_fixtures::{
    behavior_file_for, install, spawn_fixture, FixtureBehavior, FIXTURE_BINARY_NAME,
};

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
    let mut child = spawn_fixture(
        Command::new(&installed.path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
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

    let output = spawn_fixture(
        Command::new(&installed.path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn the fixture without its sidecar")
    .wait_with_output()
    .expect("collect the fixture without its sidecar");

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


/// Linux session discovery reads the child's initial environment. This check
/// also proves re-exec preserves the PID returned by spawn and handles inherited
/// provider overrides without changing the integration-test runner's home.
#[cfg(target_os = "linux")]
#[test]
fn explicit_fixture_home_is_visible_in_the_same_process_start_environment() {
    let directory = tempfile::tempdir().expect("fixture directory");
    let home = tempfile::tempdir().expect("provider home");
    let runner_home = std::env::var_os("HOME");
    let runner_codex_home = std::env::var_os("CODEX_HOME");
    let runner_claude_home = std::env::var_os("CLAUDE_CONFIG_DIR");
    let installed = install(
        directory.path(),
        "codex",
        &FixtureBehavior::EchoSubmittedLine {
            prefix: "ready".into(),
        },
    );
    ilium_test_fixtures::set_provider_home(&installed.path, home.path());
    let mut child = spawn_fixture(
        Command::new(&installed.path)
            .env("HOME", directory.path())
            .env("CODEX_HOME", directory.path().join("inherited-codex"))
            .env("CLAUDE_CONFIG_DIR", directory.path().join("inherited-claude"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn fixture with unrelated inherited home");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"home-ready\n")
        .unwrap();
    let mut answer = String::new();
    BufReader::new(child.stdout.as_mut().unwrap())
        .read_line(&mut answer)
        .unwrap();
    let homes = ilium_platform::process_info::agent_provider_homes(child.id());
    let _ = child.kill();
    let _ = child.wait();

    assert_eq!(answer.trim_end(), "ready:<home-ready>");
    assert_eq!(
        homes,
        Some(ilium_platform::process_info::AgentProviderHomes {
            home: home.path().to_path_buf(),
            codex_home: home.path().join(".codex"),
            claude_config: home.path().join(".claude"),
        })
    );
    assert_eq!(std::env::var_os("HOME"), runner_home);
    assert_eq!(std::env::var_os("CODEX_HOME"), runner_codex_home);
    assert_eq!(std::env::var_os("CLAUDE_CONFIG_DIR"), runner_claude_home);
}
