//! Historical agent identity is not permission to address a live composer.
//!
//! These are pure observations. Only the server may turn authoritative process
//! evidence into `Exited`; an absent process-table entry is `Unverified`.

use serde::{Deserialize, Serialize};

use crate::{AgentClass, AgentState};

/// The strongest process identity supplied by the current detector. The start
/// time is essential: a numerical PID alone can name a replacement process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentProcessKey {
    pub class: AgentClass,
    pub process_id: u32,
    pub started_at_unix_seconds: u64,
}

/// An actual wait/exit observation, not an interpretation of terminal text.
/// A nonzero exit is not necessarily a crash; signals do not establish intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentExitOutcome {
    ExitCode(u32),
    /// The exact signal name is held in `AgentRecovery::signal_name`.
    Signal,
    /// Disappearance is verified, but its exit status was not observable.
    Unknown,
}

impl AgentExitOutcome {
    pub const fn label(self) -> &'static str {
        match self {
            Self::ExitCode(0) => "Exited normally",
            Self::ExitCode(_) => "Exited with an error status",
            Self::Signal => "Terminated by a signal",
            Self::Unknown => "Exited; reason unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentAvailability {
    /// Discovery was inconclusive. This does not assert that the process died.
    Unverified,
    /// The pane's original interactive shell owns the foreground terminal.
    /// The former agent may still exist as a stopped/background process.
    ShellForeground,
    Exited(AgentExitOutcome),
}

impl AgentAvailability {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Unverified => "Agent presence unverified",
            Self::ShellForeground => "Shell owns the terminal",
            Self::Exited(outcome) => outcome.label(),
        }
    }

    pub const fn explanation(self) -> &'static str {
        match self {
            Self::Unverified => "The current inspection did not establish a live agent composer. Historical identity is retained; no crash or clean exit is inferred.",
            Self::ShellForeground => "The original interactive shell owns the terminal. Historical agent data remains recoverable, but agent-directed automation is not authorized.",
            Self::Exited(AgentExitOutcome::ExitCode(0)) => "An authoritative exit observation reported status zero for the recorded agent process. This is a normal exit, not a detected crash.",
            Self::Exited(AgentExitOutcome::ExitCode(_)) => "An authoritative exit observation reported a nonzero status. It establishes an error exit, not whether the user intended it.",
            Self::Exited(AgentExitOutcome::Signal) => "An authoritative observation reported signal termination. The signal is recorded without guessing whether it was deliberate.",
            Self::Exited(AgentExitOutcome::Unknown) => "The recorded process is authoritatively known to have disappeared, but no exit cause is available.",
        }
    }
}

/// A read-only recovery reference, not a live transcript ownership claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRecovery {
    pub last_known_state: AgentState,
    pub process: AgentProcessKey,
    pub availability: AgentAvailability,
    /// Exact portable-pty signal name when availability is Signal. A signal
    /// exit's placeholder numeric code is never presented as the cause.
    pub signal_name: Option<String>,
    /// Only an ID previously accepted by process/project transcript discovery.
    pub session_id: Option<String>,
    /// Last exact prompt attributed to this process before ownership was lost.
    /// The pane's separate `last_prompt` may subsequently contain shell input.
    pub last_prompt: Option<String>,
    /// An older exact prompt, retained only when a newer submitted prompt's
    /// text could not be reconstructed. Never present this as the latest.
    pub previous_exact_prompt: Option<String>,
    /// A submission occurred, but its exact text is unavailable. This also
    /// prevents a late transcript fallback from filling the unknown latest.
    pub latest_prompt_unavailable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        project_pane_signals, AgentActivity, GoalState, NowSignal, ObjectiveSignal, PaneStatus,
    };

    fn recovery(availability: AgentAvailability) -> AgentRecovery {
        AgentRecovery {
            last_known_state: AgentState::from_activity(
                AgentClass::Codex,
                AgentActivity::Working,
                Some(GoalState::Active),
            ),
            process: AgentProcessKey {
                class: AgentClass::Codex,
                process_id: 42,
                started_at_unix_seconds: 1_790_960_000,
            },
            availability,
            signal_name: None,
            session_id: Some("verified-session".to_string()),
            last_prompt: Some("authored café\r\nlast line  ".to_string()),
            previous_exact_prompt: None,
            latest_prompt_unavailable: false,
        }
    }

    #[test]
    fn historical_working_identity_does_not_authorize_a_live_agent() {
        let recovery = recovery(AgentAvailability::ShellForeground);
        let original = recovery.clone();
        let status = PaneStatus::AgentUnavailable(Box::new(recovery));

        assert!(status.agent_state().is_none());
        assert_eq!(status.known_agent_state(), Some(&original.last_known_state));
        assert_eq!(status.agent_recovery(), Some(&original));
        assert_eq!(original.last_known_state.activity(), AgentActivity::Working);
    }

    #[test]
    fn unavailable_projection_does_not_present_a_historical_goal_or_turn_as_live() {
        for availability in [
            AgentAvailability::Unverified,
            AgentAvailability::ShellForeground,
            AgentAvailability::Exited(AgentExitOutcome::ExitCode(0)),
            AgentAvailability::Exited(AgentExitOutcome::ExitCode(101)),
            AgentAvailability::Exited(AgentExitOutcome::Signal),
            AgentAvailability::Exited(AgentExitOutcome::Unknown),
        ] {
            let status = PaneStatus::AgentUnavailable(Box::new(recovery(availability)));
            let signals = project_pane_signals(&status, None, false, None);
            assert_eq!(signals.objective, ObjectiveSignal::None);
            assert_eq!(signals.now, NowSignal::AgentUnavailable(availability));
        }
    }

    #[test]
    fn exit_labels_distinguish_observed_status_from_unknown_cause() {
        assert_eq!(AgentExitOutcome::ExitCode(0).label(), "Exited normally");
        assert_eq!(
            AgentExitOutcome::ExitCode(101).label(),
            "Exited with an error status"
        );
        assert_eq!(
            AgentExitOutcome::Unknown.label(),
            "Exited; reason unavailable"
        );
        assert!(!AgentAvailability::Unverified.label().contains("crash"));
        assert!(!AgentAvailability::ShellForeground.label().contains("crash"));
    }
}
