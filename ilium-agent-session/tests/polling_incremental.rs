use ilium_agent_session::polling_activity::IncrementalReader;
use ilium_core::AgentClass;
use std::io::Write;

#[test]
fn baseline_skips_history_and_reads_only_complete_new_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    std::fs::write(&path, b"old partial").unwrap();
    let mut reader = IncrementalReader::baseline(&path).unwrap();
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    file.write_all(b" tail\n{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"channel\":\"commentary\",\"content\":[{\"type\":\"output_text\",\"text\":\"hello\"}]}}\n").unwrap();
    let (next, first) = reader.read(&path, &AgentClass::Codex, "session").unwrap();
    reader = next;
    assert_eq!(first.events.len(), 1);
    assert_eq!(first.events[0].text, "hello");
    assert!(reader
        .read(&path, &AgentClass::Codex, "session")
        .unwrap()
        .1
        .events
        .is_empty());
}

#[test]
fn truncation_or_replacement_fails_instead_of_replaying_old_activity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    std::fs::write(&path, b"existing record\n").unwrap();
    let reader = IncrementalReader::baseline(&path).unwrap();
    std::fs::write(&path, b"x").unwrap();
    assert!(reader.read(&path, &AgentClass::Codex, "session").is_err());
}

#[test]
fn rewritten_tail_is_not_mistaken_for_an_append() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    std::fs::write(&path, b"existing record\n").unwrap();
    let reader = IncrementalReader::baseline(&path).unwrap();
    std::fs::write(&path, b"replaced record\n").unwrap();
    assert!(reader.read(&path, &AgentClass::Codex, "session").is_err());
}

#[test]
fn replaced_file_with_the_same_contents_is_not_the_original_transcript() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let replacement = dir.path().join("replacement.jsonl");
    std::fs::write(&path, b"existing record\n").unwrap();
    let reader = IncrementalReader::baseline(&path).unwrap();
    std::fs::write(&replacement, b"existing record\n").unwrap();
    std::fs::rename(&replacement, &path).unwrap();
    assert!(reader.read(&path, &AgentClass::Codex, "session").is_err());
}

#[test]
fn malformed_records_report_a_gap_and_partial_lines_wait_for_completion() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    std::fs::write(&path, b"").unwrap();
    let mut reader = IncrementalReader::baseline(&path).unwrap();
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    file.write_all(b"not json\n{\"type\":\"response_item\"")
        .unwrap();
    let (next, first) = reader.read(&path, &AgentClass::Codex, "session").unwrap();
    reader = next;
    assert!(first.contains_gap);
    assert!(first.events.is_empty());
    file.write_all(b",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"channel\":\"commentary\",\"content\":[{\"type\":\"output_text\",\"text\":\"complete\"}]}}\n").unwrap();
    let (_, second) = reader.read(&path, &AgentClass::Codex, "session").unwrap();
    assert_eq!(second.events.len(), 1);
    assert_eq!(second.events[0].text, "complete");
}

#[test]
fn mismatched_claude_session_is_a_gap_not_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    std::fs::write(&path, b"").unwrap();
    let reader = IncrementalReader::baseline(&path).unwrap();
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    file.write_all(b"{\"type\":\"assistant\",\"sessionId\":\"other\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"wrong agent\"}]}}\n").unwrap();
    let (_, result) = reader.read(&path, &AgentClass::Claude, "expected").unwrap();
    assert!(result.contains_gap);
    assert!(result.events.is_empty());
}

#[test]
fn ambiguous_codex_assistant_channel_is_not_sent_as_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    std::fs::write(&path, b"").unwrap();
    let reader = IncrementalReader::baseline(&path).unwrap();
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    file.write_all(b"{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"unknown channel\"}]}}\n").unwrap();
    let (_, result) = reader.read(&path, &AgentClass::Codex, "session").unwrap();
    assert!(result.contains_gap);
    assert!(result.events.is_empty());
}

