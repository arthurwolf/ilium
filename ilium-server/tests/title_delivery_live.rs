//! Receipt authorization through real detection, IPC admission and PTY writes.
//! The external agent is a synthetic absolute-path fixture; no provider runs.

use std::time::Duration;

use ilium_core::{PaneTitleSource, ROOT_ID};
use ilium_ipc::{
    read_frame, write_frame, ClientRequest, NewPaneKind, NewPaneWorkingDirectory,
    PromptSubmissionSource, ServerEvent,
};
use ilium_server::config::DetectionConfig;
use ilium_test_fixtures::{install, FixtureBehavior};

mod common;
use common::{expect_event, read_initial_state, TestServer};

const SESSION: &str = "live-title-delivery";
const HISTORY_ID: &str = "3fba5b29-4c26-4a70-98df-9703d91e1461";
const WAIT: Duration = Duration::from_secs(30);

fn assert_title_bundle(before: &ilium_core::Node, after: &ilium_core::Node) {
    assert_eq!(after.id, before.id);
    assert_eq!(after.parent, before.parent);
    assert_eq!(after.name, before.name);
    assert_eq!(after.short_name, before.short_name);
    assert_eq!(after.inferred_icon, before.inferred_icon);
    assert_eq!(after.is_name_fixed, before.is_name_fixed);
    assert_eq!(after.presentation_revision, before.presentation_revision);
    let source = |node: &ilium_core::Node| match node.kind {
        ilium_core::NodeKind::Pane { title_source, .. } => title_source,
        _ => panic!("expected fixture pane"),
    };
    assert_eq!(source(after), source(before));
}

