use super::*;
fn actor(name: &str) -> AgentIdentity {
    AgentIdentity {
        project_id: "project".into(),
        session_id: "session".into(),
        pane_id: crate::NodeId(1),
        runtime_generation: "runtime".into(),
        invocation_id: name.into(),
        label: name.into(),
    }
}
fn context(n: i64, agent: Option<&str>) -> MutationContext {
    MutationContext {
        request_id: RequestId(format!("{n}:request")),
        clock: ResourceClock {
            unix_millis: n,
            monotonic_millis: n as u64,
            boot_id: "boot".into(),
        },
        actor: agent.map_or(ResourceActor::User, |s| ResourceActor::Agent(actor(s))),
        expected_revision: None,
        eligibility: [
            ("a".into(), InvocationEligibility::Live),
            ("b".into(), InvocationEligibility::Live),
            ("c".into(), InvocationEligibility::Live),
        ]
        .into(),
    }
}
fn catalog() -> ResourceCatalog {
    apply_resource_command(
        &ResourceCatalog::default(),
        ResourceCommand::Create {
            definition: ResourceDefinition {
                policy: ResourcePolicy::ExclusiveFifo,
                created_at_unix_millis: 0,
                updated_at_unix_millis: 0,
                id: "gpu".into(),
                title: "GPU".into(),
                kind: "gpu".into(),
                physical_identity: None,
                description: String::new(),
                rules: String::new(),
                definition_revision: 0,
                archived: false,
            },
            link: ProjectResourceLink {
                created_at_unix_millis: 0,
                project_id: "project".into(),
                resource_id: "gpu".into(),
                slug: "gpu".into(),
                project_path: "/project".into(),
                desired_instruction_revision: 0,
                sync_debt: None,
            },
        },
        &context(1, None),
    )
    .unwrap()
    .catalog
}
fn reserve(c: &ResourceCatalog, agent: &str, n: i64, seconds: u64) -> ResourceCatalog {
    apply_resource_command(
        c,
        ResourceCommand::Reserve {
            resource_id: "gpu".into(),
            reservation_id: agent.into(),
            estimate_seconds: seconds,
        },
        &context(n, Some(agent)),
    )
    .unwrap()
    .catalog
}
#[test]
fn free_resource_grants_one_owner() {
    let c = reserve(&catalog(), "a", 2, 600);
    assert_eq!(
        c.resources[&ResourceId::from("gpu")].reservations[0].state,
        ReservationState::Owned
    );
}
#[test]
fn occupied_resource_enqueues_fifo() {
    let c = reserve(
        &reserve(&reserve(&catalog(), "a", 2, 600), "b", 120002, 300),
        "c",
        120003,
        60,
    );
    let q = project_resource_queue(&c.resources[&"gpu".into()], &context(120002, None).clock);
    assert_eq!(q.queue[0].estimate.wait_seconds, Some(480));
    assert_eq!(q.queue[1].estimate.wait_seconds, Some(780));
}
#[test]
fn release_promotes_exactly_one() {
    let c = reserve(
        &reserve(&reserve(&catalog(), "a", 2, 600), "b", 3, 300),
        "c",
        4,
        60,
    );
    let c = apply_resource_command(
        &c,
        ResourceCommand::Release {
            reservation_id: "a".into(),
            ownership_generation: 1,
        },
        &context(5, Some("a")),
    )
    .unwrap()
    .catalog;
    let r = &c.resources[&"gpu".into()].reservations;
    assert_eq!(r[1].state, ReservationState::Owned);
    assert_eq!(r[2].state, ReservationState::Queued);
}
#[test]
fn cancel_after_grant_requires_release() {
    let c = reserve(&catalog(), "a", 2, 600);
    assert_eq!(
        apply_resource_command(
            &c,
            ResourceCommand::Cancel {
                reservation_id: "a".into()
            },
            &context(3, Some("a"))
        )
        .unwrap_err()
        .code,
        "already_owned"
    );
}
#[test]
fn retries_do_not_allocate_tickets() {
    let c = catalog();
    let cmd = ResourceCommand::Reserve {
        resource_id: "gpu".into(),
        reservation_id: "a".into(),
        estimate_seconds: 1,
    };
    let ctx = context(2, Some("a"));
    let first = apply_resource_command(&c, cmd.clone(), &ctx).unwrap();
    let again = apply_resource_command(&first.catalog, cmd, &ctx).unwrap();
    assert_eq!(again.catalog.revision, first.catalog.revision);
    assert!(again.result.reused);
}
#[test]
fn expired_request_refused() {
    let mut ctx = context(2, Some("a"));
    ctx.clock.unix_millis = REQUEST_WINDOW_MILLIS + 3;
    assert_eq!(
        apply_resource_command(&catalog(), ResourceCommand::ReconcileBoot, &ctx)
            .unwrap_err()
            .code,
        "idempotency_expired"
    );
}
#[test]
fn paused_release_does_not_promote() {
    let c = reserve(&reserve(&catalog(), "a", 2, 10), "b", 3, 10);
    let c = apply_resource_command(
        &c,
        ResourceCommand::SetGate {
            resource_id: "gpu".into(),
            gate: ResourceGate::Unavailable {
                reason: "unplugged".into(),
            },
        },
        &context(4, None),
    )
    .unwrap()
    .catalog;
    let c = apply_resource_command(
        &c,
        ResourceCommand::Release {
            reservation_id: "a".into(),
            ownership_generation: 1,
        },
        &context(5, Some("a")),
    )
    .unwrap()
    .catalog;
    assert_eq!(
        c.resources[&"gpu".into()].reservations[1].state,
        ReservationState::Queued
    );
}
#[test]
fn boot_change_fences_claims() {
    let c = reserve(&reserve(&catalog(), "a", 2, 10), "b", 3, 10);
    let mut ctx = context(4, None);
    ctx.actor = ResourceActor::System;
    ctx.clock.boot_id = "next".into();
    let c = apply_resource_command(&c, ResourceCommand::ReconcileBoot, &ctx)
        .unwrap()
        .catalog;
    let r = &c.resources[&"gpu".into()].reservations;
    assert!(matches!(
        r[0].state,
        ReservationState::NeedsAttention { .. }
    ));
    assert_eq!(r[1].state, ReservationState::Cancelled);
}
#[test]
fn uncertain_head_blocks_handoff() {
    let c = reserve(&reserve(&catalog(), "a", 2, 10), "b", 3, 10);
    let mut ctx = context(4, Some("a"));
    ctx.eligibility.clear();
    let c = apply_resource_command(
        &c,
        ResourceCommand::Release {
            reservation_id: "a".into(),
            ownership_generation: 1,
        },
        &ctx,
    )
    .unwrap()
    .catalog;
    assert_eq!(
        c.resources[&"gpu".into()].reservations[1].state,
        ReservationState::Queued
    );
}
#[test]
fn different_payload_same_request_refused() {
    let c = reserve(&catalog(), "a", 2, 10);
    let e = apply_resource_command(
        &c,
        ResourceCommand::Reserve {
            resource_id: "gpu".into(),
            reservation_id: "a".into(),
            estimate_seconds: 20,
        },
        &context(2, Some("a")),
    )
    .unwrap_err();
    assert_eq!(e.code, "idempotency_conflict");
}
#[test]
fn overdue_owner_never_frees_resource() {
    let c = reserve(&reserve(&catalog(), "a", 2, 600), "b", 3, 10);
    let q = project_resource_queue(&c.resources[&"gpu".into()], &context(601002, None).clock);
    assert_eq!(q.owner.unwrap().state, ReservationState::Owned);
    assert_eq!(q.queue[0].estimate.wait_seconds, None);
}
#[test]
fn old_invocation_cannot_release_new_claim() {
    let c = reserve(&catalog(), "a", 2, 10);
    let e = apply_resource_command(
        &c,
        ResourceCommand::Release {
            reservation_id: "a".into(),
            ownership_generation: 1,
        },
        &context(3, Some("replacement")),
    )
    .unwrap_err();
    assert_eq!(e.code, "stale_invocation");
}
#[test]
fn revoke_preserves_waiters_until_clear() {
    let c = reserve(&reserve(&catalog(), "a", 2, 10), "b", 3, 10);
    let mut ctx = context(4, None);
    ctx.expected_revision = Some(c.revision);
    let c = apply_resource_command(
        &c,
        ResourceCommand::Revoke {
            reservation_id: "a".into(),
            reason: "Check running workload".into(),
        },
        &ctx,
    )
    .unwrap()
    .catalog;
    assert_eq!(
        c.resources[&"gpu".into()].reservations[1].state,
        ReservationState::Queued
    );
    let mut ctx = context(5, None);
    ctx.expected_revision = Some(c.revision);
    let c = apply_resource_command(
        &c,
        ResourceCommand::ConfirmClear {
            resource_id: "gpu".into(),
            reason: "Physically confirmed clear".into(),
        },
        &ctx,
    )
    .unwrap()
    .catalog;
    assert_eq!(
        c.resources[&"gpu".into()].reservations[1].state,
        ReservationState::Owned
    );
}
#[test]
fn stale_clear_is_refused() {
    let c = reserve(&catalog(), "a", 2, 10);
    let mut ctx = context(3, None);
    ctx.expected_revision = Some(0);
    assert_eq!(
        apply_resource_command(
            &c,
            ResourceCommand::ConfirmClear {
                resource_id: "gpu".into(),
                reason: "clear".into()
            },
            &ctx
        )
        .unwrap_err()
        .code,
        "revision_conflict"
    );
}
#[test]
fn definite_owner_exit_keeps_occupancy() {
    let c = reserve(&reserve(&catalog(), "a", 2, 10), "b", 3, 10);
    let mut ctx = context(4, None);
    ctx.actor = ResourceActor::System;
    let c = apply_resource_command(
        &c,
        ResourceCommand::ReconcileInvocation {
            agent: actor("a"),
            evidence: InvocationEligibility::Dead,
        },
        &ctx,
    )
    .unwrap()
    .catalog;
    assert!(matches!(
        c.resources[&"gpu".into()].reservations[0].state,
        ReservationState::NeedsAttention { .. }
    ));
    assert_eq!(
        c.resources[&"gpu".into()].reservations[1].state,
        ReservationState::Queued
    );
}
#[test]
fn queued_exit_only_cancels_matching_invocation() {
    let c = reserve(&reserve(&catalog(), "a", 2, 10), "b", 3, 10);
    let mut ctx = context(4, None);
    ctx.actor = ResourceActor::System;
    let c = apply_resource_command(
        &c,
        ResourceCommand::ReconcileInvocation {
            agent: actor("b"),
            evidence: InvocationEligibility::Dead,
        },
        &ctx,
    )
    .unwrap()
    .catalog;
    assert_eq!(
        c.resources[&"gpu".into()].reservations[0].state,
        ReservationState::Owned
    );
    assert_eq!(
        c.resources[&"gpu".into()].reservations[1].state,
        ReservationState::Cancelled
    );
}
#[test]
fn busy_unlink_is_refused() {
    let c = reserve(&catalog(), "a", 2, 10);
    assert_eq!(
        apply_resource_command(
            &c,
            ResourceCommand::Unlink {
                project_id: "project".into(),
                resource_id: "gpu".into()
            },
            &context(3, None)
        )
        .unwrap_err()
        .code,
        "busy"
    );
}
#[test]
fn duration_bounds_and_compound_units() {
    assert_eq!(parse_resource_duration("1h30m").unwrap(), 5400);
    assert_eq!(parse_resource_duration("7d").unwrap(), 604800);
    for invalid in ["", "0s", "-1m", "1.5h", "8d", "1", "18446744073709551615d"] {
        assert!(parse_resource_duration(invalid).is_err(), "{invalid}")
    }
}
#[test]
fn same_invocation_reuses_original_estimate() {
    let c = reserve(&reserve(&catalog(), "a", 2, 10), "a", 3, 20);
    let r = &c.resources[&"gpu".into()];
    assert_eq!(r.reservations.len(), 1);
    assert_eq!(r.reservations[0].estimate_seconds, 10);
}
#[test]
fn unknown_boot_requires_fencing_transaction() {
    let c = reserve(&catalog(), "a", 2, 10);
    let mut ctx = context(3, Some("b"));
    ctx.clock.boot_id = "next".into();
    assert_eq!(
        apply_resource_command(
            &c,
            ResourceCommand::Reserve {
                resource_id: "gpu".into(),
                reservation_id: "b".into(),
                estimate_seconds: 1
            },
            &ctx
        )
        .unwrap_err()
        .code,
        "boot_reconciliation_required"
    );
}
#[test]
fn estimate_update_preserves_order() {
    let c = reserve(
        &reserve(&reserve(&catalog(), "a", 2, 600), "b", 3, 300),
        "c",
        4,
        60,
    );
    let c = apply_resource_command(
        &c,
        ResourceCommand::Estimate {
            reservation_id: "b".into(),
            estimate_seconds: 10,
        },
        &context(5, Some("b")),
    )
    .unwrap()
    .catalog;
    let q = project_resource_queue(&c.resources[&"gpu".into()], &context(1002, None).clock);
    assert_eq!(q.queue[0].reservation.ticket, 2);
    assert_eq!(q.queue[1].estimate.wait_seconds, Some(609));
}
#[test]
fn edited_physical_identity_is_unique() {
    let mut c = catalog();
    let mut definition = c.resources[&"gpu".into()].definition.clone();
    definition.physical_identity = Some("pci:0000:01:00.0".into());
    c = apply_resource_command(
        &c,
        ResourceCommand::Edit {
            definition: definition.clone(),
        },
        &context(2, None),
    )
    .unwrap()
    .catalog;
    definition.id = "other".into();
    let cmd = ResourceCommand::Create {
        definition,
        link: ProjectResourceLink {
            created_at_unix_millis: 0,
            project_id: "project".into(),
            resource_id: "other".into(),
            slug: "other".into(),
            project_path: "/project".into(),
            desired_instruction_revision: 0,
            sync_debt: None,
        },
    };
    assert_eq!(
        apply_resource_command(&c, cmd, &context(3, None))
            .unwrap_err()
            .code,
        "duplicate_physical_identity"
    );
}
#[test]
fn normal_gate_open_cannot_bypass_recovery_clearance() {
    let c = reserve(&reserve(&catalog(), "a", 2, 10), "b", 3, 10);
    let mut ctx = context(4, None);
    ctx.expected_revision = Some(c.revision);
    let c = apply_resource_command(
        &c,
        ResourceCommand::Revoke {
            reservation_id: "a".into(),
            reason: "workload uncertain".into(),
        },
        &ctx,
    )
    .unwrap()
    .catalog;
    let e = apply_resource_command(
        &c,
        ResourceCommand::SetGate {
            resource_id: "gpu".into(),
            gate: ResourceGate::Open,
        },
        &context(5, None),
    )
    .unwrap_err();
    assert_eq!(e.code, "clearance_required");
}
#[test]
fn thousand_serialized_mutations_keep_invariants() {
    let mut c = catalog();
    let mut seed = 123u64;
    for n in 2..1002 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let label = format!("agent-{}", seed % 16);
        let mut ctx = context(n, Some(&label));
        ctx.eligibility = (0..16)
            .map(|i| {
                (
                    InvocationId(format!("agent-{i}")),
                    InvocationEligibility::Live,
                )
            })
            .collect();
        let active = c
            .resources
            .values()
            .flat_map(|r| &r.reservations)
            .find(|q| q.agent.same_invocation(&actor(&label)) && q.state.active());
        let command = match active {
            None => ResourceCommand::Reserve {
                resource_id: "gpu".into(),
                reservation_id: ReservationId(format!("claim-{n}")),
                estimate_seconds: 60,
            },
            Some(q) if q.state == ReservationState::Queued => ResourceCommand::Cancel {
                reservation_id: q.id.clone(),
            },
            Some(q) => ResourceCommand::Release {
                reservation_id: q.id.clone(),
                ownership_generation: q.ownership_generation,
            },
        };
        c = apply_resource_command(&c, command, &ctx).unwrap().catalog;
        validate_resource_catalog(&c).unwrap();
    }
}
#[test]
fn malformed_catalog_multiple_owners_rejected() {
    let mut c = reserve(&reserve(&catalog(), "a", 2, 10), "b", 3, 10);
    let r = c.resources.get_mut(&"gpu".into()).unwrap();
    r.reservations[1].state = ReservationState::Owned;
    r.reservations[1].acquired_at = Some(context(3, None).clock);
    r.reservations[1].ownership_generation = 1;
    assert!(validate_resource_catalog(&c).is_err());
}
#[test]
fn active_receipt_survives_seven_day_retention() {
    let c = reserve(&catalog(), "a", 2, 10);
    let ctx = context(REQUEST_WINDOW_MILLIS + 5, Some("a"));
    let c = apply_resource_command(
        &c,
        ResourceCommand::Estimate {
            reservation_id: "a".into(),
            estimate_seconds: 60,
        },
        &ctx,
    )
    .unwrap()
    .catalog;
    assert!(c.receipts.contains_key(&RequestId("2:request".into())));
}
#[test]
fn linking_existing_binding_is_idempotent() {
    let c = catalog();
    let link = c.links[0].clone();
    let change =
        apply_resource_command(&c, ResourceCommand::Link { link }, &context(2, None)).unwrap();
    assert_eq!(change.catalog.links.len(), 1);
    assert!(change.result.reused);
}
#[test]
fn eligible_head_reconciles_after_uncertain_handoff() {
    let c = reserve(&reserve(&catalog(), "a", 2, 10), "b", 3, 10);
    let mut ctx = context(4, Some("a"));
    ctx.eligibility.clear();
    let c = apply_resource_command(
        &c,
        ResourceCommand::Release {
            reservation_id: "a".into(),
            ownership_generation: 1,
        },
        &ctx,
    )
    .unwrap()
    .catalog;
    let mut ctx = context(5, None);
    ctx.actor = ResourceActor::System;
    let c = apply_resource_command(&c, ResourceCommand::ReconcileEligibility, &ctx)
        .unwrap()
        .catalog;
    assert_eq!(
        c.resources[&"gpu".into()].reservations[1].state,
        ReservationState::Owned
    );
}

