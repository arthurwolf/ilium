//! Codex -> Claude Code conversion against a hand-written rollout, entirely
//! inside a temporary home directory.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use ilium_agent_session::TranscriptLocator;
use ilium_core::{AgentClass, BuiltinAgentProvider};
use ilium_session_convert::{convert_session, ConversionEvent, ConversionRequest, ConvertError};
use serde_json::Value;

const CODEX_SESSION_ID: &str = "01a0f000-1111-7222-8333-444444444444";
const FIXTURE: &str = include_str!("fixtures/codex_rollout_sample.jsonl");

struct Workspace {
    _temporary: tempfile::TempDir,
    home: PathBuf,
    project: PathBuf,
}

impl Workspace {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        // Canonical so the slug and cwd comparisons match the converter's.
        let root = std::fs::canonicalize(temporary.path()).unwrap();
        let home = root.join("home");
        let project = root.join("work").join("my_proj.v2");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        Self {
            _temporary: temporary,
            home,
            project,
        }
    }

    fn write_rollout(&self, content: &str) -> PathBuf {
        let directory = self.home.join(".codex/sessions/2026/09/30");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!(
            "rollout-2026-09-30T10-00-00-{CODEX_SESSION_ID}.jsonl"
        ));
        let cwd = self.project.to_string_lossy().replace('\\', "\\\\");
        std::fs::write(&path, content.replace("__CWD__", &cwd)).unwrap();
        path
    }

    fn request(&self) -> ConversionRequest {
        ConversionRequest {
            home_dir: self.home.clone(),
            project_cwd: self.project.clone(),
            source: BuiltinAgentProvider::Codex,
            target: BuiltinAgentProvider::Claude,
            source_session_id: CODEX_SESSION_ID.to_string(),
            codex_home: Some(self.home.join(".codex")),
            codex_executable: None,
        }
    }

    fn claude_project_dir(&self) -> PathBuf {
        let slug: String = self
            .project
            .to_string_lossy()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        self.home.join(".claude/projects").join(slug)
    }
}

