//! Pane-owned follow-up work that must never hold up the detection sweep.
//!
//! One watch value coalesces each kind of request while its owner is busy.
//! There is no growing task list, and waiting actors hold only weak state.

use std::sync::{Arc, Weak};

use ilium_core::NodeId;
use ilium_pty::PtyInput;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::pane::PaneResource;
use crate::state::ServerState;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Requests {
    title_checks: u64,
    completions: u64,
    completion_epoch: u64,
}

impl Requests {
    fn request(&mut self, check_title: bool, deliver_prompt: bool, completion_epoch: u64) {
        if check_title {
            self.title_checks = self.title_checks.wrapping_add(1);
        }
        if deliver_prompt {
            self.completions = self.completions.wrapping_add(1);
            self.completion_epoch = completion_epoch;
        }
    }
}

/// A completion authorizes at most one queued submission. Changing turn or
/// invocation invalidates pending and claimed authorizations alike.
#[derive(Default)]
pub(crate) struct CompletionEpoch {
    current: u64,
    claimed: Option<u64>,
}

impl CompletionEpoch {
    pub(crate) fn current(&self) -> u64 {
        self.current
    }

    pub(crate) fn invalidate(&mut self) {
        self.current = self.current.wrapping_add(1);
        self.claimed = None;
    }

    pub(crate) fn retire_submission(&mut self) {
        self.current = self.current.wrapping_add(1);
        // Merely admitting Enter must not authorize another queue head while
        // the detector still shows Idle. Only a new observed turn reopens it.
        self.claimed = Some(self.current);
    }

    pub(crate) fn is_available(&self, expected: u64) -> bool {
        self.current == expected && self.claimed != Some(expected)
    }

    pub(crate) fn claim(&mut self, expected: u64) -> bool {
        if !self.is_available(expected) {
            return false;
        }
        self.claimed = Some(expected);
        true
    }

    pub(crate) fn is_claimed_current(&self, expected: u64) -> bool {
        self.current == expected && self.claimed == Some(expected)
    }
}

pub(crate) struct DetectionEffects {
    generation: u64,
    input: PtyInput,
    requests: watch::Sender<Requests>,
    task: JoinHandle<()>,
}

impl DetectionEffects {
    pub(crate) fn start(
        state: &Arc<ServerState>,
        pane_id: NodeId,
        generation: u64,
        input: PtyInput,
    ) -> Self {
        let (requests, receiver) = watch::channel(Requests::default());
        let task = tokio::spawn(run(
            Arc::downgrade(state),
            pane_id,
            generation,
            input.clone(),
            receiver,
        ));
        Self {
            generation,
            input,
            requests,
            task,
        }
    }

    pub(crate) fn matches(&self, generation: u64, input: &PtyInput) -> bool {
        !self.task.is_finished() && self.generation == generation && self.input.same_session(input)
    }

    pub(crate) fn request(&self, check_title: bool, deliver_prompt: bool, completion_epoch: u64) {
        if check_title || deliver_prompt {
            self.requests.send_modify(|requests| {
                requests.request(check_title, deliver_prompt, completion_epoch)
            });
        }
    }
}