#[test]
fn slugs_follow_ascii_alphanumeric_segments() {
    for slug in ["a", "0", "board-alpha-2", &"a".repeat(64)] {
        let mut candidate = catalog();
        candidate.links[0].slug = slug.into();
        validate_resource_catalog(&candidate).unwrap();
    }
    for slug in [
        "",
        "-gpu",
        "gpu-",
        "gpu--one",
        "gpu_one",
        "GPU",
        "é",
        &"a".repeat(65),
    ] {
        let mut candidate = catalog();
        candidate.links[0].slug = slug.into();
        assert_eq!(
            validate_resource_catalog(&candidate).unwrap_err().code,
            "invalid_slug"
        );
    }
}
#[test]
fn explicit_project_rebind_preserves_claim_identity_and_queue() {
    let source = reserve(&reserve(&catalog(), "a", 2, 60), "b", 3, 60);
    let claims = source.resources[&ResourceId::from("gpu")]
        .reservations
        .clone();
    let mut mutation = context(4, None);
    mutation.expected_revision = Some(source.revision);
    let result = apply_resource_command(
        &source,
        ResourceCommand::RebindProject {
            project_id: "project".into(),
            project_path: "/moved-project".into(),
        },
        &mutation,
    )
    .unwrap();
    assert_eq!(result.catalog.links[0].project_path, "/moved-project");
    assert!(result.catalog.links[0].sync_debt.is_some());
    assert_eq!(
        result.catalog.resources[&ResourceId::from("gpu")].reservations,
        claims
    );
    assert_eq!(
        result.catalog.history.last().unwrap().kind,
        ResourceLifecycleKind::ProjectRebound
    );
    let mut stale = context(5, None);
    stale.expected_revision = Some(source.revision);
    assert_eq!(
        apply_resource_command(
            &result.catalog,
            ResourceCommand::RebindProject {
                project_id: "project".into(),
                project_path: "/another-path".into()
            },
            &stale
        )
        .unwrap_err()
        .code,
        "revision_conflict"
    );
}
#[test]
fn lifecycle_history_records_handoff_and_retry_is_silent() {
    let source = reserve(&reserve(&catalog(), "a", 2, 60), "b", 3, 60);
    let mut mutation = context(4, Some("a"));
    let command = ResourceCommand::Release {
        reservation_id: "a".into(),
        ownership_generation: 1,
    };
    let first = apply_resource_command(&source, command.clone(), &mutation).unwrap();
    assert!(first
        .catalog
        .history
        .iter()
        .any(|event| event.kind == ResourceLifecycleKind::Released
            && event.reservation_id == Some("a".into())));
    assert!(first
        .catalog
        .history
        .iter()
        .any(|event| event.kind == ResourceLifecycleKind::Acquired
            && event.reservation_id == Some("b".into())));
    let retried = apply_resource_command(&first.catalog, command, &mutation).unwrap();
    assert_eq!(retried.catalog.history, first.catalog.history);
    mutation = context(5, None);
    mutation.actor = ResourceActor::System;
    let attention = apply_resource_command(
        &first.catalog,
        ResourceCommand::ReconcileInvocation {
            agent: actor("b"),
            evidence: InvocationEligibility::Dead,
        },
        &mutation,
    )
    .unwrap();
    assert!(attention
        .catalog
        .history
        .iter()
        .any(|event| event.kind == ResourceLifecycleKind::NeedsAttention));
}
#[test]
fn lifecycle_history_prunes_per_resource_and_rejects_corruption() {
    let mut current = catalog();
    for number in 2..302 {
        current = apply_resource_command(
            &current,
            ResourceCommand::SetGate {
                resource_id: "gpu".into(),
                gate: ResourceGate::Paused {
                    reason: "Synthetic maintenance".into(),
                },
            },
            &context(number, None),
        )
        .unwrap()
        .catalog;
    }
    assert_eq!(current.history.len(), MAX_HISTORY_EVENTS_PER_RESOURCE);
    let last = current.history.last().unwrap().clone();
    current.history.push(last);
    assert_eq!(
        validate_resource_catalog(&current).unwrap_err().code,
        "corrupt_history"
    );
}
#[test]
fn lifecycle_history_global_bound_keeps_newest_events() {
    let mut source = catalog();
    let template = source.resources[&ResourceId::from("gpu")].clone();
    for number in 0..17 {
        let mut record = template.clone();
        record.definition.id = ResourceId(format!("history-{number}"));
        source
            .resources
            .insert(record.definition.id.clone(), record);
    }
    source.revision = 4096;
    source.history = (1..=4096)
        .map(|revision| ResourceLifecycleEvent {
            resource_id: ResourceId(format!("history-{}", revision % 17)),
            reservation_id: None,
            revision,
            ordinal: 0,
            unix_millis: revision as i64,
            actor: ResourceActor::User,
            kind: ResourceLifecycleKind::GateChanged,
            message: "Synthetic history fixture".into(),
        })
        .collect();
    validate_resource_catalog(&source).unwrap();
    let result = apply_resource_command(
        &source,
        ResourceCommand::SetGate {
            resource_id: "history-0".into(),
            gate: ResourceGate::Open,
        },
        &context(5000, None),
    )
    .unwrap();
    assert_eq!(result.catalog.history.len(), MAX_RESOURCE_HISTORY_EVENTS);
    assert_eq!(result.catalog.history.first().unwrap().revision, 2);
    assert_eq!(result.catalog.history.last().unwrap().revision, 4097);
}

