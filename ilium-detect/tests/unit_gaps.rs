//! Unit coverage for the public process-identity helpers of `ilium-detect`
//! that no other test reached: `configure_process_refresh`,
//! `identify_agent_with_extra` (and its excluding variant) and
//! `effective_arguments`.
//!
//! Every test runs against real processes. Fixture executables from
//! `ilium-test-fixtures` are copied into a temporary directory and started by
//! absolute path, so `sysinfo` sees genuine process trees. Nothing here starts
//! a real agent CLI, and nothing matches by `PATH`.

use std::borrow::Cow;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use ilium_core::AgentClass;
use ilium_detect::{
    configure_process_refresh, effective_arguments, identify_agent, identify_agent_with_extra,
    identify_agent_with_extra_excluding, refresh, AgentIdentity, AgentSignature,
    ProcessChildrenIndex,
};
use ilium_test_fixtures::{install, spawn_fixture, FixtureBehavior};
use sysinfo::{Pid, System};

/// Owns the temporary install directory and every process a test starts.
/// Dropping it kills them, so a failing assertion leaves nothing running.
struct FixtureHarness {
    install_dir: tempfile::TempDir,
    children: Vec<Child>,
}

impl FixtureHarness {
    fn new() -> Self {
        Self {
            install_dir: tempfile::tempdir().expect("temp dir for installed fixtures"),
            children: Vec::new(),
        }
    }

    fn install_fixture(&self, name: &str, behavior: &FixtureBehavior) -> PathBuf {
        install(self.install_dir.path(), name, behavior).path
    }

    fn spawn_fixture_process(&mut self, executable: &Path, arguments: &[&OsStr]) -> u32 {
        let mut command = Command::new(executable);
        command.args(arguments);
        let child = spawn_fixture(&mut command).expect("spawn the installed fixture");
        self.track(child)
    }

    fn track(&mut self, child: Child) -> u32 {
        let process_id = child.id();
        self.children.push(child);
        process_id
    }
}

impl Drop for FixtureHarness {
    fn drop(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
        // Descendants spawned by `SpawnChild` fixtures are not in `children`,
        // but their program path is inside the install directory.
        let mut system = System::new();
        refresh(&mut system);
        for process in system.processes().values() {
            let started_from_install_dir = process
                .cmd()
                .first()
                .is_some_and(|program| Path::new(program).starts_with(self.install_dir.path()));
            if started_from_install_dir {
                let _ = process.kill();
            }
        }
    }
}

/// Spawns a launcher fixture named `launcherhost` whose only child is `child`.
fn spawn_launcher_over(harness: &mut FixtureHarness, child: &Path) -> u32 {
    let launcher = harness.install_fixture(
        "launcherhost",
        &FixtureBehavior::SpawnChild {
            child_path: child.to_path_buf(),
        },
    );
    harness.spawn_fixture_process(&launcher, &[])
}

