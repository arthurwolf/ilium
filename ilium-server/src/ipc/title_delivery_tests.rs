use super::*;

const DELIVERY_SESSION: &str = "44444444-4444-4444-8444-444444444444";

/// Synthetic Codex ownership on the fixture's isolated cat PTY. These tests
/// exercise server admission/receipt policy, not real Codex process detection.
async fn title_delivery_identity(state: &Arc<ServerState>, pane_id: NodeId) {
    let mut tree = state.tree.write().await;
    tree.set_pane_status(
        pane_id,
        PaneStatus::Agent(ilium_core::AgentState::from_activity(
            ilium_core::AgentClass::Codex,
            ilium_core::AgentActivity::Idle,
            None,
        )),
    )
    .unwrap();
    let mut panes = state.panes.write().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
        panic!("fixture-owned terminal");
    };
    runtime.detected_agent_class = Some(ilium_core::AgentClass::Codex);
    runtime.detected_agent_process_id = Some(42);
    runtime.agent_process_key = Some(AgentProcessKey {
        class: ilium_core::AgentClass::Codex,
        process_id: 42,
        started_at_unix_seconds: 1,
    });
    runtime.agent_generation = 7;
    runtime.session_id = Some(DELIVERY_SESSION.into());
    runtime.session_agent_class = Some(ilium_core::AgentClass::Codex);
}

async fn title_delivery_observation(
    state: &Arc<ServerState>,
    pane_id: NodeId,
) -> PaneTitleObservation {
    let tree = state.tree.read().await;
    let panes = state.panes.read().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
        panic!("fixture-owned terminal");
    };
    TitleRuntimeSnapshot::capture(tree.get(pane_id).unwrap(), runtime, false).observation
}

fn title_delivery_history(state: &ServerState, cwd: &std::path::Path) {
    let directory = state.home_dir.join(".codex/sessions/2026/10/03");
    std::fs::create_dir_all(&directory).unwrap();
    let metadata =
        serde_json::json!({"type":"session_meta","payload":{"id":DELIVERY_SESSION,"cwd":cwd}});
    let request = serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":"Implement authentication"}});
    std::fs::write(
        directory.join(format!(
            "rollout-2026-10-03T00-00-00-{DELIVERY_SESSION}.jsonl"
        )),
        format!("{metadata}\n{request}\n"),
    )
    .unwrap();
}

async fn title_delivery_apply(
    state: &Arc<ServerState>,
    observation: &PaneTitleObservation,
    source: PaneTitleSource,
    title: &str,
) {
    handle_session_pane_title(
        state,
        SessionPaneTitleUpdate {
            pane_id: observation.pane_id,
            expected_session_id: observation.session_id.as_deref().unwrap(),
            expected_title_generation: observation.title_generation,
            expected_presentation_revision: observation.presentation_revision,
            expected_process_id: observation.process_id,
            title: title.into(),
            short_title: Some("Short task".into()),
            inferred_icon: Some("lock".into()),
            title_source: source,
        },
    )
    .await;
}

#[tokio::test]
async fn launch_intent_before_detection_cannot_use_the_plain_shell_title_bypass() {
    let (state, pane_id, _directory) =
        state_with_one_terminal_pane("title-launch-before-detection").await;
    {
        let mut panes = state.panes.write().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
            panic!("fixture-owned terminal");
        };
        // Only the intended launch is synthetic. No real Codex is started.
        runtime.origin = TerminalOrigin::Command("codex".into());
        assert!(runtime.detected_agent_class.is_none());
        assert!(runtime.session_id.is_none());
    }
    let before = state.tree.read().await.get(pane_id).unwrap().clone();
    handle_automatic_pane_title(
        &state,
        pane_id,
        "Sibling task".into(),
        Some("Wrong".into()),
        Some("lock".into()),
    )
    .await;
    assert_eq!(state.tree.read().await.get(pane_id).unwrap(), &before);
    let observation = title_delivery_observation(&state, pane_id).await;
    let evidence = collect_observed_title_evidence(&state, &[observation]).await;
    {
        let tree = state.tree.read().await;
        let panes = state.panes.read().await;
        assert!(title_grants_under_lock(&tree, &panes, &evidence, &HashMap::new()).is_empty());
    }
    teardown_state_panes(&state);
}

