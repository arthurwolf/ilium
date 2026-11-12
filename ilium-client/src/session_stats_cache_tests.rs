//! Regression coverage for resumable session statistics across client stores.

use super::*;
#[cfg(unix)]
use std::io::Write;

// Unix-only: the persistent cache is read only after an owner-only handle check,
// which is unavailable elsewhere, so other platforms never load it (see below).
#[cfg(unix)]
#[test]
fn new_store_resumes_a_previously_scanned_large_transcript() {
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let session_id = "00000000-0000-4000-8000-000000000123";
    let slug: String = project
        .path()
        .to_string_lossy()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect();
    let directory = home.path().join(".claude").join("projects").join(slug);
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("{session_id}.jsonl"));
    let mut file = std::fs::File::create(&path).unwrap();
    writeln!(
        file,
        "{}",
        serde_json::json!({
            "type": "user", "sessionId": session_id, "cwd": project.path(),
            "message": { "content": "synthetic prompt" }
        })
    )
    .unwrap();
    let line = vec![b'x'; 4 * 1024 * 1024 - 1];
    for _ in 0..6 {
        file.write_all(&line).unwrap();
        file.write_all(b"\n").unwrap();
    }
    drop(file);
    let request = StatsRequest {
        class: AgentClass::Claude,
        session_id: session_id.into(),
        project_path: project.path().to_path_buf(),
        home: home.path().to_path_buf(),
    };
    let run_one_pass = |pane_id| {
        let mut store = SessionStatsStore::default();
        assert!(store.request_refresh(pane_id, request.clone(), Instant::now()));
        for _ in 0..1000 {
            store.drain_events();
            if let Some(entry) = store.entry(pane_id) {
                if !entry.in_flight {
                    assert_eq!(entry.state, LoadState::Ready);
                    return entry.accumulator.as_ref().unwrap().offset();
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("statistics pass did not finish");
    };
    let first_offset = run_one_pass(NodeId(710));
    let second_offset = run_one_pass(NodeId(711));
    assert!(first_offset > 0);
    assert!(
        second_offset > first_offset,
        "a new store must continue from the saved scan position"
    );
}

// Unix-only: the persistent cache is read only after an owner-only handle check,
// which is unavailable elsewhere, so other platforms never load it (see below).
#[cfg(unix)]
#[test]
fn saved_accumulator_rejects_changed_transcript_and_owner() {
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let path = project.path().join("transcript.jsonl");
    std::fs::write(
        &path,
        b"{\"type\":\"user\",\"message\":{\"content\":\"one\"}}\n",
    )
    .unwrap();
    let request = StatsRequest {
        class: AgentClass::Claude,
        session_id: "session-one".into(),
        project_path: project.path().to_path_buf(),
        home: home.path().to_path_buf(),
    };
    let mut accumulator = StatsAccumulator::new(AgentClass::Claude);
    accumulator.ingest_file(&path, |_, _, _| {}).unwrap();
    persistent_cache::save(&request, &path, &accumulator, || false).unwrap();
    let (_, loaded) = persistent_cache::load(&request).expect("valid cache");
    assert_eq!(loaded.offset(), accumulator.offset());
    assert_eq!(loaded.snapshot(), accumulator.snapshot());
    let mut other = request.clone();
    other.session_id = "session-two".into();
    assert!(persistent_cache::load(&other).is_none());
    other = request.clone();
    other.class = AgentClass::Codex;
    assert!(persistent_cache::load(&other).is_none());
    other = request.clone();
    other.project_path.push("another-project");
    assert!(persistent_cache::load(&other).is_none());
    other = request.clone();
    other.home.push("another-home");
    assert!(persistent_cache::load(&other).is_none());
    std::fs::write(
        &path,
        b"{\"type\":\"user\",\"message\":{\"content\":\"two\"}}\n",
    )
    .unwrap();
    assert!(
        persistent_cache::load(&request).is_none(),
        "same-size rewrite must invalidate the cache"
    );
}

// Unix-only: the persistent cache is read only after an owner-only handle check,
// which is unavailable elsewhere, so other platforms never load it (see below).
#[cfg(unix)]
#[test]
fn older_cache_writer_does_not_replace_newer_progress() {
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let path = project.path().join("transcript.jsonl");
    std::fs::write(&path, b"{}\n").unwrap();
    let request = StatsRequest {
        class: AgentClass::Codex,
        session_id: "session".into(),
        project_path: project.path().to_path_buf(),
        home: home.path().to_path_buf(),
    };
    let mut older = StatsAccumulator::new(AgentClass::Codex);
    older.ingest_file(&path, |_, _, _| {}).unwrap();
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(file, "{{}}").unwrap();
    drop(file);
    let mut newer = older.clone();
    newer.ingest_file(&path, |_, _, _| {}).unwrap();
    persistent_cache::save(&request, &path, &newer, || false).unwrap();
    persistent_cache::save(&request, &path, &older, || false).unwrap();
    let (_, loaded) = persistent_cache::load(&request).expect("newer cache");
    assert_eq!(loaded.offset(), newer.offset());
}

// Unix-only: the persistent cache is read only after an owner-only handle check,
// which is unavailable elsewhere, so other platforms never load it (see below).
#[cfg(unix)]
#[test]
fn restored_accumulator_resumes_partial_main_and_subagent_records() {
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let path = project.path().join("main.jsonl");
    let extra = project.path().join("subagent.jsonl");
    std::fs::write(&path, b"{}\n{\"type\":\"user\"").unwrap();
    std::fs::write(&extra, b"{}\n{\"type\":\"assistant\"").unwrap();
    let request = StatsRequest {
        class: AgentClass::Claude,
        session_id: "session".into(),
        project_path: project.path().to_path_buf(),
        home: home.path().to_path_buf(),
    };
    let mut initial = StatsAccumulator::new(AgentClass::Claude);
    initial.ingest_file(&path, |_, _, _| {}).unwrap();
    initial.ingest_extra_file(&extra).unwrap();
    persistent_cache::save(&request, &path, &initial, || false).unwrap();
    let (_, mut restored) = persistent_cache::load(&request).expect("saved positions");
    for file in [&path, &extra] {
        let mut output = std::fs::OpenOptions::new().append(true).open(file).unwrap();
        writeln!(output, "}}").unwrap();
    }
    restored.ingest_file(&path, |_, _, _| {}).unwrap();
    restored.ingest_extra_file(&extra).unwrap();
    let mut fresh = StatsAccumulator::new(AgentClass::Claude);
    fresh.ingest_file(&path, |_, _, _| {}).unwrap();
    fresh.ingest_extra_file(&extra).unwrap();
    assert_eq!(restored.offset(), fresh.offset());
    assert_eq!(restored.snapshot(), fresh.snapshot());
}

#[test]
fn replacing_or_truncating_transcript_invalidates_saved_position() {
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let path = project.path().join("main.jsonl");
    let replacement = project.path().join("replacement.jsonl");
    std::fs::write(&path, b"{}\n{}\n").unwrap();
    let request = StatsRequest {
        class: AgentClass::Codex,
        session_id: "session".into(),
        project_path: project.path().to_path_buf(),
        home: home.path().to_path_buf(),
    };
    let mut accumulator = StatsAccumulator::new(AgentClass::Codex);
    accumulator.ingest_file(&path, |_, _, _| {}).unwrap();
    persistent_cache::save(&request, &path, &accumulator, || false).unwrap();
    std::fs::write(&path, b"{}\n").unwrap();
    assert!(
        persistent_cache::load(&request).is_none(),
        "truncation invalidates the saved position"
    );
    std::fs::write(&path, b"{}\n{}\n").unwrap();
    persistent_cache::save(&request, &path, &accumulator, || false).unwrap();
    std::fs::write(&replacement, b"{}\n{}\n").unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::rename(&replacement, &path).unwrap();
    assert!(
        persistent_cache::load(&request).is_none(),
        "replacement invalidates even identical content"
    );
}

/// Without an owner-only handle proof the saved cache is never trusted: a
/// saved accumulator is not loaded back, so statistics are rescanned instead.
#[cfg(not(unix))]
#[test]
fn saved_cache_is_never_loaded_without_an_owner_only_handle_check() {
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let path = project.path().join("transcript.jsonl");
    std::fs::write(
        &path,
        "{\"type\":\"user\",\"message\":{\"content\":\"synthetic\"}}\n",
    )
    .unwrap();
    let request = StatsRequest {
        class: AgentClass::Claude,
        session_id: "session-one".into(),
        project_path: project.path().to_path_buf(),
        home: home.path().to_path_buf(),
    };
    let mut accumulator = StatsAccumulator::new(AgentClass::Claude);
    accumulator.ingest_file(&path, |_, _, _| {}).unwrap();
    // Saving may succeed; loading must still refuse.
    let _ = persistent_cache::save(&request, &path, &accumulator, || false);
    assert!(persistent_cache::load(&request).is_none());
}