#[test]
fn clock_jumps_never_grant_or_change_monotonic_remaining_time() {
    let c = reserve(&reserve(&catalog(), "a", 2, 600), "b", 3, 300);
    let before = c.clone();
    for wall in [0, 120002, i64::MAX] {
        let mut clock = context(120002, None).clock;
        clock.unix_millis = wall;
        let view = project_resource_queue(&c.resources[&"gpu".into()], &clock);
        assert_eq!(view.owner.as_ref().unwrap().id, ReservationId::from("a"));
        assert_eq!(view.queue[0].estimate.wait_seconds, Some(480));
        if wall == i64::MAX {
            assert_eq!(view.queue[0].estimate.estimated_start_unix_millis, None);
        }
        assert_eq!(c, before);
    }
    let mut clock = context(1, None).clock;
    clock.monotonic_millis = 1;
    let view = project_resource_queue(&c.resources[&"gpu".into()], &clock);
    assert_eq!(view.queue[0].estimate.wait_seconds, None);
    assert!(view.observation_reason.unwrap().contains("regressed"));
}

#[test]
fn cancellation_keeps_following_tickets_and_fifo_positions() {
    let c = reserve(
        &reserve(&reserve(&catalog(), "a", 2, 600), "b", 3, 300),
        "c",
        4,
        60,
    );
    let mut fourth = context(5, Some("d"));
    fourth
        .eligibility
        .insert("d".into(), InvocationEligibility::Live);
    let c = apply_resource_command(
        &c,
        ResourceCommand::Reserve {
            resource_id: "gpu".into(),
            reservation_id: "d".into(),
            estimate_seconds: 30,
        },
        &fourth,
    )
    .unwrap()
    .catalog;
    let before = c.resources[&"gpu".into()]
        .reservations
        .iter()
        .map(|q| (q.id.clone(), q.ticket))
        .collect::<BTreeMap<_, _>>();
    let mut ctx = context(6, Some("c"));
    ctx.eligibility
        .insert("d".into(), InvocationEligibility::Live);
    let c = apply_resource_command(
        &c,
        ResourceCommand::Cancel {
            reservation_id: "c".into(),
        },
        &ctx,
    )
    .unwrap()
    .catalog;
    let view = project_resource_queue(&c.resources[&"gpu".into()], &ctx.clock);
    assert_eq!(
        view.queue
            .iter()
            .map(|q| (q.reservation.id.clone(), q.position))
            .collect::<Vec<_>>(),
        vec![("b".into(), 1), ("d".into(), 2)]
    );
    for entry in view.queue {
        assert_eq!(entry.reservation.ticket, before[&entry.reservation.id]);
    }
}

