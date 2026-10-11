//! Bounded inference for one pane: prompt rendering, concurrency gate,
//! per-call timeout and cancellation.
//!
//! The provider call is blocking, so it runs on the blocking pool. Dropping the
//! join handle on timeout or cancel does not stop that thread; the request
//! carries the same timeout, so the thread ends with its HTTP call.

use std::sync::Arc;
use std::time::Duration;

use ilium_inference::{InferenceProvider, InferenceRequest};
use serde::Serialize;
use tokio::sync::{watch, Semaphore};

use super::model::PaneRecapFacts;

/// Output ceiling for one recap call. Length is controlled by the prompt
/// (word limits are checked on the answer), not by a smaller cap.
pub const MAX_OUTPUT_TOKENS: u32 = 2_048;

pub const SYSTEM_PROMPT: &str =
    "You write factual recaps of terminal panes. Output only the requested JSON object.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferenceFailure {
    Cancelled,
    TimedOut,
    Provider(String),
    Prompt(String),
}

impl std::fmt::Display for InferenceFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(formatter, "cancelled"),
            Self::TimedOut => write!(formatter, "model call timed out"),
            Self::Provider(detail) => write!(formatter, "model provider failed: {detail}"),
            Self::Prompt(detail) => write!(formatter, "prompt could not be rendered: {detail}"),
        }
    }
}

#[derive(Serialize)]
struct PromptContext<'a> {
    activity: &'a str,
    evidence_kind: &'a str,
    evidence_age_seconds: String,
    evidence_stale: String,
    goals_json: &'a str,
    evidence_json: &'a str,
}

/// Renders the single recap prompt from the catalog template.
pub fn render_recap_prompt(
    facts: &PaneRecapFacts,
    activity: &str,
    evidence_kind: &str,
    goals_json: &str,
    evidence_json: &str,
) -> Result<String, InferenceFailure> {
    let context = PromptContext {
        activity,
        evidence_kind,
        evidence_age_seconds: facts
            .evidence_age_seconds
            .map_or_else(|| "unknown".to_string(), |seconds| seconds.to_string()),
        evidence_stale: facts.evidence_stale.to_string(),
        goals_json,
        evidence_json,
    };
    ilium_prompts::render_source("recap", ilium_prompts::recap::RECAP, &context)
        .map_err(|error| InferenceFailure::Prompt(error.to_string()))
}

/// Runs one completion under the concurrency gate, timeout and cancellation.
/// `cancel` turns `true` to abandon the wait; the sender dropping leaves the
/// call uncancelled.
pub async fn complete_bounded(
    provider: Arc<dyn InferenceProvider>,
    gate: Arc<Semaphore>,
    prompt: String,
    timeout: Duration,
    mut cancel: watch::Receiver<bool>,
) -> Result<String, InferenceFailure> {
    if *cancel.borrow() {
        return Err(InferenceFailure::Cancelled);
    }
    let permit = tokio::select! {
        biased;
        _ = wait_for_cancel(&mut cancel) => return Err(InferenceFailure::Cancelled),
        permit = gate.acquire_owned() => permit.map_err(|error| InferenceFailure::Provider(error.to_string()))?,
    };
    let request = InferenceRequest {
        system_prompt: SYSTEM_PROMPT.to_string(),
        user_prompt: prompt,
        max_tokens: MAX_OUTPUT_TOKENS,
        timeout: Some(timeout),
    };
    // The permit moves into the blocking closure: a timed-out or cancelled
    // caller returns at once, but the slot stays held until `complete` really
    // returns. The provider call carries `timeout`, so that return is bounded.
    let call = tokio::task::spawn_blocking(move || {
        let _held_until_provider_returns = permit;
        provider.complete(&request)
    });
    tokio::select! {
        biased;
        _ = wait_for_cancel(&mut cancel) => Err(InferenceFailure::Cancelled),
        joined = tokio::time::timeout(timeout, call) => match joined {
            Err(_) => Err(InferenceFailure::TimedOut),
            Ok(Err(join_error)) => Err(InferenceFailure::Provider(join_error.to_string())),
            Ok(Ok(Err(provider_error))) => Err(InferenceFailure::Provider(provider_error.to_string())),
            Ok(Ok(Ok(response))) => Ok(response.text),
        },
    }
}

