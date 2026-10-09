//! Shared evidence for a user request in a project-verified agent transcript.
//! Metadata, assistant output, tools, and injected notifications never confer
//! title eligibility. An unreadable or incomplete history is indeterminate,
//! never proof that a conversation was empty.

use std::io::{self, BufReader};
use std::path::Path;

use ilium_core::AgentClass;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenuineRequestEvidence {
    /// No verified transcript or a malformed/incomplete record. Preserve any
    /// existing title and decline a new AI title until evidence is available.
    Unavailable,
    /// A verified, wholly readable history contains no genuine user request.
    VerifiedEmpty,
    /// End byte offset of the last genuine request record. Output appended
    /// after this record does not invalidate a title, but a newer request does.
    Present { last_record_end: u64 },
}

/// Reads complete parseable JSONL records. An invalid trailing row cannot
/// prove emptiness; a completed earlier genuine request remains positive. The caller must first
/// verify the path, session ID, and cwd through TranscriptLocator.
pub(crate) fn request_evidence_from_path(
    class: &AgentClass,
    transcript_path: &Path,
    session_id: &str,
) -> io::Result<GenuineRequestEvidence> {
    let budget = super::TranscriptReadBudget::new(super::TranscriptReadLimits {
        line_bytes: 1024 * 1024,
        total_read_bytes: 16 * 1024 * 1024,
        scanned_entries: usize::MAX,
        retained_path_bytes: usize::MAX,
    });
    request_evidence_from_path_with_budget(class, transcript_path, session_id, &budget)
}

pub(crate) fn request_evidence_from_path_with_budget(
    class: &AgentClass,
    transcript_path: &Path,
    session_id: &str,
    budget: &super::TranscriptReadBudget,
) -> io::Result<GenuineRequestEvidence> {
    if matches!(class, AgentClass::Other(_)) {
        return Ok(GenuineRequestEvidence::Unavailable);
    }
    let source_path = if matches!(class, AgentClass::Antigravity) {
        let Some(root) = transcript_path.parent().and_then(Path::parent) else {
            return Ok(GenuineRequestEvidence::Unavailable);
        };
        root.join("history.jsonl")
    } else {
        transcript_path.to_path_buf()
    };
    let file = ilium_platform::secure_fs::open_regular_file(&source_path)?;
    let mut reader = BufReader::new(file);
    let mut end_offset = 0_u64;
    let mut latest_request = None;
    loop {
        let record = match super::bounded_line(&mut reader, budget) {
            Ok(Some(record)) => record,
            Ok(None) => break,
            Err(_) if budget.exhausted.load(std::sync::atomic::Ordering::Acquire) => {
                return Ok(GenuineRequestEvidence::Unavailable);
            }
            Err(error) => return Err(error),
        };
        end_offset = end_offset.saturating_add(record.len() as u64);
        // An append in progress can neither revoke earlier positive proof
        // nor establish that an otherwise empty conversation has no task.
        let Ok(entry) = serde_json::from_slice::<Value>(&record) else {
            return Ok(latest_request
                .map_or(GenuineRequestEvidence::Unavailable, |last_record_end| {
                    GenuineRequestEvidence::Present { last_record_end }
                }));
        };
        if matches!(class, AgentClass::Antigravity)
            && entry.get("conversationId").and_then(Value::as_str) != Some(session_id)
        {
            continue;
        }
        if matches!(class, AgentClass::Claude)
            && entry
                .get("sessionId")
                .and_then(Value::as_str)
                .is_some_and(|id| id != session_id)
        {
            return Ok(GenuineRequestEvidence::Unavailable);
        }
        if genuine_request_text(class, &entry).is_some() {
            latest_request = Some(end_offset);
        }
        if !record.ends_with(b"\n") {
            return Ok(latest_request
                .map_or(GenuineRequestEvidence::Unavailable, |last_record_end| {
                    GenuineRequestEvidence::Present { last_record_end }
                }));
        }
    }
    Ok(match latest_request {
        Some(last_record_end) => GenuineRequestEvidence::Present { last_record_end },
        None => GenuineRequestEvidence::VerifiedEmpty,
    })
}

