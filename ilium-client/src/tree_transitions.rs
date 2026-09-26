//! Client-only presentation state for structural tree changes.
//!
//! The server-owned [`ilium_core::Tree`] remains authoritative and changes
//! immediately when a snapshot arrives. This module retains the previous tree
//! only long enough to draw removed rows leaving, then draws newly added rows
//! entering before the existing creation pulse begins. Nothing here crosses
//! IPC or leaks animation concerns into the pure domain model.

use std::collections::{HashMap, HashSet};

use ilium_core::{NodeId, Tree};

/// A short transition is legible without making rapid tree operations feel
/// blocked behind presentation work.
pub const TREE_ENTRY_TRANSITION_MS: u128 = 220;

/// Thirteen terminal cells move a row past its fixed identity and two state
/// slots (eight cells plus indentation), so the title itself visibly slides
/// in both the ordinary and expanded sidebar while enough of a typical label
/// stays visible to retain context during the transition.
const TREE_ENTRY_SLIDE_COLUMNS: u16 = 13;

/// One row's horizontal transform after easing has been sampled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeRowMotion {
    /// Number of terminal columns the complete rendered row moves left from
    /// its settled position.
    pub left_offset: u16,
    /// A discrete terminal-friendly fade: dimming during the less important
    /// half of each transition softens the otherwise abrupt disappearance.
    pub is_dimmed: bool,
}

/// The previous snapshot and each removed id's own departure start time.
///
/// Timing is tracked per node (mirroring `entering_started_offsets`) rather
/// than as one shared clock so that a second removal arriving while an
/// earlier one is still sliding away gets its own start time instead of
/// resetting -- and so the still-valid frozen `tree` (a superset of every
/// node removed since it was captured) is kept until every tracked departure
/// has finished, instead of being replaced mid-animation by a newer snapshot
/// that no longer contains the still-departing rows.
#[derive(Debug, Clone)]
struct ExitTransition {
    tree: Tree,
    node_started_offsets: HashMap<NodeId, u128>,
}

/// Owns all short-lived state needed to animate structural snapshot changes.
#[derive(Debug, Default)]
pub struct TreeTransitions {
    entering_started_offsets: HashMap<NodeId, u128>,
    exit: Option<ExitTransition>,
}

impl TreeTransitions {
    /// Reconciles two authoritative snapshots. Removals render first from the
    /// previous snapshot; insertions in the same change wait until that exit
    /// completes, preventing two rows from crossing through each other.
    pub fn observe_snapshot_change(
        &mut self,
        previous_tree: &Tree,
        new_tree: &Tree,
        now_offset_ms: u128,
    ) -> Vec<(NodeId, u128)> {
        let _ = self.prune(now_offset_ms, new_tree);

        let previous_ids: HashSet<NodeId> = previous_tree.all_ids().collect();
        let new_ids: HashSet<NodeId> = new_tree.all_ids().collect();
        let removed_ids: HashSet<NodeId> = previous_ids.difference(&new_ids).copied().collect();

        if !removed_ids.is_empty() {
            match &mut self.exit {
                // An earlier departure is still in flight: its frozen tree
                // necessarily still contains everything removed since (ids
                // only leave the live tree once), so keep that tree and just
                // give the newly removed ids their own start time.
                Some(exit) => {
                    for node_id in removed_ids.iter().copied() {
                        exit.node_started_offsets.insert(node_id, now_offset_ms);
                    }
                }
                None => {
                    let node_started_offsets = removed_ids
                        .iter()
                        .copied()
                        .map(|node_id| (node_id, now_offset_ms))
                        .collect();
                    self.exit = Some(ExitTransition {
                        tree: previous_tree.clone(),
                        node_started_offsets,
                    });
                }
            }
        }

        let entrance_started_offset_ms = self
            .exit
            .as_ref()
            .and_then(|exit| exit.node_started_offsets.values().copied().max())
            .map_or(now_offset_ms, |latest_exit_started_offset_ms| {
                latest_exit_started_offset_ms.saturating_add(TREE_ENTRY_TRANSITION_MS)
            });
        let pulse_started_offset_ms =
            entrance_started_offset_ms.saturating_add(TREE_ENTRY_TRANSITION_MS);
        let mut created_pulse_starts = Vec::new();

        for node_id in new_ids.difference(&previous_ids).copied() {
            self.entering_started_offsets
                .insert(node_id, entrance_started_offset_ms);
            created_pulse_starts.push((node_id, pulse_started_offset_ms));
        }

        created_pulse_starts
    }

