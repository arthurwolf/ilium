use ilium_core::resource_queue::{ResourceCatalog, ResourceClock};
use ilium_ipc::resource::{ResourceOperation, ResourceReply, ResourceRequest};
use ilium_ipc::{ClientRequest, ServerEvent};
#[test]
fn resource_snapshot_request_roundtrips() {
    let request = ClientRequest::Resource {
        request_id: 41,
        request: ResourceRequest::new(None, ResourceOperation::Snapshot),
    };
    let encoded = ilium_ipc::encode_frame(&request).unwrap();
    let decoded: ClientRequest = ilium_ipc::decode_frame(&encoded).unwrap();
    assert!(matches!(
        decoded,
        ClientRequest::Resource { request_id: 41, .. }
    ));
}
#[test]
fn resource_snapshot_and_result_are_typed() {
    let catalog = ResourceCatalog::default();
    let clock = ResourceClock {
        unix_millis: 1,
        monotonic_millis: 1,
        boot_id: "test".into(),
    };
    let snapshot = ServerEvent::ResourceSnapshot {
        active_waits: Vec::new(),
        wait_revision: 0,
        local_invocations: Vec::new(),
        catalog: catalog.clone(),
        clock: clock.clone(),
    };
    assert!(matches!(snapshot,ServerEvent::ResourceSnapshot{catalog,..} if catalog.revision==0));
    let result = ServerEvent::ResourceResult {
        request_id: 42,
        result: Ok(ResourceReply {
            active_waits: Vec::new(),
            wait_revision: 0,
            local_invocations: Vec::new(),
            catalog,
            clock,
            result: None,
            reservation: None,
            recovery_token: None,
            message: None,
        }),
    };
    assert!(matches!(
        result,
        ServerEvent::ResourceResult {
            request_id: 42,
            result: Ok(_)
        }
    ));
}

