//! Background execution of the attention LLM calls that are not part of the
//! restructure call: the Turbo worker thread and the bookkeeping of calls in
//! flight.
//!
//! The thread owner is `AttentionState`: it keeps every `TurboHandle`, cancels
//! the superseded ones and reaps the finished ones, so no worker outlives the
//! client unnoticed. Nothing here touches `App`; the reply travels back over
//! the caller's `deliver` closure.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use ilium_core::NodeId;

use super::parse::KnownPane;
use super::queue::LlmReply;
use super::worker::{run_turbo_batch, AttentionCompletionClient, TurboEnvironment, TurboRequest};

/// How long a Turbo worker idles while every pending agent is backing off.
const BACKOFF_PAUSE: Duration = Duration::from_millis(250);

/// One restructure-style call that also asks for the attention list (Shared
/// mode), captured when the call was queued. The reply is parsed against this
/// snapshot, never against whatever the tree looks like when it returns.
#[derive(Debug, Clone)]
pub struct SharedAttentionCall {
    pub generation: u64,
    /// False when only the attention list is wanted and the sort plan from the
    /// same reply must be discarded.
    pub apply_plan: bool,
    pub known: HashMap<NodeId, KnownPane>,
    /// Unix milliseconds when the call was queued, for the give-up timeout.
    pub started_ms: u64,
}

/// How a Shared-mode call ended, as far as the attention list is concerned.
#[derive(Debug, Clone)]
pub enum SharedOutcome {
    /// The call failed or was cancelled: nothing was analysed.
    Failed,
    /// The call succeeded. The raw list is `None` when the model left it out.
    Replied(Option<serde_json::Value>),
}

/// A running (or finishing) Turbo worker thread.
#[derive(Debug)]
pub struct TurboHandle {
    pub generation: u64,
    cancel: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl TurboHandle {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }

    pub fn is_finished(&self) -> bool {
        self.join.as_ref().is_none_or(JoinHandle::is_finished)
    }
}

impl Drop for TurboHandle {
    /// Asks the worker to stop. It is not joined: a provider call in flight
    /// can take minutes and the client must be able to exit meanwhile. The
    /// worker checks the flag between calls and then ends by itself.
    fn drop(&mut self) {
        self.cancel();
    }
}

/// The current Turbo worker plus the cancelled ones still winding down.
#[derive(Debug, Default)]
pub struct TurboWorkers {
    current: Option<TurboHandle>,
    retiring: Vec<TurboHandle>,
}

impl TurboWorkers {
    /// Generation of the worker whose reply may still be applied.
    pub fn current_generation(&self) -> Option<u64> {
        self.current
            .as_ref()
            .filter(|handle| !handle.is_cancelled())
            .map(|handle| handle.generation)
    }

    pub fn is_running(&self) -> bool {
        self.current.is_some()
    }

    /// Cancels the current worker (if any); it is reaped once it has ended.
    pub fn cancel_current(&mut self) {
        if let Some(handle) = self.current.take() {
            handle.cancel();
            self.retiring.push(handle);
        }
    }

    /// Makes `handle` the current worker, cancelling and retiring the old one.
    pub fn supersede(&mut self, handle: TurboHandle) {
        self.cancel_current();
        self.current = Some(handle);
    }

    /// Moves the current worker to the retiring list once its reply arrived.
    pub fn finish_current(&mut self) {
        if let Some(handle) = self.current.take() {
            self.retiring.push(handle);
        }
    }

    /// Joins every retiring worker that has ended. Returns how many are left.
    pub fn reap(&mut self) -> usize {
        let mut index = 0;
        while index < self.retiring.len() {
            if self.retiring[index].is_finished() {
                let mut handle = self.retiring.swap_remove(index);
                if let Some(join) = handle.join.take() {
                    if join.join().is_err() {
                        tracing::warn!("attention turbo worker panicked");
                    }
                }
            } else {
                index += 1;
            }
        }
        self.retiring.len()
    }

    /// Cancels everything; used when the mode leaves Turbo or the client ends.
    pub fn cancel_all(&mut self) {
        self.cancel_current();
        for handle in &self.retiring {
            handle.cancel();
        }
    }
}

/// Starts one Turbo batch on its own thread. `deliver` receives the reply
/// unless the worker was cancelled meanwhile (a superseded batch is stale by
/// definition, so its reply is never sent).
pub fn launch_turbo_worker<C, D>(
    client: C,
    requests: Vec<TurboRequest>,
    already_analysed: HashMap<NodeId, u64>,
    concurrency: usize,
    generation: u64,
    now_ms: fn() -> u64,
    deliver: D,
) -> std::io::Result<TurboHandle>
where
    C: AttentionCompletionClient + Send + 'static,
    D: FnOnce(LlmReply) + Send + 'static,
{
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    let join = std::thread::Builder::new()
        .name("attention-turbo".to_owned())
        .spawn(move || {
            let pause = || std::thread::sleep(BACKOFF_PAUSE);
            let environment = TurboEnvironment {
                cancel: &worker_cancel,
                now_ms: &now_ms,
                pause: &pause,
            };
            let reply = run_turbo_batch(
                &client,
                requests,
                &already_analysed,
                concurrency,
                generation,
                &environment,
            );
            if !worker_cancel.load(Ordering::Acquire) {
                deliver(reply);
            }
        })?;
    Ok(TurboHandle {
        generation,
        cancel,
        join: Some(join),
    })
}
