//! Batch short recap: the compact per-agent evidence that rides inside the
//! project restructure call, and the validation of the optional `recap` the
//! model returns for each pane.
//!
//! The batch path never fails a plan because of a recap. A missing, oversized
//! or wrong-shape `recap` is dropped and the plan applies without it.

use serde_json::Value;

use super::evidence::{clip_middle, AgentDigest};
use super::model::{word_count, SHORT_WORD_LIMIT};

/// Lower bound of the whole-prompt evidence budget, in characters.
pub const BATCH_BUDGET_FLOOR_CHARACTERS: usize = 12_000;
/// Upper bound of the whole-prompt evidence budget, in characters.
pub const BATCH_BUDGET_CEILING_CHARACTERS: usize = 60_000;
/// Evidence share granted to each agent when sizing the budget.
pub const BATCH_CHARACTERS_PER_AGENT: usize = 1_500;

/// Per-agent tier budgets (characters). Screen text is never used in batch.
pub const BATCH_GOAL_CHARACTERS: usize = 400;
pub const BATCH_SUMMARY_CHARACTERS: usize = 300;
pub const BATCH_PROMPT_CHARACTERS: usize = 400;
pub const BATCH_ANSWER_CHARACTERS: usize = 400;

/// `max(12_000, min(1_500 x agents, 60_000))`.
pub fn batch_evidence_budget(agent_count: usize) -> usize {
    BATCH_BUDGET_FLOOR_CHARACTERS.max(
        agent_count
            .saturating_mul(BATCH_CHARACTERS_PER_AGENT)
            .min(BATCH_BUDGET_CEILING_CHARACTERS),
    )
}

/// Compact goal-aware evidence for one agent: newest goal, newest summary,
/// newest prompt and newest answer, each clipped to its tier budget.
pub fn batch_agent_evidence(digest: &AgentDigest) -> String {
    let mut lines = Vec::new();
    if let Some(goal) = digest.goals.last() {
        lines.push(format!(
            "goal ({}): {}",
            goal.state.name(),
            clip_middle(&goal.objective, BATCH_GOAL_CHARACTERS)
        ));
    }
    if let Some(summary) = digest.summaries.last() {
        lines.push(format!(
            "summary: {}",
            clip_middle(summary, BATCH_SUMMARY_CHARACTERS)
        ));
    }
    if let Some(prompt) = digest.prompts.last() {
        lines.push(format!(
            "last prompt: {}",
            clip_middle(prompt, BATCH_PROMPT_CHARACTERS)
        ));
    }
    if let Some(answer) = digest.answers.last() {
        lines.push(format!(
            "last answer: {}",
            clip_middle(answer, BATCH_ANSWER_CHARACTERS)
        ));
    }
    lines.join("\n")
}

/// Accepts a model-supplied `recap` only when it is a non-empty string of at
/// most `SHORT_WORD_LIMIT` words. Anything else yields `None`.
pub fn sanitize_batch_recap(value: Option<&Value>) -> Option<String> {
    let text = value?.as_str()?.trim();
    if text.is_empty() || word_count(text) > SHORT_WORD_LIMIT {
        return None;
    }
    Some(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recap::evidence::{GoalRecord, RecapGoalState};
    use serde_json::json;

    #[test]
    fn budget_follows_agent_count_between_floor_and_ceiling() {
        assert_eq!(batch_evidence_budget(0), 12_000);
        assert_eq!(batch_evidence_budget(1), 12_000);
        assert_eq!(batch_evidence_budget(8), 12_000);
        assert_eq!(batch_evidence_budget(10), 15_000);
        assert_eq!(batch_evidence_budget(40), 60_000);
        assert_eq!(batch_evidence_budget(500), 60_000);
    }

    #[test]
    fn evidence_uses_newest_item_per_tier_within_budget() {
        let digest = AgentDigest {
            goals: vec![
                GoalRecord {
                    objective: "old".into(),
                    state: RecapGoalState::Done,
                },
                GoalRecord {
                    objective: "x".repeat(2_000),
                    state: RecapGoalState::Active,
                },
            ],
            summaries: vec!["s1".into(), "s2".into()],
            prompts: vec!["p1".into(), "p2".into()],
            answers: vec!["a1".into(), "a2".into()],
            ..AgentDigest::default()
        };
        let text = batch_agent_evidence(&digest);
        assert!(text.contains("goal (active)"));
        assert!(!text.contains("old"));
        assert!(text.contains("summary: s2") && text.contains("last prompt: p2"));
        assert!(text.contains("last answer: a2"));
        assert!(text.chars().count() < 1_700);
    }

    #[test]
    fn recap_validation_drops_missing_oversized_and_wrong_shape() {
        assert_eq!(
            sanitize_batch_recap(Some(&json!("  Fixing tests. "))),
            Some("Fixing tests.".into())
        );
        assert_eq!(sanitize_batch_recap(None), None);
        assert_eq!(sanitize_batch_recap(Some(&json!(""))), None);
        assert_eq!(sanitize_batch_recap(Some(&json!(7))), None);
        assert_eq!(sanitize_batch_recap(Some(&json!({"a": 1}))), None);
        let long = vec!["word"; SHORT_WORD_LIMIT + 1].join(" ");
        assert_eq!(sanitize_batch_recap(Some(&json!(long))), None);
        let exact = vec!["word"; SHORT_WORD_LIMIT].join(" ");
        assert_eq!(sanitize_batch_recap(Some(&json!(exact))), Some(exact));
    }
}
