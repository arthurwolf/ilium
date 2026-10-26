//! Isolated application-service acceptance. No real catalog, provider or device.
use super::*;
use crate::config::{DetectionConfig, NotificationsConfig};
use crate::state::ServerStateOptions;
struct Fixture {
    directory: tempfile::TempDir,
    state: Arc<ServerState>,
    project: NodeId,
    playback: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.playback.abort();
    }
}
async fn fixture(shared: Option<PathBuf>) -> Fixture {
    let directory = store::durable_test_directory();
    let root = directory.path().canonicalize().unwrap();
    let (sound_requests, playback) = crate::sounds::spawn(
        Arc::new(crate::NoopSoundPlayer),
        crate::execution::test_general_client(),
    );
    let state = Arc::new(ServerState::new(ServerStateOptions {
        session_name: "resource-service-test".into(),
        session_cwd: root.clone(),
        home_dir: root.clone(),
        snapshot_path: root.join("session.json"),
        socket_path: root.join("session.sock"),
        detection_config: DetectionConfig::default(),
        notifications_config: NotificationsConfig::default(),
        sound_settings: crate::sounds::test_settings(ilium_sound::SoundSettings::default()),
        sound_requests,
        custom_signatures: Vec::new(),
        agent_debug_menu_enabled: false,
        progress_monitor_enabled: true,
    }));
    assert!(state
        .execution
        .set(crate::execution::ServerExecution::start().unwrap())
        .is_ok());
    assert!(state
        .resources
        .set(ResourceService {
            path: shared.unwrap_or_else(|| root.join("isolated-ledger/catalog.json")),
            incarnation: uuid::Uuid::new_v4().to_string(),
            waits: Default::default(),
            changed: tokio::sync::watch::channel(0).0,
            confirmations: Default::default()
        })
        .is_ok());
    let project = state.tree.read().await.project_ids()[0];
    Fixture {
        directory,
        state,
        project,
        playback,
    }
}
fn definition() -> ResourceDefinition {
    ResourceDefinition {
        policy: ResourcePolicy::ExclusiveFifo,
        created_at_unix_millis: 0,
        updated_at_unix_millis: 0,
        id: "test-gpu".into(),
        title: "Test GPU".into(),
        kind: "gpu".into(),
        physical_identity: Some("test-device:gpu-serial".into()),
        description: "Synthetic isolated GPU descriptor".into(),
        rules: "Release after workloads finish".into(),
        definition_revision: 0,
        archived: false,
    }
}
fn link() -> ProjectResourceLink {
    ProjectResourceLink {
        created_at_unix_millis: 0,
        project_id: "overwritten-by-server".into(),
        resource_id: "test-gpu".into(),
        slug: "gpu".into(),
        project_path: "/overwritten-by-server".into(),
        desired_instruction_revision: 0,
        sync_debt: None,
    }
}
fn request(f: &Fixture, command: ResourceCommand) -> ResourceRequest {
    let mut r = ResourceRequest::new(Some(f.project), ResourceOperation::Mutate(command));
    r.request_key = Some(key(&clock().unwrap()).0);
    r
}
async fn create(f: &Fixture) -> ResourceMemory<ResourceReply> {
    execute(
        &f.state,
        request(
            f,
            ResourceCommand::Create {
                definition: definition(),
                link: link(),
            },
        ),
    )
    .await
    .unwrap()
}
fn store(f: &Fixture) -> store::ResourceStore {
    store::ResourceStore::new(service(&f.state).unwrap().path.clone()).unwrap()
}
async fn snapshot_when_store_available(f: &Fixture) -> ResourceCatalog {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match store(f).snapshot() {
            Ok(catalog) => return catalog,
            Err(store::ResourceStoreError::Busy) if std::time::Instant::now() < deadline => {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            Err(error) => panic!("could not read resource catalog: {error}"),
        }
    }
}
async fn resource_snapshot_when_admitted(f: &Fixture) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        match snapshot(&f.state).await {
            Ok(event) => {
                drop(event);
                return;
            }
            Err(error)
                if error.code == "admission"
                    && error.message.contains("Busy")
                    && std::time::Instant::now() < deadline =>
            {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            Err(error) => panic!("resource snapshot was not admitted: {error}"),
        }
    }
}
fn synthetic_agent(f: &Fixture) -> AgentIdentity {
    AgentIdentity {
        project_id: project_identity(f.directory.path()).unwrap(),
        session_id: "synthetic-provider-session".into(),
        pane_id: NodeId(77),
        runtime_generation: "synthetic-provider-runtime".into(),
        invocation_id: "synthetic-provider-invocation".into(),
        label: "Synthetic GPU owner".into(),
    }
}
fn reserve_synthetic(f: &Fixture) {
    let now = clock().unwrap();
    let agent = synthetic_agent(f);
    store(f)
        .transact(
            ResourceCommand::Reserve {
                resource_id: "test-gpu".into(),
                reservation_id: "synthetic-claim".into(),
                estimate_seconds: 600,
            },
            MutationContext {
                request_id: key(&now),
                clock: now,
                actor: ResourceActor::Agent(agent.clone()),
                expected_revision: None,
                eligibility: [(agent.invocation_id, InvocationEligibility::Live)].into(),
            },
        )
        .unwrap();
}
#[tokio::test]
async fn create_and_project_binding_roundtrip_durable_authority() {
    let f = fixture(None).await;
    let reply = create(&f).await;
    let persisted = store(&f).snapshot().unwrap();
    assert_eq!(reply.catalog.links, persisted.links);
    assert_eq!(
        persisted.links[0].project_id,
        project_identity(f.directory.path()).unwrap()
    );
    assert_eq!(persisted.links[0].resource_id, ResourceId::from("test-gpu"));
    assert!(!persisted.receipts.is_empty());
    assert!(reply.catalog.receipts.is_empty());
    let settings = std::fs::read_to_string(f.directory.path().join(".ilium/config.yaml")).unwrap();
    assert!(settings.contains("test-gpu"));
    assert!(settings.contains("resource synchronization"));
}
#[tokio::test]
async fn independent_project_services_share_one_catalog_binding() {
    let first = fixture(None).await;
    create(&first).await;
    let second = fixture(Some(service(&first.state).unwrap().path.clone())).await;
    execute(
        &second.state,
        request(&second, ResourceCommand::Link { link: link() }),
    )
    .await
    .unwrap();
    let shared = store(&first).snapshot().unwrap();
    assert_eq!(shared.resources.len(), 1);
    assert_eq!(shared.links.len(), 2);
    assert!(shared
        .links
        .iter()
        .any(|l| l.project_id == project_identity(first.directory.path()).unwrap()));
    assert!(shared
        .links
        .iter()
        .any(|l| l.project_id == project_identity(second.directory.path()).unwrap()));
    assert_eq!(store(&second).snapshot().unwrap(), shared);
}

