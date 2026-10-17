//! The Claude Code hook that refuses long foreground commands in a pane (original
//! ilium-client/src/progress_guard.rs). The installer rows are skipped, see the report.

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::{json, Value};

use crate::common::*;

fn guard(host: &mut FakeHost, pane: Option<u64>, input: &Value) -> Run {
    set_pane_env(host, pane);
    let invocation = host
        .cli()
        .invoke(&["progress", "guard"], input.to_string().as_bytes());
    Run {
        stdout: invocation.stdout(),
        stderr: invocation.stderr(),
        exit: invocation.exit_code(),
    }
}

fn bash(command: &str, timeout: Option<u64>, background: bool) -> Value {
    let mut tool_input = json!({"command": command, "run_in_background": background});
    if let Some(timeout) = timeout {
        tool_input["timeout"] = json!(timeout);
    }
    json!({"tool_name": "Bash", "tool_input": tool_input})
}

fn denied(run: &Run) -> bool {
    run.stdout.contains("\"permissionDecision\":\"deny\"")
        || run.stdout.contains("\"permissionDecision\": \"deny\"")
}

/// REQ: PORT:ilium-client/src/progress_guard.rs::long_foreground_command_in_a_pane_is_refused
#[test]
fn port_060_a_foreground_command_over_180_seconds_in_a_pane_is_refused() {
    let (mut host, pane) = host_with_pane();
    let run = guard(
        &mut host,
        Some(pane),
        &bash("cargo test", Some(600_000), false),
    );
    assert!(denied(&run), "{}", run.stdout);
}

/// REQ: PORT:ilium-client/src/progress_guard.rs::progress_waits_background_and_short_commands_are_allowed
#[test]
fn port_060_waits_background_runs_short_timeouts_other_tools_and_empty_input_are_allowed() {
    let (mut host, pane) = host_with_pane();
    let allowed = [
        bash(
            "ilium progress set --command x --wait",
            Some(600_000),
            false,
        ),
        bash("ilium wait 3", Some(600_000), false),
        bash("cargo test", Some(600_000), true),
        bash("cargo test", Some(180_000), false),
        bash("cargo test", None, false),
        json!({"tool_name": "Read", "tool_input": {"timeout": 600_000}}),
        json!({}),
    ];
    for input in allowed {
        let run = guard(&mut host, Some(pane), &input);
        assert!(!denied(&run), "{input}: {}", run.stdout);
    }
}

/// REQ: PORT:ilium-client/src/progress_guard.rs::refusal_is_a_claude_code_deny_decision
#[test]
fn port_060_the_refusal_is_a_pre_tool_use_deny_decision_that_names_the_progress_monitor() {
    let (mut host, pane) = host_with_pane();
    let run = guard(
        &mut host,
        Some(pane),
        &bash("cargo test", Some(600_000), false),
    );
    let value: Value = serde_json::from_str(run.stdout.trim()).expect("one JSON object");
    assert_eq!(value["hookSpecificOutput"]["hookEventName"], "PreToolUse");
    assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
    assert!(value["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap_or_default()
        .contains("progress monitor"));
}

/// REQ: PORT:ilium-client/src/progress_guard.rs::same_command_outside_a_pane_is_allowed
#[test]
fn port_060_the_same_command_outside_a_pane_is_allowed() {
    let (mut host, _pane) = host_with_pane();
    let run = guard(&mut host, None, &bash("cargo test", Some(600_000), false));
    assert!(!denied(&run), "{}", run.stdout);
}
