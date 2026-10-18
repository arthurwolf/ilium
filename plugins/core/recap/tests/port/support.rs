//! Helpers shared by the port twins.

use serde_json::{json, Value};

use ilium_plugin_sdk::testing::fakes::cli::Invocation;
use ilium_plugin_sdk::testing::FakeHost;

use crate::common::*;

pub fn words(count: usize) -> String {
    vec!["word"; count].join(" ")
}

pub fn answer(short_words: usize, long_words: usize) -> Vec<u8> {
    json!({"short": {"text": words(short_words), "pointers": []}, "long": {"text": words(long_words), "pointers": []}})
        .to_string()
        .into_bytes()
}

/// The answer the monolith pipeline tests use: one real pointer and one invented one.
pub const GOOD_ANSWER: &str = r#"{"short": {"text": "Goal done: ship recap. Edited src/lib.rs.", "pointers": [{"path": "src/lib.rs", "note": "edited"}, {"path": "/etc/shadow", "note": "invented"}]}, "long": {"text": "Long recap text for the pane.", "pointers": []}}"#;

pub fn rows(screen: &[&str]) -> Vec<String> {
    screen.iter().map(|row| row.to_string()).collect()
}

/// One shell pane with this screen, gate open; returns the host, the pane id and the invocation.
pub fn start_shell_run(screen: &[&str]) -> (FakeHost, u64, Invocation) {
    let mut host = activated_server();
    open_gate(&mut host);
    let pane = host.tree().add(shell_pane("build shell"));
    host.tree().set_screen(pane, rows(screen));
    let invocation = host.cli().invoke(&["recap"], b"");
    (host, pane, invocation)
}

/// One agent pane with a verified session and this transcript (one string per turn).
pub fn start_agent_run(transcript: &[&str]) -> (FakeHost, u64, Invocation) {
    let mut host = activated_server();
    open_gate(&mut host);
    let pane = host.tree().add(agent_pane("worker", "claude"));
    host.tree()
        .set_agent_session(pane, "session-1", rows(transcript));
    let invocation = host.cli().invoke(&["recap"], b"");
    (host, pane, invocation)
}

/// Finishes the first job with `result`, lets the run end and returns the output records.
pub fn finish_run(host: &mut FakeHost, invocation: &Invocation, result: Vec<u8>) -> Vec<Value> {
    let request = host.jobs().list()[0].request;
    host.jobs().finish(request, Ok(result));
    host.settle();
    jsonl(&host.cli().refresh(invocation).stdout())
}

/// The `evidence` text of the first started job.
pub fn job_evidence(host: &mut FakeHost) -> String {
    let job = host
        .jobs()
        .list()
        .into_iter()
        .next()
        .expect("a job was started");
    let input: Value = serde_json::from_slice(&job.input).expect("job input is JSON");
    input["evidence"]
        .as_str()
        .expect("evidence is a string")
        .to_string()
}

pub fn first_recap(records: &[Value]) -> Value {
    of_type(records, "recap")[0].clone()
}

/// Runs a shell pane with `screen`, finishes the job with `body` and returns the short text object.
pub fn short_after_answer(screen: &[&str], files: &[(&str, &str)], body: Value) -> Value {
    let (mut host, _, invocation) = start_shell_run(screen);
    for (path, content) in files {
        host.fs()
            .write(format!("project:{path}"), content.as_bytes());
    }
    let records = finish_run(&mut host, &invocation, body.to_string().into_bytes());
    first_recap(&records)["short"].clone()
}

pub fn pointer_answer(path: &str, note: &str) -> Value {
    json!({
        "short": {"text": "pointer text", "pointers": [{"path": path, "note": note}]},
        "long": {"text": "pointer text", "pointers": []}
    })
}

/// An agent pane with a verified session, the recorded goals `(objective, status)` set on it,
/// then the run started. Status is one of active, paused, blocked, done, unknown.
pub fn start_agent_run_with_goals(
    transcript: &[&str],
    goals: &[(String, &str)],
) -> (FakeHost, u64, Invocation) {
    let mut host = activated_server();
    open_gate(&mut host);
    let pane = host.tree().add(agent_pane("worker", "claude"));
    host.tree()
        .set_agent_session(pane, "session-1", rows(transcript));
    for (objective, status) in goals {
        host.tree().set_agent_goal(pane, objective, status);
    }
    let invocation = host.cli().invoke(&["recap"], b"");
    (host, pane, invocation)
}

/// The goal lines of the only record after the run finished with a valid answer.
pub fn goal_lines(transcript: &[&str], goals: &[(String, &str)]) -> Vec<String> {
    let (mut host, _, invocation) = start_agent_run_with_goals(transcript, goals);
    let records = finish_run(&mut host, &invocation, answer(3, 3));
    first_recap(&records)["goals"]
        .as_array()
        .expect("goals is an array")
        .iter()
        .map(|line| line.as_str().expect("goal line").to_string())
        .collect()
}