/// Returns verbatim authored text for supported user records. Trimming is
/// used only to reject blank or injected records; returned whitespace remains
/// intact for the exact-prompt recovery path.
pub fn genuine_request_text(class: &AgentClass, entry: &Value) -> Option<String> {
    match class {
        AgentClass::Claude => {
            if entry.get("type")?.as_str()? != "user"
                || entry.get("isSidechain").and_then(Value::as_bool) == Some(true)
                || entry.get("isMeta").and_then(Value::as_bool) == Some(true)
                || entry.get("isCompactSummary").and_then(Value::as_bool) == Some(true)
                || entry.get("promptSource").and_then(Value::as_str) == Some("system")
            {
                return None;
            }
            let origin = entry
                .get("origin")
                .and_then(|value| value.get("kind"))
                .and_then(Value::as_str);
            if !matches!(origin, None | Some("human")) {
                return None;
            }
            let text = entry.get("message")?.get("content")?.as_str()?;
            (!text.trim().is_empty()
                && !is_claude_injected_message(text)
                && !is_non_task_control_text(text))
            .then(|| text.to_owned())
        }
        AgentClass::Codex => {
            let payload = entry.get("payload")?;
            let text = match entry.get("type")?.as_str()? {
                "event_msg" if payload.get("type")?.as_str()? == "user_message" => {
                    payload.get("message")?.as_str()?.to_owned()
                }
                "response_item"
                    if payload.get("type")?.as_str()? == "message"
                        && payload.get("role")?.as_str()? == "user" =>
                {
                    codex_user_content(payload.get("content")?)?
                }
                _ => return None,
            };
            (!text.trim().is_empty()
                && !is_codex_injected_message(&text)
                && !is_non_task_control_text(&text))
            .then_some(text)
        }
        AgentClass::Antigravity => entry
            .get("display")
            .and_then(Value::as_str)
            .filter(|text| {
                !text.trim().is_empty()
                    && !is_claude_injected_message(text)
                    && !is_non_task_control_text(text)
            })
            .map(str::to_owned),
        AgentClass::Other(_) => None,
    }
}

fn codex_user_content(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => Some(text.to_owned()),
        Value::Array(blocks) => {
            let text = blocks
                .iter()
                .filter(|block| {
                    matches!(
                        block.get("type").and_then(Value::as_str),
                        Some("input_text" | "text")
                    )
                })
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .filter(|text| !is_codex_injected_message(text))
                .collect::<Vec<_>>()
                .join("\n");
            (!text.trim().is_empty()).then_some(text)
        }
        _ => None,
    }
}

fn is_claude_injected_message(message: &str) -> bool {
    let text = message.trim_start();
    is_codex_injected_message(message)
        || text.starts_with("<local-command-")
        || text.starts_with("<system-reminder>")
        || text.starts_with("<task-notification>")
        || text.starts_with(
            "Caveat: The messages below were generated by the user while running local commands",
        )
}

/// Provider histories sometimes serialize slash bookkeeping or Ilium's own
/// update request as user-shaped text without recording its IPC source. Such
/// text cannot establish that the human asked the agent to do substantive
/// work. A `/goal` with an objective is an authored task, however.
fn is_non_task_control_text(message: &str) -> bool {
    let text = message.trim();
    let update_template = ilium_prompts::agent::ASK_FOR_UPDATE;
    let update_prefix = update_template
        .split("{{#if")
        .next()
        .unwrap_or(update_template);
    if !update_prefix.is_empty() && text.starts_with(update_prefix) {
        return true;
    }

    if let Some(rest) = text.strip_prefix("<command-name>") {
        let Some((command, tail)) = rest.split_once("</command-name>") else {
            return true;
        };
        if command != "/goal" {
            return true;
        }
        let Some(arguments) = tail
            .split("<command-args>")
            .nth(1)
            .and_then(|value| value.split_once("</command-args>"))
            .map(|(value, _)| value.trim())
        else {
            return true;
        };
        return is_goal_bookkeeping(arguments);
    }

    let mut words = text.split_whitespace();
    let Some(command) = words.next() else {
        return true;
    };
    if !command.starts_with('/') {
        return false;
    }
    if command == "/goal" {
        return is_goal_bookkeeping(words.next().unwrap_or_default());
    }
    // Ordinary slash commands are local/provider controls. Do not infer a
    // task from a bare command or one with command arguments such as /resume.
    true
}

fn is_goal_bookkeeping(first_argument: &str) -> bool {
    first_argument.is_empty()
        || matches!(
            first_argument,
            "clear" | "pause" | "resume" | "status" | "stop"
        )
}

