//! Bounded session monitoring events, independent of terminal-screen traffic.

use std::collections::{HashMap, VecDeque};

use ilium_core::NodeId;
use ilium_ipc::{MonitoringEvent, MonitoringEventKind};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CursorError {
    Expired { oldest_cursor: u64 },
    Future { latest_cursor: u64 },
    Exhausted,
}

pub(crate) struct MonitoringEventLog {
    events: VecDeque<MonitoringEvent>,
    capacity: std::num::NonZeroUsize,
    latest_cursor: u64,
    resource_revision: Option<u64>,
    epoch: uuid::Uuid,
    context: HashMap<NodeId, PaneContext>,
}

struct PaneContext {
    project: Option<String>,
    title: String,
    agent: Option<String>,
    working: bool,
    generation: u64,
    last_kind: Option<MonitoringEventKind>,
    state: &'static str,
    state_since: Option<std::time::Instant>,
    repeated_error: Option<(String, usize)>,
    signals: Vec<String>,
}

fn bounded_text(text: &str) -> String {
    text.chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .take(120)
        .collect()
}

impl MonitoringEventLog {
    pub(crate) fn new(capacity: std::num::NonZeroUsize) -> Self {
        Self {
            events: VecDeque::new(),
            capacity,
            latest_cursor: 0,
            resource_revision: None,
            epoch: uuid::Uuid::new_v4(),
            context: HashMap::new(),
        }
    }

    pub(crate) fn epoch(&self) -> uuid::Uuid {
        self.epoch
    }

    pub(crate) fn resume_cursor(&self) -> u64 {
        self.events
            .front()
            .map(|event| event.cursor.saturating_sub(1))
            .unwrap_or(self.latest_cursor)
    }

    pub(crate) fn latest_cursor(&self) -> u64 {
        self.latest_cursor
    }

    pub(crate) fn working_count(&self) -> usize {
        self.context.values().filter(|pane| pane.working).count()
    }

    pub(crate) fn state_age_seconds(&self, pane: NodeId, state: &str) -> Option<u64> {
        let context = self.context.get(&pane)?;
        if context.state != state {
            return None;
        }
        Some(context.state_since?.elapsed().as_secs())
    }

    pub(crate) fn observe_state(&mut self, pane: NodeId, state: &'static str) -> Option<u64> {
        let context = self.context.get_mut(&pane)?;
        if context.state != state || context.state_since.is_none() {
            context.state = state;
            context.state_since = Some(std::time::Instant::now());
        }
        context.state_since.map(|since| since.elapsed().as_secs())
    }

    pub(crate) fn repeated_error(&self, pane: NodeId) -> Option<(String, usize)> {
        self.context.get(&pane)?.repeated_error.clone()
    }

    pub(crate) fn changed_signals(
        &mut self,
        pane: NodeId,
        signals: Vec<ilium_control::signals::Signal>,
        message_observed: bool,
    ) -> Vec<ilium_control::signals::Signal> {
        let Some(context) = self.context.get_mut(&pane) else {
            return Vec::new();
        };
        let mut current: Vec<_> = signals
            .iter()
            .map(|signal| format!("{}:{}", signal.kind.as_str(), signal.summary))
            .collect();
        let changed = signals
            .into_iter()
            .zip(&current)
            .filter_map(|(signal, key)| (!context.signals.contains(key)).then_some(signal))
            .collect();
        if !message_observed {
            current.extend(
                context
                    .signals
                    .iter()
                    .filter(|key| key.starts_with("needs_input:"))
                    .cloned(),
            );
        }
        context.signals = current;
        changed
    }

    pub(crate) fn record_resource_revision(
        &mut self,
        revision: u64,
        now: u64,
    ) -> Result<(), CursorError> {
        if self
            .resource_revision
            .is_some_and(|previous| revision <= previous)
        {
            return Ok(());
        }
        self.append(
            None,
            MonitoringEventKind::ResourceChanged,
            now,
            "Resource catalog changed",
        )?;
        self.resource_revision = Some(revision);
        Ok(())
    }

