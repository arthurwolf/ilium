//! Builds one richly contextualized, bounded retitle request for a detected
//! coding-agent session. Live pane metadata and terminal text are captured on
//! the client event loop; project-verified transcript I/O and provider calls
//! stay on the background naming worker.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use ilium_agent_session::TranscriptLocator;
use ilium_core::{AgentActivity, AgentClass, NodeId, PaneTitleSource};
use serde::Serialize;

use crate::naming::{self, BoundedField, DualTitle, PromptCompletionClient};
use crate::transcript_context::{self, TranscriptEntry, TranscriptEntryKind};
use ilium_inference::TitleStyle;

const SESSION_TITLE_SHORT_MIN_WORDS: usize = 2;
const SESSION_TITLE_SHORT_MAX_WORDS: usize = 3;
const SESSION_TITLE_LONG_MIN_WORDS: usize = 1;
const SESSION_TITLE_LONG_MAX_WORDS: usize = 7;

pub(crate) const LABEL_INSTRUCTIONS: &str = ilium_prompts::naming::LABEL_INSTRUCTIONS;
const SUMMARY_INSTRUCTIONS: &str = ilium_prompts::naming::SESSION_SUMMARY;

const SESSION_TITLE_TEMPLATE: &str = ilium_prompts::naming::SESSION_TITLE;

/// Immutable live context captured before the background worker begins. Paths
/// remain typed locally and are converted/clipped only at the LLM boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTitleInput {
    pub pane_id: NodeId,
    pub project_name: String,
    /// The pane's launch cwd, including a worktree or project subdirectory.
    /// This also selects the transcript store and verifies its embedded cwd.
    pub project_path: PathBuf,
    pub agent_class: AgentClass,
    pub session_id: String,
    pub process_id: Option<u32>,
    pub current_title: String,
    pub current_short_title: Option<String>,
    pub current_icon: Option<String>,
    pub title_source: PaneTitleSource,
    pub activity: AgentActivity,
    pub has_persistent_goal: bool,
    pub terminal_screen: String,
    pub parent_group: String,
    pub nearby_titles: Vec<String>,
}

/// Provider-boundary evidence retained only when per-agent debug capture is
/// enabled by the caller. The ordinary title result stays unchanged so the
/// rest of the naming pipeline never depends on diagnostic data.
pub struct SessionTitleInferenceTrace {
    pub rendered_prompt: Option<String>,
    pub raw_response: Option<String>,
    pub result: anyhow::Result<DualTitle>,
}

/// Transparent completion wrapper that observes the exact bounded prompt and
/// successful raw provider response without coupling inference adapters to the
/// debug journal or IPC.
struct TracingPromptCompletionClient<'a, G> {
    inner: &'a G,
    rendered_prompt: RefCell<Option<String>>,
    raw_response: RefCell<Option<String>>,
}

impl<G: PromptCompletionClient> PromptCompletionClient for TracingPromptCompletionClient<'_, G> {
    fn complete_prompt(&self, prompt: String) -> Result<String, ilium_inference::InferenceError> {
        *self.rendered_prompt.borrow_mut() = Some(prompt.clone());
        let response = self.inner.complete_prompt(prompt)?;
        *self.raw_response.borrow_mut() = Some(response.clone());
        Ok(response)
    }

    fn title_style(&self) -> TitleStyle {
        self.inner.title_style()
    }
}

