//! Native pure fixtures; no helper, network, user storage or runtime activation.
use super::*;
use ilium_animation_js::{
    package::PackageLimits,
    permissions::{
        Capability as NativeCapability, Ceiling, PermissionBroker, PermissionPlan,
        PermissionRequest as NativeRequest, Right, Scope,
    },
    trust::TrustVerifier,
};
use ilium_execution::QuotaLimits;
fn quota(bytes: usize) -> QuotaGroup {
    QuotaGroup::new(QuotaLimits {
        clients: 0,
        jobs: 0,
        service_jobs: 0,
        input_bytes: 0,
        result_bytes: 0,
        worker_threads: 0,
        worker_bytes: bytes,
    })
}
fn fixture() -> (Package, PackageIdentity, PermissionBroker) {
    let package = Package::from_bytes(
        include_bytes!("../../../ilium-animation-js/assets/packages/carpet-1.0.0.iliumanim"),
        PackageLimits::default(),
    )
    .unwrap();
    let principal = TrustVerifier::from_release_inventory(vec![])
        .unwrap()
        .permission_identity(&package)
        .unwrap();
    let right = Right {
        id: NativeCapability::InputPointer,
        scope: Scope::AnimationViewport,
    };
    let ceiling = Ceiling {
        permissions: vec![right],
    };
    let broker = PermissionBroker::new(principal.clone(), ceiling.clone(), ceiling).unwrap();
    (package, principal, broker)
}
fn review(broker: &mut PermissionBroker, revision: u64) -> PlanReview {
    broker
        .prepare(
            7,
            revision,
            PermissionPlan {
                permissions: vec![NativeRequest {
                    request_id: Some("pointer".into()),
                    id: NativeCapability::InputPointer,
                    scope: Scope::AnimationViewport,
                    required: false,
                    reason: "Fixture-local pointer".into(),
                }],
                demands: vec![],
            },
            BTreeMap::new(),
        )
        .unwrap()
}
fn bridge(quota: QuotaGroup) -> Arc<ReviewBridge> {
    ReviewBridge::new(quota, Arc::new(Notify::new()), Box::new(|| {})).unwrap()
}
#[test]
fn empty_native_plan_resolves_without_an_interactive_permission_screen() {
    let (package, principal, mut broker) = fixture();
    let bridge = bridge(quota(2 * REVIEW_BYTES));
    bridge.select(1, true).unwrap();
    let native = broker
        .prepare(
            7,
            1,
            PermissionPlan {
                permissions: vec![],
                demands: vec![],
            },
            BTreeMap::new(),
        )
        .unwrap();
    let epoch = native.authorization_epoch();
    bridge.publish(1, &package, &principal, native).unwrap();
    assert!(bridge.session().unwrap().is_none());
    assert_eq!(bridge.status().unwrap().phase(), ReviewPhase::Resolving);
    let Some(ReviewAction::Resolve {
        review: original_review,
        answers,
        ..
    }) = bridge.take_action(1, 7, 1, epoch).unwrap()
    else {
        panic!("empty original native plan must reach the existing resolution owner");
    };
    assert!(original_review.items().is_empty());
    assert_eq!(original_review.instance_id(), 7);
    assert!(answers.is_empty());
    assert!(bridge.take_action(1, 7, 1, epoch).unwrap().is_none());

    // An actual requested right still requires the original interactive consent.
    bridge.select(2, true).unwrap();
    bridge
        .publish(2, &package, &principal, review(&mut broker, 2))
        .unwrap();
    let mut session = bridge.session().unwrap().unwrap();
    assert!(session.handle_key(&bridge, KeyCode::Enter).is_err());
}
#[test]
fn denied_unresolved_default_and_all_four_exact_native_choices() {
    for choice in PermissionChoice::ALL {
        let (package, principal, mut broker) = fixture();
        let bridge = bridge(quota(2 * REVIEW_BYTES));
        bridge.select(1, true).unwrap();
        let native = review(&mut broker, 1);
        let epoch = native.authorization_epoch();
        bridge.publish(1, &package, &principal, native).unwrap();
        let mut session = bridge.session().unwrap().unwrap();
        assert_eq!(session.view.choice(0), Some(PermissionChoice::DenySession));
        assert!(session.handle_key(&bridge, KeyCode::Enter).is_err());
        session.view.decide(0, choice).unwrap();
        session.handle_key(&bridge, KeyCode::Enter).unwrap();
        match bridge.take_action(1, 7, 1, epoch).unwrap().unwrap() {
            ReviewAction::Resolve {
                review, answers, ..
            } => {
                assert_eq!(review.instance_id(), 7);
                assert_eq!(answers["pointer"], choice.native_intent());
            }
            ReviewAction::Cancel { .. } => panic!("unexpected cancel"),
            ReviewAction::Pick { .. } => panic!("unexpected picker"),
            ReviewAction::PickAudio { .. } => panic!("unexpected audio picker"),
        }
    }
}
#[test]
fn path_input_queues_only_current_needs_selection_disk_right() {
    let (package, principal, _) = fixture();
    assert!(principal.principal_key().starts_with("unsigned:"));
    let right = Right {
        id: NativeCapability::DiskRead,
        scope: Scope::Disk {
            slot: "pictures".into(),
            selection: Selection::Folder,
        },
    };
    let ceiling = Ceiling {
        permissions: vec![right],
    };
    let mut broker = PermissionBroker::new(principal.clone(), ceiling.clone(), ceiling).unwrap();
    let native = broker
        .prepare(
            7,
            1,
            PermissionPlan {
                permissions: vec![NativeRequest {
                    request_id: Some("pictures".into()),
                    id: NativeCapability::DiskRead,
                    scope: Scope::Disk {
                        slot: "pictures".into(),
                        selection: Selection::Folder,
                    },
                    required: true,
                    reason: "Fixture selected directory".into(),
                }],
                demands: vec![],
            },
            BTreeMap::new(),
        )
        .unwrap();
    assert_eq!(native.items()[0].verdict, Verdict::NeedsSelection);
    let epoch = native.authorization_epoch();
    let bridge = bridge(quota(2 * REVIEW_BYTES));
    bridge.select(1, true).unwrap();
    bridge.publish(1, &package, &principal, native).unwrap();
    let mut session = bridge.session().unwrap().unwrap();
    assert!(session.handle_key(&bridge, KeyCode::Char('1')).is_err());
    session.handle_key(&bridge, KeyCode::Char('p')).unwrap();
    for character in "/tmp/fixture".chars() {
        session
            .handle_key(&bridge, KeyCode::Char(character))
            .unwrap();
    }
    session.handle_key(&bridge, KeyCode::Enter).unwrap();
    match bridge.take_action(1, 7, 1, epoch).unwrap().unwrap() {
        ReviewAction::Pick {
            request_id,
            path,
            slot,
            disk_selection,
            writable,
            review_revision,
            authorization_epoch,
            ..
        } => {
            assert_eq!(request_id, "pictures");
            assert_eq!(path, "/tmp/fixture");
            assert_eq!(slot, "pictures");
            assert_eq!(disk_selection, Selection::Folder);
            assert!(!writable);
            assert_eq!(review_revision, 1);
            assert_eq!(authorization_epoch, epoch);
        }
        _ => panic!("expected native picker intent"),
    }
}
#[test]
fn selected_audio_source_uses_current_native_right_and_review_fence() {
    let (package, principal, _) = fixture();
    assert!(principal.principal_key().starts_with("unsigned:"));
    let scope = Scope::Audio {
        device: "microphone".into(),
        products: std::collections::BTreeSet::from([
            ilium_animation_js::permissions::AudioProduct::Level,
        ]),
    };
    let right = Right {
        id: NativeCapability::AudioMicrophone,
        scope: scope.clone(),
    };
    let ceiling = Ceiling {
        permissions: vec![right],
    };
    let mut broker = PermissionBroker::new(principal.clone(), ceiling.clone(), ceiling).unwrap();
    let native = broker
        .prepare(
            7,
            1,
            PermissionPlan {
                permissions: vec![NativeRequest {
                    request_id: Some("mic".into()),
                    id: NativeCapability::AudioMicrophone,
                    scope,
                    required: true,
                    reason: "Select exact fixture endpoint".into(),
                }],
                demands: vec![],
            },
            BTreeMap::new(),
        )
        .unwrap();
    assert_eq!(native.items()[0].verdict, Verdict::NeedsSelection);
    let epoch = native.authorization_epoch();
    let bridge = bridge(quota(2 * REVIEW_BYTES));
    bridge.select(1, true).unwrap();
    bridge.publish(1, &package, &principal, native).unwrap();
    let mut session = bridge.session().unwrap().unwrap();
    assert!(session.handle_key(&bridge, KeyCode::Char('1')).is_err());
    session.handle_key(&bridge, KeyCode::Char('p')).unwrap();
    for character in "alsa_input.fixture".chars() {
        session
            .handle_key(&bridge, KeyCode::Char(character))
            .unwrap();
    }
    session.handle_key(&bridge, KeyCode::Enter).unwrap();
    match bridge.take_action(1, 7, 1, epoch).unwrap().unwrap() {
        ReviewAction::PickAudio {
            request_id,
            endpoint,
            scope_device,
            capability,
            review_revision,
            authorization_epoch,
            ..
        } => {
            assert_eq!(request_id, "mic");
            assert_eq!(endpoint, "alsa_input.fixture");
            assert_eq!(scope_device, "microphone");
            assert_eq!(capability, NativeCapability::AudioMicrophone);
            assert_eq!(review_revision, 1);
            assert_eq!(authorization_epoch, epoch);
        }
        _ => panic!("expected exact native audio selection"),
    }
}
#[test]
fn stale_epoch_selection_and_queued_duplicate_intents_never_resolve() {
    let (package, principal, mut broker) = fixture();
    let bridge = bridge(quota(4 * REVIEW_BYTES));
    bridge.select(1, true).unwrap();
    let native = review(&mut broker, 1);
    let epoch = native.authorization_epoch();
    bridge.publish(1, &package, &principal, native).unwrap();
    let mut session = bridge.session().unwrap().unwrap();
    session
        .view
        .decide(0, PermissionChoice::AllowRemembered)
        .unwrap();
    session.handle_key(&bridge, KeyCode::Enter).unwrap();
    assert!(session.handle_key(&bridge, KeyCode::Enter).is_err());
    assert!(bridge.take_action(2, 7, 1, epoch).is_err());
    let _native_invalidation = broker
        .revoke(Right {
            id: NativeCapability::InputPointer,
            scope: Scope::AnimationViewport,
        })
        .unwrap();
    assert!(broker.authorization_epoch() > epoch);
    assert!(bridge
        .take_action(1, 7, 1, broker.authorization_epoch())
        .is_err());
    assert!(!bridge.has_intent());
    assert!(!bridge.is_current(&session));
    bridge.select(2, true).unwrap();
    assert!(session.handle_key(&bridge, KeyCode::Enter).is_err());
}
#[test]
fn one_session_lease_and_last_escaped_snapshot_keep_original_root_charge() {
    let (package, principal, mut broker) = fixture();
    let quota = quota(4 * REVIEW_BYTES);
    let bridge = bridge(quota.clone());
    bridge.select(1, true).unwrap();
    bridge
        .publish(1, &package, &principal, review(&mut broker, 1))
        .unwrap();
    let session = bridge.session().unwrap().unwrap();
    assert!(bridge.session().unwrap().is_none());
    let escaped = session.envelope.clone();
    let status = bridge.status().unwrap();
    bridge.select(2, false).unwrap();
    drop(bridge);
    drop(session);
    assert_eq!(quota.snapshot().worker_bytes, REVIEW_BYTES + STATUS_BYTES);
    drop(escaped);
    assert_eq!(quota.snapshot().worker_bytes, STATUS_BYTES);
    drop(status);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}