    pub(crate) fn append(
        &mut self,
        pane_id: Option<NodeId>,
        kind: MonitoringEventKind,
        observed_at_unix_millis: u64,
        summary: &str,
    ) -> Result<u64, CursorError> {
        let cursor = self
            .latest_cursor
            .checked_add(1)
            .ok_or(CursorError::Exhausted)?;
        let previous = self.events.iter().rev().find(|event| {
            event.pane_id == pane_id
                && matches!(
                    event.kind,
                    MonitoringEventKind::Started
                        | MonitoringEventKind::Idle
                        | MonitoringEventKind::Done
                        | MonitoringEventKind::Closed
                )
        });
        let context = pane_id.and_then(|id| self.context.get(&id));
        let previous_kind = context
            .and_then(|context| context.last_kind)
            .or_else(|| previous.map(|event| event.kind));
        let previous_generation = context
            .map(|context| context.generation)
            .or_else(|| previous.map(|event| event.generation))
            .unwrap_or(0);
        let generation = if kind == MonitoringEventKind::Started
            && previous_kind != Some(MonitoringEventKind::Started)
        {
            cursor
        } else {
            previous_generation
        };
        if let Some(context) = pane_id.and_then(|id| self.context.get_mut(&id)) {
            context.generation = generation;
            if kind == MonitoringEventKind::Started && previous_generation != generation {
                context.repeated_error = None;
                context.signals.clear();
            }
            if kind == MonitoringEventKind::Error {
                let text = bounded_text(summary);
                context.repeated_error = Some(match context.repeated_error.take() {
                    Some((previous, count)) if previous == text => (text, count.saturating_add(1)),
                    _ => (text, 1),
                });
            }
            if matches!(
                kind,
                MonitoringEventKind::Started
                    | MonitoringEventKind::Idle
                    | MonitoringEventKind::Done
                    | MonitoringEventKind::Closed
            ) {
                context.last_kind = Some(kind);
            }
        }
        if self.events.len() == self.capacity.get() {
            self.events.pop_front();
        }
        let context = pane_id.and_then(|id| self.context.get(&id));
        self.events.push_back(MonitoringEvent {
            generation,
            cursor,
            pane_id,
            project: context.and_then(|context| context.project.clone()),
            title: context.map(|context| context.title.clone()),
            agent: context.and_then(|context| context.agent.clone()),
            kind,
            observed_at_unix_millis,
            summary: bounded_text(summary),
        });
        self.latest_cursor = cursor;
        Ok(cursor)
    }

    pub(crate) fn read_after(&self, cursor: u64) -> Result<Vec<MonitoringEvent>, CursorError> {
        if cursor > self.latest_cursor {
            return Err(CursorError::Future {
                latest_cursor: self.latest_cursor,
            });
        }
        if let Some(oldest) = self.events.front() {
            if cursor < oldest.cursor - 1 {
                return Err(CursorError::Expired {
                    oldest_cursor: oldest.cursor,
                });
            }
        }
        Ok(self
            .events
            .iter()
            .filter(|event| event.cursor > cursor)
            .cloned()
            .collect())
    }

