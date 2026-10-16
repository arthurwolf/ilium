//! Twins of ilium-client/src/agent_prompt_transcript.rs: exact user prompt evidence read from a session transcript.
//! The transcript is given as text; `offset` is the byte offset the read starts at; the cutoff is the fixed
//! instant 2026-10-02T20:00:00Z of the original tests.

use serde_json::{json, Value};

use crate::common::{ask, text, Place};

const CUTOFF: &str = "2026-10-02T20:00:00Z";

fn exact_prompt(agent: &str, transcript: &str, offset: usize) -> Value {
    ask(
        Place::Server,
        "prompt-transcript.exact-user-prompt-after",
        json!({ "agent": agent, "transcript": transcript, "offset": offset, "cutoff": CUTOFF }),
    )
}

/// REQ: PORT:ilium-client/src/agent_prompt_transcript.rs::claude_user_string_preserves_multiline_and_ignores_tool_content
#[test]
fn port_agent_prompt_transcript_claude_user_string_preserves_multiline_and_ignores_tool_content() {
    let user = json!({"timestamp":"2026-10-02T20:00:01Z","type":"user","message":{"content":"line 1\nline 2  "}});
    let tool = json!({"timestamp":"2026-10-02T20:00:02Z","type":"user","message":{"content":[{"type":"tool_result","content":"wrong"}]}});
    let reply = exact_prompt("claude", &format!("{user}\n{tool}\n"), 0);
    assert_eq!(reply["prompt"], json!("line 1\nline 2  "));
}

/// REQ: PORT:ilium-client/src/agent_prompt_transcript.rs::delayed_prior_turn_after_offset_is_rejected_by_timestamp
#[test]
fn port_agent_prompt_transcript_delayed_prior_turn_after_offset_is_rejected_by_timestamp() {
    let old = json!({"timestamp":"2026-10-02T19:59:59Z","type":"event_msg","payload":{"type":"user_message","message":"stale"}});
    let reply = exact_prompt("codex", &format!("{old}\n"), 0);
    assert_eq!(reply["prompt"], Value::Null);
    assert_eq!(reply["error"], Value::Null);
}

/// REQ: PORT:ilium-client/src/agent_prompt_transcript.rs::incomplete_suffix_is_not_a_submission
#[test]
fn port_agent_prompt_transcript_incomplete_suffix_is_not_a_submission() {
    let partial = r#"{"type":"event_msg","payload":{"type":"user_message","message":"partial"}}"#;
    let reply = exact_prompt("codex", partial, 0);
    assert_eq!(reply["prompt"], Value::Null);
    assert_eq!(reply["error"], Value::Null);
}

/// REQ: PORT:ilium-client/src/agent_prompt_transcript.rs::newer_event_user_supersedes_older_response_user
#[test]
fn port_agent_prompt_transcript_newer_event_user_supersedes_older_response_user() {
    let older = json!({"timestamp":"2026-10-02T20:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"older"}]}});
    let newer = json!({"timestamp":"2026-10-02T20:00:02Z","type":"event_msg","payload":{"type":"user_message","message":"newer  "}});
    let reply = exact_prompt("codex", &format!("{older}\n{newer}\n"), 0);
    assert_eq!(reply["prompt"], json!("newer  "));
}

/// REQ: PORT:ilium-client/src/agent_prompt_transcript.rs::oversized_suffix_record_is_an_error_not_partial_exact_evidence
#[test]
fn port_agent_prompt_transcript_oversized_suffix_record_is_an_error_not_partial_exact_evidence() {
    let oversized = " ".repeat(1024 * 1024 + 1);
    let reply = exact_prompt("codex", &oversized, 0);
    assert!(text(&reply["error"]).contains("bounded evidence limits"), "reply: {reply}");
}

/// REQ: PORT:ilium-client/src/agent_prompt_transcript.rs::repeated_identical_codex_prompt_is_new_evidence_and_keeps_trailing_text
#[test]
fn port_agent_prompt_transcript_repeated_identical_codex_prompt_is_new_evidence_and_keeps_trailing_text() {
    let old = json!({"timestamp":"2026-10-02T20:00:01Z","type":"event_msg","payload":{"type":"user_message","message":"same  "}});
    let first = format!("{old}\n");
    let baseline = first.len();
    let reply = exact_prompt("codex", &format!("{first}{old}\n"), baseline);
    assert_eq!(reply["prompt"], json!("same  "));
}
