#![cfg(target_os = "linux")]
use ilium_core::polling_detection::PollingSettings;
use ilium_ipc::polling::PollingEvidenceSubscription;
use ilium_platform::file_lock::ExclusiveFileLock;
use ilium_server::polling_auth::{AuthenticatedDetector, FeedAdmission};

#[test]
fn authenticated_feed_stops_when_opt_out_or_lease_release_revokes_authority() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("detector.lock");
    let held = ExclusiveFileLock::acquire(&path).unwrap();
    let settings = PollingSettings {
        enabled: true,
        ..Default::default()
    };
    let subscription = PollingEvidenceSubscription {
        expected_server_incarnation: "current-server".into(),
        leader_epoch: "current-leader".into(),
    };
    std::fs::write(&path, subscription.leader_epoch.as_bytes()).unwrap();
    let admitted = AuthenticatedDetector::admit(FeedAdmission {
        lease_path: &path,
        peer_process_id: Some(std::process::id()),
        subscription: &subscription,
        serving_incarnation: "current-server",
        settings: &settings,
    })
    .unwrap()
    .expect("the live lease owner is admitted");
    assert!(admitted.still_authorized(&settings).unwrap());
    assert!(!admitted
        .still_authorized(&PollingSettings::default())
        .unwrap());
    drop(held);
    assert!(!admitted.still_authorized(&settings).unwrap());
    let _replacement = ExclusiveFileLock::acquire(&path).unwrap();
    std::fs::write(&path, "replacement-leader").unwrap();
    assert!(!admitted.still_authorized(&settings).unwrap());
}

#[test]
fn stale_incarnation_missing_peer_and_disabled_capture_are_never_admitted() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("detector.lock");
    let _held = ExclusiveFileLock::acquire(&path).unwrap();
    let enabled = PollingSettings {
        enabled: true,
        ..Default::default()
    };
    let disabled = PollingSettings::default();
    let subscription = PollingEvidenceSubscription {
        expected_server_incarnation: "current-server".into(),
        leader_epoch: "current-leader".into(),
    };
    std::fs::write(&path, subscription.leader_epoch.as_bytes()).unwrap();
    for (peer, incarnation, settings) in [
        (None, "current-server", &enabled),
        (Some(0), "current-server", &enabled),
        (Some(std::process::id()), "replaced-server", &enabled),
        (Some(std::process::id()), "current-server", &disabled),
    ] {
        assert!(AuthenticatedDetector::admit(FeedAdmission {
            lease_path: &path,
            peer_process_id: peer,
            subscription: &subscription,
            serving_incarnation: incarnation,
            settings,
        })
        .unwrap()
        .is_none());
    }
}

#[test]
fn a_request_cannot_choose_an_epoch_different_from_the_held_lease() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("detector.lock");
    let _held = ExclusiveFileLock::acquire(&path).unwrap();
    std::fs::write(&path, "actual-leader-epoch").unwrap();
    let settings = PollingSettings {
        enabled: true,
        ..Default::default()
    };
    let subscription = PollingEvidenceSubscription {
        expected_server_incarnation: "current-server".into(),
        leader_epoch: "claimed-other-epoch".into(),
    };
    assert!(AuthenticatedDetector::admit(FeedAdmission {
        lease_path: &path,
        peer_process_id: Some(std::process::id()),
        subscription: &subscription,
        serving_incarnation: "current-server",
        settings: &settings,
    })
    .unwrap()
    .is_none());
}

#[test]
fn ledger_publishes_a_fresh_epoch_for_each_exclusive_owner() {
    use ilium_server::polling_ledger::Ledger;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("polling-detector.lock");
    let ledger = Ledger::open(root.path()).unwrap();
    let settings = PollingSettings {
        enabled: true,
        ..Default::default()
    };
    let subscription = PollingEvidenceSubscription {
        expected_server_incarnation: "current-server".into(),
        leader_epoch: ledger.leader_epoch().into(),
    };
    let admitted = AuthenticatedDetector::admit(FeedAdmission {
        lease_path: &path,
        peer_process_id: Some(std::process::id()),
        subscription: &subscription,
        serving_incarnation: "current-server",
        settings: &settings,
    })
    .unwrap()
    .expect("the actual ledger owner publishes its authenticated lease epoch");
    assert!(admitted.still_authorized(&settings).unwrap());
    drop(ledger);
    let replacement = Ledger::open(root.path()).unwrap();
    assert_ne!(replacement.leader_epoch(), subscription.leader_epoch);
    assert!(!admitted.still_authorized(&settings).unwrap());
    let replacement_subscription = PollingEvidenceSubscription {
        leader_epoch: replacement.leader_epoch().into(),
        ..subscription
    };
    assert!(AuthenticatedDetector::admit(FeedAdmission {
        lease_path: &path,
        peer_process_id: Some(std::process::id()),
        subscription: &replacement_subscription,
        serving_incarnation: "current-server",
        settings: &settings,
    })
    .unwrap()
    .is_some());
}

