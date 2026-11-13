//! Deterministic monitoring signals, independent of clocks and provider I/O.
use ilium_core::AgentActivity;
use ilium_ipc::MonitoringEventKind;

pub struct SignalInput<'a> {
    pub activity: AgentActivity,
    pub elapsed_seconds: u64,
    pub quiet_seconds: u64,
    pub latest_message: Option<&'a str>,
    pub repeated_error: Option<(&'a str, usize)>,
    pub holding_resource: bool,
    pub monitored: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Signal {
    pub kind: MonitoringEventKind,
    pub summary: String,
}

pub fn evaluate(input: &SignalInput<'_>) -> Vec<Signal> {
    let mut signals = Vec::new();
    let mut add = |kind, text: &str| {
        signals.push(Signal {
            kind,
            summary: text
                .chars()
                .map(|ch| if ch.is_control() { ' ' } else { ch })
                .take(120)
                .collect(),
        })
    };
    let idle = matches!(input.activity, AgentActivity::Idle | AgentActivity::Done);
    if idle {
        if let Some(message) = input
            .latest_message
            .filter(|message| message.trim_end().ends_with('?'))
        {
            add(MonitoringEventKind::NeedsInput, message);
        }
        if input.holding_resource && input.elapsed_seconds >= 60 {
            add(
                MonitoringEventKind::Stuck,
                "Agent idle for at least 1 min while holding a resource",
            );
        }
    }
    if let Some((error, _count)) = input.repeated_error.filter(|(_, count)| *count >= 3) {
        add(
            MonitoringEventKind::Stuck,
            &format!("Repeated error reports: {error}"),
        );
    }
    if input.activity == AgentActivity::Working {
        if input.elapsed_seconds >= 300 && input.quiet_seconds >= 300 {
            add(
                MonitoringEventKind::Stuck,
                "Agent working with no output for at least 5 min",
            );
        }
        if input.elapsed_seconds >= 180 && !input.monitored {
            add(
                MonitoringEventKind::Stuck,
                "Agent working for at least 3 min without a progress monitor",
            );
        }
    }
    signals
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> SignalInput<'static> {
        SignalInput {
            activity: AgentActivity::Working,
            elapsed_seconds: 0,
            quiet_seconds: 0,
            latest_message: None,
            repeated_error: None,
            holding_resource: false,
            monitored: false,
        }
    }
    #[test]
    fn question_and_repeated_error_reports_are_actionable_without_a_model() {
        let mut observed = input();
        observed.activity = AgentActivity::Idle;
        observed.latest_message = Some("Which fixture should I use?");
        assert_eq!(evaluate(&observed)[0].kind, MonitoringEventKind::NeedsInput);
        observed.latest_message = Some("The fixture passed.");
        assert!(evaluate(&observed).is_empty());
        observed.repeated_error = Some(("fixture connection refused", 3));
        let signals = evaluate(&observed);
        assert!(signals
            .iter()
            .any(|signal| signal.kind == MonitoringEventKind::Stuck
                && signal.summary.contains("fixture connection refused")));
        observed.repeated_error = Some(("different failure", 1));
        assert!(evaluate(&observed).is_empty());
    }
    #[test]
    fn time_thresholds_distinguish_stuck_unmonitored_and_idle_resource_use() {
        let mut observed = input();
        observed.elapsed_seconds = 179;
        assert!(evaluate(&observed).is_empty());
        observed.elapsed_seconds = 180;
        assert!(evaluate(&observed)
            .iter()
            .any(|signal| signal.summary.contains("progress monitor")));
        observed.monitored = true;
        assert!(evaluate(&observed).is_empty());
        observed.elapsed_seconds = 300;
        observed.quiet_seconds = 299;
        assert!(evaluate(&observed).is_empty());
        observed.quiet_seconds = 300;
        assert!(evaluate(&observed)
            .iter()
            .any(|signal| signal.summary.contains("5 min")));
        observed.activity = AgentActivity::Idle;
        observed.holding_resource = true;
        observed.elapsed_seconds = 59;
        assert!(evaluate(&observed).is_empty());
        observed.elapsed_seconds = 60;
        assert!(evaluate(&observed)
            .iter()
            .any(|signal| signal.summary.contains("resource")));
    }
}