#[test]
fn original_root_refusal_and_wrong_verified_archive_do_not_publish_partial_reviews() {
    let (package, principal, mut broker) = fixture();
    let quota = quota(MAILBOX_BYTES + REVIEW_BYTES - 1);
    let bridge = bridge(quota.clone());
    bridge.select(1, true).unwrap();
    assert!(bridge
        .publish(1, &package, &principal, review(&mut broker, 1))
        .is_err());
    assert!(bridge.session().unwrap().is_none());
    assert_eq!(quota.snapshot().worker_bytes, MAILBOX_BYTES);
    let foreign = PackageIdentity::unverified("carpet".into(), b"foreign").unwrap();
    drop(bridge);
    assert_eq!(quota.snapshot().worker_bytes, 0);
    let bridge = self::bridge(self::quota(2 * REVIEW_BYTES));
    bridge.select(1, true).unwrap();
    assert!(bridge
        .publish(1, &package, &foreign, review(&mut broker, 2))
        .is_err());
    assert!(bridge.session().unwrap().is_none());
}
#[test]
fn mouse_choice_shared_geometry_and_cancel_return_original_native_action() {
    let (package, principal, mut broker) = fixture();
    let bridge = bridge(quota(2 * REVIEW_BYTES));
    bridge.select(1, true).unwrap();
    let native = review(&mut broker, 1);
    let epoch = native.authorization_epoch();
    bridge.publish(1, &package, &principal, native).unwrap();
    let mut session = bridge.session().unwrap().unwrap();
    let area = Rect::new(10, 20, 70, 18);
    let rect = permissions::permission_choice_rect(area, PermissionChoice::DenyRemembered).unwrap();
    session
        .handle_mouse(
            &bridge,
            area,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: rect.x,
                row: rect.y,
                modifiers: crossterm::event::KeyModifiers::NONE,
            },
        )
        .unwrap();
    assert_eq!(
        session.view.choice(0),
        Some(PermissionChoice::DenyRemembered)
    );
    session.handle_key(&bridge, KeyCode::Esc).unwrap();
    assert!(matches!(
        bridge.take_action(1, 7, 1, epoch).unwrap(),
        Some(ReviewAction::Cancel {
            selection_revision: 1
        })
    ));
}

