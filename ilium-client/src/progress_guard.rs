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
use std::path::{Path, PathBuf};

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

/// Hook command written into Claude Code project settings.
pub const HOOK_COMMAND: &str = "ilium progress guard || true";

/// Earlier command text that `ensure_installed` rewrites to `HOOK_COMMAND`.
const LEGACY_HOOK_COMMAND: &str = "ilium progress guard";

/// Claude Code reads project hooks from `.claude/settings.local.json`.
pub fn project_settings_path(project_root: &Path) -> PathBuf {
    project_root.join(".claude").join("settings.local.json")
}

/// Keeps one guard hook in a project's Claude settings. Idempotent. A
/// user-written wrapper that already runs `ilium progress guard` is kept as
/// written; other hooks and keys are preserved; the file is replaced
/// atomically so a crash never leaves a half-written settings file.
pub fn ensure_installed(project_root: &Path) -> anyhow::Result<()> {
    let path = project_settings_path(project_root);
    let existing = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let mut root: serde_json::Value = if existing.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(&existing)
            .map_err(|error| anyhow::anyhow!("could not merge {}: {error}", path.display()))?
    };
    let Some(root_object) = root.as_object_mut() else {
        anyhow::bail!("{} must contain a JSON object", path.display());
    };
    let hooks = root_object
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    let Some(hooks) = hooks.as_object_mut() else {
        anyhow::bail!("{}.hooks must be a JSON object", path.display());
    };
    let groups = hooks
        .entry("PreToolUse")
        .or_insert_with(|| serde_json::json!([]));
    let Some(groups) = groups.as_array_mut() else {
        anyhow::bail!("{}.hooks.PreToolUse must be an array", path.display());
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
        return Ok(());
    }
    write_atomically(&path, &serde_json::to_string_pretty(&root)?)
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

fn write_atomically(path: &Path, contents: &str) -> anyhow::Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("{} has no parent directory", path.display()))?;
    std::fs::create_dir_all(directory)?;
    let temporary = path.with_extension("json.ilium-tmp");
    std::fs::write(&temporary, contents)?;
    std::fs::rename(&temporary, path)?;
    Ok(())
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
    use super::{
        ensure_installed, evaluate, project_settings_path, refusal_output, GuardDecision,
        HOOK_COMMAND,
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
            bash(json!({"command": "npm run dev", "timeout": 600000, "run_in_background": true})),
            bash(json!({"command": "cargo test", "timeout": 180000})),
            bash(json!({"command": "ls"})),
            json!({"tool_name": "Edit", "tool_input": {"timeout": 600000}}),
            json!({}),
        ] {
            assert_eq!(evaluate(&input, true), GuardDecision::Allow, "{input}");
        }
    }

    fn settings(directory: &std::path::Path) -> serde_json::Value {
        let contents = std::fs::read_to_string(super::project_settings_path(directory)).unwrap();
        serde_json::from_str(&contents).unwrap()
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

    #[test]
    fn installer_writes_one_bash_guard_and_is_idempotent() {
        let directory = tempfile::tempdir().unwrap();
        ensure_installed(directory.path()).unwrap();
        ensure_installed(directory.path()).unwrap();
        let root = settings(directory.path());
        assert_eq!(guard_commands(&root), vec![HOOK_COMMAND.to_string()]);
        assert_eq!(root["hooks"]["PreToolUse"][0]["matcher"], "Bash");
    }

    #[test]
    fn installer_rewrites_the_legacy_command_and_keeps_other_hooks() {
        let directory = tempfile::tempdir().unwrap();
        let path = project_settings_path(directory.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"model":"x","hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"ilium progress guard","timeout":10}]},{"matcher":"Edit","hooks":[{"type":"command","command":"other"}]}]}}"#,
        )
        .unwrap();
        ensure_installed(directory.path()).unwrap();
        let root = settings(directory.path());
        assert_eq!(root["model"], "x");
        assert_eq!(guard_commands(&root), vec![HOOK_COMMAND.to_string()]);
        assert_eq!(
            root["hooks"]["PreToolUse"][1]["hooks"][0]["command"],
            "other"
        );
    }

    #[test]
    fn installer_keeps_a_user_written_guard_wrapper() {
        let directory = tempfile::tempdir().unwrap();
        let path = project_settings_path(directory.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let wrapper = "ilium progress --help 2>/dev/null | grep -qE '^ +guard( |$)' || exit 0; exec ilium progress guard";
        let original = serde_json::json!({"hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": wrapper, "timeout": 10}]}]}});
        std::fs::write(&path, original.to_string()).unwrap();
        ensure_installed(directory.path()).unwrap();
        assert_eq!(settings(directory.path()), original);
    }

    #[test]
    fn installer_rejects_a_settings_file_that_is_not_an_object() {
        let directory = tempfile::tempdir().unwrap();
        let path = project_settings_path(directory.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "[1]").unwrap();
        assert!(ensure_installed(directory.path()).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[1]");
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
