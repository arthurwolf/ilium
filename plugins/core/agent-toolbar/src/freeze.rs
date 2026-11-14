//! Auto-freeze: releasing inactive agent processes while the tree keeps their last screen.
//!
//! The policy is pure. The client side feeds it the kernel's detailed activity (the
//! `activity-detail` signal, which keeps the two background states the six plugin activities
//! fold into `working`), the clock and the `[ui]` settings of the user-interface tab; it answers
//! which panes have been inactive long enough. The freeze itself is the kernel command
//! `kernel.freeze-agent`, because only the kernel holds the painted screen it must retain.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

/// The `signal-changed` name the kernel sends the detailed activity under.
pub const ACTIVITY_DETAIL_SIGNAL: &str = "activity-detail";
/// The kernel command that freezes one agent pane; its argument is `{ "node": <id> }`.
pub const FREEZE_AGENT_COMMAND: &str = "kernel.freeze-agent";
/// The kernel settings tab that holds the auto-freeze switches.
pub const SETTINGS_TAB: &str = "user-interface";

/// What an agent pane is doing, as the kernel classifies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    Working,
    WaitingBackground,
    BackgroundTaskStillRunning,
    WaitingApproval,
    Done,
    Idle,
}

impl Activity {
    /// The activity a `activity-detail` signal value names.
    pub fn from_detail(value: &str) -> Option<Self> {
        match value {
            "working" => Some(Self::Working),
            "waiting-background" => Some(Self::WaitingBackground),
            "background-task-still-running" => Some(Self::BackgroundTaskStillRunning),
            "waiting-approval" => Some(Self::WaitingApproval),
            "done" => Some(Self::Done),
            "idle" => Some(Self::Idle),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoFreezeState {
    Disabled,
    Eligible,
    Ineligible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoFreezeSettings {
    pub enabled: bool,
    pub after_seconds: u64,
    pub freeze_done: bool,
    pub freeze_waiting_for_input: bool,
    pub freeze_waiting_for_approval: bool,
    pub freeze_waiting_for_background: bool,
}

impl Default for AutoFreezeSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            after_seconds: 6 * 60 * 60,
            freeze_done: true,
            freeze_waiting_for_input: false,
            freeze_waiting_for_approval: false,
            freeze_waiting_for_background: false,
        }
    }
}

impl AutoFreezeSettings {
    /// Reads the user-interface tab (`[ui]` of `config.toml`); a missing key keeps its default
    /// and text that is not a JSON object reads as all defaults.
    pub fn from_tab(text: &str) -> Self {
        let tab: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        let defaults = Self::default();
        let flag =
            |key: &str, default: bool| tab.get(key).and_then(Value::as_bool).unwrap_or(default);
        Self {
            enabled: flag("auto_freeze_enabled", defaults.enabled),
            after_seconds: tab
                .get("auto_freeze_after_seconds")
                .and_then(Value::as_u64)
                .unwrap_or(defaults.after_seconds),
            freeze_done: flag("auto_freeze_done", defaults.freeze_done),
            freeze_waiting_for_input: flag("auto_freeze_idle", defaults.freeze_waiting_for_input),
            freeze_waiting_for_approval: flag(
                "auto_freeze_waiting_approval",
                defaults.freeze_waiting_for_approval,
            ),
            freeze_waiting_for_background: flag(
                "auto_freeze_waiting_background",
                defaults.freeze_waiting_for_background,
            ),
        }
    }

    pub fn state_for(self, activity: Activity) -> AutoFreezeState {
        if !self.enabled {
            return AutoFreezeState::Disabled;
        }
        let eligible = match activity {
            Activity::Done => self.freeze_done,
            Activity::Idle => self.freeze_waiting_for_input,
            Activity::WaitingApproval => self.freeze_waiting_for_approval,
            Activity::WaitingBackground | Activity::BackgroundTaskStillRunning => {
                self.freeze_waiting_for_background
            }
            Activity::Working => false,
        };
        if eligible {
            AutoFreezeState::Eligible
        } else {
            AutoFreezeState::Ineligible
        }
    }

