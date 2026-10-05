use std::path::PathBuf;

use serde_json::json;

use super::*;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
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

#[test]
fn claude_chain_stops_at_the_last_boundary_and_keeps_its_summary_as_anchor() {
    let parsed =
        parse_transcript(AgentKind::Claude, &fixture("claude_session.jsonl")).expect("parses");
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

#[test]
fn claude_noise_is_dropped_and_message_blocks_are_merged() {
    let parsed =
        parse_transcript(AgentKind::Claude, &fixture("claude_session.jsonl")).expect("parses");
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

#[test]
fn claude_head_carries_the_template_leaf_and_model() {
    let parsed =
        parse_transcript(AgentKind::Claude, &fixture("claude_session.jsonl")).expect("parses");
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

#[test]
fn claude_without_a_boundary_reads_the_whole_chain_and_cuts_missing_parents() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("s.jsonl");
    let lines = [
        json!({"type":"user","uuid":"a","parentUuid":"missing","sessionId":"s","message":{"content":"first"}}),
        json!({"type":"assistant","uuid":"b","parentUuid":"a","sessionId":"s","message":{"id":"m","content":[{"type":"text","text":"second"}]}}),
        json!({"type":"user","uuid":"c","parentUuid":"b","sessionId":"s","message":{"content":"third"}}),
    ];
    std::fs::write(
        &path,
        lines
            .iter()
            .map(|line| format!("{line}\n"))
            .collect::<String>(),
    )
    .expect("write");
    let parsed = parse_transcript(AgentKind::Claude, &path).expect("parses");
    assert_eq!(
        roles(&parsed.turns),
        [Role::User, Role::Assistant, Role::User]
    );
    assert!(parsed
        .warnings
        .iter()
        .any(|warning| warning.contains("pruned")));
}

#[test]
fn claude_preserved_segment_is_relinked_after_the_boundary() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("s.jsonl");
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
    std::fs::write(
        &path,
        lines
            .iter()
            .map(|line| format!("{line}\n"))
            .collect::<String>(),
    )
    .expect("write");
    let parsed = parse_transcript(AgentKind::Claude, &path).expect("parses");
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

#[test]
fn claude_transcript_without_chained_records_is_an_error() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("s.jsonl");
    std::fs::write(&path, "{\"type\":\"last-prompt\"}\n").expect("write");
    let error = parse_transcript(AgentKind::Claude, &path)
        .err()
        .expect("error");
    assert!(matches!(error, CompactionError::NoAnchorRecord { .. }));
}

#[test]
fn codex_compacted_record_resets_history_and_encrypted_items_become_an_opaque_anchor() {
    let parsed =
        parse_transcript(AgentKind::Codex, &fixture("codex_rollout.jsonl")).expect("parses");
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

#[test]
fn codex_tool_calls_pair_with_their_outputs_across_call_kinds() {
    let parsed =
        parse_transcript(AgentKind::Codex, &fixture("codex_rollout.jsonl")).expect("parses");
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

#[test]
fn codex_tail_state_reads_ordinals_token_count_and_window() {
    let parsed =
        parse_transcript(AgentKind::Codex, &fixture("codex_rollout.jsonl")).expect("parses");
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

#[test]
fn codex_legacy_compacted_message_keeps_recent_user_messages_plus_the_summary() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("rollout.jsonl");
    let prefix = codex_summary_prefix();
    let lines = [
        json!({"ordinal":0,"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"first request"}]}}),
        json!({"ordinal":1,"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"first answer"}]}}),
        json!({"ordinal":2,"type":"compacted","payload":{"message":format!("{prefix}\nthe legacy summary")}}),
        json!({"ordinal":3,"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"second request"}]}}),
        json!({"ordinal":4,"type":"event_msg","payload":{"type":"thread_rolled_back","num_turns":1}}),
    ];
    std::fs::write(
        &path,
        lines
            .iter()
            .map(|line| format!("{line}\n"))
            .collect::<String>(),
    )
    .expect("write");
    let parsed = parse_transcript(AgentKind::Codex, &path).expect("parses");
    let texts: Vec<String> = parsed.turns.iter().map(Turn::text).collect();
    // The legacy record keeps the old user message and appends the summary;
    // the rollback then removes the newest user turn ("second request").
    assert_eq!(texts, ["first request", "the legacy summary"]);
    assert_eq!(parsed.turns[1].role, Role::EarlierSummary);
}

#[test]
fn codex_summary_message_in_replacement_history_is_recognised_by_its_prefix() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("rollout.jsonl");
    let prefix = codex_summary_prefix();
    let message = format!("{prefix}\nthe full summary");
    let lines = [
        json!({"ordinal":0,"type":"compacted","payload":{"message":message,"replacement_history":[
            {"type":"message","role":"user","content":[{"type":"input_text","text":message}]},
            {"type":"message","role":"user","content":[{"type":"input_text","text":"kept request"}]},
        ]}}),
    ];
    std::fs::write(
        &path,
        lines
            .iter()
            .map(|line| format!("{line}\n"))
            .collect::<String>(),
    )
    .expect("write");
    let parsed = parse_transcript(AgentKind::Codex, &path).expect("parses");
    assert_eq!(roles(&parsed.turns), [Role::EarlierSummary, Role::User]);
    assert_eq!(parsed.turns[0].text(), "the full summary");
}

#[test]
fn codex_rollout_without_ordinals_is_accepted() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("rollout.jsonl");
    std::fs::write(
        &path,
        "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"hi\"}]}}\n",
    )
    .expect("write");
    let tail = parse_transcript(AgentKind::Codex, &path)
        .expect("parses")
        .codex
        .expect("tail");
    assert!(!tail.has_ordinals);
    assert_eq!(tail.last_ordinal, None);
}

#[test]
fn unreadable_paths_report_the_path() {
    let error = parse_transcript(AgentKind::Claude, &fixture("does-not-exist.jsonl"))
        .err()
        .expect("error");
    assert!(error.to_string().contains("does-not-exist.jsonl"));
}
