//! The pure alert rules of the plugin, ported from `ilium-server/src/notifications.rs`: the texts of
//! the notices, the settings the rules read, the redundancy and mid-turn rules for task outcomes, and
//! the merge window. No host call happens here, so every rule is a plain unit test.

use std::collections::HashMap;

use serde_json::Value;

use crate::sound::{NotificationEvent, NotificationSettings, SoundEvent, SoundEventSettings};

/// The sound id of an event (`contributes.sounds` of the manual page uses `ask` for the prompt).
pub const fn sound_id(event: SoundEvent) -> &'static str {
    match event {
        SoundEvent::AgentFinished => "finished",
        SoundEvent::ApprovalRequired => "ask",
        SoundEvent::AgentStarted => "started",
        SoundEvent::WaitingBackground => "waiting",
        SoundEvent::TaskSucceeded => "task-done",
        SoundEvent::TaskFailed => "task-failed",
    }
}

/// What a monitored task's terminal report means for alerting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskOutcomeKind {
    Done,
    Failed,
    Lost,
}

impl TaskOutcomeKind {
    /// Reads the `status` of a progress topic payload; `None` while the task is live or unknown.
    pub fn from_status(status: &str) -> Option<Self> {
        match status {
            "done" => Some(Self::Done),
            "error" => Some(Self::Failed),
            "lost" => Some(Self::Lost),
            _ => None,
        }
    }

    /// The desktop event that governs this outcome.
    pub const fn notification_event(self) -> NotificationEvent {
        match self {
            Self::Done => NotificationEvent::TaskSucceeded,
            Self::Failed | Self::Lost => NotificationEvent::TaskFailed,
        }
    }

    /// The sound event that governs this outcome.
    pub const fn sound_event(self) -> SoundEvent {
        match self {
            Self::Done => SoundEvent::TaskSucceeded,
            Self::Failed | Self::Lost => SoundEvent::TaskFailed,
        }
    }
}

/// The settings the alert rules read, on top of the pure defaults of [`crate::sound`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AlertSettings {
    pub notifications: NotificationSettings,
    pub sound_events: SoundEventSettings,
    /// `sound.source` is `muted`.
    pub source_muted: bool,
}

impl AlertSettings {
    /// Reads the settings object of the `sound` tab. Keys may be flat (`"sound.events.x"`) or nested
    /// (`{"sound": {"events": {"x": true}}}`); a missing or mistyped value keeps its default.
    pub fn from_json(text: &str) -> Self {
        let root: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        Self::from_root(&root)
    }

    /// Reads the object a kernel read of the `sound` tab returns: the sound keys (`source`,
    /// `events`) at the top and the desktop switches under `notifications`. A failed read or a
    /// mistyped value keeps the defaults.
    pub fn from_kernel_tab(text: &str) -> Self {
        let tab: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        let mut root = serde_json::Map::new();
        if let Some(notifications) = tab.get("notifications") {
            root.insert("notifications".to_owned(), notifications.clone());
        }
        root.insert("sound".to_owned(), tab);
        Self::from_root(&Value::Object(root))
    }

    fn from_root(root: &Value) -> Self {
        let flag = |key: &str, default: bool| {
            lookup(root, key)
                .and_then(Value::as_bool)
                .unwrap_or(default)
        };
        let defaults = NotificationSettings::default();
        let notifications = NotificationSettings {
            enabled: flag("notifications.enabled", defaults.enabled),
            agent_finished: flag("notifications.agent_finished", defaults.agent_finished),
            approval_required: flag(
                "notifications.approval_required",
                defaults.approval_required,
            ),
            task_succeeded: flag("notifications.task_succeeded", defaults.task_succeeded),
            task_failed: flag("notifications.task_failed", defaults.task_failed),
            suppress_redundant_task_outcomes: flag(
                "notifications.suppress_redundant_task_outcomes",
                defaults.suppress_redundant_task_outcomes,
            ),
            task_coalesce_seconds: lookup(root, "notifications.task_coalesce_seconds")
                .and_then(Value::as_i64)
                .map_or(defaults.task_coalesce_seconds, |seconds| {
                    u32::try_from(seconds.clamp(0, i64::from(u32::MAX)))
                        .unwrap_or(defaults.task_coalesce_seconds)
                }),
        }
        .normalized();
        let event_defaults = SoundEventSettings::default();
        let sound_events = SoundEventSettings {
            agent_finished: flag("sound.events.agent_finished", event_defaults.agent_finished),
            approval_required: flag(
                "sound.events.approval_required",
                event_defaults.approval_required,
            ),
            agent_started: flag("sound.events.agent_started", event_defaults.agent_started),
            waiting_background: flag(
                "sound.events.waiting_background",
                event_defaults.waiting_background,
            ),
            task_succeeded: flag("sound.events.task_succeeded", event_defaults.task_succeeded),
            task_failed: flag("sound.events.task_failed", event_defaults.task_failed),
        };
        let source_muted = lookup(root, "sound.source").and_then(Value::as_str) == Some("muted");
        Self {
            notifications,
            sound_events,
            source_muted,
        }
    }
}