    pub(crate) fn record_server_event(
        &mut self,
        event: &ilium_ipc::ServerEvent,
        now: u64,
    ) -> Result<(), CursorError> {
        use ilium_core::{AgentActivity, GoalState};
        use ilium_ipc::ServerEvent;

        let (pane_id, status) = match event {
            ServerEvent::TreeSnapshot(tree) => {
                self.replace_tree_context(tree);
                return Ok(());
            }
            ServerEvent::PaneProgressChanged {
                pane_id,
                progress_monitors,
            } => {
                if progress_monitors.is_empty() {
                    self.append(
                        Some(*pane_id),
                        MonitoringEventKind::Progress,
                        now,
                        "Progress monitors cleared",
                    )?;
                }
                for monitor in progress_monitors {
                    if let ilium_core::ProgressMonitorHealth::Failed { last_error, .. } =
                        &monitor.monitor_health
                    {
                        self.append(
                            Some(*pane_id),
                            MonitoringEventKind::Error,
                            now,
                            &format!("Monitor failed: {last_error}"),
                        )?;
                        continue;
                    }
                    let (kind, summary) =
                        if monitor.report.status == ilium_core::ProgressTaskStatus::Error {
                            (
                                MonitoringEventKind::Error,
                                monitor
                                    .report
                                    .error
                                    .as_deref()
                                    .unwrap_or(&monitor.report.message),
                            )
                        } else {
                            (
                                MonitoringEventKind::Progress,
                                monitor.report.message.as_str(),
                            )
                        };
                    self.append(Some(*pane_id), kind, now, summary)?;
                }
                return Ok(());
            }
            ServerEvent::ResourceWaitChanged { pane_id, .. } => {
                self.append(
                    Some(*pane_id),
                    MonitoringEventKind::ResourceChanged,
                    now,
                    "Resource wait changed",
                )?;
                return Ok(());
            }
            ServerEvent::PaneStatusChanged { pane_id, status }
            | ServerEvent::PaneDetectedStateChanged {
                pane_id, status, ..
            } => (*pane_id, status),
            _ => return Ok(()),
        };
        if let Some(context) = self.context.get_mut(&pane_id) {
            let current = ilium_control::pane_state_name(status);
            if context.state != current || context.state_since.is_none() {
                context.state = current;
                context.state_since = Some(std::time::Instant::now());
            }
            context.working = status
                .agent_state()
                .is_some_and(|agent| agent.activity() == AgentActivity::Working);
        }
        let Some(agent) = status.agent_state() else {
            return Ok(());
        };
        if let Some(context) = self.context.get_mut(&pane_id) {
            context.agent = Some(bounded_text(&agent.class.label().to_ascii_lowercase()));
        }
        let (kind, summary) = match agent.activity() {
            AgentActivity::WaitingApproval => (
                MonitoringEventKind::NeedsInput,
                "Waiting for an answer or approval",
            ),
            _ if matches!(
                agent.goal,
                Some(GoalState::Blocked | GoalState::UsageLimited)
            ) =>
            {
                (
                    MonitoringEventKind::Stuck,
                    "Agent goal is blocked or usage limited",
                )
            }
            AgentActivity::Working => (MonitoringEventKind::Started, "Agent working"),
            AgentActivity::Done => (MonitoringEventKind::Done, "Agent finished"),
            AgentActivity::Idle => (MonitoringEventKind::Idle, "Agent idle"),
            AgentActivity::WaitingBackground | AgentActivity::BackgroundTaskStillRunning => {
                (MonitoringEventKind::Progress, "Waiting for background work")
            }
        };
        self.append(Some(pane_id), kind, now, summary)?;
        Ok(())
    }

