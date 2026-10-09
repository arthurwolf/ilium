//! Rewritten transcripts: they re-parse, keep the agent's own invariants, are
//! backed up, refuse stale files and can be compacted again.

use std::io::Write;
use std::path::Path;

use ilium_remote_compaction::{AgentKind, CompactionError, Technique};
use serde_json::{json, Value};

use crate::common::{
    assert_tool_pairs_complete, claude_active_chain, dir_listing, good_reply, install_fixture,
    parse_lines, request_for, run, run_ok, small_options, uuid_of, Fake, SECRET,
};

fn long_text(marker: &str) -> String {
    format!(
        "{marker} {}",
        "detail about the next feature to build. ".repeat(60)
    )
}

fn append_lines(path: &Path, lines: &[Value]) {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .append(true)
        .open(path)
        .expect("open");
    let mut last_byte = [0_u8; 1];
    let file_length = file.metadata().expect("metadata").len();
    if file_length > 0 {
        use std::io::{Read, Seek, SeekFrom};
        file.seek(SeekFrom::End(-1)).expect("seek to final byte");
        file.read_exact(&mut last_byte).expect("read final byte");
        if last_byte[0] != b'\n' {
            file.write_all(b"\n")
                .expect("separate appended JSONL record");
        }
    }
    for line in lines {
        writeln!(file, "{line}").expect("append");
    }
}

fn backups(dir: &Path) -> Vec<String> {
    dir_listing(dir)
        .into_iter()
        .filter(|name| name.contains(".pre-compaction-") && name.ends_with(".bak"))
        .collect()
}

#[test]
fn claude_output_reparses_and_the_chain_is_valid() {
    let dir = tempfile::tempdir().expect("dir");
    let path = install_fixture(dir.path(), AgentKind::Claude);
    let original = std::fs::read_to_string(&path).expect("read");
    let summarizer = Fake::always(&good_reply(Technique::ClaudeCode));
    let request = request_for(
        AgentKind::Claude,
        &path,
        small_options(Technique::ClaudeCode),
    );
    run_ok(&request, &summarizer);

    let records = parse_lines(&path);
    // The original complete lines are kept byte for byte; only appended to.
    let kept: Vec<&str> = original.lines().collect();
    let rewritten = std::fs::read_to_string(&path).expect("read");
    for line in &kept[..kept.len() - 1] {
        assert!(rewritten.contains(line), "an original line was lost");
    }
    assert!(!rewritten.contains("toolu_orphan\"}") || rewritten.contains("tool_result"));

    let chain = claude_active_chain(&records);
    let boundary = chain[0];
    assert_eq!(boundary["subtype"], "compact_boundary");
    assert!(boundary["parentUuid"].is_null());
    assert!(boundary["logicalParentUuid"].is_string());
    assert_eq!(boundary["sessionId"], crate::common::CLAUDE_SESSION_ID);
    assert!(boundary["compactMetadata"]["preTokens"].as_u64().is_some());

    let summary = chain[1];
    assert_eq!(summary["isCompactSummary"], true);
    assert_eq!(summary["parentUuid"], boundary["uuid"]);
    let summary_text = summary["message"]["content"].to_string();
    assert!(summary_text.contains("FIRST SUMMARY MARKER"));
    assert!(!summary_text.contains("<analysis>"));

    assert_tool_pairs_complete(&chain);
    let mut seen = std::collections::HashSet::new();
    for record in &records {
        if let Some(uuid) = uuid_of(record) {
            assert!(seen.insert(uuid.to_string()), "duplicate uuid {uuid}");
        }
    }
    // Everything appended is new and carries the secret nowhere.
    let appended: String = records[kept.len() - 1..]
        .iter()
        .map(Value::to_string)
        .collect();
    assert!(!appended.contains(SECRET));
}

