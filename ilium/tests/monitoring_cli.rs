//! Monitoring reads use a direct, isolated connection and do not request a tree snapshot.

#[tokio::test]
async fn digest_selects_the_priority_view_without_attaching_or_fetching_screens() {
    use ilium_ipc::{read_frame, write_frame, ClientRequest, MonitoringView, ServerEvent};
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("digest.sock");
    let mut listener = ilium_transport::SessionEndpoint::from_path(&socket)
        .bind()
        .await
        .unwrap();
    let server = tokio::spawn(async move {
        let mut stream = listener.accept().await.unwrap();
        let request: ClientRequest = read_frame(&mut stream).await.unwrap();
        let ClientRequest::MonitoringEvents { request_id, query } = request else {
            panic!("digest must use a direct monitoring read");
        };
        assert_eq!(query.view, MonitoringView::Digest);
        assert_eq!(query.consumer.as_deref(), Some("fixture-manager"));
        write_frame(
            &mut stream,
            &ServerEvent::MonitoringEvents {
                request_id,
                result: Ok(
                    "finished 7 fixture result -> ilium ctl pane 7 --brief\nworking=0 cursor=e:4\n"
                        .into(),
                ),
            },
        )
        .await
        .unwrap();
    });
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_ilium"))
            .args(["ctl", "digest", "--as", "fixture-manager"])
            .env(ilium_ipc::pane_env::PANE_ID, "42")
            .env(ilium_ipc::pane_env::SESSION_NAME, "default")
            .env(ilium_ipc::pane_env::SESSION_SOCKET, &socket)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    server.await.unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains("fixture result"));
}

#[tokio::test]
async fn events_requests_a_bounded_page_without_an_attach_snapshot() {
    use ilium_ipc::{read_frame, write_frame, ClientRequest, MonitoringFormat, ServerEvent};
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("monitoring.sock");
    let mut listener = ilium_transport::SessionEndpoint::from_path(&socket)
        .bind()
        .await
        .unwrap();
    let server = tokio::spawn(async move {
        let mut stream = listener.accept().await.unwrap();
        let request: ClientRequest = read_frame(&mut stream).await.unwrap();
        let ClientRequest::MonitoringEvents { request_id, query } = request else {
            panic!("event reads must not attach or request a full tree");
        };
        assert_eq!(query.format, MonitoringFormat::Compact);
        assert_eq!((query.max_items, query.max_chars), (12, 4000));
        write_frame(
            &mut stream,
            &ServerEvent::MonitoringEvents {
                request_id,
                result: Ok("quiet cursor=epoch:0\n".into()),
            },
        )
        .await
        .unwrap();
    });
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_ilium"))
            .args(["ctl", "events"])
            .env(ilium_ipc::pane_env::PANE_ID, "42")
            .env(ilium_ipc::pane_env::SESSION_NAME, "default")
            .env(ilium_ipc::pane_env::SESSION_SOCKET, &socket)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    server.await.unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"quiet cursor=epoch:0\n");
}

#[tokio::test]
async fn ack_sends_the_named_consumer_and_cursor_without_attaching() {
    use ilium_ipc::{read_frame, write_frame, ClientRequest, ServerEvent};
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("monitoring-ack.sock");
    let mut listener = ilium_transport::SessionEndpoint::from_path(&socket)
        .bind()
        .await
        .unwrap();
    let server = tokio::spawn(async move {
        let mut stream = listener.accept().await.unwrap();
        let request: ClientRequest = read_frame(&mut stream).await.unwrap();
        let ClientRequest::MonitoringAck {
            request_id,
            consumer,
            cursor,
        } = request
        else {
            panic!("acknowledgements must not attach");
        };
        assert_eq!(consumer, "manager");
        assert_eq!(cursor, "epoch:4");
        write_frame(
            &mut stream,
            &ServerEvent::MonitoringAck {
                request_id,
                result: Ok(cursor),
            },
        )
        .await
        .unwrap();
    });
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_ilium"))
            .args(["ctl", "ack", "epoch:4", "--as", "manager"])
            .env(ilium_ipc::pane_env::PANE_ID, "42")
            .env(ilium_ipc::pane_env::SESSION_NAME, "default")
            .env(ilium_ipc::pane_env::SESSION_SOCKET, &socket)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    server.await.unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("epoch:4"));
}

