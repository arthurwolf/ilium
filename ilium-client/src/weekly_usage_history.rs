//! Bounded client-observed ownership periods, independent of the usage view.
use crate::session_stats_store::StatsRequest;
use ilium_core::NodeId;

/// Longest selectable usage range; older ownership can never be displayed.
const RETENTION_MS: i64 = crate::weekly_usage::UsageRange::LONGEST_MS;
const MAX_PERIODS: usize = 512;
const MAX_METADATA_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UsagePeriod {
    pub start_ms: i64,
    pub end_ms: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UsageSource {
    pub key: NodeId,
    pub pane_id: NodeId,
    pub request: StatsRequest,
    /// Custom agent profile that ran the session; `None` for the built-in agent.
    pub profile: Option<String>,
    pub periods: Vec<UsagePeriod>,
    pub ambiguous: bool,
}

struct OwnedPeriod {
    key: NodeId,
    pane_id: NodeId,
    request: StatsRequest,
    profile: Option<String>,
    start_ms: i64,
    end_ms: Option<i64>,
    observed_start_ms: i64,
}

#[derive(Default)]
pub(crate) struct UsageHistory {
    periods: Vec<OwnedPeriod>,
    /// Custom agent profile each pane runs right now; input of the next
    /// `reconcile`. A pane absent here runs the built-in agent.
    pane_profiles: std::collections::HashMap<NodeId, String>,
    next_key: u64,
    /// Earlier evidence was discarded. Never infer uniqueness before here.
    pub coverage_floor_ms: Option<i64>,
    pub revision: u64,
    storage: Option<ilium_execution::StorageAdmission>,
    coverage_gap_open: bool,
}

pub(crate) fn same_session(left: &StatsRequest, right: &StatsRequest) -> bool {
    left.class == right.class && left.session_id == right.session_id && left.home == right.home
}

impl UsageHistory {
    pub fn set_pane_profiles(&mut self, profiles: std::collections::HashMap<NodeId, String>) {
        self.pane_profiles = profiles;
    }
    /// Distinct (provider, custom profile) sections a pane owned within the
    /// retention window, so closed periods keep their own section.
    pub fn sections_of(
        &self,
        pane_id: NodeId,
        now_ms: i64,
    ) -> Vec<(ilium_core::AgentClass, Option<String>)> {
        let mut sections: Vec<(ilium_core::AgentClass, Option<String>)> = Vec::new();
        for period in self.periods.iter().filter(|period| {
            period.pane_id == pane_id
                && period.end_ms.unwrap_or(now_ms) > now_ms.saturating_sub(RETENTION_MS)
        }) {
            let section = (period.request.class.clone(), period.profile.clone());
            if !sections.contains(&section) {
                sections.push(section);
            }
        }
        sections
    }
    /// Called for binding/status/tree events even when the usage view is closed.
    pub fn reconcile(&mut self, owners: &[(NodeId, StatsRequest, i64)], now_ms: i64) {
        let was_gap_open = self.coverage_gap_open;
        if was_gap_open {
            self.coverage_floor_ms = Some(self.coverage_floor_ms.unwrap_or(i64::MIN).max(now_ms));
        }
        self.coverage_gap_open = false;
        if self.storage.is_none() && !owners.is_empty() {
            match crate::execution::process_quota().reserve_external_storage(
                MAX_METADATA_BYTES * 4 + MAX_PERIODS * std::mem::size_of::<OwnedPeriod>(),
            ) {
                // Covers original metadata, the view projection, reconciliation
                // temporaries, and the reader-context copies at their peak.
                Ok(storage) => self.storage = Some(storage),
                Err(_) => {
                    // Missing records cannot be used as evidence of unique
                    // ownership. Keep a global exclusion floor instead.
                    self.coverage_floor_ms = Some(now_ms);
                    self.coverage_gap_open = true;
                    self.revision = self.revision.wrapping_add(1);
                    return;
                }
            }
        }
        let cutoff = now_ms.saturating_sub(RETENTION_MS);
        let old_len = self.periods.len();
        self.periods
            .retain(|period| period.end_ms.is_none_or(|end| end > cutoff));
        let mut changed = old_len != self.periods.len() || was_gap_open;
        for period in self
            .periods
            .iter_mut()
            .filter(|period| period.end_ms.is_none())
        {
            let current = owners.iter().find(|(pane, _, _)| *pane == period.pane_id);
            if current.is_some_and(|(_, request, start)| {
                *request == period.request
                    && *start == period.observed_start_ms
                    && self.pane_profiles.get(&period.pane_id) == period.profile.as_ref()
            }) {
                continue;
            }
            // A renewal has an exact locally observed start; status loss has
            // only the receipt time. Never extend the old period into the new.
            let end = current.map_or(now_ms, |(_, _, start)| (*start).min(now_ms));
            period.end_ms = Some(end.max(period.start_ms));
            changed = true;
        }
        for (pane_id, request, observed_start_ms) in owners {
            if *observed_start_ms <= 0 || *observed_start_ms > now_ms {
                continue;
            }
            if self.periods.iter().any(|period| {
                period.pane_id == *pane_id
                    && period.end_ms.is_none()
                    && period.request == *request
                    && period.profile.as_ref() == self.pane_profiles.get(pane_id)
                    && period.observed_start_ms == *observed_start_ms
            }) {
                continue;
            }
            if request_bytes(request) > MAX_METADATA_BYTES {
                self.coverage_floor_ms =
                    Some(self.coverage_floor_ms.unwrap_or(i64::MIN).max(now_ms));
                self.coverage_gap_open = true;
                changed = true;
                continue;
            }
            // Wall-clock corrections cannot make two successive periods for
            // one pane overlap and count the same usage twice.
            let last_closed_end = self
                .periods
                .iter()
                .filter(|period| period.pane_id == *pane_id)
                .filter_map(|period| period.end_ms)
                .max()
                .unwrap_or(i64::MIN);
            self.next_key = self.next_key.saturating_add(1);
            self.periods.push(OwnedPeriod {
                key: NodeId(self.next_key),
                pane_id: *pane_id,
                request: request.clone(),
                profile: self.pane_profiles.get(pane_id).cloned(),
                start_ms: (*observed_start_ms).max(last_closed_end),
                observed_start_ms: *observed_start_ms,
                end_ms: None,
            });
            changed = true;
        }
        let mut bytes = self.periods.iter().map(period_bytes).sum::<usize>();
        while self.periods.len() > MAX_PERIODS || bytes > MAX_METADATA_BYTES {
            let oldest = self
                .periods
                .iter()
                .enumerate()
                .min_by_key(|(_, period)| period.end_ms.unwrap_or(now_ms))
                .map(|(index, _)| index);
            let Some(index) = oldest else { break };
            let period = self.periods.remove(index);
            bytes = bytes.saturating_sub(period_bytes(&period));
            let lost_until = period.end_ms.unwrap_or(now_ms);
            self.coverage_gap_open |= period.end_ms.is_none();
            self.coverage_floor_ms =
                Some(self.coverage_floor_ms.unwrap_or(i64::MIN).max(lost_until));
            changed = true;
        }
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
    }

    /// Removed panes still participate in overlap checks: closing a duplicate
    /// must not retroactively assign its ambiguous interval to the survivor.
    pub fn sources(
        &self,
        pane_id: NodeId,
        class: &ilium_core::AgentClass,
        profile: Option<&str>,
        now_ms: i64,
    ) -> Vec<UsageSource> {
        let cutoff = now_ms.saturating_sub(RETENTION_MS);
        let floor = self.coverage_floor_ms.unwrap_or(i64::MIN);
        let mut sources: Vec<UsageSource> = Vec::new();
        for period in self.periods.iter().filter(|period| {
            period.pane_id == pane_id
                && period.request.class == *class
                && period.profile.as_deref() == profile
        }) {
            let start = period.start_ms.max(floor);
            let end = period.end_ms.unwrap_or(i64::MAX);
            if start >= end || end <= cutoff {
                continue;
            }
            let mut ranges = vec![(start, end)];
            let mut ambiguous = false;
            for other in self.periods.iter().filter(|other| {
                other.pane_id != pane_id && same_session(&period.request, &other.request)
            }) {
                let other_end = other.end_ms.unwrap_or(i64::MAX);
                let other_start = other.start_ms.max(floor);
                if other_start >= end || other_end <= start {
                    continue;
                }
                ambiguous = true;
                ranges = ranges
                    .into_iter()
                    .flat_map(|(left, right)| {
                        let mut remaining = Vec::with_capacity(2);
                        if left < other_start {
                            remaining.push((left, right.min(other_start)));
                        }
                        if right > other_end {
                            remaining.push((left.max(other_end), right));
                        }
                        remaining.into_iter().filter(|(left, right)| left < right)
                    })
                    .collect();
            }
            let index = sources
                .iter()
                .position(|source| source.request == period.request);
            let source = match index {
                Some(index) => &mut sources[index],
                None => {
                    sources.push(UsageSource {
                        key: period.key,
                        pane_id,
                        request: period.request.clone(),
                        profile: period.profile.clone(),
                        periods: Vec::new(),
                        ambiguous: false,
                    });
                    let index = sources.len() - 1;
                    &mut sources[index]
                }
            };
            source.ambiguous |= ambiguous;
            source
                .periods
                .extend(ranges.into_iter().map(|(start_ms, end_ms)| UsagePeriod {
                    start_ms,
                    // Open endpoints stay stable between refreshes.
                    end_ms: if end_ms == i64::MAX {
                        None
                    } else {
                        Some(end_ms)
                    },
                }));
        }
        sources
    }
}

fn period_bytes(period: &OwnedPeriod) -> usize {
    std::mem::size_of::<OwnedPeriod>()
        + request_bytes(&period.request)
        + period.profile.as_ref().map_or(0, String::capacity)
}
fn request_bytes(request: &StatsRequest) -> usize {
    request.session_id.capacity()
        + request.home.capacity()
        + request.project_path.capacity()
        + request
            .transcript_hint
            .as_ref()
            .map_or(0, |path| path.capacity())
}

impl crate::app::App {
    pub(crate) fn sync_weekly_usage_history(&mut self, now_ms: i64) {
        let owners: Vec<_> = self
            .tree
            .panes()
            .filter_map(|node| {
                let ilium_core::NodeKind::Pane {
                    status: ilium_core::PaneStatus::Agent(_),
                    ..
                } = &node.kind
                else {
                    return None;
                };
                Some((
                    node.id,
                    self.session_stats_request(node.id)?,
                    *self.agent_binding_observed_at_ms.get(&node.id)?,
                ))
            })
            .collect();
        self.weekly_usage_history.set_pane_profiles(
            self.custom_agent_panes
                .iter()
                .map(|(pane, identity)| (*pane, identity.profile_id.clone()))
                .collect(),
        );
        self.weekly_usage_history.reconcile(&owners, now_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::AgentClass;
    fn request(session: &str) -> StatsRequest {
        StatsRequest {
            transcript_hint: None,
            class: AgentClass::Claude,
            session_id: session.into(),
            project_path: "/project".into(),
            home: "/home/test".into(),
        }
    }
    #[test]
    fn weekly_history_keeps_sessions_and_same_session_renewals_while_view_closed() {
        let mut history = UsageHistory::default();
        history.reconcile(&[(NodeId(1), request("a"), 100)], 100);
        history.reconcile(&[(NodeId(1), request("a"), 200)], 200);
        history.reconcile(&[(NodeId(1), request("b"), 300)], 300);
        let sources = history.sources(NodeId(1), &AgentClass::Claude, None, 400);
        assert_eq!(sources.len(), 2);
        assert_eq!(
            sources[0].periods,
            vec![
                UsagePeriod {
                    start_ms: 100,
                    end_ms: Some(200)
                },
                UsagePeriod {
                    start_ms: 200,
                    end_ms: Some(300)
                }
            ]
        );
        assert_eq!(
            sources[1].periods,
            vec![UsagePeriod {
                start_ms: 300,
                end_ms: None
            }]
        );
    }
    #[test]
    fn weekly_history_removed_duplicate_remains_ambiguity_evidence() {
        let mut history = UsageHistory::default();
        history.reconcile(&[(NodeId(1), request("a"), 100)], 100);
        history.reconcile(
            &[
                (NodeId(1), request("a"), 100),
                (NodeId(2), request("a"), 200),
            ],
            200,
        );
        history.reconcile(&[(NodeId(1), request("a"), 100)], 300);
        let sources = history.sources(NodeId(1), &AgentClass::Claude, None, 400);
        assert!(sources[0].ambiguous);
        assert_eq!(
            sources[0].periods,
            vec![
                UsagePeriod {
                    start_ms: 100,
                    end_ms: Some(200)
                },
                UsagePeriod {
                    start_ms: 300,
                    end_ms: None
                }
            ]
        );
    }
    #[test]
    fn weekly_history_capacity_loss_advances_coverage_floor() {
        let mut history = UsageHistory::default();
        for index in 1..=MAX_PERIODS + 2 {
            history.reconcile(
                &[(
                    NodeId(1),
                    request(&format!("session-{index}")),
                    index as i64,
                )],
                index as i64,
            );
        }
        assert!(history.periods.len() <= MAX_PERIODS);
        assert_eq!(history.coverage_floor_ms, Some(3));
        assert!(history
            .sources(NodeId(1), &AgentClass::Claude, None, 1000)
            .iter()
            .flat_map(|source| &source.periods)
            .all(|period| period.start_ms >= 3));
    }
    #[test]
    fn weekly_history_removed_panes_are_not_new_sources_but_closed_periods_survive() {
        let mut history = UsageHistory::default();
        history.reconcile(&[(NodeId(1), request("a"), 100)], 100);
        history.reconcile(&[], 200);
        assert_eq!(
            history.sources(NodeId(1), &AgentClass::Claude, None, 300)[0].periods,
            vec![UsagePeriod {
                start_ms: 100,
                end_ms: Some(200)
            }]
        );
        history.reconcile(&[(NodeId(1), request("b"), 300)], 300);
        assert_eq!(
            history
                .sources(NodeId(1), &AgentClass::Claude, None, 400)
                .len(),
            2
        );
        history.reconcile(&[], 400);
        history.reconcile(&[], RETENTION_MS + 1000);
        assert!(history
            .sources(NodeId(1), &AgentClass::Claude, None, RETENTION_MS + 2000)
            .is_empty());
    }
    #[test]
    fn weekly_history_open_intervals_do_not_change_on_each_projection() {
        let mut history = UsageHistory::default();
        history.reconcile(&[(NodeId(1), request("a"), 100)], 100);
        assert_eq!(
            history.sources(NodeId(1), &AgentClass::Claude, None, 200),
            history.sources(NodeId(1), &AgentClass::Claude, None, RETENTION_MS + 1000)
        );
    }
    #[test]
    fn weekly_history_clock_regression_cannot_overlap_successive_owners() {
        let mut history = UsageHistory::default();
        history.reconcile(&[(NodeId(1), request("a"), 100)], 100);
        history.reconcile(&[(NodeId(1), request("b"), 200)], 200);
        history.reconcile(&[(NodeId(1), request("c"), 150)], 150);
        let sources = history.sources(NodeId(1), &AgentClass::Claude, None, 300);
        assert_eq!(
            sources[0].periods,
            vec![UsagePeriod {
                start_ms: 100,
                end_ms: Some(200)
            }]
        );
        assert_eq!(
            sources[1].periods,
            vec![UsagePeriod {
                start_ms: 200,
                end_ms: None
            }]
        );
        assert_eq!(sources[1].request.session_id, "c");
    }
    #[test]
    fn weekly_history_oversized_metadata_is_unassigned_and_marks_lost_coverage() {
        let mut history = UsageHistory::default();
        let mut oversized = request("a");
        oversized.session_id = "a".repeat(MAX_METADATA_BYTES + 1);
        history.reconcile(&[(NodeId(1), oversized, 100)], 100);
        assert!(history
            .sources(NodeId(1), &AgentClass::Claude, None, 200)
            .is_empty());
        assert_eq!(history.coverage_floor_ms, Some(100));
    }
    #[test]
    fn weekly_history_recovery_excludes_time_with_an_evicted_active_owner() {
        let mut history = UsageHistory::default();
        let owners: Vec<_> = (1..=MAX_PERIODS + 1)
            .map(|index| (NodeId(index as u64), request("shared"), 100))
            .collect();
        history.reconcile(&owners, 200);
        assert!(history.coverage_gap_open);
        assert_eq!(history.coverage_floor_ms, Some(200));
        // The evicted owner remains live until the next full observation.
        history.reconcile(&[(NodeId(2), request("shared"), 100)], 300);
        assert!(!history.coverage_gap_open);
        assert_eq!(history.coverage_floor_ms, Some(300));
        assert_eq!(
            history.sources(NodeId(2), &AgentClass::Claude, None, 400)[0].periods,
            vec![UsagePeriod {
                start_ms: 300,
                end_ms: None
            }]
        );
    }
    #[test]
    fn weekly_history_provider_switch_preserves_both_separate_sources() {
        let mut history = UsageHistory::default();
        let mut codex = request("codex-session");
        codex.class = AgentClass::Codex;
        history.reconcile(&[(NodeId(1), request("claude-session"), 100)], 100);
        history.reconcile(&[(NodeId(1), codex, 200)], 200);
        assert!(history
            .sections_of(NodeId(1), 300)
            .contains(&(AgentClass::Claude, None)));
        assert!(history
            .sections_of(NodeId(1), 300)
            .contains(&(AgentClass::Codex, None)));
        assert_eq!(
            history.sources(NodeId(1), &AgentClass::Claude, None, 300)[0].periods,
            vec![UsagePeriod {
                start_ms: 100,
                end_ms: Some(200)
            }]
        );
        assert_eq!(
            history.sources(NodeId(1), &AgentClass::Codex, None, 300)[0].periods,
            vec![UsagePeriod {
                start_ms: 200,
                end_ms: None
            }]
        );
    }
    #[test]
    fn weekly_history_records_server_binding_changes_without_opening_view() {
        use ilium_core::{AgentActivity, PaneContentKind, PaneStatus, ROOT_ID};
        use ilium_ipc::ServerEvent;
        let mut app = crate::app::App::new("weekly history".into(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "agents").unwrap();
        let pane = app
            .tree
            .add_pane(group, "old title", PaneContentKind::Terminal)
            .unwrap();
        app.tree
            .set_pane_status(
                pane,
                PaneStatus::from_activity(AgentClass::Claude, AgentActivity::Working, None),
            )
            .unwrap();
        for session_id in ["session-a", "session-b"] {
            crate::render_cache::apply(
                &mut app,
                ServerEvent::PaneSessionIdResolved {
                    pane_id: pane,
                    session_id: session_id.into(),
                    process_id: Some(1),
                    process_identity_inode: Some(2),
                    title_generation: 0,
                    transcript_path: None,
                },
            );
        }
        assert!(matches!(app.mode, crate::app::Mode::Normal));
        assert_eq!(app.weekly_usage_history.periods.len(), 2);
        assert_eq!(
            app.weekly_usage_history.periods[0].request.session_id,
            "session-a"
        );
        assert!(app.weekly_usage_history.periods[0].end_ms.is_some());
        assert_eq!(
            app.weekly_usage_history.periods[1].request.session_id,
            "session-b"
        );
    }
}
