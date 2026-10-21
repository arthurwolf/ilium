//! `App` behaviour of the attention queue: building the live evidence,
//! reconciling the queue, the "Next: needs you" press, the right-hand panel,
//! the arrival block and persistence.
//!
//! This is a child module of `app` (declared there with `#[path]`) so it can
//! reach the private project-restructure request path without widening it.
//! Pure pieces live in `crate::attention`; this file only wires them to state.

use std::collections::HashMap;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ilium_core::{NodeId, NodeKind};
use ratatui::layout::Position;

use super::{
    is_press, App, FocusTarget, PaneRuntime, ProjectRestructureState, RestructureRequestOrigin,
    RightPanelTarget,
};
use crate::attention::panel::{self, PanelRow};
use crate::attention::provider::ProviderAttentionClient;
use crate::attention::queue::LiveFingerprints;
use crate::attention::runtime::{launch_turbo_worker, SharedAttentionCall, SharedOutcome};
use crate::attention::state::{
    Arrival, ARRIVAL_VISIBLE_MS, RECONCILE_INTERVAL_MS, SAVE_DEBOUNCE_MS,
};
use crate::attention::{
    derive_instant_item, fingerprint_parts, parse_attention, store, AttentionItem, AttentionKind,
    AttentionCompletionClient, AttentionMode, AttentionQueue, KnownPane, LivePane, LlmReply, RefreshDecision, TurboRequest,
};
use crate::naming_workers::NamingWorkerEvent;

/// Non-empty screen lines hashed into a pane's evidence fingerprint. Enough to
/// cover an approval prompt, small enough that a scrolling log above it does
/// not change the evidence.
const FINGERPRINT_TAIL_LINES: usize = 12;
/// A stored queue that no live pane has justified after this long is dropped.
const RESTORE_GRACE_MS: u64 = 20_000;
/// Rows moved by one wheel notch over the panel.
const PANEL_WHEEL_ROWS: usize = 1;
/// Lines and characters of recent screen text one Turbo call gets. The newest
/// end is kept: that is where an approval prompt or a failing loop shows.
const TURBO_EVIDENCE_LINES: usize = 60;
const TURBO_EVIDENCE_CHARACTERS: usize = 6_000;
/// An attention call or refresh with no reply after this long is given up on,
/// so a lost worker result can never keep the refresh gate closed.
const ATTENTION_CALL_TIMEOUT_MS: u64 = 10 * 60_000;

/// How a restructure call takes part in the attention refresh (Shared mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct AttentionRequest {
    /// Generation of the refresh the call belongs to; `None` starts a new one.
    pub generation: Option<u64>,
    /// False: keep the attention list, discard the sort plan.
    pub apply_plan: bool,
}

fn unix_millis_now() -> u64 {
    crate::scheduled_input::unix_millis_now()
}

/// The last `count` non-empty lines of `text`, joined, trailing space trimmed.
/// The recent end of a screen, bounded by lines and by characters.
fn bounded_turbo_evidence(text: &str) -> String {
    let tail = tail_lines(text, TURBO_EVIDENCE_LINES);
    let characters = tail.chars().count();
    if characters <= TURBO_EVIDENCE_CHARACTERS {
        return tail;
    }
    tail.chars()
        .skip(characters - TURBO_EVIDENCE_CHARACTERS)
        .collect()
}

fn tail_lines(text: &str, count: usize) -> String {
    let mut lines: Vec<&str> = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .rev()
        .take(count)
        .collect();
    lines.reverse();
    lines.join("\n")
}

impl App {
    /// Whether the attention queue exists at all. `Off` means no work, no
    /// button, no panel, no banner and no key action.
    pub fn attention_enabled(&self) -> bool {
        !matches!(self.attention_settings.mode, AttentionMode::Off)
    }

    /// Re-applies what depends on the mode: the left-panel button row, and
    /// emptying everything when the feature is switched off.
    pub(super) fn sync_attention_mode(&mut self) {
        let enabled = self.attention_enabled();
        crate::tree_ui::set_attention_button_reserved(enabled);
        if !self.attention_settings.mode.uses_llm() {
            self.abandon_attention_refresh(unix_millis_now());
        }
        if enabled {
            return;
        }
        self.attention.queue.clear();
        self.attention.known_kinds.clear();
        self.attention.arrival = None;
        self.attention.pending_restore = None;
        self.attention.restore_deadline_ms = None;
        self.attention.last_reconcile_ms = None;
        self.attention.panel_selected = 0;
        if matches!(self.right_panel_target, RightPanelTarget::Attention) {
            self.reconcile_right_panel_target();
            self.focus = FocusTarget::Tree;
        }
    }

    /// Points the queue at its file and reads what the previous client left.
    /// The stored items wait in `pending_restore` until live panes exist to
    /// re-validate them (the tree and screens arrive after start-up).
    pub fn attach_attention_store(&mut self, path: Option<PathBuf>) {
        self.attention.pending_restore = path
            .as_deref()
            .filter(|_| self.attention_enabled())
            .and_then(store::load);
        self.attention.restore_deadline_ms = None;
        self.attention.store_path = path;
    }

    // ------------------------------------------------------------ evidence

    /// One entry per agent pane in the whole tree (every project). The
    /// fingerprint covers the agent's state and the tail of its screen, so a
    /// stored or LLM verdict is only trusted while that evidence is unchanged.
    fn build_live_panes(&self) -> Vec<LivePane> {
        let mut live = Vec::new();
        for pane_id in self.tree.pane_ids_in_tree_order() {
            let Some(node) = self.tree.get(pane_id) else {
                continue;
            };
            let NodeKind::Pane { status, .. } = &node.kind else {
                continue;
            };
            let Some(agent) = status.known_agent_state() else {
                continue;
            };
            let visible_text = match self.panes.get(&pane_id) {
                Some(PaneRuntime::Terminal(view)) => view.with_screen(|screen| screen.contents()),
                _ => String::new(),
            };
            let state_key = format!(
                "{:?}|{:?}|{:?}|{}",
                agent.class, agent.turn, agent.goal, agent.completion_unread
            );
            let fingerprint = fingerprint_parts(&[
                state_key.as_bytes(),
                tail_lines(&visible_text, FINGERPRINT_TAIL_LINES).as_bytes(),
            ]);
            live.push(LivePane {
                pane_id,
                project_id: self.tree.project_ancestor(pane_id),
                title: node.name.clone(),
                agent: Some(agent.clone()),
                fingerprint,
                visible_text,
            });
        }
        live
    }