#[tokio::test]
async fn wait_selection_blocks_on_the_attached_event_stream_without_reconnecting() {
    use ilium_core::{AgentActivity, AgentClass, PaneContentKind, PaneStatus, Tree};
    use ilium_ipc::{read_frame, write_frame, ClientRequest, ServerEvent};
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("wait.sock");
    let mut listener = ilium_transport::SessionEndpoint::from_path(&socket)
        .bind()
        .await
        .unwrap();
    let mut tree = Tree::new();
    let project = tree.add_project("/fixture/wait".into()).unwrap();
    let first = tree
        .add_pane(project, "first", PaneContentKind::Terminal)
        .unwrap();
    let second = tree
        .add_pane(project, "second", PaneContentKind::Terminal)
        .unwrap();
    for pane in [first, second] {
        tree.set_pane_status(
            pane,
            PaneStatus::from_activity(AgentClass::Codex, AgentActivity::Working, None),
        )
        .unwrap();
    }
    let server = tokio::spawn(async move {
        let mut stream = listener.accept().await.unwrap();
        let request: ClientRequest = read_frame(&mut stream).await.unwrap();
        assert!(matches!(
            request,
            ClientRequest::Attach { .. } | ClientRequest::AttachInteractive { .. }
        ));
        write_frame(&mut stream, &ServerEvent::TreeSnapshot(tree))
            .await
            .unwrap();
        write_frame(&mut stream, &ServerEvent::InitialStateSyncComplete)
            .await
            .unwrap();
        // Wait longer than the former one-second reattachment interval. A second
        // connection is a regression, even if it later returns the right state.
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(1200), listener.accept())
                .await
                .is_err()
        );
        for pane_id in [first, second] {
            write_frame(
                &mut stream,
                &ServerEvent::PaneStatusChanged {
                    pane_id,
                    status: PaneStatus::from_activity(AgentClass::Codex, AgentActivity::Done, None),
                },
            )
            .await
            .unwrap();
        }
        // The owning connection may close before its queued detach is flushed.
        match read_frame::<ClientRequest, _>(&mut stream).await {
            Ok(ClientRequest::Detach) => {}
            Err(ilium_ipc::IpcError::Io(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof => {}
            other => panic!("unexpected request after completed wait: {other:?}"),
        }
    });
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_ilium"))
            .args([
                "ctl",
                "wait",
                &first.0.to_string(),
                &second.0.to_string(),
                "--all",
                "--state",
                "done",
            ])
            .env(ilium_ipc::pane_env::PANE_ID, "42")
            .env(ilium_ipc::pane_env::SESSION_NAME, "default")
            .env(ilium_ipc::pane_env::SESSION_SOCKET, &socket)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    server.await.unwrap();
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["ready"], true);
    assert_eq!(result["panes"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn pane_brief_uses_a_direct_bounded_read() {
    use ilium_ipc::{read_frame, write_frame, ClientRequest, ServerEvent};
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("brief.sock");
    let mut listener = ilium_transport::SessionEndpoint::from_path(&socket)
        .bind()
        .await
        .unwrap();
    let server = tokio::spawn(async move {
        let mut stream = listener.accept().await.unwrap();
        let request: ClientRequest = read_frame(&mut stream).await.unwrap();
        let ClientRequest::MonitoringEvents { request_id, query } = request else {
            panic!("brief must not attach or fetch screens")
        };
        assert_eq!(format!("{:?}", query.view), "Brief");
        assert_eq!(query.pane_id, Some(ilium_core::NodeId(7)));
        assert!(!query.follow);
        write_frame(&mut stream, &ServerEvent::MonitoringEvents {
            request_id, result: Ok("{\"pane\":7,\"state\":\"done\",\"last_message\":\"Fixture passed\",\"state_age_seconds\":2}\n".into()),
        }).await.unwrap();
    });
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_ilium"))
            .args(["ctl", "pane", "7", "--brief"])
            .env(ilium_ipc::pane_env::PANE_ID, "42")
            .env(ilium_ipc::pane_env::SESSION_NAME, "default")
            .env(ilium_ipc::pane_env::SESSION_SOCKET, &socket)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    server.await.unwrap();
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["last_message"], "Fixture passed");
    assert!(result.get("screen").is_none());
}

#[tokio::test]
async fn all_session_event_reads_keep_independent_cursors_and_identify_each_session() {
    use ilium_ipc::{read_frame, write_frame, ClientRequest, ServerEvent};
    let directory = tempfile::tempdir().unwrap();
    let mut workers = Vec::new();
    let mut cursors = serde_json::Map::new();
    for (name, epoch) in [("one", "epoch-one"), ("two", "epoch-two")] {
        let socket = directory
            .path()
            .join(format!("fixture-123456abcdef-{name}.sock"));
        cursors.insert(
            socket.to_string_lossy().into_owned(),
            serde_json::json!(format!("{epoch}:4")),
        );
        let mut listener = ilium_transport::SessionEndpoint::from_path(&socket)
            .bind()
            .await
            .unwrap();
        workers.push(tokio::spawn(async move {
            loop {
                let mut stream = listener.accept().await.unwrap();
                // Discovery's liveness connection deliberately sends no frame.
                let Ok(request) = read_frame::<ClientRequest, _>(&mut stream).await else {
                    continue;
                };
                let ClientRequest::MonitoringEvents { request_id, query } = request else {
                    panic!("all-session events must not attach")
                };
                assert_eq!(query.since.as_deref(), Some(format!("{epoch}:4").as_str()));
                assert_eq!(query.consumer.as_deref(), Some("fixture-manager"));
                write_frame(
                    &mut stream,
                    &ServerEvent::MonitoringEvents {
                        request_id,
                        result: Ok(
                            serde_json::json!({"events":[],"cursor":format!("{epoch}:5"),"more":0})
                                .to_string()
                                + "\n",
                        ),
                    },
                )
                .await
                .unwrap();
                break;
            }
        }));
    }
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_ilium"))
            .args([
                "ctl",
                "events",
                "--all",
                "--as",
                "fixture-manager",
                "--since",
                &serde_json::Value::Object(cursors).to_string(),
                "--format",
                "json",
            ])
            .env(
                ilium_platform::runtime_dir::SOCKET_DIR_ENV,
                directory.path(),
            )
            .env_remove(ilium_ipc::pane_env::PANE_ID)
            .env_remove(ilium_ipc::pane_env::SESSION_SOCKET)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for worker in workers {
        worker.await.unwrap();
    }
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let sessions = result["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 2);
    for session in sessions {
        assert!(session["socket"].as_str().unwrap().ends_with(".sock"));
        assert!(session["response"]["cursor"]
            .as_str()
            .unwrap()
            .contains(session["session"].as_str().unwrap()));
    }
    assert!(output.stdout.len() <= 4000);
}

#[test]
fn blanket_write_targets_are_rejected_before_any_connection() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ilium"))
        .args(["ctl", "--all-sessions", "send", "7", "fixture"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let output = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    assert!(
        output.contains("explicit session") || output.contains("read commands"),
        "{output}"
    );
}

#[test]
fn outside_write_defaults_to_read_before_connecting() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ilium"))
        .args(["ctl", "rename", "7", "fixture"])
        .env_remove(ilium_ipc::pane_env::PANE_ID)
        .env_remove(ilium_ipc::pane_env::SESSION_SOCKET)
        .env_remove("ILIUM_CONTROL_TOKEN")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("default to read"));
}

#[tokio::test]
async fn command_write_does_not_dispatch_when_durable_audit_is_refused() {
    for supplied_token in [false, true] {
        use ilium_ipc::{read_frame, write_frame, ClientRequest, ServerEvent};
        use std::io::Write;
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("audit.sock");
        let tokens = ilium_control::access::Tokens {
            read: "fixture-read-token-for-command-12345".into(),
            act: "fixture-act-token-for-command-123456".into(),
            admin: "fixture-admin-token-for-command-1234".into(),
        };
        if !supplied_token {
            let mut file = ilium_platform::secure_fs::private_open_options()
                .write(true)
                .create_new(true)
                .open(socket.with_extension("ctl-tokens.json"))
                .unwrap();
            file.write_all(serde_json::to_string(&tokens).unwrap().as_bytes())
                .unwrap();
        }
        let supplied = supplied_token.then(|| ("ILIUM_CONTROL_TOKEN", tokens.act.clone()));
        let mut tree = ilium_core::Tree::new();
        let project = tree.add_project("/fixture/audit".into()).unwrap();
        let pane = tree
            .add_pane(project, "fixture", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        let mut listener = ilium_transport::SessionEndpoint::from_path(&socket)
            .bind()
            .await
            .unwrap();
        let server = tokio::spawn(async move {
            let mut stream = listener.accept().await.unwrap();
            let request: ClientRequest = read_frame(&mut stream).await.unwrap();
            assert!(matches!(
                request,
                ClientRequest::Attach { .. } | ClientRequest::AttachInteractive { .. }
            ));
            write_frame(&mut stream, &ServerEvent::TreeSnapshot(tree))
                .await
                .unwrap();
            write_frame(&mut stream, &ServerEvent::InitialStateSyncComplete)
                .await
                .unwrap();
            let ClientRequest::ControlWrite {
                request_id,
                token,
                caller_pane,
                operation,
            } = read_frame(&mut stream).await.unwrap()
            else {
                panic!("write must first authorize and audit")
            };
            assert_eq!(token, tokens.act);
            assert_eq!(caller_pane, Some(ilium_core::NodeId(42)));
            assert!(operation.contains("rename"));
            write_frame(
                &mut stream,
                &ServerEvent::ControlWrite {
                    request_id,
                    result: Err("fixture audit unavailable".into()),
                },
            )
            .await
            .unwrap();
            match read_frame::<ClientRequest, _>(&mut stream).await {
                Ok(ClientRequest::Detach) => {}
                Err(ilium_ipc::IpcError::Io(error))
                    if error.kind() == std::io::ErrorKind::UnexpectedEof => {}
                other => panic!("mutation dispatched despite audit refusal: {other:?}"),
            }
        });
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            tokio::process::Command::new(env!("CARGO_BIN_EXE_ilium"))
                .args(["ctl", "rename", &pane.0.to_string(), "changed"])
                .env(ilium_ipc::pane_env::PANE_ID, "42")
                .env(ilium_ipc::pane_env::SESSION_NAME, "default")
                .env(ilium_ipc::pane_env::SESSION_SOCKET, &socket)
                .env_remove("ILIUM_CONTROL_TOKEN")
                .envs(supplied)
                .kill_on_drop(true)
                .output(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("fixture audit unavailable"));
        server.await.unwrap();
    }
}

// The real CLI must retain partial failures and must not let a quiet session
// hide a change delivered by a different session.
#[tokio::test]
async fn all_session_follow_returns_an_active_session_and_identifies_failures() {
    use ilium_ipc::{read_frame, write_frame, ClientRequest, ServerEvent};
    let directory = tempfile::tempdir().unwrap();
    let mut workers = Vec::new();
    for name in ["active", "broken", "quiet"] {
        let socket = directory
            .path()
            .join(format!("fixture-123456abcdef-{name}.sock"));
        let mut listener = ilium_transport::SessionEndpoint::from_path(&socket)
            .bind()
            .await
            .unwrap();
        workers.push(tokio::spawn(async move {
            loop {
                let mut stream = listener.accept().await.unwrap();
                let Ok(ClientRequest::MonitoringEvents { request_id, query }) = read_frame::<ClientRequest, _>(&mut stream).await else { continue };
                assert!(query.follow);
                match name {
                    "quiet" => { let _ = read_frame::<ClientRequest, _>(&mut stream).await; },
                    "broken" => { write_frame(&mut stream, &ServerEvent::MonitoringEvents { request_id, result: Err("fixture observation unavailable".into()) }).await.unwrap(); },
                    _ => {
                        // Give the explicit failure a chance to arrive first.
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        write_frame(&mut stream, &ServerEvent::MonitoringEvents { request_id,
                            result: Ok(serde_json::json!({"events":[{"pane_id":7,"kind":"done","summary":"fixture finished"}],"cursor":"active:2","more":0}).to_string()) }).await.unwrap();
                    }
                }
                break;
            }
        }));
    }
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_ilium"))
            .args(["ctl", "events", "--all", "--follow", "--format", "json"])
            .env(
                ilium_platform::runtime_dir::SOCKET_DIR_ENV,
                directory.path(),
            )
            .env_remove(ilium_ipc::pane_env::PANE_ID)
            .env_remove(ilium_ipc::pane_env::SESSION_SOCKET)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for worker in workers {
        worker.await.unwrap();
    }
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let sessions = response["sessions"].as_array().unwrap();
    assert!(sessions
        .iter()
        .any(|session| session["response"]["cursor"] == "active:2"));
    assert!(sessions.iter().any(|session| session["error"]
        .as_str()
        .is_some_and(|error| error.contains("fixture observation unavailable"))
        && session["socket"].as_str().unwrap().contains("broken")));
    assert!(response["more_sessions"].as_u64().unwrap() >= 1);
}

#[tokio::test]
async fn all_session_tree_find_and_wait_keep_overlapping_pane_ids_scoped() {
    use ilium_ipc::{read_frame, write_frame, ClientRequest, ServerEvent};
    for command in [
        vec!["tree", "--all"],
        vec!["find", "--all"],
        vec!["--all-sessions", "wait", "2", "--all", "--state", "closed"],
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut workers = Vec::new();
        for name in ["one", "two"] {
            let socket = directory
                .path()
                .join(format!("fixture-123456abcdef-{name}.sock"));
            let mut tree = ilium_core::Tree::new();
            let project = tree.add_project(format!("/fixture/{name}").into()).unwrap();
            tree.add_pane(project, "fixture", ilium_core::PaneContentKind::Terminal)
                .unwrap();
            let mut listener = ilium_transport::SessionEndpoint::from_path(&socket)
                .bind()
                .await
                .unwrap();
            workers.push(tokio::spawn(async move {
                loop {
                    let mut stream = listener.accept().await.unwrap();
                    let Ok(request) = read_frame::<ClientRequest, _>(&mut stream).await else {
                        continue;
                    };
                    assert!(matches!(
                        request,
                        ClientRequest::Attach { .. } | ClientRequest::AttachInteractive { .. }
                    ));
                    write_frame(&mut stream, &ServerEvent::TreeSnapshot(tree.clone()))
                        .await
                        .unwrap();
                    write_frame(&mut stream, &ServerEvent::InitialStateSyncComplete)
                        .await
                        .unwrap();
                    tree.remove_node(ilium_core::NodeId(2)).unwrap();
                    write_frame(&mut stream, &ServerEvent::TreeSnapshot(tree))
                        .await
                        .unwrap();
                    let _ = read_frame::<ClientRequest, _>(&mut stream).await;
                    break;
                }
            }));
        }
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            tokio::process::Command::new(env!("CARGO_BIN_EXE_ilium"))
                .arg("ctl")
                .args(&command)
                .env(
                    ilium_platform::runtime_dir::SOCKET_DIR_ENV,
                    directory.path(),
                )
                .env_remove(ilium_ipc::pane_env::PANE_ID)
                .env_remove(ilium_ipc::pane_env::SESSION_SOCKET)
                .kill_on_drop(true)
                .output(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(
            output.status.success(),
            "command {command:?}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        for mut worker in workers {
            let result = tokio::time::timeout(std::time::Duration::from_secs(2), &mut worker).await;
            if result.is_err() {
                worker.abort();
            }
            assert!(
                result.is_ok_and(|joined| joined.is_ok()),
                "fixture did not finish for command {command:?}: stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let sessions = response["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(sessions
            .iter()
            .all(|session| session["response"].is_object() || session["response"].is_array()));
        assert_ne!(sessions[0]["socket"], sessions[1]["socket"]);
    }
}

#[tokio::test]
async fn all_session_budget_never_advances_an_omitted_sessions_cursor() {
    use ilium_ipc::{read_frame, write_frame, ClientRequest, ServerEvent};
    let directory = tempfile::tempdir().unwrap();
    let mut workers = Vec::new();
    let mut cursors = serde_json::Map::new();
    for name in ["one", "two"] {
        let socket = directory
            .path()
            .join(format!("fixture-123456abcdef-{name}.sock"));
        cursors.insert(
            socket.to_string_lossy().into_owned(),
            serde_json::json!(format!("{name}:1")),
        );
        let mut listener = ilium_transport::SessionEndpoint::from_path(&socket)
            .bind()
            .await
            .unwrap();
        workers.push(tokio::spawn(async move {
            loop {
                let mut stream = listener.accept().await.unwrap();
                let Ok(ClientRequest::MonitoringEvents { request_id, .. }) = read_frame::<ClientRequest, _>(&mut stream).await else { continue };
                write_frame(&mut stream, &ServerEvent::MonitoringEvents { request_id,
                    result: Ok(serde_json::json!({"events":[{"pane_id":7,"kind":"done","summary":"fixture done"}],"cursor":format!("{name}:2"),"more":0}).to_string()) }).await.unwrap();
                break;
            }
        }));
    }
    let original = cursors.clone();
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_ilium"))
            .args([
                "ctl",
                "events",
                "--all",
                "--since",
                &serde_json::Value::Object(cursors).to_string(),
                "--max-items",
                "1",
                "--max-chars",
                "1800",
                "--format",
                "json",
            ])
            .env(
                ilium_platform::runtime_dir::SOCKET_DIR_ENV,
                directory.path(),
            )
            .env_remove(ilium_ipc::pane_env::PANE_ID)
            .env_remove(ilium_ipc::pane_env::SESSION_SOCKET)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for worker in workers {
        worker.await.unwrap();
    }
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(response["more_sessions"], 1);
    let shown = response["sessions"][0]["socket"].as_str().unwrap();
    for (socket, cursor) in original {
        if socket != shown {
            assert_eq!(response["cursor"][&socket], cursor);
        }
    }
    assert!(output.stdout.len() <= 1800);
}