#[test]
fn claude_new_chain_never_reuses_a_message_id_with_a_tool_pair_split() {
    let dir = tempfile::tempdir().expect("dir");
    let path = install_fixture(dir.path(), AgentKind::Claude);
    let summarizer = Fake::always(&good_reply(Technique::ClaudeCode));
    let request = request_for(
        AgentKind::Claude,
        &path,
        small_options(Technique::ClaudeCode),
    );
    run_ok(&request, &summarizer);
    let records = parse_lines(&path);
    let chain = claude_active_chain(&records);
    // Each assistant tool_use is followed directly by its user tool_result.
    for (position, record) in chain.iter().enumerate() {
        let uses: Vec<&str> = record["message"]["content"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter(|block| block["type"] == "tool_use")
            .filter_map(|block| block["id"].as_str())
            .collect();
        for id in uses {
            let next = chain[position + 1..]
                .iter()
                .find(|candidate| candidate["type"] != "assistant")
                .expect("a result follows");
            assert!(
                next["message"]["content"].to_string().contains(id),
                "{id} unanswered"
            );
        }
    }
}

#[test]
fn backup_is_created_and_equals_the_original_bytes() {
    for agent in [AgentKind::Claude, AgentKind::Codex] {
        let dir = tempfile::tempdir().expect("dir");
        let path = install_fixture(dir.path(), agent);
        let original = std::fs::read(&path).expect("read");
        let summarizer = Fake::always(&good_reply(Technique::default_for(agent)));
        let request = request_for(agent, &path, small_options(Technique::default_for(agent)));
        let (outcome, _) = run_ok(&request, &summarizer);

        assert_eq!(backups(dir.path()).len(), 1, "{agent:?}");
        assert_eq!(
            std::fs::read(&outcome.backup_path).expect("backup"),
            original
        );
        assert_eq!(outcome.transcript_path, path);
        assert_ne!(std::fs::read(&path).expect("read"), original);
        assert_eq!(
            dir_listing(dir.path()).len(),
            2,
            "only the transcript and one backup"
        );
    }
}

#[test]
fn old_backups_are_pruned_to_the_requested_count() {
    let dir = tempfile::tempdir().expect("dir");
    let path = install_fixture(dir.path(), AgentKind::Codex);
    let name = path
        .file_name()
        .expect("name")
        .to_string_lossy()
        .into_owned();
    for stamp in ["20200101000001", "20200101000002", "20200101000003"] {
        std::fs::write(
            dir.path()
                .join(format!("{name}.pre-compaction-{stamp}.bak")),
            "old",
        )
        .expect("seed");
    }
    let summarizer = Fake::always(&good_reply(Technique::Codex));
    let mut options = small_options(Technique::Codex);
    options.keep_backups = 2;
    let request = request_for(AgentKind::Codex, &path, options);
    run_ok(&request, &summarizer);
    assert_eq!(backups(dir.path()).len(), 2);
}

#[test]
fn a_transcript_that_changes_during_compaction_is_refused() {
    for agent in [AgentKind::Claude, AgentKind::Codex] {
        let dir = tempfile::tempdir().expect("dir");
        let path = install_fixture(dir.path(), agent);
        let original = std::fs::read(&path).expect("read original transcript");
        let appended_path = path.clone();
        let summarizer = Fake::always(&good_reply(Technique::default_for(agent))).on_call(move || {
            let line = match agent {
                AgentKind::Claude => json!({"type":"last-prompt","lastPrompt":"still running"}),
                AgentKind::Codex => json!({"ordinal":99,"type":"event_msg","payload":{"type":"agent_message","message":"still running"}}),
            };
            append_lines(&appended_path, &[line]);
        });
        let request = request_for(agent, &path, small_options(Technique::default_for(agent)));
        let finished = run(
            &request,
            &summarizer,
            &std::sync::atomic::AtomicBool::new(false),
        );

        assert!(
            matches!(
                finished.result,
                Err(CompactionError::TranscriptChanged { .. })
            ),
            "{agent:?}: {:?}",
            finished.result
        );
        let bytes = std::fs::read(&path).expect("read");
        assert!(
            bytes.starts_with(&original),
            "{agent:?}: original bytes changed"
        );
        let appended_bytes = bytes
            .get(original.len()..)
            .and_then(|bytes| bytes.strip_suffix(b"\n"))
            .expect("the complete concurrent append remains after the original bytes");
        let appended_record: Value =
            serde_json::from_slice(appended_bytes).expect("concurrent append is valid JSON");
        let append_is_preserved = match agent {
            AgentKind::Claude => appended_record["lastPrompt"] == "still running",
            AgentKind::Codex => appended_record["payload"]["message"] == "still running",
        };
        assert!(append_is_preserved, "{agent:?}: {appended_record}");
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            !text.contains("compact_boundary")
                || agent == AgentKind::Claude && text.matches("compact_boundary").count() == 1
        );
        assert_eq!(
            dir_listing(dir.path()).len(),
            1,
            "no backup or temp file may remain"
        );
    }
}

