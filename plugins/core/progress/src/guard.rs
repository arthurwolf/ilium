//! Claude Code `PreToolUse` guard for the progress-monitor contract, plugin form.
//!
//! Text in, text out: the host's `fs` and `cli` imports read the settings file and stdin,
//! this module only decides.
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
//! A background Bash call in a pane passes with a one-sentence reminder in
//! `additionalContext`: Haiku was observed running a three-minute task with
//! `run_in_background` and waiting for Claude Code's completion notice, which
//! shows the user no progress bar. The reminder never changes the permission
//! decision, so servers and short background commands run as before.
//!
//! The refusal is a JSON `permissionDecision` on stdout with exit status 0,
//! never exit status 2. The project hook command is `ilium progress guard ||
//! true`, so an `ilium` binary that predates this subcommand (whose argument
//! error exits 2) lets every call through instead of refusing all of them.

/// Claude Code's default Bash timeout is two minutes; a caller asking for
/// more than three expects a task the progress contract covers.
const LONG_FOREGROUND_MILLISECONDS: u64 = 180_000;

pub const REFUSAL: &str = "Ilium: this pane requires a progress monitor for commands that may run 3 minutes or more; do not run them in the foreground. Start the command detached (setsid -f, output to a log), write a probe, run 'ilium progress check --command <probe>', then 'ilium progress set --command <probe> --interval-seconds <n> --wait' as a background Bash command; the logical wait has no elapsed-time deadline. If this command is short, rerun it with a Bash timeout of at most 180000.";

pub const BACKGROUND_REMINDER: &str = "Ilium: if this background command may run 3 minutes or more, Claude Code's completion notice is not enough; also write a probe and run 'ilium progress set --command <probe> --interval-seconds <n> --wait' as a background Bash command so the user sees its progress. Servers and short commands need nothing.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardDecision {
    Allow,
    /// Allow without touching the permission decision, adding a reminder.
    Remind,
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
    let command = tool_input
        .get("command")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if command.contains("ilium progress") || command.contains("ilium wait") {
        return GuardDecision::Allow;
    }
    if tool_input
        .get("run_in_background")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return GuardDecision::Remind;
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

/// Claude Code `PreToolUse` output that adds the reminder to the model's
/// context. It sets no `permissionDecision`, so normal permissions apply.
pub fn reminder_output() -> serde_json::Value {
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "additionalContext": BACKGROUND_REMINDER,
        }
    })
}

/// Hook command written into Claude Code project settings.
pub const HOOK_COMMAND: &str = "ilium progress guard || true";

/// Earlier command text that `merged_settings` rewrites to `HOOK_COMMAND`.
const LEGACY_HOOK_COMMAND: &str = "ilium progress guard";

/// Where Claude Code reads project hooks, relative to the project root.
pub const PROJECT_SETTINGS_PATH: &str = ".claude/settings.local.json";

/// Why a settings file cannot take the guard hook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    /// The file is not valid JSON.
    NotJson(String),
    /// The file, its `hooks` key or its `hooks.PreToolUse` key has the wrong JSON type.
    WrongShape(&'static str),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotJson(reason) => {
                write!(
                    formatter,
                    "could not merge {PROJECT_SETTINGS_PATH}: {reason}"
                )
            }
            Self::WrongShape(what) => write!(formatter, "{what}"),
        }
    }
}

/// The settings text that keeps one guard hook, or `None` when `existing` already has it.
/// Idempotent. A user-written wrapper that already runs `ilium progress guard` is kept as
/// written; other hooks and keys are preserved. The caller writes the returned text
/// atomically, so a crash never leaves a half-written settings file.
pub fn merged_settings(existing: &str) -> Result<Option<String>, InstallError> {
    let mut root: serde_json::Value = if existing.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(existing).map_err(|error| InstallError::NotJson(error.to_string()))?
    };
    let Some(root_object) = root.as_object_mut() else {
        return Err(InstallError::WrongShape(
            ".claude/settings.local.json must contain a JSON object",
        ));
    };
    let hooks = root_object
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    let Some(hooks) = hooks.as_object_mut() else {
        return Err(InstallError::WrongShape(
            ".claude/settings.local.json hooks must be a JSON object",
        ));
    };
    let groups = hooks
        .entry("PreToolUse")
        .or_insert_with(|| serde_json::json!([]));
    let Some(groups) = groups.as_array_mut() else {
        return Err(InstallError::WrongShape(
            ".claude/settings.local.json hooks.PreToolUse must be an array",
        ));
    };

    let mut changed = false;
    for command in groups.iter_mut().flat_map(hook_commands_mut) {
        if command.as_str() == Some(LEGACY_HOOK_COMMAND) {
            *command = serde_json::Value::String(HOOK_COMMAND.to_string());
            changed = true;
        }
    }
    let already_installed = groups.iter().any(|group| {
        group
            .get("hooks")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|hooks| {
                hooks.iter().any(|hook| {
                    hook.get("command")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|command| command.contains(LEGACY_HOOK_COMMAND))
                })
            })
    });
    if !already_installed {
        groups.push(serde_json::json!({
            "matcher": "Bash",
            "hooks": [{"type": "command", "command": HOOK_COMMAND, "timeout": 10}],
        }));
        changed = true;
    }
    if !changed {
        return Ok(None);
    }
    serde_json::to_string_pretty(&root)
        .map(Some)
        .map_err(|error| InstallError::NotJson(error.to_string()))
}

