//! Infers a short-form (2-3 word) and long-form (5-7 word) pair of titles
//! for one agent pane's session, the same Kilo-Gateway-backed pattern
//! `project_naming` uses for the whole project: collect bounded context,
//! render an XML-shaped Handlebars prompt, call the free model, and
//! parse+validate its JSON reply. `ilium-client`'s tree panel shows the
//! short title when the panel is narrow and the long title when it's wide
//! (see `crate::tree_ui`).
//!
//! `session_naming` only turns "agent class + already-extracted prompts"
//! into a title pair; it never touches a transcript file itself
//! (`transcript_prompts` does that) and never resolves a session ID to a
//! path itself (`ilium_agent_session::TranscriptLocator` does that) -- kept
//! separate so each piece stays independently testable.

use std::path::Path;

use ilium_agent_session::TranscriptLocator;
use ilium_core::AgentClass;
use serde::Serialize;

use crate::naming::{self, BoundedField, DualTitle, PromptCompletionClient};
use crate::transcript_prompts;

const SESSION_TITLE_SHORT_MIN_WORDS: usize = 2;
const SESSION_TITLE_SHORT_MAX_WORDS: usize = 3;
const SESSION_TITLE_LONG_MIN_WORDS: usize = 5;
const SESSION_TITLE_LONG_MAX_WORDS: usize = 7;

const SESSION_TITLE_TEMPLATE: &str = r#"<instructions>
Infer two titles describing the task or work in progress in this coding-agent session: a short title of 2 to 3 words, and a long title of 5 to 7 words. Prefer the shortest accurate wording for each over a longer one. The prompts below are ordered oldest to newest -- weigh the most recent prompt the most, using the earlier ones only as background context. Do not return punctuation-only text or a generic phrase such as "coding session".
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
<output-example>{"session_title_short":"Auth Bug","session_title_long":"Fix Auth Bug In Login Flow"}</output-example>
<response-format>Return exactly one JSON object following the output example. Do not wrap it in Markdown.</response-format>"#;

/// Locates `session_id`'s transcript under `home`, extracts its most recent
/// user prompts, and asks the free model for a short/long title pair. This
/// is the entry point `main.rs` spawns a worker thread around;
/// `infer_session_title` below is the pure remainder, tested directly with
/// fabricated prompts.
pub fn infer_pane_title<G: PromptCompletionClient>(
    generator: &G,
    home: &Path,
    cwd: &Path,
    agent_class: &AgentClass,
    session_id: &str,
) -> anyhow::Result<DualTitle> {
    let transcript = TranscriptLocator::new(home, cwd)
        .transcript_for_session(agent_class, session_id)
        .ok_or_else(|| {
            anyhow::anyhow!("no project-verified transcript found for session {session_id}")
        })?;
    let prompts = transcript_prompts::recent_user_prompts(agent_class, &transcript.path)?;
    infer_session_title(generator, agent_label(agent_class), &prompts)
}