/// Locates the exact project-owned transcript, extracts recent user/assistant/
/// tool entries, then sends one fully rendered prompt to the selected provider.
pub fn infer_pane_title<G: PromptCompletionClient>(
    generator: &G,
    home: &Path,
    input: &SessionTitleInput,
) -> anyhow::Result<DualTitle> {
    let transcript = TranscriptLocator::new(home, &input.project_path)
        .transcript_for_session(&input.agent_class, &input.session_id)
        .ok_or_else(|| {
            anyhow::anyhow!(
                ilium_prompts::naming::NAMING_SESSION_NAMING_NO_PROJECT_VERIFIED_TRANSCRIPT_FOUND_FOR_SESSION,
                input.session_id
            )
        })?;
    let transcript_entries =
        transcript_context::recent_transcript_entries(&input.agent_class, &transcript.path)?;
    // Preserve the established empty-session contract: metadata and a splash
    // screen alone must not spend an inference call before the user has asked
    // the agent to do anything.
    if !transcript_entries
        .iter()
        .any(|entry| entry.kind == TranscriptEntryKind::User)
    {
        anyhow::bail!(ilium_prompts::naming::NAMING_SESSION_NAMING_NO_USER_TRANSCRIPT_ENTRIES_AVAILABLE_TO_INFER_A_SESSION_TITLE_FROM);
    }

    infer_session_title(generator, input, &transcript.path, transcript_entries)
}

/// Runs the same inference while returning its exact rendered request and raw
/// response when the provider boundary was reached. Preflight failures (for
/// example a missing verified transcript) correctly return neither.
pub fn infer_pane_title_with_trace<G: PromptCompletionClient>(
    generator: &G,
    home: &Path,
    input: &SessionTitleInput,
) -> SessionTitleInferenceTrace {
    let tracing_generator = TracingPromptCompletionClient {
        inner: generator,
        rendered_prompt: RefCell::new(None),
        raw_response: RefCell::new(None),
    };
    let result = infer_pane_title(&tracing_generator, home, input);
    SessionTitleInferenceTrace {
        rendered_prompt: tracing_generator.rendered_prompt.into_inner(),
        raw_response: tracing_generator.raw_response.into_inner(),
        result,
    }
}

fn infer_session_title<G: PromptCompletionClient>(
    generator: &G,
    input: &SessionTitleInput,
    transcript_path: &Path,
    transcript_entries: Vec<TranscriptEntry>,
) -> anyhow::Result<DualTitle> {
    let style = generator.title_style();
    let context = SessionTitleContext::new(input, transcript_path, transcript_entries, style);
    let title = naming::render_complete_and_parse(
        generator,
        "session-title",
        SESSION_TITLE_TEMPLATE,
        &context,
        |response| parse_session_title_response_for_style(response, style),
    )?;
    let provider_label = input.agent_class.label();
    if title.short.trim().eq_ignore_ascii_case(provider_label)
        || title.long.trim().eq_ignore_ascii_case(provider_label)
    {
        anyhow::bail!(ilium_prompts::naming::NAMING_SESSION_NAMING_SESSION_TITLE_ONLY_NAMES_THE_AGENT_PROVIDER);
    }
    Ok(title)
}

#[derive(Debug, Serialize)]
struct SessionTitleContext {
    style_instructions: &'static str,
    output_example: &'static str,
    is_labeling: bool,
    agent_label: String,
    pane_id: String,
    current_title: String,
    current_short_title: String,
    current_icon: String,
    title_source: String,
    activity: String,
    has_persistent_goal: String,
    session_id: String,
    process_id: String,
    project_name: String,
    project_path: String,
    parent_group: String,
    nearby_titles: Vec<String>,
    transcript_path: String,
    terminal_screen: String,
    transcript_entries: Vec<PromptTranscriptEntry>,
    user_requests: Vec<String>,
}

