//! Pipeline tests with a fake provider. No network, no paid calls.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ilium_core::AgentClass;
use ilium_inference::{
    InferenceError, InferenceProvider, InferenceProviderKind, InferenceRequest, InferenceResponse,
};
use tokio::sync::watch;

use super::fence::{PaneFence, RecapCache};
use super::model::{RecapLength, RecapPaneState};
use super::{run_recaps, FenceLookup, PaneRecapInput, RecapContext, RecapOptions, RecapSource};

const GOOD_ANSWER: &str = r#"{"short": {"text": "Goal done: ship recap. Edited src/lib.rs.", "pointers": [{"path": "src/lib.rs", "note": "edited"}, {"path": "/etc/shadow", "note": "invented"}]}, "long": {"text": "Long recap text for the pane.", "pointers": []}}"#;

struct FakeProvider {
    answer: Result<String, String>,
    calls: AtomicUsize,
    prompts: Mutex<Vec<String>>,
    fail_when_prompt_contains: Option<String>,
}

impl FakeProvider {
    fn answering(answer: &str) -> Arc<Self> {
        Arc::new(Self {
            answer: Ok(answer.to_string()),
            calls: AtomicUsize::new(0),
            prompts: Mutex::new(Vec::new()),
            fail_when_prompt_contains: None,
        })
    }