#[tokio::test]
async fn connected_server_discovers_first_external_project_link() {
    let fixture = fixture(None).await;
    let quota = fixture.state.execution.get().unwrap().client.quota_group();
    assert!(fixture.state.events.initialize_journal(quota));
    let _connected_client = fixture.state.events.subscribe_shared();
    let subscriber = fixture.state.events.subscribe_ordered().unwrap();
    let observer = spawn(Arc::clone(&fixture.state));

    // Observe the empty initial scan before simulating another server's commit.
    let initial = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let received = subscriber.recv().await.unwrap().unwrap();
            match received.event() {
                ServerEvent::ResourceSnapshot { catalog, .. } if catalog.links.is_empty() => {
                    break received;
                }
                ServerEvent::ResourceObservationFailed { .. } => {
                    subscriber.acknowledge(received.sequence).unwrap();
                }
                event => panic!("expected an empty resource snapshot, got {event:?}"),
            }
        }
    })
    .await
    .expect("observer did not publish its initial resource snapshot");
    assert!(
        matches!(initial.event(),ServerEvent::ResourceSnapshot{catalog,..} if catalog.links.is_empty())
    );
    subscriber.acknowledge(initial.sequence).unwrap();

    let mut external_link = link();
    external_link.project_id = project_identity(fixture.directory.path()).unwrap();
    external_link.project_path = fixture
        .directory
        .path()
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let now = clock().unwrap();
    store(&fixture)
        .transact(
            ResourceCommand::Create {
                definition: definition(),
                link: external_link,
            },
            MutationContext {
                request_id: key(&now),
                clock: now,
                actor: ResourceActor::User,
                expected_revision: None,
                eligibility: BTreeMap::new(),
            },
        )
        .unwrap();
    // Direct durable store mutation leaves this server's changed watch untouched.
    let updated = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let received = subscriber.recv().await.unwrap().unwrap();
            match received.event() {
                ServerEvent::ResourceSnapshot { catalog, .. } if catalog.links.len() == 1 => {
                    break received;
                }
                ServerEvent::ResourceSnapshot { .. }
                | ServerEvent::ResourceObservationFailed { .. } => {
                    subscriber.acknowledge(received.sequence).unwrap();
                }
                event => panic!("expected a resource snapshot update, got {event:?}"),
            }
        }
    })
    .await
    .expect("observer did not publish the external project link");
    let ServerEvent::ResourceSnapshot { catalog, .. } = updated.event() else {
        panic!("expected resource snapshot")
    };
    assert_eq!(catalog.links.len(), 1);
    assert_eq!(catalog.links[0].resource_id, ResourceId::from("test-gpu"));
    subscriber.acknowledge(updated.sequence).unwrap();
    observer.abort();
    let _ = observer.await;
}
#[tokio::test]
async fn unknown_live_observation_never_discards_occupied_claim() {
    let f = fixture(None).await;
    create(&f).await;
    reserve_synthetic(&f);
    let (catalog, _) = reconciled(&f.state).await.unwrap();
    assert_eq!(
        catalog.resources[&"test-gpu".into()].reservations[0].state,
        ReservationState::Owned
    );
}
#[tokio::test]
async fn stale_interactive_recovery_token_cannot_clear_new_revision() {
    let f = fixture(None).await;
    create(&f).await;
    reserve_synthetic(&f);
    let prepared = execute(
        &f.state,
        ResourceRequest::new(
            Some(f.project),
            ResourceOperation::PrepareRecovery {
                resource_id: "test-gpu".into(),
            },
        ),
    )
    .await
    .unwrap();
    let mut edited = definition();
    edited.title = "Updated GPU description".into();
    execute(
        &f.state,
        request(&f, ResourceCommand::Edit { definition: edited }),
    )
    .await
    .unwrap();
    let mut clear = request(
        &f,
        ResourceCommand::ConfirmClear {
            resource_id: "test-gpu".into(),
            reason: "Synthetic interactive confirmation".into(),
        },
    );
    clear.expected_revision = Some(prepared.catalog.revision);
    clear.recovery_token = prepared.recovery_token.clone();
    let error = execute(&f.state, clear).await.unwrap_err();
    assert_eq!(error.code, "stale_confirmation");
    assert_eq!(
        store(&f).snapshot().unwrap().resources[&"test-gpu".into()].reservations[0].state,
        ReservationState::Owned
    );
}
#[tokio::test]
async fn headless_recovery_without_fresh_confirmation_is_refused() {
    let f = fixture(None).await;
    create(&f).await;
    reserve_synthetic(&f);
    let mut clear = request(
        &f,
        ResourceCommand::ConfirmClear {
            resource_id: "test-gpu".into(),
            reason: "No fresh interaction".into(),
        },
    );
    clear.expected_revision = Some(store(&f).snapshot().unwrap().revision);
    let error = execute(&f.state, clear).await.unwrap_err();
    assert_eq!(error.code, "confirmation");
    assert_eq!(
        store(&f).snapshot().unwrap().resources[&"test-gpu".into()].reservations[0].state,
        ReservationState::Owned
    );
}
#[tokio::test]
async fn public_mutation_cannot_forge_internal_eligibility() {
    let f = fixture(None).await;
    create(&f).await;
    let error = execute(&f.state, request(&f, ResourceCommand::ReconcileEligibility))
        .await
        .unwrap_err();
    assert_eq!(error.code, "system_required");
}
#[tokio::test]
async fn provider_validation_rejects_previous_server_incarnation() {
    let f = fixture(None).await;
    let agent = synthetic_agent(&f);
    let error = execute(
        &f.state,
        ResourceRequest::new(None, ResourceOperation::ValidateInvocation { agent }),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "stale_invocation");
}
#[tokio::test]
async fn project_metadata_failure_retains_authority_and_records_sync_debt() {
    let f = fixture(None).await;
    std::fs::create_dir(f.directory.path().join(".ilium")).unwrap();
    std::fs::write(f.directory.path().join(".ilium/config.yaml"), "[malformed").unwrap();
    let reply = create(&f).await;
    assert!(reply
        .message
        .as_ref()
        .is_some_and(|message| message.contains("synchronization")));
    let catalog = store(&f).snapshot().unwrap();
    assert_eq!(catalog.resources.len(), 1);
    assert_eq!(catalog.links.len(), 1);
    assert!(catalog.links[0].sync_debt.is_some());
    assert_eq!(
        std::fs::read_to_string(f.directory.path().join(".ilium/config.yaml")).unwrap(),
        "[malformed"
    );
}

