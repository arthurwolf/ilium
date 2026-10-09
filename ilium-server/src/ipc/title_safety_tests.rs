use super::*;
use ilium_core::animation_recommendation::{
    AnimationRecommendation, PlanAnimationEntry, RecommendedRestructurePlan, ResourcePolicy,
};

#[tokio::test]
async fn all_inference_apply_lanes_preserve_an_unasked_agent_bundle() {
    // Synthetic identity on a fixture-owned real PTY. No real agent is driven.
    let (state, pane_id, _directory) = state_with_one_terminal_pane("unasked-title-lanes").await;
    let session_id = "22222222-2222-4222-8222-222222222222";
    let class = ilium_core::AgentClass::Codex;
    let project_id = {
        let mut tree = state.tree.write().await;
        tree.set_pane_status(
            pane_id,
            PaneStatus::Agent(ilium_core::AgentState::from_activity(
                class.clone(),
                ilium_core::AgentActivity::Idle,
                None,
            )),
        )
        .unwrap();
        tree.project_ancestor(pane_id).unwrap()
    };
    {
        let mut panes = state.panes.write().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
            panic!("fixture terminal");
        };
        runtime.session_id = Some(session_id.into());
        runtime.session_agent_class = Some(class.clone());
        runtime.detected_agent_class = Some(class.clone());
        runtime.detected_agent_process_id = Some(42);
        runtime.agent_process_key = Some(AgentProcessKey {
            class,
            process_id: 42,
            started_at_unix_seconds: 1,
        });
        runtime.agent_generation = 1;
    }
    let directory = state.home_dir.join(".codex/sessions/2026/10/03");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("rollout-2026-10-03T00-00-00-{session_id}.jsonl"));
    let meta = serde_json::json!({"type":"session_meta","payload":{"id":session_id,"cwd":state.session_cwd}});
    let injected = serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":"Ilium progress monitor 1 reports completion"}});
    std::fs::write(path, format!("{meta}\n{injected}\n")).unwrap();
    let original = state.tree.read().await.get(pane_id).unwrap().clone();
    let (direct_tx, mut direct_rx) = DirectEventSender::channel(16);
    for lane in 0..3 {
        let (observations, revisions, animation_generation) = {
            let tree = state.tree.read().await;
            let panes = state.panes.read().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                panic!("fixture terminal");
            };
            (
                vec![
                    TitleRuntimeSnapshot::capture(tree.get(pane_id).unwrap(), runtime, false)
                        .observation,
                ],
                tree.project_activity_revisions(project_id).unwrap(),
                tree.project_animation_generation(project_id).unwrap(),
            )
        };
        let plan = RestructurePlan {
            children: vec![ilium_core::RestructureNode::Group {
                title: format!("Useful grouping {lane}"),
                short_title: None,
                icon: None,
                children: vec![ilium_core::RestructureNode::Pane {
                    id: pane_id,
                    title: "Sibling authentication".into(),
                    short_title: Some("Wrong".into()),
                    icon: Some("lock".into()),
                }],
            }],
        };
        match lane {
            0 => handle_apply_restructure_plan(&state, plan, &observations, &direct_tx).await,
            1 => {
                handle_apply_project_restructure_plan(
                    &state,
                    project_id,
                    plan,
                    &revisions,
                    &observations,
                    &direct_tx,
                )
                .await
            }
            _ => {
                handle_apply_recommended_project_restructure_plan(
                    &state,
                    project_id,
                    RecommendedRestructurePlan {
                        structure: plan,
                        expected_animation_generation: animation_generation,
                        project: AnimationRecommendation {
                            version: 1,
                            kind: "shoreline".into(),
                            resources: ResourcePolicy::Catalog,
                            parameters: vec![],
                        },
                        entries: vec![vec![0], vec![0, 0]]
                            .into_iter()
                            .map(|path| PlanAnimationEntry {
                                path,
                                recommendation: AnimationRecommendation {
                                    version: 1,
                                    kind: "shoreline".into(),
                                    resources: ResourcePolicy::Catalog,
                                    parameters: vec![],
                                },
                            })
                            .collect(),
                    },
                    &revisions,
                    &observations,
                    &direct_tx,
                )
                .await
            }
        }
        if lane > 0 {
            let reply = direct_rx.try_recv();
            assert!(
                matches!(&reply, Ok(ServerEvent::ProjectRestructureApplied { .. })),
                "lane {lane} must acknowledge successful structure: {reply:?}"
            );
        }
        let tree = state.tree.read().await;
        let node = tree.get(pane_id).unwrap();
        assert_eq!(node.name, original.name, "lane {lane}");
        assert_eq!(node.short_name, original.short_name, "lane {lane}");
        assert_eq!(node.inferred_icon, original.inferred_icon, "lane {lane}");
        assert_eq!(node.is_name_fixed, original.is_name_fixed, "lane {lane}");
        assert_eq!(
            node.presentation_revision, original.presentation_revision,
            "lane {lane}"
        );
        let (
            NodeKind::Pane { title_source, .. },
            NodeKind::Pane {
                title_source: original_source,
                ..
            },
        ) = (&node.kind, &original.kind)
        else {
            panic!("pane bundle");
        };
        assert_eq!(title_source, original_source, "lane {lane}");
        assert_eq!(
            tree.get(node.parent.unwrap()).unwrap().name,
            format!("Useful grouping {lane}")
        );
    }
    teardown_state_panes(&state);
}

