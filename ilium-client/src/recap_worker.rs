//! Background worker for a manual pane recap (the refresh icon). It reuses the
//! `recap` pipeline of the `ilium recap` command: locate the verified
//! transcript, build bounded evidence, call the provider once and validate
//! both lengths. The client applies the result through `ApplyRecap`; the
//! server never calls a model.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ilium_agent_session::{TranscriptLocator, INTERACTIVE_TRANSCRIPT_LOOKUP_LIMITS};
use ilium_core::{AgentClass, NodeId};
use ilium_inference::InferenceSettings;
use tokio::sync::watch;

use crate::recap::fence::{PaneFence, RecapCache};
use crate::recap::model::{RecapLength, RecapPaneState};
use crate::recap::{
    run_recaps, FenceLookup, PaneRecapInput, RecapContext, RecapOptions, RecapSource,
};

/// Phase reported before the pipeline starts: finding the transcript file.
pub const PHASE_LOCATE: u8 = 1;
/// Phase value that means the result was applied. Only the App sets it.
pub const PHASE_DONE: u8 = 6;

const MAX_CONCURRENCY: usize = 1;

/// Everything the worker needs, captured when the user clicks refresh.
pub struct RecapWorkerRequest {
    pub pane_id: NodeId,
    pub session_id: String,
    pub recap_generation: u64,
    pub class: AgentClass,
    pub project_dir: PathBuf,
    pub home: PathBuf,
    pub activity: String,
    pub progress: Arc<AtomicU8>,
}

/// Both lengths of one finished recap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecapTexts {
    pub short: String,
    pub long: String,
}

pub struct RecapWorkerResult {
    pub pane_id: NodeId,
    pub session_id: String,
    pub recap_generation: u64,
    pub result: Result<RecapTexts, String>,
    pub automatic_ai_decision: crate::naming_workers::AutomaticAiDecision,
}

/// Runs the pipeline for one agent pane. Blocking: the caller is a finite
/// worker job, which owns its thread for the duration.
pub fn run_manual_recap(
    settings: &InferenceSettings,
    request: &RecapWorkerRequest,
    cancelled: bool,
) -> Result<RecapTexts, String> {
    if cancelled {
        return Err("recap cancelled before the provider call".to_string());
    }
    request.progress.fetch_max(PHASE_LOCATE, Ordering::Relaxed);
    let verified_transcript = TranscriptLocator::new_bounded(
        &request.home,
        &request.project_dir,
        INTERACTIVE_TRANSCRIPT_LOOKUP_LIMITS,
    )
    .transcript_for_session(&request.class, &request.session_id)
    .map(|verified| verified.path);

    let pane_key = request.pane_id.0.to_string();
    let fence = PaneFence {
        pane_id: pane_key.clone(),
        session_id: Some(request.session_id.clone()),
    };
    let input = PaneRecapInput {
        pane_id: pane_key.clone(),
        pane_name: String::new(),
        source: RecapSource::Agent {
            class: request.class.clone(),
            verified_transcript,
            project_dir: request.project_dir.clone(),
            activity: request.activity.clone(),
        },
        fence: fence.clone(),
    };
    // The App compares the session again when the result arrives, so the
    // pipeline's own fence only needs to stay consistent within this run.
    let fence_lookup: FenceLookup = Arc::new(move |_: &str| Some(fence.clone()));
    let (cancel_sender, cancel) = watch::channel(false);
    let context = RecapContext {
        progress: Some(Arc::clone(&request.progress)),
        provider: Arc::from(ilium_inference::provider_from_settings(settings)),
        options: RecapOptions {
            lengths: vec![RecapLength::Short, RecapLength::Long],
            max_concurrency: MAX_CONCURRENCY,
            project_roots: vec![request.project_dir.clone()],
        },
        cancel,
        fence_lookup,
        cache: Arc::new(Mutex::new(RecapCache::default())),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("recap runtime failed: {error}"))?;
    let batch = runtime.block_on(run_recaps(vec![input], context));
    drop(cancel_sender);
    let Some(pane) = batch.panes.into_iter().next() else {
        return Err("recap produced no result".to_string());
    };
    match (pane.state, pane.short, pane.long) {
        (RecapPaneState::Summarized, Some(short), Some(long)) => Ok(RecapTexts {
            short: short.text,
            long: long.text,
        }),
        (state, _, _) => Err(pane
            .failure
            .unwrap_or_else(|| format!("recap not available ({state:?})"))),
    }
}

/// Share of the bar each phase represents. The provider wait is the long
/// phase, so its share grows with elapsed time but never reaches the next
/// phase on its own; 100 % only appears once the result is applied.
pub fn phase_percent(phase: u8, provider_wait: Duration) -> u8 {
    match phase {
        0 => 0,
        PHASE_LOCATE => 8,
        2 => 20,
        3 => 30,
        4 => {
            let waited = provider_wait.as_secs().min(60) as u8;
            35 + waited / 2
        }
        5 => 90,
        _ => 100,
    }
}

/// One-line label of the running phase.
pub fn phase_label(phase: u8) -> &'static str {
    match phase {
        0 | PHASE_LOCATE => "Locating transcript",
        2 => "Extracting evidence",
        3 => "Rendering prompt",
        4 => "Waiting for the model",
        5 => "Validating answer",
        _ => "Done",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_is_monotonic_and_hits_100_only_when_done() {
        let waits = Duration::from_secs(0);
        let mut previous = 0;
        for phase in 0..=PHASE_DONE {
            let percent = phase_percent(phase, waits);
            assert!(percent >= previous, "phase {phase}");
            previous = percent;
        }
        assert_eq!(phase_percent(PHASE_DONE, waits), 100);
        assert!(phase_percent(4, Duration::from_secs(3_600)) < 90);
        assert!(phase_percent(5, waits) < 100);
    }

    #[test]
    fn cancelled_request_never_reads_or_calls_the_model() {
        let request = RecapWorkerRequest {
            pane_id: NodeId(1),
            session_id: "s".into(),
            recap_generation: 1,
            class: AgentClass::Claude,
            project_dir: std::env::temp_dir(),
            home: std::env::temp_dir(),
            activity: String::new(),
            progress: Arc::new(AtomicU8::new(0)),
        };
        let result = run_manual_recap(&InferenceSettings::default(), &request, true);
        assert!(result.is_err());
        assert_eq!(request.progress.load(Ordering::Relaxed), 0);
    }
}