impl SessionTitleContext {
    fn new(
        input: &SessionTitleInput,
        transcript_path: &Path,
        transcript_entries: Vec<TranscriptEntry>,
        style: TitleStyle,
    ) -> Self {
        let user_requests = if style == TitleStyle::Labeling {
            transcript_entries
                .iter()
                .filter(|entry| entry.kind == TranscriptEntryKind::User)
                .map(|entry| clipped(&entry.content))
                .collect()
        } else {
            Vec::new()
        };
        // A retrieval label should follow the user's continuing subject. A
        // small assistant tail can resolve "that" references; tool logs and
        // the live screen mostly describe the latest implementation step.
        let transcript_entries = if style == TitleStyle::Labeling {
            transcript_entries
                .iter()
                .rev()
                .filter(|entry| entry.kind == TranscriptEntryKind::Assistant)
                .take(4)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect()
        } else {
            transcript_entries
        };
        Self {
            style_instructions: match style {
                TitleStyle::Labeling => LABEL_INSTRUCTIONS,
                TitleStyle::Summarization => SUMMARY_INSTRUCTIONS,
            },
            output_example: match style {
                TitleStyle::Labeling => ilium_prompts::naming::SESSION_NAMING_FRAGMENT_1,
                TitleStyle::Summarization => ilium_prompts::naming::SESSION_NAMING_FRAGMENT_2,
            },
            is_labeling: style == TitleStyle::Labeling,
            agent_label: clipped(agent_label(&input.agent_class)),
            pane_id: clipped(&input.pane_id.0.to_string()),
            current_title: clipped(&input.current_title),
            current_short_title: clipped(optional_context(input.current_short_title.as_deref())),
            current_icon: clipped(optional_context(input.current_icon.as_deref())),
            title_source: clipped(title_source_label(input.title_source)),
            activity: clipped(activity_label(input.activity)),
            has_persistent_goal: clipped(if input.has_persistent_goal {
                "true"
            } else {
                "false"
            }),
            session_id: clipped(&input.session_id),
            process_id: clipped(
                &input
                    .process_id
                    .map(|process_id| process_id.to_string())
                    .unwrap_or_else(|| ilium_prompts::naming::SESSION_NAMING_FRAGMENT_3.to_string()),
            ),
            project_name: clipped(optional_context(Some(input.project_name.as_str()))),
            project_path: clipped(&input.project_path.display().to_string()),
            parent_group: clipped(&input.parent_group),
            nearby_titles: input.nearby_titles.iter().take(40).map(|title| clipped(title)).collect(),
            transcript_path: clipped(&transcript_path.display().to_string()),
            terminal_screen: clipped(&input.terminal_screen),
            transcript_entries: transcript_entries
                .into_iter()
                .map(|entry| PromptTranscriptEntry {
                    role: clipped(entry.kind.prompt_label()),
                    content: clipped(&entry.content),
                })
                .collect(),
            user_requests,
        }
    }
}

#[derive(Debug, Serialize)]
struct PromptTranscriptEntry {
    role: String,
    content: String,
}

fn clipped(value: &str) -> String {
    naming::encode_untrusted_context(&naming::clip_llm_context_value(value))
}

fn optional_context(value: Option<&str>) -> &str {
    value
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(ilium_prompts::naming::SESSION_NAMING_FRAGMENT_4)
}

fn agent_label(class: &AgentClass) -> &str {
    match class {
        AgentClass::Claude => ilium_prompts::naming::NAMING_SESSION_NAMING_CLAUDE_CODE,
        _ => class.label(),
    }
}

fn title_source_label(source: PaneTitleSource) -> &'static str {
    match source {
        PaneTitleSource::Automatic => "automatic",
        PaneTitleSource::UserSpecified => ilium_prompts::naming::NAMING_SESSION_NAMING_USER_SPECIFIED,
    }
}

fn activity_label(activity: AgentActivity) -> &'static str {
    match activity {
        AgentActivity::Working => "working",
        AgentActivity::WaitingBackground => ilium_prompts::naming::NAMING_SESSION_NAMING_WAITING_ON_BACKGROUND_TASKS,
        AgentActivity::BackgroundTaskStillRunning => ilium_prompts::naming::NAMING_SESSION_NAMING_A_BACKGROUND_TASK_IS_STILL_FINISHING_UP,
        AgentActivity::WaitingApproval => ilium_prompts::naming::NAMING_SESSION_NAMING_WAITING_FOR_USER_APPROVAL,
        AgentActivity::Done => "done",
        AgentActivity::Idle => "idle",
    }
}

