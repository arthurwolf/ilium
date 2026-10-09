//! Desktop notification on a pane's `Working -> Done`/`Idle` transition
//! (ARCHITECTURE.md M5). Three pieces, deliberately kept apart: [`is_finished_transition`]
//! is the pure decision ("does this status change deserve a notification at
//! all") that a plain `#[test]` can exercise for every relevant status-pair
//! combination with no D-Bus/notification-daemon dependency;
//! [`PendingNotification`] owns the pure presentation contract, including a
//! pane's distinct long-form agent description when one exists; and [`send`]
//! is the thin I/O adapter around `notify-rust` that actually shows one.

#[cfg(test)]
use ilium_core::PaneStatus;
use ilium_core::{NowSignal, PaneSignals};
use ilium_sound::{NotificationEvent, NotificationSettings};

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
#[cfg(test)]
pub fn is_finished_transition(previous: Option<&PaneStatus>, new: &PaneStatus) -> bool {
    let previous =
        previous.map(|status| ilium_core::project_pane_signals(status, None, false, None));
    let new = ilium_core::project_pane_signals(new, None, false, None);
    is_finished_signal_transition(previous.as_ref(), &new)
}

/// Recognizes completion from the same projected state clients render.
pub fn is_finished_signal_transition(previous: Option<&PaneSignals>, new: &PaneSignals) -> bool {
    matches!(
        previous.map(|signals| signals.now),
        Some(NowSignal::Working | NowSignal::WaitingSubagents | NowSignal::Settling)
    ) && new.now == NowSignal::FinishedUnread
}

/// A notification-worthy event, queued during a detection tick and
/// sent once the tree/pane locks that produced it have been released (see
/// `detection::run_due_panes`) -- a slow or unavailable notification daemon
/// must never hold up an attached client's tree access.
pub struct PendingNotification {
    session_name: String,
    pane_name: String,
    agent_description: Option<String>,
    kind: PendingKind,
}

enum PendingKind {
    /// An agent finished a turn and is waiting on the user.
    AgentFinished,
    /// An agent is blocked on an approval or confirmation prompt.
    NeedsApproval,
    /// A monitored task's outcome, reported separately from the agent turn.
    Task(TaskOutcomeNotice),
}

/// Presentation of a progress monitor's terminal outcome.
struct TaskOutcomeNotice {
    job_id: String,
    kind: TaskOutcomeKind,
    agent_is_working: bool,
}

/// What a monitored task's terminal report means for alerting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskOutcomeKind {
    Done,
    Failed,
    Lost,
}

impl TaskOutcomeKind {
    /// The notification event that governs this outcome.
    pub const fn notification_event(self) -> NotificationEvent {
        match self {
            Self::Done => NotificationEvent::TaskSucceeded,
            Self::Failed | Self::Lost => NotificationEvent::TaskFailed,
        }
    }

    /// The sound event that governs this outcome.
    pub const fn sound_event(self) -> ilium_sound::SoundEvent {
        match self {
            Self::Done => ilium_sound::SoundEvent::TaskSucceeded,
            Self::Failed | Self::Lost => ilium_sound::SoundEvent::TaskFailed,
        }
    }

    /// Classifies a monitor's latest progress, or `None` while it is live.
    pub fn from_progress(progress: &ilium_core::PaneProgress) -> Option<Self> {
        match progress.report.status {
            ilium_core::ProgressTaskStatus::Done => Some(Self::Done),
            ilium_core::ProgressTaskStatus::Error => Some(Self::Failed),
            _ if progress.monitor_health.is_failed() => Some(Self::Lost),
            _ => None,
        }
    }
}

/// True when a task outcome is redundant because the pane's agent is idle or
/// parked: Ilium delivers the result to it, it resumes, and its own
/// "agent finished" alert follows. Panes without an agent have no such alert,
/// so their task outcomes are never redundant.
pub fn is_task_outcome_redundant(
    settings: &NotificationSettings,
    status: &ilium_core::PaneStatus,
    progress: &ilium_core::PaneProgress,
) -> bool {
    if !settings.suppress_redundant_task_outcomes
        || !matches!(status, ilium_core::PaneStatus::Agent(_))
    {
        return false;
    }
    matches!(
        ilium_core::project_pane_signals(status, Some(progress), false, None).now,
        NowSignal::Idle | NowSignal::FinishedUnread | NowSignal::Parked
    )
}