#[test]
fn a_new_reservation_cannot_bypass_an_uncertain_head() {
    let c = reserve(&reserve(&catalog(), "a", 2, 600), "b", 3, 300);
    let mut ctx = context(4, Some("a"));
    ctx.eligibility
        .insert("b".into(), InvocationEligibility::Unknown);
    let c = apply_resource_command(
        &c,
        ResourceCommand::Release {
            reservation_id: "a".into(),
            ownership_generation: 1,
        },
        &ctx,
    )
    .unwrap()
    .catalog;
    let mut ctx = context(5, Some("c"));
    ctx.eligibility
        .insert("b".into(), InvocationEligibility::Unknown);
    let next = apply_resource_command(
        &c,
        ResourceCommand::Reserve {
            resource_id: "gpu".into(),
            reservation_id: "c".into(),
            estimate_seconds: 60,
        },
        &ctx,
    )
    .unwrap()
    .catalog;
    let view = project_resource_queue(&next.resources[&"gpu".into()], &ctx.clock);
    assert!(view.owner.is_none());
    assert_eq!(
        view.queue
            .iter()
            .map(|q| q.reservation.id.clone())
            .collect::<Vec<_>>(),
        vec!["b".into(), "c".into()]
    );
    assert!(view.queue.iter().all(|q| q.estimate.wait_seconds.is_none()));
}

