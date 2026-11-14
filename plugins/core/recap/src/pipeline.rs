//! The recap pipeline for one pane: gather bounded, redacted evidence (an agent transcript of a
//! verified session, or the shell screen), derive the deterministic goal facts, ask the model
//! once for both lengths, validate the answer and its pointers, and drop the text when the
//! pane's session changed meanwhile. Goals and states come from evidence; the model writes prose
//! only. Everything the host provides (the model, file existence, the pane's current identity)
//! comes in through `PaneEnv`, so the whole flow is testable without a host.

use std::path::Path;

use serde_json::{json, Value};

use crate::custody::denied_path_reason;
use crate::evidence::{
    build_agent_digest, build_screen_digest, read_transcript_window, AgentDigest, ByteSource,
    GoalRecord, HEAD_WINDOW_BYTES, MAX_LINE_BYTES, TAIL_WINDOW_BYTES,
};
use crate::fence::{evidence_fingerprint, fence_still_current, CacheKey, PaneFence, RecapCache};
use crate::model::{
    parse_recap_response, PaneRecap, PaneRecapFacts, RecapLength, RecapPaneState, RecapText,
};
use crate::pointers::validate_pointers;
use crate::transcript::AgentKind;

/// Transcript older than this is reported as stale evidence.
pub const EVIDENCE_STALE_SECONDS: u64 = 600;

pub const SYSTEM_PROMPT: &str =
    "You write factual recaps of terminal panes. Output only the requested JSON object.";

const PROMPT_TEMPLATE: &str = include_str!("../templates/recap.hbs");

/// What was collected for one pane.
pub enum Source<'a> {
    Agent {
        class: AgentKind,
        /// The transcript of the pane's verified session; `None` when the host has no verified
        /// session for the pane.
        transcript: Option<&'a dyn ByteSource>,
        /// When the transcript was last written, in Unix milliseconds.
        modified_unix_millis: Option<u64>,
        project_dir: String,
        activity: String,
    },
    Shell {
        cwd: String,
        screen_text: Option<String>,
    },
}

pub struct PaneInput<'a> {
    pub pane_id: String,
    pub pane_name: String,
    pub source: Source<'a>,
    pub fence: PaneFence,
}

/// The host calls the pipeline makes after evidence is gathered.
pub trait PaneEnv {
    /// One model call; the prompt asks for both lengths. `Err` is a label for the failure.
    fn complete(&mut self, prompt: &str) -> Result<String, String>;
    /// True when the run was cancelled.
    fn cancelled(&mut self) -> bool;
    /// True when `relative` (below the project directory) is an existing regular file.
    fn is_file(&mut self, relative: &str) -> bool;
    /// The identity of the pane right now; `None` when it is gone.
    fn current_fence(&mut self) -> Option<PaneFence>;
    /// Progress in percent, for the job report.
    fn progress(&mut self, _percent: f32) {}
}

enum Gathered {
    Blocked {
        state: RecapPaneState,
        reason: String,
        facts: PaneRecapFacts,
    },
    Ready(ReadyEvidence),
}

struct ReadyEvidence {
    facts: PaneRecapFacts,
    activity: String,
    evidence_kind: &'static str,
    goals_json: String,
    evidence_json: String,
}

pub fn empty_facts() -> PaneRecapFacts {
    PaneRecapFacts {
        goals: Vec::new(),
        goals_omitted: 0,
        evidence_age_seconds: None,
        evidence_stale: false,
        window_truncated: false,
        redactions: 0,
    }
}

fn blocked(state: RecapPaneState, reason: impl Into<String>) -> Gathered {
    Gathered::Blocked {
        state,
        reason: reason.into(),
        facts: empty_facts(),
    }
}

/// JSON for the prompt. `<` is escaped so evidence can never close the `<evidence>` wrapper in
/// the template.
fn json_for_prompt(value: &Value) -> String {
    value.to_string().replace('<', "\\u003c")
}

fn goals_value(goals: &[GoalRecord]) -> Value {
    Value::Array(
        goals
            .iter()
            .map(|goal| json!({"objective": goal.objective, "state": goal.state.name()}))
            .collect(),
    )
}