/// Human-readable label for the `<agent>` context tag -- distinct from
/// `tree_ui`'s display label, which is a UI concern, not a prompt-context one.
fn agent_label(class: &AgentClass) -> &str {
    match class {
        AgentClass::Claude => "Claude Code",
        _ => class.label(),
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
) -> anyhow::Result<DualTitle> {
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

fn parse_session_title_response(response: &str) -> anyhow::Result<DualTitle> {
    naming::parse_dual_bounded_word_json(
        response,
        BoundedField {
            field: "session_title_short",
            min_words: SESSION_TITLE_SHORT_MIN_WORDS,
            max_words: SESSION_TITLE_SHORT_MAX_WORDS,
        },
        BoundedField {
            field: "session_title_long",
            min_words: SESSION_TITLE_LONG_MIN_WORDS,
            max_words: SESSION_TITLE_LONG_MAX_WORDS,
        },
        "session-title",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_kilo_gateway::GatewayError;
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
            .join("ilium-session-naming-tests")
            .join(format!("{name}-{:?}", std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn empty_prompts_never_call_the_gateway() {
        let generator = FakeGenerator::new(
            r#"{"session_title_short":"Auth Bug","session_title_long":"Fix Auth Bug In Login Flow"}"#,
        );
        let result = infer_session_title(&generator, "Claude Code", &[]);
        assert!(result.is_err());
        assert_eq!(generator.calls.get(), 0);
    }

    #[test]
    fn successful_response_returns_the_normalized_title_pair() {
        let generator = FakeGenerator::new(
            r#"{"session_title_short":"  Auth   Bug  ","session_title_long":"Fix Auth Bug In Login Flow"}"#,
        );
        let result = infer_session_title(
            &generator,
            "Claude Code",
            &["fix the login bug".to_string()],
        )
        .unwrap();
        assert_eq!(result.short, "Auth Bug");
        assert_eq!(result.long, "Fix Auth Bug In Login Flow");
        assert_eq!(generator.calls.get(), 1);
    }

    #[test]
    fn prompt_is_xml_shaped_recency_ordered_and_includes_the_json_output_example() {
        let generator = FakeGenerator::new(
            r#"{"session_title_short":"Auth Bug","session_title_long":"Fix Auth Bug In Login Flow"}"#,
        );
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
        assert!(prompt.contains(
            "<output-example>{\"session_title_short\":\"Auth Bug\",\"session_title_long\":\"Fix Auth Bug In Login Flow\"}</output-example>"
        ));
    }

    #[test]
    fn rejects_non_json_and_out_of_range_word_counts() {
        assert!(parse_session_title_response("Fix Auth Bug").is_err());
        assert!(parse_session_title_response(
            r#"{"session_title_short":"Fix","session_title_long":"Fix Auth Bug In Login Flow"}"#
        )
        .is_err());
        assert!(parse_session_title_response(
            r#"{"session_title_short":"Auth Bug","session_title_long":"Fix The Whole Auth Bug In The Login Flow Today"}"#
        )
        .is_err());
    }

    #[test]
    fn infer_pane_title_locates_the_transcript_and_returns_the_gateway_title() {
        let home = scratch_dir("infer-pane-title");
        let cwd = Path::new("/home/developer/dev/ai/ilium");
        let slug = cwd.to_string_lossy().replace(['/', '.'], "-");
        let project_dir = home.join(".claude").join("projects").join(slug);
        std::fs::create_dir_all(&project_dir).unwrap();

        let session_id = "77777777-7777-4777-8777-777777777777";
        std::fs::write(
            project_dir.join(format!("{session_id}.jsonl")),
            serde_json::json!({
                "type": "user",
                "sessionId": session_id,
                "cwd": cwd,
                "isSidechain": false,
                "message": {
                    "role": "user",
                    "content": "fix the login bug",
                },
            })
            .to_string(),
        )
        .unwrap();

        let generator = FakeGenerator::new(
            r#"{"session_title_short":"Login Bug","session_title_long":"Fix Login Bug In Auth Flow"}"#,
        );
        let title =
            infer_pane_title(&generator, &home, cwd, &AgentClass::Claude, session_id).unwrap();

        assert_eq!(title.short, "Login Bug");
        assert_eq!(title.long, "Fix Login Bug In Auth Flow");
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
        let cwd = Path::new("/home/developer/dev/ai/ilium");
        let generator = FakeGenerator::new(
            r#"{"session_title_short":"Login Bug","session_title_long":"Fix Login Bug In Auth Flow"}"#,
        );

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

    #[test]
    fn infer_pane_title_rejects_a_codex_transcript_owned_by_another_project() {
        let home = scratch_dir("infer-pane-title-cross-project");
        let cwd = Path::new("/home/developer/dev/ai/money");
        let other_cwd = Path::new("/home/developer/dev/ai/ilium");
        let session_id = "99999999-9999-4999-8999-999999999999";
        let directory = home.join(".codex/sessions/2026/07/14");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(format!("rollout-2026-07-14T12-00-00-{session_id}.jsonl")),
            serde_json::json!({
                "type": "session_meta",
                "payload": {"id": session_id, "cwd": other_cwd},
            })
            .to_string(),
        )
        .unwrap();
        let generator = FakeGenerator::new(
            r#"{"session_title_short":"Wrong Project","session_title_long":"Wrong Project Session Title"}"#,
        );

        let result = infer_pane_title(&generator, &home, cwd, &AgentClass::Codex, session_id);

        assert!(result.is_err());
        assert_eq!(generator.calls.get(), 0);
    }

    #[test]
    fn infer_pane_title_with_an_empty_transcript_never_calls_the_gateway() {
        let home = scratch_dir("infer-pane-title-empty-history");
        let cwd = Path::new("/home/developer/dev/ai/ilium");
        let slug = cwd.to_string_lossy().replace(['/', '.'], "-");
        let project_dir = home.join(".claude").join("projects").join(slug);
        std::fs::create_dir_all(&project_dir).unwrap();

        let session_id = "99999999-9999-4999-8999-999999999999";
        std::fs::write(project_dir.join(format!("{session_id}.jsonl")), "").unwrap();

        let generator = FakeGenerator::new(r#"{"session_title":"Incorrect Title"}"#);
        let result = infer_pane_title(&generator, &home, cwd, &AgentClass::Claude, session_id);

        assert!(result.is_err());
        assert_eq!(generator.calls.get(), 0);
    }
}
