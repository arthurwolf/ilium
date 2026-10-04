//! Real owned fixture processes; no installed agent CLI required.
use ilium_core::{AgentClass, AgentProcessKey};
use ilium_detect::{
    agent_launcher_ancestors, identify_agent_with_extra_excluding, refresh,
    retain_current_agent_launchers, AgentIdentity, ProcessChildrenIndex,
};
use ilium_test_fixtures::{install, FixtureBehavior};
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use sysinfo::{Pid, System};

struct OwnedWrapper {
    process: Child,
    directory: std::path::PathBuf,
}
impl Drop for OwnedWrapper {
    fn drop(&mut self) {
        terminate_current_owned_programs(&self.directory);
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}
// Revalidate the current exact birth and owned argv before any termination.
fn terminate_current_owned_process(
    process_id: u32,
    started_at: u64,
    directory: &std::path::Path,
) -> bool {
    let process_id = Pid::from_u32(process_id);
    let mut system = System::new();
    system.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::Some(&[process_id]),
        true,
        sysinfo::ProcessRefreshKind::nothing().with_cmd(sysinfo::UpdateKind::Always),
    );
    if system.process(process_id).is_some_and(|process| {
        process.start_time() == started_at
            && process
                .cmd()
                .first()
                .is_some_and(|argument| std::path::Path::new(argument).starts_with(directory))
    }) {
        return ilium_platform::process_control::terminate(process_id.as_u32()).is_ok();
    }
    false
}
fn terminate_current_owned_programs(directory: &std::path::Path) {
    let mut system = System::new();
    refresh(&mut system);
    for process in system.processes().values() {
        if process
            .cmd()
            .first()
            .is_some_and(|argument| std::path::Path::new(argument).starts_with(directory))
        {
            let _ = terminate_current_owned_process(
                process.pid().as_u32(),
                process.start_time(),
                directory,
            );
        }
    }
}
fn selected(system: &System, root: Pid, exclusions: &[AgentProcessKey]) -> Option<AgentIdentity> {
    identify_agent_with_extra_excluding(
        system,
        root,
        &ProcessChildrenIndex::build(system),
        &[],
        exclusions,
    )
}
fn sample_until(system: &mut System, predicate: impl Fn(&System) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        refresh(system);
        if predicate(system) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "owned fixture evidence did not converge"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}
#[test]
fn remembered_launcher_cannot_replace_exited_native_child_but_native_candidates_remain_searchable()
{
    for class in [AgentClass::Codex, AgentClass::Claude] {
        let directory = tempfile::tempdir().expect("owned install directory");
        let label = if class == AgentClass::Codex {
            "codex"
        } else {
            "claude"
        };
        let native = install(
            directory.path(),
            &format!("{label}-native"),
            &FixtureBehavior::Idle,
        );
        let wrapper = install(
            directory.path(),
            "node",
            &FixtureBehavior::SpawnChild {
                child_path: native.path,
            },
        );
        let mut owner = OwnedWrapper {
            process: Command::new(wrapper.path)
                .arg(directory.path().join(format!("{label}-shim")))
                .spawn()
                .expect("owned wrapper"),
            directory: directory.path().to_path_buf(),
        };
        let root = Pid::from_u32(owner.process.id());
        let mut system = System::new();
        // Register the exact owned child before any assertion can unwind.
        sample_until(&mut system, |system| {
            selected(system, root, &[]).is_some_and(|identity| identity.pid != root.as_u32())
        });
        let identity = selected(&system, root, &[]).expect("native identity");
        let ancestors = agent_launcher_ancestors(&system, root, &identity, &[]);
        assert_eq!(ancestors.len(), 1);
        assert_eq!(ancestors[0].class, class);
        assert_eq!(ancestors[0].process_id, root.as_u32());
        assert_eq!(
            selected(&system, root, &ancestors),
            Some(identity.clone()),
            "excluding a parent must still traverse its native descendant"
        );
        let mut exclusions = ancestors.clone();
        exclusions.push(AgentProcessKey {
            class: class.clone(),
            process_id: identity.pid,
            started_at_unix_seconds: identity.started_at_unix_seconds,
        });
        assert_eq!(
            selected(&system, root, &exclusions),
            Some(identity.clone()),
            "native evidence cannot be suppressed by a remembered interpreter key"
        );
        let mut wrong_birth = identity.clone();
        wrong_birth.started_at_unix_seconds = wrong_birth.started_at_unix_seconds.saturating_add(1);
        assert!(agent_launcher_ancestors(&system, root, &wrong_birth, &[]).is_empty());
        assert!(agent_launcher_ancestors(&system, Pid::from_u32(0), &identity, &[]).is_empty());
        assert!(terminate_current_owned_process(
            identity.pid,
            identity.started_at_unix_seconds,
            directory.path()
        ));
        sample_until(&mut system, |system| {
            selected(system, root, &[]).is_some_and(|identity| identity.pid == root.as_u32())
        });
        assert!(
            selected(&system, root, &ancestors).is_none(),
            "old launcher must not revive agent"
        );
        assert_eq!(
            retain_current_agent_launchers(&system, &ancestors),
            ancestors
        );
        let mut reused_key = ancestors[0].clone();
        reused_key.started_at_unix_seconds = reused_key.started_at_unix_seconds.saturating_add(1);
        assert!(
            retain_current_agent_launchers(&system, std::slice::from_ref(&reused_key)).is_empty()
        );
        assert!(
            selected(&system, root, &[reused_key]).is_some(),
            "different birth is a new owner"
        );
        owner.process.kill().expect("stop exact owned wrapper");
        owner.process.wait().expect("reap exact owned wrapper");
        sample_until(&mut system, |system| system.process(root).is_none());
        assert!(retain_current_agent_launchers(&system, &ancestors).is_empty());
    }
}

