//! Pure recovery model of a pane whose agent is gone (port of `ilium-core/src/agent_recovery.rs`).
//!
//! Historical agent identity is never permission to address a live composer: the kernel keeps the
//! record, this module holds the wording and the rule the plugin applies. An absent process is
//! `Unverified`; only authoritative evidence turns it into `Exited`.

/// How an observed exit ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExitOutcome {
    /// The process returned this status code.
    ExitCode(u32),
    /// A signal ended it; the signal name is kept next to the record.
    Signal,
    /// The disappearance is verified but its status was not observable.
    Unknown,
}

impl ExitOutcome {
    /// The short label of the outcome.
    pub const fn label(self) -> &'static str {
        match self {
            Self::ExitCode(0) => "Exited normally",
            Self::ExitCode(_) => "Exited with an error status",
            Self::Signal => "Terminated by a signal",
            Self::Unknown => "Exited; reason unavailable",
        }
    }
}

/// What is known about the former agent of a pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Availability {
    /// Discovery was inconclusive; nothing says the process died.
    Unverified,
    /// The original interactive shell owns the terminal again.
    ShellForeground,
    /// An authoritative observation says the process exited.
    Exited(ExitOutcome),
}

impl Availability {
    /// The short label shown in a pane header.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Unverified => "Agent presence unverified",
            Self::ShellForeground => "Shell owns the terminal",
            Self::Exited(outcome) => outcome.label(),
        }
    }

    /// Whether agent-directed automation (resume, input) may address the pane: never for a
    /// historical record.
    pub const fn allows_agent_automation(self) -> bool {
        false
    }

    /// Whether the label may claim a crash: never, an exit status alone does not establish one.
    pub fn label_claims_a_crash(self) -> bool {
        self.label().to_ascii_lowercase().contains("crash")
    }
}

/// The two policy values of `[session] recovery_policy` the server accepts, in the order the
/// settings row lists them.
pub const RECOVERY_POLICIES: [&str; 3] =
    ["restore_automatically", "ask_before_restore", "start_fresh"];

/// Parses the policy strictly, like the server: trims and ignores case, and an unknown value is
/// `None` so the caller reports it instead of silently falling back.
pub fn parse_recovery_policy(value: &str) -> Option<&'static str> {
    let lowered = value.trim().to_ascii_lowercase();
    RECOVERY_POLICIES
        .into_iter()
        .find(|policy| *policy == lowered)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Availability; 6] = [
        Availability::Unverified,
        Availability::ShellForeground,
        Availability::Exited(ExitOutcome::ExitCode(0)),
        Availability::Exited(ExitOutcome::ExitCode(101)),
        Availability::Exited(ExitOutcome::Signal),
        Availability::Exited(ExitOutcome::Unknown),
    ];

    #[test]
    fn no_availability_authorizes_agent_automation() {
        assert!(ALL.iter().all(|state| !state.allows_agent_automation()));
    }

    #[test]
    fn exit_labels_distinguish_observed_status_from_unknown_cause() {
        assert_eq!(ExitOutcome::ExitCode(0).label(), "Exited normally");
        assert_eq!(
            ExitOutcome::ExitCode(101).label(),
            "Exited with an error status"
        );
        assert_eq!(ExitOutcome::Unknown.label(), "Exited; reason unavailable");
        assert!(ALL.iter().all(|state| !state.label_claims_a_crash()));
    }

    #[test]
    fn recovery_policy_is_parsed_strictly() {
        assert_eq!(
            parse_recovery_policy(" Ask_Before_Restore "),
            Some("ask_before_restore")
        );
        assert_eq!(parse_recovery_policy("start_fresh"), Some("start_fresh"));
        assert_eq!(parse_recovery_policy("resore_automatically"), None);
        assert_eq!(parse_recovery_policy(""), None);
    }
}