    /// Returns the old snapshot while removed rows are still sliding away.
    /// Rendering it does not change the authoritative tree used for input,
    /// pane ownership, or subsequent snapshot reconciliation.
    pub fn presentation_tree(&self, now_offset_ms: u128) -> Option<&Tree> {
        self.exit.as_ref().and_then(|exit| {
            exit.node_started_offsets
                .values()
                .any(|started_offset_ms| {
                    now_offset_ms.saturating_sub(*started_offset_ms) < TREE_ENTRY_TRANSITION_MS
                })
                .then_some(&exit.tree)
        })
    }

    /// Samples the eased horizontal motion for a row at `now_offset_ms`.
    pub fn row_motion(&self, node_id: NodeId, now_offset_ms: u128) -> Option<TreeRowMotion> {
        if let Some(exit) = &self.exit {
            if let Some(started_offset_ms) = exit.node_started_offsets.get(&node_id) {
                let age_ms = now_offset_ms.saturating_sub(*started_offset_ms);
                if age_ms < TREE_ENTRY_TRANSITION_MS {
                    let progress = normalized_progress(age_ms);
                    return Some(TreeRowMotion {
                        left_offset: eased_columns(ease_in_cubic(progress)),
                        is_dimmed: progress >= 0.5,
                    });
                }
            }
        }

        let started_offset_ms = *self.entering_started_offsets.get(&node_id)?;
        if now_offset_ms < started_offset_ms {
            return None;
        }
        let age_ms = now_offset_ms - started_offset_ms;
        if age_ms >= TREE_ENTRY_TRANSITION_MS {
            return None;
        }
        let progress = normalized_progress(age_ms);
        Some(TreeRowMotion {
            left_offset: TREE_ENTRY_SLIDE_COLUMNS
                .saturating_sub(eased_columns(ease_out_cubic(progress))),
            is_dimmed: progress < 0.5,
        })
    }

    /// Whether the event loop should keep issuing animation-rate frames.
    pub fn is_active(&self, now_offset_ms: u128) -> bool {
        let has_active_exit = self.exit.as_ref().is_some_and(|exit| {
            exit.node_started_offsets.values().any(|started_offset_ms| {
                now_offset_ms.saturating_sub(*started_offset_ms) < TREE_ENTRY_TRANSITION_MS
            })
        });
        has_active_exit
            || self
                .entering_started_offsets
                .values()
                .any(|started_offset_ms| {
                    now_offset_ms < started_offset_ms.saturating_add(TREE_ENTRY_TRANSITION_MS)
                })
    }

    /// Drops completed transitions and entries deleted before they finished
    /// entering, keeping presentation state bounded during heavy pane churn.
    pub fn prune(&mut self, now_offset_ms: u128, live_tree: &Tree) -> bool {
        let had_exit = self.exit.is_some();
        let exit_node_count = self
            .exit
            .as_ref()
            .map_or(0, |exit| exit.node_started_offsets.len());
        let entering_count = self.entering_started_offsets.len();

        if let Some(exit) = &mut self.exit {
            exit.node_started_offsets.retain(|_, started_offset_ms| {
                now_offset_ms.saturating_sub(*started_offset_ms) < TREE_ENTRY_TRANSITION_MS
            });
            if exit.node_started_offsets.is_empty() {
                self.exit = None;
            }
        }

        self.entering_started_offsets
            .retain(|node_id, started_offset_ms| {
                live_tree.get(*node_id).is_some()
                    && now_offset_ms < started_offset_ms.saturating_add(TREE_ENTRY_TRANSITION_MS)
            });

        let new_exit_node_count = self
            .exit
            .as_ref()
            .map_or(0, |exit| exit.node_started_offsets.len());

        had_exit != self.exit.is_some()
            || exit_node_count != new_exit_node_count
            || entering_count != self.entering_started_offsets.len()
    }
}

