//! G10 `process`: a per-instance import that runs real child processes under `process.run`
//! grants, reports `request-completed`, `child-output` and `child-exited` through the runtime
//! queue, caps live children per plugin and per session, and stops its children when the instance
//! ends (api-process.md).

use std::time::{Duration, Instant};

use ilium_plugin_api::generated::host::process::{
    ChildHandle, HostProcess, ProcessSpec, SpawnSpec,
};
use ilium_plugin_api::generated::types::errors::ErrorCode;
use ilium_plugin_api::generated::types::events::{Event, OutputStream};
use ilium_plugin_api::generated::types::ids::RequestId;
use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef};
use ilium_plugin_host::imports::env::{ImportEnv, SessionCounters};
use ilium_plugin_host::imports::process::ProcessImport;

const PATIENCE: Duration = Duration::from_secs(20);

fn env_with(grants: &[&str]) -> ImportEnv {
    ImportEnv::unrestricted()
        .with_identity("test.process", 1)
        .with_grants(grants.iter().map(|grant| (*grant).to_owned()).collect())
}

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
        timeout_millis: 10_000,
    }
}

fn spawn_spec(program: &str, arguments: &[&str]) -> SpawnSpec {
    SpawnSpec {
        process: spec(program, arguments),
        stdin_open: false,
        line_mode: false,
    }
}

