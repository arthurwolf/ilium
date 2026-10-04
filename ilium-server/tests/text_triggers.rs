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
use common::{expect_event, read_initial_state, TestServer};

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
    matched_output_round_trip("text-trigger-terminal", 0).await;
}

#[tokio::test]
async fn the_reply_waits_for_the_rules_delay_after_detection() {
    let waited = matched_output_round_trip("text-trigger-delayed", 3).await;
    assert!(
        waited >= Duration::from_millis(2900),
        "delay of 3 s elapsed only {waited:?} between detection and send"
    );
}

/// Returns detection (match visible on screen) to submission time.
async fn matched_output_round_trip(session: &str, delay_seconds: u32) -> Duration {
    let fixture_dir = tempfile::tempdir().expect("create text-trigger fixture directory");
    let fixture = install(
        fixture_dir.path(),
        "text-trigger-emitter",
        &FixtureBehavior::DelayedComposerThenEcho { delay_seconds: 1 },
    );
    let mut server = TestServer::start(session).await;
    let mut client = server.connect().await;
    write_frame(
        &mut client,
        &ClientRequest::Attach {
            session: session.to_owned(),
        },
    )
    .await
    .expect("attach request");
    read_initial_state(&mut client, Duration::from_secs(5)).await;

    let settings = TextTriggerSettings {
        triggers: vec![TextTrigger {
            id: "ready-reply".to_owned(),
            enabled: true,
            regexp: "Explain this codebase".to_owned(),
            message: "continue exactly once".to_owned(),
            target: TextTriggerTarget::Terminals,
            sample_text: "› Explain this codebase".to_owned(),
            delay_seconds,
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

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5 + u64::from(delay_seconds));
    let mut waited = Duration::ZERO;
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
                waited = displayed_at.elapsed();
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
    waited
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
    read_initial_state(&mut client, Duration::from_secs(5)).await;
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
    read_initial_state(&mut client, Duration::from_secs(5)).await;

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
                    delay_seconds: 0,
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
    read_initial_state(&mut client, Duration::from_secs(5)).await;

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
                    delay_seconds: 0,
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

#[tokio::test]
async fn a_second_attach_receives_accepted_text_triggers_without_replacing_them() {
    let mut server = TestServer::start("text-trigger-attach-state").await;
    let mut first = server.connect().await;
    let settings = TextTriggerSettings {
        triggers: vec![TextTrigger {
            id: "retained-rule".to_owned(),
            regexp: "fixture-ready".to_owned(),
            message: "fixture-reply".to_owned(),
            ..TextTrigger::default()
        }],
    };
    write_frame(
        &mut first,
        &ClientRequest::UpdateTextTriggers {
            settings: settings.clone(),
        },
    )
    .await
    .expect("apply isolated rule");
    expect_event(&mut first, Duration::from_secs(5), |event| {
        matches!(event, ServerEvent::TextTriggersChanged { settings: accepted, .. } if accepted == &settings)
    }).await;
    let mut second = server.connect().await;
    write_frame(
        &mut second,
        &ClientRequest::AttachInteractive {
            session: "text-trigger-attach-state".to_owned(),
        },
    )
    .await
    .expect("attach second client");
    let (_, initial_events) = read_initial_state(&mut second, Duration::from_secs(5)).await;
    assert!(initial_events.iter().any(|event| {
        matches!(event, ServerEvent::TextTriggersChanged { settings: accepted, .. } if accepted == &settings)
    }), "an attaching client must receive the retained rule list before initial sync completes");
    write_frame(&mut first, &ClientRequest::KillSession)
        .await
        .expect("stop owned server");
    tokio::time::timeout(Duration::from_secs(5), &mut server.server_task)
        .await
        .expect("server shutdown deadline")
        .expect("server task join")
        .expect("server shutdown");
}

#[tokio::test]
async fn a_fresh_server_attaches_with_durable_rules_without_a_client_replacement() {
    use ilium_server::{run, NoopSoundPlayer, ServerOptions};
    use ilium_transport::SessionEndpoint;
    use std::sync::Arc;

    struct OwnedServer(
        Option<tokio::task::JoinHandle<Result<(), ilium_server::error::ServerError>>>,
    );
    impl Drop for OwnedServer {
        fn drop(&mut self) {
            if let Some(task) = &self.0 {
                task.abort();
            }
        }
    }

    let directory = tempfile::tempdir().unwrap();
    let config_path = directory.path().join("config.toml");
    let socket_path = directory.path().join("tt.sock");
    let rules = TextTriggerSettings {
        triggers: vec![TextTrigger {
            id: "retained-durable-id".to_owned(),
            regexp: "ready".to_owned(),
            message: "reply".to_owned(),
            ..TextTrigger::default()
        }],
    };
    let mut document = toml::value::Table::new();
    document.insert(
        "text_triggers".to_owned(),
        toml::Value::try_from(&rules).unwrap(),
    );
    let durable_bytes = toml::to_string(&document).unwrap();
    std::fs::write(&config_path, &durable_bytes).unwrap();
    let mut server = OwnedServer(Some(tokio::spawn(run(ServerOptions {
        session_name: "durable-trigger".to_owned(),
        socket_path: socket_path.clone(),
        snapshot_path: directory.path().join("snapshot.json"),
        ready_log_metadata: None,
        session_cwd: ilium_platform::paths::canonicalize(directory.path()).unwrap(),
        home_dir: directory.path().to_path_buf(),
        detection_config: ilium_server::config::DetectionConfig::default(),
        notifications_config: ilium_server::config::NotificationsConfig::default(),
        sound_settings: ilium_sound::SoundSettings::default(),
        sound_config_path: Some(config_path.clone()),
        sound_player: Arc::new(NoopSoundPlayer),
        custom_signatures: Vec::new(),
        session_recovery: ilium_server::config::SessionRecoveryConfig::StartFresh,
        session_backups_enabled: false,
        agent_debug_menu_enabled: false,
        http_api: ilium_server::config::HttpApiConfig { port: 0 },
        progress_monitor_enabled: true,
    }))));
    let endpoint = SessionEndpoint::from_path(&socket_path);
    let mut client = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(client) = endpoint.connect().await {
                break client;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    write_frame(
        &mut client,
        &ClientRequest::AttachInteractive {
            session: "durable-trigger".to_owned(),
        },
    )
    .await
    .unwrap();
    let (_, events) = read_initial_state(&mut client, Duration::from_secs(5)).await;
    // Stop our isolated process even when the acceptance assertion fails.
    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        assert!(server.0.take().unwrap().await.unwrap().is_ok());
    })
    .await
    .unwrap();
    assert_eq!(std::fs::read_to_string(config_path).unwrap(), durable_bytes);
    assert!(events.iter().any(|event| matches!(event,
        ServerEvent::TextTriggersChanged { settings, .. } if settings == &rules
    )), "a fresh server must load durable rules before its first attach without a client replacement");
}

#[tokio::test]
async fn a_running_server_reconciles_durable_trigger_edits_without_client_replacement() {
    use ilium_server::{run, NoopSoundPlayer, ServerOptions};
    use ilium_transport::SessionEndpoint;
    use std::sync::Arc;

    struct OwnedServer(
        Option<tokio::task::JoinHandle<Result<(), ilium_server::error::ServerError>>>,
    );
    impl Drop for OwnedServer {
        fn drop(&mut self) {
            if let Some(task) = &self.0 {
                task.abort();
            }
        }
    }

    let directory = tempfile::tempdir().unwrap();
    let config_path = directory.path().join("config.toml");
    let socket_path = directory.path().join("tt.sock");
    let rules = TextTriggerSettings {
        triggers: vec![TextTrigger {
            id: "retained-durable-id".to_owned(),
            regexp: "ready".to_owned(),
            message: "reply".to_owned(),
            ..TextTrigger::default()
        }],
    };
    let mut document = toml::value::Table::new();
    document.insert(
        "text_triggers".to_owned(),
        toml::Value::try_from(&rules).unwrap(),
    );
    let durable_bytes = toml::to_string(&document).unwrap();
    std::fs::write(&config_path, &durable_bytes).unwrap();
    let mut server = OwnedServer(Some(tokio::spawn(run(ServerOptions {
        session_name: "durable-trigger".to_owned(),
        socket_path: socket_path.clone(),
        snapshot_path: directory.path().join("snapshot.json"),
        ready_log_metadata: None,
        session_cwd: ilium_platform::paths::canonicalize(directory.path()).unwrap(),
        home_dir: directory.path().to_path_buf(),
        detection_config: ilium_server::config::DetectionConfig::default(),
        notifications_config: ilium_server::config::NotificationsConfig::default(),
        sound_settings: ilium_sound::SoundSettings::default(),
        sound_config_path: Some(config_path.clone()),
        sound_player: Arc::new(NoopSoundPlayer),
        custom_signatures: Vec::new(),
        session_recovery: ilium_server::config::SessionRecoveryConfig::StartFresh,
        session_backups_enabled: false,
        agent_debug_menu_enabled: false,
        http_api: ilium_server::config::HttpApiConfig { port: 0 },
        progress_monitor_enabled: true,
    }))));
    let endpoint = SessionEndpoint::from_path(&socket_path);
    let mut client = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(client) = endpoint.connect().await {
                break client;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    write_frame(
        &mut client,
        &ClientRequest::AttachInteractive {
            session: "durable-trigger".to_owned(),
        },
    )
    .await
    .unwrap();
    let (_, events) = read_initial_state(&mut client, Duration::from_secs(5)).await;
    let mut updated = rules.clone();
    updated.triggers[0].message = "changed durable reply".to_owned();
    document.insert(
        "text_triggers".to_owned(),
        toml::Value::try_from(&updated).unwrap(),
    );
    let updated_bytes = toml::to_string(&document).unwrap();
    std::fs::write(&config_path, &updated_bytes).unwrap();
    let received_update = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event: ServerEvent = read_frame(&mut client).await.unwrap();
            if matches!(event, ServerEvent::TextTriggersChanged { settings, .. } if settings == updated) {
                break;
            }
        }
    }).await.is_ok();
    // Stop our isolated process even when the acceptance assertion fails.
    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        assert!(server.0.take().unwrap().await.unwrap().is_ok());
    })
    .await
    .unwrap();
    assert_eq!(std::fs::read_to_string(config_path).unwrap(), updated_bytes);
    assert!(
        received_update,
        "running server must reconcile durable edits without a client replacement"
    );
    assert!(events.iter().any(|event| matches!(event,
        ServerEvent::TextTriggersChanged { settings, .. } if settings == &rules
    )), "a fresh server must load durable rules before its first attach without a client replacement");
}