#[test]
fn retry_window_edges_never_turn_an_old_request_into_new_work() {
    let c = reserve(&catalog(), "a", 2, 600);
    let command = ResourceCommand::Reserve {
        resource_id: "gpu".into(),
        reservation_id: "a".into(),
        estimate_seconds: 600,
    };
    let mut ctx = context(2, Some("a"));
    for age in [REQUEST_WINDOW_MILLIS - 1, REQUEST_WINDOW_MILLIS] {
        ctx.clock.unix_millis = 2 + age;
        let replay = apply_resource_command(&c, command.clone(), &ctx).unwrap();
        assert!(replay.result.reused);
        assert_eq!(replay.catalog, c);
    }
    ctx.clock.unix_millis = 3 + REQUEST_WINDOW_MILLIS;
    assert_eq!(
        apply_resource_command(&c, command.clone(), &ctx)
            .unwrap_err()
            .code,
        "idempotency_expired"
    );
    ctx.clock.unix_millis = 0;
    ctx.request_id = RequestId("300001:future".into());
    assert_eq!(
        apply_resource_command(&c, command, &ctx).unwrap_err().code,
        "invalid_request_id"
    );
}

#[test]
fn terminal_release_and_cancel_retries_expire_safely_after_receipt_pruning() {
    for cancel in [false, true] {
        let initial = reserve(&reserve(&catalog(), "a", 2, 600), "b", 3, 60);
        let (command, agent, terminal_id) = if cancel {
            (
                ResourceCommand::Cancel {
                    reservation_id: "b".into(),
                },
                "b",
                "b",
            )
        } else {
            let generation = initial.resources[&"gpu".into()].reservations[0].ownership_generation;
            (
                ResourceCommand::Release {
                    reservation_id: "a".into(),
                    ownership_generation: generation,
                },
                "a",
                "a",
            )
        };
        let original_context = context(4, Some(agent));
        let committed =
            apply_resource_command(&initial, command.clone(), &original_context).unwrap();
        for age in [REQUEST_WINDOW_MILLIS - 1, REQUEST_WINDOW_MILLIS] {
            let mut retry = original_context.clone();
            retry.clock.unix_millis += age;
            let replay =
                apply_resource_command(&committed.catalog, command.clone(), &retry).unwrap();
            assert!(replay.result.reused);
            assert_eq!(replay.catalog, committed.catalog);
            assert!(replay.events.is_empty());
            assert_eq!(
                apply_resource_command(
                    &committed.catalog,
                    ResourceCommand::Estimate {
                        reservation_id: terminal_id.into(),
                        estimate_seconds: 90,
                    },
                    &retry
                )
                .unwrap_err()
                .code,
                "idempotency_conflict"
            );
        }
        let now = 5 + REQUEST_WINDOW_MILLIS;
        let mut maintenance = context(now, None);
        maintenance.expected_revision = Some(committed.catalog.revision);
        let pruned = apply_resource_command(
            &committed.catalog,
            ResourceCommand::SetGate {
                resource_id: "gpu".into(),
                gate: ResourceGate::Unavailable {
                    reason: "fixture maintenance".into(),
                },
            },
            &maintenance,
        )
        .unwrap()
        .catalog;
        assert!(!pruned.receipts.contains_key(&original_context.request_id));
        let mut expired = original_context;
        expired.clock.unix_millis = now;
        assert_eq!(
            apply_resource_command(&pruned, command, &expired)
                .unwrap_err()
                .code,
            "idempotency_expired"
        );
        assert!(pruned.resources[&"gpu".into()]
            .reservations
            .iter()
            .any(|claim| claim.state == ReservationState::Owned));
    }
}