    pub(crate) fn replace_tree_context(&mut self, tree: &ilium_core::Tree) {
        let mut previous = std::mem::take(&mut self.context);
        self.context = tree
            .panes()
            .map(|pane| {
                let project = tree
                    .project_ancestor(pane.id)
                    .and_then(|id| tree.get(id))
                    .map(|node| bounded_text(&node.name));
                let agent = match &pane.kind {
                    ilium_core::NodeKind::Pane { status, .. } => status
                        .agent_state()
                        .map(|agent| bounded_text(&agent.class.label().to_ascii_lowercase())),
                    _ => None,
                };
                let working = match &pane.kind {
                    ilium_core::NodeKind::Pane { status, .. } => {
                        status.agent_state().is_some_and(|agent| {
                            agent.activity() == ilium_core::AgentActivity::Working
                        })
                    }
                    _ => false,
                };
                let state = match &pane.kind {
                    ilium_core::NodeKind::Pane { status, .. } => {
                        ilium_control::pane_state_name(status)
                    }
                    _ => "unknown",
                };
                let previous = previous.remove(&pane.id);
                let state_since = previous
                    .as_ref()
                    .filter(|context| context.state == state)
                    .and_then(|context| context.state_since);
                let generation = previous.as_ref().map_or(0, |context| context.generation);
                let repeated_error = previous
                    .as_ref()
                    .and_then(|context| context.repeated_error.clone());
                let signals = previous
                    .as_ref()
                    .map(|context| context.signals.clone())
                    .unwrap_or_default();
                let last_kind = previous.and_then(|context| context.last_kind);
                (
                    pane.id,
                    PaneContext {
                        project,
                        title: bounded_text(&pane.name),
                        agent,
                        working,
                        generation,
                        last_kind,
                        state,
                        state_since,
                        repeated_error,
                        signals,
                    },
                )
            })
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use ilium_core::NodeId;
    use ilium_ipc::MonitoringEventKind;

    use super::{CursorError, MonitoringEventLog};

    #[test]
    fn monitoring_signals_do_not_restart_the_work_generation_or_repeat_unchanged_questions() {
        use ilium_control::signals::Signal;
        use ilium_core::{AgentActivity, AgentClass, PaneContentKind, PaneStatus, Tree};
        let mut tree = Tree::new();
        let project = tree.add_project("/fixture/signals".into()).unwrap();
        let pane = tree
            .add_pane(project, "fixture", PaneContentKind::Terminal)
            .unwrap();
        let mut log = MonitoringEventLog::new(std::num::NonZeroUsize::new(16).unwrap());
        log.replace_tree_context(&tree);
        let working = ilium_ipc::ServerEvent::PaneStatusChanged {
            pane_id: pane,
            status: PaneStatus::from_activity(AgentClass::Codex, AgentActivity::Working, None),
        };
        log.record_server_event(&working, 0).unwrap();
        let generation = log.read_after(0).unwrap()[0].generation;
        for now in 1..=3 {
            log.append(
                Some(pane),
                MonitoringEventKind::Error,
                now,
                "connection refused",
            )
            .unwrap();
        }
        log.append(
            Some(pane),
            MonitoringEventKind::Stuck,
            4,
            "Repeated error reports: connection refused",
        )
        .unwrap();
        log.record_server_event(&working, 5).unwrap();
        assert_eq!(
            log.context[&pane].generation, generation,
            "signal publication cannot start a new turn"
        );
        assert_eq!(
            log.repeated_error(pane),
            Some(("connection refused".into(), 3))
        );
        let question = || Signal {
            kind: MonitoringEventKind::NeedsInput,
            summary: "Which fixture?".into(),
        };
        assert_eq!(log.changed_signals(pane, vec![question()], true).len(), 1);
        assert!(log.changed_signals(pane, Vec::new(), false).is_empty());
        assert!(log.changed_signals(pane, vec![question()], true).is_empty());
        log.changed_signals(pane, Vec::new(), true);
        assert_eq!(log.changed_signals(pane, vec![question()], true).len(), 1);
    }

    #[test]
    fn bounded_history_reports_eviction_without_reusing_cursors() {
        let mut log = MonitoringEventLog::new(std::num::NonZeroUsize::new(2).unwrap());
        assert_eq!(
            log.append(Some(NodeId(5)), MonitoringEventKind::Started, 10, "started"),
            Ok(1)
        );
        assert_eq!(
            log.append(Some(NodeId(5)), MonitoringEventKind::Idle, 20, "idle"),
            Ok(2)
        );
        assert_eq!(
            log.append(Some(NodeId(5)), MonitoringEventKind::Done, 30, "done"),
            Ok(3)
        );

        assert_eq!(
            log.read_after(0),
            Err(CursorError::Expired { oldest_cursor: 2 })
        );
        let events = log
            .read_after(1)
            .expect("cursor immediately before retained history");
        assert_eq!(
            events.iter().map(|event| event.cursor).collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert_eq!(events[1].summary, "done");
        assert!(log.read_after(3).expect("current cursor").is_empty());
        assert_eq!(
            log.read_after(4),
            Err(CursorError::Future { latest_cursor: 3 })
        );
    }

    #[test]
    fn empty_history_and_unicode_summaries_are_bounded() {
        let mut log = MonitoringEventLog::new(std::num::NonZeroUsize::new(1).unwrap());
        assert!(log.read_after(0).unwrap().is_empty());
        log.append(None, MonitoringEventKind::Error, 0, &"é".repeat(130))
            .unwrap();
        let events = log.read_after(0).unwrap();
        assert_eq!(events[0].summary.chars().count(), 120);
        assert_eq!(events[0].pane_id, None);
    }

    #[test]
    fn independent_logs_have_distinct_cursor_epochs() {
        let first = MonitoringEventLog::new(std::num::NonZeroUsize::new(1).unwrap());
        let second = MonitoringEventLog::new(std::num::NonZeroUsize::new(1).unwrap());
        assert_ne!(first.epoch(), second.epoch());
    }

    #[test]
    fn event_summary_cannot_inject_extra_output_lines() {
        let mut log = MonitoringEventLog::new(std::num::NonZeroUsize::new(1).unwrap());
        log.append(None, MonitoringEventKind::Error, 0, "first\nsecond\tthird")
            .unwrap();
        assert_eq!(log.read_after(0).unwrap()[0].summary, "first second third");
    }

    #[test]
    fn resource_catalog_revision_is_recorded_once() {
        let mut log = MonitoringEventLog::new(std::num::NonZeroUsize::new(4).unwrap());
        log.record_resource_revision(7, 100).unwrap();
        log.record_resource_revision(7, 101).unwrap();
        log.record_resource_revision(8, 102).unwrap();
        let events = log.read_after(0).unwrap();
        assert_eq!(events.len(), 2);
        assert!(events
            .iter()
            .all(|event| event.kind == MonitoringEventKind::ResourceChanged));
    }

    #[test]
    fn exhausted_sequence_refuses_new_events_without_discarding_history() {
        let mut log = MonitoringEventLog::new(std::num::NonZeroUsize::new(1).unwrap());
        log.append(None, MonitoringEventKind::Idle, 0, "retained")
            .unwrap();
        log.latest_cursor = u64::MAX;
        assert_eq!(
            log.append(None, MonitoringEventKind::Done, 1, "lost"),
            Err(CursorError::Exhausted)
        );
        assert_eq!(log.events.front().unwrap().summary, "retained");
    }

    #[test]
    fn detected_status_changes_record_meaningful_agent_transitions() {
        use ilium_core::{AgentActivity, AgentClass, PaneStatus};
        use ilium_ipc::ServerEvent;

        let mut log = MonitoringEventLog::new(std::num::NonZeroUsize::new(8).unwrap());
        for activity in [
            AgentActivity::Working,
            AgentActivity::WaitingApproval,
            AgentActivity::Done,
            AgentActivity::Idle,
        ] {
            log.record_server_event(
                &ServerEvent::PaneStatusChanged {
                    pane_id: NodeId(9),
                    status: PaneStatus::from_activity(AgentClass::Codex, activity, None),
                },
                123,
            )
            .unwrap();
        }
        let events = log.read_after(0).unwrap();
        assert_eq!(
            events.iter().map(|event| event.kind).collect::<Vec<_>>(),
            vec![
                MonitoringEventKind::Started,
                MonitoringEventKind::NeedsInput,
                MonitoringEventKind::Done,
                MonitoringEventKind::Idle,
            ]
        );
        assert!(events.iter().all(|event| event.pane_id == Some(NodeId(9))));
    }

    #[test]
    fn generation_is_stable_within_work_and_changes_when_work_resumes() {
        let mut log = MonitoringEventLog::new(std::num::NonZeroUsize::new(8).unwrap());
        for kind in [
            MonitoringEventKind::Started,
            MonitoringEventKind::Started,
            MonitoringEventKind::Done,
            MonitoringEventKind::Started,
            MonitoringEventKind::Done,
        ] {
            log.append(Some(NodeId(1)), kind, 0, "fixture evidence")
                .unwrap();
        }
        let events = log.read_after(0).unwrap();
        assert_eq!(events[0].generation, events[1].generation);
        assert_eq!(events[0].generation, events[2].generation);
        assert_ne!(events[2].generation, events[3].generation);
        assert_eq!(events[3].generation, events[4].generation);
    }

    #[test]
    fn state_age_requires_observation_and_survives_unchanged_tree_refresh() {
        use ilium_core::{AgentActivity, AgentClass, PaneContentKind, PaneStatus, Tree};
        use ilium_ipc::ServerEvent;
        let mut tree = Tree::new();
        let project = tree.add_project("/fixture/age".into()).unwrap();
        let pane = tree
            .add_pane(project, "fixture", PaneContentKind::Terminal)
            .unwrap();
        let mut log = MonitoringEventLog::new(std::num::NonZeroUsize::new(8).unwrap());
        log.replace_tree_context(&tree);
        assert_eq!(log.state_age_seconds(pane, "shell"), None);
        let status = PaneStatus::from_activity(AgentClass::Codex, AgentActivity::Working, None);
        tree.set_pane_status(pane, status.clone()).unwrap();
        let event = ServerEvent::PaneStatusChanged {
            pane_id: pane,
            status,
        };
        log.record_server_event(&event, 0).unwrap();
        let since = log.context[&pane].state_since.unwrap();
        log.record_server_event(&event, 1).unwrap();
        log.replace_tree_context(&tree);
        assert_eq!(log.context[&pane].state_since, Some(since));
        assert!(log.state_age_seconds(pane, "working").is_some());
        assert_eq!(log.state_age_seconds(pane, "done"), None);
        tree.set_pane_status(pane, PaneStatus::PlainShell).unwrap();
        log.replace_tree_context(&tree);
        assert_eq!(log.state_age_seconds(pane, "shell"), None);
    }

    #[test]
    fn live_detection_publication_records_the_detected_status() {
        use ilium_core::{AgentActivity, AgentClass, PaneStatus};
        use ilium_ipc::{PaneDetectionEvidence, ServerEvent};

        let status =
            PaneStatus::from_activity(AgentClass::Codex, AgentActivity::WaitingApproval, None);
        let evidence = PaneDetectionEvidence {
            observed_at_unix_millis: 11,
            stale_after_millis: 1000,
            applied_status: status.clone(),
            identity: None,
            activity: None,
            goal: None,
        };
        let mut log = MonitoringEventLog::new(std::num::NonZeroUsize::new(2).unwrap());
        log.record_server_event(
            &ServerEvent::PaneDetectedStateChanged {
                pane_id: NodeId(6),
                status,
                evidence,
            },
            11,
        )
        .unwrap();
        let events = log.read_after(0).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, MonitoringEventKind::NeedsInput);
        assert_eq!(events[0].pane_id, Some(NodeId(6)));
    }

    #[test]
    fn progress_and_resource_wait_publications_are_visible() {
        use ilium_core::{PaneProgress, ProgressTaskReport, ProgressTaskStatus};
        use ilium_ipc::ServerEvent;

        let report = ProgressTaskReport::new(
            "build".into(),
            ProgressTaskStatus::Done,
            100.0,
            "Build finished".into(),
            String::new(),
            None,
        )
        .unwrap();
        let mut log = MonitoringEventLog::new(std::num::NonZeroUsize::new(4).unwrap());
        log.record_server_event(
            &ServerEvent::PaneProgressChanged {
                pane_id: NodeId(7),
                progress_monitors: vec![PaneProgress::new(3, report, 10).unwrap()],
            },
            10,
        )
        .unwrap();
        log.record_server_event(
            &ServerEvent::ResourceWaitChanged {
                wait_revision: 1,
                pane_id: NodeId(7),
                reservation_id: None,
            },
            11,
        )
        .unwrap();
        let events = log.read_after(0).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, MonitoringEventKind::Progress);
        assert_eq!(events[0].summary, "Build finished");
        assert_eq!(events[1].kind, MonitoringEventKind::ResourceChanged);
    }