#[tokio::test]
async fn guarded_recovery_retry_reuses_receipt_after_token_consumed() {
    let f = fixture(None).await;
    create(&f).await;
    reserve_synthetic(&f);
    let prepared = execute(
        &f.state,
        ResourceRequest::new(
            Some(f.project),
            ResourceOperation::PrepareRecovery {
                resource_id: "test-gpu".into(),
            },
        ),
    )
    .await
    .unwrap();
    let mut revoke = request(
        &f,
        ResourceCommand::Revoke {
            reservation_id: "synthetic-claim".into(),
            reason: "Synthetic interactive recovery".into(),
        },
    );
    revoke.expected_revision = Some(prepared.catalog.revision);
    revoke.recovery_token = prepared.recovery_token.clone();
    let first = execute(&f.state, revoke.clone()).await.unwrap();
    let second = execute(&f.state, revoke).await.unwrap();
    assert!(second.result.as_ref().unwrap().reused);
    assert_eq!(first.catalog, second.catalog);
    assert!(store(&f).snapshot().unwrap().resources[&"test-gpu".into()].clearance_required);
}
#[tokio::test]
async fn duplicate_wait_guard_keeps_badge_until_last_waiter_disconnects() {
    let f = fixture(None).await;
    let reservation = ReservationId::from("duplicate-wait");
    service(&f.state)
        .unwrap()
        .waits
        .lock()
        .unwrap()
        .entries
        .insert(reservation.clone(), (NodeId(77), 2));
    let mut events = f.state.events.subscribe_owned();
    let first = WaitGuard {
        state: &f.state,
        pane: NodeId(77),
        reservation: reservation.clone(),
    };
    let second = WaitGuard {
        state: &f.state,
        pane: NodeId(77),
        reservation: reservation.clone(),
    };
    drop(first);
    assert_eq!(
        service(&f.state)
            .unwrap()
            .waits
            .lock()
            .unwrap()
            .entries
            .get(&reservation),
        Some(&(NodeId(77), 1))
    );
    assert!(events.try_recv().is_err());
    drop(second);
    assert!(!service(&f.state)
        .unwrap()
        .waits
        .lock()
        .unwrap()
        .entries
        .contains_key(&reservation));
    assert!(matches!(
        events.try_recv().unwrap(),
        ServerEvent::ResourceWaitChanged {
            pane_id: NodeId(77),
            reservation_id: None,
            ..
        }
    ));
}

#[test]
fn bounded_duplicate_wait_attachments_reject_without_changing_registry() {
    let mut registry = WaitRegistry::default();
    let reservation = ReservationId::from("bounded-wait");
    let pane = NodeId(77);
    assert!(registry.attach(reservation.clone(), pane).unwrap());
    for _ in 1..MAX_WAIT_ATTACHMENTS_PER_RESERVATION {
        assert!(!registry.attach(reservation.clone(), pane).unwrap());
    }
    let before = registry.entries.clone();
    let revision = registry.revision;
    assert_eq!(
        registry.attach(reservation.clone(), pane).unwrap_err().code,
        "capacity"
    );
    assert_eq!(registry.entries, before);
    assert_eq!(registry.revision, revision);
    assert_eq!(
        registry.attach(reservation, NodeId(78)).unwrap_err().code,
        "actor"
    );
    assert_eq!(registry.entries, before);
    assert_eq!(registry.revision, revision);
}
#[tokio::test]
async fn shared_catalog_charge_survives_clones_and_retires_with_last_owner() {
    let f = fixture(None).await;
    drop(create(&f).await);
    let quota = f.state.execution.get().unwrap().client.quota_group();
    let before = quota.snapshot().worker_bytes;
    let catalog = read(&f.state).await.unwrap();
    let held = quota.snapshot().worker_bytes;
    assert!(
        held > before,
        "retained catalog must keep its allocation debit"
    );
    let shared = Arc::clone(&catalog);
    drop(catalog);
    assert_eq!(
        quota.snapshot().worker_bytes,
        held,
        "Arc clones share the same charge"
    );
    drop(shared);
    assert_eq!(
        quota.snapshot().worker_bytes,
        before,
        "last catalog owner retires the charge"
    );
}