    fn failing_on(marker: &str, answer: &str) -> Arc<Self> {
        Arc::new(Self {
            answer: Ok(answer.to_string()),
            calls: AtomicUsize::new(0),
            prompts: Mutex::new(Vec::new()),
            fail_when_prompt_contains: Some(marker.to_string()),
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn all_prompts(&self) -> String {
        self.prompts.lock().expect("prompts").join("\n")
    }
}

impl InferenceProvider for FakeProvider {
    fn kind(&self) -> InferenceProviderKind {
        InferenceProviderKind::KiloGateway
    }

    fn complete(&self, request: &InferenceRequest) -> Result<InferenceResponse, InferenceError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.prompts
            .lock()
            .expect("prompts")
            .push(request.user_prompt.clone());
        if let Some(marker) = &self.fail_when_prompt_contains {
            if request.user_prompt.contains(marker) {
                return Err(InferenceError::Configuration("provider refused".into()));
            }
        }
        self.answer
            .clone()
            .map(|text| InferenceResponse { text })
            .map_err(InferenceError::Configuration)
    }
}

const CLAUDE_TRANSCRIPT: &[&str] = &[
    r#"{"type":"user","message":{"role":"user","content":"<command-name>/goal</command-name><command-args>ship recap</command-args>"}}"#,
    r#"{"type":"user","message":{"role":"user","content":"please edit src/lib.rs and set api_key=abcd1234efgh5678"}}"#,
    r#"{"type":"attachment","attachment":{"type":"goal_status","condition":"ship recap","met":true}}"#,
];

fn project_with_transcript(lines: &[&str]) -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(directory.path().join("src")).expect("mkdir");
    std::fs::write(directory.path().join("src/lib.rs"), "fn main() {}").expect("write");
    let transcript = directory.path().join("transcript.jsonl");
    std::fs::write(&transcript, lines.join("\n") + "\n").expect("write transcript");
    (directory, transcript)
}

fn agent_input(pane_id: &str, project: &Path, transcript: Option<PathBuf>) -> PaneRecapInput {
    PaneRecapInput {
        pane_id: pane_id.into(),
        pane_name: format!("agent {pane_id}"),
        source: RecapSource::Agent {
            class: AgentClass::Claude,
            verified_transcript: transcript,
            project_dir: project.to_path_buf(),
            activity: "idle".into(),
        },
        fence: PaneFence {
            pane_id: pane_id.into(),
            session_id: Some("session-1".into()),
        },
    }
}

fn shell_input(pane_id: &str, cwd: &Path, screen: Option<&str>) -> PaneRecapInput {
    PaneRecapInput {
        pane_id: pane_id.into(),
        pane_name: format!("shell {pane_id}"),
        source: RecapSource::Shell {
            cwd: cwd.to_path_buf(),
            screen_text: screen.map(str::to_string),
        },
        fence: PaneFence {
            pane_id: pane_id.into(),
            session_id: None,
        },
    }
}

/// Lookup that reports each pane's identity as it was captured.
fn stable_lookup(inputs: &[PaneRecapInput]) -> FenceLookup {
    let map: HashMap<String, PaneFence> = inputs
        .iter()
        .map(|input| (input.pane_id.clone(), input.fence.clone()))
        .collect();
    Arc::new(move |pane_id: &str| map.get(pane_id).cloned())
}

fn context(
    provider: Arc<dyn InferenceProvider>,
    cancel: watch::Receiver<bool>,
    lookup: FenceLookup,
    roots: Vec<PathBuf>,
) -> RecapContext {
    RecapContext {
        progress: None,
        provider,
        options: RecapOptions {
            lengths: vec![RecapLength::Short, RecapLength::Long],
            max_concurrency: 2,
            project_roots: roots,
        },
        cancel,
        fence_lookup: lookup,
        cache: Arc::new(Mutex::new(RecapCache::default())),
    }
}

fn never_cancelled() -> watch::Receiver<bool> {
    watch::channel(false).1
}

#[tokio::test]
async fn goal_is_extracted_from_evidence_and_text_is_validated() {
    let (directory, transcript) = project_with_transcript(CLAUDE_TRANSCRIPT);
    let inputs = vec![agent_input("p1", directory.path(), Some(transcript))];
    let provider = FakeProvider::answering(GOOD_ANSWER);
    let batch = run_recaps(
        inputs.clone(),
        context(
            provider.clone(),
            never_cancelled(),
            stable_lookup(&inputs),
            vec![directory.path().to_path_buf()],
        ),
    )
    .await;

    let pane = &batch.panes[0];
    assert_eq!(pane.state, RecapPaneState::Summarized);
    assert_eq!(pane.facts.goals.len(), 1);
    assert_eq!(
        pane.facts.goal_lines(),
        vec!["Goal done: ship recap".to_string()]
    );
    let short = pane.short.as_ref().expect("short text");
    assert_eq!(
        short.pointers.len(),
        1,
        "invented /etc/shadow pointer must be rejected"
    );
    assert_eq!(short.pointers[0].path, "src/lib.rs");
    assert_eq!(short.rejected_pointers, 1);
    assert!(pane.long.is_some());
}

#[tokio::test]
async fn secrets_never_reach_the_prompt() {
    let (directory, transcript) = project_with_transcript(CLAUDE_TRANSCRIPT);
    let inputs = vec![agent_input("p1", directory.path(), Some(transcript))];
    let provider = FakeProvider::answering(GOOD_ANSWER);
    let _ = run_recaps(
        inputs.clone(),
        context(
            provider.clone(),
            never_cancelled(),
            stable_lookup(&inputs),
            vec![directory.path().to_path_buf()],
        ),
    )
    .await;
    let prompts = provider.all_prompts();
    assert!(prompts.contains("ship recap"));
    assert!(!prompts.contains("abcd1234efgh5678"));
}

#[tokio::test]
async fn agent_without_verified_transcript_is_unavailable_and_never_calls_the_model() {
    let (directory, _transcript) = project_with_transcript(CLAUDE_TRANSCRIPT);
    let inputs = vec![agent_input("p1", directory.path(), None)];
    let provider = FakeProvider::answering(GOOD_ANSWER);
    let batch = run_recaps(
        inputs.clone(),
        context(
            provider.clone(),
            never_cancelled(),
            stable_lookup(&inputs),
            vec![],
        ),
    )
    .await;
    assert_eq!(batch.panes[0].state, RecapPaneState::Unavailable);
    assert!(batch.panes[0]
        .failure
        .as_deref()
        .unwrap_or_default()
        .contains("screen text is not used"));
    assert_eq!(provider.calls(), 0);
}

#[tokio::test]
async fn protected_project_directory_is_denied_without_reading() {
    let (directory, transcript) = project_with_transcript(CLAUDE_TRANSCRIPT);
    let protected = directory.path().join(".ssh");
    std::fs::create_dir_all(&protected).expect("mkdir");
    let mut input = agent_input("p1", &protected, Some(transcript));
    input.fence.pane_id = "p1".into();
    let inputs = vec![input];
    let provider = FakeProvider::answering(GOOD_ANSWER);
    let batch = run_recaps(
        inputs.clone(),
        context(
            provider.clone(),
            never_cancelled(),
            stable_lookup(&inputs),
            vec![],
        ),
    )
    .await;
    assert_eq!(batch.panes[0].state, RecapPaneState::Denied);
    assert_eq!(provider.calls(), 0);
}

#[tokio::test]
async fn unavailable_shell_screen_is_unavailable_and_skips_the_model() {
    let directory = tempfile::tempdir().unwrap();
    let input = shell_input("p1", directory.path(), None);
    let provider = FakeProvider::answering(GOOD_ANSWER);
    let batch = run_recaps(
        vec![input.clone()],
        context(
            provider.clone(),
            never_cancelled(),
            stable_lookup(&[input]),
            vec![directory.path().to_owned()],
        ),
    )
    .await;
    assert_eq!(batch.panes[0].state, RecapPaneState::Unavailable);
    assert_eq!(provider.calls(), 0);
}

#[tokio::test]
async fn empty_shell_screen_is_idle_and_skips_the_model() {
    let directory = tempfile::tempdir().expect("tempdir");
    let inputs = vec![shell_input("s1", directory.path(), Some("   \n  "))];
    let provider = FakeProvider::answering(GOOD_ANSWER);
    let batch = run_recaps(
        inputs.clone(),
        context(
            provider.clone(),
            never_cancelled(),
            stable_lookup(&inputs),
            vec![],
        ),
    )
    .await;
    assert_eq!(batch.panes[0].state, RecapPaneState::Idle);
    assert_eq!(provider.calls(), 0);
}

#[tokio::test]
async fn malformed_answer_is_a_labelled_per_pane_failure() {
    let directory = tempfile::tempdir().expect("tempdir");
    let inputs = vec![shell_input("s1", directory.path(), Some("$ cargo test"))];
    let provider = FakeProvider::answering("I cannot comply");
    let batch = run_recaps(
        inputs.clone(),
        context(provider, never_cancelled(), stable_lookup(&inputs), vec![]),
    )
    .await;
    assert_eq!(batch.panes[0].state, RecapPaneState::Failed);
    assert!(batch.panes[0]
        .failure
        .as_deref()
        .unwrap_or_default()
        .contains("not the expected JSON"));
    assert!(batch.panes[0].short.is_none());
}

#[tokio::test]
async fn provider_failure_on_one_pane_keeps_the_other_pane_text() {
    let directory = tempfile::tempdir().expect("tempdir");
    let inputs = vec![
        shell_input("ok", directory.path(), Some("$ make build")),
        shell_input("bad", directory.path(), Some("FAILME $ make test")),
    ];
    let provider = FakeProvider::failing_on("FAILME", GOOD_ANSWER);
    let batch = run_recaps(
        inputs.clone(),
        context(provider, never_cancelled(), stable_lookup(&inputs), vec![]),
    )
    .await;
    assert_eq!(batch.panes[0].state, RecapPaneState::Summarized);
    assert_eq!(batch.panes[1].state, RecapPaneState::Failed);
    assert!(batch.panes[1]
        .failure
        .as_deref()
        .unwrap_or_default()
        .contains("provider"));
}

#[tokio::test]
async fn session_change_during_inference_drops_text_as_stale() {
    let directory = tempfile::tempdir().expect("tempdir");
    let (project, transcript) = project_with_transcript(CLAUDE_TRANSCRIPT);
    let inputs = vec![agent_input("p1", project.path(), Some(transcript))];
    let reattached: FenceLookup = Arc::new(|pane_id: &str| {
        Some(PaneFence {
            pane_id: pane_id.into(),
            session_id: Some("session-2".into()),
        })
    });
    let provider = FakeProvider::answering(GOOD_ANSWER);
    let batch = run_recaps(
        inputs,
        context(
            provider,
            never_cancelled(),
            reattached,
            vec![project.path().to_path_buf()],
        ),
    )
    .await;
    assert_eq!(batch.panes[0].state, RecapPaneState::Stale);
    assert!(batch.panes[0].short.is_none());
    let _ = directory;
}

#[tokio::test]
async fn vanished_pane_is_stale_not_summarized() {
    let directory = tempfile::tempdir().expect("tempdir");
    let inputs = vec![shell_input("gone", directory.path(), Some("$ ls"))];
    let vanished: FenceLookup = Arc::new(|_: &str| None);
    let provider = FakeProvider::answering(GOOD_ANSWER);
    let batch = run_recaps(
        inputs,
        context(provider, never_cancelled(), vanished, vec![]),
    )
    .await;
    assert_eq!(batch.panes[0].state, RecapPaneState::Stale);
}

#[tokio::test]
async fn cancellation_before_start_returns_labelled_partial_results() {
    let directory = tempfile::tempdir().expect("tempdir");
    let inputs = vec![shell_input("s1", directory.path(), Some("$ ls"))];
    let (sender, receiver) = watch::channel(false);
    sender.send(true).expect("receiver alive");
    let provider = FakeProvider::answering(GOOD_ANSWER);
    let batch = run_recaps(
        inputs.clone(),
        context(provider.clone(), receiver, stable_lookup(&inputs), vec![]),
    )
    .await;
    assert!(batch.cancelled);
    assert_eq!(batch.panes[0].state, RecapPaneState::Failed);
    assert!(batch.panes[0]
        .failure
        .as_deref()
        .unwrap_or_default()
        .contains("cancelled"));
    assert_eq!(provider.calls(), 0);
}

#[tokio::test]
async fn cache_reuses_summarized_text_for_unchanged_evidence_only() {
    let directory = tempfile::tempdir().expect("tempdir");
    let inputs = vec![shell_input("s1", directory.path(), Some("$ cargo test"))];
    let cache = Arc::new(Mutex::new(RecapCache::default()));
    let provider = FakeProvider::answering(GOOD_ANSWER);
    let mut first = context(
        provider.clone(),
        never_cancelled(),
        stable_lookup(&inputs),
        vec![],
    );
    first.cache = cache.clone();
    run_recaps(inputs.clone(), first).await;
    let mut second = context(
        provider.clone(),
        never_cancelled(),
        stable_lookup(&inputs),
        vec![],
    );
    second.cache = cache.clone();
    let batch = run_recaps(inputs.clone(), second).await;
    assert_eq!(batch.panes[0].state, RecapPaneState::Summarized);
    assert_eq!(
        provider.calls(),
        1,
        "unchanged evidence must not call the model again"
    );

    let changed = vec![shell_input("s1", directory.path(), Some("$ cargo build"))];
    let mut third = context(
        provider.clone(),
        never_cancelled(),
        stable_lookup(&changed),
        vec![],
    );
    third.cache = cache;
    run_recaps(changed, third).await;
    assert_eq!(
        provider.calls(),
        2,
        "changed screen evidence must call the model"
    );
}

#[tokio::test]
async fn final_fence_drops_text_when_session_changed_after_inference() {
    let directory = tempfile::tempdir().expect("tempdir");
    let inputs = vec![shell_input("s1", directory.path(), Some("$ make"))];
    let provider = FakeProvider::answering(GOOD_ANSWER);
    let mut batch = run_recaps(
        inputs.clone(),
        context(provider, never_cancelled(), stable_lookup(&inputs), vec![]),
    )
    .await;
    assert_eq!(batch.panes[0].state, RecapPaneState::Summarized);

    let captured: HashMap<String, PaneFence> = inputs
        .iter()
        .map(|input| (input.pane_id.clone(), input.fence.clone()))
        .collect();
    let closed = |_: &str| -> Option<PaneFence> { None };
    super::apply_final_fence(&mut batch, &captured, &closed);
    assert_eq!(batch.panes[0].state, RecapPaneState::Stale);
    assert!(batch.panes[0].short.is_none());
    assert!(batch.panes[0].long.is_none());
}

#[tokio::test]
async fn permission_denied_gives_one_denied_record_per_pane_without_inference() {
    let directory = tempfile::tempdir().expect("tempdir");
    let inputs = vec![shell_input("s1", directory.path(), Some("$ ls"))];
    let batch = super::permission_denied(&inputs, "AI is off");
    assert_eq!(batch.panes[0].state, RecapPaneState::Denied);
    assert_eq!(batch.panes[0].failure.as_deref(), Some("AI is off"));
}
