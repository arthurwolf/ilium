//! Candidate command-surface checks. Runtime ownership/handoff is tested against
//! isolated servers by the shared resource integration suite.
use std::process::Command;

fn candidate() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ilium"))
}

/// The peer refuses attempts explicitly; the CLI must preserve every wire
/// field rather than rebuilding the request or allocating a new mutation key.
async fn busy_then_identical_retry(
    stream: &mut ilium_transport::SessionStream,
    original: ilium_ipc::ClientRequest,
) -> ilium_ipc::ClientRequest {
    use ilium_ipc::{read_frame, write_frame, ClientRequest, ServerEvent};
    let ClientRequest::Resource { request_id, .. } = &original else {
        panic!("expected correlated resource request");
    };
    for _ in 0..2 {
        write_frame(
            stream,
            &ServerEvent::ResourceResult {
                request_id: *request_id,
                result: Err(ilium_ipc::resource::ResourceError {
                    code: "busy".into(),
                    message: "Synthetic catalog lock contention".into(),
                }),
            },
        )
        .await
        .unwrap();
        let retried: ClientRequest =
            tokio::time::timeout(std::time::Duration::from_secs(5), read_frame(stream))
                .await
                .expect("explicit Busy must retry")
                .unwrap();
        assert_eq!(
            serde_json::to_value(&retried).unwrap(),
            serde_json::to_value(&original).unwrap()
        );
    }
    original
}
#[test]
fn resource_help_and_spelling_alias_are_equivalent() {
    let canonical = candidate().args(["resource", "--help"]).output().unwrap();
    let alias = candidate().args(["ressource", "--help"]).output().unwrap();
    assert!(canonical.status.success());
    assert!(alias.status.success());
    let canonical = String::from_utf8(canonical.stdout).unwrap();
    let alias = String::from_utf8(alias.stdout).unwrap();
    assert_eq!(canonical, alias);
    for command in [
        "reserve", "release", "cancel", "wait", "estimate", "create", "link", "edit", "unlink",
        "archive", "recover", "rebind", "repair",
    ] {
        assert!(canonical.contains(command), "Missing {command}");
    }
}
#[test]
fn invalid_reserve_arity_is_a_usage_error() {
    for arguments in [
        vec!["resource", "reserve", "gpu"],
        vec!["resource", "reserve", "a", "b", "c", "d"],
    ] {
        let output = candidate().args(arguments).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}
#[test]
fn invalid_duration_is_structured_before_any_server_access() {
    let output = candidate()
        .args(["resource", "reserve", "gpu", "8d"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let record: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(record["type"], "error");
    assert_eq!(record["code"], "invalid_input");
    assert!(record["request_id"].as_str().unwrap().contains(':'));
}
#[test]
fn missing_server_does_not_launch_an_authority() {
    let directory = tempfile::tempdir().unwrap();
    let output = candidate()
        .args(["resource", "list", "--project"])
        .arg(directory.path())
        .env_remove(ilium_ipc::pane_env::PANE_ID)
        .env_remove(ilium_ipc::pane_env::SESSION_NAME)
        .env_remove(ilium_ipc::pane_env::SESSION_SOCKET)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    let record: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(record["code"], "service_unavailable");
    let session = ilium::session::resolve_project_session(directory.path(), "default").unwrap();
    assert!(!session.socket_path.exists());
}

#[tokio::test]
async fn explicit_cwd_never_routes_resource_commands_to_inherited_pane() {
    use ilium_ipc::{read_frame, write_frame, ClientRequest, ServerEvent};
    let directory = tempfile::tempdir().unwrap();
    let inherited_socket = directory.path().join("inherited.sock");
    let mut listener = ilium_transport::SessionEndpoint::from_path(&inherited_socket)
        .bind()
        .await
        .unwrap();
    let peer = tokio::spawn(async move {
        let mut stream = listener.accept().await.unwrap();
        let _: ClientRequest = read_frame(&mut stream).await.unwrap();
        let ClientRequest::Resource { request_id, .. } = read_frame(&mut stream).await.unwrap()
        else {
            panic!("expected resource request");
        };
        write_frame(
            &mut stream,
            &ServerEvent::ResourceResult {
                request_id,
                result: Err(ilium_ipc::resource::ResourceError {
                    code: "wrong_server".into(),
                    message: "Inherited session was incorrectly selected".into(),
                }),
            },
        )
        .await
        .unwrap();
    });
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_ilium"))
            .arg("--cwd")
            .arg(directory.path())
            .args(["resource", "list"])
            .env(ilium_ipc::pane_env::PANE_ID, "42")
            .env(ilium_ipc::pane_env::SESSION_NAME, "default")
            .env(ilium_ipc::pane_env::SESSION_SOCKET, &inherited_socket)
            .env(
                ilium_platform::runtime_dir::SOCKET_DIR_ENV,
                directory.path(),
            )
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("resource command must terminate")
    .unwrap();
    peer.abort();
    let record: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(record["code"], "service_unavailable");
}

/// This fixture verifies the CLI wire contract only; it is not hardware or
/// cross-process authority qualification.
#[tokio::test]
async fn uncertain_and_stale_errors_are_structured_without_automatic_retry() {
    use ilium_ipc::{read_frame, write_frame, ClientRequest, ServerEvent};
    for (code, expected_exit) in [("outcome_unknown", 4), ("stale_revision", 2)] {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("refusal.sock");
        let mut listener = ilium_transport::SessionEndpoint::from_path(&socket)
            .bind()
            .await
            .unwrap();
        let server = tokio::spawn(async move {
            let mut stream = listener.accept().await.unwrap();
            let attach: ClientRequest = read_frame(&mut stream).await.unwrap();
            assert!(matches!(attach, ClientRequest::AttachInteractive { .. }));
            let request: ClientRequest = read_frame(&mut stream).await.unwrap();
            let ClientRequest::Resource { request_id, .. } = request else {
                panic!("expected snapshot request");
            };
            write_frame(
                &mut stream,
                &ServerEvent::ResourceResult {
                    request_id,
                    result: Err(ilium_ipc::resource::ResourceError {
                        code: code.into(),
                        message: "Synthetic non-retryable result".into(),
                    }),
                },
            )
            .await
            .unwrap();
            if let Ok(Ok(ClientRequest::Resource { .. })) = tokio::time::timeout(
                std::time::Duration::from_secs(1),
                read_frame::<ClientRequest, _>(&mut stream),
            )
            .await
            {
                panic!("{code} must not resubmit a resource request");
            }
        });
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tokio::process::Command::new(env!("CARGO_BIN_EXE_ilium"))
                .current_dir(directory.path())
                .args(["resource", "list", "--request-id", "1000:non-retry"])
                .env(ilium_ipc::pane_env::PANE_ID, "42")
                .env(ilium_ipc::pane_env::SESSION_NAME, "default")
                .env(ilium_ipc::pane_env::SESSION_SOCKET, &socket)
                .kill_on_drop(true)
                .output(),
        )
        .await
        .expect("non-retryable result returns immediately")
        .unwrap();
        server.await.unwrap();
        assert_eq!(output.status.code(), Some(expected_exit));
        let record: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(record["code"], code);
        assert_eq!(record["request_id"], "1000:non-retry");
    }
}

#[tokio::test]
async fn reserve_retries_explicit_busy_and_retains_request_identity_over_real_transport() {
    use ilium_core::{resource_queue::*, NodeId};
    use ilium_ipc::resource::{ResourceOperation, ResourceReply};
    use ilium_ipc::{read_frame, write_frame, ClientRequest, ServerEvent};
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("resource.sock");
    let endpoint = ilium_transport::SessionEndpoint::from_path(&socket);
    let mut listener = endpoint.bind().await.unwrap();
    let path = directory
        .path()
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let clock = ResourceClock {
        unix_millis: 1000,
        monotonic_millis: 1000,
        boot_id: "synthetic-boot".into(),
    };
    let definition = ResourceDefinition {
        id: "synthetic-gpu".into(),
        title: "Synthetic GPU".into(),
        kind: "gpu".into(),
        physical_identity: None,
        description: String::new(),
        rules: String::new(),
        policy: ResourcePolicy::ExclusiveFifo,
        created_at_unix_millis: 0,
        updated_at_unix_millis: 0,
        definition_revision: 1,
        archived: false,
    };
    let mut catalog = ResourceCatalog {
        revision: 4,
        ..ResourceCatalog::default()
    };
    catalog.resources.insert(
        definition.id.clone(),
        ResourceRecord {
            definition,
            gate: ResourceGate::Paused {
                reason: "Synthetic fixture".into(),
            },
            clearance_required: false,
            next_ticket: 2,
            next_generation: 1,
            reservations: vec![],
        },
    );
    catalog.links.push(ProjectResourceLink {
        project_id: "synthetic-project".into(),
        resource_id: "synthetic-gpu".into(),
        slug: "gpu".into(),
        project_path: path,
        created_at_unix_millis: 0,
        desired_instruction_revision: 1,
        sync_debt: None,
    });
    let server = tokio::spawn(async move {
        let mut stream = listener.accept().await.unwrap();
        let attach: ClientRequest = read_frame(&mut stream).await.unwrap();
        assert!(matches!(attach, ClientRequest::AttachInteractive { .. }));
        let request: ClientRequest = read_frame(&mut stream).await.unwrap();
        let request = busy_then_identical_retry(&mut stream, request).await;
        let ClientRequest::Resource {
            request_id,
            request,
        } = request
        else {
            panic!("Expected resource snapshot")
        };
        assert!(matches!(request.operation, ResourceOperation::Snapshot));
        write_frame(
            &mut stream,
            &ServerEvent::ResourceResult {
                request_id,
                result: Ok(Box::new(ResourceReply {
                    local_invocations: Vec::new(),
                    active_waits: Vec::new(),
                    wait_revision: 0,
                    catalog: catalog.clone(),
                    clock: clock.clone(),
                    result: None,
                    reservation: None,
                    recovery_token: None,
                    message: None,
                })),
            },
        )
        .await
        .unwrap();
        let request: ClientRequest = read_frame(&mut stream).await.unwrap();
        let request = busy_then_identical_retry(&mut stream, request).await;
        let ClientRequest::Resource {
            request_id,
            request,
        } = request
        else {
            panic!("Expected resource reserve")
        };
        assert_eq!(request.request_key.as_deref(), Some("1000:cli-retry"));
        assert_eq!(request.pane_id, Some(NodeId(42)));
        let ResourceOperation::Mutate(ResourceCommand::Reserve {
            resource_id,
            reservation_id,
            estimate_seconds,
        }) = request.operation
        else {
            panic!("Expected reserve mutation")
        };
        assert_eq!(estimate_seconds, 90);
        let reservation = Reservation {
            id: reservation_id.clone(),
            request_id: "1000:cli-retry".into(),
            resource_id: resource_id.clone(),
            agent: AgentIdentity {
                project_id: "synthetic-project".into(),
                session_id: "default".into(),
                pane_id: NodeId(42),
                runtime_generation: "synthetic-runtime".into(),
                invocation_id: "synthetic-agent".into(),
                label: "Synthetic agent".into(),
            },
            ticket: 1,
            estimate_seconds,
            requested_at: clock.clone(),
            acquired_at: None,
            terminal_at: None,
            state: ReservationState::Queued,
            ownership_generation: 0,
            acquisition_pending: false,
        };
        catalog
            .resources
            .get_mut(&resource_id)
            .unwrap()
            .reservations
            .push(reservation.clone());
        write_frame(
            &mut stream,
            &ServerEvent::ResourceResult {
                request_id,
                result: Ok(Box::new(ResourceReply {
                    local_invocations: Vec::new(),
                    active_waits: Vec::new(),
                    wait_revision: 0,
                    catalog,
                    clock,
                    result: Some(ResourceCommandResult {
                        resource_id: Some(resource_id),
                        reservation_id: Some(reservation_id),
                        revision: 4,
                        reused: false,
                    }),
                    reservation: Some(reservation),
                    recovery_token: None,
                    message: None,
                })),
            },
        )
        .await
        .unwrap();
    });
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_ilium"))
        .current_dir(directory.path())
        .args([
            "resource",
            "reserve",
            "gpu",
            "1m30s",
            "--request-id",
            "1000:cli-retry",
        ])
        .env(ilium_ipc::pane_env::PANE_ID, "42")
        .env(ilium_ipc::pane_env::SESSION_NAME, "default")
        .env(ilium_ipc::pane_env::SESSION_SOCKET, &socket)
        .output()
        .await
        .unwrap();
    server.await.unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let record: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(record["state"], "queued");
    assert_eq!(record["resource_slug"], "gpu");
    assert_eq!(record["queue_position"], 1);
    assert_eq!(record["request_id"], "1000:cli-retry");
    assert_eq!(record["estimated_wait_seconds"], serde_json::Value::Null);
    assert_eq!(record["ownership_generation"], 0);
}

/// Pane numbers are only local addresses. A shared catalog may contain the
/// same number from another project, runtime, or replaced agent invocation.
#[tokio::test]
async fn bare_status_excludes_foreign_and_replaced_invocations_with_same_pane_number() {
    use ilium_core::{resource_queue::*, NodeId};
    use ilium_ipc::resource::{ResourceOperation, ResourceReply};
    use ilium_ipc::{read_frame, write_frame, ClientRequest, ServerEvent};
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("status.sock");
    let mut listener = ilium_transport::SessionEndpoint::from_path(&socket)
        .bind()
        .await
        .unwrap();
    let local = AgentIdentity {
        project_id: "local-project".into(),
        session_id: "default".into(),
        pane_id: NodeId(42),
        runtime_generation: "local-runtime".into(),
        invocation_id: "current-agent".into(),
        label: "Current local agent".into(),
    };
    let mut identities = vec![local.clone()];
    let mut foreign = local.clone();
    foreign.project_id = "foreign-project".into();
    identities.push(foreign);
    let mut replaced = local.clone();
    replaced.invocation_id = "replaced-agent".into();
    identities.push(replaced);
    let mut previous_runtime = local.clone();
    previous_runtime.runtime_generation = "old-runtime".into();
    identities.push(previous_runtime);
    let clock = ResourceClock {
        unix_millis: 1000,
        monotonic_millis: 1000,
        boot_id: "fixture".into(),
    };
    let reservations = identities
        .into_iter()
        .enumerate()
        .map(|(index, agent)| Reservation {
            id: ReservationId(format!("reservation-{index}")),
            request_id: RequestId(format!("1000:status-{index}")),
            resource_id: "shared-resource".into(),
            agent,
            ticket: index as u64 + 1,
            estimate_seconds: 60,
            requested_at: clock.clone(),
            acquired_at: None,
            terminal_at: None,
            state: ReservationState::Queued,
            ownership_generation: 0,
            acquisition_pending: false,
        })
        .collect();
    let mut catalog = ResourceCatalog::default();
    catalog.resources.insert(
        "shared-resource".into(),
        ResourceRecord {
            definition: ResourceDefinition {
                id: "shared-resource".into(),
                title: "Synthetic resource".into(),
                description: String::new(),
                rules: String::new(),
                kind: "generic".into(),
                physical_identity: None,
                policy: ResourcePolicy::ExclusiveFifo,
                created_at_unix_millis: 0,
                updated_at_unix_millis: 0,
                definition_revision: 1,
                archived: false,
            },
            gate: ResourceGate::Open,
            clearance_required: false,
            next_ticket: 5,
            next_generation: 1,
            reservations,
        },
    );
    let server = tokio::spawn(async move {
        let mut stream = listener.accept().await.unwrap();
        let attach: ClientRequest = read_frame(&mut stream).await.unwrap();
        assert!(matches!(attach, ClientRequest::AttachInteractive { .. }));
        let request: ClientRequest = read_frame(&mut stream).await.unwrap();
        let ClientRequest::Resource {
            request_id,
            request,
        } = request
        else {
            panic!("expected snapshot")
        };
        assert!(matches!(request.operation, ResourceOperation::Snapshot));
        assert_eq!(request.pane_id, Some(NodeId(42)));
        write_frame(
            &mut stream,
            &ServerEvent::ResourceResult {
                request_id,
                result: Ok(Box::new(ResourceReply {
                    local_invocations: vec![local],
                    active_waits: Vec::new(),
                    wait_revision: 0,
                    catalog,
                    clock,
                    result: None,
                    reservation: None,
                    recovery_token: None,
                    message: None,
                })),
            },
        )
        .await
        .unwrap();
    });
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_ilium"))
        .current_dir(directory.path())
        .args(["resource", "status"])
        .env(ilium_ipc::pane_env::PANE_ID, "42")
        .env(ilium_ipc::pane_env::SESSION_NAME, "default")
        .env(ilium_ipc::pane_env::SESSION_SOCKET, &socket)
        .output()
        .await
        .unwrap();
    server.await.unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let reservations = result["reservations"].as_array().unwrap();
    assert_eq!(reservations.len(), 1, "{result}");
    assert_eq!(reservations[0]["id"], "reservation-0");
}

#[test]
fn repair_and_rebind_require_explicit_targets_and_revision() {
    for arguments in [
        vec!["resource", "repair"],
        vec!["resource", "rebind", "project", "/new/path"],
    ] {
        let output = candidate().args(arguments).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

/// Real CLI/transport contract checks; synthetic authority replies establish
/// exit handling, not domain correctness or physical hardware behavior.
#[tokio::test]
async fn blocking_wait_terminal_errors_and_disconnect_are_correlated_jsonl() {
    use ilium_core::{resource_queue::*, NodeId};
    use ilium_ipc::resource::{ResourceError, ResourceOperation, ResourceReply};
    use ilium_ipc::{read_frame, write_frame, ClientRequest, ServerEvent};
    for (case, state, failure, expected_exit) in [
        ("owned", Some(ReservationState::Owned), None, 0),
        ("cancelled", Some(ReservationState::Cancelled), None, 3),
        (
            "revoked",
            Some(ReservationState::Revoked {
                reason: "Synthetic user revocation".into(),
            }),
            None,
            3,
        ),
        ("wrong_actor", None, Some("actor"), 2),
        ("missing", None, Some("reservation_not_found"), 5),
        ("disconnect", None, None, 4),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("wait.sock");
        let mut listener = ilium_transport::SessionEndpoint::from_path(&socket)
            .bind()
            .await
            .unwrap();
        let clock = ResourceClock {
            unix_millis: 1000,
            monotonic_millis: 1000,
            boot_id: "synthetic".into(),
        };
        let claim = Reservation {
            id: "existing-claim".into(),
            request_id: "1000:original-reserve".into(),
            resource_id: "synthetic-resource".into(),
            agent: AgentIdentity {
                project_id: "project".into(),
                session_id: "default".into(),
                pane_id: NodeId(42),
                runtime_generation: "runtime".into(),
                invocation_id: "invocation".into(),
                label: "Synthetic agent".into(),
            },
            ticket: 1,
            estimate_seconds: 1,
            requested_at: clock.clone(),
            acquired_at: None,
            terminal_at: None,
            state: ReservationState::Queued,
            ownership_generation: 0,
            acquisition_pending: false,
        };
        let reply = ResourceReply {
            local_invocations: vec![claim.agent.clone()],
            active_waits: Vec::new(),
            wait_revision: 0,
            catalog: ResourceCatalog::default(),
            clock,
            result: None,
            reservation: Some(claim),
            recovery_token: None,
            message: None,
        };
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let mut stream = listener.accept().await.unwrap();
            let attach: ClientRequest = read_frame(&mut stream).await.unwrap();
            assert!(matches!(attach, ClientRequest::AttachInteractive { .. }));
            let snapshot: ClientRequest = read_frame(&mut stream).await.unwrap();
            let ClientRequest::Resource {
                request_id,
                request,
            } = snapshot
            else {
                panic!("snapshot required")
            };
            assert!(matches!(request.operation, ResourceOperation::Snapshot));
            write_frame(
                &mut stream,
                &ServerEvent::ResourceResult {
                    request_id,
                    result: Ok(Box::new(reply.clone())),
                },
            )
            .await
            .unwrap();
            let wait: ClientRequest = read_frame(&mut stream).await.unwrap();
            let ClientRequest::Resource {
                request_id,
                request,
            } = wait
            else {
                panic!("wait required")
            };
            assert_eq!(request.pane_id, Some(NodeId(42)));
            assert!(
                matches!(request.operation, ResourceOperation::Wait { reservation_id } if reservation_id.0 == "existing-claim")
            );
            ready_tx.send(()).unwrap();
            release_rx.await.unwrap();
            if let Some(code) = failure {
                write_frame(
                    &mut stream,
                    &ServerEvent::ResourceResult {
                        request_id,
                        result: Err(ResourceError {
                            code: code.into(),
                            message: "Synthetic terminal failure".into(),
                        }),
                    },
                )
                .await
                .unwrap();
            } else if let Some(state) = state {
                let mut reply = reply;
                let claim = reply.reservation.as_mut().unwrap();
                claim.state = state;
                claim.ownership_generation = 7;
                write_frame(
                    &mut stream,
                    &ServerEvent::ResourceResult {
                        request_id,
                        result: Ok(Box::new(reply)),
                    },
                )
                .await
                .unwrap();
            }
            // Otherwise close the authority stream without an acknowledgement.
        });
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_ilium"))
            .current_dir(directory.path())
            .args([
                "resource",
                "wait",
                "existing-claim",
                "--request-id",
                "1000:wait-test",
            ])
            .env(ilium_ipc::pane_env::PANE_ID, "42")
            .env(ilium_ipc::pane_env::SESSION_NAME, "default")
            .env(ilium_ipc::pane_env::SESSION_SOCKET, &socket)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(20), ready_rx)
            .await
            .unwrap()
            .unwrap();
        assert!(
            child.try_wait().unwrap().is_none(),
            "{case}: wait must stay blocked until a correlated authoritative result"
        );
        release_tx.send(()).unwrap();
        let output =
            tokio::time::timeout(std::time::Duration::from_secs(20), child.wait_with_output())
                .await
                .unwrap()
                .unwrap();
        server.await.unwrap();
        assert_eq!(
            output.status.code(),
            Some(expected_exit),
            "{case}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        let lines: Vec<_> = stdout.lines().collect();
        assert_eq!(lines.len(), 1, "{case}: {stdout}");
        let record: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(record["request_id"], "1000:wait-test");
        assert_eq!(record["operation"], "resource_wait");
        if expected_exit == 0 {
            assert_eq!(record["ownership_generation"], 7);
        }
        if case == "disconnect" {
            assert_eq!(record["code"], "outcome_unknown");
        }
    }
}
