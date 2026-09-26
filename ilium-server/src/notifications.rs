//! Desktop notification on a pane's `Working -> Done`/`Idle` transition
//! (ARCHITECTURE.md M5). Three pieces, deliberately kept apart: [`is_finished_transition`]
//! is the pure decision ("does this status change deserve a notification at
//! all") that a plain `#[test]` can exercise for every relevant status-pair
//! combination with no D-Bus/notification-daemon dependency;
//! [`PendingNotification`] owns the pure presentation contract, including a
//! pane's distinct long-form agent description when one exists; and [`send`]
//! is the thin I/O adapter around `notify-rust` that actually shows one.

use ilium_core::{AgentActivity, PaneStatus};

/// True if going from `previous` to `new` is "an agent just finished a
/// turn and this is the first classification to say so" -- i.e. `previous`
/// was `Agent(_, Working)` or `Agent(_, WaitingBackground)` (both "the
/// agent is busy" states -- a live foreground turn, or waiting on
/// background subagents it dispatched) and `new` is `Agent(_, Idle)` or
/// `Agent(_, Done)`. Deliberately narrow: `None` (a pane's first-ever
/// classification, e.g. right after it was created) never notifies, since
/// there is no prior busy turn that just ended; `Idle -> Working` (a turn
/// starting) never notifies, since nothing finished; and
/// `Working -> WaitingApproval`/`Working -> WaitingBackground` never notify
/// either -- the agent is blocked on the user or on background work, not
/// done, and `ilium_detect::classify_activity` already distinguishes
/// those cases from "finished" precisely so callers like this one don't
/// have to guess.
pub fn is_finished_transition(previous: Option<&PaneStatus>, new: &PaneStatus) -> bool {
    let Some(previous) = previous else {
        return false;
    };
    matches!(
        previous,
        PaneStatus::Agent(
            _,
            AgentActivity::Working
                | AgentActivity::WaitingBackground
                | AgentActivity::BackgroundTaskStillRunning
        ) | PaneStatus::AgentWithGoal(
            _,
            AgentActivity::Working
                | AgentActivity::WaitingBackground
                | AgentActivity::BackgroundTaskStillRunning,
            _,
        )
    ) && matches!(
        // Only `Done` is a finished turn. The server leaves an agent `Idle`
        // after busy work exactly when it parked on a live progress monitor
        // (`detection.rs`), and a parked agent has not finished anything.
        new,
        PaneStatus::Agent(_, AgentActivity::Done)
            | PaneStatus::AgentWithGoal(_, AgentActivity::Done, _)
    )
}

/// A notification-worthy transition, queued during a detection tick and
/// sent once the tree/pane locks that produced it have been released (see
/// `detection::run_due_panes`) -- a slow or unavailable notification daemon
/// must never hold up an attached client's tree access.
pub struct PendingNotification {
    session_name: String,
    pane_name: String,
    agent_description: Option<String>,
    /// A monitored task's outcome, when this notification reports that
    /// rather than a finished agent turn.
    task_outcome: Option<TaskOutcomeNotice>,
}

/// Presentation of a progress monitor's terminal outcome.
struct TaskOutcomeNotice {
    job_id: String,
    kind: TaskOutcomeKind,
}

enum TaskOutcomeKind {
    Done,
    Failed,
    Lost,
}

impl PendingNotification {
    /// Builds the presentation data from a pane's long-form `name` and its
    /// optional short-form alternative. Inferred titles provide both forms:
    /// the short one identifies the pane compactly, while the distinct long
    /// one explains what the agent was working on after the existing text.
    /// A manually named pane has no distinct description, so its notification
    /// remains byte-for-byte identical to the previous presentation.
    pub fn from_pane_titles(
        session_name: String,
        long_pane_name: String,
        short_pane_name: Option<String>,
    ) -> Self {
        let pane_name = short_pane_name.unwrap_or_else(|| long_pane_name.clone());
        let agent_description = (pane_name != long_pane_name).then_some(long_pane_name);

        Self {
            session_name,
            pane_name,
            agent_description,
            task_outcome: None,
        }
    }