fn read_lines(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn message_lines(lines: &[Value]) -> Vec<&Value> {
    lines
        .iter()
        .filter(|line| matches!(line["type"].as_str(), Some("user" | "assistant")))
        .collect()
}

fn block(line: &Value) -> &Value {
    &line["message"]["content"][0]
}

#[test]
fn converts_the_fixture_into_a_resumable_claude_transcript() {
    let workspace = Workspace::new();
    workspace.write_rollout(FIXTURE);
    let cancel = AtomicBool::new(false);
    let mut events = Vec::new();
    let outcome = convert_session(&workspace.request(), &cancel, &mut |event| {
        events.push(event)
    })
    .expect("conversion succeeds");

    // The target lives at <home>/.claude/projects/<slug>/<uuid>.jsonl.
    let expected_dir = workspace.claude_project_dir();
    assert_eq!(
        outcome.target_transcript_path.parent(),
        Some(expected_dir.as_path())
    );
    assert_eq!(
        outcome
            .target_transcript_path
            .file_name()
            .unwrap()
            .to_string_lossy(),
        format!("{}.jsonl", outcome.new_session_id)
    );
    assert_eq!(outcome.new_session_id.len(), 36);
    assert_eq!(outcome.converted_items, 12);
    assert_eq!(outcome.dropped_items, 15);

    // Claude Code's own locator agrees the file belongs to this project.
    let located = TranscriptLocator::new(&workspace.home, &workspace.project)
        .transcript_for_session(&AgentClass::Claude, &outcome.new_session_id)
        .expect("locator finds the converted session");
    assert_eq!(located.path, outcome.target_transcript_path);

    // No temp file is left behind.
    let leftovers: Vec<_> = std::fs::read_dir(&expected_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(leftovers, vec![format!("{}.jsonl", outcome.new_session_id)]);

    let lines = read_lines(&outcome.target_transcript_path);
    let messages = message_lines(&lines);
    assert_eq!(messages.len(), 13);
    assert_eq!(lines.len(), 14, "13 messages plus the ai-title line");

    // First message is the real user prompt; every line carries the envelope.
    assert_eq!(messages[0]["type"], "user");
    assert_eq!(
        messages[0]["message"]["content"],
        "List the files and remember the codeword PINEAPPLE."
    );
    let project = workspace.project.to_string_lossy().into_owned();
    for line in &messages {
        assert_eq!(line["sessionId"], outcome.new_session_id.as_str());
        assert_eq!(line["cwd"], project.as_str());
        assert_eq!(line["version"], "2.1.285");
        assert_eq!(line["isSidechain"], false);
        assert_eq!(line["userType"], "external");
        assert_eq!(line["entrypoint"], "cli");
        assert_eq!(line["gitBranch"], "feature/x");
        let timestamp = line["timestamp"].as_str().unwrap();
        assert!(
            timestamp.ends_with('Z') && timestamp.len() == 24,
            "{timestamp}"
        );
    }

    // uuid chain: null, then each line's parent is the previous message.
    assert_eq!(messages[0]["parentUuid"], Value::Null);
    for pair in messages.windows(2) {
        assert_eq!(pair[1]["parentUuid"], pair[0]["uuid"]);
    }

    // Every tool_use is answered by the very next line, with matching ids.
    let mut tool_names = Vec::new();
    for (index, line) in messages.iter().enumerate() {
        if block(line)["type"] != "tool_use" {
            continue;
        }
        let id = block(line)["id"].as_str().unwrap();
        assert!(
            id.starts_with("toolu_")
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        );
        let answer = messages[index + 1];
        assert_eq!(answer["type"], "user");
        assert_eq!(block(answer)["type"], "tool_result");
        assert_eq!(block(answer)["tool_use_id"], id);
        tool_names.push((
            block(line)["name"].as_str().unwrap().to_string(),
            block(line)["input"].clone(),
            block(answer)["content"].as_str().unwrap().to_string(),
        ));
    }
    assert_eq!(tool_names.len(), 5);

    // call_1: shell_command -> Bash with the command and a description.
    assert_eq!(tool_names[0].0, "Bash");
    assert_eq!(tool_names[0].1["command"], "ls");
    assert_eq!(tool_names[0].1["description"], "Run shell command in /work");
    assert!(tool_names[0].2.contains("a.txt"));
    // call_2: exec_command {cmd} -> Bash; its output was recorded out of order.
    assert_eq!(tool_names[1].0, "Bash");
    assert_eq!(tool_names[1].1["command"], "echo hi");
    assert_eq!(tool_names[1].2, "hi\n");
    // call_3: apply_patch keeps its name with the patch text.
    assert_eq!(tool_names[2].0, "apply_patch");
    assert!(tool_names[2].1["patch"]
        .as_str()
        .unwrap()
        .starts_with("*** Begin Patch"));
    // call_4: unknown name sanitized, parsed JSON args kept, output synthesized.
    assert_eq!(tool_names[3].0, "weird_tool_name_with_spaces");
    assert_eq!(tool_names[3].1["query"], "x");
    assert_eq!(tool_names[3].2, "[no output recorded]");
    // call_5: the JavaScript `exec` tool stays `exec` with its raw input; the
    // array output is flattened.
    assert_eq!(tool_names[4].0, "exec");
    assert!(tool_names[4].1["input"]
        .as_str()
        .unwrap()
        .contains("tools.exec_command"));
    assert_eq!(tool_names[4].2, "Script completed\nOutput:\n/work");

    // Assistant text lines; stop reasons.
    let assistant_texts: Vec<&str> = messages
        .iter()
        .filter(|line| block(line)["type"] == "text" && line["type"] == "assistant")
        .map(|line| block(line)["text"].as_str().unwrap())
        .collect();
    assert_eq!(
        assistant_texts,
        vec![
            "I will list the files.",
            "The files are a.txt and b.txt. The codeword is PINEAPPLE."
        ]
    );
    assert_eq!(messages[1]["message"]["stop_reason"], "end_turn");
    assert_eq!(messages[2]["message"]["stop_reason"], "tool_use");
    assert!(messages[1]["message"]["id"]
        .as_str()
        .unwrap()
        .starts_with("msg_"));

    // Nothing that should have been dropped survived, and nothing is doubled.
    let whole = std::fs::read_to_string(&outcome.target_transcript_path).unwrap();
    for banned in [
        "gAAAAAB",
        "AGENTS.md instructions",
        "environment_context",
        "permissions instructions",
        "nobody asked",
        "web_search",
        "ghost",
    ] {
        assert!(
            !whole.contains(banned),
            "`{banned}` leaked into the transcript"
        );
    }
    assert_eq!(
        whole.matches("remember the codeword PINEAPPLE").count(),
        2,
        "user prompt once, plus once in the ai-title"
    );

    // The title line has the confirmed Claude Code shape and sits outside the chain.
    let title = lines.last().unwrap();
    assert_eq!(title["type"], "ai-title");
    assert_eq!(title["sessionId"], outcome.new_session_id.as_str());
    assert!(title["aiTitle"]
        .as_str()
        .unwrap()
        .starts_with("List the files"));

    // Events: steps 1..=6 in order, logs are single lines, progress is monotonic.
    let steps: Vec<(usize, usize)> = events
        .iter()
        .filter_map(|event| match event {
            ConversionEvent::Step { index, total, .. } => Some((*index, *total)),
            _ => None,
        })
        .collect();
    assert_eq!(steps, (1..=6).map(|index| (index, 6)).collect::<Vec<_>>());
    let progress: Vec<f32> = events
        .iter()
        .filter_map(|event| match event {
            ConversionEvent::Progress(value) => Some(*value),
            _ => None,
        })
        .collect();
    assert!(
        progress.windows(2).all(|pair| pair[0] <= pair[1]),
        "{progress:?}"
    );
    assert!(progress.iter().all(|value| (0.0..=1.0).contains(value)));
    assert_eq!(progress.last(), Some(&1.0));
    let logs: Vec<&String> = events
        .iter()
        .filter_map(|event| match event {
            ConversionEvent::Log(line) => Some(line),
            _ => None,
        })
        .collect();
    assert!(logs.len() >= 6, "{logs:?}");
    assert!(logs
        .iter()
        .all(|line| !line.contains('\n') && !line.ends_with('\n')));
    assert!(logs.iter().any(|line| line.contains("claude --resume")));
}

#[test]
fn an_unmatched_source_session_is_reported() {
    let workspace = Workspace::new();
    let cancel = AtomicBool::new(false);
    let error = convert_session(&workspace.request(), &cancel, &mut |_| {}).unwrap_err();
    assert!(
        matches!(error, ConvertError::SourceNotFound { .. }),
        "{error}"
    );
    assert!(!error.to_string().contains('\n'));
}

#[test]
fn a_rollout_of_another_project_is_not_converted() {
    let workspace = Workspace::new();
    let other = workspace.project.parent().unwrap().join("other");
    std::fs::create_dir_all(&other).unwrap();
    let foreign = FIXTURE.replace("__CWD__", &other.to_string_lossy());
    let directory = workspace.home.join(".codex/sessions/2026/09/30");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join(format!(
            "rollout-2026-09-30T10-00-00-{CODEX_SESSION_ID}.jsonl"
        )),
        foreign,
    )
    .unwrap();
    let cancel = AtomicBool::new(false);
    let error = convert_session(&workspace.request(), &cancel, &mut |_| {}).unwrap_err();
    assert!(
        matches!(error, ConvertError::SourceNotFound { .. }),
        "{error}"
    );
}

#[test]
fn unsupported_combinations_and_bad_ids_fail_fast() {
    let workspace = Workspace::new();
    let cancel = AtomicBool::new(false);
    for (source, target) in [
        (BuiltinAgentProvider::Claude, BuiltinAgentProvider::Claude),
        (
            BuiltinAgentProvider::Codex,
            BuiltinAgentProvider::Antigravity,
        ),
        (
            BuiltinAgentProvider::Antigravity,
            BuiltinAgentProvider::Codex,
        ),
    ] {
        let mut request = workspace.request();
        request.source = source;
        request.target = target;
        let error = convert_session(&request, &cancel, &mut |_| {}).unwrap_err();
        assert!(matches!(error, ConvertError::Unsupported { .. }), "{error}");
    }
    let mut request = workspace.request();
    request.source_session_id = "../../etc/passwd".to_string();
    let error = convert_session(&request, &cancel, &mut |_| {}).unwrap_err();
    assert!(
        matches!(error, ConvertError::InvalidSessionId(_)),
        "{error}"
    );
}

#[test]
fn a_rollout_without_a_user_prompt_is_an_empty_conversation() {
    let workspace = Workspace::new();
    let only_context: String = FIXTURE
        .lines()
        .filter(|line| line.contains("session_meta") || line.contains("AGENTS.md"))
        .collect::<Vec<_>>()
        .join("\n");
    workspace.write_rollout(&only_context);
    let cancel = AtomicBool::new(false);
    let error = convert_session(&workspace.request(), &cancel, &mut |_| {}).unwrap_err();
    assert!(
        matches!(error, ConvertError::EmptyConversation { .. }),
        "{error}"
    );
    assert!(!workspace.claude_project_dir().exists());
}

#[test]
fn an_assistant_first_rollout_gets_a_leading_user_message() {
    let workspace = Workspace::new();
    let reordered: String = FIXTURE
        .lines()
        .filter(|line| !line.contains("List the files and remember"))
        .chain(std::iter::once(
            r#"{"timestamp":"2026-09-30T10:00:08.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"a later prompt"}]}}"#,
        ))
        .collect::<Vec<_>>()
        .join("\n");
    workspace.write_rollout(&reordered);
    let cancel = AtomicBool::new(false);
    let outcome = convert_session(&workspace.request(), &cancel, &mut |_| {}).unwrap();
    let lines = read_lines(&outcome.target_transcript_path);
    let messages = message_lines(&lines);
    assert_eq!(messages[0]["type"], "user");
    assert!(messages[0]["message"]["content"]
        .as_str()
        .unwrap()
        .contains("converted from a Codex session"));
    assert_eq!(messages[1]["type"], "assistant");
}

#[test]
fn cancelling_before_the_work_starts_writes_nothing() {
    let workspace = Workspace::new();
    workspace.write_rollout(FIXTURE);
    let cancel = AtomicBool::new(true);
    let error = convert_session(&workspace.request(), &cancel, &mut |_| {}).unwrap_err();
    assert!(matches!(error, ConvertError::Cancelled));
    assert!(!workspace.claude_project_dir().exists());
}

#[test]
fn cancelling_during_the_write_step_leaves_no_partial_transcript() {
    let workspace = Workspace::new();
    workspace.write_rollout(FIXTURE);
    let cancel = AtomicBool::new(false);
    let error = convert_session(&workspace.request(), &cancel, &mut |event| {
        if matches!(event, ConversionEvent::Step { index: 4, .. }) {
            cancel.store(true, Ordering::Relaxed);
        }
    })
    .unwrap_err();
    assert!(matches!(error, ConvertError::Cancelled), "{error}");
    assert!(
        !workspace.claude_project_dir().exists(),
        "the freshly created project directory must be removed again"
    );
}

#[test]
fn a_rollout_of_only_orphan_outputs_is_an_empty_conversation() {
    let workspace = Workspace::new();
    let orphan_only: String = FIXTURE
        .lines()
        .filter(|line| line.contains("session_meta") || line.contains("call_orphan"))
        .collect::<Vec<_>>()
        .join("\n");
    workspace.write_rollout(&orphan_only);
    let cancel = AtomicBool::new(false);
    let error = convert_session(&workspace.request(), &cancel, &mut |_| {}).unwrap_err();
    assert!(
        matches!(error, ConvertError::EmptyConversation { .. }),
        "{error}"
    );
}

#[test]
fn a_placeholder_first_line_never_post_dates_the_lines_after_it() {
    let workspace = Workspace::new();
    let without_prompt: String = FIXTURE
        .lines()
        .filter(|line| !line.contains("List the files and remember"))
        .collect::<Vec<_>>()
        .join("\n");
    workspace.write_rollout(&without_prompt);
    let cancel = AtomicBool::new(false);
    let outcome = convert_session(&workspace.request(), &cancel, &mut |_| {}).unwrap();
    let lines = read_lines(&outcome.target_transcript_path);
    let messages = message_lines(&lines);
    let stamps: Vec<&str> = messages
        .iter()
        .map(|line| line["timestamp"].as_str().unwrap())
        .collect();
    assert!(
        stamps.windows(2).all(|pair| pair[0] <= pair[1]),
        "{stamps:?}"
    );
    assert_eq!(stamps[0], "2026-09-30T10:00:02.500Z");
}