/// Refreshes `system` and identifies the agent under `shell_process_id` until
/// `accept` returns true or ten seconds pass. Returns the last identity seen.
/// The fixture children appear a moment after their parent starts.
fn identify_until(
    system: &mut System,
    shell_process_id: u32,
    extra_signatures: &[AgentSignature],
    accept: impl Fn(&System, Option<&AgentIdentity>) -> bool,
) -> Option<AgentIdentity> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        refresh(system);
        let index = ProcessChildrenIndex::build(system);
        let identity = identify_agent_with_extra(
            system,
            Pid::from_u32(shell_process_id),
            &index,
            extra_signatures,
        );
        if accept(system, identity.as_ref()) || Instant::now() >= deadline {
            return identity;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// An extra signature that matches no built-in provider.
fn mybot_signature() -> AgentSignature {
    AgentSignature {
        name_substring: Cow::Borrowed("mybot"),
        class_of: |matched_name| AgentClass::Other(matched_name.to_string()),
    }
}

/// An extra signature that also matches the built-in `codex` provider.
fn codex_override_signature() -> AgentSignature {
    AgentSignature {
        name_substring: Cow::Borrowed("codex"),
        class_of: |_| AgentClass::Other("custom-codex".to_string()),
    }
}

#[test]
fn configure_process_refresh_can_run_repeatedly_without_panicking() {
    configure_process_refresh();
    configure_process_refresh();
}

#[test]
fn refresh_after_configure_still_lists_a_spawned_fixture_process() {
    configure_process_refresh();
    let mut harness = FixtureHarness::new();
    let executable = harness.install_fixture("codex-refresh", &FixtureBehavior::Idle);
    let process_id = harness.spawn_fixture_process(&executable, &[]);

    let mut system = System::new();
    refresh(&mut system);

    let process = system
        .process(Pid::from_u32(process_id))
        .expect("refresh after configure_process_refresh lists the spawned fixture");
    let executable_text = executable.to_string_lossy().into_owned();
    assert_eq!(
        process
            .cmd()
            .first()
            .map(|program| program.to_string_lossy().into_owned()),
        Some(executable_text),
        "refresh still reports the spawned program's command line",
    );
}

#[cfg(not(target_os = "linux"))]
#[test]
fn configure_process_refresh_is_a_noop_off_linux() {
    configure_process_refresh();
}

#[test]
fn identify_with_extra_finds_a_descendant_only_an_extra_signature_recognises() {
    let mut harness = FixtureHarness::new();
    let child = harness.install_fixture("mybot", &FixtureBehavior::Idle);
    let launcher_id = spawn_launcher_over(&mut harness, &child);
    let extra = [mybot_signature()];

    let mut system = System::new();
    let identity = identify_until(&mut system, launcher_id, &extra, |_, identity| {
        identity.is_some()
    })
    .expect("the extra signature identifies the mybot descendant");

    assert_eq!(identity.process_name, "mybot");
    assert_eq!(identity.matched_signature, "mybot");
    assert_eq!(identity.class, AgentClass::Other("mybot".to_string()));
    assert_eq!(
        identity.process_tree_depth, 1,
        "mybot is the launcher's direct child"
    );
    assert!(
        identify_agent(&system, Pid::from_u32(launcher_id)).is_none(),
        "without the extra signature the same tree has no agent",
    );
}

#[test]
fn identify_with_empty_extra_matches_identify_agent_for_the_same_tree() {
    let mut harness = FixtureHarness::new();
    let child = harness.install_fixture("codex-child", &FixtureBehavior::Idle);
    let launcher_id = spawn_launcher_over(&mut harness, &child);

    let mut system = System::new();
    let identity = identify_until(&mut system, launcher_id, &[], |_, identity| {
        identity.is_some()
    });
    assert!(
        identity.is_some(),
        "the built-in codex signature finds codex-child"
    );

    let index = ProcessChildrenIndex::build(&system);
    let shell = Pid::from_u32(launcher_id);
    assert_eq!(
        identify_agent_with_extra(&system, shell, &index, &[]),
        identify_agent(&system, shell),
        "an empty extra list must not change the result",
    );
}

#[test]
fn builtin_signature_wins_over_extra_signature_on_the_same_process() {
    let mut harness = FixtureHarness::new();
    let child = harness.install_fixture("codex-child", &FixtureBehavior::Idle);
    let launcher_id = spawn_launcher_over(&mut harness, &child);
    let extra = [codex_override_signature()];

    let mut system = System::new();
    let identity = identify_until(&mut system, launcher_id, &extra, |_, identity| {
        identity.is_some()
    })
    .expect("codex-child is identified");

    assert_eq!(
        identity.class,
        AgentClass::Codex,
        "the built-in provider keeps precedence"
    );
    assert_eq!(identity.matched_signature, "codex");
}

#[test]
fn identify_with_extra_returns_none_when_no_agent_descends_from_shell() {
    let mut harness = FixtureHarness::new();
    let launcher = harness.install_fixture("launcherhost", &FixtureBehavior::Idle);
    let launcher_id = harness.spawn_fixture_process(&launcher, &[]);
    let extra = [mybot_signature()];

    let mut system = System::new();
    let identity = identify_until(&mut system, launcher_id, &extra, |_, _| true);

    assert!(
        identity.is_none(),
        "a shell with no matching descendant identifies nothing"
    );
}

#[test]
fn identify_with_extra_returns_none_for_a_shell_pid_missing_from_snapshot() {
    let mut harness = FixtureHarness::new();
    let launcher = harness.install_fixture("launcherhost", &FixtureBehavior::Idle);
    harness.spawn_fixture_process(&launcher, &[]);
    let mut system = System::new();
    refresh(&mut system);
    let absent_pid = Pid::from_u32(u32::MAX - 1);
    assert!(
        system.process(absent_pid).is_none(),
        "the test pid must not be a live process"
    );

    let index = ProcessChildrenIndex::build(&system);
    let identity = identify_agent_with_extra(&system, absent_pid, &index, &[mybot_signature()]);

    assert!(
        identity.is_none(),
        "a shell pid absent from the snapshot identifies nothing"
    );
}

#[test]
fn excluding_empty_list_matches_identify_with_extra() {
    let mut harness = FixtureHarness::new();
    let child = harness.install_fixture("mybot", &FixtureBehavior::Idle);
    let launcher_id = spawn_launcher_over(&mut harness, &child);
    let extra = [mybot_signature()];

    let mut system = System::new();
    let identity = identify_until(&mut system, launcher_id, &extra, |_, identity| {
        identity.is_some()
    });
    assert!(
        identity.is_some(),
        "mybot is identified before comparing the two walks"
    );

    let index = ProcessChildrenIndex::build(&system);
    let shell = Pid::from_u32(launcher_id);
    assert_eq!(
        identify_agent_with_extra_excluding(&system, shell, &index, &extra, &[]),
        identify_agent_with_extra(&system, shell, &index, &extra),
        "an empty exclusion list must not change the walk",
    );
}

#[test]
fn nearer_matching_descendant_beats_deeper_native_match() {
    let mut harness = FixtureHarness::new();
    let deep = harness.install_fixture("codex-deep", &FixtureBehavior::Idle);
    let near = harness.install_fixture(
        "codex-near",
        &FixtureBehavior::SpawnChild { child_path: deep },
    );
    let launcher_id = spawn_launcher_over(&mut harness, &near);

    let mut system = System::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    let (identity, deep_seen) = loop {
        refresh(&mut system);
        let index = ProcessChildrenIndex::build(&system);
        let identity = identify_agent_with_extra(&system, Pid::from_u32(launcher_id), &index, &[]);
        let deep_seen = system
            .processes()
            .values()
            .any(|process| process.name().to_string_lossy().starts_with("codex-deep"));
        let near_identified = identity
            .as_ref()
            .is_some_and(|identity| identity.process_name.starts_with("codex-near"));
        if (deep_seen && near_identified) || Instant::now() >= deadline {
            break (identity, deep_seen);
        }
        std::thread::sleep(Duration::from_millis(100));
    };

    assert!(
        deep_seen,
        "the deeper codex-deep fixture must exist before ranking is checked"
    );
    let identity = identity.expect("codex-near is identified");
    assert!(
        identity.process_name.starts_with("codex-near"),
        "the nearer match wins, got {}",
        identity.process_name,
    );
    assert_eq!(identity.process_tree_depth, 1);
}

#[test]
fn effective_arguments_leaves_a_native_argv_unchanged() {
    let mut harness = FixtureHarness::new();
    let executable = harness.install_fixture("codex-args", &FixtureBehavior::Idle);
    let process_id = harness.spawn_fixture_process(
        &executable,
        &[OsStr::new("--flag"), OsStr::new("value with space")],
    );

    let mut system = System::new();
    refresh(&mut system);
    let process = system
        .process(Pid::from_u32(process_id))
        .expect("the spawned fixture is in the snapshot");

    assert_eq!(
        effective_arguments(process),
        vec![
            executable.to_string_lossy().into_owned(),
            "--flag".to_string(),
            "value with space".to_string(),
        ],
        "a native process keeps its arguments exactly, including spaces",
    );
}

#[cfg(unix)]
#[test]
fn effective_arguments_expands_shell_c_command_into_program_tokens() {
    let mut harness = FixtureHarness::new();
    let child = Command::new("sh")
        .arg("-c")
        .arg("sleep 5 && true")
        .spawn()
        .expect("spawn sh -c");
    let process_id = harness.track(child);

    let mut system = System::new();
    refresh(&mut system);
    let process = system
        .process(Pid::from_u32(process_id))
        .expect("the sh process is in the snapshot");

    assert_eq!(
        effective_arguments(process),
        vec!["sh", "-c", "sleep", "5", "&&", "true"],
        "the -c command line is split into the tokens the program sees",
    );
}

#[cfg(unix)]
#[test]
fn effective_arguments_keeps_script_operand_for_an_interpreter() {
    let mut harness = FixtureHarness::new();
    let script = harness.install_dir.path().join("codex-script.sh");
    std::fs::write(&script, "sleep 5\n").expect("write the script fixture");
    let child = Command::new("sh")
        .arg(&script)
        .spawn()
        .expect("spawn sh with a script operand");
    let process_id = harness.track(child);

    let mut system = System::new();
    refresh(&mut system);
    let process = system
        .process(Pid::from_u32(process_id))
        .expect("the sh process is in the snapshot");

    assert_eq!(
        effective_arguments(process),
        vec!["sh".to_string(), script.to_string_lossy().into_owned()],
        "a script operand stays one argument, so its name is still visible",
    );
}

#[cfg(unix)]
#[test]
fn effective_arguments_converts_non_utf8_arguments_lossily() {
    use std::os::unix::ffi::OsStrExt;

    let mut harness = FixtureHarness::new();
    let child = Command::new("sh")
        .arg("-c")
        .arg("sleep 5")
        .arg(OsStr::from_bytes(b"bad\xff"))
        .spawn()
        .expect("spawn sh with a non-UTF-8 argument");
    let process_id = harness.track(child);

    let mut system = System::new();
    refresh(&mut system);
    let process = system
        .process(Pid::from_u32(process_id))
        .expect("the sh process is in the snapshot");

    let arguments = effective_arguments(process);
    assert_eq!(
        arguments.last().map(String::as_str),
        Some("bad\u{FFFD}"),
        "the invalid byte is replaced rather than dropping the argument",
    );
}

#[cfg(unix)]
#[test]
fn effective_arguments_keeps_a_lone_shell_name_unchanged() {
    let mut harness = FixtureHarness::new();
    // Piped stdin keeps the shell waiting for input, so it stays alive with
    // no command operand.
    let child = Command::new("sh")
        .stdin(Stdio::piped())
        .spawn()
        .expect("spawn a bare sh");
    let process_id = harness.track(child);

    let mut system = System::new();
    refresh(&mut system);
    let process = system
        .process(Pid::from_u32(process_id))
        .expect("the sh process is in the snapshot");

    assert_eq!(
        effective_arguments(process),
        vec!["sh".to_string()],
        "a shell with no command operand reports only its own name",
    );
}