/// Converts an elapsed millisecond count into the transition's `[0, 1)` range.
fn normalized_progress(age_ms: u128) -> f64 {
    age_ms as f64 / TREE_ENTRY_TRANSITION_MS as f64
}

/// Removal accelerates away, so the row begins gently and clears decisively.
fn ease_in_cubic(progress: f64) -> f64 {
    progress * progress * progress
}

/// Insertion is the spatial inverse: it moves quickly into view and settles
/// softly at the row's final position before the creation blink starts.
fn ease_out_cubic(progress: f64) -> f64 {
    1.0 - (1.0 - progress).powi(3)
}

fn eased_columns(eased_progress: f64) -> u16 {
    (eased_progress * f64::from(TREE_ENTRY_SLIDE_COLUMNS)).round() as u16
}

#[cfg(test)]
mod tests {
    use ilium_core::{PaneContentKind, ROOT_ID};

    use super::*;

    fn tree_with_pane() -> (Tree, NodeId, NodeId) {
        let mut tree = Tree::new();
        let group_id = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = tree
            .add_pane(group_id, "shell", PaneContentKind::Terminal)
            .unwrap();
        (tree, group_id, pane_id)
    }

    #[test]
    fn insertion_moves_right_then_starts_creation_pulse_after_settling() {
        let (previous_tree, group_id, _) = tree_with_pane();
        let mut new_tree = previous_tree.clone();
        let new_pane_id = new_tree
            .add_pane(group_id, "new", PaneContentKind::Terminal)
            .unwrap();
        let mut transitions = TreeTransitions::default();

        let pulse_starts = transitions.observe_snapshot_change(&previous_tree, &new_tree, 1_000);

        assert_eq!(pulse_starts, vec![(new_pane_id, 1_220)]);
        assert_eq!(
            transitions.row_motion(new_pane_id, 1_000),
            Some(TreeRowMotion {
                left_offset: TREE_ENTRY_SLIDE_COLUMNS,
                is_dimmed: true,
            })
        );
        let midpoint = transitions.row_motion(new_pane_id, 1_110).unwrap();
        assert!(midpoint.left_offset < TREE_ENTRY_SLIDE_COLUMNS / 2);
        assert!(!midpoint.is_dimmed);
        assert_eq!(transitions.row_motion(new_pane_id, 1_220), None);
    }

    #[test]
    fn removal_retains_the_previous_tree_and_accelerates_left() {
        let (previous_tree, _, pane_id) = tree_with_pane();
        let mut new_tree = previous_tree.clone();
        new_tree.remove_node(pane_id).unwrap();
        let mut transitions = TreeTransitions::default();

        transitions.observe_snapshot_change(&previous_tree, &new_tree, 500);

        assert_eq!(transitions.presentation_tree(500), Some(&previous_tree));
        assert_eq!(
            transitions.row_motion(pane_id, 500),
            Some(TreeRowMotion {
                left_offset: 0,
                is_dimmed: false,
            })
        );
        let midpoint = transitions.row_motion(pane_id, 610).unwrap();
        assert!(midpoint.left_offset < TREE_ENTRY_SLIDE_COLUMNS / 2);
        assert!(midpoint.is_dimmed);
        assert_eq!(transitions.presentation_tree(720), None);
        assert_eq!(transitions.row_motion(pane_id, 720), None);
    }

