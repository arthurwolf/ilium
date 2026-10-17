//! api-process-control.md: `run`, `take-exit`, `spawn` and the `child` handle calls.

use ilium_plugin_api::generated::host::{ChildHandle, HostProcess, ProcessSpec, SpawnSpec};
use ilium_plugin_api::generated::types::{ErrorCode, RequestId};
use ilium_plugin_api::Tier;
use ilium_plugin_host::testing::imports_fixture::ImportsFixture;
use proptest::prelude::*;

use crate::support::*;

fn spec(program: &str, arguments: &[&str]) -> ProcessSpec {
    ProcessSpec {
        program: program.to_owned(),
        arguments: arguments
            .iter()
            .map(|argument| (*argument).to_owned())
            .collect(),
        cwd: None,
        stdin: None,
        environment: Vec::new(),
        timeout_millis: 5000,
    }
}

fn runner(programs: &[&str]) -> ImportsFixture {
    let grants: Vec<String> = programs
        .iter()
        .map(|program| format!("process.run:{program}"))
        .collect();
    let refs: Vec<&str> = grants.iter().map(String::as_str).collect();
    fixture(Tier::T1, &refs)
}

fn spawner(programs: &[&str]) -> ImportsFixture {
    let mut grants: Vec<String> = programs
        .iter()
        .map(|program| format!("process.run:{program}"))
        .collect();
    grants.push("process.long-lived".to_owned());
    let refs: Vec<&str> = grants.iter().map(String::as_str).collect();
    fixture(Tier::T1, &refs)
}

fn run_exit(
    fixture: &mut ImportsFixture,
    process: ProcessSpec,
) -> ilium_plugin_api::generated::host::ProcessExit {
    let request = expect_ok(fixture.process().run(process));
    assert!(is_ok(&complete(fixture, request)));
    fixture.process().take_exit(request).expect("exit")
}

// ---- run, take-exit --------------------------------------------------------------------------

/// REQ: WIT:ilium:host/process.run, WIT:ilium:host/process.take-exit
#[test]
fn process_run_captures_stdout_and_a_zero_status() {
    let mut fixture = runner(&["echo"]);
    let exit = run_exit(&mut fixture, spec("echo", &["hello"]));
    assert_eq!(exit.status, Some(0));
    assert_eq!(exit.stdout, b"hello\n");
}

/// REQ: WIT:ilium:host/process.take-exit
#[test]
fn process_take_exit_returns_the_exit_once() {
    let mut fixture = runner(&["echo"]);
    let request = expect_ok(fixture.process().run(spec("echo", &["x"])));
    complete(&mut fixture, request);
    assert!(fixture.process().take_exit(request).is_some());
    assert!(fixture.process().take_exit(request).is_none());
}

/// REQ: WIT:ilium:host/process.take-exit
#[test]
fn process_take_exit_for_an_unknown_request_is_none() {
    let mut fixture = runner(&["echo"]);
    assert!(fixture.process().take_exit(RequestId(61_616)).is_none());
}

/// REQ: WIT:ilium:host/process.run
#[test]
fn process_run_non_zero_exit_is_not_an_error() {
    let mut fixture = runner(&["sh"]);
    let exit = run_exit(&mut fixture, spec("sh", &["-c", "exit 3"]));
    assert_eq!(exit.status, Some(3));
}

/// REQ: WIT:ilium:host/process.run
#[test]
fn process_run_stderr_is_captured_separately() {
    let mut fixture = runner(&["sh"]);
    let exit = run_exit(&mut fixture, spec("sh", &["-c", "echo out; echo err 1>&2"]));
    assert_eq!(exit.stdout, b"out\n");
    assert_eq!(exit.stderr, b"err\n");
}

/// REQ: WIT:ilium:host/process.run
#[test]
fn process_run_stdin_bytes_reach_the_program() {
    let mut fixture = runner(&["cat"]);
    let mut process = spec("cat", &[]);
    process.stdin = Some(b"fed".to_vec());
    assert_eq!(run_exit(&mut fixture, process).stdout, b"fed");
}