    /// Brings the queue in line with the live panes. Returns true when the
    /// visible queue changed. `Off` does nothing at all.
    fn reconcile_attention(&mut self, now_ms: u64) -> bool {
        if !self.attention_enabled() {
            return false;
        }
        let live_panes = self.build_live_panes();
        let live: LiveFingerprints = live_panes
            .iter()
            .map(|pane| (pane.pane_id, pane.fingerprint))
            .collect();
        let before = self.attention.queue.to_persisted();
        let is_first_pass = self.attention.last_reconcile_ms.is_none();

        self.restore_pending_attention(&live, now_ms);

        let generation = self.attention.queue.generation();
        let mut instant: Vec<AttentionItem> = live_panes
            .iter()
            .filter_map(|pane| derive_instant_item(pane, generation, now_ms))
            .collect();
        // An unchanged item keeps its first-seen time: age and ordering must
        // not restart on every reconcile.
        for item in &mut instant {
            if let Some(existing) = self.attention.queue.item(item.pane_id) {
                if existing.fingerprint == item.fingerprint && existing.kind == item.kind {
                    item.created_ms = existing.created_ms.min(item.created_ms);
                }
            }
        }
        self.attention.queue.reconcile_instant(&live, instant);
        self.attention.queue.expire(
            now_ms,
            u64::from(self.attention_settings.expiry_seconds) * 1_000,
        );

        // The agent on screen counts as looked at.
        if let Some(pane_id) = self.focused_pane_id() {
            if self.attention.queue.item(pane_id).is_some_and(|item| {
                item.created_ms > self.attention.queue.last_looked_ms(pane_id).unwrap_or(0)
            }) {
                self.attention.queue.mark_looked(pane_id, now_ms);
            }
        }

        self.note_attention_arrival(now_ms, is_first_pass);
        self.attention.last_reconcile_ms = Some(now_ms);

        let after = self.attention.queue.to_persisted();
        let changed = before != after;
        if changed && self.attention.last_saved.as_ref() != Some(&after) {
            self.attention.dirty_since_ms.get_or_insert(now_ms);
        }
        let queue_len = self.attention.queue.len();
        if self.attention.panel_selected >= queue_len {
            self.attention.panel_selected = queue_len.saturating_sub(1);
        }
        changed
    }

    /// Adopts the stored queue once live panes can vouch for it; gives up
    /// after a grace period so a stale file cannot linger.
    fn restore_pending_attention(&mut self, live: &LiveFingerprints, now_ms: u64) {
        let Some(pending) = self.attention.pending_restore.as_ref() else {
            return;
        };
        if live.is_empty() {
            return;
        }
        let deadline = *self
            .attention
            .restore_deadline_ms
            .get_or_insert(now_ms + RESTORE_GRACE_MS);
        let restored = AttentionQueue::from_persisted(pending.clone(), live);
        if !restored.is_empty() {
            self.attention.last_saved = Some(pending.clone());
            self.attention.queue = restored;
            self.attention.pending_restore = None;
            self.attention.restore_deadline_ms = None;
            // Restored items are not news.
            self.attention.known_kinds = self
                .attention
                .queue
                .ordered()
                .iter()
                .map(|item| (item.pane_id, item.kind))
                .collect();
        } else if now_ms >= deadline {
            self.attention.pending_restore = None;
            self.attention.restore_deadline_ms = None;
        }
    }

    /// Starts the arrival block when a pane gained an item kind it did not
    /// have at the previous pass. The first pass after start-up is silent.
    fn note_attention_arrival(&mut self, now_ms: u64, is_first_pass: bool) {
        let current: std::collections::HashMap<NodeId, AttentionKind> = self
            .attention
            .queue
            .ordered()
            .iter()
            .map(|item| (item.pane_id, item.kind))
            .collect();
        if !is_first_pass {
            let viewing = self.active_pane_id();
            let arrived = self
                .attention
                .queue
                .ordered()
                .into_iter()
                .find(|item| {
                    Some(item.pane_id) != viewing
                        && self.attention.known_kinds.get(&item.pane_id) != Some(&item.kind)
                })
                .map(|item| item.pane_id);
            if let Some(pane_id) = arrived {
                self.attention.arrival = Some(Arrival {
                    pane_id,
                    arrived_ms: now_ms,
                });
            }
        }
        self.attention.known_kinds = current;
    }

    /// Per-tick work: reconcile (rate limited), end a stale arrival block and
    /// write a changed queue after it has been stable for the debounce.
    /// Returns true when something visible changed.
    pub fn tick_attention(&mut self) -> bool {
        if !self.attention_enabled() {
            return false;
        }
        let now_ms = unix_millis_now();
        let is_due = self
            .attention
            .last_reconcile_ms
            .is_none_or(|last| now_ms.saturating_sub(last) >= RECONCILE_INTERVAL_MS);
        let mut changed = is_due && self.reconcile_attention(now_ms);
        self.service_attention_workers(now_ms);
        changed |= self.expire_attention_arrival(now_ms);
        self.save_attention_if_due(now_ms);
        changed
    }

    fn expire_attention_arrival(&mut self, now_ms: u64) -> bool {
        let Some(arrival) = self.attention.arrival else {
            return false;
        };
        let is_stale = now_ms.saturating_sub(arrival.arrived_ms) > ARRIVAL_VISIBLE_MS
            || self.attention.queue.item(arrival.pane_id).is_none()
            || self
                .attention
                .queue
                .last_looked_ms(arrival.pane_id)
                .is_some_and(|looked| looked >= arrival.arrived_ms);
        if is_stale {
            self.attention.arrival = None;
        }
        is_stale
    }

    fn save_attention_if_due(&mut self, now_ms: u64) {
        let Some(dirty_since) = self.attention.dirty_since_ms else {
            return;
        };
        if now_ms.saturating_sub(dirty_since) < SAVE_DEBOUNCE_MS {
            return;
        }
        self.attention.dirty_since_ms = None;
        let snapshot = self.attention.queue.to_persisted();
        if self.attention.last_saved.as_ref() == Some(&snapshot) {
            return;
        }
        if let Some(path) = self.attention.store_path.clone() {
            if let Err(error) = store::save(&path, &snapshot) {
                self.status_message = Some(format!("Could not save the attention queue: {error}"));
            }
        }
        // Remembered even on failure so one failing disk does not retry every tick.
        self.attention.last_saved = Some(snapshot);
    }

    // --------------------------------------------------------------- press

    /// The item the next press visits: the best one that is not the pane on
    /// screen, preferring those not yet looked at since they arrived so
    /// repeated presses walk the whole queue instead of ping-ponging between
    /// the top two.
    fn next_attention_target(&self) -> Option<NodeId> {
        let viewing = self.active_pane_id();
        let queue = &self.attention.queue;
        queue
            .ordered()
            .into_iter()
            .find(|item| {
                Some(item.pane_id) != viewing
                    && item.created_ms > queue.last_looked_ms(item.pane_id).unwrap_or(0)
            })
            .map(|item| item.pane_id)
            .or_else(|| queue.next(viewing).map(|item| item.pane_id))
    }

