//! Deterministic layer: items derived from live agent detection, no LLM.

use ilium_core::{AgentState, AgentTurn, GoalState, NodeId};

use super::item::{AttentionItem, AttentionKind, AttentionSource};

/// Longest question excerpt copied from the visible screen.
const MAXIMUM_QUESTION_CHARACTERS: usize = 400;
/// Visible lines considered when excerpting what an agent is asking.
const QUESTION_TAIL_LINES: usize = 6;

/// Everything the instant layer needs to know about one pane. Gathered by the
/// caller from the render cache; this module never touches `App`.
#[derive(Debug, Clone)]
pub struct LivePane {
    pub pane_id: NodeId,
    pub project_id: Option<NodeId>,
    pub title: String,
    pub agent: Option<AgentState>,
    /// Stable fingerprint of the pane's current evidence (state + visible tail).
    pub fingerprint: u64,
    /// Visible screen text, used only to excerpt a pending question.
    pub visible_text: String,
}

/// Returns the item the live state alone justifies, if any.
pub fn derive_instant_item(pane: &LivePane, generation: u64, now_ms: u64) -> Option<AttentionItem> {
    let agent = pane.agent.as_ref()?;
    let (kind, priority, reason, question) = if agent.turn == AgentTurn::WaitingApproval {
        (
            AttentionKind::Question,
            90,
            "Waiting for your answer or approval".to_string(),
            question_excerpt(&pane.visible_text),
        )
    } else if agent.goal == Some(GoalState::Blocked) {
        (
            AttentionKind::Stuck,
            80,
            "Its goal is blocked".to_string(),
            None,
        )
    } else if agent.goal == Some(GoalState::UsageLimited) {
        (
            AttentionKind::Stuck,
            70,
            "It hit a usage limit".to_string(),
            None,
        )
    } else if agent.turn == AgentTurn::WaitingSubagents {
        (
            AttentionKind::WaitingOn,
            40,
            "Waiting on subagents or background tasks".to_string(),
            None,
        )
    } else if agent.completion_unread {
        (
            AttentionKind::Changed,
            30,
            "Finished since you last looked".to_string(),
            None,
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
        question,
        waiting_on: None,
        priority,
        source: AttentionSource::Instant,
        fingerprint: pane.fingerprint,
        generation,
        created_ms: now_ms,
    })
}

/// Last non-empty visible lines, clipped on a character boundary.
fn question_excerpt(visible_text: &str) -> Option<String> {
    let lines: Vec<&str> = visible_text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let start = lines.len().saturating_sub(QUESTION_TAIL_LINES);
    let joined = lines[start..].join(" ");
    if joined.is_empty() {
        return None;
    }
    Some(joined.chars().take(MAXIMUM_QUESTION_CHARACTERS).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::{AgentActivity, AgentClass};

    fn pane(activity: AgentActivity, goal: Option<GoalState>) -> LivePane {
        LivePane {
            pane_id: NodeId(7),
            project_id: Some(NodeId(1)),
            title: "agent".into(),
            agent: Some(AgentState::from_activity(
                AgentClass::Claude,
                activity,
                goal,
            )),
            fingerprint: 42,
            visible_text: "intro\n\nProceed with the migration? (y/n)\n".into(),
        }
    }

    #[test]
    fn approval_wait_is_a_question_with_screen_excerpt() {
        let item = derive_instant_item(&pane(AgentActivity::WaitingApproval, None), 3, 99).unwrap();
        assert_eq!(item.kind, AttentionKind::Question);
        assert_eq!(
            item.question.as_deref(),
            Some("intro Proceed with the migration? (y/n)")
        );
        assert_eq!(
            (item.generation, item.created_ms, item.fingerprint),
            (3, 99, 42)
        );
    }

    #[test]
    fn blocked_and_limited_goals_are_stuck() {
        let blocked =
            derive_instant_item(&pane(AgentActivity::Idle, Some(GoalState::Blocked)), 1, 0)
                .unwrap();
        let limited = derive_instant_item(
            &pane(AgentActivity::Idle, Some(GoalState::UsageLimited)),
            1,
            0,
        )
        .unwrap();
        assert_eq!(
            (blocked.kind, limited.kind),
            (AttentionKind::Stuck, AttentionKind::Stuck)
        );
        assert!(blocked.priority > limited.priority);
    }

    #[test]
    fn subagent_wait_and_unread_completion_map_to_waiting_and_changed() {
        let waiting =
            derive_instant_item(&pane(AgentActivity::WaitingBackground, None), 1, 0).unwrap();
        let done = derive_instant_item(&pane(AgentActivity::Done, None), 1, 0).unwrap();
        assert_eq!(waiting.kind, AttentionKind::WaitingOn);
        assert_eq!(done.kind, AttentionKind::Changed);
    }

    #[test]
    fn working_idle_and_non_agent_panes_produce_nothing() {
        assert!(derive_instant_item(&pane(AgentActivity::Working, None), 1, 0).is_none());
        assert!(derive_instant_item(&pane(AgentActivity::Idle, None), 1, 0).is_none());
        let mut shell = pane(AgentActivity::Idle, None);
        shell.agent = None;
        assert!(derive_instant_item(&shell, 1, 0).is_none());
    }

    #[test]
    fn question_wins_over_a_blocked_goal() {
        let item = derive_instant_item(
            &pane(AgentActivity::WaitingApproval, Some(GoalState::Blocked)),
            1,
            0,
        )
        .unwrap();
        assert_eq!(item.kind, AttentionKind::Question);
    }
}