/// Resolves once cancellation is requested. Without a live sender it never
/// resolves, so the other branch decides.
async fn wait_for_cancel(cancel: &mut watch::Receiver<bool>) {
    loop {
        if *cancel.borrow() {
            return;
        }
        if cancel.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_inference::{InferenceError, InferenceProviderKind, InferenceResponse};

    struct FixedProvider(Result<String, String>);

    impl InferenceProvider for FixedProvider {
        fn kind(&self) -> InferenceProviderKind {
            InferenceProviderKind::KiloGateway
        }

        fn complete(
            &self,
            _request: &InferenceRequest,
        ) -> Result<InferenceResponse, InferenceError> {
            match &self.0 {
                Ok(text) => Ok(InferenceResponse { text: text.clone() }),
                Err(detail) => Err(InferenceError::Configuration(detail.clone())),
            }
        }
    }

    struct SlowProvider;

    impl InferenceProvider for SlowProvider {
        fn kind(&self) -> InferenceProviderKind {
            InferenceProviderKind::KiloGateway
        }

        fn complete(
            &self,
            _request: &InferenceRequest,
        ) -> Result<InferenceResponse, InferenceError> {
            std::thread::sleep(Duration::from_millis(400));
            Ok(InferenceResponse {
                text: "late".into(),
            })
        }
    }

    fn open_cancel() -> (watch::Sender<bool>, watch::Receiver<bool>) {
        watch::channel(false)
    }

    #[tokio::test]
    async fn returns_provider_text_on_success() {
        let (_sender, receiver) = open_cancel();
        let text = complete_bounded(
            Arc::new(FixedProvider(Ok("answer".into()))),
            Arc::new(Semaphore::new(1)),
            "prompt".into(),
            Duration::from_secs(5),
            receiver,
        )
        .await;
        assert_eq!(text.as_deref(), Ok("answer"));
    }

    #[tokio::test]
    async fn reports_provider_failure_without_panicking() {
        let (_sender, receiver) = open_cancel();
        let result = complete_bounded(
            Arc::new(FixedProvider(Err("no key".into()))),
            Arc::new(Semaphore::new(1)),
            "prompt".into(),
            Duration::from_secs(5),
            receiver,
        )
        .await;
        assert!(matches!(result, Err(InferenceFailure::Provider(_))));
    }

    #[tokio::test]
    async fn times_out_slow_calls() {
        let (_sender, receiver) = open_cancel();
        let result = complete_bounded(
            Arc::new(SlowProvider),
            Arc::new(Semaphore::new(1)),
            "prompt".into(),
            Duration::from_millis(50),
            receiver,
        )
        .await;
        assert_eq!(result, Err(InferenceFailure::TimedOut));
    }

    #[tokio::test]
    async fn cancellation_abandons_waiting_call() {
        let (sender, receiver) = open_cancel();
        let handle = tokio::spawn(complete_bounded(
            Arc::new(SlowProvider),
            Arc::new(Semaphore::new(1)),
            "prompt".into(),
            Duration::from_secs(5),
            receiver,
        ));
        tokio::time::sleep(Duration::from_millis(30)).await;
        sender.send(true).expect("receiver alive");
        assert_eq!(
            handle.await.expect("join"),
            Err(InferenceFailure::Cancelled)
        );
    }

    #[tokio::test]
    async fn concurrency_gate_bounds_parallel_calls() {
        let gate = Arc::new(Semaphore::new(1));
        let (_sender, receiver) = open_cancel();
        let held = gate.clone().acquire_owned().await.expect("permit");
        let waiting = tokio::spawn(complete_bounded(
            Arc::new(FixedProvider(Ok("answer".into()))),
            gate.clone(),
            "prompt".into(),
            Duration::from_secs(5),
            receiver,
        ));
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(!waiting.is_finished());
        drop(held);
        assert_eq!(waiting.await.expect("join").as_deref(), Ok("answer"));
    }

    /// Counts provider calls that are still executing, including calls whose
    /// caller already gave up on timeout or cancellation.
    struct CountingProvider {
        active: Arc<std::sync::atomic::AtomicUsize>,
        peak: Arc<std::sync::atomic::AtomicUsize>,
        call_duration: Duration,
    }

    impl InferenceProvider for CountingProvider {
        fn kind(&self) -> InferenceProviderKind {
            InferenceProviderKind::KiloGateway
        }

        fn complete(
            &self,
            _request: &InferenceRequest,
        ) -> Result<InferenceResponse, InferenceError> {
            use std::sync::atomic::Ordering;
            let now_active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(now_active, Ordering::SeqCst);
            std::thread::sleep(self.call_duration);
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(InferenceResponse {
                text: "done".into(),
            })
        }
    }

    #[tokio::test]
    async fn outstanding_provider_calls_never_exceed_gate_after_timeouts_and_cancels() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        const CONCURRENCY_LIMIT: usize = 2;
        const ROUNDS: usize = 12;
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let provider: Arc<dyn InferenceProvider> = Arc::new(CountingProvider {
            active: active.clone(),
            peak: peak.clone(),
            call_duration: Duration::from_millis(150),
        });
        let gate = Arc::new(Semaphore::new(CONCURRENCY_LIMIT));

        let mut callers = Vec::new();
        for round in 0..ROUNDS {
            let (sender, receiver) = open_cancel();
            callers.push(tokio::spawn(complete_bounded(
                provider.clone(),
                gate.clone(),
                "prompt".into(),
                Duration::from_millis(20),
                receiver,
            )));
            // Every other round is cancelled while its provider call is still running.
            if round % 2 == 1 {
                tokio::time::sleep(Duration::from_millis(5)).await;
                sender.send(true).expect("receiver alive");
            }
        }
        for caller in callers {
            let result = caller.await.expect("join");
            assert!(
                matches!(
                    result,
                    Err(InferenceFailure::TimedOut) | Err(InferenceFailure::Cancelled)
                ),
                "unexpected result {result:?}"
            );
        }

        assert!(
            peak.load(Ordering::SeqCst) <= CONCURRENCY_LIMIT,
            "peak outstanding provider calls {} exceeded gate {}",
            peak.load(Ordering::SeqCst),
            CONCURRENCY_LIMIT
        );
        // Cleanup is bounded: abandoned calls end on their own, so the gate
        // drains within a few call durations, not never.
        let drain_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        // The permit drops just after the provider returns, so wait for both.
        while active.load(Ordering::SeqCst) > 0 || gate.available_permits() < CONCURRENCY_LIMIT {
            assert!(
                tokio::time::Instant::now() < drain_deadline,
                "abandoned provider calls did not release their slots"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}