/// Whether the pane's agent is mid-turn, so a task outcome is not the end of
/// the agent's work.
pub fn is_agent_mid_turn(
    status: &ilium_core::PaneStatus,
    progress: &ilium_core::PaneProgress,
) -> bool {
    matches!(status, ilium_core::PaneStatus::Agent(_))
        && matches!(
            ilium_core::project_pane_signals(status, Some(progress), false, None).now,
            NowSignal::Working
                | NowSignal::WaitingSubagents
                | NowSignal::Settling
                | NowSignal::NeedsApproval
        )
}

/// Collapses bursts of same-kind task outcomes on one pane. The first outcome
/// in a window is admitted; later ones inside the window are dropped (the
/// sidebar still shows each). Successes and failures are tracked separately
/// so a failure is never swallowed by an earlier success.
#[derive(Debug, Default)]
pub struct TaskOutcomeCoalescer {
    last_admitted: std::collections::HashMap<(ilium_core::NodeId, bool), std::time::Instant>,
}

impl TaskOutcomeCoalescer {
    pub fn admit(
        &mut self,
        pane_id: ilium_core::NodeId,
        kind: TaskOutcomeKind,
        now: std::time::Instant,
        window_seconds: u32,
    ) -> bool {
        if window_seconds == 0 {
            return true;
        }
        let window = std::time::Duration::from_secs(u64::from(window_seconds));
        let key = (pane_id, kind == TaskOutcomeKind::Done);
        self.last_admitted
            .retain(|_, admitted| now.saturating_duration_since(*admitted) < window);
        if self.last_admitted.contains_key(&key) {
            return false;
        }
        self.last_admitted.insert(key, now);
        true
    }
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
            kind: PendingKind::AgentFinished,
        }
    }

    /// The same pane titling, reporting an approval prompt instead.
    pub fn approval_from_pane_titles(
        session_name: String,
        long_pane_name: String,
        short_pane_name: Option<String>,
    ) -> Self {
        Self {
            kind: PendingKind::NeedsApproval,
            ..Self::from_pane_titles(session_name, long_pane_name, short_pane_name)
        }
    }

    /// A monitored task reached `done`/`error`, or Ilium lost sight of it.
    /// `agent_is_working` selects the "agent still working" qualifier.
    pub fn for_task_outcome(
        session_name: String,
        pane_name: String,
        kind: TaskOutcomeKind,
        job_id: String,
        agent_is_working: bool,
    ) -> Self {
        Self {
            session_name,
            pane_name,
            agent_description: None,
            kind: PendingKind::Task(TaskOutcomeNotice {
                job_id,
                kind,
                agent_is_working,
            }),
        }
    }

    /// Notification summary shown by the desktop shell. The pane title leads
    /// so the alert names its agent before anything else.
    fn summary(&self) -> String {
        match &self.kind {
            PendingKind::AgentFinished => format!("{} finished", self.pane_name),
            PendingKind::NeedsApproval => format!("{} needs approval", self.pane_name),
            PendingKind::Task(outcome) => {
                let (what, qualifier) = match outcome.kind {
                    TaskOutcomeKind::Done => ("background task finished", "agent still working"),
                    TaskOutcomeKind::Failed => {
                        ("background task failed", "agent may be continuing")
                    }
                    TaskOutcomeKind::Lost => ("background task lost", "agent may be continuing"),
                };
                if outcome.agent_is_working {
                    format!("{}: {what} ({qualifier})", self.pane_name)
                } else {
                    format!("{}: {what}", self.pane_name)
                }
            }
        }
    }

    /// Notification body, with a distinct long-form description appended as
    /// a second paragraph so the original completion text remains first.
    fn body(&self) -> String {
        let current_text = match &self.kind {
            PendingKind::Task(outcome) => {
                let what = match outcome.kind {
                    TaskOutcomeKind::Done => "completed successfully",
                    TaskOutcomeKind::Failed => "reported an error",
                    TaskOutcomeKind::Lost => {
                        "can no longer be observed by Ilium; its outcome is unknown"
                    }
                };
                let tail = if outcome.agent_is_working {
                    " The agent is still working; its own finished alert follows when it is done."
                } else {
                    ""
                };
                return format!(
                    "Session \"{}\": background task {} in \"{}\" {what}.{tail}",
                    self.session_name, outcome.job_id, self.pane_name
                );
            }
            PendingKind::NeedsApproval => format!(
                "Session \"{}\": the agent in \"{}\" is waiting for your approval.",
                self.session_name, self.pane_name
            ),
            PendingKind::AgentFinished => format!(
                "Session \"{}\": the agent in \"{}\" is done and waiting on you.",
                self.session_name, self.pane_name
            ),
        };
        match &self.agent_description {
            Some(description) => format!("{current_text}\n\nAbout: {description}"),
            None => current_text,
        }
    }
}

