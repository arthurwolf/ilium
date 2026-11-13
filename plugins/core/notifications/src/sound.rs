//! Sound and notification settings model and the event-selection rules of the core plugin
//! `ilium.core.notifications`.
//!
//! Ported from `ilium-sound/src/lib.rs`. The settings types keep their serde shape, so the
//! `[sound]` and `[notifications]` tables read and write unchanged. Playback, discovery of
//! system sounds and process launching stay with the host: the plugin only decides *which*
//! event a transition is. The kernel mapped `PaneStatus` through `ilium_core::project_pane_signals`;
//! the plugin receives the activity through the `ilium.core.agent-detect/activity-changed`
//! topic, so [`Activity`] and [`NowSignal`] are the local, dependency-free equivalents.

use serde::{Deserialize, Serialize};

use crate::synthesis::SoundDesign;

/// Which playback source the user selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundSourceKind {
    #[default]
    SystemBeep,
    SoundFile,
    BundledChirping,
    Generated,
    Muted,
}

impl SoundSourceKind {
    /// Settings cycles every source while preserving the old first toggle.
    pub const ALL: [Self; 5] = [
        Self::SystemBeep,
        Self::SoundFile,
        Self::BundledChirping,
        Self::Generated,
        Self::Muted,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::SystemBeep => "System beep",
            Self::SoundFile => "Sound file",
            Self::BundledChirping => "Bundled chirping",
            Self::Generated => "Custom sound",
            Self::Muted => "Muted",
        }
    }
}

/// User-selectable events that can independently trigger the configured
/// sound. These are semantic transitions, not raw polling states, so a pane
/// remaining idle across many detection ticks never repeats an alert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundEvent {
    AgentFinished,
    ApprovalRequired,
    AgentStarted,
    WaitingBackground,
    TaskSucceeded,
    TaskFailed,
}

impl SoundEvent {
    pub const ALL: [Self; 6] = [
        Self::AgentFinished,
        Self::ApprovalRequired,
        Self::AgentStarted,
        Self::WaitingBackground,
        Self::TaskSucceeded,
        Self::TaskFailed,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::AgentFinished => "Agent finished",
            Self::ApprovalRequired => "Agent needs approval",
            Self::AgentStarted => "Agent started working",
            Self::WaitingBackground => "Agent is waiting for background work",
            Self::TaskSucceeded => "Task succeeded",
            Self::TaskFailed => "Task failed or monitor lost",
        }
    }
}

/// Per-event checkboxes persisted under `[sound.events]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundEventSettings {
    pub agent_finished: bool,
    pub approval_required: bool,
    pub agent_started: bool,
    pub waiting_background: bool,
    pub task_succeeded: bool,
    pub task_failed: bool,
}

impl Default for SoundEventSettings {
    fn default() -> Self {
        Self {
            agent_finished: true,
            approval_required: false,
            agent_started: false,
            waiting_background: false,
            // A monitored task finishing while its agent keeps working is
            // routine progress, not something worth interrupting for.
            task_succeeded: false,
            task_failed: true,
        }
    }
}

impl SoundEventSettings {
    pub const fn is_enabled(self, event: SoundEvent) -> bool {
        match event {
            SoundEvent::AgentFinished => self.agent_finished,
            SoundEvent::ApprovalRequired => self.approval_required,
            SoundEvent::AgentStarted => self.agent_started,
            SoundEvent::WaitingBackground => self.waiting_background,
            SoundEvent::TaskSucceeded => self.task_succeeded,
            SoundEvent::TaskFailed => self.task_failed,
        }
    }

    pub fn toggle(&mut self, event: SoundEvent) {
        match event {
            SoundEvent::AgentFinished => self.agent_finished = !self.agent_finished,
            SoundEvent::ApprovalRequired => {
                self.approval_required = !self.approval_required;
            }
            SoundEvent::AgentStarted => self.agent_started = !self.agent_started,
            SoundEvent::WaitingBackground => {
                self.waiting_background = !self.waiting_background;
            }
            SoundEvent::TaskSucceeded => self.task_succeeded = !self.task_succeeded,
            SoundEvent::TaskFailed => self.task_failed = !self.task_failed,
        }
    }
}