/// Distinct synthetic USB descriptors model two independently usable boards.
/// This exercises catalog coordination only; no USB device is accessed.
#[tokio::test]
async fn individual_usb_board_bindings_have_independent_ownership_and_queues() {
    let fixture = fixture(None).await;
    for name in ["alpha", "beta"] {
        let mut board = definition();
        board.id = ResourceId(format!("test-board-{name}"));
        board.title = format!("Synthetic USB board {name}");
        board.kind = "usb-board".into();
        board.physical_identity = Some(format!("synthetic-usb:{name}"));
        let mut binding = link();
        binding.resource_id = board.id.clone();
        binding.slug = format!("board-{name}");
        execute(
            &fixture.state,
            request(
                &fixture,
                ResourceCommand::Create {
                    definition: board,
                    link: binding,
                },
            ),
        )
        .await
        .unwrap();
    }
    let mut owner = synthetic_agent(&fixture);
    owner.label = "Synthetic board owner".into();
    let mut beta_owner = owner.clone();
    beta_owner.pane_id = NodeId(79);
    beta_owner.invocation_id = "synthetic-beta-owner".into();
    let mut queued = owner.clone();
    queued.pane_id = NodeId(78);
    queued.invocation_id = "synthetic-board-waiter".into();
    let mutate = |command: ResourceCommand, agent: AgentIdentity| {
        let now = clock().unwrap();
        store(&fixture)
            .transact(
                command,
                MutationContext {
                    request_id: key(&now),
                    clock: now,
                    actor: ResourceActor::Agent(agent),
                    expected_revision: None,
                    eligibility: [
                        (owner.invocation_id.clone(), InvocationEligibility::Live),
                        (queued.invocation_id.clone(), InvocationEligibility::Live),
                        (
                            beta_owner.invocation_id.clone(),
                            InvocationEligibility::Live,
                        ),
                    ]
                    .into(),
                },
            )
            .unwrap()
    };
    for name in ["alpha", "beta"] {
        mutate(
            ResourceCommand::Reserve {
                resource_id: ResourceId(format!("test-board-{name}")),
                reservation_id: ReservationId(format!("board-{name}-owner")),
                estimate_seconds: 600,
            },
            if name == "alpha" {
                owner.clone()
            } else {
                beta_owner.clone()
            },
        );
    }
    mutate(
        ResourceCommand::Reserve {
            resource_id: "test-board-alpha".into(),
            reservation_id: "board-alpha-waiter".into(),
            estimate_seconds: 60,
        },
        queued.clone(),
    );
    let before = store(&fixture).snapshot().unwrap();
    let beta_before = before.resources[&"test-board-beta".into()].clone();
    let generation =
        before.resources[&"test-board-alpha".into()].reservations[0].ownership_generation;
    mutate(
        ResourceCommand::Release {
            reservation_id: "board-alpha-owner".into(),
            ownership_generation: generation,
        },
        owner.clone(),
    );
    let after = store(&fixture).snapshot().unwrap();
    assert_eq!(after.resources[&"test-board-beta".into()], beta_before);
    assert_eq!(
        after.resources[&"test-board-alpha".into()].reservations[1].state,
        ReservationState::Owned
    );
    let config =
        std::fs::read_to_string(fixture.directory.path().join(".ilium/config.yaml")).unwrap();
    assert!(config.contains("board-alpha"));
    assert!(config.contains("board-beta"));
}