/// Renders the single recap prompt: the template with its `{{name}}` slots filled.
pub fn render_recap_prompt(
    facts: &PaneRecapFacts,
    activity: &str,
    evidence_kind: &str,
    goals_json: &str,
    evidence_json: &str,
) -> String {
    let age = facts
        .evidence_age_seconds
        .map_or_else(|| "unknown".to_string(), |seconds| seconds.to_string());
    // The evidence goes last: it is untrusted text and must not be scanned for further slots.
    let mut prompt = PROMPT_TEMPLATE
        .replace("{{activity}}", activity)
        .replace("{{evidence_kind}}", evidence_kind)
        .replace("{{evidence_age_seconds}}", &age)
        .replace("{{evidence_stale}}", &facts.evidence_stale.to_string())
        .replace("{{goals_json}}", goals_json);
    if let Some(index) = prompt.find("{{evidence_json}}") {
        prompt.replace_range(index..index + "{{evidence_json}}".len(), evidence_json);
    }
    prompt
}

fn finish_agent_digest(digest: AgentDigest, activity: String, age: Option<u64>) -> Gathered {
    if digest.is_empty() {
        return blocked(RecapPaneState::Idle, "no current activity or evidence");
    }
    let facts = PaneRecapFacts {
        goals: digest.goals.clone(),
        goals_omitted: digest.goals_omitted,
        evidence_age_seconds: age,
        evidence_stale: age.is_some_and(|seconds| seconds > EVIDENCE_STALE_SECONDS),
        window_truncated: digest.window_truncated,
        redactions: digest.redactions,
    };
    let evidence_json = json_for_prompt(&json!({
        "summaries": digest.summaries,
        "prompts": digest.prompts,
        "answers": digest.answers,
        "screen": digest.screen,
    }));
    Gathered::Ready(ReadyEvidence {
        goals_json: json_for_prompt(&goals_value(&digest.goals)),
        evidence_json,
        facts,
        activity,
        evidence_kind: "agent_transcript",
    })
}

fn gather_evidence(source: &Source<'_>, now_unix_millis: u64) -> Gathered {
    match source {
        Source::Agent {
            class,
            transcript,
            modified_unix_millis,
            project_dir,
            activity,
        } => {
            if let Some(reason) = denied_path_reason(Path::new(project_dir)) {
                return blocked(RecapPaneState::Denied, reason);
            }
            let Some(transcript) = transcript else {
                return blocked(
                    RecapPaneState::Unavailable,
                    "no verified transcript for this agent session; screen text is not used for agents",
                );
            };
            if !matches!(class, AgentKind::Claude | AgentKind::Codex) {
                return blocked(
                    RecapPaneState::Unavailable,
                    "no transcript reader for this agent",
                );
            }
            let age = modified_unix_millis
                .map(|modified| now_unix_millis.saturating_sub(modified) / 1_000);
            match read_transcript_window(
                *transcript,
                HEAD_WINDOW_BYTES,
                TAIL_WINDOW_BYTES,
                MAX_LINE_BYTES,
            ) {
                Ok(window) => {
                    finish_agent_digest(build_agent_digest(*class, &window), activity.clone(), age)
                }
                Err(error) => blocked(
                    RecapPaneState::Failed,
                    format!("transcript unreadable: {error}"),
                ),
            }
        }
        Source::Shell { cwd, screen_text } => {
            if let Some(reason) = denied_path_reason(Path::new(cwd)) {
                return blocked(RecapPaneState::Denied, reason);
            }
            let Some(screen_text) = screen_text else {
                return blocked(
                    RecapPaneState::Unavailable,
                    "current terminal screen unavailable",
                );
            };
            let digest = build_screen_digest(screen_text);
            if digest.is_empty() {
                return blocked(RecapPaneState::Idle, "no visible screen text");
            }
            Gathered::Ready(ReadyEvidence {
                facts: PaneRecapFacts {
                    redactions: digest.redactions,
                    ..empty_facts()
                },
                activity: "shell".to_string(),
                evidence_kind: "shell_screen",
                goals_json: "[]".to_string(),
                evidence_json: json_for_prompt(&json!({"screen": digest.screen})),
            })
        }
    }
}

fn pane_without_text(
    input: &PaneInput<'_>,
    state: RecapPaneState,
    facts: PaneRecapFacts,
    failure: Option<String>,
) -> PaneRecap {
    PaneRecap {
        pane_id: input.pane_id.clone(),
        pane_name: input.pane_name.clone(),
        state,
        facts,
        short: None,
        long: None,
        failure,
    }
}