#[test]
fn malformed_catalog_and_counter_overflow_refuse_without_mutation() {
    let c = reserve(&reserve(&catalog(), "a", 2, 600), "b", 3, 300);
    let mut malformed = c.clone();
    malformed.schema_version = 999;
    assert!(validate_resource_catalog(&malformed).is_err());
    let mut malformed = c.clone();
    malformed.links.clear();
    assert!(validate_resource_catalog(&malformed).is_err());
    let mut malformed = c.clone();
    let record = malformed.resources.get_mut(&"gpu".into()).unwrap();
    record.reservations[1].ticket = record.reservations[0].ticket;
    assert!(validate_resource_catalog(&malformed).is_err());
    let mut malformed = c.clone();
    malformed
        .resources
        .get_mut(&"gpu".into())
        .unwrap()
        .reservations[0]
        .estimate_seconds = MAX_ESTIMATE_SECONDS + 1;
    assert!(validate_resource_catalog(&malformed).is_err());
    for revision_overflow in [false, true] {
        let mut saturated = c.clone();
        if revision_overflow {
            saturated.revision = u64::MAX;
        } else {
            saturated
                .resources
                .get_mut(&"gpu".into())
                .unwrap()
                .next_ticket = u64::MAX;
        }
        let unchanged = saturated.clone();
        assert!(apply_resource_command(
            &saturated,
            ResourceCommand::Reserve {
                resource_id: "gpu".into(),
                reservation_id: "c".into(),
                estimate_seconds: 60
            },
            &context(4, Some("c"))
        )
        .is_err());
        assert_eq!(saturated, unchanged);
    }
}