#[tokio::test]
async fn acknowledged_body_without_enter_cannot_establish_an_authored_title_receipt() {
    let (state, pane_id, _directory) = state_with_one_terminal_pane("title-body-receipt").await;
    title_delivery_identity(&state, pane_id).await;
    let (input, gate) = {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            panic!("fixture-owned terminal");
        };
        (
            runtime.session.input_handle(),
            Arc::clone(&runtime.input_gate),
        )
    };
    let guard = gate.lock().await;
    // Real PTY acknowledgements, synthetic agent identity: deliberately use the
    // raw writer to isolate the post-delivery receipt hook from detection.
    input
        .write(b"Implement authentication")
        .unwrap()
        .wait()
        .await
        .unwrap();
    let before_enter = title_delivery_observation(&state, pane_id).await;
    title_delivery_apply(
        &state,
        &before_enter,
        PaneTitleSource::Automatic,
        "Premature",
    )
    .await;
    assert_eq!(state.tree.read().await.get(pane_id).unwrap().name, "cat");
    {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            panic!("fixture-owned terminal");
        };
        assert!(runtime.authored_title_receipt.is_none());
    }
    input.write(b"\r").unwrap().wait().await.unwrap();
    {
        let mut tree = state.tree.write().await;
        let mut panes = state.panes.write().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
            panic!("fixture-owned terminal");
        };
        // Invoke the production hook only after both owned writes acknowledged.
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
    }
    drop(guard);
    title_delivery_apply(
        &state,
        &before_enter,
        PaneTitleSource::Automatic,
        "Old inference",
    )
    .await;
    assert_eq!(state.tree.read().await.get(pane_id).unwrap().name, "cat");
    let after_enter = title_delivery_observation(&state, pane_id).await;
    title_delivery_apply(
        &state,
        &after_enter,
        PaneTitleSource::Automatic,
        "Authentication",
    )
    .await;
    assert_eq!(
        state.tree.read().await.get(pane_id).unwrap().name,
        "Authentication"
    );
    {
        let mut panes = state.panes.write().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
            panic!("fixture-owned terminal");
        };
        runtime.agent_generation += 1;
        // Same PID and visible session do not transfer the old receipt.
    }
    let replacement = title_delivery_observation(&state, pane_id).await;
    title_delivery_apply(
        &state,
        &replacement,
        PaneTitleSource::Automatic,
        "Replacement leak",
    )
    .await;
    assert_eq!(
        state.tree.read().await.get(pane_id).unwrap().name,
        "Authentication"
    );
    teardown_state_panes(&state);
}

#[tokio::test]
async fn synthetic_submission_sources_and_controls_never_create_task_receipts() {
    let (state, pane_id, _directory) =
        state_with_one_terminal_pane("title-source-exclusions").await;
    title_delivery_identity(&state, pane_id).await;
    {
        let mut tree = state.tree.write().await;
        let mut panes = state.panes.write().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
            panic!("fixture-owned terminal");
        };
        for source in [
            PromptSubmissionSource::TextTrigger,
            PromptSubmissionSource::AskForUpdate,
            PromptSubmissionSource::ProgressResult,
            PromptSubmissionSource::ToolbarAction,
        ] {
            assert!(
                !record_authored_title_receipt(
                    &mut tree,
                    runtime,
                    pane_id,
                    "Implement authentication",
                    source
                ),
                "{source:?}"
            );
            assert!(runtime.authored_title_receipt.is_none());
        }
        // Auto-answer keys carry no authored source; raw writer receipts do not
        // call this hook. Even misclassified keyboard control text is rejected.
        for text in [
            "/clear",
            "/resume previous",
            "/goal clear",
            "Ilium progress monitor 2 reports completion",
        ] {
            assert!(
                !record_authored_title_receipt(
                    &mut tree,
                    runtime,
                    pane_id,
                    text,
                    PromptSubmissionSource::Keyboard
                ),
                "{text}"
            );
        }
        assert!(invalidate_submitted_title_observation(
            &mut tree, runtime, pane_id
        ));
        assert!(
            runtime.authored_title_receipt.is_none(),
            "unknown Enter revokes eligibility"
        );
    }
    // Auto-answer uses the raw ordered writer without an authored submission
    // source. A fully acknowledged synthetic answer on this owned PTY must
    // therefore leave eligibility absent as well.
    let input = {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            panic!("fixture-owned terminal");
        };
        runtime.session.input_handle()
    };
    input.write(b"y\r").unwrap().wait().await.unwrap();
    {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            panic!("fixture-owned terminal");
        };
        assert!(runtime.authored_title_receipt.is_none());
    }
    let observation = title_delivery_observation(&state, pane_id).await;
    title_delivery_apply(
        &state,
        &observation,
        PaneTitleSource::Automatic,
        "Injected task",
    )
    .await;
    assert_eq!(state.tree.read().await.get(pane_id).unwrap().name, "cat");
    teardown_state_panes(&state);
}

