//! Cache keep-alive eligibility and timing, independent of provider I/O.

use serde::{Deserialize, Serialize};

use crate::AgentClass;

/// Persisted user policy. An adapter must separately prove that it can refresh
/// the exact cached provider request before this policy can schedule a ping.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CacheKeepaliveSettings {
    pub enabled: bool,
    pub claude_interval_minutes: u32,
    pub codex_interval_minutes: u32,
    pub on_monitor: bool,
    pub on_command: bool,
    pub on_subagent: bool,
}

impl Default for CacheKeepaliveSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            claude_interval_minutes: 55,
            codex_interval_minutes: 25,
            on_monitor: true,
            on_command: true,
            on_subagent: true,
        }
    }
}

/// Positive observations only: missing or ambiguous wait evidence is false.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WaitObservation {
    pub monitor_running: bool,
    pub command_waiting: bool,
    pub subagent_waiting: bool,
    pub wait_started_millis: u64,
}

/// Times share the caller's monotonic clock and are specific to one verified
/// agent invocation; callers must discard them when the invocation changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScheduleInput {
    pub now_millis: u64,
    pub last_request_millis: u64,
    pub last_attempt_millis: Option<u64>,
    pub attempt_in_flight: bool,
    pub provider_ready: bool,
    pub wait: WaitObservation,
}

impl CacheKeepaliveSettings {
    /// Returns the due monotonic deadline, or None when no ping is due or the
    /// caller lacks evidence that the requested provider can refresh its cache.
    pub fn next_ping(&self, agent: &AgentClass, input: &ScheduleInput) -> Option<u64> {
        if !self.enabled || !input.provider_ready || input.attempt_in_flight {
            return None;
        }
        if !(self.on_monitor && input.wait.monitor_running
            || self.on_command && input.wait.command_waiting
            || self.on_subagent && input.wait.subagent_waiting)
        {
            return None;
        }
        let minutes = match agent {
            AgentClass::Claude => self.claude_interval_minutes,
            AgentClass::Codex => self.codex_interval_minutes,
            _ => return None,
        };
        if minutes == 0 {
            return None;
        }
        // A wait may start long after the last provider request. Starting a
        // fresh interval at the wait boundary could let the cache expire.
        let baseline = input
            .last_request_millis
            .max(input.last_attempt_millis.unwrap_or(0));
        let deadline = baseline.saturating_add(u64::from(minutes) * 60_000);
        (input.now_millis >= deadline).then_some(deadline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn waiting() -> WaitObservation {
        WaitObservation {
            monitor_running: true,
            command_waiting: false,
            subagent_waiting: false,
            wait_started_millis: 1_000,
        }
    }

    #[test]
    fn defaults_are_disabled_and_use_separate_provider_intervals() {
        let settings = CacheKeepaliveSettings::default();
        assert!(!settings.enabled);
        assert_eq!(settings.claude_interval_minutes, 55);
        assert_eq!(settings.codex_interval_minutes, 25);
    }

    #[test]
    fn disabled_or_unverified_provider_never_schedules_a_ping() {
        let mut settings = CacheKeepaliveSettings::default();
        let input = ScheduleInput {
            now_millis: 4_000_000,
            last_request_millis: 1_000,
            last_attempt_millis: None,
            attempt_in_flight: false,
            provider_ready: true,
            wait: waiting(),
        };
        assert_eq!(settings.next_ping(&crate::AgentClass::Claude, &input), None);
        settings.enabled = true;
        assert_eq!(
            settings.next_ping(
                &crate::AgentClass::Claude,
                &ScheduleInput {
                    provider_ready: false,
                    ..input
                }
            ),
            None
        );
        assert_eq!(
            settings.next_ping(&crate::AgentClass::Antigravity, &input),
            None
        );
    }

    #[test]
    fn selected_wait_conditions_are_disjunctions_and_unknown_waits_do_not_qualify() {
        let settings = CacheKeepaliveSettings {
            enabled: true,
            on_monitor: false,
            on_command: true,
            on_subagent: false,
            ..Default::default()
        };
        let mut input = ScheduleInput {
            now_millis: 3_000_000,
            last_request_millis: 0,
            last_attempt_millis: None,
            attempt_in_flight: false,
            provider_ready: true,
            wait: waiting(),
        };
        assert_eq!(settings.next_ping(&crate::AgentClass::Codex, &input), None);
        input.wait.command_waiting = true;
        assert_eq!(
            settings.next_ping(&crate::AgentClass::Codex, &input),
            Some(1_500_000)
        );
        input.wait.command_waiting = false;
        assert_eq!(settings.next_ping(&crate::AgentClass::Codex, &input), None);
    }

    #[test]
    fn latest_real_request_or_attempt_moves_the_deadline_and_inflight_blocks_duplicates() {
        let settings = CacheKeepaliveSettings {
            enabled: true,
            ..Default::default()
        };
        let input = ScheduleInput {
            now_millis: 4_000_000,
            last_request_millis: 500_000,
            last_attempt_millis: Some(900_000),
            attempt_in_flight: false,
            provider_ready: true,
            wait: waiting(),
        };
        assert_eq!(
            settings.next_ping(&crate::AgentClass::Codex, &input),
            Some(2_400_000)
        );
        assert_eq!(
            settings.next_ping(
                &crate::AgentClass::Codex,
                &ScheduleInput {
                    attempt_in_flight: true,
                    ..input
                }
            ),
            None
        );
    }

    #[test]
    fn a_late_wait_does_not_postpone_an_already_due_refresh() {
        let settings = CacheKeepaliveSettings {
            enabled: true,
            ..Default::default()
        };
        let input = ScheduleInput {
            now_millis: 2_000_000,
            last_request_millis: 1_000,
            last_attempt_millis: None,
            attempt_in_flight: false,
            provider_ready: true,
            wait: WaitObservation {
                wait_started_millis: 1_900_000,
                ..waiting()
            },
        };
        assert_eq!(
            settings.next_ping(&crate::AgentClass::Codex, &input),
            Some(1_501_000)
        );
    }

    #[test]
    fn zero_interval_is_rejected_and_deadlines_saturate_without_wrapping() {
        let settings = CacheKeepaliveSettings {
            enabled: true,
            codex_interval_minutes: 0,
            ..Default::default()
        };
        let input = ScheduleInput {
            now_millis: u64::MAX,
            last_request_millis: u64::MAX - 1,
            last_attempt_millis: None,
            attempt_in_flight: false,
            provider_ready: true,
            wait: waiting(),
        };
        assert_eq!(settings.next_ping(&crate::AgentClass::Codex, &input), None);
        let settings = CacheKeepaliveSettings {
            codex_interval_minutes: 25,
            ..settings
        };
        assert_eq!(
            settings.next_ping(&crate::AgentClass::Codex, &input),
            Some(u64::MAX)
        );
    }
}
