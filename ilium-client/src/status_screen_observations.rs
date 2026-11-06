//! Connection-scoped, revision-fenced screen-match observations.
//!
//! Terminal text stays on the server. A missing, stale or unknown observation
//! is never interpreted as a negative match, including under NOT.

use std::collections::HashMap;

use ilium_core::{
    status_rule_expression::{Condition, Observation, ScreenPattern, StatusRules},
    NodeId,
};
use ilium_ipc::{StatusScreenObservation, StatusScreenWatch, StatusScreenWatchRejection};

/// One pane's accepted answers and when this client received them. The
/// server sends only changed answers, so `received_at` is when the answer
/// last changed, not when the screen was last checked.
struct PaneObservation {
    observation: StatusScreenObservation,
    received_at: std::time::Instant,
}

#[derive(Default)]
pub struct ScreenObservations {
    revision: u64,
    watches: Vec<StatusScreenWatch>,
    panes: HashMap<NodeId, PaneObservation>,
    retry_after: Option<std::time::Instant>,
    rejection: Option<StatusScreenWatchRejection>,
}

impl ScreenObservations {
    pub fn has_watches(&self) -> bool {
        !self.watches.is_empty()
    }

    fn watches_for(rules: &StatusRules) -> Vec<StatusScreenWatch> {
        let mut patterns: Vec<ScreenPattern> = Vec::new();
        for rule in rules.rules.iter().filter(|rule| rule.enabled) {
            for term in rule.any_of.iter().flat_map(|group| &group.all_of) {
                if let Condition::ScreenMatches(pattern) = &term.condition {
                    if !patterns.contains(pattern) {
                        patterns.push(pattern.clone());
                    }
                }
            }
        }
        patterns
            .into_iter()
            .take(ilium_core::status_rule_expression::MAXIMUM_ENABLED_SCREEN_PATTERNS)
            .enumerate()
            .map(|(index, pattern)| StatusScreenWatch {
                id: index as u16,
                pattern,
            })
            .collect()
    }

    pub fn watches_match(&self, rules: &StatusRules) -> bool {
        self.retry_after.is_none() && self.watches == Self::watches_for(rules)
    }

    pub fn retry_due(&self, now: std::time::Instant) -> bool {
        self.retry_after.is_some_and(|deadline| now >= deadline)
    }

    pub fn set_subscription_admitted(&mut self, admitted: bool) {
        self.retry_after =
            (!admitted).then(|| std::time::Instant::now() + std::time::Duration::from_secs(1));
    }

    /// Compile the distinct enabled patterns into stable identifiers for a
    /// single connection revision. Replacing a watch set invalidates all old
    /// answers immediately; disconnected clients must call `clear` as well.
    pub fn replace_rules(&mut self, rules: &StatusRules) -> (u64, Vec<StatusScreenWatch>) {
        self.revision = self.revision.wrapping_add(1).max(1);
        self.panes.clear();
        self.rejection = None;
        self.watches = Self::watches_for(rules);
        (self.revision, self.watches.clone())
    }

    pub fn clear(&mut self) {
        self.revision = self.revision.wrapping_add(1).max(1);
        self.watches.clear();
        self.panes.clear();
        self.retry_after = None;
        self.rejection = None;
    }

    /// Record the server's refusal of the current revision. Every watch then
    /// stays Unknown (the server runs no observer for it) until the rules
    /// change; resending the same set would only be refused again, so no
    /// retry is scheduled. A refusal of an older revision is ignored.
    pub fn reject(&mut self, rejection: StatusScreenWatchRejection) -> bool {
        if rejection.revision != self.revision {
            return false;
        }
        self.panes.clear();
        self.rejection = Some(rejection);
        true
    }

    /// The server's refusal of the current watch set, for inline display.
    pub fn rejection(&self) -> Option<&StatusScreenWatchRejection> {
        self.rejection.as_ref()
    }