#[cfg(test)]
fn parse_session_title_response(response: &str) -> anyhow::Result<DualTitle> {
    parse_session_title_response_for_style(response, TitleStyle::Summarization)
}

fn parse_session_title_response_for_style(
    response: &str,
    style: TitleStyle,
) -> anyhow::Result<DualTitle> {
    naming::parse_dual_bounded_word_json(
        response,
        BoundedField {
            field: "session_title_short",
            min_words: if style == TitleStyle::Labeling {
                1
            } else {
                SESSION_TITLE_SHORT_MIN_WORDS
            },
            max_words: SESSION_TITLE_SHORT_MAX_WORDS,
        },
        BoundedField {
            field: "session_title_long",
            min_words: SESSION_TITLE_LONG_MIN_WORDS,
            max_words: SESSION_TITLE_LONG_MAX_WORDS,
        },
        "session-title",
    )
    .map(|mut title| {
        if style == TitleStyle::Labeling {
            title.short = title.short.to_uppercase();
            title.long = title.long.to_uppercase();
        }
        title
    })
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use ilium_inference::InferenceError;

    use super::*;

    struct FakeGenerator {
        calls: Cell<u8>,
        last_prompt: RefCell<Option<String>>,
        response: String,
    }

    impl FakeGenerator {
        fn success() -> Self {
            Self {
                calls: Cell::new(0),
                last_prompt: RefCell::new(None),
                response: r#"{"icon":"🔐","session_title_short":"Auth Bug","session_title_long":"Fix Auth Bug In Login Flow"}"#.to_string(),
            }
        }
    }

    impl PromptCompletionClient for FakeGenerator {
        fn complete_prompt(&self, prompt: String) -> Result<String, InferenceError> {
            self.calls.set(self.calls.get() + 1);
            *self.last_prompt.borrow_mut() = Some(prompt);
            Ok(self.response.clone())
        }
    }

    struct LabelGenerator(FakeGenerator);

    impl PromptCompletionClient for LabelGenerator {
        fn complete_prompt(&self, prompt: String) -> Result<String, InferenceError> {
            self.0.complete_prompt(prompt)
        }

        fn title_style(&self) -> TitleStyle {
            TitleStyle::Labeling
        }
    }

    fn input(project_path: PathBuf) -> SessionTitleInput {
        SessionTitleInput {
            pane_id: NodeId(42),
            project_name: "ilium".to_string(),
            project_path,
            agent_class: AgentClass::Codex,
            session_id: "77777777-7777-4777-8777-777777777777".to_string(),
            process_id: Some(12345),
            current_title: "Old Authentication Work".to_string(),
            current_short_title: Some("Old Auth".to_string()),
            current_icon: Some("🔐".to_string()),
            title_source: PaneTitleSource::UserSpecified,
            activity: AgentActivity::Working,
            has_persistent_goal: true,
            terminal_screen: "cargo test\ntest auth::login ... ok".to_string(),
            parent_group: "Authentication".to_string(),
            nearby_titles: vec!["PASSWORD RESET".to_string()],
        }
    }

    #[test]
    fn labeling_prompt_uses_tree_context_and_accepts_one_word_labels() {
        let generator = LabelGenerator(FakeGenerator {
            calls: Cell::new(0),
            last_prompt: RefCell::new(None),
            response: r#"{"icon":"🔐","session_title_short":"Hierarchy","session_title_long":"Hierarchy"}"#.to_string(),
        });
        let result = infer_session_title(
            &generator,
            &input(PathBuf::from("/tmp/ilium-label-test")),
            Path::new("/tmp/ilium-label-test/session.jsonl"),
            entries(),
        )
        .unwrap();
        assert_eq!(result.short, "HIERARCHY");
        assert_eq!(result.long, "HIERARCHY");
        let prompt = generator.0.last_prompt.borrow();
        let prompt = prompt.as_deref().unwrap();
        assert!(prompt.contains("thing the user will look for again"));
        assert!(prompt.contains("PASSWORD RESET"));
        assert!(prompt.contains("Authentication"));
        assert!(prompt.contains("Rewrite an automatic activity summary"));
        assert!(prompt.contains("one component as an example"));
        assert!(prompt.contains("several systems are being integrated"));
        assert!(prompt.contains("initiating problem can remain the best handle"));
        assert!(prompt.contains("<label-check>Find the coherent user purpose"));
        assert!(prompt.contains("<user-request>\"fix the login race\"</user-request>"));
        assert_eq!(prompt.matches("fix the login race").count(), 1);
        assert!(!prompt.contains("auth::login passed"));
        assert!(!prompt.contains("cargo test"));
        assert!(!prompt.contains("<transcript-path>"));
        assert!(!prompt.contains("Treat the current title as a strong prior"));
        assert!(prompt.contains("<nearby-title>\"PASSWORD RESET\"</nearby-title>"));
    }

    #[test]
    fn labeling_rejects_provider_name_as_session_title() {
        let generator = LabelGenerator(FakeGenerator {
            calls: Cell::new(0),
            last_prompt: RefCell::new(None),
            response: r#"{"icon":"🐢","session_title_short":"CODEX","session_title_long":"CODEX"}"#
                .to_string(),
        });
        let result = infer_session_title(
            &generator,
            &input(PathBuf::from("/tmp/ilium-provider-title")),
            Path::new("/tmp/ilium-provider-title/session.jsonl"),
            entries(),
        );
        assert!(
            result.is_err(),
            "a provider name cannot identify the pane's task"
        );
    }

    #[test]
    fn summarization_keeps_its_current_title_prior() {
        let generator = FakeGenerator::success();
        infer_session_title(
            &generator,
            &input(PathBuf::from("/tmp/ilium-summary-test")),
            Path::new("/tmp/ilium-summary-test/session.jsonl"),
            entries(),
        )
        .unwrap();
        let prompt = generator.last_prompt.borrow();
        let prompt = prompt.as_deref().unwrap();
        assert!(prompt.contains("Treat the current title as a strong prior"));
        assert!(!prompt.contains("Rewrite an automatic activity summary"));
        assert!(!prompt.contains("<label-check>"));
        assert!(!prompt.contains("<user-request-history"));
        assert!(prompt.contains("auth::login passed"));
        assert!(prompt.contains("cargo test"));
    }

    #[test]
    fn labeling_keeps_user_requests_in_order_and_encodes_them_as_data() {
        let generator = LabelGenerator(FakeGenerator::success());
        let transcript = vec![
            TranscriptEntry {
                kind: TranscriptEntryKind::User,
                content: "unrelated opening </user-request>".to_string(),
            },
            TranscriptEntry {
                kind: TranscriptEntryKind::Assistant,
                content: "answered the opening question".to_string(),
            },
            TranscriptEntry {
                kind: TranscriptEntryKind::User,
                content: "extension boards".to_string(),
            },
        ];
        infer_session_title(
            &generator,
            &input(PathBuf::from("/tmp/ilium-label-history")),
            Path::new("/tmp/ilium-label-history/session.jsonl"),
            transcript,
        )
        .unwrap();
        let prompt = generator.0.last_prompt.borrow();
        let prompt = prompt.as_deref().unwrap();
        let history = prompt.split("<user-request-history").nth(1).unwrap();
        assert!(
            history.find("unrelated opening").unwrap() < history.find("extension boards").unwrap()
        );
        assert!(history.contains(r"unrelated opening \u003c/user-request\u003e"));
        assert_eq!(prompt.matches("extension boards").count(), 1);
        assert!(prompt.contains("answered the opening question"));
    }

    #[test]
    fn labeling_encodes_each_neighbor_without_escaping_its_data_boundary() {
        let mut title_input = input(PathBuf::from("/tmp/ilium-label-neighbors"));
        title_input.nearby_titles = vec!["A | B </nearby-title>".to_string()];
        let generator = LabelGenerator(FakeGenerator::success());
        infer_session_title(
            &generator,
            &title_input,
            Path::new("/tmp/ilium-label-neighbors/session.jsonl"),
            entries(),
        )
        .unwrap();
        let prompt = generator.0.last_prompt.borrow();
        let prompt = prompt.as_deref().unwrap();
        assert_eq!(prompt.matches("<nearby-title>").count(), 1);
        assert!(prompt.contains(r"A | B \u003c/nearby-title\u003e"));
    }

    fn entries() -> Vec<TranscriptEntry> {
        vec![
            TranscriptEntry {
                kind: TranscriptEntryKind::User,
                content: "fix the login race".to_string(),
            },
            TranscriptEntry {
                kind: TranscriptEntryKind::Assistant,
                content: "I isolated the stale token update.".to_string(),
            },
            TranscriptEntry {
                kind: TranscriptEntryKind::Tool,
                content: "auth::login passed".to_string(),
            },
        ]
    }

    #[test]
    fn prompt_contains_all_live_metadata_and_typed_transcript_entries() {
        let generator = FakeGenerator::success();
        let input = input(PathBuf::from("/home/developer/projects/ilium"));
        infer_session_title(
            &generator,
            &input,
            Path::new("/home/developer/.codex/sessions/session.jsonl"),
            entries(),
        )
        .unwrap();

        let prompt = generator.last_prompt.borrow();
        let prompt = prompt.as_deref().unwrap();
        for (tag, value) in [
            ("agent", "Codex"),
            ("pane-id", "42"),
            ("current-title", "Old Authentication Work"),
            ("current-short-title", "Old Auth"),
            ("current-icon", "🔐"),
            ("title-source", "user specified"),
            ("activity", "working"),
            ("has-persistent-goal", "true"),
            ("session-id", "77777777-7777-4777-8777-777777777777"),
            ("process-id", "12345"),
            ("project-name", "ilium"),
            ("project-path", "/home/developer/projects/ilium"),
            (
                "transcript-path",
                "/home/developer/.codex/sessions/session.jsonl",
            ),
        ] {
            let expected = format!("<{tag}>{}</{tag}>", naming::encode_untrusted_context(value));
            assert!(
                prompt.contains(&expected),
                "missing prompt context: {expected}"
            );
        }
        for (role, content) in [
            ("user", "fix the login race"),
            ("assistant", "I isolated the stale token update."),
            ("tool", "auth::login passed"),
        ] {
            assert!(prompt.contains(&format!(
                "<role>{}</role>",
                naming::encode_untrusted_context(role)
            )));
            assert!(prompt.contains(&naming::encode_untrusted_context(content)));
        }
        assert!(prompt.contains(&naming::encode_untrusted_context(
            "cargo test\ntest auth::login ... ok"
        )));
        assert!(
            prompt.find("fix the login race").unwrap()
                < prompt.find("I isolated the stale token update.").unwrap()
        );
        assert!(
            prompt.find("I isolated the stale token update.").unwrap()
                < prompt.find("auth::login passed").unwrap()
        );
        assert!(prompt.contains(
            "The long title must use at most 7 words; this is a maximum, not a target or a minimum."
        ));
        assert!(prompt.contains(
            "A one- or two-word long title is correct when it names the work best; never add filler merely to make a long title longer."
        ));
    }

    #[test]
    fn every_large_dynamic_value_uses_the_shared_edge_budget() {
        // Clipping retains one full edge budget from both ends, so the fixture
        // must exceed twice that budget to exercise the omission boundary.
        let overflow = naming::LLM_CONTEXT_EDGE_CHARS * 2 + 500;
        let generator = FakeGenerator::success();
        let mut input = input(PathBuf::from(format!(
            "/project/{}TAIL_PATH",
            "p".repeat(overflow)
        )));
        input.current_title = format!("TITLE_HEAD{}TITLE_TAIL", "x".repeat(overflow));
        input.terminal_screen = format!("SCREEN_HEAD{}SCREEN_TAIL", "y".repeat(overflow));
        let transcript_entries = vec![TranscriptEntry {
            kind: TranscriptEntryKind::User,
            content: format!("USER_HEAD{}USER_TAIL", "z".repeat(overflow)),
        }];

        infer_session_title(
            &generator,
            &input,
            Path::new("/transcript/session.jsonl"),
            transcript_entries,
        )
        .unwrap();

        let prompt = generator.last_prompt.borrow();
        let prompt = prompt.as_deref().unwrap();
        for retained in [
            "TITLE_HEAD",
            "TITLE_TAIL",
            "SCREEN_HEAD",
            "SCREEN_TAIL",
            "USER_HEAD",
            "USER_TAIL",
            "TAIL_PATH",
        ] {
            assert!(prompt.contains(retained));
        }
        assert!(prompt.matches("characters omitted").count() >= 4);
    }

    #[test]
    fn transcript_without_a_user_entry_never_calls_the_provider() {
        let generator = FakeGenerator::success();
        let directory = tempfile::tempdir().unwrap();
        let project_path = directory.path().join("project");
        let home = directory.path().join("home");
        std::fs::create_dir_all(&project_path).unwrap();
        let session_id = "77777777-7777-4777-8777-777777777777";
        // Must match `ilium_agent_session`'s own rule exactly -- every
        // non-alphanumeric byte becomes `-`. Replacing only `/` and `.` happens
        // to agree on a Unix temp path and disagrees on `C:\Users\...`, where
        // the backslashes and colon would survive.
        let project_slug: String = ilium_platform::paths::canonicalize(&project_path)
            .unwrap_or_else(|_| project_path.clone())
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
        let transcript_dir = home.join(".claude").join("projects").join(project_slug);
        std::fs::create_dir_all(&transcript_dir).unwrap();
        std::fs::write(
            transcript_dir.join(format!("{session_id}.jsonl")),
            serde_json::json!({
                "type": "assistant",
                "sessionId": session_id,
                "cwd": project_path,
                "message": {"content": [{"type": "text", "text": "splash"}]},
            })
            .to_string(),
        )
        .unwrap();
        let mut input = input(project_path);
        input.agent_class = AgentClass::Claude;
        input.session_id = session_id.to_string();

        assert!(infer_pane_title(&generator, &home, &input).is_err());
        assert_eq!(generator.calls.get(), 0);
    }

    #[test]
    fn pane_inference_reads_user_assistant_and_tool_context_from_verified_jsonl() {
        let generator = FakeGenerator::success();
        let directory = tempfile::tempdir().unwrap();
        let project_path = directory.path().join("project");
        let home = directory.path().join("home");
        std::fs::create_dir_all(&project_path).unwrap();
        let session_id = "77777777-7777-4777-8777-777777777777";
        // Must match `ilium_agent_session`'s own rule exactly -- every
        // non-alphanumeric byte becomes `-`. Replacing only `/` and `.` happens
        // to agree on a Unix temp path and disagrees on `C:\Users\...`, where
        // the backslashes and colon would survive.
        let project_slug: String = ilium_platform::paths::canonicalize(&project_path)
            .unwrap_or_else(|_| project_path.clone())
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
        let transcript_dir = home.join(".claude").join("projects").join(project_slug);
        std::fs::create_dir_all(&transcript_dir).unwrap();
        let transcript_path = transcript_dir.join(format!("{session_id}.jsonl"));
        let transcript = [
            serde_json::json!({
                "type": "user",
                "sessionId": session_id,
                "cwd": project_path,
                "message": {"content": "repair auth"},
            }),
            serde_json::json!({
                "type": "assistant",
                "message": {"content": [{"type": "text", "text": "I found the race."}]},
            }),
            serde_json::json!({
                "type": "user",
                "message": {"content": [{"type": "tool_result", "content": "test passed"}]},
            }),
        ]
        .into_iter()
        .map(|entry| entry.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        std::fs::write(&transcript_path, transcript).unwrap();
        let mut input = input(project_path);
        input.agent_class = AgentClass::Claude;
        input.session_id = session_id.to_string();

        let trace = infer_pane_title_with_trace(&generator, &home, &input);
        assert!(trace.result.is_ok());
        assert_eq!(
            trace.raw_response.as_deref(),
            Some(generator.response.as_str())
        );
        let prompt = trace
            .rendered_prompt
            .as_deref()
            .expect("provider-boundary request should be retained");
        assert!(prompt.contains("repair auth"));
        assert!(prompt.contains("I found the race."));
        assert!(prompt.contains("test passed"));
        // Compared through `clipped`, the same encoder the prompt is built
        // with. Untrusted context is JSON-encoded on purpose, so a Windows path
        // legitimately appears as "C:\\Users\\..." -- quoted, with escaped
        // separators. Asserting the raw path only ever held on platforms whose
        // separators need no escaping.
        //
        // Either path form is accepted because which one the locator reports is
        // a platform detail: macOS reaches a temporary directory through a
        // `/var` -> `/private/var` symlink and Windows hands out 8.3 short
        // names.
        let resolved_transcript = ilium_platform::paths::canonicalize(&transcript_path)
            .unwrap_or_else(|_| transcript_path.clone());
        let given_encoded = clipped(&transcript_path.display().to_string());
        let resolved_encoded = clipped(&resolved_transcript.display().to_string());
        assert!(
            prompt.contains(&given_encoded) || prompt.contains(&resolved_encoded),
            "prompt should name the transcript it read.\n  expected one of:\n    {given_encoded}\n    {resolved_encoded}",
        );
    }

    #[test]
    fn transcript_from_another_project_is_rejected_before_provider_call() {
        let generator = FakeGenerator::success();
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("home");
        let project_path = directory.path().join("expected-project");
        let other_project = directory.path().join("other-project");
        std::fs::create_dir_all(home.join(".codex/sessions/2026/07/19")).unwrap();
        let session_id = "99999999-9999-4999-8999-999999999999";
        std::fs::write(
            home.join(format!(
                ".codex/sessions/2026/07/19/rollout-2026-07-19T12-00-00-{session_id}.jsonl"
            )),
            serde_json::json!({
                "type": "session_meta",
                "payload": {"id": session_id, "cwd": other_project},
            })
            .to_string(),
        )
        .unwrap();
        let mut input = input(project_path);
        input.session_id = session_id.to_string();

        assert!(infer_pane_title(&generator, &home, &input).is_err());
        assert_eq!(generator.calls.get(), 0);
    }

    #[test]
    fn response_still_requires_json_icon_and_bounded_title_lengths() {
        assert!(parse_session_title_response("Fix Auth Bug").is_err());
        assert!(parse_session_title_response(
            r#"{"icon":"🔐","session_title_short":"Fix","session_title_long":"Fix Auth Bug In Login Flow"}"#
        )
        .is_err());
        assert!(parse_session_title_response(
            r#"{"icon":"🔐","session_title_short":"Auth Bug","session_title_long":"Fix The Whole Auth Bug In The Login Flow Today"}"#
        )
        .is_err());
        assert!(parse_session_title_response(
            r#"{"icon":"🔐","session_title_short":"Auth Bug","session_title_long":"Login"}"#
        )
        .is_ok());
    }
}