#[tokio::test]
async fn a_stale_client_default_payload_cannot_replace_durable_rules() {
    use ilium_server::{run, NoopSoundPlayer, ServerOptions};
    use ilium_transport::SessionEndpoint;
    use std::sync::Arc;

    struct OwnedServer(
        Option<tokio::task::JoinHandle<Result<(), ilium_server::error::ServerError>>>,
    );
    impl Drop for OwnedServer {
        fn drop(&mut self) {
            if let Some(task) = &self.0 {
                task.abort();
            }
        }
    }

    let directory = tempfile::tempdir().unwrap();
    let config_path = directory.path().join("config.toml");
    let socket_path = directory.path().join("tt.sock");
    let rules = TextTriggerSettings {
        triggers: vec![TextTrigger {
            id: "retained-durable-id".to_owned(),
            regexp: "ready".to_owned(),
            message: "reply".to_owned(),
            ..TextTrigger::default()
        }],
    };
    let mut document = toml::value::Table::new();
    document.insert(
        "text_triggers".to_owned(),
        toml::Value::try_from(&rules).unwrap(),
    );
    let durable_bytes = toml::to_string(&document).unwrap();
    std::fs::write(&config_path, &durable_bytes).unwrap();
    let mut server = OwnedServer(Some(tokio::spawn(run(ServerOptions {
        session_name: "durable-trigger".to_owned(),
        socket_path: socket_path.clone(),
        snapshot_path: directory.path().join("snapshot.json"),
        ready_log_metadata: None,
        session_cwd: ilium_platform::paths::canonicalize(directory.path()).unwrap(),
        home_dir: directory.path().to_path_buf(),
        detection_config: ilium_server::config::DetectionConfig::default(),
        notifications_config: ilium_server::config::NotificationsConfig::default(),
        sound_settings: ilium_sound::SoundSettings::default(),
        sound_config_path: Some(config_path.clone()),
        sound_player: Arc::new(NoopSoundPlayer),
        custom_signatures: Vec::new(),
        session_recovery: ilium_server::config::SessionRecoveryConfig::StartFresh,
        session_backups_enabled: false,
        agent_debug_menu_enabled: false,
        http_api: ilium_server::config::HttpApiConfig { port: 0 },
        progress_monitor_enabled: true,
    }))));
    let endpoint = SessionEndpoint::from_path(&socket_path);
    let mut client = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(client) = endpoint.connect().await {
                break client;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    write_frame(
        &mut client,
        &ClientRequest::AttachInteractive {
            session: "durable-trigger".to_owned(),
        },
    )
    .await
    .unwrap();
    let (_, events) = read_initial_state(&mut client, Duration::from_secs(5)).await;
    write_frame(
        &mut client,
        &ClientRequest::UpdateTextTriggers {
            settings: TextTriggerSettings::default(),
        },
    )
    .await
    .unwrap();
    let after_stale_request = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event: ServerEvent = read_frame(&mut client).await.unwrap();
            if let ServerEvent::TextTriggersChanged { settings, .. } = event {
                break settings;
            }
        }
    })
    .await
    .unwrap();
    // Stop our isolated process even when the acceptance assertion fails.
    write_frame(&mut client, &ClientRequest::KillSession)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        assert!(server.0.take().unwrap().await.unwrap().is_ok());
    })
    .await
    .unwrap();
    assert_eq!(std::fs::read_to_string(config_path).unwrap(), durable_bytes);
    assert_eq!(
        after_stale_request, rules,
        "a stale default client payload must not replace the durable rules"
    );
    assert!(events.iter().any(|event| matches!(event,
        ServerEvent::TextTriggersChanged { settings, .. } if settings == &rules
    )), "a fresh server must load durable rules before its first attach without a client replacement");
}