/// Mutable `command` values of one `PreToolUse` group's hooks.
fn hook_commands_mut(group: &mut serde_json::Value) -> Vec<&mut serde_json::Value> {
    group
        .get_mut("hooks")
        .and_then(serde_json::Value::as_array_mut)
        .map(|hooks| {
            hooks
                .iter_mut()
                .filter_map(|hook| hook.get_mut("command"))
                .collect()
        })
        .unwrap_or_default()
}

/// What the CLI prints for one hook payload read from stdin, or `None` to print nothing
/// (the exit status is always 0). Malformed input never blocks a tool call.
pub fn hook_output(raw_input: &[u8], inside_ilium_pane: bool) -> Option<String> {
    if !inside_ilium_pane {
        return None;
    }
    let input = serde_json::from_slice::<serde_json::Value>(raw_input).ok()?;
    match evaluate(&input, inside_ilium_pane) {
        GuardDecision::Allow => None,
        GuardDecision::Remind => Some(reminder_output().to_string()),
        GuardDecision::Refuse => Some(refusal_output().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        evaluate, hook_output, merged_settings, refusal_output, reminder_output, GuardDecision,
        InstallError, BACKGROUND_REMINDER, HOOK_COMMAND,
    };
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
            bash(
                json!({"command": "ilium progress set --command /p --wait", "run_in_background": true}),
            ),
            bash(json!({"command": "cargo test", "timeout": 180000})),
            bash(json!({"command": "ls"})),
            json!({"tool_name": "Edit", "tool_input": {"timeout": 600000}}),
            json!({}),
        ] {
            assert_eq!(evaluate(&input, true), GuardDecision::Allow, "{input}");
        }
    }

    #[test]
    fn background_command_in_a_pane_is_allowed_with_a_reminder() {
        let input =
            bash(json!({"command": "npm run dev", "timeout": 600000, "run_in_background": true}));
        assert_eq!(evaluate(&input, true), GuardDecision::Remind);
        assert_eq!(evaluate(&input, false), GuardDecision::Allow);
        let output = reminder_output();
        let specific = &output["hookSpecificOutput"];
        assert_eq!(specific["hookEventName"], "PreToolUse");
        assert_eq!(specific["additionalContext"], BACKGROUND_REMINDER);
        assert!(specific.get("permissionDecision").is_none());
        assert!(BACKGROUND_REMINDER.contains("ilium progress set --command <probe>"));
    }

    fn guard_commands(root: &serde_json::Value) -> Vec<String> {
        root["hooks"]["PreToolUse"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|group| group["hooks"].as_array().unwrap().clone())
            .filter_map(|hook| hook["command"].as_str().map(str::to_string))
            .filter(|command| command.contains("ilium progress guard"))
            .collect()
    }

    fn merged(existing: &str) -> serde_json::Value {
        let text = merged_settings(existing).unwrap().unwrap();
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn installer_writes_one_bash_guard_and_is_idempotent() {
        let first = merged_settings("").unwrap().unwrap();
        assert_eq!(merged_settings(&first).unwrap(), None);
        let root: serde_json::Value = serde_json::from_str(&first).unwrap();
        assert_eq!(guard_commands(&root), vec![HOOK_COMMAND.to_string()]);
        assert_eq!(root["hooks"]["PreToolUse"][0]["matcher"], "Bash");
    }

    #[test]
    fn installer_rewrites_the_legacy_command_and_keeps_other_hooks() {
        let root = merged(
            r#"{"model":"x","hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"ilium progress guard","timeout":10}]},{"matcher":"Edit","hooks":[{"type":"command","command":"other"}]}]}}"#,
        );
        assert_eq!(root["model"], "x");
        assert_eq!(guard_commands(&root), vec![HOOK_COMMAND.to_string()]);
        assert_eq!(
            root["hooks"]["PreToolUse"][1]["hooks"][0]["command"],
            "other"
        );
    }

    #[test]
    fn installer_keeps_a_user_written_guard_wrapper() {
        let wrapper = "ilium progress --help 2>/dev/null | grep -qE '^ +guard( |$)' || exit 0; exec ilium progress guard";
        let original = serde_json::json!({"hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": wrapper, "timeout": 10}]}]}});
        assert_eq!(merged_settings(&original.to_string()).unwrap(), None);
    }

    #[test]
    fn installer_rejects_a_settings_file_that_is_not_an_object() {
        assert!(matches!(
            merged_settings("[1]"),
            Err(InstallError::WrongShape(_))
        ));
        assert!(matches!(
            merged_settings("{not json"),
            Err(InstallError::NotJson(_))
        ));
    }

    #[test]
    fn hook_output_prints_only_for_a_refused_or_reminded_call_inside_a_pane() {
        let long = br#"{"tool_name":"Bash","tool_input":{"command":"make","timeout":600000}}"#;
        assert_eq!(hook_output(long, true), Some(refusal_output().to_string()));
        assert_eq!(hook_output(long, false), None);
        assert_eq!(hook_output(b"not json", true), None);
        let background =
            br#"{"tool_name":"Bash","tool_input":{"command":"x","run_in_background":true}}"#;
        assert_eq!(
            hook_output(background, true),
            Some(reminder_output().to_string())
        );
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
