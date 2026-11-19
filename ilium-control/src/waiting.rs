//! Pure selection semantics for blocking monitoring waits.

use ilium_core::{NodeId, Tree};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitMode {
    Any,
    All,
}

#[derive(Debug, Serialize)]
pub struct PaneWaitState {
    pub pane: NodeId,
    pub state: String,
    pub closed: bool,
    pub matched: bool,
}

#[derive(Debug, Serialize)]
pub struct WaitResult {
    pub ready: bool,
    pub panes: Vec<PaneWaitState>,
}

/// A missing pane has completed its wait; a disconnected server is handled by the adapter.
pub fn evaluate(
    tree: &Tree,
    panes: &[NodeId],
    states: &str,
    mode: WaitMode,
) -> Result<WaitResult, &'static str> {
    const VALID: &[&str] = &[
        "working",
        "waiting_subagents",
        "settling",
        "idle",
        "done",
        "waiting_approval",
        "shell",
        "editor",
        "board",
        "agent_unavailable",
        "closed",
    ];
    let wanted: Vec<_> = states
        .split(',')
        .map(|state| state.trim().to_ascii_lowercase())
        .collect();
    if panes.is_empty() {
        return Err("wait requires at least one pane");
    }
    if wanted.iter().any(|state| !VALID.contains(&state.as_str())) {
        return Err("unknown or empty wait state");
    }
    let panes: Vec<_> = panes
        .iter()
        .map(|pane| {
            let status = tree.get(*pane).and_then(|node| match &node.kind {
                ilium_core::NodeKind::Pane { status, .. } => Some(status),
                _ => None,
            });
            let closed = status.is_none();
            let state = status
                .map(crate::pane_state_name)
                .unwrap_or("closed")
                .to_owned();
            let matched = closed || wanted.contains(&state);
            PaneWaitState {
                pane: *pane,
                state,
                closed,
                matched,
            }
        })
        .collect();
    let ready = match mode {
        WaitMode::Any => panes.iter().any(|pane| pane.matched),
        WaitMode::All => panes.iter().all(|pane| pane.matched),
    };
    Ok(WaitResult { ready, panes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::{AgentActivity, AgentClass, PaneContentKind, PaneStatus};

    fn sample() -> (Tree, NodeId, NodeId) {
        let mut tree = Tree::new();
        let project = tree.add_project("/fixture/wait".into()).unwrap();
        let done = tree
            .add_pane(project, "done", PaneContentKind::Terminal)
            .unwrap();
        let working = tree
            .add_pane(project, "working", PaneContentKind::Terminal)
            .unwrap();
        tree.set_pane_status(
            done,
            PaneStatus::from_activity(AgentClass::Codex, AgentActivity::Done, None),
        )
        .unwrap();
        tree.set_pane_status(
            working,
            PaneStatus::from_activity(AgentClass::Codex, AgentActivity::Working, None),
        )
        .unwrap();
        (tree, done, working)
    }

    #[test]
    fn any_and_all_evaluate_the_entire_selection() {
        let (mut tree, done, working) = sample();
        let panes = [done, working];
        assert!(
            evaluate(&tree, &panes, "idle,done", WaitMode::Any)
                .unwrap()
                .ready
        );
        let result = evaluate(&tree, &panes, "idle,done", WaitMode::All).unwrap();
        assert!(!result.ready);
        assert_eq!(result.panes.len(), 2);
        assert!(result.panes[0].matched);
        assert!(!result.panes[1].matched);
        tree.set_pane_status(
            working,
            PaneStatus::from_activity(AgentClass::Codex, AgentActivity::Done, None),
        )
        .unwrap();
        assert!(
            evaluate(&tree, &panes, "done", WaitMode::All)
                .unwrap()
                .ready
        );
    }

    #[test]
    fn closed_panes_complete_their_wait_and_invalid_selections_are_rejected() {
        let (tree, _, working) = sample();
        let missing = NodeId(u64::MAX);
        let result = evaluate(&tree, &[missing], "done", WaitMode::All).unwrap();
        assert!(result.ready && result.panes[0].closed);
        assert!(
            !evaluate(&tree, &[missing, working], "done", WaitMode::All)
                .unwrap()
                .ready
        );
        assert!(evaluate(&tree, &[], "done", WaitMode::Any).is_err());
        assert!(evaluate(&tree, &[working], "", WaitMode::Any).is_err());
        assert!(evaluate(&tree, &[working], "typo", WaitMode::Any).is_err());
    }
}
