//! Live proof that a transcript compacted in place by `compact_session` is
//! accepted by the real agent CLIs and that the resumed agent has the summary
//! (and not the dropped raw turns) as context. Ignored by default: it spends a
//! few cheap model calls and writes scratch sessions into the real Claude Code
//! and Codex stores, which it deletes again.
//!
//!   ILIUM_E2E_CWD=/media/arthur/tmp/rcm-live \
//!     cargo test -p ilium-remote-compaction --test live_resume -- --ignored --nocapture --test-threads=1

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

use ilium_remote_compaction::{
    compact_session, AgentKind, CompactionOptions, CompactionRequest, Summarizer, SummarizerError,
    SummaryRequest, SummaryResponse, Technique,
};

const DROPPED_FACT: &str = "TEAL-19";
const SUMMARY_FACT: &str = "ZEBRA-7731";

/// Stands in for the remote model: the summary carries only `SUMMARY_FACT`.
struct PlantedSummarizer;

impl Summarizer for PlantedSummarizer {
    fn summarize(&self, _request: &SummaryRequest) -> Result<SummaryResponse, SummarizerError> {
        Ok(SummaryResponse {
            text: format!(
                "## Primary Request and Intent\nThe user is testing context carry-over. \
                 The secret code word of this session is {SUMMARY_FACT}.\n\
                 ## Current Work\nAnswering questions about the code word.\n\
                 ## Pending Tasks\nNone."
            ),
            input_tokens: None,
            output_tokens: None,
        })
    }
}

fn scratch_cwd() -> PathBuf {
    let cwd = PathBuf::from(std::env::var_os("ILIUM_E2E_CWD").expect("ILIUM_E2E_CWD"));
    std::fs::create_dir_all(&cwd).expect("scratch cwd");
    std::fs::canonicalize(cwd).expect("canonical scratch cwd")
}

fn run(command: &mut Command) -> String {
    let output = command.output().expect("spawn agent CLI");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!(
        "--- {:?} -> {}\n{text}",
        command.get_program(),
        output.status
    );
    assert!(output.status.success(), "agent CLI failed: {text}");
    text
}

fn find_file(root: &Path, suffix: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file(&path, suffix) {
                return Some(found);
            }
        } else if path.to_string_lossy().ends_with(suffix) {
            return Some(path);
        }
    }
    None
}

fn compact_in_place(agent: AgentKind, path: &Path, session_id: &str, cwd: &Path) {
    let mut options = CompactionOptions {
        technique: Technique::default_for(agent),
        ..CompactionOptions::default()
    };
    options.tail_tokens = 2_000;
    let request = CompactionRequest {
        agent,
        transcript_path: path.to_path_buf(),
        session_id: session_id.to_string(),
        project_cwd: cwd.to_path_buf(),
        options,
        context_window_tokens: Some(200_000),
    };
    let cancel = AtomicBool::new(false);
    let outcome = compact_session(&request, &PlantedSummarizer, &cancel, &mut |_| {})
        .expect("compaction succeeds");
    println!("compacted: {outcome:?}");
}

fn assert_summary_not_raw(answer: &str) {
    assert!(
        answer.contains(SUMMARY_FACT),
        "summary fact missing: {answer}"
    );
    assert!(
        !answer.contains(DROPPED_FACT),
        "dropped raw turn leaked into context: {answer}"
    );
}

const QUESTION: &str = "Reply with exactly two lines. Line 1: the secret code word of this \
    session. Line 2: my favourite colour code. If you do not know either, write UNKNOWN for it.";

fn cleanup(paths: &[&Path]) {
    for path in paths {
        let _ = std::fs::remove_file(path);
    }
}

#[test]
#[ignore = "spends model calls and writes scratch sessions into the real Claude Code store"]
fn claude_resumes_an_in_place_compacted_transcript() {
    let cwd = scratch_cwd();
    let session_id = std::fs::read_to_string("/proc/sys/kernel/random/uuid")
        .expect("uuid")
        .trim()
        .to_string();
    let claude = |args: &[&str]| {
        run(Command::new("claude")
            .current_dir(&cwd)
            .env_remove("CLAUDECODE")
            .args(["--model", "haiku", "-p"])
            .args(args))
    };
    claude(&[
        "My favourite colour code is TEAL-19. Reply OK.",
        "--session-id",
        &session_id,
    ]);
    for filler in [
        "Say the word one.",
        "Say the word two.",
        "Say the word three.",
    ] {
        claude(&[filler, "--resume", &session_id]);
    }
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
    let transcript = find_file(
        &home.join(".claude/projects"),
        &format!("{session_id}.jsonl"),
    )
    .expect("claude transcript");
    compact_in_place(AgentKind::Claude, &transcript, &session_id, &cwd);

    let answer = claude(&[QUESTION, "--resume", &session_id]);
    let backups: Vec<PathBuf> = std::fs::read_dir(transcript.parent().unwrap())
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.to_string_lossy().contains(&session_id) && path != &transcript)
        .collect();
    let backup_refs: Vec<&Path> = backups.iter().map(PathBuf::as_path).collect();
    cleanup(&backup_refs);
    cleanup(&[&transcript]);
    assert_summary_not_raw(&answer);
}

