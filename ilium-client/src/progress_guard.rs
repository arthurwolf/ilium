//! Claude Code `PreToolUse` guard for the progress-monitor contract.
//!
//! The managed progress instructions tell agents to run any task of three
//! minutes or more detached, under an Ilium progress monitor. Capable models
//! follow the text; smaller ones (observed with Claude Haiku) often run the
//! task as one long foreground Bash call instead, so the user sees no progress
//! bar. This guard refuses exactly that pattern inside an Ilium pane: a
//! foreground Bash call whose tool timeout exceeds three minutes and which is
//! not itself an `ilium progress` command. Everything else passes, including
//! background commands (dev servers legitimately run that way) and every call
//! outside an Ilium pane. Malformed hook input never blocks a tool call.
//!
//! The refusal is a JSON `permissionDecision` on stdout with exit status 0,
//! never exit status 2. The project hook command is `ilium progress guard ||
//! true`, so an `ilium` binary that predates this subcommand (whose argument
//! error exits 2) lets every call through instead of refusing all of them.

use std::io::Read;

/// Claude Code's default Bash timeout is two minutes; a caller asking for
/// more than three expects a task the progress contract covers.
const LONG_FOREGROUND_MILLISECONDS: u64 = 180_000;

/// Bounded hook input; Claude Code sends one small JSON object.
const MAX_INPUT_BYTES: u64 = 1024 * 1024;

pub const REFUSAL: &str = "Ilium: this pane requires a progress monitor for commands that may run 3 minutes or more; do not run them in the foreground. Start the command detached (setsid -f, output to a log), write a probe, run 'ilium progress check --command <probe>', then 'ilium progress set --command <probe> --interval-seconds <n> --wait --timeout-seconds 590' with Bash timeout 600000. If this command is short, rerun it with a Bash timeout of at most 180000.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardDecision {
    Allow,
    Refuse,
}

/// Pure decision over one `PreToolUse` payload.
pub fn evaluate(input: &serde_json::Value, inside_ilium_pane: bool) -> GuardDecision {
    if !inside_ilium_pane {
        return GuardDecision::Allow;
    }
    if input
        .get("tool_name")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|tool| tool != "Bash")
    {
        return GuardDecision::Allow;
    }
    let Some(tool_input) = input.get("tool_input") else {
        return GuardDecision::Allow;
    };
    if tool_input
        .get("run_in_background")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return GuardDecision::Allow;
    }
    let command = tool_input
        .get("command")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if command.contains("ilium progress") || command.contains("ilium wait") {
        return GuardDecision::Allow;
    }
    let timeout = tool_input
        .get("timeout")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    if timeout > LONG_FOREGROUND_MILLISECONDS {
        GuardDecision::Refuse
    } else {
        GuardDecision::Allow
    }
}

/// Claude Code `PreToolUse` output that refuses the call and shows the
/// reason to the model.
pub fn refusal_output() -> serde_json::Value {
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": REFUSAL,
        }
    })
}

/// Runs the hook: reads the payload from stdin, prints the refusal JSON when
/// the call is refused, and returns the process exit status, which is always
/// 0 (see the module comment).
pub fn run() -> u8 {
    let inside_ilium_pane =
        std::env::var_os("ILIUM_PANE_ID").is_some_and(|value| !value.is_empty());
    if !inside_ilium_pane {
        return 0;
    }
    let mut raw = Vec::new();
    if std::io::stdin()
        .take(MAX_INPUT_BYTES)
        .read_to_end(&mut raw)
        .is_err()
    {
        return 0;
    }
    let Ok(input) = serde_json::from_slice::<serde_json::Value>(&raw) else {
        return 0;
    };
    match evaluate(&input, inside_ilium_pane) {
        GuardDecision::Allow => 0,
        GuardDecision::Refuse => {
            println!("{}", refusal_output());
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{evaluate, refusal_output, GuardDecision};
    use serde_json::json;

    fn bash(input: serde_json::Value) -> serde_json::Value {
        json!({"tool_name": "Bash", "tool_input": input})
    }

    #[test]
    fn long_foreground_command_in_a_pane_is_refused() {
        let input = bash(json!({"command": "make release", "timeout": 600000}));
        assert_eq!(evaluate(&input, true), GuardDecision::Refuse);
    }

    #[test]
    fn same_command_outside_a_pane_is_allowed() {
        let input = bash(json!({"command": "make release", "timeout": 600000}));
        assert_eq!(evaluate(&input, false), GuardDecision::Allow);
    }

    #[test]
    fn progress_waits_background_and_short_commands_are_allowed() {
        for input in [
            bash(
                json!({"command": "ilium progress set --command /p --interval-seconds 5 --wait", "timeout": 600000}),
            ),
            bash(json!({"command": "ilium wait 3", "timeout": 600000})),
            bash(json!({"command": "npm run dev", "timeout": 600000, "run_in_background": true})),
            bash(json!({"command": "cargo test", "timeout": 180000})),
            bash(json!({"command": "ls"})),
            json!({"tool_name": "Edit", "tool_input": {"timeout": 600000}}),
            json!({}),
        ] {
            assert_eq!(evaluate(&input, true), GuardDecision::Allow, "{input}");
        }
    }

    #[test]
    fn refusal_is_a_claude_code_deny_decision() {
        let output = refusal_output();
        let decision = &output["hookSpecificOutput"];
        assert_eq!(decision["hookEventName"], "PreToolUse");
        assert_eq!(decision["permissionDecision"], "deny");
        assert!(decision["permissionDecisionReason"]
            .as_str()
            .unwrap()
            .contains("progress monitor"));
    }
}