#[test]
fn codex_ordinals_stay_contiguous_and_the_compacted_record_is_well_formed() {
    let dir = tempfile::tempdir().expect("dir");
    let path = install_fixture(dir.path(), AgentKind::Codex);
    let original_count = std::fs::read_to_string(&path)
        .expect("read")
        .lines()
        .count();
    let summarizer = Fake::always(&good_reply(Technique::Codex));
    let request = request_for(AgentKind::Codex, &path, small_options(Technique::Codex));
    run_ok(&request, &summarizer);

    let records = parse_lines(&path);
    let ordinals: Vec<u64> = records
        .iter()
        .map(|record| record["ordinal"].as_u64().expect("ordinal"))
        .collect();
    let expected: Vec<u64> = (0..records.len() as u64).collect();
    assert_eq!(ordinals, expected, "ordinals must be contiguous from zero");
    assert!(records.len() > original_count - 1, "records were appended");

    let compacted = records
        .iter()
        .rev()
        .find(|record| record["type"] == "compacted")
        .expect("compacted");
    let history = compacted["payload"]["replacement_history"]
        .as_array()
        .expect("history");
    assert!(!history.is_empty());
    assert!(
        history.iter().all(|item| item["type"] == "message"),
        "no tool items in the replacement history"
    );
    let message = compacted["payload"]["message"].as_str().expect("message");
    assert!(message.contains("FIRST SUMMARY MARKER"));
    assert!(history[0]["content"][0]["text"]
        .as_str()
        .expect("text")
        .contains("FIRST SUMMARY MARKER"));
    assert!(!history.iter().any(|item| item
        .to_string()
        .contains("ghp_abcdefghijklmnopqrstuvwxyz0123")));
    assert!(!message.contains("ghp_abcdefghijklmnopqrstuvwxyz0123"));
    // The compacted record is the last history change: only events follow it.
    let position = records
        .iter()
        .rposition(|record| record["type"] == "compacted")
        .expect("position");
    assert!(records[position + 1..]
        .iter()
        .all(|record| record["type"] != "response_item"));
}

#[test]
fn a_second_compaction_anchors_on_the_first_summary_claude() {
    let dir = tempfile::tempdir().expect("dir");
    let path = install_fixture(dir.path(), AgentKind::Claude);
    let first = Fake::always(&good_reply(Technique::ClaudeCode));
    let request = request_for(
        AgentKind::Claude,
        &path,
        small_options(Technique::ClaudeCode),
    );
    run_ok(&request, &first);

    // Nothing new yet: the second run has nothing to compact.
    let second = Fake::always(&good_reply(Technique::ClaudeCode));
    let finished = run(
        &request,
        &second,
        &std::sync::atomic::AtomicBool::new(false),
    );
    assert!(
        matches!(
            finished.result,
            Err(CompactionError::NothingToCompact { .. })
        ),
        "{:?}",
        finished.result
    );
    assert_eq!(second.calls(), 0);

    let records = parse_lines(&path);
    let leaf = uuid_of(records.last().expect("record"))
        .expect("uuid")
        .to_string();
    let base = |uuid: &str, parent: &str| {
        json!({"isSidechain":false,"userType":"external","entrypoint":"cli","cwd":"/repo","sessionId":crate::common::CLAUDE_SESSION_ID,
            "version":"2.1.289","gitBranch":"main","parentUuid":parent,"uuid":uuid,"timestamp":"2026-10-05T11:00:00.000Z"})
    };
    let mut user_a = base("aaaaaaaa-0000-4000-8000-000000000001", &leaf);
    user_a["type"] = json!("user");
    user_a["message"] = json!({"role":"user","content":long_text("SECOND ROUND request")});
    let mut assistant_a = base(
        "aaaaaaaa-0000-4000-8000-000000000002",
        "aaaaaaaa-0000-4000-8000-000000000001",
    );
    assistant_a["type"] = json!("assistant");
    assistant_a["message"] = json!({"id":"msg_second","role":"assistant","model":"claude-sonnet-4-5","stop_reason":"end_turn","content":[{"type":"text","text":long_text("SECOND ROUND answer")}]});
    let mut user_b = base(
        "aaaaaaaa-0000-4000-8000-000000000003",
        "aaaaaaaa-0000-4000-8000-000000000002",
    );
    user_b["type"] = json!("user");
    user_b["message"] = json!({"role":"user","content":"and now the last small request"});
    append_lines(&path, &[user_a, assistant_a, user_b]);

    let third = Fake::always(&good_reply(Technique::ClaudeCode));
    let (outcome, _) = run_ok(&request, &third);
    assert!(!outcome.used_fallback);
    let first_request = third.requests.borrow()[0].clone();
    assert!(first_request.user.contains("<prior-summary>"));
    assert!(
        first_request.user.contains("FIRST SUMMARY MARKER"),
        "the earlier summary is carried forward"
    );
    assert!(first_request.user.contains("SECOND ROUND request"));
    assert!(
        !first_request.user.contains("old: set up"),
        "pre-boundary history stays out"
    );

    let records = parse_lines(&path);
    let chain = claude_active_chain(&records);
    assert_eq!(
        records
            .iter()
            .filter(|record| record["subtype"] == "compact_boundary")
            .count(),
        3,
        "fixture boundary plus two new ones"
    );
    assert_tool_pairs_complete(&chain);
    assert_eq!(backups(dir.path()).len(), 2);
}