#[tokio::test]
async fn an_unknown_submission_revokes_the_prior_receipt_and_title_observation() {
    let (state, pane_id, _directory) =
        state_with_one_terminal_pane("unknown-title-submission").await;
    {
        let mut tree = state.tree.write().await;
        let mut panes = state.panes.write().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
            panic!("fixture terminal");
        };
        runtime.detected_agent_class = Some(ilium_core::AgentClass::Codex);
        runtime.agent_process_key = Some(AgentProcessKey {
            class: ilium_core::AgentClass::Codex,
            process_id: 42,
            started_at_unix_seconds: 1,
        });
        assert!(invalidate_submitted_title_observation(
            &mut tree, runtime, pane_id
        ));
        assert!(record_authored_title_receipt(
            &mut tree,
            runtime,
            pane_id,
            "Implement authentication",
            PromptSubmissionSource::Keyboard
        ));
        assert!(runtime.authored_title_receipt.is_some());
        let old_revision = tree.get(pane_id).unwrap().presentation_revision;
        assert!(invalidate_submitted_title_observation(
            &mut tree, runtime, pane_id
        ));
        assert!(runtime.authored_title_receipt.is_none());
        assert!(!tree
            .accept_session_pane_title(
                pane_id,
                old_revision,
                "Old inference",
                None,
                None,
                PaneTitleSource::Automatic
            )
            .unwrap());
        assert!(!record_authored_title_receipt(
            &mut tree,
            runtime,
            pane_id,
            "Ilium progress monitor 1 reports completion",
            PromptSubmissionSource::Keyboard
        ));
        assert!(runtime.authored_title_receipt.is_none());
    }
    teardown_state_panes(&state);
}

#[tokio::test]
async fn empty_history_repair_requires_proof_and_preserves_delivered_tasks_and_manual_names() {
    let (state, pane_id, _directory) = state_with_one_terminal_pane("empty-title-proof").await;
    let session_id = "33333333-3333-4333-8333-333333333333";
    {
        let mut tree = state.tree.write().await;
        tree.set_automatic_pane_title(
            pane_id,
            "Retained task",
            Some("Retained".into()),
            Some("book".into()),
        )
        .unwrap();
        let mut panes = state.panes.write().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
            panic!("fixture terminal");
        };
        runtime.session_id = Some(session_id.into());
        runtime.session_agent_class = Some(ilium_core::AgentClass::Codex);
        runtime.detected_agent_class = Some(ilium_core::AgentClass::Codex);
        runtime.detected_agent_process_id = Some(42);
        runtime.agent_process_key = Some(AgentProcessKey {
            class: ilium_core::AgentClass::Codex,
            process_id: 42,
            started_at_unix_seconds: 1,
        });
    }
    reconcile_empty_agent_titles(&state, &[pane_id]).await;
    assert_eq!(
        state.tree.read().await.get(pane_id).unwrap().name,
        "Retained task",
        "missing history never erases a title"
    );
    let directory = state.home_dir.join(".codex/sessions/2026/10/03");
    std::fs::create_dir_all(&directory).unwrap();
    let meta = serde_json::json!({"type":"session_meta","payload":{"id":session_id,"cwd":state.session_cwd}});
    std::fs::write(
        directory.join(format!("rollout-2026-10-03T00-00-00-{session_id}.jsonl")),
        format!("{meta}\n"),
    )
    .unwrap();
    reconcile_empty_agent_titles(&state, &[pane_id]).await;
    assert_eq!(
        state.tree.read().await.get(pane_id).unwrap().name,
        pane::FRESH_AGENT_TITLE
    );
    {
        let mut tree = state.tree.write().await;
        let mut panes = state.panes.write().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
            panic!("fixture terminal");
        };
        assert!(invalidate_submitted_title_observation(
            &mut tree, runtime, pane_id
        ));
        assert!(record_authored_title_receipt(
            &mut tree,
            runtime,
            pane_id,
            "Implement authentication",
            PromptSubmissionSource::InitialAgentPrompt
        ));
        tree.set_automatic_pane_title(pane_id, "Authentication", None, None)
            .unwrap();
    }
    reconcile_empty_agent_titles(&state, &[pane_id]).await;
    assert_eq!(
        state.tree.read().await.get(pane_id).unwrap().name,
        "Authentication",
        "acknowledged task outranks a delayed transcript flush"
    );
    state
        .tree
        .write()
        .await
        .rename_node(
            pane_id,
            "My workspace",
            Some("Mine".into()),
            Some("pin".into()),
        )
        .unwrap();
    let fixed = state.tree.read().await.get(pane_id).unwrap().clone();
    {
        let mut panes = state.panes.write().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
            panic!("fixture terminal");
        };
        runtime.authored_title_receipt = None;
    }
    reconcile_empty_agent_titles(&state, &[pane_id]).await;
    assert_eq!(state.tree.read().await.get(pane_id).unwrap(), &fixed);
    teardown_state_panes(&state);
}