/// Collects queued events until `done` accepts the whole list or the patience runs out.
fn collect_until(env: &ImportEnv, done: impl Fn(&[Event]) -> bool) -> Vec<Event> {
    let started = Instant::now();
    let mut seen: Vec<Event> = Vec::new();
    while started.elapsed() < PATIENCE {
        seen.extend(env.events().drain().into_iter().map(|queued| queued.event));
        if done(&seen) {
            return seen;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    seen
}

fn completed(events: &[Event], request: RequestId) -> Option<Outcome> {
    events.iter().find_map(|event| match event {
        Event::RequestCompleted(result) if result.request == RequestRef::Call(request) => {
            Some(result.outcome.clone())
        }
        _ => None,
    })
}

fn wait_for_no_session_process(session: &SessionCounters) {
    let started = Instant::now();
    while session.processes() > 0 && started.elapsed() < PATIENCE {
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// REQ: SEC-031
#[test]
fn a_program_without_a_run_grant_is_refused_and_nothing_starts() {
    let env = env_with(&["process.run:echo"]);
    let mut process = ProcessImport::new(env.clone());
    let refused = process.run(spec("sh", &["-c", "echo no"]));
    assert_eq!(
        refused.expect_err("sh is not granted").code,
        ErrorCode::NotPermitted
    );
    assert_eq!(
        env.session().processes(),
        0,
        "a refused run holds no session slot"
    );
    let refused = process.spawn(spawn_spec("echo", &["x"]));
    assert_eq!(
        refused.expect_err("spawn needs process.long-lived").code,
        ErrorCode::NotPermitted
    );
}

/// REQ: SEC-031
#[test]
fn a_granted_run_completes_through_a_request_completed_event_and_take_exit() {
    let env = env_with(&["process.run:echo"]);
    let mut process = ProcessImport::new(env.clone());
    let request = process.run(spec("echo", &["hello"])).expect("run starts");
    let events = collect_until(&env, |seen| completed(seen, request).is_some());
    assert_eq!(completed(&events, request), Some(Outcome::Ok), "{events:?}");
    let exit = process
        .take_exit(request)
        .expect("the exit is ready after completion");
    assert_eq!(exit.status, Some(0));
    assert_eq!(String::from_utf8_lossy(&exit.stdout), "hello\n");
    assert!(
        process.take_exit(request).is_none(),
        "an exit is taken once"
    );
    assert_eq!(
        env.session().processes(),
        0,
        "the session slot is free again"
    );
}

/// REQ: SEC-033
#[test]
fn a_run_past_its_timeout_ends_as_timed_out_and_its_slot_is_freed() {
    let env = env_with(&["process.run:sleep"]);
    let mut process = ProcessImport::new(env.clone());
    let mut slow = spec("sleep", &["30"]);
    slow.timeout_millis = 300;
    let request = process.run(slow).expect("run starts");
    let events = collect_until(&env, |seen| completed(seen, request).is_some());
    match completed(&events, request) {
        Some(Outcome::Failed(error)) => assert_eq!(error.code, ErrorCode::TimedOut),
        other => panic!("expected a timed-out failure, got {other:?}"),
    }
    wait_for_no_session_process(env.session());
    assert_eq!(env.session().processes(), 0);
}

/// REQ: SEC-031
#[test]
fn two_instances_do_not_see_each_others_requests_or_events() {
    let first_env = env_with(&["process.run:echo"]).with_identity("test.first", 1);
    let second_env = env_with(&["process.run:echo"]).with_identity("test.second", 1);
    let mut first = ProcessImport::new(first_env.clone());
    let mut second = ProcessImport::new(second_env.clone());
    let request = first.run(spec("echo", &["one"])).expect("run starts");
    let events = collect_until(&first_env, |seen| completed(seen, request).is_some());
    assert!(completed(&events, request).is_some());
    assert!(
        second.take_exit(request).is_none(),
        "a request id of another instance resolves to nothing"
    );
    assert!(
        second_env.events().is_empty(),
        "the second instance has its own queue"
    );
    assert!(first.take_exit(request).is_some());
}

/// REQ: SEC-031
#[test]
fn a_spawned_child_streams_output_and_reports_its_exit_status() {
    let env = env_with(&["process.run:sh", "process.long-lived"]);
    let mut process = ProcessImport::new(env.clone());
    let handle: ChildHandle = process
        .spawn(spawn_spec(
            "sh",
            &["-c", "printf out; printf err >&2; exit 3"],
        ))
        .expect("spawn starts");
    let child = process.child_id(handle);
    let events = collect_until(&env, |seen| {
        seen.iter()
            .any(|event| matches!(event, Event::ChildExited(exited) if exited.child == child))
    });
    let output = |stream: OutputStream| -> String {
        events
            .iter()
            .filter_map(|event| match event {
                Event::ChildOutput(chunk) if chunk.child == child && chunk.stream == stream => {
                    Some(String::from_utf8_lossy(&chunk.bytes).into_owned())
                }
                _ => None,
            })
            .collect()
    };
    assert_eq!(output(OutputStream::Stdout), "out", "{events:?}");
    assert_eq!(output(OutputStream::Stderr), "err", "{events:?}");
    let exited = events
        .iter()
        .find_map(|event| match event {
            Event::ChildExited(exited) if exited.child == child => Some(exited.clone()),
            _ => None,
        })
        .expect("the child exited");
    assert_eq!(exited.status, Some(3));
}

/// REQ: SEC-033
#[test]
fn a_plugin_holds_at_most_two_live_children() {
    let env = env_with(&["process.run:sleep", "process.long-lived"]);
    let mut process = ProcessImport::new(env.clone());
    process
        .spawn(spawn_spec("sleep", &["30"]))
        .expect("first child");
    process
        .spawn(spawn_spec("sleep", &["30"]))
        .expect("second child");
    let third = process.spawn(spawn_spec("sleep", &["30"]));
    assert_eq!(
        third.expect_err("third child").code,
        ErrorCode::QuotaExceeded
    );
    drop(process);
    wait_for_no_session_process(env.session());
    assert_eq!(
        env.session().processes(),
        0,
        "dropping the import ended the children"
    );
}

/// REQ: SEC-033
#[test]
fn the_session_cap_is_shared_by_every_instance_of_the_runtime() {
    let session = SessionCounters::new();
    let mut slots = Vec::new();
    while let Some(slot) = session.try_acquire_process(16) {
        slots.push(slot);
    }
    assert_eq!(session.processes(), 16);
    let env = env_with(&["process.run:echo", "process.long-lived"]).with_session(session.clone());
    let mut process = ProcessImport::new(env);
    let busy = process.run(spec("echo", &["x"]));
    assert_eq!(busy.expect_err("the session is full").code, ErrorCode::Busy);
    let quota = process.spawn(spawn_spec("echo", &["x"]));
    assert_eq!(
        quota.expect_err("the session is full").code,
        ErrorCode::QuotaExceeded
    );
    drop(slots);
    assert_eq!(session.processes(), 0);
}

/// REQ: SEC-031
#[test]
fn a_working_folder_outside_the_granted_scopes_is_refused() {
    let env = env_with(&["process.run:echo"]);
    let mut process = ProcessImport::new(env);
    let mut escaping = spec("echo", &["x"]);
    escaping.cwd = Some(ilium_plugin_api::generated::types::paths::FsPath {
        scope: "project".to_owned(),
        path: "../outside".to_owned(),
    });
    let refused = process.run(escaping);
    assert!(
        matches!(
            refused.map_err(|error| error.code),
            Err(ErrorCode::NotPermitted | ErrorCode::InvalidArgument | ErrorCode::NotFound)
        ),
        "an escaping working folder must be refused"
    );
}
