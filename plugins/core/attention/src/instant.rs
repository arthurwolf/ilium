//! Deterministic layer: items derived from live agent detection, no LLM.

use ilium_plugin_sdk::types::{AgentActivity, NodeId};

use crate::item::{fingerprint_parts, AttentionItem, AttentionKind, AttentionSource, Briefing};

/// What the instant layer knows about one agent pane. The client side assembles it from the
/// tree view of the pane, the goal and the `activity-detail` signal; this module touches no host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LivePane {
    pub pane_id: NodeId,
    pub project_id: Option<NodeId>,
    pub title: String,
    /// The agent's name as the user knows it ("Claude", "Codex").
    pub agent_label: String,
    pub activity: AgentActivity,
    /// `active`, `paused`, `blocked`, `usage-limited` or `reached`.
    pub goal: Option<String>,
    /// The agent waits for background tasks or subagents (`activity-detail` signal).
    pub waiting_background: bool,
}

impl LivePane {
    /// Stable fingerprint of the pane's current evidence (agent state), so a stored or model
    /// verdict is only trusted while the state it was computed from is unchanged.
    pub fn fingerprint(&self) -> u64 {
        let state_key = format!(
            "{}|{:?}|{}|{}",
            self.agent_label,
            self.activity,
            self.goal.as_deref().unwrap_or(""),
            self.waiting_background
        );
        fingerprint_parts(&[state_key.as_bytes()])
    }
}

/// Returns the item the live state alone justifies, if any.
pub fn derive_instant_item(pane: &LivePane, generation: u64, now_ms: u64) -> Option<AttentionItem> {
    let (kind, priority, reason) = if pane.activity == AgentActivity::AwaitingApproval {
        (
            AttentionKind::Question,
            90,
            "Waiting for your answer or approval".to_string(),
        )
    } else if pane.goal.as_deref() == Some("blocked") {
        (AttentionKind::Stuck, 80, "Its goal is blocked".to_string())
    } else if pane.goal.as_deref() == Some("usage-limited") {
        (AttentionKind::Stuck, 70, "It hit a usage limit".to_string())
    } else if pane.waiting_background {
        (
            AttentionKind::WaitingOn,
            40,
            "Waiting on subagents or background tasks".to_string(),
        )
    } else if pane.activity == AgentActivity::Done {
        (
            AttentionKind::Changed,
            30,
            "Finished since you last looked".to_string(),
        )
    } else {
        return None;
    };
    Some(AttentionItem {
        pane_id: pane.pane_id,
        project_id: pane.project_id,
        title: pane.title.clone(),
        kind,
        reason,
        question: None,
        waiting_on: None,
        briefing: fallback_briefing(&pane.agent_label, kind, false),
        priority,
        source: AttentionSource::Instant,
        fingerprint: pane.fingerprint(),
        generation,
        created_ms: now_ms,
    })
}

/// Deterministic briefing for an item no LLM has annotated. It restates only what agent
/// detection knows (the agent's name and why detection flagged it), so it never claims anything
/// about the agent's actual work.
pub fn fallback_briefing(agent_label: &str, kind: AttentionKind, has_question: bool) -> Briefing {
    let (status_summary, why_here, suggested_action) = match kind {
        AttentionKind::Question => (
            format!("{agent_label} is waiting for your answer or approval."),
            "It paused its turn and cannot continue until you respond.",
            if has_question {
                "Read the question and answer it."
            } else {
                "Open the agent and answer or approve."
            },
        ),
        AttentionKind::Stuck => (
            format!("{agent_label} has stopped making progress."),
            "Its goal is blocked or it hit a usage limit.",
            "Open the agent and decide how to unblock it.",
        ),
        AttentionKind::WaitingOn => (
            format!("{agent_label} is waiting on subagents or background tasks."),
            "It cannot finish until those tasks return.",
            "Check whether the tasks are still running.",
        ),
        AttentionKind::Changed => (
            format!("{agent_label} finished a turn since you last looked."),
            "It has new output you have not seen yet.",
            "Review what it produced.",
        ),
    };
    Briefing {
        status_summary: Some(status_summary),
        why_here: Some(why_here.to_string()),
        suggested_action: Some(suggested_action.to_string()),
        from_llm: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(activity: AgentActivity, goal: Option<&str>, waiting_background: bool) -> LivePane {
        LivePane {
            pane_id: NodeId(7),
            project_id: Some(NodeId(1)),
            title: "api-refactor".to_string(),
            agent_label: "Claude".to_string(),
            activity,
            goal: goal.map(str::to_string),
            waiting_background,
        }
    }

    fn kind_of(pane: &LivePane) -> Option<(AttentionKind, u8)> {
        derive_instant_item(pane, 1, 1_000).map(|item| (item.kind, item.priority))
    }

    #[test]
    fn an_approval_is_a_question_and_outranks_a_blocked_goal() {
        let live = pane(AgentActivity::AwaitingApproval, Some("blocked"), false);
        assert_eq!(kind_of(&live), Some((AttentionKind::Question, 90)));
    }

    #[test]
    fn blocked_and_usage_limited_goals_are_stuck() {
        let blocked = pane(AgentActivity::Working, Some("blocked"), false);
        assert_eq!(kind_of(&blocked), Some((AttentionKind::Stuck, 80)));
        let limited = pane(AgentActivity::Idle, Some("usage-limited"), false);
        assert_eq!(kind_of(&limited), Some((AttentionKind::Stuck, 70)));
    }

    #[test]
    fn background_waits_and_unread_completions_are_ranked_below() {
        let waiting = pane(AgentActivity::Working, None, true);
        assert_eq!(kind_of(&waiting), Some((AttentionKind::WaitingOn, 40)));
        let done = pane(AgentActivity::Done, Some("active"), false);
        assert_eq!(kind_of(&done), Some((AttentionKind::Changed, 30)));
    }

    #[test]
    fn a_working_or_idle_agent_needs_nothing() {
        assert_eq!(
            kind_of(&pane(AgentActivity::Working, Some("active"), false)),
            None
        );
        assert_eq!(kind_of(&pane(AgentActivity::Idle, None, false)), None);
    }

    #[test]
    fn the_fingerprint_follows_the_state_and_nothing_else() {
        let first = pane(AgentActivity::AwaitingApproval, None, false);
        let mut renamed = first.clone();
        renamed.title = "other".to_string();
        assert_eq!(first.fingerprint(), renamed.fingerprint());
        let moved = pane(AgentActivity::Done, None, false);
        assert_ne!(first.fingerprint(), moved.fingerprint());
    }

    #[test]
    fn the_fallback_briefing_names_the_agent_and_is_not_from_the_model() {
        let item = derive_instant_item(&pane(AgentActivity::Done, None, false), 1, 5).unwrap();
        assert!(!item.briefing.from_llm);
        assert!(item
            .briefing
            .status_summary
            .as_deref()
            .is_some_and(|text| text.starts_with("Claude")));
    }
}
