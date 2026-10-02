//! Presentation-only decoration for pane titles. Completion state remains in
//! the server-owned [`ilium_core::PaneStatus`]; these helpers never mutate the
//! persisted, manually named, or LLM-inferred title itself.

use ilium_core::{AgentActivity, PaneStatus};

pub const DONE_TITLE_MARKER: &str = "[done]";

/// Prepends the completed-turn marker only while an agent is `Done`.
pub fn decorate_agent_title(activity: AgentActivity, title: &str) -> String {
    if activity != AgentActivity::Done || title.starts_with(DONE_TITLE_MARKER) {
        return title.to_string();
    }
    if title.is_empty() {
        return DONE_TITLE_MARKER.to_string();
    }
    format!("{DONE_TITLE_MARKER} {title}")
}

/// Applies agent completion decoration while leaving every non-agent pane
/// title unchanged.
pub fn decorate_pane_title(status: &PaneStatus, title: &str) -> String {
    match status {
        PaneStatus::Agent(agent) => decorate_agent_title(agent.activity(), title),
        PaneStatus::PlainShell
        | PaneStatus::AgentUnavailable(_)
        | PaneStatus::Editor { .. }
        | PaneStatus::Board => title.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use ilium_core::AgentClass;

    use super::*;

    #[test]
    fn done_marker_is_exact_idempotent_and_agent_only() {
        assert_eq!(
            decorate_pane_title(
                &PaneStatus::from_activity(AgentClass::Codex, AgentActivity::Done, None),
                "Fix authentication"
            ),
            "[done] Fix authentication"
        );
        assert_eq!(
            decorate_pane_title(
                &PaneStatus::from_activity(
                    AgentClass::Claude,
                    AgentActivity::Done,
                    Some(ilium_core::GoalState::Active)
                ),
                "[done] Goal"
            ),
            "[done] Goal"
        );
        assert_eq!(
            decorate_pane_title(
                &PaneStatus::from_activity(AgentClass::Codex, AgentActivity::Working, None),
                "Fix authentication"
            ),
            "Fix authentication"
        );
        assert_eq!(
            decorate_pane_title(&PaneStatus::PlainShell, "shell"),
            "shell"
        );
        assert_eq!(decorate_agent_title(AgentActivity::Done, ""), "[done]");
    }
}
