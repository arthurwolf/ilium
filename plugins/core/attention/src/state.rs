//! The client's attention state: queue, settings, refresh bookkeeping and persistence timing.
//!
//! Everything here is a plain function of its inputs (the live panes, the clock, a server reply),
//! so the rules are tested without a host. `client.rs` feeds it from the host.

use std::collections::HashMap;

use ilium_plugin_sdk::types::NodeId;
use serde_json::{json, Value};

use crate::instant::{derive_instant_item, LivePane};
use crate::item::{AttentionItem, AttentionKind, AttentionSource, Briefing};
use crate::queue::{AttentionQueue, LiveFingerprints, PersistedQueue};
use crate::refresh::{RefreshDecision, RefreshGate};
use crate::runner::reply_from_json;
use crate::settings::{AttentionMode, AttentionSettings};

/// Minimum time between two model refreshes started by the tick.
pub const REFRESH_MINIMUM_INTERVAL_MS: u64 = 10_000;
/// A refresh with no reply after this long is given up on, so a lost reply can never keep the
/// refresh gate closed.
pub const REFRESH_TIMEOUT_MS: u64 = 10 * 60_000;
/// A changed queue is written after it has been stable for this long.
pub const SAVE_DEBOUNCE_MS: u64 = 2_000;
/// A stored queue that no live pane has justified after this long is dropped.
pub const RESTORE_GRACE_MS: u64 = 20_000;
/// Priority of a plugin's attention mark: above "finished", below questions and blocked goals.
pub const PLUGIN_MARK_PRIORITY: u8 = 50;

/// What one reconcile pass changed.
#[derive(Debug, Default, PartialEq)]
pub struct Reconciled {
    /// The visible queue changed.
    pub changed: bool,
    /// A pane other than the one on screen gained an item kind it did not have before.
    pub arrival: Option<AttentionItem>,
}