    /// `Next: needs you`: jump to the agent that most needs the user, right
    /// now, from what is already known. Refreshing happens in the background.
    pub fn action_attention_press(&mut self) {
        if !self.attention_enabled() {
            return;
        }
        let now_ms = unix_millis_now();
        // Fresh evidence first: the press must never jump to a stale item.
        self.reconcile_attention(now_ms);
        let Some(pane_id) = self.next_attention_target() else {
            self.status_message = Some("Nothing needs you right now".to_string());
            self.request_attention_refresh();
            return;
        };
        let project_id = self
            .attention
            .queue
            .item(pane_id)
            .and_then(|item| item.project_id);
        self.open_attention_pane(pane_id, now_ms);
        self.request_attention_refresh();
        if self.attention_settings.press_sorts {
            if let Some(project_id) = project_id {
                self.restructure_project_from_attention(project_id);
            }
        }
    }

    /// Focuses `pane_id`, reveals it in the tree and records that the user
    /// has now looked at it.
    fn open_attention_pane(&mut self, pane_id: NodeId, now_ms: u64) {
        self.focus_pane(pane_id);
        self.attention.queue.mark_looked(pane_id, now_ms);
        self.attention.arrival = None;
    }

    /// Asks for a background refresh of the attention queue through the
    /// coalescing gate and says what the gate decided.
    ///
    /// `Shared`: the restructure call of every project with a changed agent
    /// also returns the attention list (see [`Self::start_shared_refresh`]).
    /// `Turbo`: one call per changed agent on a worker thread. `InstantOnly`
    /// and `Off` never call an LLM. Agents whose evidence is unchanged since
    /// their last analysis are skipped in every mode.
    pub fn request_attention_refresh(&mut self) -> Option<RefreshDecision> {
        if !self.attention_settings.mode.uses_llm() {
            return None;
        }
        let now_ms = unix_millis_now();
        let decision = self.attention.gate.request(now_ms);
        if decision == RefreshDecision::StartNow {
            self.attention.refresh_requests += 1;
            self.start_attention_refresh(now_ms);
        }
        Some(decision)
    }

    fn start_attention_refresh(&mut self, now_ms: u64) {
        let started = match self.attention_settings.mode {
            AttentionMode::Shared => self.start_shared_refresh(),
            AttentionMode::Turbo => self.start_turbo_refresh(),
            AttentionMode::Off | AttentionMode::InstantOnly => 0,
        };
        if started == 0 {
            self.finish_attention_refresh(now_ms);
            return;
        }
        self.attention.refresh_open = true;
        self.attention.refresh_started_ms = now_ms;
    }

    /// Tells the gate the refresh is over and starts the one coalesced rerun
    /// if presses arrived meanwhile.
    fn finish_attention_refresh(&mut self, now_ms: u64) {
        self.attention.refresh_open = false;
        if self.attention.gate.finished(now_ms) {
            self.attention.refresh_requests += 1;
            self.start_attention_refresh(now_ms);
        }
    }

    /// Ends the open refresh once none of its calls is left.
    fn settle_attention_refresh(&mut self, now_ms: u64) {
        if self.attention.refresh_open
            && self.attention.shared_calls.is_empty()
            && !self.attention.turbo.is_running()
        {
            self.finish_attention_refresh(now_ms);
        }
    }

    /// Cancels everything in flight and reopens the gate. Used when the mode
    /// leaves the LLM modes and when a refresh outlives its timeout.
    fn abandon_attention_refresh(&mut self, now_ms: u64) {
        self.attention.turbo.cancel_all();
        self.attention.shared_calls.clear();
        self.attention.refresh_open = false;
        // The first call may start a queued rerun; the second closes it.
        for _ in 0..2 {
            if self.attention.gate.is_in_flight() {
                self.attention.gate.finished(now_ms);
            }
        }
    }

    /// Per-tick upkeep of the workers: reap finished threads, stop Turbo when
    /// the mode moved away from it, give up on calls that never answered.
    fn service_attention_workers(&mut self, now_ms: u64) {
        self.attention.turbo.reap();
        if self.attention_settings.mode != AttentionMode::Turbo {
            self.attention.turbo.cancel_all();
        }
        self.attention
            .shared_calls
            .retain(|_, call| now_ms.saturating_sub(call.started_ms) <= ATTENTION_CALL_TIMEOUT_MS);
        let is_overdue = self.attention.refresh_open
            && now_ms.saturating_sub(self.attention.refresh_started_ms) > ATTENTION_CALL_TIMEOUT_MS;
        if is_overdue {
            tracing::warn!("attention refresh got no reply in time; giving up on it");
            self.abandon_attention_refresh(now_ms);
            return;
        }
        self.settle_attention_refresh(now_ms);
    }

    /// Where Turbo workers send their replies: the client's naming-event
    /// channel, drained by the event loop like every other background result.
    pub fn attach_attention_events(&mut self, events: tokio::sync::mpsc::Sender<NamingWorkerEvent>) {
        self.attention.events = Some(events);
    }

    // -------------------------------------------------------------- shared

    /// Shared mode asks for the attention list inside every restructure call.
    pub(super) fn default_restructure_attention(&self) -> Option<AttentionRequest> {
        (self.attention_settings.mode == AttentionMode::Shared).then_some(AttentionRequest {
            generation: None,
            apply_plan: true,
        })
    }

    fn live_attention_fingerprints(&self) -> LiveFingerprints {
        self.build_live_panes()
            .iter()
            .map(|pane| (pane.pane_id, pane.fingerprint))
            .collect()
    }

    /// Notes the agents a Shared-mode restructure call will judge, with the
    /// evidence they show now. False when the project has no agent at all (the
    /// prompt then omits the attention section).
    pub(super) fn register_shared_attention_call(
        &mut self,
        project_id: NodeId,
        request: AttentionRequest,
    ) -> bool {
        if self.attention_settings.mode != AttentionMode::Shared {
            return false;
        }
        let known: HashMap<NodeId, KnownPane> = self
            .build_live_panes()
            .into_iter()
            .filter(|pane| pane.project_id == Some(project_id))
            .map(|pane| {
                (
                    pane.pane_id,
                    KnownPane {
                        project_id: pane.project_id,
                        title: pane.title,
                        fingerprint: pane.fingerprint,
                    },
                )
            })
            .collect();
        if known.is_empty() {
            return false;
        }
        let generation = request
            .generation
            .unwrap_or_else(|| self.attention.queue.begin_refresh());
        self.attention.shared_calls.insert(
            project_id,
            SharedAttentionCall {
                generation,
                apply_plan: request.apply_plan,
                known,
                started_ms: unix_millis_now(),
            },
        );
        true
    }

