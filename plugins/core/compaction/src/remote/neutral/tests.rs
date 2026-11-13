use std::path::{Path, PathBuf};

use serde_json::json;

use super::*;

fn fixture(name: &str) -> Vec<u8> {
    match name {
        "claude_session.jsonl" => {
            include_bytes!("../../../tests/fixtures/remote-compaction/claude_session.jsonl")
                .to_vec()
        }
        "codex_rollout.jsonl" => {
            include_bytes!("../../../tests/fixtures/remote-compaction/codex_rollout.jsonl").to_vec()
        }
        other => panic!("unknown fixture {other}"),
    }
}

fn roles(turns: &[Turn]) -> Vec<Role> {
    turns.iter().map(|turn| turn.role).collect()
}

fn tool_call_ids(turn: &Turn) -> Vec<&str> {
    turn.parts
        .iter()
        .filter_map(|part| match part {
            Part::ToolCall { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect()
}

/// REQ: PORT:ilium-remote-compaction/src/neutral/tests.rs::claude_chain_stops_at_the_last_boundary_and_keeps_its_summary_as_anchor
#[test]
fn claude_chain_stops_at_the_last_boundary_and_keeps_its_summary_as_anchor() {
    let parsed = parse_transcript(
        AgentKind::Claude,
        &fixture("claude_session.jsonl"),
        Path::new("claude_session.jsonl"),
    )
    .expect("parses");
    assert_eq!(
        roles(&parsed.turns),
        [
            Role::EarlierSummary,
            Role::User,
            Role::Assistant,
            Role::ToolResults,
            Role::Assistant,
            Role::ToolResults,
            Role::Assistant,
            Role::User,
            Role::Assistant,
        ]
    );
    let anchor = parsed.turns[0].text();
    assert!(anchor.starts_with("1. Primary Request and Intent: the user is building a small CLI"));
    assert!(
        anchor.ends_with("8. Current Work: repo bootstrap."),
        "{anchor}"
    );
    assert!(!anchor.contains("If you need specific details"));
    assert!(parsed
        .turns
        .iter()
        .all(|turn| !turn.text().contains("old: set up")));
}

/// REQ: PORT:ilium-remote-compaction/src/neutral/tests.rs::claude_noise_is_dropped_and_message_blocks_are_merged
#[test]
fn claude_noise_is_dropped_and_message_blocks_are_merged() {
    let parsed = parse_transcript(
        AgentKind::Claude,
        &fixture("claude_session.jsonl"),
        Path::new("claude_session.jsonl"),
    )
    .expect("parses");
    let all_text: String = parsed
        .turns
        .iter()
        .map(Turn::text)
        .collect::<Vec<_>>()
        .join("\n");
    for noise in [
        "Plan mode",
        "sidechain noise",
        "private reasoning",
        "hook_success",
    ] {
        assert!(
            !all_text.contains(noise),
            "{noise} leaked into the neutral model"
        );
    }
    // Three records of one API message become one assistant turn.
    let first_assistant = &parsed.turns[2];
    assert_eq!(first_assistant.message_id.as_deref(), Some("msg_1"));
    assert_eq!(first_assistant.uuids.len(), 3);
    assert_eq!(
        tool_call_ids(first_assistant),
        ["toolu_read1", "toolu_bash1"]
    );
    // Two result records become one results turn, with the error flag kept.
    let results = &parsed.turns[3];
    assert_eq!(results.parts.len(), 2);
    assert!(
        matches!(&results.parts[1], Part::ToolResult { is_error: true, call_id, .. } if call_id == "toolu_bash1")
    );
    // The last assistant turn is the orphan call.
    assert_eq!(tool_call_ids(&parsed.turns[8]), ["toolu_orphan"]);
}

/// REQ: PORT:ilium-remote-compaction/src/neutral/tests.rs::claude_head_carries_the_template_leaf_and_model
#[test]
fn claude_head_carries_the_template_leaf_and_model() {
    let parsed = parse_transcript(
        AgentKind::Claude,
        &fixture("claude_session.jsonl"),
        Path::new("claude_session.jsonl"),
    )
    .expect("parses");
    let head = parsed.claude.expect("claude head");
    assert_eq!(head.leaf_uuid, "00000000-0000-4000-8000-000000000017");
    assert_eq!(
        head.session_id.as_deref(),
        Some("11111111-1111-4111-8111-111111111111")
    );
    assert_eq!(head.model.as_deref(), Some("claude-sonnet-4-5"));
    assert_eq!(head.template.get("cwd"), Some(&json!("/repo")));
    assert_eq!(head.template.get("version"), Some(&json!("2.1.289")));
    assert!(parsed.shape.dropped_torn_fragment);
    assert!(parsed
        .warnings
        .iter()
        .any(|warning| warning.contains("torn")));
}

/// REQ: PORT:ilium-remote-compaction/src/neutral/tests.rs::claude_without_a_boundary_reads_the_whole_chain_and_cuts_missing_parents
#[test]
fn claude_without_a_boundary_reads_the_whole_chain_and_cuts_missing_parents() {
    let path = PathBuf::from("s.jsonl");
    let lines = [
        json!({"type":"user","uuid":"a","parentUuid":"missing","sessionId":"s","message":{"content":"first"}}),
        json!({"type":"assistant","uuid":"b","parentUuid":"a","sessionId":"s","message":{"id":"m","content":[{"type":"text","text":"second"}]}}),
        json!({"type":"user","uuid":"c","parentUuid":"b","sessionId":"s","message":{"content":"third"}}),
    ];
    let bytes: Vec<u8> = {
        lines
            .iter()
            .map(|line| format!("{line}\n"))
            .collect::<String>()
    }
    .into_bytes();
    let parsed = parse_transcript(AgentKind::Claude, &bytes, &path).expect("parses");
    assert_eq!(
        roles(&parsed.turns),
        [Role::User, Role::Assistant, Role::User]
    );
    assert!(parsed
        .warnings
        .iter()
        .any(|warning| warning.contains("pruned")));
}

/// REQ: PORT:ilium-remote-compaction/src/neutral/tests.rs::claude_preserved_segment_is_relinked_after_the_boundary
#[test]
fn claude_preserved_segment_is_relinked_after_the_boundary() {
    let path = PathBuf::from("s.jsonl");
    // Real Claude Code compaction keeps a recent tail physically before the
    // boundary and records it in `preservedSegment`.
    let lines = [
        json!({"type":"user","uuid":"u1","parentUuid":null,"sessionId":"s","message":{"content":"old request"}}),
        json!({"type":"user","uuid":"u2","parentUuid":"u1","sessionId":"s","message":{"content":"kept request"}}),
        json!({"type":"assistant","uuid":"a2","parentUuid":"u2","sessionId":"s","message":{"id":"m","content":[{"type":"text","text":"kept reply"}]}}),
        json!({"type":"system","subtype":"compact_boundary","uuid":"b","parentUuid":null,"logicalParentUuid":"a2","sessionId":"s","compactMetadata":{"trigger":"auto","preservedSegment":{"headUuid":"u2","anchorUuid":"s1","tailUuid":"a2"}}}),
        json!({"type":"user","uuid":"s1","parentUuid":"b","sessionId":"s","isCompactSummary":true,"message":{"content":"the summary"}}),
        json!({"type":"user","uuid":"u3","parentUuid":"s1","sessionId":"s","message":{"content":"after compaction"}}),
    ];
    let bytes: Vec<u8> = {
        lines
            .iter()
            .map(|line| format!("{line}\n"))
            .collect::<String>()
    }
    .into_bytes();
    let parsed = parse_transcript(AgentKind::Claude, &bytes, &path).expect("parses");
    let texts: Vec<String> = parsed.turns.iter().map(Turn::text).collect();
    assert_eq!(
        texts,
        [
            "the summary",
            "kept request",
            "kept reply",
            "after compaction"
        ]
    );
    assert!(!parsed
        .warnings
        .iter()
        .any(|warning| warning.contains("could not be relinked")));
}

/// REQ: PORT:ilium-remote-compaction/src/neutral/tests.rs::claude_transcript_without_chained_records_is_an_error
#[test]
fn claude_transcript_without_chained_records_is_an_error() {
    let path = PathBuf::from("s.jsonl");
    let bytes = "{\"type\":\"last-prompt\"}\n".as_bytes().to_vec();
    let error = parse_transcript(AgentKind::Claude, &bytes, &path)
        .err()
        .expect("error");
    assert!(matches!(error, CompactionError::NoAnchorRecord { .. }));
}

/// REQ: PORT:ilium-remote-compaction/src/neutral/tests.rs::codex_compacted_record_resets_history_and_encrypted_items_become_an_opaque_anchor
#[test]
fn codex_compacted_record_resets_history_and_encrypted_items_become_an_opaque_anchor() {
    let parsed = parse_transcript(
        AgentKind::Codex,
        &fixture("codex_rollout.jsonl"),
        Path::new("codex_rollout.jsonl"),
    )
    .expect("parses");
    assert_eq!(
        roles(&parsed.turns),
        [
            Role::User,
            Role::EarlierSummary,
            Role::User,
            Role::Assistant,
            Role::ToolResults,
            Role::Assistant,
            Role::ToolResults,
            Role::Assistant,
            Role::ToolResults,
            Role::Assistant,
            Role::User,
            Role::Assistant,
        ]
    );
    assert_eq!(
        parsed.turns[0].text(),
        "old request: bootstrap the fetch client"
    );
    assert!(parsed.turns[1]
        .text()
        .contains("encrypted summary that cannot be read"));
    assert!(parsed
        .warnings
        .iter()
        .any(|warning| warning.contains("encrypted")));
    let all_text: String = parsed
        .turns
        .iter()
        .map(Turn::text)
        .collect::<Vec<_>>()
        .join("\n");
    for noise in ["environment_context", "permissions", "AGENTS.md", "gAAAA"] {
        assert!(!all_text.contains(noise), "{noise} leaked");
    }
}

/// REQ: PORT:ilium-remote-compaction/src/neutral/tests.rs::codex_tool_calls_pair_with_their_outputs_across_call_kinds
#[test]
fn codex_tool_calls_pair_with_their_outputs_across_call_kinds() {
    let parsed = parse_transcript(
        AgentKind::Codex,
        &fixture("codex_rollout.jsonl"),
        Path::new("codex_rollout.jsonl"),
    )
    .expect("parses");
    assert_eq!(tool_call_ids(&parsed.turns[3]), ["call_1"]);
    assert_eq!(tool_call_ids(&parsed.turns[7]), ["call_3"]);
    match &parsed.turns[3].parts[1] {
        Part::ToolCall {
            name, arguments, ..
        } => {
            assert_eq!(name, "shell");
            assert_eq!(arguments["command"][2], "rg -n fetch src");
        }
        other => panic!("unexpected part {other:?}"),
    }
    assert!(
        matches!(&parsed.turns[8].parts[0], Part::ToolResult { call_id, text, .. } if call_id == "call_3" && text.contains("Success"))
    );
    assert_eq!(tool_call_ids(&parsed.turns[11]), ["call_orphan"]);
}

/// REQ: PORT:ilium-remote-compaction/src/neutral/tests.rs::codex_tail_state_reads_ordinals_token_count_and_window
#[test]
fn codex_tail_state_reads_ordinals_token_count_and_window() {
    let parsed = parse_transcript(
        AgentKind::Codex,
        &fixture("codex_rollout.jsonl"),
        Path::new("codex_rollout.jsonl"),
    )
    .expect("parses");
    let tail = parsed.codex.expect("codex tail");
    assert!(tail.has_ordinals);
    assert_eq!(tail.last_ordinal, Some(24), "the torn line must not count");
    assert_eq!(tail.context_window, Some(258_400));
    let template = tail.token_count_template.expect("token_count");
    assert_eq!(
        template["payload"]["info"]["last_token_usage"]["total_tokens"],
        93_100
    );
    assert!(parsed.shape.dropped_torn_fragment);
}

/// REQ: PORT:ilium-remote-compaction/src/neutral/tests.rs::codex_legacy_compacted_message_keeps_recent_user_messages_plus_the_summary
#[test]
fn codex_legacy_compacted_message_keeps_recent_user_messages_plus_the_summary() {
    let path = PathBuf::from("rollout.jsonl");
    let prefix = codex_summary_prefix();
    let lines = [
        json!({"ordinal":0,"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"first request"}]}}),
        json!({"ordinal":1,"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"first answer"}]}}),
        json!({"ordinal":2,"type":"compacted","payload":{"message":format!("{prefix}\nthe legacy summary")}}),
        json!({"ordinal":3,"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"second request"}]}}),
        json!({"ordinal":4,"type":"event_msg","payload":{"type":"thread_rolled_back","num_turns":1}}),
    ];
    let bytes: Vec<u8> = {
        lines
            .iter()
            .map(|line| format!("{line}\n"))
            .collect::<String>()
    }
    .into_bytes();
    let parsed = parse_transcript(AgentKind::Codex, &bytes, &path).expect("parses");
    let texts: Vec<String> = parsed.turns.iter().map(Turn::text).collect();
    // The legacy record keeps the old user message and appends the summary;
    // the rollback then removes the newest user turn ("second request").
    assert_eq!(texts, ["first request", "the legacy summary"]);
    assert_eq!(parsed.turns[1].role, Role::EarlierSummary);
}

/// REQ: PORT:ilium-remote-compaction/src/neutral/tests.rs::codex_summary_message_in_replacement_history_is_recognised_by_its_prefix
#[test]
fn codex_summary_message_in_replacement_history_is_recognised_by_its_prefix() {
    let path = PathBuf::from("rollout.jsonl");
    let prefix = codex_summary_prefix();
    let message = format!("{prefix}\nthe full summary");
    let lines = [
        json!({"ordinal":0,"type":"compacted","payload":{"message":message,"replacement_history":[
            {"type":"message","role":"user","content":[{"type":"input_text","text":message}]},
            {"type":"message","role":"user","content":[{"type":"input_text","text":"kept request"}]},
        ]}}),
    ];
    let bytes: Vec<u8> = {
        lines
            .iter()
            .map(|line| format!("{line}\n"))
            .collect::<String>()
    }
    .into_bytes();
    let parsed = parse_transcript(AgentKind::Codex, &bytes, &path).expect("parses");
    assert_eq!(roles(&parsed.turns), [Role::EarlierSummary, Role::User]);
    assert_eq!(parsed.turns[0].text(), "the full summary");
}

/// REQ: PORT:ilium-remote-compaction/src/neutral/tests.rs::codex_rollout_without_ordinals_is_accepted
#[test]
fn codex_rollout_without_ordinals_is_accepted() {
    let path = PathBuf::from("rollout.jsonl");
    let bytes: Vec<u8> = "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"hi\"}]}}\n".as_bytes().to_vec();
    let tail = parse_transcript(AgentKind::Codex, &bytes, &path)
        .expect("parses")
        .codex
        .expect("tail");
    assert!(!tail.has_ordinals);
    assert_eq!(tail.last_ordinal, None);
}

/// REQ: PORT:ilium-remote-compaction/src/neutral/tests.rs::unreadable_paths_report_the_path
#[test]
fn unreadable_paths_report_the_path() {
    // The host reads the file; an empty read (missing or empty transcript) still names the path in the error.
    let error = parse_transcript(AgentKind::Claude, &[], Path::new("does-not-exist.jsonl"))
        .err()
        .expect("error");
    assert!(error.to_string().contains("does-not-exist.jsonl"));
}