#[test]
fn protected_render_keeps_host_scope_choices_and_submit_geometry() {
    use ratatui::{backend::TestBackend, Terminal};
    let (package, principal, mut broker) = fixture();
    let bridge = bridge(quota(2 * REVIEW_BYTES));
    bridge.select(1, true).unwrap();
    bridge
        .publish(1, &package, &principal, review(&mut broker, 1))
        .unwrap();
    let mut session = bridge.session().unwrap().unwrap();
    let area = Rect::new(0, 0, 90, 18);
    let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
    terminal
        .draw(|frame| session.draw(frame, area, Style::default()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let all: String = (0..area.height)
        .flat_map(|y| (0..area.width).map(move |x| buffer[(x, y)].symbol()))
        .collect();
    assert!(all.contains("Unverified package"));
    assert!(all.contains("Animation viewport only"));
    assert!(all.contains("No consent recorded"));
    for choice in PermissionChoice::ALL {
        let rect = permissions::permission_choice_rect(area, choice).unwrap();
        let row: String = (rect.x..rect.right())
            .map(|x| buffer[(x, rect.y)].symbol())
            .collect();
        assert_eq!(row.trim_end(), choice.label());
        assert_eq!(
            permissions::permission_choice_at(area, Position::new(rect.x, rect.y)),
            Some(choice)
        );
    }
    let footer = review_submit_rect(area).unwrap();
    let row: String = (footer.x..footer.right())
        .map(|x| buffer[(x, footer.y)].symbol())
        .collect();
    assert!(row.contains("Enter submits"));
}
#[test]
fn unicode_status_snapshot_admission_refuses_before_copy_and_retains_its_owner() {
    let original = quota(MAILBOX_BYTES + STATUS_BYTES - 1);
    let bridge = bridge(original.clone());
    bridge.select(1, true).unwrap();
    bridge
        .set_phase(1, ReviewPhase::Failed, Some(&"😀".repeat(1000)))
        .unwrap();
    assert!(bridge.status().is_err());
    assert_eq!(original.snapshot().worker_bytes, MAILBOX_BYTES);
    drop(bridge);
    assert_eq!(original.snapshot().worker_bytes, 0);
    let original = quota(MAILBOX_BYTES + STATUS_BYTES);
    let bridge = self::bridge(original.clone());
    bridge.select(1, true).unwrap();
    bridge
        .set_phase(1, ReviewPhase::Failed, Some(&"😀".repeat(1000)))
        .unwrap();
    let snapshot = bridge.status().unwrap();
    assert_eq!(snapshot.message().chars().count(), 240);
    assert_eq!(snapshot.message().len(), 960);
    drop(bridge);
    assert_eq!(original.snapshot().worker_bytes, STATUS_BYTES);
    drop(snapshot);
    assert_eq!(original.snapshot().worker_bytes, 0);
}