#[tokio::test]
async fn reply_to_event_mapping_preserves_charge_until_payload_drop() {
    let fixture = fixture(None).await;
    drop(create(&fixture).await);
    let quota = fixture.state.execution.get().unwrap().client.quota_group();
    let baseline = quota.snapshot().worker_bytes;
    let reply = execute(
        &fixture.state,
        ResourceRequest::new(Some(fixture.project), ResourceOperation::Snapshot),
    )
    .await
    .unwrap();
    let held = quota.snapshot().worker_bytes;
    assert!(held > baseline);
    let event = reply.map(|reply| ServerEvent::ResourceResult {
        request_id: 51,
        result: Ok(reply),
    });
    assert_eq!(quota.snapshot().worker_bytes, held);
    assert!(matches!(
        event.value,
        ServerEvent::ResourceResult { request_id: 51, .. }
    ));
    drop(event);
    assert_eq!(quota.snapshot().worker_bytes, baseline);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn new_server_incarnation_cannot_clear_owner_while_synthetic_workload_lives() {
    struct Workload(std::process::Child);
    impl Drop for Workload {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    // This isolated process stands in for a workload that can outlive an agent.
    let mut workload = Workload(
        std::process::Command::new("/bin/sleep")
            .arg("600")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let process = capture_resource_process(workload.0.id()).unwrap();
    let original = fixture(None).await;
    drop(create(&original).await);
    let mut agent = synthetic_agent(&original);
    agent.runtime_generation = service(&original.state).unwrap().incarnation.clone();
    agent.session_id = original.state.session_name.clone();
    agent.invocation_id = InvocationId(format!(
        "{}:{}:{}:1",
        process.boot_id, process.process_id, process.started_at
    ));
    let now = clock().unwrap();
    store(&original)
        .transact(
            ResourceCommand::Reserve {
                resource_id: "test-gpu".into(),
                reservation_id: "surviving-workload-owner".into(),
                estimate_seconds: 1,
            },
            MutationContext {
                request_id: key(&now),
                clock: now,
                actor: ResourceActor::Agent(agent.clone()),
                expected_revision: None,
                eligibility: [(agent.invocation_id.clone(), InvocationEligibility::Live)].into(),
            },
        )
        .unwrap();
    let path = service(&original.state).unwrap().path.clone();
    let restarted = fixture(Some(path)).await;
    let error = execute(
        &restarted.state,
        ResourceRequest::new(
            None,
            ResourceOperation::ValidateInvocation {
                agent: agent.clone(),
            },
        ),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "stale_invocation");
    let (observed, _) = reconciled(&restarted.state).await.unwrap();
    let claim = &observed.resources[&"test-gpu".into()].reservations[0];
    assert_eq!(claim.state, ReservationState::Owned);
    assert_eq!(claim.agent, agent);
    assert!(workload.0.try_wait().unwrap().is_none());
    assert_eq!(
        store(&restarted).snapshot().unwrap().resources[&"test-gpu".into()].reservations[0].state,
        ReservationState::Owned
    );
}

#[tokio::test]
async fn recovery_confirmation_from_previous_owner_cannot_revoke_after_handoff() {
    let fixture = fixture(None).await;
    drop(create(&fixture).await);
    reserve_synthetic(&fixture);
    let owner = synthetic_agent(&fixture);
    let mut waiter = owner.clone();
    waiter.pane_id = NodeId(78);
    waiter.invocation_id = "synthetic-next-owner".into();
    let now = clock().unwrap();
    let eligibility: BTreeMap<_, _> = [
        (owner.invocation_id.clone(), InvocationEligibility::Live),
        (waiter.invocation_id.clone(), InvocationEligibility::Live),
    ]
    .into();
    store(&fixture)
        .transact(
            ResourceCommand::Reserve {
                resource_id: "test-gpu".into(),
                reservation_id: "next-owner".into(),
                estimate_seconds: 60,
            },
            MutationContext {
                request_id: key(&now),
                clock: now,
                actor: ResourceActor::Agent(waiter),
                expected_revision: None,
                eligibility: eligibility.clone(),
            },
        )
        .unwrap();
    let prepared = execute(
        &fixture.state,
        ResourceRequest::new(
            Some(fixture.project),
            ResourceOperation::PrepareRecovery {
                resource_id: "test-gpu".into(),
            },
        ),
    )
    .await
    .unwrap();
    let generation = store(&fixture).snapshot().unwrap().resources[&"test-gpu".into()].reservations
        [0]
    .ownership_generation;
    let now = clock().unwrap();
    store(&fixture)
        .transact(
            ResourceCommand::Release {
                reservation_id: "synthetic-claim".into(),
                ownership_generation: generation,
            },
            MutationContext {
                request_id: key(&now),
                clock: now,
                actor: ResourceActor::Agent(owner),
                expected_revision: None,
                eligibility,
            },
        )
        .unwrap();
    let mut recovery = request(
        &fixture,
        ResourceCommand::Revoke {
            reservation_id: "synthetic-claim".into(),
            reason: "Stale synthetic confirmation".into(),
        },
    );
    recovery.expected_revision = Some(prepared.catalog.revision);
    recovery.recovery_token = prepared.recovery_token.clone();
    assert_eq!(
        execute(&fixture.state, recovery).await.unwrap_err().code,
        "stale_confirmation"
    );
    let persisted = store(&fixture).snapshot().unwrap();
    assert_eq!(
        persisted.resources[&"test-gpu".into()].reservations[1].state,
        ReservationState::Owned
    );
    assert!(!persisted.resources[&"test-gpu".into()].clearance_required);
}

#[test]
fn detail_projection_keeps_all_active_claims_and_latest_sixteen_terminal_claims() {
    let clock = ResourceClock {
        unix_millis: 1,
        monotonic_millis: 1,
        boot_id: "projection-test".into(),
    };
    let agent = AgentIdentity {
        project_id: "projection-project".into(),
        session_id: "projection-session".into(),
        pane_id: NodeId(1),
        runtime_generation: "projection-runtime".into(),
        invocation_id: "projection-invocation".into(),
        label: "Synthetic projection owner".into(),
    };
    let seed = Reservation {
        id: "projection-claim".into(),
        request_id: "1:projection".into(),
        resource_id: "test-gpu".into(),
        agent,
        ticket: 1,
        estimate_seconds: 60,
        requested_at: clock.clone(),
        acquired_at: None,
        terminal_at: None,
        state: ReservationState::Queued,
        ownership_generation: 0,
        acquisition_pending: false,
    };
    let mut claims = Vec::new();
    for index in 0..24 {
        let mut terminal = seed.clone();
        terminal.id = ReservationId(format!("terminal-{index}"));
        terminal.ticket = index + 3;
        terminal.state = ReservationState::Released;
        terminal.terminal_at = Some(index as i64 + 2);
        claims.push(terminal);
    }
    let mut owner = seed.clone();
    owner.id = "active-owner".into();
    owner.state = ReservationState::Owned;
    owner.acquired_at = Some(clock);
    owner.ownership_generation = 1;
    let mut queued = seed.clone();
    queued.id = "active-waiter".into();
    queued.ticket = 2;
    queued.agent.pane_id = NodeId(2);
    queued.agent.invocation_id = "projection-waiter".into();
    claims.insert(0, owner.clone());
    claims.insert(3, queued.clone());
    let mut catalog = ResourceCatalog {
        revision: 24,
        boot_id: seed.requested_at.boot_id.clone(),
        ..Default::default()
    };
    let mut resource_definition = definition();
    resource_definition.definition_revision = 1;
    let record = ResourceRecord {
        definition: resource_definition,
        gate: ResourceGate::Open,
        clearance_required: false,
        next_ticket: 27,
        next_generation: 2,
        reservations: claims,
    };
    catalog.resources.insert("test-gpu".into(), record.clone());
    let mut attention = record;
    attention.definition.id = "attention-resource".into();
    attention.definition.physical_identity = None;
    let mut stale = seed;
    stale.id = "active-attention".into();
    stale.resource_id = attention.definition.id.clone();
    stale.state = ReservationState::NeedsAttention {
        reason: "Synthetic stale owner".into(),
    };
    stale.agent.pane_id = NodeId(3);
    stale.agent.invocation_id = "projection-stale-owner".into();
    stale.acquired_at = owner.acquired_at.clone();
    stale.ownership_generation = 1;
    attention.reservations = vec![stale.clone()];
    catalog
        .resources
        .insert(attention.definition.id.clone(), attention);
    catalog.receipts.insert(
        "1:projection".into(),
        MutationReceipt {
            command: ResourceCommand::SetGate {
                resource_id: "test-gpu".into(),
                gate: ResourceGate::Open,
            },
            actor: ResourceActor::User,
            result: ResourceCommandResult {
                resource_id: Some("test-gpu".into()),
                reservation_id: None,
                revision: 1,
                reused: false,
            },
            created_at: 1,
        },
    );
    for (resource, slug) in [("test-gpu", "gpu"), ("attention-resource", "attention")] {
        catalog.links.push(ProjectResourceLink {
            created_at_unix_millis: 0,
            project_id: "projection-project".into(),
            resource_id: resource.into(),
            slug: slug.into(),
            project_path: "/synthetic-projection".into(),
            desired_instruction_revision: 1,
            sync_debt: None,
        });
    }
    validate_resource_catalog(&catalog).unwrap();
    let projected = projection(catalog);
    assert!(projected.receipts.is_empty());
    let claims = &projected.resources[&"test-gpu".into()].reservations;
    assert_eq!(claims.len(), 18);
    assert!(claims.contains(&owner));
    assert!(claims.contains(&queued));
    let terminal: Vec<_> = claims
        .iter()
        .filter(|claim| !claim.state.active())
        .map(|claim| claim.id.clone())
        .collect();
    assert_eq!(
        terminal,
        (8..24)
            .map(|index| ReservationId(format!("terminal-{index}")))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        projected.resources[&"attention-resource".into()].reservations,
        vec![stale]
    );
}

#[tokio::test]
async fn late_attach_snapshot_includes_deduplicated_waiters_and_revision() {
    let fixture = fixture(None).await;
    drop(create(&fixture).await);
    let reservation = ReservationId::from("late-attaching-waiter");
    {
        let mut waits = service(&fixture.state).unwrap().waits.lock().unwrap();
        waits.revision = 9;
        waits.entries.insert(reservation.clone(), (NodeId(77), 2));
    }
    let event = snapshot(&fixture.state).await.unwrap();
    let ServerEvent::ResourceSnapshot {
        active_waits,
        wait_revision,
        ..
    } = &event.value
    else {
        panic!("expected resource snapshot")
    };
    assert_eq!(*wait_revision, 9);
    assert_eq!(active_waits, &vec![(NodeId(77), reservation)]);
}

#[tokio::test]
async fn full_active_claim_capacity_crosses_admitted_snapshot_transport() {
    let fixture = fixture(None).await;
    let client = fixture.state.execution.get().unwrap().client.clone();
    let baseline = client.quota_group().snapshot().worker_bytes;
    let catalog = Arc::new(
        io(&fixture.state, || Ok(store::capacity_test_catalog()))
            .await
            .unwrap(),
    );
    let event = snapshot_from(&fixture.state, catalog, clock().unwrap())
        .await
        .unwrap();
    let (sender, mut receiver) = DirectEventSender::admitted_channel(1, client.clone());
    memory::send(&fixture.state, &sender, event).await.unwrap();
    let queued = receiver.recv_queued().await.unwrap();
    assert!(
        queued.producer_storage.is_some(),
        "transport retains the admitted producer envelope"
    );
    let decoded = io(&fixture.state, move || {
        let queued = queued;
        let frame = ilium_ipc::encode_frame(&queued.event).map_err(|error| err("encode", error))?;
        let decoded: ServerEvent =
            ilium_ipc::decode_bounded_frame(&frame).map_err(|error| err("decode", error))?;
        drop(queued); // Keep producer custody until framing and bounded decoding finish.
        Ok(decoded)
    })
    .await
    .unwrap();
    let ServerEvent::ResourceSnapshot { catalog, .. } = &decoded.value else {
        panic!("expected snapshot")
    };
    assert_eq!(
        catalog
            .resources
            .values()
            .map(|resource| resource.reservations.len())
            .sum::<usize>(),
        8192
    );
    drop(decoded);
    drop(sender);
    drop(receiver);
    assert_eq!(
        client.quota_group().snapshot().worker_bytes,
        baseline,
        "snapshot and transport allocations retire with their owners"
    );
}

#[tokio::test]
async fn lifecycle_history_survives_repository_reopen_and_acknowledgement_loss() {
    let fixture = fixture(None).await;
    drop(create(&fixture).await);
    reserve_synthetic(&fixture);
    let reopened = store::ResourceStore::new(store(&fixture).catalog_path()).unwrap();
    let before = reopened.snapshot().unwrap();
    assert!(before
        .history
        .iter()
        .any(|event| event.kind == ResourceLifecycleKind::Acquired));
    let now = clock().unwrap();
    let context = MutationContext {
        request_id: key(&now),
        clock: now,
        actor: ResourceActor::Agent(synthetic_agent(&fixture)),
        expected_revision: None,
        eligibility: BTreeMap::new(),
    };
    let command = ResourceCommand::Release {
        reservation_id: "synthetic-claim".into(),
        ownership_generation: 1,
    };
    let committed = reopened.transact(command.clone(), context.clone()).unwrap();
    let replay = store(&fixture).transact(command, context).unwrap();
    assert!(replay.result.reused);
    assert_eq!(committed.catalog.history, replay.catalog.history);
    assert_eq!(
        store(&fixture).snapshot().unwrap().history,
        committed.catalog.history
    );
    assert!(committed
        .catalog
        .history
        .iter()
        .any(|event| event.kind == ResourceLifecycleKind::Released));
}

/// A synthetic provider backed by a real, continuously ready PTY composer.
/// Detection metadata is explicitly installed; this is not provider detection proof.
#[cfg(target_os = "linux")]
async fn composer_fixture(fixture: &Fixture) -> (NodeId, PathBuf) {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let transcript = fixture.directory.path().join("composer-submissions.txt");
    let command=ilium_pty::PtyCommand::new("/bin/sh",fixture.directory.path(),24,160)
        .arg("-c").arg(r#"stty -echo; printf '\033[2J\033[H› '; while IFS= read -r line; do printf '%s\n' "$line" >> "$1"; printf '\033[2J\033[H› '; done"#)
        .arg("resource-composer-fixture").arg(transcript.to_string_lossy().into_owned());
    let session = ilium_pty::PtySession::spawn(command).unwrap();
    let process_id = session.process_id().unwrap();
    let mut screens = session.subscribe_screen_changed();
    let mut runtime = crate::pane::TerminalPaneRuntime::new(
        session,
        crate::pane::TerminalOrigin::Command("Synthetic resource composer".into()),
        None,
        std::time::Duration::from_secs(1),
    );
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(process_id)]), true);
    let process = system.process(Pid::from_u32(process_id)).unwrap();
    let identity = ilium_detect::AgentIdentity {
        class: ilium_core::AgentClass::Codex,
        pid: process_id,
        started_at_unix_seconds: process.start_time(),
        process_name: ilium_detect::identifying_process_names(process)
            .into_iter()
            .next()
            .unwrap(),
        matched_signature: "synthetic-composer".into(),
        process_tree_depth: 0,
    };
    runtime.adopt_agent_process(&identity).unwrap();
    runtime.detected_agent_process_id = Some(process_id);
    runtime.detected_agent_class = Some(ilium_core::AgentClass::Codex);
    runtime.detection_schedule.cached_identity = Some(identity);
    runtime.agent_input_available = true;
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if runtime.session.try_screen_snapshot().is_some_and(|screen| {
                ilium_detect::is_agent_prompt_ready_at_cursor(
                    &ilium_core::AgentClass::Codex,
                    &screen.text,
                    screen.cursor_position.0,
                    screen.cursor_position.1,
                    &screen.dimmed_cells,
                )
            }) {
                break;
            }
            screens.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    let pane = fixture
        .state
        .tree
        .write()
        .await
        .add_pane(
            fixture.project,
            "Synthetic resource composer",
            ilium_core::PaneContentKind::Terminal,
        )
        .unwrap();
    fixture
        .state
        .tree
        .write()
        .await
        .set_pane_status(
            pane,
            ilium_core::PaneStatus::from_activity(
                ilium_core::AgentClass::Codex,
                ilium_core::AgentActivity::Idle,
                None,
            ),
        )
        .unwrap();
    fixture
        .state
        .panes
        .write()
        .await
        .insert(pane, PaneResource::Terminal(Box::new(runtime)));
    (pane, transcript)
}
#[cfg(target_os = "linux")]
async fn close_composer(fixture: &Fixture, pane: NodeId) {
    let runtime = fixture.state.panes.write().await.remove(&pane).unwrap();
    tokio::task::spawn_blocking(move || {
        if let PaneResource::Terminal(mut runtime) = runtime {
            runtime
                .session
                .shutdown_blocking(std::time::Duration::from_secs(5))
                .unwrap();
        }
    })
    .await
    .unwrap();
}
#[cfg(target_os = "linux")]
#[tokio::test]
async fn captured_actor_replaced_before_commit_cannot_mutate_authority() {
    let fixture = fixture(None).await;
    drop(create(&fixture).await);
    let (pane, _) = composer_fixture(&fixture).await;
    let captured = actor(&fixture.state, Some(pane), fixture.directory.path())
        .await
        .unwrap();
    // Deterministic boundary: capture completes, replacement occurs, then the
    // durable commit is attempted using the previously authenticated actor.
    {
        let mut panes = fixture.state.panes.write().await;
        let PaneResource::Terminal(runtime) = panes.get_mut(&pane).unwrap() else {
            panic!("terminal fixture")
        };
        runtime.agent_generation += 1;
    }
    let before = store(&fixture).snapshot().unwrap();
    let now = clock().unwrap();
    let result = commit(
        &fixture.state,
        ResourceCommand::Reserve {
            resource_id: "test-gpu".into(),
            reservation_id: "stale-captured-actor".into(),
            estimate_seconds: 60,
        },
        MutationContext {
            request_id: key(&now),
            clock: now,
            actor: captured,
            expected_revision: None,
            eligibility: BTreeMap::new(),
        },
        None,
    )
    .await;
    close_composer(&fixture, pane).await;
    assert_eq!(result.unwrap_err().code, "stale_invocation");
    assert_eq!(
        store(&fixture).snapshot().unwrap(),
        before,
        "neither queue, receipts nor lifecycle history may change"
    );
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ready_notice_is_once_per_grant_across_reconnect_snapshots_and_wait_retries() {
    let fixture = fixture(None).await;
    drop(create(&fixture).await);
    reserve_synthetic(&fixture);
    let (pane, transcript) = composer_fixture(&fixture).await;
    let ResourceActor::Agent(agent) = actor(&fixture.state, Some(pane), fixture.directory.path())
        .await
        .unwrap()
    else {
        panic!("agent actor")
    };
    let now = clock().unwrap();
    let claim = ReservationId::from("composer-ready-claim");
    store(&fixture)
        .transact(
            ResourceCommand::Reserve {
                resource_id: "test-gpu".into(),
                reservation_id: claim.clone(),
                estimate_seconds: 60,
            },
            MutationContext {
                request_id: key(&now),
                clock: now,
                actor: ResourceActor::Agent(agent.clone()),
                expected_revision: None,
                eligibility: [(agent.invocation_id.clone(), InvocationEligibility::Live)].into(),
            },
        )
        .unwrap();
    assert_eq!(
        store(&fixture).snapshot().unwrap().resources[&"test-gpu".into()].reservations[1].state,
        ReservationState::Queued
    );
    let now = clock().unwrap();
    store(&fixture)
        .transact(
            ResourceCommand::Release {
                reservation_id: "synthetic-claim".into(),
                ownership_generation: 1,
            },
            MutationContext {
                request_id: key(&now),
                clock: now,
                actor: ResourceActor::Agent(synthetic_agent(&fixture)),
                expected_revision: None,
                eligibility: [(agent.invocation_id.clone(), InvocationEligibility::Live)].into(),
            },
        )
        .unwrap();
    let mut observed = service(&fixture.state).unwrap().changed.subscribe();
    struct OwnedObserver(tokio::task::JoinHandle<()>);
    impl Drop for OwnedObserver {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let mut observer = OwnedObserver(spawn(Arc::clone(&fixture.state)));
    let mut stage = "waiting for the first observer refresh";
    let result = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        // The watch channel tracks catalog revisions, not every retry of a
        // same-revision agent notice, so transcript observation also needs a
        // bounded periodic wakeup.
        let mut notice_recheck = tokio::time::interval(std::time::Duration::from_millis(100));
        loop {
            stage = "waiting for the ready notice or another catalog revision";
            tokio::select! {
                changed = observed.changed() => changed.unwrap(),
                _ = notice_recheck.tick() => {},
            }
            if std::fs::read_to_string(&transcript)
                .unwrap_or_default()
                .contains("Resource reservation composer-ready-claim acquired generation")
            {
                break;
            }
        }
        // Reconnect uses the same production snapshot boundary as late attach.
        for _ in 0..3 {
            stage = "requesting a reconnect snapshot";
            resource_snapshot_when_admitted(&fixture).await;
            stage = "waiting for the observer refresh after reconnect";
            observed.changed().await.unwrap();
        }
        stage = "reading the pending reservation state";
        let pending = snapshot_when_store_available(&fixture).await;
        assert!(
            pending.resources[&"test-gpu".into()]
                .reservations
                .iter()
                .find(|reservation| reservation.id == claim)
                .unwrap()
                .acquisition_pending,
            "a delivered hint is not acquisition acknowledgement"
        );
        for _ in 0..3 {
            stage = "waiting for the promoted reservation";
            let mut request = ResourceRequest::new(
                Some(fixture.project),
                ResourceOperation::Wait {
                    reservation_id: claim.clone(),
                },
            );
            request.pane_id = Some(pane);
            let (sender, _receiver) = DirectEventSender::channel(1);
            let admission_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let reply = loop {
                match wait(&fixture.state, &request, &sender).await {
                    Ok(reply) => break reply,
                    Err(error)
                        if error.code == "admission"
                            && error.message.contains("Busy")
                            && std::time::Instant::now() < admission_deadline =>
                    {
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    }
                    Err(error) => panic!("resource wait failed: {error}"),
                }
            };
            assert_eq!(
                reply.reservation.as_ref().unwrap().state,
                ReservationState::Owned
            );
            let mut status = ResourceRequest::new(
                Some(fixture.project),
                ResourceOperation::Status {
                    reservation_id: claim.clone(),
                },
            );
            status.pane_id = Some(pane);
            stage = "checking status after the ready notice";
            drop(execute(&fixture.state, status).await.unwrap());
            stage = "waiting for the observer refresh after status";
            observed.changed().await.unwrap();
        }
        stage = "checking duplicate notice delivery";
        let submissions = std::fs::read_to_string(&transcript).unwrap();
        assert_eq!(
            submissions
                .matches("Resource reservation composer-ready-claim acquired generation")
                .count(),
            1,
            "ready composer must not receive duplicate grant notices"
        );
        assert!(
            !store(&fixture).snapshot().unwrap().resources[&"test-gpu".into()]
                .reservations
                .iter()
                .find(|reservation| reservation.id == claim)
                .unwrap()
                .acquisition_pending
        );
    })
    .await;
    observer.0.abort();
    let _ = (&mut observer.0).await;
    close_composer(&fixture, pane).await;
    result.unwrap_or_else(|error| panic!("resource observer timed out while {stage}: {error}"));
}

#[tokio::test]
async fn wait_snapshot_vector_retains_lease_and_refuses_saturated_admission() {
    let fixture = fixture(None).await;
    let client = &fixture.state.execution.get().unwrap().client;
    let quota = client.quota_group();
    let baseline = quota.snapshot().worker_bytes;
    {
        let mut waits = service(&fixture.state).unwrap().waits.lock().unwrap();
        waits.revision = 12;
        for index in 0..64 {
            waits.entries.insert(
                ReservationId(format!("wait-allocation-{index}")),
                (NodeId(index + 1), 2),
            );
        }
    }
    let waiting = active_waits(&fixture.state).unwrap();
    assert_eq!(waiting.value.1.len(), 64);
    assert_eq!(waiting.value.0, 12);
    assert!(
        quota.snapshot().worker_bytes > baseline,
        "cloned waiter identities must retain an allocation lease"
    );
    drop(waiting);
    assert_eq!(quota.snapshot().worker_bytes, baseline);
    let available = quota.snapshot().limits.worker_bytes - baseline;
    let pressure = client.try_reserve_storage(available).unwrap();
    let error = active_waits(&fixture.state).unwrap_err();
    assert_eq!(error.code, "admission");
    assert_eq!(
        service(&fixture.state)
            .unwrap()
            .waits
            .lock()
            .unwrap()
            .entries
            .len(),
        64,
        "admission refusal must preserve authoritative wait registrations"
    );
    drop(pressure);
    assert_eq!(quota.snapshot().worker_bytes, baseline);
    drop(active_waits(&fixture.state).unwrap());
    assert_eq!(quota.snapshot().worker_bytes, baseline);
}

// These futures are constructed by every IPC request/attach, including when
// resources are disabled. Their inline size must not include the queue's full
// reconciliation and blocking-wait state machines.
#[tokio::test]
async fn resource_entry_futures_do_not_inflate_connection_stack() {
    fn future_bytes<F: std::future::Future>(_: impl FnOnce() -> F) -> usize {
        std::mem::size_of::<F>()
    }
    let fixture = fixture(None).await;
    let (sender, _receiver) = DirectEventSender::channel(1);
    let operation = || ResourceRequest::new(Some(fixture.project), ResourceOperation::Snapshot);
    let handle_bytes = future_bytes(|| handle(&fixture.state, 1, operation(), &sender));
    let attach_bytes = future_bytes(|| send_snapshot(&fixture.state, &sender));
    let inner_handle_bytes = future_bytes(|| handle_inner(&fixture.state, 1, operation(), &sender));
    let inner_attach_bytes = future_bytes(|| send_snapshot_inner(&fixture.state, &sender));
    let execute_bytes = future_bytes(|| execute(&fixture.state, operation()));
    let snapshot_bytes = future_bytes(|| snapshot(&fixture.state));
    let connection_handler_bytes = future_bytes(|| {
        crate::ipc::handlers::handle_request(
            &fixture.state,
            ilium_ipc::ClientRequest::Resource {
                request_id: 1,
                request: operation(),
            },
            &sender,
        )
    });
    eprintln!("resource async layout: entry={handle_bytes} attach={attach_bytes} inner_request={inner_handle_bytes} inner_attach={inner_attach_bytes} execute={execute_bytes} snapshot={snapshot_bytes} ipc_handler={connection_handler_bytes}");
    let pointer_envelope = 2 * std::mem::size_of::<usize>();
    assert!(
        handle_bytes <= pointer_envelope,
        "resource request workflow must be heap-backed, got {handle_bytes} inline bytes"
    );
    assert!(
        attach_bytes <= pointer_envelope,
        "resource attach workflow must be heap-backed, got {attach_bytes} inline bytes"
    );
    assert!(
        connection_handler_bytes <= pointer_envelope,
        "IPC request dispatch must be heap-backed, got {connection_handler_bytes} inline bytes"
    );
}