#[test]
fn every_resource_operation_and_event_roundtrips_with_owned_capacity() {
    use ilium_core::{resource_queue::*, NodeId};
    let clock = ResourceClock {
        unix_millis: 1000,
        monotonic_millis: 100,
        boot_id: "fixture-boot".into(),
    };
    let agent = AgentIdentity {
        project_id: "fixture-project".into(),
        session_id: "fixture-session".into(),
        pane_id: NodeId(7),
        runtime_generation: "fixture-runtime".into(),
        invocation_id: "fixture-invocation".into(),
        label: "Fixture agent".into(),
    };
    let definition = ResourceDefinition {
        id: "fixture-resource".into(),
        title: "Fixture resource".into(),
        kind: "generic".into(),
        physical_identity: None,
        description: "Synthetic protocol fixture".into(),
        rules: String::new(),
        policy: ResourcePolicy::ExclusiveFifo,
        created_at_unix_millis: 0,
        updated_at_unix_millis: 0,
        definition_revision: 1,
        archived: false,
    };
    let link = ProjectResourceLink {
        project_id: agent.project_id.clone(),
        resource_id: definition.id.clone(),
        slug: "fixture".into(),
        project_path: "/fixture".into(),
        created_at_unix_millis: 0,
        desired_instruction_revision: 1,
        sync_debt: None,
    };
    let id = ReservationId("fixture-reservation".into());
    let resource_id = definition.id.clone();
    let commands = vec![
        ResourceCommand::RebindProject {
            project_id: agent.project_id.clone(),
            project_path: "/fixture-moved".into(),
        },
        ResourceCommand::Create {
            definition: definition.clone(),
            link: link.clone(),
        },
        ResourceCommand::Link { link: link.clone() },
        ResourceCommand::Edit { definition },
        ResourceCommand::Unlink {
            project_id: agent.project_id.clone(),
            resource_id: resource_id.clone(),
        },
        ResourceCommand::Archive {
            resource_id: resource_id.clone(),
        },
        ResourceCommand::Reserve {
            resource_id: resource_id.clone(),
            reservation_id: id.clone(),
            estimate_seconds: 600,
        },
        ResourceCommand::Release {
            reservation_id: id.clone(),
            ownership_generation: 4,
        },
        ResourceCommand::Cancel {
            reservation_id: id.clone(),
        },
        ResourceCommand::CancelByUser {
            reservation_id: id.clone(),
            reason: "fixture".into(),
        },
        ResourceCommand::Estimate {
            reservation_id: id.clone(),
            estimate_seconds: 700,
        },
        ResourceCommand::Acknowledge {
            reservation_id: id.clone(),
            ownership_generation: 4,
        },
        ResourceCommand::SetGate {
            resource_id: resource_id.clone(),
            gate: ResourceGate::Paused {
                reason: "fixture".into(),
            },
        },
        ResourceCommand::Revoke {
            reservation_id: id.clone(),
            reason: "fixture".into(),
        },
        ResourceCommand::ConfirmClear {
            resource_id: resource_id.clone(),
            reason: "fixture".into(),
        },
        ResourceCommand::ReconcileInvocation {
            agent: agent.clone(),
            evidence: InvocationEligibility::Unknown,
        },
        ResourceCommand::ReconcileBoot,
        ResourceCommand::ReconcileEligibility,
        ResourceCommand::SetSyncDebt {
            project_id: agent.project_id.clone(),
            resource_id: resource_id.clone(),
            reason: Some("fixture".into()),
        },
    ];
    let mut operations = vec![
        ResourceOperation::RepairInstructions {
            resource_id: resource_id.clone(),
        },
        ResourceOperation::Snapshot,
        ResourceOperation::ValidateInvocation {
            agent: agent.clone(),
        },
        ResourceOperation::Status {
            reservation_id: id.clone(),
        },
        ResourceOperation::Wait {
            reservation_id: id.clone(),
        },
        ResourceOperation::PrepareRecovery { resource_id },
    ];
    operations.extend(commands.into_iter().map(ResourceOperation::Mutate));
    for operation in operations {
        let mut request = ResourceRequest::new(Some(NodeId(3)), operation);
        request.pane_id = Some(agent.pane_id);
        request.request_key = Some("1000:fixture".into());
        request.expected_revision = Some(4);
        request.recovery_token = Some("fixture-token".into());
        let frame = ilium_ipc::encode_frame(&ClientRequest::Resource {
            request_id: 99,
            request: request.clone(),
        })
        .unwrap();
        let decoded: ClientRequest = ilium_ipc::decode_frame(&frame).unwrap();
        assert!(
            matches!(decoded,ClientRequest::Resource{request_id:99,request:actual} if actual==request)
        );
    }
    let mut message = String::with_capacity(8192);
    message.push_str("fixture observation failed");
    let events = vec![
        ServerEvent::ResourceObservationFailed { message },
        ServerEvent::ResourceSnapshot {
            active_waits: Vec::new(),
            wait_revision: 0,
            catalog: ResourceCatalog::default(),
            clock: clock.clone(),
            local_invocations: vec![agent.clone()],
        },
        ServerEvent::ResourceResult {
            request_id: 99,
            result: Ok(ResourceReply {
                active_waits: Vec::new(),
                wait_revision: 0,
                catalog: ResourceCatalog::default(),
                clock,
                local_invocations: vec![agent],
                result: None,
                reservation: None,
                recovery_token: Some("fixture".into()),
                message: None,
            }),
        },
        ServerEvent::ResourceResult {
            request_id: 100,
            result: Err(ilium_ipc::resource::ResourceError {
                code: "fixture".into(),
                message: "fixture error".into(),
            }),
        },
        ServerEvent::ResourceWaitChanged {
            wait_revision: 1,
            pane_id: NodeId(7),
            reservation_id: Some(id),
        },
        ServerEvent::ResourceWaitChanged {
            wait_revision: 1,
            pane_id: NodeId(7),
            reservation_id: None,
        },
    ];
    for event in events {
        if matches!(&event, ServerEvent::ResourceObservationFailed { .. }) {
            assert!(
                event.retained_bytes() >= 8192,
                "Unused string capacity must remain admitted"
            );
        }
        let frame = ilium_ipc::encode_frame(&event).unwrap();
        let decoded: ServerEvent = ilium_ipc::decode_frame(&frame).unwrap();
        assert_eq!(format!("{event:?}"), format!("{decoded:?}"));
    }
}