#[test]
fn metadata_checkpoint_recovers_linked_results_without_replaying_calls_or_partial_lines() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("synthetic-checkpoint.jsonl");
    std::fs::write(&path, b"").unwrap();
    let reader = IncrementalReader::baseline(&path).unwrap();
    let call = r#"{"type":"response_item","payload":{"type":"function_call","id":"synthetic-record","call_id":"synthetic-call","name":"wait","arguments":"private-checkpoint-marker"}}"#;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(file, "{call}").unwrap();
    let (reader, batch) = reader.read(&path, &AgentClass::Codex, "session").unwrap();
    assert_eq!(batch.events.len(), 1);
    let checkpoint = reader
        .checkpoint()
        .expect("complete records publish a recoverable checkpoint");
    let serialized = serde_json::to_string(&checkpoint).unwrap();
    assert!(!serialized.contains("private-checkpoint-marker"));
    assert!(!serialized.contains("synthetic-call"));
    let checkpoint = serde_json::from_str(&serialized).unwrap();
    writeln!(file, "{call}").unwrap();
    file.write_all(r#"{"type":"response_item","payload":{"type":"function_call_output","call_id":"synthetic-call","output":"pending"}}"#.as_bytes()).unwrap();
    file.write_all(b"\n").unwrap();
    file.write_all(r#"{"type":"response_item","payload":{"type":"message","id":"new-message","role":"assistant","channel":"commentary","content":[{"type":"output_text","text":"new"}]"#.as_bytes()).unwrap();
    let resumed = IncrementalReader::resume(&path, &checkpoint).unwrap();
    let (resumed, batch) = resumed.read(&path, &AgentClass::Codex, "session").unwrap();
    assert_eq!(batch.events.len(), 1);
    assert_eq!(
        batch.events[0].kind,
        ilium_core::polling_detection::ActivityKind::ToolResult
    );
    assert_eq!(batch.events[0].text, "pending");
    assert!(
        resumed.checkpoint().is_none(),
        "partial bytes are not durable checkpoint state"
    );
    writeln!(file, "}}}}",).unwrap();
    let (_, batch) = resumed.read(&path, &AgentClass::Codex, "session").unwrap();
    assert_eq!(batch.events.len(), 1);
    assert_eq!(batch.events[0].text, "new");
}

#[test]
fn recovery_refuses_replacement_rewrite_and_invalid_checkpoint_metadata() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("synthetic.jsonl");
    std::fs::write(&path, b"historical\n").unwrap();
    let reader = IncrementalReader::baseline(&path).unwrap();
    let checkpoint = reader
        .checkpoint()
        .expect("newline baseline is checkpointable");
    let mut invalid = checkpoint.clone();
    invalid.version = 255;
    assert!(IncrementalReader::resume(&path, &invalid).is_err());
    let mut invalid = checkpoint.clone();
    invalid.anchor_digest = [0; 32];
    assert!(IncrementalReader::resume(&path, &invalid).is_err());
    let mut invalid = checkpoint.clone();
    invalid.offset += 1;
    assert!(IncrementalReader::resume(&path, &invalid).is_err());
    let mut invalid = checkpoint.clone();
    invalid.recent_record_digests = vec![[1; 32]; 2];
    assert!(IncrementalReader::resume(&path, &invalid).is_err());
    let mut invalid = checkpoint.clone();
    invalid.recent_record_digests = vec![[1; 32]; 4097];
    assert!(IncrementalReader::resume(&path, &invalid).is_err());
    std::fs::write(&path, b"rewritten!\n").unwrap();
    assert!(IncrementalReader::resume(&path, &checkpoint).is_err());
    let other = root.path().join("replacement.jsonl");
    std::fs::write(&other, b"historical\n").unwrap();
    std::fs::rename(other, &path).unwrap();
    assert!(IncrementalReader::resume(&path, &checkpoint).is_err());
}