    #[test]
    fn insertion_waits_for_a_removal_in_the_same_snapshot() {
        let (previous_tree, group_id, pane_id) = tree_with_pane();
        let mut new_tree = previous_tree.clone();
        new_tree.remove_node(pane_id).unwrap();
        let replacement_id = new_tree
            .add_pane(group_id, "replacement", PaneContentKind::Terminal)
            .unwrap();
        let mut transitions = TreeTransitions::default();

        let pulse_starts = transitions.observe_snapshot_change(&previous_tree, &new_tree, 100);

        assert_eq!(pulse_starts, vec![(replacement_id, 540)]);
        assert_eq!(transitions.row_motion(replacement_id, 319), None);
        assert_eq!(
            transitions.row_motion(replacement_id, 320),
            Some(TreeRowMotion {
                left_offset: TREE_ENTRY_SLIDE_COLUMNS,
                is_dimmed: true,
            })
        );
    }

    #[test]
    fn prune_drops_completed_and_deleted_entry_state() {
        let (previous_tree, group_id, _) = tree_with_pane();
        let mut new_tree = previous_tree.clone();
        let added_id = new_tree
            .add_pane(group_id, "temporary", PaneContentKind::Terminal)
            .unwrap();
        let mut transitions = TreeTransitions::default();
        transitions.observe_snapshot_change(&previous_tree, &new_tree, 0);
        new_tree.remove_node(added_id).unwrap();

        let visible_state_changed = transitions.prune(1, &new_tree);

        assert!(visible_state_changed);
        assert_eq!(transitions.row_motion(added_id, 1), None);
        assert!(!transitions.is_active(TREE_ENTRY_TRANSITION_MS));
    }

    #[test]
    fn a_later_removal_does_not_cut_off_an_earlier_departure_still_in_flight() {
        let mut original_tree = Tree::new();
        let group_id = original_tree.add_group(ROOT_ID, "work").unwrap();
        let first_pane_id = original_tree
            .add_pane(group_id, "first", PaneContentKind::Terminal)
            .unwrap();
        let second_pane_id = original_tree
            .add_pane(group_id, "second", PaneContentKind::Terminal)
            .unwrap();

        let mut after_first_removal = original_tree.clone();
        after_first_removal.remove_node(first_pane_id).unwrap();
        let mut after_second_removal = after_first_removal.clone();
        after_second_removal.remove_node(second_pane_id).unwrap();

        let mut transitions = TreeTransitions::default();
        transitions.observe_snapshot_change(&original_tree, &after_first_removal, 0);
        transitions.observe_snapshot_change(&after_first_removal, &after_second_removal, 100);

        // `after_first_removal` (the second call's "previous" tree) no longer
        // has `first_pane_id`, so the frozen presentation tree must still be
        // the older snapshot that has both rows, not have been replaced by
        // the second call.
        assert_eq!(transitions.presentation_tree(110), Some(&original_tree));

        // The first departure keeps animating on its own original schedule...
        let first_motion = transitions.row_motion(first_pane_id, 110).unwrap();
        assert!(first_motion.left_offset > 0);
        // ...finishing when its own 220ms elapses, unaffected by the later removal.
        assert_eq!(transitions.row_motion(first_pane_id, 220), None);

        // ...while the second, having started 100ms later, is still early
        // in its own departure at the same instant the first one finishes.
        assert!(transitions.row_motion(second_pane_id, 220).is_some());
        assert!(transitions.presentation_tree(220).is_some());

        // Once both departures have completed, the frozen tree is released.
        assert_eq!(transitions.presentation_tree(320), None);
    }

    #[test]
    fn pruning_at_the_duration_boundary_requests_the_final_settled_frame() {
        let (previous_tree, _, pane_id) = tree_with_pane();
        let mut new_tree = previous_tree.clone();
        new_tree.remove_node(pane_id).unwrap();
        let mut transitions = TreeTransitions::default();
        transitions.observe_snapshot_change(&previous_tree, &new_tree, 0);

        assert!(!transitions.is_active(TREE_ENTRY_TRANSITION_MS));
        assert!(transitions.prune(TREE_ENTRY_TRANSITION_MS, &new_tree));
        assert_eq!(
            transitions.presentation_tree(TREE_ENTRY_TRANSITION_MS),
            None
        );
        assert!(!transitions.prune(TREE_ENTRY_TRANSITION_MS + 1, &new_tree));
    }
}