/// Runs the whole pipeline for one pane. Never panics and never returns an error: every
/// failure is a labelled `PaneRecap` state.
pub fn recap_pane(
    input: &PaneInput<'_>,
    now_unix_millis: u64,
    env: &mut dyn PaneEnv,
    cache: &mut RecapCache,
) -> PaneRecap {
    env.progress(10.0);
    let ready = match gather_evidence(&input.source, now_unix_millis) {
        Gathered::Blocked {
            state,
            reason,
            facts,
        } => return pane_without_text(input, state, facts, Some(reason)),
        Gathered::Ready(ready) => ready,
    };
    let evidence_text = format!("{}\n{}", ready.goals_json, ready.evidence_json);
    let key = CacheKey {
        pane_id: input.pane_id.clone(),
        evidence_fingerprint: evidence_fingerprint(&evidence_text),
    };
    if let Some(cached) = cache.get(&key).cloned() {
        if fence_still_current(&input.fence, env.current_fence().as_ref()) {
            return cached;
        }
    }
    if env.cancelled() {
        return pane_without_text(
            input,
            RecapPaneState::Failed,
            ready.facts,
            Some("cancelled".to_string()),
        );
    }

    let prompt = render_recap_prompt(
        &ready.facts,
        &ready.activity,
        ready.evidence_kind,
        &ready.goals_json,
        &ready.evidence_json,
    );
    env.progress(40.0);
    let raw = match env.complete(&prompt) {
        Ok(raw) => raw,
        Err(failure) => {
            return pane_without_text(input, RecapPaneState::Failed, ready.facts, Some(failure))
        }
    };
    env.progress(80.0);
    let parsed = match parse_recap_response(&raw) {
        Ok(parsed) => parsed,
        Err(error) => {
            return pane_without_text(
                input,
                RecapPaneState::Failed,
                ready.facts,
                Some(error.to_string()),
            )
        }
    };
    if !fence_still_current(&input.fence, env.current_fence().as_ref()) {
        return pane_without_text(
            input,
            RecapPaneState::Stale,
            ready.facts,
            Some("pane or session changed while the recap was running".to_string()),
        );
    }

    let project_dir = match &input.source {
        Source::Agent { project_dir, .. } => project_dir.as_str(),
        Source::Shell { cwd, .. } => cwd.as_str(),
    };
    let short = attach_pointers(parsed.short, &evidence_text, project_dir, env);
    let long = attach_pointers(parsed.long, &evidence_text, project_dir, env);
    let recap = PaneRecap {
        pane_id: input.pane_id.clone(),
        pane_name: input.pane_name.clone(),
        state: RecapPaneState::Summarized,
        facts: ready.facts,
        short: Some(short),
        long: Some(long),
        failure: None,
    };
    cache.store(key, recap.clone());
    env.progress(100.0);
    recap
}

fn attach_pointers(
    text: RecapText,
    evidence_text: &str,
    project_dir: &str,
    env: &mut dyn PaneEnv,
) -> RecapText {
    let outcome = validate_pointers(text.pointers, evidence_text, project_dir, &mut |relative| {
        env.is_file(relative)
    });
    RecapText {
        text: text.text,
        pointers: outcome.accepted,
        rejected_pointers: text.rejected_pointers + outcome.rejected,
    }
}

