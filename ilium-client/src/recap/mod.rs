//! Pane recaps: one shared pipeline for the CLI and future UI consumers.
//!
//! Flow per pane: gather bounded, redacted evidence (agent transcript with a
//! verified session, or shell screen text) → deterministic state and goal
//! facts → one model call producing both lengths → validate the answer and
//! its pointers → drop the text if the pane's session changed meanwhile.
//! Goals and states come from evidence; the model writes prose only.

pub mod custody;
pub mod evidence;
pub mod fence;
pub mod infer;
pub mod model;
pub mod pointers;
pub mod render;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use ilium_core::AgentClass;
use ilium_inference::InferenceProvider;
use serde::Serialize;
use tokio::sync::{watch, Semaphore};

use custody::denied_path_reason;
use evidence::{
    build_agent_digest, build_screen_digest, read_transcript_window, AgentDigest, GoalRecord,
    HEAD_WINDOW_BYTES, MAX_LINE_BYTES, TAIL_WINDOW_BYTES,
};
use fence::{evidence_fingerprint, fence_still_current, CacheKey, PaneFence, RecapCache};
use infer::{complete_bounded, render_recap_prompt};
use model::{
    parse_recap_response, PaneRecap, PaneRecapFacts, RecapLength, RecapPaneState, RecapText,
    LONG_TIMEOUT_SECONDS, SHORT_TIMEOUT_SECONDS,
};
use pointers::validate_pointers;

/// Transcript older than this is reported as stale evidence.
pub const EVIDENCE_STALE_SECONDS: u64 = 600;