impl Drop for DetectionEffects {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn still_current(
    state: &ServerState,
    pane_id: NodeId,
    generation: u64,
    input: &PtyInput,
) -> bool {
    let panes = state.panes.read().await;
    matches!(panes.get(&pane_id), Some(PaneResource::Terminal(runtime))
        if runtime.agent_generation == generation
            && runtime.session.input_handle().same_session(input))
}

async fn run(
    state: Weak<ServerState>,
    pane_id: NodeId,
    generation: u64,
    input: PtyInput,
    mut requests: watch::Receiver<Requests>,
) {
    let mut handled = Requests::default();
    while requests.changed().await.is_ok() {
        // Borrow only long enough to copy counters. Requests during an await
        // remain unseen in the receiver and trigger the next iteration.
        let pending = *requests.borrow_and_update();
        let Some(state) = state.upgrade() else {
            return;
        };
        if !still_current(&state, pane_id, generation, &input).await {
            return;
        }
        if pending.completions != handled.completions {
            crate::prompt_queue::deliver_detection_completion(
                &state,
                pane_id,
                generation,
                &input,
                pending.completion_epoch,
            )
            .await;
        }
        // Prompt delivery may wait for persistence or input. Revalidate the
        // invocation independently before another effect can mutate titles.
        if pending.title_checks != handled.title_checks {
            if !still_current(&state, pane_id, generation, &input).await {
                return;
            }
            crate::ipc::handlers::reconcile_empty_agent_titles(&state, &[pane_id]).await;
        }
        handled = pending;
        // The strong state reference ends here, before waiting for a request.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn busy_effect_mailbox_coalesces_both_request_kinds_without_losing_latest() {
        let (sender, mut receiver) = watch::channel(Requests::default());
        sender.send_modify(|requests| requests.request(true, false, 7));
        receiver.changed().await.expect("first request");
        let in_flight = *receiver.borrow_and_update();
        assert_eq!(in_flight.title_checks, 1);
        assert_eq!(in_flight.completions, 0);

        // Model a blocked follow-up: the producer can write arbitrarily many
        // times without allocating requests or waiting for the consumer.
        for _ in 0..10_000 {
            sender.send_modify(|requests| requests.request(true, false, 7));
        }
        sender.send_modify(|requests| requests.request(false, true, 7));
        sender.send_modify(|requests| requests.request(true, false, 7));
        receiver.changed().await.expect("coalesced pending request");
        let pending = *receiver.borrow_and_update();
        assert_eq!(pending.title_checks, 10_002);
        assert_eq!(pending.completions, 1);
        assert_eq!(pending.completion_epoch, 7);
        assert_ne!(pending.title_checks, in_flight.title_checks);
        assert_ne!(pending.completions, in_flight.completions);
        assert!(!receiver.has_changed().expect("sender remains alive"));
        drop(sender);
        assert!(receiver.changed().await.is_err());
    }

    #[tokio::test]
    async fn completion_waiting_for_durability_cannot_claim_a_newer_turn() {
        let epoch = Arc::new(tokio::sync::Mutex::new(CompletionEpoch::default()));
        let captured = epoch.lock().await.current();
        let (barrier_entered, waiting_at_barrier) = tokio::sync::oneshot::channel();
        let (release_barrier, barrier) = tokio::sync::oneshot::channel();
        let delayed = Arc::clone(&epoch);
        let task = tokio::spawn(async move {
            barrier_entered.send(()).expect("announce durability wait");
            barrier.await.expect("durability result");
            delayed.lock().await.claim(captured)
        });
        waiting_at_barrier
            .await
            .expect("completion reached barrier");
        epoch.lock().await.invalidate();
        release_barrier.send(()).expect("complete durability wait");
        assert!(!task.await.expect("completion claim"));
        let current = epoch.lock().await;
        assert!(!current.is_claimed_current(captured));
        assert!(current.is_available(current.current()));
    }

    #[test]
    fn repeated_completion_requests_authorize_only_one_queued_prompt_per_turn() {
        let mut epoch = CompletionEpoch::default();
        let completed = epoch.current();
        assert!(epoch.claim(completed));
        assert!(epoch.is_claimed_current(completed));
        // A second coalesced notification cannot advance the next queue head
        // before Working (or another detector transition) starts a new epoch.
        assert!(!epoch.claim(completed));
        epoch.retire_submission();
        assert!(!epoch.is_claimed_current(completed));
        assert!(!epoch.claim(epoch.current()));
        epoch.invalidate();
        assert!(!epoch.is_claimed_current(completed));
        assert!(!epoch.claim(completed));
        assert!(epoch.claim(epoch.current()));
    }
}