// This second forcing fixture is Unix-only because replacing a live process
// is Unix exec semantics. The shell executes a literal owned script path.
#[cfg(unix)]
#[test]
fn remembered_launcher_accepts_a_new_native_descendant_and_same_pid_native_exec() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::Stdio;
    use std::sync::mpsc;
    struct Controller {
        process: Child,
        events: mpsc::Receiver<String>,
        reader: Option<std::thread::JoinHandle<()>>,
        reader_done: mpsc::Receiver<()>,
        directory: std::path::PathBuf,
    }
    impl Controller {
        fn event(&self) -> String {
            self.events
                .recv_timeout(Duration::from_secs(10))
                .expect("fixture event")
        }
        fn command(&mut self, command: &str) {
            writeln!(
                self.process.stdin.as_mut().expect("owned stdin"),
                "{command}"
            )
            .expect("fixture command");
        }
    }
    impl Drop for Controller {
        fn drop(&mut self) {
            terminate_current_owned_programs(&self.directory);
            let _ = self.process.kill();
            let _ = self.process.wait();
            if let Some(reader) = self.reader.take() {
                if self
                    .reader_done
                    .recv_timeout(Duration::from_secs(10))
                    .is_ok()
                {
                    let _ = reader.join();
                } else {
                    // Do not block cleanup forever, or double-panic during an
                    // assertion unwind. A normal-path cleanup failure fails.
                    eprintln!("owned fixture output reader did not stop after process cleanup");
                    if !std::thread::panicking() {
                        panic!("owned fixture reader cleanup timed out");
                    }
                }
            }
        }
    }
    let directory = tempfile::tempdir().expect("owned fixture directory");
    let native = install(directory.path(), "codex-native", &FixtureBehavior::Idle);
    let script = directory.path().join("codex-shim");
    std::fs::write(
        &script,
        r#"native_path=$1
child=
printf 'READY\n'
while IFS= read -r command; do
    case "$command" in
        start)
            "$native_path" </dev/null >/dev/null 2>&1 &
            child=$!
            printf 'START:%s\n' "$child"
            ;;
        stop)
            kill "$child"
            wait "$child" || :
            printf 'STOPPED\n'
            ;;
        exec)
            printf 'EXEC\n'
            exec "$native_path"
            ;;
    esac
done
"#,
    )
    .expect("fixture source");
    let mut process = Command::new("/bin/sh")
        .arg(&script)
        .arg(&native.path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("owned shell fixture");
    let stdout = process.stdout.take().expect("owned stdout");
    let (sender, events) = mpsc::channel();
    let (reader_done_sender, reader_done) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else {
                break;
            };
            if sender.send(line).is_err() {
                break;
            }
        }
        let _ = reader_done_sender.send(());
    });
    let mut controller = Controller {
        process,
        events,
        reader: Some(reader),
        reader_done,
        directory: directory.path().to_path_buf(),
    };
    assert_eq!(controller.event(), "READY");
    let root = Pid::from_u32(controller.process.id());
    let mut system = System::new();
    sample_until(&mut system, |system| selected(system, root, &[]).is_some());
    let launcher = selected(&system, root, &[]).expect("interpreted launcher");
    assert_eq!(launcher.pid, root.as_u32());
    controller.command("start");
    let first_child = controller
        .event()
        .strip_prefix("START:")
        .expect("start marker")
        .parse::<u32>()
        .expect("owned native PID");
    sample_until(&mut system, |system| {
        selected(system, root, &[]).is_some_and(|identity| identity.pid == first_child)
    });
    let first_identity = selected(&system, root, &[]).expect("first native child");
    let ancestors = agent_launcher_ancestors(&system, root, &first_identity, &[]);
    assert_eq!(ancestors.len(), 1);
    controller.command("stop");
    assert_eq!(controller.event(), "STOPPED");
    sample_until(&mut system, |system| {
        selected(system, root, &[]).is_some_and(|identity| identity.pid == root.as_u32())
    });
    assert!(selected(&system, root, &ancestors).is_none());
    controller.command("start");
    let second_child = controller
        .event()
        .strip_prefix("START:")
        .expect("start marker")
        .parse::<u32>()
        .expect("owned replacement PID");
    assert_ne!(second_child, first_child);
    sample_until(&mut system, |system| {
        selected(system, root, &ancestors).is_some_and(|identity| identity.pid == second_child)
    });
    controller.command("stop");
    assert_eq!(controller.event(), "STOPPED");
    controller.command("exec");
    assert_eq!(controller.event(), "EXEC");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        refresh(&mut system);
        // This is exactly the proposed narrow native-refresh API invocation.
        system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::Some(&[root]),
            true,
            sysinfo::ProcessRefreshKind::nothing().with_cmd(sysinfo::UpdateKind::Always),
        );
        let current = system
            .process(root)
            .expect("same owned process must survive exec");
        let native_argv = current
            .cmd()
            .first()
            .is_some_and(|argument| std::path::Path::new(argument) == native.path);
        if native_argv {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "exec command evidence not refreshed"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    let current =
        selected(&system, root, &ancestors).expect("same-key native exec is a real replacement");
    assert_eq!(current.pid, launcher.pid);
    assert_eq!(
        current.started_at_unix_seconds,
        launcher.started_at_unix_seconds
    );
    assert!(current.process_name.starts_with("codex-native"));
    assert_eq!(
        retain_current_agent_launchers(&system, &ancestors),
        ancestors,
        "memory remains exact; provenance change, rather than deleting memory, permits exec"
    );
}
