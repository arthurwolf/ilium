//! Scripted manager using a real server and an absolute-path fake agent fixture.
use ilium_core::{AgentActivity, ROOT_ID};
use ilium_ipc::{
    read_frame, write_frame, ClientRequest, MonitoringFormat, MonitoringQuery, MonitoringView,
    ServerEvent,
};
use ilium_server::config::DetectionConfig;
use ilium_test_fixtures::{install, FixtureBehavior};
use std::time::Duration;
mod common;
use common::{expect_event, read_initial_state, TestServer};

fn query(view: MonitoringView, since: Option<String>) -> MonitoringQuery {
    MonitoringQuery {
        view,
        since,
        consumer: Some("scripted-manager".into()),
        follow: false,
        min_severity: None,
        kinds: Vec::new(),
        project: None,
        agent: None,
        pane_id: None,
        max_items: 12,
        max_chars: 4000,
        format: MonitoringFormat::Json,
    }
}

async fn read(server: &TestServer, query: MonitoringQuery) -> serde_json::Value {
    let mut stream = server.connect().await;
    write_frame(
        &mut stream,
        &ClientRequest::MonitoringEvents {
            request_id: 1,
            query,
        },
    )
    .await
    .unwrap();
    let event: ServerEvent = tokio::time::timeout(Duration::from_secs(30), read_frame(&mut stream))
        .await
        .unwrap()
        .unwrap();
    let ServerEvent::MonitoringEvents {
        request_id: 1,
        result,
    } = event
    else {
        panic!("manager received unrelated event: {event:?}")
    };
    let text = result.unwrap();
    assert!(text.chars().count() <= 4000);
    serde_json::from_str(&text).unwrap()
}

#[tokio::test]
async fn monitoring_manager_follows_digests_drills_acknowledges_and_audits_a_live_fake_agent() {
    let fixture = tempfile::tempdir().unwrap();
    let marker = fixture.path().join("finish");
    let executable = install(
        fixture.path(),
        "codex",
        &FixtureBehavior::WorkingUntilMarker {
            marker_path: marker.clone(),
        },
    );
    let mut server = TestServer::start_with_detection_config(
        "monitoring-manager",
        DetectionConfig {
            working_poll_interval: Duration::from_millis(100),
            idle_poll_interval: Duration::from_millis(100),
            auto_answer_interstitial_prompts: true,
        },
    )
    .await;
    let mut control = server.connect().await;
    write_frame(
        &mut control,
        &ClientRequest::Attach {
            session: "monitoring-manager".into(),
            hello: Default::default(),
        },
    )
    .await
    .unwrap();
    read_initial_state(&mut control, Duration::from_secs(5)).await;
    write_frame(
        &mut control,
        &ClientRequest::NewPane {
            request_id: 2,
            parent_group: ROOT_ID,
            kind: ilium_ipc::NewPaneKind::Command(executable.path.to_string_lossy().into_owned()),
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .unwrap();
    let event = expect_event(
        &mut control,
        Duration::from_secs(10),
        |event| matches!(event, ServerEvent::TreeSnapshot(tree) if tree.panes().next().is_some()),
    )
    .await;
    let ServerEvent::TreeSnapshot(tree) = event else {
        unreachable!()
    };
    let pane = tree.panes().next().unwrap().id;
    expect_event(&mut control, Duration::from_secs(30), |event| {
        matches!(event,
        ServerEvent::PaneDetectedStateChanged { pane_id, status, .. } if *pane_id == pane
            && status.agent_state().is_some_and(|agent| agent.activity() == AgentActivity::Working))
    })
    .await;
    let initial = read(&server, query(MonitoringView::Events, None)).await;
    let cursor = initial["cursor"].as_str().unwrap().to_owned();
    let mut follow = query(MonitoringView::Events, Some(cursor.clone()));
    follow.follow = true;
    let mut reader = server.connect().await;
    write_frame(
        &mut reader,
        &ClientRequest::MonitoringEvents {
            request_id: 3,
            query: follow,
        },
    )
    .await
    .unwrap();
    std::fs::write(&marker, b"finish").unwrap();
    let event: ServerEvent = tokio::time::timeout(Duration::from_secs(30), read_frame(&mut reader))
        .await
        .unwrap()
        .unwrap();
    let ServerEvent::MonitoringEvents {
        request_id: 3,
        result,
    } = event
    else {
        panic!("unexpected follow reply: {event:?}")
    };
    let changes: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
    assert!(changes["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["kind"] == "done"));
    let digest = read(&server, query(MonitoringView::Digest, Some(cursor))).await;
    assert_eq!(digest["quiet"], false);
    assert!(digest.to_string().contains("--brief"));
    let mut brief = query(MonitoringView::Brief, None);
    brief.pane_id = Some(pane);
    let detail = read(&server, brief).await;
    assert_eq!(detail["pane"], pane.0);
    assert!(detail["last_message"].is_null());
    assert!(detail.get("screen").is_none());
    assert!(detail.to_string().len() < 1000);
    let mut ack = server.connect().await;
    write_frame(
        &mut ack,
        &ClientRequest::MonitoringAck {
            request_id: 4,
            consumer: "scripted-manager".into(),
            cursor: digest["cursor"].as_str().unwrap().into(),
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        read_frame::<ServerEvent, _>(&mut ack).await.unwrap(),
        ServerEvent::MonitoringAck { result: Ok(_), .. }
    ));
    assert_eq!(
        read(&server, query(MonitoringView::Digest, None)).await["quiet"],
        true
    );
    let tokens: ilium_control::access::Tokens = serde_json::from_slice(
        &std::fs::read(server.socket_path.with_extension("ctl-tokens.json")).unwrap(),
    )
    .unwrap();
    let operation = ilium_control::Operation::Rename {
        node: pane.0,
        title: "manager handled".into(),
    };
    write_frame(
        &mut control,
        &ClientRequest::ControlWrite {
            request_id: 5,
            token: tokens.act.clone(),
            caller_pane: None,
            operation: serde_json::to_string(&operation).unwrap(),
        },
    )
    .await
    .unwrap();
    expect_event(&mut control, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::ControlWrite {
                request_id: 5,
                result: Ok(())
            }
        )
    })
    .await;
    write_frame(
        &mut control,
        &ClientRequest::RenameNode {
            node_id: pane,
            title: "manager handled".into(),
            short_title: None,
            inferred_icon: None,
        },
    )
    .await
    .unwrap();
    expect_event(&mut control, Duration::from_secs(5), |event| match event {
        ServerEvent::TreeSnapshot(tree) => tree
            .get(pane)
            .is_some_and(|node| node.name == "manager handled"),
        ServerEvent::PaneNodeChanged(node) => node.id == pane && node.name == "manager handled",
        _ => false,
    })
    .await;
    let audit_path = server
        .snapshot_path
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("logs/monitoring-manager.ctl.jsonl");
    let audit = std::fs::read_to_string(audit_path).unwrap();
    assert!(audit
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .any(|entry| entry["operation"] == "rename" && entry["target"] == pane.0));
    write_frame(
        &mut control,
        &ClientRequest::ControlWrite {
            request_id: 6,
            token: tokens.act,
            caller_pane: None,
            operation: serde_json::to_string(&ilium_control::Operation::Terminate { pane: pane.0 })
                .unwrap(),
        },
    )
    .await
    .unwrap();
    expect_event(&mut control, Duration::from_secs(5), |event| {
        matches!(
            event,
            ServerEvent::ControlWrite {
                request_id: 6,
                result: Err(_)
            }
        )
    })
    .await;
    write_frame(&mut control, &ClientRequest::KillSession)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), &mut server.server_task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