/// The value at the dotted `path`, as a flat key first and then as nested objects.
fn lookup<'root>(root: &'root Value, path: &str) -> Option<&'root Value> {
    if let Some(value) = root.get(path) {
        return Some(value);
    }
    path.split('.')
        .try_fold(root, |current, segment| current.get(segment))
}

/// How a pane presents itself to a notice: its title and the objective that may describe it further.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneLabel<'label> {
    pub session: &'label str,
    pub name: &'label str,
    /// The long-form description; shown after the text only when it differs from the name.
    pub objective: Option<&'label str>,
}

/// Title of the "agent finished" notice.
pub fn finished_title(label: &PaneLabel<'_>) -> String {
    format!("{} finished", label.name)
}

/// Body of the "agent finished" notice.
pub fn finished_body(label: &PaneLabel<'_>) -> String {
    with_description(
        format!(
            "Session \"{}\": the agent in \"{}\" is done and waiting on you.",
            label.session, label.name
        ),
        label,
    )
}

/// Title of the "needs approval" notice.
pub fn approval_title(label: &PaneLabel<'_>) -> String {
    format!("{} needs approval", label.name)
}

/// Body of the "needs approval" notice.
pub fn approval_body(label: &PaneLabel<'_>) -> String {
    with_description(
        format!(
            "Session \"{}\": the agent in \"{}\" is waiting for your approval.",
            label.session, label.name
        ),
        label,
    )
}

fn with_description(text: String, label: &PaneLabel<'_>) -> String {
    match label.objective {
        Some(description) if !description.is_empty() && description != label.name => {
            format!("{text}\n\nAbout: {description}")
        }
        _ => text,
    }
}

/// Title of a task-outcome notice; `agent_is_working` adds the qualifier.
pub fn task_title(label: &PaneLabel<'_>, kind: TaskOutcomeKind, agent_is_working: bool) -> String {
    let (what, qualifier) = match kind {
        TaskOutcomeKind::Done => ("background task finished", "agent still working"),
        TaskOutcomeKind::Failed => ("background task failed", "agent may be continuing"),
        TaskOutcomeKind::Lost => ("background task lost", "agent may be continuing"),
    };
    if agent_is_working {
        format!("{}: {what} ({qualifier})", label.name)
    } else {
        format!("{}: {what}", label.name)
    }
}

/// Body of a task-outcome notice.
pub fn task_body(
    label: &PaneLabel<'_>,
    kind: TaskOutcomeKind,
    job_id: &str,
    agent_is_working: bool,
) -> String {
    let what = match kind {
        TaskOutcomeKind::Done => "completed successfully",
        TaskOutcomeKind::Failed => "reported an error",
        TaskOutcomeKind::Lost => "can no longer be observed by Ilium; its outcome is unknown",
    };
    let tail = if agent_is_working {
        " The agent is still working; its own finished alert follows when it is done."
    } else {
        ""
    };
    format!(
        "Session \"{}\": background task {job_id} in \"{}\" {what}.{tail}",
        label.session, label.name
    )
}

/// Reason of an attention mark: at most 80 characters, whatever the pane is called.
pub fn attention_reason(name: &str) -> String {
    const LIMIT: usize = 80;
    let reason = format!("{name} needs approval");
    if reason.chars().count() <= LIMIT {
        return reason;
    }
    let mut shortened: String = reason.chars().take(LIMIT - 1).collect();
    shortened.push('\u{2026}');
    shortened
}

/// A coarse view of a pane's agent, enough for the task-outcome rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    /// The pane has no agent (a plain shell).
    NoAgent,
    /// Idle, finished or parked: Ilium will deliver the result and the agent raises its own alert.
    Resting,
    /// Working, or blocked on an approval.
    MidTurn,
    /// An agent in any other state (a menu, an unclassified turn).
    Other,
}

/// True when a task outcome is redundant: suppression is on and the agent is resting. A pane without
/// an agent is never redundant.
pub fn is_task_outcome_redundant(settings: &NotificationSettings, agent: AgentState) -> bool {
    settings.suppress_redundant_task_outcomes && agent == AgentState::Resting
}

/// True while the agent is mid-turn, so a task outcome is not the end of its work.
pub fn is_agent_mid_turn(agent: AgentState) -> bool {
    agent == AgentState::MidTurn
}