#[test]
fn definition_and_link_timestamps_are_authoritative_and_retry_stable() {
    let template = catalog();
    let mut definition = template.resources[&ResourceId::from("gpu")]
        .definition
        .clone();
    definition.created_at_unix_millis = -700;
    definition.updated_at_unix_millis = -900;
    let mut link = template.links[0].clone();
    link.created_at_unix_millis = -100;
    let created = apply_resource_command(
        &ResourceCatalog::default(),
        ResourceCommand::Create { definition, link },
        &context(100, None),
    )
    .unwrap()
    .catalog;
    let definition = &created.resources[&ResourceId::from("gpu")].definition;
    assert_eq!(definition.policy, ResourcePolicy::ExclusiveFifo);
    assert_eq!(
        (
            definition.created_at_unix_millis,
            definition.updated_at_unix_millis
        ),
        (100, 100)
    );
    assert_eq!(created.links[0].created_at_unix_millis, 100);
    let mut edited = definition.clone();
    edited.title = "Updated timestamp fixture".into();
    edited.created_at_unix_millis = 99999;
    edited.updated_at_unix_millis = -1;
    let command = ResourceCommand::Edit { definition: edited };
    let edit_context = context(200, None);
    let updated = apply_resource_command(&created, command.clone(), &edit_context)
        .unwrap()
        .catalog;
    let definition = &updated.resources[&ResourceId::from("gpu")].definition;
    assert_eq!(
        (
            definition.created_at_unix_millis,
            definition.updated_at_unix_millis
        ),
        (100, 200)
    );
    let mut second_link = updated.links[0].clone();
    second_link.project_id = "second-project".into();
    second_link.project_path = "/second-project".into();
    second_link.created_at_unix_millis = -1;
    let link_command = ResourceCommand::Link { link: second_link };
    let link_context = context(300, None);
    let linked = apply_resource_command(&updated, link_command.clone(), &link_context)
        .unwrap()
        .catalog;
    assert_eq!(linked.links[1].created_at_unix_millis, 300);
    assert_eq!(
        apply_resource_command(&linked, link_command.clone(), &link_context)
            .unwrap()
            .catalog,
        linked
    );
    let relinked = apply_resource_command(&linked, link_command, &context(400, None))
        .unwrap()
        .catalog;
    assert_eq!(
        relinked.links[1].created_at_unix_millis, 300,
        "new-key duplicate link preserves creation time"
    );
    let mut move_context = context(500, None);
    move_context.expected_revision = Some(relinked.revision);
    let moved = apply_resource_command(
        &relinked,
        ResourceCommand::RebindProject {
            project_id: "project".into(),
            project_path: "/moved-project".into(),
        },
        &move_context,
    )
    .unwrap()
    .catalog;
    assert_eq!(moved.links[0].created_at_unix_millis, 100);
    assert_eq!(moved.links[1].created_at_unix_millis, 300);
    assert_eq!(
        apply_resource_command(&moved, command, &edit_context)
            .unwrap()
            .catalog,
        moved,
        "late retry cannot rewrite timestamps or definitions"
    );
}
#[test]
fn persisted_policy_and_timestamps_are_strict() {
    let original = catalog();
    for (created, updated) in [(-1, 1), (2, 1)] {
        let mut malformed = original.clone();
        let definition = &mut malformed
            .resources
            .get_mut(&ResourceId::from("gpu"))
            .unwrap()
            .definition;
        definition.created_at_unix_millis = created;
        definition.updated_at_unix_millis = updated;
        assert_eq!(
            validate_resource_catalog(&malformed).unwrap_err().code,
            "invalid_timestamp"
        );
    }
    let mut malformed = original.clone();
    malformed.links[0].created_at_unix_millis = -1;
    assert_eq!(
        validate_resource_catalog(&malformed).unwrap_err().code,
        "invalid_timestamp"
    );
    let unknown = serde::de::value::StrDeserializer::<serde::de::value::Error>::new("SharedFifo");
    assert!(<ResourcePolicy as serde::Deserialize>::deserialize(unknown).is_err());
}
#[test]
fn wall_clock_rollback_cannot_reverse_definition_update_time() {
    let initial = catalog();
    let mut definition = initial.resources[&ResourceId::from("gpu")]
        .definition
        .clone();
    definition.title = "Later edit".into();
    let later = apply_resource_command(
        &initial,
        ResourceCommand::Edit { definition },
        &context(100, None),
    )
    .unwrap()
    .catalog;
    let mut definition = later.resources[&ResourceId::from("gpu")].definition.clone();
    definition.title = "Clock rollback edit".into();
    let earlier = apply_resource_command(
        &later,
        ResourceCommand::Edit { definition },
        &context(50, None),
    )
    .unwrap()
    .catalog;
    assert_eq!(
        earlier.resources[&ResourceId::from("gpu")]
            .definition
            .updated_at_unix_millis,
        100
    );
    validate_resource_catalog(&earlier).unwrap();
}
fn recover_stale(c: &ResourceCatalog, agent: Option<&str>) -> Result<ResourceCatalog, String> {
    let mut ctx = context(9, agent);
    ctx.expected_revision = Some(c.revision);
    apply_resource_command(
        c,
        ResourceCommand::RecoverStaleOwner {
            resource_id: "gpu".into(),
            reason: "Checked no workload uses the board".into(),
        },
        &ctx,
    )
    .map(|t| t.catalog)
    .map_err(|e| e.code)
}
fn with_dead_owner() -> ResourceCatalog {
    let c = reserve(&reserve(&catalog(), "a", 2, 10), "b", 3, 10);
    let mut ctx = context(4, None);
    ctx.actor = ResourceActor::System;
    apply_resource_command(
        &c,
        ResourceCommand::ReconcileInvocation {
            agent: actor("a"),
            evidence: InvocationEligibility::Dead,
        },
        &ctx,
    )
    .unwrap()
    .catalog
}
#[test]
fn agent_clears_dead_owner_in_one_step_and_next_waiter_is_promoted() {
    let c = with_dead_owner();
    assert!(matches!(
        c.resources[&"gpu".into()].reservations[0].state,
        ReservationState::NeedsAttention { .. }
    ));
    let c = recover_stale(&c, Some("b")).unwrap();
    let r = &c.resources[&"gpu".into()];
    assert!(matches!(
        r.reservations[0].state,
        ReservationState::Revoked { .. }
    ));
    assert_eq!(r.reservations[1].state, ReservationState::Owned);
    assert!(!r.clearance_required);
}
#[test]
fn agent_cannot_clear_live_owner_or_act_as_system() {
    let live = reserve(&catalog(), "a", 2, 10);
    assert_eq!(recover_stale(&live, Some("b")).unwrap_err(), "owner_live");
    let stale = with_dead_owner();
    let mut ctx = context(9, None);
    ctx.actor = ResourceActor::System;
    ctx.expected_revision = Some(stale.revision);
    let denied = apply_resource_command(
        &stale,
        ResourceCommand::RecoverStaleOwner {
            resource_id: "gpu".into(),
            reason: "x".into(),
        },
        &ctx,
    )
    .unwrap_err();
    assert_eq!(denied.code, "agent_or_user_required");
}
#[test]
fn stale_recovery_needs_a_stale_owner_and_a_fresh_revision() {
    assert_eq!(
        recover_stale(&catalog(), Some("b")).unwrap_err(),
        "nothing_to_recover"
    );
    let stale = with_dead_owner();
    let mut ctx = context(9, Some("b"));
    ctx.expected_revision = Some(stale.revision - 1);
    let err = apply_resource_command(
        &stale,
        ResourceCommand::RecoverStaleOwner {
            resource_id: "gpu".into(),
            reason: "x".into(),
        },
        &ctx,
    )
    .unwrap_err();
    assert_eq!(err.code, "revision_conflict");
}