/// Shared with the client's display parser and exact-prompt recovery. Codex
/// places these harness messages in user-shaped records despite no authored
/// request, so neither inference nor a legacy reset may treat them as one.
pub fn is_codex_injected_message(message: &str) -> bool {
    let text = message.trim_start();
    text.starts_with(ilium_prompts::naming::NAMING_TRANSCRIPT_CONTEXT_AGENTS_MD_INSTRUCTIONS_FOR)
        || text.starts_with("<environment_context>")
        || text.starts_with(ilium_prompts::naming::NAMING_TRANSCRIPT_CONTEXT_CODEX_INTERNAL_CONTEXT)
        || text.starts_with("<task-notification>")
        || text.starts_with(ilium_prompts::naming::NAMING_TRANSCRIPT_CONTEXT_ILIUM_PROGRESS_MONITOR)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TranscriptLocator;

    #[test]
    fn harness_notifications_are_not_tasks_for_any_supported_provider() {
        for text in [
            "Ilium progress monitor 1 reports completion",
            "# AGENTS.md instructions for /tmp/project",
            "<environment_context>fixture</environment_context>",
        ] {
            let claude = serde_json::json!({"type":"user","message":{"content":text}});
            let antigravity = serde_json::json!({"display":text});
            assert_eq!(genuine_request_text(&AgentClass::Claude, &claude), None);
            assert_eq!(
                genuine_request_text(&AgentClass::Antigravity, &antigravity),
                None
            );
        }
    }

    #[test]
    fn codex_metadata_goal_and_monitor_are_not_requests() {
        for entry in [
            serde_json::json!({"type":"session_meta","payload":{"id":"session"}}),
            serde_json::json!({"type":"event_msg","payload":{"type":"thread_goal_updated","goal":{"objective":"automatic"}}}),
            serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":"Ilium progress monitor 42 reports completion"}}),
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<task-notification>done</task-notification>"}]}}),
        ] {
            assert_eq!(genuine_request_text(&AgentClass::Codex, &entry), None);
        }
        let authored = serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"fix login  "}]}});
        assert_eq!(
            genuine_request_text(&AgentClass::Codex, &authored).as_deref(),
            Some("fix login  ")
        );
    }

    #[test]
    fn claude_notification_and_tool_result_are_not_requests() {
        let notification = serde_json::json!({"type":"user","origin":{"kind":"task-notification"},"message":{"content":"job done"}});
        let tool = serde_json::json!({"type":"user","message":{"content":[{"type":"tool_result","content":"done"}]}});
        let authored = serde_json::json!({"type":"user","origin":{"kind":"human"},"message":{"content":"fix login  "}});
        assert_eq!(
            genuine_request_text(&AgentClass::Claude, &notification),
            None
        );
        assert_eq!(genuine_request_text(&AgentClass::Claude, &tool), None);
        assert_eq!(
            genuine_request_text(&AgentClass::Claude, &authored).as_deref(),
            Some("fix login  ")
        );
    }

    #[test]
    fn controls_and_update_request_do_not_count_but_goal_objective_does() {
        let codex = |message: &str| serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":message}});
        let claude =
            |content: &str| serde_json::json!({"type":"user","message":{"content":content}});
        for text in [
            "/clear",
            "/resume previous-session",
            "/goal clear",
            "/goal",
            ilium_prompts::agent::ASK_FOR_UPDATE,
        ] {
            assert_eq!(genuine_request_text(&AgentClass::Codex, &codex(text)), None);
            assert_eq!(
                genuine_request_text(&AgentClass::Claude, &claude(text)),
                None
            );
        }
        assert_eq!(
            genuine_request_text(&AgentClass::Codex, &codex("/goal repair naming")),
            Some("/goal repair naming".to_owned())
        );
        assert!(
            genuine_request_text(
                &AgentClass::Claude,
                &claude("<command-name>/goal</command-name>\n <command-message>goal</command-message>\n <command-args>repair naming</command-args>"),
            )
            .is_some()
        );
        assert_eq!(
            genuine_request_text(
                &AgentClass::Claude,
                &claude("<command-name>/compact</command-name>"),
            ),
            None
        );
    }

    #[test]
    fn verified_empty_differs_from_missing_and_authored_history() {
        let home = tempfile::tempdir().unwrap();
        let cwd = home.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        let id = "22222222-2222-4222-8222-222222222222";
        let locator = TranscriptLocator::new(home.path(), &cwd);
        assert_eq!(
            locator
                .genuine_request_evidence(&AgentClass::Codex, id)
                .unwrap(),
            GenuineRequestEvidence::Unavailable
        );
        let directory = home.path().join(".codex/sessions/2026/10/03");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("rollout-2026-10-03T00-00-00-{id}.jsonl"));
        let metadata = serde_json::json!({"type":"session_meta","payload":{"id":id,"cwd":cwd}});
        let monitor = serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":"Ilium progress monitor 42 reports completion"}});
        std::fs::write(&path, format!("{metadata}\n{monitor}\n")).unwrap();
        assert_eq!(
            locator
                .genuine_request_evidence(&AgentClass::Codex, id)
                .unwrap(),
            GenuineRequestEvidence::VerifiedEmpty
        );
        let authored = serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":"fix login"}});
        std::fs::write(&path, format!("{metadata}\n{monitor}\n{authored}\n")).unwrap();
        assert!(matches!(
            locator
                .genuine_request_evidence(&AgentClass::Codex, id)
                .unwrap(),
            GenuineRequestEvidence::Present { .. }
        ));
        std::fs::write(&path, format!("{metadata}\n{{bad json\n")).unwrap();
        assert_eq!(
            locator
                .genuine_request_evidence(&AgentClass::Codex, id)
                .unwrap(),
            GenuineRequestEvidence::Unavailable
        );
    }
}

#[cfg(test)]
mod partial_history_tests {
    use super::*;
    #[test]
    fn complete_authored_request_survives_an_incomplete_later_append() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.jsonl");
        std::fs::write(&path, "{\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"fix naming\"}}\n{\"type\":").unwrap();
        assert!(matches!(
            request_evidence_from_path(&AgentClass::Codex, &path, "session").unwrap(),
            GenuineRequestEvidence::Present { .. }
        ));
        std::fs::write(&path, "{\"type\":\"session_meta\",\"payload\":{}}").unwrap();
        assert_eq!(
            request_evidence_from_path(&AgentClass::Codex, &path, "session").unwrap(),
            GenuineRequestEvidence::Unavailable
        );
    }
}