/// Shows a desktop notification for `pending`. `notify-rust`'s `show()` is
/// synchronous, blocking I/O (a D-Bus round trip via `zbus` on Linux), so
/// the actual call runs on the server's bounded I/O lane rather than inline
/// on a tokio worker thread. The returned
/// `NotificationHandle` is dropped inside the closure (not returned out of
/// it): on macOS's default `NSUserNotificationCenter` backend, `show()`
/// only stages the notification and the actual OS delivery call happens in
/// the handle's `Drop` impl, so `deliver` drops the handle before returning
/// from the I/O-lane callback.
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
pub(crate) fn send(client: &crate::execution::ExecutionClient, pending: PendingNotification) {
    let _ = send_with(client, pending, deliver);
}

fn deliver(summary: &str, body: &str) -> Result<(), &'static str> {
    notify_rust::Notification::new()
        .summary(summary)
        .body(body)
        .show()
        .map(drop)
        .map_err(|error| {
            tracing::warn!(%error, "desktop notification backend rejected delivery");
            "desktop notification backend rejected delivery"
        })
}

fn send_with(
    client: &crate::execution::ExecutionClient,
    pending: PendingNotification,
    deliver: impl FnOnce(&str, &str) -> Result<(), &'static str> + Send + 'static,
) -> bool {
    let input_bytes = match pending
        .retained_text_bytes()
        .checked_mul(6)
        .and_then(|bytes| bytes.checked_add(2048))
    {
        Some(bytes) if bytes <= MAX_NOTIFICATION_JOB_BYTES => bytes,
        _ => {
            tracing::warn!(
                "desktop notification refused because its text exceeds the bounded job size"
            );
            return false;
        }
    };
    let job = notification_job(pending, deliver);
    let reservation = match client.foundation.try_reserve(
        ilium_execution::Lane::Io,
        ilium_execution::JobCost {
            input_bytes,
            result_bytes: 0,
        },
    ) {
        Ok(reservation) => reservation,
        Err(error) => {
            tracing::warn!(?error, "desktop notification was not admitted (continuing)");
            return false;
        }
    };
    let client = client.clone();
    tokio::spawn(async move {
        match client.run_reserved(reservation, job).await {
            Ok(result) => {
                if let Err(error) = result.view() {
                    tracing::warn!(%error, "desktop notification failed (no notification daemon? continuing)");
                }
            }
            Err(error) => tracing::warn!(?error, "desktop notification worker failed (continuing)"),
        }
    });
    true
}

/// An unavailable desktop backend is a delivery result, while the execution
/// bank reports cancellation, panic, and lost completion separately.
fn notification_job(
    pending: PendingNotification,
    deliver: impl FnOnce(&str, &str) -> Result<(), &'static str> + Send + 'static,
) -> impl ilium_execution::Job<Output = Result<(), &'static str>, Error = std::convert::Infallible>
{
    move |_context: ilium_execution::JobContext| Ok(deliver(&pending.summary(), &pending.body()))
}

const MAX_NOTIFICATION_JOB_BYTES: usize = 64 * 1024;

