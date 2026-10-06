//! Pure policy for releasing inactive agent processes while retaining their
//! tree presence and last rendered screen.
//!
//! The policy deliberately knows nothing about PTYs, rendering, or settings
//! persistence.  Callers feed it the authoritative agent activity and the
//! elapsed time since the last unfreeze or meaningful activity.  This keeps
//! the safety rule that an unfreeze starts a fresh inactivity window explicit
//! and testable.

use std::time::Duration;

use ilium_core::AgentActivity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoFreezeState {
    Disabled,
    Eligible,
    Ineligible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoFreezeSettings {
    pub enabled: bool,
    pub after: Duration,
    pub freeze_done: bool,
    pub freeze_waiting_for_input: bool,
    pub freeze_waiting_for_approval: bool,
    pub freeze_waiting_for_background: bool,
}

impl Default for AutoFreezeSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            after: Duration::from_secs(6 * 60 * 60),
            freeze_done: true,
            freeze_waiting_for_input: false,
            freeze_waiting_for_approval: false,
            freeze_waiting_for_background: false,
        }
    }
}

impl AutoFreezeSettings {
    pub fn state_for(self, activity: AgentActivity) -> AutoFreezeState {
        if !self.enabled {
            return AutoFreezeState::Disabled;
        }
        let eligible = match activity {
            AgentActivity::Done => self.freeze_done,
            AgentActivity::Idle => self.freeze_waiting_for_input,
            AgentActivity::WaitingApproval => self.freeze_waiting_for_approval,
            AgentActivity::WaitingBackground | AgentActivity::BackgroundTaskStillRunning => {
                self.freeze_waiting_for_background
            }
            AgentActivity::Working => false,
        };
        if eligible {
            AutoFreezeState::Eligible
        } else {
            AutoFreezeState::Ineligible
        }
    }

    pub fn should_freeze(self, activity: AgentActivity, inactive_for: Duration) -> bool {
        self.state_for(activity) == AutoFreezeState::Eligible && inactive_for >= self.after
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreezeClock {
    inactive_for: Duration,
}

impl Default for FreezeClock {
    fn default() -> Self {
        Self {
            inactive_for: Duration::ZERO,
        }
    }
}

impl FreezeClock {
    pub fn inactive_for(self) -> Duration {
        self.inactive_for
    }

    pub fn observe(self, elapsed: Duration, activity: AgentActivity) -> Self {
        if matches!(activity, AgentActivity::Working) {
            Self::default()
        } else {
            Self {
                inactive_for: self.inactive_for.saturating_add(elapsed),
            }
        }
    }

    /// Unfreezing always starts a new inactivity window, even when the
    /// restored agent immediately reports the same eligible state.
    pub fn reset_on_unfreeze(self) -> Self {
        Self::default()
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
        assert!(settings.should_freeze(AgentActivity::Done, Duration::from_secs(6 * 60 * 60)));
        assert!(!settings.should_freeze(AgentActivity::Idle, Duration::from_secs(99 * 60 * 60)));
        assert!(!settings.should_freeze(AgentActivity::Working, Duration::from_secs(99 * 60 * 60)));
    }

    #[test]
    fn waiting_state_options_are_independent() {
        let settings = AutoFreezeSettings {
            enabled: true,
            after: Duration::from_secs(10),
            freeze_done: false,
            freeze_waiting_for_input: true,
            freeze_waiting_for_approval: false,
            freeze_waiting_for_background: true,
        };
        assert!(settings.should_freeze(AgentActivity::Idle, Duration::from_secs(10)));
        assert!(!settings.should_freeze(AgentActivity::WaitingApproval, Duration::from_secs(10)));
        assert!(settings.should_freeze(AgentActivity::WaitingBackground, Duration::from_secs(10)));
    }

    #[test]
    fn working_activity_clears_elapsed_time() {
        let clock = FreezeClock::default()
            .observe(Duration::from_secs(20), AgentActivity::Done)
            .observe(Duration::from_secs(1), AgentActivity::Working);
        assert_eq!(clock.inactive_for(), Duration::ZERO);
    }

    #[test]
    fn unfreeze_resets_even_if_the_agent_stays_done() {
        let clock = FreezeClock::default().observe(Duration::from_secs(20), AgentActivity::Done);
        assert_eq!(clock.reset_on_unfreeze().inactive_for(), Duration::ZERO);
    }
}
