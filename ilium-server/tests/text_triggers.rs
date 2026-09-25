//! Real-server acceptance test for Text Triggers: output from a detached
//! plain terminal matches a configured regexp and causes literal input plus
//! one Enter to reach that same PTY.

use std::time::Duration;

use ilium_core::{NodeId, ROOT_ID};
use ilium_ipc::{
    read_frame, write_frame, ClientRequest, NewPaneKind, ServerEvent, TextTrigger,
    TextTriggerSettings, TextTriggerTarget,
};
use ilium_test_fixtures::{install, FixtureBehavior};

mod common;
use common::{expect_event, TestServer};

fn first_launch_project_pane(tree: &ilium_core::Tree) -> NodeId {
    let project_id = tree
        .project_ids()
        .into_iter()
        .next()
        .expect("launch project exists");
    let default_group = tree.children_of(project_id).unwrap()[0];
    tree.children_of(default_group).unwrap()[0]
}

#[tokio::test]
async fn matched_terminal_output_submits_the_literal_reply_with_enter() {
    let fixture_dir = tempfile::tempdir().expect("create text-trigger fixture directory");
    let fixture = install(
        fixture_dir.path(),
        "text-trigger-emitter",
        &FixtureBehavior::DelayedComposerThenEcho { delay_seconds: 1 },
    );
    let mut server = TestServer::start("text-trigger-terminal").await;
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "text-trigger-terminal".to_owned(),
        },
    )
    .await
    .expect("attach request");
    let _ = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::InitialStateSyncComplete)
    })
    .await;

    let settings = TextTriggerSettings {
        triggers: vec![TextTrigger {
            id: "ready-reply".to_owned(),
            enabled: true,
            regexp: "Explain this codebase".to_owned(),
            message: "continue exactly once".to_owned(),
            target: TextTriggerTarget::Terminals,
            sample_text: "› Explain this codebase".to_owned(),
        }],
    };
    write_frame(
        &mut client,
        &ClientRequest::UpdateTextTriggers {
            settings: settings.clone(),
        },
    )
    .await
    .expect("text trigger update");
    let changed = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TextTriggersChanged { settings: received } if received == &settings)
    })
    .await;
    assert!(matches!(changed, ServerEvent::TextTriggersChanged { .. }));

    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::Command(fixture.path.to_string_lossy().into_owned()),
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .expect("new terminal pane");
    let tree = expect_event(
        &mut client,
        Duration::from_secs(5),
        |event| matches!(event, ServerEvent::TreeSnapshot(tree) if tree.panes().count() == 1),
    )
    .await;
    let ServerEvent::TreeSnapshot(tree) = tree else {
        unreachable!("predicate returned a populated tree snapshot")
    };
    let pane_id = first_launch_project_pane(&tree);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut saw_submission = false;
    let mut screen_bytes = Vec::new();
    let mut trigger_displayed_at: Option<tokio::time::Instant> = None;
    while !saw_submission
        || !String::from_utf8_lossy(&screen_bytes)
            .contains("received-after-ready:<continue exactly once>")
    {
        let event = tokio::time::timeout_at(deadline, read_frame::<ServerEvent, _>(&mut client))
            .await
            .expect("text trigger outcome should arrive before timeout")
            .expect("read text trigger outcome");
        match event {
            ServerEvent::PanePromptSubmitted {
                pane_id: submitted_pane_id,
                source: ilium_ipc::PromptSubmissionSource::TextTrigger,
            } if submitted_pane_id == pane_id => {
                let displayed_at = trigger_displayed_at
                    .expect("matching output must be visible before its reply is submitted");
                assert!(
                    displayed_at.elapsed() >= Duration::from_millis(200),
                    "Enter must follow the text in a separate, processed input burst"
                );
                saw_submission = true;
            }
            ServerEvent::ScreenUpdate {
                pane_id: updated_pane_id,
                bytes,
                ..
            } if updated_pane_id == pane_id => {
                screen_bytes.extend_from_slice(&bytes);
                if trigger_displayed_at.is_none()
                    && String::from_utf8_lossy(&screen_bytes).contains("Explain this codebase")
                {
                    trigger_displayed_at = Some(tokio::time::Instant::now());
                }
            }
            _ => {}
        }
    }

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("kill isolated test session");
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}

