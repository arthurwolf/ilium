//! Real isolated servers and fake provider PTYs; no real device or user catalog.
#![cfg(target_os = "linux")]
use ilium_core::{resource_queue::*, AgentClass, NodeId, ROOT_ID};
use ilium_ipc::{
    read_frame, resource::*, write_frame, ClientRequest, NewPaneKind, NewPaneWorkingDirectory,
    ServerEvent,
};
use ilium_server::{
    config::{DetectionConfig, HttpApiConfig, NotificationsConfig, SessionRecoveryConfig},
    NoopSoundPlayer, ServerOptions,
};
use ilium_test_fixtures::{install, FixtureBehavior};
use ilium_transport::{SessionEndpoint, SessionStream};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
mod common;
const TIMEOUT: Duration = Duration::from_secs(180);
static RUNTIME_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
struct Server {
    socket: PathBuf,
    project: PathBuf,
    session: String,
    task: tokio::task::JoinHandle<Result<(), ilium_server::error::ServerError>>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn start(root: &Path, name: &str, catalog: &Path) -> Self {
        let project = root.join(name);
        std::fs::create_dir_all(&project).unwrap();
        let project = project.canonicalize().unwrap();
        let socket = root.join(format!("{name}.sock"));
        let options = ServerOptions {
            resource_catalog_path: Some(catalog.to_path_buf()),
            session_name: name.into(),
            socket_path: socket.clone(),
            snapshot_path: project.join("snapshot.json"),
            ready_log_metadata: None,
            session_cwd: project.clone(),
            home_dir: project.clone(),
            detection_config: DetectionConfig {
                working_poll_interval: Duration::from_secs(1),
                idle_poll_interval: Duration::from_secs(1),
                ..Default::default()
            },
            notifications_config: NotificationsConfig {
                enabled: false,
                ..Default::default()
            },
            sound_settings: Default::default(),
            sound_config_path: None,
            sound_player: Arc::new(NoopSoundPlayer),
            custom_signatures: Vec::new(),
            session_recovery: SessionRecoveryConfig::RestoreAutomatically,
            session_backups_enabled: false,
            agent_debug_menu_enabled: false,
            http_api: HttpApiConfig { port: 0 },
            progress_monitor_enabled: true,
        };
        let task = tokio::spawn(ilium_server::run(options));
        let server = Self {
            socket,
            project,
            session: name.into(),
            task,
        };
        assert!(common::wait_until(|| server.socket.exists(), Duration::from_secs(10)).await);
        server
    }
    async fn attach(&self) -> (SessionStream, NodeId) {
        let mut stream = SessionEndpoint::from_path(&self.socket)
            .connect()
            .await
            .unwrap();
        write_frame(
            &mut stream,
            &ClientRequest::Attach {
                session: self.session.clone(),
                hello: Default::default(),
            },
        )
        .await
        .unwrap();
        let (tree, _) = common::read_initial_state(&mut stream, TIMEOUT).await;
        (stream, tree.project_ids()[0])
    }
}
fn durable_directory() -> tempfile::TempDir {
    let root = std::env::var_os("ILIUM_RESOURCE_TEST_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/data/ilium-resource-queue-tests"));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join(".scratch-reaper"), b"").unwrap();
    tempfile::tempdir_in(root).unwrap()
}
fn request(project: NodeId, pane: Option<NodeId>, operation: ResourceOperation) -> ResourceRequest {
    let mut request = ResourceRequest::new(Some(project), operation);
    request.pane_id = pane;
    let now = ilium_platform::resource_fs::resource_clock().unwrap();
    request.request_key = Some(format!("{}:{}", now.unix_millis, uuid::Uuid::new_v4()));
    request
}
async fn rpc(
    stream: &mut SessionStream,
    id: u64,
    request: ResourceRequest,
) -> Result<ResourceReply, ResourceError> {
    tokio::time::timeout(TIMEOUT, async {
        let mut reported_busy = false;
        loop {
            write_frame(
                stream,
                &ClientRequest::Resource {
                    request_id: id,
                    request: request.clone(),
                },
            )
            .await
            .unwrap();
            let result = loop {
                let event: ServerEvent = read_frame(stream).await.unwrap();
                if let ServerEvent::ResourceResult { request_id, result } = event {
                    if request_id == id {
                        break result;
                    }
                }
            };
            if matches!(&result,Err(error) if error.code=="busy") {
                if !reported_busy {
                    eprintln!(
                        "Resource RPC {id} {:?} first Busy: {result:?}",
                        std::mem::discriminant(&request.operation)
                    );
                    reported_busy = true;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
                continue;
            }
            return result.map(|reply| *reply);
        }
    })
    .await
    .expect("resource RPC finished")
}

fn resource_definition() -> ResourceDefinition {
    ResourceDefinition {
        policy: ResourcePolicy::ExclusiveFifo,
        created_at_unix_millis: 0,
        updated_at_unix_millis: 0,
        id: "runtime-gpu".into(),
        title: "Synthetic runtime GPU".into(),
        kind: "gpu".into(),
        physical_identity: Some("test-device:runtime-gpu".into()),
        description: "No physical GPU operations".into(),
        rules: "Explicit release only".into(),
        definition_revision: 0,
        archived: false,
    }
}
fn link() -> ProjectResourceLink {
    ProjectResourceLink {
        created_at_unix_millis: 0,
        project_id: "server-resolves".into(),
        resource_id: "runtime-gpu".into(),
        slug: "gpu".into(),
        project_path: "/server-resolves".into(),
        desired_instruction_revision: 0,
        sync_debt: None,
    }
}
async fn expected_event(
    stream: &mut SessionStream,
    label: &str,
    predicate: impl Fn(&ServerEvent) -> bool,
) -> ServerEvent {
    let mut observed = std::collections::VecDeque::new();
    tokio::time::timeout(TIMEOUT, async {
        loop {
            let event = read_frame::<ServerEvent, _>(stream).await.unwrap();
            if observed.len() == 8 {
                observed.pop_front();
            }
            observed.push_back(format!("{:?}", std::mem::discriminant(&event)));
            if predicate(&event) {
                return event;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("resource runtime event {label}; recent kinds: {observed:?}"))
}

// Observe the wait connection itself: authentication/catalog admission may
// return Busy before any waiting badge is published to the observer.
async fn register_wait(stream: &mut SessionStream, request_id: u64, request: ResourceRequest) {
    let pane = request.pane_id.unwrap();
    let ResourceOperation::Wait { reservation_id } = &request.operation else {
        panic!("wait registration requires a Wait request");
    };
    tokio::time::timeout(TIMEOUT, async {
        loop {
            write_frame(
                stream,
                &ClientRequest::Resource {
                    request_id,
                    request: request.clone(),
                },
            )
            .await
            .unwrap();
            loop {
                match read_frame::<ServerEvent, _>(stream).await.unwrap() {
                    ServerEvent::ResourceWaitChanged {
                        pane_id,
                        reservation_id: Some(id),
                        ..
                    } if pane_id == pane && id == *reservation_id => return,
                    ServerEvent::ResourceResult {
                        request_id: response_id,
                        result,
                    } if response_id == request_id => match result {
                        Err(error) if error.code == "busy" => {
                            tokio::time::sleep(Duration::from_millis(10)).await;
                            break;
                        }
                        result => panic!("queued wait ended before registration: {result:?}"),
                    },
                    _ => {}
                }
            }
        }
    })
    .await
    .expect("queued wait must register before its owner changes");
}

async fn spawn_agent(stream: &mut SessionStream, server: &Server) -> NodeId {
    std::fs::create_dir_all(server.project.join("fake-bin")).unwrap();
    let fixture = install(
        &server.project.join("fake-bin"),
        "codex",
        &FixtureBehavior::WorkingUntilMarker {
            marker_path: server.project.join("finish-agent"),
        },
    );
    write_frame(
        stream,
        &ClientRequest::NewPane {
            request_id: 400,
            parent_group: ROOT_ID,
            kind: NewPaneKind::Command(fixture.path.to_string_lossy().into_owned()),
            working_directory: NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .unwrap();
    let event = expected_event(stream, "new Codex agent detection", |event| {
        matches!(event,
            ServerEvent::PaneDetectedStateChanged { status, .. }
            if status.agent_state().is_some_and(|state| state.class == AgentClass::Codex)
        )
    })
    .await;
    let ServerEvent::PaneDetectedStateChanged { pane_id, .. } = event else {
        unreachable!()
    };
    pane_id
}
async fn reserve(
    stream: &mut SessionStream,
    project: NodeId,
    pane: NodeId,
    id: &str,
    request_id: u64,
) -> ResourceReply {
    rpc(
        stream,
        request_id,
        request(
            project,
            Some(pane),
            ResourceOperation::Mutate(ResourceCommand::Reserve {
                resource_id: "runtime-gpu".into(),
                reservation_id: id.into(),
                estimate_seconds: 600,
            }),
        ),
    )
    .await
    .unwrap()
}
async fn readback(catalog: &Path) -> ResourceCatalog {
    let path = catalog.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut admitted = None;
        let deadline = std::time::Instant::now() + Duration::from_secs(600);
        while std::time::Instant::now() < deadline {
            admitted = ilium_platform::resource_fs::ResourceLedgerFile::try_lock(&path).unwrap();
            if admitted.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let ledger = admitted.expect("authoritative ledger lock admission");
        let bytes = ledger.read_reconciled(32 * 1024 * 1024).unwrap().unwrap();
        let catalog: ResourceCatalog = serde_json::from_slice(&bytes).unwrap();
        validate_resource_catalog(&catalog).unwrap();
        catalog
    })
    .await
    .unwrap()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shared_resource_cross_project_wait_handoff_and_dead_owner_are_authoritative() {
    let _serial = RUNTIME_TEST_LOCK.lock().await;
    // This binary owns its environment and starts no tasks until this assignment.
    std::env::set_var("SHELL", "/bin/sh");
    let directory = durable_directory();
    let catalog = directory.path().join("authority/catalog.json");
    let mut first = Server::start(directory.path(), "first", &catalog).await;
    let mut second = Server::start(directory.path(), "second", &catalog).await;
    let (mut a, project_a) = first.attach().await;
    let (mut b, project_b) = second.attach().await;
    rpc(
        &mut a,
        1,
        request(
            project_a,
            None,
            ResourceOperation::Mutate(ResourceCommand::Create {
                definition: resource_definition(),
                link: link(),
            }),
        ),
    )
    .await
    .unwrap();
    rpc(
        &mut b,
        2,
        request(
            project_b,
            None,
            ResourceOperation::Mutate(ResourceCommand::Link { link: link() }),
        ),
    )
    .await
    .unwrap();
    write_frame(
        &mut a,
        &ClientRequest::NewPane {
            request_id: 399,
            parent_group: ROOT_ID,
            kind: NewPaneKind::Command("exec /bin/cat".into()),
            working_directory: NewPaneWorkingDirectory::ProjectRoot,
        },
    )
    .await
    .unwrap();
    let plain = expected_event(
        &mut a,
        "plain shell creation",
        |event| matches!(event,ServerEvent::TreeSnapshot(tree) if tree.panes().next().is_some()),
    )
    .await;
    let ServerEvent::TreeSnapshot(tree) = plain else {
        unreachable!()
    };
    let plain_id = tree.panes().next().unwrap().id;
    let denied = rpc(
        &mut a,
        399,
        request(
            project_a,
            Some(plain_id),
            ResourceOperation::Mutate(ResourceCommand::Reserve {
                resource_id: "runtime-gpu".into(),
                reservation_id: "plain-shell-claim".into(),
                estimate_seconds: 60,
            }),
        ),
    )
    .await
    .unwrap_err();
    assert_eq!(denied.code, "actor");
    let pane_a = spawn_agent(&mut a, &first).await;
    let pane_b = spawn_agent(&mut b, &second).await;
    let owned = reserve(&mut a, project_a, pane_a, "owner-a", 3)
        .await
        .reservation
        .unwrap();
    assert_eq!(owned.state, ReservationState::Owned);
    let queued = reserve(&mut b, project_b, pane_b, "waiter-b", 4)
        .await
        .reservation
        .unwrap();
    assert_eq!(queued.state, ReservationState::Queued);
    let (mut interrupted, _) = second.attach().await;
    register_wait(
        &mut interrupted,
        10,
        request(
            project_b,
            Some(pane_b),
            ResourceOperation::Wait {
                reservation_id: queued.id.clone(),
            },
        ),
    )
    .await;
    expected_event(&mut b,"interrupted wait badge",|event|matches!(event,ServerEvent::ResourceWaitChanged{pane_id,reservation_id:Some(id),..} if *pane_id==pane_b&&*id==queued.id)).await;
    drop(interrupted);
    assert_eq!(
        rpc(
            &mut b,
            11,
            request(
                project_b,
                Some(pane_b),
                ResourceOperation::Status {
                    reservation_id: queued.id.clone()
                }
            )
        )
        .await
        .unwrap()
        .reservation
        .unwrap()
        .state,
        ReservationState::Queued
    );
    let (mut waiting, _) = second.attach().await;
    register_wait(
        &mut waiting,
        12,
        request(
            project_b,
            Some(pane_b),
            ResourceOperation::Wait {
                reservation_id: queued.id.clone(),
            },
        ),
    )
    .await;
    rpc(
        &mut a,
        13,
        request(
            project_a,
            Some(pane_a),
            ResourceOperation::Mutate(ResourceCommand::Release {
                reservation_id: owned.id.clone(),
                ownership_generation: owned.ownership_generation,
            }),
        ),
    )
    .await
    .unwrap();
    let acquired = tokio::time::timeout(TIMEOUT, async {
        loop {
            let event: ServerEvent = read_frame(&mut waiting).await.unwrap();
            if let ServerEvent::ResourceResult {
                request_id: 12,
                result,
            } = event
            {
                return result.unwrap().reservation.unwrap();
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(acquired.state, ReservationState::Owned);
    assert_eq!(acquired.id, queued.id);
    let next = reserve(&mut a, project_a, pane_a, "waiter-a-again", 14)
        .await
        .reservation
        .unwrap();
    assert_eq!(next.state, ReservationState::Queued);
    // Acquisition without a blocking wait remains durable and pending until the
    // exact owning invocation reconnects and explicitly asks for its status.
    drop(a);
    rpc(
        &mut b,
        15,
        request(
            project_b,
            Some(pane_b),
            ResourceOperation::Mutate(ResourceCommand::Release {
                reservation_id: acquired.id.clone(),
                ownership_generation: acquired.ownership_generation,
            }),
        ),
    )
    .await
    .unwrap();
    let pending = readback(&catalog).await;
    let pending_owner = pending.resources[&ResourceId::from("runtime-gpu")]
        .reservations
        .iter()
        .find(|claim| claim.id == next.id)
        .unwrap();
    assert_eq!(pending_owner.state, ReservationState::Owned);
    assert!(pending_owner.acquisition_pending);
    let wrong = rpc(
        &mut b,
        16,
        request(
            project_b,
            Some(pane_b),
            ResourceOperation::Status {
                reservation_id: next.id.clone(),
            },
        ),
    )
    .await
    .unwrap_err();
    assert_eq!(wrong.code, "actor");
    let (mut a, _) = first.attach().await;
    let acknowledged = rpc(
        &mut a,
        17,
        request(
            project_a,
            Some(pane_a),
            ResourceOperation::Status {
                reservation_id: next.id.clone(),
            },
        ),
    )
    .await
    .unwrap()
    .reservation
    .unwrap();
    assert_eq!(acknowledged.state, ReservationState::Owned);
    assert!(!acknowledged.acquisition_pending);
    assert!(
        !readback(&catalog).await.resources[&ResourceId::from("runtime-gpu")]
            .reservations
            .iter()
            .find(|claim| claim.id == next.id)
            .unwrap()
            .acquisition_pending
    );
    let final_waiter = reserve(&mut b, project_b, pane_b, "waiter-b-again", 18)
        .await
        .reservation
        .unwrap();
    assert_eq!(final_waiter.state, ReservationState::Queued);
    write_frame(&mut a, &ClientRequest::ClosePane { pane_id: pane_a })
        .await
        .unwrap();
    expected_event(&mut b,"closed owner needs-attention snapshot",|event|matches!(event,ServerEvent::ResourceSnapshot{catalog,..} if catalog.resources[&ResourceId::from("runtime-gpu")].reservations.iter().any(|q|q.id==acknowledged.id&&matches!(q.state,ReservationState::NeedsAttention{..})))).await;
    let persisted = readback(&catalog).await;
    let record = &persisted.resources[&"runtime-gpu".into()];
    assert_eq!(persisted.links.len(), 2);
    assert!(record
        .reservations
        .iter()
        .any(|q| q.id == final_waiter.id && q.state == ReservationState::Queued));
    assert!(record
        .reservations
        .iter()
        .any(|q| q.id == acknowledged.id
            && matches!(q.state, ReservationState::NeedsAttention { .. })));
    assert!(!record
        .reservations
        .iter()
        .any(|q| q.state == ReservationState::Owned));
    write_frame(&mut a, &ClientRequest::KillSession)
        .await
        .unwrap();
    write_frame(&mut b, &ClientRequest::KillSession)
        .await
        .unwrap();
    tokio::time::timeout(TIMEOUT, &mut first.task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    tokio::time::timeout(TIMEOUT, &mut second.task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

/// Abruptly stop only this test's server task. A separate synthetic workload
/// remains alive, so restarting coordination cannot imply hardware clearance.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_server_task_restart_preserves_occupied_claim_while_workload_lives() {
    let _serial = RUNTIME_TEST_LOCK.lock().await;
    std::env::set_var("SHELL", "/bin/sh");
    struct Workload(std::process::Child);
    impl Drop for Workload {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut workload = Workload(
        std::process::Command::new("/bin/sleep")
            .arg("600")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let directory = durable_directory();
    let catalog = directory.path().join("authority/catalog.json");
    let mut original = Server::start(directory.path(), "restart", &catalog).await;
    let (mut connection, project) = original.attach().await;
    rpc(
        &mut connection,
        101,
        request(
            project,
            None,
            ResourceOperation::Mutate(ResourceCommand::Create {
                definition: resource_definition(),
                link: link(),
            }),
        ),
    )
    .await
    .unwrap();
    let pane = spawn_agent(&mut connection, &original).await;
    let owner = reserve(&mut connection, project, pane, "restart-owner", 102)
        .await
        .reservation
        .unwrap();
    assert_eq!(owner.state, ReservationState::Owned);
    assert!(workload.0.try_wait().unwrap().is_none());
    drop(connection);
    original.task.abort();
    assert!((&mut original.task).await.unwrap_err().is_cancelled());
    // The abruptly stopped test listener can leave its own endpoint pathname.
    match std::fs::remove_file(&original.socket) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("test socket cleanup: {error}"),
    }
    let mut restarted = Server::start(directory.path(), "restart", &catalog).await;
    let (mut connection, project) = restarted.attach().await;
    let stale = rpc(
        &mut connection,
        103,
        ResourceRequest::new(
            None,
            ResourceOperation::ValidateInvocation {
                agent: owner.agent.clone(),
            },
        ),
    )
    .await
    .unwrap_err();
    assert_eq!(stale.code, "stale_invocation");
    let observed = rpc(
        &mut connection,
        104,
        request(project, None, ResourceOperation::Snapshot),
    )
    .await
    .unwrap();
    let retained = observed.catalog.resources[&ResourceId::from("runtime-gpu")]
        .reservations
        .iter()
        .find(|claim| claim.id == owner.id)
        .unwrap();
    assert!(retained.state.occupied());
    assert_eq!(retained.agent, owner.agent);
    assert!(workload.0.try_wait().unwrap().is_none());
    let authoritative = readback(&catalog).await;
    assert!(authoritative.resources[&ResourceId::from("runtime-gpu")]
        .reservations
        .iter()
        .find(|claim| claim.id == owner.id)
        .unwrap()
        .state
        .occupied());
    write_frame(&mut connection, &ClientRequest::KillSession)
        .await
        .unwrap();
    tokio::time::timeout(TIMEOUT, &mut restarted.task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
