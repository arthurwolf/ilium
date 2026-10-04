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
        }
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