/// User-selectable events that can independently raise a desktop
/// notification. Names match [`SoundEvent`] so Sound and Notifications read as
/// two outputs of one event vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationEvent {
    AgentFinished,
    ApprovalRequired,
    TaskSucceeded,
    TaskFailed,
}

impl NotificationEvent {
    pub const ALL: [Self; 4] = [
        Self::AgentFinished,
        Self::ApprovalRequired,
        Self::TaskSucceeded,
        Self::TaskFailed,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::AgentFinished => "Agent finished",
            Self::ApprovalRequired => "Agent needs approval",
            Self::TaskSucceeded => "Task succeeded",
            Self::TaskFailed => "Task failed or monitor lost",
        }
    }
}

/// The `[notifications]` table. `enabled` is the master switch; every event
/// is additionally gated by its own flag. Flat keys keep hand-written config
/// simple (`task_succeeded = false`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationSettings {
    pub enabled: bool,
    pub agent_finished: bool,
    pub approval_required: bool,
    pub task_succeeded: bool,
    pub task_failed: bool,
    /// Suppress task-outcome notifications and sounds while the agent in the
    /// pane is idle or parked: it is about to receive the result and will
    /// raise its own "agent finished" alert, so the outcome is redundant.
    pub suppress_redundant_task_outcomes: bool,
    /// Task-outcome alerts of the same kind on the same pane closer together
    /// than this are collapsed into the first. Zero disables coalescing.
    pub task_coalesce_seconds: u32,
}

impl NotificationSettings {
    pub const MAX_TASK_COALESCE_SECONDS: u32 = 600;
    pub const TASK_COALESCE_STEP_SECONDS: u32 = 10;

    pub const fn is_enabled(self, event: NotificationEvent) -> bool {
        self.enabled
            && match event {
                NotificationEvent::AgentFinished => self.agent_finished,
                NotificationEvent::ApprovalRequired => self.approval_required,
                NotificationEvent::TaskSucceeded => self.task_succeeded,
                NotificationEvent::TaskFailed => self.task_failed,
            }
    }

    /// The event's own flag, ignoring the master switch (for Settings rows).
    pub const fn event_flag(self, event: NotificationEvent) -> bool {
        match event {
            NotificationEvent::AgentFinished => self.agent_finished,
            NotificationEvent::ApprovalRequired => self.approval_required,
            NotificationEvent::TaskSucceeded => self.task_succeeded,
            NotificationEvent::TaskFailed => self.task_failed,
        }
    }

    pub fn toggle(&mut self, event: NotificationEvent) {
        let flag = match event {
            NotificationEvent::AgentFinished => &mut self.agent_finished,
            NotificationEvent::ApprovalRequired => &mut self.approval_required,
            NotificationEvent::TaskSucceeded => &mut self.task_succeeded,
            NotificationEvent::TaskFailed => &mut self.task_failed,
        };
        *flag = !*flag;
    }

    /// Clamps hand-edited values into their supported range.
    pub fn normalized(mut self) -> Self {
        self.task_coalesce_seconds = self
            .task_coalesce_seconds
            .min(Self::MAX_TASK_COALESCE_SECONDS);
        self
    }
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            agent_finished: true,
            approval_required: true,
            task_succeeded: false,
            task_failed: true,
            suppress_redundant_task_outcomes: true,
            task_coalesce_seconds: 30,
        }
    }
}