/// What the caller collected for one pane. The caller owns IPC and locating
/// files; this module owns reading, digesting and inference.
#[derive(Debug, Clone)]
pub enum RecapSource {
    Agent {
        class: AgentClass,
        /// Transcript path, present only when the project and session were
        /// verified by the caller.
        verified_transcript: Option<PathBuf>,
        /// Project directory the agent works in; protected locations are denied.
        project_dir: PathBuf,
        activity: String,
    },
    Shell {
        cwd: PathBuf,
        screen_text: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct PaneRecapInput {
    pub pane_id: String,
    pub pane_name: String,
    pub source: RecapSource,
    pub fence: PaneFence,
}

#[derive(Debug, Clone)]
pub struct RecapOptions {
    pub lengths: Vec<RecapLength>,
    pub max_concurrency: usize,
    /// Directories a pointer may resolve under (project root, then cwd).
    pub project_roots: Vec<PathBuf>,
}

/// Pane lookup used by the stale fence. Returns `None` when the pane is gone.
pub type FenceLookup = Arc<dyn Fn(&str) -> Option<PaneFence> + Send + Sync>;

pub struct RecapContext {
    pub provider: Arc<dyn InferenceProvider>,
    pub options: RecapOptions,
    pub cancel: watch::Receiver<bool>,
    pub fence_lookup: FenceLookup,
    pub cache: Arc<Mutex<RecapCache>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecapBatch {
    pub panes: Vec<PaneRecap>,
    pub cancelled: bool,
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

fn empty_facts() -> PaneRecapFacts {
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

/// JSON for the prompt. `<` is escaped so evidence can never close the
/// `<evidence>` wrapper in the template.
fn json_for_prompt(value: &impl Serialize) -> String {
    serde_json::to_string(value)
        .unwrap_or_else(|_| "null".to_string())
        .replace('<', "\\u003c")
}

fn goals_value(goals: &[GoalRecord]) -> serde_json::Value {
    serde_json::Value::Array(
        goals
            .iter()
            .map(
                |goal| serde_json::json!({"objective": goal.objective, "state": goal.state.name()}),
            )
            .collect(),
    )
}

fn transcript_age_seconds(path: &Path) -> Option<u64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    SystemTime::now()
        .duration_since(modified)
        .ok()
        .map(|elapsed| elapsed.as_secs())
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
    let evidence_json = json_for_prompt(&serde_json::json!({
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

/// Reads and digests evidence. Blocking: call from the blocking pool.
fn gather_evidence(source: RecapSource) -> Gathered {
    match source {
        RecapSource::Agent {
            class,
            verified_transcript,
            project_dir,
            activity,
        } => {
            if let Some(reason) = denied_path_reason(&project_dir) {
                return blocked(RecapPaneState::Denied, reason);
            }
            let Some(path) = verified_transcript else {
                return blocked(
                    RecapPaneState::Unavailable,
                    "no verified transcript for this agent session; screen text is not used for agents",
                );
            };
            if !matches!(class, AgentClass::Claude | AgentClass::Codex) {
                return blocked(
                    RecapPaneState::Unavailable,
                    "no transcript reader for this agent",
                );
            }
            let age = transcript_age_seconds(&path);
            match read_transcript_window(
                &path,
                HEAD_WINDOW_BYTES,
                TAIL_WINDOW_BYTES,
                MAX_LINE_BYTES,
            ) {
                Ok(window) => {
                    finish_agent_digest(build_agent_digest(&class, &window), activity, age)
                }
                Err(error) => blocked(
                    RecapPaneState::Failed,
                    format!("transcript unreadable: {error}"),
                ),
            }
        }
        RecapSource::Shell { cwd, screen_text } => {
            if let Some(reason) = denied_path_reason(&cwd) {
                return blocked(RecapPaneState::Denied, reason);
            }
            let Some(screen_text) = screen_text else {
                return blocked(
                    RecapPaneState::Unavailable,
                    "current terminal screen unavailable",
                );
            };
            let digest = build_screen_digest(&screen_text);
            if digest.is_empty() {
                return blocked(RecapPaneState::Idle, "no visible screen text");
            }
            Gathered::Ready(ReadyEvidence {
                facts: PaneRecapFacts {
                    goals: Vec::new(),
                    goals_omitted: 0,
                    evidence_age_seconds: None,
                    evidence_stale: false,
                    window_truncated: false,
                    redactions: digest.redactions,
                },
                activity: "shell".to_string(),
                evidence_kind: "shell_screen",
                goals_json: "[]".to_string(),
                evidence_json: json_for_prompt(&serde_json::json!({"screen": digest.screen})),
            })
        }
    }
}

fn pane_without_text(
    input: &PaneRecapInput,
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

async fn recap_pane(
    input: PaneRecapInput,
    context: Arc<RecapContext>,
    gate: Arc<Semaphore>,
) -> PaneRecap {
    let captured = input.fence.clone();
    let pane_id = input.pane_id.clone();
    let source = input.source.clone();
    let gathered = match tokio::task::spawn_blocking(move || gather_evidence(source)).await {
        Ok(gathered) => gathered,
        Err(error) => blocked(
            RecapPaneState::Failed,
            format!("evidence task failed: {error}"),
        ),
    };
    let ready = match gathered {
        Gathered::Blocked {
            state,
            reason,
            facts,
        } => {
            return pane_without_text(&input, state, facts, Some(reason));
        }
        Gathered::Ready(ready) => ready,
    };

    let evidence_text = format!("{}\n{}", ready.goals_json, ready.evidence_json);
    let key = CacheKey {
        pane_id: pane_id.clone(),
        evidence_fingerprint: evidence_fingerprint(&evidence_text),
    };
    let cached = context
        .cache
        .lock()
        .ok()
        .and_then(|cache| cache.get(&key).cloned());
    if let Some(cached) = cached {
        if fence_still_current(&captured, (context.fence_lookup)(&pane_id).as_ref()) {
            return cached;
        }
    }

    let prompt = match render_recap_prompt(
        &ready.facts,
        &ready.activity,
        ready.evidence_kind,
        &ready.goals_json,
        &ready.evidence_json,
    ) {
        Ok(prompt) => prompt,
        Err(error) => {
            return pane_without_text(
                &input,
                RecapPaneState::Failed,
                ready.facts,
                Some(error.to_string()),
            )
        }
    };
    let timeout = if context.options.lengths.contains(&RecapLength::Long) {
        Duration::from_secs(LONG_TIMEOUT_SECONDS)
    } else {
        Duration::from_secs(SHORT_TIMEOUT_SECONDS)
    };
    let raw = match complete_bounded(
        context.provider.clone(),
        gate,
        prompt,
        timeout,
        context.cancel.clone(),
    )
    .await
    {
        Ok(raw) => raw,
        Err(failure) => {
            let state = RecapPaneState::Failed;
            return pane_without_text(&input, state, ready.facts, Some(failure.to_string()));
        }
    };
    let parsed = match parse_recap_response(&raw) {
        Ok(parsed) => parsed,
        Err(error) => {
            return pane_without_text(
                &input,
                RecapPaneState::Failed,
                ready.facts,
                Some(error.to_string()),
            );
        }
    };

    if !fence_still_current(&captured, (context.fence_lookup)(&pane_id).as_ref()) {
        return pane_without_text(
            &input,
            RecapPaneState::Stale,
            ready.facts,
            Some("pane or session changed while the recap was running".to_string()),
        );
    }

    let roots = &context.options.project_roots;
    let short = attach_pointers(parsed.short, &evidence_text, roots);
    let long = attach_pointers(parsed.long, &evidence_text, roots);
    let recap = PaneRecap {
        pane_id: pane_id.clone(),
        pane_name: input.pane_name.clone(),
        state: RecapPaneState::Summarized,
        facts: ready.facts,
        short: Some(short),
        long: Some(long),
        failure: None,
    };
    if let Ok(mut cache) = context.cache.lock() {
        cache.store(key, recap.clone());
    }
    recap
}

fn attach_pointers(text: RecapText, evidence_text: &str, roots: &[PathBuf]) -> RecapText {
    let outcome = validate_pointers(text.pointers, evidence_text, roots);
    RecapText {
        text: text.text,
        pointers: outcome.accepted,
        rejected_pointers: text.rejected_pointers + outcome.rejected,
    }
}

/// Runs recaps for all inputs. Concurrency is bounded by the semaphore in
/// `complete_bounded`; each pane's result is independent, so one failure never
/// removes another pane's text.
pub async fn run_recaps(inputs: Vec<PaneRecapInput>, context: RecapContext) -> RecapBatch {
    let gate = Arc::new(Semaphore::new(context.options.max_concurrency.max(1)));
    let context = Arc::new(context);
    let mut handles = Vec::with_capacity(inputs.len());
    for input in &inputs {
        let identity = (input.pane_id.clone(), input.pane_name.clone());
        let handle = tokio::spawn(recap_pane(input.clone(), context.clone(), gate.clone()));
        handles.push((identity, handle));
    }
    let mut panes = Vec::with_capacity(handles.len());
    for ((pane_id, pane_name), handle) in handles {
        let pane = match handle.await {
            Ok(pane) => pane,
            Err(error) => PaneRecap {
                pane_id,
                pane_name,
                state: RecapPaneState::Failed,
                facts: empty_facts(),
                short: None,
                long: None,
                failure: Some(format!("pane task failed: {error}")),
            },
        };
        panes.push(pane);
    }
    let cancelled = *context.cancel.borrow();
    RecapBatch { panes, cancelled }
}

/// Final identity check for callers that run the pipeline on an attach-time
/// snapshot (the CLI). Drops text for each pane whose session changed or
/// vanished while inference ran; `latest` reports the pane's identity now.
pub fn apply_final_fence(
    batch: &mut RecapBatch,
    captured: &HashMap<String, PaneFence>,
    latest: &dyn Fn(&str) -> Option<PaneFence>,
) {
    for pane in &mut batch.panes {
        if pane.state != RecapPaneState::Summarized {
            continue;
        }
        let Some(captured_fence) = captured.get(&pane.pane_id) else {
            continue;
        };
        if !fence_still_current(captured_fence, latest(&pane.pane_id).as_ref()) {
            pane.state = RecapPaneState::Stale;
            pane.short = None;
            pane.long = None;
            pane.failure = Some("pane or session changed while the recap was running".to_string());
        }
    }
}

/// A batch where every pane is denied without reading evidence or calling the
/// model. Used when AI permission is off, so the caller still gets one record
/// per selected pane.
pub fn permission_denied(inputs: &[PaneRecapInput], reason: &str) -> RecapBatch {
    RecapBatch {
        panes: inputs
            .iter()
            .map(|input| PaneRecap {
                pane_id: input.pane_id.clone(),
                pane_name: input.pane_name.clone(),
                state: RecapPaneState::Denied,
                facts: empty_facts(),
                short: None,
                long: None,
                failure: Some(reason.to_string()),
            })
            .collect(),
        cancelled: false,
    }
}

#[cfg(test)]
mod pipeline_tests;
