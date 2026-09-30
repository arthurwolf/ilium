//! Live proof that Codex's real importer accepts a converted Claude Code
//! session. Ignored by default: it needs the `codex` binary on `PATH`. It runs
//! entirely inside temporary `HOME` and `CODEX_HOME` directories and never
//! touches the user's real ones.
//!
//! Run with: `cargo test -p ilium-session-convert --test live_codex_import -- --ignored --nocapture`

use std::sync::atomic::AtomicBool;

use ilium_agent_session::TranscriptLocator;
use ilium_core::{AgentClass, BuiltinAgentProvider};
use ilium_session_convert::{convert_session, ConversionEvent, ConversionRequest};
use serde_json::json;

const CLAUDE_SESSION_ID: &str = "11111111-2222-4333-8444-555555555555";

#[test]
#[ignore = "needs the real `codex` binary on PATH"]
fn the_real_codex_importer_creates_a_verified_thread() {
    let temporary = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(temporary.path()).unwrap();
    let home = root.join("home");
    let project = root.join("proj");
    std::fs::create_dir_all(&project).unwrap();
    let slug: String = project
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let directory = home.join(".claude/projects").join(slug);
    std::fs::create_dir_all(&directory).unwrap();
    let cwd = project.to_string_lossy();
    let session = |kind: &str, uuid: &str, parent: Option<&str>, message: serde_json::Value| {
        json!({"type": kind, "uuid": uuid, "parentUuid": parent, "sessionId": CLAUDE_SESSION_ID,
            "timestamp": "2026-09-30T10:00:00.000Z", "cwd": cwd, "version": "2.1.285",
            "isSidechain": false, "userType": "external", "message": message})
        .to_string()
    };
    let lines = [
        session(
            "user",
            "a1",
            None,
            json!({"role": "user", "content": "My codeword is PINEAPPLE."}),
        ),
        session(
            "assistant",
            "a2",
            Some("a1"),
            json!({"id": "msg_1", "type": "message", "role": "assistant",
            "model": "claude-haiku-4-5-20251001", "content": [{"type": "text", "text": "Noted."}], "stop_reason": "end_turn"}),
        ),
    ];
    std::fs::write(
        directory.join(format!("{CLAUDE_SESSION_ID}.jsonl")),
        lines.join("\n"),
    )
    .unwrap();

    let request = ConversionRequest {
        home_dir: home.clone(),
        project_cwd: project.clone(),
        source: BuiltinAgentProvider::Claude,
        target: BuiltinAgentProvider::Codex,
        source_session_id: CLAUDE_SESSION_ID.to_string(),
        codex_home: Some(home.join(".codex")),
        codex_executable: None,
    };
    let cancel = AtomicBool::new(false);
    let outcome = convert_session(&request, &cancel, &mut |event| match event {
        ConversionEvent::Step {
            index,
            total,
            title,
        } => println!("[{index}/{total}] {title}"),
        ConversionEvent::Log(line) => println!("  {line}"),
        ConversionEvent::Progress(value) => println!("  progress {value:.2}"),
    })
    .expect("the real importer succeeds");

    println!("{outcome:?}");
    assert!(outcome.target_transcript_path.is_file());
    let located = TranscriptLocator::new(&home, &project)
        .transcript_for_session(&AgentClass::Codex, &outcome.new_session_id)
        .expect("the locator accepts the imported rollout for this project");
    assert_eq!(located.path, outcome.target_transcript_path);
}