impl PendingNotification {
    fn retained_text_bytes(&self) -> usize {
        let mut bytes = self
            .session_name
            .capacity()
            .saturating_add(self.pane_name.capacity());
        if let Some(description) = &self.agent_description {
            bytes = bytes.saturating_add(description.capacity());
        }
        if let PendingKind::Task(outcome) = &self.kind {
            bytes = bytes.saturating_add(outcome.job_id.capacity());
        }
        bytes.saturating_add(std::mem::size_of::<Self>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::{AgentActivity, AgentClass};
    use std::time::Duration;

    fn working() -> PaneStatus {
        PaneStatus::from_activity(AgentClass::Claude, AgentActivity::Working, None)
    }
    fn idle() -> PaneStatus {
        PaneStatus::from_activity(AgentClass::Claude, AgentActivity::Idle, None)
    }

    #[tokio::test]
    async fn backend_refusal_remains_a_delivery_result() {
        let execution = crate::execution::ServerExecution::start().expect("execution bank");
        let reservation = execution
            .client
            .foundation
            .try_reserve(
                ilium_execution::Lane::Io,
                ilium_execution::JobCost {
                    input_bytes: 4096,
                    result_bytes: 0,
                },
            )
            .expect("notification admission");
        let pending = PendingNotification::from_pane_titles("session".into(), "pane".into(), None);
        let result = execution
            .client
            .run_reserved(
                reservation,
                notification_job(pending, |_, _| Err("backend unavailable")),
            )
            .await
            .expect("the execution job completes despite backend refusal");
        assert_eq!(result.view(), &Err("backend unavailable"));
    }

    #[tokio::test]
    async fn notification_delivery_is_owned_by_the_bounded_execution_bank() {
        let execution = crate::execution::ServerExecution::start().expect("execution bank");
        let monitor = execution.test_monitor();
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let admitted = send_with(
            &execution.client,
            PendingNotification::from_pane_titles(
                "session".into(),
                "A long task".into(),
                Some("Task".into()),
            ),
            move |_, _| {
                let _ = entered_tx.send(std::thread::current().id());
                release_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("release notification delivery");
                Ok(())
            },
        );
        assert!(admitted, "bounded I/O admission accepts the notification");

        let delivery_thread = tokio::time::timeout(Duration::from_secs(2), entered_rx)
            .await
            .expect("notification delivery starts")
            .expect("delivery reports its thread");
        let health = monitor.health();
        assert_eq!(health.quota.jobs, 1);
        assert!(health.quota.worker_threads > 0);
        assert_ne!(delivery_thread, std::thread::current().id());

        release_tx.send(()).expect("release worker");
        tokio::time::timeout(Duration::from_secs(2), async {
            while monitor.health().quota.jobs != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("notification receipt retires after delivery");
    }

    #[tokio::test]
    async fn oversized_notification_is_refused_before_execution_admission() {
        let execution = crate::execution::ServerExecution::start().expect("execution bank");
        let monitor = execution.test_monitor();
        let mut oversized_session = String::with_capacity(MAX_NOTIFICATION_JOB_BYTES);
        oversized_session.push('x');

        let admitted = send_with(
            &execution.client,
            PendingNotification::from_pane_titles(oversized_session, "pane".into(), None),
            |_, _| panic!("oversized notification must not be delivered"),
        );

        assert!(!admitted, "oversized notification is refused");
        assert_eq!(monitor.health().quota.jobs, 0);
    }

    #[tokio::test]
    async fn notification_is_dropped_when_io_admission_is_saturated() {
        let execution = crate::execution::ServerExecution::start().expect("execution bank");
        let monitor = execution.test_monitor();
        let limits = monitor.health().quota.limits;
        let occupied = execution
            .client
            .foundation
            .try_reserve(
                ilium_execution::Lane::Io,
                ilium_execution::JobCost {
                    input_bytes: limits.input_bytes,
                    result_bytes: 0,
                },
            )
            .expect("fill the shared input-byte admission budget");

        let admitted = send_with(
            &execution.client,
            PendingNotification::from_pane_titles("session".into(), "pane".into(), None),
            |_, _| panic!("a refused notification must not reach the I/O lane"),
        );

        assert!(!admitted, "admission pressure drops advisory notifications");
        assert_eq!(monitor.health().quota.jobs, 1);
        drop(occupied);
    }

    fn done() -> PaneStatus {
        PaneStatus::from_activity(AgentClass::Claude, AgentActivity::Done, None)
    }
    fn waiting_approval() -> PaneStatus {
        PaneStatus::from_activity(AgentClass::Claude, AgentActivity::WaitingApproval, None)
    }
    fn waiting_background() -> PaneStatus {
        PaneStatus::from_activity(AgentClass::Claude, AgentActivity::WaitingBackground, None)
    }
    fn background_task_still_running() -> PaneStatus {
        PaneStatus::from_activity(
            AgentClass::Claude,
            AgentActivity::BackgroundTaskStillRunning,
            None,
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
        let claude_working =
            PaneStatus::from_activity(AgentClass::Claude, AgentActivity::Working, None);
        let codex_done = PaneStatus::from_activity(AgentClass::Codex, AgentActivity::Done, None);
        assert!(is_finished_transition(Some(&claude_working), &codex_done));
    }

    #[test]
    fn agent_with_goal_working_to_agent_with_goal_done_notifies() {
        let goal_working = PaneStatus::from_activity(
            AgentClass::Claude,
            AgentActivity::Working,
            Some(ilium_core::GoalState::Active),
        );
        let goal_done = PaneStatus::from_activity(
            AgentClass::Claude,
            AgentActivity::Done,
            Some(ilium_core::GoalState::Active),
        );
        assert!(is_finished_transition(Some(&goal_working), &goal_done));
    }

    #[test]
    fn agent_with_goal_working_to_plain_agent_done_notifies_when_goal_clears_on_completion() {
        let goal_working = PaneStatus::from_activity(
            AgentClass::Claude,
            AgentActivity::Working,
            Some(ilium_core::GoalState::Active),
        );
        let plain_done = PaneStatus::from_activity(AgentClass::Claude, AgentActivity::Done, None);
        assert!(is_finished_transition(Some(&goal_working), &plain_done));
    }

    #[test]
    fn agent_with_goal_waiting_background_to_agent_with_goal_done_notifies() {
        let goal_waiting = PaneStatus::from_activity(
            AgentClass::Claude,
            AgentActivity::WaitingBackground,
            Some(ilium_core::GoalState::Active),
        );
        let goal_done = PaneStatus::from_activity(
            AgentClass::Claude,
            AgentActivity::Done,
            Some(ilium_core::GoalState::Active),
        );
        assert!(is_finished_transition(Some(&goal_waiting), &goal_done));
    }

    #[test]
    fn agent_with_goal_working_to_agent_with_goal_waiting_approval_does_not_notify() {
        let goal_working = PaneStatus::from_activity(
            AgentClass::Claude,
            AgentActivity::Working,
            Some(ilium_core::GoalState::Active),
        );
        let goal_waiting_approval = PaneStatus::from_activity(
            AgentClass::Claude,
            AgentActivity::WaitingApproval,
            Some(ilium_core::GoalState::Active),
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

    fn progress_with(status: ilium_core::ProgressTaskStatus) -> ilium_core::PaneProgress {
        let error = (status == ilium_core::ProgressTaskStatus::Error).then(|| "boom".to_string());
        ilium_core::PaneProgress::new(
            1,
            ilium_core::ProgressTaskReport::new(
                "build".to_string(),
                status,
                50.0,
                "message".to_string(),
                String::new(),
                error,
            )
            .expect("valid report"),
            0,
        )
        .expect("valid progress")
    }

    #[test]
    fn task_outcome_kind_follows_the_monitor_report() {
        use ilium_core::ProgressTaskStatus as Status;
        assert_eq!(
            TaskOutcomeKind::from_progress(&progress_with(Status::Done)),
            Some(TaskOutcomeKind::Done)
        );
        assert_eq!(
            TaskOutcomeKind::from_progress(&progress_with(Status::Error)),
            Some(TaskOutcomeKind::Failed)
        );
        assert_eq!(
            TaskOutcomeKind::from_progress(&progress_with(Status::Running)),
            None
        );
    }

    #[test]
    fn task_outcomes_map_to_their_own_events() {
        assert_eq!(
            TaskOutcomeKind::Done.notification_event(),
            NotificationEvent::TaskSucceeded
        );
        assert_eq!(
            TaskOutcomeKind::Lost.notification_event(),
            NotificationEvent::TaskFailed
        );
        assert_eq!(
            TaskOutcomeKind::Failed.sound_event(),
            ilium_sound::SoundEvent::TaskFailed
        );
    }

    #[test]
    fn task_outcome_is_redundant_only_for_an_idle_or_finished_agent() {
        let settings = NotificationSettings::default();
        let outcome = progress_with(ilium_core::ProgressTaskStatus::Done);
        assert!(is_task_outcome_redundant(&settings, &idle(), &outcome));
        assert!(is_task_outcome_redundant(&settings, &done(), &outcome));
        assert!(!is_task_outcome_redundant(&settings, &working(), &outcome));
        assert!(!is_task_outcome_redundant(
            &settings,
            &plain_shell(),
            &outcome
        ));
        let keep_all = NotificationSettings {
            suppress_redundant_task_outcomes: false,
            ..settings
        };
        assert!(!is_task_outcome_redundant(&keep_all, &idle(), &outcome));
    }

    #[test]
    fn agent_mid_turn_is_true_only_while_the_agent_is_active() {
        let outcome = progress_with(ilium_core::ProgressTaskStatus::Done);
        assert!(is_agent_mid_turn(&working(), &outcome));
        assert!(is_agent_mid_turn(&waiting_approval(), &outcome));
        assert!(!is_agent_mid_turn(&idle(), &outcome));
        assert!(!is_agent_mid_turn(&plain_shell(), &outcome));
    }

    #[test]
    fn coalescer_merges_same_kind_outcomes_inside_the_window() {
        let mut coalescer = TaskOutcomeCoalescer::default();
        let pane = ilium_core::NodeId(7);
        let start = std::time::Instant::now();
        assert!(coalescer.admit(pane, TaskOutcomeKind::Done, start, 30));
        let soon = start + std::time::Duration::from_secs(10);
        assert!(!coalescer.admit(pane, TaskOutcomeKind::Done, soon, 30));
        // A failure is never swallowed by an earlier success, nor another pane's.
        assert!(coalescer.admit(pane, TaskOutcomeKind::Failed, soon, 30));
        assert!(coalescer.admit(ilium_core::NodeId(8), TaskOutcomeKind::Done, soon, 30));
        let later = start + std::time::Duration::from_secs(31);
        assert!(coalescer.admit(pane, TaskOutcomeKind::Done, later, 30));
    }

    #[test]
    fn coalescer_with_a_zero_window_admits_everything() {
        let mut coalescer = TaskOutcomeCoalescer::default();
        let pane = ilium_core::NodeId(7);
        let now = std::time::Instant::now();
        assert!(coalescer.admit(pane, TaskOutcomeKind::Done, now, 0));
        assert!(coalescer.admit(pane, TaskOutcomeKind::Done, now, 0));
    }

    #[test]
    fn task_outcome_text_leads_with_the_pane_and_names_the_agent_state() {
        let working = PendingNotification::for_task_outcome(
            "default".to_string(),
            "Auth Bug".to_string(),
            TaskOutcomeKind::Done,
            "build".to_string(),
            true,
        );
        assert_eq!(
            working.summary(),
            "Auth Bug: background task finished (agent still working)"
        );
        assert!(working.body().contains("The agent is still working"));

        let failed = PendingNotification::for_task_outcome(
            "default".to_string(),
            "Auth Bug".to_string(),
            TaskOutcomeKind::Failed,
            "build".to_string(),
            true,
        );
        assert_eq!(
            failed.summary(),
            "Auth Bug: background task failed (agent may be continuing)"
        );

        let shell = PendingNotification::for_task_outcome(
            "default".to_string(),
            "Shell".to_string(),
            TaskOutcomeKind::Done,
            "build".to_string(),
            false,
        );
        assert_eq!(shell.summary(), "Shell: background task finished");
        assert!(!shell.body().contains("still working"));
    }

    #[test]
    fn approval_notification_names_the_pane_and_the_prompt() {
        let pending = PendingNotification::approval_from_pane_titles(
            "default".to_string(),
            "Auth Bug".to_string(),
            None,
        );
        assert_eq!(pending.summary(), "Auth Bug needs approval");
        assert_eq!(
            pending.body(),
            "Session \"default\": the agent in \"Auth Bug\" is waiting for your approval."
        );
    }
}