    /// One press: a restructure call per project that has an agent whose
    /// evidence changed since it was last analysed. With `press_sorts` the
    /// call also sorts; when it cannot (nothing to sort, or sorting is off) an
    /// attention-only call is made instead. Returns how many calls started.
    fn start_shared_refresh(&mut self) -> usize {
        let generation = self.attention.queue.begin_refresh();
        let analysed = self.attention.queue.analysed_fingerprints().clone();
        let mut projects: Vec<NodeId> = Vec::new();
        for pane in self.build_live_panes() {
            let is_unchanged = analysed.get(&pane.pane_id) == Some(&pane.fingerprint);
            if let Some(project_id) = pane.project_id.filter(|_| !is_unchanged) {
                if !projects.contains(&project_id) {
                    projects.push(project_id);
                }
            }
        }
        let mut started = 0;
        for project_id in projects {
            if self.is_project_restructure_loading(project_id) {
                continue;
            }
            let mut is_queued = self.attention_settings.press_sorts
                && self.request_project_restructure_with_attention(
                    project_id,
                    RestructureRequestOrigin::Manual,
                    Some(AttentionRequest {
                        generation: Some(generation),
                        apply_plan: true,
                    }),
                );
            if !is_queued {
                is_queued = self.request_project_restructure_with_attention(
                    project_id,
                    RestructureRequestOrigin::Automatic,
                    Some(AttentionRequest {
                        generation: Some(generation),
                        apply_plan: false,
                    }),
                );
            }
            started += usize::from(is_queued);
        }
        started
    }

    /// Takes in the attention list of a finished Shared-mode restructure call
    /// and returns whether the sort plan of the same reply should be applied.
    ///
    /// The list never decides the plan's fate: a missing, malformed or
    /// partly wrong list degrades to fewer items. A reply is applied pane by
    /// pane against the evidence the call saw (see `AttentionQueue::apply_llm`).
    pub(crate) fn complete_shared_attention(
        &mut self,
        project_id: NodeId,
        outcome: SharedOutcome,
    ) -> bool {
        let Some(call) = self.attention.shared_calls.remove(&project_id) else {
            return true;
        };
        let now_ms = unix_millis_now();
        if let (SharedOutcome::Replied(value), AttentionMode::Shared) =
            (outcome, self.attention_settings.mode)
        {
            let parsed = parse_attention(value.as_ref(), &call.known, call.generation, now_ms);
            if parsed.skipped > 0 {
                tracing::debug!(
                    skipped = parsed.skipped,
                    "attention entries ignored in a restructure reply"
                );
            }
            let mut analysed: Vec<(NodeId, u64)> = call
                .known
                .iter()
                .map(|(pane_id, known)| (*pane_id, known.fingerprint))
                .collect();
            analysed.sort();
            self.merge_attention_reply(
                LlmReply {
                    generation: call.generation,
                    analysed,
                    items: parsed.items,
                },
                now_ms,
            );
        }
        self.settle_attention_refresh(now_ms);
        call.apply_plan
    }

    /// Merges a reply into the queue and marks it for saving. Never touches
    /// the user's focus or the open panel.
    fn merge_attention_reply(&mut self, reply: LlmReply, now_ms: u64) {
        let live = self.live_attention_fingerprints();
        let changed = self.attention.queue.apply_llm(reply, &live);
        if changed == 0 {
            return;
        }
        let after = self.attention.queue.to_persisted();
        if self.attention.last_saved.as_ref() != Some(&after) {
            self.attention.dirty_since_ms.get_or_insert(now_ms);
        }
    }

    // --------------------------------------------------------------- turbo

    /// One LLM call per agent whose evidence changed, on a worker thread.
    /// Returns 1 when the worker started, 0 when there was nothing to do or
    /// no way to report back.
    fn start_turbo_refresh(&mut self) -> usize {
        let client = ProviderAttentionClient::new(
            &self.inference_settings,
            self.attention_settings.model_override.as_deref(),
        );
        self.launch_turbo_refresh(client)
    }

    /// Turbo launch with an injectable provider (tests use a fake).
    fn launch_turbo_refresh<C>(&mut self, client: C) -> usize
    where
        C: AttentionCompletionClient + Send + 'static,
    {
        let Some(events) = self.attention.events.clone() else {
            tracing::warn!("attention turbo refresh skipped: no event channel attached");
            return 0;
        };
        let analysed = self.attention.queue.analysed_fingerprints().clone();
        let requests: Vec<TurboRequest> = self
            .build_live_panes()
            .into_iter()
            .filter(|pane| analysed.get(&pane.pane_id) != Some(&pane.fingerprint))
            .map(|pane| TurboRequest {
                pane_id: pane.pane_id,
                project_id: pane.project_id,
                title: pane.title,
                evidence: bounded_turbo_evidence(&pane.visible_text),
                fingerprint: pane.fingerprint,
            })
            .collect();
        if requests.is_empty() {
            return 0;
        }
        let generation = self.attention.queue.begin_refresh();
        let launched = launch_turbo_worker(
            client,
            requests,
            analysed,
            usize::from(self.attention_settings.turbo_concurrency),
            generation,
            unix_millis_now,
            move |reply| {
                if events
                    .blocking_send(NamingWorkerEvent::Attention(reply))
                    .is_err()
                {
                    tracing::debug!("attention turbo reply dropped: the client is gone");
                }
            },
        );
        match launched {
            Ok(handle) => {
                self.attention.turbo.supersede(handle);
                1
            }
            Err(error) => {
                tracing::warn!(%error, "attention turbo worker could not start");
                0
            }
        }
    }

    /// Applies a Turbo batch. A reply from a superseded or cancelled batch, or
    /// one that arrives after the mode left Turbo, is dropped whole.
    pub(crate) fn apply_attention_reply(&mut self, reply: LlmReply) {
        let now_ms = unix_millis_now();
        let is_current = self.attention_settings.mode == AttentionMode::Turbo
            && self.attention.turbo.current_generation() == Some(reply.generation);
        if !is_current {
            tracing::debug!(generation = reply.generation, "stale attention reply dropped");
            return;
        }
        self.attention.turbo.finish_current();
        self.merge_attention_reply(reply, now_ms);
        self.settle_attention_refresh(now_ms);
    }

    /// Press-to-sort: a press also re-sorts the target's project, unless a
    /// restructure of it is already underway (no repeated "already running"
    /// notices for a repeated press).
    fn restructure_project_from_attention(&mut self, project_id: NodeId) {
        let is_busy = self
            .project_restructure_jobs
            .get(&project_id)
            .is_some_and(|job| {
                matches!(
                    job.state,
                    ProjectRestructureState::Queued
                        | ProjectRestructureState::Running
                        | ProjectRestructureState::Applying
                )
            });
        if !is_busy {
            self.request_project_restructure(project_id, RestructureRequestOrigin::Manual);
        }
    }

    // --------------------------------------------------------------- panel

    /// Shows or hides the attention panel in the right-hand area.
    pub fn action_toggle_attention_panel(&mut self) {
        if !self.attention_enabled() {
            return;
        }
        if matches!(self.right_panel_target, RightPanelTarget::Attention) {
            self.right_panel_target = RightPanelTarget::Empty;
            self.focus = FocusTarget::Tree;
            return;
        }
        // Leaving a pane for the panel must release the pane's focus first.
        self.leave_pane_focus();
        self.reconcile_attention(unix_millis_now());
        self.attention.panel_selected = 0;
        self.right_panel_target = RightPanelTarget::Attention;
        self.focus = FocusTarget::Pane;
        self.resize_displayed_panes(super::PaneResizeCause::RightPanelPresentation);
    }