    #[test]
    fn clearing_all_progress_monitors_is_visible() {
        let mut log = MonitoringEventLog::new(std::num::NonZeroUsize::new(2).unwrap());
        log.record_server_event(
            &ilium_ipc::ServerEvent::PaneProgressChanged {
                pane_id: NodeId(7),
                progress_monitors: Vec::new(),
            },
            10,
        )
        .unwrap();
        let events = log.read_after(0).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, MonitoringEventKind::Progress);
        assert_eq!(events[0].summary, "Progress monitors cleared");
    }

    #[test]
    fn failed_progress_task_is_an_error_event() {
        use ilium_core::{PaneProgress, ProgressTaskReport, ProgressTaskStatus};
        let report = ProgressTaskReport::new(
            "build".into(),
            ProgressTaskStatus::Error,
            20.0,
            "Build failed".into(),
            String::new(),
            Some("disk full".into()),
        )
        .unwrap();
        let mut log = MonitoringEventLog::new(std::num::NonZeroUsize::new(2).unwrap());
        log.record_server_event(
            &ilium_ipc::ServerEvent::PaneProgressChanged {
                pane_id: NodeId(7),
                progress_monitors: vec![PaneProgress::new(3, report, 10).unwrap()],
            },
            10,
        )
        .unwrap();
        let events = log.read_after(0).unwrap();
        assert_eq!(events[0].kind, MonitoringEventKind::Error);
        assert_eq!(events[0].summary, "disk full");
    }

    #[test]
    fn failed_progress_observer_does_not_claim_task_failure() {
        use ilium_core::{
            PaneProgress, ProgressMonitorHealth, ProgressTaskReport, ProgressTaskStatus,
        };
        let report = ProgressTaskReport::new(
            "build".into(),
            ProgressTaskStatus::Running,
            20.0,
            "Building".into(),
            String::new(),
            None,
        )
        .unwrap();
        let mut progress = PaneProgress::new(3, report, 10).unwrap();
        progress.monitor_health = ProgressMonitorHealth::Failed {
            consecutive_failures: 3,
            last_error: "probe unavailable".into(),
        };
        let mut log = MonitoringEventLog::new(std::num::NonZeroUsize::new(2).unwrap());
        log.record_server_event(
            &ilium_ipc::ServerEvent::PaneProgressChanged {
                pane_id: NodeId(7),
                progress_monitors: vec![progress],
            },
            10,
        )
        .unwrap();
        let events = log.read_after(0).unwrap();
        assert_eq!(events[0].kind, MonitoringEventKind::Error);
        assert_eq!(events[0].summary, "Monitor failed: probe unavailable");
    }
}
