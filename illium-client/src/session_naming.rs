//! Infers a short (2-4 word) title for one agent pane's session, the same
//! Kilo-Gateway-backed pattern `project_naming` uses for the whole project:
//! collect bounded context, render an XML-shaped Handlebars prompt, call the
//! free model, and parse+validate its JSON reply.
//!
//! `session_naming` only turns "agent class + already-extracted prompts"
//! into a title; it never touches a transcript file itself
//! (`transcript_prompts` does that) and never resolves a session ID to a
//! path itself (`session_transcript::transcript_path_for_session` does
//! that) -- kept separate so each piece stays independently testable.

use std::path::Path;

use illium_core::AgentClass;
use serde::Serialize;

use crate::naming::{self, PromptCompletionClient};
use crate::session_transcript;
use crate::transcript_prompts;

const SESSION_TITLE_MIN_WORDS: usize = 2;
const SESSION_TITLE_MAX_WORDS: usize = 4;

const SESSION_TITLE_TEMPLATE: &str = r#"<instructions>
Infer a short, 2 to 4 word title describing the task or work in progress in this coding-agent session. Prefer the shortest accurate title over a longer one. The prompts below are ordered oldest to newest -- weigh the most recent prompt the most, using the earlier ones only as background context. Do not return punctuation-only text or a generic phrase such as "coding session".
</instructions>
<agent-session>
    <agent>{{agent_label}}</agent>
    <prompts>
    {{#each prompts}}
        <prompt>
{{this}}
        </prompt>
    {{/each}}
    </prompts>
</agent-session>
<output-example>{"session_title":"Fix Auth Bug"}</output-example>
<response-format>Return exactly one JSON object following the output example. Do not wrap it in Markdown.</response-format>"#;

/// Locates `session_id`'s transcript under `home`, extracts its most recent
/// user prompts, and asks the free model for a title. This is the entry
/// point `main.rs` spawns a worker thread around; `infer_session_title`
/// below is the pure remainder, tested directly with fabricated prompts.
pub fn infer_pane_title<G: PromptCompletionClient>(
    generator: &G,
    home: &Path,
    cwd: &Path,
    agent_class: &AgentClass,
    session_id: &str,
) -> anyhow::Result<String> {
    let transcript_path =
        session_transcript::transcript_path_for_session(agent_class, home, cwd, session_id)
            .ok_or_else(|| anyhow::anyhow!("no transcript file found for session {session_id}"))?;
    let prompts = transcript_prompts::recent_user_prompts(agent_class, &transcript_path)?;
    infer_session_title(generator, agent_label(agent_class), &prompts)
}

/// Human-readable label for the `<agent>` context tag -- distinct from
/// `tree_ui`'s display label, which is a UI concern, not a prompt-context one.
fn agent_label(class: &AgentClass) -> &str {
    match class {
        AgentClass::Claude => "Claude Code",
        AgentClass::Codex => "Codex",
        AgentClass::Other(name) => name,
    }
}

/// Renders the prompt from already-extracted user prompts and calls
/// `generator` exactly once. Errors (no prompts, gateway failure, an
/// unparseable or out-of-range response) are the caller's to surface;
/// there is no retry here, matching `project_naming::bootstrap_project_name`.
fn infer_session_title<G: PromptCompletionClient>(
    generator: &G,
    agent_label: &str,
    prompts: &[String],
) -> anyhow::Result<String> {
    if prompts.is_empty() {
        anyhow::bail!("no user prompts available to infer a session title from");
    }

    let context = SessionTitleContext {
        agent_label: agent_label.to_string(),
        prompts: prompts.to_vec(),
    };
    let response =
        naming::render_and_complete(generator, "session-title", SESSION_TITLE_TEMPLATE, &context)?;
    parse_session_title_response(&response)
}

#[derive(Debug, Serialize)]
struct SessionTitleContext {
    agent_label: String,
    prompts: Vec<String>,
}

fn parse_session_title_response(response: &str) -> anyhow::Result<String> {
    naming::parse_bounded_word_json(
        response,
        "session_title",
        SESSION_TITLE_MIN_WORDS,
        SESSION_TITLE_MAX_WORDS,
        "session-title",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use illium_kilo_gateway::GatewayError;
    use std::cell::{Cell, RefCell};

    struct FakeGenerator {
        calls: Cell<u8>,
        last_prompt: RefCell<Option<String>>,
        response: String,
    }

    impl FakeGenerator {
        fn new(response: impl Into<String>) -> Self {
            Self {
                calls: Cell::new(0),
                last_prompt: RefCell::new(None),
                response: response.into(),
            }
        }
    }

    impl PromptCompletionClient for FakeGenerator {
        fn complete_prompt(&self, prompt: String) -> Result<String, GatewayError> {
            self.calls.set(self.calls.get() + 1);
            *self.last_prompt.borrow_mut() = Some(prompt);
            Ok(self.response.clone())
        }
    }

    fn scratch_dir(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir()
            .join("illium-session-naming-tests")
            .join(format!("{name}-{:?}", std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn empty_prompts_never_call_the_gateway() {
        let generator = FakeGenerator::new(r#"{"session_title":"Fix Auth Bug"}"#);
        let result = infer_session_title(&generator, "Claude Code", &[]);
        assert!(result.is_err());
        assert_eq!(generator.calls.get(), 0);
    }

    #[test]
    fn successful_response_returns_the_normalized_title() {
        let generator = FakeGenerator::new(r#"{"session_title":"  Fix   Auth Bug  "}"#);
        let result = infer_session_title(
            &generator,
            "Claude Code",
            &["fix the login bug".to_string()],
        )
        .unwrap();
        assert_eq!(result, "Fix Auth Bug");
        assert_eq!(generator.calls.get(), 1);
    }

    #[test]
    fn prompt_is_xml_shaped_recency_ordered_and_includes_the_json_output_example() {
        let generator = FakeGenerator::new(r#"{"session_title":"Fix Auth Bug"}"#);
        infer_session_title(
            &generator,
            "Codex",
            &["oldest prompt".to_string(), "newest prompt".to_string()],
        )
        .unwrap();

        let prompt = generator.last_prompt.borrow().clone().unwrap();
        assert!(prompt.contains("<agent-session>"));
        assert!(prompt.contains("<agent>Codex</agent>"));
        assert!(prompt.contains("oldest prompt"));
        assert!(prompt.contains("newest prompt"));
        assert!(prompt.find("oldest prompt").unwrap() < prompt.find("newest prompt").unwrap());
        assert!(prompt
            .contains("<output-example>{\"session_title\":\"Fix Auth Bug\"}</output-example>"));
    }

    #[test]
    fn rejects_non_json_and_out_of_range_word_counts() {
        assert!(parse_session_title_response("Fix Auth Bug").is_err());
        assert!(parse_session_title_response(r#"{"session_title":"Fix"}"#).is_err());
        assert!(parse_session_title_response(
            r#"{"session_title":"Fix The Whole Auth Bug Today"}"#
        )
        .is_err());
    }

    #[test]
    fn infer_pane_title_locates_the_transcript_and_returns_the_gateway_title() {
        let home = scratch_dir("infer-pane-title");
        let cwd = Path::new("/home/developer/dev/ai/illium");
        let slug = cwd.to_string_lossy().replace(['/', '.'], "-");
        let project_dir = home.join(".claude").join("projects").join(slug);
        std::fs::create_dir_all(&project_dir).unwrap();

        let session_id = "77777777-7777-4777-8777-777777777777";
        std::fs::write(
            project_dir.join(format!("{session_id}.jsonl")),
            r#"{"type":"user","message":{"role":"user","content":"fix the login bug"}}"#,
        )
        .unwrap();

        let generator = FakeGenerator::new(r#"{"session_title":"Fix Login Bug"}"#);
        let title =
            infer_pane_title(&generator, &home, cwd, &AgentClass::Claude, session_id).unwrap();

        assert_eq!(title, "Fix Login Bug");
        assert!(generator
            .last_prompt
            .borrow()
            .as_ref()
            .unwrap()
            .contains("fix the login bug"));
    }

    #[test]
    fn infer_pane_title_errors_when_no_transcript_matches_the_session_id() {
        let home = scratch_dir("infer-pane-title-missing");
        let cwd = Path::new("/home/developer/dev/ai/illium");
        let generator = FakeGenerator::new(r#"{"session_title":"Fix Login Bug"}"#);

        let result = infer_pane_title(
            &generator,
            &home,
            cwd,
            &AgentClass::Claude,
            "88888888-8888-4888-8888-888888888888",
        );

        assert!(result.is_err());
        assert_eq!(generator.calls.get(), 0);
    }
}
