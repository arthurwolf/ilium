//! Synthetic wire fixtures: historical agent recovery must survive both delta
//! delivery and reconnect snapshots without becoming a live-agent assertion.

use ilium_core::{
    AgentActivity, AgentAvailability, AgentClass, AgentExitOutcome, AgentProcessKey, AgentRecovery,
    AgentState, NodeKind, PaneContentKind, PaneStatus, Tree,
};
use ilium_ipc::{decode_bounded_frame, decode_frame, encode_frame, ServerEvent};

#[test]
fn unavailable_agent_wire_preserves_identity_cause_and_exact_prompt_uncertainty() {
    let exact = "authored café 日本語\r\n\nlast line  \t";
    let previous = "previous exact Ελληνικά\ntrailing  ";
    for class in [AgentClass::Codex, AgentClass::Claude] {
        for availability in [
            AgentAvailability::Unverified,
            AgentAvailability::ShellForeground,
            AgentAvailability::Exited(AgentExitOutcome::ExitCode(0)),
            AgentAvailability::Exited(AgentExitOutcome::ExitCode(101)),
            AgentAvailability::Exited(AgentExitOutcome::Signal),
            AgentAvailability::Exited(AgentExitOutcome::Unknown),
        ] {
            for (last_prompt, previous_exact_prompt, latest_prompt_unavailable) in [
                (Some(exact.to_owned()), None, false),
                (None, Some(previous.to_owned()), true),
                (None, None, true),
            ] {
                let recovery = AgentRecovery {
                    last_known_state: AgentState::from_activity(
                        class.clone(),
                        AgentActivity::Working,
                        None,
                    ),
                    process: AgentProcessKey {
                        class: class.clone(),
                        process_id: 42,
                        started_at_unix_seconds: 1_790_960_000,
                    },
                    availability,
                    signal_name: (availability
                        == AgentAvailability::Exited(AgentExitOutcome::Signal))
                    .then(|| "SIGABRT".to_owned()),
                    session_id: Some("verified-session".to_owned()),
                    last_prompt,
                    previous_exact_prompt,
                    latest_prompt_unavailable,
                };
                let status = PaneStatus::AgentUnavailable(Box::new(recovery.clone()));
                let mut tree = Tree::new();
                let project = tree
                    .add_project(std::path::PathBuf::from("/synthetic/wire-fixture"))
                    .unwrap();
                let group = tree.add_group(project, "recovery fixture").unwrap();
                let pane_id = tree
                    .add_pane(group, "stopped agent", PaneContentKind::Terminal)
                    .unwrap();
                // Shell input must never replace the historical agent prompt.
                tree.set_last_prompt(pane_id, Some("echo shell input".to_owned()))
                    .unwrap();
                tree.set_pane_status(pane_id, status.clone()).unwrap();

                for event in [
                    ServerEvent::PaneStatusChanged { pane_id, status },
                    ServerEvent::TreeSnapshot(tree),
                ] {
                    let frame = encode_frame(&event).unwrap();
                    let ordinary: ServerEvent = decode_frame(&frame).unwrap();
                    let bounded: ServerEvent = decode_bounded_frame(&frame).unwrap();
                    assert_eq!(ordinary, event, "ordinary decode changed {recovery:?}");
                    assert_eq!(bounded, event, "bounded decode changed {recovery:?}");
                    let decoded_status = match &bounded {
                        ServerEvent::PaneStatusChanged { status, .. } => status,
                        ServerEvent::TreeSnapshot(tree) => {
                            assert_eq!(tree.last_prompt(pane_id), Some("echo shell input"));
                            assert_eq!(tree.agent_recovery(pane_id), Some(&recovery));
                            let NodeKind::Pane { status, .. } = &tree.get(pane_id).unwrap().kind
                            else {
                                panic!("recovery snapshot changed pane kind");
                            };
                            status
                        }
                        _ => unreachable!("only recovery delta and snapshot were encoded"),
                    };
                    assert!(decoded_status.agent_state().is_none());
                    assert_eq!(decoded_status.agent_recovery(), Some(&recovery));
                }
            }
        }
    }
}