#[tokio::test]
async fn acknowledged_authored_enter_authorizes_title_before_request_history_flush() {
    let mut server = TestServer::start_with_detection_config(
        SESSION,
        DetectionConfig {
            working_poll_interval: Duration::from_millis(100),
            idle_poll_interval: Duration::from_millis(100),
            auto_answer_interstitial_prompts: false,
        },
    )
    .await;
    let fixtures = tempfile::tempdir().unwrap();
    let agent = install(
        fixtures.path(),
        "codex",
        &FixtureBehavior::HoldArgument { argument_index: 1 },
    );
    let directory = server.home_dir.join(".codex/sessions/2026/10/03");
    std::fs::create_dir_all(&directory).unwrap();
    let history = directory.join(format!("rollout-2026-10-03T00-00-00-{HISTORY_ID}.jsonl"));
    let metadata = format!(
        "{}\n",
        serde_json::json!({"type":"session_meta","payload":{"id":HISTORY_ID,"cwd":server.project_cwd}})
    );
    std::fs::write(&history, &metadata).unwrap();
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: SESSION.into(),
        },
    )
    .await
    .unwrap();
    read_initial_state(&mut client, WAIT).await;
    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::Command(format!("{} {}", agent.path.display(), history.display())),
            working_directory: NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .unwrap();
    let resolution = expect_event(&mut client, WAIT, |event| {
        matches!(event, ServerEvent::PaneSessionIdResolved { session_id, .. } if session_id == HISTORY_ID)
    })
    .await;
    let ServerEvent::PaneSessionIdResolved {
        pane_id,
        process_id,
        title_generation,
        ..
    } = resolution
    else {
        unreachable!()
    };
    assert!(process_id.is_some());
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: SESSION.into(),
        },
    )
    .await
    .unwrap();
    let (mut before, _) = read_initial_state(&mut client, WAIT).await;
    // Verified empty own history resets the launch label asynchronously.
    // Capture the preservation baseline only after that legitimate repair.
    if before.get(pane_id).unwrap().name != "<new>" {
        let reset = expect_event(&mut client, WAIT, |event| {
            matches!(event, ServerEvent::TreeSnapshot(tree) if tree.get(pane_id).is_some_and(|node| node.name == "<new>"))
        })
        .await;
        let ServerEvent::TreeSnapshot(tree) = reset else {
            unreachable!()
        };
        before = tree;
    }
    let original = before.get(pane_id).unwrap().clone();
    let title_request = |revision, title: &str| ClientRequest::SetSessionPaneTitle {
        pane_id,
        expected_session_id: HISTORY_ID.into(),
        expected_title_generation: title_generation,
        expected_presentation_revision: revision,
        expected_process_id: process_id,
        title: title.into(),
        short_title: Some("Auth task".into()),
        inferred_icon: Some("lock".into()),
        title_source: PaneTitleSource::Automatic,
    };

    // A body write without Enter is real delivered input, but not a task receipt.
    write_frame(
        &mut client,
        &ClientRequest::UserKeyInput {
            pane_id,
            bytes: b"Implement authentication validation".to_vec(),
            submission: None,
            prompt_epoch: None,
        },
    )
    .await
    .unwrap();
    write_frame(
        &mut client,
        &title_request(original.presentation_revision, "Unsubmitted title"),
    )
    .await
    .unwrap();
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: SESSION.into(),
        },
    )
    .await
    .unwrap();
    let (body_only, _) = read_initial_state(&mut client, WAIT).await;
    assert_title_bundle(&original, body_only.get(pane_id).unwrap());
    assert_eq!(body_only.last_prompt(pane_id), before.last_prompt(pane_id));

    write_frame(
        &mut client,
        &ClientRequest::UserKeyInput {
            pane_id,
            bytes: b"\r".to_vec(),
            submission: Some(PromptSubmissionSource::Keyboard),
            prompt_epoch: Some("live-title-delivery-enter-1".into()),
        },
    )
    .await
    .unwrap();
    let mut observed_revision_fence = false;
    tokio::time::timeout(WAIT, async {
        loop {
            match read_frame::<ServerEvent, _>(&mut client).await.unwrap() {
                ServerEvent::TreeSnapshot(tree) => {
                    observed_revision_fence |= tree.get(pane_id).is_some_and(|node| {
                        node.presentation_revision > original.presentation_revision
                    });
                }
                ServerEvent::PanePromptSubmitted {
                    pane_id: submitted,
                    source,
                } if submitted == pane_id => {
                    assert_eq!(source, PromptSubmissionSource::Keyboard);
                    assert!(
                        observed_revision_fence,
                        "presentation fence must precede prompt triggers"
                    );
                    break;
                }
                ServerEvent::Error { message } => panic!("input admission failed: {message}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: SESSION.into(),
        },
    )
    .await
    .unwrap();
    let (submitted, _) = read_initial_state(&mut client, WAIT).await;
    let current = submitted.get(pane_id).unwrap();
    assert_eq!(
        submitted.last_prompt(pane_id),
        Some("Implement authentication validation")
    );
    assert!(current.presentation_revision > original.presentation_revision);
    assert_eq!(
        std::fs::read_to_string(&history).unwrap(),
        metadata,
        "the fixture has not flushed any genuine request history"
    );

    write_frame(
        &mut client,
        &title_request(current.presentation_revision, "Authentication validation"),
    )
    .await
    .unwrap();
    let accepted = expect_event(&mut client, WAIT, |event| {
        matches!(event, ServerEvent::TreeSnapshot(tree) if tree.get(pane_id).is_some_and(|node| node.name == "Authentication validation"))
    }).await;
    let ServerEvent::TreeSnapshot(accepted) = accepted else {
        unreachable!()
    };
    let named = accepted.get(pane_id).unwrap();
    assert!(!named.is_name_fixed);
    assert!(matches!(
        named.kind,
        ilium_core::NodeKind::Pane {
            title_source: PaneTitleSource::Automatic,
            ..
        }
    ));
    assert_eq!(std::fs::read_to_string(&history).unwrap(), metadata);

    // The proposal captured before this exact submission cannot return later.
    write_frame(
        &mut client,
        &title_request(original.presentation_revision, "Stale earlier proposal"),
    )
    .await
    .unwrap();
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: SESSION.into(),
        },
    )
    .await
    .unwrap();
    let (final_tree, _) = read_initial_state(&mut client, WAIT).await;
    assert_title_bundle(named, final_tree.get(pane_id).unwrap());
    assert_eq!(
        final_tree.last_prompt(pane_id),
        submitted.last_prompt(pane_id)
    );
    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .unwrap();
    tokio::time::timeout(WAIT, &mut server.server_task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn exited_agent_titles_require_verified_own_history_and_preserve_manual_names() {
    use ilium_core::{AgentClass, PaneStatus, RestructureNode, RestructurePlan};
    use ilium_ipc::PaneTitleObservation;

    const RECOVERY_SESSION: &str = "live-title-recovery";
    const RECOVERY_ID: &str = "f40b28a5-c328-4f9c-9d5f-0ee2a7e70b43";
    let mut server = TestServer::start_with_detection_config(
        RECOVERY_SESSION,
        DetectionConfig {
            working_poll_interval: Duration::from_millis(100),
            idle_poll_interval: Duration::from_millis(100),
            auto_answer_interstitial_prompts: false,
        },
    )
    .await;
    let fixtures = tempfile::tempdir().unwrap();
    let directory = server.home_dir.join(".codex/sessions/2026/10/03");
    std::fs::create_dir_all(&directory).unwrap();
    let history = directory.join(format!("rollout-2026-10-03T00-00-00-{RECOVERY_ID}.jsonl"));
    let encode_history = |cwd: &std::path::Path, genuine: bool| {
        let mut text = format!(
            "{}\n",
            serde_json::json!({"type":"session_meta","payload":{"id":RECOVERY_ID,"cwd":cwd}})
        );
        if genuine {
            text.push_str(&format!(
                "{}\n",
                serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Implement recovery validation"}]}})
            ));
        }
        text
    };
    std::fs::write(&history, encode_history(&server.project_cwd, false)).unwrap();
    let agent = install(
        fixtures.path(),
        "codex",
        &FixtureBehavior::CrashAfterSubmittedPrompt {
            prompt_path: fixtures.path().join("received-input.txt"),
            transcript_path: Some(history.clone()),
            exit_code: 42,
            mouse_tracking: false,
        },
    );
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: RECOVERY_SESSION.into(),
        },
    )
    .await
    .unwrap();
    read_initial_state(&mut client, WAIT).await;
    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::Command(agent.path.display().to_string()),
            working_directory: NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .unwrap();
    let resolution = expect_event(&mut client, WAIT, |event| {
        matches!(event, ServerEvent::PaneSessionIdResolved { session_id, .. } if session_id == RECOVERY_ID)
    }).await;
    let ServerEvent::PaneSessionIdResolved {
        pane_id,
        process_id,
        title_generation,
        ..
    } = resolution
    else {
        unreachable!()
    };
    assert!(process_id.is_some());
    // Raw automation causes the fixture exit without creating authored intent.
    write_frame(
        &mut client,
        &ClientRequest::KeyInput {
            pane_id,
            bytes: b"exit fixture\r".to_vec(),
            submission: None,
        },
    )
    .await
    .unwrap();
    expect_event(&mut client, WAIT, |event| {
        matches!(event, ServerEvent::PaneDetectedStateChanged { pane_id: changed_pane, status: PaneStatus::AgentUnavailable(_), .. } if *changed_pane == pane_id)
    })
    .await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: RECOVERY_SESSION.into(),
        },
    )
    .await
    .unwrap();
    let (mut current, _) = read_initial_state(&mut client, WAIT).await;
    assert_eq!(
        current
            .agent_recovery(pane_id)
            .unwrap()
            .session_id
            .as_deref(),
        Some(RECOVERY_ID)
    );
    let unrelated = fixtures.path().join("unrelated-project");
    std::fs::create_dir_all(&unrelated).unwrap();
    for (cwd, genuine, eligible) in [
        (server.project_cwd.as_path(), false, false),
        (unrelated.as_path(), true, false),
        (server.project_cwd.as_path(), true, true),
    ] {
        std::fs::write(&history, encode_history(cwd, genuine)).unwrap();
        let original = current.get(pane_id).unwrap().clone();
        let project_id = current.project_ancestor(pane_id).unwrap();
        write_frame(
            &mut client,
            &ClientRequest::ApplyProjectRestructurePlan {
                project_id,
                title_observations: vec![PaneTitleObservation {
                    pane_id,
                    presentation_revision: original.presentation_revision,
                    agent_class: Some(AgentClass::Codex),
                    session_id: Some(RECOVERY_ID.into()),
                    process_id,
                    title_generation,
                }],
                inference_activity_revisions: current
                    .project_activity_revisions(project_id)
                    .unwrap(),
                plan: RestructurePlan {
                    children: vec![RestructureNode::Pane {
                        id: pane_id,
                        title: "Verified recovery task".into(),
                        short_title: Some("Recovery".into()),
                        icon: Some("lock".into()),
                    }],
                },
            },
        )
        .await
        .unwrap();
        write_frame(
            &mut client,
            &ClientRequest::Attach {
                session: RECOVERY_SESSION.into(),
            },
        )
        .await
        .unwrap();
        let (next, events) = read_initial_state(&mut client, WAIT).await;
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, ServerEvent::Error { .. })),
            "{events:?}"
        );
        assert!(
            events.iter().any(|event| matches!(event,
            ServerEvent::ProjectRestructureApplied { project_id: applied, .. }
                if *applied == project_id)),
            "grouping must commit: {events:?}"
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, ServerEvent::ProjectRestructureRejected { .. })),
            "{events:?}"
        );
        let updated = next.get(pane_id).unwrap();
        if eligible {
            assert_eq!(updated.name, "Verified recovery task");
            assert!(!updated.is_name_fixed);
            assert!(updated.presentation_revision > original.presentation_revision);
        } else {
            // The plan may move the leaf; all its presentation must remain exact.
            let mut moved_original = original;
            moved_original.parent = updated.parent;
            assert_title_bundle(&moved_original, updated);
        }
        current = next;
    }
    let old_revision = current.get(pane_id).unwrap().presentation_revision;
    write_frame(
        &mut client,
        &ClientRequest::RenameNode {
            node_id: pane_id,
            title: "Authored recovery name".into(),
            short_title: Some("Mine".into()),
            inferred_icon: Some("pin".into()),
        },
    )
    .await
    .unwrap();
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: RECOVERY_SESSION.into(),
        },
    )
    .await
    .unwrap();
    let (manual, _) = read_initial_state(&mut client, WAIT).await;
    let project_id = manual.project_ancestor(pane_id).unwrap();
    write_frame(
        &mut client,
        &ClientRequest::ApplyProjectRestructurePlan {
            project_id,
            title_observations: vec![PaneTitleObservation {
                pane_id,
                presentation_revision: old_revision,
                agent_class: Some(AgentClass::Codex),
                session_id: Some(RECOVERY_ID.into()),
                process_id,
                title_generation,
            }],
            inference_activity_revisions: manual.project_activity_revisions(project_id).unwrap(),
            plan: RestructurePlan {
                children: vec![RestructureNode::Pane {
                    id: pane_id,
                    title: "Late recovered title".into(),
                    short_title: None,
                    icon: None,
                }],
            },
        },
    )
    .await
    .unwrap();
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: RECOVERY_SESSION.into(),
        },
    )
    .await
    .unwrap();
    let (final_tree, events) = read_initial_state(&mut client, WAIT).await;
    assert!(
        events.iter().any(|event| matches!(event,
        ServerEvent::ProjectRestructureApplied { project_id: applied, .. }
            if *applied == project_id)),
        "stale title must not reject grouping: {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ServerEvent::ProjectRestructureRejected { .. })),
        "{events:?}"
    );
    assert!(manual.get(pane_id).unwrap().is_name_fixed);
    assert_title_bundle(
        manual.get(pane_id).unwrap(),
        final_tree.get(pane_id).unwrap(),
    );
    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .unwrap();
    tokio::time::timeout(WAIT, &mut server.server_task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