    /// Rows for the panel and the pointer tests, best first.
    pub fn attention_panel_rows(&self) -> Vec<PanelRow> {
        let now_ms = unix_millis_now();
        let queue = &self.attention.queue;
        queue
            .ordered()
            .into_iter()
            .map(|item| PanelRow {
                kind: item.kind,
                reason: item.reason.clone(),
                question: item.question.clone(),
                project: item
                    .project_id
                    .and_then(|id| self.tree.get(id))
                    .map(|node| node.name.clone())
                    .unwrap_or_default(),
                title: self
                    .tree
                    .get(item.pane_id)
                    .map_or_else(|| item.title.clone(), |node| node.name.clone()),
                age_ms: now_ms.saturating_sub(item.created_ms),
                is_looked: queue.last_looked_ms(item.pane_id).unwrap_or(0) >= item.created_ms,
            })
            .collect()
    }

    /// Keyboard in the panel: arrows move, Enter opens, Esc returns to the tree.
    pub fn handle_attention_key(&mut self, key: KeyEvent) {
        if !is_press(&key) {
            return;
        }
        let total = self.attention.queue.len();
        match key.code {
            KeyCode::Esc => self.leave_pane_focus(),
            KeyCode::Up | KeyCode::Char('k') => {
                self.attention.panel_selected = self.attention.panel_selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.attention.panel_selected =
                    (self.attention.panel_selected + 1).min(total.saturating_sub(1));
            }
            KeyCode::Home => self.attention.panel_selected = 0,
            KeyCode::End => self.attention.panel_selected = total.saturating_sub(1),
            KeyCode::Enter => self.open_attention_entry(self.attention.panel_selected),
            _ => {}
        }
    }