#[tokio::test]
async fn unavailable_agent_title_grants_require_verified_own_project_history() {
    let (state, pane_id, _directory) =
        state_with_one_terminal_pane("title-unavailable-history").await;
    title_delivery_identity(&state, pane_id).await;
    {
        let mut tree = state.tree.write().await;
        let mut panes = state.panes.write().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
            panic!("fixture-owned terminal");
        };
        let owner = runtime.agent_process_key.clone().unwrap();
        tree.set_pane_status(
            pane_id,
            PaneStatus::AgentUnavailable(Box::new(ilium_core::AgentRecovery {
                last_known_state: ilium_core::AgentState::from_activity(
                    ilium_core::AgentClass::Codex,
                    ilium_core::AgentActivity::Idle,
                    None,
                ),
                process: owner,
                availability: ilium_core::AgentAvailability::Unverified,
                signal_name: None,
                session_id: Some(DELIVERY_SESSION.into()),
                last_prompt: Some("Historical text is not eligibility".into()),
                previous_exact_prompt: None,
                latest_prompt_unavailable: false,
            })),
        )
        .unwrap();
        runtime.detected_agent_class = None;
        runtime.detected_agent_process_id = None;
        runtime.agent_process_key = None;
        runtime.session_id = None;
        runtime.session_agent_class = None;
        runtime.agent_input_available = false;
    }
    for valid_project in [None, Some(false), Some(true)] {
        if let Some(is_own) = valid_project {
            let unrelated = state.home_dir.join("unrelated-project");
            std::fs::create_dir_all(&unrelated).unwrap();
            title_delivery_history(
                &state,
                if is_own {
                    &state.session_cwd
                } else {
                    &unrelated
                },
            );
        }
        let observation = title_delivery_observation(&state, pane_id).await;
        let evidence = collect_observed_title_evidence(&state, &[observation]).await;
        let tree = state.tree.read().await;
        let panes = state.panes.read().await;
        let grants = title_grants_under_lock(&tree, &panes, &evidence, &HashMap::new());
        assert_eq!(
            grants.len(),
            usize::from(valid_project == Some(true)),
            "history={valid_project:?}"
        );
    }
    let observation = title_delivery_observation(&state, pane_id).await;
    let evidence = collect_observed_title_evidence(&state, &[observation]).await;
    {
        let mut panes = state.panes.write().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
            panic!("fixture-owned terminal");
        };
        runtime.is_session_identity_invalidated = true;
    }
    {
        let tree = state.tree.read().await;
        let panes = state.panes.read().await;
        assert!(title_grants_under_lock(&tree, &panes, &evidence, &HashMap::new()).is_empty());
    }
    teardown_state_panes(&state);
}

#[tokio::test]
async fn automatic_and_explicit_session_titles_require_eligibility_and_preserve_manual_names() {
    for source in [PaneTitleSource::Automatic, PaneTitleSource::UserSpecified] {
        let (state, pane_id, _directory) = state_with_one_terminal_pane("title-session-cas").await;
        title_delivery_identity(&state, pane_id).await;
        let observation = title_delivery_observation(&state, pane_id).await;
        title_delivery_apply(&state, &observation, source, "Unasked title").await;
        assert_eq!(state.tree.read().await.get(pane_id).unwrap().name, "cat");
        title_delivery_history(&state, &state.session_cwd);
        let original = state.tree.read().await.get(pane_id).unwrap().clone();
        let mut stale = vec![observation.clone(); 3];
        stale[0].process_id = Some(43);
        stale[1].title_generation += 1;
        stale[2].session_id = Some("55555555-5555-4555-8555-555555555555".into());
        for mismatched in stale {
            title_delivery_apply(&state, &mismatched, source, "Wrong identity").await;
            assert_eq!(state.tree.read().await.get(pane_id).unwrap(), &original);
        }
        title_delivery_apply(&state, &observation, source, "Authentication").await;
        let accepted = state.tree.read().await.get(pane_id).unwrap().clone();
        assert_eq!(accepted.name, "Authentication");
        assert!(
            !accepted.is_name_fixed,
            "AI retitle is never manual ownership"
        );
        assert!(matches!(
            accepted.kind,
            NodeKind::Pane {
                title_source: PaneTitleSource::Automatic,
                ..
            }
        ));
        title_delivery_apply(&state, &observation, source, "Stale old inference").await;
        assert_eq!(state.tree.read().await.get(pane_id).unwrap(), &accepted);
        let before_manual = title_delivery_observation(&state, pane_id).await;
        state
            .tree
            .write()
            .await
            .rename_node(
                pane_id,
                "My fixed name",
                Some("Mine".into()),
                Some("pin".into()),
            )
            .unwrap();
        let manual = state.tree.read().await.get(pane_id).unwrap().clone();
        title_delivery_apply(&state, &before_manual, source, "Stale manual overwrite").await;
        let after_manual = title_delivery_observation(&state, pane_id).await;
        title_delivery_apply(&state, &after_manual, source, "Fresh manual overwrite").await;
        assert_eq!(state.tree.read().await.get(pane_id).unwrap(), &manual);
        teardown_state_panes(&state);
    }
}