    /// A monitored task reached `done`/`error`, or Ilium lost sight of it.
    /// Returns `None` for a still-live monitor: there is no outcome yet.
    pub fn for_task_outcome(
        session_name: String,
        pane_name: String,
        progress: &ilium_core::PaneProgress,
    ) -> Option<Self> {
        let kind = match progress.report.status {
            ilium_core::ProgressTaskStatus::Done => TaskOutcomeKind::Done,
            ilium_core::ProgressTaskStatus::Error => TaskOutcomeKind::Failed,
            _ if progress.monitor_health.is_failed() => TaskOutcomeKind::Lost,
            _ => return None,
        };
        Some(Self {
            session_name,
            pane_name,
            agent_description: None,
            task_outcome: Some(TaskOutcomeNotice {
                job_id: progress.report.job_id.clone(),
                kind,
            }),
        })
    }

    /// Notification summary shown by the desktop shell.
    fn summary(&self) -> String {
        match self.task_outcome.as_ref().map(|outcome| &outcome.kind) {
            None => format!("{} finished", self.pane_name),
            Some(TaskOutcomeKind::Done) => format!("{}: task done", self.pane_name),
            Some(TaskOutcomeKind::Failed) => format!("{}: task failed", self.pane_name),
            Some(TaskOutcomeKind::Lost) => format!("{}: task lost", self.pane_name),
        }
    }

    /// Notification body, with a distinct long-form description appended as
    /// a second paragraph so the original completion text remains first.
    fn body(&self) -> String {
        if let Some(outcome) = &self.task_outcome {
            let what = match outcome.kind {
                TaskOutcomeKind::Done => "completed successfully",
                TaskOutcomeKind::Failed => "reported an error",
                TaskOutcomeKind::Lost => {
                    "can no longer be observed by Ilium; its outcome is unknown"
                }
            };
            return format!(
                "Session \"{}\": task {} in \"{}\" {what}.",
                self.session_name, outcome.job_id, self.pane_name
            );
        }
        let current_text = format!(
            "Session \"{}\": the agent in \"{}\" is done and waiting on you.",
            self.session_name, self.pane_name
        );
        match &self.agent_description {
            Some(description) => format!("{current_text}\n\nAbout: {description}"),
            None => current_text,
        }
    }
}