#[test]
fn authenticated_subscription_cannot_follow_a_replaced_lease_with_a_copied_epoch() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("detector.lock");
    let _held = ExclusiveFileLock::acquire(&path).unwrap();
    let settings = PollingSettings {
        enabled: true,
        ..Default::default()
    };
    let subscription = PollingEvidenceSubscription {
        expected_server_incarnation: "current-server".into(),
        leader_epoch: "current-leader".into(),
    };
    std::fs::write(&path, subscription.leader_epoch.as_bytes()).unwrap();
    let admitted = AuthenticatedDetector::admit(FeedAdmission {
        lease_path: &path,
        peer_process_id: Some(std::process::id()),
        subscription: &subscription,
        serving_incarnation: "current-server",
        settings: &settings,
    })
    .unwrap()
    .unwrap();
    std::fs::rename(&path, root.path().join("retired-lease.lock")).unwrap();
    let _replacement = ExclusiveFileLock::acquire(&path).unwrap();
    std::fs::write(&path, subscription.leader_epoch.as_bytes()).unwrap();
    assert!(!admitted.still_authorized(&settings).unwrap(), "the subscription belongs to the original lease inode even when the peer and epoch are copied");
}

#[cfg(target_os = "linux")]
mod serving_peer {
    use ilium_server::{
        polling_auth::AuthenticatedSessionServer,
        polling_discovery::{publish, SessionEndpointHint},
    };

    // A readable hint or unrelated descriptor must never replace a live
    // birth-fenced kernel lock owned by the connected peer.
    #[test]
    fn polling_serving_peer_requires_and_retains_the_publisher_lease() {
        let root = tempfile::tempdir().unwrap();
        let hint = SessionEndpointHint {
            incarnation: uuid::Uuid::new_v4(),
            socket_path: root.path().join("synthetic.sock"),
        };
        let publication = publish(root.path(), hint.incarnation, &hint.socket_path).unwrap();
        let proof = AuthenticatedSessionServer::admit(root.path(), &hint, Some(std::process::id()))
            .unwrap()
            .expect("live publication authenticates its peer");
        assert!(proof.still_authorized().unwrap());
        drop(publication);
        assert!(
            !proof.still_authorized().unwrap(),
            "keeping the proof open must not keep the publisher's lease alive"
        );
        assert!(
            AuthenticatedSessionServer::admit(root.path(), &hint, Some(std::process::id()))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn polling_serving_peer_rejects_missing_peer_and_wrong_endpoint() {
        let root = tempfile::tempdir().unwrap();
        let mut hint = SessionEndpointHint {
            incarnation: uuid::Uuid::new_v4(),
            socket_path: root.path().join("synthetic.sock"),
        };
        let _publication = publish(root.path(), hint.incarnation, &hint.socket_path).unwrap();
        assert!(AuthenticatedSessionServer::admit(root.path(), &hint, None)
            .unwrap()
            .is_none());
        assert!(
            AuthenticatedSessionServer::admit(root.path(), &hint, Some(0))
                .unwrap()
                .is_none()
        );
        hint.socket_path = root.path().join("foreign.sock");
        assert!(
            AuthenticatedSessionServer::admit(root.path(), &hint, Some(std::process::id()))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn polling_serving_peer_cannot_follow_a_replaced_record() {
        use ilium_platform::{
            file_lock::ExclusiveFileLock,
            secure_fs::{self, PrivateFileMode},
        };
        use std::io::Write;
        let root = tempfile::tempdir().unwrap();
        let hint = SessionEndpointHint {
            incarnation: uuid::Uuid::new_v4(),
            socket_path: root.path().join("synthetic.sock"),
        };
        let _publication = publish(root.path(), hint.incarnation, &hint.socket_path).unwrap();
        let proof = AuthenticatedSessionServer::admit(root.path(), &hint, Some(std::process::id()))
            .unwrap()
            .expect("live publication authenticates its peer");
        let path = root
            .path()
            .join(format!("server-{}.json", hint.incarnation));
        std::fs::rename(&path, root.path().join("retired.json")).unwrap();
        let mut file =
            secure_fs::open_private_regular_file(&path, PrivateFileMode::CreateNew).unwrap();
        file.write_all(&serde_json::to_vec(&hint.socket_path).unwrap())
            .unwrap();
        let _replacement = ExclusiveFileLock::try_acquire_opened(file)
            .unwrap()
            .unwrap();
        assert!(
            !proof.still_authorized().unwrap(),
            "same process and copied endpoint cannot revive the old inode proof"
        );
    }
}