    pub fn should_freeze(self, activity: Activity, inactive_seconds: u64) -> bool {
        self.state_for(activity) == AutoFreezeState::Eligible
            && inactive_seconds >= self.after_seconds
    }
}

/// Seconds an agent has been inactive. A working agent starts the count over, and so does an
/// unfreeze, even when the restored agent at once reports the same eligible state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FreezeClock {
    inactive_seconds: u64,
}

impl FreezeClock {
    pub fn inactive_seconds(self) -> u64 {
        self.inactive_seconds
    }

    pub fn observe(self, elapsed_seconds: u64, activity: Activity) -> Self {
        if activity == Activity::Working {
            Self::default()
        } else {
            Self {
                inactive_seconds: self.inactive_seconds.saturating_add(elapsed_seconds),
            }
        }
    }

    pub fn reset_on_unfreeze(self) -> Self {
        Self::default()
    }
}

/// The agent panes the plugin follows: their activity, when each became eligible and which ones
/// it already asked the kernel to freeze (asked once; a refused freeze is not retried every tick).
#[derive(Debug, Default)]
pub struct FreezeBook {
    activity: BTreeMap<u64, Activity>,
    eligible_since_millis: BTreeMap<u64, u64>,
    requested: BTreeSet<u64>,
}

impl FreezeBook {
    /// Records one `activity-detail` signal. An empty value means the pane no longer runs an
    /// agent; a working agent restarts the inactivity window.
    pub fn observe(&mut self, pane: u64, value: &str) {
        if value.is_empty() {
            self.forget(pane);
            return;
        }
        let Some(activity) = Activity::from_detail(value) else {
            return;
        };
        if activity == Activity::Working {
            self.eligible_since_millis.remove(&pane);
            self.requested.remove(&pane);
        }
        self.activity.insert(pane, activity);
    }

    pub fn forget(&mut self, pane: u64) {
        self.activity.remove(&pane);
        self.eligible_since_millis.remove(&pane);
        self.requested.remove(&pane);
    }

    /// Whether no pane is followed, so no timer is needed.
    pub fn is_empty(&self) -> bool {
        self.activity.is_empty()
    }

    /// Re-evaluates every pane against `settings` at `now_millis` and returns the panes to
    /// freeze now, once each. A pane that stopped being eligible loses its window.
    pub fn due(&mut self, now_millis: u64, settings: AutoFreezeSettings) -> Vec<u64> {
        if !settings.enabled {
            self.eligible_since_millis.clear();
            self.requested.clear();
            return Vec::new();
        }
        let mut due = Vec::new();
        for (&pane, &activity) in &self.activity {
            if settings.state_for(activity) != AutoFreezeState::Eligible {
                self.eligible_since_millis.remove(&pane);
                self.requested.remove(&pane);
                continue;
            }
            let since = *self.eligible_since_millis.entry(pane).or_insert(now_millis);
            let inactive_seconds = now_millis.saturating_sub(since) / 1_000;
            if settings.should_freeze(activity, inactive_seconds) && self.requested.insert(pane) {
                due.push(pane);
            }
        }
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_only_freeze_finished_agents_after_six_hours() {
        let settings = AutoFreezeSettings {
            enabled: true,
            ..AutoFreezeSettings::default()
        };
        assert!(settings.should_freeze(Activity::Done, 6 * 60 * 60));
        assert!(!settings.should_freeze(Activity::Idle, 99 * 60 * 60));
        assert!(!settings.should_freeze(Activity::Working, 99 * 60 * 60));
    }

    #[test]
    fn waiting_state_options_are_independent() {
        let settings = AutoFreezeSettings {
            enabled: true,
            after_seconds: 10,
            freeze_done: false,
            freeze_waiting_for_input: true,
            freeze_waiting_for_approval: false,
            freeze_waiting_for_background: true,
        };
        assert!(settings.should_freeze(Activity::Idle, 10));
        assert!(!settings.should_freeze(Activity::WaitingApproval, 10));
        assert!(settings.should_freeze(Activity::WaitingBackground, 10));
        assert!(settings.should_freeze(Activity::BackgroundTaskStillRunning, 10));
    }

    #[test]
    fn working_activity_clears_elapsed_time() {
        let clock = FreezeClock::default()
            .observe(20, Activity::Done)
            .observe(1, Activity::Working);
        assert_eq!(clock.inactive_seconds(), 0);
    }

    #[test]
    fn unfreeze_resets_even_if_the_agent_stays_done() {
        let clock = FreezeClock::default().observe(20, Activity::Done);
        assert_eq!(clock.reset_on_unfreeze().inactive_seconds(), 0);
    }

    #[test]
    fn the_tab_reads_every_switch_and_defaults_the_rest() {
        let settings = AutoFreezeSettings::from_tab(
            r#"{"auto_freeze_enabled": true, "auto_freeze_after_seconds": 900,
                "auto_freeze_done": false, "auto_freeze_idle": true,
                "auto_freeze_waiting_approval": true, "auto_freeze_waiting_background": true}"#,
        );
        assert_eq!(
            settings,
            AutoFreezeSettings {
                enabled: true,
                after_seconds: 900,
                freeze_done: false,
                freeze_waiting_for_input: true,
                freeze_waiting_for_approval: true,
                freeze_waiting_for_background: true,
            }
        );
        assert_eq!(
            AutoFreezeSettings::from_tab("{}"),
            AutoFreezeSettings::default()
        );
        assert_eq!(
            AutoFreezeSettings::from_tab("not json"),
            AutoFreezeSettings::default()
        );
    }

