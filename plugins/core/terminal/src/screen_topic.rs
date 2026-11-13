//! Rate limit and payload of the topic `ilium.core.terminal/screen-changed`.
//!
//! A pane that redraws constantly would flood subscribers, so each pane publishes at most once
//! per `MIN_INTERVAL_MS` (10 a second). A change inside the quiet window is not lost: it is kept
//! (row range widened, newest revision) and published when the window ends, either by the next
//! event of that pane or by the timer the server arms for the earliest due time.

use std::collections::BTreeMap;

use serde_json::json;

/// The topic of the manifest (`provides.topics.publishes`).
pub const TOPIC: &str = "ilium.core.terminal/screen-changed";
/// Shortest time between two publications of one pane.
pub const MIN_INTERVAL_MS: u64 = 100;

/// Rows of one pane that changed, as the `screen-changed` event reported them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    pub pane: u64,
    pub first_row: u32,
    pub last_row: u32,
    pub revision: u64,
}

impl Change {
    fn merged_with(self, newer: Change) -> Change {
        Change {
            pane: self.pane,
            first_row: self.first_row.min(newer.first_row),
            last_row: self.last_row.max(newer.last_row),
            revision: self.revision.max(newer.revision),
        }
    }

    /// The JSON payload: `pane`, the changed rows and the screen `revision`. `generation` repeats
    /// the revision under the name the trigger and detector plugins read.
    pub fn payload(&self) -> String {
        json!({
            "pane": self.pane,
            "first-row": self.first_row,
            "last-row": self.last_row,
            "revision": self.revision,
            "generation": self.revision,
        })
        .to_string()
    }
}

/// What the limiter decided for one offered change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offer {
    /// Publish this change now (it may include earlier deferred rows).
    Publish(Change),
    /// Keep it; the pane may publish again at `due` (milliseconds of the clock passed to `offer`).
    Deferred { due: u64 },
}

#[derive(Debug, Clone, Copy)]
struct PaneState {
    last_published: u64,
    pending: Option<Change>,
}

/// Per-pane publication limiter. Time is any monotonic millisecond clock.
#[derive(Debug, Default)]
pub struct Limiter {
    panes: BTreeMap<u64, PaneState>,
}

impl Limiter {
    /// Decides what to do with `change` at `now`.
    pub fn offer(&mut self, change: Change, now: u64) -> Offer {
        let Some(state) = self.panes.get_mut(&change.pane) else {
            self.panes.insert(
                change.pane,
                PaneState {
                    last_published: now,
                    pending: None,
                },
            );
            return Offer::Publish(change);
        };
        let merged = state
            .pending
            .map_or(change, |earlier| earlier.merged_with(change));
        let due = state.last_published.saturating_add(MIN_INTERVAL_MS);
        if now >= due {
            state.last_published = now;
            state.pending = None;
            Offer::Publish(merged)
        } else {
            state.pending = Some(merged);
            Offer::Deferred { due }
        }
    }

    /// Takes every deferred change whose quiet window has ended at `now`, and says when the next
    /// remaining one is due.
    pub fn take_due(&mut self, now: u64) -> (Vec<Change>, Option<u64>) {
        let mut ready = Vec::new();
        let mut next_due: Option<u64> = None;
        for state in self.panes.values_mut() {
            let Some(pending) = state.pending else {
                continue;
            };
            let due = state.last_published.saturating_add(MIN_INTERVAL_MS);
            if now >= due {
                state.last_published = now;
                state.pending = None;
                ready.push(pending);
            } else {
                next_due = Some(next_due.map_or(due, |current| current.min(due)));
            }
        }
        (ready, next_due)
    }

    /// Forgets a pane that was closed, so its state does not grow without bound.
    pub fn forget(&mut self, pane: u64) {
        self.panes.remove(&pane);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(pane: u64, first_row: u32, last_row: u32, revision: u64) -> Change {
        Change {
            pane,
            first_row,
            last_row,
            revision,
        }
    }

    #[test]
    fn the_first_change_of_a_pane_publishes_at_once() {
        let mut limiter = Limiter::default();
        assert_eq!(
            limiter.offer(change(1, 0, 3, 5), 1_000),
            Offer::Publish(change(1, 0, 3, 5))
        );
    }

    #[test]
    fn a_change_inside_the_window_is_deferred_until_the_window_ends() {
        let mut limiter = Limiter::default();
        limiter.offer(change(1, 0, 3, 5), 1_000);
        assert_eq!(
            limiter.offer(change(1, 2, 8, 6), 1_040),
            Offer::Deferred { due: 1_100 }
        );
    }

    #[test]
    fn the_next_event_after_the_window_carries_the_deferred_rows() {
        let mut limiter = Limiter::default();
        limiter.offer(change(1, 4, 5, 1), 0);
        limiter.offer(change(1, 2, 3, 2), 30);
        assert_eq!(
            limiter.offer(change(1, 9, 9, 3), 100),
            Offer::Publish(change(1, 2, 9, 3))
        );
    }

    #[test]
    fn forty_events_in_one_second_publish_at_most_ten_times() {
        let mut limiter = Limiter::default();
        let published = (0..40u64)
            .filter(|step| {
                matches!(
                    limiter.offer(change(7, 0, 1, step + 1), step * 25),
                    Offer::Publish(_)
                )
            })
            .count();
        assert_eq!(published, 10);
    }

    #[test]
    fn panes_are_limited_independently() {
        let mut limiter = Limiter::default();
        limiter.offer(change(1, 0, 0, 1), 0);
        assert_eq!(
            limiter.offer(change(2, 0, 0, 1), 10),
            Offer::Publish(change(2, 0, 0, 1))
        );
    }

    #[test]
    fn take_due_releases_a_deferred_change_when_its_window_ends() {
        let mut limiter = Limiter::default();
        limiter.offer(change(1, 0, 0, 1), 0);
        limiter.offer(change(1, 3, 4, 2), 10);
        assert_eq!(limiter.take_due(50), (Vec::new(), Some(100)));
        assert_eq!(limiter.take_due(100), (vec![change(1, 3, 4, 2)], None));
        assert_eq!(limiter.take_due(500), (Vec::new(), None));
    }

    #[test]
    fn the_payload_names_pane_rows_revision_and_generation() {
        let value: serde_json::Value =
            serde_json::from_str(&change(7, 2, 4, 9).payload()).expect("json");
        assert_eq!(value["pane"], 7);
        assert_eq!(value["first-row"], 2);
        assert_eq!(value["last-row"], 4);
        assert_eq!(value["revision"], 9);
        assert_eq!(value["generation"], 9);
    }

    #[test]
    fn a_forgotten_pane_publishes_at_once_again() {
        let mut limiter = Limiter::default();
        limiter.offer(change(1, 0, 0, 1), 0);
        limiter.forget(1);
        assert!(matches!(
            limiter.offer(change(1, 0, 0, 2), 5),
            Offer::Publish(_)
        ));
    }
}