/// REQ: WIT:ilium:host/process.run
#[test]
fn process_run_program_not_in_the_grants_invalid_argument() {
    let mut fixture = runner(&["echo"]);
    assert_eq!(
        expect_code(fixture.process().run(spec("ls", &[]))),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/process.run
#[test]
fn process_run_without_any_process_grant_not_permitted_or_invalid_argument() {
    let mut fixture = fixture(Tier::T1, &[]);
    let code = expect_code(fixture.process().run(spec("echo", &[])));
    assert!(
        matches!(code, ErrorCode::NotPermitted | ErrorCode::InvalidArgument),
        "{code:?}"
    );
}

/// REQ: WIT:ilium:host/process.run
#[test]
fn process_run_environment_holds_only_path_home_lang_and_the_extras() {
    let mut fixture = runner(&["env"]);
    let mut process = spec("env", &[]);
    process.environment = vec![("EXTRA".to_owned(), "1".to_owned())];
    let text = String::from_utf8(run_exit(&mut fixture, process).stdout).unwrap();
    let mut names: Vec<&str> = text
        .lines()
        .filter_map(|line| line.split('=').next())
        .collect();
    names.sort_unstable();
    names.retain(|name| !matches!(*name, "PWD" | "SHLVL" | "_" | "OLDPWD"));
    assert_eq!(names, ["EXTRA", "HOME", "LANG", "PATH"]);
}

/// REQ: WIT:ilium:host/process.run
#[test]
fn process_run_timeout_boundaries_0_1_600000_600001() {
    let mut fixture = runner(&["echo"]);
    for (millis, accepted) in [(0u32, false), (1, true), (600_000, true), (600_001, false)] {
        let mut process = spec("echo", &[]);
        process.timeout_millis = millis;
        let result = fixture.process().run(process);
        assert_eq!(result.is_ok(), accepted, "timeout {millis}");
        if !accepted {
            assert_eq!(result.unwrap_err().code, ErrorCode::InvalidArgument);
        }
    }
}

/// REQ: WIT:ilium:host/process.run
#[test]
fn process_run_stdin_of_4_mib_accepted_and_one_more_invalid_argument() {
    let mut fixture = runner(&["cat"]);
    let mut ok = spec("cat", &[]);
    ok.stdin = Some(vec![b'a'; 4 * MIB]);
    assert!(fixture.process().run(ok).is_ok());
    let mut over = spec("cat", &[]);
    over.stdin = Some(vec![b'a'; 4 * MIB + 1]);
    assert_eq!(
        expect_code(fixture.process().run(over)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/process.run
#[test]
fn process_run_1024_arguments_accepted_and_1025_invalid_argument() {
    let mut fixture = runner(&["echo"]);
    let many = |count: usize| ProcessSpec {
        arguments: (0..count).map(|index| index.to_string()).collect(),
        ..spec("echo", &[])
    };
    assert!(fixture.process().run(many(1024)).is_ok());
    assert_eq!(
        expect_code(fixture.process().run(many(1025))),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/process.run
#[test]
fn process_run_64_environment_pairs_accepted_and_65_invalid_argument() {
    let mut fixture = runner(&["echo"]);
    let envs = |count: usize| ProcessSpec {
        environment: (0..count)
            .map(|index| (format!("V{index}"), "1".to_owned()))
            .collect(),
        ..spec("echo", &[])
    };
    assert!(fixture.process().run(envs(64)).is_ok());
    assert_eq!(
        expect_code(fixture.process().run(envs(65))),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/process.run
#[test]
fn process_run_cwd_outside_a_readable_scope_not_permitted() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = fixture_in(dir.path(), Tier::T1, &["process.run:echo"]);
    let mut process = spec("echo", &[]);
    process.cwd = Some(project_path("."));
    assert_eq!(
        expect_code(fixture.process().run(process)),
        ErrorCode::NotPermitted
    );
}

/// REQ: WIT:ilium:host/process.run
#[test]
fn process_run_cwd_in_a_readable_scope_is_used() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = fixture_in(
        dir.path(),
        Tier::T1,
        &["process.run:pwd", "fs.read:project"],
    );
    let mut process = spec("pwd", &[]);
    process.cwd = Some(project_path("."));
    let out = String::from_utf8(run_exit(&mut fixture, process).stdout).unwrap();
    assert_eq!(
        std::fs::canonicalize(out.trim()).unwrap(),
        std::fs::canonicalize(dir.path()).unwrap()
    );
}

/// REQ: WIT:ilium:host/process.run
#[test]
fn process_run_timeout_completes_failed_timed_out() {
    let mut fixture = runner(&["sleep"]);
    let mut process = spec("sleep", &["30"]);
    process.timeout_millis = 200;
    let request = expect_ok(fixture.process().run(process));
    assert_eq!(
        outcome_code(&complete(&mut fixture, request)),
        Some(ErrorCode::TimedOut)
    );
}

/// REQ: WIT:ilium:host/process.run
#[test]
fn process_run_output_over_the_cap_completes_failed_quota_exceeded() {
    let mut fixture = runner(&["sh"]);
    let process = spec("sh", &["-c", "head -c 40000000 /dev/zero"]);
    let request = expect_ok(fixture.process().run(process));
    assert_eq!(
        outcome_code(&complete(&mut fixture, request)),
        Some(ErrorCode::QuotaExceeded)
    );
}

/// REQ: WIT:ilium:host/process.run
#[test]
fn process_run_missing_program_completes_failed_not_found() {
    let mut fixture = runner(&["definitely-not-installed-xyz"]);
    let result = fixture
        .process()
        .run(spec("definitely-not-installed-xyz", &[]));
    match result {
        Err(error) => assert_eq!(error.code, ErrorCode::NotFound),
        Ok(request) => assert_eq!(
            outcome_code(&complete(&mut fixture, request)),
            Some(ErrorCode::NotFound)
        ),
    }
}

/// REQ: WIT:ilium:host/process.run
#[test]
fn process_run_is_logged_at_info_with_program_and_arguments() {
    let mut fixture = runner(&["echo"]);
    run_exit(&mut fixture, spec("echo", &["marker-arg"]));
    assert!(fixture.logs().iter().any(|line| line.level == "info"
        && line.message.contains("echo")
        && line.message.contains("marker-arg")));
}

/// REQ: WIT:ilium:host/process.run
#[test]
fn process_run_65th_pending_request_busy() {
    let mut fixture = runner(&["echo"]);
    fixture.hold_async_completions(true);
    for _ in 0..64 {
        expect_ok(fixture.process().run(spec("echo", &[])));
    }
    assert_eq!(
        expect_code(fixture.process().run(spec("echo", &[]))),
        ErrorCode::Busy
    );
}

/// REQ: WIT:ilium:host/process.run
#[test]
fn process_run_17th_session_child_busy() {
    let mut fixture = runner(&["sleep"]);
    fixture.hold_async_completions(true);
    let mut refused = None;
    for _ in 0..40 {
        if let Err(error) = fixture.process().run(spec("sleep", &["5"])) {
            refused = Some(error.code);
            break;
        }
    }
    assert!(matches!(refused, Some(ErrorCode::Busy)), "{refused:?}");
}

proptest! {
    // REQ: WIT:ilium:host/process.run
    #[test]
    fn process_run_echo_returns_its_arguments_verbatim(word in "[a-zA-Z0-9]{1,40}") {
        let mut fixture = runner(&["echo"]);
        let exit = run_exit(&mut fixture, spec("echo", &[word.as_str()]));
        prop_assert_eq!(exit.stdout, format!("{word}\n").into_bytes());
    }
}

// ---- spawn, child ----------------------------------------------------------------------------

fn spawn_spec(program: &str, arguments: &[&str]) -> SpawnSpec {
    SpawnSpec {
        process: spec(program, arguments),
        stdin_open: true,
        line_mode: false,
    }
}

/// REQ: WIT:ilium:host/process.spawn
#[test]
fn process_spawn_returns_a_handle_with_a_pid_hint() {
    let mut fixture = spawner(&["cat"]);
    let child = expect_ok(fixture.process().spawn(spawn_spec("cat", &[])));
    assert!(fixture.process().child_pid_hint(child) > 0);
}

/// REQ: WIT:ilium:host/process.spawn
#[test]
fn process_spawn_without_the_long_lived_grant_not_permitted() {
    let mut fixture = runner(&["cat"]);
    assert_eq!(
        expect_code(fixture.process().spawn(spawn_spec("cat", &[]))),
        ErrorCode::NotPermitted
    );
}

/// REQ: WIT:ilium:host/process.spawn
#[test]
fn process_spawn_without_the_run_grant_for_the_program_not_permitted() {
    let mut fixture = fixture(Tier::T1, &["process.long-lived"]);
    assert_eq!(
        expect_code(fixture.process().spawn(spawn_spec("cat", &[]))),
        ErrorCode::NotPermitted
    );
}

/// REQ: WIT:ilium:host/process.spawn
#[test]
fn process_spawn_missing_program_not_found() {
    let mut fixture = spawner(&["definitely-not-installed-xyz"]);
    assert_eq!(
        expect_code(
            fixture
                .process()
                .spawn(spawn_spec("definitely-not-installed-xyz", &[]))
        ),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/process.spawn
#[test]
fn process_spawn_third_child_of_a_plugin_quota_exceeded() {
    let mut fixture = spawner(&["cat"]);
    expect_ok(fixture.process().spawn(spawn_spec("cat", &[])));
    expect_ok(fixture.process().spawn(spawn_spec("cat", &[])));
    assert_eq!(
        expect_code(fixture.process().spawn(spawn_spec("cat", &[]))),
        ErrorCode::QuotaExceeded
    );
}

/// REQ: WIT:ilium:host/process.child.id
#[test]
fn process_child_id_is_stable_and_distinct_between_children() {
    let mut fixture = spawner(&["cat"]);
    let first = expect_ok(fixture.process().spawn(spawn_spec("cat", &[])));
    let second = expect_ok(fixture.process().spawn(spawn_spec("cat", &[])));
    let first_id = fixture.process().child_id(first);
    assert_eq!(fixture.process().child_id(first), first_id);
    assert_ne!(fixture.process().child_id(second), first_id);
}

/// REQ: WIT:ilium:host/process.child.pid-hint
#[test]
fn process_child_pid_hint_differs_between_children() {
    let mut fixture = spawner(&["cat"]);
    let first = expect_ok(fixture.process().spawn(spawn_spec("cat", &[])));
    let second = expect_ok(fixture.process().spawn(spawn_spec("cat", &[])));
    assert_ne!(
        fixture.process().child_pid_hint(first),
        fixture.process().child_pid_hint(second)
    );
}

/// REQ: WIT:ilium:host/process.child.write-stdin
#[test]
fn process_child_write_stdin_reaches_the_child_output() {
    let mut fixture = spawner(&["cat"]);
    let child = expect_ok(fixture.process().spawn(spawn_spec("cat", &[])));
    expect_ok(
        fixture
            .process()
            .child_write_stdin(child, b"ping\n".to_vec()),
    );
    assert!(fixture.wait_child_output(child, b"ping\n", WAIT));
}

/// REQ: WIT:ilium:host/process.child.write-stdin
#[test]
fn process_child_write_stdin_with_stdin_closed_invalid_argument() {
    let mut fixture = spawner(&["cat"]);
    let mut closed = spawn_spec("cat", &[]);
    closed.stdin_open = false;
    let child = expect_ok(fixture.process().spawn(closed));
    assert_eq!(
        expect_code(fixture.process().child_write_stdin(child, b"x".to_vec())),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/process.child.write-stdin
#[test]
fn process_child_write_stdin_after_close_stdin_invalid_argument() {
    let mut fixture = spawner(&["cat"]);
    let child = expect_ok(fixture.process().spawn(spawn_spec("cat", &[])));
    fixture.process().child_close_stdin(child);
    assert_eq!(
        expect_code(fixture.process().child_write_stdin(child, b"x".to_vec())),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/process.child.close-stdin
#[test]
fn process_child_close_stdin_ends_a_cat_child() {
    let mut fixture = spawner(&["cat"]);
    let child = expect_ok(fixture.process().spawn(spawn_spec("cat", &[])));
    fixture.process().child_close_stdin(child);
    assert!(fixture.wait_child_exit(child, WAIT));
}

/// REQ: WIT:ilium:host/process.child.close-stdin
#[test]
fn process_child_close_stdin_twice_is_harmless() {
    let mut fixture = spawner(&["cat"]);
    let child = expect_ok(fixture.process().spawn(spawn_spec("cat", &[])));
    fixture.process().child_close_stdin(child);
    fixture.process().child_close_stdin(child);
    assert!(fixture.wait_child_exit(child, WAIT));
}

/// REQ: WIT:ilium:host/process.child.signal
#[test]
fn process_child_signal_each_known_signal_is_accepted() {
    for signal in ["interrupt", "terminate", "kill", "hangup"] {
        let mut fixture = spawner(&["sleep"]);
        let child = expect_ok(fixture.process().spawn(spawn_spec("sleep", &["30"])));
        expect_ok(fixture.process().child_signal(child, signal.to_owned()));
        assert!(fixture.wait_child_exit(child, WAIT), "{signal}");
    }
}

/// REQ: WIT:ilium:host/process.child.signal
#[test]
fn process_child_signal_unknown_name_invalid_argument() {
    let mut fixture = spawner(&["sleep"]);
    let child = expect_ok(fixture.process().spawn(spawn_spec("sleep", &["30"])));
    assert_eq!(
        expect_code(fixture.process().child_signal(child, "explode".to_owned())),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/process.child.signal
#[test]
fn process_child_signal_unknown_handle_not_found() {
    let mut fixture = spawner(&["sleep"]);
    assert_eq!(
        expect_code(
            fixture
                .process()
                .child_signal(ChildHandle(4_000_000), "kill".to_owned())
        ),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/process.take-exit
#[test]
fn process_take_exit_for_a_signalled_child_run_reports_the_signal() {
    let mut fixture = runner(&["sh"]);
    let exit = run_exit(&mut fixture, spec("sh", &["-c", "kill -TERM $$"]));
    assert!(exit.signal.is_some() || exit.status.map_or(false, |status| status != 0));
}