pub struct State {
    pub settings: AttentionSettings,
    pub queue: AttentionQueue,
    gate: RefreshGate,
    refresh_started_ms: Option<u64>,
    /// The pane the user is looking at.
    pub focused: Option<NodeId>,
    /// Panes whose `activity-detail` signal says they wait for background tasks.
    waiting_background: HashMap<NodeId, bool>,
    /// The live panes of the last reconcile, for the replies that arrive later.
    last_panes: Vec<LivePane>,
    pending_restore: Option<PersistedQueue>,
    restore_deadline_ms: Option<u64>,
    last_saved: Option<PersistedQueue>,
    dirty_since_ms: Option<u64>,
    known_kinds: HashMap<NodeId, AttentionKind>,
    is_first_pass: bool,
    /// The selected row of the panel and the overview.
    pub selected: usize,
    /// The last status text sent to the chrome, so an unchanged one is not sent again.
    pub shown_status: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl State {
    pub fn new() -> Self {
        Self {
            settings: AttentionSettings::default(),
            queue: AttentionQueue::new(),
            gate: RefreshGate::new(REFRESH_MINIMUM_INTERVAL_MS),
            refresh_started_ms: None,
            focused: None,
            waiting_background: HashMap::new(),
            last_panes: Vec::new(),
            pending_restore: None,
            restore_deadline_ms: None,
            last_saved: None,
            dirty_since_ms: None,
            known_kinds: HashMap::new(),
            is_first_pass: true,
            selected: 0,
            shown_status: None,
        }
    }

    /// Applies the plugin's settings document. Returns whether the mode changed.
    pub fn apply_settings(&mut self, json_text: &str) -> bool {
        let next = AttentionSettings::from_json(json_text);
        let mode_changed = next.mode != self.settings.mode;
        self.settings = next;
        if mode_changed && !self.settings.enabled() {
            self.queue.clear();
            self.known_kinds.clear();
        }
        mode_changed
    }

    /// Records one `activity-detail` signal; an empty value means the pane runs no agent.
    pub fn observe_detail(&mut self, pane: NodeId, value: &str) {
        if value.is_empty() {
            self.waiting_background.remove(&pane);
        } else {
            self.waiting_background
                .insert(pane, value == "waiting-background");
        }
    }

    pub fn is_waiting_background(&self, pane: NodeId) -> bool {
        self.waiting_background.get(&pane).copied().unwrap_or(false)
    }

    pub fn forget_pane(&mut self, pane: NodeId) {
        self.waiting_background.remove(&pane);
    }

    /// Hands over the stored queue; it is adopted once live panes can vouch for it.
    pub fn set_pending_restore(&mut self, stored: PersistedQueue) {
        self.pending_restore = Some(stored);
    }

    fn live_fingerprints(panes: &[LivePane]) -> LiveFingerprints {
        panes
            .iter()
            .map(|pane| (pane.pane_id, pane.fingerprint()))
            .collect()
    }

    /// Adopts the stored queue once live panes can vouch for it; gives up after a grace period so
    /// a stale store cannot linger.
    fn restore_pending(&mut self, live: &LiveFingerprints, now_ms: u64) {
        let Some(pending) = self.pending_restore.as_ref() else {
            return;
        };
        if live.is_empty() {
            return;
        }
        let deadline = *self
            .restore_deadline_ms
            .get_or_insert(now_ms + RESTORE_GRACE_MS);
        let restored = AttentionQueue::from_persisted(pending.clone(), live);
        if !restored.to_persisted().items.is_empty() {
            self.last_saved = Some(pending.clone());
            self.queue = restored;
            self.pending_restore = None;
            self.restore_deadline_ms = None;
            // Restored items are not news.
            self.known_kinds = self
                .queue
                .ordered()
                .iter()
                .map(|item| (item.pane_id, item.kind))
                .collect();
        } else if now_ms >= deadline {
            self.pending_restore = None;
            self.restore_deadline_ms = None;
        }
    }

    /// Builds the live pane view from what the host knows, with this state's background waits.
    pub fn with_background(&self, mut panes: Vec<LivePane>) -> Vec<LivePane> {
        for pane in &mut panes {
            pane.waiting_background = self.is_waiting_background(pane.pane_id);
        }
        panes
    }

    /// Brings the queue in line with the live panes. `Off` does nothing at all. `plugin_marks`
    /// is the set of nodes that still exist, used to drop the marks of vanished panes.
    pub fn reconcile(
        &mut self,
        panes: Vec<LivePane>,
        node_exists: &dyn Fn(NodeId) -> bool,
        now_ms: u64,
    ) -> Reconciled {
        if !self.settings.enabled() {
            return Reconciled::default();
        }
        let live = Self::live_fingerprints(&panes);
        let before = self.queue.to_persisted();
        self.restore_pending(&live, now_ms);
        let generation = self.queue.generation();
        let mut instant: Vec<AttentionItem> = panes
            .iter()
            .filter_map(|pane| derive_instant_item(pane, generation, now_ms))
            .collect();
        // An unchanged item keeps its first-seen time: age and ordering must not restart on
        // every reconcile.
        for item in &mut instant {
            if let Some(existing) = self.queue.item(item.pane_id) {
                if existing.fingerprint == item.fingerprint && existing.kind == item.kind {
                    item.created_ms = existing.created_ms.min(item.created_ms);
                }
            }
        }
        self.queue.retain_plugin_marks(node_exists);
        self.queue.reconcile_instant(&live, instant);
        self.queue
            .expire(now_ms, u64::from(self.settings.expiry_seconds) * 1_000);
        // The agent on screen counts as looked at.
        if let Some(pane_id) = self.focused {
            if self.queue.item(pane_id).is_some_and(|item| {
                item.created_ms > self.queue.last_looked_ms(pane_id).unwrap_or(0)
            }) {
                self.queue.mark_looked(pane_id, now_ms);
            }
        }
        let arrival = self.note_arrival();
        self.last_panes = panes;
        let after = self.queue.to_persisted();
        let changed = before != after;
        if changed && self.last_saved.as_ref() != Some(&after) {
            self.dirty_since_ms.get_or_insert(now_ms);
        }
        let queue_len = self.queue.len();
        if self.selected >= queue_len {
            self.selected = queue_len.saturating_sub(1);
        }
        Reconciled { changed, arrival }
    }

    /// The item whose pane gained a kind it did not have at the previous pass. The first pass
    /// after start-up is silent.
    fn note_arrival(&mut self) -> Option<AttentionItem> {
        let current: HashMap<NodeId, AttentionKind> = self
            .queue
            .ordered()
            .iter()
            .map(|item| (item.pane_id, item.kind))
            .collect();
        let arrived = if self.is_first_pass {
            None
        } else {
            self.queue
                .ordered()
                .into_iter()
                .find(|item| {
                    Some(item.pane_id) != self.focused
                        && self.known_kinds.get(&item.pane_id) != Some(&item.kind)
                })
                .cloned()
        };
        self.is_first_pass = false;
        self.known_kinds = current;
        arrived
    }

    /// Records a plugin's mark or ack of a node (`attention-changed`).
    pub fn plugin_mark(
        &mut self,
        node: NodeId,
        project_id: Option<NodeId>,
        title: String,
        plugin: &str,
        reason: Option<String>,
        now_ms: u64,
    ) {
        match reason {
            Some(reason) => {
                let item = AttentionItem {
                    pane_id: node,
                    project_id,
                    title,
                    kind: AttentionKind::Changed,
                    reason: format!("{plugin}: {reason}"),
                    question: None,
                    waiting_on: None,
                    briefing: Briefing::default(),
                    priority: PLUGIN_MARK_PRIORITY,
                    source: AttentionSource::Plugin,
                    fingerprint: crate::item::fingerprint_parts(&[
                        plugin.as_bytes(),
                        reason.as_bytes(),
                    ]),
                    generation: self.queue.generation(),
                    created_ms: now_ms,
                };
                self.queue.mark_from_plugin(item);
                self.dirty_since_ms.get_or_insert(now_ms);
            }
            None => {
                if self.queue.ack_from_plugin(node) {
                    self.dirty_since_ms.get_or_insert(now_ms);
                }
            }
        }
    }

    /// The next agent the user should visit, in priority order with the rotation rules.
    pub fn target(&self, now_ms: u64) -> Option<NodeId> {
        self.queue
            .next_to_visit(self.focused, now_ms)
            .map(|item| item.pane_id)
    }

    /// Records that the user has now looked at `pane`.
    pub fn visit(&mut self, pane: NodeId, now_ms: u64) {
        self.queue.mark_looked(pane, now_ms);
        self.dirty_since_ms.get_or_insert(now_ms);
    }

    pub fn snooze(&mut self, pane: NodeId, now_ms: u64) {
        self.queue.snooze(pane);
        self.dirty_since_ms.get_or_insert(now_ms);
    }

    /// The pane under the selected row.
    pub fn selected_pane(&self, now_ms: u64) -> Option<NodeId> {
        self.queue
            .ordered_for_display(now_ms)
            .get(self.selected)
            .map(|item| item.pane_id)
    }

    pub fn move_selection(&mut self, delta: isize, now_ms: u64) {
        let len = self.queue.ordered_for_display(now_ms).len();
        if len == 0 {
            self.selected = 0;
            return;
        }
        let next = self.selected as isize + delta;
        self.selected = next.clamp(0, len as isize - 1) as usize;
    }

    /// Starts a model refresh when the mode uses one and some agent changed since it was last
    /// analysed. Returns the `analyse` message payload for the server side.
    pub fn analysis_request(&mut self, now_ms: u64, forced: bool) -> Option<Value> {
        if !self.settings.mode.uses_llm() {
            return None;
        }
        if let Some(started) = self.refresh_started_ms {
            if now_ms.saturating_sub(started) > REFRESH_TIMEOUT_MS {
                self.finish_refresh(now_ms);
            }
        }
        let analysed = self.queue.analysed_fingerprints().clone();
        let turbo = self.settings.mode == AttentionMode::Turbo;
        let wanted: Vec<&LivePane> = self
            .last_panes
            .iter()
            .filter(|pane| {
                // Shared mode looks at agents that stopped; Turbo looks at every agent.
                (turbo || pane.activity != ilium_plugin_sdk::types::AgentActivity::Working)
                    && analysed.get(&pane.pane_id) != Some(&pane.fingerprint())
            })
            .collect();
        if wanted.is_empty() {
            return None;
        }
        if forced {
            // A press asks now; the gate's interval only coalesces the periodic refreshes.
            self.gate.force_next();
        }
        match self.gate.request(now_ms) {
            RefreshDecision::StartNow => {}
            RefreshDecision::QueuedAfterCurrent | RefreshDecision::TooSoon => return None,
        }
        let panes: Vec<Value> = wanted
            .iter()
            .map(|pane| {
                json!({
                    "id": pane.pane_id.0,
                    "project": pane.project_id.map(|id| id.0),
                    "title": pane.title,
                    "fingerprint": pane.fingerprint(),
                })
            })
            .collect();
        let generation = self.queue.begin_refresh();
        self.refresh_started_ms = Some(now_ms);
        let concurrency = if turbo {
            u64::from(self.settings.turbo_concurrency)
        } else {
            1
        };
        Some(json!({
            "generation": generation,
            "concurrency": concurrency,
            "analysed": analysed.iter().map(|(pane, fingerprint)| json!([pane.0, fingerprint])).collect::<Vec<_>>(),
            "panes": panes,
        }))
    }

    /// Applies a server reply. Returns whether the visible queue changed.
    pub fn apply_reply(&mut self, reply: &Value, now_ms: u64) -> bool {
        self.finish_refresh(now_ms);
        let Some(reply) = reply_from_json(reply) else {
            return false;
        };
        let live = Self::live_fingerprints(&self.last_panes);
        let changed = self.queue.apply_llm(reply, &live) > 0;
        if changed {
            self.dirty_since_ms.get_or_insert(now_ms);
        }
        changed
    }

    /// The server could not run the refresh: the gate opens again.
    pub fn refresh_failed(&mut self, now_ms: u64) {
        self.finish_refresh(now_ms);
    }

    fn finish_refresh(&mut self, now_ms: u64) {
        self.refresh_started_ms = None;
        // A rerun the gate queued while the job ran starts nothing by itself: the next tick asks
        // again if an agent is still unanalysed, so the gate is closed again here.
        if self.gate.finished(now_ms) {
            self.gate.finished(now_ms);
        }
    }

    /// The queue to write when it has been stable for the debounce and differs from the last
    /// write.
    pub fn save_due(&mut self, now_ms: u64) -> Option<PersistedQueue> {
        let since = self.dirty_since_ms?;
        if now_ms.saturating_sub(since) < SAVE_DEBOUNCE_MS {
            return None;
        }
        self.dirty_since_ms = None;
        let snapshot = self.queue.to_persisted();
        if self.last_saved.as_ref() == Some(&snapshot) {
            return None;
        }
        // Remembered even when the write fails so one failing store does not retry every tick.
        self.last_saved = Some(snapshot.clone());
        Some(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_plugin_sdk::types::AgentActivity;

    fn pane(id: u64, activity: AgentActivity) -> LivePane {
        LivePane {
            pane_id: NodeId(id),
            project_id: Some(NodeId(1)),
            title: format!("agent-{id}"),
            agent_label: "Claude".to_string(),
            activity,
            goal: None,
            waiting_background: false,
        }
    }

    fn exists(_: NodeId) -> bool {
        true
    }

    #[test]
    fn off_computes_nothing_and_drops_the_queue() {
        let mut state = State::new();
        state.reconcile(
            vec![pane(1, AgentActivity::AwaitingApproval)],
            &exists,
            1_000,
        );
        assert_eq!(state.queue.len(), 1);
        assert!(state.apply_settings(r#"{"mode":"off"}"#));
        assert_eq!(state.queue.len(), 0);
        let result = state.reconcile(
            vec![pane(1, AgentActivity::AwaitingApproval)],
            &exists,
            2_000,
        );
        assert_eq!(result, Reconciled::default());
        assert_eq!(state.queue.len(), 0);
    }

    /// REQ: attention-B03
    #[test]
    fn an_approval_becomes_a_question_and_leaves_with_the_pane() {
        let mut state = State::new();
        state.reconcile(
            vec![pane(1, AgentActivity::AwaitingApproval)],
            &exists,
            1_000,
        );
        let kinds: Vec<_> = state.queue.ordered().iter().map(|i| i.kind).collect();
        assert_eq!(kinds, vec![AttentionKind::Question]);
        state.reconcile(vec![], &exists, 2_000);
        assert!(state.queue.is_empty());
    }

    #[test]
    fn the_first_pass_is_silent_and_a_later_arrival_is_reported_once() {
        let mut state = State::new();
        let first = state.reconcile(
            vec![pane(1, AgentActivity::AwaitingApproval)],
            &exists,
            1_000,
        );
        assert!(first.arrival.is_none(), "start-up is not news");
        let second = state.reconcile(
            vec![
                pane(1, AgentActivity::AwaitingApproval),
                pane(2, AgentActivity::AwaitingApproval),
            ],
            &exists,
            2_000,
        );
        assert_eq!(second.arrival.map(|item| item.pane_id), Some(NodeId(2)));
        let third = state.reconcile(
            vec![
                pane(1, AgentActivity::AwaitingApproval),
                pane(2, AgentActivity::AwaitingApproval),
            ],
            &exists,
            3_000,
        );
        assert!(third.arrival.is_none());
    }

    #[test]
    fn the_focused_agent_is_never_an_arrival() {
        let mut state = State::new();
        state.reconcile(vec![], &exists, 1_000);
        state.focused = Some(NodeId(2));
        let result = state.reconcile(
            vec![pane(2, AgentActivity::AwaitingApproval)],
            &exists,
            2_000,
        );
        assert!(result.arrival.is_none());
    }

    #[test]
    fn background_waits_come_from_the_activity_detail_signal() {
        let mut state = State::new();
        state.observe_detail(NodeId(3), "waiting-background");
        let panes = state.with_background(vec![pane(3, AgentActivity::Working)]);
        state.reconcile(panes, &exists, 1_000);
        let kinds: Vec<_> = state.queue.ordered().iter().map(|i| i.kind).collect();
        assert_eq!(kinds, vec![AttentionKind::WaitingOn]);
        state.observe_detail(NodeId(3), "");
        assert!(!state.is_waiting_background(NodeId(3)));
    }

    #[test]
    fn plugin_marks_show_until_acked_or_the_node_vanishes() {
        let mut state = State::new();
        state.reconcile(vec![], &exists, 1_000);
        state.plugin_mark(
            NodeId(9),
            None,
            "build".to_string(),
            "ilium.core.ci",
            Some("tests failed".to_string()),
            1_500,
        );
        assert_eq!(state.queue.len(), 1);
        state.reconcile(vec![], &exists, 2_000);
        assert_eq!(state.queue.len(), 1, "a mark survives a reconcile");
        state.reconcile(vec![], &|_| false, 3_000);
        assert_eq!(state.queue.len(), 0, "a vanished node drops its mark");
        state.plugin_mark(
            NodeId(9),
            None,
            "build".to_string(),
            "p",
            Some("x".to_string()),
            4_000,
        );
        state.plugin_mark(NodeId(9), None, "build".to_string(), "p", None, 4_100);
        assert!(state.queue.is_empty());
    }

    #[test]
    fn a_visit_moves_the_agent_back_and_the_next_press_goes_elsewhere() {
        let mut state = State::new();
        state.reconcile(
            vec![
                pane(1, AgentActivity::AwaitingApproval),
                pane(2, AgentActivity::Done),
            ],
            &exists,
            1_000,
        );
        let first = state.target(2_000).expect("a target");
        assert_eq!(first, NodeId(1), "a question outranks a finished agent");
        state.visit(first, 2_000);
        assert_eq!(state.target(3_000), Some(NodeId(2)));
    }

    /// REQ: attention-B04
    #[test]
    fn instant_only_never_asks_the_model() {
        let mut state = State::new();
        state.apply_settings(r#"{"mode":"instant_only"}"#);
        state.reconcile(vec![pane(1, AgentActivity::Idle)], &exists, 1_000);
        assert!(state.analysis_request(1_000, false).is_none());
    }

    #[test]
    fn shared_asks_for_stopped_agents_once_and_turbo_for_every_agent() {
        let mut shared = State::new();
        shared.reconcile(
            vec![
                pane(1, AgentActivity::Idle),
                pane(2, AgentActivity::Working),
            ],
            &exists,
            1_000,
        );
        let request = shared.analysis_request(1_000, false).expect("a request");
        assert_eq!(request["panes"].as_array().map(Vec::len), Some(1));
        assert_eq!(request["concurrency"], 1);
        assert!(
            shared.analysis_request(2_000, false).is_none(),
            "one refresh at a time"
        );

        let mut turbo = State::new();
        turbo.apply_settings(r#"{"mode":"turbo","turbo_concurrency":5}"#);
        turbo.reconcile(
            vec![
                pane(1, AgentActivity::Idle),
                pane(2, AgentActivity::Working),
            ],
            &exists,
            1_000,
        );
        let request = turbo.analysis_request(1_000, false).expect("a request");
        assert_eq!(request["panes"].as_array().map(Vec::len), Some(2));
        assert_eq!(request["concurrency"], 5);
    }

    #[test]
    fn a_reply_applies_once_and_an_analysed_agent_is_not_asked_again() {
        let mut state = State::new();
        state.reconcile(vec![pane(1, AgentActivity::Idle)], &exists, 1_000);
        let request = state.analysis_request(1_000, false).expect("a request");
        let generation = request["generation"].as_u64().expect("a generation");
        let fingerprint = request["panes"][0]["fingerprint"]
            .as_u64()
            .expect("a fingerprint");
        let reply = json!({
            "generation": generation,
            "analysed": [[1, fingerprint]],
            "items": [],
        });
        assert!(
            !state.apply_reply(&reply, 2_000),
            "nothing to report changes nothing"
        );
        assert!(state.analysis_request(30_000, false).is_none());
    }

    #[test]
    fn a_failed_refresh_reopens_the_gate_after_its_interval() {
        let mut state = State::new();
        state.reconcile(vec![pane(1, AgentActivity::Idle)], &exists, 1_000);
        assert!(state.analysis_request(1_000, false).is_some());
        state.refresh_failed(2_000);
        assert!(
            state.analysis_request(2_500, false).is_none(),
            "too soon after the last start"
        );
        assert!(state.analysis_request(20_000, false).is_some());
    }

    #[test]
    fn a_lost_reply_cannot_keep_the_gate_closed_forever() {
        let mut state = State::new();
        state.reconcile(vec![pane(1, AgentActivity::Idle)], &exists, 1_000);
        assert!(state.analysis_request(1_000, false).is_some());
        assert!(state
            .analysis_request(1_000 + REFRESH_TIMEOUT_MS + 1, false)
            .is_some());
    }

    #[test]
    fn a_press_asks_at_once_even_inside_the_refresh_interval() {
        let mut state = State::new();
        state.reconcile(vec![pane(1, AgentActivity::Idle)], &exists, 1_000);
        assert!(state.analysis_request(1_000, false).is_some());
        state.refresh_failed(1_100);
        assert!(state.analysis_request(1_200, false).is_none());
        assert!(state.analysis_request(1_200, true).is_some());
    }

    #[test]
    fn the_queue_is_saved_after_it_settles_and_restored_once_panes_vouch_for_it() {
        let mut state = State::new();
        state.reconcile(
            vec![pane(1, AgentActivity::AwaitingApproval)],
            &exists,
            1_000,
        );
        assert!(state.save_due(1_500).is_none(), "debounced");
        let saved = state
            .save_due(1_000 + SAVE_DEBOUNCE_MS)
            .expect("a snapshot");
        assert_eq!(saved.items.len(), 1);
        assert!(
            state.save_due(10_000).is_none(),
            "unchanged: no second write"
        );

        let mut restarted = State::new();
        restarted.set_pending_restore(saved);
        restarted.reconcile(vec![], &exists, 100);
        assert!(restarted.queue.is_empty(), "no live pane yet");
        restarted.reconcile(vec![pane(1, AgentActivity::AwaitingApproval)], &exists, 200);
        assert_eq!(restarted.queue.len(), 1);
    }

    #[test]
    fn a_stored_queue_nobody_vouches_for_is_dropped_after_the_grace() {
        let mut stored = State::new();
        stored.reconcile(
            vec![pane(1, AgentActivity::AwaitingApproval)],
            &exists,
            1_000,
        );
        let snapshot = stored.save_due(10_000).expect("a snapshot");
        let mut restarted = State::new();
        restarted.set_pending_restore(snapshot);
        let other = || vec![pane(7, AgentActivity::Working)];
        restarted.reconcile(other(), &exists, 100);
        restarted.reconcile(other(), &exists, 100 + RESTORE_GRACE_MS + 1);
        // The pending store is gone: a later matching pane no longer revives it.
        restarted.reconcile(
            vec![pane(1, AgentActivity::AwaitingApproval)],
            &exists,
            100 + RESTORE_GRACE_MS + 2,
        );
        let sources: Vec<_> = restarted.queue.ordered().iter().map(|i| i.source).collect();
        assert_eq!(sources, vec![AttentionSource::Instant]);
    }

    #[test]
    fn selection_stays_inside_the_list() {
        let mut state = State::new();
        state.reconcile(
            vec![
                pane(1, AgentActivity::AwaitingApproval),
                pane(2, AgentActivity::Done),
            ],
            &exists,
            1_000,
        );
        state.move_selection(5, 2_000);
        assert_eq!(state.selected, 1);
        state.move_selection(-9, 2_000);
        assert_eq!(state.selected, 0);
        assert_eq!(state.selected_pane(2_000), Some(NodeId(1)));
    }

    /// REQ: attention-B06
    #[test]
    fn an_llm_item_expires_after_the_configured_time() {
        let mut state = State::new();
        state.apply_settings(r#"{"expiry_seconds":60}"#);
        state.reconcile(vec![pane(1, AgentActivity::Idle)], &exists, 1_000);
        let request = state.analysis_request(1_000, false).expect("a request");
        let generation = request["generation"].as_u64().expect("a generation");
        let fingerprint = request["panes"][0]["fingerprint"]
            .as_u64()
            .expect("a fingerprint");
        let item = AttentionItem {
            pane_id: NodeId(1),
            project_id: Some(NodeId(1)),
            title: "agent-1".to_string(),
            kind: AttentionKind::Stuck,
            reason: "loops".to_string(),
            question: None,
            waiting_on: None,
            briefing: Briefing::default(),
            priority: 60,
            source: AttentionSource::Llm,
            fingerprint,
            generation,
            created_ms: 1_500,
        };
        let reply = json!({
            "generation": generation,
            "analysed": [[1, fingerprint]],
            "items": [item],
        });
        assert!(state.apply_reply(&reply, 2_000));
        assert_eq!(state.queue.len(), 1);
        state.reconcile(vec![pane(1, AgentActivity::Idle)], &exists, 1_500 + 61_000);
        assert!(
            state.queue.is_empty(),
            "60 s later the model's item is stale"
        );
    }
}