#[tokio::test]
async fn client_semantic_submission_reaches_the_pty_after_a_separate_enter() {
    let fixture_dir = tempfile::tempdir().expect("create submission fixture directory");
    let fixture = install(
        fixture_dir.path(),
        "semantic-submission-echo",
        &FixtureBehavior::EchoSubmittedLine {
            prefix: "submitted".to_owned(),
        },
    );
    let mut server = TestServer::start("semantic-submission-terminal").await;
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "semantic-submission-terminal".to_owned(),
        },
    )
    .await
    .expect("attach request");
    let _ = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::InitialStateSyncComplete)
    })
    .await;
    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::Command(fixture.path.to_string_lossy().into_owned()),
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .expect("new terminal pane");
    let tree = expect_event(
        &mut client,
        Duration::from_secs(5),
        |event| matches!(event, ServerEvent::TreeSnapshot(tree) if tree.panes().count() == 1),
    )
    .await;
    let ServerEvent::TreeSnapshot(tree) = tree else {
        unreachable!("predicate returned a populated tree snapshot")
    };
    let pane_id = first_launch_project_pane(&tree);

    let sent_at = tokio::time::Instant::now();
    write_frame(
        &mut client,
        &ClientRequest::SubmitTerminalText {
            pane_id,
            text: "one literal command".to_owned(),
            source: ilium_ipc::PromptSubmissionSource::ToolbarAction,
        },
    )
    .await
    .expect("semantic submission request");

    let deadline = sent_at + Duration::from_secs(5);
    let mut saw_submission = false;
    let mut screen_bytes = Vec::new();
    while !saw_submission
        || !String::from_utf8_lossy(&screen_bytes).contains("submitted:<one literal command>")
    {
        let event = tokio::time::timeout_at(deadline, read_frame::<ServerEvent, _>(&mut client))
            .await
            .expect("semantic submission outcome should arrive before timeout")
            .expect("read semantic submission outcome");
        match event {
            ServerEvent::PanePromptSubmitted {
                pane_id: submitted_pane_id,
                source: ilium_ipc::PromptSubmissionSource::ToolbarAction,
            } if submitted_pane_id == pane_id => {
                assert!(sent_at.elapsed() >= Duration::from_millis(200));
                saw_submission = true;
            }
            ServerEvent::ScreenUpdate {
                pane_id: updated_pane_id,
                bytes,
                ..
            } if updated_pane_id == pane_id => screen_bytes.extend_from_slice(&bytes),
            _ => {}
        }
    }

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("kill isolated test session");
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}

