//! Pure edits of the project files that `ilium chat init` maintains next to `CHATROOM.md`: the
//! Codex and Claude lifecycle hook files. Moved from `ilium-client/src/chatroom.rs`
//! (`ensure_hook_file`, `is_ilium_hook_group`, `upgrade_ilium_hook_command`).
//!
//! Every function takes the current text of the file and returns the new text, so the host file
//! calls stay in the caller and these rules run without a file.

use serde_json::{json, Map, Value};

use crate::messages::{HOOK_COMMAND, HOOK_COMMAND_PREFIX};

/// Project file of the Codex hooks, relative to the project root.
pub const CODEX_HOOKS_PATH: &str = ".codex/hooks.json";
/// Project file of the Claude Code hooks, relative to the project root.
pub const CLAUDE_HOOKS_PATH: &str = ".claude/settings.local.json";
/// The lifecycle events whose hook injects the unread room records.
pub const HOOK_EVENTS: [&str; 2] = ["SessionStart", "UserPromptSubmit"];

/// Merges the chatroom hook into the JSON text of a hook file for each of `events`.
///
/// Returns the new text when anything changed (an older chatroom command is upgraded in place, a
/// missing group is added, other hooks are kept), `None` when the file already holds the current
/// hook. An empty text counts as `{}`. Text that is not a JSON object is refused with a message,
/// so a broken file is never overwritten.
pub fn merge_hook_file(
    existing: &str,
    path_label: &str,
    events: &[&str],
) -> Result<Option<String>, String> {
    let source = if existing.trim().is_empty() {
        "{}"
    } else {
        existing
    };
    let mut root: Value = serde_json::from_str(source)
        .map_err(|error| format!("could not merge {path_label}: {error}"))?;
    let root_object = root
        .as_object_mut()
        .ok_or_else(|| format!("{path_label} must contain a JSON object"))?;
    let mut changed = false;
    let hooks = root_object
        .entry("hooks".to_string())
        .or_insert_with(|| {
            changed = true;
            Value::Object(Map::new())
        })
        .as_object_mut()
        .ok_or_else(|| format!("{path_label}.hooks must be a JSON object"))?;
    for event in events {
        let groups = hooks
            .entry((*event).to_string())
            .or_insert_with(|| {
                changed = true;
                Value::Array(Vec::new())
            })
            .as_array_mut()
            .ok_or_else(|| format!("{path_label}.hooks.{event} must be an array"))?;
        for group in groups.iter_mut() {
            changed |= upgrade_hook_command(group);
        }
        if !groups.iter().any(is_chatroom_hook_group) {
            groups.push(json!({
                "hooks": [{
                    "type": "command",
                    "command": HOOK_COMMAND,
                    "timeout": 10,
                    "statusMessage": "Checking ilium chatroom"
                }]
            }));
            changed = true;
        }
    }
    if !changed {
        return Ok(None);
    }
    let text = serde_json::to_string_pretty(&root).map_err(|error| error.to_string())?;
    Ok(Some(format!("{text}\n")))
}

/// True when one hook group already runs the chatroom context command.
fn is_chatroom_hook_group(group: &Value) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hooks| {
            hooks.iter().any(|hook| {
                hook.get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|command| command.starts_with(HOOK_COMMAND_PREFIX))
            })
        })
}

/// Rewrites an older chatroom hook command of one group to the current one; true when it changed.
fn upgrade_hook_command(group: &mut Value) -> bool {
    let Some(hooks) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut changed = false;
    for hook in hooks {
        let outdated = hook
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|command| {
                command.starts_with(HOOK_COMMAND_PREFIX) && command != HOOK_COMMAND
            });
        if outdated {
            hook["command"] = Value::String(HOOK_COMMAND.to_string());
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_file_gets_both_events_and_a_second_merge_changes_nothing() {
        let first = merge_hook_file("", "hooks.json", &HOOK_EVENTS)
            .unwrap()
            .expect("a new file is written");
        assert!(first.contains("SessionStart") && first.contains("UserPromptSubmit"));
        assert!(first.contains(HOOK_COMMAND));
        assert_eq!(
            merge_hook_file(&first, "hooks.json", &HOOK_EVENTS),
            Ok(None)
        );
    }

    #[test]
    fn an_older_command_is_upgraded_and_foreign_hooks_stay() {
        let existing = r#"{"model":"x","hooks":{"SessionStart":[
            {"hooks":[{"type":"command","command":"ilium chat context --limit 5"}]},
            {"hooks":[{"type":"command","command":"echo other"}]}
        ],"UserPromptSubmit":[]}}"#;
        let merged = merge_hook_file(existing, "settings.json", &HOOK_EVENTS)
            .unwrap()
            .expect("the older command changes");
        assert!(merged.contains("echo other"));
        assert!(merged.contains("\"model\""));
        assert!(!merged.contains("--limit 5"));
    }

    #[test]
    fn a_file_that_is_not_an_object_is_refused() {
        assert!(merge_hook_file("[1]", "hooks.json", &HOOK_EVENTS).is_err());
        assert!(merge_hook_file("{", "hooks.json", &HOOK_EVENTS).is_err());
        assert!(merge_hook_file(r#"{"hooks":[]}"#, "hooks.json", &HOOK_EVENTS).is_err());
    }
}