/// Shows a desktop notification for `pending`. `notify-rust`'s `show()` is
/// synchronous, blocking I/O (a D-Bus round trip via `zbus` on Linux) even
/// though this crate's own call sites are async, so the actual call runs on
/// a `spawn_blocking` thread rather than inline on a tokio worker thread --
/// see `CLAUDE.md`'s async-task rule and `detection.rs`'s identical
/// treatment of the `sysinfo` refresh for the same reason. The returned
/// `NotificationHandle` is dropped inside the closure (not returned out of
/// it): on macOS's default `NSUserNotificationCenter` backend, `show()`
/// only stages the notification and the actual OS delivery call happens in
/// the handle's `Drop` impl, so letting the handle escape `spawn_blocking`
/// would move that same blocking delivery call onto the tokio worker thread
/// that awaits this function -- exactly the foot-gun `spawn_blocking` exists
/// to avoid.
///
/// Never propagates a failure: no notification daemon/D-Bus session (this
/// sandboxed environment, most containers, some window managers) is a
/// normal, expected condition, not a bug, and must never affect pane
/// detection -- logged and continued, exactly like every other per-pane
/// failure in the detection loop (see `detection::run_due_panes`'s
/// `set_pane_status` error handling for the established pattern this
/// mirrors). Deliberately not unit tested: exercising it for real would
/// mean asserting a notification daemon is present, which is exactly the
/// environment-dependent flakiness the pure `is_finished_transition` above
/// exists to keep out of the test suite.
pub async fn send(pending: PendingNotification) {
    let result = tokio::task::spawn_blocking(move || {
        notify_rust::Notification::new()
            .summary(&pending.summary())
            .body(&pending.body())
            .show()
            .map(drop)
    })
    .await;

    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::warn!(
                "desktop notification failed (no notification daemon? continuing): {error}"
            );
        }
        Err(join_error) => {
            tracing::warn!("desktop notification task panicked (continuing): {join_error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::AgentClass;

    fn working() -> PaneStatus {
        PaneStatus::Agent(AgentClass::Claude, AgentActivity::Working)
    }
    fn idle() -> PaneStatus {
        PaneStatus::Agent(AgentClass::Claude, AgentActivity::Idle)
    }
    fn done() -> PaneStatus {
        PaneStatus::Agent(AgentClass::Claude, AgentActivity::Done)
    }
    fn waiting_approval() -> PaneStatus {
        PaneStatus::Agent(AgentClass::Claude, AgentActivity::WaitingApproval)
    }
    fn waiting_background() -> PaneStatus {
        PaneStatus::Agent(AgentClass::Claude, AgentActivity::WaitingBackground)
    }
    fn background_task_still_running() -> PaneStatus {
        PaneStatus::Agent(
            AgentClass::Claude,
            AgentActivity::BackgroundTaskStillRunning,
        )
    }
    fn plain_shell() -> PaneStatus {
        PaneStatus::PlainShell
    }

    #[test]
    fn working_to_idle_is_a_parked_agent_and_does_not_notify() {
        // The detection loop promotes a finished busy->idle turn to Done; a
        // busy->Idle edge only survives when the agent parked on a monitor.
        assert!(!is_finished_transition(Some(&working()), &idle()));
    }

    #[test]
    fn working_to_done_notifies() {
        assert!(is_finished_transition(Some(&working()), &done()));
    }

    #[test]
    fn idle_to_working_does_not_notify() {
        assert!(!is_finished_transition(Some(&idle()), &working()));
    }

    #[test]
    fn done_to_working_does_not_notify() {
        assert!(!is_finished_transition(Some(&done()), &working()));
    }

    #[test]
    fn working_to_waiting_approval_does_not_notify() {
        assert!(!is_finished_transition(
            Some(&working()),
            &waiting_approval()
        ));
    }

    #[test]
    fn waiting_approval_to_idle_does_not_notify() {
        assert!(!is_finished_transition(Some(&waiting_approval()), &idle()));
    }

    #[test]
    fn working_to_waiting_background_does_not_notify() {
        assert!(!is_finished_transition(
            Some(&working()),
            &waiting_background()
        ));
    }

    #[test]
    fn waiting_background_to_idle_does_not_notify() {
        assert!(!is_finished_transition(
            Some(&waiting_background()),
            &idle()
        ));
    }

    #[test]
    fn working_to_background_task_still_running_does_not_notify() {
        assert!(!is_finished_transition(
            Some(&working()),
            &background_task_still_running()
        ));
    }

    #[test]
    fn background_task_still_running_to_done_notifies() {
        assert!(is_finished_transition(
            Some(&background_task_still_running()),
            &done()
        ));
    }

    #[test]
    fn waiting_background_to_done_notifies() {
        assert!(is_finished_transition(Some(&waiting_background()), &done()));
    }

    #[test]
    fn waiting_background_to_waiting_background_does_not_notify() {
        assert!(!is_finished_transition(
            Some(&waiting_background()),
            &waiting_background()
        ));
    }

    #[test]
    fn idle_to_done_does_not_notify() {
        assert!(!is_finished_transition(Some(&idle()), &done()));
    }

    #[test]
    fn done_to_idle_does_not_notify() {
        assert!(!is_finished_transition(Some(&done()), &idle()));
    }

    #[test]
    fn working_to_working_does_not_notify() {
        assert!(!is_finished_transition(Some(&working()), &working()));
    }

    #[test]
    fn idle_to_idle_does_not_notify() {
        assert!(!is_finished_transition(Some(&idle()), &idle()));
    }

    #[test]
    fn working_to_plain_shell_does_not_notify() {
        assert!(!is_finished_transition(Some(&working()), &plain_shell()));
    }

    #[test]
    fn plain_shell_to_working_does_not_notify() {
        assert!(!is_finished_transition(Some(&plain_shell()), &working()));
    }

    #[test]
    fn first_ever_classification_never_notifies_even_if_it_looks_finished() {
        assert!(!is_finished_transition(None, &idle()));
        assert!(!is_finished_transition(None, &done()));
        assert!(!is_finished_transition(None, &working()));
    }

    #[test]
    fn working_to_done_notifies_regardless_of_which_agent_class() {
        let claude_working = PaneStatus::Agent(AgentClass::Claude, AgentActivity::Working);
        let codex_done = PaneStatus::Agent(AgentClass::Codex, AgentActivity::Done);
        assert!(is_finished_transition(Some(&claude_working), &codex_done));
    }

    #[test]
    fn agent_with_goal_working_to_agent_with_goal_done_notifies() {
        let goal_working = PaneStatus::AgentWithGoal(
            AgentClass::Claude,
            AgentActivity::Working,
            ilium_core::GoalState::Active,
        );
        let goal_done = PaneStatus::AgentWithGoal(
            AgentClass::Claude,
            AgentActivity::Done,
            ilium_core::GoalState::Active,
        );
        assert!(is_finished_transition(Some(&goal_working), &goal_done));
    }

    #[test]
    fn agent_with_goal_working_to_plain_agent_done_notifies_when_goal_clears_on_completion() {
        let goal_working = PaneStatus::AgentWithGoal(
            AgentClass::Claude,
            AgentActivity::Working,
            ilium_core::GoalState::Active,
        );
        let plain_done = PaneStatus::Agent(AgentClass::Claude, AgentActivity::Done);
        assert!(is_finished_transition(Some(&goal_working), &plain_done));
    }

    #[test]
    fn agent_with_goal_waiting_background_to_agent_with_goal_done_notifies() {
        let goal_waiting = PaneStatus::AgentWithGoal(
            AgentClass::Claude,
            AgentActivity::WaitingBackground,
            ilium_core::GoalState::Active,
        );
        let goal_done = PaneStatus::AgentWithGoal(
            AgentClass::Claude,
            AgentActivity::Done,
            ilium_core::GoalState::Active,
        );
        assert!(is_finished_transition(Some(&goal_waiting), &goal_done));
    }

    #[test]
    fn agent_with_goal_working_to_agent_with_goal_waiting_approval_does_not_notify() {
        let goal_working = PaneStatus::AgentWithGoal(
            AgentClass::Claude,
            AgentActivity::Working,
            ilium_core::GoalState::Active,
        );
        let goal_waiting_approval = PaneStatus::AgentWithGoal(
            AgentClass::Claude,
            AgentActivity::WaitingApproval,
            ilium_core::GoalState::Active,
        );
        assert!(!is_finished_transition(
            Some(&goal_working),
            &goal_waiting_approval
        ));
    }

    #[test]
    fn notification_appends_distinct_long_agent_description_after_current_text() {
        let pending = PendingNotification::from_pane_titles(
            "default".to_string(),
            "Fix Authentication Bug In Login Flow".to_string(),
            Some("Auth Bug".to_string()),
        );

        assert_eq!(pending.summary(), "Auth Bug finished");
        assert_eq!(
            pending.body(),
            "Session \"default\": the agent in \"Auth Bug\" is done and waiting on you.\n\n\
             About: Fix Authentication Bug In Login Flow"
        );
    }

    #[test]
    fn notification_without_distinct_long_description_preserves_current_text() {
        let pending = PendingNotification::from_pane_titles(
            "release".to_string(),
            "Manually Named Pane".to_string(),
            None,
        );

        assert_eq!(pending.summary(), "Manually Named Pane finished");
        assert_eq!(
            pending.body(),
            "Session \"release\": the agent in \"Manually Named Pane\" is done and waiting on you."
        );
    }

    #[test]
    fn equal_short_and_long_names_do_not_duplicate_the_description() {
        let pending = PendingNotification::from_pane_titles(
            "default".to_string(),
            "Agent Work".to_string(),
            Some("Agent Work".to_string()),
        );

        assert_eq!(
            pending.body(),
            "Session \"default\": the agent in \"Agent Work\" is done and waiting on you."
        );
    }
}