#[test]
fn resource_states_history_and_wait_associations_survive_bounded_wire_decode() {
    use ilium_core::{resource_queue::*, NodeId};
    let clock = ResourceClock {
        unix_millis: 1000,
        monotonic_millis: 100,
        boot_id: "wire-boot".into(),
    };
    let agent = AgentIdentity {
        project_id: "wire-project".into(),
        session_id: "wire-session".into(),
        pane_id: NodeId(17),
        runtime_generation: "wire-runtime".into(),
        invocation_id: "wire-invocation".into(),
        label: "Synthetic agent 硬件".into(),
    };
    let states = [
        ReservationState::Queued,
        ReservationState::Owned,
        ReservationState::NeedsAttention {
            reason: "Uncertain workload clearance".into(),
        },
        ReservationState::Released,
        ReservationState::Cancelled,
        ReservationState::Revoked {
            reason: "User recovery".into(),
        },
    ];
    let gates = [
        ResourceGate::Open,
        ResourceGate::Paused {
            reason: "Inspect hardware".into(),
        },
        ResourceGate::Unavailable {
            reason: "Synthetic device absent".into(),
        },
    ];
    let kinds = [
        ResourceLifecycleKind::Created,
        ResourceLifecycleKind::Linked,
        ResourceLifecycleKind::Unlinked,
        ResourceLifecycleKind::Edited,
        ResourceLifecycleKind::Archived,
        ResourceLifecycleKind::Reserved,
        ResourceLifecycleKind::Acquired,
        ResourceLifecycleKind::Released,
        ResourceLifecycleKind::Cancelled,
        ResourceLifecycleKind::Revoked,
        ResourceLifecycleKind::NeedsAttention,
        ResourceLifecycleKind::GateChanged,
        ResourceLifecycleKind::RecoveryConfirmed,
        ResourceLifecycleKind::EstimateChanged,
        ResourceLifecycleKind::Acknowledged,
        ResourceLifecycleKind::ProjectRebound,
        ResourceLifecycleKind::BootFenced,
    ];
    for state in states {
        for gate in &gates {
            let id = ReservationId::from("wire-reservation");
            let claim = Reservation {
                id: id.clone(),
                request_id: "1000:wire-request".into(),
                resource_id: "wire-resource".into(),
                agent: agent.clone(),
                ticket: 1,
                estimate_seconds: 600,
                requested_at: clock.clone(),
                acquired_at: state.occupied().then(|| clock.clone()),
                terminal_at: (!state.active()).then_some(1000),
                state: state.clone(),
                ownership_generation: 1,
                acquisition_pending: state == ReservationState::Owned,
            };
            let mut catalog = ResourceCatalog {
                revision: 17,
                boot_id: clock.boot_id.clone(),
                ..Default::default()
            };
            catalog.resources.insert(
                "wire-resource".into(),
                ResourceRecord {
                    definition: ResourceDefinition {
                        id: "wire-resource".into(),
                        title: "Synthetic GPU / 板".into(),
                        kind: "gpu".into(),
                        physical_identity: None,
                        description: "Protocol fixture, no actual device".into(),
                        rules: String::new(),
                        policy: ResourcePolicy::ExclusiveFifo,
                        created_at_unix_millis: 0,
                        updated_at_unix_millis: 0,
                        definition_revision: 1,
                        archived: false,
                    },
                    gate: gate.clone(),
                    clearance_required: !matches!(gate, ResourceGate::Open),
                    next_ticket: 2,
                    next_generation: 2,
                    reservations: vec![claim.clone()],
                },
            );
            catalog.history = kinds
                .iter()
                .enumerate()
                .map(|(index, kind)| ResourceLifecycleEvent {
                    resource_id: "wire-resource".into(),
                    reservation_id: Some(id.clone()),
                    revision: index as u64 + 1,
                    ordinal: 0,
                    unix_millis: 1000,
                    actor: ResourceActor::Agent(agent.clone()),
                    kind: kind.clone(),
                    message: "Synthetic bounded lifecycle event".into(),
                })
                .collect();
            let event = ServerEvent::ResourceResult {
                request_id: 901,
                result: Ok(ResourceReply {
                    catalog,
                    clock: clock.clone(),
                    result: None,
                    reservation: Some(claim),
                    recovery_token: None,
                    message: None,
                    local_invocations: vec![agent.clone()],
                    active_waits: vec![(agent.pane_id, id)],
                    wait_revision: 42,
                }),
            };
            let frame = ilium_ipc::encode_frame(&event).unwrap();
            let decoded: ServerEvent = ilium_ipc::decode_bounded_frame(&frame).unwrap();
            assert_eq!(format!("{event:?}"), format!("{decoded:?}"));
        }
    }
}