/// Which lengths a run asks for, from the `--length` value.
pub fn lengths_for(choice: &str) -> Option<Vec<RecapLength>> {
    match choice {
        "short" => Some(vec![RecapLength::Short]),
        "long" => Some(vec![RecapLength::Long]),
        "both" => Some(vec![RecapLength::Short, RecapLength::Long]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::ByteSource;

    const GOOD_ANSWER: &str = r#"{"short": {"text": "Goal done: ship recap. Edited src/lib.rs.", "pointers": [{"path": "src/lib.rs", "note": "edited"}, {"path": "/etc/shadow", "note": "invented"}]}, "long": {"text": "Long recap text for the pane.", "pointers": []}}"#;

    struct Bytes(Vec<u8>);

    impl ByteSource for Bytes {
        fn length(&self) -> Result<u64, String> {
            Ok(self.0.len() as u64)
        }

        fn read_at(&self, offset: u64, max: usize) -> Result<Vec<u8>, String> {
            let start = usize::try_from(offset)
                .unwrap_or(usize::MAX)
                .min(self.0.len());
            let end = start.saturating_add(max).min(self.0.len());
            Ok(self.0[start..end].to_vec())
        }
    }

    struct Env {
        answer: Result<String, String>,
        prompts: Vec<String>,
        fence: Option<PaneFence>,
        existing: Vec<&'static str>,
        cancelled: bool,
    }

    impl Env {
        fn answering(answer: &str, fence: &PaneFence) -> Self {
            Self {
                answer: Ok(answer.to_string()),
                prompts: Vec::new(),
                fence: Some(fence.clone()),
                existing: vec!["src/lib.rs"],
                cancelled: false,
            }
        }
    }

    impl PaneEnv for Env {
        fn complete(&mut self, prompt: &str) -> Result<String, String> {
            self.prompts.push(prompt.to_string());
            self.answer.clone()
        }

        fn cancelled(&mut self) -> bool {
            self.cancelled
        }

        fn is_file(&mut self, relative: &str) -> bool {
            self.existing.contains(&relative)
        }

        fn current_fence(&mut self) -> Option<PaneFence> {
            self.fence.clone()
        }
    }

    fn fence(session: Option<&str>) -> PaneFence {
        PaneFence {
            pane_id: "7".into(),
            session_id: session.map(str::to_string),
        }
    }

    fn transcript(lines: &[&str]) -> Bytes {
        Bytes(lines.join("\n").into_bytes())
    }

    fn agent_input<'a>(transcript: Option<&'a dyn ByteSource>, project: &str) -> PaneInput<'a> {
        PaneInput {
            pane_id: "7".into(),
            pane_name: "agent".into(),
            source: Source::Agent {
                class: AgentKind::Claude,
                transcript,
                modified_unix_millis: Some(1_000_000),
                project_dir: project.into(),
                activity: "working".into(),
            },
            fence: fence(Some("s1")),
        }
    }

    fn shell_input(screen: Option<&str>) -> PaneInput<'static> {
        PaneInput {
            pane_id: "7".into(),
            pane_name: "shell".into(),
            source: Source::Shell {
                cwd: "/work/app".into(),
                screen_text: screen.map(str::to_string),
            },
            fence: fence(None),
        }
    }

    const GOAL_LINES: [&str; 3] = [
        r#"{"type":"user","message":{"content":"<command-name>/goal</command-name><command-args>ship recap</command-args>"}}"#,
        r#"{"type":"attachment","attachment":{"type":"goal_status","condition":"ship recap","met":true}}"#,
        r#"{"type":"user","message":{"content":"please edit src/lib.rs"}}"#,
    ];

    #[test]
    fn goal_is_extracted_from_evidence_and_text_is_validated() {
        let bytes = transcript(&GOAL_LINES);
        let input = agent_input(Some(&bytes), "/work/app");
        let mut env = Env::answering(GOOD_ANSWER, &input.fence);
        let recap = recap_pane(&input, 1_700_000, &mut env, &mut RecapCache::default());
        assert_eq!(recap.state, RecapPaneState::Summarized);
        assert_eq!(recap.facts.goal_lines(), vec!["Goal done: ship recap"]);
        let short = recap.short.expect("short text");
        // The invented path is rejected; the existing one named in the evidence survives.
        assert_eq!(short.pointers.len(), 1);
        assert_eq!(short.rejected_pointers, 1);
        assert_eq!(recap.facts.evidence_age_seconds, Some(700));
        assert!(recap.facts.evidence_stale);
        assert_eq!(env.prompts.len(), 1, "one model call covers both lengths");
        assert!(env.prompts[0].contains("evidence_kind: agent_transcript"));
    }

    #[test]
    fn secrets_never_reach_the_prompt() {
        let secret =
            r#"{"type":"user","message":{"content":"my api_key=abcd1234efgh5678 is set"}}"#;
        let bytes = transcript(&[secret]);
        let input = agent_input(Some(&bytes), "/work/app");
        let mut env = Env::answering(GOOD_ANSWER, &input.fence);
        recap_pane(&input, 0, &mut env, &mut RecapCache::default());
        assert!(!env.prompts[0].contains("abcd1234efgh5678"));
    }

    #[test]
    fn agent_without_verified_transcript_is_unavailable_and_never_calls_the_model() {
        let input = agent_input(None, "/work/app");
        let mut env = Env::answering(GOOD_ANSWER, &input.fence);
        let recap = recap_pane(&input, 0, &mut env, &mut RecapCache::default());
        assert_eq!(recap.state, RecapPaneState::Unavailable);
        assert!(env.prompts.is_empty());
    }

    #[test]
    fn protected_project_directory_is_denied_without_reading() {
        let bytes = transcript(&GOAL_LINES);
        let input = agent_input(Some(&bytes), "/home/me/.ssh");
        let mut env = Env::answering(GOOD_ANSWER, &input.fence);
        let recap = recap_pane(&input, 0, &mut env, &mut RecapCache::default());
        assert_eq!(recap.state, RecapPaneState::Denied);
        assert!(env.prompts.is_empty());
    }

    #[test]
    fn unavailable_and_empty_shell_screens_skip_the_model() {
        for (screen, state) in [
            (None, RecapPaneState::Unavailable),
            (Some("  \n "), RecapPaneState::Idle),
        ] {
            let input = shell_input(screen);
            let mut env = Env::answering(GOOD_ANSWER, &input.fence);
            let recap = recap_pane(&input, 0, &mut env, &mut RecapCache::default());
            assert_eq!(recap.state, state);
            assert!(env.prompts.is_empty());
        }
    }

    #[test]
    fn malformed_answer_and_provider_failure_are_labelled_failures() {
        let input = shell_input(Some("$ cargo test"));
        let mut env = Env::answering("no json at all", &input.fence);
        let recap = recap_pane(&input, 0, &mut env, &mut RecapCache::default());
        assert_eq!(recap.state, RecapPaneState::Failed);
        assert!(recap.failure.is_some());

        let mut env = Env::answering(GOOD_ANSWER, &input.fence);
        env.answer = Err("model provider failed: no key".into());
        let recap = recap_pane(&input, 0, &mut env, &mut RecapCache::default());
        assert_eq!(recap.state, RecapPaneState::Failed);
        assert_eq!(
            recap.failure.as_deref(),
            Some("model provider failed: no key")
        );
    }

    #[test]
    fn session_change_or_vanished_pane_during_inference_is_stale() {
        let bytes = transcript(&GOAL_LINES);
        let input = agent_input(Some(&bytes), "/work/app");
        let mut env = Env::answering(GOOD_ANSWER, &input.fence);
        env.fence = Some(fence(Some("s2")));
        let recap = recap_pane(&input, 0, &mut env, &mut RecapCache::default());
        assert_eq!(recap.state, RecapPaneState::Stale);
        assert!(recap.short.is_none());

        env.fence = None;
        let recap = recap_pane(&input, 0, &mut env, &mut RecapCache::default());
        assert_eq!(recap.state, RecapPaneState::Stale);
    }

    #[test]
    fn cancellation_before_the_model_call_is_a_labelled_failure() {
        let input = shell_input(Some("$ ls"));
        let mut env = Env::answering(GOOD_ANSWER, &input.fence);
        env.cancelled = true;
        let recap = recap_pane(&input, 0, &mut env, &mut RecapCache::default());
        assert_eq!(recap.state, RecapPaneState::Failed);
        assert_eq!(recap.failure.as_deref(), Some("cancelled"));
        assert!(env.prompts.is_empty());
    }

    #[test]
    fn cache_reuses_summarized_text_for_unchanged_evidence_only() {
        let input = shell_input(Some("$ cargo test"));
        let mut cache = RecapCache::default();
        let mut env = Env::answering(GOOD_ANSWER, &input.fence);
        recap_pane(&input, 0, &mut env, &mut cache);
        recap_pane(&input, 0, &mut env, &mut cache);
        assert_eq!(env.prompts.len(), 1, "the second run is a cache hit");
        let changed = shell_input(Some("$ cargo test\nerror[E0308]"));
        recap_pane(&changed, 0, &mut env, &mut cache);
        assert_eq!(
            env.prompts.len(),
            2,
            "changed evidence asks the model again"
        );
    }

    #[test]
    fn prompt_keeps_evidence_slots_literal() {
        let facts = empty_facts();
        let prompt = render_recap_prompt(
            &facts,
            "shell",
            "shell_screen",
            "[]",
            "{\"screen\":\"{{activity}}\"}",
        );
        assert!(prompt.contains("{\"screen\":\"{{activity}}\"}"));
        assert!(prompt.contains("activity: shell"));
        assert!(prompt.contains("evidence_age_seconds: unknown"));
    }

    #[test]
    fn length_choice_maps_to_lengths() {
        assert_eq!(lengths_for("both").map(|lengths| lengths.len()), Some(2));
        assert_eq!(lengths_for("short"), Some(vec![RecapLength::Short]));
        assert_eq!(lengths_for("medium"), None);
    }
}