#[test]
#[ignore = "spends model calls and writes scratch sessions into the real Claude Code store"]
fn claude_resumes_after_compaction_with_an_orphan_tool_use_at_the_end() {
    let cwd = scratch_cwd();
    let session_id = std::fs::read_to_string("/proc/sys/kernel/random/uuid")
        .expect("uuid")
        .trim()
        .to_string();
    let claude = |args: &[&str]| {
        run(Command::new("claude")
            .current_dir(&cwd)
            .env_remove("CLAUDECODE")
            .args(["--model", "haiku", "--allowedTools", "Bash", "-p"])
            .args(args))
    };
    claude(&[
        "My favourite colour code is TEAL-19. Reply OK.",
        "--session-id",
        &session_id,
    ]);
    for filler in ["Say the word one.", "Say the word two."] {
        claude(&[filler, "--resume", &session_id]);
    }
    claude(&[
        "Run the shell command `echo hello` with the Bash tool.",
        "--resume",
        &session_id,
    ]);
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
    let transcript = find_file(
        &home.join(".claude/projects"),
        &format!("{session_id}.jsonl"),
    )
    .expect("claude transcript");
    // Simulate a kill between the tool call and its result: keep everything
    // up to the last assistant record that contains a tool_use.
    let content = std::fs::read_to_string(&transcript).expect("read transcript");
    let lines: Vec<&str> = content.lines().collect();
    let last_tool_use = lines
        .iter()
        .rposition(|line| line.contains("\"type\":\"tool_use\""))
        .expect("the session used a tool");
    std::fs::write(
        &transcript,
        format!("{}\n", lines[..=last_tool_use].join("\n")),
    )
    .expect("truncate transcript");
    compact_in_place(AgentKind::Claude, &transcript, &session_id, &cwd);

    let answer = claude(&[QUESTION, "--resume", &session_id]);
    let backups: Vec<PathBuf> = std::fs::read_dir(transcript.parent().unwrap())
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.to_string_lossy().contains(&session_id) && path != &transcript)
        .collect();
    let backup_refs: Vec<&Path> = backups.iter().map(PathBuf::as_path).collect();
    cleanup(&backup_refs);
    cleanup(&[&transcript]);
    assert_summary_not_raw(&answer);
}

#[test]
#[ignore = "spends model calls and writes scratch sessions into the real Codex store"]
fn codex_resumes_an_in_place_compacted_transcript() {
    let cwd = scratch_cwd();
    let codex = |args: &[&str]| {
        run(Command::new("codex")
            .current_dir(&cwd)
            .args(["exec", "--skip-git-repo-check", "-s", "read-only"])
            .args(args))
    };
    let first = run(Command::new("codex").current_dir(&cwd).args([
        "exec",
        "--skip-git-repo-check",
        "-s",
        "read-only",
        "--json",
        "My favourite colour code is TEAL-19. Reply OK.",
    ]));
    let session_id = first
        .split("\"thread_id\":\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("thread id in codex --json output")
        .to_string();
    for filler in [
        "Say the word one.",
        "Say the word two.",
        "Say the word three.",
    ] {
        codex(&["resume", &session_id, filler]);
    }
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
    let rollout = find_file(
        &home.join(".codex/sessions"),
        &format!("{session_id}.jsonl"),
    )
    .expect("codex rollout");
    compact_in_place(AgentKind::Codex, &rollout, &session_id, &cwd);

    let answer = codex(&["resume", &session_id, QUESTION]);
    let backups: Vec<PathBuf> = std::fs::read_dir(rollout.parent().unwrap())
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.to_string_lossy().contains(&session_id) && path != &rollout)
        .collect();
    let backup_refs: Vec<&Path> = backups.iter().map(PathBuf::as_path).collect();
    cleanup(&backup_refs);
    cleanup(&[&rollout]);
    assert_summary_not_raw(&answer);
}