    #[test]
    fn the_signal_names_every_kernel_activity() {
        for (name, activity) in [
            ("working", Activity::Working),
            ("waiting-background", Activity::WaitingBackground),
            (
                "background-task-still-running",
                Activity::BackgroundTaskStillRunning,
            ),
            ("waiting-approval", Activity::WaitingApproval),
            ("done", Activity::Done),
            ("idle", Activity::Idle),
        ] {
            assert_eq!(Activity::from_detail(name), Some(activity));
        }
        assert_eq!(Activity::from_detail("none"), None);
    }

    fn enabled(after_seconds: u64) -> AutoFreezeSettings {
        AutoFreezeSettings {
            enabled: true,
            after_seconds,
            ..AutoFreezeSettings::default()
        }
    }

    #[test]
    fn a_done_pane_is_frozen_once_after_the_window() {
        let mut book = FreezeBook::default();
        book.observe(7, "done");
        assert!(book.due(1_000, enabled(60)).is_empty());
        assert!(book.due(60_999, enabled(60)).is_empty());
        assert_eq!(book.due(61_000, enabled(60)), vec![7]);
        assert!(book.due(120_000, enabled(60)).is_empty());
    }

    #[test]
    fn a_working_blip_restarts_the_window() {
        let mut book = FreezeBook::default();
        book.observe(7, "done");
        assert!(book.due(0, enabled(60)).is_empty());
        book.observe(7, "working");
        book.observe(7, "done");
        assert!(book.due(59_000, enabled(60)).is_empty());
        assert!(book.due(118_000, enabled(60)).is_empty());
        assert_eq!(book.due(119_000, enabled(60)), vec![7]);
    }

    #[test]
    fn an_ineligible_state_or_a_disabled_switch_never_freezes() {
        let mut book = FreezeBook::default();
        book.observe(1, "idle");
        book.observe(2, "done");
        assert!(book.due(0, enabled(1)).is_empty());
        assert_eq!(book.due(5_000, enabled(1)), vec![2]);
        let mut off = FreezeBook::default();
        off.observe(2, "done");
        assert!(off.due(0, AutoFreezeSettings::default()).is_empty());
        assert!(off
            .due(99 * 3_600_000, AutoFreezeSettings::default())
            .is_empty());
    }

    #[test]
    fn a_pane_that_leaves_is_forgotten() {
        let mut book = FreezeBook::default();
        book.observe(7, "done");
        assert!(!book.is_empty());
        book.observe(7, "");
        assert!(book.is_empty());
        assert!(book.due(99 * 3_600_000, enabled(1)).is_empty());
    }
}