#[test]
fn a_second_compaction_anchors_on_the_first_summary_codex() {
    let dir = tempfile::tempdir().expect("dir");
    let path = install_fixture(dir.path(), AgentKind::Codex);
    let first = Fake::always(&good_reply(Technique::Codex));
    let request = request_for(AgentKind::Codex, &path, small_options(Technique::Codex));
    run_ok(&request, &first);

    let second = Fake::always(&good_reply(Technique::Codex));
    let finished = run(
        &request,
        &second,
        &std::sync::atomic::AtomicBool::new(false),
    );
    assert!(
        matches!(
            finished.result,
            Err(CompactionError::NothingToCompact { .. })
        ),
        "{:?}",
        finished.result
    );

    let next = parse_lines(&path).len() as u64;
    let message = |ordinal: u64, role: &str, text: String| {
        let kind = if role == "user" {
            "input_text"
        } else {
            "output_text"
        };
        json!({"timestamp":"2026-10-05T11:00:00.000Z","ordinal":ordinal,"type":"response_item",
            "payload":{"type":"message","role":role,"content":[{"type":kind,"text":text}]}})
    };
    append_lines(
        &path,
        &[
            message(next, "user", long_text("SECOND ROUND request")),
            message(next + 1, "assistant", long_text("SECOND ROUND answer")),
            message(next + 2, "user", "and now the last small request".into()),
        ],
    );

    let third = Fake::always(&good_reply(Technique::Codex));
    let (outcome, _) = run_ok(&request, &third);
    assert!(!outcome.used_fallback);
    let first_request = third.requests.borrow()[0].clone();
    assert!(first_request.user.contains("<prior-summary>"));
    assert!(first_request.user.contains("FIRST SUMMARY MARKER"));
    assert!(first_request.user.contains("SECOND ROUND request"));

    let records = parse_lines(&path);
    let ordinals: Vec<u64> = records
        .iter()
        .map(|record| record["ordinal"].as_u64().expect("ordinal"))
        .collect();
    assert_eq!(ordinals, (0..records.len() as u64).collect::<Vec<_>>());
    assert_eq!(
        records
            .iter()
            .filter(|record| record["type"] == "compacted")
            .count(),
        3
    );
}

#[test]
fn a_tiny_session_is_still_compacted_because_the_tail_is_capped_at_half() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir
        .path()
        .join("rollout-2026-10-05T10-00-00-33333333-3333-4333-8333-333333333333.jsonl");
    std::fs::write(
        &path,
        "{\"ordinal\":0,\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"hi\"}]}}\n",
    )
    .expect("write");
    let summarizer = Fake::always(&good_reply(Technique::Codex));
    let request = request_for(AgentKind::Codex, &path, small_options(Technique::Codex));
    let (outcome, _) = run_ok(&request, &summarizer);
    assert!(!outcome.used_fallback);
    assert_eq!(backups(dir.path()).len(), 1);
}