/// The complete, transport-safe `[sound]` configuration shared by the client
/// and server. A missing `file` while `source == SoundFile` is a valid
/// temporarily-unconfigured state: the UI can show that no system sound was
/// found and playback simply returns a useful error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundSettings {
    pub source: SoundSourceKind,
    pub file: Option<String>,
    /// Kept when another source is selected so reopening the studio restores
    /// the authored design. Generated playback normalizes it before rendering.
    pub design: SoundDesign,
    pub events: SoundEventSettings,
}

impl Default for SoundSettings {
    fn default() -> Self {
        Self {
            source: SoundSourceKind::SystemBeep,
            file: None,
            design: SoundDesign::default(),
            events: SoundEventSettings::default(),
        }
    }
}

/// The agent activity carried by the `ilium.core.agent-detect/activity-changed` topic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    Working,
    Idle,
    Done,
    WaitingApproval,
    WaitingBackground,
    BackgroundTaskStillRunning,
}

impl Activity {
    /// Parses the topic payload's `activity` string. `with-goal` is a working turn with a goal.
    pub fn from_topic(text: &str) -> Option<Self> {
        match text {
            "working" | "with-goal" => Some(Self::Working),
            "idle" => Some(Self::Idle),
            "done" => Some(Self::Done),
            "awaiting-approval" => Some(Self::WaitingApproval),
            "waiting-background" => Some(Self::WaitingBackground),
            "background-task-still-running" => Some(Self::BackgroundTaskStillRunning),
            _ => None,
        }
    }

    /// The projection `ilium_core::project_pane_signals` applies with no monitors and no
    /// scheduled input.
    pub const fn now_signal(self) -> NowSignal {
        match self {
            Self::WaitingApproval => NowSignal::NeedsApproval,
            Self::Working => NowSignal::Working,
            Self::WaitingBackground => NowSignal::WaitingSubagents,
            Self::BackgroundTaskStillRunning => NowSignal::Settling,
            Self::Done => NowSignal::FinishedUnread,
            Self::Idle => NowSignal::Idle,
        }
    }
}

/// The current-activity slot of a pane (the agent subset of the kernel's `NowSignal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NowSignal {
    NeedsApproval,
    Working,
    WaitingSubagents,
    Settling,
    /// Idle while a live progress monitor watches its task.
    Parked,
    FinishedUnread,
    Idle,
}

/// Maps a real activity change to at most one semantic sound event.
///
/// `None -> Working` is deliberately silent: it is merely the first poll of an already-running
/// agent, not evidence that a new turn just started.
pub fn event_for_transition(previous: Option<Activity>, new: Activity) -> Option<SoundEvent> {
    event_for_signals(previous.map(Activity::now_signal), new.now_signal())
}

/// Selects an alert from the pane signal projection so sound transitions follow the same
/// precedence and attention state as every client's icons.
pub fn event_for_signals(previous: Option<NowSignal>, new: NowSignal) -> Option<SoundEvent> {
    if matches!(
        previous,
        Some(NowSignal::Working | NowSignal::WaitingSubagents | NowSignal::Settling)
    ) && new == NowSignal::FinishedUnread
    {
        return Some(SoundEvent::AgentFinished);
    }

    if previous != Some(NowSignal::NeedsApproval) && new == NowSignal::NeedsApproval {
        return Some(SoundEvent::ApprovalRequired);
    }

    if matches!(
        previous,
        Some(
            NowSignal::Idle
                | NowSignal::NeedsApproval
                | NowSignal::Parked
                | NowSignal::FinishedUnread
        )
    ) && new == NowSignal::Working
    {
        return Some(SoundEvent::AgentStarted);
    }

    // `BackgroundTaskStillRunning` reuses the `WaitingBackground` event: both mean "the agent is
    // blocked on something running in the background", and moving directly between the two must
    // not refire the chime.
    if !matches!(
        previous,
        Some(NowSignal::WaitingSubagents | NowSignal::Settling)
    ) && matches!(new, NowSignal::WaitingSubagents | NowSignal::Settling)
    {
        return Some(SoundEvent::WaitingBackground);
    }

    None
}