    /// The pattern the refusal names, when it names one watch.
    pub fn rejected_pattern(&self) -> Option<&ScreenPattern> {
        let watch_id = self.rejection.as_ref()?.watch_id?;
        self.watches
            .iter()
            .find(|watch| watch.id == watch_id)
            .map(|watch| &watch.pattern)
    }

    /// When this client last received a changed answer for `pane_id` under
    /// the current revision; `None` before the first answer.
    pub fn last_changed_at(&self, pane_id: NodeId) -> Option<std::time::Instant> {
        self.panes.get(&pane_id).map(|pane| pane.received_at)
    }

    /// Ignore late replies from an obsolete subscription or older screen.
    pub fn update(&mut self, observation: StatusScreenObservation) -> bool {
        if observation.revision != self.revision
            || self.rejection.is_some()
            || self.panes.get(&observation.pane_id).is_some_and(|old| {
                old.observation.screen_generation > observation.screen_generation
            })
        {
            return false;
        }
        self.panes.insert(
            observation.pane_id,
            PaneObservation {
                observation,
                received_at: std::time::Instant::now(),
            },
        );
        true
    }

    pub fn observe(&self, pane_id: NodeId, pattern: &ScreenPattern) -> Observation {
        let Some(watch) = self.watches.iter().find(|watch| &watch.pattern == pattern) else {
            return Observation::Unknown;
        };
        let Some(PaneObservation { observation, .. }) = self.panes.get(&pane_id) else {
            return Observation::Unknown;
        };
        if observation.unknown.contains(&watch.id) {
            Observation::Unknown
        } else if observation.matched.contains(&watch.id) {
            Observation::True
        } else {
            Observation::False
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::status_rule_expression::{default_rules, ScreenRegion};

    #[test]
    fn refused_subscription_is_marked_for_delayed_retry() {
        let rules = default_rules();
        let mut observations = ScreenObservations::default();
        observations.replace_rules(&rules);
        observations.set_subscription_admitted(false);
        assert!(!observations.watches_match(&rules));
        assert!(
            observations.retry_due(std::time::Instant::now() + std::time::Duration::from_secs(2))
        );
        observations.set_subscription_admitted(true);
        assert!(observations.watches_match(&rules));
        assert!(
            !observations.retry_due(std::time::Instant::now() + std::time::Duration::from_secs(2))
        );
    }

    #[test]
    fn unknown_stale_and_superseded_observations_never_become_false() {
        let mut rules = default_rules();
        let rule = rules
            .rules
            .iter_mut()
            .find(|rule| rule.id == "screen-error-example")
            .unwrap();
        rule.enabled = true;
        let pattern = ScreenPattern {
            pattern: "error".into(),
            ignore_case: true,
            region: ScreenRegion::All,
        };
        rule.any_of[0].all_of[0].condition = Condition::ScreenMatches(pattern.clone());
        let mut observations = ScreenObservations::default();
        let (revision, watches) = observations.replace_rules(&rules);
        let pane_id = NodeId(7);
        assert_eq!(
            observations.observe(pane_id, &pattern),
            Observation::Unknown
        );
        let event = StatusScreenObservation {
            pane_id,
            revision,
            screen_generation: 4,
            matched: vec![watches[0].id],
            unknown: vec![],
        };
        assert!(observations.update(event.clone()));
        assert_eq!(observations.observe(pane_id, &pattern), Observation::True);
        rules.rules[0].name = "renamed".into();
        assert!(observations.watches_match(&rules));
        rules
            .rules
            .iter_mut()
            .find(|rule| rule.id == "screen-error-example")
            .unwrap()
            .enabled = false;
        assert!(!observations.watches_match(&rules));
        assert!(!observations.update(StatusScreenObservation {
            screen_generation: 3,
            matched: vec![],
            ..event.clone()
        }));
        assert_eq!(observations.observe(pane_id, &pattern), Observation::True);
        observations.replace_rules(&rules);
        assert!(!observations.update(event));
        assert_eq!(
            observations.observe(pane_id, &pattern),
            Observation::Unknown
        );
    }

    fn enabled_example() -> (StatusRules, ScreenPattern) {
        let mut rules = default_rules();
        let example = rules
            .rules
            .iter_mut()
            .find(|rule| rule.id == "screen-error-example")
            .unwrap();
        example.enabled = true;
        let pattern = match &example.any_of[0].all_of[0].condition {
            Condition::ScreenMatches(pattern) => pattern.clone(),
            _ => panic!("the example is a screen pattern"),
        };
        (rules, pattern)
    }

    #[test]
    fn a_rejected_revision_keeps_every_watch_unknown_until_the_rules_change() {
        let (rules, pattern) = enabled_example();
        let mut observations = ScreenObservations::default();
        let (revision, watches) = observations.replace_rules(&rules);
        let pane_id = NodeId(3);
        assert!(observations.update(StatusScreenObservation {
            pane_id,
            revision,
            screen_generation: 1,
            matched: vec![watches[0].id],
            unknown: vec![],
        }));
        assert!(observations.last_changed_at(pane_id).is_some());
        // A refusal of an older revision is stale and changes nothing.
        assert!(!observations.reject(StatusScreenWatchRejection {
            revision: revision.wrapping_sub(1),
            watch_id: Some(0),
            message: "old".into(),
        }));
        assert_eq!(observations.observe(pane_id, &pattern), Observation::True);
        assert!(observations.rejection().is_none());
        // Refusing the current revision drops answers and blocks late ones.
        assert!(observations.reject(StatusScreenWatchRejection {
            revision,
            watch_id: Some(watches[0].id),
            message: "Invalid screen watch 0".into(),
        }));
        assert_eq!(
            observations.observe(pane_id, &pattern),
            Observation::Unknown
        );
        assert_eq!(observations.rejected_pattern(), Some(&pattern));
        assert!(observations.last_changed_at(pane_id).is_none());
        assert!(!observations.update(StatusScreenObservation {
            pane_id,
            revision,
            screen_generation: 2,
            matched: vec![watches[0].id],
            unknown: vec![],
        }));
        assert_eq!(
            observations.observe(pane_id, &pattern),
            Observation::Unknown
        );
        // No automatic retry: the same set would be refused again.
        assert!(
            !observations.retry_due(std::time::Instant::now() + std::time::Duration::from_secs(5))
        );
        // A set-wide refusal names no pattern.
        let (revision, _) = observations.replace_rules(&rules);
        assert!(observations.rejection().is_none());
        assert!(observations.reject(StatusScreenWatchRejection {
            revision,
            watch_id: None,
            message: "count".into()
        }));
        assert_eq!(observations.rejected_pattern(), None);
        observations.clear();
        assert!(observations.rejection().is_none());
    }

    #[test]
    fn duplicate_patterns_share_a_watch_and_unknown_wins_over_matched() {
        let mut rules = default_rules();
        let example = rules
            .rules
            .iter_mut()
            .find(|rule| rule.id == "screen-error-example")
            .unwrap();
        example.enabled = true;
        let pattern = match &example.any_of[0].all_of[0].condition {
            Condition::ScreenMatches(pattern) => pattern.clone(),
            _ => panic!("the example is a screen pattern"),
        };
        let mut duplicate = example.clone();
        duplicate.id = "duplicate-pattern".into();
        rules.rules.push(duplicate);
        let mut observations = ScreenObservations::default();
        let (revision, watches) = observations.replace_rules(&rules);
        assert_eq!(watches.len(), 1);
        let pane_id = NodeId(1);
        assert!(observations.update(StatusScreenObservation {
            pane_id,
            revision,
            screen_generation: 1,
            matched: vec![watches[0].id],
            unknown: vec![watches[0].id],
        }));
        assert_eq!(
            observations.observe(pane_id, &pattern),
            Observation::Unknown
        );
        observations.clear();
        assert_eq!(
            observations.observe(pane_id, &pattern),
            Observation::Unknown
        );
    }
}