/// Collapses bursts of same-kind task outcomes on one pane. The first outcome in a window is
/// admitted; later ones inside the window are dropped. Successes and failures are tracked
/// separately so a failure is never swallowed by an earlier success.
#[derive(Debug, Default)]
pub struct TaskOutcomeCoalescer {
    last_admitted: HashMap<(u64, bool), u64>,
}

impl TaskOutcomeCoalescer {
    /// Admits or drops one outcome. `now_millis` is any monotonic clock; a window of zero admits all.
    pub fn admit(
        &mut self,
        pane: u64,
        kind: TaskOutcomeKind,
        now_millis: u64,
        window_seconds: u32,
    ) -> bool {
        if window_seconds == 0 {
            return true;
        }
        let window_millis = u64::from(window_seconds) * 1000;
        self.last_admitted
            .retain(|_, admitted| now_millis.saturating_sub(*admitted) < window_millis);
        let key = (pane, kind == TaskOutcomeKind::Done);
        if self.last_admitted.contains_key(&key) {
            return false;
        }
        self.last_admitted.insert(key, now_millis);
        true
    }
}

/// True for a dedup key the host accepts: 1 to 128 characters from `A-Za-z0-9._:/-`.
pub fn is_valid_dedup_key(key: &str) -> bool {
    !key.is_empty()
        && key.chars().count() <= 128
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '/' | '-'))
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    fn label<'label>(name: &'label str, objective: Option<&'label str>) -> PaneLabel<'label> {
        PaneLabel {
            session: "default",
            name,
            objective,
        }
    }

    /// REQ: PORT:ilium-server/src/notifications.rs::notification_appends_distinct_long_agent_description_after_current_text
    #[test]
    fn a_distinct_description_follows_the_current_text() {
        let body = finished_body(&label("Auth Bug", Some("Fix Authentication Bug")));
        assert!(body.ends_with(
            "the agent in \"Auth Bug\" is done and waiting on you.\n\nAbout: Fix Authentication Bug"
        ));
    }

    /// REQ: PORT:ilium-server/src/notifications.rs::equal_short_and_long_names_do_not_duplicate_the_description
    #[test]
    fn an_equal_description_is_not_repeated() {
        let body = finished_body(&label("Agent Work", Some("Agent Work")));
        assert!(!body.contains("About:"));
    }

    /// REQ: PORT:ilium-server/src/notifications.rs::approval_notification_names_the_pane_and_the_prompt
    #[test]
    fn the_approval_notice_names_the_pane() {
        let pane = label("Auth Bug", None);
        assert_eq!(approval_title(&pane), "Auth Bug needs approval");
        assert!(approval_body(&pane).ends_with("is waiting for your approval."));
    }

    /// REQ: PORT:ilium-server/src/notifications.rs::task_outcome_text_leads_with_the_pane_and_names_the_agent_state
    #[test]
    fn task_titles_carry_the_agent_qualifier_only_mid_turn() {
        let pane = label("Auth Bug", None);
        assert_eq!(
            task_title(&pane, TaskOutcomeKind::Done, true),
            "Auth Bug: background task finished (agent still working)"
        );
        assert_eq!(
            task_title(&pane, TaskOutcomeKind::Failed, true),
            "Auth Bug: background task failed (agent may be continuing)"
        );
        assert_eq!(
            task_title(&pane, TaskOutcomeKind::Lost, false),
            "Auth Bug: background task lost"
        );
        assert!(task_body(&pane, TaskOutcomeKind::Done, "build", true)
            .contains("The agent is still working"));
        assert!(!task_body(&pane, TaskOutcomeKind::Done, "build", false).contains("still working"));
    }

    /// REQ: PORT:ilium-server/src/notifications.rs::task_outcome_kind_follows_the_monitor_report
    #[test]
    fn a_status_maps_to_an_outcome_and_a_live_task_to_none() {
        assert_eq!(
            TaskOutcomeKind::from_status("done"),
            Some(TaskOutcomeKind::Done)
        );
        assert_eq!(
            TaskOutcomeKind::from_status("error"),
            Some(TaskOutcomeKind::Failed)
        );
        assert_eq!(
            TaskOutcomeKind::from_status("lost"),
            Some(TaskOutcomeKind::Lost)
        );
        assert_eq!(TaskOutcomeKind::from_status("running"), None);
    }

    /// REQ: PORT:ilium-server/src/notifications.rs::task_outcome_is_redundant_only_for_an_idle_or_finished_agent
    #[test]
    fn only_a_resting_agent_makes_an_outcome_redundant() {
        let on = NotificationSettings::default();
        assert!(is_task_outcome_redundant(&on, AgentState::Resting));
        assert!(!is_task_outcome_redundant(&on, AgentState::MidTurn));
        assert!(!is_task_outcome_redundant(&on, AgentState::NoAgent));
        let off = NotificationSettings {
            suppress_redundant_task_outcomes: false,
            ..on
        };
        assert!(!is_task_outcome_redundant(&off, AgentState::Resting));
    }

    /// REQ: PORT:ilium-server/src/notifications.rs::agent_mid_turn_is_true_only_while_the_agent_is_active
    #[test]
    fn mid_turn_only_while_the_agent_is_active() {
        assert!(is_agent_mid_turn(AgentState::MidTurn));
        assert!(!is_agent_mid_turn(AgentState::Resting));
        assert!(!is_agent_mid_turn(AgentState::NoAgent));
    }

    /// REQ: PORT:ilium-server/src/notifications.rs::coalescer_merges_same_kind_outcomes_inside_the_window
    #[test]
    fn the_coalescer_merges_same_kind_outcomes_inside_the_window() {
        let mut coalescer = TaskOutcomeCoalescer::default();
        assert!(coalescer.admit(1, TaskOutcomeKind::Failed, 0, 30));
        assert!(!coalescer.admit(1, TaskOutcomeKind::Failed, 10_000, 30));
        assert!(coalescer.admit(1, TaskOutcomeKind::Done, 10_000, 30));
        assert!(coalescer.admit(2, TaskOutcomeKind::Failed, 10_000, 30));
        assert!(!coalescer.admit(1, TaskOutcomeKind::Failed, 29_999, 30));
        assert!(coalescer.admit(1, TaskOutcomeKind::Failed, 30_000, 30));
    }

    /// REQ: PORT:ilium-server/src/notifications.rs::coalescer_with_a_zero_window_admits_everything
    #[test]
    fn a_zero_window_admits_everything() {
        let mut coalescer = TaskOutcomeCoalescer::default();
        assert!(coalescer.admit(1, TaskOutcomeKind::Failed, 0, 0));
        assert!(coalescer.admit(1, TaskOutcomeKind::Failed, 0, 0));
    }

    /// REQ: F-048
    #[test]
    fn the_attention_reason_never_exceeds_eighty_characters() {
        let reason = attention_reason(&"x".repeat(300));
        assert_eq!(reason.chars().count(), 80);
        assert_eq!(attention_reason("Auth"), "Auth needs approval");
    }

    /// REQ: F-047
    #[test]
    fn settings_read_flat_and_nested_keys_with_the_defaults_kept() {
        let defaults = AlertSettings::from_json("{}");
        assert!(defaults.notifications.enabled);
        assert!(defaults.sound_events.agent_finished);
        assert!(!defaults.sound_events.approval_required);
        assert!(!defaults.source_muted);
        let flat = AlertSettings::from_json(
            r#"{"sound.events.approval_required": true, "notifications.task_coalesce_seconds": 100000, "sound.source": "muted"}"#,
        );
        assert!(flat.sound_events.approval_required);
        assert_eq!(flat.notifications.task_coalesce_seconds, 600);
        assert!(flat.source_muted);
        let nested = AlertSettings::from_json(
            r#"{"notifications": {"enabled": false, "task_coalesce_seconds": -5}}"#,
        );
        assert!(!nested.notifications.enabled);
        assert_eq!(nested.notifications.task_coalesce_seconds, 0);
        assert_eq!(AlertSettings::from_json("not json"), defaults);
    }

    /// The kernel read of the `sound` tab carries the desktop switches under `notifications`.
    #[test]
    fn the_kernel_sound_tab_carries_sound_and_desktop_switches() {
        let tab = AlertSettings::from_kernel_tab(
            r#"{"source": "muted", "events": {"approval_required": true, "agent_finished": false},
                "notifications": {"enabled": false, "task_failed": false, "task_coalesce_seconds": 45}}"#,
        );
        assert!(tab.source_muted);
        assert!(tab.sound_events.approval_required);
        assert!(!tab.sound_events.agent_finished);
        assert!(!tab.notifications.enabled);
        assert!(!tab.notifications.task_failed);
        assert_eq!(tab.notifications.task_coalesce_seconds, 45);
        assert_eq!(
            AlertSettings::from_kernel_tab("{}"),
            AlertSettings::from_json("{}")
        );
        assert_eq!(
            AlertSettings::from_kernel_tab("not json"),
            AlertSettings::from_json("{}")
        );
    }

    /// REQ: F-047
    #[test]
    fn dedup_keys_follow_the_host_rule() {
        assert!(is_valid_dedup_key("ask:7:4"));
        assert!(!is_valid_dedup_key(""));
        assert!(!is_valid_dedup_key("has space"));
        assert!(!is_valid_dedup_key(&"k".repeat(129)));
    }
}