    /// Pointer in the panel: a click opens the entry, the wheel moves the
    /// selection.
    pub fn handle_attention_mouse(&mut self, mouse: MouseEvent, position: Position) {
        let total = self.attention.queue.len();
        let layout = panel::layout(self.layout.pane_area);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let first =
                    panel::first_visible(self.attention.panel_selected, total, layout.capacity);
                if let Some(index) = panel::entry_at(layout, position, first, total) {
                    self.attention.panel_selected = index;
                    self.open_attention_entry(index);
                }
            }
            MouseEventKind::ScrollUp => {
                self.attention.panel_selected = self
                    .attention
                    .panel_selected
                    .saturating_sub(PANEL_WHEEL_ROWS);
            }
            MouseEventKind::ScrollDown => {
                self.attention.panel_selected =
                    (self.attention.panel_selected + PANEL_WHEEL_ROWS).min(total.saturating_sub(1));
            }
            _ => {}
        }
    }

    fn open_attention_entry(&mut self, index: usize) {
        let Some(pane_id) = self
            .attention
            .queue
            .ordered()
            .get(index)
            .map(|item| item.pane_id)
        else {
            return;
        };
        self.open_attention_pane(pane_id, unix_millis_now());
    }

    // ------------------------------------------------------------- surfaces

    /// Count shown on the left-panel button.
    pub fn attention_count(&self) -> usize {
        self.attention.queue.len()
    }

    /// The button's rectangle inside the tree area, when it is drawn.
    pub fn attention_button_rect(&self) -> Option<ratatui::layout::Rect> {
        if !self.attention_enabled() {
            return None;
        }
        crate::tree_ui::attention_button_area(self.layout.tree_area)
    }

    /// The arrival block's content: the best queued item, while an arrival is
    /// pending, the feature is on and the user is not already on that agent.
    pub fn attention_arrival_view(&self) -> Option<(AttentionKind, String, String)> {
        if !self.attention_enabled() {
            return None;
        }
        self.attention.arrival?;
        let item = self.attention.queue.next(self.active_pane_id())?;
        let title = self
            .tree
            .get(item.pane_id)
            .map_or_else(|| item.title.clone(), |node| node.name.clone());
        Some((item.kind, title, item.reason.clone()))
    }

    /// Typing (or any key to the right panel) dismisses the arrival block.
    pub(super) fn dismiss_attention_arrival(&mut self) {
        self.attention.arrival = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::AttentionSettings;
    use crossterm::event::KeyModifiers;
    use ilium_core::{AgentActivity, AgentClass, PaneContentKind, PaneStatus, ROOT_ID};
    use ratatui::layout::Rect;

    fn app() -> App {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        app.set_screen_area(Rect::new(0, 0, 100, 30));
        app.apply_attention_settings(AttentionSettings::default());
        app
    }

    fn add_agent(app: &mut App, name: &str, activity: AgentActivity) -> NodeId {
        let group = app
            .tree
            .children_of(ROOT_ID)
            .ok()
            .and_then(|children| children.first().copied())
            .filter(|id| app.tree.get(*id).is_some_and(|node| node.is_group()))
            .unwrap_or_else(|| app.tree.add_group(ROOT_ID, "work").unwrap());
        let pane_id = app
            .tree
            .add_pane(group, name, PaneContentKind::Terminal)
            .unwrap();
        set_activity(app, pane_id, activity);
        pane_id
    }

    fn set_activity(app: &mut App, pane_id: NodeId, activity: AgentActivity) {
        app.tree
            .set_pane_status(
                pane_id,
                PaneStatus::from_activity(AgentClass::Claude, activity, None),
            )
            .unwrap();
    }

    fn with_mode(app: &mut App, mode: AttentionMode) {
        app.apply_attention_settings(AttentionSettings {
            mode,
            ..AttentionSettings::default()
        });
    }

    fn click(app: &mut App, position: Position) {
        crate::mouse::handle_mouse_event(
            app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: position.x,
                row: position.y,
                modifiers: KeyModifiers::NONE,
            },
        );
    }

    #[test]
    fn a_waiting_agent_is_queued_as_a_question_and_a_finished_one_as_changed() {
        let mut app = app();
        let asking = add_agent(&mut app, "asks", AgentActivity::WaitingApproval);
        let finished = add_agent(&mut app, "done", AgentActivity::Done);
        let _working = add_agent(&mut app, "busy", AgentActivity::Working);

        assert!(app.tick_attention());

        let kinds: Vec<(NodeId, AttentionKind)> = app
            .attention
            .queue
            .ordered()
            .iter()
            .map(|item| (item.pane_id, item.kind))
            .collect();
        assert_eq!(
            kinds,
            vec![
                (asking, AttentionKind::Question),
                (finished, AttentionKind::Changed)
            ]
        );
        assert_eq!(app.attention_count(), 2);
    }

    #[test]
    fn off_shows_no_button_banner_or_panel_and_the_keys_do_nothing() {
        let mut app = app();
        let asking = add_agent(&mut app, "asks", AgentActivity::WaitingApproval);
        app.tick_attention();
        assert!(app.attention_button_rect().is_some());
        assert_eq!(app.attention_count(), 1);

        with_mode(&mut app, AttentionMode::Off);

        assert!(!app.attention_enabled());
        assert_eq!(app.attention_count(), 0, "turning it off empties the queue");
        assert_eq!(app.attention_button_rect(), None);
        assert_eq!(
            crate::tree_ui::attention_button_area(app.layout.tree_area),
            None
        );
        assert!(!app.tick_attention());
        assert_eq!(app.attention_arrival_view(), None);
        let _ = app.take_outbound_requests();

        app.action_attention_press();
        app.action_toggle_attention_panel();

        assert_eq!(app.active_pane_id(), None, "the press must not jump");
        assert_eq!(app.right_panel_target, RightPanelTarget::Empty);
        assert!(app.take_outbound_requests().is_empty());
        assert_eq!(app.attention.refresh_requests, 0);
        let _ = asking;
    }

    #[test]
    fn turning_the_feature_off_closes_an_open_panel() {
        let mut app = app();
        add_agent(&mut app, "asks", AgentActivity::WaitingApproval);
        app.tick_attention();
        app.action_toggle_attention_panel();
        assert_eq!(app.right_panel_target, RightPanelTarget::Attention);

        with_mode(&mut app, AttentionMode::Off);

        assert_eq!(app.right_panel_target, RightPanelTarget::Empty);
    }

    #[test]
    fn a_press_jumps_at_once_and_repeated_presses_walk_the_whole_queue() {
        let mut app = app();
        let asking = add_agent(&mut app, "asks", AgentActivity::WaitingApproval);
        let finished = add_agent(&mut app, "done", AgentActivity::Done);
        let blocked = add_agent(&mut app, "stuck", AgentActivity::Idle);
        app.tree
            .set_pane_status(
                blocked,
                PaneStatus::from_activity(
                    AgentClass::Claude,
                    AgentActivity::Idle,
                    Some(ilium_core::GoalState::Blocked),
                ),
            )
            .unwrap();
        app.tick_attention();
        let _ = app.take_outbound_requests();

        app.action_attention_press();
        assert_eq!(app.active_pane_id(), Some(asking), "Question is first");
        assert_eq!(app.focus, FocusTarget::Pane);
        assert!(app.attention.queue.last_looked_ms(asking).is_some());
        assert!(app.take_outbound_requests().iter().any(|request| matches!(
            request,
            ilium_ipc::ClientRequest::SetPaneFocus { pane_id, focused: true } if *pane_id == asking
        )));

        app.action_attention_press();
        assert_eq!(app.active_pane_id(), Some(blocked), "then Stuck");
        app.action_attention_press();
        assert_eq!(app.active_pane_id(), Some(finished), "then Changed");
        app.action_attention_press();
        assert_eq!(
            app.active_pane_id(),
            Some(asking),
            "everything looked at: back to the best one that is not on screen"
        );
    }

    #[test]
    fn a_press_with_an_empty_queue_says_so_and_moves_nothing() {
        let mut app = app();
        add_agent(&mut app, "busy", AgentActivity::Working);
        app.action_attention_press();
        assert_eq!(app.active_pane_id(), None);
        assert_eq!(
            app.status_message.as_deref(),
            Some("Nothing needs you right now")
        );
    }

    #[test]
    fn refresh_requests_pass_the_gate_once_and_instant_only_never_asks() {
        let mut app = app();
        assert_eq!(
            app.request_attention_refresh(),
            Some(RefreshDecision::StartNow)
        );
        assert_eq!(
            app.request_attention_refresh(),
            Some(RefreshDecision::TooSoon)
        );
        assert_eq!(app.attention.refresh_requests, 1);

        with_mode(&mut app, AttentionMode::InstantOnly);
        assert_eq!(app.request_attention_refresh(), None);
        assert_eq!(app.attention.refresh_requests, 1);
    }

    #[test]
    fn the_queue_survives_a_client_restart_and_drops_what_changed() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("attention").join("queue.json");

        let mut first = app();
        first.attach_attention_store(Some(path.clone()));
        let asking = add_agent(&mut first, "asks", AgentActivity::WaitingApproval);
        first.tick_attention();
        let saved_item = first.attention.queue.item(asking).cloned().unwrap();
        // Make the debounce elapse instead of sleeping through it.
        first.attention.dirty_since_ms = Some(0);
        first.tick_attention();
        assert!(path.is_file(), "the debounced save wrote the queue");

        let mut second = app();
        second.attach_attention_store(Some(path.clone()));
        assert!(second.attention.pending_restore.is_some());
        let asking_again = add_agent(&mut second, "asks", AgentActivity::WaitingApproval);
        assert_eq!(asking_again, asking, "same tree, same pane id");
        second.tick_attention();
        assert!(second.attention.pending_restore.is_none());
        let restored = second.attention.queue.item(asking).cloned().unwrap();
        assert_eq!(restored.created_ms, saved_item.created_ms);
        assert_eq!(restored.kind, AttentionKind::Question);
        assert_eq!(
            second.attention.arrival, None,
            "restored items are not news"
        );

        let mut third = app();
        third.attach_attention_store(Some(path));
        add_agent(&mut third, "asks", AgentActivity::Working);
        third.tick_attention();
        assert!(third.attention.queue.is_empty(), "the evidence moved on");
    }

    #[test]
    fn a_new_item_shows_the_arrival_block_until_the_user_types() {
        let mut app = app();
        let agent = add_agent(&mut app, "asks", AgentActivity::Idle);
        let other = add_agent(&mut app, "other", AgentActivity::Idle);
        app.tick_attention();
        assert_eq!(app.attention.arrival, None);

        set_activity(&mut app, agent, AgentActivity::WaitingApproval);
        assert!(app.reconcile_attention(unix_millis_now()));

        let (kind, title, reason) = app.attention_arrival_view().expect("an arrival");
        assert_eq!(kind, AttentionKind::Question);
        assert_eq!(title, "asks");
        assert_eq!(reason, "Waiting for your answer or approval");

        app.focus_pane(other);
        app.handle_pane_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(app.attention_arrival_view(), None);
    }

    #[test]
    fn items_already_present_at_start_up_do_not_announce_themselves() {
        let mut app = app();
        add_agent(&mut app, "asks", AgentActivity::WaitingApproval);
        app.tick_attention();
        assert_eq!(app.attention_count(), 1);
        assert_eq!(app.attention.arrival, None);
    }

    #[test]
    fn an_arrival_ends_when_its_item_goes_stale_or_is_looked_at() {
        let mut app = app();
        let agent = add_agent(&mut app, "asks", AgentActivity::Idle);
        app.tick_attention();
        set_activity(&mut app, agent, AgentActivity::WaitingApproval);
        let now = unix_millis_now();
        app.reconcile_attention(now);
        assert!(app.attention.arrival.is_some());

        set_activity(&mut app, agent, AgentActivity::Idle);
        app.reconcile_attention(now + 1);
        assert!(app.expire_attention_arrival(now + 1));
        assert_eq!(app.attention.arrival, None);

        set_activity(&mut app, agent, AgentActivity::WaitingApproval);
        app.reconcile_attention(now + 2);
        assert!(app.attention.arrival.is_some());
        assert!(app.expire_attention_arrival(now + 2 + ARRIVAL_VISIBLE_MS + 1));
    }

    #[test]
    fn the_panel_lists_the_queue_and_a_click_on_a_row_opens_that_agent() {
        let mut app = app();
        let asking = add_agent(&mut app, "asks", AgentActivity::WaitingApproval);
        let finished = add_agent(&mut app, "done", AgentActivity::Done);
        app.tick_attention();

        app.action_toggle_attention_panel();
        assert_eq!(app.right_panel_target, RightPanelTarget::Attention);
        assert_eq!(app.active_pane_id(), None);
        assert!(app.pane_viewports().is_empty());
        let rows = app.attention_panel_rows();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].kind, AttentionKind::Question);
        assert_eq!(rows[0].title, "asks");
        assert!(!rows[0].is_looked);

        let layout = panel::layout(app.layout.pane_area);
        let second_row = panel::entry_rect(layout, 1);
        click(&mut app, Position::new(second_row.x + 1, second_row.y));

        assert_eq!(app.active_pane_id(), Some(finished));
        assert!(app.attention.queue.last_looked_ms(finished).is_some());
        assert_ne!(app.active_pane_id(), Some(asking));
        assert!(matches!(
            app.right_panel_target,
            RightPanelTarget::Pane { .. }
        ));
    }

    #[test]
    fn panel_keys_move_the_selection_and_enter_opens_it() {
        let mut app = app();
        let asking = add_agent(&mut app, "asks", AgentActivity::WaitingApproval);
        let finished = add_agent(&mut app, "done", AgentActivity::Done);
        app.tick_attention();
        app.action_toggle_attention_panel();

        app.handle_pane_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.attention.panel_selected, 1);
        app.handle_pane_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.attention.panel_selected, 1, "clamped at the last row");
        app.handle_pane_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.attention.panel_selected, 0);
        app.handle_pane_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.active_pane_id(), Some(asking));
        let _ = finished;
    }

    #[test]
    fn toggling_the_panel_twice_closes_it() {
        let mut app = app();
        add_agent(&mut app, "asks", AgentActivity::WaitingApproval);
        app.action_toggle_attention_panel();
        assert_eq!(app.right_panel_target, RightPanelTarget::Attention);
        app.action_toggle_attention_panel();
        assert_eq!(app.right_panel_target, RightPanelTarget::Empty);
    }

    #[test]
    fn clicking_the_button_presses_it() {
        let mut app = app();
        let asking = add_agent(&mut app, "asks", AgentActivity::WaitingApproval);
        app.tick_attention();
        let button = app.attention_button_rect().expect("the button is drawn");
        assert_eq!(app.attention_count(), 1);

        click(&mut app, Position::new(button.x + 1, button.y));

        assert_eq!(app.active_pane_id(), Some(asking));
    }

    #[test]
    fn the_tail_of_a_screen_ignores_blank_lines_and_what_scrolled_away() {
        let text = "one\n\n  \ntwo  \nthree\n\nfour\n";
        assert_eq!(tail_lines(text, 2), "three\nfour");
        assert_eq!(tail_lines(text, 10), "one\ntwo\nthree\nfour");
        assert_eq!(tail_lines("", 3), "");
    }

    // ---- refresh wiring: fakes only, no real provider call ----

    use crate::attention::{AttentionCallError, AttentionSource};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn add_project_agent(app: &mut App, activity: AgentActivity) -> (NodeId, NodeId) {
        let project = app
            .tree
            .add_project(PathBuf::from("/tmp/attention-wiring-project"))
            .unwrap();
        let group = app.tree.add_group(project, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "agent", PaneContentKind::Terminal)
            .unwrap();
        set_activity(app, pane_id, activity);
        (project, pane_id)
    }

    fn with_settings(app: &mut App, settings: AttentionSettings) {
        app.apply_attention_settings(settings);
    }

    #[derive(Clone)]
    struct CountingClient {
        calls: Arc<AtomicUsize>,
    }

    impl AttentionCompletionClient for CountingClient {
        fn complete_attention_prompt(&self, _prompt: &str) -> Result<String, AttentionCallError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(r#"{"kind":"stuck","reason":"keeps rerunning one failing test","priority":70}"#
                .to_owned())
        }
    }

    fn stuck_list(pane_id: NodeId) -> serde_json::Value {
        serde_json::json!([{
            "id": pane_id.0,
            "kind": "stuck",
            "reason": "keeps rerunning one failing test",
            "priority": 70
        }])
    }

    #[test]
    fn off_and_instant_only_start_no_attention_call_and_add_no_attention_section() {
        for mode in [AttentionMode::Off, AttentionMode::InstantOnly] {
            let mut app = app();
            let (project, _pane) = add_project_agent(&mut app, AgentActivity::Working);
            let (events, _receiver) = tokio::sync::mpsc::channel(4);
            app.attach_attention_events(events);
            with_mode(&mut app, mode);

            assert_eq!(app.request_attention_refresh(), None, "{mode:?}");
            assert!(app.default_restructure_attention().is_none(), "{mode:?}");
            assert!(app.attention.shared_calls.is_empty(), "{mode:?}");
            assert!(!app.attention.turbo.is_running(), "{mode:?}");
            assert_eq!(app.attention.refresh_requests, 0, "{mode:?}");

            assert!(app.request_project_restructure(project, RestructureRequestOrigin::Manual));
            let pending = app.take_pending_restructure_requests();
            assert_eq!(pending.len(), 1, "{mode:?}: the plain sort still runs");
            assert!(!pending[0].include_attention, "{mode:?}");
        }
    }

    #[test]
    fn a_shared_press_asks_inside_one_restructure_call_and_applies_the_list() {
        let mut app = app();
        let (project, pane_id) = add_project_agent(&mut app, AgentActivity::Working);
        with_mode(&mut app, AttentionMode::Shared);

        assert_eq!(
            app.request_attention_refresh(),
            Some(RefreshDecision::StartNow)
        );
        let pending = app.take_pending_restructure_requests();
        assert_eq!(pending.len(), 1, "one coalesced call per project");
        assert!(pending[0].include_attention);
        assert_eq!(pending[0].project_id, project);
        assert!(app.attention.refresh_open);
        assert_eq!(
            app.request_attention_refresh(),
            Some(RefreshDecision::QueuedAfterCurrent),
            "a second press while the call runs only queues"
        );

        let focus_before = app.active_pane_id();
        let apply_plan = app.complete_shared_attention(
            project,
            SharedOutcome::Replied(Some(stuck_list(pane_id))),
        );

        assert!(apply_plan, "the sort plan of the same reply is still applied");
        let item = app.attention.queue.item(pane_id).expect("the list landed");
        assert_eq!(item.source, AttentionSource::Llm);
        assert_eq!(item.kind, AttentionKind::Stuck);
        assert_eq!(app.active_pane_id(), focus_before, "focus never moves");
        assert!(app.attention.shared_calls.is_empty());
    }

    #[test]
    fn with_press_sorts_off_the_call_keeps_the_list_and_drops_the_plan() {
        let mut app = app();
        let (project, pane_id) = add_project_agent(&mut app, AgentActivity::Working);
        with_settings(
            &mut app,
            AttentionSettings {
                mode: AttentionMode::Shared,
                press_sorts: false,
                ..AttentionSettings::default()
            },
        );

        assert_eq!(
            app.request_attention_refresh(),
            Some(RefreshDecision::StartNow)
        );
        let pending = app.take_pending_restructure_requests();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].include_attention);
        assert!(
            !app.is_project_restructure_loading(project),
            "no sort job is shown for an attention-only call"
        );

        let apply_plan = app.complete_shared_attention(
            project,
            SharedOutcome::Replied(Some(stuck_list(pane_id))),
        );
        assert!(!apply_plan, "the plan must not be applied");
        assert!(app.attention.queue.item(pane_id).is_some());
    }

    #[test]
    fn a_failed_or_malformed_shared_reply_adds_nothing_and_releases_the_gate() {
        for outcome in [
            SharedOutcome::Failed,
            SharedOutcome::Replied(None),
            SharedOutcome::Replied(Some(serde_json::json!("garbage"))),
            SharedOutcome::Replied(Some(serde_json::json!([{"id": 999999, "kind": "stuck"}]))),
        ] {
            let mut app = app();
            let (project, pane_id) = add_project_agent(&mut app, AgentActivity::Working);
            with_mode(&mut app, AttentionMode::Shared);
            app.request_attention_refresh();
            app.take_pending_restructure_requests();

            let apply_plan = app.complete_shared_attention(project, outcome.clone());

            assert!(apply_plan, "{outcome:?}: the sort is never held back");
            assert!(app.attention.queue.item(pane_id).is_none(), "{outcome:?}");
            assert!(!app.attention.refresh_open, "{outcome:?}");
            assert!(!app.attention.gate.is_in_flight(), "{outcome:?}");
        }
    }

    #[test]
    fn a_shared_call_that_never_answers_is_given_up_on() {
        let mut app = app();
        let (_project, _pane) = add_project_agent(&mut app, AgentActivity::Working);
        with_mode(&mut app, AttentionMode::Shared);
        app.request_attention_refresh();
        assert!(app.attention.refresh_open);

        let later = unix_millis_now() + ATTENTION_CALL_TIMEOUT_MS + 1;
        app.service_attention_workers(later);

        assert!(app.attention.shared_calls.is_empty());
        assert!(!app.attention.refresh_open);
        assert!(!app.attention.gate.is_in_flight());
    }

    #[test]
    fn turbo_applies_its_reply_skips_unchanged_agents_and_drops_stale_replies() {
        let mut app = app();
        let (_project, pane_id) = add_project_agent(&mut app, AgentActivity::Working);
        let (events, mut receiver) = tokio::sync::mpsc::channel(8);
        app.attach_attention_events(events);
        with_mode(&mut app, AttentionMode::Turbo);
        let calls = Arc::new(AtomicUsize::new(0));
        let client = || CountingClient {
            calls: Arc::clone(&calls),
        };

        assert_eq!(app.launch_turbo_refresh(client()), 1);
        let Some(NamingWorkerEvent::Attention(reply)) = receiver.blocking_recv() else {
            panic!("the worker delivers an attention reply");
        };
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let first_generation = reply.generation;
        let focus_before = app.active_pane_id();
        app.apply_attention_reply(reply);
        let item = app.attention.queue.item(pane_id).expect("reply applied");
        assert_eq!(item.source, AttentionSource::Llm);
        assert_eq!(app.active_pane_id(), focus_before, "focus never moves");
        assert!(!app.attention.turbo.is_running(), "worker handed over");

        assert_eq!(
            app.launch_turbo_refresh(client()),
            0,
            "unchanged evidence: no worker and no call"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Evidence changed: a new batch runs; a reply of the old generation
        // arriving afterwards is stale and ignored.
        set_activity(&mut app, pane_id, AgentActivity::WaitingApproval);
        assert_eq!(app.launch_turbo_refresh(client()), 1);
        let current = app.attention.turbo.current_generation();
        assert!(current.is_some_and(|generation| generation > first_generation));
        let stale = LlmReply {
            generation: first_generation,
            analysed: Vec::new(),
            items: Vec::new(),
        };
        app.apply_attention_reply(stale);
        assert!(
            app.attention.turbo.is_running(),
            "a stale reply must not close the running batch"
        );
        let Some(NamingWorkerEvent::Attention(reply)) = receiver.blocking_recv() else {
            panic!("the second batch delivers too");
        };
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        app.apply_attention_reply(reply);
        assert!(!app.attention.turbo.is_running());
    }

    #[test]
    fn a_superseded_or_cancelled_turbo_batch_is_never_applied() {
        let mut app = app();
        let (_project, pane_id) = add_project_agent(&mut app, AgentActivity::Working);
        let (events, mut receiver) = tokio::sync::mpsc::channel(8);
        app.attach_attention_events(events);
        with_mode(&mut app, AttentionMode::Turbo);
        let calls = Arc::new(AtomicUsize::new(0));

        assert_eq!(
            app.launch_turbo_refresh(CountingClient {
                calls: Arc::clone(&calls)
            }),
            1
        );
        let Some(NamingWorkerEvent::Attention(reply)) = receiver.blocking_recv() else {
            panic!("reply expected");
        };
        // Leaving Turbo cancels the worker; its late reply is dropped.
        with_mode(&mut app, AttentionMode::Shared);
        app.service_attention_workers(unix_millis_now());
        app.apply_attention_reply(reply);
        assert!(app.attention.queue.item(pane_id).is_none());
    }

    #[test]
    fn turbo_evidence_keeps_the_newest_lines_within_both_bounds() {
        let many_lines: String = (0..200).map(|line| format!("line {line}\n")).collect();
        let evidence = bounded_turbo_evidence(&many_lines);
        assert_eq!(evidence.lines().count(), TURBO_EVIDENCE_LINES);
        assert!(evidence.ends_with("line 199"));

        let wide = format!("old\n{}", "x".repeat(TURBO_EVIDENCE_CHARACTERS * 2));
        let evidence = bounded_turbo_evidence(&wide);
        assert_eq!(evidence.chars().count(), TURBO_EVIDENCE_CHARACTERS);
        assert!(!evidence.contains("old"));
    }
}