#[tokio::test]
async fn repainting_one_matching_screen_line_submits_only_once() {
    let fixture_dir = tempfile::tempdir().expect("create redraw fixture directory");
    let fixture = install(
        fixture_dir.path(),
        "text-trigger-redraw",
        &FixtureBehavior::ChangeOnly,
    );
    let mut server = TestServer::start("text-trigger-redraw-test").await;
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "text-trigger-redraw-test".to_owned(),
        },
    )
    .await
    .expect("attach request");
    let _ = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::InitialStateSyncComplete)
    })
    .await;

    write_frame(
        &mut client,
        &ClientRequest::UpdateTextTriggers {
            settings: TextTriggerSettings {
                triggers: vec![TextTrigger {
                    id: "repaint-rule".to_owned(),
                    enabled: true,
                    regexp: "Pursuing goal".to_owned(),
                    message: "reply once".to_owned(),
                    target: TextTriggerTarget::Terminals,
                    sample_text: "Pursuing goal".to_owned(),
                }],
            },
        },
    )
    .await
    .expect("text trigger update");
    let _ = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TextTriggersChanged { .. })
    })
    .await;

    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::Command(fixture.path.to_string_lossy().into_owned()),
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .expect("new terminal pane");
    let tree = expect_event(
        &mut client,
        Duration::from_secs(5),
        |event| matches!(event, ServerEvent::TreeSnapshot(tree) if tree.panes().count() == 1),
    )
    .await;
    let ServerEvent::TreeSnapshot(tree) = tree else {
        unreachable!("predicate returned a populated tree snapshot")
    };
    let pane_id = first_launch_project_pane(&tree);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
    let mut screen_bytes = Vec::new();
    let mut submissions = 0;
    while let Ok(Ok(event)) =
        tokio::time::timeout_at(deadline, read_frame::<ServerEvent, _>(&mut client)).await
    {
        match event {
            ServerEvent::PanePromptSubmitted {
                pane_id: submitted_pane_id,
                source: ilium_ipc::PromptSubmissionSource::TextTrigger,
            } if submitted_pane_id == pane_id => submissions += 1,
            ServerEvent::ScreenUpdate {
                pane_id: updated_pane_id,
                bytes,
                ..
            } if updated_pane_id == pane_id => screen_bytes.extend_from_slice(&bytes),
            _ => {}
        }
    }
    let output = String::from_utf8_lossy(&screen_bytes);
    assert!(output.contains("Pursuing goal (1m)"), "first redraw absent");
    assert!(output.contains("Pursuing goal (3m)"), "later redraw absent");
    assert_eq!(
        submissions, 1,
        "one continuously visible match fired more than once"
    );

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("kill isolated test session");
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}

#[tokio::test]
async fn a_match_reappearing_after_the_row_clears_submits_again() {
    let fixture_dir = tempfile::tempdir().expect("create reappearance fixture directory");
    let fixture = install(
        fixture_dir.path(),
        "text-trigger-reappearance",
        &FixtureBehavior::RepaintThenReappear,
    );
    let mut server = TestServer::start("text-trigger-reappearance-test").await;
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: "text-trigger-reappearance-test".to_owned(),
        },
    )
    .await
    .expect("attach request");
    let _ = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::InitialStateSyncComplete)
    })
    .await;

    write_frame(
        &mut client,
        &ClientRequest::UpdateTextTriggers {
            settings: TextTriggerSettings {
                triggers: vec![TextTrigger {
                    id: "reappearance-rule".to_owned(),
                    enabled: true,
                    regexp: "^trigger-ready$".to_owned(),
                    message: "reply to each appearance".to_owned(),
                    target: TextTriggerTarget::Terminals,
                    sample_text: "trigger-ready".to_owned(),
                }],
            },
        },
    )
    .await
    .expect("text trigger update");
    let _ = expect_event(&mut client, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TextTriggersChanged { .. })
    })
    .await;

    write_frame(
        &mut client,
        &ClientRequest::NewPane {
            parent_group: ROOT_ID,
            kind: NewPaneKind::Command(fixture.path.to_string_lossy().into_owned()),
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .expect("new terminal pane");
    let tree = expect_event(
        &mut client,
        Duration::from_secs(5),
        |event| matches!(event, ServerEvent::TreeSnapshot(tree) if tree.panes().count() == 1),
    )
    .await;
    let ServerEvent::TreeSnapshot(tree) = tree else {
        unreachable!("predicate returned a populated tree snapshot")
    };
    let pane_id = first_launch_project_pane(&tree);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(6);
    let mut submissions = 0;
    while let Ok(Ok(event)) =
        tokio::time::timeout_at(deadline, read_frame::<ServerEvent, _>(&mut client)).await
    {
        if matches!(event, ServerEvent::PanePromptSubmitted {
            pane_id: submitted_pane_id,
            source: ilium_ipc::PromptSubmissionSource::TextTrigger,
        } if submitted_pane_id == pane_id)
        {
            submissions += 1;
        }
    }
    assert_eq!(
        submissions, 2,
        "the repainted row and later occurrence were not distinguished"
    );

    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .expect("kill isolated test session");
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.server_task).await;
}
