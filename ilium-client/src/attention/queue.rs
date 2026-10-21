//! The in-memory attention queue: ordering, merging, staleness, persistence.

use std::collections::HashMap;

use ilium_core::NodeId;
use serde::{Deserialize, Serialize};

use super::item::{AttentionItem, AttentionKind, AttentionSource, Briefing};

/// How long a visited agent stays at the back of the line when only its
/// screen changed. An agent that becomes more urgent comes back at once.
pub const REVISIT_COOLDOWN_MS: u64 = 5 * 60 * 1000;

/// What the user saw when they last visited an agent: the evidence and the
/// kind of attention it asked for. A press only returns to the agent early if
/// it has since become more urgent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisitedAs {
    pub fingerprint: u64,
    pub kind: AttentionKind,
}

/// Current evidence fingerprint of every pane that still exists. Anything not
/// in this map is gone, and anything whose fingerprint differs has moved on.
pub type LiveFingerprints = HashMap<NodeId, u64>;

/// An LLM answer for one refresh. `analysed` lists every pane the call looked
/// at with the fingerprint it saw, so a pane absent from `items` is understood
/// as "analysed, needs nothing" rather than "not analysed".
#[derive(Debug, Clone, Default)]
pub struct LlmReply {
    pub generation: u64,
    pub analysed: Vec<(NodeId, u64)>,
    pub items: Vec<AttentionItem>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedQueue {
    pub items: Vec<AttentionItem>,
    pub last_looked_ms: Vec<(NodeId, u64)>,
    /// Older stores lack this: they load with no visit memory.
    #[serde(default)]
    pub visited_as: Vec<(NodeId, VisitedAs)>,
}

#[derive(Debug, Default)]
pub struct AttentionQueue {
    items: HashMap<NodeId, AttentionItem>,
    generation: u64,
    last_looked_ms: HashMap<NodeId, u64>,
    visited_as: HashMap<NodeId, VisitedAs>,
    /// Evidence fingerprint each pane was last analysed at by an accepted LLM
    /// reply, including replies that found nothing to report. Not persisted:
    /// after a restart every agent is analysed once more.
    analysed: HashMap<NodeId, u64>,
}

impl AttentionQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Panes already analysed by the LLM layer, with the fingerprint seen.
    /// A pane whose current fingerprint equals its entry needs no new call.
    pub fn analysed_fingerprints(&self) -> &HashMap<NodeId, u64> {
        &self.analysed
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn item(&self, pane_id: NodeId) -> Option<&AttentionItem> {
        self.items.get(&pane_id)
    }

    /// Starts a refresh and returns the generation that stamps its replies.
    pub fn begin_refresh(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    /// Items best-first: kind rank, then priority, then oldest, then pane id.
    pub fn ordered(&self) -> Vec<&AttentionItem> {
        let mut items: Vec<&AttentionItem> = self.items.values().collect();
        items.sort_by(|a, b| {
            a.kind
                .cmp(&b.kind)
                .then(b.priority.cmp(&a.priority))
                .then(a.created_ms.cmp(&b.created_ms))
                .then(a.pane_id.cmp(&b.pane_id))
        });
        items
    }

    /// Best item that is not the pane the user is already on. Not consumed:
    /// an item leaves only when its evidence goes stale or the pane closes.
    pub fn next(&self, focused: Option<NodeId>) -> Option<&AttentionItem> {
        self.ordered()
            .into_iter()
            .find(|item| Some(item.pane_id) != focused)
    }

    pub fn mark_looked(&mut self, pane_id: NodeId, now_ms: u64) {
        self.last_looked_ms.insert(pane_id, now_ms);
        if let Some(item) = self.items.get(&pane_id) {
            self.visited_as.insert(
                pane_id,
                VisitedAs {
                    fingerprint: item.fingerprint,
                    kind: item.kind,
                },
            );
        }
    }

    /// Whether the user has dealt with this item and nothing important has
    /// changed since: visited, not more urgent than then, and either the same
    /// evidence or still inside the revisit cooldown.
    pub fn is_handled(&self, item: &AttentionItem, now_ms: u64) -> bool {
        let (Some(visit), Some(looked_ms)) = (
            self.visited_as.get(&item.pane_id),
            self.last_looked_ms.get(&item.pane_id),
        ) else {
            return false;
        };
        if item.kind < visit.kind {
            return false;
        }
        item.fingerprint == visit.fingerprint
            || now_ms.saturating_sub(*looked_ms) < REVISIT_COOLDOWN_MS
    }

    /// The agent the next press should visit, never the one on screen.
    /// Unhandled items come first in priority order. When every item has been
    /// handled the queue rotates instead of repeating: the agent visited
    /// longest ago comes next, so a long queue is walked end to end before any
    /// agent is shown twice.
    pub fn next_to_visit(&self, focused: Option<NodeId>, now_ms: u64) -> Option<&AttentionItem> {
        let candidates: Vec<&AttentionItem> = self
            .ordered()
            .into_iter()
            .filter(|item| Some(item.pane_id) != focused)
            .collect();
        candidates
            .iter()
            .copied()
            .find(|item| !self.is_handled(item, now_ms))
            .or_else(|| {
                candidates
                    .iter()
                    .copied()
                    .min_by_key(|item| self.last_looked_ms.get(&item.pane_id).copied().unwrap_or(0))
            })
    }

    /// Items in the order the panel and dialog show them: unhandled first,
    /// handled ones sunk to the back, each group in priority order.
    pub fn ordered_for_display(&self, now_ms: u64) -> Vec<&AttentionItem> {
        let (mut open, handled): (Vec<_>, Vec<_>) = self
            .ordered()
            .into_iter()
            .partition(|item| !self.is_handled(item, now_ms));
        open.extend(handled);
        open
    }

    pub fn last_looked_ms(&self, pane_id: NodeId) -> Option<u64> {
        self.last_looked_ms.get(&pane_id).copied()
    }

    /// Replaces the instant layer with `instant` and drops every item whose
    /// pane vanished or whose evidence moved on. LLM items that survive are
    /// kept and, when both layers flag a pane, combined by [`combine`].
    pub fn reconcile_instant(&mut self, live: &LiveFingerprints, instant: Vec<AttentionItem>) {
        self.items.retain(|pane_id, item| {
            item.source == AttentionSource::Llm && live.get(pane_id) == Some(&item.fingerprint)
        });
        self.last_looked_ms
            .retain(|pane_id, _| live.contains_key(pane_id));
        self.visited_as
            .retain(|pane_id, _| live.contains_key(pane_id));
        self.analysed
            .retain(|pane_id, _| live.contains_key(pane_id));
        for fresh in instant {
            if live.get(&fresh.pane_id) != Some(&fresh.fingerprint) {
                continue;
            }
            let merged = match self.items.remove(&fresh.pane_id) {
                Some(existing) => combine(existing, fresh),
                None => fresh,
            };
            self.items.insert(merged.pane_id, merged);
        }
    }

    /// Merges one LLM reply. Returns how many panes changed.
    ///
    /// A reply is rejected per pane when the pane's fingerprint moved since the
    /// call began, or when a newer-generation LLM item is already stored.
    pub fn apply_llm(&mut self, reply: LlmReply, live: &LiveFingerprints) -> usize {
        let mut changed = 0;
        for (pane_id, seen) in &reply.analysed {
            if live.get(pane_id) != Some(seen) {
                continue;
            }
            let newer_stored = self.items.get(pane_id).is_some_and(|stored| {
                stored.source == AttentionSource::Llm && stored.generation > reply.generation
            });
            if newer_stored {
                continue;
            }
            self.analysed.insert(*pane_id, *seen);
            if let Some(stored) = self.items.get(pane_id) {
                if stored.source == AttentionSource::Llm {
                    self.items.remove(pane_id);
                    changed += 1;
                }
            }
        }
        for mut item in reply.items {
            let analysed = reply
                .analysed
                .iter()
                .any(|(pane_id, seen)| *pane_id == item.pane_id && *seen == item.fingerprint);
            if !analysed || live.get(&item.pane_id) != Some(&item.fingerprint) {
                continue;
            }
            if self.items.get(&item.pane_id).is_some_and(|stored| {
                stored.source == AttentionSource::Llm && stored.generation > reply.generation
            }) {
                continue;
            }
            item.source = AttentionSource::Llm;
            item.generation = reply.generation;
            let merged = match self.items.remove(&item.pane_id) {
                Some(existing) => combine(existing, item),
                None => item,
            };
            self.items.insert(merged.pane_id, merged);
            changed += 1;
        }
        changed
    }

    /// Drops LLM items older than `ttl_ms`. Instant items are always current.
    pub fn expire(&mut self, now_ms: u64, ttl_ms: u64) -> usize {
        let before = self.items.len();
        self.items.retain(|_, item| {
            item.source == AttentionSource::Instant
                || now_ms.saturating_sub(item.created_ms) <= ttl_ms
        });
        before - self.items.len()
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.analysed.clear();
    }

    pub fn to_persisted(&self) -> PersistedQueue {
        let mut items: Vec<AttentionItem> = self.items.values().cloned().collect();
        items.sort_by_key(|item| item.pane_id);
        let mut last_looked_ms: Vec<(NodeId, u64)> = self
            .last_looked_ms
            .iter()
            .map(|(id, ms)| (*id, *ms))
            .collect();
        last_looked_ms.sort();
        let mut visited_as: Vec<(NodeId, VisitedAs)> = self
            .visited_as
            .iter()
            .map(|(id, visit)| (*id, *visit))
            .collect();
        visited_as.sort_by_key(|(id, _)| *id);
        PersistedQueue {
            items,
            last_looked_ms,
            visited_as,
        }
    }

    /// Restores a persisted queue, keeping only what the live panes still
    /// justify (the pane exists and its evidence fingerprint is unchanged).
    pub fn from_persisted(persisted: PersistedQueue, live: &LiveFingerprints) -> Self {
        let mut queue = Self::new();
        queue.generation = persisted
            .items
            .iter()
            .map(|item| item.generation)
            .max()
            .unwrap_or(0);
        for item in persisted.items {
            if live.get(&item.pane_id) == Some(&item.fingerprint) {
                queue.items.insert(item.pane_id, item);
            }
        }
        for (pane_id, ms) in persisted.last_looked_ms {
            if live.contains_key(&pane_id) {
                queue.last_looked_ms.insert(pane_id, ms);
            }
        }
        for (pane_id, visit) in persisted.visited_as {
            if live.contains_key(&pane_id) {
                queue.visited_as.insert(pane_id, visit);
            }
        }
        queue
    }
}

/// Combines two items for one pane. The more urgent kind wins; on equal kinds
/// the LLM's wording replaces the instant wording but never loses priority.
/// The earlier `created_ms` is kept so an item does not look newer each refresh.
fn combine(existing: AttentionItem, incoming: AttentionItem) -> AttentionItem {
    let created_ms = existing.created_ms.min(incoming.created_ms);
    let (mut winner, loser) = if incoming.kind < existing.kind {
        (incoming, existing)
    } else if existing.kind < incoming.kind {
        (existing, incoming)
    } else if incoming.source == AttentionSource::Llm {
        (incoming, existing)
    } else {
        (existing, incoming)
    };
    if winner.kind == loser.kind {
        winner.priority = winner.priority.max(loser.priority);
        if winner.question.is_none() {
            winner.question = loser.question;
        }
        if winner.waiting_on.is_none() {
            winner.waiting_on = loser.waiting_on;
        }
        winner.briefing = merge_briefing(winner.briefing, &loser.briefing);
    }
    winner.created_ms = created_ms;
    winner
}

/// Keeps LLM wording over deterministic wording. Automatic and AI text are
/// never mixed in one box: an LLM briefing is only completed from another LLM
/// briefing, and a deterministic one gives way to any LLM wording.
fn merge_briefing(winner: Briefing, loser: &Briefing) -> Briefing {
    match (winner.from_llm, loser.from_llm) {
        (true, true) => winner.filled_from(loser),
        (true, false) => winner,
        (false, true) => loser.clone(),
        (false, false) => winner.filled_from(loser),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::AttentionKind;

    fn item(
        pane: u64,
        kind: AttentionKind,
        priority: u8,
        source: AttentionSource,
    ) -> AttentionItem {
        AttentionItem {
            pane_id: NodeId(pane),
            project_id: None,
            title: format!("p{pane}"),
            kind,
            reason: "r".into(),
            question: None,
            waiting_on: None,
            briefing: Briefing::default(),
            priority,
            source,
            fingerprint: 100 + pane,
            generation: 0,
            created_ms: pane,
        }
    }

    fn live(panes: &[u64]) -> LiveFingerprints {
        panes.iter().map(|p| (NodeId(*p), 100 + p)).collect()
    }

    #[test]
    fn ordering_is_kind_then_priority_then_age() {
        let mut queue = AttentionQueue::new();
        queue.reconcile_instant(
            &live(&[1, 2, 3, 4]),
            vec![
                item(1, AttentionKind::Changed, 99, AttentionSource::Instant),
                item(2, AttentionKind::Question, 10, AttentionSource::Instant),
                item(3, AttentionKind::Question, 50, AttentionSource::Instant),
                item(4, AttentionKind::Stuck, 90, AttentionSource::Instant),
            ],
        );
        let order: Vec<u64> = queue.ordered().iter().map(|i| i.pane_id.0).collect();
        assert_eq!(order, vec![3, 2, 4, 1]);
    }

    fn queue_of(count: u64) -> AttentionQueue {
        let mut queue = AttentionQueue::new();
        let live: LiveFingerprints = (1..=count).map(|id| (NodeId(id), id)).collect();
        let items = (1..=count)
            .map(|id| {
                let mut entry = item(id, AttentionKind::Question, 50, AttentionSource::Instant);
                entry.fingerprint = id;
                entry.created_ms = id;
                entry
            })
            .collect();
        queue.reconcile_instant(&live, items);
        queue
    }

    #[test]
    fn repeated_presses_walk_the_whole_queue_before_repeating() {
        let mut queue = queue_of(25);
        let mut seen = Vec::new();
        let mut focused = None;
        for step in 0..25 {
            let pane = queue.next_to_visit(focused, 10_000 + step).unwrap().pane_id;
            queue.mark_looked(pane, 10_000 + step);
            seen.push(pane);
            focused = Some(pane);
        }
        let mut unique = seen.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 25, "every agent exactly once: {seen:?}");
        // Second lap rotates in the same order (longest ago first).
        let again = queue.next_to_visit(focused, 20_000).unwrap().pane_id;
        assert_eq!(again, seen[0]);
    }

    #[test]
    fn a_visited_agent_returns_early_only_when_it_becomes_more_urgent() {
        let mut queue = AttentionQueue::new();
        let live: LiveFingerprints = [(NodeId(1), 1), (NodeId(2), 2)].into();
        let changed = {
            let mut entry = item(1, AttentionKind::Changed, 50, AttentionSource::Instant);
            entry.fingerprint = 1;
            entry
        };
        let waiting = {
            let mut entry = item(2, AttentionKind::WaitingOn, 50, AttentionSource::Instant);
            entry.fingerprint = 2;
            entry
        };
        queue.reconcile_instant(&live, vec![changed, waiting]);
        queue.mark_looked(NodeId(2), 1_000);
        assert_eq!(queue.next_to_visit(None, 2_000).unwrap().pane_id, NodeId(1));
        // Pane 2 now asks a question: urgent again despite being visited.
        let live: LiveFingerprints = [(NodeId(1), 1), (NodeId(2), 3)].into();
        let mut question = item(2, AttentionKind::Question, 50, AttentionSource::Instant);
        question.fingerprint = 3;
        let mut still_changed = item(1, AttentionKind::Changed, 50, AttentionSource::Instant);
        still_changed.fingerprint = 1;
        queue.reconcile_instant(&live, vec![still_changed, question]);
        assert_eq!(queue.next_to_visit(None, 3_000).unwrap().pane_id, NodeId(2));
    }

    #[test]
    fn handled_agents_sink_to_the_back_of_the_display_order() {
        let mut queue = queue_of(3);
        queue.mark_looked(NodeId(1), 1_000);
        let order: Vec<NodeId> = queue
            .ordered_for_display(2_000)
            .iter()
            .map(|entry| entry.pane_id)
            .collect();
        assert_eq!(order, vec![NodeId(2), NodeId(3), NodeId(1)]);
    }

    #[test]
    fn a_visited_agent_whose_screen_changed_waits_out_the_cooldown() {
        let mut queue = queue_of(2);
        queue.mark_looked(NodeId(1), 1_000);
        let live: LiveFingerprints = [(NodeId(1), 99), (NodeId(2), 2)].into();
        let mut moved = item(1, AttentionKind::Question, 50, AttentionSource::Instant);
        moved.fingerprint = 99;
        let mut other = item(2, AttentionKind::Question, 50, AttentionSource::Instant);
        other.fingerprint = 2;
        queue.reconcile_instant(&live, vec![moved, other]);
        assert!(queue.is_handled(
            queue.item(NodeId(1)).unwrap(),
            1_000 + REVISIT_COOLDOWN_MS - 1
        ));
        assert!(!queue.is_handled(queue.item(NodeId(1)).unwrap(), 1_000 + REVISIT_COOLDOWN_MS));
    }

    #[test]
    fn visit_memory_survives_persistence_and_old_stores_still_load() {
        let mut queue = queue_of(2);
        queue.mark_looked(NodeId(1), 500);
        let live: LiveFingerprints = [(NodeId(1), 1), (NodeId(2), 2)].into();
        let restored = AttentionQueue::from_persisted(queue.to_persisted(), &live);
        assert!(restored.is_handled(restored.item(NodeId(1)).unwrap(), 600));
        let old: PersistedQueue =
            serde_json::from_str(r#"{"items":[],"last_looked_ms":[]}"#).unwrap();
        assert!(old.visited_as.is_empty());
    }

    #[test]
    fn next_skips_the_focused_pane_without_consuming() {
        let mut queue = AttentionQueue::new();
        queue.reconcile_instant(
            &live(&[1, 2]),
            vec![
                item(1, AttentionKind::Question, 90, AttentionSource::Instant),
                item(2, AttentionKind::Changed, 30, AttentionSource::Instant),
            ],
        );
        assert_eq!(queue.next(Some(NodeId(1))).unwrap().pane_id, NodeId(2));
        assert_eq!(queue.next(None).unwrap().pane_id, NodeId(1));
        assert_eq!(queue.len(), 2);
        assert!(AttentionQueue::new().next(None).is_none());
    }

    #[test]
    fn reconcile_drops_closed_and_moved_on_panes() {
        let mut queue = AttentionQueue::new();
        queue.reconcile_instant(
            &live(&[1, 2]),
            vec![
                item(1, AttentionKind::Question, 90, AttentionSource::Instant),
                item(2, AttentionKind::Question, 90, AttentionSource::Instant),
            ],
        );
        let mut moved = live(&[1]);
        moved.insert(NodeId(2), 999);
        queue.reconcile_instant(
            &moved,
            vec![item(
                1,
                AttentionKind::Question,
                90,
                AttentionSource::Instant,
            )],
        );
        assert_eq!(queue.len(), 1);
        assert!(queue.item(NodeId(2)).is_none());
    }

    #[test]
    fn stale_llm_reply_is_rejected_when_fingerprint_moved() {
        let mut queue = AttentionQueue::new();
        let generation = queue.begin_refresh();
        let reply = LlmReply {
            generation,
            analysed: vec![(NodeId(1), 101)],
            items: vec![item(1, AttentionKind::Stuck, 70, AttentionSource::Llm)],
        };
        let mut moved = live(&[1]);
        moved.insert(NodeId(1), 555);
        assert_eq!(queue.apply_llm(reply, &moved), 0);
        assert!(queue.is_empty());
    }

    #[test]
    fn older_generation_reply_never_overwrites_a_newer_item() {
        let mut queue = AttentionQueue::new();
        let old = queue.begin_refresh();
        let newer = queue.begin_refresh();
        let mut fresh = item(1, AttentionKind::Stuck, 70, AttentionSource::Llm);
        fresh.reason = "new".into();
        queue.apply_llm(
            LlmReply {
                generation: newer,
                analysed: vec![(NodeId(1), 101)],
                items: vec![fresh],
            },
            &live(&[1]),
        );
        let mut stale = item(1, AttentionKind::Stuck, 70, AttentionSource::Llm);
        stale.reason = "old".into();
        queue.apply_llm(
            LlmReply {
                generation: old,
                analysed: vec![(NodeId(1), 101)],
                items: vec![stale],
            },
            &live(&[1]),
        );
        assert_eq!(queue.item(NodeId(1)).unwrap().reason, "new");
    }

    #[test]
    fn analysed_pane_absent_from_reply_clears_its_llm_item_only() {
        let mut queue = AttentionQueue::new();
        queue.reconcile_instant(
            &live(&[1, 2]),
            vec![item(
                2,
                AttentionKind::Question,
                90,
                AttentionSource::Instant,
            )],
        );
        let g1 = queue.begin_refresh();
        queue.apply_llm(
            LlmReply {
                generation: g1,
                analysed: vec![(NodeId(1), 101)],
                items: vec![item(1, AttentionKind::Stuck, 70, AttentionSource::Llm)],
            },
            &live(&[1, 2]),
        );
        assert_eq!(queue.len(), 2);
        let g2 = queue.begin_refresh();
        queue.apply_llm(
            LlmReply {
                generation: g2,
                analysed: vec![(NodeId(1), 101), (NodeId(2), 102)],
                items: vec![],
            },
            &live(&[1, 2]),
        );
        assert!(queue.item(NodeId(1)).is_none());
        // The instant item survives: the LLM never removes what detection justifies.
        assert!(queue.item(NodeId(2)).is_some());
    }

    #[test]
    fn llm_item_for_unanalysed_or_unknown_pane_is_ignored() {
        let mut queue = AttentionQueue::new();
        let generation = queue.begin_refresh();
        let applied = queue.apply_llm(
            LlmReply {
                generation,
                analysed: vec![(NodeId(1), 101)],
                items: vec![
                    item(2, AttentionKind::Stuck, 70, AttentionSource::Llm),
                    item(9, AttentionKind::Stuck, 70, AttentionSource::Llm),
                ],
            },
            &live(&[1, 2]),
        );
        assert_eq!(applied, 0);
        assert!(queue.is_empty());
    }

    #[test]
    fn combine_keeps_the_more_urgent_kind_and_the_earlier_creation_time() {
        let mut queue = AttentionQueue::new();
        queue.reconcile_instant(
            &live(&[1]),
            vec![item(
                1,
                AttentionKind::Changed,
                30,
                AttentionSource::Instant,
            )],
        );
        let generation = queue.begin_refresh();
        let mut llm = item(1, AttentionKind::Question, 60, AttentionSource::Llm);
        llm.created_ms = 500;
        llm.question = Some("which database?".into());
        queue.apply_llm(
            LlmReply {
                generation,
                analysed: vec![(NodeId(1), 101)],
                items: vec![llm],
            },
            &live(&[1]),
        );
        let merged = queue.item(NodeId(1)).unwrap();
        assert_eq!(merged.kind, AttentionKind::Question);
        assert_eq!(merged.created_ms, 1);
        assert_eq!(merged.question.as_deref(), Some("which database?"));
    }

    #[test]
    fn expire_drops_only_old_llm_items() {
        let mut queue = AttentionQueue::new();
        queue.reconcile_instant(
            &live(&[2]),
            vec![item(
                2,
                AttentionKind::Question,
                90,
                AttentionSource::Instant,
            )],
        );
        let generation = queue.begin_refresh();
        let mut llm = item(1, AttentionKind::Stuck, 70, AttentionSource::Llm);
        llm.created_ms = 1_000;
        queue.apply_llm(
            LlmReply {
                generation,
                analysed: vec![(NodeId(1), 101)],
                items: vec![llm],
            },
            &live(&[1, 2]),
        );
        assert_eq!(queue.expire(10_000, 5_000), 1);
        assert!(queue.item(NodeId(1)).is_none());
        assert!(queue.item(NodeId(2)).is_some());
    }

    #[test]
    fn persistence_round_trips_and_revalidates_against_live_state() {
        let mut queue = AttentionQueue::new();
        queue.reconcile_instant(
            &live(&[1, 2]),
            vec![
                item(1, AttentionKind::Question, 90, AttentionSource::Instant),
                item(2, AttentionKind::Changed, 30, AttentionSource::Instant),
            ],
        );
        queue.mark_looked(NodeId(1), 77);
        let text = serde_json::to_string(&queue.to_persisted()).unwrap();
        let persisted: PersistedQueue = serde_json::from_str(&text).unwrap();
        let mut moved = live(&[1]);
        moved.insert(NodeId(2), 1);
        let restored = AttentionQueue::from_persisted(persisted, &moved);
        assert_eq!(restored.len(), 1);
        assert!(restored.item(NodeId(1)).is_some());
        assert_eq!(restored.last_looked_ms(NodeId(1)), Some(77));
    }

    fn briefed(pane: u64, source: AttentionSource, from_llm: bool, summary: &str) -> AttentionItem {
        let mut base = item(pane, AttentionKind::Question, 50, source);
        base.briefing = Briefing {
            status_summary: Some(summary.into()),
            why_here: None,
            suggested_action: None,
            from_llm,
        };
        base
    }

    #[test]
    fn an_llm_reply_fills_the_briefing_and_the_next_instant_pass_keeps_it() {
        let mut queue = AttentionQueue::new();
        let live = live(&[1]);
        queue.reconcile_instant(
            &live,
            vec![briefed(1, AttentionSource::Instant, false, "automatic")],
        );
        queue.apply_llm(
            LlmReply {
                generation: 1,
                analysed: vec![(NodeId(1), 101)],
                items: vec![briefed(1, AttentionSource::Llm, true, "from the model")],
            },
            &live,
        );
        let shown = queue.item(NodeId(1)).unwrap();
        assert_eq!(
            shown.briefing.status_summary.as_deref(),
            Some("from the model")
        );
        assert!(shown.briefing.from_llm);

        queue.reconcile_instant(
            &live,
            vec![briefed(1, AttentionSource::Instant, false, "automatic")],
        );
        let kept = queue.item(NodeId(1)).unwrap();
        assert_eq!(
            kept.briefing.status_summary.as_deref(),
            Some("from the model")
        );
        assert!(kept.briefing.from_llm);
    }

    #[test]
    fn an_llm_item_without_briefing_text_keeps_the_automatic_one() {
        let mut queue = AttentionQueue::new();
        let live = live(&[1]);
        queue.reconcile_instant(
            &live,
            vec![briefed(1, AttentionSource::Instant, false, "automatic")],
        );
        queue.apply_llm(
            LlmReply {
                generation: 1,
                analysed: vec![(NodeId(1), 101)],
                items: vec![item(1, AttentionKind::Question, 60, AttentionSource::Llm)],
            },
            &live,
        );
        let shown = queue.item(NodeId(1)).unwrap();
        assert_eq!(shown.briefing.status_summary.as_deref(), Some("automatic"));
        assert!(!shown.briefing.from_llm);
    }

    #[test]
    fn a_partial_llm_briefing_is_never_completed_with_automatic_text() {
        let mut automatic = briefed(1, AttentionSource::Instant, false, "automatic");
        automatic.briefing.why_here = Some("automatic why".into());
        let mut model = item(1, AttentionKind::Question, 60, AttentionSource::Llm);
        model.briefing.why_here = Some("model why".into());
        model.briefing.from_llm = true;
        let merged = combine(automatic, model);
        assert_eq!(merged.briefing.why_here.as_deref(), Some("model why"));
        assert_eq!(merged.briefing.status_summary, None);
    }
}
